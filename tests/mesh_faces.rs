//! `mesh::plane` (ADR-0001 §2.6, M2) and `mesh::analytic` (M3a) against
//! OCCT, the golden reference: every planar face's Rust mesh area equals the
//! kernel's `--topology` face area (`BRepGProp` on the exact face) within
//! 1e-6 relative, and every cylinder's and cone's within the deflection's
//! bound.
//!
//! **Cylinders and cones.** Their mesh is inscribed, its triangles' normals
//! within a of the surface's, so it projects onto the surface with a
//! Jacobian of at least cos a: its area is the exact face's within
//! `A·(1 − cos a)`, plus `d·L` for the strip between the chords of its
//! curved edges (length L) and their curves, plus the vertex gaps below.
//!
//! **The exact boundary.** The mesh is bounded by chords of the edge
//! polylines, OCCT's area by the exact curves. So the mesh area is compared
//! after adding, chord by chord, the exact area between each chord and its
//! curve: a circle's segment `r²/2·(φ − sin φ)` and an ellipse's
//! `A·B/2·(Δt − sin Δt)`, positive when the face lies on the curve's
//! centre side of the chord. A line adds nothing, so a face bounded by
//! lines alone compares its mesh area as is. What can't be exact is bounded,
//! and such a face passes within 1e-6 relative plus its bound:
//! - a B-spline chord lies within d of its curve, so it adds at most
//!   `d·|chord|` either way;
//! - a vertex g off one of its lines or conics (a file may write them apart,
//!   within its uncertainty; Rust's face follows the vertex, OCCT's the
//!   curve) moves every edge it ends, by at most `g·(L + 2g)` each. A vertex
//!   off beyond the uncertainty leaves the face undefined: it is counted,
//!   not compared, and Rust should route it (#207).
//!
//! **The context** is [`Context::of`]'s: the declared uncertainty, floored at
//! 1e-7 mm (#210). Faces meshed or routed under a floored context are
//! counted as `uncertainty:floored`.
//!
//! **The face join** (G7's, `graph_colors`): the kernel reports a part's
//! faces in transfer order, with no STEP ids until M5, and
//! [`stepv::graph::Colors::parts`] lists a product's faces in that order. A
//! product's kernel parts are those of every leaf it is, one per prototype.
//! A part's faces are joined by position, and the join is checked, never
//! assumed: the counts agree, each pair is planar on both sides or on
//! neither, and every kernel mesh vertex of a face lies in the graph's
//! conservative box of its partner (G4). Where it fails (XCAF splits or
//! healing reorders), each Rust area must fit an unused OCCT face's of its
//! kind,
//! the tightest bounds first. A file the kernel or the graph can't read, a
//! face whose area fits no OCCT face, or a planar Rust normal not parallel
//! to OCCT's fails the gate. Faces Rust routes are counted by reason.
//!
//! `STEPV_CORPUS` (paths, `:`-separated, files or directories) adds the
//! local corpus. The perforated plate (ABC `00000046`, named by
//! `STEPV_PLATE`) is meshed apart, Rust only, with its time and peak RSS
//! reported. `STEPV_TIMING` names a file whose planar, cylinder and cone
//! faces are all meshed in Rust, timed, with the share meshed.

mod corpus;
mod stepgen;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use corpus::{apply, corpus_files, edges_of, invert, kernel_summary};
use stepgen::{F, part_file};
use stepv::Deflection;
use stepv::graph::{self, Bound, Graph, Node, Value};
use stepv::mesh::{Context, FaceMesh, Polylines, Tolerance, analytic, plane};
use stepv::topology::{self, Surface};

#[derive(Default, Debug)]
struct Tally {
    products: usize,
    /// Multi-file products, whose geometry is in other files.
    external: usize,
    /// Planar faces compared exactly, and within a bound (B-spline chords,
    /// vertices off their curves).
    exact: usize,
    bounded: usize,
    /// Parts whose join failed, and the faces they matched by value.
    unjoined: usize,
    by_value: usize,
    /// The largest relative area difference over the compared faces.
    worst: f64,
    /// Faces whose OCCT normal is opposite to Rust's (shell orientation).
    opposite: usize,
    routed: BTreeMap<String, usize>,
    /// Faces meshed or routed under a floored context (#210).
    floored: usize,
    /// The graph's faces in the compared parts (kept in the planar tally).
    faces: usize,
    /// Meshed faces with a vertex off its curve beyond the uncertainty, not
    /// compared (#207).
    inconsistent: usize,
    failing: Vec<String>,
    not_compared: Vec<String>,
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// What the golden compares: faces on planes, and on cylinders or cones.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Plane = 0,
    Analytic = 1,
}

/// `face`'s [`Kind`], if it is an `ADVANCED_FACE` of one.
fn kind(g: &Graph, face: Node) -> Option<Kind> {
    if !g.is_a(face, "ADVANCED_FACE") {
        return None;
    }
    let s = g.args(face).ok().and_then(|a| at(a.get(2)))?;
    if g.is_a(s, "PLANE") {
        Some(Kind::Plane)
    } else if g.is_a(s, "CYLINDRICAL_SURFACE") || g.is_a(s, "CONICAL_SURFACE") {
        Some(Kind::Analytic)
    } else {
        None
    }
}

/// An OCCT face's [`Kind`].
fn occt_kind(f: &topology::Face) -> Option<Kind> {
    match f.surface {
        Surface::Plane { .. } => Some(Kind::Plane),
        Surface::Cylinder { .. } | Surface::Cone { .. } => Some(Kind::Analytic),
        _ => None,
    }
}

/// The sum of the mesh's triangle areas.
fn area(m: &FaceMesh) -> f64 {
    m.triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| m.point(i as usize));
            norm(cross(sub(b, a), sub(c, a))) / 2.
        })
        .sum()
}

fn refs(v: Option<&Value<'_>>) -> Vec<Node> {
    match v {
        Some(Value::List(l)) => l
            .iter()
            .filter_map(|x| match x {
                Value::Ref(n) => Some(*n),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn at(v: Option<&Value<'_>>) -> Option<Node> {
    match v? {
        Value::Ref(n) => Some(*n),
        _ => None,
    }
}

fn real(v: Option<&Value<'_>>) -> Option<f64> {
    match v? {
        Value::Real(x) => Some(*x),
        Value::Int(i) => Some(*i as f64),
        Value::Typed(_, v) => real(Some(v)),
        _ => None,
    }
}

/// A `CARTESIAN_POINT` or `DIRECTION`'s three coordinates.
fn triple(g: &Graph, n: Node) -> Option<[f64; 3]> {
    let a = g.args(n).ok()?;
    let Some(Value::List(c)) = a.get(1) else {
        return None;
    };
    Some([real(c.first())?, real(c.get(1))?, real(c.get(2))?])
}

/// A circle or an ellipse in mm: `c + a·cos t·x + b·sin t·(z × x)`.
#[derive(Clone, Copy)]
struct Conic {
    c: [f64; 3],
    x: [f64; 3],
    z: [f64; 3],
    a: f64,
    b: f64,
}

/// An edge's conic, at `s` mm per unit; `None` for anything else.
fn conic(g: &Graph, edge: Node, s: f64) -> Option<Conic> {
    let mut c = at(g.args(edge).ok()?.get(3))?;
    while g.is_a(c, "SURFACE_CURVE") || g.is_a(c, "SEAM_CURVE") {
        c = at(g.args(c).ok()?.get(1))?;
    }
    let circle = g.is_a(c, "CIRCLE");
    if !circle && !g.is_a(c, "ELLIPSE") {
        return None;
    }
    let a = g.args(c).ok()?;
    let ax = g.args(at(a.get(1))?).ok()?;
    let o = triple(g, at(ax.get(1))?)?.map(|x| x * s);
    let z = triple(g, at(ax.get(2))?)?;
    let z = z.map(|v| v / norm(z));
    // A circle's x axis is any perpendicular: only angles between points
    // on it are used.
    let r = match at(ax.get(3)).and_then(|r| triple(g, r)) {
        Some(r) => r,
        None if circle => {
            let k = (0..3).min_by(|&i, &j| z[i].abs().total_cmp(&z[j].abs()))?;
            let mut e = [0.; 3];
            e[k] = 1.;
            e
        }
        None => return None,
    };
    let x = sub(r, z.map(|v| v * dot(r, z)));
    let x = x.map(|v| v / norm(x));
    let ra = real(a.get(2))? * s;
    let rb = if circle { ra } else { real(a.get(3))? * s };
    Some(Conic {
        c: o,
        x,
        z,
        a: ra,
        b: rb,
    })
}

/// The distances of `edge`'s two vertices from its line or conic, at `s`
/// mm per unit (0 on B-splines, which M1b checks), less f64's rounding of them (as
/// `mesh::plane` allows it), with the vertices: a file may write them
/// apart, within its uncertainty.
fn vertex_gaps(g: &Graph, edge: Node, s: f64) -> Option<[(Node, f64); 2]> {
    let e = g.args(edge).ok()?;
    let mut c = at(e.get(3))?;
    while g.is_a(c, "SURFACE_CURVE") || g.is_a(c, "SEAM_CURVE") {
        c = at(g.args(c).ok()?.get(1))?;
    }
    let gap = |k: usize| -> Option<(Node, f64)> {
        let v = at(e.get(k))?;
        let q = triple(g, at(g.args(v).ok()?.get(1))?)?.map(|x| x * s);
        let gap = if g.is_a(c, "LINE") {
            let a = g.args(c).ok()?;
            let o = triple(g, at(a.get(1))?)?.map(|x| x * s);
            let dir = triple(g, at(g.args(at(a.get(2))?).ok()?.get(1))?)?;
            let rounding = 8. * f64::EPSILON * (norm(q) + norm(o));
            (norm(cross(sub(q, o), dir)) / norm(dir) - rounding).max(0.)
        } else if let Some(k) = conic(g, edge, s) {
            // In the conic's frame; off an ellipse to first order, |F|/|∇F|
            // with F = (u/a)² + (v/b)² − 1, exact on a circle.
            let w = sub(q, k.c);
            let (u, v, h) = (dot(w, k.x), dot(w, cross(k.z, k.x)), dot(w, k.z));
            let f = (u / k.a).powi(2) + (v / k.b).powi(2) - 1.;
            let grad = (2. * u / (k.a * k.a)).hypot(2. * v / (k.b * k.b));
            let off = if k.a == k.b {
                u.hypot(v) - k.a
            } else {
                f / grad
            };
            let rounding = 8. * f64::EPSILON * (norm(q) + norm(k.c) + k.a.max(k.b));
            (h.hypot(off) - rounding).max(0.)
        } else {
            0.
        };
        Some((v, gap))
    };
    Some([gap(1)?, gap(2)?])
}

/// A face's area as compared with OCCT's (module docs).
struct Measured {
    /// The mesh area, plus on a plane the exact chord-to-curve areas.
    area: f64,
    /// The relative slack: 1e-6, plus 1 − cos a on a cylinder or cone.
    rel: f64,
    /// How far the exact area can be from `area`: B-spline chords, and
    /// vertices off their curves.
    bound: f64,
    /// The face's outward normal, with its shell's orientation unknown.
    normal: [f64; 3],
    /// The largest distance of a vertex from one of its curves.
    gap: f64,
}

/// `face`'s [`Measured`] area, from its mesh `m` of kind `k`; `None` when a
/// loop can't be read.
fn measure(
    g: &Graph,
    face: Node,
    k: Kind,
    m: &FaceMesh,
    p: &Polylines,
    s: f64,
    tol: Tolerance,
) -> Option<Measured> {
    let (d, a) = (tol.linear(), tol.angular());
    let n = m.normal(0);
    let (mut exact, mut bound) = (0., 0.);
    // How far each vertex is from any of its curves: moving it onto them
    // moves every edge it ends.
    let mut gaps: HashMap<Node, f64> = HashMap::new();
    for b in refs(g.args(face).ok()?.get(1)) {
        for oe in refs(g.args(at(g.args(b).ok()?.get(1))?).ok()?.get(1)) {
            let edge = at(g.args(oe).ok()?.get(3))?;
            for (v, gap) in vertex_gaps(g, edge, s)? {
                let e = gaps.entry(v).or_default();
                *e = e.max(gap);
            }
        }
    }
    for b in refs(g.args(face).ok()?.get(1)) {
        let a = g.args(b).ok()?;
        let sign = match a.get(2)? {
            Value::Enum("T") => 1.,
            _ => -1.,
        };
        for oe in refs(g.args(at(a.get(1))?).ok()?.get(1)) {
            let edge = at(g.args(oe).ok()?.get(3))?;
            let pts: Vec<[f64; 3]> = p.oriented(g, oe).ok()?.map(|(_, q)| q).collect();
            let length: f64 = pts.windows(2).map(|w| norm(sub(w[1], w[0]))).sum();
            let [(v1, _), (v2, _)] = vertex_gaps(g, edge, s)?;
            let gap = gaps[&v1].max(gaps[&v2]);
            bound += gap * (length + 2. * gap);
            let curve = conic(g, edge, s);
            let mut basis = at(g.args(edge).ok()?.get(3))?;
            while g.is_a(basis, "SURFACE_CURVE") || g.is_a(basis, "SEAM_CURVE") {
                basis = at(g.args(basis).ok()?.get(1))?;
            }
            for w in pts.windows(2) {
                let (u, v) = (w[0], w[1]);
                match curve {
                    _ if k == Kind::Analytic => {
                        if !g.is_a(basis, "LINE") {
                            bound += d * norm(sub(v, u));
                        }
                    }
                    Some(Conic {
                        c,
                        x,
                        z,
                        a: ra,
                        b: rb,
                    }) => {
                        let y = cross(z, x);
                        let t = |q: [f64; 3]| {
                            let r = sub(q, c);
                            (dot(r, y) / rb).atan2(dot(r, x) / ra)
                        };
                        let dt = (t(v) - t(u)).rem_euclid(std::f64::consts::TAU);
                        let dt = dt.min(std::f64::consts::TAU - dt);
                        let seg = ra * rb / 2. * (dt - dt.sin());
                        let left = dot(cross(sub(v, u), sub(c, u)), n) > 0.;
                        exact += sign * if left { seg } else { -seg };
                    }
                    None if g.is_a(basis, "LINE") => {}
                    None => bound += d * norm(sub(v, u)),
                }
            }
        }
    }
    Some(Measured {
        area: area(m) + exact,
        rel: if k == Kind::Plane {
            1e-6
        } else {
            1e-6 + (1. - a.cos())
        },
        bound,
        normal: n,
        gap: gaps.values().fold(0., |a, &b| f64::max(a, b)),
    })
}

impl Measured {
    /// Whether OCCT's `occt` area is this one's within its relative slack
    /// plus the bound.
    fn fits(&self, occt: f64) -> bool {
        (self.area - occt).abs() <= self.rel * occt + self.bound
    }

    /// Counts a match: exact when its bound is under a tenth of the gate's
    /// 1e-6 (rounding in the file's vertices); a cylinder's or cone's is
    /// always within its bound, the worst relative difference recorded.
    fn tally(&self, occt: f64, all: &mut Tally) {
        if self.rel > 1e-6 {
            all.bounded += 1;
            all.worst = all.worst.max((self.area - occt).abs() / occt);
        } else if self.bound > 1e-7 * occt {
            all.bounded += 1;
        } else {
            all.exact += 1;
            all.worst = all.worst.max((self.area - occt).abs() / occt);
        }
    }
}

/// A kernel part's faces' mesh vertices, in its product's coordinates.
fn face_points(part: &stepv::Part, placement: &[f64; 12]) -> Vec<Vec<[f64; 3]>> {
    let back = invert(placement);
    let mut points = vec![Vec::new(); part.faces.len()];
    for (t, &f) in part.mesh.face_ids.iter().enumerate() {
        for &v in &part.mesh.indices[t * 3..t * 3 + 3] {
            let x = &part.mesh.positions[v as usize * 3..v as usize * 3 + 3];
            points[f as usize].push(apply(&back, [x[0], x[1], x[2]].map(f64::from)));
        }
    }
    points
}

/// Whether `face` is an `ADVANCED_FACE` on a `PLANE`.
fn on_plane(g: &Graph, face: Node) -> bool {
    kind(g, face) == Some(Kind::Plane)
}

/// Why the position join of `graph`'s faces to a prototype's fails, if it
/// does (module docs).
fn join_fails(
    g: &Graph,
    b: &graph::Bounds,
    graph: &[Node],
    proto: &topology::Prototype,
    points: &[Vec<[f64; 3]>],
) -> Option<String> {
    if graph.len() != proto.faces.len() {
        return Some(format!("{} faces, OCCT {}", graph.len(), proto.faces.len()));
    }
    for (k, (&f, of)) in graph.iter().zip(&proto.faces).enumerate() {
        if kind(g, f) != occt_kind(of) {
            return Some(format!("face {k} #{}: its kind on one side", g.id(f).0));
        }
        if let Bound::Box(bb) = b.node(f) {
            let size = (0..3).map(|k| bb.max[k] - bb.min[k]).fold(0., f64::max);
            let out = points[k].iter().any(|q| {
                let slack = 1e-6 * (size + corpus::norm(*q)).max(1.);
                (0..3).any(|i| q[i] < bb.min[i] - slack || q[i] > bb.max[i] + slack)
            });
            if out {
                return Some(format!("face {k} #{}: outside its box", g.id(f).0));
            }
        }
    }
    None
}

/// One product's faces, meshed: each graph face on a plane, cylinder or
/// cone with its [`Measured`] area, or routed (counted in `all`).
struct Rust<'a> {
    g: &'a Graph,
    polylines: Polylines,
    ctx: Context,
    tol: Tolerance,
}

impl Rust<'_> {
    fn measure(&self, f: Node, k: Kind, here: &str, all: &mut Tally) -> Option<Measured> {
        all.floored += usize::from(self.ctx.floored);
        let mesh = match k {
            Kind::Plane => plane::mesh(self.g, f, &self.polylines, self.ctx, false),
            Kind::Analytic => analytic::mesh(self.g, f, &self.polylines, self.ctx, self.tol, false),
        };
        let mesh = match mesh {
            Ok(m) => m,
            Err(r) => {
                *all.routed.entry(r.to_string()).or_default() += 1;
                return None;
            }
        };
        let s = self.ctx.mm_per_unit;
        let id = self.g.id(f).0;
        let Some(m) = measure(self.g, f, k, &mesh, &self.polylines, s, self.tol) else {
            all.failing
                .push(format!("{here} face #{id}: boundary unread"));
            return None;
        };
        // A vertex off its curve beyond the uncertainty: the file defines no
        // exact face, and Rust should route it (#207).
        if m.gap > self.ctx.uncertainty {
            all.inconsistent += 1;
            return None;
        }
        Some(m)
    }

    /// Compares `faces` with OCCT's `occt` faces by value, where the join by
    /// position fails: each Rust area must fit an OCCT plane face's, each
    /// OCCT face used once, the tightest bounds matched first so a loose one
    /// can't take another face's partner.
    fn by_value(&self, faces: &[Node], occt: &[&topology::Face], here: &str, all: &mut [Tally; 2]) {
        for k in [Kind::Plane, Kind::Analytic] {
            self.by_value_of(k, faces, occt, here, &mut all[k as usize]);
        }
    }

    fn by_value_of(
        &self,
        k: Kind,
        faces: &[Node],
        occt: &[&topology::Face],
        here: &str,
        all: &mut Tally,
    ) {
        let mut left: Vec<f64> = occt
            .iter()
            .filter(|f| occt_kind(f) == Some(k))
            .map(|f| f.area)
            .collect();
        let mut mine: Vec<(Node, Measured)> = faces
            .iter()
            .filter(|&&f| kind(self.g, f) == Some(k))
            .filter_map(|&f| Some((f, self.measure(f, k, here, all)?)))
            .collect();
        mine.sort_by(|a, b| a.1.bound.total_cmp(&b.1.bound));
        for (f, m) in mine {
            let gap = |k: usize| (left[k] - m.area).abs();
            let best = (0..left.len())
                .filter(|&k| m.fits(left[k]))
                .min_by(|&i, &j| gap(i).total_cmp(&gap(j)));
            match best {
                Some(k) => {
                    m.tally(left.swap_remove(k), all);
                    all.by_value += 1;
                }
                None => all.failing.push(format!(
                    "{here} face #{}: area {} fits no OCCT {k:?} face (unjoined)",
                    self.g.id(f).0,
                    m.area
                )),
            }
        }
    }
}

fn check(input: &Path, all: &mut [Tally; 2]) -> Result<(), String> {
    let g = Graph::open(input).map_err(|e| format!("graph: {e}"))?;
    let s = graph::structure(&g).map_err(|e| format!("structure: {e}"))?;
    let b = graph::bounds(&g, &s);
    let colors = graph::colors(&g);
    let (scene, topo, summary) = kernel_summary(input)?;
    if scene.parts.len() != topo.parts.len() {
        return Err("mesh and topology parts differ".into());
    }
    let d = summary.linear_abs;
    let tol = Tolerance::from_mm(d, Deflection::PREVIEW.angular_deg.to_radians())
        .ok_or("no deflection")?;
    let multi_file = g.of_type("DOCUMENT_FILE").next().is_some();
    let file = input.file_name().unwrap().to_string_lossy().into_owned();
    // Each product once, with the kernel parts of every leaf it is, one per
    // prototype: instances repeat a prototype, and XCAF can show one graph
    // product as several leaves.
    let mut order: Vec<(usize, usize)> = Vec::new();
    let mut kernel_parts: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut seen = HashSet::new();
    for (leaf, parts) in topology::parts_per_leaf(&topo.tree).iter().enumerate() {
        let path = s.leaf_path(leaf as u64).ok_or("no leaf path")?;
        let p = path.0.last().unwrap().0;
        let mine = kernel_parts.entry(p).or_insert_with(|| {
            order.push((p, leaf));
            Vec::new()
        });
        for &i in parts {
            if seen.insert((p, topo.parts[i].prototype)) {
                mine.push(i);
            }
        }
    }
    for &(p, leaf) in &order {
        let parts = &kernel_parts[&p];
        let mine = colors.parts(&g, &s, p);
        if multi_file && mine.iter().all(|m| m.faces.is_empty()) {
            all[0].external += 1;
            continue;
        }
        all[0].products += 1;
        all[0].faces += mine.iter().map(|m| m.faces.len()).sum::<usize>();
        let me = &s.products[p];
        let edges = edges_of(&g, &me.bodies);
        let ctx = me.bodies.first().map_or_else(
            || Context::new(me.mm_per_unit, 1., None),
            |&r| Context::of(&g, r),
        );
        let rust = Rust {
            g: &g,
            polylines: Polylines::build(&g, edges.iter().copied(), me.mm_per_unit, tol),
            ctx,
            tol,
        };
        let here = format!("{file} leaf {leaf}");
        let proto = |i: usize| &topo.prototypes[topo.parts[i].prototype];
        if mine.len() != parts.len() {
            // XCAF split the product unlike the graph: the leaf by value.
            all[0].unjoined += 1;
            let faces: Vec<Node> = mine
                .iter()
                .flat_map(|m| m.faces.iter().map(|f| f.face))
                .collect();
            let occt: Vec<_> = parts.iter().flat_map(|&i| &proto(i).faces).collect();
            rust.by_value(&faces, &occt, &here, all);
            continue;
        }
        for (&i, m) in parts.iter().zip(&mine) {
            let faces: Vec<Node> = m.faces.iter().map(|f| f.face).collect();
            let points = face_points(&scene.parts[i], &topo.parts[i].transform);
            if join_fails(&g, &b, &faces, proto(i), &points).is_some() {
                all[0].unjoined += 1;
                let occt: Vec<_> = proto(i).faces.iter().collect();
                rust.by_value(&faces, &occt, &here, all);
                continue;
            }
            for (&f, of) in faces.iter().zip(&proto(i).faces) {
                let Some(k) = occt_kind(of) else {
                    continue;
                };
                let all = &mut all[k as usize];
                let Some(m) = rust.measure(f, k, &here, all) else {
                    continue;
                };
                let id = g.id(f).0;
                if !m.fits(of.area) {
                    all.failing.push(format!(
                        "{here} face #{id}: area {}, OCCT {} (rel {:.2e}, bound {:.2e})",
                        m.area,
                        of.area,
                        (m.area - of.area).abs() / of.area,
                        m.bound
                    ));
                    continue;
                }
                m.tally(of.area, all);
                let Surface::Plane { normal, .. } = of.surface else {
                    continue;
                };
                // OCCT's normal is outward in the shell: Rust's ± it.
                let c = dot(m.normal, normal);
                if c.abs() < 1. - 1e-9 {
                    let n = m.normal;
                    all.failing
                        .push(format!("{here} face #{id}: normal {n:?}, OCCT {normal:?}"));
                } else if c < 0. {
                    all.opposite += 1;
                }
            }
        }
    }
    Ok(())
}

fn run(files: &[PathBuf]) -> [Tally; 2] {
    let mut all = [Tally::default(), Tally::default()];
    for f in files {
        let stem = f.file_stem().unwrap_or_default();
        // The multi-file assemblies' top files: their parts live in other files.
        if stem == "s1-c5-214" || stem == "s1_pe_214" {
            continue;
        }
        if let Err(e) = check(f, &mut all) {
            all[0].not_compared.push(format!("{}: {e}", f.display()));
        }
    }
    let t = &all[0];
    eprintln!(
        "mesh faces: {} files, {} products ({} with external geometry skipped), {} faces; {} \
         matched by value in {} unjoined parts; {} files not compared",
        files.len(),
        t.products,
        t.external,
        t.faces,
        all[0].by_value + all[1].by_value,
        t.unjoined,
        t.not_compared.len()
    );
    for (name, t) in ["planar", "cylinder and cone"].iter().zip(&all) {
        eprintln!(
            "mesh faces, {name}: {} matched exactly, {} within their bound (worst {:.2e} \
             relative); {} routed {:?}; {} with a vertex off its curve beyond the uncertainty, \
             not compared (#207); {} under a floored uncertainty (uncertainty:floored, #210); \
             {} failing; {} normals opposite to OCCT's",
            t.exact,
            t.bounded,
            t.worst,
            t.routed.values().sum::<usize>(),
            t.routed,
            t.inconsistent,
            t.floored,
            t.failing.len(),
            t.opposite,
        );
    }
    assert!(
        all[0].not_compared.is_empty(),
        "{} files not compared:\n{}",
        all[0].not_compared.len(),
        all[0].not_compared.join("\n")
    );
    let failing: Vec<&String> = all.iter().flat_map(|t| &t.failing).collect();
    assert!(
        failing.is_empty(),
        "{} failing:\n{}",
        failing.len(),
        failing
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(all[0].exact + all[0].bounded > 0, "no planar face compared");
    all
}

/// A shaft's band: a cylinder of radius 2 from z = 0 to 10, bounded by two
/// circles and no seam edge (ABC's form).
fn shaft() -> PathBuf {
    let mut f = F::new();
    let faces = f.shaft([0.; 3], 2., 10., false);
    let item = f.item(&faces, false);
    part_file("shaft.step", 1e-7, &item, &f.out)
}

/// A 20 × 12 mm plate with 3 × 2 circular holes of radius 1.5: lines
/// counter-clockwise outside, circles walked clockwise inside.
fn perforated_plate() -> PathBuf {
    let mut f = F::new();
    let corners = [[0., 0., 0.], [20., 0., 0.], [20., 12., 0.], [0., 12., 0.]];
    let v: Vec<u32> = corners
        .iter()
        .map(|&c| {
            let p = f.pt(c);
            f.add(&format!("VERTEX_POINT('',#{p})"))
        })
        .collect();
    let mut oes = Vec::new();
    for k in 0..4 {
        let (a, b) = (corners[k], corners[(k + 1) % 4]);
        let dir = sub(b, a).map(|x| x / norm(sub(b, a)));
        let p = f.pt(a);
        let dn = f.add(&format!(
            "DIRECTION('',({:?},{:?},{:?}))",
            dir[0], dir[1], dir[2]
        ));
        let vec = f.add(&format!("VECTOR('',#{dn},{:?})", norm(sub(b, a))));
        let l = f.add(&format!("LINE('',#{p},#{vec})"));
        let e = f.add(&format!(
            "EDGE_CURVE('',#{},#{},#{l},.T.)",
            v[k],
            v[(k + 1) % 4]
        ));
        oes.push(f.add(&format!("ORIENTED_EDGE('',*,*,#{e},.T.)")));
    }
    let l: Vec<String> = oes.iter().map(|e| format!("#{e}")).collect();
    let lp = f.add(&format!("EDGE_LOOP('',({}))", l.join(",")));
    let mut bounds = vec![f.add(&format!("FACE_OUTER_BOUND('',#{lp},.T.)"))];
    for i in 0..3 {
        for j in 0..2 {
            let c = [4. + 6. * f64::from(i), 3. + 6. * f64::from(j), 0.];
            bounds.push(f.circle_bound(c, 1.5, false));
        }
    }
    let ax = f.ax([0.; 3]);
    let plane = f.add(&format!("PLANE('',#{ax})"));
    let face = f.face(&bounds, plane);
    let item = f.item(&[face], false);
    part_file("perforated-plate.step", 1e-7, &item, &f.out)
}

#[test]
fn tests_data_faces_match_occts_areas() {
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
    files.push(perforated_plate());
    files.push(shaft());
    let all = run(&files);
    assert!(
        all[0].exact >= 2,
        "the plate and tests/data's planes compare"
    );
    assert!(all[1].bounded >= 1, "the shaft's band compares");
}

#[test]
fn corpus_faces_match_occts_areas() {
    if let Some(files) = corpus_files("mesh_faces") {
        let all = run(&files);
        assert!(all[1].bounded > 0, "no cylinder or cone compared");
    }
}

/// Peak resident memory of this process, in bytes.
fn peak_rss() -> u64 {
    let mut r = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: getrusage writes the struct it is given.
    let r = unsafe {
        libc::getrusage(libc::RUSAGE_SELF, r.as_mut_ptr());
        r.assume_init()
    };
    let kb = if cfg!(target_os = "macos") { 1 } else { 1024 };
    u64::try_from(r.ru_maxrss).unwrap_or(0) * kb
}

#[test]
fn abc_00000046_perforated_plate_meshes() {
    // ABC 00000046: two planar faces with 2,499 holes each (plan.md, S1,
    // says about 1,250), where OCCT's BRepMesh takes 7.7 s and 4.2 GB at
    // 30°.
    // `STEPV_PLATE` names the file, as `STEPV_CORPUS` names the corpus:
    // unset, the test skips loudly; set to a path that isn't a file, it
    // fails.
    let Some(file) = std::env::var_os("STEPV_PLATE") else {
        let _ = writeln!(
            std::io::stderr(),
            "mesh_faces plate: SKIPPED, nothing meshed: STEPV_PLATE is unset"
        );
        return;
    };
    let file = PathBuf::from(file);
    assert!(
        file.is_file(),
        "STEPV_PLATE={} is not a file",
        file.display()
    );
    let t = Instant::now();
    let g = Graph::open(&file).unwrap();
    let s = graph::structure(&g).unwrap();
    let diag = match graph::bounds(&g, &s).model() {
        Bound::Box(b) => b.diagonal(),
        Bound::Unknown(r) => panic!("no model box: {r}"),
    };
    let tol = Tolerance::new(Deflection::PREVIEW, diag).unwrap();
    let me = &s.products[0];
    let read = t.elapsed();
    let t = Instant::now();
    let edges = edges_of(&g, &me.bodies);
    let p = Polylines::build(&g, edges.iter().copied(), me.mm_per_unit, tol);
    let polylines = t.elapsed();
    let t = Instant::now();
    let ctx = Context::of(&g, me.bodies[0]);
    let (mut faces, mut holes, mut triangles) = (0, 0, 0);
    let mut routed = BTreeMap::new();
    for f in g.of_type("ADVANCED_FACE").filter(|&f| on_plane(&g, f)) {
        match plane::mesh(&g, f, &p, ctx, false) {
            Ok(m) => {
                faces += 1;
                holes = holes.max(refs(g.args(f).unwrap().get(1)).len() - 1);
                triangles += m.triangles.len();
            }
            Err(r) => *routed.entry(r.to_string()).or_insert(0usize) += 1,
        }
    }
    let meshed = t.elapsed();
    eprintln!(
        "mesh_faces plate: {} planar faces meshed (up to {holes} holes), {triangles} triangles, \
         routed {routed:?}; read + structure + boxes {:.2} s, polylines {:.2} s, faces {:.3} s; \
         peak RSS {:.0} MB (OCCT: 7.7 s, 4.2 GB)",
        faces,
        read.as_secs_f64(),
        polylines.as_secs_f64(),
        meshed.as_secs_f64(),
        peak_rss() as f64 / 1e6
    );
    assert!(routed.is_empty(), "{routed:?}");
    assert!(holes >= 2499, "the plate's faces have their holes");
}

#[test]
fn a_files_faces_mesh_in_rust_timed() {
    // `STEPV_TIMING` names a file (Ultimaker's 9066.STEP): every product's
    // planar, cylinder and cone faces meshed in Rust, the share of all its
    // faces meshed, and the time. Unset, the test skips loudly.
    let Some(file) = std::env::var_os("STEPV_TIMING") else {
        let _ = writeln!(
            std::io::stderr(),
            "mesh_faces timing: SKIPPED, nothing meshed: STEPV_TIMING is unset"
        );
        return;
    };
    let file = PathBuf::from(file);
    assert!(
        file.is_file(),
        "STEPV_TIMING={} is not a file",
        file.display()
    );
    let t = Instant::now();
    let g = Graph::open(&file).unwrap();
    let s = graph::structure(&g).unwrap();
    let diag = match graph::bounds(&g, &s).model() {
        Bound::Box(b) => b.diagonal(),
        Bound::Unknown(r) => panic!("no model box: {r}"),
    };
    let tol = Tolerance::new(Deflection::PREVIEW, diag).unwrap();
    let read = t.elapsed();
    let t = Instant::now();
    let (mut all, mut meshed, mut triangles) = ([0usize; 2], [0usize; 2], 0);
    let mut routed: BTreeMap<String, usize> = BTreeMap::new();
    let mut faces = 0;
    let colors = graph::colors(&g);
    for (i, me) in s
        .products
        .iter()
        .enumerate()
        .filter(|(_, p)| !p.bodies.is_empty())
    {
        let edges = edges_of(&g, &me.bodies);
        let p = Polylines::build(&g, edges.iter().copied(), me.mm_per_unit, tol);
        let ctx = Context::of(&g, me.bodies[0]);
        let mine: Vec<Node> = colors
            .parts(&g, &s, i)
            .iter()
            .flat_map(|m| m.faces.iter().map(|f| f.face))
            .collect();
        faces += mine.len();
        for f in mine {
            let Some(k) = kind(&g, f) else { continue };
            all[k as usize] += 1;
            let m = match k {
                Kind::Plane => plane::mesh(&g, f, &p, ctx, false),
                Kind::Analytic => analytic::mesh(&g, f, &p, ctx, tol, false),
            };
            match m {
                Ok(m) => {
                    meshed[k as usize] += 1;
                    triangles += m.triangles.len();
                }
                Err(r) => *routed.entry(r.to_string()).or_default() += 1,
            }
        }
    }
    let meshing = t.elapsed();
    eprintln!(
        "mesh_faces timing: {faces} faces; planar {}/{} and cylinder or cone {}/{} meshed in Rust \
         ({:.1}% of all faces), {triangles} triangles, routed {routed:?}; read + structure + \
         boxes {:.2} s, edges + faces {:.2} s",
        meshed[0],
        all[0],
        meshed[1],
        all[1],
        100. * (meshed[0] + meshed[1]) as f64 / faces.max(1) as f64,
        read.as_secs_f64(),
        meshing.as_secs_f64()
    );
}
