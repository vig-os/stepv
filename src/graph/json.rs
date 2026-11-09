//! The graph as JSON (ADR-0001 §2.4): what `stepv graph` prints and what its
//! `query` filters read.
//!
//! Every string that comes from the file (header fields, product and instance
//! names, type names, enumerations) is written as `{"untrusted": "…"}`, never
//! as a bare string or an object key (#124's layer 1): a reader can always
//! tell the file's text from stepv's. Numbers, step ids and stepv's own words
//! (reasons, tiers) are written plainly.
//!
//! A [`View`] computes the structure and the boxes on first use, so a caller
//! that only reads entities never pays for them.

use std::cell::{OnceCell, RefCell};
use std::time::{Duration, Instant};

use serde_json::{Value as Json, json};

use super::{
    Bound, Bounds, Census, ExpandError, Graph, Node, Placement, Structure, StructureError, TextRef,
    TypeName, Value, bounds, structure,
};

/// File text as data: `{"untrusted": "…"}`.
#[must_use]
pub fn untrusted(s: &str) -> Json {
    json!({ "untrusted": s })
}

fn text(g: &Graph, t: Option<TextRef>) -> Json {
    t.map_or(Json::Null, |t| untrusted(&g.text(t).decode()))
}

/// A box in mm with its tier (#124's honesty contract: a graph box is a
/// proxy), or why there is none.
#[must_use]
pub fn bound(b: Bound) -> Json {
    match b {
        Bound::Box(b) => json!({ "min": b.min, "max": b.max, "tier": "proxy" }),
        Bound::Unknown(r) => json!({ "unknown": r.0 }),
    }
}

fn placement(p: Placement) -> Json {
    match p {
        Placement::Placed(t) => json!(t),
        Placement::Unplaced(r) => json!({ "unplaced": r.0 }),
    }
}

/// The census: totals, and the instances per type and per part, the type
/// names as data.
#[must_use]
pub fn census(c: &Census) -> Json {
    let counts = |m: &std::collections::BTreeMap<String, u64>| -> Json {
        m.iter()
            .map(|(t, n)| json!({ "type": untrusted(t), "count": n }))
            .collect()
    };
    json!({
        "entities": c.entities,
        "complex": c.complex,
        "duplicate_ids": c.duplicate_ids,
        "dangling": c.dangling,
        "types": counts(&c.types),
        "parts": counts(&c.parts),
    })
}

/// The caps and the page of [`View::tree`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeOptions {
    /// Occurrences deeper than this (roots being 0) are not listed.
    pub depth: u32,
    /// Listed occurrences to skip.
    pub offset: u64,
    /// The most to list.
    pub limit: u64,
    /// The expansion's caps, the outline's by default.
    pub expand: super::Expand,
}

impl Default for TreeOptions {
    fn default() -> Self {
        Self {
            depth: u32::MAX,
            offset: 0,
            limit: 1000,
            expand: super::Expand::default(),
        }
    }
}

/// A graph with its structure and boxes computed on first use, and the time
/// each phase took.
pub struct View<'g> {
    g: &'g Graph,
    structure: OnceCell<Result<Structure, StructureError>>,
    bounds: OnceCell<Bounds>,
    timings: RefCell<Vec<(&'static str, Duration)>>,
}

impl<'g> View<'g> {
    /// A view of `g`; nothing is computed yet.
    #[must_use]
    pub fn new(g: &'g Graph) -> Self {
        Self {
            g,
            structure: OnceCell::new(),
            bounds: OnceCell::new(),
            timings: RefCell::new(Vec::new()),
        }
    }

    /// The graph.
    #[must_use]
    pub fn graph(&self) -> &'g Graph {
        self.g
    }

    fn timed<T>(&self, phase: &'static str, f: impl FnOnce() -> T) -> T {
        let t = Instant::now();
        let v = f();
        self.timings.borrow_mut().push((phase, t.elapsed()));
        v
    }

    /// The phases run so far, in order.
    #[must_use]
    pub fn timings(&self) -> Vec<(&'static str, Duration)> {
        self.timings.borrow().clone()
    }

    /// The product structure, resolved on first use.
    ///
    /// # Errors
    /// Why the file has no structure.
    pub fn structure(&self) -> Result<&Structure, &StructureError> {
        self.structure
            .get_or_init(|| self.timed("structure", || structure(self.g)))
            .as_ref()
    }

    /// The boxes, computed on first use.
    ///
    /// # Errors
    /// Why the file has no structure.
    pub fn bounds(&self) -> Result<&Bounds, &StructureError> {
        let s = self.structure()?;
        Ok(self
            .bounds
            .get_or_init(|| self.timed("bounds", || bounds(self.g, s))))
    }

    /// The header, the units, the census's totals, the structure's size and
    /// the model's box. A file without a structure says why instead.
    #[must_use]
    pub fn summary(&self) -> Json {
        let h = self.g.header();
        let all = |v: &[String]| -> Json { v.iter().map(|s| untrusted(s)).collect() };
        let one = |v: &Option<String>| v.as_deref().map_or(Json::Null, untrusted);
        let header = json!({
            "format": h.format,
            "file_size": h.file_size,
            "schema": all(&h.schema),
            "protocol": h.protocol,
            "description": all(&h.description),
            "name": one(&h.name),
            "timestamp": one(&h.timestamp),
            "author": all(&h.author),
            "organization": all(&h.organization),
            "preprocessor": one(&h.preprocessor),
            "originating_system": one(&h.originating_system),
            "authorization": one(&h.authorization),
            "header_error": h.header_error,
        });
        let c = self.g.census();
        let mut out = json!({
            "header": header,
            "census": {
                "entities": c.entities,
                "complex": c.complex,
                "duplicate_ids": c.duplicate_ids,
                "dangling": c.dangling,
                "types": c.types.len(),
            },
        });
        match self.bounds() {
            Ok(b) => {
                let s = self.structure().expect("bounds imply a structure");
                out["units"] = units(s);
                out["structure"] = json!({
                    "products": s.products.len(),
                    "roots": s.roots.len(),
                    "leaves": s.leaves,
                });
                out["model_box"] = bound(b.model());
            }
            Err(e) => out["structure_error"] = json!(e.to_string()),
        }
        out
    }

    /// Every product: its names as data, its bodies and instances, and its
    /// box in its own mm.
    ///
    /// # Errors
    /// Why the file has no structure.
    pub fn products(&self) -> Result<Json, &StructureError> {
        let (s, b) = (self.structure()?, self.bounds()?);
        let id = |n: Node| self.g.id(n).0;
        Ok(s.products
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let children: Vec<Json> = p
                    .children
                    .iter()
                    .map(|c| {
                        json!({
                            "product": c.child,
                            "instance": id(c.nauo),
                            "name": text(self.g, c.name),
                            "placement": placement(c.placement),
                        })
                    })
                    .collect();
                json!({
                    "product": i,
                    "def": id(p.def),
                    "name": text(self.g, p.name),
                    "id": text(self.g, p.id),
                    "bodies": p.bodies.iter().map(|&n| id(n)).collect::<Vec<_>>(),
                    "shown": p.shown,
                    "leaf_count": p.leaf_count,
                    "mm_per_unit": p.mm_per_unit,
                    "children": children,
                    "box": bound(b.product(i)),
                })
            })
            .collect())
    }

    /// One page of the occurrence tree, depth-first in the outline's order:
    /// `{"rows", "offset", "next"}`, `next` being the offset of the next page
    /// or null. Each row is an occurrence: its depth, its path of
    /// `[product, slot]` pairs, the outline's label as data, its leaf number
    /// (null for an assembly) and its placement in the model.
    ///
    /// # Errors
    /// Why the file has no structure, or the outline's error when the
    /// expansion passes a cap before the page is full.
    pub fn tree(&self, opt: TreeOptions) -> Result<Json, TreeError> {
        let s = self
            .structure()
            .map_err(|e| TreeError::Structure(e.clone()))?;
        let mut rows = Vec::new();
        let mut listed = 0u64;
        let mut next = Json::Null;
        let walked = self.timed("expand", || {
            for o in s.occurrences(opt.expand) {
                let o = o.map_err(TreeError::Expand)?;
                let depth = o.path.0.len() - 1;
                if depth as u64 > u64::from(opt.depth) {
                    continue;
                }
                listed += 1;
                if listed <= opt.offset {
                    continue;
                }
                if rows.len() as u64 == opt.limit {
                    next = json!(opt.offset + opt.limit);
                    break;
                }
                rows.push(self.row(s, &o));
            }
            Ok(())
        });
        walked?;
        Ok(json!({ "rows": rows, "offset": opt.offset, "next": next }))
    }

    /// An occurrence's row, labelled as [`Structure::tree`] labels it: its
    /// product's name, else (when empty) its instance's.
    fn row(&self, s: &Structure, o: &super::Occurrence) -> Json {
        let label =
            |t: Option<TextRef>| t.map(|t| self.g.text(t).decode()).filter(|s| !s.is_empty());
        let (p, slot) = *o.path.0.last().expect("a path has a level");
        let mut name = label(s.products[p].name);
        if name.is_none() && o.path.0.len() > 1 {
            let (parent, _) = o.path.0[o.path.0.len() - 2];
            name = label(s.products[parent].children[slot].name);
        }
        json!({
            "depth": o.path.0.len() - 1,
            "path": o.path.0,
            "name": name.map_or(Json::Null, |n| untrusted(&n)),
            "leaf": o.leaf,
            "placement": placement(o.placement),
        })
    }

    /// The entity with step id `id`: `{"id", "type", "args"}`, or null when
    /// no record defines it. A complex entity's type is the list of its parts
    /// and its args a list of `{"part", "args"}`, one per part. References are
    /// `{"ref": id}`, strings, enumerations and type names data.
    #[must_use]
    pub fn entity(&self, id: u32) -> Json {
        let g = self.g;
        let Some(n) = g.node(super::StepId(id)) else {
            return Json::Null;
        };
        let args = |r: Result<super::Args<'_>, super::ArgError>| match r {
            Ok(a) => a.iter().map(|v| self.value(v)).collect(),
            Err(e) => json!({ "error": e.to_string() }),
        };
        let (ty, args) = match g.type_name(n) {
            TypeName::Simple(t) => (untrusted(t), args(g.args(n))),
            TypeName::Complex(parts) => (
                parts.iter().map(|p| untrusted(p)).collect(),
                parts
                    .iter()
                    .map(|p| {
                        let a = g.args_of(n, p).expect("a part of the entity");
                        json!({ "part": untrusted(p), "args": args(a) })
                    })
                    .collect(),
            ),
        };
        json!({ "id": id, "type": ty, "args": args })
    }

    fn value(&self, v: &Value<'_>) -> Json {
        match v {
            Value::Ref(n) => json!({ "ref": self.g.id(*n).0 }),
            Value::Int(i) => json!(i),
            Value::Real(r) => json!(r),
            Value::Str(s) => untrusted(&s.decode()),
            Value::Enum(e) => json!({ "enum": untrusted(e) }),
            Value::Binary(b) => json!({ "binary": untrusted(&String::from_utf8_lossy(b)) }),
            Value::List(l) => l.iter().map(|v| self.value(v)).collect(),
            Value::Typed(t, v) => json!({ "typed": untrusted(t), "value": self.value(v) }),
            Value::Null => Json::Null,
            Value::Derived => json!({ "derived": true }),
        }
    }

    /// The step ids entity `id` references, in record order; null when no
    /// record defines it.
    #[must_use]
    pub fn refs(&self, id: u32) -> Json {
        self.ids(id, Graph::refs)
    }

    /// The step ids of the entities that reference entity `id`; null when no
    /// record defines it.
    #[must_use]
    pub fn referrers(&self, id: u32) -> Json {
        self.ids(id, Graph::referrers)
    }

    fn ids(&self, id: u32, f: impl Fn(&'g Graph, Node) -> &'g [Node]) -> Json {
        self.g.node(super::StepId(id)).map_or(Json::Null, |n| {
            f(self.g, n).iter().map(|&r| self.g.id(r).0).collect()
        })
    }
}

/// The units the products' shapes are in: per distinct millimetres per
/// length unit, how many products use it.
fn units(s: &Structure) -> Json {
    let mut seen: Vec<(f64, u64)> = Vec::new();
    for p in s.products.iter().filter(|p| !p.bodies.is_empty()) {
        match seen.iter_mut().find(|(u, _)| *u == p.mm_per_unit) {
            Some((_, n)) => *n += 1,
            None => seen.push((p.mm_per_unit, 1)),
        }
    }
    seen.sort_by(|a, b| a.0.total_cmp(&b.0));
    seen.iter()
        .map(|(u, n)| json!({ "mm_per_unit": u, "products": n }))
        .collect()
}

/// Why [`View::tree`] has no page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeError {
    /// The file has no structure.
    Structure(StructureError),
    /// The expansion passed a cap.
    Expand(ExpandError),
}

impl std::fmt::Display for TreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Structure(e) => write!(f, "no structure: {e}"),
            Self::Expand(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for TreeError {}
