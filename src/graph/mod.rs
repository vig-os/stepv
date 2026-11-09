//! stepv's own graph of a STEP file (ADR-0001, `docs/adr/0001-graph-core.md`).
//!
//! This is the entity layer: the file is memory-mapped and scanned once
//! (`lex.rs`) into an entity table, one row per entity instance in file
//! order, holding its `#id`, its interned type, the byte span of its
//! parameters and its references in CSR form, plus an id → node lookup.
//! Arguments are parsed only when something reads them ([`Graph::args`]);
//! reverse references are built on first use ([`Graph::referrers`]). No OCCT
//! is involved.
//!
//! A file the scanner can't read is refused with a [`GraphError`] naming the
//! byte offset; the callers fall back to the OCCT-only path (ADR §3).

mod args;
mod lex;

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::Path;
use std::sync::OnceLock;

pub use args::{ArgError, Args, MAX_DEPTH, Untrusted, Value};

use crate::header;

/// An entity instance: a dense index into the table, in file order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Node(u32);

impl Node {
    /// The row: 0 for the file's first entity instance.
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// An entity instance name as the file writes it: the `n` of `#n`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StepId(pub u32);

/// An entity's type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeName<'a> {
    /// `#n = NAME(…);`
    Simple(&'a str),
    /// `#n = (A(…) B(…));`: the part types, sorted.
    Complex(&'a [Box<str>]),
}

/// Why a file has no graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphError {
    /// Where in the file the problem is (0 for an I/O error).
    pub offset: u64,
    pub kind: ErrorKind,
}

/// What went wrong, at [`GraphError::offset`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    /// The file cannot be opened or mapped.
    Io(String),
    /// No `ISO-10303-21;` banner and `HEADER` section.
    NotPart21,
    UnterminatedString,
    UnterminatedComment,
    /// A record cut short, or with unbalanced parentheses.
    UnterminatedRecord,
    /// A section with no `ENDSEC;`.
    MissingEndsec,
    /// No `END-ISO-10303-21;`.
    MissingEnd,
    /// An `#id` over `u32::MAX`.
    IdOverflow,
    /// Valid Part 21 the graph does not read yet (edition 3 references and
    /// value instances, ADR §2.1); OCCT reads the file instead.
    Unsupported(&'static str),
    /// More entities or types than the table's columns hold, or a record
    /// longer than 4 GiB.
    TooLarge(&'static str),
    Syntax(&'static str),
}

impl GraphError {
    fn at(offset: usize, kind: ErrorKind) -> Self {
        Self {
            offset: offset as u64,
            kind,
        }
    }
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match &self.kind {
            ErrorKind::Io(e) => return write!(f, "cannot read the file: {e}"),
            ErrorKind::NotPart21 => "not an ISO 10303-21 file",
            ErrorKind::UnterminatedString => "unterminated string",
            ErrorKind::UnterminatedComment => "unterminated comment",
            ErrorKind::UnterminatedRecord => "unterminated entity instance",
            ErrorKind::MissingEndsec => "section without ENDSEC",
            ErrorKind::MissingEnd => "no END-ISO-10303-21",
            ErrorKind::IdOverflow => "entity id over 4294967295",
            ErrorKind::Unsupported(s) | ErrorKind::TooLarge(s) | ErrorKind::Syntax(s) => s,
        };
        write!(f, "byte {}: {what}", self.offset)
    }
}

impl std::error::Error for GraphError {}

/// The file's bytes: mapped, or owned ([`Graph::from_bytes`]).
enum Bytes {
    Map(memmap2::Mmap),
    Owned(Vec<u8>),
}

impl std::ops::Deref for Bytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Map(m) => m,
            Self::Owned(v) => v,
        }
    }
}

/// An interned entity type.
enum Type {
    Simple(Box<str>),
    Complex(Box<[Box<str>]>),
}

impl Type {
    fn has(&self, part: &str) -> bool {
        match self {
            Self::Simple(s) => &**s == part,
            Self::Complex(p) => p.binary_search_by(|p| (**p).cmp(part)).is_ok(),
        }
    }
}

/// What a file holds, by type ([`Graph::census`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Census {
    /// Entity instances, duplicates included.
    pub entities: u64,
    /// How many of them are complex.
    pub complex: u64,
    /// Records that repeat an id ([`Graph::duplicate_ids`]).
    pub duplicate_ids: u64,
    /// References to undefined ids ([`Graph::dangling`]).
    pub dangling: u64,
    /// Instances per type; a complex type is written `(A B C)`, its parts
    /// sorted.
    pub types: BTreeMap<String, u64>,
    /// Instances per part type: a simple entity counts once for its type, a
    /// complex one once for each of its parts.
    pub parts: BTreeMap<String, u64>,
}

/// The `#id` → node lookup (ADR §2.1): dense when the ids are dense enough,
/// else sorted and bisected. It holds each id's first definition.
enum Lookup {
    Dense(Vec<u32>),
    Sorted(Vec<(u32, u32)>),
}

const NONE: u32 = u32::MAX;

/// A STEP file's entity table.
pub struct Graph {
    bytes: Bytes,
    header: header::Info,
    types: Vec<Type>,
    ids: Vec<u32>,
    type_of: Vec<u16>,
    arg_start: Vec<u64>,
    arg_len: Vec<u32>,
    /// The forward CSR: node `n`'s references are
    /// `refs[ref_start[n]..ref_start[n + 1]]`.
    ref_start: Vec<u32>,
    refs: Vec<Node>,
    /// `(referrer, id)` of every reference to an id no record defines.
    dangling: Vec<(Node, StepId)>,
    /// The reverse CSR, built on first use.
    referrers: OnceLock<(Vec<u32>, Vec<Node>)>,
    lookup: Lookup,
    /// `(id, node)` of every definition of an id defined more than once,
    /// sorted.
    duplicates: Vec<(u32, u32)>,
    duplicate_ids: usize,
    data_params: Vec<Option<(usize, usize)>>,
}

impl Graph {
    /// Maps and scans the file at `path`.
    ///
    /// # Errors
    /// When the file can't be read, or isn't Part 21 that the scanner reads.
    pub fn open(path: &Path) -> Result<Graph, GraphError> {
        let io = |e: std::io::Error| GraphError::at(0, ErrorKind::Io(e.to_string()));
        let file = std::fs::File::open(path).map_err(io)?;
        if file.metadata().map_err(io)?.len() == 0 {
            return Err(GraphError::at(0, ErrorKind::NotPart21));
        }
        // SAFETY: the map is read-only and private to this process. A file
        // truncated by another process while it is mapped raises SIGBUS;
        // that hardening is filed for later (ADR §5).
        let map = unsafe { memmap2::Mmap::map(&file) }.map_err(io)?;
        Self::scan(Bytes::Map(map))
    }

    /// Scans a file already in memory (an fd input, tests).
    ///
    /// # Errors
    /// As [`Graph::open`].
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Graph, GraphError> {
        Self::scan(Bytes::Owned(bytes))
    }

    fn scan(bytes: Bytes) -> Result<Graph, GraphError> {
        let mut types = Vec::new();
        let mut interned: HashMap<Vec<u8>, u16> = HashMap::new();
        let (mut ids, mut type_of, mut arg_start, mut arg_len) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        // Until every id is known, `refs` holds the raw ids as `Node`s.
        let (mut ref_start, mut refs) = (vec![0u32], Vec::new());
        let mut sorted: Vec<&[u8]> = Vec::new();
        let mut key = Vec::new();
        let sections = lex::scan(&bytes, |r| {
            let too_large = |what| GraphError::at(r.args.0, ErrorKind::TooLarge(what));
            // A complex type's key is its sorted parts behind a '(', which
            // no simple type name starts with.
            key.clear();
            if r.complex {
                sorted.clear();
                sorted.extend_from_slice(r.parts);
                sorted.sort_unstable();
                for p in &sorted {
                    key.push(b'(');
                    key.extend_from_slice(p);
                }
            } else {
                key.extend_from_slice(r.parts[0]);
            }
            let t = match interned.get(key.as_slice()) {
                Some(&t) => t,
                None => {
                    let t = u16::try_from(types.len())
                        .map_err(|_| too_large("over 65536 entity types"))?;
                    // Type names are keywords, ASCII by construction.
                    let name = |p: &[u8]| Box::from(String::from_utf8_lossy(p));
                    types.push(if r.complex {
                        Type::Complex(sorted.iter().map(|p| name(p)).collect())
                    } else {
                        Type::Simple(name(r.parts[0]))
                    });
                    interned.insert(key.clone(), t);
                    t
                }
            };
            if ids.len() == NONE as usize {
                return Err(too_large("over 4294967294 entities"));
            }
            ids.push(r.id);
            type_of.push(t);
            arg_start.push(r.args.0 as u64);
            arg_len.push(
                u32::try_from(r.args.1 - r.args.0).map_err(|_| too_large("a record over 4 GiB"))?,
            );
            refs.extend(r.refs.iter().map(|&id| Node(id)));
            ref_start.push(
                u32::try_from(refs.len()).map_err(|_| too_large("over 4294967295 references"))?,
            );
            Ok(())
        })?;
        let (lookup, duplicates, duplicate_ids) = index(&ids);
        let mut header = header::Info {
            format: "step".into(),
            file_size: bytes.len() as u64,
            ..header::Info::default()
        };
        header::parse_head(&bytes[..bytes.len().min(header::HEAD_BYTES)], &mut header);
        let mut g = Graph {
            bytes,
            header,
            types,
            ids,
            type_of,
            arg_start,
            arg_len,
            ref_start,
            refs: Vec::new(),
            dangling: Vec::new(),
            referrers: OnceLock::new(),
            lookup,
            duplicates,
            duplicate_ids,
            data_params: sections.data,
        };
        g.ids.shrink_to_fit();
        g.type_of.shrink_to_fit();
        g.arg_start.shrink_to_fit();
        g.arg_len.shrink_to_fit();
        g.resolve_refs(refs);
        let products = g.of_simple_type("PRODUCT");
        g.header.product_count = Some(products as u64);
        Ok(g)
    }

    /// The `HEADER` section, as [`header::read`] reports it, with
    /// `product_count` from the table.
    #[must_use]
    pub fn header(&self) -> &header::Info {
        &self.header
    }

    /// How many entity instances the file holds, duplicates included.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Whether the `DATA` sections are empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Every node, in file order.
    pub fn nodes(&self) -> impl Iterator<Item = Node> + '_ {
        (0..self.ids.len() as u32).map(Node)
    }

    /// The node `#id` names, or `None` when no record defines it. An id
    /// defined more than once gives its first definition: what a reference
    /// written before every record would get (see [`Graph::resolve`]).
    #[must_use]
    pub fn node(&self, id: StepId) -> Option<Node> {
        let n = match &self.lookup {
            Lookup::Dense(v) => *v.get(id.0 as usize)?,
            Lookup::Sorted(v) => v[v.binary_search_by_key(&id.0, |&(i, _)| i).ok()?].1,
        };
        (n != NONE).then_some(Node(n))
    }

    /// The node a reference to `#id` written in `from`'s record names, as
    /// OCCT's reader resolves it. For an id defined once that is
    /// [`Graph::node`]. For a duplicated id, both records stay in the table,
    /// and a reference names the nearest definition before its own record,
    /// else the nearest after it (`StepData_StepReaderData::SetEntityNumbers`,
    /// checked against `occt::outline` in this module's tests).
    #[must_use]
    pub fn resolve(&self, id: StepId, from: Node) -> Option<Node> {
        let lo = self.duplicates.partition_point(|&(i, _)| i < id.0);
        let hi = self.duplicates.partition_point(|&(i, _)| i <= id.0);
        let defs = &self.duplicates[lo..hi];
        if defs.is_empty() {
            return self.node(id);
        }
        let before = defs.partition_point(|&(_, n)| n < from.0);
        let after = defs.partition_point(|&(_, n)| n <= from.0);
        defs[..before]
            .last()
            .or(defs.get(after))
            .map(|&(_, n)| Node(n))
    }

    /// How many records define an id an earlier record already defined.
    #[must_use]
    pub fn duplicate_ids(&self) -> usize {
        self.duplicate_ids
    }

    /// The `#id` of `n`'s record.
    #[must_use]
    pub fn id(&self, n: Node) -> StepId {
        StepId(self.ids[n.index()])
    }

    /// `n`'s type.
    #[must_use]
    pub fn type_name(&self, n: Node) -> TypeName<'_> {
        match &self.types[self.type_of[n.index()] as usize] {
            Type::Simple(s) => TypeName::Simple(s),
            Type::Complex(p) => TypeName::Complex(p),
        }
    }

    /// The parameter list of each `DATA` section (edition 3's
    /// `DATA('name', ('schema'));`), raw, in file order: `None` for a plain
    /// `DATA;`. Recorded, otherwise ignored (ADR §2.1).
    pub fn data_parameters(&self) -> impl Iterator<Item = Option<&[u8]>> + '_ {
        self.data_params
            .iter()
            .map(|p| p.map(|(s, e)| &self.bytes[s..e]))
    }

    /// The raw bytes of `n`'s parameters, parentheses included; for a
    /// complex instance, every part.
    pub(crate) fn arg_bytes(&self, n: Node) -> &[u8] {
        let s = self.arg_start[n.index()] as usize;
        &self.bytes[s..s + self.arg_len[n.index()] as usize]
    }

    /// Resolves the scan's raw reference ids into the forward CSR, per
    /// reference with [`Graph::resolve`] (#149). A dangling reference is left
    /// out of `refs` and listed in `dangling`.
    fn resolve_refs(&mut self, mut refs: Vec<Node>) {
        let (mut w, mut s) = (0, 0);
        for n in 0..self.len() {
            let e = self.ref_start[n + 1] as usize;
            self.ref_start[n] = w as u32;
            for k in s..e {
                let id = StepId(refs[k].0);
                match self.resolve(id, Node(n as u32)) {
                    Some(to) => {
                        refs[w] = to;
                        w += 1;
                    }
                    None => self.dangling.push((Node(n as u32), id)),
                }
            }
            s = e;
        }
        *self
            .ref_start
            .last_mut()
            .expect("one start per node, plus one") = w as u32;
        refs.truncate(w);
        refs.shrink_to_fit();
        self.ref_start.shrink_to_fit();
        self.refs = refs;
    }

    /// The nodes `n` references, across all its parts, in record order. A
    /// duplicated id resolves per reference ([`Graph::resolve`]); a dangling
    /// one is left out (see [`Graph::dangling`]).
    #[must_use]
    pub fn refs(&self, n: Node) -> &[Node] {
        let i = n.index();
        &self.refs[self.ref_start[i] as usize..self.ref_start[i + 1] as usize]
    }

    /// Every reference to an id that no record defines, as `(referrer, id)`,
    /// in file order. They don't refuse the file; [`Graph::args`] on a
    /// referrer gives [`ArgError::Dangling`].
    #[must_use]
    pub fn dangling(&self) -> &[(Node, StepId)] {
        &self.dangling
    }

    /// The nodes that reference `n`, in file order, once per reference. The
    /// reverse CSR is built on the first call, by a counting sort over the
    /// forward one, O(references).
    #[must_use]
    pub fn referrers(&self, n: Node) -> &[Node] {
        let (start, from) = self.referrers.get_or_init(|| {
            let mut start = vec![0u32; self.len() + 1];
            for r in &self.refs {
                start[r.index() + 1] += 1;
            }
            for i in 1..start.len() {
                start[i] += start[i - 1];
            }
            // `start[t]` is the next free slot of target t; after the fill it
            // is where t + 1's run begins, so shifting it once restores it.
            let mut from = vec![Node(0); self.refs.len()];
            for referrer in self.nodes() {
                for &t in self.refs(referrer) {
                    from[start[t.index()] as usize] = referrer;
                    start[t.index()] += 1;
                }
            }
            start.rotate_right(1);
            start[0] = 0;
            (start, from)
        });
        &from[start[n.index()] as usize..start[n.index() + 1] as usize]
    }

    /// Whether `n` is a `part`: its simple type, or one part of its complex
    /// type.
    #[must_use]
    pub fn is_a(&self, n: Node, part: &str) -> bool {
        self.types[self.type_of[n.index()] as usize].has(part)
    }

    /// `n`'s part types: its type for a simple entity, the sorted parts of a
    /// complex one.
    pub fn parts(&self, n: Node) -> impl Iterator<Item = &str> + '_ {
        let parts = match &self.types[self.type_of[n.index()] as usize] {
            Type::Simple(s) => std::slice::from_ref(s),
            Type::Complex(p) => p,
        };
        parts.iter().map(|p| &**p)
    }

    /// The nodes that are a `name` ([`Graph::is_a`]), complex ones included,
    /// in file order.
    pub fn of_type(&self, name: &str) -> impl Iterator<Item = Node> + '_ {
        let want: Vec<bool> = self.types.iter().map(|t| t.has(name)).collect();
        self.nodes()
            .filter(move |n| want[self.type_of[n.index()] as usize])
    }

    /// A simple entity's parameters, parsed now.
    ///
    /// # Errors
    /// [`ArgError::Complex`] for a complex entity (read its parts with
    /// [`Graph::args_of`]); otherwise when the span doesn't parse.
    pub fn args(&self, n: Node) -> Result<Args<'_>, ArgError> {
        match self.types[self.type_of[n.index()] as usize] {
            Type::Simple(_) => args::Parser::new(self, n).args(),
            Type::Complex(_) => Err(ArgError::Complex),
        }
    }

    /// The parameters of one part of `n`: in Part 21's internal mapping,
    /// only the attributes that part's own type declares. For a simple
    /// entity, its [`Graph::args`] when `part` is its type. `None` when `n`
    /// has no part `part`.
    #[must_use]
    pub fn args_of(&self, n: Node, part: &str) -> Option<Result<Args<'_>, ArgError>> {
        match &self.types[self.type_of[n.index()] as usize] {
            Type::Simple(s) => (&**s == part).then(|| self.args(n)),
            Type::Complex(p) if p.iter().any(|p| &**p == part) => {
                args::Parser::new(self, n).part(part)
            }
            Type::Complex(_) => None,
        }
    }

    /// Counts by type and by part.
    #[must_use]
    pub fn census(&self) -> Census {
        let mut per_type = vec![0u64; self.types.len()];
        for &t in &self.type_of {
            per_type[t as usize] += 1;
        }
        let mut c = Census {
            entities: self.len() as u64,
            duplicate_ids: self.duplicate_ids as u64,
            dangling: self.dangling.len() as u64,
            ..Census::default()
        };
        for (t, &count) in self.types.iter().zip(&per_type).filter(|(_, c)| **c > 0) {
            let parts = match t {
                Type::Simple(s) => {
                    c.types.insert(s.to_string(), count);
                    std::slice::from_ref(s)
                }
                Type::Complex(p) => {
                    c.complex += count;
                    c.types.insert(format!("({})", p.join(" ")), count);
                    p
                }
            };
            for p in parts {
                *c.parts.entry(p.to_string()).or_default() += count;
            }
        }
        c
    }

    fn of_simple_type(&self, name: &str) -> usize {
        let Some(t) = self
            .types
            .iter()
            .position(|t| matches!(t, Type::Simple(s) if &**s == name))
        else {
            return 0;
        };
        self.type_of
            .iter()
            .filter(|&&x| usize::from(x) == t)
            .count()
    }
}

/// Builds the lookup over `ids` (ADR §2.1: dense when the largest id is at
/// most 4× the count), the sorted `(id, node)` definitions of every
/// duplicated id, and how many records repeat an id.
fn index(ids: &[u32]) -> (Lookup, Vec<(u32, u32)>, usize) {
    let mut pairs: Vec<(u32, u32)> = ids.iter().copied().zip(0..).collect();
    pairs.sort_unstable();
    let duplicates: Vec<(u32, u32)> = pairs
        .chunk_by(|a, b| a.0 == b.0)
        .filter(|defs| defs.len() > 1)
        .flatten()
        .copied()
        .collect();
    pairs.dedup_by_key(|p| p.0);
    let extra = ids.len() - pairs.len();
    let max = pairs.last().map_or(0, |p| p.0 as usize);
    let lookup = if max <= 4 * ids.len() {
        let mut dense = vec![NONE; max + 1];
        pairs.iter().for_each(|&(id, n)| dense[id as usize] = n);
        Lookup::Dense(dense)
    } else {
        pairs.shrink_to_fit();
        Lookup::Sorted(pairs)
    };
    (lookup, duplicates, extra)
}

#[cfg(test)]
mod tests;
