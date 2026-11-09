//! The viewer's first stage (ADR-0001 §2.7–2.8, G6a #138): the graph's tree
//! and a box placeholder per leaf, read in-process while the kernel runs, so
//! the window shows the assembly's layout long before the mesh arrives.
//!
//! A placeholder is the leaf's conservative box ([`crate::graph::Bounds::leaf`]):
//! it contains the leaf's exact geometry. A leaf whose box is `Unknown` has
//! no placeholder and is counted instead. When the kernel's scene arrives,
//! each placeholder is replaced by its leaf's mesh, joined by leaf number
//! ([`crate::topology::parts_per_leaf`]).

use std::path::Path;

use crate::BBox;
use crate::graph::{self, Bound, Expand, Graph};
use crate::topology;

/// The graph's view of a file: what the window shows until the kernel's.
#[derive(Debug, Clone)]
pub struct GraphView {
    /// The outline's tree, leaf numbers included ([`graph::Structure::tree`]).
    pub tree: Vec<topology::Node>,
    /// The tree's leaf count.
    pub leaves: usize,
    /// A box per leaf that has one.
    pub placeholders: Placeholders,
}

/// What the window keeps of the graph, whichever of the graph and the
/// kernel's scene arrives first (#138). The status line always gets the
/// graph's word; its boxes are drawn only while the scene is still to come.
#[derive(Debug, Default)]
pub struct GraphState {
    /// The status line: the graph's counts, or why it refused the file.
    pub status: Option<String>,
    /// The boxes still drawn: set only before the scene.
    pub placeholders: Option<Placeholders>,
    scene: bool,
}

impl GraphState {
    /// The graph's load arrived: records its status, and hands back its
    /// view (tree and boxes) to show when the scene hasn't come yet.
    pub fn graph(&mut self, g: Result<Box<GraphView>, String>) -> Option<Box<GraphView>> {
        match g {
            Err(why) => {
                self.status = Some(why);
                None
            }
            Ok(g) => {
                self.status = Some(g.placeholders.status());
                if self.scene {
                    return None;
                }
                self.placeholders = Some(g.placeholders.clone());
                Some(g)
            }
        }
    }

    /// The kernel's scene arrived, with its topology's tree: the boxes left
    /// once each leaf's mesh replaced its own ([`Placeholders::replace`]).
    /// Without the tree there is no join, and the scene replaces them all.
    pub fn scene(&mut self, tree: Option<&[topology::Node]>) -> Option<Placeholders> {
        self.scene = true;
        let mut p = self.placeholders.take()?;
        p.replace(&topology::parts_per_leaf(tree?));
        (!p.boxes.is_empty()).then_some(p)
    }
}

/// The leaves still drawn as boxes, and what the graph said of the rest.
#[derive(Debug, Clone, PartialEq)]
pub struct Placeholders {
    /// (leaf number, box in model mm), in leaf order.
    pub boxes: Vec<(u64, BBox)>,
    /// The file's leaf count.
    pub leaves: u64,
    /// Leaves without a box (`Bound::Unknown`): no placeholder.
    pub unknown: u64,
}

impl Placeholders {
    /// Every leaf's box from `b`, over the structure's `leaves` leaves.
    #[must_use]
    pub fn of(b: &graph::Bounds, leaves: u64) -> Self {
        let mut p = Self {
            boxes: Vec::new(),
            leaves,
            unknown: 0,
        };
        for leaf in 0..leaves {
            match b.leaf(leaf) {
                Bound::Box(bb) => p.boxes.push((leaf, bb)),
                Bound::Unknown(_) => p.unknown += 1,
            }
        }
        p
    }

    /// Drops the placeholder of every leaf the kernel delivered a mesh for:
    /// `delivered[leaf]` lists that leaf's mesh parts
    /// ([`topology::parts_per_leaf`]).
    pub fn replace(&mut self, delivered: &[Vec<usize>]) {
        self.boxes.retain(|(leaf, _)| {
            usize::try_from(*leaf)
                .ok()
                .and_then(|l| delivered.get(l))
                .is_none_or(Vec::is_empty)
        });
    }

    /// The status line: "graph: 1,234 parts, 12 without a box".
    #[must_use]
    pub fn status(&self) -> String {
        match self.unknown {
            0 => format!("graph: {} parts", thousands(self.leaves)),
            n => format!(
                "graph: {} parts, {} without a box",
                thousands(self.leaves),
                thousands(n)
            ),
        }
    }
}

/// `n` with a comma every three digits.
fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Reads `path`'s graph: its tree and placeholders, or why the graph
/// refuses it ([`crate::graph::GraphError`], a structure or expansion
/// error). A refusal leaves the viewer as it was before the graph: the
/// kernel's tree and scene, with the reason in the status line.
///
/// # Errors
/// The refusal's reason, as the status line says it.
pub fn load(path: &Path) -> Result<GraphView, String> {
    let refused = |e: &dyn std::fmt::Display| format!("graph refused: {e}");
    let g = Graph::open(path).map_err(|e| refused(&e))?;
    let s = graph::structure(&g).map_err(|e| refused(&e))?;
    let tree = s.tree(&g, Expand::default()).map_err(|e| refused(&e))?;
    let b = graph::bounds(&g, &s);
    Ok(GraphView {
        tree,
        // The expansion passed its cap of a million nodes: this fits.
        leaves: s.leaves as usize,
        placeholders: Placeholders::of(&b, s.leaves),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_read_with_thousands_separators() {
        assert_eq!(thousands(7), "7");
        assert_eq!(thousands(1234), "1,234");
        assert_eq!(thousands(1_234_567), "1,234,567");
        let p = Placeholders {
            boxes: Vec::new(),
            leaves: 1234,
            unknown: 12,
        };
        assert_eq!(p.status(), "graph: 1,234 parts, 12 without a box");
        assert_eq!(
            Placeholders { unknown: 0, ..p }.status(),
            "graph: 1,234 parts"
        );
    }

    /// #138 review: the graph's status reaches the status line whether the
    /// graph or the scene arrives first; its boxes only when it is first.
    #[test]
    fn the_graph_is_heard_in_either_order() {
        let view = || {
            let b = BBox {
                min: [0.0; 3],
                max: [1.0; 3],
            };
            Ok(Box::new(GraphView {
                tree: vec![topology::Node::Part {
                    name: "p".into(),
                    part: 0,
                }],
                leaves: 2,
                placeholders: Placeholders {
                    boxes: vec![(0, b), (1, b)],
                    leaves: 2,
                    unknown: 0,
                },
            }))
        };
        let tree = [
            topology::Node::Part {
                name: "a".into(),
                part: 0,
            },
            topology::Node::Part {
                name: "b".into(),
                part: 1,
            },
        ];
        // The graph first: its boxes show, then the meshes replace them.
        let mut s = GraphState::default();
        assert!(s.graph(view()).is_some());
        assert_eq!(s.status.as_deref(), Some("graph: 2 parts"));
        assert_eq!(s.scene(Some(&tree[..1])).map(|p| p.boxes.len()), Some(1));
        assert_eq!(s.status.as_deref(), Some("graph: 2 parts"));
        // The scene first (a mesh-cache hit): no boxes, the status still.
        let mut s = GraphState::default();
        assert!(s.scene(Some(&tree)).is_none());
        assert!(s.graph(view()).is_none());
        assert!(s.placeholders.is_none());
        assert_eq!(s.status.as_deref(), Some("graph: 2 parts"));
        // A refusal, either side of the scene.
        let why = || Err("graph refused: byte 9: a REFERENCE section".to_owned());
        for scene_first in [false, true] {
            let mut s = GraphState::default();
            if scene_first {
                s.scene(None);
            }
            assert!(s.graph(why()).is_none());
            if !scene_first {
                assert!(s.scene(None).is_none());
            }
            assert_eq!(
                s.status.as_deref(),
                Some("graph refused: byte 9: a REFERENCE section")
            );
        }
    }

    #[test]
    fn a_leaf_keeps_its_box_until_its_mesh_comes() {
        let b = BBox {
            min: [0.0; 3],
            max: [1.0; 3],
        };
        let mut p = Placeholders {
            boxes: vec![(0, b), (1, b), (2, b)],
            leaves: 3,
            unknown: 0,
        };
        // Leaf 1 came without geometry; leaf 2 is past the kernel's tree.
        p.replace(&[vec![0, 1], vec![]]);
        assert_eq!(p.boxes, vec![(1, b), (2, b)]);
    }
}
