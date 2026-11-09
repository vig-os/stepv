//! stepv's own graph of a STEP file (ADR-0001, `docs/adr/0001-graph-core.md`).
//!
//! This is the entity layer, slice G1: the file is memory-mapped and scanned
//! once (`lex.rs`) into an entity table, one row per entity instance in file
//! order, holding its `#id`, its interned type and the byte span of its
//! parameters, plus an id → node lookup. No OCCT is involved. References in
//! CSR form and lazily parsed arguments come with G2.
//!
//! A file the scanner can't read is refused with a [`GraphError`] naming the
//! byte offset; the callers fall back to the OCCT-only path (ADR §3).

mod lex;

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

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
            lookup,
            duplicates,
            duplicate_ids,
            data_params: sections.data,
        };
        g.ids.shrink_to_fit();
        g.type_of.shrink_to_fit();
        g.arg_start.shrink_to_fit();
        g.arg_len.shrink_to_fit();
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
    /// complex instance, every part. G2 parses them.
    #[cfg_attr(not(test), expect(dead_code, reason = "G2's lazy arguments read it"))]
    pub(crate) fn arg_bytes(&self, n: Node) -> &[u8] {
        let s = self.arg_start[n.index()] as usize;
        &self.bytes[s..s + self.arg_len[n.index()] as usize]
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
