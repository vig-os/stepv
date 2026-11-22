//! B-spline edges (ADR-0001 §2.6, M1b), from the maths: de Boor's
//! triangle for the blossom, de Casteljau halving (knot insertion on a
//! Bézier span) for the subdivision, and the convex-hull property, which
//! holds for a rational curve when every weight is > 0.
//!
//! **The range.** An `EDGE_CURVE` gives vertices, not parameters, so each
//! vertex is located on the curve ([`invert`]); one farther than d is
//! `vertex off curve`. The edge runs in the curve's direction from its start's
//! parameter to its end's (swapped when `same_sense` is `.F.`, then the points
//! reversed), through the curve's seam when the curve is closed (its ends
//! within d) and must. A closed edge sweeps the whole curve.
//!
//! **Pieces.** The range is cut at its C0 knots (interior multiplicity ≥ p)
//! and at the seam. Each piece is cut at its knots into Bézier spans, and a
//! span is halved until it passes [`flat`]. Every span end is a polyline
//! point; a piece's ends are exempt from the turn bound.

use super::{MAX_SEGMENTS, Polylines, Tolerance};
use crate::graph::Reason;
use crate::graph::curve::BSpline;

/// A homogeneous point, `(w·x, w·y, w·z, w)`: four lanes.
type H = [f64; 4];

/// The most halvings of one span. The segment count stays below
/// [`MAX_SEGMENTS`] anyway; this bounds the stack.
const MAX_DEPTH: u32 = 48;

/// The most spans [`invert`] examines for one vertex.
const MAX_NODES: usize = 1 << 16;

/// `(1 − t)·a + t·b`, each term scaled first: a sum of two coordinates
/// near f64::MAX would overflow.
fn mix(a: H, b: H, t: f64) -> H {
    [0, 1, 2, 3].map(|k| (1. - t) * a[k] + t * b[k])
}

/// Euclidean `h`.
fn euclid(h: H) -> [f64; 3] {
    [h[0] / h[3], h[1] / h[3], h[2] / h[3]]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// The distance from `p` to the segment `a`–`b`.
fn to_segment(p: [f64; 3], a: [f64; 3], b: [f64; 3]) -> f64 {
    let (ab, ap) = (sub(b, a), sub(p, a));
    let l = dot(ab, ab);
    let t = if l > 0. {
        (dot(ap, ab) / l).clamp(0., 1.)
    } else {
        0.
    };
    let q = [0, 1, 2].map(|i| ap[i] - t * ab[i]);
    dot(q, q).sqrt()
}

/// The blossom of `c` at `p − j` copies of `a` and `j` of `b`, all in the
/// non-empty knot span `[knots[i], knots[i + 1]]`: point j of the Bézier
/// span over `[a, b]`, by de Boor's triangle. Each step is a convex
/// combination, as `knots[k] ≤ a, b ≤ knots[k + p + 1 − r]`.
fn blossom(c: &BSpline, i: usize, a: f64, b: f64, j: usize, tmp: &mut Vec<H>) -> H {
    let (p, u) = (c.degree, &c.knots);
    tmp.clear();
    tmp.extend((i - p..=i).map(|k| {
        let w = c.w[k];
        [c.x[k] * w, c.y[k] * w, c.z[k] * w, w]
    }));
    for r in 1..=p {
        let t = if r <= p - j { a } else { b };
        for m in (r..=p).rev() {
            let k = i - p + m;
            let al = (t - u[k]) / (u[k + p + 1 - r] - u[k]);
            tmp[m] = mix(tmp[m - 1], tmp[m], al);
        }
    }
    tmp[p]
}

/// The Bézier span of `c` over `[a, b]` within knot span i, onto `out`.
fn bezier(c: &BSpline, i: usize, a: f64, b: f64, out: &mut Vec<H>, tmp: &mut Vec<H>) {
    for j in 0..=c.degree {
        let h = blossom(c, i, a, b, j, tmp);
        out.push(h);
    }
}

/// `c`'s point at `t` in its range.
fn at(c: &BSpline, t: f64) -> [f64; 3] {
    let (p, u) = (c.degree, &c.knots);
    let mut i = (u.partition_point(|&k| k <= t).max(1) - 1).clamp(p, u.len() - p - 2);
    while u[i] == u[i + 1] {
        i -= 1;
    }
    euclid(blossom(c, i, t, t, 0, &mut Vec::new()))
}

/// Puts the rational Bézier span `q` in standard form, its end weights
/// equal, by scaling weight j (and its point) by `c^j`, `c = (w_0/w_p)^(1/p)`:
/// a reparametrisation that changes neither the curve nor its control
/// points, but keeps a halving from landing where an extreme end weight
/// makes the curve stall. The largest weight is then scaled back to 1.
fn standard(q: &mut [H]) {
    let p = q.len() - 1;
    let c = (q[0][3] / q[p][3]).powf(1. / p as f64);
    let mut f = 1.;
    for h in q.iter_mut() {
        *h = h.map(|x| x * f);
        f *= c;
    }
    let top = q.iter().fold(0f64, |m, h| m.max(h[3]));
    if top.is_finite() && top > 0. {
        q.iter_mut().for_each(|h| *h = h.map(|x| x / top));
    }
}

/// Halves the Bézier span `q` at its middle parameter (de Casteljau),
/// into `left` and `right`. In place: after step r, `right[p − r]` is no
/// longer touched, so the triangle's last diagonal is the right half.
fn halve(q: &[H], left: &mut Vec<H>, right: &mut Vec<H>) {
    let p = q.len() - 1;
    right.clear();
    right.extend_from_slice(q);
    left.clear();
    left.push(q[0]);
    for r in 1..=p {
        for j in 0..=p - r {
            right[j] = mix(right[j], right[j + 1], 0.5);
        }
        left.push(right[0]);
    }
}

/// §2.6's test on one Bézier span: every control point within d of the
/// chord, and every two non-zero legs (of the Euclidean control points)
/// within θ of each other, `cos θ` given. `joint` tells which ends meet a
/// neighbouring span of the same piece. An end leg there is `zero tangent`,
/// a vanishing derivative and a possible cusp, when rounding could turn it
/// by over 10⁻³·θ: when it is no longer than 2⁻⁴⁶ of the span's largest
/// coordinate (about the rounding of 48 halvings) over 10⁻³·θ.
///
/// **Soundness.** The span lies in its control points' hull, which lies
/// within d of the chord (a convex set). The chord is the sum of the legs,
/// so within θ of each; so is the end tangent at a joint, the end leg,
/// which the neighbour shares as its own end leg (collinear by de
/// Casteljau, or C1 at a knot of multiplicity < p). Consecutive chords so
/// turn by at most 2θ = a. Weights > 0 keep the hull, and the tangent
/// direction of the end leg, for a rational span.
fn flat(q: &[H], d: f64, cos: f64, joint: [bool; 2], s: &mut Scratch) -> Result<bool, Reason> {
    let (pts, legs) = (&mut s.pts, &mut s.legs);
    pts.clear();
    pts.extend(q.iter().map(|&h| euclid(h)));
    let p = pts.len() - 1;
    legs.clear();
    legs.extend(
        pts.windows(2)
            .map(|w| sub(w[1], w[0]))
            .map(|l| (l, dot(l, l).sqrt())),
    );
    if !legs
        .iter()
        .all(|(l, n)| n.is_finite() && l.iter().all(|x| x.is_finite()))
    {
        return Err(Reason("non-finite point"));
    }
    let big = pts.iter().flatten().fold(0f64, |m, x| m.max(x.abs()));
    let tiny = big * f64::powi(2., -46) / (1e-3 * cos.acos());
    if (joint[0] && legs[0].1 <= tiny) || (joint[1] && legs[p - 1].1 <= tiny) {
        return Err(Reason("zero tangent"));
    }
    if !pts[1..p]
        .iter()
        .all(|&x| to_segment(x, pts[0], pts[p]) <= d)
    {
        return Ok(false);
    }
    legs.retain(|l| l.1 > 0.);
    for (i, (u, nu)) in legs.iter().enumerate() {
        for (v, nv) in &legs[i + 1..] {
            // Finite, checked above.
            if dot(*u, *v) < nu * nv * cos {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Buffers reused across spans: the subdivision allocates nothing per span.
#[derive(Default)]
struct Scratch {
    pts: Vec<[f64; 3]>,
    legs: Vec<([f64; 3], f64)>,
    q: Vec<H>,
    tmp: Vec<H>,
    left: Vec<H>,
    right: Vec<H>,
    /// The spans still to test, p + 1 points each, and their depth and joints.
    stack: Vec<H>,
    meta: Vec<(u32, [bool; 2])>,
}

/// The parameter of the point of `c` nearest `v`, and its distance, by
/// branch and bound over halved Bézier spans: its chord less its control
/// points' deviation from it bounds a span's distance from below, its end
/// points from above. A span is
/// refined until its box is within `eps`, or until it can't beat the best
/// by more than `eps`: the result is within `eps` of the nearest.
fn invert(c: &BSpline, v: [f64; 3], eps: f64) -> Result<(f64, f64), Reason> {
    let (p, u) = (c.degree, &c.knots);
    let (mut best, mut tmp, mut q) = ((f64::INFINITY, u[p]), Vec::new(), Vec::new());
    let mut stack: Vec<(f64, f64, u32, Vec<H>)> = Vec::new();
    for i in (p..u.len() - p - 1).filter(|&i| u[i] < u[i + 1]) {
        q.clear();
        bezier(c, i, u[i], u[i + 1], &mut q, &mut tmp);
        stack.push((u[i], u[i + 1], 0, q.clone()));
    }
    let mut nodes = 0;
    while let Some((lo, hi, depth, q)) = stack.pop() {
        nodes += 1;
        if nodes > MAX_NODES {
            return Err(Reason("vertex inversion"));
        }
        let pts: Vec<[f64; 3]> = q.iter().map(|&h| euclid(h)).collect();
        let dist = |a: [f64; 3]| norm(sub(a, v));
        for (t, e) in [(lo, pts[0]), (hi, pts[p])] {
            let e = dist(e);
            if !e.is_finite() {
                return Err(Reason("non-finite point"));
            }
            if e < best.0 {
                best = (e, t);
            }
        }
        // The span lies within `dev` of its chord, so no nearer to `v` than
        // the chord less `dev`: a bound that tightens with the square of
        // the span's length on a smooth curve, where a box's tightens
        // linearly.
        let dev = pts
            .iter()
            .map(|&x| to_segment(x, pts[0], pts[p]))
            .fold(0f64, f64::max);
        let low = to_segment(v, pts[0], pts[p]) - dev;
        // Within eps of the best, a span can't improve it by more than eps.
        if low >= best.0 - eps || norm(sub(pts[p], pts[0])) + 2. * dev <= eps || depth >= 60 {
            continue;
        }
        let (mut l, mut r) = (Vec::new(), Vec::new());
        halve(&q, &mut l, &mut r);
        let m = 0.5 * lo + 0.5 * hi;
        stack.push((m, hi, depth + 1, r));
        stack.push((lo, m, depth + 1, l));
    }
    Ok((best.1, best.0))
}

impl Polylines {
    /// Appends `q`, unless it repeats the last point since `start`.
    fn push_new(&mut self, start: usize, q: [f64; 3]) {
        let n = self.x.len();
        if n == start || [self.x[n - 1], self.y[n - 1], self.z[n - 1]] != q {
            self.push(q);
        }
    }

    /// Appends the edge over B-spline `c` from vertex point `ends[0]` to
    /// `ends[1]` (one vertex when `closed`) in the curve's direction or,
    /// when `!same_sense`, against it.
    pub(super) fn bspline(
        &mut self,
        c: &BSpline,
        ends: [[f64; 3]; 2],
        closed: bool,
        same_sense: bool,
        tol: Tolerance,
    ) -> Result<(), Reason> {
        let (p, u, d) = (c.degree, &c.knots, tol.linear);
        let [va, vb] = if same_sense { ends } else { [ends[1], ends[0]] };
        let (lo, hi) = (u[p], u[u.len() - p - 1]);
        let (ta, ea) = invert(c, va, d * 1e-6)?;
        let (tb, eb) = invert(c, vb, d * 1e-6)?;
        if !(ea <= d && eb <= d) {
            return Err(Reason("vertex off curve"));
        }
        let gap = sub(at(c, lo), at(c, hi));
        let seam = dot(gap, gap).sqrt() <= d;
        let ranges = match (closed, ta < tb, seam) {
            (true, _, true) => [(ta, hi), (lo, ta)],
            (false, true, _) => [(ta, tb), (hi, hi)],
            (false, false, true) => [(ta, hi), (lo, tb)],
            (true, _, false) => return Err(Reason("closed edge on an open curve")),
            (false, false, false) => return Err(Reason("vertex order")),
        };
        // `cos θ`, θ = a/2: at most π/2, where the cone of directions within
        // θ of a leg is still convex.
        let cos = (tol.angular.min(std::f64::consts::PI) / 2.).cos();
        let start = self.x.len();
        self.push(va);
        let mut s = Scratch::default();
        let mut segments = 0;
        for (a, b) in ranges.into_iter().filter(|(a, b)| a < b) {
            // The C0 knots inside: runs of at least p equal knots.
            let mut cuts = vec![a];
            cuts.extend(
                u.chunk_by(|x, y| x == y)
                    .filter(|k| k.len() >= p && k[0] > a && k[0] < b)
                    .map(|k| k[0]),
            );
            cuts.push(b);
            for w in cuts.windows(2) {
                let (a, b) = (w[0], w[1]);
                for i in
                    (p..u.len() - p - 1).filter(|&i| u[i] < u[i + 1] && u[i] < b && u[i + 1] > a)
                {
                    let (lo, hi) = (u[i].max(a), u[i + 1].min(b));
                    bezier(c, i, lo, hi, &mut s.stack, &mut s.tmp);
                    s.meta.push((0, [lo > a, hi < b]));
                    while let Some((depth, joint)) = s.meta.pop() {
                        let at = s.meta.len() * (p + 1);
                        s.q.clear();
                        s.q.extend(s.stack.drain(at..));
                        let mut q = std::mem::take(&mut s.q);
                        standard(&mut q);
                        let ok = flat(&q, d, cos, joint, &mut s);
                        s.q = q;
                        if ok? {
                            segments += 1;
                            if segments > MAX_SEGMENTS {
                                return Err(Reason("too many segments"));
                            }
                            let end = s.pts[p];
                            self.push_new(start, end);
                            continue;
                        }
                        if depth >= MAX_DEPTH {
                            return Err(Reason("too many segments"));
                        }
                        halve(&s.q, &mut s.left, &mut s.right);
                        s.stack.extend_from_slice(&s.right);
                        s.meta.push((depth + 1, [true, joint[1]]));
                        s.stack.extend_from_slice(&s.left);
                        s.meta.push((depth + 1, [joint[0], true]));
                    }
                }
            }
        }
        // The last point is the curve's end: the vertex point replaces it.
        if self.x.len() - start >= 2 {
            self.x.pop();
            self.y.pop();
            self.z.pop();
        }
        self.push(vb);
        if !same_sense {
            self.x[start..].reverse();
            self.y[start..].reverse();
            self.z[start..].reverse();
        }
        Ok(())
    }
}
