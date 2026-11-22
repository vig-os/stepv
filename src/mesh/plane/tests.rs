//! Planar faces (ADR-0001 §2.6, M2) against their own boundaries: the area
//! of the kept triangles equals the loops' (outer minus holes), no triangle
//! lies outside the face or has zero area, every vertex is an edge
//! polyline's point bit for bit, normals are exact, and what the
//! triangulation can't take exactly is routed with its reason.

use super::*;
use crate::graph::StepId;
use crate::mesh::Tolerance;

/// A STEP file under construction: lines and circles on planes.
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
    /// An axis placement at `o`, normal `z`, x axis `x`.
    fn ax(&mut self, o: [f64; 3], z: [f64; 3], x: [f64; 3]) -> u32 {
        let (o, z, x) = (self.pt(o), self.dir(z), self.dir(x));
        self.add(&format!("AXIS2_PLACEMENT_3D('',#{o},#{z},#{x})"))
    }
    fn vertex(&mut self, p: [f64; 3]) -> u32 {
        let p = self.pt(p);
        self.add(&format!("VERTEX_POINT('',#{p})"))
    }
    /// A line edge between two vertices at `a` and `b`.
    fn line(&mut self, va: u32, vb: u32, a: [f64; 3], b: [f64; 3]) -> u32 {
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let len = d.iter().map(|c| c * c).sum::<f64>().sqrt();
        let (p, dir) = (self.pt(a), self.dir(d.map(|c| c / len)));
        let v = self.add(&format!("VECTOR('',#{dir},{})", num(len)));
        let l = self.add(&format!("LINE('',#{p},#{v})"));
        self.add(&format!("EDGE_CURVE('',#{va},#{vb},#{l},.T.)"))
    }
    fn oe(&mut self, e: u32, sense: bool) -> u32 {
        let s = if sense { "T" } else { "F" };
        self.add(&format!("ORIENTED_EDGE('',*,*,#{e},.{s}.)"))
    }
    /// A closed polygon of lines through `pts` on z = 0, as oriented edges.
    fn polygon(&mut self, pts: &[[f64; 2]]) -> Vec<u32> {
        let p3: Vec<[f64; 3]> = pts.iter().map(|p| [p[0], p[1], 0.]).collect();
        let v: Vec<u32> = p3.iter().map(|&p| self.vertex(p)).collect();
        self.lines(&v, &p3)
    }
    /// Lines from vertex `v[k]` at `p[k]` to the next, closed.
    fn lines(&mut self, v: &[u32], p3: &[[f64; 3]]) -> Vec<u32> {
        (0..v.len())
            .map(|k| {
                let j = (k + 1) % v.len();
                let e = self.line(v[k], v[j], p3[k], p3[j]);
                self.oe(e, true)
            })
            .collect()
    }
    /// A full circle of radius `r` about `c` on z = 0, one closed edge whose
    /// curve runs counter-clockwise about +z; its oriented edge is walked
    /// clockwise when `cw`.
    fn circle(&mut self, c: [f64; 2], r: f64, cw: bool) -> Vec<u32> {
        let ax = self.ax([c[0], c[1], 0.], [0., 0., 1.], [1., 0., 0.]);
        let k = self.add(&format!("CIRCLE('',#{ax},{})", num(r)));
        let v = self.vertex([c[0] + r, c[1], 0.]);
        let e = self.add(&format!("EDGE_CURVE('',#{v},#{v},#{k},.T.)"));
        vec![self.oe(e, !cw)]
    }
    fn bound(&mut self, oes: &[u32], outer: bool, orientation: bool) -> u32 {
        let l: Vec<String> = oes.iter().map(|e| format!("#{e}")).collect();
        let lp = self.add(&format!("EDGE_LOOP('',({}))", l.join(",")));
        let t = if outer {
            "FACE_OUTER_BOUND"
        } else {
            "FACE_BOUND"
        };
        let o = if orientation { "T" } else { "F" };
        self.add(&format!("{t}('',#{lp},.{o}.)"))
    }
    /// A face on the plane through `ax`.
    fn face(&mut self, bounds: &[u32], ax: u32, same_sense: bool) -> u32 {
        let b: Vec<String> = bounds.iter().map(|e| format!("#{e}")).collect();
        let pl = self.add(&format!("PLANE('',#{ax})"));
        let s = if same_sense { "T" } else { "F" };
        self.add(&format!("ADVANCED_FACE('',({}),#{pl},.{s}.)", b.join(",")))
    }
    fn xy(&mut self) -> u32 {
        self.ax([0.; 3], [0., 0., 1.], [1., 0., 0.])
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
    uncertainty: 1e-7,
};

fn node(g: &Graph, id: u32) -> Node {
    g.node(StepId(id)).unwrap()
}

/// Every edge of `g`'s polyline.
fn polylines(g: &Graph) -> Polylines {
    let tol = Tolerance::from_mm(1e-3, 0.3).unwrap();
    Polylines::build(g, g.of_type("EDGE_CURVE"), 1., tol)
}

/// Meshes face `#face` of `f`, with the graph and polylines.
fn run(f: &F, face: u32, reversed: bool) -> (Graph, Polylines, Result<FaceMesh, Route>) {
    let g = f.graph();
    let p = polylines(&g);
    let m = mesh(&g, node(&g, face), &p, CTX, reversed);
    (g, p, m)
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

/// A loop's points in x, y: its oriented edges' polylines joined.
fn ring(g: &Graph, p: &Polylines, oes: &[u32]) -> Vec<[f64; 2]> {
    let mut out: Vec<[f64; 2]> = Vec::new();
    for &oe in oes {
        let pts: Vec<_> = p.oriented(g, node(g, oe)).unwrap().collect();
        out.extend(pts[..pts.len() - 1].iter().map(|(_, q)| [q[0], q[1]]));
    }
    out
}

/// The shoelace area of a ring, counter-clockwise positive.
fn shoelace(r: &[[f64; 2]]) -> f64 {
    (0..r.len())
        .map(|k| {
            let (a, b) = (r[k], r[(k + 1) % r.len()]);
            a[0] * b[1] - a[1] * b[0]
        })
        .sum::<f64>()
        / 2.
}

/// Whether `q` is inside the ring (even-odd).
fn contains(r: &[[f64; 2]], q: [f64; 2]) -> bool {
    let mut inside = false;
    for k in 0..r.len() {
        let (a, b) = (r[k], r[(k + 1) % r.len()]);
        if (a[1] > q[1]) != (b[1] > q[1])
            && q[0] < a[0] + (q[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0])
        {
            inside = !inside;
        }
    }
    inside
}

/// The invariants every mesh keeps, against its face's `outer` ring and
/// `holes`: the area is theirs, every triangle is non-degenerate, wound
/// about the normal, and inside the outer ring and outside every hole;
/// every vertex is its edge point's, bit for bit, and finite.
fn check(g: &Graph, p: &Polylines, m: &FaceMesh, outer: &[[f64; 2]], holes: &[Vec<[f64; 2]>]) {
    let want = shoelace(outer).abs() - holes.iter().map(|h| shoelace(h).abs()).sum::<f64>();
    let mut area = 0.;
    for t in &m.triangles {
        let [a, b, c] = t.map(|i| m.point(i as usize));
        let n = cross(sub(b, a), sub(c, a));
        let twice = n[0] * m.normal[0] + n[1] * m.normal[1] + n[2] * m.normal[2];
        assert!(twice > 0., "a zero-area or reversed triangle {t:?}");
        area += twice / 2.;
        let q = [0, 1].map(|k| (a[k] + b[k] + c[k]) / 3.);
        assert!(contains(outer, q), "triangle {t:?} outside the outer loop");
        assert!(
            holes.iter().all(|h| !contains(h, q)),
            "triangle {t:?} in a hole"
        );
    }
    assert!(
        (area - want).abs() <= 1e-12 * want.max(1.),
        "{area} vs {want}"
    );
    assert_eq!(m.points.len(), m.x.len());
    for (i, ep) in m.points.iter().enumerate() {
        let q = p.get(ep.edge).unwrap().unwrap().point(ep.index as usize);
        assert_eq!(m.point(i), q, "vertex {i} is its edge point, bit for bit");
        assert!(q.iter().all(|c| c.is_finite()));
    }
    let n = m.normal;
    assert!((n[0] * n[0] + n[1] * n[1] + n[2] * n[2] - 1.).abs() < 1e-15);
    let _ = g;
}

const SQUARE: [[f64; 2]; 4] = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]];

#[test]
fn a_square_is_two_triangles_on_its_corners() {
    let mut f = F::default();
    let oes = f.polygon(&SQUARE);
    let b = f.bound(&oes, true, true);
    let ax = f.xy();
    let face = f.face(&[b], ax, true);
    let (g, p, m) = run(&f, face, false);
    let m = m.unwrap();
    assert_eq!(m.triangles.len(), 2);
    assert_eq!(m.points.len(), 4);
    assert_eq!(m.normal, [0., 0., 1.]);
    check(&g, &p, &m, &ring(&g, &p, &oes), &[]);
}

#[test]
fn a_square_on_a_tilted_plane_inverts_within_the_uncertainty() {
    // The plane through (1, 2, 3) with normal (0, −0.6, 0.8): its points are
    // o + u·x + v·y, rounded.
    let (o, z, x) = ([1., 2., 3.], [0., -0.6, 0.8], [1., 0., 0.]);
    let y = cross(z, x);
    let at = |q: [f64; 2]| [0, 1, 2].map(|k| o[k] + q[0] * x[k] + q[1] * y[k]);
    let mut f = F::default();
    let v: Vec<u32> = SQUARE.iter().map(|&q| f.vertex(at(q))).collect();
    let oes: Vec<u32> = (0..4)
        .map(|k| {
            let j = (k + 1) % 4;
            let e = f.line(v[k], v[j], at(SQUARE[k]), at(SQUARE[j]));
            f.oe(e, true)
        })
        .collect();
    let b = f.bound(&oes, true, true);
    let ax = f.ax(o, z, x);
    let face = f.face(&[b], ax, true);
    let (_, _, m) = run(&f, face, false);
    let m = m.unwrap();
    assert_eq!(m.triangles.len(), 2);
    assert_eq!(m.normal, [0., -0.6, 0.8]);
}

#[test]
fn a_square_with_a_circular_hole() {
    let mut f = F::default();
    let outer = f.polygon(&[[0., 0.], [4., 0.], [4., 4.], [0., 4.]]);
    let hole = f.circle([2., 2.], 1., true);
    let (bo, bh) = (f.bound(&outer, true, true), f.bound(&hole, false, true));
    let ax = f.xy();
    let face = f.face(&[bo, bh], ax, true);
    let (g, p, m) = run(&f, face, false);
    let m = m.unwrap();
    let h = ring(&g, &p, &hole);
    assert!(h.len() > 8, "the circle is a polyline");
    check(&g, &p, &m, &ring(&g, &p, &outer), &[h]);
}

#[test]
fn a_plate_with_many_holes_and_only_face_bounds() {
    // 12 × 12 circular holes, every bound a FACE_BOUND: the flood needs no
    // outer loop.
    let mut f = F::default();
    let outer = f.polygon(&[[0., 0.], [24., 0.], [24., 24.], [0., 24.]]);
    let mut bounds = vec![f.bound(&outer, false, true)];
    let mut holes = Vec::new();
    for i in 0..12 {
        for j in 0..12 {
            let c = [2. * f64::from(i) + 1., 2. * f64::from(j) + 1.];
            let h = f.circle(c, 0.4, true);
            bounds.push(f.bound(&h, false, true));
            holes.push(h);
        }
    }
    let ax = f.xy();
    let face = f.face(&bounds, ax, true);
    let (g, p, m) = run(&f, face, false);
    let m = m.unwrap();
    let rings: Vec<_> = holes.iter().map(|h| ring(&g, &p, h)).collect();
    check(&g, &p, &m, &ring(&g, &p, &outer), &rings);
}

#[test]
fn a_hole_written_counter_clockwise_with_orientation_f() {
    // The bound's orientation reverses the loop: the face walks it clockwise.
    let mut f = F::default();
    let outer = f.polygon(&[[0., 0.], [4., 0.], [4., 4.], [0., 4.]]);
    let hole = f.circle([2., 2.], 1., false);
    let (bo, bh) = (f.bound(&outer, true, true), f.bound(&hole, false, false));
    let ax = f.xy();
    let face = f.face(&[bo, bh], ax, true);
    let (g, p, m) = run(&f, face, false);
    let h = ring(&g, &p, &hole);
    check(&g, &p, &m.unwrap(), &ring(&g, &p, &outer), &[h]);
}

#[test]
fn a_hole_touching_the_outer_loop_at_a_vertex_meshes() {
    // A triangular hole whose corner is the outer square's corner (2, 0):
    // the loops share that vertex, and no constraint is split.
    let mut f = F::default();
    let pts = [[0., 0.], [2., 0.], [4., 0.], [4., 4.], [0., 4.]].map(|q| [q[0], q[1], 0.]);
    let v: Vec<u32> = pts.iter().map(|&q| f.vertex(q)).collect();
    let outer = f.lines(&v, &pts);
    let tri = [pts[1], [1., 1., 0.], [3., 1., 0.]];
    let hv = [v[1], f.vertex(tri[1]), f.vertex(tri[2])];
    let hole = f.lines(&hv, &tri);
    let (bo, bh) = (f.bound(&outer, true, true), f.bound(&hole, false, true));
    let ax = f.xy();
    let face = f.face(&[bo, bh], ax, true);
    let (g, p, m) = run(&f, face, false);
    let m = m.unwrap();
    assert_eq!(m.points.len(), 7, "the shared vertex once");
    check(&g, &p, &m, &ring(&g, &p, &outer), &[ring(&g, &p, &hole)]);
}

#[test]
fn a_hole_touching_an_outer_segment_is_routed() {
    // The hole's corner (2, 0) lies inside the outer loop's bottom segment:
    // the constraint would be split there, so the face is routed.
    let mut f = F::default();
    let outer = f.polygon(&SQUARE.map(|q| [4. * q[0], 4. * q[1]]));
    let hole = f.polygon(&[[2., 0.], [1., 1.], [3., 1.]]);
    let (bo, bh) = (f.bound(&outer, true, true), f.bound(&hole, false, true));
    let ax = f.xy();
    let face = f.face(&[bo, bh], ax, true);
    let (_, _, m) = run(&f, face, false);
    assert_eq!(m.unwrap_err(), Route::Face(Reason("rs-fail:cdt-touching")));
}

#[test]
fn a_concave_outer_loop() {
    // A U: the notch between its arms is outside the face, inside the hull.
    let mut f = F::default();
    let u = [
        [0., 0.],
        [3., 0.],
        [3., 3.],
        [2., 3.],
        [2., 1.],
        [1., 1.],
        [1., 3.],
        [0., 3.],
    ];
    let outer = f.polygon(&u);
    let b = f.bound(&outer, true, true);
    let ax = f.xy();
    let face = f.face(&[b], ax, true);
    let (g, p, m) = run(&f, face, false);
    let m = m.unwrap();
    assert_eq!(m.triangles.len(), 6, "n − 2 triangles");
    check(&g, &p, &m, &ring(&g, &p, &outer), &[]);
}

#[test]
fn same_sense_f_flips_the_normal_and_the_winding() {
    // The plane's normal is +z; the face's is −z, so its loops run
    // clockwise in the plane: the square walked backwards.
    let mut f = F::default();
    let oes = f.polygon(&SQUARE);
    let b = f.bound(&oes, true, false);
    let ax = f.xy();
    let face = f.face(&[b], ax, false);
    let (g, p, m) = run(&f, face, false);
    let m = m.unwrap();
    assert_eq!(m.normal, [-0., -0., -1.]);
    check(&g, &p, &m, &ring(&g, &p, &oes), &[]);
    // Reversed in its shell, it faces +z again.
    let (_, _, r) = run(&f, face, true);
    let r = r.unwrap();
    assert_eq!(r.normal, [0., 0., 1.]);
    check(&g, &p, &r, &ring(&g, &p, &oes), &[]);
}

#[test]
fn a_loop_against_the_topological_normal_is_routed() {
    // The square counter-clockwise about +z, on a face whose normal is −z.
    let mut f = F::default();
    let oes = f.polygon(&SQUARE);
    let b = f.bound(&oes, true, true);
    let ax = f.xy();
    let face = f.face(&[b], ax, false);
    let (_, _, m) = run(&f, face, false);
    assert_eq!(m.unwrap_err(), Route::Face(Reason("rs-fail:orientation")));
    // A hole walked like its outer loop.
    let mut f = F::default();
    let outer = f.polygon(&[[0., 0.], [4., 0.], [4., 4.], [0., 4.]]);
    let hole = f.circle([2., 2.], 1., false);
    let (bo, bh) = (f.bound(&outer, true, true), f.bound(&hole, false, true));
    let ax = f.xy();
    let face = f.face(&[bo, bh], ax, true);
    let (_, _, m) = run(&f, face, false);
    assert_eq!(m.unwrap_err(), Route::Face(Reason("rs-fail:orientation")));
}

#[test]
fn degenerate_and_self_intersecting_loops_route_with_a_reason() {
    let reason = |pts: &[[f64; 2]]| {
        let mut f = F::default();
        let oes = f.polygon(pts);
        let b = f.bound(&oes, true, true);
        let ax = f.xy();
        let face = f.face(&[b], ax, true);
        run(&f, face, false).2.unwrap_err()
    };
    let face = |r: &'static str| Route::Face(Reason(r));
    // A bow tie: its two diagonals cross.
    let bowtie = [[0., 0.], [1., 1.], [1., 0.], [0., 1.]];
    assert_eq!(reason(&bowtie), face("rs-fail:cdt-crossing"));
    // Two lines out and back: fewer than three points.
    assert_eq!(reason(&[[0., 0.], [1., 0.]]), face("loop:degenerate"));
    // Collinear: the closing segment runs through the middle point.
    assert_eq!(
        reason(&[[0., 0.], [1., 0.], [2., 0.]]),
        face("rs-fail:cdt-touching")
    );
    // A zero-length line from (1, 0) to a second vertex at (1, 0).
    let mut f = F::default();
    let p = [[0., 0., 0.], [1., 0., 0.], [1., 0., 0.], [0., 1., 0.]];
    let v: Vec<u32> = p.iter().map(|&q| f.vertex(q)).collect();
    let (o, x) = (f.pt(p[1]), f.dir([1., 0., 0.]));
    let vec = f.add(&format!("VECTOR('',#{x},1.)"));
    let line = f.add(&format!("LINE('',#{o},#{vec})"));
    let zero = f.add(&format!("EDGE_CURVE('',#{},#{},#{line},.T.)", v[1], v[2]));
    let edges = [
        f.line(v[0], v[1], p[0], p[1]),
        zero,
        f.line(v[2], v[3], p[2], p[3]),
        f.line(v[3], v[0], p[3], p[0]),
    ];
    let oes: Vec<u32> = edges.iter().map(|&e| f.oe(e, true)).collect();
    let b = f.bound(&oes, true, true);
    let ax = f.xy();
    let zface = f.face(&[b], ax, true);
    assert_eq!(
        run(&f, zface, false).2.unwrap_err(),
        face("loop:degenerate")
    );
    // A figure eight through (1, 1), twice in one loop.
    let eight = [[0., 0.], [1., 1.], [2., 0.], [2., 2.], [1., 1.], [0., 2.]];
    assert_eq!(reason(&eight), face("loop:self-touching"));
}

#[test]
fn open_loops_off_surface_points_and_routed_edges_route() {
    // Two edges that don't meet.
    let mut f = F::default();
    let (a, b, c) = ([0., 0., 0.], [1., 0., 0.], [0., 1., 0.]);
    let (va, vb, vc, vd) = (f.vertex(a), f.vertex(b), f.vertex(c), f.vertex(c));
    let e1 = f.line(va, vb, a, b);
    let e2 = f.line(vc, va, c, a);
    let e3 = f.line(vb, vd, b, c);
    let oes: Vec<u32> = [e1, e2, e3].iter().map(|&e| f.oe(e, true)).collect();
    let bd = f.bound(&oes, true, true);
    let ax = f.xy();
    let face = f.face(&[bd], ax, true);
    assert_eq!(
        run(&f, face, false).2.unwrap_err(),
        Route::Face(Reason("loop:open"))
    );
    // The square on z = 0, on the plane z = 1e-6.
    let mut f = F::default();
    let oes = f.polygon(&SQUARE);
    let bd = f.bound(&oes, true, true);
    let ax = f.ax([0., 0., 1e-6], [0., 0., 1.], [1., 0., 0.]);
    let face = f.face(&[bd], ax, true);
    assert_eq!(
        run(&f, face, false).2.unwrap_err(),
        Route::Face(Reason("curve:off-surface"))
    );
    // A circle of radius 0: `mesh::edges` routes it, and so the face.
    let mut f = F::default();
    let oes = f.circle([0., 0.], 0., false);
    let bd = f.bound(&oes, true, true);
    let ax = f.xy();
    let face = f.face(&[bd], ax, true);
    let r = run(&f, face, false).2.unwrap_err();
    assert_eq!(r, Route::Edge(Reason("degenerate conic")));
    assert_eq!(r.to_string(), "edge routed: degenerate conic");
    // Not a plane.
    let mut f = F::default();
    let oes = f.polygon(&SQUARE);
    let bd = f.bound(&oes, true, true);
    let ax = f.xy();
    let cyl = f.add(&format!("CYLINDRICAL_SURFACE('',#{ax},1.)"));
    let face = f.add(&format!("ADVANCED_FACE('',(#{bd}),#{cyl},.T.)"));
    assert_eq!(
        run(&f, face, false).2.unwrap_err(),
        Route::Face(Reason("surface:not a plane"))
    );
}

#[test]
fn two_planar_faces_sharing_an_edge_share_its_points() {
    // A disc of radius 2 cut by the arc of a circle of radius 2 about (2, 0):
    // the lens left of it and the crescent right of it share the arc.
    let mut f = F::default();
    let (top, bottom) = ([1., 3f64.sqrt(), 0.], [1., -(3f64.sqrt()), 0.]);
    let (vt, vb) = (f.vertex(top), f.vertex(bottom));
    let circle = |f: &mut F, c: f64| {
        let ax = f.ax([c, 0., 0.], [0., 0., 1.], [1., 0., 0.]);
        f.add(&format!("CIRCLE('',#{ax},2.)"))
    };
    let (disc, cut) = (circle(&mut f, 0.), circle(&mut f, 2.));
    // On the disc's circle: bottom → top through x > 0, and top → bottom
    // through x < 0, both counter-clockwise about +z.
    let right = f.add(&format!("EDGE_CURVE('',#{vb},#{vt},#{disc},.T.)"));
    let left = f.add(&format!("EDGE_CURVE('',#{vt},#{vb},#{disc},.T.)"));
    // The cut: bottom → top through x < 1, against its circle.
    let arc = f.add(&format!("EDGE_CURVE('',#{vb},#{vt},#{cut},.F.)"));
    let lens: Vec<u32> = [(right, true), (arc, false)]
        .iter()
        .map(|&(e, s)| f.oe(e, s))
        .collect();
    let rest: Vec<u32> = [(arc, true), (left, true)]
        .iter()
        .map(|&(e, s)| f.oe(e, s))
        .collect();
    let (bl, br) = (f.bound(&lens, true, true), f.bound(&rest, true, true));
    let ax = f.xy();
    let (fl, fr) = (f.face(&[bl], ax, true), f.face(&[br], ax, true));
    let g = f.graph();
    let p = polylines(&g);
    let ml = mesh(&g, node(&g, fl), &p, CTX, false).unwrap();
    let mr = mesh(&g, node(&g, fr), &p, CTX, false).unwrap();
    check(&g, &p, &ml, &ring(&g, &p, &lens), &[]);
    check(&g, &p, &mr, &ring(&g, &p, &rest), &[]);
    // The arc's interior points, by edge point (a vertex is named by
    // whichever edge reached it first).
    let n = p.get(node(&g, arc)).unwrap().unwrap().len();
    let last = u32::try_from(n - 1).unwrap();
    let on_arc = |m: &FaceMesh| -> Vec<(u32, [f64; 3])> {
        let mut v: Vec<_> = (0..m.points.len())
            .filter(|&i| {
                m.points[i].edge == node(&g, arc) && (1..last).contains(&m.points[i].index)
            })
            .map(|i| (m.points[i].index, m.point(i)))
            .collect();
        v.sort_by_key(|q| q.0);
        v
    };
    let (a, b) = (on_arc(&ml), on_arc(&mr));
    assert!(n > 3, "the arc has interior points");
    assert_eq!(a.len(), n - 2);
    assert_eq!(a, b, "the same points, bit for bit");
    // Each face walks the arc in its own direction: opposite ways.
    let walked = |m: &FaceMesh, oe: u32| -> Vec<[f64; 3]> {
        let ours: Vec<[f64; 3]> = (0..m.points.len()).map(|i| m.point(i)).collect();
        p.oriented(&g, node(&g, oe))
            .unwrap()
            .map(|(_, q)| {
                assert!(ours.contains(&q));
                q
            })
            .collect()
    };
    let mut back = walked(&mr, rest[0]);
    back.reverse();
    assert_eq!(walked(&ml, lens[1]), back);
}

#[test]
fn no_non_finite_output_at_large_coordinates() {
    // A plate 1e50 mm across meshes, every output finite; at 1e100 mm the
    // triangulation's exact predicates can't take it, and it is routed.
    let plate = |s: f64| {
        let mut f = F::default();
        let oes = f.polygon(&SQUARE.map(|q| [q[0] * s, q[1] * s]));
        let b = f.bound(&oes, true, true);
        let ax = f.xy();
        let face = f.face(&[b], ax, true);
        let g = f.graph();
        let p = polylines(&g);
        let ctx = Context {
            uncertainty: s * 1e-7,
            ..CTX
        };
        mesh(&g, node(&g, face), &p, ctx, false)
    };
    let m = plate(1e50).unwrap();
    assert_eq!(m.triangles.len(), 2);
    for i in 0..m.points.len() {
        assert!(m.point(i).iter().all(|c| c.is_finite()));
    }
    assert!(m.normal.iter().all(|c| c.is_finite()));
    assert_eq!(
        plate(1e100).unwrap_err(),
        Route::Face(Reason("rs-fail:coordinate"))
    );
}

#[test]
fn an_uncertainty_below_f64_rounding_still_checks_the_plane() {
    // ABC 00000046's side faces: the plane x = 1 with the normal
    // (−1, 0, −1.8e-16), and an uncertainty of 1e-17 mm, under the rounding
    // of the distance itself (9e-17 here). On the plane, it meshes; 1e-9 off
    // it, it is routed.
    let square = |x: f64| {
        let mut f = F::default();
        let p = SQUARE.map(|q| [x, q[0], q[1]]);
        let v: Vec<u32> = p.iter().map(|&q| f.vertex(q)).collect();
        let oes = f.lines(&v, &p);
        let b = f.bound(&oes, true, true);
        let ax = f.ax(
            [1., -0.01, 0.5],
            [-1., 0., -1.836_970_198_721_03e-16],
            [0., 0., 1.],
        );
        let face = f.face(&[b], ax, false);
        let g = f.graph();
        let p = polylines(&g);
        let ctx = Context {
            uncertainty: 1e-17,
            ..CTX
        };
        mesh(&g, node(&g, face), &p, ctx, false)
    };
    assert_eq!(square(1.).unwrap().triangles.len(), 2);
    assert_eq!(
        square(1. + 1e-9).unwrap_err(),
        Route::Face(Reason("curve:off-surface"))
    );
}
