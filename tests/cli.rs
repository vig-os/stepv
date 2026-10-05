//! End-to-end tests of the `stepv` binary against the committed corpus in
//! `tests/data/` (regenerate with `just test-data`).
//!
//! These run the REAL kernel. If it is missing they FAIL, loudly, unless
//! `STEPV_SKIP_KERNEL_TESTS=1` is set on purpose: a suite that quietly skips
//! when its subject is absent is exactly how `occt-wasm`'s tests stayed green
//! over a kernel that could not start (`plan.md` §5 "S1 result").

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

/// A scratch directory unique to one test, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("stepv-cli-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Returns false (test should return early) only when skipping was asked for.
fn kernel_available() -> bool {
    let k = stepv::occt::kernel_path();
    if k.is_file() {
        return true;
    }
    if std::env::var_os("STEPV_SKIP_KERNEL_TESTS").is_some_and(|v| v == "1") {
        eprintln!("SKIPPING: kernel not built and STEPV_SKIP_KERNEL_TESTS=1");
        return false;
    }
    panic!(
        "kernel not found at {} — run `just kernel`, set STEPV_OCCT, or set \
         STEPV_SKIP_KERNEL_TESTS=1 to skip deliberately",
        k.display()
    );
}

/// Runs stepv with a private cache dir, returning (exit code, stdout JSON).
fn stepv(args: &[&str], home: &Path) -> (i32, Value, Output) {
    let out = Command::new(env!("CARGO_BIN_EXE_stepv"))
        .args(args)
        .env("HOME", home)
        .env("XDG_CACHE_HOME", home.join("xdg"))
        .output()
        .expect("spawn stepv");
    let code = out.status.code().unwrap_or(-1);
    let json = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .and_then(|l| serde_json::from_str(l).ok())
        .unwrap_or(Value::Null);
    (code, json, out)
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn info_reads_the_header_without_the_kernel() {
    let t = Scratch::new("info");
    let (code, j, _) = stepv(&[s(&data("assembly.step")), "--info"], &t.0);
    assert_eq!(code, 0);
    assert_eq!(j["status"], "info");
    assert_eq!(j["info"]["format"], "step");
    assert_eq!(j["info"]["protocol"], "AP214");
    assert_eq!(j["info"]["product_count"], 3);
    assert!(j["info"]["header_error"].is_null());
}

#[test]
fn info_never_fails_on_garbage() {
    let t = Scratch::new("info-garbage");
    for (name, body) in [
        ("empty.step", &b""[..]),
        ("noise.step", &[0xffu8, 0x00, 0x13, 0x37][..]),
        ("missing.step", &b"-"[..]),
    ] {
        let p = t.path(name);
        if name != "missing.step" {
            std::fs::write(&p, body).unwrap();
        }
        let (code, j, _) = stepv(&[s(&p), "--info"], &t.0);
        assert_eq!(code, 0, "{name}");
        assert!(j["info"]["header_error"].is_string(), "{name}: {j}");
    }
}

#[test]
fn png_of_the_assembly_has_its_colours() {
    if !kernel_available() {
        return;
    }
    let t = Scratch::new("png");
    let out = t.path("a.png");
    let (code, j, o) = stepv(
        &[s(&data("assembly.step")), "--png", s(&out), "--size", "128"],
        &t.0,
    );
    assert_eq!(code, 0, "{j} {}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(j["status"], "ok");
    assert_eq!(j["kernel"]["parts"], 3);
    assert_eq!(j["cached"], false);
    let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&out).unwrap()));
    let mut r = dec.read_info().unwrap();
    let mut buf = vec![0; r.output_buffer_size().unwrap()];
    let info = r.next_frame(&mut buf).unwrap();
    assert_eq!((info.width, info.height), (128, 128));
    let px: Vec<[i32; 4]> = buf
        .chunks_exact(4)
        .filter(|p| p[3] == 255)
        .map(|p| [p[0], p[1], p[2], p[3]].map(i32::from))
        .collect();
    let blue = px
        .iter()
        .filter(|p| p[2] > p[0] + 40 && p[2] > p[1])
        .count();
    let red = px
        .iter()
        .filter(|p| p[0] > p[1] + 80 && p[0] > p[2] + 80)
        .count();
    let orange = px
        .iter()
        .filter(|p| p[0] > 150 && p[1] > 60 && p[2] < 60 && p[0] > p[1] + 30)
        .count();
    assert!(blue > 20, "plate side faces are blue ({blue})");
    assert!(red > 50, "the plate's top face carries its own red ({red})");
    assert!(orange > 10, "pins are orange ({orange})");
}

#[test]
fn second_run_is_served_from_the_cache() {
    if !kernel_available() {
        return;
    }
    let t = Scratch::new("cache");
    let (a, b) = (t.path("1.png"), t.path("2.png"));
    let (c1, j1, _) = stepv(
        &[s(&data("box.brep")), "--png", s(&a), "--size", "64"],
        &t.0,
    );
    let (c2, j2, _) = stepv(
        &[s(&data("box.brep")), "--png", s(&b), "--size", "64"],
        &t.0,
    );
    assert_eq!((c1, c2), (0, 0));
    assert_eq!(
        (j1["cached"].as_bool(), j2["cached"].as_bool()),
        (Some(false), Some(true))
    );
    assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
    // A different size is a different artefact, not a hit.
    let (_, j3, _) = stepv(
        &[s(&data("box.brep")), "--png", s(&b), "--size", "96"],
        &t.0,
    );
    assert_eq!(j3["cached"], false);
}

#[test]
fn glb_is_valid_and_keeps_part_names() {
    if !kernel_available() {
        return;
    }
    let t = Scratch::new("glb");
    let out = t.path("a.glb");
    let (code, j, _) = stepv(
        &[s(&data("assembly.step")), "--glb", s(&out), "--no-cache"],
        &t.0,
    );
    assert_eq!(code, 0, "{j}");
    let (doc, _, _) = gltf::import(&out).expect("valid glTF");
    let mut names: Vec<_> = doc
        .nodes()
        .filter_map(|n| n.name().map(str::to_owned))
        .collect();
    names.sort();
    assert_eq!(names, ["pin", "pin", "plate", "stepv"]);
}

#[test]
fn the_kernel_runs_sandboxed() {
    if !kernel_available() {
        return;
    }
    let t = Scratch::new("sandbox");
    let png = t.path("a.png");
    let (code, j, out) = stepv(&[s(&data("assembly.step")), "--png", s(&png)], &t.0);
    assert_eq!(code, 0, "{j}");
    // Seatbelt cannot nest in nix's macOS build sandbox: see tests/sandbox.rs.
    if j["kernel"]["sandbox"] == "macos-outer" && std::env::var_os("NIX_BUILD_TOP").is_some() {
        eprintln!("SKIPPING: inside nix's build sandbox, which Seatbelt cannot nest in");
        return;
    }
    let expected = if cfg!(target_os = "macos") {
        "macos-profile"
    } else {
        "landlock+seccomp"
    };
    assert_eq!(j["kernel"]["sandbox"], expected, "{j}");
    assert!(!String::from_utf8_lossy(&out.stderr).contains("warning"));
}

/// `--topology` alone, parsed and validated (#21).
fn topology_of(name: &str, t: &Scratch) -> stepv::topology::Topology {
    let out = t.path("topology.json");
    let (code, j, _) = stepv(&[s(&data(name)), "--topology", s(&out)], &t.0);
    assert_eq!(code, 0, "{j}");
    assert_eq!(j["topology"], s(&out), "{j}");
    stepv::topology::Topology::parse(&std::fs::read(&out).unwrap()).unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6 * b.abs().max(1.0)
}

#[test]
fn topology_measures_the_assembly_exactly() {
    use stepv::topology::{Curve, Node, Surface};
    if !kernel_available() {
        return;
    }
    let t = Scratch::new("topology");
    let topo = topology_of("assembly.step", &t);
    let Node::Assembly { name, children } = &topo.tree[0] else {
        panic!("the root is the assembly: {:?}", topo.tree)
    };
    assert_eq!(name, "bracket-assy");
    let leaves: Vec<_> = children
        .iter()
        .map(|c| match c {
            Node::Part { name, part } => (name.as_str(), *part),
            Node::Assembly { .. } => panic!("no sub-assemblies here"),
        })
        .collect();
    assert_eq!(leaves, [("plate", 0), ("pin", 1), ("pin", 2)]);
    // Two pins, one prototype; placed at x = 6 and x = 34, y = 15.
    let protos: Vec<_> = topo.parts.iter().map(|p| p.prototype).collect();
    assert_eq!(protos, [0, 1, 1]);
    let tx = |i: usize| [topo.parts[i].transform[3], topo.parts[i].transform[7]];
    assert_eq!([tx(1), tx(2)], [[6.0, 15.0], [34.0, 15.0]]);

    let pi = std::f64::consts::PI;
    let plate = &topo.prototypes[0];
    // 40 x 30 x 5, less a hole of radius 4.
    assert!(close(plate.volume.unwrap(), 6000.0 - pi * 16.0 * 5.0));
    assert!(close(
        plate.area,
        2.0 * (1200.0 + 200.0 + 150.0) - 2.0 * pi * 16.0 + 2.0 * pi * 4.0 * 5.0
    ));
    let holes: Vec<_> = plate
        .faces
        .iter()
        .filter_map(|f| match f.surface {
            Surface::Cylinder { radius, .. } => Some(radius),
            _ => None,
        })
        .collect();
    assert_eq!(holes, [4.0]);
    assert!(
        plate
            .edges
            .iter()
            .any(|e| matches!(e.curve, Curve::Circle { radius, .. } if radius == 4.0))
    );
    // A pin: radius 2, length 15.
    assert!(close(topo.prototypes[1].volume.unwrap(), pi * 4.0 * 15.0));
}

#[test]
fn topology_of_a_box_and_a_sketch() {
    use stepv::topology::{Curve, Surface};
    if !kernel_available() {
        return;
    }
    let t = Scratch::new("topology-box");
    let b = &topology_of("box.brep", &t).prototypes[0];
    assert!(close(b.volume.unwrap(), 20.0 * 10.0 * 5.0));
    assert!(close(b.area, 2.0 * (200.0 + 100.0 + 50.0)));
    assert_eq!(b.faces.len(), 6);
    assert!(
        b.faces
            .iter()
            .all(|f| matches!(f.surface, Surface::Plane { .. }) && f.edges.len() == 4)
    );
    assert_eq!((b.edges.len(), b.vertices.len()), (12, 8));
    // Every normal points OUT of the box: a viewer's angle between faces,
    // and which side a section caps, depend on it.
    let centre = [10.0, 5.0, 2.5];
    for f in &b.faces {
        let Surface::Plane { origin, normal } = f.surface else {
            unreachable!()
        };
        let out: f64 = (0..3).map(|i| normal[i] * (origin[i] - centre[i])).sum();
        assert!(out > 0.0, "inward normal {normal:?} at {origin:?}");
    }

    let sk = &topology_of("sketch.step", &t).prototypes[0];
    assert_eq!((sk.faces.len(), sk.volume), (0, None));
    assert!(
        matches!(sk.edges[..], [ref e] if matches!(e.curve, Curve::Circle { radius, .. } if radius == 25.0))
    );
}

#[test]
fn topology_normals_follow_the_surface_not_the_axis() {
    // A plane with left-handed axes: Axis() is +z, but the surface, and so
    // the face, faces -z (kernel/fixture-gen.cpp).
    use stepv::topology::Surface;
    if !kernel_available() {
        return;
    }
    let t = Scratch::new("topology-left-handed");
    let topo = topology_of("left-handed-plane.brep", &t);
    let Surface::Plane { normal, .. } = topo.prototypes[0].faces[0].surface else {
        panic!("a plane")
    };
    assert_eq!(normal, [0.0, 0.0, -1.0]);
}

#[test]
fn an_output_on_the_input_is_refused_untouched() {
    // Creating an output truncates it: on the input, that would destroy the
    // very file being previewed.
    let k = stepv::occt::kernel_path();
    if !kernel_available() {
        return;
    }
    let t = Scratch::new("output-is-input");
    let input = t.path("a.step");
    std::fs::copy(data("assembly.step"), &input).unwrap();
    let before = std::fs::read(&input).unwrap();
    for args in [
        vec!["--mesh", s(&input)],
        vec!["--topology", s(&input)],
        vec!["--mesh", s(&t.path("x")), "--topology", s(&t.path("x"))],
    ] {
        let out = Command::new(&k).arg(&input).args(&args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
    assert_eq!(
        std::fs::read(&input).unwrap(),
        before,
        "the input was touched"
    );
}

#[test]
fn topology_comes_beside_any_output() {
    if !kernel_available() {
        return;
    }
    let t = Scratch::new("topology-png");
    let (png, topo) = (t.path("a.png"), t.path("a.json"));
    let (code, j, _) = stepv(
        &[
            s(&data("assembly.step")),
            "--png",
            s(&png),
            "--topology",
            s(&topo),
        ],
        &t.0,
    );
    assert_eq!(code, 0, "{j}");
    assert!(png.is_file() && topo.is_file(), "{j}");
}

#[test]
fn multifile_assembly_loads_its_part_files() {
    if !kernel_available() {
        return;
    }
    let t = Scratch::new("multifile");
    let png = t.path("m.png");
    let (code, j, _) = stepv(
        &[s(&data("multifile/bracket.step")), "--png", s(&png)],
        &t.0,
    );
    assert_eq!(code, 0, "{j}");
    assert_eq!(j["kernel"]["parts"], 3, "{j}");
    assert_eq!(j["kernel"]["external_files"], 2, "{j}");
    assert_eq!(j["kernel"]["external_missing"], 0, "{j}");
}

#[test]
fn multifile_assembly_without_its_parts_says_why() {
    // What a sandbox granting only the one file (Quick Look) sees (#19).
    if !kernel_available() {
        return;
    }
    let t = Scratch::new("multifile-alone");
    let top = t.path("bracket.step");
    std::fs::copy(data("multifile/bracket.step"), &top).unwrap();
    let (code, j, _) = stepv(&[s(&top), "--png", s(&t.path("m.png"))], &t.0);
    assert_eq!(code, 3, "{j}");
    assert_eq!(j["kernel"]["external_missing"], 2, "{j}");
    let error = j["error"].as_str().unwrap_or_default();
    assert!(
        error.contains("multi-file assembly: 2 of 2 part files"),
        "the failure must name the cause: {j}"
    );
}

#[test]
fn mesh_output_decodes_and_carries_face_colours() {
    if !kernel_available() {
        return;
    }
    let t = Scratch::new("mesh");
    let out = t.path("a.msh");
    let (code, j, _) = stepv(
        &[s(&data("assembly.step")), "--mesh", s(&out), "--no-cache"],
        &t.0,
    );
    assert_eq!(code, 0, "{j}");
    let scene = stepv::occt::read_mesh(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(scene.parts.len(), 3);
    let red_faces = scene
        .parts
        .iter()
        .flat_map(|p| &p.faces)
        .filter(|f| f.color.is_some_and(|c| c.r > 0.8 && c.g < 0.2))
        .count();
    assert_eq!(red_faces, 1, "the plate's top face keeps its own colour");
}

#[test]
fn every_format_renders() {
    if !kernel_available() {
        return;
    }
    let t = Scratch::new("formats");
    for f in ["box.igs", "box.brep", "sketch.step"] {
        let out = t.path(&format!("{f}.png"));
        let (code, j, _) = stepv(
            &[s(&data(f)), "--png", s(&out), "--no-cache", "--size", "64"],
            &t.0,
        );
        assert_eq!(code, 0, "{f}: {j}");
        assert!(out.is_file(), "{f}");
    }
    // The sketch is curves only: no faces, so no face verdict.
    let (_, j, _) = stepv(
        &[
            s(&data("sketch.step")),
            "--png",
            s(&t.path("k.png")),
            "--no-cache",
        ],
        &t.0,
    );
    assert!(j["worst_face"].is_null());
    assert!(j["kernel"]["segments"].as_u64().unwrap() > 10);
}

#[test]
fn broken_input_exits_3_with_valid_metadata() {
    if !kernel_available() {
        return;
    }
    let t = Scratch::new("broken");
    let full = std::fs::read(data("assembly.step")).unwrap();
    let cases: [(&str, Vec<u8>); 4] = [
        ("empty.step", Vec::new()),
        ("truncated.step", full[..full.len() / 3].to_vec()),
        (
            "noise.step",
            (0..4096u32)
                .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
                .collect(),
        ),
        ("not-iges.igs", b"hello\n".repeat(20)),
    ];
    for (name, body) in cases {
        let p = t.path(name);
        std::fs::write(&p, body).unwrap();
        let (code, j, _) = stepv(&[s(&p), "--png", s(&t.path("o.png")), "--no-cache"], &t.0);
        assert_eq!(code, 3, "{name}: {j}");
        assert!(j["error"].is_string(), "{name}");
        assert!(j["info"].is_object(), "{name}: metadata survives");
    }
    // The truncated file still reports its real header.
    let (_, j, _) = stepv(
        &[
            s(&t.path("truncated.step")),
            "--png",
            s(&t.path("o.png")),
            "--no-cache",
        ],
        &t.0,
    );
    assert_eq!(j["info"]["protocol"], "AP214");
}

#[test]
fn failures_are_cached_too() {
    if !kernel_available() {
        return;
    }
    let t = Scratch::new("failcache");
    let p = t.path("bad.step");
    std::fs::write(&p, b"ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\nENDSEC;\n").unwrap();
    let (c1, j1, _) = stepv(&[s(&p), "--png", s(&t.path("o.png"))], &t.0);
    let (c2, j2, _) = stepv(&[s(&p), "--png", s(&t.path("o.png"))], &t.0);
    assert_eq!((c1, c2), (3, 3));
    assert_eq!(j1["cached"].as_bool(), None);
    assert_eq!(j2["cached"], true);
    assert_eq!(j1["error"], j2["error"]);
}

#[test]
fn timeout_exits_4() {
    if !kernel_available() {
        return;
    }
    let t = Scratch::new("timeout");
    let (code, j, _) = stepv(
        &[
            s(&data("assembly.step")),
            "--png",
            s(&t.path("o.png")),
            "--timeout",
            "0.001",
            "--no-cache",
        ],
        &t.0,
    );
    assert_eq!(code, 4, "{j}");
    assert_eq!(j["status"], "timeout");
    assert!(j["info"].is_object());
}

#[test]
fn memory_cap_exits_3() {
    if !kernel_available() {
        return;
    }
    let t = Scratch::new("memcap");
    // The kernel's test hook allocates 512 MiB and holds it; a 64 MiB cap must
    // catch that. A real small file's footprint is too timing-dependent: it
    // stays under 1 MiB for its first 60 ms (shared dylib pages don't count).
    let out = Command::new(env!("CARGO_BIN_EXE_stepv"))
        .args([s(&data("assembly.step")), "--png", s(&t.path("o.png"))])
        .args(["--memory-mb", "64", "--no-cache"])
        .env("HOME", &t.0)
        .env("STEPV_OCCT_TEST_BALLOON_MB", "512")
        .output()
        .unwrap();
    let code = out.status.code().unwrap_or(-1);
    let j: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    assert_eq!(code, 3, "{j}");
    assert_eq!(j["status"], "memory-cap");
    // Killed well before the hook's 5 s hold ends.
    assert!(j["wall_ms"].as_f64().unwrap() < 4000.0, "{j}");
}

#[test]
fn usage_errors_exit_2_without_json() {
    let t = Scratch::new("usage");
    for args in [
        &["x.step"][..],
        &["x.step", "--png"],
        &["x.step", "--bogus", "1"],
    ] {
        let (code, j, _) = stepv(args, &t.0);
        assert_eq!(code, 2, "{args:?}");
        assert!(j.is_null(), "{args:?}: usage errors print no JSON");
    }
}

/// Runs stepv with the kernel's topology test hook on (#38): every
/// prototype's `prototype_json` throws, the mesh is untouched.
fn stepv_topology_fails(args: &[&str], home: &Path) -> (i32, Value, Output) {
    let out = Command::new(env!("CARGO_BIN_EXE_stepv"))
        .args(args)
        .env("HOME", home)
        .env("XDG_CACHE_HOME", home.join("xdg"))
        .env("STEPV_OCCT_TEST_TOPOLOGY_FAIL", "1")
        .output()
        .expect("spawn stepv");
    let code = out.status.code().unwrap_or(-1);
    let json = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    (code, json, out)
}

#[test]
fn a_failed_topology_keeps_the_png() {
    if !kernel_available() {
        return;
    }
    let t = Scratch::new("topology-fails-png");
    let (png, topo) = (t.path("a.png"), t.path("a.json"));
    let (code, j, o) = stepv_topology_fails(
        &[
            s(&data("assembly.step")),
            "--png",
            s(&png),
            "--topology",
            s(&topo),
            "--size",
            "64",
        ],
        &t.0,
    );
    assert_eq!(code, 0, "{j} {}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(j["status"], "ok");
    assert_eq!(j["output"], s(&png));
    assert!(png.is_file(), "the PNG is written");
    assert_eq!(j["kernel"]["stage"], "done", "{j}");
    let err = j["topology_error"].as_str().unwrap_or_default();
    assert!(err.contains("test hook"), "{j}");
    assert!(j["topology"].is_null(), "{j}");
    assert!(!topo.exists(), "no topology file, not even an empty one");
}

#[test]
fn a_failed_topology_alone_fails() {
    if !kernel_available() {
        return;
    }
    let t = Scratch::new("topology-fails-alone");
    let topo = t.path("a.json");
    let (code, j, _) =
        stepv_topology_fails(&[s(&data("assembly.step")), "--topology", s(&topo)], &t.0);
    assert_eq!(code, 3, "{j}");
    assert_eq!(j["status"], "error");
    assert!(
        j["error"]
            .as_str()
            .unwrap_or_default()
            .contains("test hook"),
        "{j}"
    );
    assert!(!topo.exists());
}
