//! The product structure (ADR-0001 §2.2): products as a DAG, their child
//! instances, and the outline's tree expanded from it on demand.
//! Placements come with G3b (#135).
//!
//! It mirrors `kernel/outline.cpp` (`occt::outline`) rule for rule, so that
//! the outline is its test oracle and the leaf numbering stays the join key
//! between the graph and the kernel's parts: the roots, the children and their
//! order, which products are shown, both naming rules, and the depth and node
//! caps.
//!
//! Known differences, none of them in the corpus (#157): the outline memoises
//! "shown" at the depth it first meets a product, so a shapeless chain deeper
//! than 60 below a product it also meets higher up can differ; a cycle
//! refuses the whole file, even where the outline never reaches it.

use std::collections::HashMap;
use std::fmt;

use super::{ArgError, Args, Graph, Node, TextRef, Untrusted, Value};
use crate::topology;

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
                children.push(Instance {
                    nauo: r,
                    child,
                    name,
                });
            }
        }
        if root {
            roots.push(i);
        }
        products.push(Product {
            def,
            name,
            id,
            bodies: bodies(g, def),
            shown: false,
            leaf_count: 1,
            children,
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
    /// Per open level: the product and its next child to look at.
    stack: Vec<(usize, usize)>,
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
                    slot.map(|k| (s.roots[k], k))
                }
                Some((p, k)) => {
                    let kids = &s.products[*p].children;
                    let slot = (*k..kids.len()).find(|&j| s.visits(kids[j].child, depth, self.cap));
                    *k = slot.map_or(kids.len(), |j| j + 1);
                    match slot {
                        Some(j) => Some((kids[j].child, j)),
                        None => {
                            self.stack.pop();
                            self.path.pop();
                            continue;
                        }
                    }
                }
            };
            let Some((p, slot)) = next else {
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
            self.stack.push((p, 0));
            return Some(Ok(Occurrence {
                path: Path(self.path.clone()),
                leaf,
            }));
        }
        None
    }
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

#[cfg(test)]
mod tests;
