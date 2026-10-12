//! Content-addressed cache keys for previews and thumbnails.
//!
//! A previewer is asked for the same file over and over — Finder re-requests a
//! thumbnail on every window that shows the folder — so the cache is not an
//! optimisation, it is the difference between a snappy previewer and a fan
//! event. See `plan.md` §4.
//!
//! The key deliberately does NOT hash file CONTENTS. Reading a 400 MB STEP
//! file to decide whether to read a 400 MB STEP file is self-defeating;
//! `(len, mtime)` is what every thumbnailer on both platforms uses and it is
//! wrong only for a file rewritten within one mtime tick at identical length.
//!
//! An entry is only ever a COMPLETE result (#62): [`store`] frames it with a
//! version and a checksum and puts it in place atomically ([`write_atomic`]),
//! and [`load`] treats anything else (truncated, corrupt, another version) as
//! a miss. A run killed partway leaves no entry.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Cache key inputs. Every field that can change the OUTPUT must be in here,
/// which is why the deflection and the output kind are part of the key and not
/// just the file identity: the same file previewed at thumbnail and at preview
/// quality are two different artifacts.
///
/// `PartialEq` but not `Eq`: the deflection fields are `f64`. That is not a
/// derive accident — it is exactly why `quantised_linear` exists, and
/// why these values are never compared directly on the hashing path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyInputs<'a> {
    /// Canonical path, as bytes. Not hashed for identity (the file may move)
    /// but included so two hard links do not collide in a user-visible way.
    pub path: &'a [u8],
    pub len: u64,
    /// Modification time as whole nanoseconds since the Unix epoch.
    pub mtime_nanos: i128,
    /// Linear deflection fraction, quantised — see `quantised_linear`.
    pub linear_rel: f64,
    pub angular_deg: f64,
    /// Discriminates `.png` from `.glb` from raw buffers.
    pub output: Output,
    /// Every remaining output-affecting option, packed by the caller (PNG
    /// edge length, construction-curve visibility, …). Opaque here on
    /// purpose: the cache only needs "same options or not".
    pub variant: u64,
}

/// What was produced, since it changes the cached bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    ThumbnailPng,
    Glb,
    Buffers,
}

impl Output {
    const fn tag(self) -> u8 {
        match self {
            Self::ThumbnailPng => 0,
            Self::Glb => 1,
            Self::Buffers => 2,
        }
    }
}

impl KeyInputs<'_> {
    /// Deflection quantised to 1e-6 before hashing.
    ///
    /// Floats in a cache key are a footgun: `0.001` arrived at by two
    /// different code paths can differ in the last bit and silently halve the
    /// hit rate. Quantising makes the key stable at a resolution far finer
    /// than any visible difference.
    fn quantised_linear(&self) -> i64 {
        (self.linear_rel * 1e6).round() as i64
    }

    fn quantised_angular(&self) -> i64 {
        (self.angular_deg * 1e6).round() as i64
    }
}

/// Hash the inputs into a lowercase hex cache key.
///
/// Field boundaries are length-prefixed so no two distinct input sets can
/// serialise to the same byte string (the classic concatenation ambiguity:
/// `"ab" + "c"` and `"a" + "bc"`).
#[must_use]
pub fn key(inputs: &KeyInputs<'_>) -> String {
    let mut h = blake3::Hasher::new();
    h.update(&(inputs.path.len() as u64).to_le_bytes());
    h.update(inputs.path);
    h.update(&inputs.len.to_le_bytes());
    h.update(&inputs.mtime_nanos.to_le_bytes());
    h.update(&inputs.quantised_linear().to_le_bytes());
    h.update(&inputs.quantised_angular().to_le_bytes());
    h.update(&[inputs.output.tag()]);
    h.update(&inputs.variant.to_le_bytes());
    h.finalize().to_hex().to_string()
}

/// Per-platform cache directory for `stepv`.
///
/// Returns `None` rather than falling back to a temp dir: a previewer that
/// cannot cache should run uncached and say so, not scatter files somewhere
/// the user never agreed to.
#[must_use]
pub fn cache_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| Path::new(&h).join("Library/Caches/ch.exoma.stepv"))
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(|x| Path::new(&x).join("stepv"))
            .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".cache/stepv")))
    }
}

/// The version of [`store`]'s entry framing. A reader treats an entry of any
/// other version as a miss, so bumping it invalidates every cached result.
pub const ENTRY_VERSION: u32 = 1;

const ENTRY_MAGIC: &[u8; 8] = b"STEPVCE\0";
/// Magic, version (u32 LE), payload length (u64 LE).
const ENTRY_HEAD: usize = 8 + 4 + 8;
/// The payload's blake3 hash, after it: the completeness tag.
const ENTRY_TAIL: usize = 32;

/// How old a `.part` file in the cache directory must be before [`store`]
/// sweeps it. A writer finishes in seconds; one this old was killed between
/// its write and its rename.
pub const STALE_PART_AGE: Duration = Duration::from_secs(3600);

/// Stores `payload` as the complete cache entry `path`: framed with
/// [`ENTRY_VERSION`], its length and its hash, and written with
/// [`write_atomic`]. Stale `.part` files beside it are swept first.
///
/// The hash catches truncation and corruption, not forgery: anyone who can
/// write the cache directory (the user's own) can write a valid entry.
///
/// # Errors
/// When the directory cannot be created or the entry written.
pub fn store(path: &Path, payload: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        sweep_parts(dir, STALE_PART_AGE);
    }
    let mut entry = Vec::with_capacity(ENTRY_HEAD + payload.len() + ENTRY_TAIL);
    entry.extend_from_slice(ENTRY_MAGIC);
    entry.extend_from_slice(&ENTRY_VERSION.to_le_bytes());
    entry.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    entry.extend_from_slice(payload);
    entry.extend_from_slice(blake3::hash(payload).as_bytes());
    write_atomic(path, &entry)
}

/// The payload of the complete cache entry `path`, or `None` for a miss: no
/// file, or one that is truncated, corrupt, not an entry, or of another
/// [`ENTRY_VERSION`]. The caller regenerates it and [`store`]s it over.
#[must_use]
pub fn load(path: &Path) -> Option<Vec<u8>> {
    decode(&std::fs::read(path).ok()?).map(<[u8]>::to_vec)
}

/// The payload of a framed entry, if it is complete and of this version.
fn decode(entry: &[u8]) -> Option<&[u8]> {
    let (head, rest) = entry.split_at_checked(ENTRY_HEAD)?;
    let version = u32::from_le_bytes(head[8..12].try_into().ok()?);
    let len = usize::try_from(u64::from_le_bytes(head[12..].try_into().ok()?)).ok()?;
    if &head[..8] != ENTRY_MAGIC
        || version != ENTRY_VERSION
        || rest.len() != len.checked_add(ENTRY_TAIL)?
        || blake3::hash(&rest[..len]).as_bytes() != &rest[len..]
    {
        return None;
    }
    Some(&rest[..len])
}

/// Writes `bytes` to `path` so that a reader (Finder, a concurrent stepv)
/// sees either the old file or the whole new one, never part of one: a
/// sibling `<name>.<pid>.<nonce>.part`, `fsync`ed, renamed over `path`, then
/// the directory `fsync`ed (Unix) so the rename itself survives a crash.
///
/// The temporary file is in `path`'s own directory because `rename` is only
/// atomic within one filesystem. Each writer, thread or process, has its own,
/// so concurrent writers of one path never mix: the last rename wins whole.
///
/// A failed directory `fsync` after the rename is not an error: the entry is
/// complete and in place, and at worst a crash loses it and the next run
/// regenerates it.
///
/// # Errors
/// When the directory cannot be created, or a write, the file's sync or the
/// rename fails; the temporary file is then removed.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static NONCE: AtomicU64 = AtomicU64::new(0);
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let n = NONCE.fetch_add(1, Ordering::Relaxed);
    name.push(format!(".{}.{n}-{nanos}.part", std::process::id()));
    let tmp = dir.join(name);
    let written = (|| {
        let mut f = std::fs::File::create_new(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written?;
    // A directory opens read-only on Unix, and its fsync persists the rename.
    #[cfg(unix)]
    let _ = std::fs::File::open(dir).and_then(|d| d.sync_all());
    Ok(())
}

/// Removes the `.part` files in `dir` older than `age`: writers killed
/// between their write and their rename. Best effort: a sweep that fails
/// leaves them for the next one.
///
/// Age is by wall clock, so a writer stopped (suspended, or behind a clock
/// jump) for longer than `age` loses its `.part`. That is harmless: its
/// rename fails with `ENOENT` and nothing is cached, nothing corrupted.
pub fn sweep_parts(dir: &Path, age: Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        let stale = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|a| a >= age);
        if stale && path.extension().is_some_and(|x| x == "part") {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> KeyInputs<'static> {
        KeyInputs {
            path: b"/models/bracket.step",
            len: 4096,
            mtime_nanos: 1_700_000_000_000_000_000,
            linear_rel: 0.001,
            angular_deg: 20.0,
            output: Output::ThumbnailPng,
            variant: 512,
        }
    }

    #[test]
    fn key_is_stable() {
        assert_eq!(key(&base()), key(&base()));
    }

    #[test]
    fn mtime_change_changes_key() {
        let mut b = base();
        b.mtime_nanos += 1;
        assert_ne!(key(&base()), key(&b));
    }

    #[test]
    fn deflection_change_changes_key() {
        let mut b = base();
        b.linear_rel = 0.005;
        assert_ne!(key(&base()), key(&b));
    }

    #[test]
    fn output_kind_change_changes_key() {
        let mut b = base();
        b.output = Output::Glb;
        assert_ne!(key(&base()), key(&b));
    }

    #[test]
    fn variant_change_changes_key() {
        let mut b = base();
        b.variant = 256;
        assert_ne!(key(&base()), key(&b));
    }

    #[test]
    fn float_noise_below_quantisation_does_not_change_key() {
        let mut b = base();
        b.linear_rel = 0.001 + 1e-12;
        assert_eq!(key(&base()), key(&b));
    }

    #[test]
    fn length_prefix_prevents_field_boundary_collision() {
        // Without the length prefix on `path`, a shorter path plus a shifted
        // `len` could serialise identically. Guard the property directly.
        let a = KeyInputs {
            path: b"/ab",
            ..base()
        };
        let b = KeyInputs {
            path: b"/a",
            ..base()
        };
        assert_ne!(key(&a), key(&b));
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("stepv-cache-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Each writer's payload, 64 KiB of its own byte.
    const RACE_LEN: usize = 64 << 10;

    /// Many small writes, so a reader often lands inside one: a mixed or
    /// short entry shows as a frame that does not verify.
    fn write_many(path: &Path, byte: u8) {
        for _ in 0..100 {
            store(path, &vec![byte; RACE_LEN]).unwrap();
        }
    }

    /// One writer process for `concurrent_writers_never_tear_an_entry`:
    /// ignored when run, re-executed by it with the entry's path.
    #[test]
    #[ignore = "a helper process, run by concurrent_writers_never_tear_an_entry"]
    fn writer_process() {
        if let (Some(p), Ok(b)) = (
            std::env::var_os("STEPV_TEST_CACHE_ENTRY"),
            std::env::var("STEPV_TEST_CACHE_BYTE"),
        ) {
            write_many(Path::new(&p), b.parse().unwrap());
        }
    }

    #[test]
    fn concurrent_writers_never_tear_an_entry() {
        let dir = scratch("race");
        let path = dir.join("k.png");
        // Processes, so each has its own pid in the temporary name, and
        // threads within this one, which share it.
        let exe = std::env::current_exe().unwrap();
        let mut procs: Vec<_> = (0u8..4)
            .map(|b| {
                std::process::Command::new(&exe)
                    .args(["--ignored", "--exact", "cache::tests::writer_process"])
                    .env("STEPV_TEST_CACHE_ENTRY", &path)
                    .env("STEPV_TEST_CACHE_BYTE", b.to_string())
                    .stdout(std::process::Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .collect();
        let threads: Vec<_> = (4u8..8)
            .map(|b| {
                let path = path.clone();
                std::thread::spawn(move || write_many(&path, b))
            })
            .collect();
        let (mut whole, mut torn) = (0, 0);
        let mut running = true;
        while running {
            running = !threads.iter().all(std::thread::JoinHandle::is_finished)
                || procs.iter_mut().any(|p| p.try_wait().unwrap().is_none());
            // The raw bytes, not `load`, which would hide a torn entry as a miss.
            let Ok(raw) = std::fs::read(&path) else {
                continue;
            };
            match decode(&raw) {
                Some(p) if p.len() == RACE_LEN && p.iter().all(|&x| x == p[0]) => whole += 1,
                _ => torn += 1,
            }
        }
        threads.into_iter().for_each(|t| t.join().unwrap());
        for mut p in procs {
            assert!(p.wait().unwrap().success());
        }
        assert_eq!(torn, 0, "{torn} torn reads ({whole} whole)");
        assert!(whole > 0);
        // Every temporary file was renamed away.
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(left.len(), 1, "{left:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_a_complete_entry_of_this_version_loads() {
        let dir = scratch("load");
        let path = dir.join("k.msh");
        store(&path, b"payload").unwrap();
        assert_eq!(load(&path).as_deref(), Some(&b"payload"[..]));
        let whole = std::fs::read(&path).unwrap();
        for cut in [0, 5, ENTRY_HEAD, whole.len() - 1] {
            std::fs::write(&path, &whole[..cut]).unwrap();
            assert_eq!(load(&path), None, "truncated at {cut}");
        }
        let mut old = whole.clone();
        old[8..12].copy_from_slice(&(ENTRY_VERSION - 1).to_le_bytes());
        std::fs::write(&path, &old).unwrap();
        assert_eq!(load(&path), None, "old version");
        let mut flipped = whole;
        flipped[ENTRY_HEAD] ^= 1;
        std::fs::write(&path, &flipped).unwrap();
        assert_eq!(load(&path), None, "corrupt payload");
        // A raw output, as caches before #62 held.
        std::fs::write(&path, b"\x89PNG\r\n\x1a\n").unwrap();
        assert_eq!(load(&path), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn store_sweeps_stale_parts_only() {
        let dir = scratch("sweep");
        let (stale, fresh) = (dir.join("a.png.1.0-0.part"), dir.join("b.png.2.0-0.part"));
        std::fs::write(&stale, b"x").unwrap();
        std::fs::write(&fresh, b"x").unwrap();
        let old = SystemTime::now() - STALE_PART_AGE - Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(old)
            .unwrap();
        store(&dir.join("c.png"), b"c").unwrap();
        assert!(!stale.exists(), "a stale .part survived the sweep");
        assert!(fresh.exists(), "a writer's live .part was swept");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
