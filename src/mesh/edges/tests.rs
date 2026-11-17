//! The quality contract (ADR-0001 §2.6) measured against the exact curve:
//! every chord within d, every turn within a, no NaN, the vertex points as
//! the ends, over radii from 1e-3 to 1e3 mm (d > r included), ellipses with
//! B/A down to 0.01, `same_sense = .F.`, closed edges and arcs through the
//! seam. And sharing: two faces walk one edge's points in opposite order.

use std::f64::consts::{PI, TAU};

use super::*;
use crate::graph::Graph;

fn file(data: &str) -> Graph {
    let text = format!(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('t','',(''),(''),'','','');\n\
         FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA;\n{data}\nENDSEC;\nEND-ISO-10303-21;\n"
    );
    Graph::from_bytes(text.into_bytes()).unwrap()
}

fn node(g: &Graph, id: u32) -> Node {
    g.node(crate::graph::StepId(id)).unwrap()
}

/// A tilted frame: the conic's centre, x and y.
const C: [f64; 3] = [1.5, -2., 3.];
const Z: [f64; 3] = [
    0.267_261_241_912_424_4,
    0.534_522_483_824_848_8,
    0.801_783_725_737_273_2,
];
const X: [f64; 3] = [0.894_427_190_999_915_9, -0.447_213_595_499_957_9, 0.];

/// One conic edge, `#10`: a circle of radius `a` when `b` is `None`, else an
/// ellipse with semi-axes `a` (along x) and `b`, from the angle `t0` to
/// `t1` (the same vertex when `closed`), with `same_sense`.
struct Arc {
    a: f64,
    b: Option<f64>,
    t0: f64,
    t1: f64,
    closed: bool,
    sense: bool,
}

impl Arc {
    fn graph(&self) -> Graph {
        let b = self.b.unwrap_or(self.a);
        let y = cross(Z, X);
        let at = |t: f64| [0, 1, 2].map(|i| C[i] + self.a * t.cos() * X[i] + b * t.sin() * y[i]);
        let curve = match self.b {
            None => format!("CIRCLE('',#4,{:?})", self.a),
            Some(b) => format!("ELLIPSE('',#4,{:?},{b:?})", self.a),
        };
        let pt = |p: [f64; 3]| format!("CARTESIAN_POINT('',({:?},{:?},{:?}))", p[0], p[1], p[2]);
        let end = if self.closed { 7 } else { 9 };
        let s = if self.sense { "T" } else { "F" };
        file(&format!(
            "#1={};\n#2=DIRECTION('',({:?},{:?},{:?}));\n#3=DIRECTION('',({:?},{:?},{:?}));\n\
             #4=AXIS2_PLACEMENT_3D('',#1,#2,#3);\n#5={curve};\n#6={};\n#7=VERTEX_POINT('',#6);\n\
             #8={};\n#9=VERTEX_POINT('',#8);\n#10=EDGE_CURVE('',#7,#{end},#5,.{s}.);\n",
            pt(C),
            Z[0],
            Z[1],
            Z[2],
            X[0],
            X[1],
            X[2],
            pt(at(self.t0)),
            pt(at(self.t1)),
        ))
    }
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
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

/// The angle between two vectors, stable for small angles.
fn angle(u: [f64; 3], v: [f64; 3]) -> f64 {
    norm(cross(u, v)).atan2(dot(u, v))
}

/// The distance from `p` to the segment `a`–`b`.
fn to_segment(p: [f64; 3], a: [f64; 3], b: [f64; 3]) -> f64 {
    let ab = sub(b, a);
    let l = dot(ab, ab);
    let t = if l > 0. {
        (dot(sub(p, a), ab) / l).clamp(0., 1.)
    } else {
        0.
    };
    norm(sub(p, [0, 1, 2].map(|i| a[i] + t * ab[i])))
}

/// What one polyline measured against its exact conic.
struct Measured {
    segments: usize,
    /// The largest chordal error, sampled densely, and analytic on a circle.
    chord: f64,
    /// The largest turn between consecutive segments, the closing vertex of
    /// a closed edge included.
    turn: f64,
    /// The signed parameter swept, from start to end.
    swept: f64,
}

/// Builds `arc` at `tol` and measures it against the graph's own conic, the
/// exact curve, after checking the ends and that nothing is NaN.
fn measure(arc: &Arc, tol: Tolerance) -> Measured {
    let g = arc.graph();
    let e = node(&g, 10);
    let p = Polylines::build(&g, [e], 1., tol);
    let line = p.get(e).unwrap().unwrap();
    let pts: Vec<[f64; 3]> = line.points().collect();
    assert!(
        pts.iter().flatten().all(|c| c.is_finite()),
        "NaN in {pts:?}"
    );
    let v = |id| curve::vertex(&g, node(&g, id), 1.).unwrap();
    assert_eq!(pts[0], v(7), "start vertex");
    assert_eq!(
        pts[pts.len() - 1],
        v(if arc.closed { 7 } else { 9 }),
        "end vertex"
    );
    let k = curve::conic(&g, node(&g, 5), 1.).unwrap();
    let on = |t: f64| [0, 1, 2].map(|i| k.c[i] + k.a * t.cos() * k.x[i] + k.b * t.sin() * k.y[i]);
    // Unwrapped parameters: every step is under π.
    let mut ts = vec![param(&k, pts[0])];
    for q in &pts[1..] {
        let prev = *ts.last().unwrap();
        let d = (param(&k, *q) - prev + PI).rem_euclid(TAU) - PI;
        ts.push(prev + d);
    }
    let mut chord = 0f64;
    for i in 0..pts.len() - 1 {
        for s in 1..64 {
            let t = ts[i] + (ts[i + 1] - ts[i]) * f64::from(s) / 64.;
            chord = chord.max(to_segment(on(t), pts[i], pts[i + 1]));
        }
        if arc.b.is_none() {
            let w = angle(sub(pts[i], k.c), sub(pts[i + 1], k.c));
            chord = chord.max(k.a * (1. - (w / 2.).cos()));
        }
    }
    let seg = |i: usize| sub(pts[i + 1], pts[i]);
    let n = pts.len() - 1;
    let mut turn = (1..n)
        .map(|i| angle(seg(i - 1), seg(i)))
        .fold(0f64, f64::max);
    if arc.closed {
        turn = turn.max(angle(seg(n - 1), seg(0)));
    }
    Measured {
        segments: n,
        chord,
        turn,
        swept: ts[n] - ts[0],
    }
}

/// The arcs every conic is tried on: (t0, t1, closed, same_sense, the
/// signed sweep expected).
fn arcs() -> Vec<(f64, f64, bool, bool, f64)> {
    let r = f64::to_radians;
    vec![
        (r(0.), r(90.), false, true, r(90.)),
        (r(30.), r(300.), false, true, r(270.)),
        // Through the seam at 0/2π, both ways.
        (r(350.), r(20.), false, true, r(30.)),
        (r(20.), r(350.), false, false, -r(30.)),
        (r(90.), r(0.), false, false, -r(90.)),
        (r(10.), r(5.), false, true, r(355.)),
        (r(1.), r(2.), false, true, r(1.)),
        (r(45.), r(45.), true, true, TAU),
        (r(200.), r(200.), true, false, -TAU),
    ]
}

/// Checks one measurement against the contract, with slack for rounding
/// only.
fn check(what: &str, m: &Measured, tol: Tolerance, size: f64, sweep: f64) {
    let eps = 1e-12 * size.max(1.);
    assert!(
        m.chord <= tol.linear() * (1. + 1e-9) + eps,
        "{what}: chord {:e} > d {:e}",
        m.chord,
        tol.linear()
    );
    assert!(
        m.turn <= tol.angular() * (1. + 1e-9) + 1e-12,
        "{what}: turn {} > a {}",
        m.turn,
        tol.angular()
    );
    assert!(
        (m.swept - sweep).abs() < 1e-9,
        "{what}: swept {} not {sweep}",
        m.swept
    );
}

/// `n` is §2.6's `max(n_min, ⌈steps⌉)`, up to the rounding of the sweep
/// the vertices give when `steps` is within 1e-9 of an integer.
fn assert_count(n: usize, steps: f64, closed: bool, what: &str) {
    let min = if closed { 3. } else { 1. };
    let (lo, hi) = ((steps * (1. - 1e-9)).ceil(), (steps * (1. + 1e-9)).ceil());
    let n = n as f64;
    assert!(
        n == lo.max(min) || n == hi.max(min),
        "{what}: {n} segments for {steps} steps"
    );
}

#[test]
fn circle_arcs_meet_the_bounds_from_1e_3_to_1e3_mm() {
    for r in [1e-3, 1e-2, 0.1, 1., 10., 100., 1e3] {
        for (d, a) in [
            (1e-4, 20f64),
            (1e-2, 20.),
            (1., 20.),
            (1e-2, 5.),
            (1e-3, 60.),
        ] {
            let tol = Tolerance::from_mm(d, a.to_radians()).unwrap();
            for (t0, t1, closed, sense, sweep) in arcs() {
                let arc = Arc {
                    a: r,
                    b: None,
                    t0,
                    t1,
                    closed,
                    sense,
                };
                let m = measure(&arc, tol);
                let what =
                    format!("r {r} d {d} a {a} {t0:.3}→{t1:.3} closed {closed} sense {sense}");
                check(&what, &m, tol, r, sweep);
                let theta = sweep.abs();
                let steps =
                    (theta / tol.angular()).max(theta / (2. * (1. - (d / r).min(1.)).acos()));
                assert_count(m.segments, steps, closed, &what);
            }
        }
    }
}

#[test]
fn a_tiny_arc_against_a_coarse_deflection_is_clamped_not_nan() {
    // d = 1 mm against r = 1e-3 mm: acos(1 − d/r) is NaN without the clamp.
    let tol = Tolerance::from_mm(1., 20f64.to_radians()).unwrap();
    let arc = Arc {
        a: 1e-3,
        b: None,
        t0: 0.,
        t1: 1.,
        closed: false,
        sense: true,
    };
    let m = measure(&arc, tol);
    assert_eq!(m.segments, 3, "⌈1 rad / 20°⌉");
    let full = Arc {
        closed: true,
        t1: 0.,
        ..arc
    };
    assert_eq!(measure(&full, tol).segments, 18, "⌈2π / 20°⌉");
}

#[test]
fn ellipses_meet_the_bounds_down_to_b_over_a_0_01() {
    for big in [1e-3, 1., 1e3] {
        for ratio in [1., 0.5, 0.1, 0.01] {
            for (d, a) in [(1e-4, 20f64), (1e-2, 20.), (1e-3, 60.)] {
                let tol = Tolerance::from_mm(d, a.to_radians()).unwrap();
                for (t0, t1, closed, sense, sweep) in arcs() {
                    // Semi-axis 1 the major one, and the minor one.
                    for (sa, sb) in [(big, big * ratio), (big * ratio, big)] {
                        let arc = Arc {
                            a: sa,
                            b: Some(sb),
                            t0,
                            t1,
                            closed,
                            sense,
                        };
                        let m = measure(&arc, tol);
                        let what = format!(
                            "A {sa} B {sb} d {d} a {a} {t0:.3}→{t1:.3} closed {closed} sense {sense}"
                        );
                        check(&what, &m, tol, big, sweep);
                        let (lo, hi) = (sa.min(sb), sa.max(sb));
                        let step = (tol.angular() * lo / hi).min((8. * d / hi).sqrt());
                        assert_count(m.segments, sweep.abs() / step, closed, &what);
                    }
                }
            }
        }
    }
}

#[test]
fn the_circle_formula_is_not_the_ellipse_bound() {
    // §2.6's counter-example: A = 1, B = 0.5, d = 1e-3. The circle formula
    // at r = B²/A steps Δt ≈ 0.179, sagging ≈ 4·d at the major vertex;
    // √(8d/A) steps ≈ 0.089.
    let tol = Tolerance::from_mm(1e-3, 60f64.to_radians()).unwrap();
    let arc = Arc {
        a: 1.,
        b: Some(0.5),
        t0: -0.5,
        t1: 0.5,
        closed: false,
        sense: true,
    };
    let m = measure(&arc, tol);
    assert_eq!(m.segments, (1. / (8e-3f64).sqrt()).ceil() as usize);
    assert!(m.chord <= 1e-3, "{}", m.chord);
}

#[test]
fn lines_take_their_vertices_and_curves_route_with_a_reason() {
    let g = file(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=VERTEX_POINT('',#1);\n\
         #3=CARTESIAN_POINT('',(3.,4.,0.));\n#4=VERTEX_POINT('',#3);\n\
         #5=DIRECTION('',(0.6,0.8,0.));\n#6=VECTOR('',#5,5.);\n#7=LINE('',#1,#6);\n\
         #8=SURFACE_CURVE('',#7,(#1),.CURVE_3D.);\n#9=EDGE_CURVE('',#2,#4,#8,.F.);\n\
         #10=B_SPLINE_CURVE_WITH_KNOTS('',1,(#1,#3),.UNSPECIFIED.,.F.,.F.,(2,2),(0.,1.),.UNSPECIFIED.);\n\
         #11=EDGE_CURVE('',#2,#4,#10,.T.);\n\
         #12=AXIS2_PLACEMENT_3D('',#1,$,$);\n#13=ELLIPSE('',#12,1.,0.);\n#14=EDGE_CURVE('',#2,#4,#13,.T.);\n\
         #15=CIRCLE('',#12,1E300);\n#16=CARTESIAN_POINT('',(1E300,0.,0.));\n#17=VERTEX_POINT('',#16);\n\
         #18=EDGE_CURVE('',#17,#17,#15,.T.);\n",
    );
    let tol = Tolerance::from_mm(1e-3, 0.3).unwrap();
    let [line, spline, flat, huge] = [9, 11, 14, 18].map(|id| node(&g, id));
    // Twice listed, once built; in inches.
    let p = Polylines::build(&g, [line, spline, line, flat, huge], 25.4, tol);
    let l = p.get(line).unwrap().unwrap();
    assert_eq!(
        l.points().collect::<Vec<_>>(),
        [[0.; 3], [3. * 25.4, 4. * 25.4, 0.]]
    );
    assert_eq!(
        p.get(spline).unwrap().unwrap_err(),
        Reason("unsupported curve")
    );
    assert_eq!(
        p.get(flat).unwrap().unwrap_err(),
        Reason("degenerate conic")
    );
    assert_eq!(
        p.get(huge).unwrap().unwrap_err(),
        Reason("too many segments")
    );
    assert_eq!(p.point_count(), 2, "a routed edge leaves no points");
    assert_eq!(p.routed().count(), 3);
    assert!(p.get(node(&g, 1)).is_none());
    assert!(Tolerance::from_mm(0., 1.).is_none());
    assert!(Tolerance::from_mm(1., f64::NAN).is_none());
    let t = Tolerance::new(Deflection::PREVIEW, 200.).unwrap();
    assert!((t.linear() - 0.2).abs() < 1e-15 && (t.angular() - 20f64.to_radians()).abs() < 1e-15);
}

#[test]
fn two_faces_sharing_an_edge_walk_it_in_opposite_order() {
    // A half disc and a half cylinder share the arc #10 (`same_sense = .F.`,
    // through the seam): the disc walks it .T., the cylinder .F.
    let g = file(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=DIRECTION('',(0.,0.,1.));\n#3=DIRECTION('',(1.,0.,0.));\n\
         #4=AXIS2_PLACEMENT_3D('',#1,#2,#3);\n#5=CIRCLE('',#4,2.);\n\
         #6=CARTESIAN_POINT('',(0.,2.,0.));\n#7=VERTEX_POINT('',#6);\n\
         #8=CARTESIAN_POINT('',(0.,-2.,0.));\n#9=VERTEX_POINT('',#8);\n\
         #10=EDGE_CURVE('',#9,#7,#5,.F.);\n\
         #11=DIRECTION('',(0.,1.,0.));\n#12=VECTOR('',#11,4.);\n#13=LINE('',#8,#12);\n\
         #14=EDGE_CURVE('',#9,#7,#13,.T.);\n\
         #20=ORIENTED_EDGE('',*,*,#10,.T.);\n#21=ORIENTED_EDGE('',*,*,#14,.F.);\n\
         #22=EDGE_LOOP('',(#20,#21));\n#23=FACE_BOUND('',#22,.T.);\n#24=PLANE('',#4);\n\
         #25=ADVANCED_FACE('',(#23),#24,.T.);\n\
         #30=ORIENTED_EDGE('',*,*,#10,.F.);\n#31=EDGE_LOOP('',(#30));\n#32=FACE_BOUND('',#31,.T.);\n\
         #33=CYLINDRICAL_SURFACE('',#4,2.);\n#34=ADVANCED_FACE('',(#32),#33,.T.);\n",
    );
    let tol = Tolerance::from_mm(1e-3, 0.3).unwrap();
    let p = Polylines::build(&g, [node(&g, 10), node(&g, 14)], 1., tol);
    let disc: Vec<_> = p.oriented(&g, node(&g, 20)).unwrap().collect();
    let mut cyl: Vec<_> = p.oriented(&g, node(&g, 30)).unwrap().collect();
    assert!(disc.len() > 2);
    assert_eq!(disc[0].1, [0., -2., 0.], "the start vertex first");
    assert!(disc[1].1[0] < 0., "against the circle: through x < 0");
    cyl.reverse();
    assert_eq!(disc, cyl, "the same points, in opposite order");
    let index: Vec<u32> = disc.iter().map(|(e, _)| e.index).collect();
    assert_eq!(index, (0..disc.len() as u32).collect::<Vec<_>>());
    assert!(
        p.oriented(&g, node(&g, 10)).is_err(),
        "not an oriented edge"
    );
    let unbuilt = Polylines::build(&g, [node(&g, 14)], 1., tol);
    assert_eq!(
        unbuilt.oriented(&g, node(&g, 20)).err(),
        Some(Reason("edge not built"))
    );
}

#[test]
fn a_non_finite_conic_centre_is_routed_not_nan_or_one_chord() {
    // `1.E400` reads as +inf: the centre's parameter would be NaN, a closed
    // edge NaN points and an open one a single chord.
    let g = file(
        "#1=CARTESIAN_POINT('',(1.E400,0.,0.));\n#2=AXIS2_PLACEMENT_3D('',#1,$,$);\n\
         #3=CIRCLE('',#2,1.);\n#4=CARTESIAN_POINT('',(1.,0.,0.));\n#5=VERTEX_POINT('',#4);\n\
         #6=CARTESIAN_POINT('',(0.,1.,0.));\n#7=VERTEX_POINT('',#6);\n\
         #8=EDGE_CURVE('',#5,#5,#3,.T.);\n#9=EDGE_CURVE('',#5,#7,#3,.T.);\n",
    );
    let tol = Tolerance::from_mm(1e-3, 0.3).unwrap();
    let [closed, open] = [8, 9].map(|id| node(&g, id));
    let p = Polylines::build(&g, [closed, open], 1., tol);
    for e in [closed, open] {
        assert_eq!(p.get(e).unwrap().unwrap_err(), Reason("non-finite conic"));
    }
    assert_eq!(p.point_count(), 0);
}

#[test]
fn a_non_finite_step_count_is_refused_before_n_min() {
    // `NaN.ceil().max(1.)` is 1: a NaN count must not become one segment.
    assert_eq!(segments(f64::NAN, false), Err(Reason("non-finite sweep")));
    assert_eq!(segments(f64::NAN, true), Err(Reason("non-finite sweep")));
    assert_eq!(
        segments(f64::INFINITY, false),
        Err(Reason("too many segments"))
    );
    assert_eq!(segments(2.5, false), Ok(3));
    assert_eq!(segments(0.5, true), Ok(3));
}

#[test]
fn a_closed_edge_whose_vertex_overflows_from_the_centre_is_routed() {
    // The centre and the vertex are finite, `p0 − c` is −inf, and its
    // parameter `atan2(−inf·0, …)` NaN; the closed sweep is still 2π.
    let g = file(
        "#1=CARTESIAN_POINT('',(1E308,0.,0.));\n#2=AXIS2_PLACEMENT_3D('',#1,$,$);\n\
         #3=CIRCLE('',#2,1.);\n#4=CARTESIAN_POINT('',(-1E308,0.,0.));\n#5=VERTEX_POINT('',#4);\n\
         #6=EDGE_CURVE('',#5,#5,#3,.T.);\n",
    );
    let tol = Tolerance::from_mm(1e-3, 0.3).unwrap();
    let e = node(&g, 6);
    let p = Polylines::build(&g, [e], 1., tol);
    assert_eq!(
        p.get(e).unwrap().unwrap_err(),
        Reason("non-finite parameter")
    );
    assert_eq!(p.point_count(), 0);
}

#[test]
fn a_line_that_overflows_in_mm_is_routed() {
    // 1E307 inches is 2.54E308 mm: past f64::MAX.
    let g = file(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=VERTEX_POINT('',#1);\n\
         #3=CARTESIAN_POINT('',(1E307,0.,0.));\n#4=VERTEX_POINT('',#3);\n\
         #5=DIRECTION('',(1.,0.,0.));\n#6=VECTOR('',#5,1.);\n#7=LINE('',#1,#6);\n\
         #8=EDGE_CURVE('',#2,#4,#7,.T.);\n",
    );
    let tol = Tolerance::from_mm(1e-3, 0.3).unwrap();
    let e = node(&g, 8);
    let p = Polylines::build(&g, [e], 25.4, tol);
    assert_eq!(
        p.get(e).unwrap().unwrap_err(),
        Reason("not a finite 3-D point")
    );
    assert_eq!(p.point_count(), 0);
}

#[test]
fn no_non_finite_coordinate_escapes_at_the_overflow_corners() {
    // Huge coordinates, radii and unit scales, on lines, circles and
    // ellipses: every edge is either routed or entirely finite.
    let mut built = 0;
    for big in [1., 1e300, 1e307, 1e308, f64::MAX] {
        for s in [1., 25.4, 1000.] {
            for (cx, vx, r) in [
                (big, -big, 1.),
                (big, big + 1., 1.),
                (0., big, big),
                (-big, 0., big),
                (big / 2., -big / 2., big),
            ] {
                let g = file(&format!(
                    "#1=CARTESIAN_POINT('',({cx:?},0.,0.));\n#2=AXIS2_PLACEMENT_3D('',#1,$,$);\n\
                     #3=CIRCLE('',#2,{r:?});\n#4=ELLIPSE('',#2,{r:?},{:?});\n\
                     #5=CARTESIAN_POINT('',({vx:?},0.,0.));\n#6=VERTEX_POINT('',#5);\n\
                     #7=CARTESIAN_POINT('',({cx:?},{r:?},0.));\n#8=VERTEX_POINT('',#7);\n\
                     #9=DIRECTION('',(1.,0.,0.));\n#10=VECTOR('',#9,1.);\n#11=LINE('',#1,#10);\n\
                     #12=EDGE_CURVE('',#6,#8,#11,.T.);\n#13=EDGE_CURVE('',#6,#6,#3,.T.);\n\
                     #14=EDGE_CURVE('',#6,#8,#3,.F.);\n#15=EDGE_CURVE('',#6,#6,#4,.T.);\n\
                     #16=EDGE_CURVE('',#6,#8,#4,.T.);\n",
                    r / 2.
                ));
                let tol = Tolerance::from_mm(1e-3, 0.3).unwrap();
                let edges = (12..=16).map(|id| node(&g, id));
                let p = Polylines::build(&g, edges, s, tol);
                for (e, line) in p.edges() {
                    built += 1;
                    assert!(
                        line.points().flatten().all(f64::is_finite),
                        "big {big:e} s {s} #{}: {:?}",
                        g.id(e).0,
                        line.points().take(3).collect::<Vec<_>>()
                    );
                }
            }
        }
    }
    assert!(built > 0, "the finite cases build");
    // Finite centre, radius and vertices, (c, ∓r): the arc through t = 0
    // reaches c + r, past f64::MAX. Only the exit check sees it.
    let g = file(
        "#1=CARTESIAN_POINT('',(1.5E308,0.,0.));\n#2=AXIS2_PLACEMENT_3D('',#1,$,$);\n\
         #3=CIRCLE('',#2,1E308);\n#4=CARTESIAN_POINT('',(1.5E308,-1E308,0.));\n\
         #5=VERTEX_POINT('',#4);\n#6=CARTESIAN_POINT('',(1.5E308,1E308,0.));\n\
         #7=VERTEX_POINT('',#6);\n#8=EDGE_CURVE('',#5,#7,#3,.T.);\n",
    );
    let e = node(&g, 8);
    // d at the radius' scale, so the count stays small.
    let p = Polylines::build(&g, [e], 1., Tolerance::from_mm(1e306, 0.3).unwrap());
    assert_eq!(p.get(e).unwrap().unwrap_err(), Reason("non-finite point"));
    assert_eq!(p.point_count(), 0, "nothing of it is left behind");
}
