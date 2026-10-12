//! The CLI's progress line (#63): on stderr when it is a terminal, through a
//! real pseudo-terminal, and nothing when it is not; stdout's JSON line is
//! the same either way. Runs the real kernel against `tests/data/`.

use std::io::Read as _;
use std::os::fd::{FromRawFd as _, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;
use stepv::{cache, occt};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
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

/// A private HOME, removed on drop.
struct Home(PathBuf);

impl Home {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("stepv-progress-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }

    fn cache_dir(&self) -> PathBuf {
        if cfg!(target_os = "macos") {
            self.0.join("Library/Caches/ch.exoma.stepv")
        } else {
            self.0.join("xdg/stepv")
        }
    }

    fn command(&self, args: &[&str], env: &[(&str, &str)]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_stepv"));
        cmd.args(args)
            .env("HOME", &self.0)
            .env("XDG_CACHE_HOME", self.0.join("xdg"))
            .envs(env.iter().copied())
            .stdout(Stdio::piped());
        cmd
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The first line of `stdout` as JSON.
fn json(stdout: &[u8]) -> Value {
    let text = String::from_utf8_lossy(stdout);
    assert_eq!(text.lines().count(), 1, "stdout is not one line: {text:?}");
    serde_json::from_str(text.trim_end()).expect("stdout is JSON")
}

/// Runs `cmd` with stderr on a pseudo-terminal: its exit code, its stdout
/// JSON, and everything it wrote to the terminal.
fn on_a_tty(mut cmd: Command) -> (i32, Value, String) {
    let (mut master, mut slave) = (0, 0);
    // SAFETY: openpty fills the two descriptors; no name, termios or size.
    let rc = unsafe {
        libc::openpty(
            &raw mut master,
            &raw mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(rc, 0, "openpty: {}", std::io::Error::last_os_error());
    // SAFETY: both are fresh descriptors this function now owns.
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
    let child = cmd.stderr(slave).spawn().expect("spawn stepv");
    // The command keeps its copy of the slave until dropped. Once it is, the
    // child holds the only one, and the terminal ends with it: a read then
    // returns 0 (macOS) or EIO (Linux).
    drop(cmd);
    let reader = std::thread::spawn(move || {
        let mut f = std::fs::File::from(master);
        let (mut all, mut buf) = (Vec::new(), [0u8; 4096]);
        while let Ok(n @ 1..) = f.read(&mut buf) {
            all.extend_from_slice(&buf[..n]);
        }
        String::from_utf8_lossy(&all).into_owned()
    });
    let out = child.wait_with_output().unwrap();
    let tty = reader.join().unwrap();
    (out.status.code().unwrap_or(-1), json(&out.stdout), tty)
}

fn png_args<'a>(input: &'a Path, out: &'a Path) -> Vec<&'a str> {
    vec![
        input.to_str().unwrap(),
        "--png",
        out.to_str().unwrap(),
        "--size",
        "64",
        "--no-cache",
    ]
}

#[test]
fn a_terminal_sees_the_parts_and_each_stage_and_stdout_is_unchanged() {
    let k = kernel();
    let h = Home::new("tty");
    let (input, out) = (data("assembly.step"), h.0.join("o.png"));
    let leaves = occt::outline(&k, &input, occt::Limits::DEFAULT)
        .unwrap()
        .leaves;
    let (code, j, tty) = on_a_tty(h.command(&png_args(&input, &out), &[]));
    assert_eq!(code, 0, "{j}\n{tty:?}");
    for want in [
        "stepv: reading".to_owned(),
        format!("stepv: {leaves} parts · reading"),
        format!("stepv: {leaves} parts · transferring 100%"),
        format!("stepv: {leaves} parts · meshing 100%"),
    ] {
        assert!(tty.contains(&want), "{want:?} not in {tty:?}");
    }
    // Redrawn in place, and cleared at the end: a later line starts clean.
    assert!(!tty.contains('\n'), "{tty:?}");
    assert!(tty.ends_with("\r\x1b[K"), "{tty:?}");

    // The same run into a pipe: no progress, and the same report.
    let o = h
        .command(&png_args(&input, &out), &[])
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    let (stderr, piped) = (String::from_utf8_lossy(&o.stderr), json(&o.stdout));
    assert_eq!(o.status.code(), Some(0), "{piped}");
    assert!(
        !stderr.contains("stepv: reading") && !stderr.contains('\r'),
        "progress on a pipe: {stderr:?}"
    );
    let keys = |v: &Value| {
        let mut k: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        k.sort();
        k
    };
    assert_eq!(keys(&j), keys(&piped));
    assert_eq!(keys(&j["kernel"]), keys(&piped["kernel"]));
    for key in ["status", "info", "output", "worst_face"] {
        assert_eq!(j[key], piped[key], "{key}");
    }
    for key in ["parts", "faces", "triangles", "bbox"] {
        assert_eq!(j["kernel"][key], piped["kernel"][key], "{key}");
    }
}

#[test]
fn a_cached_outline_names_the_parts_before_the_kernel_starts() {
    let k = kernel();
    let h = Home::new("cached");
    let input = h.0.join("m.step");
    std::fs::copy(data("assembly.step"), &input).unwrap();
    let out = h.0.join("o.png");
    // A kernel that never says a word, killed at the 2 s cap: whatever the
    // line names came from the cache.
    let mute = h.0.join("mute-kernel");
    std::fs::write(&mute, "#!/bin/sh\nexec sleep 30\n").unwrap();
    std::fs::set_permissions(&mute, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let args = [
        input.to_str().unwrap(),
        "--png",
        out.to_str().unwrap(),
        "--timeout",
        "2",
    ];
    let env = [("STEPV_OCCT", mute.to_str().unwrap())];

    let (code, j, tty) = on_a_tty(h.command(&args, &env));
    assert_eq!(code, 4, "{j}");
    assert!(!tty.contains("parts"), "no outline, yet {tty:?}");

    let outline = occt::outline(&k, &input, occt::Limits::DEFAULT).unwrap();
    let leaves = outline.leaves;
    let a = cache::OutlineArtifact::new(stepv::header::read(&input), outline);
    let file = cache::FileId::of(&input).unwrap();
    cache::store_outline(&cache::outline_path(&h.cache_dir(), &file), &a).unwrap();
    let (code, j, tty) = on_a_tty(h.command(&args, &env));
    assert_eq!(code, 4, "{j}");
    assert!(
        tty.contains(&format!("stepv: {leaves} parts · reading")),
        "{tty:?}"
    );
}
