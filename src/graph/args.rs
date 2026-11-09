//! Lazy arguments (ADR-0001 §2.1): one entity's parameter span, parsed only
//! when something asks for it, into values that borrow the file's bytes.

use std::fmt;

use super::{Graph, Node, StepId};
use crate::header;

/// The nesting cap for lists and typed parameters: deeper is an
/// [`ArgError::Depth`], so a hostile file can't overflow the stack.
pub const MAX_DEPTH: usize = 64;

/// One parameter.
#[derive(Debug, Clone, PartialEq)]
pub enum Value<'a> {
    /// `#n`, resolved per reference as the forward CSR resolves it.
    Ref(Node),
    /// An integer.
    Int(i64),
    /// A real: a number with a `.` or an exponent.
    Real(f64),
    /// `'…'`: file text, still encoded.
    Str(Untrusted<'a>),
    /// `.NAME.`, without the dots.
    Enum(&'a str),
    /// `"…"`: the hex digits, without the quotes.
    Binary(&'a [u8]),
    /// `(a, b, …)`
    List(Vec<Value<'a>>),
    /// `NAME(value)`: a typed parameter, such as `LENGTH_MEASURE(25.4)`.
    Typed(&'a str, Box<Value<'a>>),
    /// `$`: no value.
    Null,
    /// `*`: derived by a supertype.
    Derived,
}

/// An entity's (or one complex part's) parameters, in order.
#[derive(Debug, Clone, PartialEq)]
pub struct Args<'a>(pub Vec<Value<'a>>);

impl<'a> std::ops::Deref for Args<'a> {
    type Target = [Value<'a>];
    fn deref(&self) -> &[Value<'a>] {
        &self.0
    }
}

/// File text: attacker-controlled (#124). No `Display` and no `Deref<str>`,
/// and `Debug` shows only the length: callers decide what leaves, through
/// #124's text policy, after an explicit [`Untrusted::decode`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Untrusted<'a>(&'a [u8]);

impl Untrusted<'_> {
    /// The string's value: `''` collapsed, CR/LF stripped, then the Part 21
    /// escapes decoded ([`header::decode_string`], as the header path does).
    #[must_use]
    pub fn decode(&self) -> String {
        let mut raw = Vec::with_capacity(self.0.len());
        let mut i = 0;
        while i < self.0.len() {
            raw.push(self.0[i]);
            i += if self.0[i..].starts_with(b"''") { 2 } else { 1 };
        }
        header::decode_string(&raw)
    }
}

impl fmt::Debug for Untrusted<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Untrusted({} bytes)", self.0.len())
    }
}

/// Why parameters didn't parse. Offsets are bytes into the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgError {
    /// [`Graph::args`] on a complex entity: its parts are read one at a time
    /// with [`Graph::args_of`], never as a silent first part.
    Complex,
    /// A reference to an id no record defines.
    Dangling {
        /// The reference's byte offset.
        offset: u64,
        /// The undefined id.
        id: StepId,
    },
    /// Nested deeper than [`MAX_DEPTH`].
    Depth {
        /// Where the cap was hit.
        offset: u64,
    },
    /// Not a Part 21 parameter list.
    Syntax {
        /// Where it went wrong.
        offset: u64,
        /// What was expected.
        what: &'static str,
    },
}

impl fmt::Display for ArgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Complex => f.write_str("a complex entity's arguments are read per part"),
            Self::Dangling { offset, id } => write!(f, "byte {offset}: #{} is not defined", id.0),
            Self::Depth { offset } => write!(f, "byte {offset}: nested over {MAX_DEPTH} deep"),
            Self::Syntax { offset, what } => write!(f, "byte {offset}: {what}"),
        }
    }
}

impl std::error::Error for ArgError {}

/// A parser over one record's parameter bytes.
pub(super) struct Parser<'a> {
    g: &'a Graph,
    from: Node,
    b: &'a [u8],
    /// Where `b` starts in the file, for error offsets.
    base: u64,
    i: usize,
}

impl<'a> Parser<'a> {
    pub(super) fn new(g: &'a Graph, from: Node) -> Self {
        let b = g.arg_bytes(from);
        Self {
            g,
            from,
            b,
            base: g.arg_start[from.index()],
            i: 0,
        }
    }

    fn syntax(&self, what: &'static str) -> ArgError {
        ArgError::Syntax {
            offset: self.base + self.i as u64,
            what,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    /// Skips whitespace and comments (the scanner already checked that each
    /// comment ends).
    fn ws(&mut self) {
        loop {
            while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
                self.i += 1;
            }
            if !self.b[self.i..].starts_with(b"/*") {
                return;
            }
            self.i += self.b[self.i..]
                .windows(2)
                .skip(2)
                .position(|w| w == b"*/")
                .map_or(self.b.len() - self.i, |n| n + 4);
        }
    }

    fn word(&mut self) -> &'a str {
        let start = self.i;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'!'))
        {
            self.i += 1;
        }
        // ASCII by the loop's test.
        std::str::from_utf8(&self.b[start..self.i]).unwrap_or_default()
    }

    /// A simple entity's parameter list: the whole span.
    pub(super) fn args(mut self) -> Result<Args<'a>, ArgError> {
        self.list(0).map(Args)
    }

    /// One part of a complex entity's `(A(…) B(…))`, or `None` when it has
    /// no part `part`.
    pub(super) fn part(mut self, part: &str) -> Option<Result<Args<'a>, ArgError>> {
        self.i = 1;
        loop {
            self.ws();
            if self.peek() != Some(b'(') && self.word() == part {
                self.ws();
                return Some(self.list(0).map(Args));
            }
            self.ws();
            if self.peek() != Some(b'(') {
                return None;
            }
            if let Err(e) = self.skip() {
                return Some(Err(e));
            }
        }
    }

    /// `(v, v, …)` at `self.i`.
    fn list(&mut self, depth: usize) -> Result<Vec<Value<'a>>, ArgError> {
        if depth >= MAX_DEPTH {
            return Err(ArgError::Depth {
                offset: self.base + self.i as u64,
            });
        }
        if self.peek() != Some(b'(') {
            return Err(self.syntax("expected '('"));
        }
        self.i += 1;
        let mut out = Vec::new();
        self.ws();
        if self.peek() == Some(b')') {
            self.i += 1;
            return Ok(out);
        }
        loop {
            self.ws();
            out.push(self.value(depth + 1)?);
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b')') => {
                    self.i += 1;
                    return Ok(out);
                }
                _ => return Err(self.syntax("expected ',' or ')'")),
            }
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value<'a>, ArgError> {
        let start = self.i;
        let Some(c) = self.peek() else {
            return Err(self.syntax("expected a parameter"));
        };
        Ok(match c {
            b'#' => {
                self.i += 1;
                let digits = self.b[self.i..]
                    .iter()
                    .take_while(|c| c.is_ascii_digit())
                    .count();
                self.i += digits;
                // The scanner checked the digits and their range.
                let id = StepId(
                    std::str::from_utf8(&self.b[start + 1..self.i])
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .ok_or_else(|| self.syntax("bad entity id"))?,
                );
                match self.g.resolve(id, self.from) {
                    Some(n) => Value::Ref(n),
                    None => {
                        return Err(ArgError::Dangling {
                            offset: self.base + start as u64,
                            id,
                        });
                    }
                }
            }
            b'\'' => {
                self.i += 1;
                loop {
                    let n = self.b[self.i..]
                        .iter()
                        .position(|&c| c == b'\'')
                        .ok_or_else(|| self.syntax("unterminated string"))?;
                    self.i += n + 1;
                    if self.peek() != Some(b'\'') {
                        break;
                    }
                    self.i += 1;
                }
                Value::Str(Untrusted(&self.b[start + 1..self.i - 1]))
            }
            b'"' => {
                self.i += 1;
                let n = self.b[self.i..]
                    .iter()
                    .position(|&c| c == b'"')
                    .ok_or_else(|| self.syntax("unterminated binary"))?;
                self.i += n + 1;
                Value::Binary(&self.b[start + 1..self.i - 1])
            }
            b'.' => {
                self.i += 1;
                let name = self.word();
                if self.peek() != Some(b'.') || name.is_empty() {
                    self.i = start;
                    return Err(self.syntax("expected an enumeration"));
                }
                self.i += 1;
                Value::Enum(name)
            }
            b'$' => {
                self.i += 1;
                Value::Null
            }
            b'*' => {
                self.i += 1;
                Value::Derived
            }
            b'(' => Value::List(self.list(depth)?),
            b'0'..=b'9' | b'+' | b'-' => self.number()?,
            _ => {
                let name = self.word();
                if name.is_empty() {
                    return Err(self.syntax("expected a parameter"));
                }
                self.ws();
                let inner = self.list(depth)?;
                let [v] = <[Value; 1]>::try_from(inner).map_err(|_| ArgError::Syntax {
                    offset: self.base + start as u64,
                    what: "a typed parameter holds one value",
                })?;
                Value::Typed(name, Box::new(v))
            }
        })
    }

    /// An integer, or a real when it has a `.` or an exponent.
    fn number(&mut self) -> Result<Value<'a>, ArgError> {
        let start = self.i;
        self.i += 1;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_digit() || matches!(c, b'.' | b'E' | b'e' | b'+' | b'-'))
        {
            self.i += 1;
        }
        let s = std::str::from_utf8(&self.b[start..self.i]).unwrap_or_default();
        let v = if s.contains(['.', 'E', 'e']) {
            s.parse().ok().map(Value::Real)
        } else {
            s.parse().ok().map(Value::Int)
        };
        v.ok_or(ArgError::Syntax {
            offset: self.base + start as u64,
            what: "bad number, or an integer over 64 bits",
        })
    }

    /// Skips a parenthesised group at `self.i` without building values.
    fn skip(&mut self) -> Result<(), ArgError> {
        let mut depth = 0usize;
        loop {
            match self.peek() {
                None => return Err(self.syntax("unbalanced parentheses")),
                Some(b'\'' | b'"') => {
                    self.value(0)?;
                    continue;
                }
                Some(b'/') if self.b[self.i..].starts_with(b"/*") => {
                    self.ws();
                    continue;
                }
                Some(b'(') => depth += 1,
                Some(b')') => {
                    depth -= 1;
                    if depth == 0 {
                        self.i += 1;
                        return Ok(());
                    }
                }
                Some(_) => {}
            }
            self.i += 1;
        }
    }
}
