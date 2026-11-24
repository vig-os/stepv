//! Curved analytic faces (ADR-0001 §2.6, M3) on fixture solids, against
//! the exact surfaces: watertight once a seam's and a pole's duplicates are
//! identified (every interior edge in exactly two triangles, every boundary
//! segment in one), the area within the deflection's bound of the exact
//! area, every triangle within d of the surface and its vertex normals
//! within a of each other, every output finite, and every failure routed
//! with its reason. Also the #210 floor.

use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

use super::*;
use crate::graph::{Reason, StepId};
use crate::mesh::face::{FLOOR_MM, FLOORED};

/// A STEP file under construction.
#[derive(Default)]
struct F {
    out: String,
    n: u32,
}

fn num(x: f64) -> String {
    format!("{x:.17}")
}

impl F {
    fn add(&mut self, r: &str) -> u32 {
        self.n += 1;
        self.out += &format!("#{}={r};\n", self.n);
        self.n
    }
    fn pt(&mut self, p: [f64; 3]) -> u32 {
        let [x, y, z] = p.map(num);
        self.add(&format!("CARTESIAN_POINT('',({x},{y},{z}))"))
    }
    fn dir(&mut self, d: [f64; 3]) -> u32 {
        let [x, y, z] = d.map(num);
        self.add(&format!("DIRECTION('',({x},{y},{z}))"))
    }
    /// An axis placement at `o`, axis `z`, x axis `x`.
    fn ax(&mut self, o: [f64; 3], z: [f64; 3], x: [f64; 3]) -> u32 {
        let (o, z, x) = (self.pt(o), self.dir(z), self.dir(x));
        self.add(&format!("AXIS2_PLACEMENT_3D('',#{o},#{z},#{x})"))
    }
    fn vertex(&mut self, p: [f64; 3]) -> u32 {
        let p = self.pt(p);
        self.add(&format!("VERTEX_POINT('',#{p})"))
    }
    fn edge(&mut self, a: u32, b: u32, curve: u32) -> u32 {
        self.add(&format!("EDGE_CURVE('',#{a},#{b},#{curve},.T.)"))
    }
    fn line(&mut self, va: u32, vb: u32, a: [f64; 3], b: [f64; 3]) -> u32 {
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let len = d.iter().map(|c| c * c).sum::<f64>().sqrt();
        let (p, dir) = (self.pt(a), self.dir(d.map(|c| c / len)));
        let v = self.add(&format!("VECTOR('',#{dir},{})", num(len)));
        let l = self.add(&format!("LINE('',#{p},#{v})"));
        self.edge(va, vb, l)
    }
    /// A circle of radius `r` about `o` in the plane normal to `z`, from
    /// `x` counter-clockwise about `z`.
    fn circle(&mut self, o: [f64; 3], z: [f64; 3], x: [f64; 3], r: f64) -> u32 {
        let ax = self.ax(o, z, x);
        self.add(&format!("CIRCLE('',#{ax},{})", num(r)))
    }
    fn oe(&mut self, e: u32, sense: bool) -> u32 {
        let s = if sense { "T" } else { "F" };
        self.add(&format!("ORIENTED_EDGE('',*,*,#{e},.{s}.)"))
    }
    fn bound(&mut self, oes: &[(u32, bool)]) -> u32 {
        let l: Vec<String> = oes
            .iter()
            .map(|&(e, s)| format!("#{}", self.oe(e, s)))
            .collect();
        let lp = self.add(&format!("EDGE_LOOP('',({}))", l.join(",")));
        self.add(&format!("FACE_BOUND('',#{lp},.T.)"))
    }
    fn face(&mut self, bounds: &[u32], surface: u32, same_sense: bool) -> u32 {
        let b: Vec<String> = bounds.iter().map(|e| format!("#{e}")).collect();
        let s = if same_sense { "T" } else { "F" };
        self.add(&format!(
            "ADVANCED_FACE('',({}),#{surface},.{s}.)",
            b.join(",")
        ))
    }
    /// A surface record `kind` on the z axis through `o`, with `args`.
    fn surface(&mut self, kind: &str, o: [f64; 3], args: &str) -> u32 {
        let ax = self.ax(o, [0., 0., 1.], [1., 0., 0.]);
        self.add(&format!("{kind}('',#{ax},{args})"))
    }
    fn graph(&self) -> Graph {
        let text = format!(
            "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('t','',(''),(''),'','','');\n\
             FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA;\n{}ENDSEC;\nEND-ISO-10303-21;\n",
            self.out
        );
        Graph::from_bytes(text.into_bytes()).unwrap()
    }
}

const CTX: Context = Context {
    mm_per_unit: 1.,
    rad_per_unit: 1.,
    uncertainty: 1e-7,
    floored: false,
};

fn tol() -> Tolerance {
    Tolerance::from_mm(1e-3, 0.3).unwrap()
}

fn node(g: &Graph, id: u32) -> Node {
    g.node(StepId(id)).unwrap()
}

/// Meshes face `#face` of `f` within `tol`.
fn run_with(
    f: &F,
    face: u32,
    tol: Tolerance,
    reversed: bool,
) -> (Graph, Polylines, Result<FaceMesh, Route>) {
    let g = f.graph();
    let p = Polylines::build(&g, g.of_type("EDGE_CURVE"), 1., tol);
    let m = mesh(&g, node(&g, face), &p, CTX, tol, reversed);
    (g, p, m)
}

fn run(f: &F, face: u32) -> (Graph, Polylines, FaceMesh) {
    let (g, p, m) = run_with(f, face, tol(), false);
    (g, p, m.unwrap())
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The exact surface, written apart from the mesher: a point's distance
/// from it and its outward unit normal there.
struct Exact {
    dist: Box<dyn Fn([f64; 3]) -> f64>,
    normal: Box<dyn Fn([f64; 3]) -> [f64; 3]>,
}

/// On the z axis through the origin.
fn cylinder(r: f64) -> Exact {
    Exact {
        dist: Box::new(move |p| (p[0].hypot(p[1]) - r).abs()),
        normal: Box::new(|p| {
            let l = p[0].hypot(p[1]);
            [p[0] / l, p[1] / l, 0.]
        }),
    }
}

/// Apex at the origin, opening towards +z with semi-angle `t`.
fn cone(t: f64) -> Exact {
    Exact {
        dist: Box::new(move |p| (p[0].hypot(p[1]) * t.cos() - p[2] * t.sin()).abs()),
        normal: Box::new(move |p| {
            let l = p[0].hypot(p[1]);
            [t.cos() * p[0] / l, t.cos() * p[1] / l, -t.sin()]
        }),
    }
}

/// Every invariant of a mesh of face `face`, against `exact`, whose area is
/// `area` (bounded by `length` mm of curved edges): all outputs finite;
/// every boundary vertex its edge point, bit for bit; vertex normals unit
/// and the exact surface's (flipped by `outward`); watertight once equal
/// 3-D points are identified; every triangle wound about its normals, its
/// vertices' normals within a, its points (an order-8 barycentric grid)
/// within d of the surface; the area within `A·(1 − cos a) + d·length`.
#[allow(clippy::too_many_arguments)] // the fixture and what it should be
fn check(
    g: &Graph,
    p: &Polylines,
    face: u32,
    m: &FaceMesh,
    exact: &Exact,
    area: f64,
    length: f64,
    outward: f64,
) {
    let (d, a) = (tol().linear(), tol().angular());
    let n = m.x.len();
    for v in [&m.x, &m.y, &m.z, &m.nx, &m.ny, &m.nz] {
        assert_eq!(v.len(), n);
        assert!(v.iter().all(|c| c.is_finite()), "non-finite output");
    }
    for (i, ep) in m.points.iter().enumerate() {
        let q = p.get(ep.edge).unwrap().unwrap().point(ep.index as usize);
        assert_eq!(m.point(i), q, "vertex {i} is its edge point");
    }
    for i in 0..n {
        let (q, nm) = (m.point(i), m.normal(i));
        assert!(
            (exact.dist)(q) <= 1e-7 + 1e-12,
            "vertex {i} off the surface"
        );
        let len = nm.iter().map(|c| c * c).sum::<f64>().sqrt();
        assert!((len - 1.).abs() < 1e-12);
        // At an axis point the exact normal is undefined: the apex.
        if q[0].hypot(q[1]) > 1e-9 {
            let e = (exact.normal)(q);
            let c: f64 = (0..3).map(|k| e[k] * nm[k]).sum();
            assert!(
                (c - outward).abs() < 1e-9,
                "vertex {i}: normal {nm:?}, exact {e:?}"
            );
        }
    }
    // Vertices identified by their 3-D point.
    let mut id: HashMap<[u64; 3], usize> = HashMap::new();
    let ids: Vec<usize> = (0..n)
        .map(|i| {
            let k = id.len();
            *id.entry(m.point(i).map(f64::to_bits)).or_insert(k)
        })
        .collect();
    let mut uses: HashMap<(usize, usize), usize> = HashMap::new();
    let mut total = 0.;
    for t in &m.triangles {
        let [pa, pb, pc] = t.map(|i| m.point(i as usize));
        let nt = cross(sub(pb, pa), sub(pc, pa));
        let twice = nt.iter().map(|c| c * c).sum::<f64>().sqrt();
        assert!(twice > 0., "a degenerate triangle {t:?}");
        total += twice / 2.;
        for &i in t {
            let nm = m.normal(i as usize);
            assert!(
                (0..3).map(|k| nm[k] * nt[k]).sum::<f64>() > 0.,
                "triangle {t:?} wound against its normals: {:?} {:?}",
                t.map(|i| m.point(i as usize)),
                t.map(|i| m.normal(i as usize))
            );
            for &j in t {
                let c: f64 = (0..3).map(|k| nm[k] * m.normal(j as usize)[k]).sum();
                assert!(
                    c.clamp(-1., 1.).acos() <= a * (1. + 1e-9),
                    "normals {i}, {j} turn past a"
                );
            }
        }
        for i in 0..=8 {
            for j in 0..=8 - i {
                let k = 8 - i - j;
                let q = [0, 1, 2].map(|c| {
                    (f64::from(i) * pa[c] + f64::from(j) * pb[c] + f64::from(k) * pc[c]) / 8.
                });
                assert!(
                    (exact.dist)(q) <= d * (1. + 1e-9),
                    "triangle {t:?} off the surface by {}",
                    (exact.dist)(q)
                );
            }
        }
        for e in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (x, y) = (ids[e.0 as usize], ids[e.1 as usize]);
            *uses.entry((x.min(y), x.max(y))).or_default() += 1;
        }
    }
    // The boundary: segments of edges the face uses once; a seam's (used
    // twice) are interior.
    let mut once: HashMap<(usize, usize), ()> = HashMap::new();
    let mut count: HashMap<Node, usize> = HashMap::new();
    for ep in &m.points {
        count.entry(ep.edge).or_default();
    }
    let (bounds, _, _) = curve::advanced_face(g, node(g, face)).unwrap();
    for b in bounds {
        if curve::vertex_loop(g, b).is_some() {
            continue;
        }
        for oe in curve::face_bound(g, b).unwrap().0 {
            *count
                .entry(curve::oriented_edge(g, oe).unwrap().0)
                .or_default() += 1;
        }
    }
    let key = |q: [f64; 3]| id[&q.map(f64::to_bits)];
    for (&e, &c) in &count {
        if c == 1 {
            let pl = p.get(e).unwrap().unwrap();
            for k in 1..pl.len() {
                let (x, y) = (key(pl.point(k - 1)), key(pl.point(k)));
                once.insert((x.min(y), x.max(y)), ());
            }
        }
    }
    for (e, &u) in &uses {
        let want = if once.contains_key(e) { 1 } else { 2 };
        let at = |k: usize| {
            (0..n)
                .filter(|&i| ids[i] == k)
                .map(|i| (i, m.point(i)))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            u,
            want,
            "edge {e:?} used {u} times, want {want}: {:?} {:?}",
            at(e.0),
            at(e.1)
        );
    }
    assert!(
        once.keys().all(|e| uses.contains_key(e)),
        "a boundary segment in no triangle"
    );
    let bound = area * (1. - a.cos()) + d * length;
    assert!(
        (total - area).abs() <= bound,
        "area {total}, exact {area}, bound {bound}"
    );
}

/// A cylinder of radius 1 about z from z = 0 to 2, its band bounded by two
/// circles and the seam line at u = 0 (OCCT's form).
fn full_cylinder(f: &mut F, same_sense: bool) -> u32 {
    let (z, x) = ([0., 0., 1.], [1., 0., 0.]);
    let (p0, p1) = ([1., 0., 0.], [1., 0., 2.]);
    let (v0, v1) = (f.vertex(p0), f.vertex(p1));
    let c0 = f.circle([0.; 3], z, x, 1.);
    let c1 = f.circle([0., 0., 2.], z, x, 1.);
    let (bottom, top) = (f.edge(v0, v0, c0), f.edge(v1, v1, c1));
    let seam = f.line(v0, v1, p0, p1);
    let cyl = f.surface("CYLINDRICAL_SURFACE", [0.; 3], "1.");
    // Outward, the face lies left of the bottom walked +u; inward, right.
    let b = if same_sense {
        f.bound(&[(bottom, true), (seam, true), (top, false), (seam, false)])
    } else {
        f.bound(&[(seam, true), (top, true), (seam, false), (bottom, false)])
    };
    f.face(&[b], cyl, same_sense)
}

#[test]
fn a_full_cylinder_with_a_seam_is_watertight() {
    let mut f = F::default();
    let face = full_cylinder(&mut f, true);
    let (g, p, m) = run(&f, face);
    check(&g, &p, face, &m, &cylinder(1.), 4. * PI, 4. * PI, 1.);
    // Reversed in its shell, every normal points in.
    let (g, p, r) = run_with(&f, face, tol(), true);
    let r = r.unwrap();
    check(&g, &p, face, &r, &cylinder(1.), 4. * PI, 4. * PI, -1.);
}

#[test]
fn a_full_cylinder_facing_in() {
    // same_sense .F.: the bore of a hole, its normals towards the axis.
    let mut f = F::default();
    let face = full_cylinder(&mut f, false);
    let (g, p, m) = run(&f, face);
    check(&g, &p, face, &m, &cylinder(1.), 4. * PI, 4. * PI, -1.);
}

#[test]
fn a_partial_cylinder() {
    // A quarter: u from 0 to π/2, z from 0 to 2, two arcs and two lines.
    let mut f = F::default();
    let (z, x) = ([0., 0., 1.], [1., 0., 0.]);
    let pts = [[1., 0., 0.], [0., 1., 0.], [0., 1., 2.], [1., 0., 2.]];
    let v: Vec<u32> = pts.iter().map(|&q| f.vertex(q)).collect();
    let c0 = f.circle([0.; 3], z, x, 1.);
    let c1 = f.circle([0., 0., 2.], z, x, 1.);
    let lo = f.edge(v[0], v[1], c0);
    let up = f.line(v[1], v[2], pts[1], pts[2]);
    let hi = f.edge(v[3], v[2], c1);
    let down = f.line(v[3], v[0], pts[3], pts[0]);
    let b = f.bound(&[(lo, true), (up, true), (hi, false), (down, true)]);
    let cyl = f.surface("CYLINDRICAL_SURFACE", [0.; 3], "1.");
    let face = f.face(&[b], cyl, true);
    let (g, p, m) = run(&f, face);
    check(&g, &p, face, &m, &cylinder(1.), PI, PI, 1.);
}

/// A cone with its apex at the origin, semi-angle 30°, its frame at z = 1
/// (radius tan 30° there), from the apex to its base circle at z = 2,
/// bounded by the base and a seam line to the apex.
fn cone_to_apex(f: &mut F, semi: &str) -> (u32, f64) {
    let t = 30f64.to_radians();
    let rb = 2. * t.tan();
    let (pa, pb) = ([0.; 3], [rb, 0., 2.]);
    let (va, vb) = (f.vertex(pa), f.vertex(pb));
    let base = f.circle([0., 0., 2.], [0., 0., 1.], [1., 0., 0.], rb);
    let base = f.edge(vb, vb, base);
    let seam = f.line(va, vb, pa, pb);
    let cone = f.surface(
        "CONICAL_SURFACE",
        [0., 0., 1.],
        &format!("{},{semi}", num(t.tan())),
    );
    let b = f.bound(&[(seam, true), (base, false), (seam, false)]);
    (f.face(&[b], cone, true), t)
}

#[test]
fn a_cone_down_to_its_apex() {
    // OCCT's form: a seam edge along a ruling down to the apex.
    let mut f = F::default();
    let (face, t) = cone_to_apex(&mut f, &num(30f64.to_radians()));
    let (g, p, m) = run(&f, face);
    let rb = 2. * t.tan();
    check(
        &g,
        &p,
        face,
        &m,
        &cone(t),
        PI * rb * 2. / t.cos(),
        TAU * rb,
        1.,
    );
    // The apex is a run of vertices, one point, each with its ruling's normal.
    let apex: Vec<usize> = (0..m.x.len()).filter(|&i| m.point(i) == [0.; 3]).collect();
    assert!(apex.len() > 2);
    assert_ne!(m.normal(apex[0]), m.normal(apex[1]));
}

/// The cone of [`cone_to_apex`] bounded by its base circle alone, with a
/// `VERTEX_LOOP` at the apex when `vertex_loop`.
fn cone_base_only(f: &mut F, vertex_loop: bool, semi: &str) -> (u32, f64) {
    let t = 30f64.to_radians();
    let rb = 2. * t.tan();
    let vb = f.vertex([rb, 0., 2.]);
    let base = f.circle([0., 0., 2.], [0., 0., 1.], [1., 0., 0.], rb);
    let base = f.edge(vb, vb, base);
    let cone = f.surface(
        "CONICAL_SURFACE",
        [0., 0., 1.],
        &format!("{},{semi}", num(t.tan())),
    );
    let mut bounds = vec![f.bound(&[(base, false)])];
    if vertex_loop {
        let va = f.vertex([0.; 3]);
        let lp = f.add(&format!("VERTEX_LOOP('',#{va})"));
        bounds.push(f.add(&format!("FACE_BOUND('',#{lp},.T.)")));
    }
    (f.face(&bounds, cone, true), t)
}

#[test]
fn a_cone_to_its_apex_without_a_seam_edge() {
    // ABC's form: the base circle and a VERTEX_LOOP at the apex; and the
    // base circle alone, the apex the cone's own.
    for vertex_loop in [true, false] {
        let mut f = F::default();
        let (face, t) = cone_base_only(&mut f, vertex_loop, &num(30f64.to_radians()));
        let (g, p, m) = run(&f, face);
        let rb = 2. * t.tan();
        check(
            &g,
            &p,
            face,
            &m,
            &cone(t),
            PI * rb * 2. / t.cos(),
            TAU * rb,
            1.,
        );
        assert!(
            (0..m.x.len()).any(|i| m.point(i) == [0.; 3]),
            "the apex is a vertex"
        );
    }
}

#[test]
fn a_cone_in_degrees() {
    // The plane angle unit is a degree: the semi-angle is 30, not 30 rad.
    let mut f = F::default();
    let (face, t) = cone_base_only(&mut f, true, "30.");
    let g = f.graph();
    let p = Polylines::build(&g, g.of_type("EDGE_CURVE"), 1., tol());
    let ctx = Context {
        rad_per_unit: PI / 180.,
        ..CTX
    };
    let m = mesh(&g, node(&g, face), &p, ctx, tol(), false).unwrap();
    let rb = 2. * t.tan();
    check(
        &g,
        &p,
        face,
        &m,
        &cone(t),
        PI * rb * 2. / t.cos(),
        TAU * rb,
        1.,
    );
}

#[test]
fn a_cylinder_cut_by_a_cylinder_along_a_b_spline() {
    // The cylinder x² + y² = 1 cut by its twin x² + z² = 1 meets it in the
    // ellipse (cos t, sin t, sin t): written as a rational
    // quadratic B-spline, exactly (the NURBS circle, mapped). The face runs
    // from it up to the circle at z = 3, with a seam line at u = 0.
    let mut f = F::default();
    let ctrl = [
        [1., 0.],
        [1., 1.],
        [0., 1.],
        [-1., 1.],
        [-1., 0.],
        [-1., -1.],
        [0., -1.],
        [1., -1.],
        [1., 0.],
    ];
    let refs: Vec<String> = ctrl
        .iter()
        .map(|q| format!("#{}", f.pt([q[0], q[1], q[1]])))
        .collect();
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let w: Vec<String> = (0..9)
        .map(|k| num(if k % 2 == 1 { h } else { 1. }))
        .collect();
    let bs = f.add(&format!(
        "(BOUNDED_CURVE() B_SPLINE_CURVE(2,({}),.UNSPECIFIED.,.T.,.F.) \
         B_SPLINE_CURVE_WITH_KNOTS((3,2,2,2,3),(0.,0.25,0.5,0.75,1.),.UNSPECIFIED.) CURVE() \
         GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_CURVE(({})) REPRESENTATION_ITEM(''))",
        refs.join(","),
        w.join(",")
    ));
    let (p0, p1) = ([1., 0., 0.], [1., 0., 3.]);
    let (v0, v1) = (f.vertex(p0), f.vertex(p1));
    let cut = f.edge(v0, v0, bs);
    let c1 = f.circle([0., 0., 3.], [0., 0., 1.], [1., 0., 0.], 1.);
    let top = f.edge(v1, v1, c1);
    let seam = f.line(v0, v1, p0, p1);
    let cyl = f.surface("CYLINDRICAL_SURFACE", [0.; 3], "1.");
    let b = f.bound(&[(cut, true), (seam, true), (top, false), (seam, false)]);
    let face = f.face(&[b], cyl, true);
    let (g, p, m) = run(&f, face);
    // ∫ (3 − sin t) dt over a turn; the ellipse is under 2π·√1.5 long.
    let length = TAU + TAU * 1.5f64.sqrt();
    check(&g, &p, face, &m, &cylinder(1.), 6. * PI, length, 1.);
    // Without the seam edge: a virtual seam from the ellipse to the circle.
    let (b0, b1) = (f.bound(&[(cut, true)]), f.bound(&[(top, false)]));
    let face = f.face(&[b0, b1], cyl, true);
    let (g, p, m) = run(&f, face);
    check(&g, &p, face, &m, &cylinder(1.), 6. * PI, length, 1.);
}

/// The bounds of a band of the unit cylinder from z = 0 to 2, two circles
/// and no seam edge, and the cylinder.
fn band(f: &mut F) -> (u32, u32, u32) {
    let (z, x) = ([0., 0., 1.], [1., 0., 0.]);
    let (v0, v1) = (f.vertex([1., 0., 0.]), f.vertex([1., 0., 2.]));
    let c0 = f.circle([0.; 3], z, x, 1.);
    let c1 = f.circle([0., 0., 2.], z, x, 1.);
    let (lo, hi) = (f.edge(v0, v0, c0), f.edge(v1, v1, c1));
    let cyl = f.surface("CYLINDRICAL_SURFACE", [0.; 3], "1.");
    (f.bound(&[(lo, true)]), f.bound(&[(hi, false)]), cyl)
}

#[test]
fn a_seamless_band_is_cut_by_a_virtual_seam() {
    // Two circles, no seam edge (ABC's form): the virtual seam runs from the
    // bottom circle's start to the top's, and the band is watertight once
    // its two copies are identified.
    let mut f = F::default();
    let (b0, b1, cyl) = band(&mut f);
    let face = f.face(&[b0, b1], cyl, true);
    let (g, p, m) = run(&f, face);
    check(&g, &p, face, &m, &cylinder(1.), 4. * PI, 4. * PI, 1.);
    // Its bounds the other way round, and facing in.
    let other = f.face(&[b1, b0], cyl, true);
    let (g, p, m) = run(&f, other);
    check(&g, &p, other, &m, &cylinder(1.), 4. * PI, 4. * PI, 1.);
}

#[test]
fn a_seam_through_a_hole_is_routed() {
    // A window in the band across u = 0, where the seam runs: two arcs at
    // z = 0.8 and 1.2 from u = −0.2 to 0.2, and two rulings, walked
    // clockwise in (u, v).
    let mut f = F::default();
    let (b0, b1, cyl) = band(&mut f);
    let (s, c) = (0.2f64.sin(), 0.2f64.cos());
    let pts = [[c, -s, 0.8], [c, -s, 1.2], [c, s, 1.2], [c, s, 0.8]];
    let v: Vec<u32> = pts.iter().map(|&q| f.vertex(q)).collect();
    let (z, x) = ([0., 0., 1.], [1., 0., 0.]);
    let up = f.line(v[0], v[1], pts[0], pts[1]);
    let a1 = f.circle([0., 0., 1.2], z, x, 1.);
    let top = f.edge(v[1], v[2], a1);
    let down = f.line(v[2], v[3], pts[2], pts[3]);
    let a0 = f.circle([0., 0., 0.8], z, x, 1.);
    let bottom = f.edge(v[0], v[3], a0);
    let hole = f.bound(&[(up, true), (top, true), (down, true), (bottom, false)]);
    let face = f.face(&[b0, b1, hole], cyl, true);
    assert_eq!(
        run_with(&f, face, tol(), false).2.unwrap_err(),
        reason("loop:seam-crosses")
    );
}

fn reason(r: &'static str) -> Route {
    Route::Face(Reason(r))
}

#[test]
fn failures_route_with_their_reason() {
    // A band bounded by two circles, no seam edge: it wraps the axis.
    let mut f = F::default();
    let (z, x) = ([0., 0., 1.], [1., 0., 0.]);
    let (b0, b1, cyl) = band(&mut f);
    // One wrapping loop with no partner, on a cylinder (no apex).
    let lone = f.face(&[b0], cyl, true);
    assert_eq!(
        run_with(&f, lone, tol(), false).2.unwrap_err(),
        reason("loop:seam-unresolved")
    );
    // The same circles on a cylinder of radius 1.001.
    let wide = f.surface("CYLINDRICAL_SURFACE", [0.; 3], "1.001");
    let off = f.face(&[b0, b1], wide, true);
    assert_eq!(
        run_with(&f, off, tol(), false).2.unwrap_err(),
        reason("curve:off-surface")
    );
    // On a plane, a torus (M3b), or a cone of semi-angle 0.
    let ax = f.ax([0.; 3], z, x);
    let plane = f.add(&format!("PLANE('',#{ax})"));
    let pf = f.face(&[b0], plane, true);
    assert_eq!(
        run_with(&f, pf, tol(), false).2.unwrap_err(),
        reason("surface:not analytic")
    );
    let torus = f.surface("TOROIDAL_SURFACE", [0.; 3], "3.,1.");
    let sf = f.face(&[b0], torus, true);
    assert_eq!(
        run_with(&f, sf, tol(), false).2.unwrap_err(),
        reason("surface:TOROIDAL_SURFACE")
    );
    let flat = f.surface("CONICAL_SURFACE", [0.; 3], "1.,0.");
    let cf = f.face(&[b0], flat, true);
    assert_eq!(
        run_with(&f, cf, tol(), false).2.unwrap_err(),
        reason("surface:degenerate cone")
    );
    // A triangle of secant lines across the cylinder: its chord from u = 0
    // to u = 1 is a constraint no triangle can follow within d.
    let pts = [[1., 0., 0.], [1f64.cos(), 1f64.sin(), 0.], [1., 0., 1.]];
    let v: Vec<u32> = pts.iter().map(|&q| f.vertex(q)).collect();
    let e: Vec<u32> = (0..3)
        .map(|k| f.line(v[k], v[(k + 1) % 3], pts[k], pts[(k + 1) % 3]))
        .collect();
    let b = f.bound(&[(e[0], true), (e[1], true), (e[2], true)]);
    let secant = f.face(&[b], cyl, true);
    assert_eq!(
        run_with(&f, secant, tol(), false).2.unwrap_err(),
        reason("rs-fail:deflection")
    );
    // The same, walked against the topological normal.
    let inward = f.face(&[b], cyl, false);
    assert_eq!(
        run_with(&f, inward, tol(), false).2.unwrap_err(),
        reason("rs-fail:orientation")
    );
    // A circle of radius 0: the edge is routed, and so the face.
    let c = f.circle([0.; 3], z, x, 0.);
    let v = f.vertex([0.; 3]);
    let e = f.edge(v, v, c);
    let b = f.bound(&[(e, true)]);
    let ef = f.face(&[b], cyl, true);
    assert_eq!(
        run_with(&f, ef, tol(), false).2.unwrap_err(),
        Route::Edge(Reason("degenerate conic"))
    );
}

#[test]
fn the_uncertainty_floor() {
    // #210: below 1e-7 mm, or absent, the context is raised and recorded.
    let low = Context::new(1., 1., Some(1e-17));
    assert_eq!(low.uncertainty, FLOOR_MM);
    assert!(low.floored);
    assert_eq!(low.reason(), Some(FLOORED));
    assert_eq!(FLOORED.to_string(), "uncertainty:floored");
    assert!(Context::new(1., 1., None).floored);
    let high = Context::new(1., 1., Some(1e-3));
    assert_eq!(high.uncertainty, 1e-3);
    assert!(!high.floored && high.reason().is_none());
    // From a file's context: a 1e-17 mm file is floored, a 1e-3 mm one isn't.
    let ctx = |unc: &str, angle: &str| {
        let text = format!(
            "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('t','',(''),(''),'','','');\n\
             FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA;\n\
             #1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n\
             #2=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\n{angle}\
             #4=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE({unc}),#1,'distance_accuracy_value','');\n\
             #5=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#4)) \
             GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#3)) REPRESENTATION_CONTEXT('',''));\n\
             #6=SHAPE_REPRESENTATION('',(),#5);\nENDSEC;\nEND-ISO-10303-21;\n"
        );
        let g = Graph::from_bytes(text.into_bytes()).unwrap();
        Context::of(&g, node(&g, 6))
    };
    let radian = "#3=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\n";
    let abc = ctx("1.E-17", radian);
    assert!(abc.floored);
    assert_eq!(abc.uncertainty, FLOOR_MM);
    let coarse = ctx("1.E-03", radian);
    assert!(!coarse.floored);
    assert_eq!(coarse.uncertainty, 1e-3);
    assert_eq!(coarse.rad_per_unit, 1.);
    // A degree, as a conversion-based unit of the radian.
    let degree = "#7=PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(0.0174532925199433),#2);\n\
                  #3=(CONVERSION_BASED_UNIT('DEGREE',#7) NAMED_UNIT(*) PLANE_ANGLE_UNIT());\n";
    assert_eq!(ctx("1.E-03", degree).rad_per_unit, 0.017_453_292_519_943_3);
}

#[test]
fn a_floored_context_meshes_a_point_a_declared_one_routes() {
    // A partial cylinder whose arc vertex is 5e-8 mm off the surface: within
    // the floor, beyond ABC's declared 1e-17 mm.
    let mut f = F::default();
    let (z, x) = ([0., 0., 1.], [1., 0., 0.]);
    let e = 5e-8;
    let pts = [[1. + e, 0., 0.], [0., 1., 0.], [0., 1., 2.], [1., 0., 2.]];
    let v: Vec<u32> = pts.iter().map(|&q| f.vertex(q)).collect();
    let c0 = f.circle([0.; 3], z, x, 1.);
    let c1 = f.circle([0., 0., 2.], z, x, 1.);
    let lo = f.edge(v[0], v[1], c0);
    let up = f.line(v[1], v[2], pts[1], pts[2]);
    let hi = f.edge(v[3], v[2], c1);
    let down = f.line(v[3], v[0], pts[3], pts[0]);
    let b = f.bound(&[(lo, true), (up, true), (hi, false), (down, true)]);
    let cyl = f.surface("CYLINDRICAL_SURFACE", [0.; 3], "1.");
    let face = f.face(&[b], cyl, true);
    let g = f.graph();
    let p = Polylines::build(&g, g.of_type("EDGE_CURVE"), 1., tol());
    let declared = Context {
        uncertainty: 1e-17,
        ..CTX
    };
    let r = mesh(&g, node(&g, face), &p, declared, tol(), false);
    assert_eq!(r.unwrap_err(), reason("curve:off-surface"));
    let floored = Context::new(1., 1., Some(1e-17));
    assert!(mesh(&g, node(&g, face), &p, floored, tol(), false).is_ok());
}
