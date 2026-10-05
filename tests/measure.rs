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
    let close = |x: [f64; 3], y: [f64; 3]| (0..3).all(|k| (x[k] - y[k]).abs() < 1e-9);
    assert!(close(a.point.unwrap(), [10.0, 5.0, 5.0]), "{a:?}");
    assert!(close(a.normal.unwrap(), [0.0, 0.0, 1.0]), "{a:?}");
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

#[test]
fn edges_are_entities_too() {
    // Two box edges that share a vertex meet at 90 degrees; an edge and the
    // one parallel to it across a face are that face's width apart.
    let topo = topology("box.brep");
    let proto = &topo.prototypes[0];
    let ends = |e: usize| proto.edges[e].vertices.map(Option::unwrap);
    let mut s = Server::new(&kernel(), &data("box.brep"), limits());
    let meeting = (1..proto.edges.len())
        .find(|&j| ends(0).iter().any(|v| ends(j).contains(v)))
        .unwrap();
    let a = s
        .ask(&Query::Angle(
            Entity::Edge { part: 0, edge: 0 },
            Entity::Edge {
                part: 0,
                edge: meeting as u32,
            },
        ))
        .unwrap();
    assert!((a.angle_deg.unwrap() - 90.0).abs() < 1e-9, "{a:?}");
    // The distance from edge 0 to every other edge: the parallel ones are
    // at the box's dimensions (20 x 10 x 5), never less than 5 apart unless
    // they touch.
    for j in 1..proto.edges.len() {
        let d = s
            .ask(&Query::Distance(
                Entity::Edge { part: 0, edge: 0 },
                Entity::Edge {
                    part: 0,
                    edge: j as u32,
                },
            ))
            .unwrap()
            .distance
            .unwrap();
        assert!(!(1e-9..5.0 - 1e-9).contains(&d), "edge 0 to {j}: {d}");
    }
}

#[test]
fn malformed_and_over_long_queries_are_refused_and_the_kernel_stays() {
    let mut s = Server::new(&kernel(), &data("box.brep"), limits());
    let e = s
        .raw(&json!({"id": 1, "op": "distance", "a": {"part": 0}}))
        .unwrap_err();
    assert!(matches!(e, Error::Refused(_)), "{e:?}");
    let e = s.raw(&json!({"id": 2, "op": "teleport"})).unwrap_err();
    assert!(matches!(e, Error::Refused(_)), "{e:?}");
    // A line past the kernel's 64 KiB: one refusal, not two answers.
    let long = "x".repeat(100_000);
    let e = s
        .raw(&json!({"id": 3, "op": "ping", "pad": long}))
        .unwrap_err();
    assert_eq!(e, Error::Refused("line too long".into()));
    // And the next query gets its own answer, from the same kernel.
    let q = Query::Point(Entity::Face { part: 0, face: 0 }, [0.0; 3]);
    s.ask(&q).expect("answers after the refusals");
    assert_eq!(s.starts, 1);
}

#[test]
fn a_query_past_its_memory_limit_is_killed() {
    let limits = Limits {
        timeout: Duration::from_secs(30),
        memory: Some(300 << 20),
    };
    let mut s =
        Server::new(&kernel(), &data("box.brep"), limits).with_env("STEPV_OCCT_TEST_HOOKS", "1");
    s.raw(&json!({"id": 1, "op": "test_balloon", "mb": 1}))
        .expect("a small balloon answers");
    let e = s
        .raw(&json!({"id": 2, "op": "test_balloon", "mb": 1024}))
        .unwrap_err();
    assert_eq!(e, Error::MemoryCap);
    s.raw(&json!({"id": 3, "op": "test_balloon", "mb": 1}))
        .expect("the next query gets a new kernel");
    assert_eq!(s.starts, 2);
}

#[test]
fn a_file_that_will_not_load_is_not_reloaded_per_query() {
    let bad = std::env::temp_dir().join(format!("stepv-measure-bad-{}.step", std::process::id()));
    std::fs::write(&bad, "ISO-10303-21;\nHEADER;\nENDSEC;\n").unwrap();
    let mut s = Server::new(&kernel(), &bad, limits());
    let q = Query::Point(Entity::Face { part: 0, face: 0 }, [0.0; 3]);
    assert!(matches!(s.ask(&q), Err(Error::Load(_))));
    assert!(matches!(s.ask(&q), Err(Error::Load(_))));
    assert_eq!(s.starts, 1, "loaded once");
    let _ = std::fs::remove_file(bad);
}

/// SIGKILL, without a libc dependency in the tests.
fn kill9(pid: u32) {
    let _ = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status();
}

/// The area of a cap's triangles.
fn cap_area(c: &stepv::measure::Cap) -> f64 {
    let p = |i: u32| {
        let i = i as usize * 3;
        [0, 1, 2].map(|k| f64::from(c.positions[i + k]))
    };
    c.indices
        .chunks_exact(3)
        .map(|t| {
            let (a, b, d) = (p(t[0]), p(t[1]), p(t[2]));
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [d[0] - a[0], d[1] - a[1], d[2] - a[2]];
            let n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() / 2.0
        })
        .sum()
}

/// #43: the exact section, per part. At y = 15 the plane runs through the
/// plate's hole and both pins' axes: the plate's cap is its 40 x 5 section
/// less the hole's 8 x 5, and each pin's is a 4 x 15 rectangle, the part
/// inside the plate included (the pins interfere with it).
#[test]
fn a_section_caps_each_part_exactly() {
    let mut s = Server::new(&kernel(), &data("assembly.step"), limits());
    let a = s.ask(&Query::Section([0.0, 1.0, 0.0, 15.0])).unwrap();
    let caps = a.caps.expect("a section answer has caps");
    let mut by_part: Vec<(u32, f64)> = caps.iter().map(|c| (c.part, cap_area(c))).collect();
    by_part.sort_by_key(|&(p, _)| p);
    assert_eq!(
        by_part.iter().map(|&(p, _)| p).collect::<Vec<_>>(),
        [0, 1, 2]
    );
    for (&(part, area), want) in by_part.iter().zip([160.0, 60.0, 60.0]) {
        assert!(
            (area - want).abs() < 1e-3,
            "part {part}: {area} mm², want {want}"
        );
    }
    for c in &caps {
        assert!(!c.indices.is_empty() && c.indices.len() % 3 == 0);
        assert!(
            c.indices
                .iter()
                .all(|&i| (i as usize) < c.positions.len() / 3)
        );
        assert!(
            c.positions
                .chunks_exact(3)
                .all(|p| (p[1] - 15.0).abs() < 1e-4),
            "part {}: off the plane",
            c.part
        );
    }
    // The plane is (n, w), kept dot(p, n) <= w: flipped, the same section.
    let flipped = s.ask(&Query::Section([0.0, -1.0, 0.0, -15.0])).unwrap();
    let total = |caps: &[stepv::measure::Cap]| caps.iter().map(cap_area).sum::<f64>();
    assert!((total(&flipped.caps.unwrap()) - 280.0).abs() < 1e-3);
    // Past the model, and through a sketch (no solids): no caps, no error.
    let past = s.ask(&Query::Section([0.0, 1.0, 0.0, 100.0])).unwrap();
    assert_eq!(past.caps.map(|c| c.len()), Some(0));
    let mut sketch = Server::new(&kernel(), &data("sketch.step"), limits());
    let none = sketch.ask(&Query::Section([0.0, 0.0, 1.0, 0.0])).unwrap();
    assert_eq!(none.caps.map(|c| c.len()), Some(0));
}

/// A plane the kernel cannot use is refused, and the server lives on.
#[test]
fn a_degenerate_section_plane_is_refused() {
    let mut s = Server::new(&kernel(), &data("box.brep"), limits());
    let e = s.ask(&Query::Section([0.0, 0.0, 0.0, 1.0])).unwrap_err();
    assert!(matches!(e, Error::Refused(_)), "{e:?}");
    let a = s.ask(&Query::Section([0.0, 0.0, 1.0, 2.5])).unwrap();
    let caps = a.caps.unwrap();
    assert_eq!(caps.len(), 1);
    assert!(
        (cap_area(&caps[0]) - 200.0).abs() < 1e-3,
        "the box is 20 x 10"
    );
    assert_eq!(s.starts, 1);
}
