//! Conservative bounding boxes from the graph (ADR-0001 §2.3), without OCCT.
//!
//! A box is computed per geometric entity type, never from "every point
//! reachable": a full-circle edge has one vertex and a sphere one centre, so
//! a box of points can be too small. Each box therefore contains the exact
//! geometry, or is [`Bound::Unknown`] with the reason. It is never a guess:
//!
//! - an edge on a `LINE` is its two vertices; on a `CIRCLE` or an `ELLIPSE`
//!   the whole conic's box; on a B-spline its control points' (a rational
//!   one needs every weight > 0); a `SURFACE_CURVE` or `SEAM_CURVE` its 3-D
//!   curve's;
//! - a face on a `PLANE`, `CYLINDRICAL_SURFACE`, `CONICAL_SURFACE` or
//!   `SURFACE_OF_LINEAR_EXTRUSION` is the union of its bounds (these surfaces
//!   are linear in v, so every coordinate is extreme on the boundary); on a
//!   `SPHERICAL_SURFACE` the sphere's box and its bounds; on a
//!   `TOROIDAL_SURFACE` the torus' local box through its 8 corners; on a
//!   B-spline its control points'; on a `SURFACE_OF_REVOLUTION` a local box
//!   in the axis frame (the basis curve's farthest distance from the axis as
//!   the radius), through its 8 corners; on an `OFFSET_SURFACE` its basis
//!   surface's, grown by the distance;
//! - anything else is `Unknown`, and so is every node above an `Unknown`
//!   child: a box of the rest would be too small. So is anything non-finite:
//!   a coordinate, a radius, a weight, a unit or a placement.
//!
//! Every change of frame carries a box's 8 corners and boxes them. Boxes
//! compose in one post-order pass over the product DAG: face → shell → solid
//! → representation → product, in its own mm, → instance → assembly → model.
//! Each product's box is computed once, however often it is instanced.
//! Lengths are scaled to mm by their representation's unit, as placements
//! are. Wireframe, `GEOMETRIC_CURVE_SET` and PMI lie outside the B-rep and
//! outside the box.

use std::collections::HashMap;

use super::structure::{
    compose_raw, default_x, fields, frame, mm_per_unit, post_order, real, refers,
};
use super::{Args, Graph, Node, Placement, Reason, Structure, TypeName, Value, mapped_placement};
use crate::BBox;

/// A node's box in mm, or why it has none.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Bound {
    /// Contains the node's exact geometry.
    Box(BBox),
    /// No box this graph can vouch for.
    Unknown(Reason),
}

/// Every product's, instance's and B-rep node's box ([`bounds`]): owned
/// data, no borrow of the graph.
#[derive(Debug, Clone)]
pub struct Bounds {
    structure: Structure,
    products: Vec<Acc>,
    model: Acc,
    nodes: HashMap<Node, Acc>,
}

impl Bounds {
    /// The whole model's: its roots', at the identity.
    #[must_use]
    pub fn model(&self) -> Bound {
        self.model.bound()
    }

    /// Product `p`'s, in its own coordinates.
    #[must_use]
    pub fn product(&self, p: usize) -> Bound {
        self.products
            .get(p)
            .map_or(Bound::Unknown(Reason("no such product")), |a| a.bound())
    }

    /// Leaf number `leaf`'s, in model coordinates: its product's box placed
    /// by the instances along [`Structure::leaf_path`].
    #[must_use]
    pub fn leaf(&self, leaf: u64) -> Bound {
        let s = &self.structure;
        let Some(path) = s.leaf_path(leaf) else {
            return Bound::Unknown(Reason("no such leaf"));
        };
        let mut t = IDENTITY;
        for w in path.0.windows(2) {
            match s.products[w[0].0].children[w[1].1].placement {
                Placement::Placed(p) => t = compose_raw(t, p),
                Placement::Unplaced(r) => return Bound::Unknown(r),
            }
        }
        let (p, _) = *path.0.last().expect("a path has a level");
        self.products[p].placed(&t).bound()
    }

    /// The box of a face, shell, solid, edge, vertex or representation the
    /// walk reached, in its representation's mm.
    #[must_use]
    pub fn node(&self, n: Node) -> Bound {
        self.nodes
            .get(&n)
            .map_or(Bound::Unknown(Reason("not reached")), |a| a.bound())
    }
}

/// Computes every box in one pass, in O(entities + instances).
#[must_use]
pub fn bounds(g: &Graph, s: &Structure) -> Bounds {
    let mut w = Walk {
        g,
        memo: HashMap::new(),
        reps: HashMap::new(),
        open: Vec::new(),
    };
    let mut products = vec![Acc::Empty; s.products.len()];
    match post_order(&s.products) {
        Ok(order) => {
            for p in order {
                let me = &s.products[p];
                let mut acc = Acc::Empty;
                for &b in &me.bodies {
                    acc = acc.union(w.rep(b)).union(w.related(b));
                }
                for c in &me.children {
                    let child = products[c.child];
                    acc = acc.union(match (child, c.placement) {
                        (Acc::Empty, _) => Acc::Empty,
                        (_, Placement::Placed(t)) => child.placed(&t),
                        (_, Placement::Unplaced(r)) => Acc::Unknown(r),
                    });
                }
                products[p] = acc;
            }
        }
        Err(_) => products.fill(why("cyclic structure")),
    }
    let model = s
        .roots
        .iter()
        .fold(Acc::Empty, |a, &r| a.union(products[r]));
    let mut nodes: HashMap<Node, Acc> = w.memo.into_iter().map(|(n, (_, a))| (n, a)).collect();
    nodes.extend(w.reps);
    Bounds {
        structure: s.clone(),
        products,
        model,
        nodes,
    }
}

const IDENTITY: [f64; 12] = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0.];

/// Nesting deeper than this (mapped items, oriented shells, curves on
/// curves) is `Unknown`, so a hostile file can't overflow the stack.
const MAX_DEPTH: usize = 64;

/// A box under construction: `Empty` is the union's identity.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Acc {
    Empty,
    Box(BBox),
    Unknown(Reason),
}

const fn why(r: &'static str) -> Acc {
    Acc::Unknown(Reason(r))
}

impl Acc {
    fn bound(self) -> Bound {
        match self {
            Self::Box(b) => Bound::Box(b),
            Self::Unknown(r) => Bound::Unknown(r),
            Self::Empty => Bound::Unknown(Reason("no B-rep geometry")),
        }
    }

    fn point(p: [f64; 3]) -> Self {
        if p.iter().all(|x| x.is_finite()) {
            Self::Box(BBox { min: p, max: p })
        } else {
            why("non-finite coordinate")
        }
    }

    /// The box `c ± h`.
    fn around(c: [f64; 3], h: [f64; 3]) -> Self {
        let at = |sign: f64| [0, 1, 2].map(|i| c[i] + sign * h[i]);
        Self::point(at(-1.)).union(Self::point(at(1.)))
    }

    fn union(self, o: Self) -> Self {
        match (self, o) {
            (Self::Unknown(r), _) | (_, Self::Unknown(r)) => Self::Unknown(r),
            (Self::Empty, b) | (b, Self::Empty) => b,
            (Self::Box(a), Self::Box(b)) => Self::Box(BBox {
                min: [0, 1, 2].map(|i| a.min[i].min(b.min[i])),
                max: [0, 1, 2].map(|i| a.max[i].max(b.max[i])),
            }),
        }
    }

    fn grown(self, d: f64) -> Self {
        match self {
            Self::Box(b) => {
                Self::point(b.min.map(|x| x - d)).union(Self::point(b.max.map(|x| x + d)))
            }
            a => a,
        }
    }

    /// The box of its 8 corners under `t` (row-major 3×4).
    fn placed(self, t: &[f64; 12]) -> Self {
        let Self::Box(b) = self else {
            return self;
        };
        if !t.iter().all(|x| x.is_finite()) {
            return why("non-finite placement");
        }
        (0..8).fold(Self::Empty, |acc, k| {
            let p = [0, 1, 2].map(|i| if k >> i & 1 == 0 { b.min[i] } else { b.max[i] });
            acc.union(Self::point(apply(t, p)))
        })
    }
}

fn apply(t: &[f64; 12], p: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|r| t[r * 4] * p[0] + t[r * 4 + 1] * p[1] + t[r * 4 + 2] * p[2] + t[r * 4 + 3])
}

/// Column `c` of a frame: 0 its x axis, 1 y, 2 z, 3 its origin.
fn column(t: &[f64; 12], c: usize) -> [f64; 3] {
    [t[c], t[4 + c], t[8 + c]]
}

/// A finite real.
fn num(v: Option<&Value<'_>>) -> Option<f64> {
    v.and_then(real).filter(|x| x.is_finite())
}

/// The references in a list argument; nested lists flattened (a B-spline
/// surface's grid).
fn ref_list(v: Option<&Value<'_>>, out: &mut Vec<Node>) {
    match v {
        Some(Value::Ref(n)) => out.push(*n),
        Some(Value::List(l)) => l.iter().for_each(|v| ref_list(Some(v), out)),
        _ => {}
    }
}

const SKIPPED: [&str; 7] = [
    "AXIS1_PLACEMENT",
    "AXIS2_PLACEMENT_2D",
    "AXIS2_PLACEMENT_3D",
    "CARTESIAN_POINT",
    "GEOMETRIC_CURVE_SET",
    "GEOMETRIC_SET",
    "EDGE_BASED_WIREFRAME_MODEL",
];

/// Solids, shells and face sets: the union of what they reference.
const CONTAINERS: [&str; 11] = [
    "MANIFOLD_SOLID_BREP",
    "BREP_WITH_VOIDS",
    "FACETED_BREP",
    "CLOSED_SHELL",
    "OPEN_SHELL",
    "ORIENTED_CLOSED_SHELL",
    "ORIENTED_OPEN_SHELL",
    "ORIENTED_FACE",
    "SHELL_BASED_SURFACE_MODEL",
    "FACE_BASED_SURFACE_MODEL",
    "CONNECTED_FACE_SET",
];

const BSPLINE_CURVES: [&str; 6] = [
    "B_SPLINE_CURVE",
    "B_SPLINE_CURVE_WITH_KNOTS",
    "BEZIER_CURVE",
    "UNIFORM_CURVE",
    "QUASI_UNIFORM_CURVE",
    "RATIONAL_B_SPLINE_CURVE",
];

const BSPLINE_SURFACES: [&str; 6] = [
    "B_SPLINE_SURFACE",
    "B_SPLINE_SURFACE_WITH_KNOTS",
    "BEZIER_SURFACE",
    "UNIFORM_SURFACE",
    "QUASI_UNIFORM_SURFACE",
    "RATIONAL_B_SPLINE_SURFACE",
];

struct Walk<'g> {
    g: &'g Graph,
    /// B-rep nodes, in mm at the scale they were met with.
    memo: HashMap<Node, (f64, Acc)>,
    /// Representations, in their own mm.
    reps: HashMap<Node, Acc>,
    /// The representations being walked, for mapped-item cycles.
    open: Vec<Node>,
}

impl Walk<'_> {
    fn any(&self, n: Node, types: &[&str]) -> bool {
        types.iter().any(|t| self.g.is_a(n, t))
    }

    /// A representation's items, in its own mm.
    fn rep(&mut self, rep: Node) -> Acc {
        if let Some(&a) = self.reps.get(&rep) {
            return a;
        }
        if self.open.contains(&rep) {
            return why("mapped item cycle");
        }
        if self.open.len() >= MAX_DEPTH {
            return why("nested too deep");
        }
        let s = mm_per_unit(self.g, rep);
        if !(s.is_finite() && s > 0.) {
            return why("non-finite unit");
        }
        let mut items = Vec::new();
        if let Ok(a) = fields(self.g, rep, "REPRESENTATION") {
            ref_list(a.get(1), &mut items);
        }
        self.open.push(rep);
        let mut acc = Acc::Empty;
        for item in items {
            acc = acc.union(self.item(item, rep, s));
        }
        self.open.pop();
        self.reps.insert(rep, acc);
        acc
    }

    /// The representations `rep` is related to without a transformation,
    /// as OCCT's transfer adds them (`TransferRelatedSRR`): AP203 hybrids.
    fn related(&mut self, rep: Node) -> Acc {
        let g = self.g;
        let mut acc = Acc::Empty;
        for &r in g.referrers(rep) {
            let simple = |t| g.type_name(r) == TypeName::Simple(t);
            if !simple("SHAPE_REPRESENTATION_RELATIONSHIP")
                && !simple("MECHANICAL_DESIGN_AND_DRAUGHTING_RELATIONSHIP")
            {
                continue;
            }
            let Ok(a) = g.args(r) else { continue };
            let other = if refers(&a, 2) == Some(rep) {
                refers(&a, 3)
            } else {
                refers(&a, 2)
            };
            if let Some(o) = other.filter(|&o| o != rep) {
                acc = acc.union(self.rep(o));
            }
        }
        acc
    }

    fn item(&mut self, item: Node, rep: Node, s: f64) -> Acc {
        let g = self.g;
        if g.is_a(item, "MAPPED_ITEM") {
            let map = fields(g, item, "MAPPED_ITEM")
                .ok()
                .and_then(|a| refers(&a, 1));
            let mapped = map
                .and_then(|m| fields(g, m, "REPRESENTATION_MAP").ok())
                .and_then(|a| refers(&a, 1));
            let Some(mapped) = mapped else {
                return why("malformed mapped item");
            };
            return match (self.rep(mapped), mapped_placement(g, item, rep)) {
                (Acc::Empty, _) => Acc::Empty,
                (b, Placement::Placed(t)) => b.placed(&t),
                (_, Placement::Unplaced(r)) => Acc::Unknown(r),
            };
        }
        if self.any(item, &SKIPPED) {
            return Acc::Empty;
        }
        self.topo(item, s, 0)
    }

    /// A solid, shell, face, edge or vertex, memoised.
    fn topo(&mut self, n: Node, s: f64, depth: usize) -> Acc {
        if let Some(&(at, a)) = self.memo.get(&n)
            && at == s
        {
            return a;
        }
        let a = if depth > MAX_DEPTH {
            why("nested too deep")
        } else if self.any(n, &["ADVANCED_FACE", "FACE_SURFACE"]) {
            self.face(n, s, depth)
        } else if self.g.is_a(n, "EDGE_CURVE") {
            match self.g.args(n) {
                Ok(a) => self.curve(refers(&a, 3), s, Some((refers(&a, 1), refers(&a, 2))), 0),
                Err(_) => why("malformed edge"),
            }
        } else if self.g.is_a(n, "VERTEX_POINT") {
            self.point(self.g.args(n).ok().and_then(|a| refers(&a, 1)), s)
        } else if self.any(n, &CONTAINERS) {
            let kids = self.g.refs(n).to_vec();
            kids.into_iter()
                .fold(Acc::Empty, |a, k| a.union(self.topo(k, s, depth + 1)))
        } else {
            why("unsupported representation item")
        };
        self.memo.entry(n).or_insert((s, a));
        a
    }

    fn point(&self, p: Option<Node>, s: f64) -> Acc {
        let c = p
            .and_then(|p| fields(self.g, p, "CARTESIAN_POINT").ok())
            .and_then(|a| match a.get(1) {
                Some(Value::List(v)) if v.len() == 3 => {
                    let c: Option<Vec<f64>> = v.iter().map(|x| num(Some(x))).collect();
                    c.map(|c| [c[0] * s, c[1] * s, c[2] * s])
                }
                _ => None,
            });
        c.map_or(why("not a finite 3-D point"), Acc::point)
    }

    fn face(&mut self, n: Node, s: f64, depth: usize) -> Acc {
        let g = self.g;
        let Ok(a) = g.args(n) else {
            return why("malformed face");
        };
        let Some(surf) = refers(&a, 2) else {
            return why("malformed face");
        };
        let ruled = [
            "PLANE",
            "CYLINDRICAL_SURFACE",
            "CONICAL_SURFACE",
            "SURFACE_OF_LINEAR_EXTRUSION",
        ];
        let sphere = g.is_a(surf, "SPHERICAL_SURFACE");
        if !sphere && !self.any(surf, &ruled) {
            return self.surface(surf, s, depth);
        }
        let mut bounds = Vec::new();
        ref_list(a.get(1), &mut bounds);
        let (mut acc, mut edges) = (Acc::Empty, false);
        for b in bounds {
            let lp = fields(g, b, "FACE_BOUND").ok().and_then(|a| refers(&a, 1));
            match lp {
                Some(lp) if g.is_a(lp, "EDGE_LOOP") => {
                    for &oe in g.refs(lp) {
                        let e = g.args(oe).ok().and_then(|a| refers(&a, 3));
                        let Some(e) = e else {
                            return why("malformed oriented edge");
                        };
                        acc = acc.union(self.topo(e, s, depth + 1));
                        edges = true;
                    }
                }
                Some(lp) if g.is_a(lp, "VERTEX_LOOP") => {
                    let v = g.refs(lp).first().copied();
                    acc =
                        acc.union(v.map_or(why("malformed loop"), |v| self.topo(v, s, depth + 1)));
                }
                _ => return why("unsupported loop"),
            }
        }
        if sphere {
            self.surface(surf, s, depth).union(acc)
        } else if edges {
            acc
        } else {
            why("face without edges")
        }
    }

    /// A curve's box; a line's needs its edge's vertices `ends`.
    fn curve(
        &mut self,
        c: Option<Node>,
        s: f64,
        ends: Option<(Option<Node>, Option<Node>)>,
        depth: usize,
    ) -> Acc {
        let g = self.g;
        let Some(c) = c else {
            return why("malformed edge");
        };
        if depth > MAX_DEPTH {
            return why("nested too deep");
        }
        if self.any(c, &BSPLINE_CURVES) {
            return self.poles(c, s, false);
        }
        let Ok(a) = g.args(c) else {
            return why("unsupported curve");
        };
        if self.any(c, &["SURFACE_CURVE", "SEAM_CURVE", "INTERSECTION_CURVE"]) {
            return self.curve(refers(&a, 1), s, ends, depth + 1);
        }
        if g.is_a(c, "LINE") {
            return match ends {
                Some((v1, v2)) => {
                    let v = |v: Option<Node>, w: &mut Self| {
                        v.map_or(why("malformed edge"), |v| w.topo(v, s, depth + 1))
                    };
                    v(v1, self).union(v(v2, self))
                }
                None => why("unbounded line"),
            };
        }
        let conic = g.is_a(c, "CIRCLE") || g.is_a(c, "ELLIPSE");
        let Some(f) = refers(&a, 1)
            .filter(|_| conic)
            .and_then(|p| frame(g, p, s).ok())
        else {
            return why(if conic {
                "malformed conic"
            } else {
                "unsupported curve"
            });
        };
        let (x, y, z) = (column(&f, 0), column(&f, 1), column(&f, 2));
        let h = if g.is_a(c, "CIRCLE") {
            let Some(r) = num(a.get(2)) else {
                return why("non-finite radius");
            };
            z.map(|n| r.abs() * s * (1. - n * n).max(0.).sqrt())
        } else {
            let (Some(p), Some(q)) = (num(a.get(2)), num(a.get(3))) else {
                return why("non-finite radius");
            };
            [0, 1, 2].map(|i| ((p * s * x[i]).powi(2) + (q * s * y[i]).powi(2)).sqrt())
        };
        Acc::around(column(&f, 3), h)
    }

    /// A surface's own box, without its face's bounds.
    fn surface(&mut self, sf: Node, s: f64, depth: usize) -> Acc {
        let g = self.g;
        if depth > MAX_DEPTH {
            return why("nested too deep");
        }
        if self.any(sf, &BSPLINE_SURFACES) {
            return self.poles(sf, s, true);
        }
        let Ok(a) = g.args(sf) else {
            return why("unsupported surface");
        };
        if g.is_a(sf, "OFFSET_SURFACE") {
            let Some(d) = num(a.get(2)) else {
                return why("non-finite offset");
            };
            return match refers(&a, 1) {
                Some(b) => self.surface(b, s, depth + 1).grown(d.abs() * s),
                None => why("malformed offset surface"),
            };
        }
        if g.is_a(sf, "SURFACE_OF_REVOLUTION") {
            return self.revolution(&a, s, depth);
        }
        let torus = g.is_a(sf, "TOROIDAL_SURFACE") || g.is_a(sf, "DEGENERATE_TOROIDAL_SURFACE");
        if !torus && !g.is_a(sf, "SPHERICAL_SURFACE") {
            return why("unsupported surface");
        }
        let Some(f) = refers(&a, 1).and_then(|p| frame(g, p, s).ok()) else {
            return why("malformed surface");
        };
        let Some(r) = num(a.get(2)).map(|r| r.abs() * s) else {
            return why("non-finite radius");
        };
        if !torus {
            return Acc::around(column(&f, 3), [r; 3]);
        }
        let Some(m) = num(a.get(3)).map(|m| m.abs() * s) else {
            return why("non-finite radius");
        };
        Acc::around([0.; 3], [r + m, r + m, m]).placed(&f)
    }

    /// `SURFACE_OF_REVOLUTION(name, swept curve, AXIS1_PLACEMENT)`.
    fn revolution(&mut self, a: &Args<'_>, s: f64, depth: usize) -> Acc {
        let g = self.g;
        let Acc::Box(b) = self.curve(refers(a, 1), s, None, depth + 1) else {
            return why("unbounded swept curve");
        };
        let axis = refers(a, 2).and_then(|n| fields(g, n, "AXIS1_PLACEMENT").ok());
        let Some(axis) = axis else {
            return why("malformed axis");
        };
        let Acc::Box(o) = self.point(refers(&axis, 1), s) else {
            return why("malformed axis");
        };
        let dir = refers(&axis, 2)
            .and_then(|d| fields(g, d, "DIRECTION").ok())
            .and_then(|d| match d.get(1) {
                Some(Value::List(v)) if v.len() == 3 => {
                    v.iter().map(|x| num(Some(x))).collect::<Option<Vec<_>>>()
                }
                _ => None,
            })
            .map_or(Some([0., 0., 1.]), |v| {
                let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                (n > 0. && n.is_finite()).then(|| [v[0] / n, v[1] / n, v[2] / n])
            });
        let Some(z) = dir else {
            return why("malformed axis");
        };
        let o = o.min;
        // Distance from the axis and height along it are convex and linear,
        // so their extremes over the curve's box lie at its corners.
        let (mut r, mut lo, mut hi) = (0f64, f64::INFINITY, f64::NEG_INFINITY);
        for k in 0..8 {
            let p = [0, 1, 2].map(|i| (if k >> i & 1 == 0 { b.min[i] } else { b.max[i] }) - o[i]);
            let h = p[0] * z[0] + p[1] * z[1] + p[2] * z[2];
            let q = [0, 1, 2].map(|i| p[i] - h * z[i]);
            r = r.max((q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt());
            (lo, hi) = (lo.min(h), hi.max(h));
        }
        let x = default_x(z);
        let y = [
            z[1] * x[2] - z[2] * x[1],
            z[2] * x[0] - z[0] * x[2],
            z[0] * x[1] - z[1] * x[0],
        ];
        let t = [
            x[0], y[0], z[0], o[0], x[1], y[1], z[1], o[1], x[2], y[2], z[2], o[2],
        ];
        Acc::point([-r, -r, lo])
            .union(Acc::point([r, r, hi]))
            .placed(&t)
    }

    /// A B-spline's control points' box: their convex hull holds the curve
    /// or surface, a rational one's only when every weight is positive.
    fn poles(&self, n: Node, s: f64, surface: bool) -> Acc {
        let g = self.g;
        let simple = matches!(g.type_name(n), TypeName::Simple(_));
        let (part, rational) = if surface {
            ("B_SPLINE_SURFACE", "RATIONAL_B_SPLINE_SURFACE")
        } else {
            ("B_SPLINE_CURVE", "RATIONAL_B_SPLINE_CURVE")
        };
        // A simple entity lists its name first, and a rational one its
        // weights last; a complex one holds each in its own part.
        let k = usize::from(surface) + usize::from(simple) + 1;
        let Some(Ok(a)) = (if simple {
            Some(g.args(n))
        } else {
            g.args_of(n, part)
        }) else {
            return why("malformed B-spline");
        };
        let mut points = Vec::new();
        ref_list(a.get(k), &mut points);
        if g.is_a(n, rational) {
            let w = if simple {
                a.get(if surface { 8 } else { 6 }).cloned()
            } else {
                g.args_of(n, rational)
                    .and_then(Result::ok)
                    .and_then(|w| w.first().cloned())
            };
            let mut ws = Vec::new();
            weights(w.as_ref(), &mut ws);
            if ws.len() != points.len() {
                return why("malformed weights");
            }
            if !ws.iter().all(|w| w.is_finite() && *w > 0.) {
                return why("non-positive weight");
            }
        }
        if points.is_empty() {
            return why("malformed B-spline");
        }
        points
            .into_iter()
            .fold(Acc::Empty, |acc, p| acc.union(self.point(Some(p), s)))
    }
}

/// The weights in a (nested) list; a non-number reads as NaN.
fn weights(v: Option<&Value<'_>>, out: &mut Vec<f64>) {
    match v {
        Some(Value::List(l)) => l.iter().for_each(|v| weights(Some(v), out)),
        Some(v) => out.push(real(v).unwrap_or(f64::NAN)),
        None => {}
    }
}

#[cfg(test)]
mod tests;
