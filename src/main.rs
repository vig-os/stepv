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

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
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
    stepv view <input> [options]     Interactive viewer window

ARGS:
    <input>              .step / .stp / .iges / .igs / .brep

OPTIONS:
    --png <path>         Render a PNG thumbnail
    --glb <path>         Write a binary glTF
    --mesh <path>        Write the raw STEPVMSH buffers (for front-ends)
    --topology <path>    Also write the exact topology as JSON: assembly tree,
                         surface/curve types and parameters, areas, volumes
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
    to inspect its exact surface (Esc clears it); R reset, F front, T top,
    C construction curves, E edges, Q or Esc to quit. The Section panel cuts the
    model along X, Y or Z. Defaults to
    --quality preview and --timeout 120. Draws on the GPU (Metal, Vulkan
    or GL), or in software when there is no usable adapter; the JSON line
    says which as \"backend\".

OUTPUT:
    One JSON line on stdout for every run past argument parsing, success
    or not: header metadata, the outcome, and the kernel's summary.

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
    let info = header::read(&args.input);
    let mut report = json!({ "stepv": env!("CARGO_PKG_VERSION"), "info": info });
    if args.info {
        report["status"] = json!("info");
        return (EXIT_OK, report);
    }
    if args.view {
        // The exact B-rep for the inspector (#29) comes with the mesh.
        let topo_tmp = temp_path("json");
        let mut result = tessellate(args, &mut report, None, Some(&topo_tmp));
        let mut topo_bytes = std::fs::read(&topo_tmp);
        let _ = std::fs::remove_file(&topo_tmp);
        // The kernel failed in the topology stage, after the mesh: open the
        // viewer without the inspector rather than not at all (#29 review).
        if result.is_err() && topology_failed(&report) {
            eprintln!(
                "stepv: the exact topology failed ({}); opening without the inspector",
                report["error"].as_str().unwrap_or("unknown error")
            );
            report = json!({ "stepv": report["stepv"], "info": report["info"] });
            result = tessellate(args, &mut report, None, None);
            topo_bytes = Err(std::io::Error::other("the kernel's topology stage failed"));
        }
        let scene = match result {
            Ok((s, _)) => s,
            Err(code) => return (code, report),
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
        let sandbox = report["kernel"]["sandbox"].as_str().map(str::to_owned);
        let opts = view::Options {
            software: args.software,
            theme: args.theme,
            sandboxed: sandbox.as_deref().is_some_and(occt::full_sandbox),
            sandbox,
            file: name,
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
            pick_at: std::env::var("STEPV_VIEW_PICK")
                .ok()
                .and_then(|s| view::parse_pick_at(&s)),
        };
        return match view::run(scene, topology, &title, &opts) {
            Ok(ran) => {
                report["status"] = json!("ok");
                report["backend"] = json!(ran.backend.name());
                if let Some(f) = ran.frames {
                    report["frames"] = json!({
                        "count": f.count, "p50_ms": f.p50_ms, "p95_ms": f.p95_ms,
                    });
                }
                (EXIT_OK, report)
            }
            Err(e) => fail(report, EXIT_FAILED, "error", &e),
        };
    }
    // ── Cache ── for an output alone: a topology needs the kernel anyway.
    let cache_path = args
        .output
        .as_ref()
        .filter(|_| args.cache && args.topology.is_none())
        .and_then(|(format, _)| cache_path(args, *format));
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
    );
    let topology = topology_tmp.as_ref().map(|t| {
        let bytes = std::fs::read(t);
        let _ = std::fs::remove_file(t);
        bytes
    });
    let (scene, raw) = match result {
        Ok(s) => s,
        Err(code) => return (code, report),
    };
    report["worst_face"] = json!(scene.worst_face().map(|s| format!("{s:?}").to_lowercase()));

    if let (Some(bytes), Some(dest)) = (topology, &args.topology) {
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
        if let Err(e) = write_atomic(dest, &bytes) {
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
    if let Err(e) = write_atomic(&out, &bytes) {
        return fail(
            report,
            EXIT_FAILED,
            "error",
            &format!("cannot write {}: {e}", out.display()),
        );
    }
    if let Some(cp) = &cache_path {
        let _ = write_atomic(cp, &bytes);
    }
    report["status"] = json!("ok");
    report["output"] = json!(out);
    report["cached"] = json!(false);
    (EXIT_OK, report)
}

fn fail_in(report: &mut Value, code: u8, status: &str, msg: &str) -> u8 {
    report["status"] = json!(status);
    report["error"] = json!(msg);
    code
}

/// Whether a failed kernel run got as far as the topology, its last stage:
/// the mesh was fine, only `--topology` failed.
fn topology_failed(report: &Value) -> bool {
    report["kernel"]["stage"] == "topology"
}

/// Runs the kernel and decodes its buffers. On failure, fills `report` and
/// returns the exit code. Shared by `--png`/`--glb` and `view`.
fn tessellate(
    args: &Args,
    report: &mut Value,
    cache_path: Option<&Path>,
    topology_out: Option<&Path>,
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
    let result = occt::run(
        &kernel,
        &args.input,
        args.deflection,
        args.limits,
        Some(&mesh),
        topology_out,
        // The viewer draws and picks the B-rep edges (STEPVMSH v4, #31);
        // thumbnails, --glb and --mesh (Quick Look's format) keep v3.
        args.view,
    );
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
        "stage": s.stage, "error": s.error, "parts": s.parts, "faces": s.faces,
        "triangles": s.triangles, "segments": s.segments, "bbox": s.bbox,
        "faces_approx": s.faces_approx, "faces_missing": s.faces_missing,
        "peak_rss_bytes": s.peak_rss_bytes, "sandbox": s.sandbox,
        "external_files": s.external_files, "external_missing": s.external_missing,
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
        Outcome::Ok => None,
        Outcome::Timeout => Some((
            EXIT_TIMEOUT,
            "timeout",
            "wall-clock cap exceeded".to_owned(),
        )),
        Outcome::MemoryCap => Some((EXIT_FAILED, "memory-cap", "memory cap exceeded".to_owned())),
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
        // Timeouts are not cached: a busy machine is not a property of the file.
        if code == EXIT_FAILED
            && status != "crashed"
            && let Some(cp) = cache_path
        {
            remember_failure(cp, status, &msg);
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
    scene.map_err(|e| {
        fail_in(
            report,
            EXIT_FAILED,
            "error",
            &format!("bad kernel output: {e}"),
        )
    })
}

fn fail(mut report: Value, code: u8, status: &str, msg: &str) -> (u8, Value) {
    report["status"] = json!(status);
    report["error"] = json!(msg);
    (code, report)
}

/// The cache file for this input and these options; `None` if there is no
/// cache directory or the input cannot be stat'ed.
fn cache_path(args: &Args, format: Format) -> Option<PathBuf> {
    let dir = cache::cache_dir()?;
    let canon = std::fs::canonicalize(&args.input).ok()?;
    let meta = std::fs::metadata(&canon).ok()?;
    let mtime = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    let (output, ext) = match format {
        Format::Png => (cache::Output::ThumbnailPng, "png"),
        Format::Glb => (cache::Output::Glb, "glb"),
        Format::Mesh => (cache::Output::Buffers, "msh"),
    };
    let variant = u64::from(args.size) | (u64::from(args.show_construction) << 32);
    let key = cache::key(&cache::KeyInputs {
        path: canon.as_os_str().as_encoded_bytes(),
        len: meta.len(),
        mtime_nanos: i128::try_from(mtime.as_nanos()).ok()?,
        linear_rel: args.deflection.linear_rel,
        angular_deg: args.deflection.angular_deg,
        output,
        variant,
    });
    Some(dir.join(format!("{key}.{ext}")))
}

/// Serves a cache hit, or a cached failure, into `out`. `None` on a miss.
fn from_cache(cp: &Path, out: &Path, report: &mut Value) -> Option<u8> {
    if let Ok(bytes) = std::fs::read(cp) {
        if write_atomic(out, &bytes).is_ok() {
            report["status"] = json!("ok");
            report["output"] = json!(out);
            report["cached"] = json!(true);
            return Some(EXIT_OK);
        }
        return None;
    }
    // A failure is as expensive to recompute as a success; Finder will ask
    // again on every window that shows the folder.
    let fail = std::fs::read(cp.with_extension("fail")).ok()?;
    let v: Value = serde_json::from_slice(&fail).ok()?;
    report["status"] = v["status"].clone();
    report["error"] = v["error"].clone();
    report["cached"] = json!(true);
    Some(EXIT_FAILED)
}

fn remember_failure(cp: &Path, status: &str, msg: &str) {
    let body = json!({ "status": status, "error": msg }).to_string();
    let _ = write_atomic(&cp.with_extension("fail"), body.as_bytes());
}

/// Writes via a sibling temp file and a rename, so a reader (Finder, a
/// concurrent stepv) never sees a half-written PNG.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
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
    fn only_a_topology_stage_failure_is_retried() {
        assert!(topology_failed(&json!({"kernel": {"stage": "topology"}})));
        for stage in ["read", "transfer", "mesh", "done"] {
            assert!(!topology_failed(&json!({"kernel": {"stage": stage}})));
        }
        assert!(
            !topology_failed(&json!({"kernel": null})),
            "no summary: a crash, not this"
        );
    }

    #[test]
    fn info_needs_no_output() {
        assert!(args(&["m.step", "--info"]).unwrap().info);
    }
}
