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
use stepv::view::gpu::{Headless, Layout, View, require_gpu};
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
    let run = occt::run(
        &kernel,
        &data(name),
        Deflection::PREVIEW,
        Limits::DEFAULT,
        Some(&mesh),
        None,
    )
    .expect("start the kernel");
    assert_eq!(run.outcome, Outcome::Ok, "{name}: {:?}", run.summary);
    let bytes = std::fs::read(&mesh).unwrap();
    let _ = std::fs::remove_file(&mesh);
    occt::read_mesh(&bytes).expect("a valid STEPVMSH")
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
        let view = View {
            camera: cam,
            show_construction: false,
            clear: [0.0; 4],
            stripe: 6,
        };
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

#[test]
fn per_face_colours_reach_the_gpu() {
    let Some(g) = require_gpu(Headless::new()) else {
        return;
    };
    // The assembly's faces carry their own XCAF colours (macOS's checks look
    // for them too): the GPU frame must show more than one hue.
    let scene = load("assembly.step");
    let gs = g.upload(&scene);
    let img = g.render(
        &gs,
        &View {
            camera: Camera::for_scene(&scene),
            show_construction: false,
            clear: [0.0; 4],
            stripe: 6,
        },
        256,
        192,
    );
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
        let view = View {
            camera: cam,
            show_construction: false,
            clear: [0.0; 4],
            stripe: 6,
        };
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
