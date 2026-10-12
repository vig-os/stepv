//! The per-face healing budget (#51). A file without p-curves (SolidWorks
//! writes none) has the reader rebuild every edge's 2D curve on each face by
//! projection; on one face of a real assembly that took 25 s. Each face gets
//! `STEPV_OCCT_FACE_BUDGET_MS` of projection: a face that runs past it is
//! left unhealed, drawn as its outline (Missing) and counted, and the run
//! goes on. These run the real kernel against `tests/data/no-pcurves.step`,
//! the bracket assembly written without p-curves, its pins' cylindrical face
//! green.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use stepv::occt::{self, Summary};
use stepv::topology::Topology;
use stepv::{Color, FaceStatus, Scene};

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

const BUDGET: &str = "STEPV_OCCT_FACE_BUDGET_MS";
const UNHEALED: &str = "STEPV_OCCT_UNHEALED";

struct Out {
    summary: Summary,
    scene: Scene,
    topology: Topology,
}

/// One kernel run with exactly `env` of the two variables set.
fn run_with(input: &str, env: &[(&str, &str)]) -> Out {
    static RUNS: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "stepv-heal-{}-{}",
        std::process::id(),
        RUNS.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let (mesh, topo) = (dir.join("out.mesh"), dir.join("out.json"));
    let mut cmd = Command::new(kernel());
    cmd.arg(data(input))
        .arg("--mesh")
        .arg(&mesh)
        .arg("--topology")
        .arg(&topo)
        .env_remove(BUDGET)
        .env_remove(UNHEALED)
        .envs(env.iter().copied());
    let out = cmd.output().unwrap();
    let line = String::from_utf8(out.stdout).unwrap();
    let summary: Summary = serde_json::from_str(line.lines().next().unwrap())
        .unwrap_or_else(|e| panic!("bad summary ({e}): {line}"));
    assert!(summary.ok, "run failed: {line}");
    let scene = occt::read_mesh(&std::fs::read(&mesh).unwrap()).unwrap();
    let topology = Topology::parse(&std::fs::read(&topo).unwrap()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    Out {
        summary,
        scene,
        topology,
    }
}

/// A run with the budget set (`None`: unset).
fn run(input: &str, budget: Option<&str>) -> Out {
    match budget {
        Some(ms) => run_with(input, &[(BUDGET, ms)]),
        None => run_with(input, &[]),
    }
}

/// Whether some face of the part named `part` has a colour `like`.
fn has_face_color(scene: &Scene, part: &str, like: impl Fn(Color) -> bool) -> bool {
    scene
        .parts
        .iter()
        .filter(|p| p.name.as_deref() == Some(part))
        .any(|p| p.faces.iter().any(|f| f.color.is_some_and(&like)))
}

fn red(c: Color) -> bool {
    c.r > 0.5 && c.g < 0.3 && c.b < 0.3
}

fn green(c: Color) -> bool {
    c.g > 0.5 && c.r < 0.3 && c.b < 0.3
}

#[test]
fn a_file_without_pcurves_heals_within_the_default_budget() {
    let Out {
        summary: s, scene, ..
    } = run("no-pcurves.step", None);
    let with = run("assembly.step", None).summary;
    assert_eq!(s.faces_over_budget, 0);
    assert!(s.unhealed_faces.is_empty());
    assert_eq!(s.faces_unmeshed, 0, "every face meshed first time");
    assert_eq!((s.faces, s.triangles), (with.faces, with.triangles));
    assert_eq!((s.parts_named, s.parts_colored), (3, 3));
    assert!(has_face_color(&scene, "plate", red), "the top face is red");
    assert!(
        has_face_color(&scene, "pin", green),
        "the pins' side is green"
    );
}

#[test]
fn a_face_past_its_budget_is_drawn_as_its_outline_and_the_run_goes_on() {
    let Out {
        summary: s, scene, ..
    } = run("no-pcurves.step", Some("0"));
    let plain = run("no-pcurves.step", None).summary;
    // A zero budget allows no projection, and the two cylinders, the plate's
    // hole and the pin, need it; planes do not (OCCT computes their 2D
    // curves on demand).
    assert_eq!(s.faces_over_budget, 2);
    assert_eq!(s.unhealed_faces.len(), 2);
    assert_eq!(s.faces_unmeshed, s.faces_over_budget);
    assert_eq!(s.faces_missing, s.faces_over_budget);
    // Every face is still there: the plate's 7 and each pin's 3.
    assert_eq!(s.faces, plain.faces);
    assert!(s.triangles > 0 && s.triangles < plain.triangles);
    assert!(s.segments > 0, "the unhealed faces' outlines are drawn");
    for p in &scene.parts {
        let missing = p
            .faces
            .iter()
            .filter(|f| f.status == FaceStatus::Missing)
            .count();
        assert_eq!(missing, 1, "{:?}: one cylinder each", p.name);
        assert!(
            p.faces
                .iter()
                .all(|f| matches!(f.status, FaceStatus::Ok | FaceStatus::Missing))
        );
    }
    // Names and colours come from the file, not from healing: the pins'
    // green side is one of the faces left unhealed.
    assert_eq!((s.parts_named, s.parts_colored), (3, 3));
    assert!(has_face_color(&scene, "plate", red), "the top face is red");
    for p in scene
        .parts
        .iter()
        .filter(|p| p.name.as_deref() == Some("pin"))
    {
        let side = p
            .faces
            .iter()
            .find(|f| f.status == FaceStatus::Missing)
            .unwrap();
        assert!(side.color.is_some_and(green), "the unhealed side is green");
    }
}

#[test]
fn an_unhealed_face_is_bounded_by_its_edges() {
    // Without p-curves OCCT bounds a face by its whole surface, and a
    // cylinder's is infinite.
    let healed = run("no-pcurves.step", None);
    let unhealed = run("no-pcurves.step", Some("0"));
    assert_eq!(unhealed.summary.bbox, healed.summary.bbox);
    for (u, h) in unhealed
        .topology
        .prototypes
        .iter()
        .zip(&healed.topology.prototypes)
    {
        let (u, h) = (u.bbox.unwrap(), h.bbox.unwrap());
        for (a, b) in u.iter().zip(&h) {
            assert!((a - b).abs() < 1e-3, "{u:?} against {h:?}");
        }
    }
}

#[test]
fn a_replay_gives_up_on_the_same_faces_whatever_its_clock() {
    let first = run("no-pcurves.step", Some("0")).summary;
    let list = first
        .unhealed_faces
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    // With no clock to run out, the replay still gives up on those faces...
    let again = run_with("no-pcurves.step", &[(UNHEALED, &list)]).summary;
    assert_eq!(again.unhealed_faces, first.unhealed_faces);
    assert_eq!(again.faces_over_budget, first.faces_over_budget);
    assert_eq!(again.triangles, first.triangles);
    // ...and with a clock that has already run out, on no others.
    let none = run_with("no-pcurves.step", &[(UNHEALED, ""), (BUDGET, "0")]).summary;
    assert_eq!(none.faces_over_budget, 0);
    assert!(none.unhealed_faces.is_empty());
}

#[test]
fn the_budget_spends_nothing_on_a_file_that_carries_its_pcurves() {
    let s = run("assembly.step", Some("0")).summary;
    let plain = run("assembly.step", None).summary;
    assert_eq!(s.faces_over_budget, 0);
    assert_eq!((s.faces, s.triangles), (plain.faces, plain.triangles));
}

#[test]
fn a_budget_that_is_not_a_number_is_the_default() {
    let s = run("no-pcurves.step", Some("soon")).summary;
    assert_eq!(s.faces_over_budget, 0);
    assert_eq!(s.faces_unmeshed, 0);
}
