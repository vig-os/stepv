//! Edge polylines (ADR-0001 §2.6, M1a): one canonical polyline per
//! `EDGE_CURVE`, built once and shared by every face that uses it.
//!
//! A polyline runs from the edge's start vertex to its end vertex, whatever
//! its curve's direction (`same_sense`), and its first and last points are
//! the vertex points, bit for bit. Interior points lie on the exact curve. A
//! face walks it through an `ORIENTED_EDGE` ([`Polylines::oriented`]), in
//! the oriented edge's direction, and refers to its points as
//! [`EdgePoint`]s: two faces sharing an edge get the same points in
//! opposite order.
//!
//! **Curves:** `LINE`, `CIRCLE` and `ELLIPSE`, directly or as a
//! `SURFACE_CURVE`'s or `SEAM_CURVE`'s 3-D curve. Any other curve is routed
//! with its reason, never dropped.
//!
//! **The quality contract**, with d the linear and a the angular deflection
//! of a [`Tolerance`]: every chord lies within d of its curve, and
//! consecutive segments turn by at most a.
//! - A line gets no interior points.
//! - A circle arc of radius r and sweep θ takes
//!   `n = max(n_min, ⌈max(θ/a, θ/(2·acos(1 − min(d/r, 1))))⌉)` equal
//!   segments, with `n_min` 1 for an open arc and 3 for a closed edge. A
//!   segment's sag is `r·(1 − cos(θ/2n)) ≤ d`, and it turns by `θ/n ≤ a`.
//!   The clamp keeps `acos` defined when d > r.
//! - An ellipse with semi-axes A ≥ B takes equal parameter steps
//!   `Δt ≤ min(a·B/A, √(8d/A))`, and the same `n_min`. The tangent turns at
//!   most A/B times faster than t, and a chord over Δt sags at most
//!   `Δt²/8 · max|C''| = Δt²·A/8`.
//!
//! **The sweep:** a vertex's parameter is its angle in the conic's frame,
//! `atan2(y/B, x/A)`. The edge runs from its start's parameter to its end's,
//! increasing when `same_sense` and decreasing otherwise, through the seam
//! at 0/2π when it must. An edge whose two vertices are one vertex, or give
//! the same parameter, sweeps the whole conic.
//!
//! The points are a struct of arrays, so a later slice can add rayon and
//! SIMD (M4a) without changing the layout.

use std::collections::HashMap;
use std::f64::consts::TAU;

use crate::Deflection;
use crate::graph::curve::{self, Conic};
use crate::graph::{Graph, Node, Reason};

/// The most segments one edge may take. An edge that needs more (a
/// needle-thin ellipse against a fine deflection) is routed with
/// `too many segments` instead of exhausting memory.
pub const MAX_SEGMENTS: usize = 1 << 20;

/// The deflection a polyline honours: d in mm and a in radians, both finite
/// and positive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerance {
    linear: f64,
    angular: f64,
}

impl Tolerance {
    /// The kernel's numbers for a model of bounding-box diagonal `diagonal`
    /// mm: `d = deflection.linear_abs(diagonal)`, `a = angular_deg`. `None`
    /// unless both are finite and positive.
    #[must_use]
    pub fn new(deflection: Deflection, diagonal: f64) -> Option<Self> {
        Self::from_mm(
            deflection.linear_abs(diagonal),
            deflection.angular_deg.to_radians(),
        )
    }

    /// d = `linear` mm and a = `angular` radians. `None` unless both are
    /// finite and positive.
    #[must_use]
    pub fn from_mm(linear: f64, angular: f64) -> Option<Self> {
        let ok = |x: f64| x.is_finite() && x > 0.;
        (ok(linear) && ok(angular)).then_some(Self { linear, angular })
    }

    /// d, the chordal deviation bound, in mm.
    #[must_use]
    pub fn linear(self) -> f64 {
        self.linear
    }

    /// a, the turning-angle bound, in radians.
    #[must_use]
    pub fn angular(self) -> f64 {
        self.angular
    }
}

/// A point of an edge's canonical polyline, as a face boundary refers to
/// it: point `index` of `edge`'s polyline, counted from its start vertex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EdgePoint {
    /// The `EDGE_CURVE`.
    pub edge: Node,
    pub index: u32,
}

/// One edge's polyline, in mm: at least two points, from its start vertex
/// to its end vertex.
#[derive(Debug, Clone, Copy)]
pub struct Polyline<'a> {
    x: &'a [f64],
    y: &'a [f64],
    z: &'a [f64],
}

impl<'a> Polyline<'a> {
    /// Its number of points.
    #[must_use]
    pub fn len(&self) -> usize {
        self.x.len()
    }

    /// Always false: a polyline has at least two points.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.x.is_empty()
    }

    /// Point `i`.
    ///
    /// # Panics
    /// When `i` is out of range.
    #[must_use]
    pub fn point(&self, i: usize) -> [f64; 3] {
        [self.x[i], self.y[i], self.z[i]]
    }

    /// Its points, in order.
    pub fn points(&self) -> impl ExactSizeIterator<Item = [f64; 3]> + 'a {
        let (x, y, z) = (self.x, self.y, self.z);
        (0..x.len()).map(move |i| [x[i], y[i], z[i]])
    }

    /// Its coordinates, one slice per axis.
    #[must_use]
    pub fn coords(&self) -> [&'a [f64]; 3] {
        [self.x, self.y, self.z]
    }
}

/// Every edge's canonical polyline ([`Polylines::build`]), or why it has
/// none. Owned data, no borrow of the graph.
#[derive(Debug, Clone, Default)]
pub struct Polylines {
    x: Vec<f64>,
    y: Vec<f64>,
    z: Vec<f64>,
    spans: HashMap<Node, Result<(usize, usize), Reason>>,
}

impl Polylines {
    /// The polylines of `edges` (`EDGE_CURVE`s; each built once, however
    /// often it is listed), in mm at `mm_per_unit` (their representation's
    /// unit, as [`crate::graph::Product::mm_per_unit`]).
    #[must_use]
    pub fn build(
        g: &Graph,
        edges: impl IntoIterator<Item = Node>,
        mm_per_unit: f64,
        tol: Tolerance,
    ) -> Self {
        let mut p = Self::default();
        for e in edges {
            if p.spans.contains_key(&e) {
                continue;
            }
            let start = p.x.len();
            let r = if mm_per_unit.is_finite() && mm_per_unit > 0. {
                p.flatten(g, e, mm_per_unit, tol)
            } else {
                Err(Reason("non-finite unit"))
            };
            if r.is_err() {
                p.x.truncate(start);
                p.y.truncate(start);
                p.z.truncate(start);
            }
            p.spans.insert(e, r.map(|()| (start, p.x.len())));
        }
        p
    }

    /// `edge`'s polyline or why it has none; `None` when it wasn't built.
    #[must_use]
    pub fn get(&self, edge: Node) -> Option<Result<Polyline<'_>, Reason>> {
        let span = *self.spans.get(&edge)?;
        Some(span.map(|(a, b)| Polyline {
            x: &self.x[a..b],
            y: &self.y[a..b],
            z: &self.z[a..b],
        }))
    }

    /// The `ORIENTED_EDGE` `oe`'s points, walked in its direction: its edge's
    /// polyline, reversed when the orientation is `.F.`.
    ///
    /// # Errors
    /// A malformed oriented edge, an edge that wasn't built (`edge not
    /// built`), or the reason its edge has no polyline.
    pub fn oriented<'s>(
        &'s self,
        g: &Graph,
        oe: Node,
    ) -> Result<impl ExactSizeIterator<Item = (EdgePoint, [f64; 3])> + 's, Reason> {
        let (edge, forward) = curve::oriented_edge(g, oe)?;
        let p = self.get(edge).ok_or(Reason("edge not built"))??;
        let n = p.len();
        Ok((0..n).map(move |k| {
            let i = if forward { k } else { n - 1 - k };
            let index = u32::try_from(i).expect("at most MAX_SEGMENTS + 1 points");
            (EdgePoint { edge, index }, p.point(i))
        }))
    }

    /// The edges with a polyline.
    pub fn edges(&self) -> impl Iterator<Item = (Node, Polyline<'_>)> + '_ {
        self.spans
            .keys()
            .filter_map(|&e| Some((e, self.get(e)?.ok()?)))
    }

    /// The edges without one, and why.
    pub fn routed(&self) -> impl Iterator<Item = (Node, Reason)> + '_ {
        self.spans
            .iter()
            .filter_map(|(&e, r)| r.err().map(|r| (e, r)))
    }

    /// The number of points over every polyline.
    #[must_use]
    pub fn point_count(&self) -> usize {
        self.x.len()
    }

    fn push(&mut self, p: [f64; 3]) {
        self.x.push(p[0]);
        self.y.push(p[1]);
        self.z.push(p[2]);
    }

    /// Appends `e`'s polyline at `s` mm per unit.
    fn flatten(&mut self, g: &Graph, e: Node, s: f64, tol: Tolerance) -> Result<(), Reason> {
        let ec = curve::edge_curve(g, e)?;
        let p0 = curve::vertex(g, ec.start, s)?;
        let p1 = curve::vertex(g, ec.end, s)?;
        let c = curve::basis(g, ec.geometry)?;
        if g.is_a(c, "LINE") {
            self.push(p0);
            self.push(p1);
            return Ok(());
        }
        let k = curve::conic(g, c, s)?;
        let (big, small) = (k.a.max(k.b), k.a.min(k.b));
        // Finite: `curve::conic` refuses a non-finite radius.
        if small <= 0. {
            return Err(Reason("degenerate conic"));
        }
        let t0 = param(&k, p0);
        let dir = if ec.same_sense { 1. } else { -1. };
        let mut sweep = (dir * (param(&k, p1) - t0)).rem_euclid(TAU);
        let closed = ec.start == ec.end || sweep == 0.;
        if closed {
            sweep = TAU;
        }
        let (d, a) = (tol.linear, tol.angular);
        let steps = if g.is_a(c, "CIRCLE") {
            let chord = 2. * (1. - (d / k.a).min(1.)).acos();
            (sweep / a).max(sweep / chord)
        } else {
            sweep / (a * small / big).min((8. * d / big).sqrt())
        };
        let n = segments(steps, closed)?;
        let step = dir * sweep / n as f64;
        self.x.reserve(n + 1);
        self.y.reserve(n + 1);
        self.z.reserve(n + 1);
        self.push(p0);
        for i in 1..n {
            let t = t0 + step * i as f64;
            let (sin, cos) = t.sin_cos();
            let (u, v) = (k.a * cos, k.b * sin);
            self.x.push(k.c[0] + u * k.x[0] + v * k.y[0]);
            self.y.push(k.c[1] + u * k.x[1] + v * k.y[1]);
            self.z.push(k.c[2] + u * k.x[2] + v * k.y[2]);
        }
        self.push(p1);
        Ok(())
    }
}

/// `max(n_min, ⌈steps⌉)`, with `n_min` 3 on a closed edge and 1 otherwise.
fn segments(steps: f64, closed: bool) -> Result<usize, Reason> {
    let min = if closed { 3. } else { 1. };
    let n = steps.ceil().max(min);
    if n.is_finite() && n <= MAX_SEGMENTS as f64 {
        // Finite, positive and at most 2^20: exact as an integer.
        Ok(n as usize)
    } else {
        Err(Reason("too many segments"))
    }
}

/// The parameter of `p`, on or near the conic `k`: its angle in the conic's
/// frame with the axes scaled to a unit circle, in (−π, π].
fn param(k: &Conic, p: [f64; 3]) -> f64 {
    let u = [p[0] - k.c[0], p[1] - k.c[1], p[2] - k.c[2]];
    let dot = |w: [f64; 3]| u[0] * w[0] + u[1] * w[1] + u[2] * w[2];
    (dot(k.y) / k.b).atan2(dot(k.x) / k.a)
}

#[cfg(test)]
mod tests;
