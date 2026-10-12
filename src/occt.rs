//! The Plan B kernel: native OCCT in a subprocess (`kernel/stepv-occt`).
//!
//! The C++ side does the OCCT work and nothing else; this module runs it,
//! enforces the wall-clock cap by killing it, and turns its output into a
//! [`Scene`]. Running OCCT out of process is deliberate: a file that crashes
//! the kernel crashes a child, and the caller still gets an honest
//! [`Outcome::Crashed`] instead of a dead previewer. That is the isolation the
//! WASM sandbox was going to provide under Plan A (`plan.md` §3).
//!
//! The mesh file format is specified at the top of `kernel/stepv-occt.cpp`;
//! [`read_mesh`] is its only reader.

use std::io::{BufReader, Read};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ChildStdout, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::{BBox, Color, Deflection, Edges, Face, FaceStatus, LineKind, Lines, Mesh, Part, Scene};

/// The kernel's JSON summary, one line on its stdout. Present on success AND
/// on a clean failure (exit 3) — the header-level facts survive a failed
/// tessellation, which is what the front-ends' honest degradation rests on.
#[derive(Debug, Clone, Deserialize)]
pub struct Summary {
    pub ok: bool,
    /// The stage reached, or the one that failed: `read`, `transfer`,
    /// `walk`, `mesh`, `extract`, `done`.
    pub stage: String,
    pub error: Option<String>,
    pub format: String,
    pub bbox: Option<[f64; 6]>,
    pub diagonal: f64,
    pub linear_abs: f64,
    pub parts: usize,
    pub parts_named: usize,
    /// Parts with a part-level colour, their own or inherited from an
    /// assembly or instance.
    pub parts_colored: usize,
    /// Parts with no part-level colour but colours on individual faces.
    pub parts_face_colored: usize,
    pub prototypes: usize,
    pub prototypes_mesh_failed: usize,
    pub faces: usize,
    /// Unique faces the FIRST meshing pass left without triangles. Each then
    /// went down the recovery ladder and landed in one of the seven below.
    pub faces_unmeshed: usize,
    pub faces_remeshed: usize,
    pub faces_healed: usize,
    pub faces_refined: usize,
    pub faces_coarse: usize,
    pub faces_degenerate: usize,
    pub faces_approx: usize,
    pub faces_missing: usize,
    /// Of `faces_missing`: faces whose healing ran past its time budget
    /// (#51, `STEPV_OCCT_FACE_BUDGET_MS`) and was left undone. Absent from
    /// older kernels.
    #[serde(default)]
    pub faces_over_budget: usize,
    /// Faceless parts in a file with no faces at all: drawn as curves.
    pub sketch_parts: usize,
    /// Faceless parts beside solids: construction geometry.
    pub construction_parts: usize,
    pub vertices: usize,
    pub triangles: usize,
    pub segments: usize,
    /// Multi-file STEP assemblies (#19): the part files the top-level file
    /// references, and how many could not be read (missing, or outside what
    /// a sandbox lets the kernel open). Absent from older kernels.
    #[serde(default)]
    pub external_files: usize,
    #[serde(default)]
    pub external_missing: usize,
    /// Why the exact topology (`--topology`) failed, when it did after a
    /// good mesh (#38): the run still succeeds, without the topology file.
    /// Absent from older kernels, which failed the run instead.
    #[serde(default)]
    pub topology_error: Option<String>,
    pub t_read_ms: f64,
    pub t_transfer_ms: f64,
    pub t_mesh_ms: f64,
    pub t_extract_ms: f64,
    pub peak_rss_bytes: u64,
    /// The sandbox the kernel ran in (#18): `landlock+seccomp` (Linux) or
    /// `macos-profile` when complete; `landlock`, `seccomp` or `none` when the
    /// OS could not provide all of it. Absent from kernels that predate it.
    #[serde(default)]
    pub sandbox: Option<String>,
}

impl Summary {
    /// Whether the kernel ran fully sandboxed.
    #[must_use]
    pub fn sandboxed(&self) -> bool {
        self.sandbox.as_deref().is_some_and(full_sandbox)
    }
}

/// Whether a reported `"sandbox"` is the complete one.
#[must_use]
pub fn full_sandbox(sandbox: &str) -> bool {
    matches!(sandbox, "landlock+seccomp" | "macos-profile")
}

/// How a kernel run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Exit 0: geometry produced.
    Ok,
    /// Exit 3: a clean, reported failure. The summary is still valid.
    Failed,
    /// Killed by a signal, or an exit code outside the contract.
    Crashed {
        signal: Option<i32>,
        code: Option<i32>,
    },
    /// Exceeded the wall-clock cap and was killed.
    Timeout,
    /// Exceeded the memory cap: killed, or refused an allocation (Linux).
    MemoryCap,
}

/// One kernel invocation.
#[derive(Debug)]
pub struct Run {
    pub outcome: Outcome,
    /// `None` when the kernel crashed or timed out before printing.
    pub summary: Option<Summary>,
    /// Parent-side wall-clock, process spawn included: what a user waits for.
    pub wall: Duration,
}

/// Where the kernel binary is: `$STEPV_OCCT`, else beside the current
/// executable, else `../libexec/stepv/` relative to it, else this
/// checkout's `target/kernel/` (`just kernel`).
#[must_use]
pub fn kernel_path() -> PathBuf {
    if let Some(p) = std::env::var_os("STEPV_OCCT") {
        return PathBuf::from(p);
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        // Beside the binary (a dev build, the macOS bundle's MacOS/), or in
        // the FHS location a Linux package installs it to.
        for candidate in [
            dir.join("stepv-occt"),
            dir.join("../libexec/stepv/stepv-occt"),
        ] {
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("target/kernel/stepv-occt")
}

/// The limits a kernel run is held to. Both are enforced from outside the
/// kernel, by killing it: OCCT has no cooperative cancellation, and a hung or
/// ballooning previewer is a worse bug than one that shows an icon
/// (`plan.md` §4).
///
/// The memory cap is the child's *footprint* (resident on Linux, physical
/// footprint on macOS), sampled every 5 ms — not `RLIMIT_AS`. macOS does not
/// enforce `RLIMIT_AS`, and on Linux it caps virtual address space, which
/// OCCT plus glibc's per-thread malloc arenas reserve by the hundreds of MB:
/// a cap meant for resident memory would kill the kernel at load time and
/// look like a crash. The footprint excludes shared library pages, so it is
/// far below the kernel's RSS: a small file never exceeds 5 MB of it, while
/// its RSS is ~30 MB, nearly all of it OCCT's shared dylibs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    pub timeout: Duration,
    /// Peak memory, in bytes. `None` = uncapped.
    pub memory: Option<u64>,
}

impl Limits {
    /// The CLI defaults: 20 s, and 4 GiB. S1 measured the perforated-plate
    /// pathology at 7.2 GB and a 745 MB assembly at 5.5 GB; nothing a user
    /// waits for in a preview needs more than this.
    pub const DEFAULT: Self = Self {
        timeout: Duration::from_secs(20),
        memory: Some(4 << 30),
    };
}

/// Runs the kernel on `input`, writing mesh buffers to `mesh_out` and the
/// exact topology ([`crate::topology`]) to `topology_out`, each if given.
///
/// # Errors
/// Only when the kernel cannot be spawned at all. Every failure of the kernel
/// itself is reported through [`Run::outcome`].
pub fn run(
    kernel: &Path,
    input: &Path,
    deflection: Deflection,
    limits: Limits,
    mesh_out: Option<&Path>,
    topology_out: Option<&Path>,
    edges: bool,
) -> std::io::Result<Run> {
    let cmd = command(kernel, input, deflection, mesh_out, topology_out, edges);
    let s = supervise(
        cmd,
        limits,
        |mut out| {
            let mut text = String::new();
            let _ = out.read_to_string(&mut text);
            text
        },
        || {},
    )?;
    let summary: Option<Summary> = s
        .output
        .lines()
        .next()
        .and_then(|l| serde_json::from_str(l).ok());
    Ok(Run {
        outcome: outcome(s.status, s.over_memory, summary.as_ref()),
        summary,
        wall: s.wall,
    })
}

/// [`run`], streaming (`stepv-occt --stream`, #54): `on_event` gets the
/// run's [`Event`]s as they happen, on the calling thread: a STEP file's
/// outline once it is parsed, then progress through the transfer and the
/// meshing. The summary ends the stream and is the [`Run`]'s, as in [`run`].
///
/// A run killed partway (the wall clock, the memory cap) keeps the events
/// that arrived and has no summary: it is not complete, whatever arrived. A
/// kernel that exits with a stream that breaks the contract ([`read_events`])
/// has crashed, whatever its exit code says. Progress is coalesced: when
/// `on_event` falls behind, progress within a stage is dropped, but never a
/// stage's first or last event ([`Progress::droppable`]), the outline or the
/// order.
///
/// The outline costs the run its parse time again inside `limits.timeout`
/// (about 5% of a large assembly's run): a run near its cap can time out
/// streamed and not plain.
///
/// # Errors
/// Only when the kernel cannot be spawned at all, as [`run`].
#[allow(clippy::too_many_arguments)]
pub fn run_streaming(
    kernel: &Path,
    input: &Path,
    deflection: Deflection,
    limits: Limits,
    mesh_out: Option<&Path>,
    topology_out: Option<&Path>,
    edges: bool,
    on_event: &mut dyn FnMut(Event),
) -> std::io::Result<Run> {
    let mut cmd = command(kernel, input, deflection, mesh_out, topology_out, edges);
    cmd.arg("--stream");
    let (tx, rx) = mpsc::sync_channel::<Event>(64);
    let s = supervise(
        cmd,
        limits,
        move |out| {
            let mut stage = None;
            read_events(BufReader::new(out), |e| match e {
                Event::Progress(p) if p.droppable(&mut stage) => {
                    let _ = tx.try_send(e);
                }
                e => {
                    let _ = tx.send(e);
                }
            })
        },
        || {
            while let Ok(e) = rx.try_recv() {
                on_event(e);
            }
        },
    )?;
    let (outcome, summary) = match (s.status, s.output) {
        // Exited, but not with the stream the contract promises.
        (Some(status), Err(_)) => (
            Outcome::Crashed {
                signal: status.signal(),
                code: status.code(),
            },
            None,
        ),
        // Killed: not complete, even when the summary made it out first.
        (None, _) => (outcome(None, s.over_memory, None), None),
        (status, stream) => {
            let summary = stream.ok();
            (outcome(status, s.over_memory, summary.as_ref()), summary)
        }
    };
    Ok(Run {
        outcome,
        summary,
        wall: s.wall,
    })
}

/// The kernel's command line for a run.
fn command(
    kernel: &Path,
    input: &Path,
    deflection: Deflection,
    mesh_out: Option<&Path>,
    topology_out: Option<&Path>,
    edges: bool,
) -> Command {
    let mut cmd = Command::new(kernel);
    // The nix product's wrapper sets it for the viewer's window and GPU
    // libraries; the sandboxed kernel needs none of them (#32 review).
    cmd.env_remove("LD_LIBRARY_PATH");
    cmd.arg(input)
        .arg("--linear-rel")
        .arg(deflection.linear_rel.to_string())
        .arg("--angular-deg")
        .arg(deflection.angular_deg.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(out) = mesh_out {
        cmd.arg("--mesh").arg(out);
    }
    if edges {
        // STEPVMSH v4: the B-rep edges too (#31).
        cmd.arg("--edges");
    }
    if let Some(out) = topology_out {
        cmd.arg("--topology").arg(out);
    }
    cmd
}

/// How a run ended, from how its process did and the summary it gave.
fn outcome(status: Option<ExitStatus>, over_memory: bool, summary: Option<&Summary>) -> Outcome {
    match status {
        None if over_memory => Outcome::MemoryCap,
        None => Outcome::Timeout,
        Some(s) => match s.code() {
            Some(0) => Outcome::Ok,
            // The OS refusing an allocation (std::bad_alloc in the kernel)
            // surfaces as a clean "out of memory" summary: a memory limit,
            // just the machine's rather than ours.
            Some(3) if summary.is_some_and(|s| s.error.as_deref() == Some("out of memory")) => {
                Outcome::MemoryCap
            }
            Some(3) => Outcome::Failed,
            code => Outcome::Crashed {
                signal: s.signal(),
                code,
            },
        },
    }
}

/// How a supervised kernel process ended.
struct Supervised<T> {
    /// `None`: killed, at the wall clock or the memory cap.
    status: Option<ExitStatus>,
    over_memory: bool,
    /// What `read` made of its stdout.
    output: T,
    wall: Duration,
}

/// Spawns `cmd` (stdout piped) and holds it to `limits`: killed past the wall
/// clock, or once its footprint passes the cap (polled every 5 ms).
///
/// `read` consumes its stdout on a thread of its own, so a chatty kernel can
/// never fill the pipe and deadlock against the wait loop. `tick` runs on
/// this thread every couple of milliseconds until `read` is done: the place
/// to take what `read` hands over as it goes.
fn supervise<T: Send + 'static>(
    mut cmd: Command,
    limits: Limits,
    read: impl FnOnce(ChildStdout) -> T + Send + 'static,
    mut tick: impl FnMut(),
) -> std::io::Result<Supervised<T>> {
    let start = Instant::now();
    // Killed and reaped on every way out, a panicking `tick` among them: the
    // kernel never outlives its supervisor's limits.
    struct Reap(std::process::Child);
    impl Drop for Reap {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Reap(cmd.spawn()?);
    let child = &mut child.0;
    let stdout = child.stdout.take().expect("stdout is piped");
    let reader = std::thread::spawn(move || read(stdout));

    let mut over_memory = false;
    let mut last_probe = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if start.elapsed() >= limits.timeout {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        if let Some(cap) = limits.memory
            && last_probe.elapsed() >= Duration::from_millis(5)
        {
            last_probe = Instant::now();
            if footprint(child.id()).is_some_and(|b| b > cap) {
                over_memory = true;
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
        tick();
        std::thread::sleep(Duration::from_millis(2));
    };
    let wall = start.elapsed();
    // The process is gone, so its stdout ends; `read` may still be handing
    // over what it had, and waits on `tick` to take it.
    while !reader.is_finished() {
        tick();
        std::thread::sleep(Duration::from_millis(1));
    }
    let output = match reader.join() {
        Ok(output) => output,
        Err(panic) => std::panic::resume_unwind(panic),
    };
    tick();
    Ok(Supervised {
        status,
        over_memory,
        output,
        wall,
    })
}

/// A STEP file's assembly outline (#54, `kernel/outline.h`): the tree and
/// names of its products, read from the product structure alone, so it
/// arrives in the time the file takes to parse rather than after the
/// transfer.
///
/// It is [`crate::topology::products`] of the tree `--topology` writes after
/// the full run: the same products, names and order, with a product the
/// transfer splits into unnamed bodies as one leaf. The geometry's tree
/// replaces it when it arrives. A multi-file assembly's products in other
/// files are leaves: only the given file is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outline {
    /// The roots, in file order; a leaf's `part` numbers the outline's
    /// leaves (products), not the mesh's parts.
    pub tree: Vec<crate::topology::Node>,
    /// How many leaves the tree has.
    pub leaves: usize,
}

/// Why there is no outline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineError(pub String);

impl std::fmt::Display for OutlineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for OutlineError {}

/// Reads the assembly outline of the STEP file `input` (`stepv-occt
/// --outline`), held to `limits` like a run.
///
/// # Errors
/// When `input` is not STEP, does not read, or the kernel cannot be spawned,
/// times out, passes the memory cap, crashes, or answers something that is
/// not a valid outline (leaves not numbered `0..leaves` in order).
pub fn outline(kernel: &Path, input: &Path, limits: Limits) -> Result<Outline, OutlineError> {
    let mut cmd = Command::new(kernel);
    cmd.env_remove("LD_LIBRARY_PATH");
    cmd.arg(input)
        .arg("--outline")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let s = supervise(
        cmd,
        limits,
        |mut out| {
            let mut text = String::new();
            let _ = out.read_to_string(&mut text);
            text
        },
        || {},
    )
    .map_err(|e| OutlineError(format!("cannot start kernel: {e}")))?;
    let Some(status) = s.status else {
        return Err(OutlineError(
            if s.over_memory {
                "memory cap exceeded"
            } else {
                "wall-clock cap exceeded"
            }
            .to_owned(),
        ));
    };
    match s.output.lines().next() {
        Some(line) => parse_outline(line.as_bytes()),
        None => Err(OutlineError(format!(
            "kernel answered no outline ({status})"
        ))),
    }
}

/// An outline as the kernel writes it (`kernel/outline.h`), validated.
fn parse_outline(json: &[u8]) -> Result<Outline, OutlineError> {
    #[derive(Deserialize)]
    struct Wire {
        format: Option<String>,
        version: Option<u32>,
        tree: Option<Vec<crate::topology::Node>>,
        leaves: Option<usize>,
        error: Option<String>,
    }
    let fail = |m: &str| Err(OutlineError(m.to_owned()));
    let Ok(wire) = serde_json::from_slice::<Wire>(json) else {
        return fail("kernel answered no outline");
    };
    if let Some(e) = wire.error {
        return Err(OutlineError(e));
    }
    let (Some("stepv-outline"), Some(1), Some(tree), Some(count)) =
        (wire.format.as_deref(), wire.version, wire.tree, wire.leaves)
    else {
        return fail("not a stepv-outline version 1");
    };
    // The leaves are numbered 0, 1, ... in tree order.
    fn leaves(n: &crate::topology::Node, out: &mut Vec<usize>) {
        match n {
            crate::topology::Node::Part { part, .. } => out.push(*part),
            crate::topology::Node::Assembly { children, .. } => {
                children.iter().for_each(|c| leaves(c, out));
            }
        }
    }
    let mut seen = Vec::new();
    tree.iter().for_each(|n| leaves(n, &mut seen));
    if !seen.iter().copied().eq(0..count) {
        return fail("outline leaves are not numbered in order");
    }
    Ok(Outline {
        tree,
        leaves: count,
    })
}

/// What a streamed run ([`run_streaming`]) reports before its summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A STEP file's [`Outline`], once the file is parsed: before the
    /// transfer, which is most of a large assembly's wait (#51).
    Tree(Outline),
    Progress(Progress),
}

/// How far a run is through one [`Stage`]: `done` of `total`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Progress {
    pub stage: Stage,
    pub done: u64,
    pub total: u64,
}

impl Progress {
    /// Whether a reader that has fallen behind may drop this event: not when
    /// it starts a stage (`stage` is the last one seen, and is updated) or
    /// finishes one, so a progress bar never sticks short of its end.
    #[must_use]
    pub fn droppable(&self, stage: &mut Option<Stage>) -> bool {
        let fresh = *stage != Some(self.stage);
        *stage = Some(self.stage);
        !fresh && self.done < self.total
    }

    /// `done / total`, in `0.0..=1.0`.
    #[must_use]
    pub fn fraction(&self) -> f64 {
        if self.total == 0 {
            1.0
        } else {
            self.done as f64 / self.total as f64
        }
    }
}

/// The stages a run reports progress through, in the order they come.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    /// Shapes from the parsed file: thousandths of OCCT's own progress.
    Transfer,
    /// Tessellation and its recovery ladder: two passes over the prototypes.
    Mesh,
}

/// The largest frame [`read_events`] accepts, checked before it allocates:
/// `STEPV_OCCT_MAX_FRAME` (`kernel/stepv_occt.h`), which the kernel holds
/// its frames to (an outline over it is left out, not sent).
pub const MAX_FRAME: u32 = 64 << 20;

/// A `--stream` that breaks its contract (`kernel/stepv_occt.h`). The kernel
/// writes it, so each is a crashed or misbehaving kernel, never bad input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamError {
    /// It ended without the summary frame, at a frame boundary.
    NoSummary,
    /// It ended partway through a frame.
    Truncated,
    BadMagic,
    BadKind(u8),
    TooLarge(u32),
    /// A frame whose payload is not what its kind promises.
    BadPayload(String),
    /// Bytes after the summary.
    TrailingBytes,
    Io(std::io::ErrorKind),
}

impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSummary => write!(f, "event stream ended without its summary"),
            Self::Truncated => write!(f, "event stream ended partway through a frame"),
            Self::BadMagic => write!(f, "not an event frame"),
            Self::BadKind(k) => write!(f, "unknown event kind {k}"),
            Self::TooLarge(n) => write!(f, "event frame of {n} bytes is over the cap"),
            Self::BadPayload(e) => write!(f, "malformed event: {e}"),
            Self::TrailingBytes => write!(f, "bytes after the summary"),
            Self::Io(k) => write!(f, "reading the event stream: {k}"),
        }
    }
}

impl std::error::Error for StreamError {}

/// Fills `buf` from `r` until it is full or `r` ends; returns how much it
/// filled.
fn fill(r: &mut impl Read, buf: &mut [u8]) -> Result<usize, StreamError> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(StreamError::Io(e.kind())),
        }
    }
    Ok(n)
}

/// Decodes a `stepv-occt --stream` stream (`kernel/stepv_occt.h`): passes
/// each event to `on_event` as its frame arrives and returns the summary
/// that ends it.
///
/// # Errors
/// See [`StreamError`]. The events before the error were passed on.
pub fn read_events(
    mut r: impl Read,
    mut on_event: impl FnMut(Event),
) -> Result<Summary, StreamError> {
    loop {
        let mut head = [0u8; 9];
        match fill(&mut r, &mut head)? {
            0 => return Err(StreamError::NoSummary),
            9 => {}
            _ => return Err(StreamError::Truncated),
        }
        if &head[..4] != b"SVEV" {
            return Err(StreamError::BadMagic);
        }
        let kind = head[4];
        let len = u32::from_le_bytes(head[5..9].try_into().expect("4 bytes"));
        if !(1..=3).contains(&kind) {
            return Err(StreamError::BadKind(kind));
        }
        if len > MAX_FRAME {
            return Err(StreamError::TooLarge(len));
        }
        let mut payload = vec![0; len as usize];
        if fill(&mut r, &mut payload)? < payload.len() {
            return Err(StreamError::Truncated);
        }
        let bad = |e: &dyn std::fmt::Display| StreamError::BadPayload(e.to_string());
        match kind {
            1 => on_event(Event::Tree(parse_outline(&payload).map_err(|e| bad(&e))?)),
            2 => {
                let p: Progress = serde_json::from_slice(&payload).map_err(|e| bad(&e))?;
                if p.done > p.total {
                    return Err(bad(&format_args!("progress {} of {}", p.done, p.total)));
                }
                on_event(Event::Progress(p));
            }
            _ => {
                let summary: Summary = serde_json::from_slice(&payload).map_err(|e| bad(&e))?;
                return match fill(&mut r, &mut [0])? {
                    0 => Ok(summary),
                    _ => Err(StreamError::TrailingBytes),
                };
            }
        }
    }
}

/// A process's current memory footprint in bytes, as the OS accounts it for
/// the user: `ri_phys_footprint` on macOS (what Activity Monitor shows), the
/// resident set on Linux. `None` when it cannot be read (the process exited).
#[must_use]
pub fn footprint(pid: u32) -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
        // SAFETY: `info` is a correctly sized, writable rusage_info_v2, which
        // is what flavor RUSAGE_INFO_V2 writes.
        let rc = unsafe {
            libc::proc_pid_rusage(
                pid as libc::c_int,
                libc::RUSAGE_INFO_V2,
                (&raw mut info).cast(),
            )
        };
        (rc == 0).then_some(info.ri_phys_footprint)
    }
    #[cfg(target_os = "linux")]
    {
        // /proc/<pid>/statm: size resident shared ... in pages.
        let statm = std::fs::read_to_string(format!("/proc/{pid}/statm")).ok()?;
        let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
        Some(pages * 4096)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = pid;
        None
    }
}

/// A malformed mesh file. The kernel wrote it, so any of these is a bug on
/// one side of the format, never bad user input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeshError {
    BadMagic,
    UnsupportedVersion(u32),
    /// A face-status or line-kind byte outside the contract.
    BadEnum(u8),
    Truncated,
    TrailingBytes(usize),
    /// A v4 edge polyline of fewer than two points.
    BadEdge,
}

impl std::fmt::Display for MeshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadMagic => write!(f, "not a STEPVMSH file"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported STEPVMSH version {v}"),
            Self::BadEnum(b) => write!(f, "invalid face-status or line-kind byte {b}"),
            Self::Truncated => write!(f, "STEPVMSH file is truncated"),
            Self::TrailingBytes(n) => write!(f, "{n} unexpected trailing bytes"),
            Self::BadEdge => write!(f, "an edge polyline with fewer than two points"),
        }
    }
}

impl std::error::Error for MeshError {}

struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], MeshError> {
        if self.0.len() < n {
            return Err(MeshError::Truncated);
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8, MeshError> {
        Ok(self.take(1)?[0])
    }

    fn enums<T>(&mut self, count: usize, f: fn(u8) -> Option<T>) -> Result<Vec<T>, MeshError> {
        self.take(count)?
            .iter()
            .map(|&b| f(b).ok_or(MeshError::BadEnum(b)))
            .collect()
    }

    fn u32(&mut self) -> Result<u32, MeshError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }

    fn f32(&mut self) -> Result<f32, MeshError> {
        Ok(f32::from_le_bytes(
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }

    fn f64(&mut self) -> Result<f64, MeshError> {
        Ok(f64::from_le_bytes(
            self.take(8)?.try_into().expect("8 bytes"),
        ))
    }

    /// `count` items of 4 bytes each. Sized against what remains BEFORE
    /// allocating, so a corrupt count cannot ask for gigabytes.
    fn vec4<T>(&mut self, count: usize, f: fn([u8; 4]) -> T) -> Result<Vec<T>, MeshError> {
        let bytes = self.take(count.checked_mul(4).ok_or(MeshError::Truncated)?)?;
        Ok(bytes
            .chunks_exact(4)
            .map(|c| f(c.try_into().expect("4 bytes")))
            .collect())
    }
}

const fn line_kind(b: u8) -> Option<LineKind> {
    match b {
        0 => Some(LineKind::Sketch),
        1 => Some(LineKind::MissingOutline),
        2 => Some(LineKind::Construction),
        _ => None,
    }
}

/// Decodes a `STEPVMSH` file into a [`Scene`].
///
/// # Errors
/// See [`MeshError`].
pub fn read_mesh(bytes: &[u8]) -> Result<Scene, MeshError> {
    let mut c = Cursor(bytes);
    if c.take(8).map_err(|_| MeshError::BadMagic)? != b"STEPVMSH" {
        return Err(MeshError::BadMagic);
    }
    let version = c.u32()?;
    if version != 3 && version != 4 {
        return Err(MeshError::UnsupportedVersion(version));
    }
    let mut b = [0.0; 6];
    for v in &mut b {
        *v = c.f64()?;
    }
    let bbox = BBox {
        min: [b[0], b[1], b[2]],
        max: [b[3], b[4], b[5]],
    };

    let part_count = c.u32()? as usize;
    // Every part is at least 33 bytes, so a count beyond that is corruption.
    if part_count > c.0.len() / 33 {
        return Err(MeshError::Truncated);
    }
    let mut parts = Vec::with_capacity(part_count);
    for _ in 0..part_count {
        let name_len = c.u32()? as usize;
        let name = c.take(name_len)?;
        let has_color = c.u8()? != 0;
        let (r, g, b) = (c.f32()?, c.f32()?, c.f32()?);
        let face_count = c.u32()? as usize;
        // Every face entry is 14 bytes; bound the count before allocating.
        if face_count > c.0.len() / 14 {
            return Err(MeshError::Truncated);
        }
        let mut faces = Vec::with_capacity(face_count);
        for _ in 0..face_count {
            let b = c.u8()?;
            let status = FaceStatus::from_u8(b).ok_or(MeshError::BadEnum(b))?;
            let has = c.u8()? != 0;
            let (r, g, b) = (c.f32()?, c.f32()?, c.f32()?);
            faces.push(Face {
                status,
                color: has.then_some(Color { r, g, b }),
            });
        }
        let vertices = c.u32()? as usize;
        let triangles = c.u32()? as usize;
        let mesh = Mesh {
            positions: c.vec4(vertices * 3, f32::from_le_bytes)?,
            normals: c.vec4(vertices * 3, f32::from_le_bytes)?,
            indices: c.vec4(triangles * 3, u32::from_le_bytes)?,
            face_ids: c.vec4(triangles, u32::from_le_bytes)?,
        };
        let segments = c.u32()? as usize;
        let lines = Lines {
            positions: c.vec4(segments * 6, f32::from_le_bytes)?,
            kinds: c.enums(segments, line_kind)?,
        };
        // v4: the B-rep edges (#31).
        let edges = if version == 4 {
            let n = c.u32()? as usize;
            // Each edge has 8 bytes of id and length: bound the count
            // before allocating (the points are bounded below).
            if n > c.0.len() / 8 {
                return Err(MeshError::Truncated);
            }
            let ids = c.vec4(n, u32::from_le_bytes)?;
            let lens = c.vec4(n, u32::from_le_bytes)?;
            if lens.iter().any(|&l| l < 2) {
                return Err(MeshError::BadEdge);
            }
            // Checked: on a 32-bit target a crafted file's lengths could
            // wrap the sum past the bound below.
            let total = lens
                .iter()
                .try_fold(0usize, |t, &l| t.checked_add(l as usize))
                .ok_or(MeshError::Truncated)?;
            if total > c.0.len() / 12 {
                return Err(MeshError::Truncated);
            }
            Edges {
                ids,
                lens,
                points: c.vec4(total * 3, f32::from_le_bytes)?,
            }
        } else {
            Edges::default()
        };
        parts.push(Part {
            name: (name_len > 0).then(|| String::from_utf8_lossy(name).into_owned()),
            color: has_color.then_some(Color { r, g, b }),
            mesh,
            faces,
            lines,
            edges,
        });
    }
    if !c.0.is_empty() {
        return Err(MeshError::TrailingBytes(c.0.len()));
    }
    Ok(Scene { bbox, parts })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One-triangle, one-segment file, built the way the kernel writes it.
    fn one_triangle(name: &str) -> Vec<u8> {
        let mut v = b"STEPVMSH".to_vec();
        v.extend(3u32.to_le_bytes());
        for x in [0.0f64, 0.0, 0.0, 1.0, 1.0, 0.0] {
            v.extend(x.to_le_bytes());
        }
        v.extend(1u32.to_le_bytes());
        v.extend((name.len() as u32).to_le_bytes());
        v.extend(name.as_bytes());
        v.push(1);
        for x in [0.5f32, 0.25, 1.0] {
            v.extend(x.to_le_bytes());
        }
        v.extend(2u32.to_le_bytes()); // faces
        v.extend([FaceStatus::Ok as u8, 0]);
        v.extend([0u8; 12]);
        v.extend([FaceStatus::Missing as u8, 1]);
        for x in [0.0f32, 1.0, 0.0] {
            v.extend(x.to_le_bytes());
        }
        v.extend(3u32.to_le_bytes());
        v.extend(1u32.to_le_bytes());
        for x in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            v.extend(x.to_le_bytes());
        }
        for x in [0.0f32, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0] {
            v.extend(x.to_le_bytes());
        }
        for i in [0u32, 1, 2, 0] {
            v.extend(i.to_le_bytes());
        }
        v.extend(1u32.to_le_bytes()); // segments
        for x in [0.0f32, 0.0, 0.0, 1.0, 1.0, 0.0] {
            v.extend(x.to_le_bytes());
        }
        v.push(LineKind::MissingOutline as u8);
        v
    }

    /// `one_triangle` as v4, with `edges` polylines (id, points) appended.
    fn with_edges(name: &str, edges: &[(u32, &[[f32; 3]])]) -> Vec<u8> {
        let mut v = one_triangle(name);
        v[8..12].copy_from_slice(&4u32.to_le_bytes());
        v.extend((edges.len() as u32).to_le_bytes());
        for (id, _) in edges {
            v.extend(id.to_le_bytes());
        }
        for (_, pts) in edges {
            v.extend((pts.len() as u32).to_le_bytes());
        }
        for (_, pts) in edges {
            for p in *pts {
                for x in p {
                    v.extend(x.to_le_bytes());
                }
            }
        }
        v
    }

    #[test]
    fn decodes_v4_edges() {
        let s = read_mesh(&with_edges(
            "bolt",
            &[
                (7, &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]),
                (2, &[[0.0; 3], [0.0, 1.0, 0.0], [0.0, 1.0, 1.0]]),
            ],
        ))
        .unwrap();
        let e = &s.parts[0].edges;
        assert_eq!(
            (e.ids.as_slice(), e.lens.as_slice()),
            (&[7, 2][..], &[2, 3][..])
        );
        assert_eq!(e.points.len(), 15);
        assert!(s.parts[0].is_well_formed());
        // v3 has none.
        assert_eq!(
            read_mesh(&one_triangle("bolt")).unwrap().parts[0]
                .edges
                .count(),
            0
        );
    }

    #[test]
    fn rejects_malformed_v4_edges() {
        // A one-point polyline.
        assert_eq!(
            read_mesh(&with_edges("b", &[(0, &[[0.0; 3]])])).unwrap_err(),
            MeshError::BadEdge
        );
        // Points cut off partway.
        let mut v = with_edges("b", &[(0, &[[0.0; 3], [1.0; 3]])]);
        v.truncate(v.len() - 5);
        assert_eq!(read_mesh(&v).unwrap_err(), MeshError::Truncated);
        // A huge edge count, and huge lengths: refused before allocating.
        let mut v = one_triangle("b");
        v[8..12].copy_from_slice(&4u32.to_le_bytes());
        v.extend(u32::MAX.to_le_bytes());
        assert_eq!(read_mesh(&v).unwrap_err(), MeshError::Truncated);
        let mut v = one_triangle("b");
        v[8..12].copy_from_slice(&4u32.to_le_bytes());
        v.extend(2u32.to_le_bytes());
        v.extend([0u8; 8]); // ids
        v.extend(u32::MAX.to_le_bytes());
        v.extend(u32::MAX.to_le_bytes());
        v.extend([0u8; 64]);
        assert_eq!(read_mesh(&v).unwrap_err(), MeshError::Truncated);
    }

    #[test]
    fn decodes_a_well_formed_file() {
        let scene = read_mesh(&one_triangle("bolt")).unwrap();
        assert_eq!(scene.parts.len(), 1);
        let p = &scene.parts[0];
        assert_eq!(p.name.as_deref(), Some("bolt"));
        assert_eq!(
            p.color,
            Some(Color {
                r: 0.5,
                g: 0.25,
                b: 1.0
            })
        );
        assert!(p.is_well_formed());
        assert_eq!(
            p.faces,
            [
                Face::plain(FaceStatus::Ok),
                Face {
                    status: FaceStatus::Missing,
                    color: Some(Color {
                        r: 0.0,
                        g: 1.0,
                        b: 0.0
                    })
                }
            ]
        );
        assert_eq!(p.lines.kinds, [LineKind::MissingOutline]);
        assert_eq!(scene.worst_face(), Some(FaceStatus::Missing));
        assert_eq!(scene.triangle_count(), 1);
        assert_eq!(scene.segment_count(), 1);
        assert!((scene.bbox.diagonal() - 2f64.sqrt()).abs() < 1e-12);
    }

    #[test]
    fn empty_name_decodes_as_none() {
        assert_eq!(read_mesh(&one_triangle("")).unwrap().parts[0].name, None);
    }

    #[test]
    fn every_truncation_is_an_error_not_a_panic() {
        let full = one_triangle("bolt");
        for n in 0..full.len() {
            assert!(
                read_mesh(&full[..n]).is_err(),
                "prefix of {n} bytes decoded"
            );
        }
    }

    #[test]
    fn out_of_contract_status_byte_is_rejected() {
        let mut v = one_triangle("bolt");
        // magic 8 + version 4 + bbox 48 + count 4 + name 4+4 + colour 1+12 + faces 4
        v[89] = 9;
        assert_eq!(read_mesh(&v).unwrap_err(), MeshError::BadEnum(9));
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        let mut v = one_triangle("bolt");
        v.push(0);
        assert_eq!(read_mesh(&v).unwrap_err(), MeshError::TrailingBytes(1));
    }

    fn frame(kind: u8, payload: &[u8]) -> Vec<u8> {
        let mut v = b"SVEV".to_vec();
        v.push(kind);
        v.extend((payload.len() as u32).to_le_bytes());
        v.extend(payload);
        v
    }

    const SUMMARY: &str = r#"{"ok":true,"stage":"done","error":null,"format":"step",
        "bbox":[0,0,0,1,1,1],"diagonal":1.7,"linear_abs":0.0017,"parts":1,
        "parts_named":1,"parts_colored":0,"parts_face_colored":0,"prototypes":1,
        "prototypes_mesh_failed":0,"faces":6,"faces_unmeshed":0,"faces_remeshed":0,
        "faces_healed":0,"faces_refined":0,"faces_coarse":0,"faces_degenerate":0,
        "faces_approx":0,"faces_missing":0,"sketch_parts":0,"construction_parts":0,
        "vertices":24,"triangles":12,"segments":0,"t_read_ms":1,"t_transfer_ms":1,
        "t_mesh_ms":1,"t_extract_ms":1,"peak_rss_bytes":1,"sandbox":"none"}"#;

    /// An outline, two progress events and the summary.
    fn a_stream() -> Vec<u8> {
        let mut v = frame(
            1,
            br#"{"format":"stepv-outline","version":1,"tree":[{"name":"bolt","part":0}],"leaves":1}"#,
        );
        v.extend(frame(2, br#"{"stage":"transfer","done":0,"total":1000}"#));
        v.extend(frame(2, br#"{"stage":"mesh","done":2,"total":2}"#));
        v.extend(frame(3, SUMMARY.as_bytes()));
        v
    }

    #[test]
    fn decodes_an_event_stream() {
        let mut events = Vec::new();
        let summary = read_events(a_stream().as_slice(), |e| events.push(e)).unwrap();
        assert_eq!(summary.triangles, 12);
        assert_eq!(summary.sandbox.as_deref(), Some("none"));
        let [Event::Tree(tree), Event::Progress(a), Event::Progress(b)] = events.as_slice() else {
            panic!("{events:?}");
        };
        assert_eq!(tree.leaves, 1);
        assert_eq!((a.stage, a.done, a.total), (Stage::Transfer, 0, 1000));
        assert_eq!((b.stage, b.fraction()), (Stage::Mesh, 1.0));
        assert!(Stage::Transfer < Stage::Mesh);
    }

    #[test]
    fn every_truncated_stream_is_an_error() {
        let full = a_stream();
        for n in 0..full.len() {
            let r = read_events(&full[..n], |_| {});
            assert!(r.is_err(), "prefix of {n} bytes decoded");
        }
        // Cut at a frame boundary: no summary; inside a frame: truncated.
        let first = frame(
            1,
            br#"{"format":"stepv-outline","version":1,"tree":[{"name":"bolt","part":0}],"leaves":1}"#,
        )
        .len();
        assert_eq!(
            read_events(&full[..first], |_| {}).unwrap_err(),
            StreamError::NoSummary
        );
        assert_eq!(
            read_events(&full[..first + 3], |_| {}).unwrap_err(),
            StreamError::Truncated
        );
    }

    #[test]
    fn a_stage_s_first_and_last_progress_are_never_dropped() {
        let p = |stage, done, total| Progress { stage, done, total };
        let mut stage = None;
        let kept: Vec<bool> = [
            p(Stage::Transfer, 0, 1000),
            p(Stage::Transfer, 400, 1000),
            p(Stage::Transfer, 1000, 1000),
            p(Stage::Mesh, 0, 4),
            p(Stage::Mesh, 2, 4),
            p(Stage::Mesh, 4, 4),
        ]
        .iter()
        .map(|e| !e.droppable(&mut stage))
        .collect();
        assert_eq!(kept, [true, false, true, true, false, true]);
    }

    #[test]
    fn a_broken_frame_is_rejected() {
        let err = |v: Vec<u8>| read_events(v.as_slice(), |_| {}).unwrap_err();
        assert_eq!(err(b"STEPVMSH and more".to_vec()), StreamError::BadMagic);
        assert_eq!(err(frame(9, b"{}")), StreamError::BadKind(9));
        assert_eq!(err(frame(0, b"{}")), StreamError::BadKind(0));
        // Refused on its header alone: nothing that size is allocated.
        let mut huge = b"SVEV\x02".to_vec();
        huge.extend(u32::MAX.to_le_bytes());
        assert_eq!(err(huge), StreamError::TooLarge(u32::MAX));
        assert!(matches!(
            err(frame(2, br#"{"stage":"mesh","done":3,"total":2}"#)),
            StreamError::BadPayload(_)
        ));
        assert!(matches!(
            err(frame(2, br#"{"stage":"bake","done":0,"total":2}"#)),
            StreamError::BadPayload(_)
        ));
        assert!(matches!(
            err(frame(1, br#"{"format":"stepv-outline","version":1,"tree":[{"name":"a","part":1}],"leaves":1}"#)),
            StreamError::BadPayload(_)
        ));
        let mut v = frame(3, SUMMARY.as_bytes());
        v.push(0);
        assert_eq!(err(v), StreamError::TrailingBytes);
    }

    #[test]
    fn absurd_part_count_does_not_allocate() {
        let mut v = one_triangle("bolt");
        v[60..64].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(read_mesh(&v).unwrap_err(), MeshError::Truncated);
    }
}
