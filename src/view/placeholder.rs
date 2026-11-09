//! The viewer's first stage (ADR-0001 §2.7–2.8, G6a #138): the graph's tree
//! and a box placeholder per leaf, read in-process while the kernel runs, so
//! the window shows the assembly's layout long before the mesh arrives.
//!
//! A placeholder is the leaf's conservative box ([`crate::graph::Bounds::leaf`]):
//! it contains the leaf's exact geometry. A leaf whose box is `Unknown` has
//! no placeholder and is counted instead. When the kernel's scene arrives,
//! each placeholder is replaced by its leaf's mesh, joined by leaf number
//! ([`crate::topology::parts_per_leaf`]), and none outlives it (G6b, #139):
//! a leaf the kernel has no mesh for loses its box and is counted as
//! without geometry, and when the kernel's tree has another leaf count than
//! the graph's, the kernel's tree wins: no box maps, and all are dropped
//! and counted.

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

    /// The kernel's scene arrived, with its topology's tree: each leaf's
    /// mesh replaces its box ([`Placeholders::replace`]), the rest are
    /// dropped ([`Placeholders::done`]), and the status line counts them.
    /// Without the tree there is no join, and the scene replaces them all.
    /// No box is drawn after the scene.
    pub fn scene(&mut self, tree: Option<&[topology::Node]>) {
        self.scene = true;
        let (Some(mut p), Some(tree)) = (self.placeholders.take(), tree) else {
            return;
        };
        p.replace(&topology::parts_per_leaf(tree));
        p.done();
        self.status = Some(p.status());
    }
}

/// The leaves still drawn as boxes, and what the graph said of the rest.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Placeholders {
    /// (leaf number, box in model mm), in leaf order.
    pub boxes: Vec<(u64, BBox)>,
    /// The file's leaf count.
    pub leaves: u64,
    /// Leaves without a box (`Bound::Unknown`): no placeholder.
    pub unknown: u64,
    /// Boxes dropped once the kernel was done without a mesh for their
    /// leaf ([`Self::done`]): leaves without geometry.
    pub without_geometry: u64,
    /// The kernel's leaf count, when its tree disagrees with the graph's
    /// ([`Self::replace`]).
    pub kernel_leaves: Option<u64>,
    /// Boxes dropped because the kernel's tree disagrees: none maps.
    pub unmapped: u64,
}

impl Placeholders {
    /// Every leaf's box from `b`, over the structure's `leaves` leaves.
    #[must_use]
    pub fn of(b: &graph::Bounds, leaves: u64) -> Self {
        let mut p = Self {
            boxes: Vec::new(),
            leaves,
            unknown: 0,
            without_geometry: 0,
            kernel_leaves: None,
            unmapped: 0,
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
    /// `delivered[leaf]` lists that leaf's mesh parts, one entry per leaf
    /// of the kernel's tree ([`topology::parts_per_leaf`]).
    ///
    /// The join is by leaf number, so it holds only when both trees have
    /// the same leaves (ADR-0001 §3). When the kernel's leaf count differs,
    /// as on a multi-file assembly whose part files the graph doesn't read,
    /// the kernel's tree wins: every box left is dropped as unmapped.
    pub fn replace(&mut self, delivered: &[Vec<usize>]) {
        let kernel = delivered.len() as u64;
        if kernel != self.leaves {
            self.kernel_leaves = Some(kernel);
            self.unmapped += self.boxes.len() as u64;
            self.boxes.clear();
            return;
        }
        self.boxes.retain(|(leaf, _)| {
            usize::try_from(*leaf)
                .ok()
                .and_then(|l| delivered.get(l))
                .is_none_or(Vec::is_empty)
        });
    }

    /// The kernel is done: the boxes still left are of leaves it has no
    /// mesh for. They are dropped, and counted as without geometry, so the
    /// final view never shows a box that reads as "pending".
    pub fn done(&mut self) {
        self.without_geometry += self.boxes.len() as u64;
        self.boxes.clear();
    }

    /// The status line: "graph: 1,234 parts, 12 without a box", then what
    /// the scene dropped: ", 3 without geometry", or on a tree mismatch
    /// "; the kernel's tree has 1,240 parts: 1,222 placeholders dropped".
    #[must_use]
    pub fn status(&self) -> String {
        let mut s = format!("graph: {} parts", thousands(self.leaves));
        if self.unknown > 0 {
            s += &format!(", {} without a box", thousands(self.unknown));
        }
        if self.without_geometry > 0 {
            s += &format!(", {} without geometry", thousands(self.without_geometry));
        }
        if let Some(k) = self.kernel_leaves {
            s += &format!(
                "; the kernel's tree has {} parts: {} placeholders dropped",
                thousands(k),
                thousands(self.unmapped)
            );
        }
        s
    }

    /// The artifact's per-leaf boxes ([`crate::cache::OutlineArtifact::boxes`]),
    /// `[xmin, ymin, zmin, xmax, ymax, zmax]` in leaf order: only when every
    /// leaf has a box, since the schema can't say `Unknown`.
    #[must_use]
    pub fn leaf_boxes(&self) -> Option<Vec<[f64; 6]>> {
        (self.unknown == 0 && self.boxes.len() as u64 == self.leaves).then(|| {
            self.boxes
                .iter()
                .map(|(_, b)| [b.min[0], b.min[1], b.min[2], b.max[0], b.max[1], b.max[2]])
                .collect()
        })
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
            leaves: 1234,
            unknown: 12,
            ..Placeholders::default()
        };
        assert_eq!(p.status(), "graph: 1,234 parts, 12 without a box");
        assert_eq!(
            Placeholders {
                unknown: 0,
                ..p.clone()
            }
            .status(),
            "graph: 1,234 parts"
        );
        assert_eq!(
            Placeholders {
                without_geometry: 3,
                ..p.clone()
            }
            .status(),
            "graph: 1,234 parts, 12 without a box, 3 without geometry"
        );
        assert_eq!(
            Placeholders {
                kernel_leaves: Some(1240),
                unmapped: 1222,
                ..p
            }
            .status(),
            "graph: 1,234 parts, 12 without a box; the kernel's tree has 1,240 parts: \
             1,222 placeholders dropped"
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
                    ..Placeholders::default()
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
        s.scene(Some(&tree));
        assert!(s.placeholders.is_none());
        assert_eq!(s.status.as_deref(), Some("graph: 2 parts"));
        // The scene first (a mesh-cache hit): no boxes, the status still.
        let mut s = GraphState::default();
        s.scene(Some(&tree));
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
                s.scene(None);
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
            ..Placeholders::default()
        };
        // Leaf 0's mesh came; leaves 1 and 2 are still to come.
        p.replace(&[vec![0, 1], vec![], vec![]]);
        assert_eq!(p.boxes, vec![(1, b), (2, b)]);
        assert_eq!(p.leaf_boxes(), None, "boxes were replaced");
        // Done: leaf 2's came, leaf 1 has none, and its box goes too.
        p.replace(&[vec![0, 1], vec![], vec![2]]);
        p.done();
        assert!(p.boxes.is_empty());
        assert_eq!(p.without_geometry, 1);
        assert_eq!(p.kernel_leaves, None);
    }

    /// #186 item 1: a kernel tree with another leaf count shifts the
    /// numbering, so no box maps: all are dropped, and counted.
    #[test]
    fn a_kernel_tree_with_other_leaves_drops_every_box() {
        let b = BBox {
            min: [0.0; 3],
            max: [1.0; 3],
        };
        let mut p = Placeholders {
            boxes: vec![(0, b), (2, b)],
            leaves: 3,
            unknown: 1,
            ..Placeholders::default()
        };
        p.replace(&[vec![0], vec![1], vec![2], vec![3]]);
        p.done();
        assert!(p.boxes.is_empty());
        assert_eq!(
            (p.kernel_leaves, p.unmapped, p.without_geometry),
            (Some(4), 2, 0)
        );
    }

    #[test]
    fn the_artifact_has_boxes_only_when_every_leaf_has_one() {
        let b = BBox {
            min: [0.0, 1.0, 2.0],
            max: [3.0, 4.0, 5.0],
        };
        let all = Placeholders {
            boxes: vec![(0, b), (1, b)],
            leaves: 2,
            ..Placeholders::default()
        };
        assert_eq!(
            all.leaf_boxes(),
            Some(vec![[0.0, 1.0, 2.0, 3.0, 4.0, 5.0]; 2])
        );
        let some = Placeholders {
            boxes: vec![(0, b)],
            leaves: 2,
            unknown: 1,
            ..Placeholders::default()
        };
        assert_eq!(some.leaf_boxes(), None);
    }
}
