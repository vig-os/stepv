//! Planar faces (ADR-0001 §2.6, M2): an `ADVANCED_FACE` on a `PLANE`, its
//! outer loop and any number of holes, as triangles whose vertices are
//! exactly its edges' canonical polylines ([`super::edges`]). The boundary
//! walk, triangulation, flood, orientation check and routes are
//! [`super::face`]'s, shared with [`super::analytic`].
//!
//! A planar face adds no point: §2.6 puts Steiner points only on curved
//! directions, and none on a constraint.
//!
//! **Inversion.** Every boundary point p is first checked against the plane
//! (origin o, unit axes x, y, normal z) within the context's length
//! uncertainty, `|(p − o)·z| ≤ uncertainty`, else the face is routed
//! (`curve:off-surface`). The comparison also allows f64's rounding of the
//! distance itself, `8ε·(|p| + |o|)`. Its (u, v) is then
//! `((p − o)·x, (p − o)·y)`.
//!
//! **Normals** are exact: z, flipped by `same_sense` and again when the face
//! is reversed in its shell (an `ORIENTED_FACE` or `ORIENTED_CLOSED_SHELL`
//! `.F.`, which the shell walk passes in). Triangles wind counter-clockwise
//! about it.

use super::edges::Polylines;
pub use super::face::{Context, FaceMesh, Route};
use super::face::{Vertex, route, triangulate, walk};
use crate::graph::curve;
use crate::graph::{Graph, Node};

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// A bound on the rounding of `(p − o)·z` in f64 for a unit z: the
/// subtraction's `ε·(|p| + |o|)` and the dot product's `3ε·|p − o|`, with
/// room to spare. Below it, "within the uncertainty" can't be decided.
pub(super) fn rounding(p: [f64; 3], o: [f64; 3]) -> f64 {
    8. * f64::EPSILON * (dot(p, p).sqrt() + dot(o, o).sqrt())
}

/// Meshes the `ADVANCED_FACE` `face` on a `PLANE` from `edges`, which holds
/// its edges' polylines; `reversed` when its shell uses it reversed.
///
/// # Errors
/// The [`Route`] that sends it to OCCT (module docs): a surface other than
/// a plane, an edge without a polyline, or a boundary the triangulation
/// can't take exactly.
pub fn mesh(
    g: &Graph,
    face: Node,
    edges: &Polylines,
    ctx: Context,
    reversed: bool,
) -> Result<FaceMesh, Route> {
    let (bounds, surface, same_sense) = curve::advanced_face(g, face).map_err(Route::Face)?;
    if !g.is_a(surface, "PLANE") {
        return Err(route("surface:not a plane"));
    }
    if !(ctx.uncertainty.is_finite() && ctx.uncertainty > 0.) {
        return Err(route("no length uncertainty"));
    }
    let [o, x, y, z] = curve::plane(g, surface, ctx.mm_per_unit).map_err(Route::Face)?;
    let mut loops = Vec::with_capacity(bounds.len());
    for &b in &bounds {
        let mut lp = Vec::new();
        for (ep, p) in walk(g, b, edges)? {
            let d = [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
            if dot(d, z).abs() > ctx.uncertainty + rounding(p, o) {
                return Err(route("curve:off-surface"));
            }
            let uv = [dot(d, x), dot(d, y)];
            lp.push(Vertex {
                ep: Some(ep),
                p,
                uv,
                pole: false,
                cut: false,
            });
        }
        loops.push(lp);
    }
    let t = triangulate(&loops, same_sense)?;
    let flip = same_sense == reversed;
    let (mut m, _, _) = t.into_mesh(flip);
    m.normals(flip, |_| z);
    Ok(m)
}

#[cfg(test)]
mod tests;
