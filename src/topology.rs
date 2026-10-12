//! The model's exact topology (#21), as the kernel writes it with
//! `--topology`: the assembly tree, and per prototype its faces' surfaces,
//! edges' curves, vertices, area and volume. What a viewer needs for a model
//! tree and for measuring on the B-rep instead of the mesh.
//!
//! The format is specified at the top of `kernel/topology.cpp`. Its indices
//! line up with the mesh's: `parts[i]` is [`crate::Scene`]'s part `i`, and a
//! prototype's `faces[j]` is that part's mesh face id `j`.
//! [`Topology::check_against`] holds a file to that.

use serde::Deserialize;

pub type Point = [f64; 3];

#[derive(Debug, Clone, Deserialize)]
pub struct Topology {
    pub format: String,
    pub version: u32,
    pub units: String,
    /// The roots of the assembly tree, in file order.
    pub tree: Vec<Node>,
    pub parts: Vec<PlacedPart>,
    pub prototypes: Vec<Prototype>,
}

/// One node of the assembly tree.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum Node {
    /// A leaf: `parts[part]`.
    Part {
        name: String,
        part: usize,
    },
    Assembly {
        name: String,
        children: Vec<Node>,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlacedPart {
    pub name: String,
    pub prototype: usize,
    /// Row-major 3x4: prototype coordinates to model coordinates.
    pub transform: [f64; 12],
}

#[derive(Debug, Clone, Deserialize)]
pub struct Prototype {
    pub area: f64,
    /// Solids only.
    pub volume: Option<f64>,
    pub bbox: Option<[f64; 6]>,
    pub faces: Vec<Face>,
    pub edges: Vec<Edge>,
    pub vertices: Vec<Point>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Face {
    #[serde(flatten)]
    pub surface: Surface,
    pub area: f64,
    /// Indices into the prototype's edges.
    pub edges: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "surface", rename_all = "snake_case")]
pub enum Surface {
    /// `normal` points outward: the face's orientation is applied.
    Plane {
        origin: Point,
        normal: Point,
    },
    Cylinder {
        origin: Point,
        axis: Point,
        radius: f64,
    },
    Cone {
        origin: Point,
        axis: Point,
        radius: f64,
        semi_angle_deg: f64,
    },
    Sphere {
        center: Point,
        radius: f64,
    },
    Torus {
        origin: Point,
        axis: Point,
        major_radius: f64,
        minor_radius: f64,
    },
    Bspline,
    Bezier,
    Revolution,
    Extrusion,
    Offset,
    Other,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Edge {
    #[serde(flatten)]
    pub curve: Curve,
    pub length: f64,
    /// Indices into the prototype's vertices; equal on a closed edge.
    pub vertices: [Option<usize>; 2],
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "curve", rename_all = "snake_case")]
pub enum Curve {
    Line,
    Circle {
        center: Point,
        normal: Point,
        radius: f64,
    },
    Ellipse {
        center: Point,
        normal: Point,
        major_radius: f64,
        minor_radius: f64,
    },
    Hyperbola,
    Parabola,
    Bezier,
    Bspline,
    Offset,
    Other,
}

/// Why a topology file is not one this crate can use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopologyError(pub String);

impl std::fmt::Display for TopologyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bad topology: {}", self.0)
    }
}

impl std::error::Error for TopologyError {}

/// The product-level tree of `tree`: every assembly with nothing named
/// anywhere below it (one product the transfer split into several bodies, as
/// AP203's hybrid models are, sometimes nested a level) becomes one leaf
/// with its name, and the leaves are renumbered `0..` in order. This is what
/// [`crate::occt::Outline`] promises to equal.
#[must_use]
pub fn products(tree: &[Node]) -> Vec<Node> {
    fn unnamed(n: &Node) -> bool {
        match n {
            Node::Part { name, .. } => name.is_empty(),
            Node::Assembly { name, children } => name.is_empty() && children.iter().all(unnamed),
        }
    }
    fn go(n: &Node, next: &mut usize) -> Node {
        let leaf = |name: &str, next: &mut usize| {
            *next += 1;
            Node::Part {
                name: name.to_owned(),
                part: *next - 1,
            }
        };
        match n {
            Node::Part { name, .. } => leaf(name, next),
            Node::Assembly { name, children }
                if !children.is_empty() && children.iter().all(unnamed) =>
            {
                leaf(name, next)
            }
            Node::Assembly { name, children } => Node::Assembly {
                name: name.clone(),
                children: children.iter().map(|c| go(c, next)).collect(),
            },
        }
    }
    let mut next = 0;
    tree.iter().map(|n| go(n, &mut next)).collect()
}

impl Topology {
    /// Parses and validates a kernel topology file: version, and every index
    /// in range.
    ///
    /// # Errors
    /// On malformed JSON, another format or version, or a dangling index.
    pub fn parse(bytes: &[u8]) -> Result<Self, TopologyError> {
        let t: Self = serde_json::from_slice(bytes).map_err(|e| TopologyError(e.to_string()))?;
        if t.format != "stepv-topology" || t.version != 1 {
            return Err(TopologyError(format!("{} version {}", t.format, t.version)));
        }
        let err = |m: String| Err(TopologyError(m));
        for (i, p) in t.parts.iter().enumerate() {
            if p.prototype >= t.prototypes.len() {
                return err(format!("part {i} names prototype {}", p.prototype));
            }
        }
        fn leaves<'a>(n: &'a Node, out: &mut Vec<&'a usize>) {
            match n {
                Node::Part { part, .. } => out.push(part),
                Node::Assembly { children, .. } => children.iter().for_each(|c| leaves(c, out)),
            }
        }
        let mut parts = Vec::new();
        t.tree.iter().for_each(|n| leaves(n, &mut parts));
        if let Some(p) = parts.iter().find(|&&&p| p >= t.parts.len()) {
            return err(format!("tree names part {p}"));
        }
        for (k, proto) in t.prototypes.iter().enumerate() {
            if proto
                .faces
                .iter()
                .flat_map(|f| &f.edges)
                .any(|&e| e >= proto.edges.len())
            {
                return err(format!("prototype {k}: a face names a missing edge"));
            }
            if proto
                .edges
                .iter()
                .flat_map(|e| e.vertices.iter().flatten())
                .any(|&v| v >= proto.vertices.len())
            {
                return err(format!("prototype {k}: an edge names a missing vertex"));
            }
        }
        Ok(t)
    }

    /// Holds the topology to the mesh it was written with: the same parts,
    /// and per part the same faces, so a picked triangle's face id names its
    /// exact face.
    ///
    /// # Errors
    /// On the first part or face count that differs.
    pub fn check_against(&self, scene: &crate::Scene) -> Result<(), TopologyError> {
        if self.parts.len() != scene.parts.len() {
            return Err(TopologyError(format!(
                "{} parts, the mesh has {}",
                self.parts.len(),
                scene.parts.len()
            )));
        }
        for (i, (p, m)) in self.parts.iter().zip(&scene.parts).enumerate() {
            let faces = self.prototypes[p.prototype].faces.len();
            if faces != m.faces.len() {
                return Err(TopologyError(format!(
                    "part {i}: {faces} faces, the mesh has {}",
                    m.faces.len()
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(json: &str) -> Vec<Node> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_product_split_into_unnamed_bodies_is_one_product() {
        // NIST's hybrid AP203 parts: a solid and a wireframe under one name.
        let t = tree(r#"[{"name":"ctc","children":[{"name":"","part":0},{"name":"","part":1}]}]"#);
        assert_eq!(products(&t), tree(r#"[{"name":"ctc","part":0}]"#));
    }

    #[test]
    fn products_renumber_the_leaves_and_keep_real_assemblies() {
        // as1: a split PLATE beside a real sub-assembly of named parts.
        let t = tree(
            r#"[{"name":"as1","children":[
                {"name":"PLATE","children":[{"name":"","part":0},{"name":"","part":1}]},
                {"name":"NUT_BOLT","children":[{"name":"BOLT","part":2},{"name":"NUT","part":3}]}]}]"#,
        );
        assert_eq!(
            products(&t),
            tree(
                r#"[{"name":"as1","children":[{"name":"PLATE","part":0},
                    {"name":"NUT_BOLT","children":[{"name":"BOLT","part":1},{"name":"NUT","part":2}]}]}]"#
            )
        );
    }

    #[test]
    fn unnamed_bodies_nested_a_level_are_still_one_product() {
        // NIST CTC 02 (AP242): a body, an unnamed group of two, a body.
        let t = tree(
            r#"[{"name":"ctc2","children":[{"name":"","part":0},
                {"name":"","children":[{"name":"","part":1},{"name":"","part":2}]},
                {"name":"","part":3}]}]"#,
        );
        assert_eq!(products(&t), tree(r#"[{"name":"ctc2","part":0}]"#));
    }

    #[test]
    fn an_assembly_with_one_named_child_is_not_collapsed() {
        let t = tree(r#"[{"name":"a","children":[{"name":"","part":0},{"name":"pin","part":1}]}]"#);
        assert_eq!(products(&t), t);
    }

    const ONE_BOX: &str = r#"{"format":"stepv-topology","version":1,"units":"mm",
        "tree":[{"name":"assy","children":[{"name":"box","part":0}]}],
        "parts":[{"name":"box","prototype":0,"transform":[1,0,0,0,0,1,0,0,0,0,1,0]}],
        "prototypes":[{"area":6,"volume":1,"bbox":[0,0,0,1,1,1],
          "faces":[{"surface":"plane","origin":[0,0,0],"normal":[0,0,-1],"area":1,"edges":[0]},
                   {"surface":"cylinder","origin":[0,0,0],"axis":[0,0,1],"radius":2,"area":1,"edges":[]},
                   {"surface":"bspline","area":1,"edges":[]}],
          "edges":[{"curve":"circle","center":[0,0,0],"normal":[0,0,1],"radius":2,"length":12.5,"vertices":[0,0]},
                   {"curve":"line","length":1,"vertices":[null,null]}],
          "vertices":[[0,0,0]]}]}"#;

    #[test]
    fn parses_every_shape_of_entry() {
        let t = Topology::parse(ONE_BOX.as_bytes()).unwrap();
        assert!(matches!(&t.tree[0], Node::Assembly { children, .. } if children.len() == 1));
        let p = &t.prototypes[0];
        assert_eq!(
            p.faces[1].surface,
            Surface::Cylinder {
                origin: [0.0; 3],
                axis: [0.0, 0.0, 1.0],
                radius: 2.0
            }
        );
        assert_eq!(p.faces[2].surface, Surface::Bspline);
        assert!(matches!(p.edges[0].curve, Curve::Circle { radius, .. } if radius == 2.0));
        assert_eq!(p.edges[1].vertices, [None, None]);
    }

    #[test]
    fn rejects_dangling_indices_and_other_versions() {
        for (from, to) in [
            (r#""prototype":0"#, r#""prototype":1"#),
            (r#""part":0"#, r#""part":7"#),
            (r#""edges":[0]"#, r#""edges":[9]"#),
            (r#""vertices":[0,0]"#, r#""vertices":[0,4]"#),
            (r#""version":1"#, r#""version":2"#),
        ] {
            let bad = ONE_BOX.replacen(from, to, 1);
            assert!(Topology::parse(bad.as_bytes()).is_err(), "{to}");
        }
        assert!(Topology::parse(b"{}").is_err());
    }
}
