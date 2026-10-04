//! `stepv` CLI — the one binary both platforms' front-ends shell out to.
//!
//! The CLI is the product boundary (see `plan.md` §4): macOS Quick Look and
//! the Linux `.thumbnailer` both invoke THIS, so anything it cannot do, the
//! previewer cannot do. It is intentionally the first thing that exists.
//!
//! Current state: argument surface only. The kernel wiring is spike step S1
//! (`plan.md` §5) and this binary reports honestly that it is not there yet
//! rather than pretending with a placeholder image.

use std::process::ExitCode;

const USAGE: &str = "\
stepv — STEP/IGES/BREP preview and thumbnails

USAGE:
    stepv <input> [--png <out> | --glb <out>] [options]

ARGS:
    <input>              .step / .stp / .iges / .igs / .brep

OPTIONS:
    --png <path>         Render a PNG thumbnail
    --glb <path>         Write a binary glTF
    --size <px>          PNG edge length (default 512)
    --quality <q>        thumbnail | preview (default thumbnail)
    --timeout <secs>     Hard wall-clock cap (default 20)
    --info               Print header metadata as JSON and exit
    -h, --help           Print this help

EXIT CODES:
    0  success
    2  usage error
    3  tessellation failed (metadata on stdout is still valid)
    4  timeout exceeded
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    eprintln!(
        "stepv: not implemented yet — the kernel spike (plan.md §5, step S1) \
         has not landed.\n\
         \n\
         This binary exists so the CLI contract is fixed before either \
         front-end is written.\n\
         Run `stepv --help` for the argument surface it will honour."
    );
    ExitCode::from(3)
}
