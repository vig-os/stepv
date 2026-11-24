//! Curved analytic faces (ADR-0001 §2.6, M3a): an `ADVANCED_FACE` on a
//! `CYLINDRICAL_SURFACE` or `CONICAL_SURFACE`, as triangles whose boundary
//! vertices are exactly its edges' canonical polylines ([`super::edges`]),
//! plus points on the surface. The boundary walk, triangulation, flood,
//! orientation check and routes are [`super::face`]'s, shared with
//! [`super::plane`]. Spheres and tori are M3b (#212).
//!
//! **Surfaces** (ISO 10303-42 §4.4), in the surface's frame (origin c, unit
//! axes x, y, z) with `e(u) = cos u·x + sin u·y`, as `c + ρ(v)·e(u) + v·z`:
//! a cylinder of radius r has `ρ = r`, a cone of radius r and semi-angle α
//! `ρ = r + v·tan α`. Both have (u, v) right-handed about their outward
//! normal `n = cos β·e(u) − sin β·z` (β = 0 on the cylinder, α on the
//! cone), which is constant along each ruling.
//!
//! **Inversion after the on-surface check.** With `w = p − c`, ρ_p the
//! distance of p from the axis and `h = w·z`, a boundary point's distance
//! from the surface, `|ρ_p − r|` or `|ρ_p − (r + h·tan α)|·cos α` (on the
//! cone's nappe `r + h·tan α ≥ 0`), must be within the context's uncertainty
//! plus f64's rounding of it, `8ε·(|p| + |c| + r)`, else the face is routed
//! (`curve:off-surface`). Then `u = atan2(w·y, w·x)` and `v = h`.
//!
//! **Poles.** A point within that tolerance of the axis is the cone's apex,
//! at `v = −r/tan α`, where u is undefined. A pole is a run of vertices along
//! its line in (u, v), one 3-D point: from the u the loop arrives with to
//! the u it leaves with, a vertex every grid step, so no triangle fanning
//! from it spans more than a step. Walked with the face on its left, a loop
//! crosses the apex towards +u (−u when not `same_sense`), by more than 0 and
//! at most 2π: a full turn when it leaves along the edge it came by, as on a
//! seam edge to the apex. The pole line's triangles are degenerate and
//! dropped. Each apex vertex's normal is exact at its own u: its ruling's.
//!
//! **u unwrapped; seams.** Along each loop, u is unwrapped: each point takes
//! the branch nearest its predecessor's. So a seam edge, used twice with
//! opposite senses, becomes the two sides of the cut, 2π apart, with one
//! 3-D point on both. A loop that comes back to its start a turn away wraps
//! the axis.
//!
//! **Virtual seam** (owner decision, ADR §2.6). Wrapping loops without a
//! seam edge, the corpus norm, are cut open by a constraint polyline that
//! adds no point on any edge, so the face stays watertight with its
//! neighbours. From A, the first point of one wrapping loop, it runs
//! straight in (u, v) to B, the point of the other wrapping loop whose u is
//! nearest u_A (|u_B − u_A| ≤ π), or to the apex: a `VERTEX_LOOP`'s, or,
//! with no partner loop, the cone's own. Its interior points lie on the
//! surface at the grid step. It is used twice, at u and a turn on, so the
//! loops and the seam's two copies bound one simply connected domain: the
//! first loop from A to A + 2π, the seam to B + 2π, the second loop back to
//! B, the seam back to A. A seam that would cross or touch a loop is routed
//! (`loop:seam-crosses`); any other set of wrapping loops (three, or two
//! turning alike) is routed `loop:seam-unresolved`.
//!
//! **Steiner grid.** The triangulation runs in scaled coordinates
//! `(u·s_u, v·s_v)`: s_u the largest radius on the boundary, s_v the length
//! per unit v along a ruling (1, or 1/cos α). The curved direction u takes
//! half the circle step of that radius, `min(a, 2·acos(1 − min(d/ρ, 1)))`:
//! the pole lines' vertices and a virtual seam's interior points are spaced
//! by it. The straight direction v takes no row: the bound depends on a
//! triangle's u span alone (below), and grid points beside a ruling edge
//! into the apex make folds, triangles (apex, two points of one ruling) that
//! the empty-circle test in (u, v) accepts. Instead, where a triangle breaks
//! the bound, its (u, v) centroid is added: strictly inside a triangle
//! inside the face, so never on a constraint. Up to 16 rounds; a face still
//! beyond the bound is routed (`rs-fail:deflection`).
//!
//! **The deflection bound, checked.** Every triangle is checked, and the
//! face is routed (`rs-fail:deflection`) unless, with 1e-9 relative allowed
//! for rounding, the angle between any two of its vertices' normals is at
//! most a, and ADR §2.6's closed-form bound on its distance from the
//! surface, `ρ_max·(1 − cos(Δu/2))` with Δu ≤ π its u span, is at most d.
//! The surfaces are developable, so that bound depends on Δu alone: a
//! triangle on a boundary chord sags no more than a chord over its span.
//!
//! A triangle at the apex whose 3-D area is under 1e-9 of its two edges'
//! product is a fold, and routes the face (`rs-fail:degenerate triangle`):
//! a guard, since without points along the rulings none is made (#213).
//!
//! **Normals** are exact from the surface at each vertex's (u, v), flipped
//! by `same_sense` and again when the face is reversed in its shell.
//! Triangles wind counter-clockwise about them. Every coordinate is finite,
//! or the face is routed (`non-finite point`).

use std::f64::consts::{PI, TAU};

use super::edges::{EdgePoint, Polylines, Tolerance};
use super::face::{Context, FaceMesh, MAX_STEINER, Route, Vertex, route, triangulate, walk};
use crate::graph::curve::{self, Elementary};
use crate::graph::{Graph, Node};

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Relative allowance for rounding in the deflection check.
const ROUNDING: f64 = 1e-9;

/// The most rounds of centroid refinement (module docs).
const ROUNDS: usize = 16;

/// A cylinder or cone in mm (module docs).
struct Surface {
    c: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    z: [f64; 3],
    r: f64,
    shape: Elementary,
}

/// A boundary point inverted: its u (raw, in (−π, π]) and v, and its pole:
/// −1 at the apex, 0 elsewhere.
struct Inv {
    u: f64,
    v: f64,
    pole: f64,
}

/// A loop in unscaled (u, v), and its net turns about the axis.
struct Loop {
    vs: Vec<Vertex>,
    turns: f64,
}

fn shift(v: Vertex, du: f64, cut: bool) -> Vertex {
    Vertex {
        uv: [v.uv[0] + du, v.uv[1]],
        cut,
        ..v
    }
}

impl Surface {
    /// ρ(v), the radius at v.
    fn radius(&self, v: f64) -> f64 {
        match self.shape {
            Elementary::Cylinder => self.r,
            Elementary::Cone(t) => self.r + v * t.tan(),
        }
    }

    fn at(&self, [u, v]: [f64; 2]) -> [f64; 3] {
        let rho = self.radius(v);
        let (s, c) = u.sin_cos();
        [0, 1, 2].map(|k| self.c[k] + rho * (c * self.x[k] + s * self.y[k]) + v * self.z[k])
    }

    /// The outward unit normal at (u, v).
    fn normal(&self, [u, _]: [f64; 2]) -> [f64; 3] {
        let (a, b) = match self.shape {
            Elementary::Cylinder => (1., 0.),
            Elementary::Cone(t) => (t.cos(), -t.sin()),
        };
        let (s, c) = u.sin_cos();
        [0, 1, 2].map(|k| a * (c * self.x[k] + s * self.y[k]) + b * self.z[k])
    }

    /// The apex's v, on a cone.
    fn apex(&self) -> Option<f64> {
        match self.shape {
            Elementary::Cylinder => None,
            Elementary::Cone(t) => Some(-self.r / t.tan()),
        }
    }

    /// `p`'s (u, v), after checking it lies on the surface within `unc`
    /// (module docs).
    fn invert(&self, p: [f64; 3], unc: f64) -> Result<Inv, Route> {
        let w = [0, 1, 2].map(|k| p[k] - self.c[k]);
        let (a, b, h) = (dot(w, self.x), dot(w, self.y), dot(w, self.z));
        let rho = a.hypot(b);
        let tol =
            unc + 8. * f64::EPSILON * (dot(p, p).sqrt() + dot(self.c, self.c).sqrt() + self.r);
        let off = match self.shape {
            Elementary::Cylinder => rho - self.r,
            Elementary::Cone(t) => {
                let side = self.r + h * t.tan();
                if side < -tol {
                    return Err(route("curve:off-surface"));
                }
                (rho - side) * t.cos()
            }
        };
        if off.is_nan() || off.abs() > tol {
            return Err(route("curve:off-surface"));
        }
        let (pole, v) = match self.apex() {
            Some(v) if rho <= tol => (-1., v),
            _ => (0., h),
        };
        Ok(Inv {
            u: b.atan2(a),
            v,
            pole,
        })
    }

    /// One loop in unscaled (u, v), unwrapped from its first point that is
    /// no pole, its poles split (module docs), for a face left of it when
    /// `same_sense`.
    fn unwrap(
        &self,
        pts: &[(EdgePoint, [f64; 3])],
        unc: f64,
        same_sense: bool,
    ) -> Result<Loop, Route> {
        let inv = pts
            .iter()
            .map(|&(_, p)| self.invert(p, unc))
            .collect::<Result<Vec<_>, _>>()?;
        let n = pts.len();
        let k0 = inv
            .iter()
            .position(|q| q.pole == 0.)
            .ok_or(route("loop:degenerate"))?;
        let ss = if same_sense { 1. } else { -1. };
        let vertex = |k: usize, uv: [f64; 2], pole: bool| Vertex {
            ep: Some(pts[k].0),
            p: pts[k].1,
            uv,
            pole,
            cut: false,
        };
        let mut u = inv[k0].u;
        let mut vs = vec![vertex(k0, [u, inv[k0].v], false)];
        let mut pole: Option<usize> = None;
        for step in 1..=n {
            let k = (k0 + step) % n;
            let q = &inv[k];
            if q.pole != 0. {
                if pole.replace(k).is_some() {
                    return Err(route("loop:degenerate"));
                }
                continue;
            }
            let nu = match pole.take() {
                Some(pk) => {
                    let dir = -inv[pk].pole * ss;
                    let mut turn = (dir * (q.u - u)).rem_euclid(TAU);
                    if turn == 0. {
                        turn = TAU;
                    }
                    let nu = u + dir * turn;
                    vs.push(vertex(pk, [u, inv[pk].v], true));
                    vs.push(vertex(pk, [nu, inv[pk].v], true));
                    nu
                }
                None => q.u + TAU * ((u - q.u) / TAU).round(),
            };
            if step == n {
                let turns = ((nu - vs[0].uv[0]) / TAU).round();
                return Ok(Loop { vs, turns });
            }
            u = nu;
            vs.push(vertex(k, [u, q.v], false));
        }
        unreachable!("the loop closes at step n")
    }

    /// The scales `(s_u, s_v)` and grid steps (in scaled units) for the
    /// vertices `vs` (module docs).
    fn grid<'v>(
        &self,
        vs: impl Iterator<Item = &'v Vertex>,
        tol: Tolerance,
    ) -> ([f64; 2], [f64; 2]) {
        let (d, a) = (tol.linear(), tol.angular());
        let step = |rho: f64| a.min(2. * (1. - (d / rho).min(1.)).acos());
        let top = vs.map(|q| self.radius(q.uv[1])).fold(0., f64::max);
        let per_v = match self.shape {
            Elementary::Cylinder => 1.,
            Elementary::Cone(t) => 1. / t.cos(),
        };
        // The rulings are straight: no point is needed along them (module
        // docs).
        ([top, per_v], [step(top) * top / 2., f64::INFINITY])
    }

    /// The virtual seam's interior points from `a` to `b` (unscaled), at
    /// most a grid step `h` (unscaled) apart. Both copies use these, so
    /// their 3-D points are one.
    fn seam(&self, a: [f64; 2], b: [f64; 2], h: [f64; 2]) -> Vec<Vertex> {
        let n = ((b[0] - a[0]) / h[0])
            .abs()
            .max(((b[1] - a[1]) / h[1]).abs())
            .ceil();
        let n = n.clamp(1., MAX_STEINER as f64) as usize;
        (1..n)
            .map(|i| {
                let t = i as f64 / n as f64;
                let uv = [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];
                Vertex {
                    ep: None,
                    p: self.at(uv),
                    uv,
                    pole: false,
                    cut: true,
                }
            })
            .collect()
    }

    /// Whether triangle `t` (vertices at unscaled `uv`, normals unflipped
    /// `n`) is within the deflection (module docs).
    fn within(&self, uv: &[[f64; 2]], n: &[[f64; 3]], t: [u32; 3], tol: Tolerance) -> bool {
        let t = t.map(|i| i as usize);
        let (d, a) = (
            tol.linear() * (1. + ROUNDING),
            tol.angular() * (1. + ROUNDING),
        );
        let turn = |i: usize, j: usize| dot(n[t[i]], n[t[j]]).clamp(-1., 1.).acos();
        let us = t.map(|i| uv[i][0]);
        let du = us[0].max(us[1]).max(us[2]) - us[0].min(us[1]).min(us[2]);
        let top = t
            .map(|i| self.radius(uv[i][1]))
            .into_iter()
            .fold(0., f64::max);
        turn(0, 1).max(turn(1, 2)).max(turn(2, 0)) <= a
            && du <= PI
            && top * (1. - (du / 2.).cos()) <= d
    }
}

/// `lp` with a vertex on each grid column `u0 + k·du` along each pole's
/// line between its two ends, all one 3-D point: on the columns, so no
/// triangle joins a pole vertex to two points of one ruling, which would be
/// collinear in 3-D.
fn fill_poles(lp: &[Vertex], u0: f64, du: f64) -> Vec<Vertex> {
    let mut out = Vec::with_capacity(lp.len());
    for (k, a) in lp.iter().enumerate() {
        out.push(*a);
        let b = lp[(k + 1) % lp.len()];
        if a.pole && b.pole && a.p == b.p {
            let col = |u: f64| (u - u0) / du;
            let (i, j) = (col(a.uv[0]), col(b.uv[0]));
            if (j - i).abs() > MAX_STEINER as f64 {
                continue;
            }
            let ks: Vec<f64> = if i < j {
                (i.floor() as i64 + 1..=j.ceil() as i64 - 1)
                    .map(|k| k as f64)
                    .collect()
            } else {
                (j.floor() as i64 + 1..=i.ceil() as i64 - 1)
                    .rev()
                    .map(|k| k as f64)
                    .collect()
            };
            out.extend(
                ks.into_iter()
                    .filter(|&k| k != i && k != j)
                    .map(|k| Vertex {
                        uv: [u0 + k * du, a.uv[1]],
                        ..*a
                    }),
            );
        }
    }
    out
}

/// Meshes the `ADVANCED_FACE` `face` on a cylinder or cone from `edges`,
/// which holds its edges' polylines, within `tol`; `reversed` when its shell
/// uses it reversed.
///
/// # Errors
/// The [`Route`] that sends it to OCCT (module docs and [`super::face`]):
/// another surface, an edge without a polyline, a boundary point off the
/// surface, wrapping loops no virtual seam resolves, a boundary the
/// triangulation can't take exactly, or a triangle beyond the deflection.
pub fn mesh(
    g: &Graph,
    face: Node,
    edges: &Polylines,
    ctx: Context,
    tol: Tolerance,
    reversed: bool,
) -> Result<FaceMesh, Route> {
    let (bounds, surface, same_sense) = curve::advanced_face(g, face).map_err(Route::Face)?;
    if !(ctx.uncertainty.is_finite() && ctx.uncertainty > 0.) {
        return Err(route("no length uncertainty"));
    }
    let ([c, x, y, z], r, shape) =
        curve::elementary(g, surface, ctx.mm_per_unit, ctx.rad_per_unit).map_err(Route::Face)?;
    let s = Surface {
        c,
        x,
        y,
        z,
        r,
        shape,
    };
    let (mut loops, mut wraps, mut apex) = (Vec::new(), Vec::new(), None);
    for &b in &bounds {
        if let Some(v) = curve::vertex_loop(g, b) {
            let p = curve::vertex(g, v, ctx.mm_per_unit).map_err(Route::Face)?;
            if s.invert(p, ctx.uncertainty)?.pole == 0. || apex.replace(p).is_some() {
                return Err(route("loop:pole"));
            }
            continue;
        }
        let lp = s.unwrap(&walk(g, b, edges)?, ctx.uncertainty, same_sense)?;
        if lp.turns == 0. {
            loops.push(lp.vs);
        } else {
            wraps.push(lp);
        }
    }
    let all = || {
        loops
            .iter()
            .flatten()
            .chain(wraps.iter().flat_map(|l| &l.vs))
    };
    let (scale, h) = s.grid(all(), tol);
    let step = [h[0] / scale[0], h[1] / scale[1]];
    match (wraps.as_slice(), s.apex()) {
        ([], _) if apex.is_none() => {}
        ([one, two], _) if apex.is_none() && one.turns.abs() == 1. && one.turns == -two.turns => {
            let a = one.vs[0];
            // B: a point of the other loop nearest u_A in u, a turn aside.
            let gap = |v: &Vertex| (v.uv[0] - a.uv[0] + PI).rem_euclid(TAU) - PI;
            let j = (0..two.vs.len())
                .filter(|&k| !two.vs[k].pole)
                .min_by(|&i, &k| gap(&two.vs[i]).abs().total_cmp(&gap(&two.vs[k]).abs()))
                .ok_or(route("loop:degenerate"))?;
            let b = two.vs[j];
            let to_b = a.uv[0] + gap(&b) - b.uv[0];
            let second: Vec<Vertex> = (two.vs[j..].iter().map(|&v| shift(v, to_b, false)))
                .chain(
                    two.vs[..j]
                        .iter()
                        .map(|&v| shift(v, to_b + TAU * two.turns, false)),
                )
                .collect();
            let (b, on) = (second[0], TAU * one.turns);
            let cut = s.seam(a.uv, b.uv, step);
            let mut lp = one.vs.clone();
            lp.push(shift(a, on, true));
            lp.extend(cut.iter().map(|&v| shift(v, on, true)));
            lp.extend(second.iter().map(|&v| shift(v, on, false)));
            lp.push(shift(b, 0., true));
            lp.extend(cut.iter().rev());
            loops.push(lp);
        }
        ([one], Some(va)) if one.turns.abs() == 1. => {
            let a = one.vs[0];
            let on = TAU * one.turns;
            let pole = Vertex {
                ep: None,
                p: apex.unwrap_or_else(|| s.at([0., va])),
                uv: [a.uv[0] + on, va],
                pole: true,
                cut: false,
            };
            let cut = s.seam(a.uv, [a.uv[0], va], step);
            let mut lp = one.vs.clone();
            lp.push(shift(a, on, true));
            lp.extend(cut.iter().map(|&v| shift(v, on, true)));
            lp.push(pole);
            lp.push(shift(pole, -on, true));
            lp.extend(cut.iter().rev());
            loops.push(lp);
        }
        _ => return Err(route("loop:seam-unresolved")),
    }
    let lo = loops
        .iter()
        .flatten()
        .map(|q| q.uv[0])
        .fold(f64::MAX, f64::min);
    for lp in &mut loops {
        *lp = fill_poles(lp, lo, step[0]);
        for q in lp.iter_mut() {
            q.uv = [q.uv[0] * scale[0], q.uv[1] * scale[1]];
        }
    }
    let mut t = triangulate(&loops, same_sense)?;
    let at = |q: [f64; 2]| s.at([q[0] / scale[0], q[1] / scale[1]]);
    for _ in 0..ROUNDS {
        let uv: Vec<[f64; 2]> =
            t.uv.iter()
                .map(|q| [q[0] / scale[0], q[1] / scale[1]])
                .collect();
        let n: Vec<[f64; 3]> = uv.iter().map(|&q| s.normal(q)).collect();
        let bad: Vec<[f64; 2]> = t
            .triangles()
            .into_iter()
            .filter(|&tr| !s.within(&uv, &n, tr, tol))
            .map(|tr| {
                let q = tr.map(|i| t.uv[i as usize]);
                [0, 1].map(|k| (q[0][k] + q[1][k] + q[2][k]) / 3.)
            })
            .collect();
        if bad.is_empty() {
            break;
        }
        t.steiner(bad.into_iter(), at)?;
    }
    let flip = same_sense == reversed;
    let (mut m, uv, pole) = t.into_mesh(flip);
    let uv: Vec<[f64; 2]> = uv
        .iter()
        .map(|q| [q[0] / scale[0], q[1] / scale[1]])
        .collect();
    let n: Vec<[f64; 3]> = uv.iter().map(|&q| s.normal(q)).collect();
    if !m.triangles.iter().all(|&t| s.within(&uv, &n, t, tol)) {
        return Err(route("rs-fail:deflection"));
    }
    // A fold: two vertices on one ruling and the third at the apex, which
    // every ruling passes through (a seam edge down to the apex).
    let fold = |t: [u32; 3]| {
        if !t.iter().any(|&i| pole[i as usize]) {
            return false;
        }
        let [a, b, c] = t.map(|i| m.point(i as usize));
        let (e, f) = (
            [0, 1, 2].map(|k| b[k] - a[k]),
            [0, 1, 2].map(|k| c[k] - a[k]),
        );
        let x = [
            e[1] * f[2] - e[2] * f[1],
            e[2] * f[0] - e[0] * f[2],
            e[0] * f[1] - e[1] * f[0],
        ];
        dot(x, x).sqrt() <= ROUNDING * dot(e, e).sqrt() * dot(f, f).sqrt()
    };
    if m.triangles.iter().any(|&t| fold(t)) {
        return Err(route("rs-fail:degenerate triangle"));
    }
    m.normals(flip, |i| n[i]);
    let finite = |v: &[f64]| v.iter().all(|c| c.is_finite());
    if !(finite(&m.x)
        && finite(&m.y)
        && finite(&m.z)
        && finite(&m.nx)
        && finite(&m.ny)
        && finite(&m.nz))
    {
        return Err(route("non-finite point"));
    }
    Ok(m)
}

#[cfg(test)]
mod tests;
