//! The graph's corpus census (ADR-0001 §2.6, `docs/adr/0001-census.md`):
//! what share of the faces and edges the Rust fast path could take, and how
//! many shape representations have only such faces.
//!
//! ```text
//! cargo run --release --example graph_census -- PATH...
//! ```
//!
//! Each PATH is a STEP file or a directory searched for `.step`/`.stp`.
//! Per file it checks the acceptance of slice G2: every entity's arguments
//! (every part's, for a complex one) parse, and the references they hold are
//! exactly `refs`, in order. Then it counts, from the graph alone:
//! - **faces:** `ADVANCED_FACE`s whose surface is analytic (plane, cylinder,
//!   cone, sphere, torus);
//! - **edges:** `EDGE_CURVE`s whose 3-D curve, direct or through a
//!   `SURFACE_CURVE`/`SEAM_CURVE`, is an MVP curve: `LINE`, `CIRCLE`,
//!   `ELLIPSE` (analytic) or a `B_SPLINE_CURVE_WITH_KNOTS`, simple or complex.
//!   P-curves are never counted;
//! - **representations:** each `…SHAPE_REPRESENTATION` with at least one
//!   face, its faces found by reachability over `refs` without entering
//!   another representation. "In scope" means every face is analytic and
//!   bounded only by MVP edges (§2.6). Per representation until G3 gives the
//!   product → faces mapping.
//!
//! It prints a Markdown table, one row per file, and the totals, and the
//! heap beyond the map without and with `referrers` (§2.7). A refused file
//! is listed as skipped. Exits 1 when any argument or round-trip check failed.

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

use stepv::graph::{Graph, Node, TypeName, Value};

/// The system allocator, counting the bytes live.
struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call is forwarded to `System` unchanged; only counts are added.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            LIVE.fetch_add(l.size(), Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        LIVE.fetch_sub(l.size(), Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, size) };
        if !q.is_null() {
            LIVE.fetch_add(size, Relaxed);
            LIVE.fetch_sub(l.size(), Relaxed);
        }
        q
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

const ANALYTIC_SURFACES: [&str; 5] = [
    "PLANE",
    "CYLINDRICAL_SURFACE",
    "CONICAL_SURFACE",
    "SPHERICAL_SURFACE",
    "TOROIDAL_SURFACE",
];
const ANALYTIC_CURVES: [&str; 3] = ["LINE", "CIRCLE", "ELLIPSE"];

#[derive(Default, Clone, Copy)]
struct Counts {
    faces: u64,
    analytic_faces: u64,
    edges: u64,
    analytic_edges: u64,
    bspline_edges: u64,
    reps: u64,
    reps_analytic_surfaces: u64,
    reps_in_scope: u64,
    /// Representations out of scope by one or two faces.
    reps_one_or_two_off: u64,
}

impl std::ops::AddAssign for Counts {
    fn add_assign(&mut self, o: Self) {
        self.faces += o.faces;
        self.analytic_faces += o.analytic_faces;
        self.edges += o.edges;
        self.analytic_edges += o.analytic_edges;
        self.bspline_edges += o.bspline_edges;
        self.reps += o.reps;
        self.reps_analytic_surfaces += o.reps_analytic_surfaces;
        self.reps_in_scope += o.reps_in_scope;
        self.reps_one_or_two_off += o.reps_one_or_two_off;
    }
}

fn pct(a: u64, b: u64) -> String {
    if b == 0 {
        "–".into()
    } else {
        format!("{:.1}%", 100.0 * a as f64 / b as f64)
    }
}

fn label(g: &Graph, n: Node) -> String {
    match g.type_name(n) {
        TypeName::Simple(s) => s.to_owned(),
        TypeName::Complex(p) => format!("({})", p.join(" ")),
    }
}

/// The `i`th argument of a simple entity, when it is a reference.
fn ref_arg(g: &Graph, n: Node, i: usize) -> Option<Node> {
    match g.args(n).ok()?.get(i)? {
        Value::Ref(r) => Some(*r),
        _ => None,
    }
}

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

/// G2's acceptance on one file: args parse for every entity and part, and
/// their references are `refs`. Returns the failures, as messages.
fn check(g: &Graph) -> Vec<String> {
    let mut bad = Vec::new();
    for n in g.nodes() {
        let mut got = Vec::new();
        let mut want = g.refs(n).to_vec();
        if let TypeName::Simple(_) = g.type_name(n) {
            match g.args(n) {
                Ok(a) => value_refs(&a, &mut got),
                Err(e) => bad.push(format!("#{}: {e}", g.id(n).0)),
            }
        } else {
            for p in g.parts(n) {
                match g.args_of(n, p) {
                    Some(Ok(a)) => value_refs(&a, &mut got),
                    Some(Err(e)) => bad.push(format!("#{} {p}: {e}", g.id(n).0)),
                    None => bad.push(format!("#{} {p}: part not found", g.id(n).0)),
                }
            }
            // Parts come sorted; `refs` is in record order.
            got.sort_unstable();
            want.sort_unstable();
        }
        if got != want && g.dangling().iter().all(|&(r, _)| r != n) {
            bad.push(format!("#{}: refs differ from the arguments", g.id(n).0));
        }
    }
    bad
}

/// The 3-D curve of an `EDGE_CURVE`'s geometry.
fn curve_3d(g: &Graph, c: Node) -> Option<Node> {
    if g.is_a(c, "SURFACE_CURVE") || g.is_a(c, "SEAM_CURVE") {
        ref_arg(g, c, 1)
    } else {
        Some(c)
    }
}

fn census(g: &Graph, other: &mut BTreeMap<String, u64>) -> Counts {
    let mut c = Counts::default();
    // Per node: 0 unknown, 1 in scope, 2 not (edges and faces).
    let mut ok = vec![0u8; g.len()];
    for e in g.of_type("EDGE_CURVE") {
        c.edges += 1;
        let curve = ref_arg(g, e, 3).and_then(|c| curve_3d(g, c));
        let analytic = curve.is_some_and(|k| ANALYTIC_CURVES.iter().any(|t| g.is_a(k, t)));
        let bspline = curve.is_some_and(|k| g.is_a(k, "B_SPLINE_CURVE_WITH_KNOTS"));
        c.analytic_edges += u64::from(analytic);
        c.bspline_edges += u64::from(bspline);
        ok[e.index()] = if analytic || bspline { 1 } else { 2 };
        if !(analytic || bspline) {
            let l = curve.map_or("(unreadable)".into(), |k| format!("curve {}", label(g, k)));
            *other.entry(l).or_default() += 1;
        }
    }
    let mut stack: Vec<Node> = Vec::new();
    let mut seen = vec![u32::MAX; g.len()];
    for f in g.of_type("ADVANCED_FACE") {
        c.faces += 1;
        let surface = ref_arg(g, f, 2);
        let analytic = surface.is_some_and(|s| ANALYTIC_SURFACES.iter().any(|t| g.is_a(s, t)));
        c.analytic_faces += u64::from(analytic);
        if !analytic {
            let l = surface.map_or("(unreadable)".into(), |s| {
                format!("surface {}", label(g, s))
            });
            *other.entry(l).or_default() += 1;
        }
        // Every EDGE_CURVE under the face's bounds must be an MVP edge.
        let mut edges_ok = true;
        stack.extend(g.refs(f).iter().filter(|&&r| Some(r) != surface));
        while let Some(n) = stack.pop() {
            if seen[n.index()] == f.index() as u32 {
                continue;
            }
            seen[n.index()] = f.index() as u32;
            if g.is_a(n, "EDGE_CURVE") {
                edges_ok &= ok[n.index()] == 1;
            } else {
                stack.extend_from_slice(g.refs(n));
            }
        }
        ok[f.index()] = if analytic && edges_ok { 1 } else { 2 };
    }
    // Shape representations: their faces by reachability, not entering
    // another representation.
    let is_rep = |n: Node| g.parts(n).any(|p| p.ends_with("SHAPE_REPRESENTATION"));
    seen.fill(u32::MAX);
    for r in g.nodes().filter(|&n| is_rep(n)) {
        let (mut faces, mut analytic, mut out) = (0u64, 0u64, 0u64);
        stack.extend_from_slice(g.refs(r));
        while let Some(n) = stack.pop() {
            if seen[n.index()] == r.index() as u32 || is_rep(n) {
                continue;
            }
            seen[n.index()] = r.index() as u32;
            if g.is_a(n, "ADVANCED_FACE") {
                faces += 1;
                let s = ref_arg(g, n, 2);
                analytic +=
                    u64::from(s.is_some_and(|s| ANALYTIC_SURFACES.iter().any(|t| g.is_a(s, t))));
                out += u64::from(ok[n.index()] != 1);
            } else {
                stack.extend_from_slice(g.refs(n));
            }
        }
        if faces > 0 {
            c.reps += 1;
            c.reps_analytic_surfaces += u64::from(analytic == faces);
            c.reps_in_scope += u64::from(out == 0);
            c.reps_one_or_two_off += u64::from(matches!(out, 1 | 2));
        }
    }
    c
}

fn step_files(p: &Path, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(p)
            .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect())
            .unwrap_or_default();
        entries.sort();
        for e in entries {
            step_files(&e, out);
        }
    } else if p
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("step") || e.eq_ignore_ascii_case("stp"))
    {
        out.push(p.to_owned());
    }
}

fn main() {
    let mut files = Vec::new();
    for a in std::env::args().skip(1) {
        step_files(Path::new(&a), &mut files);
    }
    if files.is_empty() {
        eprintln!("usage: graph_census PATH...");
        std::process::exit(2);
    }
    let mut total = Counts::default();
    let mut other = BTreeMap::new();
    let (mut failed, mut skipped, mut counted, mut entities) = (0usize, Vec::new(), 0usize, 0u64);
    let (mut scan_secs, t0) = (0.0, Instant::now());
    println!(
        "| file | entities | faces | analytic faces | edges | MVP edges (analytic + B-spline) \
         | reps | reps, analytic surfaces | reps in scope | scan s | B/entity | B/entity with referrers |"
    );
    println!(
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |"
    );
    for f in &files {
        let before = LIVE.load(Relaxed);
        let t = Instant::now();
        let g = match Graph::open(f) {
            Ok(g) => g,
            Err(e) => {
                skipped.push(format!("{}: {e}", f.display()));
                continue;
            }
        };
        let secs = t.elapsed().as_secs_f64();
        scan_secs += secs;
        let heap = LIVE.load(Relaxed) - before;
        if let Some(n) = g.nodes().next() {
            let _ = g.referrers(n);
        }
        let with = LIVE.load(Relaxed) - before;
        let per = |h: usize| h as f64 / g.len().max(1) as f64;
        let bad = check(&g);
        if !bad.is_empty() {
            failed += bad.len();
            for b in bad.iter().take(5) {
                eprintln!("{}: {b}", f.display());
            }
        }
        let c = census(&g, &mut other);
        total += c;
        counted += 1;
        entities += g.len() as u64;
        println!(
            "| {} | {} | {} | {} | {} | {} ({} + {}) | {} | {} | {} | {secs:.3} | {:.1} | {:.1} |",
            f.display(),
            g.len(),
            c.faces,
            pct(c.analytic_faces, c.faces),
            c.edges,
            pct(c.analytic_edges + c.bspline_edges, c.edges),
            pct(c.analytic_edges, c.edges),
            pct(c.bspline_edges, c.edges),
            c.reps,
            pct(c.reps_analytic_surfaces, c.reps),
            pct(c.reps_in_scope, c.reps),
            per(heap),
            per(with),
        );
    }
    let t = total;
    println!();
    println!(
        "**Totals:** {counted} files counted, {} skipped, {entities} entities. \
         Faces {} of {} analytic ({}). Edges {} of {} MVP ({}: analytic {}, B-spline {}). \
         Representations with faces: {}; all surfaces analytic {} ({}); in scope {} ({}); \
         out by one or two faces {} ({}). Argument or round-trip failures: {failed}. \
         Scan {scan_secs:.1} s, wall {:.1} s.",
        skipped.len(),
        t.analytic_faces,
        t.faces,
        pct(t.analytic_faces, t.faces),
        t.analytic_edges + t.bspline_edges,
        t.edges,
        pct(t.analytic_edges + t.bspline_edges, t.edges),
        pct(t.analytic_edges, t.edges),
        pct(t.bspline_edges, t.edges),
        t.reps,
        t.reps_analytic_surfaces,
        pct(t.reps_analytic_surfaces, t.reps),
        t.reps_in_scope,
        pct(t.reps_in_scope, t.reps),
        t.reps_one_or_two_off,
        pct(t.reps_one_or_two_off, t.reps),
        t0.elapsed().as_secs_f64(),
    );
    println!();
    println!("**Out of scope, by type:**");
    println!();
    let mut other: Vec<_> = other.into_iter().collect();
    other.sort_by_key(|o| std::cmp::Reverse(o.1));
    for (k, v) in other {
        println!("- `{k}`: {v}");
    }
    println!();
    println!("**Skipped (refused):**");
    println!();
    for s in &skipped {
        println!("- {s}");
    }
    std::process::exit(i32::from(failed > 0));
}
