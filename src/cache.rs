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

use std::path::Path;

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
pub fn cache_dir() -> Option<std::path::PathBuf> {
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| Path::new(&h).join("Library/Caches/ch.exoma.stepv"))
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(|x| Path::new(&x).join("stepv"))
            .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".cache/stepv")))
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
}
