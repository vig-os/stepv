//! `stepv graph`: the graph core on the command line (ADR-0001 §2.4, G5).
//! The kernel isn't involved: every subcommand reads the file through
//! [`stepv::graph`] alone and prints one JSON value on stdout.

use std::io::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::Value;
use stepv::graph::Graph;
use stepv::graph::json::{self, TreeOptions, View};

/// `stepv graph --help`.
pub const USAGE: &str = "\
stepv graph — the file's own graph, without the kernel

USAGE:
    stepv graph summary <input> [--timings]
    stepv graph census <input> [--timings]
    stepv graph tree <input> [--depth <n>] [--offset <n>] [--limit <n>] [--timings]
    stepv graph query <filter> <input> [--limit <n>] [--timings]

SUBCOMMANDS:
    summary   The header, the units, the census's totals, the structure's
              size and the model's box
    census    Entity instances per type and per part type
    tree      One page of the occurrence tree, depth-first: rows of depth,
              path, label, leaf number and placement in the model; \"next\"
              is the next page's --offset, or null
    query     A jq filter (jaq). `.` is null; the view is the filters
              summary, census, products, tree, entity($id), refs($id) and
              referrers($id), $id being a step id (the n of #n). Nothing
              bounds a filter's work yet (#187): deep recursion aborts the
              process, and collecting a walk over a reference cycle never
              ends

OPTIONS:
    --depth <n>    tree: list occurrences down to depth n (roots are 0)
    --offset <n>   tree: skip n listed occurrences (default 0)
    --limit <n>    tree: rows per page; query: outputs kept (default 1000,
                   at least 1)
    --timings      Each phase's time on stderr

OUTPUT:
    One JSON value on stdout. Text from the file (names, header fields,
    type names) is always {\"untrusted\": \"…\"} in summary, census and
    tree; query emits it so too, unless the filter unwraps it on purpose.
    Boxes are conservative, in mm, with \"tier\": \"proxy\".

EXIT CODES:
    0  success
    2  usage error, or a filter that doesn't parse or compile
    3  the file has no graph or structure, the tree passed a cap, or the
       filter failed
";

const EXIT_USAGE: u8 = 2;
const EXIT_FAILED: u8 = 3;

#[derive(Debug)]
struct Args {
    /// `summary`, `census`, `tree` or `query`.
    command: &'static str,
    input: PathBuf,
    filter: String,
    tree: TreeOptions,
    limit: u64,
    timings: bool,
}

fn parse(argv: &[String]) -> Result<Args, String> {
    let (command, rest) = argv.split_first().ok_or("give a subcommand")?;
    let command = ["summary", "census", "tree", "query"]
        .into_iter()
        .find(|c| c == command)
        .ok_or(format!("unknown subcommand {command:?}"))?;
    let mut a = Args {
        command,
        input: PathBuf::new(),
        filter: String::new(),
        tree: TreeOptions::default(),
        limit: 1000,
        timings: false,
    };
    let mut positional = Vec::new();
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let mut number = |flag: &str| -> Result<u64, String> {
            it.next()
                .and_then(|v| v.parse().ok())
                .ok_or(format!("{flag} needs a number"))
        };
        match arg.as_str() {
            "--depth" if command == "tree" => {
                a.tree.depth = u32::try_from(number(arg)?).map_err(|_| "--depth is too large")?;
            }
            "--offset" if command == "tree" => a.tree.offset = number(arg)?,
            "--limit" if matches!(command, "tree" | "query") => {
                a.limit = number(arg)?;
            }
            "--timings" => a.timings = true,
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            _ => positional.push(arg.clone()),
        }
    }
    let (want, what) = match command {
        "query" => (2, "a filter and an input file"),
        _ => (1, "one input file"),
    };
    if positional.len() != want {
        return Err(format!("graph {command}: give {what}"));
    }
    a.input = PathBuf::from(positional.pop().expect("checked"));
    a.filter = positional.pop().unwrap_or_default();
    if a.limit == 0 {
        return Err("--limit must be at least 1".into());
    }
    a.tree.limit = a.limit;
    Ok(a)
}

/// Runs `stepv graph …` (`argv` after `graph`) and returns the exit code.
pub fn main(argv: &[String]) -> u8 {
    if argv.is_empty() || argv.iter().any(|a| a == "-h" || a == "--help") {
        print!("{USAGE}");
        return 0;
    }
    let a = match parse(argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("stepv: {e}\nRun `stepv graph --help` for usage.");
            return EXIT_USAGE;
        }
    };
    let start = Instant::now();
    let mut phases: Vec<(&'static str, Duration)> = Vec::new();
    let g = match Graph::open(&a.input) {
        Ok(g) => g,
        Err(e) => return fail(EXIT_FAILED, &format!("no graph: {e}")),
    };
    phases.push(("scan", start.elapsed()));
    let view = View::new(&g);
    let t = Instant::now();
    let out = answer(&a, &view);
    let total = t.elapsed();
    let ran: Vec<_> = view.timings();
    let inner: Duration = ran.iter().map(|(_, d)| *d).sum();
    phases.extend(ran);
    phases.push(("answer", total.saturating_sub(inner)));
    let code = match out {
        Ok(v) => {
            let mut stdout = std::io::stdout().lock();
            match serde_json::to_writer(&mut stdout, &v).map(|()| writeln!(stdout)) {
                Ok(Ok(())) => 0,
                Ok(Err(e)) => fail(EXIT_FAILED, &format!("cannot write stdout: {e}")),
                Err(e) => fail(EXIT_FAILED, &format!("cannot write stdout: {e}")),
            }
        }
        Err((code, e)) => fail(code, &e),
    };
    if a.timings {
        let list: Vec<String> = phases
            .iter()
            .map(|(p, d)| format!("{p} {:.3} s", d.as_secs_f64()))
            .collect();
        eprintln!(
            "stepv graph: timings: {}, total {:.3} s",
            list.join(", "),
            start.elapsed().as_secs_f64()
        );
    }
    code
}

fn answer(a: &Args, view: &View<'_>) -> Result<Value, (u8, String)> {
    let failed = |e: String| (EXIT_FAILED, e);
    match a.command {
        "summary" => Ok(view.summary()),
        "census" => Ok(json::census(&view.graph().census())),
        "tree" => view.tree(a.tree).map_err(|e| failed(e.to_string())),
        _ => query(a, view),
    }
}

#[cfg(feature = "query")]
fn query(a: &Args, view: &View<'_>) -> Result<Value, (u8, String)> {
    use stepv::graph::query::{QueryError, query};
    let limit = usize::try_from(a.limit).unwrap_or(usize::MAX);
    match query(view, &a.filter, limit) {
        Ok(r) => Ok(serde_json::json!({ "results": r.results, "more": r.more })),
        Err(e @ QueryError::Filter(_)) => Err((EXIT_USAGE, e.to_string())),
        Err(e @ QueryError::Runtime(_)) => Err((EXIT_FAILED, e.to_string())),
    }
}

#[cfg(not(feature = "query"))]
fn query(_: &Args, _: &View<'_>) -> Result<Value, (u8, String)> {
    Err((EXIT_USAGE, "built without the `query` feature".into()))
}

fn fail(code: u8, e: &str) -> u8 {
    eprintln!("stepv graph: {e}");
    code
}
