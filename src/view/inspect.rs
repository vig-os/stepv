//! What the inspector says about a picked face (#29), from `--topology`:
//! the exact surface, not the mesh. Pure, so the numbers are tested without
//! a window.
//!
//! Everything is in model coordinates: a prototype's surfaces are placed by
//! its part's transform, so a pin's axis is where that pin stands.

use crate::topology::{Curve, Point, Surface, Topology};

/// One `key: value` line of the inspector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub key: &'static str,
    pub value: String,
}

fn row(key: &'static str, value: impl Into<String>) -> Row {
    Row {
        key,
        value: value.into(),
    }
}

/// A number with up to four decimals and no trailing zeros: `4`, `1149.7346`.
#[must_use]
pub fn num(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// A direction: `+z` when it is an axis, else its components.
#[must_use]
pub fn dir(d: Point) -> String {
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2])
        .sqrt()
        .max(f64::MIN_POSITIVE);
    let d = d.map(|c| c / len);
    for (k, name) in ["x", "y", "z"].iter().enumerate() {
        if (d[k].abs() - 1.0).abs() < 1e-9 {
            return format!("{}{name}", if d[k] > 0.0 { "+" } else { "−" });
        }
    }
    format!("({}, {}, {})", num(d[0]), num(d[1]), num(d[2]))
}

fn point(p: Point) -> String {
    format!("({}, {}, {})", num(p[0]), num(p[1]), num(p[2]))
}

/// A placement: row-major 3x4, prototype to model coordinates.
struct Placement([f64; 12]);

impl Placement {
    fn point(&self, p: Point) -> Point {
        let m = &self.0;
        [0, 1, 2]
            .map(|r| m[4 * r] * p[0] + m[4 * r + 1] * p[1] + m[4 * r + 2] * p[2] + m[4 * r + 3])
    }

    fn dir(&self, d: Point) -> Point {
        let m = &self.0;
        [0, 1, 2].map(|r| m[4 * r] * d[0] + m[4 * r + 1] * d[1] + m[4 * r + 2] * d[2])
    }

    /// The placement's uniform scale (OCCT's `gp_Trsf` folds it into the
    /// matrix): `|det|^(1/3)`, 1 for rotations and mirrors. The topology's
    /// radii, areas and volumes are the prototype's, so a scaled instance's
    /// are scaled by this, as its mesh is.
    fn scale(&self) -> f64 {
        let m = &self.0;
        let det = m[0] * (m[5] * m[10] - m[6] * m[9]) - m[1] * (m[4] * m[10] - m[6] * m[8])
            + m[2] * (m[4] * m[9] - m[5] * m[8]);
        det.abs().cbrt()
    }
}

/// The surface's name, as the inspector heads it.
#[must_use]
pub fn surface_name(s: &Surface) -> &'static str {
    match s {
        Surface::Plane { .. } => "Plane",
        Surface::Cylinder { .. } => "Cylinder",
        Surface::Cone { .. } => "Cone",
        Surface::Sphere { .. } => "Sphere",
        Surface::Torus { .. } => "Torus",
        Surface::Bspline => "B-spline",
        Surface::Bezier => "Bézier",
        Surface::Revolution => "Surface of revolution",
        Surface::Extrusion => "Extrusion",
        Surface::Offset => "Offset surface",
        Surface::Other => "Other",
    }
}

/// The rows for face `face` of placed part `part`, `None` when the
/// topology has no such face.
#[must_use]
pub fn face(topo: &Topology, part: usize, face: usize) -> Option<Vec<Row>> {
    let placed = topo.parts.get(part)?;
    let proto = topo.prototypes.get(placed.prototype)?;
    let f = proto.faces.get(face)?;
    let at = Placement(placed.transform);
    let k = at.scale();
    let u = &topo.units;
    let len = |v: f64| format!("{} {u}", num(v * k));
    let mut rows = vec![
        row("Part", format!("{} (#{part})", placed.name)),
        row("Face", format!("#{face}")),
        row("Surface", surface_name(&f.surface)),
    ];
    match f.surface {
        Surface::Plane { origin, normal } => {
            rows.push(row("Normal", dir(at.dir(normal))));
            rows.push(row("Origin", point(at.point(origin))));
        }
        Surface::Cylinder {
            origin,
            axis,
            radius,
        } => {
            rows.push(row("Radius", len(radius)));
            rows.push(row("Diameter", format!("Ø {}", len(2.0 * radius))));
            rows.push(row("Axis", dir(at.dir(axis))));
            rows.push(row("Through", point(at.point(origin))));
        }
        Surface::Cone {
            origin,
            axis,
            radius,
            semi_angle_deg,
        } => {
            rows.push(row("Radius", len(radius)));
            rows.push(row("Half-angle", format!("{}°", num(semi_angle_deg))));
            rows.push(row("Axis", dir(at.dir(axis))));
            rows.push(row("Through", point(at.point(origin))));
        }
        Surface::Sphere { center, radius } => {
            rows.push(row("Radius", len(radius)));
            rows.push(row("Centre", point(at.point(center))));
        }
        Surface::Torus {
            origin,
            axis,
            major_radius,
            minor_radius,
        } => {
            rows.push(row("Major radius", len(major_radius)));
            rows.push(row("Minor radius", len(minor_radius)));
            rows.push(row("Axis", dir(at.dir(axis))));
            rows.push(row("Centre", point(at.point(origin))));
        }
        _ => {}
    }
    rows.push(row("Face area", format!("{} {u}²", num(f.area * k * k))));
    Some(rows)
}

/// The curve's name, as the inspector heads it.
#[must_use]
pub fn curve_name(c: &Curve) -> &'static str {
    match c {
        Curve::Line => "Line",
        Curve::Circle { .. } => "Circle",
        Curve::Ellipse { .. } => "Ellipse",
        Curve::Hyperbola => "Hyperbola",
        Curve::Parabola => "Parabola",
        Curve::Bezier => "Bézier",
        Curve::Bspline => "B-spline",
        Curve::Offset => "Offset curve",
        Curve::Other => "Other",
    }
}

/// The rows for edge `edge` of placed part `part` (#31), `None` when the
/// topology has no such edge.
#[must_use]
pub fn edge(topo: &Topology, part: usize, edge: usize) -> Option<Vec<Row>> {
    let placed = topo.parts.get(part)?;
    let proto = topo.prototypes.get(placed.prototype)?;
    let e = proto.edges.get(edge)?;
    let at = Placement(placed.transform);
    let k = at.scale();
    let u = &topo.units;
    let len = |v: f64| format!("{} {u}", num(v * k));
    let mut rows = vec![
        row("Part", format!("{} (#{part})", placed.name)),
        row("Edge", format!("#{edge}")),
        row("Curve", curve_name(&e.curve)),
    ];
    match e.curve {
        Curve::Circle {
            center,
            normal,
            radius,
        } => {
            rows.push(row("Radius", len(radius)));
            rows.push(row("Diameter", format!("Ø {}", len(2.0 * radius))));
            rows.push(row("Centre", point(at.point(center))));
            rows.push(row("Normal", dir(at.dir(normal))));
        }
        Curve::Ellipse {
            center,
            normal,
            major_radius,
            minor_radius,
        } => {
            rows.push(row("Major radius", len(major_radius)));
            rows.push(row("Minor radius", len(minor_radius)));
            rows.push(row("Centre", point(at.point(center))));
            rows.push(row("Normal", dir(at.dir(normal))));
        }
        _ => {}
    }
    rows.push(row("Length", len(e.length)));
    Some(rows)
}

/// The rows for placed part `part`: volume and its box in the model.
#[must_use]
pub fn part(topo: &Topology, part: usize) -> Vec<Row> {
    let Some(placed) = topo.parts.get(part) else {
        return Vec::new();
    };
    let Some(proto) = topo.prototypes.get(placed.prototype) else {
        return Vec::new();
    };
    let u = &topo.units;
    let at = Placement(placed.transform);
    let k = at.scale();
    let mut rows = vec![row(
        "Part volume",
        proto
            .volume
            .map_or("—".into(), |v| format!("{} {u}³", num(v * k * k * k))),
    )];
    if let Some(b) = proto.bbox {
        // The prototype's box, placed: the box around its eight corners.
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for c in 0..8 {
            let p = at.point([0, 1, 2].map(|k| if c >> k & 1 == 0 { b[k] } else { b[k + 3] }));
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        rows.push(row(
            "Part size",
            format!(
                "{} × {} × {} {u}",
                num(hi[0] - lo[0]),
                num(hi[1] - lo[1]),
                num(hi[2] - lo[2])
            ),
        ));
        rows.push(row("Part min", point(lo)));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn topo(json: &str) -> Topology {
        Topology::parse(json.as_bytes()).unwrap()
    }

    const PLATE_AND_PIN: &str = r#"{
      "format": "stepv-topology", "version": 1, "units": "mm",
      "tree": [{"name": "a", "children": [{"name": "plate", "part": 0}, {"name": "pin", "part": 1}]}],
      "parts": [
        {"name": "plate", "prototype": 0, "transform": [1,0,0,0, 0,1,0,0, 0,0,1,0]},
        {"name": "pin", "prototype": 1, "transform": [0,0,1,6, 0,1,0,15, -1,0,0,5]}
      ],
      "prototypes": [
        {"area": 1, "volume": 5748.67, "bbox": [0,0,0,40,30,5], "vertices": [],
         "edges": [
           {"curve": "line", "length": 40, "vertices": [null, null]},
           {"curve": "circle", "center": [20,15,5], "normal": [0,0,1], "radius": 4, "length": 25.132741228718345, "vertices": [null, null]}
         ],
         "faces": [
           {"surface": "plane", "origin": [0,0,5], "normal": [0,0,1], "area": 1149.7345175, "edges": []},
           {"surface": "cylinder", "origin": [20,15,0], "axis": [0,0,1], "radius": 4, "area": 125.66, "edges": []}
         ]},
        {"area": 1, "volume": null, "bbox": [-2,-2,0,2,2,15], "vertices": [], "edges": [],
         "faces": [{"surface": "cylinder", "origin": [0,0,0], "axis": [0,0,1], "radius": 2, "area": 188.5, "edges": []}]}
      ]
    }"#;

    #[test]
    fn numbers_are_short_and_honest() {
        assert_eq!(num(4.0), "4");
        assert_eq!(num(1_149.734_517_5), "1149.7345");
        assert_eq!(num(-0.000_01), "0");
        assert_eq!(num(0.5), "0.5");
        assert_eq!(dir([0.0, 0.0, 2.0]), "+z");
        assert_eq!(dir([-1.0, 0.0, 0.0]), "−x");
        assert_eq!(dir([1.0, 1.0, 0.0]), "(0.7071, 0.7071, 0)");
    }

    #[test]
    fn a_plane_shows_its_normal_and_area() {
        let t = topo(PLATE_AND_PIN);
        let rows = face(&t, 0, 0).unwrap();
        let get = |k| rows.iter().find(|r| r.key == k).unwrap().value.clone();
        assert_eq!(get("Surface"), "Plane");
        assert_eq!(get("Normal"), "+z");
        assert_eq!(get("Face area"), "1149.7345 mm²");
        assert_eq!(get("Part"), "plate (#0)");
    }

    #[test]
    fn a_cylinder_shows_radius_and_diameter() {
        let t = topo(PLATE_AND_PIN);
        let rows = face(&t, 0, 1).unwrap();
        let get = |k| rows.iter().find(|r| r.key == k).unwrap().value.clone();
        assert_eq!(get("Surface"), "Cylinder");
        assert_eq!(get("Radius"), "4 mm");
        assert_eq!(get("Diameter"), "Ø 8 mm");
        assert_eq!(get("Axis"), "+z");
    }

    #[test]
    fn surfaces_are_placed_by_their_part() {
        // The pin's prototype stands along +z; its placement turns z into
        // -x and moves it to (6, 15, 5).
        let t = topo(PLATE_AND_PIN);
        let rows = face(&t, 1, 0).unwrap();
        let get = |k| rows.iter().find(|r| r.key == k).unwrap().value.clone();
        assert_eq!(get("Axis"), "+x");
        assert_eq!(get("Through"), "(6, 15, 5)");
        let p = part(&t, 1);
        let get = |k| p.iter().find(|r| r.key == k).unwrap().value.clone();
        assert_eq!(get("Part size"), "15 × 4 × 4 mm");
        assert_eq!(get("Part volume"), "—");
    }

    #[test]
    fn a_scaled_instance_reports_scaled_sizes() {
        // The plate placed at twice its size: radius, area and volume follow
        // the mesh, which the kernel placed with the same matrix.
        let scaled = PLATE_AND_PIN.replace(
            r#""transform": [1,0,0,0, 0,1,0,0, 0,0,1,0]"#,
            r#""transform": [2,0,0,0, 0,2,0,0, 0,0,2,0]"#,
        );
        let t = topo(&scaled);
        let rows = face(&t, 0, 1).unwrap();
        let get = |k| rows.iter().find(|r| r.key == k).unwrap().value.clone();
        assert_eq!(get("Radius"), "8 mm");
        assert_eq!(get("Face area"), "502.64 mm²");
        let p = part(&t, 0);
        let get = |k| p.iter().find(|r| r.key == k).unwrap().value.clone();
        assert_eq!(get("Part volume"), "45989.36 mm³");
        assert_eq!(get("Part size"), "80 × 60 × 10 mm");
    }

    #[test]
    fn an_edge_shows_its_curve_and_length() {
        let t = topo(PLATE_AND_PIN);
        let rows = edge(&t, 0, 1).unwrap();
        let get = |k| rows.iter().find(|r| r.key == k).unwrap().value.clone();
        assert_eq!(get("Curve"), "Circle");
        assert_eq!(get("Radius"), "4 mm");
        assert_eq!(get("Length"), "25.1327 mm");
        assert_eq!(get("Normal"), "+z");
        let line = edge(&t, 0, 0).unwrap();
        assert!(line.iter().any(|r| r.key == "Length" && r.value == "40 mm"));
        assert!(edge(&t, 0, 2).is_none());
    }

    #[test]
    fn out_of_range_picks_have_no_rows() {
        let t = topo(PLATE_AND_PIN);
        assert!(face(&t, 0, 9).is_none());
        assert!(face(&t, 7, 0).is_none());
        assert!(part(&t, 7).is_empty());
    }
}
