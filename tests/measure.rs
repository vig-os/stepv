//! The measurement server (#33) against the REAL kernel: the acceptance
//! numbers, the sandbox, and that a crash, a hang or a refusal never takes
//! the caller down with it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;
use stepv::measure::{Entity, Error, Query, Server};
use stepv::occt::{self, Limits};
use stepv::topology::{Surface, Topology};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

fn kernel() -> PathBuf {
    let k = occt::kernel_path();
    assert!(
        k.is_file(),
        "kernel not found at {}: run `just kernel`",
        k.display()
    );
    k
}

fn limits() -> Limits {
    Limits {
        timeout: Duration::from_secs(60),
        memory: Some(2 << 30),
    }
}

/// `name`'s topology, from the same kernel.
fn topology(name: &str) -> Topology {
    // Unique per call: tests run in parallel and load the same files.
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let out = std::env::temp_dir().join(format!(
        "stepv-measure-{}-{n}-{name}.json",
        std::process::id()
    ));
    let run = occt::run(
        &kernel(),
        &data(name),
        stepv::Deflection::PREVIEW,
        limits(),
        None,
        Some(&out),
        false,
    )
    .unwrap();
    assert_eq!(run.outcome, occt::Outcome::Ok);
    let t = Topology::parse(&std::fs::read(&out).unwrap()).unwrap();
    let _ = std::fs::remove_file(out);
    t
}

/// #33's acceptance: the distance between the two pins' axes, through their
/// cylinders, is 28 mm (they stand at x = 6 and x = 34).
#[test]
fn the_pins_axes_are_28_mm_apart() {
    let topo = topology("assembly.step");
    // Parts 1 and 2 are the pins; their prototype's cylinder face.
    let proto = &topo.prototypes[topo.parts[1].prototype];
    let face = proto
        .faces
        .iter()
        .position(|f| matches!(f.surface, Surface::Cylinder { radius, .. } if radius == 2.0))
        .expect("the pin's cylinder") as u32;
    let mut s = Server::new(&kernel(), &data("assembly.step"), limits());
    let a = s
        .ask(&Query::Distance(
            Entity::Face { part: 1, face },
            Entity::Face { part: 2, face },
        ))
        .unwrap();
    assert!((a.axis_distance.unwrap() - 28.0).abs() < 1e-6, "{a:?}");
    // Surface to surface: 28 less two radii of 2.
    assert!((a.distance.unwrap() - 24.0).abs() < 1e-6, "{a:?}");
    let [p, q] = a.points.unwrap();
    let gap = (0..3).map(|k| (p[k] - q[k]).powi(2)).sum::<f64>().sqrt();
    assert!((gap - 24.0).abs() < 1e-6, "witness points {p:?} {q:?}");
}

/// #33's acceptance: two adjacent faces of a box meet at 90°.
#[test]
fn adjacent_box_faces_meet_at_90_degrees() {
    let topo = topology("box.brep");
    let faces = &topo.prototypes[0].faces;
    let normal = |i: usize| match faces[i].surface {
        Surface::Plane { normal, .. } => normal,
        _ => panic!("a box face that is not a plane"),
    };
    // Adjacent: they share an edge.
    let (i, j) = (0..faces.len())
        .flat_map(|i| (i + 1..faces.len()).map(move |j| (i, j)))
        .find(|&(i, j)| faces[i].edges.iter().any(|e| faces[j].edges.contains(e)))
        .unwrap();
    let mut s = Server::new(&kernel(), &data("box.brep"), limits());
    let a = s
        .ask(&Query::Angle(
            Entity::Face {
                part: 0,
                face: i as u32,
            },
            Entity::Face {
                part: 0,
                face: j as u32,
            },
        ))
        .unwrap();
    assert!((a.angle_deg.unwrap() - 90.0).abs() < 1e-9, "{a:?}");
    // And a face with the one opposite: 180 between outward normals.
    let opposite = (0..faces.len())
        .find(|&k| {
            let (n, m) = (normal(i), normal(k));
            (n[0] * m[0] + n[1] * m[1] + n[2] * m[2] + 1.0).abs() < 1e-9
        })
        .unwrap();
    let a = s
        .ask(&Query::Angle(
            Entity::Face {
                part: 0,
                face: i as u32,
            },
            Entity::Face {
                part: 0,
                face: opposite as u32,
            },
        ))
        .unwrap();
    assert!((a.angle_deg.unwrap() - 180.0).abs() < 1e-9, "{a:?}");
    // One server, one kernel, for every query.
    assert_eq!(s.starts, 1);
}

#[test]
fn a_point_lands_on_the_face_with_its_normal() {
    let topo = topology("assembly.step");
    let top = topo.prototypes[0]
        .faces
        .iter()
        .position(
            |f| matches!(f.surface, Surface::Plane { normal, .. } if normal == [0.0, 0.0, 1.0]),
        )
        .unwrap() as u32;
    let mut s = Server::new(&kernel(), &data("assembly.step"), limits());
    let a = s
        .ask(&Query::Point(
            Entity::Face { part: 0, face: top },
            [10.0, 5.0, 40.0],
        ))
        .unwrap();
    assert_eq!(a.point.unwrap(), [10.0, 5.0, 5.0]);
    assert_eq!(a.normal.unwrap(), [0.0, 0.0, 1.0]);
}

#[test]
fn the_server_runs_sandboxed() {
    // Inside nix's own build sandbox the kernel's cannot nest (#18).
    if std::env::var_os("NIX_BUILD_TOP").is_some() {
        eprintln!("SKIPPING: inside the nix build sandbox");
        return;
    }
    let mut s = Server::new(&kernel(), &data("assembly.step"), limits());
    s.ask(&Query::Point(Entity::Face { part: 0, face: 0 }, [0.0; 3]))
        .unwrap();
    assert!(s.sandboxed(), "sandbox: {:?}", s.sandbox());
}

#[test]
fn a_refused_query_leaves_the_kernel_running() {
    let mut s = Server::new(&kernel(), &data("assembly.step"), limits());
    let e = s
        .ask(&Query::Distance(
            Entity::Face { part: 9, face: 0 },
            Entity::Face { part: 0, face: 0 },
        ))
        .unwrap_err();
    assert_eq!(e, Error::Refused("a: no such entity".into()));
    // A sphere-free file: an angle needs axes, and a face of the box
    // against an edge is fine; an edge index past the end is not.
    let e = s
        .ask(&Query::Angle(
            Entity::Face { part: 0, face: 0 },
            Entity::Edge { part: 0, edge: 999 },
        ))
        .unwrap_err();
    assert!(matches!(e, Error::Refused(_)), "{e:?}");
    assert_eq!(s.starts, 1, "a refusal is no crash");
}

#[test]
fn a_crashed_kernel_is_restarted() {
    let mut s = Server::new(&kernel(), &data("assembly.step"), limits());
    let q = Query::Point(Entity::Face { part: 0, face: 0 }, [0.0; 3]);
    s.ask(&q).unwrap();
    let pid = s.pid().unwrap();
    kill9(pid);
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        matches!(s.ask(&q), Err(Error::Crashed(_))),
        "the dead kernel is reported"
    );
    s.ask(&q).expect("and the next query restarts it");
    assert_eq!(s.starts, 2);
}

#[test]
fn a_query_past_its_time_limit_is_killed() {
    let limits = Limits {
        timeout: Duration::from_millis(1500),
        memory: None,
    };
    let mut s =
        Server::new(&kernel(), &data("box.brep"), limits).with_env("STEPV_OCCT_TEST_HOOKS", "1");
    s.raw(&json!({"id": 1, "op": "test_sleep", "ms": 10}))
        .expect("a short sleep answers");
    let t = std::time::Instant::now();
    let e = s
        .raw(&json!({"id": 2, "op": "test_sleep", "ms": 60_000}))
        .unwrap_err();
    assert_eq!(e, Error::Timeout);
    assert!(
        t.elapsed() < Duration::from_secs(5),
        "the caller waited {:?}",
        t.elapsed()
    );
    s.raw(&json!({"id": 3, "op": "test_sleep", "ms": 10}))
        .expect("the next query gets a new kernel");
    assert_eq!(s.starts, 2);
}

/// SIGKILL, without a libc dependency in the tests.
fn kill9(pid: u32) {
    let _ = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status();
}
