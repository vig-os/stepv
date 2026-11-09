//! `graph::structure` (ADR-0001 §2.2, G3) against its oracle, the kernel:
//! the tree equals `stepv-occt --outline`'s, and the placements equal
//! `--topology`'s `PlacedPart::transform`. Every kernel run goes through
//! `stepv-cap` at 4 GiB, one at a time.
//!
//! The fixtures are written here: an inch sub-assembly in a mm assembly, a
//! relationship that reverses its NAUO, a pair of placements that don't
//! commute, a mapped item, a cycle, and expansions past each cap.
//! `STEPV_CORPUS` (paths, `:`-separated, files or directories) adds the
//! local corpus.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use stepv::graph::{self, Expand, Graph, Placement, Structure};
use stepv::occt;
use stepv::topology::{self, Topology};

// ── The oracle ───────────────────────────────────────────────────────────────

/// Runs the kernel on `input` under the memory cap.
fn kernel(input: &Path, args: &[&str]) -> std::process::Output {
    let k = occt::kernel_path();
    assert!(
        k.is_file(),
        "kernel not found at {}: run `just kernel`",
        k.display()
    );
    Command::new(env!("CARGO_BIN_EXE_stepv-cap"))
        .args(["--memory-mb", "4096", "--"])
        .arg(k)
        .arg(input)
        .args(args)
        .output()
        .unwrap()
}

/// `--outline`: the tree, or the kernel's error.
fn outline(input: &Path) -> Result<Vec<topology::Node>, String> {
    let out = kernel(input, &["--outline"]);
    let line = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(line.lines().next().unwrap_or(""))
        .map_err(|e| format!("no outline ({}): {e}", out.status))?;
    if let Some(e) = v.get("error") {
        return Err(e.as_str().unwrap_or_default().to_owned());
    }
    Ok(serde_json::from_value(v["tree"].clone()).unwrap())
}

/// `--topology` after a full run.
fn topology_of(input: &Path) -> Topology {
    let out = scratch(&format!(
        "topo-{}",
        input.file_name().unwrap().to_string_lossy()
    ));
    let run = kernel(input, &["--topology", out.to_str().unwrap()]);
    assert!(
        run.status.success(),
        "{}: kernel {}",
        input.display(),
        run.status
    );
    let t = Topology::parse(&std::fs::read(&out).unwrap()).unwrap();
    std::fs::remove_file(&out).unwrap();
    t
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("stepv-graph-structure-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// Per product-level leaf (`topology::products`' numbering), the mesh parts
/// under it.
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

/// Each leaf's placement against the kernel's parts under it, within 1e-9
/// relative; the mismatches, described.
fn placement_mismatches(s: &Structure, topo: &Topology) -> Vec<String> {
    let per_leaf = parts_per_leaf(&topo.tree);
    let mut bad = Vec::new();
    for o in s.occurrences(Expand::default()) {
        let o = o.unwrap();
        let Some(leaf) = o.leaf else { continue };
        let Some(parts) = per_leaf.get(leaf as usize) else {
            bad.push(format!("leaf {leaf}: not in the topology"));
            continue;
        };
        for &p in parts {
            let want = topo.parts[p].transform;
            match o.placement {
                Placement::Placed(got) if close(&got, &want) => {}
                got => bad.push(format!("leaf {leaf} part {p}: {got:?}, kernel {want:?}")),
            }
        }
    }
    bad
}

fn close(a: &[f64; 12], b: &[f64; 12]) -> bool {
    let scale = b.iter().fold(1f64, |m, x| m.max(x.abs()));
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 1e-9 * scale)
}

/// The graph's tree against the outline's, and when `placements`, every
/// leaf's placement against the kernel's.
fn check(input: &Path, placements: bool) -> Result<Structure, String> {
    let g = Graph::open(input).map_err(|e| format!("graph: {e}"))?;
    let s = graph::structure(&g).map_err(|e| format!("structure: {e}"))?;
    let tree = s.tree(&g, Expand::default()).map_err(|e| e.to_string());
    let want = outline(input);
    if tree != want {
        return Err(format!("tree: graph {tree:?}\n  outline {want:?}"));
    }
    if let Ok(t) = &tree {
        let n = count_leaves(t);
        if s.leaves != n as u64 {
            return Err(format!("leaves: {} against {n}", s.leaves));
        }
        for leaf in 0..s.leaves.min(10_000) {
            let p = s.leaf_path(leaf).ok_or(format!("no path to leaf {leaf}"))?;
            if s.leaf_of(&p) != Some(leaf) {
                return Err(format!("leaf {leaf}: path {p:?} gives {:?}", s.leaf_of(&p)));
            }
        }
    }
    if placements && tree.is_ok() {
        let bad = placement_mismatches(&s, &topology_of(input));
        if !bad.is_empty() {
            return Err(format!("placements: {bad:#?}"));
        }
    }
    Ok(s)
}

fn count_leaves(t: &[topology::Node]) -> usize {
    t.iter()
        .map(|n| match n {
            topology::Node::Part { .. } => 1,
            topology::Node::Assembly { children, .. } => count_leaves(children),
        })
        .sum()
}

// ── Fixtures ─────────────────────────────────────────────────────────────────

/// A STEP file built a record at a time.
struct Fx {
    out: String,
    next: u32,
    ctx_mm: u32,
    ctx_inch: u32,
}

#[derive(Clone, Copy)]
struct Prod {
    pd: u32,
    sr: u32,
}

impl Fx {
    fn new() -> Self {
        let mut f = Fx {
            out: String::new(),
            next: 1,
            ctx_mm: 0,
            ctx_inch: 0,
        };
        f.add("APPLICATION_CONTEXT('mechanical design')");
        let mm = f.add("(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.))");
        let rad = f.add("(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.))");
        let sr = f.add("(NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT())");
        let u = f.add(&format!("UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-07),#{mm},'distance_accuracy_value','')"));
        let ctx = |f: &mut Fx, len: u32| {
            f.add(&format!("(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{u})) GLOBAL_UNIT_ASSIGNED_CONTEXT((#{len},#{rad},#{sr})) REPRESENTATION_CONTEXT('',''))"))
        };
        f.ctx_mm = ctx(&mut f, mm);
        let m = f.add(&format!(
            "LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#{mm})"
        ));
        let e = f.add("DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.)");
        let inch = f.add(&format!(
            "(CONVERSION_BASED_UNIT('INCH',#{m}) LENGTH_UNIT() NAMED_UNIT(#{e}))"
        ));
        f.ctx_inch = ctx(&mut f, inch);
        f
    }

    fn add(&mut self, record: &str) -> u32 {
        let id = self.next;
        self.next += 1;
        writeln!(self.out, "#{id}={record};").unwrap();
        id
    }

    fn axis(&mut self, at: [f64; 3], z: [f64; 3], x: [f64; 3]) -> u32 {
        let v = |a: [f64; 3]| format!("({:?},{:?},{:?})", a[0], a[1], a[2]);
        let p = self.add(&format!("CARTESIAN_POINT('',{})", v(at)));
        let z = self.add(&format!("DIRECTION('',{})", v(z)));
        let x = self.add(&format!("DIRECTION('',{})", v(x)));
        self.add(&format!("AXIS2_PLACEMENT_3D('',#{p},#{z},#{x})"))
    }

    /// A product; a part when `items` is `None`, holding a circle, else an
    /// assembly whose shape representation lists `items` (its placements).
    fn product(&mut self, name: &str, inch: bool, items: Option<&[u32]>) -> Prod {
        let ctx = if inch { self.ctx_inch } else { self.ctx_mm };
        let pc = self.add("PRODUCT_CONTEXT('',#1,'mechanical')");
        let p = self.add(&format!("PRODUCT('{name}','{name}','',(#{pc}))"));
        let pdf = self.add(&format!("PRODUCT_DEFINITION_FORMATION('','',#{p})"));
        let pdc = self.add("PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design')");
        let pd = self.add(&format!("PRODUCT_DEFINITION('design','',#{pdf},#{pdc})"));
        let pds = self.add(&format!("PRODUCT_DEFINITION_SHAPE('','',#{pd})"));
        let origin = self.axis([0.; 3], [0., 0., 1.], [1., 0., 0.]);
        let sr = match items {
            None => {
                let c = self.axis([1., 2., 3.], [0., 0., 1.], [1., 0., 0.]);
                let circle = self.add(&format!("CIRCLE('',#{c},5.)"));
                let set = self.add(&format!("GEOMETRIC_CURVE_SET('',(#{circle}))"));
                self.add(&format!("GEOMETRICALLY_BOUNDED_WIREFRAME_SHAPE_REPRESENTATION('',(#{origin},#{set}),#{ctx})"))
            }
            Some(items) => {
                let list: Vec<String> = std::iter::once(origin)
                    .chain(items.iter().copied())
                    .map(|i| format!("#{i}"))
                    .collect();
                self.add(&format!(
                    "SHAPE_REPRESENTATION('',({}),#{ctx})",
                    list.join(",")
                ))
            }
        };
        self.add(&format!("SHAPE_DEFINITION_REPRESENTATION(#{pds},#{sr})"));
        Prod { pd, sr }
    }

    /// `child` placed in `parent` by `(item_child, item_parent)`, CAx-IF's
    /// order (rep_1 the child), or reversed (rep_1 the parent).
    fn instance(
        &mut self,
        name: &str,
        parent: Prod,
        child: Prod,
        items: (u32, u32),
        reversed: bool,
    ) {
        let n = self.add(&format!(
            "NEXT_ASSEMBLY_USAGE_OCCURRENCE('{name}','{name}','',#{},#{},$)",
            parent.pd, child.pd
        ));
        let (r1, r2, i1, i2) = if reversed {
            (parent.sr, child.sr, items.1, items.0)
        } else {
            (child.sr, parent.sr, items.0, items.1)
        };
        let t = self.add(&format!("ITEM_DEFINED_TRANSFORMATION('','',#{i1},#{i2})"));
        let rr = self.add(&format!("(REPRESENTATION_RELATIONSHIP('','',#{r1},#{r2}) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{t}) SHAPE_REPRESENTATION_RELATIONSHIP())"));
        let pds = self.add(&format!("PRODUCT_DEFINITION_SHAPE('','',#{n})"));
        self.add(&format!(
            "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{rr},#{pds})"
        ));
    }

    /// Writes the file under `name`.
    fn write(&self, name: &str) -> PathBuf {
        let path = scratch(name);
        let text = format!(
            "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('{name}','2026-11-09T00:00:00',(''),(''),'','','');\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN {{ 1 0 10303 214 1 1 1 1 }}'));\nENDSEC;\nDATA;\n{}ENDSEC;\nEND-ISO-10303-21;\n",
            self.out
        );
        std::fs::write(&path, text).unwrap();
        path
    }
}

const Z: [f64; 3] = [0., 0., 1.];
const X: [f64; 3] = [1., 0., 0.];

/// Rz(90°) at the origin.
fn rz90(f: &mut Fx) -> u32 {
    f.axis([0.; 3], Z, [0., 1., 0.])
}

fn apply(t: &[f64; 12], p: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|r| t[r * 4] * p[0] + t[r * 4 + 1] * p[1] + t[r * 4 + 2] * p[2] + t[r * 4 + 3])
}

fn placed(s: &Structure) -> Vec<[f64; 12]> {
    s.occurrences(Expand::default())
        .map(Result::unwrap)
        .filter(|o| o.leaf.is_some())
        .map(|o| match o.placement {
            Placement::Placed(t) => t,
            u => panic!("{u:?}"),
        })
        .collect()
}

#[test]
fn an_inch_sub_assembly_in_a_mm_assembly_places_as_the_kernel_does() {
    let mut f = Fx::new();
    let part = f.product("pin", false, None);
    let part_in_sub = f.axis([1., 2., 0.], Z, X); // inches
    let sub_here = f.axis([0.5, 0., 0.], [0., 1., 0.], X); // the sub's own frame, inches
    let sub = f.product("inch-sub", true, Some(&[part_in_sub, sub_here]));
    let origin = f.axis([0.; 3], Z, X);
    f.instance("pin-in-sub", sub, part, (origin, part_in_sub), false);
    let sub_in_top = f.axis([100., 0., 0.], Z, X); // mm
    let top = f.product("mm-top", false, Some(&[sub_in_top]));
    f.instance("sub-in-top", top, sub, (sub_here, sub_in_top), false);
    let path = f.write("inch-in-mm.step");
    let s = check(&path, true).unwrap();
    // By hand, in mm: the pin at (25.4, 50.8, 0) in the sub; the sub's frame
    // 12.7 mm along x and turned so its y is the top's z, at x = 100. Raw
    // inches would give (100.5, 0, 2).
    let t = placed(&s)[0];
    let at = [t[3], t[7], t[11]];
    assert!(
        at.iter()
            .zip([112.7, 0., 50.8])
            .all(|(a, b)| (a - b).abs() < 1e-9),
        "{at:?}"
    );
}

#[test]
fn a_relationship_that_reverses_its_nauo_places_as_the_kernel_does() {
    let mut f = Fx::new();
    let part = f.product("pin", false, None);
    let at = f.axis([10., 20., 30.], [0., 1., 0.], X);
    let top = f.product("top", false, Some(&[at]));
    let origin = rz90(&mut f);
    f.instance("reversed", top, part, (origin, at), true);
    check(&f.write("reversed.step"), true).unwrap();
}

#[test]
fn placements_that_do_not_commute_place_as_the_kernel_does_and_not_the_other_way() {
    // The child's item a rotation, the parent's a translation: OCCT's
    // S(child)⁻¹·S(parent) maps the origin to (0, -10, 0); the other order,
    // S(parent)·S(child)⁻¹, would give (10, 0, 0).
    let mut f = Fx::new();
    let part = f.product("pin", false, None);
    let at = f.axis([10., 0., 0.], Z, X);
    let top = f.product("top", false, Some(&[at]));
    let rot = rz90(&mut f);
    f.instance("rotated", top, part, (rot, at), false);
    let path = f.write("non-commuting.step");
    let s = check(&path, true).unwrap();
    let t = placed(&s)[0];
    assert!(
        close(
            &[t[3], t[7], t[11], 0., 0., 0., 0., 0., 0., 0., 0., 0.],
            &[0., -10., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.]
        ),
        "{t:?}"
    );
    // S(parent)·S(child)⁻¹ = Tx(10)·Rz(-90°), against the kernel's.
    let other = [0., 1., 0., 10., -1., 0., 0., 0., 0., 0., 1., 0.];
    let kernel = topology_of(&path).parts[0].transform;
    assert!(
        !close(&other, &kernel),
        "the kernel follows the other order: {kernel:?}"
    );
}

#[test]
fn a_mapped_item_places_as_the_kernel_does() {
    let mut f = Fx::new();
    let c = f.axis([0.; 3], Z, X);
    let circle = f.add(&format!("CIRCLE('',#{c},5.)"));
    let set = f.add(&format!("GEOMETRIC_CURVE_SET('',(#{circle}))"));
    let origin = rz90(&mut f);
    let ctx = f.ctx_inch;
    let mapped = f.add(&format!(
        "GEOMETRICALLY_BOUNDED_WIREFRAME_SHAPE_REPRESENTATION('',(#{origin},#{set}),#{ctx})"
    ));
    let map = f.add(&format!("REPRESENTATION_MAP(#{origin},#{mapped})"));
    let target = f.axis([10., 0., 5.], [0., 1., 0.], X);
    let item = f.add(&format!("MAPPED_ITEM('',#{map},#{target})"));
    let part = f.product("mapped", false, Some(&[item]));
    let path = f.write("mapped.step");
    let s = check(&path, false).unwrap();
    let g = Graph::open(&path).unwrap();
    let (item, rep) = (
        g.node(graph::StepId(item)).unwrap(),
        g.node(graph::StepId(part.sr)).unwrap(),
    );
    let Placement::Placed(t) = graph::mapped_placement(&g, item, rep) else {
        panic!()
    };
    // Unlike an instance's, a mapped item's move is baked into the kernel's
    // geometry: `--topology` gives the part an identity transform and moves
    // the prototype itself. So this checks a vertex rather than a transform:
    // the circle's one vertex, its start (5 in, 0, 0) in the mapped
    // representation, is where `t` puts it.
    let topo = topology_of(&path);
    let part = &topo.parts[parts_per_leaf(&topo.tree)[0][0]];
    let want = apply(&t, [127., 0., 0.]);
    let got: Vec<[f64; 3]> = topo.prototypes[part.prototype]
        .vertices
        .iter()
        .map(|&v| apply(&part.transform, v))
        .collect();
    assert!(
        got.iter()
            .any(|v| v.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-9 * 137.)),
        "{want:?} against {got:?}"
    );
    assert_eq!(s.leaves, 1);
}

#[test]
fn a_cycle_refuses_and_the_outline_fails_it_too() {
    let mut f = Fx::new();
    let a_at = f.axis([1., 0., 0.], Z, X);
    let b_at = f.axis([0., 1., 0.], Z, X);
    let a = f.product("a", false, Some(&[b_at]));
    let b = f.product("b", false, Some(&[a_at]));
    let top_at = f.axis([0., 0., 1.], Z, X);
    let top = f.product("top", false, Some(&[top_at]));
    let o = f.axis([0.; 3], Z, X);
    f.instance("a-in-top", top, a, (o, top_at), false);
    f.instance("b-in-a", a, b, (o, b_at), false);
    f.instance("a-in-b", b, a, (o, a_at), false);
    let path = f.write("cycle.step");
    let g = Graph::open(&path).unwrap();
    let a = g.node(graph::StepId(a.pd)).unwrap();
    let b = g.node(graph::StepId(b.pd)).unwrap();
    match graph::structure(&g) {
        Err(graph::StructureError::Cycle(n)) => assert!(n == a || n == b),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        outline(&path).unwrap_err(),
        "assembly nesting deeper than 60"
    );
}

/// A chain of `levels` assemblies, each instancing the next `fan` times,
/// down to one part.
fn chain(levels: usize, fan: usize) -> Fx {
    let mut f = Fx::new();
    let o = f.axis([0.; 3], Z, X);
    let mut below = f.product("leaf", false, None);
    for level in (0..levels).rev() {
        let at: Vec<u32> = (0..fan)
            .map(|k| f.axis([k as f64, level as f64, 0.], Z, X))
            .collect();
        let me = f.product(&format!("level{level}"), false, Some(&at));
        for (k, &a) in at.iter().enumerate() {
            f.instance(&format!("i{level}.{k}"), me, below, (o, a), false);
        }
        below = me;
    }
    f
}

#[test]
fn a_shared_sub_assembly_past_the_node_cap_fails_as_the_outline_while_structure_stays_small() {
    // 2^20 leaves: 2^21 - 1 occurrences from 21 products.
    let path = chain(20, 2).write("shared.step");
    let g = Graph::open(&path).unwrap();
    let t = Instant::now();
    let s = graph::structure(&g).unwrap();
    let took = t.elapsed();
    assert_eq!(s.products.len(), 21);
    assert_eq!(s.leaves, 1 << 20);
    assert!(took.as_millis() < 200, "{took:?}");
    let e = s.tree(&g, Expand::default()).unwrap_err();
    assert_eq!(Err(e.to_string()), outline(&path).map(|_| ()));
    // Without expanding: the last leaf's path, and back.
    let p = s.leaf_path((1 << 20) - 1).unwrap();
    assert_eq!(p.0.len(), 21);
    assert_eq!(s.leaf_of(&p), Some((1 << 20) - 1));
    assert!(s.leaf_path(1 << 20).is_none());
}

#[test]
fn nesting_past_the_depth_cap_fails_as_the_outline() {
    let path = chain(61, 1).write("deep.step");
    let g = Graph::open(&path).unwrap();
    let s = graph::structure(&g).unwrap();
    let e = s.tree(&g, Expand::default()).unwrap_err();
    assert_eq!(Err(e.to_string()), outline(&path).map(|_| ()));
    // One level less is within the cap, and equals the outline.
    check(&chain(60, 1).write("deep-ok.step"), false).unwrap();
}

#[test]
fn tests_data_equal_the_outline_and_the_topology() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    for f in [
        "assembly.step",
        "sketch.step",
        "no-pcurves.step",
        "multifile/plate.stp",
        "multifile/bracket.step",
        "multifile/pin.stp",
    ] {
        check(&data.join(f), true).unwrap_or_else(|e| panic!("{f}: {e}"));
    }
}

/// The STEP files `STEPV_CORPUS` names (paths, `:`-separated, files or
/// directories). Unset, the corpus tests skip, and say so on the real stderr,
/// which the test harness doesn't capture. Set but naming no file, they fail:
/// a mistyped path in CI must not pass as zero files compared.
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

/// The local corpus, when `STEPV_CORPUS` names it: every STEP file's tree
/// against the outline, the multi-file assemblies included (the outline, like
/// the graph, reads only the given file), and the placements of those with
/// instances against the topology. The two multi-file assemblies' top files
/// differ from the topology by design (their parts live in other files), so
/// only their trees are checked. CI runs it on `tests/fixtures/nist-pmi`
/// (`.github/workflows/kernel.yml`).
#[test]
fn the_corpus_equals_the_outline_and_the_topology() {
    let Some(files) = corpus_files("the_corpus_equals_the_outline_and_the_topology") else {
        return;
    };
    let (mut ok, mut placing, mut bad) = (0, 0, Vec::new());
    for f in &files {
        let stem = f.file_stem().unwrap_or_default();
        let multi = stem == "s1-c5-214" || stem == "s1_pe_214";
        let bytes = std::fs::read(f).unwrap();
        let instances = !multi && find(&bytes, b"NEXT_ASSEMBLY_USAGE_OCCURRENCE");
        placing += usize::from(instances);
        match check(f, instances) {
            Ok(_) => ok += 1,
            Err(e) => bad.push(format!("{}: {e}", f.display())),
        }
    }
    eprintln!(
        "corpus: {} files, {ok} equal, placements checked on {placing}",
        files.len()
    );
    assert!(bad.is_empty(), "{} differ:\n{}", bad.len(), bad.join("\n"));
}

fn find(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
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
