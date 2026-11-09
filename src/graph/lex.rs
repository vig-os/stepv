//! The Part 21 scanner (ADR-0001 §2.1): one pass over the file's bytes that
//! finds every entity instance's `#id`, its type (or, for a complex instance,
//! its part types), the byte span of its parameters and the `#n` references
//! in them, in record order. Parameters are not otherwise parsed here: that
//! is [`super::Graph`]'s lazy layer.
//!
//! The grammar handled: the `ISO-10303-21;` banner, the `HEADER` section,
//! edition 3's `ANCHOR` and `SIGNATURE` sections (skipped) and parameterised
//! `DATA('name', ('schema'));`, records `#n = NAME(…);` and complex records
//! `#n = (A(…) B(…));`, strings with `''`, binaries, and `/* … */` comments.
//! Anything else refuses the file with the byte offset where it went wrong.

use super::{ErrorKind, GraphError};

/// One entity instance, as the scanner found it.
pub(crate) struct Record<'r, 'a> {
    pub id: u32,
    /// The type: one name for a simple instance, the part names in record
    /// order for a complex one.
    pub parts: &'r [&'a [u8]],
    pub complex: bool,
    /// The parameters' byte span, parentheses included: `(…)` after the type
    /// of a simple instance, the whole `(A(…) B(…))` of a complex one.
    pub args: (usize, usize),
    /// The ids of the `#n` references in the parameters, in record order.
    pub refs: &'r [u32],
}

/// What the scan found besides the records.
#[derive(Default)]
pub(crate) struct Sections {
    /// Each `DATA` section's parameter list `(…)` (edition 3), in file order;
    /// `None` for a plain `DATA;`.
    pub data: Vec<Option<(usize, usize)>>,
}

/// Bytes that end a run of ordinary parameter text.
const SPECIAL: [bool; 256] = {
    let mut t = [false; 256];
    let mut i = 0;
    let s = b"'\"/()@<;#";
    while i < s.len() {
        t[s[i] as usize] = true;
        i += 1;
    }
    t
};

/// Scans `b`, calling `on` for every entity instance in file order.
pub(crate) fn scan<'a>(
    b: &'a [u8],
    mut on: impl FnMut(&Record<'_, 'a>) -> Result<(), GraphError>,
) -> Result<Sections, GraphError> {
    let mut s = Scanner {
        b,
        i: 0,
        refs: Vec::new(),
    };
    let mut sections = Sections::default();
    let mut parts: Vec<&'a [u8]> = Vec::new();
    s.ws()?;
    if !s.b[s.i..].starts_with(b"ISO-10303-21") {
        return Err(s.err(ErrorKind::NotPart21));
    }
    s.i += 12;
    s.semicolon()?;
    s.ws()?;
    if s.word() != b"HEADER" {
        return Err(s.err(ErrorKind::NotPart21));
    }
    s.semicolon()?;
    s.skip_section()?;
    loop {
        s.ws()?;
        let at = s.i;
        match s.word() {
            b"DATA" => {
                s.ws()?;
                let params = if s.peek() == Some(b'(') {
                    Some((s.i, s.group(s.i, false)?))
                } else {
                    None
                };
                sections.data.push(params);
                s.semicolon()?;
                s.data_section(&mut parts, &mut on)?;
            }
            b"ANCHOR" | b"SIGNATURE" => {
                s.semicolon()?;
                s.skip_section()?;
            }
            b"REFERENCE" => {
                return Err(GraphError::at(
                    at,
                    ErrorKind::Unsupported("a REFERENCE section"),
                ));
            }
            b"END-ISO-10303-21" => {
                s.semicolon()?;
                // Whatever follows (edition 3 signatures) is not the model.
                return Ok(sections);
            }
            b"" if s.i == s.b.len() => return Err(s.err(ErrorKind::MissingEnd)),
            _ => return Err(GraphError::at(at, ErrorKind::Syntax("expected a section"))),
        }
    }
}

struct Scanner<'a> {
    b: &'a [u8],
    i: usize,
    /// The current record's references, as [`Scanner::group`] finds them.
    refs: Vec<u32>,
}

impl<'a> Scanner<'a> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn err(&self, kind: ErrorKind) -> GraphError {
        GraphError::at(self.i, kind)
    }

    /// Skips whitespace and comments.
    fn ws(&mut self) -> Result<(), GraphError> {
        loop {
            while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
                self.i += 1;
            }
            if !self.b[self.i..].starts_with(b"/*") {
                return Ok(());
            }
            self.comment()?;
        }
    }

    /// A comment starting at `self.i`.
    fn comment(&mut self) -> Result<(), GraphError> {
        let at = self.i;
        match self.b[at + 2..].windows(2).position(|w| w == b"*/") {
            Some(n) => self.i = at + 2 + n + 2,
            None => return Err(GraphError::at(at, ErrorKind::UnterminatedComment)),
        }
        Ok(())
    }

    /// A keyword: letters, digits, `_`, `!` (user-defined) and `-` (only
    /// `END-ISO-10303-21` has one). Empty when there is none.
    fn word(&mut self) -> &'a [u8] {
        let start = self.i;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'!' | b'-'))
        {
            self.i += 1;
        }
        &self.b[start..self.i]
    }

    fn semicolon(&mut self) -> Result<(), GraphError> {
        self.ws()?;
        if self.peek() != Some(b';') {
            return Err(self.err(ErrorKind::Syntax("expected ';'")));
        }
        self.i += 1;
        Ok(())
    }

    /// Skips a section's statements up to and including its `ENDSEC;`.
    fn skip_section(&mut self) -> Result<(), GraphError> {
        loop {
            self.ws()?;
            if self.i == self.b.len() {
                return Err(self.err(ErrorKind::MissingEndsec));
            }
            let at = self.i;
            if self.word() == b"ENDSEC" {
                return self.semicolon();
            }
            self.i = at;
            self.group(at, true)?;
        }
    }

    /// Records up to and including `ENDSEC;`.
    fn data_section(
        &mut self,
        parts: &mut Vec<&'a [u8]>,
        on: &mut impl FnMut(&Record<'_, 'a>) -> Result<(), GraphError>,
    ) -> Result<(), GraphError> {
        loop {
            self.ws()?;
            let at = self.i;
            match self.peek() {
                None => return Err(self.err(ErrorKind::MissingEndsec)),
                Some(b'#') => self.record(parts, on)?,
                _ if self.word() == b"ENDSEC" => return self.semicolon(),
                _ => {
                    return Err(GraphError::at(
                        at,
                        ErrorKind::Syntax("expected an entity instance or ENDSEC"),
                    ));
                }
            }
        }
    }

    /// `#n = NAME(…);` or `#n = (A(…) B(…));`, at its `#`.
    fn record(
        &mut self,
        parts: &mut Vec<&'a [u8]>,
        on: &mut impl FnMut(&Record<'_, 'a>) -> Result<(), GraphError>,
    ) -> Result<(), GraphError> {
        let at = self.i;
        let id = self.id()?;
        self.refs.clear();
        self.ws()?;
        if self.peek() != Some(b'=') {
            return Err(self.err(ErrorKind::Syntax("expected '='")));
        }
        self.i += 1;
        self.ws()?;
        parts.clear();
        let start = self.i;
        let complex = self.peek() == Some(b'(');
        if complex {
            self.i += 1;
            loop {
                self.ws()?;
                match self.peek() {
                    Some(b')') if !parts.is_empty() => break,
                    None => return Err(GraphError::at(at, ErrorKind::UnterminatedRecord)),
                    _ => {}
                }
                parts.push(self.type_name()?);
                self.ws()?;
                self.group(at, false)?;
            }
            self.i += 1;
        } else {
            parts.push(self.type_name()?);
            self.ws()?;
        }
        let args_start = if complex { start } else { self.i };
        if !complex {
            self.group(at, false)?;
        }
        let args = (args_start, self.i);
        self.semicolon()?;
        on(&Record {
            id,
            parts,
            complex,
            args,
            refs: &self.refs,
        })
    }

    /// `#n` at its `#`: the `n`.
    fn id(&mut self) -> Result<u32, GraphError> {
        let at = self.i;
        self.i += 1;
        let digits = self.b[self.i..]
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .count();
        if digits == 0 {
            return Err(self.err(ErrorKind::Syntax("expected an entity id after '#'")));
        }
        let id = self.b[self.i..self.i + digits]
            .iter()
            .try_fold(0u32, |n, &d| {
                n.checked_mul(10)?.checked_add(u32::from(d - b'0'))
            })
            .ok_or(GraphError::at(at, ErrorKind::IdOverflow))?;
        self.i += digits;
        Ok(id)
    }

    fn type_name(&mut self) -> Result<&'a [u8], GraphError> {
        let name = self.word();
        if name.is_empty() || name.contains(&b'-') {
            return Err(self.err(ErrorKind::Syntax("expected an entity type")));
        }
        Ok(name)
    }

    /// With `statement` false: a parenthesised group at `self.i`, through
    /// its matching `)`; returns the offset after it. With `statement` true:
    /// everything through the next `;` outside strings and comments (a
    /// header or anchor statement). `record` is where a truncation is
    /// reported. A group's `#n` references are appended to `self.refs`.
    fn group(&mut self, record: usize, statement: bool) -> Result<usize, GraphError> {
        if !statement && self.peek() != Some(b'(') {
            return Err(self.err(ErrorKind::Syntax("expected '('")));
        }
        let mut depth = 0u32;
        loop {
            while self.i < self.b.len() && !SPECIAL[self.b[self.i] as usize] {
                self.i += 1;
            }
            let at = self.i;
            if self.peek() == Some(b'#') && !statement {
                let id = self.id()?;
                self.refs.push(id);
                continue;
            }
            let Some(c) = self.peek() else {
                return Err(GraphError::at(
                    record,
                    if statement {
                        ErrorKind::MissingEndsec
                    } else {
                        ErrorKind::UnterminatedRecord
                    },
                ));
            };
            self.i += 1;
            match c {
                b'\'' => loop {
                    match self.b[self.i..].iter().position(|&c| c == b'\'') {
                        None => return Err(GraphError::at(at, ErrorKind::UnterminatedString)),
                        Some(n) => self.i += n + 1,
                    }
                    if self.peek() != Some(b'\'') {
                        break;
                    }
                    self.i += 1;
                },
                b'"' => match self.b[self.i..].iter().position(|&c| c == b'"') {
                    None => return Err(GraphError::at(at, ErrorKind::UnterminatedString)),
                    Some(n) => self.i += n + 1,
                },
                b'/' if self.peek() == Some(b'*') => {
                    self.i = at;
                    self.comment()?;
                }
                b'(' => depth += 1,
                b')' if depth > 0 => {
                    depth -= 1;
                    if depth == 0 && !statement {
                        return Ok(self.i);
                    }
                }
                b';' if statement && depth == 0 => return Ok(self.i),
                b';' | b')' if !statement => {
                    return Err(GraphError::at(record, ErrorKind::UnterminatedRecord));
                }
                b'@' if !statement => {
                    return Err(GraphError::at(
                        at,
                        ErrorKind::Unsupported("a value instance (@n)"),
                    ));
                }
                b'<' if !statement => {
                    return Err(GraphError::at(
                        at,
                        ErrorKind::Unsupported("an external reference (<…>)"),
                    ));
                }
                _ => {}
            }
        }
    }
}
