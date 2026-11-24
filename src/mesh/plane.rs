//! Planar faces (ADR-0001 §2.6, M2): an `ADVANCED_FACE` on a `PLANE`, its
//! outer loop and any number of holes, as triangles whose vertices are
//! exactly its edges' canonical polylines ([`super::edges`]).
//!
//! **Boundary.** Each bound's `EDGE_LOOP` is walked through its oriented
//! edges' shared polylines ([`Polylines::oriented`]), never resampled, and
//! reversed when the bound's `orientation` is `.F.`. Consecutive edges must
//! meet at a bit-identical point, and the loop must close (`loop:open`). The
//! mesh's vertices are those points, bit for bit, so two faces sharing an
//! edge share its points, and a planar face adds no other point: §2.6 puts
//! Steiner points only on curved directions, and none on a constraint.
//!
//! **Inversion.** Every boundary point p is first checked against the plane
//! (origin o, unit axes x, y, normal z) within the representation context's
//! length uncertainty, `|(p − o)·z| ≤ uncertainty`, else the face is routed
//! (`curve:off-surface`). The comparison allows f64's rounding of the
//! distance itself, `8ε·(|p| + |o|)`: an uncertainty below it (ABC
//! `00000046` writes 1e-17 mm) can't be checked in f64, and a point on the
//! plane would otherwise fail it. Its (u, v) is then
//! `((p − o)·x, (p − o)·y)`.
//!
//! **Triangulation.** `spade`'s constrained Delaunay triangulation, with
//! exact predicates, of every loop point, then every loop segment as a
//! constraint. A constraint that would cross another (`rs-fail:cdt-crossing`),
//! pass through a vertex, which would split it (`rs-fail:cdt-touching`), or
//! repeat one (`rs-fail:loops-overlap`) routes the face: a constraint is
//! never split, and no point is moved or added.
//!
//! **Flood.** The triangles inside the face are found by a flood from the
//! outside, crossing a constraint toggling inside and outside. So the flood
//! needs no outer loop: a face with only `FACE_BOUND`s meshes like one with a
//! `FACE_OUTER_BOUND`, and its holes are what the loops enclose.
//!
//! **Orientation.** ISO 10303-42 puts the face to the left of each bound,
//! walked in its oriented direction and seen from the face's topological
//! normal, which is the plane's normal z when `same_sense`, else −z. Every
//! segment is checked against the flood: the triangle to its left must be
//! inside (`rs-fail:orientation`). This is exact, as both are combinatorial.
//!
//! **Normals** are exact: z, flipped by `same_sense` and again when the face
//! is reversed in its shell (an `ORIENTED_FACE` or `ORIENTED_CLOSED_SHELL`
//! `.F.`, which the shell walk passes in). Triangles wind counter-clockwise
//! about it.
//!
//! **Degenerate loops** route too: fewer than three distinct points or a
//! zero-length segment (`loop:degenerate`), a loop through one point twice
//! (`loop:self-touching`), and two different points at one (u, v)
//! (`rs-fail:coincident`). Two loops may share a vertex.

use std::fmt;

use spade::handles::FixedVertexHandle;
use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};

use super::edges::{EdgePoint, Polylines};
use crate::graph::curve;
use crate::graph::{Graph, Node, Reason};

/// A face's representation context, as the face needs it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Context {
    /// Millimetres per length unit ([`crate::graph::Product::mm_per_unit`]).
    pub mm_per_unit: f64,
    /// The length uncertainty in mm ([`crate::graph::length_uncertainty`]
    /// times `mm_per_unit`).
    pub uncertainty: f64,
}

/// Why a face isn't meshed in Rust: it goes to OCCT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// An edge of its boundary has no polyline: `mesh::edges` routed it.
    Edge(Reason),
    /// The face itself.
    Face(Reason),
}

impl fmt::Display for Route {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Edge(r) => write!(f, "edge routed: {r}"),
            Self::Face(r) => write!(f, "{r}"),
        }
    }
}

/// A planar face's mesh in mm, as a struct of arrays.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceMesh {
    /// Each vertex as an edge point: where the loop walk first met it.
    pub points: Vec<EdgePoint>,
    /// Each vertex's coordinates: its edge point's, bit for bit.
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub z: Vec<f64>,
    /// Vertex indices, counter-clockwise about `normal`.
    pub triangles: Vec<[u32; 3]>,
    /// The face's outward unit normal, exact.
    pub normal: [f64; 3],
}

impl FaceMesh {
    /// Vertex `i`.
    ///
    /// # Panics
    /// When `i` is out of range.
    #[must_use]
    pub fn point(&self, i: usize) -> [f64; 3] {
        [self.x[i], self.y[i], self.z[i]]
    }
}

type Cdt = ConstrainedDelaunayTriangulation<Point2<f64>>;

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// A bound on the rounding of `(p − o)·z` in f64 for a unit z: the
/// subtraction's `ε·(|p| + |o|)` and the dot product's `3ε·|p − o|`, with
/// room to spare. Below it, "within the uncertainty" can't be decided.
fn rounding(p: [f64; 3], o: [f64; 3]) -> f64 {
    8. * f64::EPSILON * (dot(p, p).sqrt() + dot(o, o).sqrt())
}

fn route(r: &'static str) -> Route {
    Route::Face(Reason(r))
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
    let mut m = FaceMesh {
        points: Vec::new(),
        x: Vec::new(),
        y: Vec::new(),
        z: Vec::new(),
        triangles: Vec::new(),
        normal: z,
    };
    let mut cdt = Cdt::new();
    // Per spade vertex, ours; per ours, its loop.
    let (mut ours, mut owner) = (Vec::<u32>::new(), Vec::<usize>::new());
    let mut loops: Vec<Vec<FixedVertexHandle>> = Vec::with_capacity(bounds.len());
    for &b in &bounds {
        let pts = walk(g, b, edges)?;
        let mut lp = Vec::with_capacity(pts.len());
        for (ep, p) in pts {
            let d = [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
            if dot(d, z).abs() > ctx.uncertainty + rounding(p, o) {
                return Err(route("curve:off-surface"));
            }
            let h = cdt
                .insert(spade::mitigate_underflow(Point2::new(dot(d, x), dot(d, y))))
                .map_err(|_| route("rs-fail:coordinate"))?;
            if h.index() == ours.len() {
                let i = u32::try_from(m.points.len()).map_err(|_| route("too many points"))?;
                ours.push(i);
                owner.push(loops.len());
                m.points.push(ep);
                m.x.push(p[0]);
                m.y.push(p[1]);
                m.z.push(p[2]);
            } else {
                let i = ours[h.index()] as usize;
                if m.point(i) != p {
                    return Err(route("rs-fail:coincident"));
                }
                if lp.last() == Some(&h) {
                    return Err(route("loop:degenerate"));
                }
                if owner[i] == loops.len() {
                    return Err(route("loop:self-touching"));
                }
            }
            lp.push(h);
        }
        if lp.len() < 3 {
            return Err(route("loop:degenerate"));
        }
        loops.push(lp);
    }
    if loops.is_empty() {
        return Err(route("loop:degenerate"));
    }
    // Every vertex first, so no later point can land on a constraint.
    for lp in &loops {
        for (k, &a) in lp.iter().enumerate() {
            let b = lp[(k + 1) % lp.len()];
            if cdt.exists_constraint(a, b) {
                return Err(route("rs-fail:loops-overlap"));
            }
            match cdt.try_add_constraint(a, b).len() {
                0 => return Err(route("rs-fail:cdt-crossing")),
                1 => {}
                _ => return Err(route("rs-fail:cdt-touching")),
            }
        }
    }
    let inside = flood(&cdt);
    // The topological normal is z when `same_sense`: the face lies left of
    // every segment in (u, v); about −z, right of it.
    for lp in &loops {
        for (k, &a) in lp.iter().enumerate() {
            let b = lp[(k + 1) % lp.len()];
            let e = cdt
                .get_edge_from_neighbors(a, b)
                .ok_or(route("rs-fail:orientation"))?;
            let side = if same_sense { e.face() } else { e.rev().face() };
            if side.is_outer() || !inside[side.fix().index()] {
                return Err(route("rs-fail:orientation"));
            }
        }
    }
    let flip = same_sense == reversed;
    if flip {
        m.normal = z.map(|c| -c);
    }
    for f in cdt.inner_faces().filter(|f| inside[f.fix().index()]) {
        let [a, b, c] = f.vertices().map(|v| ours[v.fix().index()]);
        m.triangles.push(if flip { [a, c, b] } else { [a, b, c] });
    }
    Ok(m)
}

/// The bound `b`'s points, in the face's direction: its oriented edges'
/// polylines joined at their shared, bit-identical ends, the closing point
/// once.
fn walk(g: &Graph, b: Node, edges: &Polylines) -> Result<Vec<(EdgePoint, [f64; 3])>, Route> {
    let (oes, forward) = curve::face_bound(g, b).map_err(Route::Face)?;
    let mut out: Vec<(EdgePoint, [f64; 3])> = Vec::new();
    for oe in oes {
        let (edge, _) = curve::oriented_edge(g, oe).map_err(Route::Face)?;
        match edges.get(edge) {
            None => return Err(route("edge not built")),
            Some(Err(r)) => return Err(Route::Edge(r)),
            Some(Ok(_)) => {}
        }
        let mut pts = edges.oriented(g, oe).map_err(Route::Face)?;
        let first = pts.next().ok_or(route("loop:degenerate"))?;
        match out.last() {
            Some(&(_, p)) if p != first.1 => return Err(route("loop:open")),
            Some(_) => {}
            None => out.push(first),
        }
        out.extend(pts);
    }
    match (out.first(), out.last()) {
        (Some(a), Some(z)) if out.len() > 1 && a.1 == z.1 => {
            out.pop();
        }
        _ => return Err(route("loop:open")),
    }
    if !forward {
        out.reverse();
    }
    Ok(out)
}

/// Per face index of `cdt`, whether it is inside the loops: a flood from
/// the outer face, toggled by every constraint it crosses.
fn flood(cdt: &Cdt) -> Vec<bool> {
    let mut inside = vec![false; cdt.num_all_faces()];
    let mut seen = vec![false; cdt.num_all_faces()];
    // (the side crossed from is inside, the edge crossed, whose left face
    // is entered); from the outer face across the hull first.
    let mut next: Vec<_> = cdt.convex_hull().map(|e| (false, e.rev())).collect();
    while let Some((from, e)) = next.pop() {
        let Some(f) = e.face().as_inner() else {
            continue;
        };
        let k = f.fix().index();
        if seen[k] {
            continue;
        }
        seen[k] = true;
        inside[k] = from != e.is_constraint_edge();
        next.extend(f.adjacent_edges().map(|a| (inside[k], a.rev())));
    }
    inside
}

#[cfg(test)]
mod tests;
