//! Colours from the style chain (ADR-0001 §2.2, G7): `STYLED_ITEM` →
//! `PRESENTATION_STYLE_ASSIGNMENT` → `SURFACE_STYLE_USAGE` →
//! `SURFACE_SIDE_STYLE` → `SURFACE_STYLE_FILL_AREA` → `FILL_AREA_STYLE` →
//! `FILL_AREA_STYLE_COLOUR` → `COLOUR_RGB` or `DRAUGHTING_PRE_DEFINED_COLOUR`.
//!
//! The graph is the source of truth for colours (ADR §7.2); OCCT's XCAF is
//! the test oracle, and each rule below is its observed output on the
//! fixtures of `tests/graph_colors.rs`, derived black-box (ADR §4: no OCCT
//! source read):
//! - a colour is the file's sRGB, converted to linear RGB like
//!   [`crate::Color`];
//! - an assignment's surface colour is its last `.BOTH.` or `.POSITIVE.`
//!   usage's, else its last `.NEGATIVE.` usage's;
//! - `OVER_RIDING_STYLED_ITEM` is a styled item like any other: of several
//!   styles on one item, the last in the file wins;
//! - a style need not be held by a presentation representation;
//! - XCAF splits a product into **parts** ([`Colors::parts`]) per shape
//!   definition representation and per representation one relates to
//!   (`SHAPE_REPRESENTATION_RELATIONSHIP`), per solid or shell of a related
//!   representation holding several, and for a shape aspect's solids and
//!   shells no other representation lists; annotations and placements are
//!   no shapes. It splits more than that in
//!   cases not modelled (a solid beside a curve set in one representation),
//!   where the parity test compares each face's drawn colour instead;
//! - the **product** colour (XCAF's product label) is a style on one of its
//!   shape representations, which XCAF applies only when some styled item
//!   in the file styles a representation item;
//! - a **part** colour is the style on its shape when that shape is one
//!   item (a solid, or a surface model of one shell, which is that shell
//!   too), else the product's; when a style styles both, the item's wins;
//! - a **face** colour (XCAF's sub-shape labels) is the face's own style,
//!   else the latest in the file of its containers' (shells, solids, sets)
//!   below its part's shape; `None` leaves the face the part's colour;
//! - a leaf's colour is its product's: styles on an assembly's
//!   representation or on an instance's context-dependent representation
//!   colour nothing in XCAF.
//!
//! Anything else (a rendering style, a curve style) colours nothing here;
//! where XCAF colours it, the corpus gate fails rather than defaulting.

use std::collections::{HashMap, HashSet};

use super::bounds::{CONTAINERS, SKIPPED, mapped_rep, ref_list, related_reps, wire};
use super::structure::{fields, refers};
use super::{Graph, MAX_DEPTH, Node, Product, Structure, Value};
use crate::Color;

/// A face's colour, in transfer order ([`PartColors::faces`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceColor {
    /// The face: an `ADVANCED_FACE`, `FACE_SURFACE`, triangulated face, or
    /// a surface of a geometric set.
    pub face: Node,
    /// Its own colour, or its container's; `None` when it shows its part's
    /// colour (or none).
    pub color: Option<Color>,
}

/// One of XCAF's parts of a product ([`Colors::parts`]).
#[derive(Debug, Clone, PartialEq)]
pub struct PartColors {
    /// The part's colour.
    pub color: Option<Color>,
    /// Its faces, in the order OCCT's transfer explores them.
    pub faces: Vec<FaceColor>,
}

/// A style: the styled item's node, which orders styles by file position,
/// and its colour.
type Style = (Node, Color);

/// The file's surface styles, by the item they style ([`colors`]).
#[derive(Debug, Clone, Default)]
pub struct Colors {
    /// Per styled representation item, its last style.
    items: HashMap<Node, Style>,
    /// Per styled representation, its last style.
    reps: HashMap<Node, Style>,
    /// Whether any styled item styles a representation item, which XCAF
    /// needs before it applies a representation's style.
    items_styled: bool,
}

/// Indexes every `STYLED_ITEM` and `OVER_RIDING_STYLED_ITEM`, simple or
/// complex, in O(file).
#[must_use]
pub fn colors(g: &Graph) -> Colors {
    let mut c = Colors::default();
    for si in g.nodes() {
        if !g.is_a(si, "STYLED_ITEM") && !g.is_a(si, "OVER_RIDING_STYLED_ITEM") {
            continue;
        }
        // A simple entity lists its name first; a complex one keeps the
        // name in its REPRESENTATION_ITEM part.
        let Ok(a) = fields(g, si, "STYLED_ITEM") else {
            continue;
        };
        let k = usize::from(g.args(si).is_ok());
        let Some(item) = refers(&a, k + 1) else {
            continue;
        };
        let rep = g.parts(item).any(|p| p.ends_with("REPRESENTATION"));
        c.items_styled |= !rep;
        let mut psas = Vec::new();
        ref_list(a.get(k), &mut psas);
        if let Some(colour) = surface_colour(g, &psas) {
            let map = if rep { &mut c.reps } else { &mut c.items };
            map.insert(item, (si, colour));
        }
    }
    c
}

impl Colors {
    /// The colour styled on `item`, a representation item.
    #[must_use]
    pub fn item(&self, item: Node) -> Option<Color> {
        self.items.get(&item).map(|&(_, c)| c)
    }

    /// The product's colour as XCAF colours its leaf: its one part's, else
    /// its product label's.
    #[must_use]
    pub fn product(&self, g: &Graph, p: &Product) -> Option<Color> {
        match &split(g, p)[..] {
            [items] => self.part(g, items, p),
            _ => self.label(p),
        }
    }

    /// Product `p`'s parts, in XCAF's order, each with its colour and its
    /// faces' (items in order, mapped items expanded, each item once).
    #[must_use]
    pub fn parts(&self, g: &Graph, s: &Structure, p: usize) -> Vec<PartColors> {
        let me = &s.products[p];
        let parts = split(g, me);
        let mut out = Vec::with_capacity(parts.len());
        for items in parts {
            let mut w = Walk {
                c: self,
                g,
                whole: whole(g, &items),
                seen: HashSet::new(),
                open: Vec::new(),
                out: Vec::new(),
            };
            w.items(&items);
            let color = self.part(g, &items, me);
            out.push(PartColors {
                color,
                faces: w.out,
            });
        }
        out
    }

    /// A part's colour: the style on its whole shape, else the product's.
    fn part(&self, g: &Graph, items: &[Node], p: &Product) -> Option<Color> {
        let whole = whole(g, items);
        let own = whole
            .iter()
            .filter_map(|n| self.items.get(n))
            .max_by_key(|s| s.0);
        own.map(|&(_, c)| c).or_else(|| self.label(p))
    }

    /// The product label's colour: a style on one of its representations.
    fn label(&self, p: &Product) -> Option<Color> {
        let styled = p.bodies.iter().filter_map(|r| self.reps.get(r));
        let style = styled.max_by_key(|s| s.0).filter(|_| self.items_styled);
        style.map(|&(_, c)| c)
    }
}

/// The product's shape items, split into XCAF's parts. Per shape
/// definition representation: its own items, then each related
/// representation's (one part per item when it holds several solids or
/// shells); its items alone when nothing is related. A shape aspect's
/// representation adds a part only for the solids and shells no other
/// lists. Each item once; placements and annotations left out.
fn split(g: &Graph, p: &Product) -> Vec<Vec<Node>> {
    let mut seen = HashSet::new();
    let mut take = |rep: Node| {
        let mut items = Vec::new();
        if let Ok(a) = fields(g, rep, "REPRESENTATION") {
            ref_list(a.get(1), &mut items);
        }
        items.retain(|&i| shape(g, i) && seen.insert(i));
        items
    };
    let mut parts = Vec::new();
    let direct: Vec<Node> = p
        .bodies
        .iter()
        .copied()
        .filter(|&b| defines(g, b, p.def))
        .collect();
    let defining = if direct.is_empty() {
        &p.bodies
    } else {
        &direct
    };
    for &body in defining {
        let own = take(body);
        let mut related = Vec::new();
        for rep in related_reps(g, body) {
            let items = take(rep);
            let shells = items
                .iter()
                .filter(|&&i| CONTAINERS.iter().any(|t| g.is_a(i, t)));
            if shells.count() > 1 {
                related.extend(items.into_iter().map(|i| vec![i]));
            } else if !items.is_empty() {
                related.push(items);
            }
        }
        if !own.is_empty() {
            parts.push(own);
        }
        parts.append(&mut related);
    }
    // A shape aspect's own solids and shells.
    for &body in &p.bodies {
        let items = take(body);
        let solid = |i: &Node| CONTAINERS.iter().chain(&TESSELLATED).any(|t| g.is_a(*i, t));
        if items.iter().any(solid) {
            parts.push(items.into_iter().filter(solid).collect());
        }
    }
    if parts.is_empty() {
        parts.push(Vec::new());
    }
    parts
}

/// An item OCCT transfers to a shape: not a placement, nor an annotation.
fn shape(g: &Graph, n: Node) -> bool {
    let note = |p: &str| p.contains("TEXT") || p.contains("CALLOUT") || p.contains("ANNOTATION");
    !SKIPPED.iter().any(|t| g.is_a(n, t)) && !g.parts(n).any(note)
}

/// Whether a `SHAPE_DEFINITION_REPRESENTATION` gives `rep` as the shape of
/// `def` itself, not of one of its shape aspects.
fn defines(g: &Graph, rep: Node, def: Node) -> bool {
    g.referrers(rep).iter().any(|&sdr| {
        let pds = g
            .args_of(sdr, "SHAPE_DEFINITION_REPRESENTATION")
            .and_then(Result::ok)
            .and_then(|a| refers(&a, 0));
        pds.and_then(|d| fields(g, d, "PRODUCT_DEFINITION_SHAPE").ok())
            .is_some_and(|a| refers(&a, 2) == Some(def))
    })
}

/// The nodes whose shape is a part's whole shape: its single item, and a
/// surface model's single shell.
fn whole(g: &Graph, items: &[Node]) -> Vec<Node> {
    let [item] = items[..] else {
        return Vec::new();
    };
    let mut out = vec![item];
    if let Some(Ok(a)) = g.args_of(item, "SHELL_BASED_SURFACE_MODEL")
        && let Some(Value::List(shells)) = a.get(1)
        && let [Value::Ref(shell)] = shells[..]
    {
        out.push(shell);
    }
    out
}

/// Faces, which [`CONTAINERS`] lacks.
const FACES: [&str; 4] = [
    "ADVANCED_FACE",
    "FACE_SURFACE",
    "TRIANGULATED_FACE",
    "COMPLEX_TRIANGULATED_FACE",
];

/// Tessellated containers: their items are their second argument.
const TESSELLATED: [&str; 2] = ["TESSELLATED_SOLID", "TESSELLATED_SHELL"];

struct Walk<'a> {
    c: &'a Colors,
    g: &'a Graph,
    whole: Vec<Node>,
    /// Items met: a mapped item's twice is transferred once.
    seen: HashSet<Node>,
    /// The representations being walked, for mapped-item cycles.
    open: Vec<Node>,
    out: Vec<FaceColor>,
}

impl Walk<'_> {
    /// A mapped representation's items.
    fn rep(&mut self, rep: Node) {
        if self.open.contains(&rep) || self.open.len() >= MAX_DEPTH {
            return;
        }
        let mut items = Vec::new();
        if let Ok(a) = fields(self.g, rep, "REPRESENTATION") {
            ref_list(a.get(1), &mut items);
        }
        self.open.push(rep);
        self.items(&items);
        self.open.pop();
    }

    fn items(&mut self, items: &[Node]) {
        let g = self.g;
        for &item in items {
            if !self.seen.insert(item) || !shape(g, item) {
                continue;
            }
            if g.is_a(item, "MAPPED_ITEM") {
                if let Some(r) = mapped_rep(g, item) {
                    self.rep(r);
                }
            } else if let Some(Ok(a)) = g.args_of(item, "GEOMETRIC_SET") {
                let set = self.style(item, None);
                let mut elements = Vec::new();
                ref_list(a.get(1), &mut elements);
                for e in elements.into_iter().filter(|&e| !wire(g, e)) {
                    self.face(e, set);
                }
            } else {
                self.topo(item, None, 0);
            }
        }
    }

    /// `n`'s style when it is later in the file than `up`, else `up`. The
    /// product's whole shape passes `up` on: its style is the product's.
    fn style(&self, n: Node, up: Option<Style>) -> Option<Style> {
        match self.c.items.get(&n) {
            Some(&s) if !self.whole.contains(&n) && up.is_none_or(|u| s.0 > u.0) => Some(s),
            _ => up,
        }
    }

    /// A face's own style beats its containers', whatever their order.
    fn face(&mut self, n: Node, up: Option<Style>) {
        let own = self.c.items.get(&n).filter(|_| !self.whole.contains(&n));
        let color = own.copied().or(up).map(|(_, c)| c);
        self.out.push(FaceColor { face: n, color });
    }

    fn topo(&mut self, n: Node, up: Option<Style>, depth: usize) {
        let g = self.g;
        if depth > MAX_DEPTH {
            return;
        }
        if FACES.iter().any(|t| g.is_a(n, t)) {
            self.face(n, up);
        } else if let Some(t) = TESSELLATED.iter().find(|t| g.is_a(n, t)) {
            let here = self.style(n, up);
            let mut kids = Vec::new();
            if let Some(Ok(a)) = g.args_of(n, t) {
                ref_list(a.get(1), &mut kids);
            }
            kids.into_iter().for_each(|k| self.topo(k, here, depth + 1));
        } else if CONTAINERS.iter().any(|t| g.is_a(n, t)) {
            let here = self.style(n, up);
            for k in g.refs(n).to_vec() {
                self.topo(k, here, depth + 1);
            }
        }
    }
}

/// The surface colour of a styled item's assignments.
fn surface_colour(g: &Graph, psas: &[Node]) -> Option<Color> {
    let (mut front, mut back) = (None, None);
    for &psa in psas {
        let Ok(a) = fields(g, psa, "PRESENTATION_STYLE_ASSIGNMENT") else {
            continue;
        };
        let mut usages = Vec::new();
        ref_list(a.first(), &mut usages);
        for u in usages {
            let Ok(u) = fields(g, u, "SURFACE_STYLE_USAGE") else {
                continue;
            };
            let Some(c) = refers(&u, 1).and_then(|side| side_colour(g, side)) else {
                continue;
            };
            match u.first() {
                Some(Value::Enum("NEGATIVE")) => back = Some(c),
                Some(Value::Enum("BOTH" | "POSITIVE")) => front = Some(c),
                _ => {}
            }
        }
    }
    front.or(back)
}

/// A `SURFACE_SIDE_STYLE`'s first fill-area colour.
fn side_colour(g: &Graph, side: Node) -> Option<Color> {
    let a = fields(g, side, "SURFACE_SIDE_STYLE").ok()?;
    let mut elements = Vec::new();
    ref_list(a.get(1), &mut elements);
    elements.into_iter().find_map(|e| {
        let fill = refers(&fields(g, e, "SURFACE_STYLE_FILL_AREA").ok()?, 0)?;
        let mut colours = Vec::new();
        ref_list(
            fields(g, fill, "FILL_AREA_STYLE").ok()?.get(1),
            &mut colours,
        );
        colours.into_iter().find_map(|fc| {
            let a = fields(g, fc, "FILL_AREA_STYLE_COLOUR").ok()?;
            colour(g, refers(&a, 1)?)
        })
    })
}

/// A `COLOUR_RGB` (sRGB in the file) or a `DRAUGHTING_PRE_DEFINED_COLOUR`,
/// in linear RGB.
fn colour(g: &Graph, n: Node) -> Option<Color> {
    let [r, gr, b] = if let Some(a) = g.args_of(n, "COLOUR_RGB") {
        let a = a.ok()?;
        let x = |k| match a.get(k) {
            Some(Value::Real(r)) => Some(*r),
            Some(Value::Int(i)) => Some(*i as f64),
            _ => None,
        };
        [x(1)?, x(2)?, x(3)?]
    } else {
        let a = g.args_of(n, "DRAUGHTING_PRE_DEFINED_COLOUR")?.ok()?;
        let Some(Value::Str(name)) = a.first() else {
            return None;
        };
        match name.decode().as_str() {
            "red" => [1., 0., 0.],
            "green" => [0., 1., 0.],
            "blue" => [0., 0., 1.],
            "yellow" => [1., 1., 0.],
            "magenta" => [1., 0., 1.],
            "cyan" => [0., 1., 1.],
            "black" => [0., 0., 0.],
            "white" => [1., 1., 1.],
            _ => return None,
        }
    };
    let linear = |c: f64| -> f32 {
        let c = c.clamp(0., 1.);
        let l = if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        };
        l as f32
    };
    Some(Color {
        r: linear(r),
        g: linear(gr),
        b: linear(b),
    })
}
