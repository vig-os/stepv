//! `mesh::edges` (ADR-0001 §2.6, M1a/M1b) against OCCT's edge polylines, the
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
//! circle, ellipse or B-spline edge that bounds a face must match a Rust
//! polyline: 0 unmatched either way. The two directions' largest distances
//! are reported in units of d.
//!
//! **Every file is compared, or the gate fails** (#200, #203): a file whose
//! check fails is counted with its reason, and only the multi-file
//! assemblies' top files, whose parts are in other files, are skipped.
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
    /// The Rust polylines over B-spline curves, and their points.
    bspline: usize,
    bspline_points: usize,
    /// Rust edges without a polyline, by reason.
    routed: BTreeMap<String, usize>,
    /// OCCT edges not compared, by why.
    occt_skipped: BTreeMap<&'static str, usize>,
    /// Files that couldn't be compared, with why.
    not_compared: Vec<String>,
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
        // OCCT's line, conic and B-spline face edges, in product mm, each
        // prototype once.
        let mut occt: Vec<Vec<[f64; 3]>> = Vec::new();
        // Per OCCT edge: a B-spline seam (twice in one face).
        let mut seams: Vec<bool> = Vec::new();
        let mut protos = HashSet::new();
        for &part in parts {
            let placed = &topo.parts[part];
            if !protos.insert(placed.prototype) {
                continue;
            }
            let proto = &topo.prototypes[placed.prototype];
            let on_face = face_edges(proto);
            let seam = seam_edges(proto);
            let back = invert(&placed.transform);
            let e = &scene.parts[part].edges;
            let mut at = 0;
            for (&id, &len) in e.ids.iter().zip(&e.lens) {
                let pts = &e.points[at * 3..(at + len as usize) * 3];
                at += len as usize;
                let meshed = matches!(
                    proto.edges[id as usize].curve,
                    Curve::Line | Curve::Circle { .. } | Curve::Ellipse { .. } | Curve::Bspline
                );
                if meshed && on_face[id as usize] {
                    let bspline = proto.edges[id as usize].curve == Curve::Bspline;
                    seams.push(bspline && seam[id as usize]);
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
        // The vertex points of the edges Rust routes, in product mm.
        let mut routed_ends = Vec::new();
        for (e, r) in rust.routed() {
            *all.routed.entry(r.to_string()).or_default() += 1;
            routed_ends.extend(ends_of(&g, e, me.mm_per_unit));
        }
        let rust: Vec<(Node, Vec<[f64; 3]>)> = edges
            .iter()
            .filter_map(|&e| Some((e, rust.get(e)?.ok()?.points().collect())))
            .collect();
        all.rust += rust.len();
        for (_, pts) in rust.iter().filter(|(e, _)| is_bspline(&g, *e)) {
            all.bspline += 1;
            all.bspline_points += pts.len();
        }
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
        let rust_ends: Vec<[[f64; 3]; 2]> =
            rust.iter().map(|(_, p)| [p[0], p[p.len() - 1]]).collect();
        for ((o, _), &seam) in occt
            .iter()
            .zip(&used)
            .zip(&seams)
            .filter(|((_, u), _)| !**u)
        {
            let ends = Ends {
                rust: &rust_ends,
                routed: &routed_ends,
            };
            match occt_left(o, seam, &rust_all, ends, d, slack(o)) {
                Left::Near(gap) => {
                    all.by_points += 1;
                    all.worst[1] = all.worst[1].max(gap / d);
                }
                Left::Skipped(why) => *all.occt_skipped.entry(why).or_default() += 1,
                Left::Far(gap) => {
                    all.by_points += 1;
                    all.unmatched.push(format!(
                        "{file} product {p}: OCCT edge {:?} → {:?} ({} points) lies {gap:?} mm from Rust's",
                        o[0],
                        o[o.len() - 1],
                        o.len()
                    ));
                }
            }
        }
    }
    Ok(())
}

/// The end points of Rust's polylines and of the edges Rust routes, in
/// product mm.
#[derive(Clone, Copy)]
struct Ends<'a> {
    rust: &'a [[[f64; 3]; 2]],
    routed: &'a [[[f64; 3]; 2]],
}

/// What became of an OCCT edge no Rust polyline matched one to one.
#[derive(Debug, PartialEq)]
enum Left {
    /// Within 2·d of Rust's polylines, point by point.
    Near(f64),
    /// Counted, not compared.
    Skipped(&'static str),
    /// Unmatched: the gate fails.
    Far(Option<f64>),
}

/// The OCCT→Rust verdict on such an edge `o`, at deflection `d` with
/// OCCT's f32 `slack`. Off Rust's polylines, it is skipped only when it provably has no
/// Rust counterpart: OCCT's healing closes a periodic face bounded by two
/// loops with a seam no `EDGE_CURVE` names, so a `seam` whose ends are no
/// Rust polyline's ends; and an edge whose ends are those of an edge Rust
/// routes, which has no polyline.
fn occt_left(
    o: &[[f64; 3]],
    seam: bool,
    rust: &[&[[f64; 3]]],
    ends: Ends<'_>,
    d: f64,
    slack: f64,
) -> Left {
    let tol = d + slack;
    let gap = farthest(o, rust);
    if let Some(gap) = gap.filter(|&g| g <= 2. * d + slack) {
        return Left::Near(gap);
    }
    let (oa, ob) = (o[0], o[o.len() - 1]);
    let near = |&[a, b]: &[[f64; 3]; 2]| {
        (dist(a, oa) <= tol && dist(b, ob) <= tol) || (dist(a, ob) <= tol && dist(b, oa) <= tol)
    };
    if seam && !ends.rust.iter().any(near) {
        Left::Skipped("OCCT B-spline seam")
    } else if ends.routed.iter().any(near) {
        Left::Skipped("routed in Rust")
    } else {
        Left::Far(gap)
    }
}

/// An `EDGE_CURVE`'s two vertex points at `s` mm per unit, as written.
fn ends_of(g: &Graph, e: Node, s: f64) -> Option<[[f64; 3]; 2]> {
    let point = |v: Node| -> Option<[f64; 3]> {
        let p = *g.refs(v).first()?;
        let a = g.args(p).ok()?;
        let graph::Value::List(c) = a.get(1)? else {
            return None;
        };
        let num = |x: &graph::Value<'_>| match x {
            graph::Value::Real(r) => Some(*r * s),
            graph::Value::Int(i) => Some(*i as f64 * s),
            _ => None,
        };
        Some([num(c.first()?)?, num(c.get(1)?)?, num(c.get(2)?)?])
    };
    let r = g.refs(e);
    Some([point(*r.first()?)?, point(*r.get(1)?)?])
}

/// Which of a prototype's edges are seams: twice in one face.
fn seam_edges(proto: &topology::Prototype) -> Vec<bool> {
    let mut seam = vec![false; proto.edges.len()];
    for f in &proto.faces {
        let mut seen = HashSet::new();
        for &e in &f.edges {
            if !seen.insert(e) {
                seam[e] = true;
            }
        }
    }
    seam
}

/// Whether `e`'s curve, directly or under a `SURFACE_CURVE` or
/// `SEAM_CURVE`, is a B-spline.
fn is_bspline(g: &Graph, e: Node) -> bool {
    let mut c = g.refs(e).last().copied();
    while let Some(n) = c {
        if g.is_a(n, "B_SPLINE_CURVE_WITH_KNOTS") {
            return true;
        }
        if !(g.is_a(n, "SURFACE_CURVE") || g.is_a(n, "SEAM_CURVE")) {
            return false;
        }
        c = g.refs(n).first().copied();
    }
    false
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
            all.not_compared.push(format!("{}: {e}", f.display()));
        }
    }
    eprintln!(
        "mesh::edges: {} files, {} products ({} with external geometry skipped); {} Rust polylines \
         against {} OCCT edges, {} joined one to one, {} polylines compared point by point; \
         worst Rust → OCCT {:.3}·d, OCCT → Rust {:.3}·d; {} unmatched; {} B-spline polylines \
         ({} points); routed {:?}; OCCT edges skipped {:?}; {} not compared",
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
        all.bspline,
        all.bspline_points,
        all.routed,
        all.occt_skipped,
        all.not_compared.len()
    );
    assert!(
        all.not_compared.is_empty(),
        "{} files not compared:\n{}",
        all.not_compared.len(),
        all.not_compared.join("\n")
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

/// A plate bounded by a cubic B-spline with a simple interior knot, from
/// (0, 0) to (10, 0) through y > 0, and back by a rational semicircle in
/// the complex form (two quarters, a C0 knot between them) through y < 0.
fn bspline_plate() -> PathBuf {
    let mut f = F::new();
    let ax = f.ax([0., 0., 0.]);
    let cubic = [[0., 0.], [1., 4.], [4., 6.], [7., 2.], [10., 0.]];
    let half = [[10., 0.], [10., -5.], [5., -5.], [0., -5.], [0., 0.]];
    let refs = |f: &mut F, pts: &[[f64; 2]]| -> String {
        let r: Vec<String> = pts
            .iter()
            .map(|p| format!("#{}", f.pt([p[0], p[1], 0.])))
            .collect();
        r.join(",")
    };
    let c = refs(&mut f, &cubic);
    let cubic = f.add(&format!(
        "B_SPLINE_CURVE_WITH_KNOTS('',3,({c}),.UNSPECIFIED.,.F.,.F.,(4,1,4),(0.,0.4,1.),.UNSPECIFIED.)"
    ));
    let h = refs(&mut f, &half);
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let half = f.add(&format!(
        "(BOUNDED_CURVE() B_SPLINE_CURVE(2,({h}),.CIRCULAR_ARC.,.F.,.F.) \
         B_SPLINE_CURVE_WITH_KNOTS((3,2,3),(0.,1.,2.),.UNSPECIFIED.) CURVE() \
         GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_CURVE((1.,{w:?},1.,{w:?},1.)) \
         REPRESENTATION_ITEM(''))"
    ));
    let (a, b) = (f.pt([0., 0., 0.]), f.pt([10., 0., 0.]));
    let va = f.add(&format!("VERTEX_POINT('',#{a})"));
    let vb = f.add(&format!("VERTEX_POINT('',#{b})"));
    let e1 = f.add(&format!("EDGE_CURVE('',#{va},#{vb},#{cubic},.T.)"));
    // Against the curve: its direction runs from (10, 0) to (0, 0).
    let e2 = f.add(&format!("EDGE_CURVE('',#{va},#{vb},#{half},.F.)"));
    let o1 = f.add(&format!("ORIENTED_EDGE('',*,*,#{e1},.T.)"));
    let o2 = f.add(&format!("ORIENTED_EDGE('',*,*,#{e2},.F.)"));
    let lp = f.add(&format!("EDGE_LOOP('',(#{o1},#{o2}))"));
    let outer = f.add(&format!("FACE_OUTER_BOUND('',#{lp},.F.)"));
    let plane = f.add(&format!("PLANE('',#{ax})"));
    let face = f.face(&[outer], plane);
    let item = f.item(&[face], false);
    part_file("bspline-plate.step", 1e-7, &item, &f.out)
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
    files.push(bspline_plate());
    let mut f = F::new();
    let shaft = f.shaft([1., 2., 3.], 4., 20., true);
    let item = f.item(&shaft, true);
    files.push(part_file("shaft.step", 1e-7, &item, &f.out));
    let all = run(&files);
    assert!(all.routed.is_empty(), "{:?}", all.routed);
    assert!(all.bspline >= 2, "the B-spline plate's edges are compared");
}

#[test]
fn corpus_edges_match_occts() {
    if let Some(files) = corpus_files("mesh_edges") {
        run(&files);
    }
}

#[test]
fn a_seam_whose_ends_a_rust_polyline_shares_is_not_exempt() {
    // OCCT's closed seam loop at V = (1, 0, 0), and a Rust closed edge at V
    // over only part of it: every Rust point lies on OCCT's loop, so the
    // pair passes Rust → OCCT, but most of OCCT's loop is far from Rust's.
    let ring = |t: f64| [t.cos(), t.sin(), 0.];
    let occt: Vec<[f64; 3]> = (0..=64)
        .map(|i| ring(f64::from(i) * std::f64::consts::TAU / 64.))
        .collect();
    let rust = vec![ring(0.), ring(0.3), ring(0.6), ring(0.)];
    let ends = Ends {
        rust: &[[rust[0], rust[3]]],
        routed: &[],
    };
    let left = occt_left(&occt, true, &[&rust], ends, 1e-3, 0.);
    assert!(matches!(left, Left::Far(Some(g)) if g > 0.5), "{left:?}");
    // A healing seam, its ends no Rust polyline's: counted.
    let none = Ends {
        rust: &[],
        routed: &[],
    };
    assert_eq!(
        occt_left(&occt, true, &[&rust], none, 1e-3, 0.),
        Left::Skipped("OCCT B-spline seam")
    );
    // An edge Rust routes: counted.
    let routed = Ends {
        rust: &[],
        routed: &[[ring(0.), ring(0.)]],
    };
    assert_eq!(
        occt_left(&occt, false, &[&rust], routed, 1e-3, 0.),
        Left::Skipped("routed in Rust")
    );
}
