//! What planar (M2, [`super::plane`]) and analytic (M3, [`super::analytic`])
//! faces share (ADR-0001 §2.6): the representation context, the mesh, the
//! route, and the triangulation of a face's boundary in its surface's
//! (u, v).
//!
//! **Boundary.** Each bound's `EDGE_LOOP` is walked through its oriented
//! edges' shared polylines ([`Polylines::oriented`]), never resampled, and
//! reversed when the bound's `orientation` is `.F.`. Consecutive edges must
//! meet at a bit-identical point, and the loop must close (`loop:open`). The
//! surface maps each point to (u, v); a point may appear at two (u, v), a
//! seam's two sides or a pole's two ends, with one 3-D point.
//!
//! **Triangulation.** `spade`'s constrained Delaunay triangulation, with
//! exact predicates, of every loop point, then every loop segment as a
//! constraint. A constraint that would cross another (`rs-fail:cdt-crossing`),
//! pass through a vertex, which would split it (`rs-fail:cdt-touching`), or
//! repeat one (`rs-fail:loops-overlap`) routes the face: a constraint is
//! never split, and no boundary point is moved or added. Steiner points
//! (M3) are added afterwards, only strictly inside a triangle inside the
//! face, so never on a constraint.
//!
//! **Flood.** The triangles inside the face are found by a flood from the
//! outside, crossing a constraint toggling inside and outside. So the flood
//! needs no outer loop: a face with only `FACE_BOUND`s meshes like one with a
//! `FACE_OUTER_BOUND`, and its holes are what the loops enclose.
//!
//! **Orientation.** ISO 10303-42 puts the face to the left of each bound,
//! walked in its oriented direction and seen from the face's topological
//! normal: the surface's normal when `same_sense`, else its opposite. Every
//! surface here has (u, v) right-handed about its normal, so every segment
//! is checked against the flood: the triangle to its left in (u, v) (right
//! when not `same_sense`) must be inside (`rs-fail:orientation`). This is
//! exact, as both are combinatorial.
//!
//! A virtual seam ([`super::analytic`]) is a run of constraints like any
//! other; one that would cross or touch another is routed
//! `loop:seam-crosses`.
//!
//! **Degenerate loops** route too: fewer than three distinct points or a
//! zero-length segment (`loop:degenerate`), a loop through one (u, v) twice
//! (`loop:self-touching`), and two different points at one (u, v)
//! (`rs-fail:coincident`). Two loops may share a vertex.
//!
//! **The uncertainty floor (#210).** Every boundary point is checked against
//! its surface within the context's length uncertainty, at least
//! [`FLOOR_MM`]: see [`Context::new`].

use std::fmt;

use spade::handles::FixedVertexHandle;
use spade::{ConstrainedDelaunayTriangulation, Point2, PositionInTriangulation, Triangulation};

use super::edges::{EdgePoint, Polylines};
use crate::graph::{Graph, Node, Reason, curve, length_uncertainty};

/// The least length uncertainty the meshers use, in mm: OCCT's confusion
/// tolerance (`Precision::Confusion()`), so Rust and the golden oracle
/// agree on what lies on a surface (owner decision #210, ADR §2.6).
pub const FLOOR_MM: f64 = 1e-7;

/// The reason recorded for every face whose context was raised to
/// [`FLOOR_MM`].
pub const FLOORED: Reason = Reason("uncertainty:floored");

/// The most Steiner points one face may take; more is routed (`too many
/// points`) rather than exhausting memory.
pub const MAX_STEINER: usize = 1 << 22;

/// A face's representation context, as the face needs it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Context {
    /// Millimetres per length unit ([`crate::graph::Product::mm_per_unit`]).
    pub mm_per_unit: f64,
    /// Radians per plane angle unit (a cone's semi-angle is in it).
    pub rad_per_unit: f64,
    /// The length uncertainty in mm, at least [`FLOOR_MM`].
    pub uncertainty: f64,
    /// Whether the declared uncertainty was below [`FLOOR_MM`], or absent,
    /// and raised to it ([`FLOORED`]).
    pub floored: bool,
}

impl Context {
    /// The context with `declared` (mm) as its uncertainty, raised to
    /// [`FLOOR_MM`] when it is smaller, absent or not finite (#210): ABC's
    /// files declare 1e-17 mm, which neither f64 coordinates nor their own
    /// geometry meet.
    #[must_use]
    pub fn new(mm_per_unit: f64, rad_per_unit: f64, declared: Option<f64>) -> Self {
        let kept = declared.filter(|u| u.is_finite() && *u >= FLOOR_MM);
        Self {
            mm_per_unit,
            rad_per_unit,
            uncertainty: kept.unwrap_or(FLOOR_MM),
            floored: kept.is_none(),
        }
    }

    /// The context of the shape representation `rep`: its units and its
    /// declared length uncertainty, floored ([`Context::new`]). The one
    /// place a mesh context is built from the graph.
    #[must_use]
    pub fn of(g: &Graph, rep: Node) -> Self {
        let mm = curve::mm_per_unit(g, rep);
        let declared = length_uncertainty(g, rep).map(|u| u * mm);
        Self::new(mm, curve::rad_per_unit(g, rep), declared)
    }

    /// [`FLOORED`] when the uncertainty was raised, for the ledger.
    #[must_use]
    pub fn reason(self) -> Option<Reason> {
        self.floored.then_some(FLOORED)
    }
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

/// A face's mesh in mm, as a struct of arrays.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FaceMesh {
    /// The boundary vertices: vertex `i < points.len()` is `points[i]`'s
    /// edge point, bit for bit. A seam's or a pole's point is several
    /// vertices with one 3-D point. The vertices after them lie on the
    /// surface: a virtual seam's points, an apex that is no edge's, and
    /// Steiner points.
    pub points: Vec<EdgePoint>,
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub z: Vec<f64>,
    /// Each vertex's outward unit normal, exact from its surface.
    pub nx: Vec<f64>,
    pub ny: Vec<f64>,
    pub nz: Vec<f64>,
    /// Vertex indices, counter-clockwise about the normals.
    pub triangles: Vec<[u32; 3]>,
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

    /// Vertex `i`'s normal.
    ///
    /// # Panics
    /// When `i` is out of range.
    #[must_use]
    pub fn normal(&self, i: usize) -> [f64; 3] {
        [self.nx[i], self.ny[i], self.nz[i]]
    }

    fn push(&mut self, p: [f64; 3]) {
        self.x.push(p[0]);
        self.y.push(p[1]);
        self.z.push(p[2]);
    }

    /// Sets every vertex's normal from `n`, negated when `flip`.
    pub(super) fn normals(&mut self, flip: bool, n: impl Fn(usize) -> [f64; 3]) {
        let s = if flip { -1. } else { 1. };
        for i in 0..self.x.len() {
            let v = n(i);
            self.nx.push(s * v[0]);
            self.ny.push(s * v[1]);
            self.nz.push(s * v[2]);
        }
    }
}

pub(super) fn route(r: &'static str) -> Route {
    Route::Face(Reason(r))
}

/// A boundary point as the triangulation takes it: its edge point (none on
/// a virtual seam or at an apex that is no edge's), its 3-D point, its
/// (u, v) in the triangulation's coordinates, whether it is on a pole's line
/// (two such at one 3-D point bound a degenerate triangle, which is
/// dropped), and whether the segment from it is a virtual seam's.
#[derive(Debug, Clone, Copy)]
pub(super) struct Vertex {
    pub ep: Option<EdgePoint>,
    pub p: [f64; 3],
    pub uv: [f64; 2],
    pub pole: bool,
    pub cut: bool,
}

type Cdt = ConstrainedDelaunayTriangulation<Point2<f64>>;

/// A face's triangulation in (u, v), its boundary checked (module docs).
pub(super) struct Tri {
    cdt: Cdt,
    /// Per spade vertex, ours.
    ours: Vec<u32>,
    /// Per face of `cdt`, whether it is inside the face.
    inside: Vec<bool>,
    /// The mesh, without normals or triangles yet.
    pub mesh: FaceMesh,
    /// Per vertex of ours, its (u, v) and whether it is a pole's end.
    pub uv: Vec<[f64; 2]>,
    pole: Vec<bool>,
}

/// Triangulates `loops` (module docs), whose face lies left of each when
/// `same_sense`, else right.
pub(super) fn triangulate(loops: &[Vec<Vertex>], same_sense: bool) -> Result<Tri, Route> {
    let mut t = Tri {
        cdt: Cdt::new(),
        ours: Vec::new(),
        inside: Vec::new(),
        mesh: FaceMesh::default(),
        uv: Vec::new(),
        pole: Vec::new(),
    };
    // Per vertex of ours, its loop.
    let mut owner = Vec::<usize>::new();
    let mut found: Vec<Vec<Option<FixedVertexHandle>>> =
        loops.iter().map(|lp| vec![None; lp.len()]).collect();
    // Edge points first, so they are vertices `0..points.len()`.
    for edge_points in [true, false] {
        for (l, lp) in loops.iter().enumerate() {
            for (k, v) in lp.iter().enumerate() {
                if v.ep.is_some() != edge_points {
                    continue;
                }
                let n = t.uv.len();
                let h = t.insert(v.uv)?;
                if t.uv.len() > n {
                    t.mesh.points.extend(v.ep);
                    t.mesh.push(v.p);
                    t.pole.push(v.pole);
                    owner.push(l);
                } else {
                    let i = t.ours[h.index()] as usize;
                    if v.cut && v.ep.is_none() {
                        return Err(route("loop:seam-crosses"));
                    }
                    if t.mesh.point(i) != v.p {
                        return Err(route("rs-fail:coincident"));
                    }
                    if k > 0 && found[l][k - 1] == Some(h) {
                        return Err(route("loop:degenerate"));
                    }
                    if owner[i] == l {
                        return Err(route("loop:self-touching"));
                    }
                }
                found[l][k] = Some(h);
            }
        }
    }
    let handles: Vec<Vec<FixedVertexHandle>> = found
        .into_iter()
        .map(|hs| hs.into_iter().flatten().collect())
        .collect();
    if handles.is_empty() || handles.iter().any(|hs| hs.len() < 3) {
        return Err(route("loop:degenerate"));
    }
    // Every vertex first, so no later point can land on a constraint. A
    // segment that fails from a virtual seam's vertex is the seam's.
    let segments = || {
        handles.iter().zip(loops).flat_map(|(hs, lp)| {
            (0..hs.len()).map(move |k| (hs[k], hs[(k + 1) % hs.len()], lp[k].cut))
        })
    };
    for (a, b, cut) in segments() {
        let fail = |r: &'static str| Err(route(if cut { "loop:seam-crosses" } else { r }));
        if t.cdt.exists_constraint(a, b) {
            return fail("rs-fail:loops-overlap");
        }
        let made = t.cdt.try_add_constraint(a, b);
        // A touched vertex that is no edge point is a virtual seam's.
        let seam = made.iter().any(|&e| {
            let v = t.cdt.directed_edge(e).from().fix();
            t.ours[v.index()] as usize >= t.mesh.points.len()
        });
        match made.len() {
            0 => return fail("rs-fail:cdt-crossing"),
            1 => {}
            _ if seam => return Err(route("loop:seam-crosses")),
            _ => return fail("rs-fail:cdt-touching"),
        }
    }
    t.inside = flood(&t.cdt);
    for (a, b, _) in segments() {
        let e = t
            .cdt
            .get_edge_from_neighbors(a, b)
            .ok_or(route("rs-fail:orientation"))?;
        let side = if same_sense { e.face() } else { e.rev().face() };
        if side.is_outer() || !t.inside[side.fix().index()] {
            return Err(route("rs-fail:orientation"));
        }
    }
    Ok(t)
}

impl Tri {
    /// Inserts `uv` as a vertex, ours or one already there.
    fn insert(&mut self, uv: [f64; 2]) -> Result<FixedVertexHandle, Route> {
        let h = self
            .cdt
            .insert(spade::mitigate_underflow(Point2::new(uv[0], uv[1])))
            .map_err(|_| route("rs-fail:coordinate"))?;
        if h.index() == self.ours.len() {
            let i = u32::try_from(self.uv.len()).map_err(|_| route("too many points"))?;
            self.ours.push(i);
            self.uv.push(uv);
        }
        Ok(h)
    }

    /// Adds each of `grid` that lies strictly inside a triangle inside the
    /// face, or on an edge between two such that is no constraint, as a
    /// Steiner point at 3-D point `at(uv)`: never on a constraint, so no
    /// boundary segment is split.
    pub(super) fn steiner(
        &mut self,
        grid: impl Iterator<Item = [f64; 2]>,
        at: impl Fn([f64; 2]) -> [f64; 3],
    ) -> Result<(), Route> {
        let keep: Vec<[f64; 2]> = grid
            .filter(|q| {
                let p = Point2::new(q[0], q[1]);
                let inside = |f: spade::handles::FaceHandle<'_, _, _, _, _, _>| {
                    f.as_inner().is_some_and(|f| self.inside[f.fix().index()])
                };
                match self.cdt.locate(p) {
                    PositionInTriangulation::OnFace(f) => self.inside[f.index()],
                    PositionInTriangulation::OnEdge(e) => {
                        let e = self.cdt.directed_edge(e);
                        !e.is_constraint_edge() && inside(e.face()) && inside(e.rev().face())
                    }
                    _ => false,
                }
            })
            .collect();
        for q in keep {
            let n = self.uv.len();
            self.insert(q)?;
            if self.uv.len() > n {
                self.mesh.push(at(q));
                self.pole.push(false);
            }
        }
        self.inside = flood(&self.cdt);
        Ok(())
    }

    /// The mesh, and every vertex's (u, v) and whether it is a pole's: the
    /// triangles inside the face,
    /// counter-clockwise in (u, v), reversed when `flip`; a pole's
    /// degenerate triangles (two ends of one pole) left out. Normals are the
    /// caller's ([`FaceMesh::normals`]).
    pub(super) fn into_mesh(self, flip: bool) -> (FaceMesh, Vec<[f64; 2]>, Vec<bool>) {
        let triangles = self.triangles();
        let mut m = self.mesh;
        m.triangles = triangles
            .into_iter()
            .map(|[a, b, c]| if flip { [a, c, b] } else { [a, b, c] })
            .collect();
        (m, self.uv, self.pole)
    }

    /// The triangles inside the face, counter-clockwise in (u, v), a pole's
    /// degenerate ones left out.
    pub(super) fn triangles(&self) -> Vec<[u32; 3]> {
        let pole_pair = |a: u32, b: u32| {
            let (a, b) = (a as usize, b as usize);
            self.pole[a] && self.pole[b] && self.mesh.point(a) == self.mesh.point(b)
        };
        self.cdt
            .inner_faces()
            .filter(|f| self.inside[f.fix().index()])
            .map(|f| f.vertices().map(|v| self.ours[v.fix().index()]))
            .filter(|&[a, b, c]| !(pole_pair(a, b) || pole_pair(b, c) || pole_pair(c, a)))
            .collect()
    }
}

/// The bound `b`'s points, in the face's direction: its oriented edges'
/// polylines joined at their shared, bit-identical ends, the closing point
/// once.
pub(super) fn walk(
    g: &Graph,
    b: Node,
    edges: &Polylines,
) -> Result<Vec<(EdgePoint, [f64; 3])>, Route> {
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
