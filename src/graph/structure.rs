//! The product structure (ADR-0001 §2.2): products as a DAG, their child
//! instances with placements in mm, and the outline's tree expanded from it
//! on demand.
//!
//! It mirrors `kernel/outline.cpp` (`occt::outline`) rule for rule, so that
//! the outline is its test oracle and the leaf numbering stays the join key
//! between the graph and the kernel's parts: the roots, the children and their
//! order, which products are shown, both naming rules, and the depth and node
//! caps. Placements follow OCCT's transfer (`STEPControl_ActorRead`), read for
//! behaviour only: see [`Placement`].
//!
//! Known differences, none of them in the corpus (#157): the outline memoises
//! "shown" at the depth it first meets a product, so a shapeless chain deeper
//! than 60 below a product it also meets higher up can differ; OCCT places
//! the product whose shape uses the child representation rather than the
//! NAUO's child, and takes the last of several placements of one NAUO with
//! the first such product; a representation context without units keeps the
//! previous context's in OCCT, and is 1 here; a cycle refuses the whole file,
//! even where the outline never reaches it.

use std::collections::HashMap;
use std::fmt;

use super::{ArgError, Args, Graph, Node, TextRef, Untrusted, Value};
use crate::topology;

/// Why an instance has no placement. Never the identity in disguise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reason(pub &'static str);

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// Where an instance puts its child: a rigid transform from the child's mm
/// to the parent's mm, row-major 3×4 like `topology::PlacedPart::transform`.
///
/// For an `ITEM_DEFINED_TRANSFORMATION(item1, item2)` on the relationship
/// `(rep_1, rep_2)` it is `S_mm(item_child)⁻¹ · S_mm(item_parent)`, OCCT's
/// change of coordinates, applied to the child's shape. `S_mm` is the item's
/// frame with its location scaled by its own representation's unit. `item_k`
/// belongs to `rep_k` (swapped back when each is found in the other's items),
/// and the child is the representation the NAUO's child product uses, so a
/// relationship that reverses the NAUO gives the inverse.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Placement {
    /// Placed by this transform.
    Placed([f64; 12]),
    /// No placement this graph trusts.
    Unplaced(Reason),
}

/// One `PRODUCT_DEFINITION`: a node of the DAG.
#[derive(Debug, Clone, PartialEq)]
pub struct Product {
    /// The `PRODUCT_DEFINITION`.
    pub def: Node,
    /// What STEPCAF names it by: its product's name, else its id.
    pub name: Option<TextRef>,
    /// Its product's id.
    pub id: Option<TextRef>,
    /// The representations of its shape, as the transfer finds them: those of
    /// its shape definitions, and of its shape aspects' (GD&T aspects aside).
    pub bodies: Vec<Node>,
    /// Whether the outline shows it: it has a body, or a shown child.
    pub shown: bool,
    /// The outline leaves under it: 1 with no shown child, else its shown
    /// children's sum (saturating).
    pub leaf_count: u64,
    /// Its child instances, in the outline's order; shown or not.
    pub children: Vec<Instance>,
    /// Millimetres per length unit of its first body's context (1 without).
    pub mm_per_unit: f64,
}

/// A `NEXT_ASSEMBLY_USAGE_OCCURRENCE`: an edge of the DAG.
#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    /// The NAUO.
    pub nauo: Node,
    /// The child, an index into [`Structure::products`].
    pub child: usize,
    /// What STEPCAF names it by: its description, else its name, else its id.
    pub name: Option<TextRef>,
    /// The child's placement in this product.
    pub placement: Placement,
}

/// An occurrence's position: per level, `(product, slot)`, where `slot` is
/// the product's index in [`Structure::roots`] at the first level and in its
/// parent's `children` below.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path(pub Vec<(usize, usize)>);

/// The expansion's caps, the outline's by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Expand {
    /// The deepest level, roots being 0.
    pub max_depth: u32,
    /// The most occurrences.
    pub max_nodes: u64,
}

impl Default for Expand {
    fn default() -> Self {
        Self {
            max_depth: 60,
            max_nodes: 1_000_000,
        }
    }
}

/// An expansion over a cap; it reads as the outline's error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpandError {
    /// Nested deeper than this.
    Depth(u32),
    /// More occurrences than this.
    Nodes(u64),
}

impl fmt::Display for ExpandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Depth(d) => write!(f, "assembly nesting deeper than {d}"),
            Self::Nodes(1_000_000) => f.write_str("outline larger than a million nodes"),
            Self::Nodes(n) => write!(f, "outline larger than {n} nodes"),
        }
    }
}

impl std::error::Error for ExpandError {}

/// Why a file has no structure; the view falls back to OCCT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructureError {
    /// A product that is its own ancestor.
    Cycle(Node),
    /// An entity of the structure whose arguments don't read.
    Malformed(Node, ArgError),
}

impl fmt::Display for StructureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cycle(n) => write!(f, "product definition node {} is its own ancestor", n.0),
            Self::Malformed(n, e) => write!(f, "node {}: {e}", n.0),
        }
    }
}

impl std::error::Error for StructureError {}

/// One expanded occurrence.
#[derive(Debug, Clone, PartialEq)]
pub struct Occurrence {
    /// Where it is.
    pub path: Path,
    /// Its leaf number, when it is a leaf.
    pub leaf: Option<u64>,
    /// Its placement in the model: the instances' along the path, composed.
    pub placement: Placement,
}

/// The product DAG ([`structure`]): owned data only, no borrow of the graph.
#[derive(Debug, Clone, PartialEq)]
pub struct Structure {
    /// One per product definition, in file order.
    pub products: Vec<Product>,
    /// The products the transfer starts from, in file order, shown or not.
    pub roots: Vec<usize>,
    /// The outline's leaf count (saturating).
    pub leaves: u64,
    /// Per product, its longest chain of children (saturating).
    height: Vec<u32>,
}

const IDENTITY: [f64; 12] = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0.];

/// Resolves `g`'s product structure, in O(products + instances): nothing is
/// expanded.
///
/// # Errors
/// [`StructureError::Cycle`] when a product is its own ancestor;
/// [`StructureError::Malformed`] when a product, its formation, or an
/// instance doesn't read.
pub fn structure(g: &Graph) -> Result<Structure, StructureError> {
    let defs: Vec<Node> = g.nodes().filter(|&n| is_definition(g, n)).collect();
    let index: HashMap<Node, usize> = defs.iter().enumerate().map(|(i, &d)| (d, i)).collect();
    let mut products = Vec::with_capacity(defs.len());
    let mut roots = Vec::new();
    for (i, &def) in defs.iter().enumerate() {
        let (name, id) = product_names(g, def)?;
        let mut children = Vec::new();
        let mut root = !is_document(g, def);
        let mut last = None;
        for &r in g.referrers(def) {
            if !g.is_a(r, "NEXT_ASSEMBLY_USAGE_OCCURRENCE") || last.replace(r) == Some(r) {
                continue;
            }
            let a = fields(g, r, "NEXT_ASSEMBLY_USAGE_OCCURRENCE")?;
            if refers(&a, 4) == Some(def) {
                root = false;
            }
            let child = refers(&a, 4).and_then(|c| index.get(&c));
            if let (Some(&child), Some(true)) = (child, refers(&a, 3).map(|p| p == def)) {
                let name = [2, 1, 0]
                    .into_iter()
                    .find_map(|k| usable(g, r, &a, k))
                    .or_else(|| text(g, r, &a, 0));
                let placement = instance_placement(g, r);
                children.push(Instance {
                    nauo: r,
                    child,
                    name,
                    placement,
                });
            }
        }
        if root {
            roots.push(i);
        }
        let bodies = bodies(g, def);
        let mm_per_unit = bodies.first().map_or(1., |&b| mm_per_unit(g, b));
        products.push(Product {
            def,
            name,
            id,
            bodies,
            shown: false,
            leaf_count: 1,
            children,
            mm_per_unit,
        });
    }
    let order = post_order(&products)?;
    let mut height = vec![0u32; products.len()];
    for &p in &order {
        let kids = &products[p].children;
        let h = kids.iter().map(|c| height[c.child].saturating_add(1)).max();
        height[p] = h.unwrap_or(0);
        let shown: Vec<u64> = kids
            .iter()
            .filter(|c| products[c.child].shown)
            .map(|c| products[c.child].leaf_count)
            .collect();
        let me = &mut products[p];
        me.shown = !me.bodies.is_empty() || !shown.is_empty();
        if !shown.is_empty() {
            me.leaf_count = shown.into_iter().fold(0, u64::saturating_add);
        }
    }
    let leaves = roots
        .iter()
        .filter(|&&r| products[r].shown)
        .map(|&r| products[r].leaf_count)
        .fold(0, u64::saturating_add);
    Ok(Structure {
        products,
        roots,
        leaves,
        height,
    })
}

/// The children in post-order, by an iterative three-colour DFS, so a deep or
/// cyclic file can't overflow the stack.
fn post_order(products: &[Product]) -> Result<Vec<usize>, StructureError> {
    let mut colour = vec![0u8; products.len()]; // white, grey, black
    let mut order = Vec::with_capacity(products.len());
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for start in 0..products.len() {
        if colour[start] != 0 {
            continue;
        }
        colour[start] = 1;
        stack.push((start, 0));
        while let Some((p, k)) = stack.last_mut() {
            let Some(c) = products[*p].children.get(*k).map(|c| c.child) else {
                colour[*p] = 2;
                order.push(*p);
                stack.pop();
                continue;
            };
            *k += 1;
            match colour[c] {
                0 => {
                    colour[c] = 1;
                    stack.push((c, 0));
                }
                1 => return Err(StructureError::Cycle(products[c].def)),
                _ => {}
            }
        }
    }
    Ok(order)
}

impl Structure {
    /// Whether the outline visits product `c` as a child at `depth`: a shown
    /// product, or one whose children reach past the depth cap, where the
    /// outline counts every child as shown and so fails there.
    fn visits(&self, c: usize, depth: u32, cap: Expand) -> bool {
        self.products[c].shown
            || u64::from(depth) + u64::from(self.height[c]) > u64::from(cap.max_depth)
    }

    fn shown_children(&self, p: usize) -> impl Iterator<Item = (usize, &Instance)> + '_ {
        self.products[p]
            .children
            .iter()
            .enumerate()
            .filter(|(_, c)| self.products[c.child].shown)
    }

    /// The path to leaf number `leaf`, in O(depth × fan-out), without
    /// expanding; `None` past the last leaf.
    #[must_use]
    pub fn leaf_path(&self, leaf: u64) -> Option<Path> {
        let mut rest = leaf;
        let mut path = Vec::new();
        for (slot, &r) in self
            .roots
            .iter()
            .enumerate()
            .filter(|(_, r)| self.products[**r].shown)
        {
            let n = self.products[r].leaf_count;
            if rest >= n {
                rest -= n;
                continue;
            }
            path.push((r, slot));
            while let Some(&(p, _)) = path.last() {
                let mut next = None;
                for (slot, c) in self.shown_children(p) {
                    let n = self.products[c.child].leaf_count;
                    if rest < n {
                        next = Some((c.child, slot));
                        break;
                    }
                    rest -= n;
                }
                match next {
                    Some(step) => path.push(step),
                    None => return Some(Path(path)),
                }
            }
        }
        None
    }

    /// The leaf number of the leaf at `path`; `None` when `path` names no
    /// shown leaf.
    #[must_use]
    pub fn leaf_of(&self, path: &Path) -> Option<u64> {
        let (&(first, slot), rest) = path.0.split_first()?;
        let shown = |p: usize| self.products.get(p).is_some_and(|p| p.shown);
        if self.roots.get(slot) != Some(&first) || !shown(first) {
            return None;
        }
        let before = |n: u64, p: usize| n.saturating_add(self.products[p].leaf_count);
        let mut leaf = self.roots[..slot]
            .iter()
            .filter(|&&r| shown(r))
            .fold(0, |n, &r| before(n, r));
        let mut parent = first;
        for &(p, slot) in rest {
            let kids = &self.products[parent].children;
            if kids.get(slot).map(|c| c.child) != Some(p) || !shown(p) {
                return None;
            }
            leaf = kids[..slot]
                .iter()
                .filter(|c| shown(c.child))
                .fold(leaf, |n, c| before(n, c.child));
            parent = p;
        }
        (self.shown_children(parent).next().is_none()).then_some(leaf)
    }

    /// The occurrences, depth-first in the outline's order, expanded as they
    /// are read. Past a cap the next item is the outline's error, and the
    /// iteration ends.
    #[must_use]
    pub fn occurrences(&self, cap: Expand) -> Occurrences<'_> {
        Occurrences {
            s: self,
            cap,
            root: 0,
            stack: Vec::new(),
            path: Vec::new(),
            nodes: 0,
            leaf: 0,
            done: false,
        }
    }

    /// The outline's tree, names and leaf numbers included: what
    /// `occt::outline` gives for the same file.
    ///
    /// # Errors
    /// The outline's error when the expansion passes a cap.
    pub fn tree(&self, g: &Graph, cap: Expand) -> Result<Vec<topology::Node>, ExpandError> {
        let label = |t: Option<TextRef>| t.map(|t| g.text(t).decode()).unwrap_or_default();
        let mut stack: Vec<(String, Vec<topology::Node>)> = vec![(String::new(), Vec::new())];
        let close = |stack: &mut Vec<(String, Vec<topology::Node>)>| {
            let (name, children) = stack.pop().expect("an open assembly");
            stack
                .last_mut()
                .expect("the roots")
                .1
                .push(topology::Node::Assembly { name, children });
        };
        for o in self.occurrences(cap) {
            let o = o?;
            while stack.len() > o.path.0.len() {
                close(&mut stack);
            }
            let (p, slot) = *o.path.0.last().expect("a path has a level");
            let mut name = label(self.products[p].name);
            if name.is_empty() && o.path.0.len() > 1 {
                let (parent, _) = o.path.0[o.path.0.len() - 2];
                name = label(self.products[parent].children[slot].name);
            }
            match o.leaf {
                Some(part) => stack
                    .last_mut()
                    .expect("the roots")
                    .1
                    .push(topology::Node::Part {
                        name,
                        part: part as usize,
                    }),
                None => stack.push((name, Vec::new())),
            }
        }
        while stack.len() > 1 {
            close(&mut stack);
        }
        Ok(stack.pop().expect("the roots").1)
    }
}

/// The lazy expansion ([`Structure::occurrences`]).
pub struct Occurrences<'s> {
    s: &'s Structure,
    cap: Expand,
    root: usize,
    /// Per open level: the product, its next child to look at, its placement.
    stack: Vec<(usize, usize, Placement)>,
    path: Vec<(usize, usize)>,
    nodes: u64,
    leaf: u64,
    done: bool,
}

impl Iterator for Occurrences<'_> {
    type Item = Result<Occurrence, ExpandError>;

    fn next(&mut self) -> Option<Self::Item> {
        let s = self.s;
        while !self.done {
            let depth = self.stack.len() as u32;
            let next = match self.stack.last_mut() {
                None => {
                    let slot =
                        (self.root..s.roots.len()).find(|&k| s.visits(s.roots[k], 0, self.cap));
                    self.root = slot.map_or(usize::MAX, |k| k + 1);
                    slot.map(|k| (s.roots[k], k, Placement::Placed(IDENTITY)))
                }
                Some((p, k, at)) => {
                    let kids = &s.products[*p].children;
                    let slot = (*k..kids.len()).find(|&j| s.visits(kids[j].child, depth, self.cap));
                    *k = slot.map_or(kids.len(), |j| j + 1);
                    match slot {
                        Some(j) => Some((kids[j].child, j, compose(*at, kids[j].placement))),
                        None => {
                            self.stack.pop();
                            self.path.pop();
                            continue;
                        }
                    }
                }
            };
            let Some((p, slot, placement)) = next else {
                self.done = true;
                break;
            };
            // The outline's order of checks: the depth, then the count.
            self.nodes += 1;
            let err = if depth > self.cap.max_depth {
                Some(ExpandError::Depth(self.cap.max_depth))
            } else {
                (self.nodes > self.cap.max_nodes).then_some(ExpandError::Nodes(self.cap.max_nodes))
            };
            if let Some(e) = err {
                self.done = true;
                return Some(Err(e));
            }
            let kids = &s.products[p].children;
            let leaf = (!kids.iter().any(|c| s.visits(c.child, depth + 1, self.cap))).then(|| {
                self.leaf += 1;
                self.leaf - 1
            });
            self.path.push((p, slot));
            self.stack.push((p, 0, placement));
            return Some(Ok(Occurrence {
                path: Path(self.path.clone()),
                leaf,
                placement,
            }));
        }
        None
    }
}

/// The placement of a `MAPPED_ITEM` inside the representation `rep`:
/// `S_mm(mapping_origin)⁻¹ · S_mm(mapping_target)`, the origin in the mapped
/// representation's units and the target in `rep`'s, as OCCT's transfer
/// moves the mapped shape.
///
/// OCCT bakes this move into the shape, so `--topology` shows it in the
/// prototype's geometry, not in a part's transform; the test against the
/// kernel compares vertices for that reason.
#[must_use]
pub fn mapped_placement(g: &Graph, item: Node, rep: Node) -> Placement {
    let get = |n: Node, part: &str, k: usize| fields(g, n, part).ok().and_then(|a| refers(&a, k));
    let (Some(map), Some(target)) = (get(item, "MAPPED_ITEM", 1), get(item, "MAPPED_ITEM", 2))
    else {
        return Placement::Unplaced(Reason("malformed mapped item"));
    };
    let (Some(origin), Some(mapped)) = (
        get(map, "REPRESENTATION_MAP", 0),
        get(map, "REPRESENTATION_MAP", 1),
    ) else {
        return Placement::Unplaced(Reason("malformed representation map"));
    };
    match change_of_coordinates(g, origin, mapped, target, rep) {
        Ok(t) => Placement::Placed(t),
        Err(r) => Placement::Unplaced(r),
    }
}

/// The placement of NAUO `nauo`'s child, from the last
/// `CONTEXT_DEPENDENT_SHAPE_REPRESENTATION` on its shape, as the transfer
/// takes it.
fn instance_placement(g: &Graph, nauo: Node) -> Placement {
    let shapes = g
        .referrers(nauo)
        .iter()
        .filter(|&&s| g.is_a(s, "PRODUCT_DEFINITION_SHAPE"));
    let cdsr = shapes
        .flat_map(|&s| g.referrers(s))
        .filter(|&&c| g.is_a(c, "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION"))
        .last();
    let Some(&cdsr) = cdsr else {
        return Placement::Unplaced(Reason("no context-dependent shape representation"));
    };
    let unplaced = |r| Placement::Unplaced(Reason(r));
    let Some(rr) = fields(g, cdsr, "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION")
        .ok()
        .and_then(|a| refers(&a, 0))
    else {
        return unplaced("malformed context-dependent shape representation");
    };
    let Some(a) = fields(g, rr, "REPRESENTATION_RELATIONSHIP").ok() else {
        return unplaced("malformed representation relationship");
    };
    let (Some(rep1), Some(rep2)) = (refers(&a, 2), refers(&a, 3)) else {
        return unplaced("malformed representation relationship");
    };
    // A relationship without a transformation places at the identity, as in
    // OCCT; a simple one with it lists the operator last.
    const WITH: &str = "REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION";
    if !g.is_a(rr, WITH) {
        return Placement::Placed(IDENTITY);
    }
    let op = match g.type_name(rr) {
        super::TypeName::Simple(_) => refers(&a, 4),
        super::TypeName::Complex(_) => fields(g, rr, WITH).ok().and_then(|t| refers(&t, 0)),
    };
    let Some(op) = op else {
        return unplaced("malformed transformation");
    };
    if !g.is_a(op, "ITEM_DEFINED_TRANSFORMATION") {
        return unplaced(if g.is_a(op, "CARTESIAN_TRANSFORMATION_OPERATOR_3D") {
            "cartesian transformation operator"
        } else {
            "unsupported transformation"
        });
    }
    let idt = fields(g, op, "ITEM_DEFINED_TRANSFORMATION").ok();
    let (Some(item1), Some(item2)) = (
        idt.as_ref().and_then(|a| refers(a, 2)),
        idt.as_ref().and_then(|a| refers(a, 3)),
    ) else {
        return unplaced("malformed item-defined transformation");
    };
    match change_of_coordinates(g, item1, rep1, item2, rep2) {
        Ok(t) if reverses(g, nauo, rep1, rep2) => Placement::Placed(invert(t)),
        Ok(t) => Placement::Placed(t),
        Err(r) => Placement::Unplaced(r),
    }
}

/// Whether the relationship `(rep1, rep2)` reverses the NAUO: rep2 is the
/// child's (`STEPConstruct_Assembly::CheckSRRReversesNAUO`).
fn reverses(g: &Graph, nauo: Node, rep1: Node, rep2: Node) -> bool {
    let Ok(a) = fields(g, nauo, "NEXT_ASSEMBLY_USAGE_OCCURRENCE") else {
        return false;
    };
    let (parent, child) = (refers(&a, 3), refers(&a, 4));
    // The product whose plain shape definition uses `rep`.
    let owner = |rep: Node| {
        g.referrers(rep).iter().find_map(|&sdr| {
            if g.type_name(sdr) != super::TypeName::Simple("SHAPE_DEFINITION_REPRESENTATION") {
                return None;
            }
            let a = g.args(sdr).ok()?;
            let pds = refers(&a, 0).filter(|_| refers(&a, 1) == Some(rep))?;
            let def = g.args(pds).ok().and_then(|p| refers(&p, 2))?;
            is_definition(g, def).then_some(def)
        })
    };
    let (pd1, pd2) = (owner(rep1), owner(rep2));
    if pd1 == child && pd2 == parent {
        return false;
    }
    (pd2 == child && pd2.is_some()) || (pd1 == parent && pd1.is_some())
}

/// OCCT's change of coordinates `S_mm(origin)⁻¹ · S_mm(target)`, the origin
/// an item of `orig_rep` and the target one of `targ_rep`
/// (`STEPControl_ActorRead::ComputeTransformation`): items found each in the
/// other's representation are swapped back, and each is scaled to mm by the
/// representation it belongs to.
fn change_of_coordinates(
    g: &Graph,
    origin: Node,
    orig_rep: Node,
    target: Node,
    targ_rep: Node,
) -> Result<[f64; 12], Reason> {
    let items = |rep: Node| {
        fields(g, rep, "REPRESENTATION")
            .ok()
            .map(|a| match a.get(1) {
                Some(Value::List(v)) => v
                    .iter()
                    .filter_map(|v| {
                        if let Value::Ref(n) = v {
                            Some(*n)
                        } else {
                            None
                        }
                    })
                    .collect(),
                _ => Vec::new(),
            })
            .unwrap_or_default()
    };
    let (oi, ti) = (items(orig_rep), items(targ_rep));
    let (mut o, mut t) = (origin, target);
    if (!oi.contains(&o) || !ti.contains(&t)) && ti.contains(&o) && oi.contains(&t) {
        std::mem::swap(&mut o, &mut t);
    }
    let o = frame(g, o, mm_per_unit(g, orig_rep))?;
    let t = frame(g, t, mm_per_unit(g, targ_rep))?;
    Ok(compose_raw(invert(o), t))
}

/// `S_mm`: an `AXIS2_PLACEMENT_3D` as a frame (its local coordinates to its
/// representation's, in mm), built as OCCT builds a `gp_Ax2`.
fn frame(g: &Graph, item: Node, mm: f64) -> Result<[f64; 12], Reason> {
    let bad = Reason("placement is not a 3-D axis placement");
    if !g.is_a(item, "AXIS2_PLACEMENT_3D") {
        return Err(bad);
    }
    let a = fields(g, item, "AXIS2_PLACEMENT_3D").map_err(|_| bad)?;
    let coords = |n: Option<Node>, part: &str| -> Option<Vec<f64>> {
        let a = fields(g, n?, part).ok()?;
        let Some(Value::List(v)) = a.get(1) else {
            return None;
        };
        v.iter().map(real).collect()
    };
    let loc = coords(refers(&a, 1), "CARTESIAN_POINT")
        .filter(|c| c.len() == 3)
        .ok_or(bad)?;
    // A direction OCCT can't build leaves the default.
    let dir = |k: usize| {
        let d = coords(refers(&a, k), "DIRECTION").filter(|d| d.len() >= 3)?;
        let v = [d[0], d[1], d[2]];
        let n = dot(v, v).sqrt();
        (v.iter().all(|x| x.abs() < 1e100) && n > 0.).then(|| v.map(|x| x / n))
    };
    let z = dir(2).unwrap_or([0., 0., 1.]);
    let x = match dir(3) {
        // Not parallel within OCCT's angular precision (1e-12 rad).
        Some(r) if cross(z, r).iter().map(|c| c * c).sum::<f64>().sqrt() > (1e-12f64).sin() => {
            let d = dot(r, z);
            normalize([r[0] - d * z[0], r[1] - d * z[1], r[2] - d * z[2]])
        }
        _ => default_x(z),
    };
    let y = cross(z, x);
    let o = [loc[0] * mm, loc[1] * mm, loc[2] * mm];
    Ok([
        x[0], y[0], z[0], o[0], x[1], y[1], z[1], o[1], x[2], y[2], z[2], o[2],
    ])
}

/// The X direction `gp_Ax2(P, V)` picks for a main direction alone.
fn default_x(v: [f64; 3]) -> [f64; 3] {
    let [a, b, c] = v;
    let (aa, ba, ca) = (a.abs(), b.abs(), c.abs());
    normalize(if ba <= aa && ba <= ca {
        if aa > ca { [-c, 0., a] } else { [c, 0., -a] }
    } else if aa <= ba && aa <= ca {
        if ba > ca { [0., -c, b] } else { [0., c, -b] }
    } else if aa > ba {
        [-b, a, 0.]
    } else {
        [b, -a, 0.]
    })
}

/// Millimetres per length unit of `rep`'s context, as OCCT reads it
/// (`STEPConstruct_UnitContext::ComputeFactors`): the last length unit wins;
/// an `SI_UNIT` is its prefix's metres; a `CONVERSION_BASED_UNIT` its factor's
/// value times its SI target's prefix; anything else, or none, is 1.
fn mm_per_unit(g: &Graph, rep: Node) -> f64 {
    let ctx = fields(g, rep, "REPRESENTATION")
        .ok()
        .and_then(|a| refers(&a, 2));
    let units = ctx
        .and_then(|c| g.args_of(c, "GLOBAL_UNIT_ASSIGNED_CONTEXT"))
        .and_then(Result::ok);
    let Some(Value::List(units)) = units.as_ref().and_then(|a| a.first()) else {
        return 1.;
    };
    // A simple SI_UNIT lists its NAMED_UNIT's dimensions first.
    let prefix = |n: Node| -> Option<f64> {
        let a = g.args_of(n, "SI_UNIT")?.ok()?;
        let k = usize::from(matches!(g.type_name(n), super::TypeName::Simple(_)));
        Some(match a.get(k)? {
            Value::Enum(p) => si_prefix(p)?,
            _ => 1.,
        })
    };
    let mut mm = 1.;
    for u in units.iter().filter_map(|u| {
        if let Value::Ref(n) = u {
            Some(*n)
        } else {
            None
        }
    }) {
        if !g.is_a(u, "LENGTH_UNIT") {
            continue;
        }
        let factor = if g.is_a(u, "CONVERSION_BASED_UNIT") {
            (|| {
                let a = g.args_of(u, "CONVERSION_BASED_UNIT")?.ok()?;
                let m = fields(g, refers(&a, 1)?, "MEASURE_WITH_UNIT").ok()?;
                Some(real(m.first()?)? * prefix(refers(&m, 1)?)?)
            })()
        } else {
            prefix(u)
        };
        if let Some(f) = factor {
            mm = f * 1000.;
        }
    }
    mm
}

fn si_prefix(p: &str) -> Option<f64> {
    const P: [(&str, f64); 16] = [
        ("EXA", 1e18),
        ("PETA", 1e15),
        ("TERA", 1e12),
        ("GIGA", 1e9),
        ("MEGA", 1e6),
        ("KILO", 1e3),
        ("HECTO", 1e2),
        ("DECA", 1e1),
        ("DECI", 1e-1),
        ("CENTI", 1e-2),
        ("MILLI", 1e-3),
        ("MICRO", 1e-6),
        ("NANO", 1e-9),
        ("PICO", 1e-12),
        ("FEMTO", 1e-15),
        ("ATTO", 1e-18),
    ];
    P.iter().find(|(n, _)| *n == p).map(|&(_, f)| f)
}

/// The representations a product's shape transfers from (the outline's
/// `has_shape`, all of them rather than the first).
fn bodies(g: &Graph, def: Node) -> Vec<Node> {
    let mut out = Vec::new();
    let sdr = |s: Node, out: &mut Vec<Node>| {
        if g.type_name(s) == super::TypeName::Simple("SHAPE_DEFINITION_REPRESENTATION") {
            out.extend(g.args(s).ok().and_then(|a| refers(&a, 1)));
        }
    };
    for &pds in g
        .referrers(def)
        .iter()
        .filter(|&&p| g.is_a(p, "PRODUCT_DEFINITION_SHAPE"))
    {
        for &s in g.referrers(pds) {
            sdr(s, &mut out);
            if !is_aspect(g, s) || g.referrers(s).iter().any(|&t| is_tolerance(g, t)) {
                continue;
            }
            for &prop in g.referrers(s).iter().filter(|&&p| {
                g.is_a(p, "PROPERTY_DEFINITION") || g.is_a(p, "PRODUCT_DEFINITION_SHAPE")
            }) {
                for &s in g.referrers(prop) {
                    sdr(s, &mut out);
                }
            }
        }
    }
    out.dedup();
    out
}

/// A shape aspect other than a datum feature, by OCCT's class tree.
fn is_aspect(g: &Graph, n: Node) -> bool {
    const KINDS: [&str; 12] = [
        "SHAPE_ASPECT",
        "DATUM",
        "DATUM_TARGET",
        "PLACED_DATUM_TARGET_FEATURE",
        "COMPOSITE_SHAPE_ASPECT",
        "COMPOSITE_GROUP_SHAPE_ASPECT",
        "CONTINUOUS_SHAPE_ASPECT",
        "DERIVED_SHAPE_ASPECT",
        "ALL_AROUND_SHAPE_ASPECT",
        "BETWEEN_SHAPE_ASPECT",
        "DATUM_SYSTEM",
        "TOLERANCE_ZONE",
    ];
    !g.is_a(n, "DATUM_FEATURE") && KINDS.iter().any(|k| g.is_a(n, k))
}

/// A geometric tolerance of any kind.
fn is_tolerance(g: &Graph, n: Node) -> bool {
    g.parts(n).any(|p| {
        p == "GEOMETRIC_TOLERANCE"
            || p.ends_with("_TOLERANCE") && p != "PLUS_MINUS_TOLERANCE"
            || p.starts_with("GEOMETRIC_TOLERANCE_WITH")
    })
}

fn is_definition(g: &Graph, n: Node) -> bool {
    g.is_a(n, "PRODUCT_DEFINITION") || g.is_a(n, "PRODUCT_DEFINITION_WITH_ASSOCIATED_DOCUMENTS")
}

/// A document's definition, which the transfer doesn't start from: one whose
/// formation a `DOCUMENT_PRODUCT_EQUIVALENCE` shares.
fn is_document(g: &Graph, def: Node) -> bool {
    g.is_a(def, "PRODUCT_DEFINITION_WITH_ASSOCIATED_DOCUMENTS")
        && g.refs(def)
            .iter()
            .filter(|&&f| {
                g.parts(f)
                    .any(|p| p.starts_with("PRODUCT_DEFINITION_FORMATION"))
            })
            .any(|&f| {
                g.referrers(f)
                    .iter()
                    .any(|&e| g.is_a(e, "DOCUMENT_PRODUCT_EQUIVALENCE"))
            })
}

/// A product's name as STEPCAF takes it (its name, else its id), and its id.
fn product_names(
    g: &Graph,
    def: Node,
) -> Result<(Option<TextRef>, Option<TextRef>), StructureError> {
    let part = if g.is_a(def, "PRODUCT_DEFINITION") {
        "PRODUCT_DEFINITION"
    } else {
        "PRODUCT_DEFINITION_WITH_ASSOCIATED_DOCUMENTS"
    };
    let Some(formation) = refers(&fields(g, def, part)?, 2) else {
        return Ok((None, None));
    };
    let Some(product) = g.args(formation).ok().and_then(|a| refers(&a, 2)) else {
        return Ok((None, None));
    };
    if !g.is_a(product, "PRODUCT") {
        return Ok((None, None));
    }
    let a = fields(g, product, "PRODUCT")?;
    let id = text(g, product, &a, 0);
    Ok((usable(g, product, &a, 1).or(id), id))
}

/// `n`'s arguments: a simple entity's, or a complex one's part `part`.
fn fields<'g>(g: &'g Graph, n: Node, part: &str) -> Result<Args<'g>, StructureError> {
    let a = match g.type_name(n) {
        super::TypeName::Simple(_) => g.args(n),
        super::TypeName::Complex(_) => g.args_of(n, part).unwrap_or(Err(ArgError::Complex)),
    };
    a.map_err(|e| StructureError::Malformed(n, e))
}

fn refers(a: &Args<'_>, k: usize) -> Option<Node> {
    match a.get(k)? {
        Value::Ref(n) => Some(*n),
        _ => None,
    }
}

fn text(g: &Graph, n: Node, a: &Args<'_>, k: usize) -> Option<TextRef> {
    match a.get(k)? {
        Value::Str(s) => g.text_ref(n, *s),
        _ => None,
    }
}

/// Argument `k` when OCCT counts it as a name: it has a printable character
/// (`TCollection_AsciiString::UsefullLength`).
fn usable(g: &Graph, n: Node, a: &Args<'_>, k: usize) -> Option<TextRef> {
    let t = text(g, n, a, k)?;
    let s: Untrusted<'_> = g.text(t);
    s.decode()
        .bytes()
        .any(|b| b.is_ascii_graphic())
        .then_some(t)
}

fn real(v: &Value<'_>) -> Option<f64> {
    match v {
        Value::Real(x) => Some(*x),
        Value::Int(i) => Some(*i as f64),
        Value::Typed(_, v) => real(v),
        _ => None,
    }
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

fn normalize(v: [f64; 3]) -> [f64; 3] {
    let n = dot(v, v).sqrt();
    v.map(|x| x / n)
}

/// `a · b`: `b` applied first.
fn compose_raw(a: [f64; 12], b: [f64; 12]) -> [f64; 12] {
    let mut m = [0.; 12];
    for r in 0..3 {
        for c in 0..4 {
            m[r * 4 + c] = (0..3).map(|k| a[r * 4 + k] * b[k * 4 + c]).sum::<f64>()
                + if c == 3 { a[r * 4 + 3] } else { 0. };
        }
    }
    m
}

fn compose(a: Placement, b: Placement) -> Placement {
    match (a, b) {
        (Placement::Placed(a), Placement::Placed(b)) => Placement::Placed(compose_raw(a, b)),
        (Placement::Unplaced(r), _) | (_, Placement::Unplaced(r)) => Placement::Unplaced(r),
    }
}

/// The inverse of a rigid transform.
fn invert(m: [f64; 12]) -> [f64; 12] {
    let mut r = [0.; 12];
    for i in 0..3 {
        for j in 0..3 {
            r[i * 4 + j] = m[j * 4 + i];
        }
        r[i * 4 + 3] = -(0..3).map(|k| m[k * 4 + i] * m[k * 4 + 3]).sum::<f64>();
    }
    r
}

#[cfg(test)]
mod tests;
