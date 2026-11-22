//! `mesh::edges` (ADR-0001 §2.6, M1a) against OCCT's edge polylines, the
//! golden reference: the kernel's `--edges` output, at the same deflection
//! (its own absolute d, from its model diagonal, and 20°).
//!
//! **The join.** The kernel numbers a prototype's edges in its own order,
//! with no STEP entity id until M5, so an `EDGE_CURVE` is joined to OCCT's
//! edges by geometry: a candidate's ends equal the Rust polyline's (in
//! either order) within d, and the pair matches when every Rust point lies
//! within d of OCCT's polyline (Rust's points are on the curve, OCCT's
//! polyline within its d of it) and every OCCT point within `2·d` (d plus
//! OCCT's own d) of Rust's. Every Rust
//! polyline must match an OCCT edge of its product, and every OCCT line,
//! circle or ellipse edge that bounds a face must match a Rust polyline:
//! 0 unmatched either way. The two directions' largest distances are
//! reported in units of d.
//!
//! Products are compared in their own mm: OCCT's points are taken back
//! through each part's placement, as `graph_bounds` does. `STEPV_CORPUS`
//! (paths, `:`-separated, files or directories) adds the local corpus.

mod corpus;
mod stepgen;

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use corpus::{apply, corpus_files, face_edges, invert, kernel_summary};
use stepgen::{F, part_file};
use stepv::Deflection;
use stepv::graph::{self, Graph, Node};
use stepv::mesh::{Polylines, Tolerance};
use stepv::topology::{self, Curve};

#[derive(Default)]
struct Tally {
    products: usize,
    rust: usize,
    occt: usize,
    /// Products whose geometry is in other files.
    external: usize,
    /// Rust polylines joined to one OCCT edge, and polylines of either side
    /// compared point by point instead.
    joined: usize,
    by_points: usize,
    /// Rust edges without a polyline, by reason.
    routed: BTreeMap<String, usize>,
    unmatched: Vec<String>,
    /// The largest distance of a Rust point from its OCCT polyline, and of
    /// an OCCT point from its Rust polyline, in units of d.
    worst: [f64; 2],
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = sub(a, b);
    dot(d, d).sqrt()
}

/// The distance from `q` to the polyline `p`.
fn to_polyline(q: [f64; 3], p: &[[f64; 3]]) -> f64 {
    p.windows(2)
        .map(|w| {
            let ab = sub(w[1], w[0]);
            let l = dot(ab, ab);
            let t = if l > 0. {
                (dot(sub(q, w[0]), ab) / l).clamp(0., 1.)
            } else {
                0.
            };
            dist(q, [0, 1, 2].map(|i| w[0][i] + t * ab[i]))
        })
        .fold(f64::INFINITY, f64::min)
}

/// The larger of the two one-sided distances, each the largest over a
/// polyline's points.
fn apart(a: &[[f64; 3]], b: &[[f64; 3]]) -> [f64; 2] {
    let one =
        |a: &[[f64; 3]], b: &[[f64; 3]]| a.iter().map(|&q| to_polyline(q, b)).fold(0f64, f64::max);
    [one(a, b), one(b, a)]
}

/// The `EDGE_CURVE`s under a product's bodies and the representations
/// they're related to without a transformation (AP203 hybrids, as
/// `graph::bounds` adds them), in the graph's order.
fn edges_of(g: &Graph, bodies: &[Node]) -> Vec<Node> {
    let mut reps = bodies.to_vec();
    let mut k = 0;
    while let Some(&rep) = reps.get(k) {
        k += 1;
        for &r in g.referrers(rep) {
            let simple = |t| g.type_name(r) == graph::TypeName::Simple(t);
            if simple("SHAPE_REPRESENTATION_RELATIONSHIP")
                || simple("MECHANICAL_DESIGN_AND_DRAUGHTING_RELATIONSHIP")
            {
                for &o in g.refs(r) {
                    if !reps.contains(&o) {
                        reps.push(o);
                    }
                }
            }
        }
    }
    let (mut seen, mut out, mut stack) = (HashSet::new(), Vec::new(), reps);
    while let Some(n) = stack.pop() {
        if !seen.insert(n) || g.is_a(n, "MAPPED_ITEM") {
            continue;
        }
        if g.is_a(n, "EDGE_CURVE") {
            out.push(n);
        } else {
            stack.extend(g.refs(n).iter().rev());
        }
    }
    out
}

fn check(input: &Path, all: &mut Tally) -> Result<(), String> {
    let g = Graph::open(input).map_err(|e| format!("graph: {e}"))?;
    let s = graph::structure(&g).map_err(|e| format!("structure: {e}"))?;
    let (scene, topo, summary) = kernel_summary(input)?;
    let d = summary.linear_abs;
    let tol = Tolerance::from_mm(d, Deflection::PREVIEW.angular_deg.to_radians())
        .ok_or("no deflection")?;
    let file = input.file_name().unwrap().to_string_lossy().into_owned();
    let mut done = HashSet::new();
    for (leaf, parts) in topology::parts_per_leaf(&topo.tree).iter().enumerate() {
        let path = s.leaf_path(leaf as u64).ok_or("no leaf path")?;
        let (p, _) = *path.0.last().unwrap();
        if !done.insert(p) {
            continue;
        }
        let me = &s.products[p];
        let edges = edges_of(&g, &me.bodies);
        let rust = Polylines::build(&g, edges.iter().copied(), me.mm_per_unit, tol);
        // OCCT's analytic face edges, in product mm, each prototype once.
        let mut occt: Vec<Vec<[f64; 3]>> = Vec::new();
        let mut protos = HashSet::new();
        for &part in parts {
            let placed = &topo.parts[part];
            if !protos.insert(placed.prototype) {
                continue;
            }
            let proto = &topo.prototypes[placed.prototype];
            let on_face = face_edges(proto);
            let back = invert(&placed.transform);
            let e = &scene.parts[part].edges;
            let mut at = 0;
            for (&id, &len) in e.ids.iter().zip(&e.lens) {
                let pts = &e.points[at * 3..(at + len as usize) * 3];
                at += len as usize;
                let analytic = matches!(
                    proto.edges[id as usize].curve,
                    Curve::Line | Curve::Circle { .. } | Curve::Ellipse { .. }
                );
                if analytic && on_face[id as usize] {
                    occt.push(
                        pts.chunks_exact(3)
                            .map(|v| apply(&back, [0, 1, 2].map(|i| f64::from(v[i]))))
                            .collect(),
                    );
                }
            }
        }
        if edges.is_empty() {
            // Its geometry is in other files, which the graph doesn't read (#92).
            all.external += 1;
            continue;
        }
        all.products += 1;
        all.occt += occt.len();
        for (_, r) in rust.routed() {
            *all.routed.entry(r.to_string()).or_default() += 1;
        }
        let rust: Vec<(Node, Vec<[f64; 3]>)> = edges
            .iter()
            .filter_map(|&e| Some((e, rust.get(e)?.ok()?.points().collect())))
            .collect();
        all.rust += rust.len();
        // OCCT's f32 coordinates: 1e-6 of the largest coordinate as slack.
        let slack =
            |pts: &[[f64; 3]]| 1e-6 * pts.iter().flatten().fold(1f64, |m, x| m.max(x.abs()));
        let (mut used, mut left) = (vec![false; occt.len()], Vec::new());
        for (e, pts) in &rust {
            let (a, b) = (pts[0], pts[pts.len() - 1]);
            let mut best: Option<(usize, [f64; 2])> = None;
            for (i, o) in occt.iter().enumerate() {
                let (oa, ob) = (o[0], o[o.len() - 1]);
                let ends = (dist(a, oa).max(dist(b, ob))).min(dist(a, ob).max(dist(b, oa)));
                if ends > d + slack(o) {
                    continue;
                }
                let gap = apart(pts, o);
                let better = best.is_none_or(|(_, w)| gap[0].max(gap[1]) < w[0].max(w[1]));
                if gap[0] <= d + slack(o) && gap[1] <= 2. * d + slack(o) && better {
                    best = Some((i, gap));
                }
            }
            match best {
                Some((i, gap)) => {
                    used[i] = true;
                    all.joined += 1;
                    all.worst[0] = all.worst[0].max(gap[0] / d);
                    all.worst[1] = all.worst[1].max(gap[1] / d);
                }
                None => left.push((*e, pts)),
            }
        }
        // The rest, point by point against the other side's every polyline:
        // OCCT's healing splits some closed edges in two.
        let occt_all: Vec<&[[f64; 3]]> = occt.iter().map(Vec::as_slice).collect();
        let rust_all: Vec<&[[f64; 3]]> = rust.iter().map(|(_, p)| p.as_slice()).collect();
        for (e, pts) in left {
            all.by_points += 1;
            match farthest(pts, &occt_all) {
                Some(gap) if gap <= d + slack(pts) => all.worst[0] = all.worst[0].max(gap / d),
                gap => all.unmatched.push(format!(
                    "{file} product {p}: Rust edge #{} ({} points) lies {gap:?} mm from OCCT's",
                    g.id(e).0,
                    pts.len()
                )),
            }
        }
        for (o, _) in occt.iter().zip(&used).filter(|(_, u)| !**u) {
            all.by_points += 1;
            match farthest(o, &rust_all) {
                Some(gap) if gap <= 2. * d + slack(o) => all.worst[1] = all.worst[1].max(gap / d),
                gap => all.unmatched.push(format!(
                    "{file} product {p}: OCCT edge {:?} → {:?} ({} points) lies {gap:?} mm from Rust's",
                    o[0],
                    o[o.len() - 1],
                    o.len()
                )),
            }
        }
    }
    Ok(())
}

/// The largest distance of a point of `pts` from the nearest of `lines`;
/// `None` when there are none.
fn farthest(pts: &[[f64; 3]], lines: &[&[[f64; 3]]]) -> Option<f64> {
    let boxes: Vec<([f64; 3], [f64; 3])> = lines
        .iter()
        .map(|l| {
            l.iter().fold(
                ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]),
                |(lo, hi), q| {
                    (
                        [0, 1, 2].map(|i| lo[i].min(q[i])),
                        [0, 1, 2].map(|i| hi[i].max(q[i])),
                    )
                },
            )
        })
        .collect();
    let mut worst = 0f64;
    for &q in pts {
        // Nearest box first, then only lines whose box is nearer than the best.
        let gap = |b: &([f64; 3], [f64; 3])| {
            let o = [0, 1, 2].map(|i| (b.0[i] - q[i]).max(q[i] - b.1[i]).max(0.));
            dot(o, o).sqrt()
        };
        let mut best = f64::INFINITY;
        let mut order: Vec<usize> = (0..lines.len()).collect();
        order.sort_by(|&a, &b| gap(&boxes[a]).total_cmp(&gap(&boxes[b])));
        for k in order {
            if gap(&boxes[k]) >= best {
                break;
            }
            best = best.min(to_polyline(q, lines[k]));
        }
        worst = worst.max(best);
    }
    worst.is_finite().then_some(worst)
}

fn run(files: &[PathBuf]) -> Tally {
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
    eprintln!(
        "mesh::edges: {} files, {} products ({} with external geometry skipped); {} Rust polylines \
         against {} OCCT edges, {} joined one to one, {} polylines compared point by point; \
         worst Rust → OCCT {:.3}·d, OCCT → Rust {:.3}·d; {} unmatched; routed {:?}",
        files.len(),
        all.products,
        all.external,
        all.rust,
        all.occt,
        all.joined,
        all.by_points,
        all.worst[0],
        all.worst[1],
        all.unmatched.len(),
        all.routed
    );
    assert!(
        all.unmatched.is_empty(),
        "{} unmatched:\n{}",
        all.unmatched.len(),
        all.unmatched.join("\n")
    );
    assert!(all.rust > 0, "no edge compared");
    all
}

/// A plate bounded by a full ellipse (B/A = 0.2) and a hole made of two
/// circle arcs, one of them `same_sense = .F.`: closed edges, and arcs
/// through the seam.
fn ellipse_plate() -> PathBuf {
    let mut f = F::new();
    let ax = f.ax([0., 0., 0.]);
    let el = f.add(&format!("ELLIPSE('',#{ax},10.,2.)"));
    let p = f.pt([10., 0., 0.]);
    let v = f.add(&format!("VERTEX_POINT('',#{p})"));
    let e = f.add(&format!("EDGE_CURVE('',#{v},#{v},#{el},.T.)"));
    let oe = f.add(&format!("ORIENTED_EDGE('',*,*,#{e},.T.)"));
    let outer_loop = f.add(&format!("EDGE_LOOP('',(#{oe}))"));
    let outer = f.add(&format!("FACE_OUTER_BOUND('',#{outer_loop},.T.)"));
    let hole_ax = f.ax([5., 0., 0.]);
    let c = f.add(&format!("CIRCLE('',#{hole_ax},1.)"));
    let (a, b) = (f.pt([5., 1., 0.]), f.pt([5., -1., 0.]));
    let (va, vb) = (
        f.add(&format!("VERTEX_POINT('',#{a})")),
        f.add(&format!("VERTEX_POINT('',#{b})")),
    );
    // Through x < 5 (the seam side at t = π) against the circle, and back
    // through x > 5 (t = 0) with it.
    let left = f.add(&format!("EDGE_CURVE('',#{vb},#{va},#{c},.F.)"));
    let right = f.add(&format!("EDGE_CURVE('',#{vb},#{va},#{c},.T.)"));
    let o1 = f.add(&format!("ORIENTED_EDGE('',*,*,#{left},.F.)"));
    let o2 = f.add(&format!("ORIENTED_EDGE('',*,*,#{right},.T.)"));
    let hole_loop = f.add(&format!("EDGE_LOOP('',(#{o1},#{o2}))"));
    let hole = f.add(&format!("FACE_BOUND('',#{hole_loop},.F.)"));
    let plane = f.add(&format!("PLANE('',#{ax})"));
    let face = f.face(&[outer, hole], plane);
    let item = f.item(&[face], false);
    part_file("ellipse-plate.step", 1e-7, &item, &f.out)
}

#[test]
fn tests_data_edges_match_occts() {
    // `multifile/bracket.step` is left out: its parts' geometry is in the
    // other files, which the kernel reads and the graph doesn't (#92).
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let mut files = [
        "assembly.step",
        "no-pcurves.step",
        "multifile/plate.stp",
        "multifile/pin.stp",
    ]
    .map(|f| data.join(f))
    .to_vec();
    files.push(ellipse_plate());
    let mut f = F::new();
    let shaft = f.shaft([1., 2., 3.], 4., 20., true);
    let item = f.item(&shaft, true);
    files.push(part_file("shaft.step", 1e-7, &item, &f.out));
    let all = run(&files);
    assert!(all.routed.is_empty(), "{:?}", all.routed);
}

#[test]
fn corpus_edges_match_occts() {
    if let Some(files) = corpus_files("mesh_edges") {
        run(&files);
    }
}
