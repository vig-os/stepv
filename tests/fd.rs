//! The kernel reads a file its caller opened (#91): `/dev/fd/N` with
//! `--format`, as #64's Quick Look service hands it one it cannot open by
//! name. Each test runs the REAL kernel, as tests/cli.rs does.

use std::fs::File;
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;
use stepv::occt::{self, Cause, Format, Input, Limits, Outcome};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

/// The kernel, or `None` when skipping was asked for (tests/cli.rs).
fn kernel() -> Option<PathBuf> {
    let k = occt::kernel_path();
    if k.is_file() {
        return Some(k);
    }
    if std::env::var_os("STEPV_SKIP_KERNEL_TESTS").is_some_and(|v| v == "1") {
        eprintln!("SKIPPING: kernel not built and STEPV_SKIP_KERNEL_TESTS=1");
        return None;
    }
    panic!("kernel not found at {} — run `just kernel`", k.display());
}

/// A scratch directory unique to one test, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("stepv-fd-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs the kernel on `input` with `args`; `file`, if given, is its fd 3.
/// Returns the exit code and the summary, its timings and peak memory left
/// out: what two runs of one file share.
fn kernel_run(k: &Path, input: &str, file: Option<&File>, args: &[&str]) -> (i32, Value) {
    let mut cmd = Command::new(k);
    cmd.arg(input).args(args).stdout(Stdio::piped());
    if let Some(f) = file {
        let n = f.as_raw_fd();
        // SAFETY: dup2 and fcntl are async-signal-safe, on the child's copy.
        unsafe {
            cmd.pre_exec(move || {
                if (n != 3 && libc::dup2(n, 3) == -1) || libc::fcntl(3, libc::F_SETFD, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let out = cmd.output().expect("spawn kernel");
    let line = String::from_utf8_lossy(&out.stdout);
    let mut summary: Value = serde_json::from_str(line.lines().next().unwrap_or("null")).unwrap();
    if let Some(m) = summary.as_object_mut() {
        m.retain(|k, _| !k.starts_with("t_") && k != "peak_rss_bytes");
    }
    (out.status.code().unwrap_or(-1), summary)
}

/// STEP, IGES and BREP from fd 3, with the file's name gone (deleted after
/// opening): the same summary and the same mesh bytes as a run by path.
#[test]
fn an_fd_reads_as_the_path_does() {
    let Some(k) = kernel() else { return };
    let dir = Scratch::new("same");
    for (name, format) in [
        ("assembly.step", "step"),
        ("box.igs", "iges"),
        ("box.brep", "brep"),
    ] {
        let by_path = dir.0.join(format!("{name}.path.msh"));
        let by_fd = dir.0.join(format!("{name}.fd.msh"));
        let want = kernel_run(
            k.as_path(),
            data(name).to_str().unwrap(),
            None,
            &["--mesh", by_path.to_str().unwrap()],
        );
        let copy = dir.0.join(name);
        std::fs::copy(data(name), &copy).unwrap();
        let file = File::open(&copy).unwrap();
        std::fs::remove_file(&copy).unwrap();
        let got = kernel_run(
            k.as_path(),
            "/dev/fd/3",
            Some(&file),
            &["--format", format, "--mesh", by_fd.to_str().unwrap()],
        );
        assert_eq!(want.0, 0, "{name}: {}", want.1);
        assert_eq!(got, want, "{name}");
        assert_eq!(
            std::fs::read(&by_fd).unwrap(),
            std::fs::read(&by_path).unwrap(),
            "{name}: mesh"
        );
    }
}

/// The kernel reads its descriptor and never re-opens `/dev/fd/N` by name
/// (#102): on macOS 15 the sandbox it inherits from #64's Quick Look service
/// denies that re-open (EPERM), where macOS 26 allows it. Here the file's
/// mode is 000 once it is open, so a re-open fails on any platform (EACCES:
/// macOS's fdesc and Linux's /proc/self/fd both check it), while reading
/// the open descriptor still works. A run, an outline and `--serve`, each
/// format: the same as by path.
#[test]
fn the_kernel_reads_its_descriptor_without_reopening_it() {
    let Some(k) = kernel() else { return };
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("SKIPPING: as root, mode 000 does not stop a re-open");
        return;
    }
    let dir = Scratch::new("no-reopen");
    for (name, format) in [
        ("assembly.step", "step"),
        ("box.igs", "iges"),
        ("box.brep", "brep"),
    ] {
        let by_path = dir.0.join(format!("{name}.path.msh"));
        let by_fd = dir.0.join(format!("{name}.fd.msh"));
        let want = kernel_run(
            k.as_path(),
            data(name).to_str().unwrap(),
            None,
            &["--mesh", by_path.to_str().unwrap()],
        );
        let copy = dir.0.join(name);
        std::fs::copy(data(name), &copy).unwrap();
        let file = File::open(&copy).unwrap();
        std::fs::set_permissions(&copy, std::os::unix::fs::PermissionsExt::from_mode(0)).unwrap();
        assert!(
            File::open(format!("/dev/fd/{}", file.as_raw_fd())).is_err(),
            "{name}: the test's premise: a re-open of the descriptor fails"
        );
        let got = kernel_run(
            k.as_path(),
            "/dev/fd/3",
            Some(&file),
            &["--format", format, "--mesh", by_fd.to_str().unwrap()],
        );
        assert_eq!(want.0, 0, "{name}: {}", want.1);
        assert_eq!(got, want, "{name}");
        assert_eq!(
            std::fs::read(&by_fd).unwrap(),
            std::fs::read(&by_path).unwrap(),
            "{name}: mesh"
        );
        let (code, ready) = kernel_run(k.as_path(), "/dev/fd/3", Some(&file), &["--format", format, "--serve"]);
        assert_eq!((code, &ready["ready"]), (0, &Value::Bool(true)), "{name} --serve: {ready}");
        if format == "step" {
            let want = kernel_run(k.as_path(), data(name).to_str().unwrap(), None, &["--outline"]);
            let got = kernel_run(k.as_path(), "/dev/fd/3", Some(&file), &["--format", format, "--outline"]);
            assert_eq!(got, want, "{name} --outline");
        }
    }
}

/// IGES has no stream reader in OCCT, so the kernel reads a private copy
/// of the descriptor, in `$TMPDIR` (#102): it is gone once the run is, on
/// success and on failure alike.
#[test]
fn the_iges_copy_is_unlinked() {
    let Some(k) = kernel() else { return };
    let dir = Scratch::new("iges-copy");
    let tmp = dir.0.join("tmp");
    std::fs::create_dir(&tmp).unwrap();
    for (name, ok) in [("box.igs", true), ("box.brep", false)] {
        let file = File::open(data(name)).unwrap();
        let n = file.as_raw_fd();
        let mut cmd = Command::new(&k);
        cmd.args(["/dev/fd/3", "--format", "iges"])
            .env("TMPDIR", &tmp)
            .stdout(Stdio::piped());
        // SAFETY: dup2 and fcntl are async-signal-safe, on the child's copy.
        unsafe {
            cmd.pre_exec(move || {
                if (n != 3 && libc::dup2(n, 3) == -1) || libc::fcntl(3, libc::F_SETFD, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let out = cmd.output().unwrap();
        assert_eq!(out.status.code(), Some(if ok { 0 } else { 3 }), "{name}");
        let left: Vec<_> = std::fs::read_dir(&tmp).unwrap().collect();
        assert!(left.is_empty(), "{name}: left in $TMPDIR: {left:?}");
    }
}

/// A descriptor has no extension: without `--format`, or with one the
/// kernel has no reader for, the run fails before reading, and the cause is
/// the environment: the caller asked wrongly, and the same file reads when
/// asked right, so a front-end must not remember the file as bad (#62). A
/// format the file does not read as is the file's, as a wrong extension is.
#[test]
fn an_fd_needs_a_format_it_reads_as() {
    let Some(k) = kernel() else { return };
    let file = File::open(data("box.brep")).unwrap();
    for (args, cause, error) in [
        (&[][..], "environment", "needs --format step|iges|brep"),
        (
            &["--format", "dwg"][..],
            "environment",
            "unsupported --format: dwg",
        ),
        (&["--format", "iges"][..], "input", "IGES read failed"),
    ] {
        let (code, s) = kernel_run(k.as_path(), "/dev/fd/3", Some(&file), args);
        assert_eq!(code, 3, "{args:?}: {s}");
        assert_eq!(s["ok"], false, "{args:?}: {s}");
        assert_eq!(s["stage"], "read", "{args:?}: {s}");
        assert_eq!(s["cause"], cause, "{args:?}: {s}");
        assert!(
            s["error"].as_str().is_some_and(|e| e.contains(error)),
            "{args:?}: {s}"
        );
    }
}

/// `occt::run` and `occt::outline` on an [`Input::Fd`]: the same scene and
/// outline as by path. A pipe cannot be rewound: the environment's fault.
#[test]
fn the_rust_api_passes_an_fd() {
    let Some(k) = kernel() else { return };
    let dir = Scratch::new("api");
    let lim = Limits::DEFAULT;
    let d = stepv::Deflection::PREVIEW;
    for (name, format) in [
        ("assembly.step", Format::Step),
        ("box.igs", Format::Iges),
        ("box.brep", Format::Brep),
    ] {
        let (by_path, by_fd) = (dir.0.join("path.msh"), dir.0.join("fd.msh"));
        let want = occt::run(&k, &data(name), d, lim, Some(&by_path), None, false).unwrap();
        let file = File::open(data(name)).unwrap();
        let input = Input::Fd {
            fd: file.as_fd(),
            format,
        };
        let got = occt::run(&k, input, d, lim, Some(&by_fd), None, false).unwrap();
        assert_eq!(got.outcome, Outcome::Ok, "{name}: {:?}", got.summary);
        assert_eq!(got.summary.unwrap().format, format.as_str());
        assert_eq!(
            std::fs::read(&by_fd).unwrap(),
            std::fs::read(&by_path).unwrap(),
            "{name}"
        );
        assert_eq!(want.outcome, Outcome::Ok);
    }

    let file = File::open(data("assembly.step")).unwrap();
    let fd = Input::Fd {
        fd: file.as_fd(),
        format: Format::Step,
    };
    assert_eq!(
        occt::outline(&k, fd, lim).unwrap(),
        occt::outline(&k, &data("assembly.step"), lim).unwrap()
    );

    let (reader, _writer) = std::io::pipe().unwrap();
    let pipe = Input::Fd {
        fd: reader.as_fd(),
        format: Format::Step,
    };
    let run = occt::run(&k, pipe, d, lim, None, None, false).unwrap();
    assert_eq!(run.outcome, Outcome::Failed);
    assert_eq!(run.summary.unwrap().cause, Some(Cause::Environment));
}
