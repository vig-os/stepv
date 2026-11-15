//! The kernel as the oracle for the graph's corpus gates (G4 `graph_bounds`,
//! G6c `graph_fits`): its mesh and topology of a file under the memory cap,
//! the points on the exact geometry they hold, and the corpus `STEPV_CORPUS`
//! names.

#![allow(dead_code)]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use stepv::topology::{self, Topology};
use stepv::{Scene, occt};

/// Runs the kernel on `input` under the memory cap, for its mesh with edge
/// polylines and its topology.
pub fn kernel(input: &Path) -> Result<(Scene, Topology), String> {
    let k = occt::kernel_path();
    assert!(
        k.is_file(),
        "kernel not found at {}: run `just kernel`",
        k.display()
    );
    let dir = std::env::temp_dir().join(format!("stepv-corpus-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let name = input.file_name().unwrap().to_string_lossy();
    let (mesh, topo) = (
        dir.join(format!("{name}.mesh")),
        dir.join(format!("{name}.topo")),
    );
    let run = Command::new(env!("CARGO_BIN_EXE_stepv-cap"))
        .args(["--memory-mb", "4096", "--"])
        .arg(k)
        .arg(input)
        .arg("--edges")
        .arg("--mesh")
        .arg(&mesh)
        .arg("--topology")
        .arg(&topo)
        .output()
        .unwrap();
    if !run.status.success() {
        return Err(format!("kernel {}", run.status));
    }
    let read = |p: &Path| {
        let b = std::fs::read(p).map_err(|e| e.to_string());
        let _ = std::fs::remove_file(p);
        b
    };
    let scene = occt::read_mesh(&read(&mesh)?).map_err(|e| format!("mesh: {e:?}"))?;
    let t = Topology::parse(&read(&topo)?).map_err(|e| e.to_string())?;
    Ok((scene, t))
}

/// The inverse of a rigid transform, row-major 3×4.
pub fn invert(m: &[f64; 12]) -> [f64; 12] {
    let mut r = [0.; 12];
    for i in 0..3 {
        for j in 0..3 {
            r[i * 4 + j] = m[j * 4 + i];
        }
        r[i * 4 + 3] = -(0..3).map(|k| m[k * 4 + i] * m[k * 4 + 3]).sum::<f64>();
    }
    r
}

pub fn apply(t: &[f64; 12], p: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|r| t[r * 4] * p[0] + t[r * 4 + 1] * p[1] + t[r * 4 + 2] * p[2] + t[r * 4 + 3])
}

/// A point's largest coordinate.
pub fn norm(p: [f64; 3]) -> f64 {
    p.iter().fold(0f64, |m, x| m.max(x.abs()))
}

/// The prototype's edges that bound a face: the others are wireframe,
/// outside the B-rep and the box (ADR §2.3).
pub fn face_edges(proto: &topology::Prototype) -> Vec<bool> {
    let mut on_face = vec![false; proto.edges.len()];
    proto
        .faces
        .iter()
        .flat_map(|f| &f.edges)
        .for_each(|&e| on_face[e] = true);
    on_face
}

/// Mesh part `part`'s points on the exact geometry, in model coordinates:
/// its triangulation's nodes, and its polylines of the edges that bound a
/// face.
pub fn points(scene: &Scene, part: usize, proto: &topology::Prototype) -> Vec<[f64; 3]> {
    let on_face = face_edges(proto);
    let p = &scene.parts[part];
    let xyz = |v: &[f32]| [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])];
    let mut out: Vec<[f64; 3]> = p.mesh.positions.chunks_exact(3).map(xyz).collect();
    let mut at = 0;
    for (&id, &len) in p.edges.ids.iter().zip(&p.edges.lens) {
        let pts = &p.edges.points[at * 3..(at + len as usize) * 3];
        at += len as usize;
        if on_face.get(id as usize) == Some(&true) {
            out.extend(pts.chunks_exact(3).map(xyz));
        }
    }
    out
}

/// The STEP files `STEPV_CORPUS` names (paths, `:`-separated, files or
/// directories). Unset, the corpus test skips, and says so on the real
/// stderr, which the test harness doesn't capture. Set but naming no file,
/// it fails: a mistyped path in CI must not pass as zero files compared.
pub fn corpus_files(test: &str) -> Option<Vec<PathBuf>> {
    let Some(roots) = std::env::var_os("STEPV_CORPUS") else {
        let _ = writeln!(
            std::io::stderr(),
            "{test}: SKIPPED, nothing compared: STEPV_CORPUS is unset"
        );
        return None;
    };
    let mut files = Vec::new();
    for root in std::env::split_paths(&roots) {
        collect(&root, &mut files);
    }
    assert!(
        !files.is_empty(),
        "STEPV_CORPUS={roots:?} names no STEP file"
    );
    files.sort();
    Some(files)
}

pub fn collect(p: &Path, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        if p.file_name().is_some_and(|n| n == "malformed") {
            return;
        }
        for e in std::fs::read_dir(p).unwrap() {
            collect(&e.unwrap().path(), out);
        }
    } else if p
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "step" | "stp" | "p21"))
    {
        out.push(p.to_path_buf());
    }
}
