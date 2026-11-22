//! Primitive fits (ADR-0001 §2.8, G6c): a leaf's placeholder drawn as a
//! cylinder or a sphere instead of its box, when that is also conservative
//! and smaller.
//!
//! A product gets a fit when every face of its B-rep is on one of:
//! - **cylinder:** cylinders, cones and tori of one axis, and planes
//!   perpendicular to it. The axis is the first non-planar face's. The
//!   radius is the largest of a torus' R + r and a cylinder's, a cone's or
//!   a plane's farthest edge from the axis; the axial extent is the union
//!   of the faces' edges' extents and each torus' ±r.
//! - **sphere:** spheres of one centre, the first sphere's, and planes. The
//!   radius is the largest sphere radius and the farthest plane edge from
//!   the centre.
//!
//! Edges are read as [`Piece`]s, the box's own curve bounds: distance from
//! a line or a point is convex, and an axial coordinate linear, so over a
//! plane (in its edges' convex hull) or a cylinder or cone (ruled, linear
//! in v) they are extreme on the edges, however the face's own axis is
//! tilted. A cylinder's r about its own axis would not do: tilted by θ, its
//! cut across the fitted axis reaches r / cos θ.
//!
//! **Tolerance.** ε is the farthest any face's own axis (over the fit's
//! axial extent) or centre lies from the fitted one, plus the context's
//! length uncertainty (`GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT`, the largest
//! over the representations read, in mm). The radius grows by ε, and a
//! cylinder's extent by ε at each end. A fit whose ε passes 10× the
//! uncertainty isn't used, nor one with a face's axis tilted by θ with
//! sin θ · extent past it, nor a plane spanning more than that along the
//! axis: it isn't parallel, or perpendicular. Without an uncertainty measured
//! in the context's length unit, "within the uncertainty" has no meaning,
//! and the leaf keeps its box. So does any product whose box is `Unknown`,
//! or that has a face on any other surface.
//!
//! **Drawn conservative too.** A cylinder is a prism of [`PRISM_SIDES`]
//! sides circumscribed about the radius; a sphere a latitude–longitude
//! polyhedron whose vertices lie at r / cos α, α being its largest facet's
//! circumcircle's angular radius, so every facet's plane is at least r from
//! the centre. [`Fits::leaf`] uses a fit only when that drawn volume is
//! smaller than the leaf's box's.

use super::{
    Bound, Bounds, CONTAINERS, IDENTITY, MAX_DEPTH, Piece, SKIPPED, Walk, apply, at, column,
    mapped_rep, num, ref_list, related_reps, wire,
};
use crate::BBox;
use crate::graph::curve::Conic;
use crate::graph::structure::{compose_raw, default_x, fields, frame, mm_per_unit, real, refers};
use crate::graph::{Graph, Node, Placement, Reason, Value, mapped_placement};

/// The sides of a fitted cylinder's drawn prism.
const PRISM_SIDES: usize = 32;
/// A fitted sphere's drawn polyhedron: bands of latitude, sectors of
/// longitude.
const BANDS: usize = 12;
const SECTORS: usize = 24;

/// A fitted placeholder in mm: it contains its product's exact geometry,
/// inflated by its ε (module docs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fit {
    /// From `base` along the unit `axis` for `length`.
    Cylinder {
        base: [f64; 3],
        axis: [f64; 3],
        length: f64,
        radius: f64,
    },
    Sphere {
        centre: [f64; 3],
        radius: f64,
    },
}

impl Fit {
    /// How far `p` lies outside the fit, in mm; ≤ 0 inside.
    #[must_use]
    pub fn outside(&self, p: [f64; 3]) -> f64 {
        match *self {
            Self::Cylinder {
                base,
                axis,
                length,
                radius,
            } => {
                let h = dot(sub(p, base), axis);
                let r = radial(&Piece::Point(p), base, axis);
                (r - radius).max(-h).max(h - length)
            }
            Self::Sphere { centre, radius } => norm(sub(p, centre)) - radius,
        }
    }

    /// The drawn shape's volume (§2.8's size test): the circumscribed prism
    /// `n · R² · tan(π/n) · h`, or the circumscribed polyhedron's.
    #[must_use]
    pub fn drawn_volume(&self) -> f64 {
        match *self {
            Self::Cylinder { length, radius, .. } => {
                let n = PRISM_SIDES as f64;
                n * radius * radius * (std::f64::consts::PI / n).tan() * length
            }
            Self::Sphere { radius, .. } => {
                let (tris, d) = unit_sphere();
                let v: f64 = tris
                    .chunks_exact(3)
                    .map(|t| dot(t[0], cross(t[1], t[2])).abs() / 6.)
                    .sum();
                v * (radius / d).powi(3)
            }
        }
    }

    /// The drawn shape: its triangles (three points each) and its wireframe
    /// (two points a segment), in mm.
    #[must_use]
    pub fn drawn(&self) -> (Vec<[f64; 3]>, Vec<[f64; 3]>) {
        let (mut tris, mut lines) = (Vec::new(), Vec::new());
        match *self {
            Self::Cylinder {
                base,
                axis,
                length,
                radius,
            } => {
                let n = PRISM_SIDES;
                let rho = radius / (std::f64::consts::PI / n as f64).cos();
                let (u, top) = (default_x(axis), scale(axis, length));
                let v = cross(axis, u);
                let ring: Vec<[f64; 3]> = (0..n)
                    .map(|k| {
                        let a = std::f64::consts::TAU * k as f64 / n as f64;
                        add(base, add(scale(u, rho * a.cos()), scale(v, rho * a.sin())))
                    })
                    .collect();
                for k in 0..n {
                    let (p, q) = (ring[k], ring[(k + 1) % n]);
                    let (pt, qt) = (add(p, top), add(q, top));
                    tris.extend([p, q, qt, p, qt, pt, base, q, p]);
                    tris.extend([add(base, top), pt, qt]);
                    lines.extend([p, q, pt, qt]);
                    if k % 4 == 0 {
                        lines.extend([p, pt]);
                    }
                }
            }
            Self::Sphere { centre, radius } => {
                let (unit, d) = unit_sphere();
                let at = |p: [f64; 3]| add(centre, scale(p, radius / d));
                tris = unit.into_iter().map(at).collect();
                let ring = |i: usize, j: usize| at(lat_long(i, j % SECTORS));
                for i in 1..BANDS {
                    for j in 0..SECTORS {
                        lines.extend([ring(i, j), ring(i, j + 1)]);
                    }
                }
                for j in (0..SECTORS).step_by(3) {
                    for i in 0..BANDS {
                        lines.extend([ring(i, j), ring(i + 1, j)]);
                    }
                }
            }
        }
        (tris, lines)
    }

    /// The fit moved by the rigid transform `t`; `None` for one that isn't.
    fn placed(self, t: &[f64; 12]) -> Option<Self> {
        let c = [0, 1, 2].map(|k| column(t, k));
        let rigid = (0..3)
            .all(|i| (0..3).all(|j| (dot(c[i], c[j]) - f64::from(u8::from(i == j))).abs() < 1e-9));
        let rot = |v: [f64; 3]| sub(apply(t, v), column(t, 3));
        let mut f = self;
        match &mut f {
            Self::Cylinder { base, axis, .. } => (*base, *axis) = (apply(t, *base), rot(*axis)),
            Self::Sphere { centre, .. } => *centre = apply(t, *centre),
        }
        rigid.then_some(f)
    }

    /// Every number finite, and the radius and length positive.
    fn finite(&self) -> bool {
        let (point, axis, length, radius) = match *self {
            Self::Cylinder {
                base,
                axis,
                length,
                radius,
            } => (base, axis, length, radius),
            Self::Sphere { centre, radius } => (centre, [0.; 3], 1., radius),
        };
        point.iter().chain(&axis).all(|x| x.is_finite())
            && [length, radius].iter().all(|x| x.is_finite() && *x > 0.)
    }
}

/// Every product's fit ([`fits`]), in its own coordinates.
#[derive(Debug, Clone, Default)]
pub struct Fits {
    products: Vec<Option<Fit>>,
}

impl Fits {
    /// Product `p`'s fit in its own coordinates, before the size test.
    #[must_use]
    pub fn product(&self, p: usize) -> Option<Fit> {
        self.products.get(p).copied().flatten()
    }

    /// Leaf `leaf`'s placeholder fit, in model coordinates: its product's
    /// fit placed like its box, and only when its drawn volume is smaller
    /// than the leaf's box's ([`Bounds::leaf`]). `None` keeps the box.
    #[must_use]
    pub fn leaf(&self, b: &Bounds, leaf: u64) -> Option<Fit> {
        let Bound::Box(bb) = b.leaf(leaf) else {
            return None;
        };
        let (p, t) = b.leaf_placement(leaf).ok()?;
        let f = self.product(p)?.placed(&t)?;
        (f.drawn_volume() < volume(&bb)).then_some(f)
    }
}

fn volume(b: &BBox) -> f64 {
    (0..3).map(|i| b.max[i] - b.min[i]).product()
}

/// Fits every product of `b`'s structure whose box is known and that has no
/// shown child, from its B-rep faces in its own mm.
#[must_use]
pub fn fits(g: &Graph, b: &Bounds) -> Fits {
    let s = &b.structure;
    let mut w = Walk::new(g);
    let mut product = |p: usize| -> Option<Fit> {
        let me = &s.products[p];
        if !matches!(b.product(p), Bound::Box(_))
            || me.children.iter().any(|c| s.products[c.child].shown)
        {
            return None;
        }
        let (mut faces, mut unc) = (Vec::new(), 0f64);
        for &body in &me.bodies {
            for rep in std::iter::once(body).chain(related_reps(g, body)) {
                w.faces(rep, IDENTITY, 0, &mut faces, &mut unc).ok()?;
            }
        }
        fit(&faces, unc)
    };
    Fits {
        products: (0..s.products.len()).map(&mut product).collect(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Plane,
    Cylinder,
    Cone,
    Torus,
    Sphere,
}

/// A face in its product's mm: its surface's frame `f` (z the axis or
/// normal, origin the centre), radius `r` (a torus' major), a torus' minor
/// radius `m`, and what holds its edges.
struct Face {
    kind: Kind,
    f: [f64; 12],
    r: f64,
    m: f64,
    edges: Vec<Piece>,
}

impl Walk<'_> {
    /// `rep`'s faces, carried by `t` into the product's mm; the largest
    /// length uncertainty seen into `unc`. Anything but faces and wireframe
    /// is an error: the product keeps its box.
    fn faces(
        &mut self,
        rep: Node,
        t: [f64; 12],
        depth: usize,
        out: &mut Vec<Face>,
        unc: &mut f64,
    ) -> Result<(), Reason> {
        let g = self.g;
        if depth > MAX_DEPTH {
            return Err(Reason("nested too deep"));
        }
        let s = mm_per_unit(g, rep);
        let u = uncertainty(g, rep).ok_or(Reason("no length uncertainty"))?;
        *unc = unc.max(u * s);
        let mut items = Vec::new();
        if let Ok(a) = fields(g, rep, "REPRESENTATION") {
            ref_list(a.get(1), &mut items);
        }
        for item in items {
            if g.is_a(item, "MAPPED_ITEM") {
                let mapped = mapped_rep(g, item).ok_or(Reason("malformed mapped item"))?;
                let Placement::Placed(m) = mapped_placement(g, item, rep) else {
                    return Err(Reason("unplaced mapped item"));
                };
                self.faces(mapped, compose_raw(t, m), depth + 1, out, unc)?;
            } else if self.any(item, &["GEOMETRIC_SET", "GEOMETRIC_CURVE_SET"]) {
                let mut elements = Vec::new();
                if let Ok(a) = fields(g, item, "GEOMETRIC_SET") {
                    ref_list(a.get(1), &mut elements);
                }
                if elements.iter().any(|&e| !wire(g, e)) {
                    return Err(Reason("a surface outside a face"));
                }
            } else if !self.any(item, &SKIPPED) {
                self.topo_faces(item, s, &t, depth, out)?;
            }
        }
        Ok(())
    }

    fn topo_faces(
        &mut self,
        n: Node,
        s: f64,
        t: &[f64; 12],
        depth: usize,
        out: &mut Vec<Face>,
    ) -> Result<(), Reason> {
        if depth > MAX_DEPTH {
            return Err(Reason("nested too deep"));
        }
        if self.any(n, &["ADVANCED_FACE", "FACE_SURFACE"]) {
            let f = self.fit_face(n, s, t)?;
            out.push(f);
        } else if self.any(n, &CONTAINERS) {
            for k in self.g.refs(n).to_vec() {
                self.topo_faces(k, s, t, depth + 1, out)?;
            }
        } else {
            return Err(Reason("not a face"));
        }
        Ok(())
    }

    fn fit_face(&mut self, n: Node, s: f64, t: &[f64; 12]) -> Result<Face, Reason> {
        const KINDS: [(&str, Kind); 6] = [
            ("PLANE", Kind::Plane),
            ("CYLINDRICAL_SURFACE", Kind::Cylinder),
            ("CONICAL_SURFACE", Kind::Cone),
            ("TOROIDAL_SURFACE", Kind::Torus),
            ("DEGENERATE_TOROIDAL_SURFACE", Kind::Torus),
            ("SPHERICAL_SURFACE", Kind::Sphere),
        ];
        let g = self.g;
        let a = g.args(n).map_err(|_| Reason("malformed face"))?;
        let surf = refers(&a, 2).ok_or(Reason("malformed face"))?;
        let kind = KINDS
            .iter()
            .find(|k| g.is_a(surf, k.0))
            .ok_or(Reason("no fitted surface"))?
            .1;
        let sa = g.args(surf).map_err(|_| Reason("malformed surface"))?;
        let f = refers(&sa, 1)
            .and_then(|p| frame(g, p, s).ok())
            .ok_or(Reason("malformed surface"))?;
        let len = |k| {
            num(sa.get(k))
                .map(|r| r.abs() * s)
                .ok_or(Reason("non-finite radius"))
        };
        let (r, m) = match kind {
            Kind::Plane | Kind::Cylinder | Kind::Cone => (0., 0.),
            Kind::Torus => (len(2)?, len(3)?),
            Kind::Sphere => (len(2)?, 0.),
        };
        let mut edges = Vec::new();
        for b in self.boundary(&a)?.0 {
            if g.is_a(b, "EDGE_CURVE") {
                edges.extend(self.edge(b, s)?);
            } else if g.is_a(b, "VERTEX_POINT") {
                edges.push(Piece::Point(at(self.topo(b, s, 0))?));
            } else {
                return Err(Reason("unsupported loop vertex"));
            }
        }
        let rot = |v: [f64; 3]| sub(apply(t, v), column(t, 3));
        let edges = edges
            .into_iter()
            .map(|p| match p {
                Piece::Point(p) => Piece::Point(apply(t, p)),
                Piece::Conic(Conic { c, x, y, a, b }) => Piece::Conic(Conic {
                    c: apply(t, c),
                    x: rot(x),
                    y: rot(y),
                    a,
                    b,
                }),
            })
            .collect();
        Ok(Face {
            kind,
            f: compose_raw(*t, f),
            r,
            m,
            edges,
        })
    }
}

/// The length uncertainty of `rep`'s context, in its units: the largest
/// `UNCERTAINTY_MEASURE_WITH_UNIT` in the context's length unit (the last
/// one, which [`crate::graph::Product::mm_per_unit`] reads). `None` without one, or when none is
/// finite and positive.
#[must_use]
pub fn uncertainty(g: &Graph, rep: Node) -> Option<f64> {
    let ctx = fields(g, rep, "REPRESENTATION")
        .ok()
        .and_then(|a| refers(&a, 2))?;
    let refs = |part: &str| -> Option<Vec<Node>> {
        let a = g.args_of(ctx, part)?.ok()?;
        let Some(Value::List(l)) = a.first() else {
            return None;
        };
        Some(
            l.iter()
                .filter_map(|v| match v {
                    Value::Ref(n) => Some(*n),
                    _ => None,
                })
                .collect(),
        )
    };
    let unit = refs("GLOBAL_UNIT_ASSIGNED_CONTEXT")?
        .into_iter()
        .rfind(|&u| g.is_a(u, "LENGTH_UNIT"))?;
    refs("GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT")?
        .into_iter()
        .filter_map(|u| {
            let a = fields(g, u, "UNCERTAINTY_MEASURE_WITH_UNIT").ok()?;
            (refers(&a, 1) == Some(unit)).then(|| a.first().and_then(real))?
        })
        .filter(|x| x.is_finite() && *x > 0.)
        .reduce(f64::max)
}

/// The fit of `faces`, within the uncertainty `unc` (mm).
fn fit(faces: &[Face], unc: f64) -> Option<Fit> {
    if !(unc.is_finite() && unc > 0.) {
        return None;
    }
    let first = faces.iter().find(|f| f.kind != Kind::Plane)?;
    let (o, d) = (column(&first.f, 3), column(&first.f, 2));
    let fit = if first.kind == Kind::Sphere {
        sphere(faces, o, unc)?
    } else {
        cylinder(faces, o, d, unc)?
    };
    fit.finite().then_some(fit)
}

fn cylinder(faces: &[Face], o: [f64; 3], d: [f64; 3], unc: f64) -> Option<Fit> {
    let (mut lo, mut hi, mut rho) = (f64::INFINITY, f64::NEG_INFINITY, 0f64);
    for f in faces {
        let (a, b) = match f.kind {
            Kind::Sphere => return None,
            Kind::Torus => {
                let (z, c) = (column(&f.f, 2), dot(sub(column(&f.f, 3), o), d));
                // Within R + r of its centre, whose height is in the extent,
                // so the centre is within `off` ≤ ε of the fitted axis.
                let zd = dot(z, d).abs();
                let e = (f.r + f.m) * (1. - zd * zd).max(0.).sqrt() + f.m * zd;
                rho = rho.max(f.r + f.m);
                (c - e, c + e)
            }
            Kind::Plane | Kind::Cylinder | Kind::Cone => {
                if f.edges.is_empty() {
                    return None;
                }
                let span = f
                    .edges
                    .iter()
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |s, p| {
                        let (l, h) = along(p, o, d);
                        (s.0.min(l), s.1.max(h))
                    });
                // Ruled (a cylinder or a cone, linear in v) or planar (in
                // its edges' hull): the distance from any line is convex,
                // so its largest over the face is on the edges, whatever
                // the face's own axis. No tilt term is needed.
                let far = f.edges.iter().map(|p| radial(p, o, d));
                rho = rho.max(far.fold(0., f64::max));
                if f.kind == Kind::Plane && span.1 - span.0 > 10. * unc {
                    return None;
                }
                span
            }
        };
        (lo, hi) = (lo.min(a), hi.max(b));
    }
    // A face's axis' distance from the fitted one is convex along it, so
    // over the extent it is largest at the ends. Its direction is parallel
    // when it drifts at most 10× the uncertainty over the extent:
    // sin θ · (hi − lo) ≤ 10 · unc (and |cos θ| ≥ ½, for an extent near 0).
    let mut off = 0f64;
    for f in faces.iter().filter(|f| f.kind != Kind::Plane) {
        let (of, df) = (column(&f.f, 3), column(&f.f, 2));
        let cos = dot(df, d);
        if cos.abs() < 0.5 || norm(cross(df, d)) * (hi - lo) > 10. * unc {
            return None;
        }
        for t in [lo, hi] {
            let p = add(of, scale(df, (t - dot(sub(of, o), d)) / cos));
            off = off.max(radial(&Piece::Point(p), o, d));
        }
    }
    let eps = off + unc;
    (eps <= 10. * unc).then(|| Fit::Cylinder {
        base: add(o, scale(d, lo - eps)),
        axis: d,
        length: hi - lo + 2. * eps,
        radius: rho + eps,
    })
}

fn sphere(faces: &[Face], c: [f64; 3], unc: f64) -> Option<Fit> {
    let (mut r, mut off) = (0f64, 0f64);
    for f in faces {
        match f.kind {
            Kind::Sphere => {
                off = off.max(norm(sub(column(&f.f, 3), c)));
                r = r.max(f.r);
            }
            Kind::Plane => {
                for p in &f.edges {
                    r = r.max(match *p {
                        Piece::Point(p) => norm(sub(p, c)),
                        Piece::Conic(Conic { c: o, a, b, .. }) => norm(sub(o, c)) + a.max(b),
                    });
                }
            }
            Kind::Cylinder | Kind::Cone | Kind::Torus => return None,
        }
    }
    let eps = off + unc;
    (eps <= 10. * unc).then_some(Fit::Sphere {
        centre: c,
        radius: r + eps,
    })
}

/// The interval a piece covers along the axis `(o, d)`.
fn along(p: &Piece, o: [f64; 3], d: [f64; 3]) -> (f64, f64) {
    match *p {
        Piece::Point(p) => {
            let t = dot(sub(p, o), d);
            (t, t)
        }
        Piece::Conic(Conic { c, x, y, a, b }) => {
            let t = dot(sub(c, o), d);
            let h = ((a * dot(x, d)).powi(2) + (b * dot(y, d)).powi(2)).sqrt();
            (t - h, t + h)
        }
    }
}

/// A bound on a piece's distance from the axis `(o, d)`.
fn radial(p: &Piece, o: [f64; 3], d: [f64; 3]) -> f64 {
    let (q, extra) = match *p {
        Piece::Point(q) => (q, 0.),
        Piece::Conic(Conic { c, a, b, .. }) => (c, a.max(b)),
    };
    let w = sub(q, o);
    norm(sub(w, scale(d, dot(w, d)))) + extra
}

/// The unit sphere's point at latitude ring `i` (0 the north pole) and
/// longitude `j`.
fn lat_long(i: usize, j: usize) -> [f64; 3] {
    let (phi, th) = (
        std::f64::consts::PI * i as f64 / BANDS as f64,
        std::f64::consts::TAU * j as f64 / SECTORS as f64,
    );
    [phi.sin() * th.cos(), phi.sin() * th.sin(), phi.cos()]
}

/// The polyhedron on the unit sphere's latitude–longitude grid, as
/// triangles, and its facets' least distance from the centre: cos α. Each
/// quad's corners are coplanar (an isosceles trapezoid), so its two
/// triangles share its plane.
fn unit_sphere() -> (Vec<[f64; 3]>, f64) {
    let mut tris = Vec::with_capacity(BANDS * SECTORS * 6);
    for i in 0..BANDS {
        for j in 0..SECTORS {
            let [a, b, c, e] = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)]
                .map(|(i, j)| lat_long(i, j % SECTORS));
            if i > 0 {
                tris.extend([a, b, e]);
            }
            if i + 1 < BANDS {
                tris.extend([b, c, e]);
            }
        }
    }
    let d = tris
        .chunks_exact(3)
        .map(|t| {
            let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
            dot(n, t[0]).abs() / norm(n)
        })
        .fold(f64::INFINITY, f64::min);
    (tris, d)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: [f64; 3], k: f64) -> [f64; 3] {
    a.map(|x| x * k)
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}
