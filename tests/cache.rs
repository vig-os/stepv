//! The cache (#62) end to end: an entry that is truncated or of another
//! version is regenerated. Runs the real kernel against `tests/data/`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use stepv::{cache, occt};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

/// A private HOME, removed on drop.
struct Home(PathBuf);

impl Home {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("stepv-cachetest-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }

    /// What `cache::cache_dir` resolves to under this HOME.
    fn cache_dir(&self) -> PathBuf {
        if cfg!(target_os = "macos") {
            self.0.join("Library/Caches/ch.exoma.stepv")
        } else {
            self.0.join("xdg/stepv")
        }
    }

    /// The cache's files with extension `ext`.
    fn entries(&self, ext: &str) -> Vec<PathBuf> {
        std::fs::read_dir(self.cache_dir())
            .map(|d| {
                d.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x == ext))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Runs stepv, returning its exit code and JSON line.
    fn stepv(&self, args: &[&str], env: &[(&str, &str)]) -> (i32, Value) {
        let out = Command::new(env!("CARGO_BIN_EXE_stepv"))
            .args(args)
            .env("HOME", &self.0)
            .env("XDG_CACHE_HOME", self.0.join("xdg"))
            .envs(env.iter().copied())
            .output()
            .expect("spawn stepv");
        let json = String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .and_then(|l| serde_json::from_str(l).ok())
            .unwrap_or(Value::Null);
        (out.status.code().unwrap_or(-1), json)
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn kernel() -> PathBuf {
    let k = occt::kernel_path();
    assert!(
        k.is_file(),
        "kernel not found at {}: run `just kernel`",
        k.display()
    );
    k
}

#[test]
fn a_truncated_or_old_version_entry_is_regenerated() {
    kernel();
    let h = Home::new("regen");
    let input = data("assembly.step");
    let out = h.0.join("o.png");
    let args = [
        input.to_str().unwrap(),
        "--png",
        out.to_str().unwrap(),
        "--size",
        "64",
    ];
    let (code, j) = h.stepv(&args, &[]);
    assert_eq!((code, &j["cached"]), (0, &Value::Bool(false)), "{j}");
    let entries = h.entries("png");
    let [entry] = &entries[..] else {
        panic!("not one entry: {entries:?}");
    };
    let whole = std::fs::read(entry).unwrap();
    let good = cache::load(entry).expect("the run cached a complete entry");
    assert_eq!(good, std::fs::read(&out).unwrap());

    let mut old = whole.clone();
    old[8..12].copy_from_slice(&(cache::ENTRY_VERSION + 1).to_le_bytes());
    for (what, planted) in [
        ("truncated", &whole[..whole.len() / 2]),
        ("old version", &old[..]),
    ] {
        std::fs::write(entry, planted).unwrap();
        let (code, j) = h.stepv(&args, &[]);
        assert_eq!(code, 0, "{what}: {j}");
        assert_eq!(j["cached"], false, "{what} was served: {j}");
        assert_eq!(
            cache::load(entry).as_ref(),
            Some(&good),
            "{what} not regenerated"
        );
        assert_eq!(std::fs::read(&out).unwrap(), good);
    }
    assert_eq!(h.stepv(&args, &[]).1["cached"], true);
}
