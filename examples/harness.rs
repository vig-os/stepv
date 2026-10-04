//! The S1 kernel harness (`plan.md` §5): run the kernel over the corpus and
//! print the numbers the acceptance gate asks for.
//!
//! ```sh
//! just fixtures && just harness            # whole corpus
//! just harness tests/fixtures/nist-pmi     # one source
//! just harness --cold-start 30             # spawn-to-result latency only
//! just harness --strict tests/fixtures/x   # CI gate: non-zero on any crash
//! ```
//!
//! For each file: import through XCAF, tessellate at bbox-relative
//! `Deflection::PREVIEW`, decode the buffers into a `Scene`, and require
//! `Mesh::is_well_formed()` on every part. One kernel PROCESS per file, run
//! serially, so a crash is contained and the wall-clock and peak-RSS numbers
//! are not contaminated by a neighbour.
//!
//! Per-file results go to `harness-out/results.jsonl`; the summary table is
//! printed as Markdown, ready to paste into `plan.md`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use stepv::occt::{self, Outcome};
use stepv::{Deflection, FaceStatus};

const FIXTURES: &str = "tests/fixtures";
const OUT: &str = "harness-out";
const CORPUS_EXTS: &[&str] = &["step", "stp", "iges", "igs", "brep"];

/// What happened to one file, judged against what its source expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Verdict {
    /// Loaded; every face is shown faithfully (`FaceStatus::is_faithful`:
    /// exact triangles from any rung of the ladder, or a degenerate sliver
    /// with nothing to draw).
    Pass,
    /// No surfaces at all, only curves (a sketch), drawn as lines. Counted as
    /// a pass: everything in the file is shown.
    Wireframe,
    /// Some faces are `Approx`: drawn, shape right, boundary jagged, and
    /// flagged for the renderer's warning treatment. Not a pass.
    Degraded,
    /// Some faces are `Missing`: holes, drawn as outlines. Not a pass.
    Partial,
    /// The kernel reported a clean failure (exit 3, valid summary).
    CleanFail,
    /// Kernel exited 0 but its buffers failed `is_well_formed()` or did not
    /// decode. Always a stepv bug.
    BadMesh,
    Crash,
    Timeout,
    /// Killed at `--memory-gb` (off by default: the harness measures peaks).
    MemoryCap,
}

impl Verdict {
    const ALL: [Self; 9] = [
        Self::Pass,
        Self::Wireframe,
        Self::Degraded,
        Self::Partial,
        Self::CleanFail,
        Self::BadMesh,
        Self::Crash,
        Self::Timeout,
        Self::MemoryCap,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Wireframe => "wireframe",
            Self::Degraded => "degraded",
            Self::Partial => "partial",
            Self::CleanFail => "clean-fail",
            Self::BadMesh => "bad-mesh",
            Self::Crash => "crash",
            Self::Timeout => "timeout",
            Self::MemoryCap => "memory-cap",
        }
    }
}

struct Record {
    source: String,
    path: PathBuf,
    verdict: Verdict,
    wall_ms: f64,
    run: occt::Run,
    note: Option<String>,
}

struct Args {
    paths: Vec<PathBuf>,
    timeout: Duration,
    deflection: Deflection,
    cold_start: Option<usize>,
    memory: Option<u64>,
    strict: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        paths: Vec::new(),
        timeout: Duration::from_secs(60),
        deflection: Deflection::PREVIEW,
        cold_start: None,
        memory: None,
        strict: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| it.next().ok_or(format!("{flag} needs a value"));
        match arg.as_str() {
            "--timeout" => {
                let s: f64 = value("--timeout")?
                    .parse()
                    .map_err(|e| format!("--timeout: {e}"))?;
                a.timeout = Duration::from_secs_f64(s);
            }
            "--quality" => {
                a.deflection = match value("--quality")?.as_str() {
                    "preview" => Deflection::PREVIEW,
                    "thumbnail" => Deflection::THUMBNAIL,
                    q => return Err(format!("--quality: unknown {q:?}")),
                }
            }
            "--strict" => a.strict = true,
            "--memory-gb" => {
                let gb: f64 = value("--memory-gb")?
                    .parse()
                    .map_err(|e| format!("--memory-gb: {e}"))?;
                a.memory = Some((gb * f64::from(1u32 << 30)) as u64);
            }
            "--cold-start" => {
                a.cold_start = Some(
                    value("--cold-start")?
                        .parse()
                        .map_err(|e| format!("--cold-start: {e}"))?,
                );
            }
            s if s.starts_with('-') => return Err(format!("unknown flag {s}")),
            _ => a.paths.push(PathBuf::from(arg)),
        }
    }
    if a.paths.is_empty() {
        a.paths.push(PathBuf::from(FIXTURES));
    }
    Ok(a)
}

fn collect(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect();
        entries.sort();
        for e in entries {
            collect(&e, out);
        }
    } else if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| CORPUS_EXTS.contains(&e.to_ascii_lowercase().as_str()))
    {
        out.push(path.to_path_buf());
    }
}

/// The corpus source a file belongs to: its first directory under fixtures/.
fn source_of(path: &Path) -> String {
    let rel = path.strip_prefix(FIXTURES).unwrap_or(path);
    rel.components()
        .next()
        .filter(|_| rel.components().count() > 1)
        .map_or_else(
            || "(other)".into(),
            |c| c.as_os_str().to_string_lossy().into_owned(),
        )
}

fn judge(kernel: &Path, path: &Path, args: &Args, mesh: &Path) -> std::io::Result<Record> {
    let _ = std::fs::remove_file(mesh);
    let limits = occt::Limits {
        timeout: args.timeout,
        memory: args.memory,
    };
    let run = occt::run(kernel, path, args.deflection, limits, Some(mesh))?;
    let mut note = run.summary.as_ref().and_then(|s| s.error.clone());
    let verdict = match run.outcome {
        Outcome::Timeout => Verdict::Timeout,
        Outcome::MemoryCap => Verdict::MemoryCap,
        Outcome::Crashed { signal, code } => {
            note = Some(format!("signal {signal:?}, exit {code:?}"));
            Verdict::Crash
        }
        // A clean failure must still carry a parseable summary; one that does
        // not has broken the exit-3 contract (plan.md §4).
        Outcome::Failed if run.summary.is_none() => {
            note = Some("exit 3 without a valid summary".into());
            Verdict::Crash
        }
        Outcome::Failed => Verdict::CleanFail,
        Outcome::Ok => match std::fs::read(mesh)
            .map_err(|e| e.to_string())
            .and_then(|b| occt::read_mesh(&b).map_err(|e| e.to_string()))
        {
            Err(e) => {
                note = Some(e);
                Verdict::BadMesh
            }
            Ok(scene) => {
                let bad = scene.parts.iter().position(|p| {
                    !p.is_well_formed()
                        || !p
                            .mesh
                            .positions
                            .iter()
                            .chain(&p.mesh.normals)
                            .chain(&p.lines.positions)
                            .all(|v| v.is_finite())
                });
                let s = run.summary.as_ref();
                if let Some(i) = bad {
                    note = Some(format!("part {i} is not well formed"));
                    Verdict::BadMesh
                } else if s.is_none_or(|s| s.triangles != scene.triangle_count()) {
                    note = Some("summary and buffers disagree on triangle count".into());
                    Verdict::BadMesh
                } else if s.is_none_or(|s| s.segments != scene.segment_count()) {
                    note = Some("summary and buffers disagree on segment count".into());
                    Verdict::BadMesh
                } else {
                    let s = s.expect("checked");
                    if s.faces_unmeshed > 0 {
                        note = Some(format!(
                            "{} of {} faces failed first pass: {} remeshed, {} healed, \
                             {} refined, {} coarse, {} degenerate, {} approx, {} missing",
                            s.faces_unmeshed,
                            s.faces,
                            s.faces_remeshed,
                            s.faces_healed,
                            s.faces_refined,
                            s.faces_coarse,
                            s.faces_degenerate,
                            s.faces_approx,
                            s.faces_missing
                        ));
                    }
                    match scene.worst_face() {
                        None if scene.segment_count() > 0 => Verdict::Wireframe,
                        Some(FaceStatus::Missing) => Verdict::Partial,
                        Some(FaceStatus::Approx) => Verdict::Degraded,
                        _ => Verdict::Pass,
                    }
                }
            }
        },
    };
    let _ = std::fs::remove_file(mesh);
    Ok(Record {
        source: source_of(path),
        path: path.to_path_buf(),
        verdict,
        wall_ms: run.wall.as_secs_f64() * 1e3,
        run,
        note,
    })
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let rank = (p / 100.0 * (sorted.len() - 1) as f64).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

fn pct(n: usize, d: usize) -> String {
    if d == 0 {
        "—".into()
    } else {
        format!("{:.1}%", 100.0 * n as f64 / d as f64)
    }
}

fn jsonl(r: &Record) -> String {
    serde_json::json!({
        "source": r.source,
        "path": r.path,
        "verdict": r.verdict.name(),
        "wall_ms": r.wall_ms,
        "note": r.note,
        "summary": r.run.summary.as_ref().map(|s| serde_json::json!({
            "stage": s.stage, "format": s.format, "parts": s.parts,
            "parts_named": s.parts_named, "parts_colored": s.parts_colored,
            "parts_face_colored": s.parts_face_colored, "faces": s.faces,
            "faces_unmeshed": s.faces_unmeshed, "faces_remeshed": s.faces_remeshed,
            "faces_healed": s.faces_healed, "faces_refined": s.faces_refined,
            "faces_coarse": s.faces_coarse, "faces_degenerate": s.faces_degenerate,
            "faces_approx": s.faces_approx, "faces_missing": s.faces_missing,
            "sketch_parts": s.sketch_parts, "construction_parts": s.construction_parts,
            "segments": s.segments,
            "triangles": s.triangles,
            "t_read_ms": s.t_read_ms, "t_transfer_ms": s.t_transfer_ms,
            "t_mesh_ms": s.t_mesh_ms, "t_extract_ms": s.t_extract_ms,
            "peak_rss_bytes": s.peak_rss_bytes,
        })),
    })
    .to_string()
}

fn report(records: &[Record]) -> String {
    let mut by_source: BTreeMap<&str, Vec<&Record>> = BTreeMap::new();
    for r in records {
        by_source.entry(&r.source).or_default().push(r);
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "| Source | Files | Pass | Wireframe | Degraded | Partial | Clean fail | Bad mesh | \
         Crash | Timeout | Memory cap | Pass rate | Named | Coloured (part / face) | p50 ms | p95 ms | \
         Max RSS MB |"
    );
    let _ = writeln!(out, "| --- |{}", " ---: |".repeat(16));
    let mut row = |name: &str, rs: &[&Record]| {
        let count = |v| rs.iter().filter(|r| r.verdict == v).count();
        let passed: Vec<_> = rs.iter().filter(|r| r.verdict == Verdict::Pass).collect();
        let mut walls: Vec<f64> = passed.iter().map(|r| r.wall_ms).collect();
        walls.sort_by(f64::total_cmp);
        let sums: Vec<_> = rs
            .iter()
            .filter(|r| {
                matches!(
                    r.verdict,
                    Verdict::Pass | Verdict::Wireframe | Verdict::Degraded | Verdict::Partial
                )
            })
            .filter_map(|r| r.run.summary.as_ref())
            .collect();
        let parts: usize = sums.iter().map(|s| s.parts).sum();
        let named: usize = sums.iter().map(|s| s.parts_named).sum();
        let colored: usize = sums.iter().map(|s| s.parts_colored).sum();
        let face_colored: usize = sums.iter().map(|s| s.parts_face_colored).sum();
        let rss = rs
            .iter()
            .filter_map(|r| r.run.summary.as_ref())
            .map(|s| s.peak_rss_bytes)
            .max()
            .unwrap_or(0);
        let c = Verdict::ALL.map(count);
        let _ = writeln!(
            out,
            "| {name} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} / {} | {:.0} | {:.0} | {:.0} |",
            rs.len(),
            c[0],
            c[1],
            c[2],
            c[3],
            c[4],
            c[5],
            c[6],
            c[7],
            c[8],
            pct(c[0] + c[1], rs.len()),
            pct(named, parts),
            pct(colored, parts),
            pct(face_colored, parts),
            percentile(&walls, 50.0),
            percentile(&walls, 95.0),
            rss as f64 / 1e6,
        );
    };
    for (name, rs) in &by_source {
        // Malformed files are SUPPOSED to fail: their success column is
        // "Clean fail", and a 0% pass rate there is the right answer.
        if *name == "malformed" {
            row("malformed (want clean fail)", rs);
        } else {
            row(name, rs);
        }
    }
    // The headline excludes `malformed`: those files are SUPPOSED to fail, and
    // folding their clean failures in would make the pass rate meaningless.
    let real: Vec<&Record> = records.iter().filter(|r| r.source != "malformed").collect();
    row("**all but malformed**", &real);

    // Where the first-pass failures landed on the recovery ladder.
    let sums: Vec<_> = real.iter().filter_map(|r| r.run.summary.as_ref()).collect();
    let total = |f: fn(&occt::Summary) -> usize| sums.iter().map(|s| f(s)).sum::<usize>();
    let _ = writeln!(
        out,
        "\n| Faces failing first pass | Remeshed | Healed | Refined | Coarse | Degenerate | \
         Approx | Missing | Sketch parts | Construction parts |\n|{}\n\
         | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
        " ---: |".repeat(10),
        total(|s| s.faces_unmeshed),
        total(|s| s.faces_remeshed),
        total(|s| s.faces_healed),
        total(|s| s.faces_refined),
        total(|s| s.faces_coarse),
        total(|s| s.faces_degenerate),
        total(|s| s.faces_approx),
        total(|s| s.faces_missing),
        total(|s| s.sketch_parts),
        total(|s| s.construction_parts),
    );
    out
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("harness: {e}");
            return ExitCode::from(2);
        }
    };
    let kernel = occt::kernel_path();
    if !kernel.is_file() {
        eprintln!(
            "harness: no kernel at {} — run `just kernel`",
            kernel.display()
        );
        return ExitCode::from(2);
    }
    let mut files = Vec::new();
    for p in &args.paths {
        collect(p, &mut files);
    }
    if files.is_empty() {
        // An empty corpus is a failure, never a green run (plan.md §7).
        eprintln!(
            "harness: no corpus files under {:?} — run `just fixtures`",
            args.paths
        );
        return ExitCode::FAILURE;
    }
    if std::fs::create_dir_all(OUT).is_err() {
        eprintln!("harness: cannot create {OUT}/");
        return ExitCode::FAILURE;
    }
    let mesh = Path::new(OUT).join("current.msh");

    if let Some(n) = args.cold_start {
        return cold_start(&kernel, &files, &args, &mesh, n);
    }

    let mut records = Vec::with_capacity(files.len());
    let mut lines = String::new();
    for (i, f) in files.iter().enumerate() {
        let r = match judge(&kernel, f, &args, &mesh) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("harness: cannot spawn kernel: {e}");
                return ExitCode::FAILURE;
            }
        };
        eprintln!(
            "[{:>4}/{}] {:<10} {:>8.0} ms  {}{}",
            i + 1,
            files.len(),
            r.verdict.name(),
            r.wall_ms,
            f.display(),
            r.note
                .as_deref()
                .map(|n| format!("  ({n})"))
                .unwrap_or_default()
        );
        lines.push_str(&jsonl(&r));
        lines.push('\n');
        records.push(r);
    }
    let _ = std::fs::write(Path::new(OUT).join("results.jsonl"), lines);
    let table = report(&records);
    let _ = std::fs::write(Path::new(OUT).join("summary.md"), &table);
    println!("{table}");
    if args.strict {
        // CI gate (vig-os/stepv#11): nothing may crash, hang, mis-decode or
        // blow the cap, and every malformed file must fail CLEANLY.
        let bad: Vec<_> = records
            .iter()
            .filter(|r| {
                matches!(
                    r.verdict,
                    Verdict::Crash | Verdict::Timeout | Verdict::BadMesh | Verdict::MemoryCap
                ) || (r.source == "malformed" && r.verdict != Verdict::CleanFail)
            })
            .collect();
        for r in &bad {
            eprintln!(
                "harness --strict: {} {}",
                r.verdict.name(),
                r.path.display()
            );
        }
        if !bad.is_empty() {
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

/// Spawn-to-result latency: the same small passing file, `n` times. This is
/// the Plan B counterpart of Plan A's `.cwasm` vs JIT cold-start question —
/// there is no module to compile, so what remains is process start, OCCT
/// static init, read, mesh and write.
fn cold_start(kernel: &Path, files: &[PathBuf], args: &Args, mesh: &Path, n: usize) -> ExitCode {
    let Some(probe) = files
        .iter()
        .filter_map(|f| Some((std::fs::metadata(f).ok()?.len(), f)))
        .filter(|(len, _)| *len > 0)
        .min()
        .map(|(_, f)| f.clone())
    else {
        eprintln!("harness: no non-empty file to probe");
        return ExitCode::FAILURE;
    };
    let mut walls = Vec::with_capacity(n);
    for _ in 0..n {
        match judge(kernel, &probe, args, mesh) {
            Ok(r) if r.verdict == Verdict::Pass => walls.push(r.wall_ms),
            Ok(r) => {
                eprintln!(
                    "harness: probe {} did not pass: {:?}",
                    probe.display(),
                    r.verdict
                );
                return ExitCode::FAILURE;
            }
            Err(e) => {
                eprintln!("harness: cannot spawn kernel: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    walls.sort_by(f64::total_cmp);
    println!(
        "cold start over {n} runs of {} ({} bytes): min {:.1} ms, p50 {:.1} ms, p95 {:.1} ms",
        probe.display(),
        std::fs::metadata(&probe).map_or(0, |m| m.len()),
        walls[0],
        percentile(&walls, 50.0),
        percentile(&walls, 95.0),
    );
    ExitCode::SUCCESS
}
