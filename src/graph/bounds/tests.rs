//! One check per row of ADR-0001 §2.3's table, the 8-corner rule, `Unknown`
//! propagation and the non-finite rule; `tests/graph_bounds.rs` holds the
//! boxes to the kernel's.

use super::*;
use crate::graph::structure;

/// A STEP file built a record at a time; every product a part whose shape
/// representation lists the given items.
struct F {
    out: String,
    next: u32,
    inch: u32,
}

impl F {
    fn new() -> Self {
        let mut f = F {
            out: String::new(),
            next: 1,
            inch: 0,
        };
        let mm = f.add("(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.))");
        let m = f.add(&format!(
            "LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#{mm})"
        ));
        let e = f.add("DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.)");
        let inch = f.add(&format!(
            "(CONVERSION_BASED_UNIT('INCH',#{m}) LENGTH_UNIT() NAMED_UNIT(#{e}))"
        ));
        f.inch = f.add(&format!(
            "(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#{inch})) REPRESENTATION_CONTEXT('',''))"
        ));
        f
    }

    fn add(&mut self, record: &str) -> u32 {
        let id = self.next;
        self.next += 1;
        self.out += &format!("#{id}={record};\n");
        id
    }

    fn pt(&mut self, p: [f64; 3]) -> u32 {
        self.add(&format!(
            "CARTESIAN_POINT('',({:?},{:?},{:?}))",
            p[0], p[1], p[2]
        ))
    }

    fn ax(&mut self, at: [f64; 3], z: [f64; 3], x: [f64; 3]) -> u32 {
        let p = self.pt(at);
        let z = self.add(&format!("DIRECTION('',({:?},{:?},{:?}))", z[0], z[1], z[2]));
        let x = self.add(&format!("DIRECTION('',({:?},{:?},{:?}))", x[0], x[1], x[2]));
        self.add(&format!("AXIS2_PLACEMENT_3D('',#{p},#{z},#{x})"))
    }

    fn vtx(&mut self, p: [f64; 3]) -> u32 {
        let p = self.pt(p);
        self.add(&format!("VERTEX_POINT('',#{p})"))
    }

    /// An edge loop of edges `(v1, v2, curve)`.
    fn edge_loop(&mut self, edges: &[(u32, u32, u32)]) -> u32 {
        let oes: Vec<String> = edges
            .iter()
            .map(|&(a, b, c)| {
                let e = self.add(&format!("EDGE_CURVE('',#{a},#{b},#{c},.T.)"));
                format!("#{}", self.add(&format!("ORIENTED_EDGE('',*,*,#{e},.T.)")))
            })
            .collect();
        self.add(&format!("EDGE_LOOP('',({}))", oes.join(",")))
    }

    fn face(&mut self, loops: &[u32], surf: u32) -> u32 {
        let b: Vec<String> = loops
            .iter()
            .map(|l| format!("#{}", self.add(&format!("FACE_BOUND('',#{l},.T.)"))))
            .collect();
        self.add(&format!("ADVANCED_FACE('',({}),#{surf},.T.)", b.join(",")))
    }

    /// A solid of `faces`.
    fn solid(&mut self, faces: &[u32]) -> u32 {
        let l: Vec<String> = faces.iter().map(|f| format!("#{f}")).collect();
        let sh = self.add(&format!("CLOSED_SHELL('',({}))", l.join(",")));
        self.add(&format!("MANIFOLD_SOLID_BREP('',#{sh})"))
    }

    /// A product; returns its definition and its shape representation.
    fn part(&mut self, items: &[u32], inch: bool) -> (u32, u32) {
        let p = self.add("PRODUCT('p','p','',())");
        let f = self.add(&format!("PRODUCT_DEFINITION_FORMATION('','',#{p})"));
        let pd = self.add(&format!("PRODUCT_DEFINITION('','',#{f},$)"));
        let pds = self.add(&format!("PRODUCT_DEFINITION_SHAPE('','',#{pd})"));
        let l: Vec<String> = items.iter().map(|i| format!("#{i}")).collect();
        let ctx = if inch {
            format!("#{}", self.inch)
        } else {
            "$".into()
        };
        let sr = self.add(&format!("SHAPE_REPRESENTATION('',({}),{ctx})", l.join(",")));
        self.add(&format!("SHAPE_DEFINITION_REPRESENTATION(#{pds},#{sr})"));
        (pd, sr)
    }

    /// `child` in `parent`, placed by the change of coordinates from the
    /// identity to `at`.
    fn instance(&mut self, parent: (u32, u32), child: (u32, u32), at: u32) {
        let id = self.ax([0.; 3], Z, X);
        let n = self.add(&format!(
            "NEXT_ASSEMBLY_USAGE_OCCURRENCE('i','i','',#{},#{},$)",
            parent.0, child.0
        ));
        let t = self.add(&format!("ITEM_DEFINED_TRANSFORMATION('','',#{id},#{at})"));
        let rr = self.add(&format!("(REPRESENTATION_RELATIONSHIP('','',#{},#{}) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{t}) SHAPE_REPRESENTATION_RELATIONSHIP())", child.1, parent.1));
        let pds = self.add(&format!("PRODUCT_DEFINITION_SHAPE('','',#{n})"));
        self.add(&format!(
            "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{rr},#{pds})"
        ));
    }

    fn bounds(&self) -> (Graph, Bounds) {
        let g = Graph::from_bytes(
            format!(
                "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('t','',(''),(''),'','','');\n\
                 FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA;\n{}ENDSEC;\nEND-ISO-10303-21;\n",
                self.out
            )
            .into_bytes(),
        )
        .unwrap();
        let s = structure(&g).unwrap();
        let b = bounds(&g, &s);
        (g, b)
    }
}

const Z: [f64; 3] = [0., 0., 1.];
const X: [f64; 3] = [1., 0., 0.];

fn the_box(b: Bound) -> BBox {
    match b {
        Bound::Box(b) => b,
        Bound::Unknown(r) => panic!("unknown: {r}"),
    }
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-9)
}

fn assert_box(b: Bound, min: [f64; 3], max: [f64; 3]) {
    let b = the_box(b);
    assert!(
        near(b.min, min) && near(b.max, max),
        "{b:?}, want {min:?} {max:?}"
    );
}

/// `b` holds every sample, and some sample touches each side within `tight`.
fn assert_holds(b: Bound, samples: &[[f64; 3]], tight: f64) {
    let b = the_box(b);
    for p in samples {
        assert!(
            (0..3).all(|i| b.min[i] - 1e-9 <= p[i] && p[i] <= b.max[i] + 1e-9),
            "{p:?} outside {b:?}"
        );
    }
    for i in 0..3 {
        let lo = samples.iter().map(|p| p[i]).fold(f64::INFINITY, f64::min);
        let hi = samples
            .iter()
            .map(|p| p[i])
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            lo - b.min[i] <= tight && b.max[i] - hi <= tight,
            "axis {i}: {b:?}"
        );
    }
}

fn unknown(b: Bound, reason: &'static str) {
    assert_eq!(b, Bound::Unknown(Reason(reason)));
}

/// A unit circle's points in the frame `(x, y)` around `c`.
fn ring(c: [f64; 3], x: [f64; 3], y: [f64; 3], a: f64, b: f64) -> Vec<[f64; 3]> {
    (0..3600)
        .map(|k| {
            let t = f64::from(k) * std::f64::consts::TAU / 3600.;
            [0, 1, 2].map(|i| c[i] + a * t.cos() * x[i] + b * t.sin() * y[i])
        })
        .collect()
}

/// A planar triangle: the box of its vertices, through `VERTEX_POINT` and
/// edges on `LINE`s.
fn triangle(f: &mut F, p: [[f64; 3]; 3]) -> u32 {
    let v = p.map(|p| f.vtx(p));
    let line = f.add("LINE('',#1,#1)"); // never read: a line's box is its vertices
    let l = f.edge_loop(&[(v[0], v[1], line), (v[1], v[2], line), (v[2], v[0], line)]);
    let ax = f.ax([0.; 3], Z, X);
    let plane = f.add(&format!("PLANE('',#{ax})"));
    f.face(&[l], plane)
}

#[test]
fn a_plane_face_on_lines_is_its_vertices() {
    let mut f = F::new();
    let t = triangle(&mut f, [[0., 0., 0.], [4., 0., 0.], [0., 3., 0.]]);
    let s = f.solid(&[t]);
    f.part(&[s], false);
    let (g, b) = f.bounds();
    assert_box(b.product(0), [0.; 3], [4., 3., 0.]);
    let face = g.node(crate::graph::StepId(t)).unwrap();
    assert_box(b.node(face), [0.; 3], [4., 3., 0.]);
    assert_eq!(b.model(), b.product(0));
}

#[test]
fn a_full_circle_edge_is_the_whole_circle_and_bounds_a_cylinder() {
    // A cylinder on a tilted axis, bounded by two full circles, each with a
    // single vertex: the box of the points would be a sliver.
    let z = [0., 0.6, 0.8];
    let x = [1., 0., 0.];
    let y = [0., 0.8, -0.6];
    let mut f = F::new();
    let mut loops = Vec::new();
    let mut samples = Vec::new();
    for h in [0., 10.] {
        let c = [1. + h * z[0], 2. + h * z[1], 3. + h * z[2]];
        let ax = f.ax(c, z, x);
        let circle = f.add(&format!("CIRCLE('',#{ax},2.)"));
        let v = f.vtx([c[0] + 2., c[1], c[2]]);
        loops.push(f.edge_loop(&[(v, v, circle)]));
        samples.extend(ring(c, x, y, 2., 2.));
    }
    let ax = f.ax([1., 2., 3.], z, x);
    let cyl = f.add(&format!("CYLINDRICAL_SURFACE('',#{ax},2.)"));
    let face = f.face(&loops, cyl);
    let s = f.solid(&[face]);
    f.part(&[s], false);
    assert_holds(f.bounds().1.product(0), &samples, 1e-4);
}

#[test]
fn an_ellipse_edge_is_the_whole_ellipse() {
    let (x, y, z) = ([0.6, 0.8, 0.], [0., 0., 1.], [0.8, -0.6, 0.]);
    let mut f = F::new();
    let ax = f.ax([5., 0., -1.], z, x);
    let e = f.add(&format!("ELLIPSE('',#{ax},3.,1.)"));
    let v = f.vtx([5. + 1.8, 2.4, -1.]);
    let l = f.edge_loop(&[(v, v, e)]);
    let pax = f.ax([5., 0., -1.], z, x);
    let plane = f.add(&format!("PLANE('',#{pax})"));
    let face = f.face(&[l], plane);
    f.part(&[face], false);
    assert_holds(
        f.bounds().1.product(0),
        &ring([5., 0., -1.], x, y, 3., 1.),
        1e-4,
    );
}

/// A cubic B-spline curve through `poles`, rational when `w` is given.
fn spline_curve(f: &mut F, poles: &[[f64; 3]], w: Option<&[f64]>) -> u32 {
    let p: Vec<String> = poles.iter().map(|&p| format!("#{}", f.pt(p))).collect();
    let n = poles.len();
    let knots = format!(
        "(4,{}4),(0.,{}1.)",
        "1,".repeat(n - 4),
        (1..n - 3)
            .map(|k| format!("{:?},", k as f64 / (n - 3) as f64))
            .collect::<String>()
    );
    match w {
        None => f.add(&format!(
            "B_SPLINE_CURVE_WITH_KNOTS('',3,({}),.UNSPECIFIED.,.F.,.F.,{knots},.UNSPECIFIED.)",
            p.join(",")
        )),
        Some(w) => {
            let w: Vec<String> = w.iter().map(|w| format!("{w:?}")).collect();
            f.add(&format!(
                "(BOUNDED_CURVE() B_SPLINE_CURVE(3,({}),.UNSPECIFIED.,.F.,.F.) B_SPLINE_CURVE_WITH_KNOTS({knots},.UNSPECIFIED.) CURVE() GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_CURVE(({})) REPRESENTATION_ITEM(''))",
                p.join(","),
                w.join(",")
            ))
        }
    }
}

const POLES: [[f64; 3]; 5] = [
    [0., 0., 0.],
    [1., 5., 0.],
    [2., -3., 1.],
    [3., 0., 7.],
    [4., 1., 0.],
];

/// A plane face bounded by `curve` (from the first pole to the last) and a
/// line back, inside a `SURFACE_CURVE` when `wrapped`.
fn spline_face(f: &mut F, curve: u32, wrapped: bool) -> u32 {
    let curve = if wrapped {
        f.add(&format!("SURFACE_CURVE('',#{curve},(),.CURVE_3D.)"))
    } else {
        curve
    };
    let (a, b) = (f.vtx(POLES[0]), f.vtx(POLES[4]));
    let line = f.add("LINE('',#1,#1)");
    let l = f.edge_loop(&[(a, b, curve), (b, a, line)]);
    let ax = f.ax([0.; 3], Z, X);
    let plane = f.add(&format!("PLANE('',#{ax})"));
    f.face(&[l], plane)
}

#[test]
fn a_b_spline_edge_is_its_control_points_and_a_surface_curve_its_3d_curve() {
    for (w, wrapped) in [(None, false), (Some(&[1., 2., 0.5, 1., 3.][..]), true)] {
        let mut f = F::new();
        let c = spline_curve(&mut f, &POLES, w);
        let face = spline_face(&mut f, c, wrapped);
        f.part(&[face], false);
        assert_box(f.bounds().1.product(0), [0., -3., 0.], [4., 5., 7.]);
    }
}

#[test]
fn a_rational_b_spline_with_a_zero_weight_is_unknown() {
    let mut f = F::new();
    let c = spline_curve(&mut f, &POLES, Some(&[1., 1., 0., 1., 1.]));
    let face = spline_face(&mut f, c, false);
    f.part(&[face], false);
    unknown(f.bounds().1.product(0), "non-positive weight");
}

#[test]
fn a_sphere_with_a_single_vertex_is_the_whole_sphere() {
    let mut f = F::new();
    let ax = f.ax([1., 2., 3.], Z, X);
    let sphere = f.add(&format!("SPHERICAL_SURFACE('',#{ax},4.)"));
    let v = f.vtx([1., 2., 7.]);
    let l = f.add(&format!("VERTEX_LOOP('',#{v})"));
    let face = f.face(&[l], sphere);
    let s = f.solid(&[face]);
    f.part(&[s], false);
    assert_box(f.bounds().1.product(0), [-3., -2., -1.], [5., 6., 7.]);
}

#[test]
fn a_torus_under_rotation_holds_the_whole_torus() {
    // Its axis tilted 45° about x: the local box through its 8 corners.
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let (x, y, z) = ([1., 0., 0.], [0., h, h], [0., -h, h]);
    let mut f = F::new();
    let ax = f.ax([0., 0., 10.], z, x);
    let torus = f.add(&format!("TOROIDAL_SURFACE('',#{ax},5.,1.)"));
    let face = f.face(&[], torus);
    f.part(&[face], false);
    let mut samples = Vec::new();
    for a in 0..360 {
        let (sa, ca) = (
            f64::from(a).to_radians().sin(),
            f64::from(a).to_radians().cos(),
        );
        for b in 0..360 {
            let (sb, cb) = (
                f64::from(b).to_radians().sin(),
                f64::from(b).to_radians().cos(),
            );
            let (u, v, w) = ((5. + cb) * ca, (5. + cb) * sa, sb);
            samples.push([0, 1, 2].map(|i| [0., 0., 10.][i] + u * x[i] + v * y[i] + w * z[i]));
        }
    }
    // Conservative everywhere; loose by at most the rotation's growth.
    let b = f.bounds().1.product(0);
    assert_holds(b, &samples, 6. * (2f64.sqrt() - 1.) + 1e-3);
    assert_box(
        b,
        [-6., -6. * h - h, 10. - 6. * h - h],
        [6., 6. * h + h, 10. + 6. * h + h],
    );
}

/// A bicubic B-spline surface over a 4×4 grid, rational when `w` is given.
fn spline_surface(f: &mut F, w: Option<f64>) -> u32 {
    let rows: Vec<String> = (0..4)
        .map(|i| {
            let r: Vec<String> = (0..4)
                .map(|j| {
                    format!(
                        "#{}",
                        f.pt([f64::from(i), f64::from(j), f64::from(i * j % 3)])
                    )
                })
                .collect();
            format!("({})", r.join(","))
        })
        .collect();
    let knots = "(4,4),(4,4),(0.,1.),(0.,1.)";
    match w {
        None => f.add(&format!(
            "B_SPLINE_SURFACE_WITH_KNOTS('',3,3,({}),.UNSPECIFIED.,.F.,.F.,.F.,{knots},.UNSPECIFIED.)",
            rows.join(",")
        )),
        Some(w) => {
            let ws = format!("({})", vec![format!("(1.,1.,{w:?},1.)"); 4].join(","));
            f.add(&format!(
                "(BOUNDED_SURFACE() B_SPLINE_SURFACE(3,3,({}),.UNSPECIFIED.,.F.,.F.,.F.) B_SPLINE_SURFACE_WITH_KNOTS({knots},.UNSPECIFIED.) GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_SURFACE({ws}) REPRESENTATION_ITEM('') SURFACE())",
                rows.join(",")
            ))
        }
    }
}

#[test]
fn a_b_spline_face_is_its_control_points_and_needs_positive_weights() {
    for (w, ok) in [
        (None, true),
        (Some(2.), true),
        (Some(0.), false),
        (Some(-1.), false),
    ] {
        let mut f = F::new();
        let sf = spline_surface(&mut f, w);
        let face = f.face(&[], sf);
        f.part(&[face], false);
        let b = f.bounds().1.product(0);
        if ok {
            assert_box(b, [0.; 3], [3., 3., 2.]);
        } else {
            unknown(b, "non-positive weight");
        }
    }
}

#[test]
fn a_surface_of_revolution_holds_the_swept_curve() {
    // The B-spline swept about the axis through (0, 0, 0) along y.
    let mut f = F::new();
    let c = spline_curve(&mut f, &POLES, None);
    let o = f.pt([0.; 3]);
    let d = f.add("DIRECTION('',(0.,1.,0.))");
    let axis = f.add(&format!("AXIS1_PLACEMENT('',#{o},#{d})"));
    let rev = f.add(&format!("SURFACE_OF_REVOLUTION('',#{c},#{axis})"));
    let face = f.face(&[], rev);
    f.part(&[face], false);
    // The poles' box reaches √(4² + 7²) from the axis, and y runs from -3
    // to 5; the local frame's corners are the box itself here.
    let r = 65f64.sqrt();
    assert_box(f.bounds().1.product(0), [-r, -3., -r], [r, 5., r]);
}

#[test]
fn an_offset_surface_grows_its_basis() {
    let mut f = F::new();
    let ax = f.ax([0.; 3], Z, X);
    let sphere = f.add(&format!("SPHERICAL_SURFACE('',#{ax},2.)"));
    let off = f.add(&format!("OFFSET_SURFACE('',#{sphere},-0.5,.F.)"));
    let face = f.face(&[], off);
    f.part(&[face], false);
    assert_box(f.bounds().1.product(0), [-2.5; 3], [2.5; 3]);
}

#[test]
fn anything_else_is_unknown_and_so_is_everything_above_it() {
    let mut f = F::new();
    let ax = f.ax([0.; 3], Z, X);
    let odd = f.add(&format!(
        "RECTANGULAR_TRIMMED_SURFACE('',#{ax},0.,1.,0.,1.,.T.,.T.)"
    ));
    let bad = f.face(&[], odd);
    let good = triangle(&mut f, [[0.; 3], [1., 0., 0.], [0., 1., 0.]]);
    let s = f.solid(&[good, bad]);
    let part = f.part(&[s], false);
    let ax2 = f.ax([0.; 3], Z, X);
    let pc = f.add("PARABOLA('',#1,1.)");
    let v = f.vtx([0.; 3]);
    let l = f.edge_loop(&[(v, v, pc)]);
    let plane = f.add(&format!("PLANE('',#{ax2})"));
    let face = f.face(&[l], plane);
    let other = f.part(&[face], false);
    let top = f.part(&[], false);
    let at = f.ax([0.; 3], Z, X);
    f.instance(top, part, at);
    let at = f.ax([0.; 3], Z, X);
    f.instance(top, other, at);
    let (_, b) = f.bounds();
    unknown(b.product(0), "unsupported surface");
    unknown(b.product(1), "unsupported curve");
    unknown(b.product(2), "unsupported surface");
    unknown(b.model(), "unsupported surface");
    // A plane face needs its edges.
    let mut f = F::new();
    let ax = f.ax([0.; 3], Z, X);
    let plane = f.add(&format!("PLANE('',#{ax})"));
    let face = f.face(&[], plane);
    f.part(&[face], false);
    unknown(f.bounds().1.product(0), "face without edges");
}

#[test]
fn instances_place_through_8_corners_once_per_product_and_leaves_follow_their_path() {
    let mut f = F::new();
    let t = triangle(&mut f, [[0.; 3], [2., 0., 0.], [0., 1., 0.]]);
    let bolt = f.part(&[t], false);
    let sub = f.part(&[], false);
    let top = f.part(&[], false);
    // The sub-assembly holds the bolt at the origin and in the frame Rz(90°)
    // at (10, 0, 0), p ↦ (10 - y, x, z); the top holds the sub-assembly at
    // z = -5.
    let at = f.ax([0.; 3], Z, X);
    f.instance(sub, bolt, at);
    let at = f.ax([10., 0., 0.], Z, [0., 1., 0.]);
    f.instance(sub, bolt, at);
    let at = f.ax([0., 0., -5.], Z, X);
    f.instance(top, sub, at);
    let (_, b) = f.bounds();
    assert_box(b.product(0), [0.; 3], [2., 1., 0.]);
    assert_box(b.product(1), [0.; 3], [10., 2., 0.]);
    assert_box(b.product(2), [0., 0., -5.], [10., 2., -5.]);
    assert_eq!(b.model(), b.product(2));
    assert_box(b.leaf(0), [0., 0., -5.], [2., 1., -5.]);
    assert_box(b.leaf(1), [9., 0., -5.], [10., 2., -5.]);
    assert_eq!(b.leaf(2), Bound::Unknown(Reason("no such leaf")));
}

#[test]
fn a_non_finite_placement_is_unknown_never_a_box_of_nan() {
    // A location that overflows to infinity: the placement holds inf and NaN.
    let mut f = F::new();
    let t = triangle(&mut f, [[0.; 3], [1., 0., 0.], [0., 1., 0.]]);
    let bolt = f.part(&[t], false);
    let top = f.part(&[], false);
    let p = f.add("CARTESIAN_POINT('',(1.E400,0.,0.))");
    let d = f.add("DIRECTION('',(0.,0.,1.))");
    let at = f.add(&format!("AXIS2_PLACEMENT_3D('',#{p},#{d},$)"));
    f.instance(top, bolt, at);
    let (_, b) = f.bounds();
    unknown(b.product(1), "non-finite placement");
    unknown(b.leaf(0), "non-finite placement");
    unknown(b.model(), "non-finite placement");
    // NaN directly, which `min`/`max` alone would swallow.
    let one = Acc::point([0.; 3]).union(Acc::point([1.; 3]));
    let mut t = IDENTITY;
    t[5] = f64::NAN;
    assert_eq!(one.placed(&t), why("non-finite placement"));
    assert_eq!(
        one.union(Acc::point([f64::NAN, 0., 0.])),
        why("non-finite coordinate")
    );
}

#[test]
fn non_finite_geometry_is_unknown() {
    let mut f = F::new();
    let ax = f.ax([0.; 3], Z, X);
    let sphere = f.add(&format!("SPHERICAL_SURFACE('',#{ax},1.E400)"));
    let face = f.face(&[], sphere);
    f.part(&[face], false);
    assert!(matches!(f.bounds().1.product(0), Bound::Unknown(_)));
    let mut f = F::new();
    let ax = f.ax([0.; 3], Z, X);
    let circle = f.add(&format!("CIRCLE('',#{ax},1.E400)"));
    let v = f.vtx([0.; 3]);
    let l = f.edge_loop(&[(v, v, circle)]);
    let plane = f.add(&format!("PLANE('',#{ax})"));
    let face = f.face(&[l], plane);
    f.part(&[face], false);
    assert!(matches!(f.bounds().1.product(0), Bound::Unknown(_)));
}

#[test]
fn lengths_are_scaled_to_mm_by_their_representation() {
    let mut f = F::new();
    let t = triangle(&mut f, [[0.; 3], [1., 0., 0.], [0., 2., 0.]]);
    f.part(&[t], true);
    assert_box(f.bounds().1.product(0), [0.; 3], [25.4, 50.8, 0.]);
}

#[test]
fn a_mapped_item_moves_its_representation() {
    let mut f = F::new();
    let t = triangle(&mut f, [[0.; 3], [1., 0., 0.], [0., 1., 0.]]);
    let origin = f.ax([0.; 3], Z, X);
    let mapped = f.add(&format!("SHAPE_REPRESENTATION('',(#{t},#{origin}),$)"));
    let map = f.add(&format!("REPRESENTATION_MAP(#{origin},#{mapped})"));
    let target = f.ax([0., 0., -3.], Z, X);
    let item = f.add(&format!("MAPPED_ITEM('',#{map},#{target})"));
    f.part(&[item, target], false);
    assert_box(f.bounds().1.product(0), [0., 0., -3.], [1., 1., -3.]);
}
