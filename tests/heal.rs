//! The per-face healing budget (#51). A file without p-curves (SolidWorks
//! writes none) has the reader rebuild every edge's 2D curve on each face by
//! projection; on one face of a real assembly that took 25 s. Each face gets
//! `STEPV_OCCT_FACE_BUDGET_MS` of projection: a face that runs past it is
//! left unhealed, drawn as its outline (Missing) and counted, and the run
//! goes on. These run the real kernel against `tests/data/no-pcurves.step`,
//! the bracket assembly written without p-curves.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use stepv::occt::{self, Summary};
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

/// One kernel run with the budget set (`None`: the variable unset), its
/// summary and its mesh.
fn run(input: &str, budget: Option<&str>) -> (Summary, Scene) {
    static RUNS: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "stepv-heal-{}-{}",
        std::process::id(),
        RUNS.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let mesh = dir.join("out.mesh");
    let mut cmd = Command::new(kernel());
    cmd.arg(data(input)).arg("--mesh").arg(&mesh);
    match budget {
        Some(ms) => cmd.env("STEPV_OCCT_FACE_BUDGET_MS", ms),
        None => cmd.env_remove("STEPV_OCCT_FACE_BUDGET_MS"),
    };
    let out = cmd.output().unwrap();
    let line = String::from_utf8(out.stdout).unwrap();
    let summary: Summary = serde_json::from_str(line.lines().next().unwrap())
        .unwrap_or_else(|e| panic!("bad summary ({e}): {line}"));
    assert!(summary.ok, "run failed: {line}");
    let scene = occt::read_mesh(&std::fs::read(&mesh).unwrap()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    (summary, scene)
}

/// The plate's top face is red in the file (tests/data, fixture-gen).
fn has_red_face(scene: &Scene) -> bool {
    let red = |c: Color| c.r > 0.5 && c.g < 0.3 && c.b < 0.3;
    scene
        .parts
        .iter()
        .filter(|p| p.name.as_deref() == Some("plate"))
        .any(|p| p.faces.iter().any(|f| f.color.is_some_and(red)))
}

#[test]
fn a_file_without_pcurves_heals_within_the_default_budget() {
    let (s, scene) = run("no-pcurves.step", None);
    let (with, _) = run("assembly.step", None);
    assert_eq!(s.faces_over_budget, 0);
    assert_eq!(s.faces_unmeshed, 0, "every face meshed first time");
    assert_eq!((s.faces, s.triangles), (with.faces, with.triangles));
    assert_eq!((s.parts_named, s.parts_colored), (3, 3));
    assert!(has_red_face(&scene), "the top face keeps its colour");
}

#[test]
fn a_face_past_its_budget_is_drawn_as_its_outline_and_the_run_goes_on() {
    let (s, scene) = run("no-pcurves.step", Some("0"));
    let (plain, _) = run("no-pcurves.step", None);
    // A zero budget allows no projection, and the two cylinders, the plate's
    // hole and the pin, need it; planes do not (OCCT computes their 2D
    // curves on demand).
    assert_eq!(s.faces_over_budget, 2);
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
    // Names and colours come from the file, not from healing.
    assert_eq!((s.parts_named, s.parts_colored), (3, 3));
    assert!(has_red_face(&scene), "the top face keeps its colour");
}

#[test]
fn the_budget_spends_nothing_on_a_file_that_carries_its_pcurves() {
    let (s, _) = run("assembly.step", Some("0"));
    let (plain, _) = run("assembly.step", None);
    assert_eq!(s.faces_over_budget, 0);
    assert_eq!((s.faces, s.triangles), (plain.faces, plain.triangles));
}

#[test]
fn a_budget_that_is_not_a_number_is_the_default() {
    let (s, _) = run("no-pcurves.step", Some("soon"));
    assert_eq!(s.faces_over_budget, 0);
    assert_eq!(s.faces_unmeshed, 0);
}
