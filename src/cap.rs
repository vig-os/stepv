//! The kernel memory cap (#83): one sampler, shared by the CLI's supervised
//! kernel runs ([`crate::occt`], [`crate::measure`]) and the `stepv-cap`
//! wrapper that every other kernel launch goes through (`src/bin/stepv-cap.rs`).
//!
//! The cap is on the *footprint*, polled every [`POLL`], and enforced by
//! killing: not `RLIMIT_AS`, for the reasons on [`crate::occt::Limits`].

use std::time::{Duration, Instant};

/// How often the footprint is sampled.
pub const POLL: Duration = Duration::from_millis(5);

/// `stepv-cap`'s exit code when it killed its command at the cap: outside
/// the CLI's codes, `125`-`127` (its own failures, as `timeout(1)` has
/// them) and `128 + signal`.
pub const EXIT_OVER_CAP: u8 = 86;

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

/// The footprint of `root` and every live descendant of it, summed, and the
/// pids it counted (`root` first). `None` when `root` itself has exited.
///
/// A descendant that leaves the tree (its parent died and it was reparented)
/// is no longer counted: what `stepv-cap` wraps is a test or a kernel, and
/// neither daemonizes.
#[must_use]
pub fn tree_footprint(root: u32) -> Option<(u64, Vec<u32>)> {
    let mut total = footprint(root)?;
    let mut pids = vec![root];
    let mut i = 0;
    while i < pids.len() {
        for c in children(pids[i]) {
            if let Some(b) = footprint(c) {
                total += b;
                pids.push(c);
            }
        }
        i += 1;
    }
    Some((total, pids))
}

/// The direct children of `pid`.
fn children(pid: u32) -> Vec<u32> {
    #[cfg(target_os = "macos")]
    {
        // libproc.h's PROC_PPID_ONLY; not in the libc crate.
        const PROC_PPID_ONLY: u32 = 6;
        let mut buf = vec![0 as libc::pid_t; 256];
        loop {
            let size = libc::c_int::try_from(buf.len() * size_of::<libc::pid_t>())
                .unwrap_or(libc::c_int::MAX);
            // SAFETY: `buf` is writable for `size` bytes; the call writes at
            // most that many and returns how many it wrote.
            let n =
                unsafe { libc::proc_listpids(PROC_PPID_ONLY, pid, buf.as_mut_ptr().cast(), size) };
            let Ok(n) = usize::try_from(n) else {
                return Vec::new();
            };
            let n = n / size_of::<libc::pid_t>();
            // A full buffer may have been cut short: ask again with more room.
            if n < buf.len() {
                return buf[..n]
                    .iter()
                    .filter_map(|&p| u32::try_from(p).ok())
                    .filter(|&p| p != 0)
                    .collect();
            }
            buf.resize(buf.len() * 2, 0);
        }
    }
    #[cfg(target_os = "linux")]
    {
        // Every thread's own list (CONFIG_PROC_CHILDREN, checked by
        // `tree_supported`): a child belongs to the thread that forked it.
        let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
            return Vec::new();
        };
        tasks
            .flatten()
            .filter_map(|t| std::fs::read_to_string(t.path().join("children")).ok())
            .flat_map(|s| {
                s.split_whitespace()
                    .filter_map(|p| p.parse().ok())
                    .collect::<Vec<_>>()
            })
            .collect()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = pid;
        Vec::new()
    }
}

/// Whether this OS lets [`tree_footprint`] see descendants. On Linux that
/// needs `/proc/<pid>/task/<tid>/children` (`CONFIG_PROC_CHILDREN`); without
/// it a capped tree would silently count only its root.
#[must_use]
pub fn tree_supported() -> bool {
    #[cfg(target_os = "macos")]
    {
        true
    }
    #[cfg(target_os = "linux")]
    {
        std::path::Path::new(&format!("/proc/self/task/{}/children", std::process::id())).exists()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        false
    }
}

/// Holds a running process to a footprint cap: sampled at most every
/// [`POLL`], the peak remembered.
#[derive(Debug)]
pub struct Sampler {
    cap: Option<u64>,
    peak: u64,
    last: Instant,
}

impl Sampler {
    /// A sampler for a cap of `cap` bytes (`None`: never over, never
    /// sampled). The first sample is due one [`POLL`] from now.
    #[must_use]
    pub fn new(cap: Option<u64>) -> Self {
        Self {
            cap,
            peak: 0,
            last: Instant::now(),
        }
    }

    /// Whether the process is over the cap, sampling it with `measure` if a
    /// sample is due. `measure` returns the footprint in bytes, `None` when
    /// it cannot be read (the process exited): never over.
    pub fn over(&mut self, measure: impl FnOnce() -> Option<u64>) -> bool {
        let Some(cap) = self.cap else {
            return false;
        };
        if self.last.elapsed() < POLL {
            return false;
        }
        self.last = Instant::now();
        let Some(bytes) = measure() else {
            return false;
        };
        self.peak = self.peak.max(bytes);
        bytes > cap
    }

    /// The largest footprint sampled so far, in bytes.
    #[must_use]
    pub fn peak(&self) -> u64 {
        self.peak
    }
}
