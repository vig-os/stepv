//! The GPU viewer's renderer against the REAL kernel's output for every file
//! in `tests/data/` (#28).
//!
//! The kernel must be built (`just kernel`, or `STEPV_OCCT`), as for
//! tests/cli.rs. The GPU tests need an adapter: without one they skip, saying
//! so, unless `STEPV_REQUIRE_GPU=1` (macOS CI) makes that a failure.

#![cfg(feature = "viewer")]

use std::path::{Path, PathBuf};

use stepv::occt::{self, Limits, Outcome};
use stepv::render::{self, Camera};
use stepv::topology::Topology;
use stepv::view::gpu::{Headless, Layout, Pick, View, clip_matrix, require_gpu};
use stepv::{Deflection, Scene};

const FILES: [&str; 5] = [
    "assembly.step",
    "box.brep",
    "box.igs",
    "sketch.step",
    "left-handed-plane.brep",
];

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

/// The kernel's preview-quality mesh of `name`, as `stepv view` gets it.
fn load(name: &str) -> Scene {
    load_with_topology(name).0
}

/// The mesh and the exact topology, checked against each other.
fn load_with_topology(name: &str) -> (Scene, Topology) {
    let kernel = occt::kernel_path();
    assert!(
        kernel.is_file(),
        "kernel not found at {}: run `just kernel` or set STEPV_OCCT",
        kernel.display()
    );
    // Unique per call: tests run in parallel and load the same files.
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mesh =
        std::env::temp_dir().join(format!("stepv-view-{}-{n}-{name}.msh", std::process::id()));
    let topo = mesh.with_extension("json");
    let run = occt::run(
        &kernel,
        &data(name),
        Deflection::PREVIEW,
        Limits::DEFAULT,
        Some(&mesh),
        Some(&topo),
        // As stepv view asks: with the B-rep edges (STEPVMSH v4).
        true,
    )
    .expect("start the kernel");
    assert_eq!(run.outcome, Outcome::Ok, "{name}: {:?}", run.summary);
    let bytes = std::fs::read(&mesh).unwrap();
    let json = std::fs::read(&topo).unwrap();
    let _ = std::fs::remove_file(&mesh);
    let _ = std::fs::remove_file(&topo);
    let scene = occt::read_mesh(&bytes).expect("a valid STEPVMSH");
    let topology = Topology::parse(&json).expect("valid topology");
    topology
        .check_against(&scene)
        .expect("topology matches the mesh");
    (scene, topology)
}

#[test]
fn kernel_meshes_need_no_vertex_splits() {
    // The GPU layout keeps one face id per vertex. STEPVMSH writes each
    // face's vertices on their own, so no vertex is shared between faces and
    // the layout costs nothing extra; if the kernel ever starts sharing them,
    // this says so before the memory does.
    for name in FILES {
        let scene = load(name);
        let l = Layout::new(&scene);
        assert_eq!(l.split_vertices, 0, "{name}");
        assert_eq!(l.parts.len(), scene.parts.len(), "{name}");
        assert_eq!(l.faces.len() * 3, l.positions.len(), "{name}");
    }
}

#[test]
fn the_gpu_frames_every_file_as_the_thumbnail_renderer_does() {
    let Some(g) = require_gpu(Headless::new()) else {
        return;
    };
    for name in FILES {
        let scene = load(name);
        let cam = Camera::for_scene(&scene);
        let gs = g.upload(&scene);
        let (w, h) = (192, 144);
        let view = View::new(cam);
        let gpu = g.render(&gs, &view, w, h);
        let cpu = render::render(
            &scene,
            &render::Options {
                width: w,
                height: h,
                show_construction: false,
                supersample: 2,
                camera: cam,
                fit: render::Fit::Sphere,
            },
        )
        .unwrap();
        let drawn = |img: &render::Image| -> Vec<bool> {
            img.rgba.chunks_exact(4).map(|p| p[3] > 127).collect()
        };
        let (a, b) = (drawn(&gpu), drawn(&cpu));
        let bbox = |m: &[bool]| {
            let xs = m
                .iter()
                .enumerate()
                .filter(|(_, d)| **d)
                .map(|(i, _)| i as u32 % w);
            let ys = m
                .iter()
                .enumerate()
                .filter(|(_, d)| **d)
                .map(|(i, _)| i as u32 / w);
            (xs.clone().min(), xs.max(), ys.clone().min(), ys.max())
        };
        if scene.triangle_count() == 0 {
            // Curves only (the sketch): lines are one pixel wide in both, so
            // compare where they are, not their pixels.
            let (ga, gb) = (bbox(&a), bbox(&b));
            for (x, y) in [(ga.0, gb.0), (ga.1, gb.1), (ga.2, gb.2), (ga.3, gb.3)] {
                let (x, y) = (x.unwrap(), y.unwrap());
                assert!(
                    x.abs_diff(y) <= 3,
                    "{name}: GPU extent {ga:?}, software {gb:?}"
                );
            }
            continue;
        }
        let both = a.iter().zip(&b).filter(|(x, y)| **x && **y).count();
        let any = a.iter().zip(&b).filter(|(x, y)| **x || **y).count();
        let iou = both as f32 / any.max(1) as f32;
        assert!(
            iou > 0.95,
            "{name}: GPU and software silhouettes overlap only {iou}"
        );
    }
}

/// #29's acceptance: clicking the bracket plate's hole shows a cylinder of
/// r = 4, and its top a plane with normal +z and area 1200 - 16π, driven
/// headless at pixels computed from a known camera.
#[test]
fn picking_the_plate_names_its_exact_faces() {
    let Some(g) = require_gpu(Headless::new()) else {
        return;
    };
    let (scene, topo) = load_with_topology("assembly.step");
    let gs = g.upload(&scene);
    let cam = Camera::default(); // azimuth -35, elevation 30
    let (w, h) = (640, 480);
    let pixel = |p: [f32; 3]| {
        let m = clip_matrix(&cam, &gs.fits[0].unwrap(), w, h);
        let c = [0, 1].map(|r| m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r]);
        (
            ((c[0] + 1.0) / 2.0 * w as f32) as u32,
            ((1.0 - c[1]) / 2.0 * h as f32) as u32,
        )
    };
    let rows_at = |p: [f32; 3]| {
        let (x, y) = pixel(p);
        let Pick { part, face } = g
            .pick(&gs, &View::new(cam), w, h, x, y)
            .unwrap_or_else(|| panic!("nothing at {p:?} ({x}, {y})"));
        let rows = stepv::view::inspect::face(&topo, part as usize, face as usize).unwrap();
        move |k: &str| rows.iter().find(|r| r.key == k).map(|r| r.value.clone())
    };
    // The hole (centre (20, 15), r = 4, through z 0..5): its wall on the far
    // side from the eye, which sits at +x, -y, +z, is visible from above.
    let a = (-35f32).to_radians();
    let away = [a.sin(), a.cos()]; // the eye's horizontal direction, negated
    // 0.9 r and z = 4: well inside the wall's silhouette, whatever the
    // deflection does to the facets.
    let wall = [20.0 + 4.0 * away[0] * 0.9, 15.0 + 4.0 * away[1] * 0.9, 4.0];
    let hole = rows_at(wall);
    assert_eq!(hole("Surface").as_deref(), Some("Cylinder"));
    assert_eq!(hole("Radius").as_deref(), Some("4 mm"));
    assert_eq!(hole("Part").as_deref(), Some("plate (#0)"));
    // The top, away from the hole and the pins.
    let top = rows_at([10.0, 5.0, 5.0]);
    assert_eq!(top("Surface").as_deref(), Some("Plane"));
    assert_eq!(top("Normal").as_deref(), Some("+z"));
    let area: f64 = top("Face area")
        .unwrap()
        .trim_end_matches(" mm²")
        .parse()
        .unwrap();
    assert!(
        (area - (1200.0 - 16.0 * std::f64::consts::PI)).abs() < 1e-3,
        "area {area}"
    );
}

/// STEPVMSH v4 (#31): every part's edge polylines are well formed and name
/// edges the topology has, so a picked edge always has a curve to show.
#[test]
fn kernel_edges_are_numbered_as_the_topology() {
    for name in FILES {
        let (scene, topo) = load_with_topology(name);
        for (i, part) in scene.parts.iter().enumerate() {
            let e = &part.edges;
            assert!(e.is_well_formed(), "{name} part {i}");
            let proto = &topo.prototypes[topo.parts[i].prototype];
            // Each polyline is the edge its id names: in range, and as long
            // as the topology says (a shuffled or shifted numbering fails).
            let scale = {
                let m = &topo.parts[i].transform;
                let det = m[0] * (m[5] * m[10] - m[6] * m[9]) - m[1] * (m[4] * m[10] - m[6] * m[8])
                    + m[2] * (m[4] * m[9] - m[5] * m[8]);
                det.abs().cbrt()
            };
            let mut at = 0;
            for (&id, &n) in e.ids.iter().zip(&e.lens) {
                assert!(
                    (id as usize) < proto.edges.len(),
                    "{name} part {i}: edge {id}"
                );
                let pts = &e.points[at * 3..(at + n as usize) * 3];
                let polyline: f64 = pts
                    .chunks_exact(3)
                    .zip(pts.chunks_exact(3).skip(1))
                    .map(|(a, b)| {
                        (0..3)
                            .map(|k| f64::from(b[k] - a[k]).powi(2))
                            .sum::<f64>()
                            .sqrt()
                    })
                    .sum();
                let exact = proto.edges[id as usize].length * scale;
                // Chords are shorter than their arc, by at most a few
                // percent at the preview's angular deflection.
                assert!(
                    polyline <= exact * 1.0001 + 1e-6 && polyline >= exact * 0.97,
                    "{name} part {i} edge {id}: polyline {polyline}, topology {exact}"
                );
                at += n as usize;
            }
            if !part.faces.is_empty() {
                assert!(e.count() > 0, "{name} part {i} has faces but no edges");
            }
        }
    }
}

/// #31's acceptance: clicking the plate's hole edge shows a circle of
/// r = 4 and length 8π, at a pixel projected from a known camera.
#[test]
fn picking_the_hole_rim_names_its_circle() {
    let Some(g) = require_gpu(Headless::new()) else {
        return;
    };
    let (scene, topo) = load_with_topology("assembly.step");
    let gs = g.upload(&scene);
    let cam = Camera::default();
    let (w, h) = (640, 480);
    let m = clip_matrix(&cam, &gs.fits[0].unwrap(), w, h);
    let pixel = |p: [f32; 3]| {
        let c = [0, 1].map(|r| m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r]);
        (
            ((c[0] + 1.0) / 2.0 * w as f32) as u32,
            ((1.0 - c[1]) / 2.0 * h as f32) as u32,
        )
    };
    // The top rim (z = 5) on the side nearest the eye: the eye sits at
    // +x, -y, so the rim point toward it is centre + 4 (sin 35°, -cos 35°).
    let a = 35f32.to_radians();
    let (x, y) = pixel([20.0 + 4.0 * a.sin(), 15.0 - 4.0 * a.cos(), 5.0]);
    // Two pixels off the rim, onto the plate's top: the edge still wins.
    let hit = g
        .pick(&gs, &View::new(cam), w, h, x, y + 2)
        .expect("something under the click");
    let edge = hit
        .edge_id()
        .unwrap_or_else(|| panic!("a face, not the rim: {hit:?}"));
    let rows = stepv::view::inspect::edge(&topo, hit.part as usize, edge as usize).unwrap();
    let get = |k: &str| rows.iter().find(|r| r.key == k).map(|r| r.value.clone());
    assert_eq!(get("Curve").as_deref(), Some("Circle"));
    assert_eq!(get("Radius").as_deref(), Some("4 mm"));
    let len: f64 = get("Length")
        .unwrap()
        .trim_end_matches(" mm")
        .parse()
        .unwrap();
    assert!(
        (len - 8.0 * std::f64::consts::PI).abs() < 1e-3,
        "length {len}"
    );
}

/// #34's acceptance: a section through the bracket shows a filled plate
/// profile with the hole open. Cut at y = 15, through the hole's and the
/// pins' axes, and seen from the cut side.
#[test]
fn a_section_through_the_bracket_caps_the_plate_and_keeps_the_hole_open() {
    let Some(g) = require_gpu(Headless::new()) else {
        return;
    };
    let scene = load("assembly.step");
    let gs = g.upload(&scene);
    let cam = Camera {
        azimuth_deg: 180.0,
        elevation_deg: 0.0,
        ..Camera::default()
    };
    let view = View {
        section: Some([0.0, 1.0, 0.0, 15.0]),
        ..View::new(cam)
    };
    let (w, h) = (640, 480);
    let img = g.render(&gs, &view, w, h);
    let m = clip_matrix(&cam, &gs.fits[0].unwrap(), w, h);
    let pixel = |p: [f32; 3]| {
        let c = [0, 1].map(|r| m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r]);
        img.pixel(
            ((c[0] + 1.0) / 2.0 * w as f32) as u32,
            ((1.0 - c[1]) / 2.0 * h as f32) as u32,
        )
    };
    // The cap: grey, low saturation (the plate is red, the pins orange).
    let grey = |p: [u8; 4]| {
        let (hi, lo) = (p[0].max(p[1]).max(p[2]), p[0].min(p[1]).min(p[2]));
        p[3] == 255 && hi - lo < 20
    };
    // The plate where no pin is (the fixture's pins pass through it, and
    // overlapping solids cancel each other's parity: interference is not
    // capped), and a pin above the plate.
    for at in [
        [2.0, 15.0, 2.5],
        [12.0, 15.0, 1.0],
        [30.0, 15.0, 4.0],
        [6.0, 15.0, 10.0],
    ] {
        assert!(grey(pixel(at)), "{at:?} is not capped: {:?}", pixel(at));
    }
    // The hole (x 16..24) is open: what shows there is the hole's wall
    // behind the cut, the part's own colour, not the cap.
    let hole = pixel([20.0, 15.0, 2.5]);
    assert!(!grey(hole), "the hole is capped over: {hole:?}");
    // Above the plate, between the pins: nothing at all.
    assert_eq!(pixel([20.0, 15.0, 10.0])[3], 0);
}

#[test]
fn per_face_colours_reach_the_gpu() {
    let Some(g) = require_gpu(Headless::new()) else {
        return;
    };
    // The assembly's faces carry their own XCAF colours (macOS's checks look
    // for them too): the GPU frame must show more than one hue.
    let scene = load("assembly.step");
    let gs = g.upload(&scene);
    let img = g.render(&gs, &View::new(Camera::for_scene(&scene)), 256, 192);
    let mut hues = std::collections::BTreeSet::new();
    for p in img.rgba.chunks_exact(4).filter(|p| p[3] == 255) {
        // Coarse, so shading does not count as a new colour.
        let (r, g, b) = (i32::from(p[0]), i32::from(p[1]), i32::from(p[2]));
        let max = r.max(g).max(b).max(1);
        hues.insert(((r * 4 / max), (g * 4 / max), (b * 4 / max)));
    }
    assert!(hues.len() >= 2, "one colour only: {hues:?}");
}

/// #28's acceptance: the stress assembly (1.77M triangles) draws at display
/// rate. Opt-in, as the file is not committed:
///   STEPV_STRESS=tests/fixtures/stress-assembly/<file> cargo test --release \
///     --test view -- --ignored --nocapture
/// GPU-synchronised: each frame waits for the GPU, as #27's methodology
/// review asked; a loop rate with vsync off would only time CPU submission.
#[test]
#[ignore = "needs STEPV_STRESS and a GPU"]
fn stress_assembly_draws_at_display_rate() {
    let path = PathBuf::from(std::env::var_os("STEPV_STRESS").expect("STEPV_STRESS"));
    let g = require_gpu(Headless::new()).expect("a GPU");
    let kernel = occt::kernel_path();
    let mesh = std::env::temp_dir().join(format!("stepv-stress-{}.msh", std::process::id()));
    let limits = Limits {
        timeout: std::time::Duration::from_secs(300),
        memory: None,
    };
    let run = occt::run(
        &kernel,
        &path,
        Deflection::PREVIEW,
        limits,
        Some(&mesh),
        None,
        true,
    )
    .unwrap();
    assert_eq!(run.outcome, Outcome::Ok);
    let scene = occt::read_mesh(&std::fs::read(&mesh).unwrap()).unwrap();
    let _ = std::fs::remove_file(&mesh);
    let t0 = std::time::Instant::now();
    let gs = g.upload(&scene);
    let upload = t0.elapsed();
    let mut cam = Camera::for_scene(&scene);
    let target = stepv::view::gpu::Target::new(&g.device, 1280, 800);
    let renderer = stepv::view::gpu::Renderer::new(&g.device);
    let mut ms = Vec::new();
    for _ in 0..60 {
        cam.azimuth_deg += 1.0;
        let view = View::new(cam);
        let t = std::time::Instant::now();
        let frame = renderer.render(&g.device, &g.queue, &gs, &target, &view);
        g.queue.submit([frame]);
        g.device
            .poll(stepv::view::gpu::wgpu::PollType::wait_indefinitely())
            .unwrap();
        ms.push(t.elapsed().as_secs_f64() * 1e3);
    }
    ms.sort_by(f64::total_cmp);
    let (p50, p95) = (ms[30], ms[57]);
    println!(
        "stress: {} parts, {} triangles on {} ({}): upload {:.0} ms, GPU-synced frame \
         (1280x800, 4x MSAA) p50 {p50:.2} ms p95 {p95:.2} ms",
        scene.parts.len(),
        scene.triangle_count(),
        g.adapter,
        g.backend.name(),
        upload.as_secs_f64() * 1e3
    );
    assert!(p95 < 1000.0 / 60.0, "p95 {p95:.2} ms is over a 60 Hz frame");
}
