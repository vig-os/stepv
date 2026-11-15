//! `graph::bounds` (ADR-0001 §2.3, G4) against the exact geometry, as the
//! kernel triangulates it: every node of OCCT's mesh of a product's
//! prototype, every point of its edges' polylines and every B-rep vertex lie
//! inside the graph's box for that product, and, placed, inside the leaf's.
//! Those points lie on the exact faces and edges. Every kernel run goes
//! through `stepv-cap` at 4 GiB, one at a time.
//!
//! OCCT's own prototype box is reported, not gated: `BndLib` boxes a conic
//! arc by points at `r / cos(π/8)`, so its box reaches up to about 8.2% of a
//! radius past the exact shape, and "contains OCCT's box" would test that
//! over-reach. The volume ratio (graph ÷ OCCT box) is reported as tightness
//! data, with the over-reach count. `STEPV_CORPUS` (paths, `:`-separated,
//! files or directories) adds the local corpus.

mod corpus;

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use corpus::{apply, corpus_files, face_edges, invert, kernel, norm, points};
use stepv::BBox;
use stepv::graph::{self, Bound, Graph};
use stepv::topology::{self, Topology};

/// What the files gave.
#[derive(Default)]
struct Tally {
    /// (product, prototype) pairs compared.
    products: usize,
    leaves: usize,
    /// Points on the exact geometry compared: mesh nodes, edge polyline
    /// points and B-rep vertices, in product coordinates.
    points: usize,
    /// Pairs whose graph box is `Unknown`, by reason.
    unknown: BTreeMap<String, usize>,
    violations: Vec<String>,
    /// Products whose OCCT box reaches past the graph's by over 1e-6 of its
    /// diagonal (`BndLib`'s over-reach), and the worst reach in mm.
    over_reach: usize,
    worst_reach: (f64, String),
    /// Products whose OCCT box isn't compared: a prototype has free edges.
    wireframe: usize,
    /// Graph ÷ OCCT box volume, per product, with what it was measured on.
    ratios: Vec<(f64, String)>,
    secs: f64,
}

/// How far `p` lies outside `b`, beyond the slack the mesh's f32
/// coordinates need: 1e-6 of the box's size and of `at`, the largest model
/// coordinate the point was rounded at.
fn outside(b: &BBox, p: [f64; 3], at: f64) -> Option<f64> {
    let size = b.diagonal() + at;
    let over = (0..3)
        .map(|i| (b.min[i] - p[i]).max(p[i] - b.max[i]))
        .fold(0f64, f64::max);
    (over > 1e-6 * size.max(1.)).then_some(over)
}

/// The B-rep vertices of the edges that bound a face, in prototype
/// coordinates.
fn vertices(proto: &topology::Prototype) -> Vec<[f64; 3]> {
    let on_face = face_edges(proto);
    let mut used = vec![false; proto.vertices.len()];
    for (e, _) in proto.edges.iter().zip(&on_face).filter(|(_, f)| **f) {
        e.vertices.iter().flatten().for_each(|&v| used[v] = true);
    }
    proto
        .vertices
        .iter()
        .zip(used)
        .filter_map(|(v, u)| u.then_some(*v))
        .collect()
}

fn check(input: &Path, all: &mut Tally) -> Result<(), String> {
    let g = Graph::open(input).map_err(|e| format!("graph: {e}"))?;
    let t = Instant::now();
    let s = graph::structure(&g).map_err(|e| format!("structure: {e}"))?;
    let b = graph::bounds(&g, &s);
    all.secs += t.elapsed().as_secs_f64();
    let (scene, topo) = kernel(input)?;
    if scene.parts.len() != topo.parts.len() {
        return Err("mesh and topology parts differ".into());
    }
    let file = input.file_name().unwrap().to_string_lossy().into_owned();
    let (mut seen, mut products) = (HashSet::new(), HashSet::new());
    for (leaf, parts) in topology::parts_per_leaf(&topo.tree).iter().enumerate() {
        let path = s
            .leaf_path(leaf as u64)
            .ok_or(format!("no path to leaf {leaf}"))?;
        let (p, _) = *path.0.last().unwrap();
        for &part in parts {
            let placed = &topo.parts[part];
            let proto = &topo.prototypes[placed.prototype];
            let fresh = seen.insert((p, placed.prototype));
            let (gb, lb) = match (b.product(p), b.leaf(leaf as u64)) {
                (Bound::Box(gb), Bound::Box(lb)) => (gb, lb),
                (Bound::Unknown(r), _) | (_, Bound::Unknown(r)) => {
                    if fresh {
                        *all.unknown.entry(r.to_string()).or_default() += 1;
                    }
                    continue;
                }
            };
            let model = points(&scene, part, proto);
            all.leaves += 1;
            if let Some((q, over)) = model
                .iter()
                .find_map(|&q| outside(&lb, q, norm(q)).map(|o| (q, o)))
            {
                all.violations.push(format!(
                    "{file} leaf {leaf}: {q:?} {over:.3e} mm outside {lb:?}"
                ));
            }
            if !fresh {
                continue;
            }
            all.products += 1;
            let back = invert(&placed.transform);
            let local = model.iter().map(|&q| (apply(&back, q), norm(q)));
            let exact = vertices(proto).into_iter().map(|v| (v, 0.));
            let mut bad = None;
            for (q, at) in local.chain(exact) {
                all.points += 1;
                if let Some(over) = outside(&gb, q, at) {
                    bad.get_or_insert(format!(
                        "{file} product {p}: {q:?} {over:.3e} mm outside {gb:?}"
                    ));
                }
            }
            all.violations.extend(bad);
        }
        if let (true, Bound::Box(gb)) = (products.insert(p), b.product(p)) {
            occt_box(all, &file, p, &gb, &topo, parts);
        }
    }
    Ok(())
}

/// OCCT's box of product `p`, the union of its leaf's prototypes' boxes, as
/// data: how far it reaches past the graph's box, and the volume ratio.
/// Skipped when a prototype has free edges, which OCCT's box holds and the
/// graph's leaves out.
fn occt_box(all: &mut Tally, file: &str, p: usize, gb: &BBox, topo: &Topology, parts: &[usize]) {
    let mut o = [
        f64::INFINITY,
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for &part in parts {
        let proto = &topo.prototypes[topo.parts[part].prototype];
        let Some(b) = proto.bbox else { continue };
        if face_edges(proto).contains(&false) {
            all.wireframe += 1;
            return;
        }
        for i in 0..3 {
            o[i] = o[i].min(b[i]);
            o[3 + i] = o[3 + i].max(b[3 + i]);
        }
    }
    if !o[0].is_finite() {
        return;
    }
    let reach = (0..3)
        .map(|i| (gb.min[i] - o[i]).max(o[3 + i] - gb.max[i]))
        .fold(0f64, f64::max);
    if reach > 1e-6 * gb.diagonal().max(1.) {
        all.over_reach += 1;
    }
    if reach > all.worst_reach.0 {
        all.worst_reach = (reach, format!("{file} product {p}"));
    }
    // Extents floored at 1e-3 of the OCCT box's diagonal, so a flat part's
    // ratio measures its outline, not float noise.
    let d = [0, 1, 2].map(|i| o[3 + i] - o[i]);
    let floor = 1e-3 * (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let vol = |d: [f64; 3]| d.iter().map(|x| x.max(floor)).product::<f64>();
    if floor > 0. {
        let gd = [0, 1, 2].map(|i| gb.max[i] - gb.min[i]);
        all.ratios
            .push((vol(gd) / vol(d), format!("{file} product {p}")));
    }
}

fn quantile(v: &[(f64, String)], q: f64) -> f64 {
    v.get(((v.len().max(1) - 1) as f64 * q).round() as usize)
        .map_or(f64::NAN, |r| r.0)
}

/// Runs `check` on `files`, reports on stderr, and fails on any point
/// outside its box or when nothing was compared.
fn run(files: &[PathBuf]) {
    let mut all = Tally::default();
    for f in files {
        let stem = f.file_stem().unwrap_or_default();
        // The multi-file assemblies' top files: their parts live in other files.
        if stem == "s1-c5-214" || stem == "s1_pe_214" {
            continue;
        }
        if let Err(e) = check(f, &mut all) {
            eprintln!("{}: not compared: {e}", f.display());
        }
    }
    all.ratios.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (median, p95) = (quantile(&all.ratios, 0.5), quantile(&all.ratios, 0.95));
    eprintln!(
        "bounds: {} files; {} products ({} points) and {} leaves compared, {} violations; unknown {:?}; \
         structure + boxes {:.3} s (debug)",
        files.len(),
        all.products,
        all.points,
        all.leaves,
        all.violations.len(),
        all.unknown,
        all.secs
    );
    eprintln!(
        "bounds: OCCT's box reaches past the graph's on {} of {} products ({} with wireframe not compared), worst {:.3e} mm ({}); \
         tightness (graph ÷ OCCT volume) median {median:.3}, p95 {p95:.3}, top {:?}",
        all.over_reach,
        all.ratios.len(),
        all.wireframe,
        all.worst_reach.0,
        all.worst_reach.1,
        all.ratios.iter().rev().take(5).collect::<Vec<_>>()
    );
    assert!(
        all.violations.is_empty(),
        "{} outside:\n{}",
        all.violations.len(),
        all.violations.join("\n")
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

/// A one-part file whose shape representation lists `items`, written by
/// `records` (ids from #50 up), in mm.
fn part_file(name: &str, items: &str, records: &str) -> PathBuf {
    let path = std::env::temp_dir()
        .join(format!("stepv-graph-bounds-{}", std::process::id()))
        .join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let text = format!(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('{name}','2026-11-09T00:00:00',(''),(''),'','','');\n\
         FILE_SCHEMA(('AUTOMOTIVE_DESIGN {{ 1 0 10303 214 1 1 1 1 }}'));\nENDSEC;\nDATA;\n\
         #1=APPLICATION_CONTEXT('mechanical design');\n\
         #2=APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#1);\n\
         #3=PRODUCT_CONTEXT('',#1,'mechanical');\n#4=PRODUCT('p','p','',(#3));\n\
         #5=PRODUCT_DEFINITION_FORMATION('','',#4);\n#6=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');\n\
         #7=PRODUCT_DEFINITION('design','',#5,#6);\n#8=PRODUCT_DEFINITION_SHAPE('','',#7);\n\
         #9=SHAPE_DEFINITION_REPRESENTATION(#8,#10);\n#10=SHAPE_REPRESENTATION('',({items}),#20);\n\
         #11=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n\
         #12=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\n\
         #13=(NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT());\n\
         #14=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-07),#11,'distance_accuracy_value','');\n\
         #20=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#14)) \
         GLOBAL_UNIT_ASSIGNED_CONTEXT((#11,#12,#13)) REPRESENTATION_CONTEXT('',''));\n\
         {records}ENDSEC;\nEND-ISO-10303-21;\n"
    );
    std::fs::write(&path, text).unwrap();
    path
}

/// A triangle (0,0,0), (10,0,0), (0,10,0) as a surface model: item #79.
const TRIANGLE: &str = "\
#50=CARTESIAN_POINT('',(0.,0.,0.));#51=CARTESIAN_POINT('',(10.,0.,0.));#52=CARTESIAN_POINT('',(0.,10.,0.));
#53=VERTEX_POINT('',#50);#54=VERTEX_POINT('',#51);#55=VERTEX_POINT('',#52);
#56=DIRECTION('',(1.,0.,0.));#57=VECTOR('',#56,1.);#58=LINE('',#50,#57);
#59=DIRECTION('',(-0.70710678118654746,0.70710678118654746,0.));#60=VECTOR('',#59,1.);#61=LINE('',#51,#60);
#62=DIRECTION('',(0.,-1.,0.));#63=VECTOR('',#62,1.);#64=LINE('',#52,#63);
#65=EDGE_CURVE('',#53,#54,#58,.T.);#66=EDGE_CURVE('',#54,#55,#61,.T.);#67=EDGE_CURVE('',#55,#53,#64,.T.);
#68=ORIENTED_EDGE('',*,*,#65,.T.);#69=ORIENTED_EDGE('',*,*,#66,.T.);#70=ORIENTED_EDGE('',*,*,#67,.T.);
#71=EDGE_LOOP('',(#68,#69,#70));#72=FACE_OUTER_BOUND('',#71,.T.);
#73=DIRECTION('',(0.,0.,1.));#74=AXIS2_PLACEMENT_3D('',#50,#73,#56);#75=PLANE('',#74);
#76=ADVANCED_FACE('',(#72),#75,.T.);#77=OPEN_SHELL('',(#76));#79=SHELL_BASED_SURFACE_MODEL('',(#77));
";

/// A `GEOMETRIC_SET` (item #90) of a bilinear B-spline surface reaching
/// z = 30, far off the triangle, and a circle, which is wireframe.
const SET: &str = "\
#80=CARTESIAN_POINT('',(0.,0.,20.));#81=CARTESIAN_POINT('',(0.,5.,25.));
#82=CARTESIAN_POINT('',(5.,0.,20.));#83=CARTESIAN_POINT('',(5.,5.,30.));
#84=B_SPLINE_SURFACE_WITH_KNOTS('',1,1,((#80,#81),(#82,#83)),.UNSPECIFIED.,.F.,.F.,.F.,(2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.);
#85=CARTESIAN_POINT('',(0.,0.,-50.));#86=DIRECTION('',(0.,0.,1.));#87=DIRECTION('',(1.,0.,0.));
#88=AXIS2_PLACEMENT_3D('',#85,#86,#87);#89=CIRCLE('',#88,100.);
#90=GEOMETRIC_SET('',(#84,#89));
";

/// A geometric set's surfaces are faces in OCCT's transfer, so they are in
/// the box; its curves and points are wireframe and are not (ADR §2.3).
#[test]
fn a_geometric_sets_surfaces_are_in_the_box() {
    run(&[
        part_file("brep-and-set.step", "#79,#90", &format!("{TRIANGLE}{SET}")),
        part_file("set-alone.step", "#90", SET),
    ]);
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
