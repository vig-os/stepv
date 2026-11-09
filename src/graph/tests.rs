use std::path::{Path, PathBuf};
use std::time::Duration;

use regex::bytes::Regex;

use super::*;

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

fn step_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            step_files(&p, out);
        } else if matches!(p.extension().and_then(|e| e.to_str()), Some("step" | "stp")) {
            out.push(p);
        }
    }
}

fn graph(text: &str) -> Result<Graph, GraphError> {
    Graph::from_bytes(text.as_bytes().to_vec())
}

/// A minimal file around `data`.
fn file(data: &str) -> String {
    format!(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('t','',(''),(''),'','','');\n\
         FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA;\n{data}\nENDSEC;\nEND-ISO-10303-21;\n"
    )
}

/// `(id, type)` per record, a complex type as its sorted parts.
type Row = (u32, Vec<String>);

fn rows(g: &Graph) -> Vec<Row> {
    g.nodes()
        .map(|n| {
            let t = match g.type_name(n) {
                TypeName::Simple(s) => vec![s.to_owned()],
                TypeName::Complex(p) => std::iter::once("(".to_owned())
                    .chain(p.iter().map(|s| s.to_string()))
                    .collect(),
            };
            (g.id(n).0, t)
        })
        .collect()
}

/// The independent oracle: ids and types from regexes over the DATA
/// section, one record per `…;` line end, as every file in tests/data is
/// written.
fn oracle(bytes: &[u8]) -> Vec<Row> {
    let data = Regex::new(r"(?s)\nDATA;(.*?)\nENDSEC;").unwrap();
    let record = Regex::new(r"(?s)#(\d+)\s*=\s*(.*?);\s*(?:\n|$)").unwrap();
    let simple = Regex::new(r"^([A-Z_][A-Z0-9_]*)\s*\(").unwrap();
    let part = Regex::new(r"(?:^\(|\))\s*([A-Z_][A-Z0-9_]*)\s*\(").unwrap();
    let s = |b: &[u8]| String::from_utf8(b.to_vec()).unwrap();
    let mut out = Vec::new();
    for section in data.captures_iter(bytes) {
        for r in record.captures_iter(&section[1]) {
            let id = s(&r[1]).parse().unwrap();
            let body = &r[2];
            let t = if body.starts_with(b"(") {
                let mut parts: Vec<String> = part.captures_iter(body).map(|c| s(&c[1])).collect();
                parts.sort();
                std::iter::once("(".to_owned()).chain(parts).collect()
            } else {
                vec![s(&simple.captures(body).unwrap()[1])]
            };
            out.push((id, t));
        }
    }
    out
}

#[test]
fn table_equals_a_regex_oracle_on_tests_data() {
    let mut files = Vec::new();
    step_files(&data_dir(), &mut files);
    assert!(files.len() >= 6, "tests/data: {files:?}");
    for f in files {
        let g = Graph::open(&f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        let want = oracle(&std::fs::read(&f).unwrap());
        assert!(!want.is_empty(), "{}", f.display());
        assert_eq!(rows(&g), want, "{}", f.display());
        assert_eq!(g.duplicate_ids(), 0);
        for n in 0..g.len() as u32 {
            assert_eq!(g.node(g.id(Node(n))), Some(Node(n)));
            let a = g.arg_bytes(Node(n));
            assert!(a.starts_with(b"(") && a.ends_with(b")"), "{}", f.display());
        }
        assert_eq!(g.header().schema, crate::header::read(&f).schema);
    }
}

#[test]
fn complex_types_are_interned_as_sorted_parts() {
    let g = graph(&file(
        "#1=(NAMED_UNIT(*) LENGTH_UNIT() SI_UNIT(.MILLI.,.METRE.));\n\
         #2 = ( LENGTH_UNIT ( ) SI_UNIT($,.METRE.) /* c */ NAMED_UNIT(*) );\n\
         #3=CARTESIAN_POINT('',(0.,0.,0.));",
    ))
    .unwrap();
    let parts = ["LENGTH_UNIT", "NAMED_UNIT", "SI_UNIT"];
    for n in [0, 1] {
        let TypeName::Complex(p) = g.type_name(Node(n)) else {
            panic!()
        };
        assert!(p.iter().map(|s| &**s).eq(parts));
    }
    assert_eq!(g.type_of[0], g.type_of[1]);
    assert_eq!(g.types.len(), 2);
    assert_eq!(
        g.arg_bytes(Node(0)),
        b"(NAMED_UNIT(*) LENGTH_UNIT() SI_UNIT(.MILLI.,.METRE.))"
    );
    assert_eq!(g.type_name(Node(2)), TypeName::Simple("CARTESIAN_POINT"));
}

#[test]
fn strings_comments_and_binaries_do_not_end_a_record() {
    let g = graph(&file(
        "#1 = PRODUCT('a;b)c','it''s /* not a comment',\"0FF\",(#2));\n\
         /* a comment; with ) */ #2\n=\nPRODUCT_CONTEXT(''';',#3,'x');",
    ))
    .unwrap();
    assert_eq!(
        rows(&g),
        [
            (1, vec!["PRODUCT".into()]),
            (2, vec!["PRODUCT_CONTEXT".into()])
        ]
    );
    assert_eq!(g.arg_bytes(Node(1)), b"(''';',#3,'x')");
    assert_eq!(g.header().product_count, Some(1));
}

#[test]
fn lookup_is_dense_or_sorted() {
    let g = graph(&file("#3=A();#1=B();#2=C();")).unwrap();
    assert!(matches!(g.lookup, Lookup::Dense(_)));
    assert_eq!(g.node(StepId(1)), Some(Node(1)));
    assert_eq!(g.node(StepId(0)), None);
    assert_eq!(g.node(StepId(9)), None);
    let g = graph(&file("#4000000000=A();#7=B();#100=C();")).unwrap();
    assert!(matches!(g.lookup, Lookup::Sorted(_)));
    assert_eq!(g.node(StepId(4_000_000_000)), Some(Node(0)));
    assert_eq!(g.node(StepId(100)), Some(Node(2)));
    assert_eq!(g.node(StepId(8)), None);
}

#[test]
fn edition_3_sections() {
    let g = graph(
        "ISO-10303-21;HEADER;FILE_NAME('t','',(''),(''),'','','');FILE_SCHEMA(('X'));ENDSEC;\n\
         ANCHOR;<a> = #1; <b> = 'x;y';ENDSEC;\n\
         DATA('one',('AP242'));#1=A(#2);ENDSEC;\n\
         DATA;#2=B('');ENDSEC;\n\
         END-ISO-10303-21;\nSIGNATURE;garbage ) ( ' ENDSEC;",
    )
    .unwrap();
    assert_eq!(g.len(), 2);
    assert_eq!(g.node(StepId(2)), Some(Node(1)));
    let params: Vec<_> = g.data_parameters().collect();
    assert_eq!(params, [Some(&b"('one',('AP242'))"[..]), None]);
}

#[test]
fn refusals_name_the_byte_offset() {
    let head = file("");
    let base = head.find("DATA;\n").unwrap() + 6;
    let at = |data: &str, needle: &str| (file(data), base + data.find(needle).unwrap());
    let end = |data: &str| {
        let f = file(data);
        let cut = f.find("\nENDSEC;\nEND").unwrap() + 1;
        (f[..cut].to_owned(), cut)
    };
    let cases: Vec<((String, usize), ErrorKind)> = vec![
        (at("#1=A('abc);", "'abc"), ErrorKind::UnterminatedString),
        (at("#1=A(1,2;#2=B();", "#1"), ErrorKind::UnterminatedRecord),
        (
            at("#1=A(1) /* open", "/* open"),
            ErrorKind::UnterminatedComment,
        ),
        (
            at("#1=A(@3);", "@3"),
            ErrorKind::Unsupported("a value instance (@n)"),
        ),
        (
            at("#1=A(<x>);", "<x>"),
            ErrorKind::Unsupported("an external reference (<…>)"),
        ),
        (at("#99999999999=A();", "#9"), ErrorKind::IdOverflow),
        (
            at("#1=A();garbage", "garbage"),
            ErrorKind::Syntax("expected an entity instance or ENDSEC"),
        ),
        (
            at("#=A();", "=A"),
            ErrorKind::Syntax("expected an entity id after '#'"),
        ),
        (
            at("#1=();", ")"),
            ErrorKind::Syntax("expected an entity type"),
        ),
        (end("#1=A();"), ErrorKind::MissingEndsec),
        (
            (file("").replace("END-ISO-10303-21;\n", ""), head.len() - 18),
            ErrorKind::MissingEnd,
        ),
        (
            (file("").replace("DATA;", "REFERENCE;"), base - 6),
            ErrorKind::Unsupported("a REFERENCE section"),
        ),
        (("PNG".into(), 0), ErrorKind::NotPart21),
    ];
    for ((text, offset), kind) in cases {
        let e = graph(&text)
            .err()
            .unwrap_or_else(|| panic!("{text:?} scanned"));
        assert_eq!(e, GraphError::at(offset, kind), "{text:?}");
        assert!(e.to_string().starts_with(&format!("byte {offset}: ")));
    }
}

/// `scripts/fetch-fixtures.py`'s malformed set, rebuilt here from
/// tests/data so the test needs no fetched corpus: every case is refused
/// with an offset inside the file, and nothing panics. `header-only.step`
/// is valid Part 21 with an empty DATA section, which is not an ADR §2.1
/// refusal: its graph is empty.
#[test]
fn malformed_inputs_refuse_without_panicking() {
    let real = std::fs::read(data_dir().join("assembly.step")).unwrap();
    let mut rng = 0x57E9_u64;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng
    };
    let mut random = vec![0u8; 65536];
    random.iter_mut().for_each(|b| *b = next() as u8);
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend_from_slice(&random[..4096]);
    let start = real.windows(5).position(|w| w == b"DATA;").unwrap() + 5;
    let mut cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("whitespace", b"   \n\n\t\n".to_vec()),
        ("not-step-png", png),
        ("random-bytes", random),
        ("not-iges", b"this is not an IGES file\n".repeat(10)),
        (
            "not-brep",
            b"DBRep_DrawableShape\n\nCASCADE Topology V1, (c) Matra-Datavision\n".to_vec(),
        ),
        ("truncated-half", real[..real.len() / 2].to_vec()),
        ("truncated-header", real[..200].to_vec()),
    ];
    // Bit flips inside DATA: each of 200 seeds flips one printable byte, so
    // every flip is tried on its own as well as all together.
    let mut all = real.clone();
    for _ in 0..200 {
        let i = start + next() as usize % (real.len() - start);
        all[i] = 32 + (next() % 95) as u8;
    }
    cases.push(("bitflipped", all));
    for (name, bytes) in cases {
        let len = bytes.len() as u64;
        let e = Graph::from_bytes(bytes)
            .err()
            .unwrap_or_else(|| panic!("{name} scanned"));
        assert!(e.offset <= len, "{name}: {e}");
    }
    let g = graph(&file("")).unwrap();
    assert!(g.is_empty());
}

/// The fetched malformed set itself, when `just fixtures malformed` has run.
#[test]
fn fetched_malformed_fixtures_refuse() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/malformed");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("skipped: no {} (`just fixtures malformed`)", dir.display());
        return;
    };
    for p in entries.map(|e| e.unwrap().path()) {
        let r = Graph::open(&p);
        if p.file_name().unwrap() == "header-only.step" {
            assert!(r.unwrap().is_empty());
        } else {
            let e = r.err().unwrap_or_else(|| panic!("{} scanned", p.display()));
            assert!(
                e.offset <= std::fs::metadata(&p).unwrap().len(),
                "{}: {e}",
                p.display()
            );
        }
    }
}

#[test]
fn resolve_picks_the_nearest_definition_before_else_after() {
    // #5 is defined at nodes 1 and 3.
    let g = graph(&file("#1=R(#5);#5=A();#2=R(#5);#5=B();#3=R(#5);")).unwrap();
    assert_eq!(g.duplicate_ids(), 1);
    assert_eq!(g.node(StepId(5)), Some(Node(1)));
    assert_eq!(g.resolve(StepId(5), Node(0)), Some(Node(1)));
    assert_eq!(g.resolve(StepId(5), Node(2)), Some(Node(1)));
    assert_eq!(g.resolve(StepId(5), Node(4)), Some(Node(3)));
    // A duplicate's own record names the other definition.
    assert_eq!(g.resolve(StepId(5), Node(1)), Some(Node(3)));
    assert_eq!(g.resolve(StepId(1), Node(4)), Some(Node(0)));
    let sparse = graph(&file("#9000000=A();#5=X();#5=Y();#2=R(#5);")).unwrap();
    assert!(matches!(sparse.lookup, Lookup::Sorted(_)));
    assert_eq!(sparse.duplicate_ids(), 1);
    assert_eq!(sparse.resolve(StepId(5), Node(3)), Some(Node(2)));
}

/// ADR §2.1's duplicate-id fixture, against OCCT: `#7 = PRODUCT(…)` is
/// defined twice, with the record that references it (`#6`) before, between
/// or after the two. The outline's product name is the definition OCCT's
/// reader resolved; the graph's [`Graph::resolve`] must name the same one.
#[test]
fn duplicate_ids_resolve_as_occt_does() {
    let kernel = crate::occt::kernel_path();
    assert!(
        kernel.is_file(),
        "kernel not found at {}: run `just kernel`",
        kernel.display()
    );
    let sketch = std::fs::read_to_string(data_dir().join("sketch.step")).unwrap();
    let product = Regex::new(r"(?s)#7 = PRODUCT\(.*?;\n").unwrap();
    let base = product.replace(sketch.as_bytes(), &b""[..]);
    let base = std::str::from_utf8(&base).unwrap();
    let p = |n: &str| format!("#7 = PRODUCT('{n}','{n}','',(#8));\n");
    let dir = std::env::temp_dir().join(format!("stepv-graph-dup-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, text, occt_name) in [
        (
            "between",
            base.replacen("#6 = ", &(p("A") + "#6 = "), 1).replacen(
                "#8 = ",
                &(p("B") + "#8 = "),
                1,
            ),
            "A",
        ),
        (
            "after",
            base.replacen("#8 = ", &(p("A") + &p("B") + "#8 = "), 1),
            "A",
        ),
        (
            "before",
            base.replacen("#6 = ", &(p("A") + &p("B") + "#6 = "), 1),
            "B",
        ),
    ] {
        let path = dir.join(format!("{name}.step"));
        std::fs::write(&path, &text).unwrap();
        let limits = crate::occt::Limits {
            timeout: Duration::from_secs(60),
            memory: Some(4 << 30),
        };
        let outline = crate::occt::outline(&kernel, &path, limits).unwrap();
        let crate::topology::Node::Part { name: shown, .. } = &outline.tree[0] else {
            panic!("{name}: {outline:?}")
        };
        assert_eq!(shown, occt_name, "{name}: OCCT's reader changed its rule");
        let g = Graph::open(&path).unwrap();
        assert_eq!(g.duplicate_ids(), 1);
        let referrer = g.node(StepId(6)).unwrap();
        let def = g.resolve(StepId(7), referrer).unwrap();
        // The forward CSR resolves the reference the same way (#149).
        assert_eq!(
            g.refs(referrer)
                .iter()
                .filter(|&&r| g.id(r) == StepId(7))
                .collect::<Vec<_>>(),
            [&def],
            "{name}"
        );
        let args = String::from_utf8_lossy(g.arg_bytes(def)).into_owned();
        assert!(
            args.starts_with(&format!("('{occt_name}'")),
            "{name}: {args}"
        );
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The `#n` ids in a parameter span, by regex, outside strings, binaries
/// and comments: the round trip's oracle.
fn span_ids(span: &[u8]) -> Vec<u32> {
    let quoted = Regex::new(r#"(?s)'(?:[^']|'')*'|"[^"]*"|/\*.*?\*/"#).unwrap();
    let id = Regex::new(r"#(\d+)").unwrap();
    let bare = quoted.replace_all(span, &b""[..]);
    id.captures_iter(&bare)
        .map(|c| std::str::from_utf8(&c[1]).unwrap().parse().unwrap())
        .collect()
}

/// Every reference in `v`, depth first, in order.
fn value_refs(v: &[Value], out: &mut Vec<Node>) {
    for v in v {
        match v {
            Value::Ref(n) => out.push(*n),
            Value::List(l) => value_refs(l, out),
            Value::Typed(_, v) => value_refs(std::slice::from_ref(&**v), out),
            _ => {}
        }
    }
}

#[test]
fn refs_round_trip_and_args_parse_on_tests_data() {
    let mut files = Vec::new();
    step_files(&data_dir(), &mut files);
    assert!(!files.is_empty());
    let (mut simple, mut parts) = (0, 0);
    for f in files {
        let g = Graph::open(&f).unwrap();
        for n in g.nodes() {
            let ids: Vec<u32> = g.refs(n).iter().map(|&r| g.id(r).0).collect();
            assert_eq!(
                ids,
                span_ids(g.arg_bytes(n)),
                "{}: {:?}",
                f.display(),
                g.id(n)
            );
            let mut from_args = Vec::new();
            if let TypeName::Simple(_) = g.type_name(n) {
                value_refs(&g.args(n).unwrap(), &mut from_args);
                simple += 1;
            } else {
                for p in g.parts(n) {
                    value_refs(&g.args_of(n, p).unwrap().unwrap(), &mut from_args);
                    parts += 1;
                }
                // Parts come sorted, references in record order.
                from_args.sort_unstable();
            }
            let mut want = g.refs(n).to_vec();
            if matches!(g.type_name(n), TypeName::Complex(_)) {
                want.sort_unstable();
            }
            assert_eq!(from_args, want, "{}: {:?}", f.display(), g.id(n));
        }
    }
    assert!(
        simple > 1000 && parts > 10,
        "{simple} entities, {parts} parts"
    );
}

#[test]
fn csr_resolves_duplicated_ids_per_reference() {
    // #5 is defined at nodes 1 and 3 (#149).
    let g = graph(&file("#1=R(#5);#5=A();#2=R(#5,#1);#5=B();#3=R(#5);")).unwrap();
    assert_eq!(g.refs(Node(0)), [Node(1)]);
    assert_eq!(g.refs(Node(2)), [Node(1), Node(0)]);
    assert_eq!(g.refs(Node(4)), [Node(3)]);
    assert_eq!(g.args(Node(4)).unwrap()[0], Value::Ref(Node(3)));
    assert_eq!(g.referrers(Node(1)), [Node(0), Node(2)]);
    assert_eq!(g.referrers(Node(3)), [Node(4)]);
}

#[test]
fn dangling_references_are_listed_not_fatal() {
    let g = graph(&file("#1=R(#9,#2);#2=A('#7');")).unwrap();
    assert_eq!(g.refs(Node(0)), [Node(1)]);
    assert_eq!(g.refs(Node(1)), []);
    assert_eq!(g.dangling(), [(Node(0), StepId(9))]);
    assert!(matches!(
        g.args(Node(0)),
        Err(ArgError::Dangling { id: StepId(9), .. })
    ));
    assert_eq!(g.census().dangling, 1);
}

#[test]
fn referrers_are_the_reverse_csr() {
    let g = graph(&file("#1=A();#2=R(#1,#1,#3);#3=R(#1);#4=R((#3),#2);")).unwrap();
    assert_eq!(g.referrers(Node(0)), [Node(1), Node(1), Node(2)]);
    assert_eq!(g.referrers(Node(1)), [Node(3)]);
    assert_eq!(g.referrers(Node(2)), [Node(1), Node(3)]);
    assert_eq!(g.referrers(Node(3)), []);
}

#[test]
fn args_parse_every_kind_of_value() {
    let g = graph(&file(
        "#1=X(#2, 3 ,-4.5E-1,'it''s',.T.,\"0F\",(1,(2.)),LENGTH_MEASURE (25.4),$,*,(),/* c */1.);#2=Y();",
    ))
    .unwrap();
    let a = g.args(Node(0)).unwrap();
    let Value::Str(s) = &a[3] else { panic!() };
    assert_eq!(s.decode(), "it's");
    assert_eq!(format!("{s:?}"), "Untrusted(5 bytes)");
    assert_eq!(
        a[..],
        [
            Value::Ref(Node(1)),
            Value::Int(3),
            Value::Real(-0.45),
            a[3].clone(),
            Value::Enum("T"),
            Value::Binary(b"0F"),
            Value::List(vec![Value::Int(1), Value::List(vec![Value::Real(2.)])]),
            Value::Typed("LENGTH_MEASURE", Box::new(Value::Real(25.4))),
            Value::Null,
            Value::Derived,
            Value::List(vec![]),
            Value::Real(1.),
        ]
    );
    assert_eq!(g.args(Node(1)).unwrap()[..], []);
}

#[test]
fn args_nesting_is_capped() {
    let nest = |k: usize| format!("#1=X({}1{});", "(".repeat(k - 1), ")".repeat(k - 1));
    let g = graph(&file(&nest(MAX_DEPTH))).unwrap();
    assert!(g.args(Node(0)).is_ok());
    let g = graph(&file(&nest(MAX_DEPTH + 1))).unwrap();
    assert!(matches!(g.args(Node(0)), Err(ArgError::Depth { .. })));
    // Far deeper: an error, not a stack overflow.
    let g = graph(&file(&nest(100_000))).unwrap();
    assert!(matches!(g.args(Node(0)), Err(ArgError::Depth { .. })));
}

#[test]
fn args_errors_name_the_byte_offset() {
    let text = file("#1=X(1 2);#2=X(.T);#3=X(99999999999999999999);#4=X(A(1,2));");
    let g = graph(&text).unwrap();
    for (n, at) in [(0, "2);#2"), (1, ".T);"), (2, "999"), (3, "A(1,2)")] {
        let Err(ArgError::Syntax { offset, .. }) = g.args(Node(n)) else {
            panic!("{n}: {:?}", g.args(Node(n)))
        };
        assert_eq!(offset, text.find(at).unwrap() as u64, "{n}");
    }
}

/// The complex-entity cases ADR §2.4 says the MVP relies on.
const WALKTHROUGHS: &str = "\
#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));
#3=(NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT());
#4=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-07),#1,'distance_accuracy_value','');
#5=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#4)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#2,#3)) REPRESENTATION_CONTEXT('',''));
#6=DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.);
#7=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#1);
#8=(CONVERSION_BASED_UNIT('INCH',#7) LENGTH_UNIT() NAMED_UNIT(#6));
#10=SHAPE_REPRESENTATION('r1',(),#5);
#11=SHAPE_REPRESENTATION('r2',(),#5);
#12=ITEM_DEFINED_TRANSFORMATION('','',#13,#13);
#13=AXIS2_PLACEMENT_3D('',#14,$,$);
#14=CARTESIAN_POINT('',(0.,0.,0.));
#15=(REPRESENTATION_RELATIONSHIP('','',#10,#11) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#12) SHAPE_REPRESENTATION_RELATIONSHIP());
#20=(BOUNDED_SURFACE() B_SPLINE_SURFACE(1,1,((#14,#14),(#14,#14)),.UNSPECIFIED.,.F.,.F.,.F.) B_SPLINE_SURFACE_WITH_KNOTS((2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.) GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_SURFACE(((1.,0.5),(0.5,1.))) REPRESENTATION_ITEM('') SURFACE());
#21=(BOUNDED_CURVE() B_SPLINE_CURVE(1,(#14,#14),.UNSPECIFIED.,.F.,.F.) B_SPLINE_CURVE_WITH_KNOTS((2,2),(0.,1.),.UNSPECIFIED.) CURVE() GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_CURVE((1.,2.)) REPRESENTATION_ITEM(''));
";

fn part<'g>(g: &'g Graph, id: u32, part: &str) -> Args<'g> {
    g.args_of(g.node(StepId(id)).unwrap(), part)
        .unwrap()
        .unwrap()
}

fn node(g: &Graph, id: u32) -> Value<'static> {
    Value::Ref(g.node(StepId(id)).unwrap())
}

#[test]
fn walkthrough_units() {
    let g = graph(&file(WALKTHROUGHS)).unwrap();
    let mm = g.node(StepId(1)).unwrap();
    assert!(g.is_a(mm, "SI_UNIT") && g.is_a(mm, "LENGTH_UNIT") && !g.is_a(mm, "SI"));
    assert_eq!(
        g.parts(mm).collect::<Vec<_>>(),
        ["LENGTH_UNIT", "NAMED_UNIT", "SI_UNIT"]
    );
    assert_eq!(g.args(mm), Err(ArgError::Complex));
    assert_eq!(
        part(&g, 1, "SI_UNIT")[..],
        [Value::Enum("MILLI"), Value::Enum("METRE")]
    );
    assert_eq!(part(&g, 1, "NAMED_UNIT")[..], [Value::Derived]);
    assert_eq!(part(&g, 1, "LENGTH_UNIT")[..], []);
    assert!(g.args_of(mm, "CONVERSION_BASED_UNIT").is_none());
    // of_type finds complex units by part.
    let si: Vec<u32> = g.of_type("SI_UNIT").map(|n| g.id(n).0).collect();
    assert_eq!(si, [1, 2, 3]);
    // The inch: the factor through CONVERSION_BASED_UNIT's second attribute.
    let inch = part(&g, 8, "CONVERSION_BASED_UNIT");
    let Value::Str(name) = &inch[0] else { panic!() };
    assert_eq!(name.decode(), "INCH");
    assert_eq!(inch[1], node(&g, 7));
    let Value::Ref(m) = inch[1] else { panic!() };
    assert_eq!(
        g.args(m).unwrap()[..],
        [
            Value::Typed("LENGTH_MEASURE", Box::new(Value::Real(25.4))),
            node(&g, 1)
        ]
    );
    assert_eq!(part(&g, 8, "NAMED_UNIT")[..], [node(&g, 6)]);
}

#[test]
fn walkthrough_context() {
    let g = graph(&file(WALKTHROUGHS)).unwrap();
    let units = part(&g, 5, "GLOBAL_UNIT_ASSIGNED_CONTEXT");
    assert_eq!(
        units[..],
        [Value::List(vec![node(&g, 1), node(&g, 2), node(&g, 3)])]
    );
    let unc = part(&g, 5, "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT");
    assert_eq!(unc[..], [Value::List(vec![node(&g, 4)])]);
    assert_eq!(
        part(&g, 5, "GEOMETRIC_REPRESENTATION_CONTEXT")[..],
        [Value::Int(3)]
    );
    let ctx = g.node(StepId(5)).unwrap();
    // refs: every part's references, in record order.
    let ids: Vec<u32> = g.refs(ctx).iter().map(|&r| g.id(r).0).collect();
    assert_eq!(ids, [4, 1, 2, 3]);
    assert!(g.of_type("REPRESENTATION_CONTEXT").any(|n| n == ctx));
}

#[test]
fn walkthrough_relationship() {
    let g = graph(&file(WALKTHROUGHS)).unwrap();
    let rr = part(&g, 15, "REPRESENTATION_RELATIONSHIP");
    assert_eq!(rr[2..], [node(&g, 10), node(&g, 11)]);
    let t = part(&g, 15, "REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION");
    assert_eq!(t[..], [node(&g, 12)]);
    assert_eq!(part(&g, 15, "SHAPE_REPRESENTATION_RELATIONSHIP")[..], []);
    let rel = g.node(StepId(15)).unwrap();
    assert_eq!(g.referrers(g.node(StepId(10)).unwrap()), [rel]);
    assert_eq!(g.referrers(g.node(StepId(12)).unwrap()), [rel]);
}

#[test]
fn walkthrough_rational_b_splines() {
    let g = graph(&file(WALKTHROUGHS)).unwrap();
    let p = node(&g, 14);
    let bs = part(&g, 20, "B_SPLINE_SURFACE");
    assert_eq!(bs[..2], [Value::Int(1), Value::Int(1)]);
    assert_eq!(
        bs[2],
        Value::List(vec![
            Value::List(vec![p.clone(), p.clone()]),
            Value::List(vec![p.clone(), p.clone()])
        ])
    );
    let knots = part(&g, 20, "B_SPLINE_SURFACE_WITH_KNOTS");
    assert_eq!(
        knots[2],
        Value::List(vec![Value::Real(0.), Value::Real(1.)])
    );
    let w = part(&g, 20, "RATIONAL_B_SPLINE_SURFACE");
    let row = |a: f64, b: f64| Value::List(vec![Value::Real(a), Value::Real(b)]);
    assert_eq!(w[..], [Value::List(vec![row(1., 0.5), row(0.5, 1.)])]);
    assert_eq!(part(&g, 21, "RATIONAL_B_SPLINE_CURVE")[..], [row(1., 2.)]);
    assert_eq!(
        part(&g, 21, "B_SPLINE_CURVE")[1],
        Value::List(vec![p.clone(), p])
    );
    let curves: Vec<u32> = g
        .of_type("B_SPLINE_CURVE_WITH_KNOTS")
        .map(|n| g.id(n).0)
        .collect();
    assert_eq!(curves, [21]);
}

#[test]
fn census_counts_by_type_and_by_part() {
    let c = graph(&file(WALKTHROUGHS)).unwrap().census();
    assert_eq!(
        (c.entities, c.complex, c.duplicate_ids, c.dangling),
        (16, 8, 0, 0)
    );
    assert_eq!(c.types["SHAPE_REPRESENTATION"], 2);
    assert_eq!(c.types["(LENGTH_UNIT NAMED_UNIT SI_UNIT)"], 1);
    assert_eq!(c.parts["SI_UNIT"], 3);
    assert_eq!(c.parts["NAMED_UNIT"], 4);
    assert_eq!(c.parts["SHAPE_REPRESENTATION"], 2);
    assert_eq!(c.parts.values().sum::<u64>(), 8 + 3 * 5 + 4 + 7 * 2);
}
