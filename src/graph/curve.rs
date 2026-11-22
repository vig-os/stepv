//! Curve resolution shared by the boxes (§2.3), the fits (§2.8) and the
//! edge polylines (§2.6, `mesh::edges`): an `EDGE_CURVE`'s references, a
//! `SURFACE_CURVE`'s or `SEAM_CURVE`'s 3-D curve, a whole `CIRCLE` or
//! `ELLIPSE`, and a `CARTESIAN_POINT` or `VERTEX_POINT`, in mm.

use super::structure::{fields, frame, real, refers};
use super::{Graph, MAX_DEPTH, Node, Reason, Value};

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
