//! `stepv` CLI — the one binary both platforms' front-ends shell out to.
//!
//! The CLI is the product boundary (see `plan.md` §4): macOS Quick Look and
//! the Linux `.thumbnailer` both invoke THIS, so anything it cannot do, the
//! previewer cannot do.
//!
//! Every run that gets past argument parsing prints ONE line of JSON on
//! stdout — the file's header metadata plus what happened — including on
//! failure. That is the exit-3 contract: a front-end that could not get
//! geometry still has something honest to show.

use std::io::{IsTerminal as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, UNIX_EPOCH};

use serde_json::{Value, json};
use stepv::occt::{self, Limits, Outcome};
use stepv::topology::Topology;
use stepv::{Deflection, Scene, cache, glb, header, render, view};

const USAGE: &str = "\
stepv — STEP/IGES/BREP preview and thumbnails

USAGE:
    stepv <input> [--png <out> | --glb <out>] [options]
    stepv <input> --info
    stepv view <input> [options]     Interactive viewer window (needs a desktop
                                     session; fails at once without one)

ARGS:
    <input>              .step / .stp / .iges / .igs / .brep

OPTIONS:
    --png <path>         Render a PNG thumbnail
    --glb <path>         Write a binary glTF
    --mesh <path>        Write the raw STEPVMSH buffers (for front-ends)
    --topology <path>    Also write the exact topology as JSON: assembly tree,
                         surface/curve types and parameters, areas, volumes
                         (if it fails beside --png/--glb/--mesh, the output is
                         still written, with \"topology_error\" in the report)
    --size <px>          PNG edge length, 16..=4096 (default 512)
    --quality <q>        thumbnail | preview (default thumbnail)
    --timeout <secs>     Hard wall-clock cap (default 20)
    --memory-mb <n>      Kernel memory cap, 0 = none (default 4096)
    --show-construction  Draw construction curves beside solids
    --no-cache           Neither read nor write the cache
    --info               Print header metadata as JSON and exit
    --software           view: the software window, not the GPU
    --theme <t>          view: auto | light | dark (default auto: the OS's)
    --frames <n>         view: orbit for n frames, then exit and report the
                         frame intervals (\"frames\" in the JSON)
    -V, --version        Print the version
    -h, --help           Print this help

VIEWER:
    Drag to orbit, right- or shift-drag to pan, scroll to zoom; click a face
    to inspect its exact surface (Esc clears it); M measures between two
    clicks (distance, axis distance, angle); R reset, F front, T top,
    C construction curves, E edges, Q or Esc to quit. The Section panel cuts the
    model along X, Y or Z. Defaults to
    --quality preview and --timeout 120. Draws on the GPU (Metal, Vulkan
    or GL), or in software when there is no usable adapter; the JSON line
    says which as \"backend\".

OUTPUT:
    One JSON line on stdout for every run past argument parsing, success
    or not: header metadata, the outcome, and the kernel's summary. When
    stderr is a terminal, the run's progress is shown there as it goes.

EXIT CODES:
    0  success
    2  usage error
    3  no geometry (unreadable, tessellation failed, memory cap); the
       metadata on stdout is still valid
    4  timeout exceeded
";

const EXIT_OK: u8 = 0;
const EXIT_USAGE: u8 = 2;
const EXIT_FAILED: u8 = 3;
const EXIT_TIMEOUT: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Png,
    Glb,
    /// The kernel's STEPVMSH buffers, validated: what the macOS preview
    /// extension reads (format spec at the top of kernel/stepv-occt.cpp).
    Mesh,
}

#[derive(Debug)]
struct Args {
    input: PathBuf,
    output: Option<(Format, PathBuf)>,
    /// `--topology`: written beside any output, or alone.
    topology: Option<PathBuf>,
    info: bool,
    size: u32,
    deflection: Deflection,
    limits: Limits,
    show_construction: bool,
    cache: bool,
    view: bool,
    /// `view --software`.
    software: bool,
    /// `view --frames N`.
    frames: Option<u32>,
    theme: view::ThemePref,
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut a = Args {
        input: PathBuf::new(),
        output: None,
        topology: None,
        info: false,
        size: 512,
        deflection: Deflection::THUMBNAIL,
        limits: Limits::DEFAULT,
        show_construction: false,
        cache: true,
        view: false,
        software: false,
        frames: None,
        theme: view::ThemePref::Auto,
    };
    let argv = match argv.split_first() {
        Some((first, rest)) if first == "view" => {
            a.view = true;
            // A person is waiting at a window, not a file manager in a loop.
            a.deflection = Deflection::PREVIEW;
            a.limits.timeout = Duration::from_secs(120);
            rest
        }
        _ => argv,
    };
    let mut input = None;
    let mut it = argv.iter();
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| it.next().cloned().ok_or(format!("{flag} needs a value"));
        match arg.as_str() {
            "--png" | "--glb" | "--mesh" => {
                if a.output.is_some() {
                    return Err("give one of --png, --glb or --mesh".into());
                }
                let f = match arg.as_str() {
                    "--png" => Format::Png,
                    "--glb" => Format::Glb,
                    _ => Format::Mesh,
                };
                a.output = Some((f, PathBuf::from(value(arg)?)));
            }
            "--topology" => a.topology = Some(PathBuf::from(value(arg)?)),
            "--size" => {
                let n: u32 = value(arg)?.parse().map_err(|_| "--size needs a number")?;
                if !(16..=4096).contains(&n) {
                    return Err("--size must be within 16..=4096".into());
                }
                a.size = n;
            }
            "--quality" => {
                a.deflection = match value(arg)?.as_str() {
                    "thumbnail" => Deflection::THUMBNAIL,
                    "preview" => Deflection::PREVIEW,
                    q => {
                        return Err(format!(
                            "--quality: expected thumbnail or preview, got {q:?}"
                        ));
                    }
                };
            }
            "--timeout" => {
                let s: f64 = value(arg)?.parse().map_err(|_| "--timeout needs seconds")?;
                if !(s > 0.0 && s.is_finite()) {
                    return Err("--timeout must be positive".into());
                }
                a.limits.timeout = Duration::from_secs_f64(s);
            }
            "--memory-mb" => {
                let mb: u64 = value(arg)?
                    .parse()
                    .map_err(|_| "--memory-mb needs a number")?;
                a.limits.memory = (mb > 0).then_some(mb << 20);
            }
            "--show-construction" => a.show_construction = true,
            "--no-cache" => a.cache = false,
            "--software" => a.software = true,
            "--frames" => {
                let n: u32 = value(arg)?.parse().map_err(|_| "--frames needs a number")?;
                if !(1..=100_000).contains(&n) {
                    return Err("--frames must be within 1..=100000".into());
                }
                a.frames = Some(n);
            }
            "--theme" => a.theme = value(arg)?.parse()?,
            "--info" => a.info = true,
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            _ if input.is_some() => return Err(format!("unexpected argument {arg:?}")),
            _ => input = Some(PathBuf::from(arg)),
        }
    }
    a.input = input.ok_or("no input file given")?;
    if a.info && (a.output.is_some() || a.topology.is_some()) {
        return Err("--info writes no files: drop --png/--glb/--mesh/--topology".into());
    }
    if a.view && (a.output.is_some() || a.topology.is_some()) {
        return Err("view takes no --png/--glb/--mesh/--topology".into());
    }
    if !a.view && (a.software || a.frames.is_some() || a.theme != view::ThemePref::Auto) {
        return Err("--software, --frames and --theme are for `stepv view`".into());
    }
    if !a.info && !a.view && a.output.is_none() && a.topology.is_none() {
        return Err("nothing to do: give --png, --glb, --topology or --info".into());
    }
    Ok(a)
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.is_empty() || argv.iter().any(|a| a == "-h" || a == "--help") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if argv.iter().any(|a| a == "-V" || a == "--version") {
        println!("stepv {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("stepv: {e}\nRun `stepv --help` for usage.");
            return ExitCode::from(EXIT_USAGE);
        }
    };
    let (code, report) = run(&args);
    // One line, always — the front-ends parse it.
    let _ = writeln!(std::io::stdout(), "{report}");
    ExitCode::from(code)
}

/// The whole run, returning the exit code and the stdout report.
fn run(args: &Args) -> (u8, Value) {
    // Before anything reads the file: what this run caches is keyed on the
    // file as it was then, never on a later save ([`cache::FileId`]).
    let file = cache::FileId::of(&args.input);
    let info = header::read(&args.input);
    let mut report = json!({ "stepv": env!("CARGO_PKG_VERSION"), "info": &info });
    if args.info {
        report["status"] = json!("info");
        return (EXIT_OK, report);
    }
    // The outline artifact (#62) under the snapshot: a streamed run leaves
    // it as soon as the outline arrives, the viewer's run included (#63).
    let outline = cache::cache_dir()
        .filter(|_| args.cache)
        .zip(file.clone())
        .map(|(dir, file)| (cache::outline_path(&dir, &file), info));
    if args.view {
        // Before the kernel: without a window there is nothing to show its
        // run in, and a user without a GUI must not wait for it first.
        if let Err(e) = view::session() {
            return fail(report, EXIT_FAILED, "error", &e);
        }
        return run_view(args, report, outline);
    }
    // ── Cache ── for an output alone: a topology needs the kernel anyway.
    let cache_path = args
        .output
        .as_ref()
        .filter(|_| args.cache && args.topology.is_none())
        .zip(file.as_ref())
        .and_then(|((format, _), file)| cache_path(args, *format, file));
    if let (Some(cp), Some((_, out))) = (&cache_path, &args.output)
        && let Some(code) = from_cache(cp, out, &mut report)
    {
        return (code, report);
    }

    let topology_tmp = args.topology.as_ref().map(|_| temp_path("json"));
    let result = tessellate(
        args,
        &mut report,
        cache_path.as_deref(),
        topology_tmp.as_deref(),
        outline,
        None,
    );
    let topology = topology_tmp.as_ref().map(|t| topology_bytes(&report, t));
    let (scene, raw) = match result {
        Ok(s) => s,
        Err(code) => return (code, report),
    };
    report["worst_face"] = json!(scene.worst_face().map(|s| format!("{s:?}").to_lowercase()));

    // The topology failed after a good mesh (#38): with an output, reported
    // beside it; asked for alone, the run failed.
    let topology_error = report["kernel"]["topology_error"]
        .as_str()
        .map(str::to_owned);
    if let Some(e) = &topology_error {
        if args.output.is_none() {
            return fail(report, EXIT_FAILED, "error", &format!("topology: {e}"));
        }
        report["topology_error"] = json!(e);
        // An older topology at the destination would pass for this model's.
        if let Some(dest) = &args.topology {
            let _ = std::fs::remove_file(dest);
        }
    }
    if topology_error.is_none()
        && let (Some(bytes), Some(dest)) = (topology, &args.topology)
    {
        // The kernel wrote it: a file that does not parse, or does not match
        // the mesh it came with, is a stepv bug.
        let checked = bytes.map_err(|e| e.to_string()).and_then(|b| {
            Topology::parse(&b)
                .and_then(|t| t.check_against(&scene))
                .map(|()| b)
                .map_err(|e| e.to_string())
        });
        let bytes = match checked {
            Ok(b) => b,
            Err(e) => {
                return fail(
                    report,
                    EXIT_FAILED,
                    "error",
                    &format!("bad kernel output: {e}"),
                );
            }
        };
        if let Err(e) = cache::write_atomic(dest, &bytes) {
            let msg = format!("cannot write {}: {e}", dest.display());
            return fail(report, EXIT_FAILED, "error", &msg);
        }
        report["topology"] = json!(dest);
    }
    let Some((format, out)) = args.output.clone() else {
        report["status"] = json!("ok");
        return (EXIT_OK, report);
    };

    let bytes = match format {
        Format::Png => {
            let opts = render::Options {
                show_construction: args.show_construction,
                camera: render::Camera::for_scene(&scene),
                ..render::Options::square(args.size)
            };
            match render::render(&scene, &opts).map(|img| img.to_png()) {
                Ok(Ok(png)) => png,
                Ok(Err(e)) => {
                    return fail(report, EXIT_FAILED, "error", &format!("PNG encode: {e}"));
                }
                Err(e) => return fail(report, EXIT_FAILED, "failed", &e.to_string()),
            }
        }
        Format::Glb => glb::to_glb(
            &scene,
            &glb::Options {
                show_construction: args.show_construction,
            },
        ),
        Format::Mesh => raw,
    };
    if let Err(e) = cache::write_atomic(&out, &bytes) {
        return fail(
            report,
            EXIT_FAILED,
            "error",
            &format!("cannot write {}: {e}", out.display()),
        );
    }
    // Only a complete result gets here: `tessellate` returned a scene. A part
    // file that did not read is outside the key: the whole assembly may be
    // there next time.
    if let Some(cp) = cache_path
        .as_ref()
        .filter(|_| report["kernel"]["external_missing"].as_u64() == Some(0))
        && let Err(e) = cache::store(cp, &bytes)
    {
        eprintln!("stepv: cannot cache {}: {e}", cp.display());
    }
    report["status"] = json!("ok");
    report["output"] = json!(out);
    report["cached"] = json!(false);
    (EXIT_OK, report)
}

/// `stepv view`: the window opens at once, on the tree and the run's
/// progress, while the kernel runs on a thread of its own, and the scene
/// follows (#85). Closing the window first cancels the run: status
/// `cancelled`, exit 0, and a `message` (not an `error`) saying so.
fn run_view(
    args: &Args,
    mut report: Value,
    outline: Option<(PathBuf, header::Info)>,
) -> (u8, Value) {
    let name = args
        .input
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let opts = view::Options {
        software: args.software,
        theme: args.theme,
        file: name.clone(),
        // --frames closes the window itself: with a screenshot too,
        // whichever came first would win, and the other go missing.
        screenshot: std::env::var_os("STEPV_VIEW_SCREENSHOT")
            .filter(|_| {
                let keep = args.frames.is_none();
                if !keep {
                    eprintln!("stepv: --frames ignores STEPV_VIEW_SCREENSHOT");
                }
                keep
            })
            .map(PathBuf::from),
        frames: args.frames,
        input: Some(args.input.clone()),
        limits: Some(args.limits),
        pick_at: std::env::var("STEPV_VIEW_PICK")
            .map(|s| view::parse_pick_at(&s))
            .unwrap_or_default(),
        measure: std::env::var_os("STEPV_VIEW_MEASURE").is_some_and(|v| v == "1"),
        section: std::env::var("STEPV_VIEW_SECTION").ok().and_then(|s| {
            let mut f = s.split(',').map(str::trim);
            let (a, o): (usize, f32) = (f.next()?.parse().ok()?, f.next()?.parse().ok()?);
            let flip = f.next() == Some("flip");
            (a < 3 && (0.0..=1.0).contains(&o)).then_some((a, o, flip))
        }),
        close_loading: std::env::var_os("STEPV_VIEW_CLOSE_LOADING").is_some_and(|v| v == "1"),
        // The kernel's sandbox and unhealed faces come with the scene.
        ..view::Options::default()
    };
    let (tx, rx) = std::sync::mpsc::channel();
    // A cached outline shows the tree before the kernel even starts.
    if let Some(a) = outline
        .as_ref()
        .and_then(|(p, _)| cache::load_outline_at(p))
    {
        let _ = tx.send(view::Load::Tree(a.tree, a.leaves));
    }
    let cancel = AtomicBool::new(false);
    let (ran, loaded) = std::thread::scope(|s| {
        let (report, cancel) = (&mut report, &cancel);
        // The loader owns the sender: however it ends, a panic included,
        // the channel closes with it, and the window never waits on it.
        let loader = s.spawn(move || load_view(args, report, outline, tx, cancel));
        let ran = view::run(rx, &format!("stepv — {name}"), &opts);
        // The window is closed: a run still going is for nobody.
        cancel.store(true, Ordering::Relaxed);
        (ran, loader.join())
    });
    let loaded = loaded.unwrap_or_else(|p| std::panic::resume_unwind(p));
    if let Ok(ran) = &ran {
        report["backend"] = json!(ran.backend.name());
        if let Some(f) = ran.frames {
            report["frames"] = json!({ "count": f.count, "p50_ms": f.p50_ms, "p95_ms": f.p95_ms });
        }
    }
    match (loaded, ran) {
        // The run's own failure is the report's, the window aside; a window
        // that never opened cancelled the run, and is the error.
        (Err(code), _) if report["status"] != "cancelled" => (code, report),
        (_, Err(e)) => fail(report, EXIT_FAILED, "error", &e),
        (Err(code), Ok(_)) => (code, report),
        (Ok(()), Ok(_)) => {
            report["status"] = json!("ok");
            (EXIT_OK, report)
        }
    }
}

/// The viewer's load, on the loader thread: the kernel's run, streamed to
/// the window through `tx` as [`view::Load`]s and stopped by `cancel`, then
/// the scene and its exact topology. On failure, fills `report`, tells the
/// window, and returns the exit code.
fn load_view(
    args: &Args,
    report: &mut Value,
    outline: Option<(PathBuf, header::Info)>,
    tx: std::sync::mpsc::Sender<view::Load>,
    cancel: &AtomicBool,
) -> Result<(), u8> {
    // The exact B-rep for the inspector (#29) comes with the mesh.
    let topo_tmp = temp_path("json");
    let result = tessellate(
        args,
        report,
        None,
        Some(&topo_tmp),
        outline,
        Some((&tx, cancel)),
    );
    let topo_bytes = topology_bytes(report, &topo_tmp);
    let scene = match result {
        Ok((s, _)) => s,
        Err(code) => {
            let _ = tx.send(view::Load::Failed);
            return Err(code);
        }
    };
    // Without it the viewer still opens: it only cannot name surfaces.
    let topology = match topo_bytes
        .map_err(|e| e.to_string())
        .and_then(|b| Topology::parse(&b).map_err(|e| e.to_string()))
        .and_then(|t| {
            t.check_against(&scene)
                .map(|()| t)
                .map_err(|e| e.to_string())
        }) {
        Ok(t) => Some(t),
        Err(e) => {
            eprintln!("stepv: no exact topology for the inspector: {e}");
            None
        }
    };
    let name = args
        .input
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let warn = match scene.worst_face() {
        Some(stepv::FaceStatus::Approx) => " — some faces approximated",
        Some(stepv::FaceStatus::Missing) => " — some faces missing",
        _ => "",
    };
    let title = format!("stepv — {name} ({} parts){warn}", scene.parts.len());
    let _ = tx.send(view::Load::Done(Box::new(view::Loaded {
        scene,
        topology,
        title,
        sandbox: report["kernel"]["sandbox"].as_str().map(str::to_owned),
        // Absent (an older kernel, or no summary): no replay.
        unhealed: serde_json::from_value(report["kernel"]["unhealed_faces"].clone()).ok(),
    })));
    Ok(())
}

fn fail_in(report: &mut Value, code: u8, status: &str, msg: &str) -> u8 {
    report["status"] = json!(status);
    report["error"] = json!(msg);
    code
}

/// The topology the kernel wrote to `tmp`, which is removed: an error when
/// the kernel reported the topology failed (#38), whatever the file holds.
fn topology_bytes(report: &Value, tmp: &Path) -> std::io::Result<Vec<u8>> {
    let bytes = std::fs::read(tmp);
    let _ = std::fs::remove_file(tmp);
    match report["kernel"]["topology_error"].as_str() {
        Some(e) => Err(std::io::Error::other(format!(
            "the kernel's topology failed: {e}"
        ))),
        None => bytes,
    }
}

/// Runs the kernel and decodes its buffers. On failure, fills `report` and
/// returns the exit code. Shared by `--png`/`--glb` and `view`.
///
/// With `outline` (the artifact's path and the file's header), the run is
/// streamed and leaves a [`cache::OutlineArtifact`] once the outline arrives,
/// whether or not it then completes; a complete run adds the model's box.
/// With stderr a terminal, the run is streamed too, and its progress shown
/// there ([`Progress`]).
/// With `viewer` (#85), the run is streamed to the viewer's window too, and
/// stopped when its flag is set.
fn tessellate(
    args: &Args,
    report: &mut Value,
    cache_path: Option<&Path>,
    topology_out: Option<&Path>,
    outline: Option<(PathBuf, header::Info)>,
    viewer: Option<(&std::sync::mpsc::Sender<view::Load>, &AtomicBool)>,
) -> Result<(Scene, Vec<u8>), u8> {
    // ── Kernel ──
    let kernel = occt::kernel_path();
    if !kernel.is_file() {
        return Err(fail_in(
            report,
            EXIT_FAILED,
            "error",
            &format!("kernel not found at {}", kernel.display()),
        ));
    }
    let mesh = temp_path("msh");
    // The viewer draws and picks the B-rep edges (STEPVMSH v4, #31);
    // thumbnails, --glb and --mesh (Quick Look's format) keep v3.
    let edges = args.view;
    let mut artifact = None;
    // Progress for a person at a terminal (#63), and nothing for a pipe: a
    // front-end reads the stdout line alone.
    let mut progress = std::io::stderr().is_terminal().then(|| {
        // A cached outline names the parts before the kernel even starts.
        let cached = outline
            .as_ref()
            .and_then(|(path, _)| cache::load_outline_at(path));
        Progress::start(cached.map(|a| a.leaves))
    });
    let result = match (&outline, progress.is_some() || viewer.is_some()) {
        (None, false) => occt::run(
            &kernel,
            &args.input,
            args.deflection,
            args.limits,
            Some(&mesh),
            topology_out,
            edges,
        ),
        _ => occt::run_streaming(
            &kernel,
            &args.input,
            args.deflection,
            args.limits,
            Some(&mesh),
            topology_out,
            edges,
            viewer.map(|(_, cancel)| cancel),
            &mut |e| {
                if let Some(p) = progress.as_mut() {
                    p.event(&e);
                }
                if let Some((tx, _)) = viewer {
                    let _ = tx.send(match &e {
                        occt::Event::Tree(t) => view::Load::Tree(t.tree.clone(), t.leaves),
                        occt::Event::Progress(p) => view::Load::Progress(*p),
                    });
                }
                if let (occt::Event::Tree(tree), Some((path, info))) = (e, &outline) {
                    let a = cache::OutlineArtifact::new(info.clone(), tree);
                    // One already there is of this very file, and may have
                    // its box from a run that completed: keep it.
                    if cache::load_outline_at(path).is_none() {
                        store_outline(path, &a);
                    }
                    artifact = Some(a);
                }
            },
        ),
    };
    // Off the line before anything else is said on stderr.
    if let Some(p) = progress {
        p.finish();
    }
    let run = match result {
        Ok(r) => r,
        Err(e) => {
            let _ = std::fs::remove_file(&mesh);
            return Err(fail_in(
                report,
                EXIT_FAILED,
                "error",
                &format!("cannot start kernel: {e}"),
            ));
        }
    };
    report["kernel"] = json!(run.summary.as_ref().map(|s| json!({
        "stage": s.stage, "error": s.error, "cause": s.cause.map(|c| format!("{c:?}").to_lowercase()),
        "parts": s.parts, "faces": s.faces,
        "triangles": s.triangles, "segments": s.segments, "bbox": s.bbox,
        "faces_approx": s.faces_approx, "faces_missing": s.faces_missing,
        "faces_over_budget": s.faces_over_budget, "unhealed_faces": s.unhealed_faces,
        "peak_rss_bytes": s.peak_rss_bytes, "sandbox": s.sandbox,
        "external_files": s.external_files, "external_missing": s.external_missing,
        "topology_error": s.topology_error,
    })));
    // Loudly: the run worked, but less contained than it should have been.
    if let Some(s) = run.summary.as_ref().filter(|s| !s.sandboxed()) {
        eprintln!(
            "stepv: warning: the kernel ran without its full sandbox ({}): this OS \
             lacks Landlock or seccomp, or refused the sandbox profile",
            s.sandbox.as_deref().unwrap_or("kernel predates sandboxing")
        );
    }
    report["wall_ms"] = json!((run.wall.as_secs_f64() * 1e3).round());

    let outcome = match run.outcome {
        // Not a crash: the kernel exited as the contract allows, but broke
        // it by leaving out the summary (`occt::run`, `occt::run_streaming`).
        Outcome::Ok | Outcome::Failed if run.summary.is_none() => Some((
            EXIT_FAILED,
            "error",
            "bad kernel output: the kernel exited without a summary".to_owned(),
        )),
        Outcome::Ok => None,
        Outcome::Timeout => Some((
            EXIT_TIMEOUT,
            "timeout",
            "wall-clock cap exceeded".to_owned(),
        )),
        Outcome::MemoryCap => Some((EXIT_FAILED, "memory-cap", "memory cap exceeded".to_owned())),
        // Not a failure: the viewer's window closed first, and nobody waits.
        Outcome::Cancelled => Some((
            EXIT_OK,
            "cancelled",
            "the viewer was closed before the run finished".to_owned(),
        )),
        Outcome::Failed => Some((
            EXIT_FAILED,
            "failed",
            run.summary
                .as_ref()
                .and_then(|s| s.error.clone())
                .unwrap_or_else(|| "kernel failed".into()),
        )),
        Outcome::Crashed { signal, code } => Some((
            EXIT_FAILED,
            "crashed",
            format!("kernel crashed (signal {signal:?}, exit {code:?})"),
        )),
    };
    if let Some((code, status, msg)) = outcome {
        let _ = std::fs::remove_file(&mesh);
        // Only a failure the kernel puts down to the file itself repeats. A
        // timeout or a memory cap depends on the limits and the machine, a
        // crash may not repeat, and an environment failure (an unwritable
        // output, a missing part file) is outside the key: caching one would
        // block a later run that could succeed. An older kernel gives no
        // cause, and is not cached either.
        if run.outcome == Outcome::Failed
            && run.summary.as_ref().and_then(|s| s.cause) == Some(occt::Cause::Input)
            && let Some(cp) = cache_path
        {
            remember_failure(cp, status, &msg);
        }
        if run.outcome == Outcome::Cancelled {
            // Not an error: the report's `message` says what happened.
            report["status"] = json!(status);
            report["message"] = json!(msg);
            return Err(code);
        }
        return Err(fail_in(report, code, status, &msg));
    }
    let scene = std::fs::read(&mesh)
        .map_err(|e| e.to_string())
        .and_then(|b| {
            occt::read_mesh(&b)
                .map(|s| (s, b))
                .map_err(|e| e.to_string())
        });
    let _ = std::fs::remove_file(&mesh);
    // The kernel wrote it: a decode failure is a stepv bug, not user input.
    let scene = scene.map_err(|e| {
        fail_in(
            report,
            EXIT_FAILED,
            "error",
            &format!("bad kernel output: {e}"),
        )
    })?;
    // Complete: the box goes in only now.
    if let (Some((path, _)), Some(mut a)) = (outline, artifact) {
        a.bbox = run.summary.as_ref().and_then(|s| s.bbox);
        store_outline(&path, &a);
    }
    Ok(scene)
}

/// The CLI's progress line on a terminal's stderr (#63): the part count
/// once known, then the stage and its percentage, redrawn in place and
/// cleared when the run ends.
struct Progress {
    parts: Option<usize>,
    /// The stage and whole percent last drawn.
    shown: Option<(occt::Stage, u64)>,
}

impl Progress {
    /// Starts the line, with the part count of a cached outline.
    fn start(parts: Option<usize>) -> Self {
        let p = Self { parts, shown: None };
        p.draw(&p.line("reading"));
        p
    }

    fn event(&mut self, e: &occt::Event) {
        match e {
            occt::Event::Tree(t) => {
                self.parts = Some(t.leaves);
                let stage = self.shown.map_or_else(|| "reading".into(), stage_text);
                self.draw(&self.line(&stage));
            }
            occt::Event::Progress(p) => {
                let at = (p.stage, (p.fraction() * 100.0).floor() as u64);
                // A redraw per whole percent: the kernel reports thousandths.
                if self.shown != Some(at) {
                    self.shown = Some(at);
                    self.draw(&self.line(&stage_text(at)));
                }
            }
        }
    }

    /// `stepv: 12 parts · meshing 40%`.
    fn line(&self, stage: &str) -> String {
        match self.parts {
            Some(1) => format!("stepv: 1 part · {stage}"),
            Some(n) => format!("stepv: {n} parts · {stage}"),
            None => format!("stepv: {stage}"),
        }
    }

    fn draw(&self, line: &str) {
        // `\r` and erase-to-end: the line is redrawn over itself.
        eprint!("\r{line}\x1b[K");
    }

    fn finish(self) {
        eprint!("\r\x1b[K");
    }
}

fn stage_text((stage, pct): (occt::Stage, u64)) -> String {
    let name = match stage {
        occt::Stage::Transfer => "transferring",
        occt::Stage::Mesh => "meshing",
    };
    format!("{name} {pct}%")
}

fn fail(mut report: Value, code: u8, status: &str, msg: &str) -> (u8, Value) {
    report["status"] = json!(status);
    report["error"] = json!(msg);
    (code, report)
}

/// The cache file for `file` and these options; `None` if there is no
/// cache directory.
fn cache_path(args: &Args, format: Format, file: &cache::FileId) -> Option<PathBuf> {
    let dir = cache::cache_dir()?;
    let (output, ext) = match format {
        Format::Png => (cache::Output::ThumbnailPng, "png"),
        Format::Glb => (cache::Output::Glb, "glb"),
        Format::Mesh => (cache::Output::Buffers, "msh"),
    };
    let variant = u64::from(args.size) | (u64::from(args.show_construction) << 32);
    let key = cache::key(&cache::KeyInputs {
        path: file.path.as_os_str().as_encoded_bytes(),
        len: file.len,
        mtime_nanos: file.mtime_nanos,
        linear_rel: args.deflection.linear_rel,
        angular_deg: args.deflection.angular_deg,
        output,
        variant,
    });
    Some(dir.join(format!("{key}.{ext}")))
}

/// Writes the outline artifact; a failure costs the next run its head
/// start, not this run its result.
fn store_outline(path: &Path, a: &cache::OutlineArtifact) {
    if let Err(e) = cache::store_outline(path, a) {
        eprintln!("stepv: cannot cache the outline: {e}");
    }
}
/// Serves a cache hit, or a cached failure, into `out`. `None` on a miss,
/// which an incomplete or old-version entry is ([`cache::load`]).
fn from_cache(cp: &Path, out: &Path, report: &mut Value) -> Option<u8> {
    if let Some(bytes) = cache::load(cp) {
        if cache::write_atomic(out, &bytes).is_ok() {
            report["status"] = json!("ok");
            report["output"] = json!(out);
            report["cached"] = json!(true);
            return Some(EXIT_OK);
        }
        return None;
    }
    // A failure is as expensive to recompute as a success; Finder will ask
    // again on every window that shows the folder.
    let fail = cache::load(&cp.with_extension("fail"))?;
    let v: Value = serde_json::from_slice(&fail).ok()?;
    report["status"] = v["status"].clone();
    report["error"] = v["error"].clone();
    report["cached"] = json!(true);
    Some(EXIT_FAILED)
}

fn remember_failure(cp: &Path, status: &str, msg: &str) {
    let body = json!({ "status": status, "error": msg }).to_string();
    let _ = cache::store(&cp.with_extension("fail"), body.as_bytes());
}

fn temp_path(ext: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    std::env::temp_dir().join(format!("stepv-{}-{nanos}.{ext}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Result<Args, String> {
        parse_args(&v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn parses_a_full_command_line() {
        let a = args(&[
            "m.step",
            "--png",
            "o.png",
            "--size",
            "256",
            "--quality",
            "preview",
            "--timeout",
            "5",
            "--memory-mb",
            "0",
        ])
        .unwrap();
        assert_eq!(a.input, PathBuf::from("m.step"));
        assert_eq!(a.output, Some((Format::Png, PathBuf::from("o.png"))));
        assert_eq!(a.size, 256);
        assert_eq!(a.deflection, Deflection::PREVIEW);
        assert_eq!(a.limits.timeout, Duration::from_secs(5));
        assert_eq!(a.limits.memory, None);
    }

    #[test]
    fn rejects_bad_command_lines() {
        for bad in [
            &["m.step"][..],
            &["--png", "o.png"],
            &["m.step", "--png", "a.png", "--glb", "b.glb"],
            &["m.step", "--png"],
            &["m.step", "--png", "o.png", "--size", "8"],
            &["m.step", "--png", "o.png", "--quality", "ultra"],
            &["m.step", "--png", "o.png", "--timeout", "-1"],
            &["m.step", "--png", "o.png", "--frobnicate"],
            &["a.step", "b.step", "--info"],
        ] {
            assert!(args(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn view_subcommand_takes_preview_defaults() {
        let a = args(&["view", "m.step"]).unwrap();
        assert!(a.view);
        assert_eq!(a.deflection, Deflection::PREVIEW);
        assert_eq!(a.limits.timeout, Duration::from_secs(120));
        assert!(args(&["view", "m.step", "--png", "o.png"]).is_err());
        assert!(!a.software);
        assert_eq!(a.theme, view::ThemePref::Auto);
        let a = args(&["view", "m.step", "--software", "--theme", "dark"]).unwrap();
        assert!(a.software);
        assert_eq!(a.theme, view::ThemePref::Dark);
        assert!(args(&["view", "m.step", "--theme", "blue"]).is_err());
        assert_eq!(
            args(&["view", "m.step", "--frames", "30"]).unwrap().frames,
            Some(30)
        );
        assert!(args(&["view", "m.step", "--frames", "0"]).is_err());
        assert!(args(&["m.step", "--png", "o.png", "--frames", "3"]).is_err());
        // Viewer options make no sense without the viewer.
        assert!(args(&["m.step", "--png", "o.png", "--software"]).is_err());
        assert!(args(&["m.step", "--info", "--theme", "dark"]).is_err());
        // An explicit option still wins over the view default.
        assert_eq!(
            args(&["view", "m.step", "--timeout", "5"])
                .unwrap()
                .limits
                .timeout,
            Duration::from_secs(5)
        );
    }

    #[test]
    fn info_needs_no_output() {
        assert!(args(&["m.step", "--info"]).unwrap().info);
    }
}
