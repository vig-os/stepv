//! File-header metadata without the kernel: the `--info` path.
//!
//! This is the honest-degradation path (`plan.md` §4): when tessellation
//! fails, or before it finishes, the previewer still shows what the file says
//! about itself. So it is pure Rust, reads only the head of the file (plus one
//! bounded streaming pass to count products), and **cannot fail**: every field
//! is optional, and a file with no recognisable header still yields an
//! [`Info`] carrying its format guess and size.
//!
//! STEP: ISO 10303-21 `HEADER;` section — `FILE_DESCRIPTION`, `FILE_NAME`,
//! `FILE_SCHEMA` — with the standard's string escapes (`''`, `\X\HH`,
//! `\X2\…\X0\`, `\X4\…\X0\`, `\S\c`) decoded. IGES: the Start and Global
//! sections. BREP: the `CASCADE Topology` banner.

use std::io::Read;
use std::path::Path;

use serde::Serialize;

/// How many bytes of the file head are parsed for the header. Real headers
/// are a few hundred bytes; 64 KiB tolerates absurd author lists.
const HEAD_BYTES: usize = 64 * 1024;

/// What a file says about itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Info {
    /// `step`, `iges`, `brep` or `unknown`, from the content when it is
    /// recognisable and the extension otherwise.
    pub format: String,
    pub file_size: u64,
    /// `FILE_SCHEMA` entries (STEP), e.g. `AUTOMOTIVE_DESIGN` (AP214).
    pub schema: Vec<String>,
    /// The application protocol, when the schema names one we recognise.
    pub protocol: Option<String>,
    pub description: Vec<String>,
    pub name: Option<String>,
    pub timestamp: Option<String>,
    pub author: Vec<String>,
    pub organization: Vec<String>,
    pub preprocessor: Option<String>,
    pub originating_system: Option<String>,
    pub authorization: Option<String>,
    /// `PRODUCT` entities in the DATA section (STEP): the number of distinct
    /// part/assembly definitions, not instances. `None` when not counted.
    pub product_count: Option<u64>,
    /// Model units from the IGES Global section.
    pub units: Option<String>,
    /// Set when the header was absent or malformed. The other fields hold
    /// whatever was recovered before the problem.
    pub header_error: Option<String>,
}

/// `info` as a JSON object (the `info` field of the CLI report).
#[must_use]
pub fn to_json(info: &Info) -> String {
    serde_json::to_string(info).expect("Info is plain data and always serialises")
}

/// Reads `path`'s header. Never fails: I/O errors become `header_error`.
#[must_use]
pub fn read(path: &Path) -> Info {
    let mut info = Info {
        format: format_from_extension(path).to_owned(),
        ..Info::default()
    };
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            info.header_error = Some(format!("cannot open: {e}"));
            return info;
        }
    };
    info.file_size = file.metadata().map_or(0, |m| m.len());
    let mut head = Vec::with_capacity(HEAD_BYTES.min(info.file_size as usize));
    if let Err(e) = (&mut file).take(HEAD_BYTES as u64).read_to_end(&mut head) {
        info.header_error = Some(format!("cannot read: {e}"));
        return info;
    }
    parse_head(&head, &mut info);
    if info.format == "step" && info.header_error.is_none() {
        info.product_count = count_products(path);
    }
    info
}

fn format_from_extension(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("step" | "stp") => "step",
        Some("iges" | "igs") => "iges",
        Some("brep") => "brep",
        _ => "unknown",
    }
}

/// Fills `info` from the head of a file. Content wins over the extension.
pub fn parse_head(head: &[u8], info: &mut Info) {
    let trimmed = trim_start(head);
    if trimmed.starts_with(b"ISO-10303-21") {
        info.format = "step".into();
        parse_step(head, info);
    } else if head.contains_subslice(b"CASCADE Topology") {
        info.format = "brep".into();
        parse_brep(head, info);
    } else if looks_like_iges(head) {
        info.format = "iges".into();
        parse_iges(head, info);
    } else {
        info.header_error = Some(match info.format.as_str() {
            "step" => "no ISO-10303-21 header".into(),
            "iges" => "no IGES Start/Global sections".into(),
            "brep" => "no CASCADE Topology banner".into(),
            _ => "unrecognised file format".into(),
        });
    }
}

fn trim_start(b: &[u8]) -> &[u8] {
    let skip = b.iter().take_while(|c| c.is_ascii_whitespace()).count();
    &b[skip..]
}

trait ContainsSubslice {
    fn contains_subslice(&self, needle: &[u8]) -> bool;
}

impl ContainsSubslice for [u8] {
    fn contains_subslice(&self, needle: &[u8]) -> bool {
        self.windows(needle.len()).any(|w| w == needle)
    }
}

// ── STEP ────────────────────────────────────────────────────────────────────

/// A header-entity argument.
#[derive(Debug, Clone, PartialEq)]
enum Arg {
    Str(String),
    List(Vec<Arg>),
    /// `$`, `*`, numbers, enumerations: kept as text, never interpreted.
    Other(String),
}

impl Arg {
    fn as_str(&self) -> Option<String> {
        match self {
            Self::Str(s) if !s.trim().is_empty() => Some(s.trim().to_owned()),
            _ => None,
        }
    }

    fn strings(&self) -> Vec<String> {
        match self {
            Self::List(v) => v.iter().filter_map(Self::as_str).collect(),
            other => other.as_str().into_iter().collect(),
        }
    }
}

fn parse_step(head: &[u8], info: &mut Info) {
    let Some(start) = find(head, b"HEADER;") else {
        info.header_error = Some("no HEADER section".into());
        return;
    };
    let body = &head[start + 7..];
    let end = find(body, b"ENDSEC;");
    if end.is_none() {
        info.header_error = Some("HEADER section not terminated in the first 64 KiB".into());
    }
    let mut p = Parser {
        b: &body[..end.unwrap_or(body.len())],
        i: 0,
    };
    while let Some((name, args)) = p.entity() {
        match name.as_str() {
            "FILE_DESCRIPTION" => {
                info.description = args.first().map(Arg::strings).unwrap_or_default();
            }
            "FILE_NAME" => {
                let s = |i: usize| args.get(i).and_then(Arg::as_str);
                let l = |i: usize| args.get(i).map(Arg::strings).unwrap_or_default();
                info.name = s(0);
                info.timestamp = s(1);
                info.author = l(2);
                info.organization = l(3);
                info.preprocessor = s(4);
                info.originating_system = s(5);
                info.authorization = s(6);
            }
            "FILE_SCHEMA" => {
                info.schema = args.first().map(Arg::strings).unwrap_or_default();
                info.protocol = info.schema.iter().find_map(|s| protocol_of(s));
            }
            _ => {}
        }
    }
    if info.schema.is_empty() && info.name.is_none() && info.header_error.is_none() {
        info.header_error = Some("HEADER section has no FILE_NAME or FILE_SCHEMA".into());
    }
}

/// Maps a `FILE_SCHEMA` entry to the application protocol a user knows.
fn protocol_of(schema: &str) -> Option<String> {
    let s = schema.to_ascii_uppercase();
    let p = if s.starts_with("AP242") || s.contains("MANAGED_MODEL_BASED_3D_ENGINEERING") {
        "AP242"
    } else if s.starts_with("AUTOMOTIVE_DESIGN") || s.starts_with("AP214") {
        "AP214"
    } else if s.starts_with("CONFIG_CONTROL_DESIGN") || s.starts_with("AP203") {
        "AP203"
    } else if s.starts_with("AP209") || s.starts_with("STRUCTURAL_ANALYSIS_DESIGN") {
        "AP209"
    } else {
        return None;
    };
    Some(p.into())
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    /// Skips whitespace and `/* … */` comments.
    fn skip(&mut self) {
        loop {
            while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
                self.i += 1;
            }
            if self.b[self.i..].starts_with(b"/*") {
                match find(&self.b[self.i + 2..], b"*/") {
                    Some(n) => self.i += n + 4,
                    None => self.i = self.b.len(),
                }
            } else {
                return;
            }
        }
    }

    /// `NAME ( args ) ;` — returns `None` at the end or on garbage that
    /// cannot be resynchronised.
    fn entity(&mut self) -> Option<(String, Vec<Arg>)> {
        loop {
            self.skip();
            let start = self.i;
            while self
                .peek()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
            {
                self.i += 1;
            }
            let name = String::from_utf8_lossy(&self.b[start..self.i]).to_ascii_uppercase();
            self.skip();
            if name.is_empty() || self.peek() != Some(b'(') {
                // Resynchronise on the next ';', or give up at the end.
                let next = find(&self.b[self.i..], b";")?;
                self.i += next + 1;
                if self.i >= self.b.len() {
                    return None;
                }
                continue;
            }
            let args = match self.value(0) {
                Some(Arg::List(v)) => v,
                _ => Vec::new(),
            };
            self.skip();
            if self.peek() == Some(b';') {
                self.i += 1;
            }
            return Some((name, args));
        }
    }

    fn value(&mut self, depth: usize) -> Option<Arg> {
        self.skip();
        match self.peek()? {
            b'(' if depth < 32 => {
                self.i += 1;
                let mut items = Vec::new();
                loop {
                    self.skip();
                    match self.peek()? {
                        b')' => {
                            self.i += 1;
                            return Some(Arg::List(items));
                        }
                        b',' => self.i += 1,
                        _ => items.push(self.value(depth + 1)?),
                    }
                }
            }
            b'\'' => self.string().map(Arg::Str),
            _ => {
                let start = self.i;
                while self
                    .peek()
                    .is_some_and(|c| !matches!(c, b',' | b')' | b'(' | b';' | b'\''))
                {
                    self.i += 1;
                }
                if self.i == start {
                    self.i += 1; // never stall on an unexpected byte
                }
                Some(Arg::Other(
                    String::from_utf8_lossy(&self.b[start..self.i])
                        .trim()
                        .to_owned(),
                ))
            }
        }
    }

    /// A Part 21 string, opening quote at `self.i`.
    fn string(&mut self) -> Option<String> {
        self.i += 1;
        let mut raw = Vec::new();
        loop {
            let c = self.peek()?;
            self.i += 1;
            if c == b'\'' {
                if self.peek() == Some(b'\'') {
                    raw.push(b'\'');
                    self.i += 1;
                } else {
                    return Some(decode_part21(&raw));
                }
            } else {
                raw.push(c);
            }
        }
    }
}

/// Decodes ISO 10303-21 string escapes into UTF-8. Unknown or broken escapes
/// are kept literally: a header is metadata for a human, so showing a stray
/// backslash beats dropping the field.
#[must_use]
pub fn decode_part21(raw: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0;
    let hex =
        |s: &[u8]| -> Option<u32> { u32::from_str_radix(std::str::from_utf8(s).ok()?, 16).ok() };
    while i < raw.len() {
        let rest = &raw[i..];
        if rest.starts_with(b"\\X2\\") || rest.starts_with(b"\\X4\\") {
            let width = if rest[2] == b'2' { 4 } else { 8 };
            let body = &rest[4..];
            if let Some(end) = find(body, b"\\X0\\") {
                let units = &body[..end];
                if units.len().is_multiple_of(width) {
                    let mut ok = true;
                    let mut buf16 = Vec::new();
                    let mut decoded = String::new();
                    for chunk in units.chunks(width) {
                        match hex(chunk) {
                            Some(v) if width == 4 => buf16.push(v as u16),
                            Some(v) => decoded.push(char::from_u32(v).unwrap_or('\u{FFFD}')),
                            None => ok = false,
                        }
                    }
                    if ok {
                        if width == 4 {
                            decoded = String::from_utf16_lossy(&buf16);
                        }
                        out.push_str(&decoded);
                        i += 4 + end + 4;
                        continue;
                    }
                }
            }
        } else if rest.starts_with(b"\\X\\") && rest.len() >= 5 {
            if let Some(v) = hex(&rest[3..5]) {
                // ISO 8859-1 maps 1:1 onto the first 256 code points.
                out.push(char::from_u32(v).unwrap_or('\u{FFFD}'));
                i += 5;
                continue;
            }
        } else if rest.starts_with(b"\\S\\") && rest.len() >= 4 {
            // Upper half of ISO 8859-1: the character code plus 128.
            out.push(char::from_u32(u32::from(rest[3]) + 128).unwrap_or('\u{FFFD}'));
            i += 4;
            continue;
        } else if rest.starts_with(b"\\\\") {
            out.push('\\');
            i += 2;
            continue;
        } else if rest.starts_with(b"\\P") && rest.len() >= 4 && rest[3] == b'\\' {
            i += 4; // code-page directive: no output
            continue;
        }
        // Plain byte run up to the next backslash, decoded leniently.
        let n = rest
            .iter()
            .skip(1)
            .position(|&c| c == b'\\')
            .map_or(rest.len(), |p| p + 1);
        out.push_str(&String::from_utf8_lossy(&rest[..n]));
        i += n;
    }
    out
}

/// Counts `PRODUCT(` entity instances in the whole file, streaming in 1 MiB
/// blocks. Bounded by I/O speed (~0.5 s for 745 MB on sage); returns `None`
/// on any read error rather than a wrong number.
fn count_products(path: &Path) -> Option<u64> {
    const NEEDLE: &[u8] = b"=PRODUCT(";
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; 1 << 20];
    let mut carry: Vec<u8> = Vec::new();
    let mut count = 0u64;
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        let mut block = std::mem::take(&mut carry);
        block.extend_from_slice(&buf[..n]);
        // Entities may be written `#12 = PRODUCT (`; normalise cheaply by
        // matching on a whitespace-stripped view of each candidate.
        count += count_in(&block, NEEDLE);
        let keep = block.len().min(64);
        carry = block[block.len() - keep..].to_vec();
        // A match wholly inside the carried tail is counted again when the
        // tail leads the next block, so take it back out here...
        count -= count_in(&carry, NEEDLE);
    }
    // ...and after the last block there is no next block to count it.
    count += count_in(&carry, NEEDLE);
    Some(count)
}

/// Occurrences of `=PRODUCT(` ignoring ASCII whitespace around `=` and `(`.
fn count_in(block: &[u8], _needle: &[u8]) -> u64 {
    let mut n = 0;
    let mut i = 0;
    while let Some(p) = find(&block[i..], b"PRODUCT") {
        let at = i + p;
        let before = block[..at]
            .iter()
            .rev()
            .find(|c| !c.is_ascii_whitespace())
            .copied();
        let after = block[at + 7..]
            .iter()
            .find(|c| !c.is_ascii_whitespace())
            .copied();
        let word_start =
            at == 0 || !(block[at - 1].is_ascii_alphanumeric() || block[at - 1] == b'_');
        if word_start && before == Some(b'=') && after == Some(b'(') {
            n += 1;
        }
        i = at + 7;
    }
    n
}

// ── IGES ────────────────────────────────────────────────────────────────────

/// IGES records are 80 columns with the section letter in column 73.
fn iges_records(head: &[u8]) -> impl Iterator<Item = (&[u8], u8)> {
    head.split(|&c| c == b'\n').filter_map(|line| {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        (line.len() >= 73).then(|| (&line[..72], line[72]))
    })
}

fn looks_like_iges(head: &[u8]) -> bool {
    let mut it = iges_records(head);
    matches!(it.next(), Some((_, b'S'))) && iges_records(head).any(|(_, s)| s == b'G')
}

fn parse_iges(head: &[u8], info: &mut Info) {
    let start: Vec<String> = iges_records(head)
        .filter(|(_, s)| *s == b'S')
        .map(|(d, _)| String::from_utf8_lossy(d).trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();
    if !start.is_empty() {
        info.description = vec![start.join(" ")];
    }
    let global: Vec<u8> = iges_records(head)
        .filter(|(_, s)| *s == b'G')
        .flat_map(|(d, _)| d.iter().copied())
        .collect();
    let fields = iges_global_fields(&global);
    let f = |n: usize| fields.get(n - 1).filter(|s| !s.is_empty()).cloned();
    // Global section parameter numbers (IGES 5.3 §2.2.4.3).
    info.name = f(4);
    info.originating_system = f(5);
    info.preprocessor = f(6);
    info.units = f(15).or_else(|| match f(14).as_deref() {
        Some("1") => Some("IN".into()),
        Some("2") => Some("MM".into()),
        Some("6") => Some("M".into()),
        _ => None,
    });
    info.timestamp = f(18);
    info.author = f(21).into_iter().collect();
    info.organization = f(22).into_iter().collect();
    if fields.is_empty() {
        info.header_error = Some("empty IGES Global section".into());
    }
}

/// Splits the Global section on its own delimiters (fields 1 and 2), with
/// Hollerith strings (`5HHELLO`) decoded.
fn iges_global_fields(g: &[u8]) -> Vec<String> {
    let (mut pd, mut rd) = (b',', b';');
    let mut i = 0;
    // Field 1/2 may redefine the delimiters as `1Hx`.
    if g.starts_with(b"1H") && g.len() > 3 {
        pd = g[2];
        if g[3..].starts_with(&[pd, b'1', b'H']) && g.len() > 6 {
            rd = g[6];
        }
    }
    let mut fields = Vec::new();
    let mut cur = Vec::new();
    while i < g.len() {
        let c = g[i];
        // Record padding and spacing before a field are not part of it, and
        // would hide a Hollerith count from the check below.
        if cur.is_empty() && c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        // Hollerith: digits then 'H' then that many bytes.
        let digits = g[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        if cur.is_empty()
            && digits > 0
            && g.get(i + digits) == Some(&b'H')
            && let Ok(n) = std::str::from_utf8(&g[i..i + digits])
                .unwrap_or("x")
                .parse::<usize>()
        {
            let s = i + digits + 1;
            let e = (s + n).min(g.len());
            cur.extend_from_slice(&g[s..e]);
            i = e;
            continue;
        }
        if c == pd || c == rd {
            fields.push(String::from_utf8_lossy(&cur).trim().to_owned());
            cur.clear();
            if c == rd {
                break;
            }
        } else {
            cur.push(c);
        }
        i += 1;
    }
    fields
}

// ── BREP ────────────────────────────────────────────────────────────────────

fn parse_brep(head: &[u8], info: &mut Info) {
    if let Some(line) = head
        .split(|&c| c == b'\n')
        .find(|l| l.contains_subslice(b"CASCADE Topology"))
    {
        info.description = vec![String::from_utf8_lossy(line).trim().to_owned()];
        info.originating_system = Some("Open CASCADE".into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(header: &str) -> Info {
        let mut i = Info::default();
        parse_head(header.as_bytes(), &mut i);
        i
    }

    const HDR: &str = "ISO-10303-21;\nHEADER;\n\
        /* written by a test */\n\
        FILE_DESCRIPTION(('A part','with two lines'),'2;1');\n\
        FILE_NAME('bracket.step','2026-10-04T12:00:00',('Ada','Lin'),('Exoma'),\n  'ST-DEVELOPER v18','SolidWorks 2025','');\n\
        FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }'));\n\
        ENDSEC;\nDATA;\n#1=PRODUCT('p','p','',(#2));\nENDSEC;\nEND-ISO-10303-21;\n";

    #[test]
    fn parses_a_complete_step_header() {
        let i = step(HDR);
        assert_eq!(i.format, "step");
        assert_eq!(i.description, ["A part", "with two lines"]);
        assert_eq!(i.name.as_deref(), Some("bracket.step"));
        assert_eq!(i.timestamp.as_deref(), Some("2026-10-04T12:00:00"));
        assert_eq!(i.author, ["Ada", "Lin"]);
        assert_eq!(i.organization, ["Exoma"]);
        assert_eq!(i.preprocessor.as_deref(), Some("ST-DEVELOPER v18"));
        assert_eq!(i.originating_system.as_deref(), Some("SolidWorks 2025"));
        assert_eq!(i.authorization, None);
        assert_eq!(i.protocol.as_deref(), Some("AP242"));
        assert_eq!(i.header_error, None);
    }

    #[test]
    fn decodes_part21_escapes() {
        assert_eq!(decode_part21(b"M\\X\\FCller"), "Müller");
        assert_eq!(decode_part21(b"\\X2\\00C400D6\\X0\\-Teil"), "ÄÖ-Teil");
        assert_eq!(decode_part21(b"\\X4\\0001F600\\X0\\"), "😀");
        assert_eq!(decode_part21(b"a\\\\b"), "a\\b");
        assert_eq!(decode_part21(b"\\S\\d"), "ä");
        // Broken escapes stay literal rather than vanishing.
        assert_eq!(decode_part21(b"\\X2\\00C\\X0\\"), "\\X2\\00C\\X0\\");
    }

    #[test]
    fn doubled_quote_is_one_quote() {
        let i = step(
            "ISO-10303-21;HEADER;FILE_NAME('it''s.step','',(''),(''),'','','');FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));ENDSEC;",
        );
        assert_eq!(i.name.as_deref(), Some("it's.step"));
        assert_eq!(i.protocol.as_deref(), Some("AP203"));
    }

    #[test]
    fn garbage_never_panics_and_reports() {
        for g in [
            &b""[..],
            b"   ",
            b"ISO-10303-21;",
            b"ISO-10303-21;HEADER;FILE_NAME(((((((",
            b"ISO-10303-21;HEADER;FILE_NAME('unterminated",
            b"ISO-10303-21;HEADER;);;;(((;ENDSEC;",
            b"\x89PNG\r\n\x1a\n\0\0\0",
        ] {
            let mut i = Info::default();
            parse_head(g, &mut i);
            assert!(i.header_error.is_some(), "{:?} should report an error", g);
        }
    }

    #[test]
    fn deep_nesting_is_bounded() {
        let s = format!(
            "ISO-10303-21;HEADER;FILE_NAME({}'x'{});ENDSEC;",
            "(".repeat(5000),
            ")".repeat(5000)
        );
        let mut i = Info::default();
        parse_head(s.as_bytes(), &mut i);
        assert!(i.header_error.is_some());
    }

    #[test]
    fn counts_products_with_spacing_variants() {
        let b = b"#1=PRODUCT('a');#2 = PRODUCT ('b');#3=PRODUCT_DEFINITION('c');#4=XPRODUCT('d');";
        assert_eq!(count_in(b, b""), 2);
    }

    #[test]
    fn product_count_across_block_boundaries() {
        let dir = std::env::temp_dir().join(format!("stepv-hdr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("big.step");
        let mut body = HDR.replace("#1=PRODUCT('p','p','',(#2));\n", "");
        // ~3 MiB of DATA so matches straddle the 1 MiB block edges.
        for k in 0..60_000 {
            body.push_str(&format!("#{}=PRODUCT('p{k}','p','',(#2));\n", k + 10));
        }
        std::fs::write(&p, &body).unwrap();
        let i = read(&p);
        assert_eq!(i.product_count, Some(60_000));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn parses_an_iges_global_section() {
        let mut s = String::new();
        let rec = |data: &str, sec: char, n: usize| format!("{data:<72}{sec}{n:>7}\n");
        s += &rec("Test part exported for stepv", 'S', 1);
        s += &rec(
            "1H,,1H;,7HPART-01,11Hbracket.igs,10HSolidWorks,7HSW 2025,32,",
            'G',
            1,
        );
        s += &rec(
            "38,6,308,15,PART-01,1.,2,2HMM,1,0.01,15H20261004.120000,",
            'G',
            2,
        );
        s += &rec("0.001,100.,3HAda,5HExoma,11,0,15H20261004.120000;", 'G', 3);
        let mut i = Info::default();
        parse_head(s.as_bytes(), &mut i);
        assert_eq!(i.format, "iges");
        assert_eq!(i.description, ["Test part exported for stepv"]);
        assert_eq!(i.name.as_deref(), Some("bracket.igs"));
        assert_eq!(i.originating_system.as_deref(), Some("SolidWorks"));
        assert_eq!(i.units.as_deref(), Some("MM"));
        assert_eq!(i.author, ["Ada"]);
        assert_eq!(i.organization, ["Exoma"]);
    }

    #[test]
    fn recognises_brep() {
        let mut i = Info::default();
        parse_head(
            b"\nDBRep_DrawableShape\n\nCASCADE Topology V1, (c) Matra-Datavision\nLocations 0\n",
            &mut i,
        );
        assert_eq!(i.format, "brep");
        assert_eq!(i.originating_system.as_deref(), Some("Open CASCADE"));
    }
}
