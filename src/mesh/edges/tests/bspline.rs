//! B-spline edges (M1b) against the exact curve, evaluated here by the
//! Cox–de Boor recursion on the basis functions: independent of the
//! subdivision's blossoms and halving. Every chord within d, every turn
//! within a (except at C0 knots and a closed curve's seam), no NaN, the
//! vertex points as the ends; and each malformed input routed with its
//! reason.

use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_4};

use super::*;

/// One B-spline edge, `#10`, simple or (rational) complex.
#[derive(Clone)]
struct Spline {
    p: usize,
    pts: Vec<[f64; 3]>,
    w: Option<Vec<f64>>,
    mults: Vec<usize>,
    knots: Vec<f64>,
    /// The start and end vertices' parameters (only the first when
    /// `closed`).
    t: [f64; 2],
    closed: bool,
    sense: bool,
    /// Through a `SURFACE_CURVE`.
    surface: bool,
}

impl Spline {
    /// Non-rational, the whole range, `same_sense`.
    fn new(p: usize, pts: Vec<[f64; 3]>, mults: Vec<usize>, knots: Vec<f64>) -> Self {
        let mut s = Self {
            p,
            pts,
            w: None,
            mults,
            knots,
            t: [0.; 2],
            closed: false,
            sense: true,
            surface: false,
        };
        s.t = s.range();
        s
    }

    fn expanded(&self) -> Vec<f64> {
        let mut u = Vec::new();
        for (&m, &k) in self.mults.iter().zip(&self.knots) {
            u.extend(std::iter::repeat_n(k, m));
        }
        u
    }

    fn range(&self) -> [f64; 2] {
        let u = self.expanded();
        [u[self.p], u[self.pts.len()]]
    }

    /// The exact point at `t`: `Σ N_i,p(t)·w_i·P_i / Σ N_i,p(t)·w_i`.
    fn eval(&self, t: f64) -> [f64; 3] {
        let (u, p) = (self.expanded(), self.p);
        let hi = self.range()[1];
        let mut nb: Vec<f64> = (0..u.len() - 1)
            .map(|i| {
                let inside = u[i] <= t && t < u[i + 1] && t != hi;
                let last = t == hi && u[i] < hi && u[i + 1] == hi;
                if inside || last { 1. } else { 0. }
            })
            .collect();
        for k in 1..=p {
            for i in 0..u.len() - 1 - k {
                let a = if u[i + k] > u[i] {
                    (t - u[i]) / (u[i + k] - u[i]) * nb[i]
                } else {
                    0.
                };
                let b = if u[i + k + 1] > u[i + 1] {
                    (u[i + k + 1] - t) / (u[i + k + 1] - u[i + 1]) * nb[i + 1]
                } else {
                    0.
                };
                nb[i] = a + b;
            }
        }
        let (mut s, mut w) = ([0.; 3], 0.);
        for (i, q) in self.pts.iter().enumerate() {
            let wi = self.w.as_ref().map_or(1., |w| w[i]) * nb[i];
            w += wi;
            (0..3).for_each(|k| s[k] += wi * q[k]);
        }
        s.map(|x| x / w)
    }

    fn text(&self) -> String {
        let pt = |p: [f64; 3]| format!("CARTESIAN_POINT('',({:?},{:?},{:?}))", p[0], p[1], p[2]);
        let join = |v: Vec<String>| v.join(",");
        let mut s = String::new();
        for (i, &q) in self.pts.iter().enumerate() {
            s += &format!("#{}={};\n", 101 + i, pt(q));
        }
        let refs = join(
            (0..self.pts.len())
                .map(|i| format!("#{}", 101 + i))
                .collect(),
        );
        let m = join(self.mults.iter().map(ToString::to_string).collect());
        let k = join(self.knots.iter().map(|k| format!("{k:?}")).collect());
        let p = self.p;
        s += &match &self.w {
            None => format!(
                "#20=B_SPLINE_CURVE_WITH_KNOTS('',{p},({refs}),.UNSPECIFIED.,.F.,.F.,({m}),({k}),.UNSPECIFIED.);\n"
            ),
            Some(w) => format!(
                "#20=(BOUNDED_CURVE() B_SPLINE_CURVE({p},({refs}),.UNSPECIFIED.,.F.,.F.) \
                 B_SPLINE_CURVE_WITH_KNOTS(({m}),({k}),.UNSPECIFIED.) CURVE() \
                 GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_CURVE(({})) \
                 REPRESENTATION_ITEM(''));\n",
                join(w.iter().map(|w| format!("{w:?}")).collect())
            ),
        };
        let curve = if self.surface {
            s += "#21=SURFACE_CURVE('',#20,(),.CURVE_3D.);\n";
            21
        } else {
            20
        };
        let end = if self.closed { 7 } else { 9 };
        let sense = if self.sense { "T" } else { "F" };
        s + &format!(
            "#6={};\n#7=VERTEX_POINT('',#6);\n#8={};\n#9=VERTEX_POINT('',#8);\n\
             #10=EDGE_CURVE('',#7,#{end},#{curve},.{sense}.);\n",
            pt(self.eval(self.t[0])),
            pt(self.eval(self.t[1])),
        )
    }

    fn build(&self, tol: Tolerance) -> Result<Vec<[f64; 3]>, Reason> {
        let g = file(&self.text());
        let e = node(&g, 10);
        let p = Polylines::build(&g, [e], 1., tol);
        p.get(e).unwrap().map(|l| l.points().collect())
    }
}

/// The distance from `q` to the polyline `p`.
fn to_polyline(q: [f64; 3], p: &[[f64; 3]]) -> f64 {
    p.windows(2)
        .map(|w| to_segment(q, w[0], w[1]))
        .fold(f64::INFINITY, f64::min)
}

/// Points of `sp` over `[a, b]`: 2,000 equal parameter steps, each halved
/// while its middle point lies over d/100 from its chord, as an extreme
/// weight makes the curve rush through a few steps.
fn dense(sp: &Spline, a: f64, b: f64, d: f64) -> Vec<[f64; 3]> {
    const N: usize = 2_000;
    let mut out = vec![sp.eval(a)];
    for i in 0..N {
        let mut stack = vec![(a + (b - a) * (i + 1) as f64 / N as f64, 0)];
        let mut t = a + (b - a) * i as f64 / N as f64;
        while let Some(&(e, depth)) = stack.last() {
            let m = 0.5 * (t + e);
            let (p, q) = (sp.eval(t), sp.eval(e));
            if depth < 40 && to_segment(sp.eval(m), p, q) > d / 100. {
                stack.push((m, depth + 1));
            } else {
                out.push(q);
                t = e;
                stack.pop();
            }
        }
    }
    out
}

/// What a B-spline polyline measured against its exact curve.
#[derive(Debug)]
struct Got {
    pts: Vec<[f64; 3]>,
    /// The largest distance of a dense curve sample from the polyline.
    chord: f64,
    /// The largest distance of a polyline point from the densely sampled
    /// curve.
    off: f64,
    /// The largest turn between consecutive segments, at points other than
    /// `exempt` (the closing vertex of a closed edge included).
    turn: f64,
}

/// Builds `sp` and measures it over the parameter ranges `sweep`, after
/// checking that its ends are its vertex points and every coordinate is
/// finite.
fn measure(sp: &Spline, tol: Tolerance, sweep: &[(f64, f64)], exempt: &[[f64; 3]]) -> Got {
    let pts = sp.build(tol).unwrap();
    assert!(pts.iter().flatten().all(|x| x.is_finite()));
    let n = pts.len();
    assert_eq!(pts[0], sp.eval(sp.t[0]), "the start vertex first");
    let last = if sp.closed { sp.t[0] } else { sp.t[1] };
    assert_eq!(pts[n - 1], sp.eval(last), "the end vertex last");
    let samples: Vec<[f64; 3]> = sweep
        .iter()
        .flat_map(|&(a, b)| dense(sp, a, b, tol.linear()))
        .collect();
    let chord = samples
        .iter()
        .map(|&q| to_polyline(q, &pts))
        .fold(0f64, f64::max);
    let off = pts
        .iter()
        .map(|&q| to_polyline(q, &samples))
        .fold(0f64, f64::max);
    let near = |q: [f64; 3]| {
        exempt
            .iter()
            .any(|&e| norm(sub(q, e)) <= 1e-9 * (1. + norm(e)))
    };
    let seg = |i: usize| sub(pts[i + 1], pts[i]);
    let mut turn = (1..n - 1)
        .filter(|&i| !near(pts[i]))
        .map(|i| angle(seg(i - 1), seg(i)))
        .fold(0f64, f64::max);
    if sp.closed && !near(pts[0]) {
        turn = turn.max(angle(seg(n - 2), seg(0)));
    }
    Got {
        pts,
        chord,
        off,
        turn,
    }
}

fn within(what: &str, got: &Got, tol: Tolerance) {
    let (d, a) = (tol.linear(), tol.angular());
    assert!(
        got.chord <= d * (1. + 1e-9),
        "{what}: chord {} > d {d}",
        got.chord
    );
    assert!(got.off <= d, "{what}: a point {} off the curve", got.off);
    assert!(
        got.turn <= a * (1. + 1e-9) + 1e-12,
        "{what}: turn {} > a {a}",
        got.turn
    );
}

/// The rational quadratic quarter circle of radius `r` about `c`, in the
/// xy plane, from (r, 0) to (0, r): weights 1, √½, 1.
fn quarter(c: [f64; 3], r: f64) -> Spline {
    let at = |x: f64, y: f64| [c[0] + x * r, c[1] + y * r, c[2]];
    let mut s = Spline::new(
        2,
        vec![at(1., 0.), at(1., 1.), at(0., 1.)],
        vec![3, 3],
        vec![0., 1.],
    );
    s.w = Some(vec![1., FRAC_1_SQRT_2, 1.]);
    s
}

/// The 9-point rational quadratic full circle of radius `r` about `c`,
/// from (r, 0): knots of multiplicity 2 at the quarters.
fn circle(c: [f64; 3], r: f64) -> Spline {
    let sq = [
        (1., 0.),
        (1., 1.),
        (0., 1.),
        (-1., 1.),
        (-1., 0.),
        (-1., -1.),
        (0., -1.),
        (1., -1.),
        (1., 0.),
    ];
    let pts = sq
        .iter()
        .map(|&(x, y)| [c[0] + x * r, c[1] + y * r, c[2]])
        .collect();
    let mut s = Spline::new(2, pts, vec![3, 2, 2, 2, 3], vec![0., 0.25, 0.5, 0.75, 1.]);
    s.w = Some(
        (0..9)
            .map(|i| if i % 2 == 0 { 1. } else { FRAC_1_SQRT_2 })
            .collect(),
    );
    s
}

#[test]
fn degree_1_is_exactly_its_control_polygon() {
    let pts = vec![
        [0., 0., 0.],
        [1., 2., 0.],
        [2., -1., 1.],
        [3., 3., -2.],
        [5., 0., 0.],
    ];
    let mut s = Spline::new(
        1,
        pts.clone(),
        vec![2, 1, 1, 1, 2],
        vec![0., 1., 2.5, 3., 4.],
    );
    let tol = Tolerance::from_mm(1e-3, 0.3).unwrap();
    assert_eq!(s.build(tol).unwrap(), pts);
    // Against the curve: the same points, from the end vertex.
    (s.t, s.sense) = ([4., 0.], false);
    let back: Vec<_> = pts.iter().rev().copied().collect();
    assert_eq!(s.build(tol).unwrap(), back);
}

#[test]
fn the_adr_counterexample_turns_within_a_not_2a() {
    // ADR §2.6's: each span's legs turn by 20° and lie 0.017 from its chord,
    // so "legs within a" accepts both spans, whose chords turn by 39.8°.
    let a = 20f64.to_radians();
    let q2 = [10. + 0.1 * a.cos(), 0.1 * a.sin(), 0.];
    let q3 = [
        q2[0] + 10. * (2. * a).cos(),
        q2[1] + 10. * (2. * a).sin(),
        0.,
    ];
    let pts = vec![[0., 0., 0.], [10., 0., 0.], q2, q3];
    let s = Spline::new(2, pts, vec![3, 1, 3], vec![0., 1., 2.]);
    let tol = Tolerance::from_mm(0.02, a).unwrap();
    let got = measure(&s, tol, &[(0., 2.)], &[]);
    within("counterexample", &got, tol);
}

#[test]
fn a_rational_quarter_circle_is_the_exact_arc_within_d_and_a() {
    let c = [1.5, -2., 3.];
    for r in [1e-3, 1e-1, 1., 10., 1e3] {
        for (d, a) in [
            (1e-3, 0.3),
            (1e-2, 20f64.to_radians()),
            (1e-4, 0.05),
            (5e-3, 1.),
        ] {
            let tol = Tolerance::from_mm(d, a).unwrap();
            let mut s = quarter(c, r);
            s.surface = true;
            let got = measure(&s, tol, &[(0., 1.)], &[]);
            within(&format!("r {r} d {d}"), &got, tol);
            for (i, q) in got.pts.iter().enumerate() {
                let rho = norm(sub(*q, c));
                assert!(
                    (rho - r).abs() <= 1e-12 * r.max(1.),
                    "point {i} at radius {rho}, not {r}"
                );
            }
            // The sag of each chord of the exact circle, analytically.
            for w in got.pts.windows(2) {
                let phi = angle(sub(w[0], c), sub(w[1], c));
                assert!(
                    r * (1. - (phi / 2.).cos()) <= d * (1. + 1e-9),
                    "r {r}: sag over {phi}"
                );
            }
        }
    }
}

#[test]
fn a_rational_full_circle_closes_through_its_seam_and_wraps() {
    let c = [0., 1., -1.];
    let tol = Tolerance::from_mm(1e-3, 0.2).unwrap();
    for t in [0., 0.3, 0.75] {
        let mut s = circle(c, 2.);
        (s.t, s.closed) = ([t, t], true);
        let got = measure(&s, tol, &[(t, 1.), (0., t)], &[]);
        // Tangent-continuous everywhere, so the turn holds at the seam too.
        within(&format!("from {t}"), &got, tol);
        assert!(got.pts.len() > 30);
    }
}

#[test]
fn a_c0_knot_is_an_exact_corner_and_the_bound_holds_on_both_sides() {
    // Two cubic Béziers meeting at (3, 0, 0) at 45°: the interior knot has
    // multiplicity 3 = p.
    let pts = vec![
        [0., 0., 0.],
        [1., 1., 0.],
        [2., 1., 0.],
        [3., 0., 0.],
        [3., -1., 0.],
        [4., -2., 1.],
        [5., -2., 0.],
    ];
    let s = Spline::new(3, pts, vec![4, 3, 4], vec![0., 1., 2.]);
    let tol = Tolerance::from_mm(1e-3, 0.2).unwrap();
    let got = measure(&s, tol, &[(0., 2.)], &[[3., 0., 0.]]);
    within("kink", &got, tol);
    let k = got
        .pts
        .iter()
        .position(|&q| q == [3., 0., 0.])
        .expect("the kink, exactly");
    let corner = angle(
        sub(got.pts[k], got.pts[k - 1]),
        sub(got.pts[k + 1], got.pts[k]),
    );
    // Chords, not tangents, but near the curve's own 45°, far above a.
    assert!(
        (corner - FRAC_PI_4).abs() < 0.01,
        "the corner is the curve's own: {corner}"
    );
}

/// A cubic over [0, 3] with a double (C1) and a simple interior knot: a
/// repeated knot leaves an empty knot span between its copies.
fn wave() -> Spline {
    let pts = vec![
        [0., 0., 0.],
        [1., 2., 0.],
        [2., -2., 1.],
        [3., 2., 0.],
        [4., 0., -1.],
        [5., 1., 0.],
        [6., -1., 0.5],
    ];
    Spline::new(3, pts, vec![4, 2, 1, 4], vec![0., 1., 2., 3.])
}

#[test]
fn a_trimmed_range_against_the_curve_is_the_same_points_reversed() {
    let tol = Tolerance::from_mm(1e-3, 20f64.to_radians()).unwrap();
    let mut s = wave();
    s.t = [0.3, 2.6];
    let fwd = measure(&s, tol, &[(0.3, 2.6)], &[]);
    within("forward", &fwd, tol);
    (s.t, s.sense) = ([2.6, 0.3], false);
    let back = measure(&s, tol, &[(0.3, 2.6)], &[]);
    within("backward", &back, tol);
    let mut rev = back.pts;
    rev.reverse();
    assert_eq!(fwd.pts, rev);
}

#[test]
fn an_unclamped_closed_cubic_wraps_through_its_seam() {
    // Uniform knots, the first three control points repeated at the end:
    // C(3) = C(9), and the closed edge starts at C(5).
    let ring = [
        [2., 0., 0.],
        [1., 2., 0.5],
        [-1., 2., 0.],
        [-2., 0., -0.5],
        [-1., -2., 0.],
        [1., -2., 0.],
    ];
    let pts: Vec<_> = (0..9).map(|i| ring[i % 6]).collect();
    let mut s = Spline::new(3, pts, vec![1; 13], (0..13).map(f64::from).collect());
    (s.t, s.closed) = ([5., 5.], true);
    let tol = Tolerance::from_mm(1e-3, 0.3).unwrap();
    let got = measure(&s, tol, &[(5., 9.), (3., 5.)], &[s.eval(3.)]);
    within("ring", &got, tol);
}

#[test]
fn extreme_weights_stay_within_the_bounds_or_route() {
    let pts = vec![[0., 0., 0.], [1., 3., 0.], [3., 3., 1.], [4., 0., 0.]];
    let tol = Tolerance::from_mm(1e-3, 0.3).unwrap();
    let spline = |w: [f64; 4]| {
        let mut s = Spline::new(3, pts.clone(), vec![4, 4], vec![0., 1.]);
        s.w = Some(w.to_vec());
        s
    };
    for w in [
        [1e-6, 1., 1., 1e6],
        [1e6, 1e-6, 1e6, 1e-6],
        [1e-3, 1e3, 1e3, 1e-3],
    ] {
        let got = measure(&spline(w), tol, &[(0., 1.)], &[]);
        within(&format!("{w:?}"), &got, tol);
    }
    // P1 outweighs P2 by 1e12: the tangent turns from P3 − P1 to the end
    // tangent P3 − P2 about 1e-18 mm from P3, below f64's resolution there.
    let s = spline([1., 1e6, 1e-6, 1.]);
    assert_eq!(s.build(tol).unwrap_err(), Reason("zero tangent"));
}

#[test]
fn no_non_finite_coordinate_escapes_from_a_huge_b_spline() {
    let mut built = 0;
    for big in [1., 1e300, 1e307, 1e308, f64::MAX] {
        for s in [1., 25.4, 1000.] {
            for (a, b) in [(big, -big), (big, big), (-big, big / 2.)] {
                let mut sp = Spline::new(
                    2,
                    vec![[a, 0., 0.], [b, b, 0.], [a, b, a]],
                    vec![3, 3],
                    vec![0., 1.],
                );
                for w in [None, Some(vec![1., 1e6, 1e-6])] {
                    sp.w = w;
                    // The end control points as the vertices: `eval` may
                    // overflow.
                    let text: String = sp
                        .text()
                        .lines()
                        .filter(|l| !l.starts_with("#6=") && !l.starts_with("#8="))
                        .collect::<Vec<_>>()
                        .join("\n")
                        .replace("#7=VERTEX_POINT('',#6)", "#7=VERTEX_POINT('',#101)")
                        .replace("#9=VERTEX_POINT('',#8)", "#9=VERTEX_POINT('',#103)");
                    let g = file(&text);
                    let e = node(&g, 10);
                    let tol = Tolerance::from_mm(1e-3 * big.min(1e300), 0.3).unwrap();
                    let p = Polylines::build(&g, [e], s, tol);
                    if let Ok(line) = p.get(e).unwrap() {
                        built += 1;
                        assert!(
                            line.points().flatten().all(f64::is_finite),
                            "big {big:e} s {s}"
                        );
                    }
                }
            }
        }
    }
    assert!(built > 0, "the finite cases build");
}

#[test]
fn malformed_b_splines_route_with_their_reason() {
    let tol = Tolerance::from_mm(1e-3, 0.3).unwrap();
    let base = || {
        Spline::new(
            2,
            vec![[0., 0., 0.], [1., 1., 0.], [2., 0., 0.]],
            vec![3, 3],
            vec![0., 1.],
        )
    };
    let routes = |s: &Spline, text: &dyn Fn(String) -> String| {
        let g = file(&text(s.text()));
        let e = node(&g, 10);
        let p = Polylines::build(&g, [e], 1., tol);
        assert_eq!(p.point_count(), 0);
        p.get(e).unwrap().unwrap_err()
    };
    let same = |t: String| t;
    // (reason, text in the valid file, replacement)
    let cases = [
        ("unsupported degree", "KNOTS('',2,", "KNOTS('',0,"),
        ("unsupported degree", "KNOTS('',2,", "KNOTS('',26,"),
        ("knot count", "(3,3),", "(3,2),"),
        ("bad multiplicity", "(3,3),(0.0,1.0)", "(4,2),(0.0,1.0)"),
        ("bad multiplicity", "(3,3),(0.0,1.0)", "(0,3),(0.0,1.0)"),
        ("decreasing knots", "(0.0,1.0)", "(1.0,0.0)"),
        ("empty knot range", "(0.0,1.0)", "(1.0,1.0)"),
        ("non-finite knot", "(0.0,1.0)", "(0.0,1E400)"),
        ("malformed B-spline", ".F.,.F.,(3,3)", "'x',.F.,(3,3)"),
        (
            "not a finite 3-D point",
            "#102=CARTESIAN_POINT('',(1.0",
            "#102=CARTESIAN_POINT('',(1E400",
        ),
        (
            "vertex off curve",
            "#6=CARTESIAN_POINT('',(0.0,",
            "#6=CARTESIAN_POINT('',(0.5,",
        ),
        (
            "vertex order",
            "#10=EDGE_CURVE('',#7,#9",
            "#10=EDGE_CURVE('',#9,#7",
        ),
        (
            "closed edge on an open curve",
            "#10=EDGE_CURVE('',#7,#9",
            "#10=EDGE_CURVE('',#7,#7",
        ),
    ];
    for (why, from, to) in cases {
        let text = |t: String| t.replacen(from, to, 1);
        assert_eq!(routes(&base(), &text), Reason(why), "{why}");
    }
    let r = quarter([0.; 3], 1.);
    for (w, why) in [
        ("0.0", "non-positive weight"),
        ("-1.0", "non-positive weight"),
        ("1E400", "non-positive weight"),
    ] {
        let to = format!("RATIONAL_B_SPLINE_CURVE((1.0,{w},1.0))");
        let text = move |t: String| {
            t.replacen(
                "RATIONAL_B_SPLINE_CURVE((1.0,0.7071067811865476,1.0))",
                &to,
                1,
            )
        };
        assert_eq!(routes(&r, &text), Reason(why), "{w}");
    }
    let two = |t: String| t.replacen("((1.0,0.7071067811865476,1.0))", "((1.0,1.0))", 1);
    assert_eq!(routes(&r, &two), Reason("malformed weights"));
    // An interior knot of multiplicity p + 1: the curve may jump there.
    let mut jump = base();
    jump.pts = vec![
        [0., 0., 0.],
        [1., 1., 0.],
        [2., 0., 0.],
        [3., 0., 0.],
        [4., 1., 0.],
        [5., 0., 0.],
    ];
    (jump.mults, jump.knots) = (vec![3, 3, 3], vec![0., 1., 2.]);
    jump.t = [0., 2.];
    assert_eq!(routes(&jump, &same), Reason("discontinuous B-spline"));
    // A vanishing derivative at a simple knot, a possible cusp.
    let mut stall = Spline::new(
        2,
        vec![[0., 0., 0.], [1., 1., 0.], [1., 1., 0.], [3., 0., 0.]],
        vec![3, 1, 3],
        vec![0., 1., 2.],
    );
    assert_eq!(routes(&stall, &same), Reason("zero tangent"));
    // A cusp inside a span (t² , t³ at t = 0, a third of the way): no
    // halving ever passes the angle test; the depth bound stops it.
    stall = Spline::new(
        3,
        vec![[1., -1., 0.], [-1., 2., 0.], [0., -4., 0.], [4., 8., 0.]],
        vec![4, 4],
        vec![0., 1.],
    );
    assert_eq!(routes(&stall, &same), Reason("too many segments"));
}
