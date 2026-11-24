//! Curve resolution shared by the boxes (§2.3), the fits (§2.8) and the
//! edge polylines (§2.6, `mesh::edges`): an `EDGE_CURVE`'s references, a
//! `SURFACE_CURVE`'s or `SEAM_CURVE`'s 3-D curve, a whole `CIRCLE` or
//! `ELLIPSE`, a `B_SPLINE_CURVE_WITH_KNOTS`, and a `CARTESIAN_POINT` or
//! `VERTEX_POINT`, in mm.

use super::structure::{fields, frame, real, refers};
pub(crate) use super::structure::{mm_per_unit, rad_per_unit};
use super::{Graph, MAX_DEPTH, Node, Reason, TypeName, Value};

/// A circle or an ellipse in mm: `c + a·cos t·x + b·sin t·y`, with x and y
/// unit and orthogonal, and a, b ≥ 0 (a circle has a = b). t is the
/// ISO 10303-42 parameter, in radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Conic {
    pub c: [f64; 3],
    pub x: [f64; 3],
    pub y: [f64; 3],
    pub a: f64,
    pub b: f64,
}

/// An `EDGE_CURVE`'s arguments: its vertices, its geometry and its sense.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EdgeCurve {
    pub start: Node,
    pub end: Node,
    pub geometry: Node,
    /// The edge runs in the curve's direction.
    pub same_sense: bool,
}

/// `e`'s `EDGE_CURVE` arguments.
pub(crate) fn edge_curve(g: &Graph, e: Node) -> Result<EdgeCurve, Reason> {
    let bad = Reason("malformed edge");
    let a = fields(g, e, "EDGE_CURVE").map_err(|_| bad)?;
    Ok(EdgeCurve {
        start: refers(&a, 1).ok_or(bad)?,
        end: refers(&a, 2).ok_or(bad)?,
        geometry: refers(&a, 3).ok_or(bad)?,
        same_sense: logical(a.get(4)).ok_or(bad)?,
    })
}

/// A `BOOLEAN` argument.
fn logical(v: Option<&Value<'_>>) -> Option<bool> {
    match v? {
        Value::Enum("T") => Some(true),
        Value::Enum("F") => Some(false),
        _ => None,
    }
}

/// An `ORIENTED_EDGE`'s edge element, and whether it is walked from the
/// element's start to its end.
pub(crate) fn oriented_edge(g: &Graph, oe: Node) -> Result<(Node, bool), Reason> {
    let bad = Reason("malformed oriented edge");
    let a = fields(g, oe, "ORIENTED_EDGE").map_err(|_| bad)?;
    Ok((refers(&a, 3).ok_or(bad)?, logical(a.get(4)).ok_or(bad)?))
}

/// The 3-D curve under `c`: itself, or a `SURFACE_CURVE`'s, `SEAM_CURVE`'s
/// or `INTERSECTION_CURVE`'s `curve_3d`, nested at most [`MAX_DEPTH`] deep.
pub(crate) fn basis(g: &Graph, mut c: Node) -> Result<Node, Reason> {
    for _ in 0..=MAX_DEPTH {
        if !["SURFACE_CURVE", "SEAM_CURVE", "INTERSECTION_CURVE"]
            .iter()
            .any(|t| g.is_a(c, t))
        {
            return Ok(c);
        }
        let a = g.args(c).map_err(|_| Reason("unsupported curve"))?;
        c = refers(&a, 1).ok_or(Reason("malformed edge"))?;
    }
    Err(Reason("nested too deep"))
}

/// A finite real.
fn num(v: Option<&Value<'_>>) -> Option<f64> {
    v.and_then(real).filter(|x| x.is_finite())
}

/// A `CIRCLE` or an `ELLIPSE`, whole, at `s` mm per unit: its centre, axes
/// and radii all finite.
pub(crate) fn conic(g: &Graph, c: Node, s: f64) -> Result<Conic, Reason> {
    let circle = g.is_a(c, "CIRCLE");
    let is_conic = circle || g.is_a(c, "ELLIPSE");
    if !is_conic {
        return Err(Reason("unsupported curve"));
    }
    let a = g.args(c).map_err(|_| Reason("malformed conic"))?;
    let f = refers(&a, 1)
        .and_then(|p| frame(g, p, s).ok())
        .ok_or(Reason("malformed conic"))?;
    let p = num(a.get(2)).ok_or(Reason("non-finite radius"))?;
    let q = if circle {
        p
    } else {
        num(a.get(3)).ok_or(Reason("non-finite radius"))?
    };
    // The placement's reals are read as written: `1.E400` is +inf.
    if !f.iter().all(|v| v.is_finite()) {
        return Err(Reason("non-finite conic"));
    }
    // Finite after scaling, as a radius in inches can overflow in mm.
    let (p, q) = (p.abs() * s, q.abs() * s);
    if !(p.is_finite() && q.is_finite()) {
        return Err(Reason("non-finite radius"));
    }
    let col = |k: usize| [f[k], f[4 + k], f[8 + k]];
    Ok(Conic {
        c: col(3),
        x: col(0),
        y: col(1),
        a: p,
        b: q,
    })
}

/// A `CARTESIAN_POINT` at `s` mm per unit, its three coordinates finite
/// after scaling (1E307 inches overflow).
pub(crate) fn point(g: &Graph, p: Node, s: f64) -> Result<[f64; 3], Reason> {
    fields(g, p, "CARTESIAN_POINT")
        .ok()
        .and_then(|a| match a.get(1) {
            Some(Value::List(v)) if v.len() == 3 => {
                let c: Option<Vec<f64>> = v.iter().map(|x| num(Some(x))).collect();
                c.map(|c| [c[0] * s, c[1] * s, c[2] * s])
            }
            _ => None,
        })
        .filter(|c| c.iter().all(|x| x.is_finite()))
        .ok_or(Reason("not a finite 3-D point"))
}

/// A `VERTEX_POINT`'s point, at `s` mm per unit.
pub(crate) fn vertex(g: &Graph, v: Node, s: f64) -> Result<[f64; 3], Reason> {
    let p = fields(g, v, "VERTEX_POINT")
        .ok()
        .and_then(|a| refers(&a, 1))
        .ok_or(Reason("not a finite 3-D point"))?;
    point(g, p, s)
}

/// The highest B-spline degree [`bspline`] accepts, OCCT's own limit. Work
/// per span grows with the cube of the degree, so a larger one is routed.
pub(crate) const MAX_DEGREE: usize = 25;

/// A `B_SPLINE_CURVE_WITH_KNOTS` in mm (ISO 10303-42 §4.3.4), rational or
/// not, simple or complex: degree p, n control points as a struct of arrays
/// with their weights, and the n + p + 1 knots, expanded by their
/// multiplicities. Its parameter range is `[knots[p], knots[n]]`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BSpline {
    pub degree: usize,
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub z: Vec<f64>,
    /// 1 unless rational; finite, > 0 and scaled so the largest is 1 (a
    /// rational curve doesn't change when every weight is scaled alike), so
    /// a weighted coordinate can't overflow.
    pub w: Vec<f64>,
    pub knots: Vec<f64>,
}

/// An integer argument.
fn int(v: Option<&Value<'_>>) -> Option<i64> {
    match v? {
        Value::Int(i) => Some(*i),
        _ => None,
    }
}

/// A list argument.
fn list<'v, 'a>(v: Option<&'v Value<'a>>) -> Option<&'v [Value<'a>]> {
    match v? {
        Value::List(l) => Some(l),
        _ => None,
    }
}

/// A `B_SPLINE_CURVE_WITH_KNOTS` at `s` mm per unit, validated: degree
/// 1..=[`MAX_DEGREE`]; multiplicities from 1 to p + 1 summing to n + p + 1;
/// finite, non-decreasing knots; no interior knot of multiplicity over p,
/// where the curve could jump (`discontinuous B-spline`); a non-empty
/// range; finite weights > 0, one per control point; control points finite
/// in mm. `closed_curve` and `self_intersect` must be LOGICALs, but are
/// "for information only" (ISO 10303-42), so nothing relies on them.
pub(crate) fn bspline(g: &Graph, c: Node, s: f64) -> Result<BSpline, Reason> {
    if !g.is_a(c, "B_SPLINE_CURVE_WITH_KNOTS") {
        return Err(Reason("unsupported curve"));
    }
    let bad = Reason("malformed B-spline");
    // A simple entity lists every attribute, its name first; a complex one
    // holds each type's own in its part.
    let simple = matches!(g.type_name(c), TypeName::Simple(_));
    let b = fields(g, c, "B_SPLINE_CURVE").map_err(|_| bad)?;
    let (o, k) = if simple {
        (1, b.clone())
    } else {
        let k = fields(g, c, "B_SPLINE_CURVE_WITH_KNOTS").map_err(|_| bad)?;
        (0, k)
    };
    let ko = if simple { 6 } else { 0 };
    let p = int(b.get(o))
        .and_then(|p| usize::try_from(p).ok())
        .filter(|p| (1..=MAX_DEGREE).contains(p))
        .ok_or(Reason("unsupported degree"))?;
    let logical = |v: Option<&Value<'_>>| matches!(v, Some(Value::Enum("T" | "F" | "U")));
    if !logical(b.get(o + 3)) || !logical(b.get(o + 4)) {
        return Err(bad);
    }
    let poles = list(b.get(o + 1)).ok_or(bad)?;
    let n = poles.len();
    let mults = list(k.get(ko)).ok_or(bad)?;
    let values = list(k.get(ko + 1)).ok_or(bad)?;
    if mults.len() != values.len() {
        return Err(bad);
    }
    let mut knots = Vec::with_capacity(n + p + 1);
    for (m, v) in mults.iter().zip(values) {
        let m = int(Some(m))
            .and_then(|m| usize::try_from(m).ok())
            .filter(|m| (1..=p + 1).contains(m))
            .ok_or(Reason("bad multiplicity"))?;
        let v = num(Some(v)).ok_or(Reason("non-finite knot"))?;
        if knots.last().is_some_and(|&l| v < l) {
            return Err(Reason("decreasing knots"));
        }
        if knots.len() + m > n + p + 1 {
            return Err(Reason("knot count"));
        }
        knots.extend(std::iter::repeat_n(v, m));
    }
    if knots.len() != n + p + 1 || n <= p {
        return Err(Reason("knot count"));
    }
    let (lo, hi) = (knots[p], knots[n]);
    if lo >= hi {
        return Err(Reason("empty knot range"));
    }
    // Equal neighbours add up: the multiplicity is the run of one value.
    for r in knots.chunk_by(|a, b| a == b) {
        let inside = r[0] > lo && r[0] < hi;
        if r.len() > p + 1 || (inside && r.len() > p) {
            return Err(Reason("discontinuous B-spline"));
        }
    }
    let mut w = vec![1.; n];
    if g.is_a(c, "RATIONAL_B_SPLINE_CURVE") {
        let a = fields(g, c, "RATIONAL_B_SPLINE_CURVE").map_err(|_| bad)?;
        let ws = list(a.first()).ok_or(bad)?;
        if ws.len() != n {
            return Err(Reason("malformed weights"));
        }
        for (w, v) in w.iter_mut().zip(ws) {
            *w = num(Some(v))
                .filter(|&v| v > 0.)
                .ok_or(Reason("non-positive weight"))?;
        }
        let top = w.iter().fold(0f64, |m, &v| m.max(v));
        w.iter_mut().for_each(|v| *v /= top);
        // A ratio past f64's range underflows to 0.
        if w.contains(&0.) {
            return Err(Reason("non-positive weight"));
        }
    }
    let (mut x, mut y, mut z) = (
        Vec::with_capacity(n),
        Vec::with_capacity(n),
        Vec::with_capacity(n),
    );
    for v in poles {
        let Value::Ref(r) = v else { return Err(bad) };
        let q = point(g, *r, s)?;
        x.push(q[0]);
        y.push(q[1]);
        z.push(q[2]);
    }
    Ok(BSpline {
        degree: p,
        x,
        y,
        z,
        w,
        knots,
    })
}

/// A list of references, `None` unless every element is one.
fn refs_list(v: Option<&Value<'_>>) -> Option<Vec<Node>> {
    list(v)?
        .iter()
        .map(|x| match x {
            Value::Ref(n) => Some(*n),
            _ => None,
        })
        .collect()
}

/// An `ADVANCED_FACE`'s bounds (`FACE_BOUND`s and `FACE_OUTER_BOUND`s), its
/// surface, and whether the surface's normal is the face's (`same_sense`).
pub(crate) fn advanced_face(g: &Graph, f: Node) -> Result<(Vec<Node>, Node, bool), Reason> {
    let bad = Reason("malformed face");
    let a = fields(g, f, "ADVANCED_FACE").map_err(|_| bad)?;
    let bounds = refs_list(a.get(1)).ok_or(bad)?;
    Ok((
        bounds,
        refers(&a, 2).ok_or(bad)?,
        logical(a.get(3)).ok_or(bad)?,
    ))
}

/// A `FACE_BOUND`'s or `FACE_OUTER_BOUND`'s `EDGE_LOOP`, as its oriented
/// edges, and its `orientation`: whether the face walks the loop in the
/// loop's own direction.
pub(crate) fn face_bound(g: &Graph, b: Node) -> Result<(Vec<Node>, bool), Reason> {
    let bad = Reason("malformed bound");
    let a = ["FACE_BOUND", "FACE_OUTER_BOUND"]
        .iter()
        .find(|t| g.is_a(b, t))
        .and_then(|t| fields(g, b, t).ok())
        .ok_or(bad)?;
    let lp = refers(&a, 1).ok_or(bad)?;
    if !g.is_a(lp, "EDGE_LOOP") {
        return Err(Reason("loop:not an edge loop"));
    }
    let edges = fields(g, lp, "EDGE_LOOP")
        .ok()
        .and_then(|l| refs_list(l.get(1)))
        .ok_or(bad)?;
    Ok((edges, logical(a.get(2)).ok_or(bad)?))
}

/// A `PLANE`'s position at `s` mm per unit: its origin, then its unit axes
/// x, y and z (the normal, `x × y`), all finite.
pub(crate) fn plane(g: &Graph, p: Node, s: f64) -> Result<[[f64; 3]; 4], Reason> {
    let bad = Reason("malformed plane");
    let a = fields(g, p, "PLANE").map_err(|_| bad)?;
    let f = refers(&a, 1).and_then(|n| frame(g, n, s).ok()).ok_or(bad)?;
    if !f.iter().all(|v| v.is_finite()) {
        return Err(Reason("non-finite plane"));
    }
    let col = |k: usize| [f[k], f[4 + k], f[8 + k]];
    Ok([col(3), col(0), col(1), col(2)])
}

/// A developable elementary surface's shape (ISO 10303-42 §4.4), in its
/// frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Elementary {
    /// `CYLINDRICAL_SURFACE`: radius r.
    Cylinder,
    /// `CONICAL_SURFACE`: radius r at the frame's origin, this semi-angle in
    /// radians, in (0, π/2).
    Cone(f64),
}

/// A `CYLINDRICAL_SURFACE` or `CONICAL_SURFACE` at `s` mm and `rad`
/// radians per unit: its origin and unit axes x, y, z, its radius r (> 0, or
/// ≥ 0 on a cone), and its shape, all finite. Any other surface is routed
/// by its type (`surface:SPHERICAL_SURFACE`, M3b).
pub(crate) fn elementary(
    g: &Graph,
    n: Node,
    s: f64,
    rad: f64,
) -> Result<([[f64; 3]; 4], f64, Elementary), Reason> {
    let cone = g.is_a(n, "CONICAL_SURFACE");
    if !cone && !g.is_a(n, "CYLINDRICAL_SURFACE") {
        return Err(if g.is_a(n, "SPHERICAL_SURFACE") {
            Reason("surface:SPHERICAL_SURFACE")
        } else if g.is_a(n, "TOROIDAL_SURFACE") {
            Reason("surface:TOROIDAL_SURFACE")
        } else {
            Reason("surface:not analytic")
        });
    }
    let bad = Reason("malformed surface");
    let a = g.args(n).map_err(|_| bad)?;
    let f = refers(&a, 1).and_then(|p| frame(g, p, s).ok()).ok_or(bad)?;
    let r = num(a.get(2)).map(|r| r * s).ok_or(bad)?;
    let shape = if cone {
        let t = num(a.get(3)).ok_or(bad)? * rad;
        if !(t > 0. && t < std::f64::consts::FRAC_PI_2) {
            return Err(Reason("surface:degenerate cone"));
        }
        Elementary::Cone(t)
    } else {
        Elementary::Cylinder
    };
    let ok = f.iter().all(|v| v.is_finite()) && r.is_finite();
    // A cone may have radius 0 at its origin (its apex); a cylinder can't.
    if !ok || r < 0. || (r == 0. && !cone) {
        return Err(Reason("surface:degenerate radius"));
    }
    let col = |k: usize| [f[k], f[4 + k], f[8 + k]];
    Ok(([col(3), col(0), col(1), col(2)], r, shape))
}

/// A `FACE_BOUND`'s or `FACE_OUTER_BOUND`'s `VERTEX_LOOP` vertex, or `None`
/// when its loop is not a vertex loop.
pub(crate) fn vertex_loop(g: &Graph, b: Node) -> Option<Node> {
    let a = ["FACE_BOUND", "FACE_OUTER_BOUND"]
        .iter()
        .find(|t| g.is_a(b, t))
        .and_then(|t| fields(g, b, t).ok())?;
    let lp = refers(&a, 1).filter(|&l| g.is_a(l, "VERTEX_LOOP"))?;
    refers(&fields(g, lp, "VERTEX_LOOP").ok()?, 1)
}
