//! `graph::bounds` (ADR-0001 §2.3, G4) against its oracle, the kernel: each
//! product's graph box contains the box `--topology` gives its prototype,
//! within the kernel's tolerance enlargement, and each leaf's box contains
//! that box carried by the part's transform. Every kernel run goes through
//! `stepv-cap` at 4 GiB, one at a time.
//!
//! The tightness ratio (graph box ÷ OCCT box, by volume) is reported as
//! data, not a gate. `STEPV_CORPUS` (paths, `:`-separated, files or
//! directories) adds the local corpus.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use stepv::graph::{self, Bound, Graph};
use stepv::occt;
use stepv::topology::{self, Topology};

/// How far an OCCT box may reach past the graph's: `BRepBndLib::Add` grows
/// each box by its shapes' tolerances, and the kernel reads with OCCT's
/// default `read.maxprecision.val`, 1 mm, the most healing lets a tolerance
/// grow to (ADR §2.3: "within the kernel's own enlargement").
const ENLARGEMENT_MM: f64 = 1.0;

/// Runs the kernel's `--topology` on `input` under the memory cap.
fn topology_of(input: &Path) -> Result<Topology, String> {
    let k = occt::kernel_path();
    assert!(
        k.is_file(),
        "kernel not found at {}: run `just kernel`",
        k.display()
    );
    let dir = std::env::temp_dir().join(format!("stepv-graph-bounds-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join(input.file_name().unwrap());
    let run = Command::new(env!("CARGO_BIN_EXE_stepv-cap"))
        .args(["--memory-mb", "4096", "--"])
        .arg(k)
        .arg(input)
        .arg("--topology")
        .arg(&out)
        .output()
        .unwrap();
    if !run.status.success() {
        return Err(format!("kernel {}", run.status));
    }
    let t = Topology::parse(&std::fs::read(&out).unwrap()).map_err(|e| e.to_string());
    std::fs::remove_file(&out).unwrap();
    t
}

/// Per product-level leaf (the graph's numbering), the mesh parts under it:
/// a leaf whose shape the kernel splits shows them as unnamed children.
fn parts_per_leaf(tree: &[topology::Node]) -> Vec<Vec<usize>> {
    fn unnamed(n: &topology::Node) -> bool {
        match n {
            topology::Node::Part { name, .. } => name.is_empty(),
            topology::Node::Assembly { name, children } => {
                name.is_empty() && children.iter().all(unnamed)
            }
        }
    }
    fn all(n: &topology::Node, out: &mut Vec<usize>) {
        match n {
            topology::Node::Part { part, .. } => out.push(*part),
            topology::Node::Assembly { children, .. } => children.iter().for_each(|c| all(c, out)),
        }
    }
    fn go(n: &topology::Node, out: &mut Vec<Vec<usize>>) {
        match n {
            topology::Node::Assembly { children, .. }
                if children.is_empty() || !children.iter().all(unnamed) =>
            {
                children.iter().for_each(|c| go(c, out));
            }
            _ => {
                let mut v = Vec::new();
                all(n, &mut v);
                out.push(v);
            }
        }
    }
    let mut out = Vec::new();
    tree.iter().for_each(|n| go(n, &mut out));
    out
}

/// What one file gave.
#[derive(Default)]
struct Tally {
    /// (product, prototype) pairs compared.
    products: usize,
    leaves: usize,
    /// Pairs whose graph box is `Unknown`, by reason.
    unknown: std::collections::BTreeMap<String, usize>,
    /// Prototypes with edges or vertices outside any face: wireframe, which
    /// the graph's box leaves out by design (ADR §2.3).
    wireframe: usize,
    violations: Vec<String>,
    /// Graph ÷ OCCT box volume, per pair.
    ratios: Vec<f64>,
    /// Kernel vertices (exact points of the B-rep) outside the graph box by
    /// more than 1e-6 of its diagonal.
    vertex_violations: Vec<String>,
    vertices: usize,
    /// The largest reach of an OCCT box past the graph's, in mm.
    worst: f64,
    secs: f64,
}

/// How far `o` reaches past `g`, when past the enlargement; the largest
/// reach, within it or not, goes to `worst`.
fn contains(g: &stepv::BBox, o: &[f64; 6], worst: &mut f64) -> Option<f64> {
    let over = (0..3)
        .map(|i| (g.min[i] - o[i]).max(o[3 + i] - g.max[i]))
        .fold(0f64, f64::max);
    *worst = worst.max(over);
    (over > ENLARGEMENT_MM).then_some(over)
}

/// A prototype has geometry outside its faces.
fn wireframe(p: &topology::Prototype) -> bool {
    let mut on_face = vec![false; p.edges.len()];
    p.faces
        .iter()
        .flat_map(|f| &f.edges)
        .for_each(|&e| on_face[e] = true);
    let mut on_edge = vec![false; p.vertices.len()];
    p.edges
        .iter()
        .flat_map(|e| e.vertices.iter().flatten())
        .for_each(|&v| on_edge[v] = true);
    on_face.contains(&false) || on_edge.contains(&false)
}

fn apply(t: &[f64; 12], p: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|r| t[r * 4] * p[0] + t[r * 4 + 1] * p[1] + t[r * 4 + 2] * p[2] + t[r * 4 + 3])
}

/// The box of `o`'s 8 corners under `t`.
fn placed(o: &[f64; 6], t: &[f64; 12]) -> [f64; 6] {
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for k in 0..8 {
        let p = apply(
            t,
            [0, 1, 2].map(|i| o[if k >> i & 1 == 0 { i } else { 3 + i }]),
        );
        for i in 0..3 {
            b[i] = b[i].min(p[i]);
            b[3 + i] = b[3 + i].max(p[i]);
        }
    }
    b
}

fn check(input: &Path) -> Result<Tally, String> {
    let g = Graph::open(input).map_err(|e| format!("graph: {e}"))?;
    let t1 = Instant::now();
    let s = graph::structure(&g).map_err(|e| format!("structure: {e}"))?;
    let b = graph::bounds(&g, &s);
    let mut tally = Tally {
        secs: t1.elapsed().as_secs_f64(),
        ..Tally::default()
    };
    let topo = topology_of(input)?;
    let per_leaf = parts_per_leaf(&topo.tree);
    let mut seen = std::collections::HashSet::new();
    for (leaf, parts) in per_leaf.iter().enumerate() {
        let path = s
            .leaf_path(leaf as u64)
            .ok_or(format!("no path to leaf {leaf}"))?;
        let (p, _) = *path.0.last().unwrap();
        for &part in parts {
            let placed_part = &topo.parts[part];
            let proto = &topo.prototypes[placed_part.prototype];
            let Some(o) = proto.bbox else { continue };
            let fresh = seen.insert((p, placed_part.prototype));
            if wireframe(proto) {
                tally.wireframe += usize::from(fresh);
                continue;
            }
            match (b.product(p), b.leaf(leaf as u64)) {
                (Bound::Box(gb), Bound::Box(lb)) => {
                    tally.leaves += 1;
                    if let Some(over) =
                        contains(&lb, &placed(&o, &placed_part.transform), &mut tally.worst)
                    {
                        tally
                            .violations
                            .push(format!("leaf {leaf} part {part}: {over:.3e} mm outside"));
                    }
                    if !fresh {
                        continue;
                    }
                    tally.products += 1;
                    let slack = 1e-6 * gb.diagonal().max(1.);
                    for v in &proto.vertices {
                        tally.vertices += 1;
                        if (0..3).any(|i| v[i] < gb.min[i] - slack || v[i] > gb.max[i] + slack) {
                            tally
                                .vertex_violations
                                .push(format!("product {p}: vertex {v:?} outside {gb:?}"));
                        }
                    }
                    if let Some(over) = contains(&gb, &o, &mut tally.worst) {
                        tally.violations.push(format!(
                            "product {p} prototype {}: {over:.3e} mm outside; graph {gb:?}, kernel {o:?}",
                            placed_part.prototype
                        ));
                    }
                    // Extents floored at 1e-3 of the OCCT box's diagonal, so a
                    // flat part's ratio measures its outline, not float noise.
                    let d = [0, 1, 2].map(|i| o[3 + i] - o[i]);
                    let floor = 1e-3 * (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                    let vol = |d: [f64; 3]| d.iter().map(|x| x.max(floor)).product::<f64>();
                    if floor > 0. {
                        let gd = [0, 1, 2].map(|i| gb.max[i] - gb.min[i]);
                        tally.ratios.push(vol(gd) / vol(d));
                    }
                }
                (Bound::Unknown(r), _) | (_, Bound::Unknown(r)) => {
                    if fresh {
                        *tally.unknown.entry(r.to_string()).or_default() += 1;
                    }
                }
            }
        }
    }
    Ok(tally)
}

fn quantile(v: &mut [f64], q: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * q).round() as usize]
}

/// Runs `check` on `files`, reports on stderr, and fails on any violation
/// or when nothing was compared.
fn run(files: &[PathBuf]) {
    let mut all = Tally::default();
    let mut bad = Vec::new();
    for f in files {
        let stem = f.file_stem().unwrap_or_default();
        // The multi-file assemblies' top files: their parts live in other files.
        if stem == "s1-c5-214" || stem == "s1_pe_214" {
            continue;
        }
        match check(f) {
            Ok(t) => {
                if !t.violations.is_empty() {
                    bad.push(format!("{}:\n  {}", f.display(), t.violations.join("\n  ")));
                }
                eprintln!(
                    "{}: {} products, {} leaves, {} unknown, {} wireframe, {} violations (worst reach {:.2e} mm), structure + boxes {:.3} s",
                    f.display(),
                    t.products,
                    t.leaves,
                    t.unknown.values().sum::<usize>(),
                    t.wireframe,
                    t.violations.len(),
                    t.worst,
                    t.secs
                );
                all.products += t.products;
                all.leaves += t.leaves;
                all.wireframe += t.wireframe;
                all.violations.extend(t.violations);
                all.ratios.extend(t.ratios);
                all.vertices += t.vertices;
                if !t.vertex_violations.is_empty() {
                    eprintln!(
                        "{}: VERTICES OUTSIDE:\n  {}",
                        f.display(),
                        t.vertex_violations.join("\n  ")
                    );
                }
                all.vertex_violations.extend(t.vertex_violations);
                all.worst = all.worst.max(t.worst);
                for (k, v) in t.unknown {
                    *all.unknown.entry(k).or_default() += v;
                }
            }
            Err(e) => eprintln!("{}: not compared: {e}", f.display()),
        }
    }
    let n = all.ratios.len();
    let (median, p95) = (
        quantile(&mut all.ratios, 0.5),
        quantile(&mut all.ratios, 0.95),
    );
    eprintln!(
        "bounds: {} files, {} products and {} leaves compared, {} violations (worst reach {:.2e} mm, allowed {ENLARGEMENT_MM} mm); {} wireframe, unknown {:?}; \
         tightness (graph ÷ OCCT volume, {n} products) median {median:.3}, p95 {p95:.3}",
        files.len(),
        all.products,
        all.leaves,
        all.violations.len(),
        all.worst,
        all.wireframe,
        all.unknown
    );
    eprintln!(
        "bounds: {} kernel vertices checked, {} outside the graph box",
        all.vertices,
        all.vertex_violations.len()
    );
    assert!(
        bad.is_empty(),
        "{} files violate:\n{}",
        bad.len(),
        bad.join("\n")
    );
    assert!(all.products > 0, "no product compared");
}

#[test]
fn tests_data_boxes_contain_the_kernels() {
    // `multifile/bracket.step` is left out: its parts' geometry is in the
    // other files, which the kernel reads and the graph doesn't (#92).
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    run(&[
        "assembly.step",
        "no-pcurves.step",
        "multifile/plate.stp",
        "multifile/pin.stp",
    ]
    .map(|f| data.join(f)));
}

/// The STEP files `STEPV_CORPUS` names (paths, `:`-separated, files or
/// directories). Unset, the corpus test skips, and says so on the real
/// stderr, which the test harness doesn't capture. Set but naming no file,
/// it fails: a mistyped path in CI must not pass as zero files compared.
fn corpus_files(test: &str) -> Option<Vec<PathBuf>> {
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

/// The local corpus, when `STEPV_CORPUS` names it: every product's box
/// against the kernel's, failing on any violation or when it compared no
/// product. CI runs it on `tests/fixtures/nist-pmi` and `occt-import-js`
/// (`.github/workflows/kernel.yml`).
#[test]
fn the_corpus_boxes_contain_the_kernels() {
    let Some(files) = corpus_files("the_corpus_boxes_contain_the_kernels") else {
        return;
    };
    run(&files);
}

fn collect(p: &Path, out: &mut Vec<PathBuf>) {
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
