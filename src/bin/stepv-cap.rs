//! `stepv-cap`: runs a command under the kernel memory cap (#83).
//!
//! ```text
//! stepv-cap --memory-mb N [--label NAME] [--report-json PATH] -- CMD [ARGS...]
//! ```
//!
//! It is for kernel launches outside the `stepv` CLI (the scripts', the
//! tests', the nix checks'), and for a test runner to wrap each test in;
//! routing them all through it is #83's next step. The cap is the
//! CLI's own ([`stepv::cap`]): the footprint of `CMD` and all its
//! descendants, summed, sampled every 5 ms. `CMD` runs in a process group of
//! its own; past `N` MiB, that group and every descendant are killed with
//! SIGKILL, one line on stderr names `NAME` (default: the command line), the
//! cap and the peak, and `stepv-cap` exits [`EXIT_OVER_CAP`]. SIGINT,
//! SIGTERM, SIGHUP and SIGQUIT are passed on to the group. `CMD` sees its
//! cap in `STEPV_CAP_MB`.
//!
//! Otherwise it exits as `CMD` did: its exit code, or the same signal.
//! `125` is stepv-cap's own failure (usage, an unwritable report, an OS that
//! cannot list a process's children), `126` a `CMD` that cannot run, `127`
//! one that does not exist.
//!
//! `--report-json PATH` writes, however the run ends, one JSON object:
//! `label`, `cap_bytes`, `peak_bytes` (the largest footprint sampled),
//! `wall_ms`, `over_cap`, and `code` / `signal` (each `null` when it does
//! not apply).

use std::ffi::OsString;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Command, ExitCode, ExitStatus};
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

use stepv::cap::{EXIT_OVER_CAP, Sampler, tree_footprint, tree_supported};

const USAGE: &str =
    "usage: stepv-cap --memory-mb N [--label NAME] [--report-json PATH] -- CMD [ARGS...]";

/// stepv-cap's own failure, as `timeout(1)` has it.
const EXIT_ERROR: u8 = 125;

/// The last signal to pass on to the group; 0 for none.
static FORWARD: AtomicI32 = AtomicI32::new(0);

extern "C" fn forward(sig: libc::c_int) {
    FORWARD.store(sig, Ordering::Relaxed);
}

struct Args {
    cap_mb: u64,
    label: Option<String>,
    report: Option<OsString>,
    cmd: Vec<OsString>,
}

fn parse(mut it: impl Iterator<Item = OsString>) -> Result<Args, String> {
    let (mut cap_mb, mut label, mut report) = (None, None, None);
    loop {
        let Some(arg) = it.next() else {
            return Err("no command after --".to_owned());
        };
        let mut value = |flag: &str| it.next().ok_or(format!("{flag} needs a value"));
        match arg.to_str() {
            Some("--") => break,
            Some("--memory-mb") => {
                let n: u64 = value("--memory-mb")?
                    .to_str()
                    .and_then(|s| s.parse().ok())
                    .filter(|&n| n > 0)
                    .ok_or("--memory-mb needs a whole number of MiB above 0")?;
                cap_mb = Some(n);
            }
            Some("--label") => label = Some(value("--label")?.to_string_lossy().into_owned()),
            Some("--report-json") => report = Some(value("--report-json")?),
            _ => return Err(format!("unexpected {}", arg.to_string_lossy())),
        }
    }
    let cmd: Vec<OsString> = it.collect();
    if cmd.is_empty() {
        return Err("no command after --".to_owned());
    }
    Ok(Args {
        cap_mb: cap_mb.ok_or("--memory-mb is required")?,
        label,
        report,
        cmd,
    })
}

fn main() -> ExitCode {
    let args = match parse(std::env::args_os().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("stepv-cap: {e}\n{USAGE}");
            return ExitCode::from(EXIT_ERROR);
        }
    };
    if !tree_supported() {
        eprintln!(
            "stepv-cap: cannot list a process's children here (Linux needs CONFIG_PROC_CHILDREN)"
        );
        return ExitCode::from(EXIT_ERROR);
    }
    let label = args.label.clone().unwrap_or_else(|| {
        let words: Vec<_> = args.cmd.iter().map(|a| a.to_string_lossy()).collect();
        words.join(" ")
    });
    for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT] {
        // SAFETY: `forward` only stores to an atomic: async-signal-safe.
        unsafe { libc::signal(sig, forward as *const () as libc::sighandler_t) };
    }

    let start = Instant::now();
    let mut child = match Command::new(&args.cmd[0])
        .args(&args.cmd[1..])
        .env("STEPV_CAP_MB", args.cap_mb.to_string())
        .process_group(0)
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "stepv-cap: cannot run {}: {e}",
                args.cmd[0].to_string_lossy()
            );
            let not_found = e.kind() == std::io::ErrorKind::NotFound;
            return ExitCode::from(if not_found { 127 } else { 126 });
        }
    };
    let pid = child.id();
    let cap = args.cap_mb << 20;
    let mut memory = Sampler::new(Some(cap));
    let mut tree = vec![pid];
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(e) => {
                eprintln!("stepv-cap: cannot wait for {label}: {e}");
                return ExitCode::from(EXIT_ERROR);
            }
        }
        let sig = FORWARD.swap(0, Ordering::Relaxed);
        if sig != 0 {
            // SAFETY: plain syscall; the group is the child's own.
            unsafe { libc::killpg(pid as libc::pid_t, sig) };
        }
        let sampled = memory.over(|| {
            tree_footprint(pid).map(|(bytes, pids)| {
                tree = pids;
                bytes
            })
        });
        if sampled {
            // The group, and descendants that left it (a nested stepv-cap's).
            // SAFETY: plain syscalls on processes this run started.
            unsafe { libc::killpg(pid as libc::pid_t, libc::SIGKILL) };
            for &p in &tree {
                unsafe { libc::kill(p as libc::pid_t, libc::SIGKILL) };
            }
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let wall = start.elapsed();
    if status.is_none() {
        eprintln!(
            "stepv-cap: {label}: over the memory cap of {} MiB (peak {} MiB): killed",
            args.cap_mb,
            memory.peak() >> 20
        );
    }
    if let Some(path) = &args.report {
        let report = serde_json::json!({
            "label": label,
            "cap_bytes": cap,
            "peak_bytes": memory.peak(),
            "wall_ms": wall.as_secs_f64() * 1000.0,
            "over_cap": status.is_none(),
            "code": status.and_then(|s| s.code()),
            "signal": status.and_then(|s| s.signal()),
        });
        if let Err(e) = std::fs::write(path, format!("{report}\n")) {
            eprintln!("stepv-cap: cannot write {}: {e}", path.to_string_lossy());
            return ExitCode::from(EXIT_ERROR);
        }
    }
    match status {
        None => ExitCode::from(EXIT_OVER_CAP),
        Some(s) => exit_as(s),
    }
}

/// Ends this process as `status` ended `CMD`'s: the same code, or the same
/// signal.
fn exit_as(status: ExitStatus) -> ExitCode {
    if let Some(code) = status.code() {
        return ExitCode::from(u8::try_from(code & 0xff).unwrap_or(1));
    }
    let sig = status.signal().unwrap_or(libc::SIGKILL);
    // SAFETY: restoring the default action, then raising: plain syscalls.
    unsafe {
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
    // A signal whose default is not to terminate: the shells' convention.
    ExitCode::from(u8::try_from(128 + sig).unwrap_or(1))
}
