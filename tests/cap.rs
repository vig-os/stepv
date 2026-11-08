//! `stepv-cap` (#83): the kernel memory cap for every launch outside the CLI.
//! A ballooning kernel is killed and named; anything else exits as it would
//! have; and a ballooning test run under it, as a per-test runner would run
//! it, fails by its name.

use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde_json::Value;
use stepv::cap::EXIT_OVER_CAP;

fn cap() -> Command {
    Command::new(env!("CARGO_BIN_EXE_stepv-cap"))
}

fn kernel() -> PathBuf {
    let k = stepv::occt::kernel_path();
    assert!(
        k.is_file(),
        "kernel not found at {}: run `just kernel`",
        k.display()
    );
    k
}

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("stepv-cap-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The balloon proof: the kernel's memory test hook allocates 512 MiB and
/// holds it for 5 s; under a 64 MiB cap it is killed well before, and the
/// one line says who, the cap and the peak.
#[test]
fn a_ballooning_kernel_is_killed_and_named() {
    let start = Instant::now();
    let out = cap()
        .args(["--memory-mb", "64", "--label", "balloon-kernel", "--"])
        .arg(kernel())
        .arg(data("assembly.step"))
        .env("STEPV_OCCT_TEST_BALLOON_MB", "512")
        .output()
        .unwrap();
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(EXIT_OVER_CAP.into()), "{err}");
    assert!(
        err.contains("stepv-cap: balloon-kernel: over the memory cap of 64 MiB (peak "),
        "{err}"
    );
    assert!(
        start.elapsed() < Duration::from_secs(4),
        "{:?}",
        start.elapsed()
    );
}

/// The cap is on the whole tree: a kernel started by a shell, and the
/// report has the peak; the label defaults to the command line.
#[test]
fn the_cap_holds_the_whole_tree_and_reports_its_peak() {
    let report = scratch("tree").join("report.json");
    let out = cap()
        .args(["--memory-mb", "64", "--report-json"])
        .arg(&report)
        .args(["--", "sh", "-c", r#""$0" "$1"; exit 0"#])
        .arg(kernel())
        .arg(data("assembly.step"))
        .env("STEPV_OCCT_TEST_BALLOON_MB", "512")
        .output()
        .unwrap();
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(EXIT_OVER_CAP.into()), "{err}");
    assert!(err.contains("stepv-cap: sh -c "), "{err}");
    let r: Value = serde_json::from_slice(&std::fs::read(&report).unwrap()).unwrap();
    assert_eq!(r["over_cap"], true, "{r}");
    assert_eq!(r["cap_bytes"], 64 << 20, "{r}");
    assert!(r["peak_bytes"].as_u64().unwrap() > 64 << 20, "{r}");
    assert!(r["code"].is_null() && r["signal"].is_null(), "{r}");
}

/// Under the cap, stepv-cap is invisible: the command's output, exit code
/// or signal, and its cap in the environment.
#[test]
fn under_the_cap_the_command_exits_as_it_would() {
    let report = scratch("pass").join("report.json");
    let out = cap()
        .args(["--memory-mb", "4096", "--report-json"])
        .arg(&report)
        .arg("--")
        .arg(kernel())
        .arg(data("box.brep"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let summary: Value =
        serde_json::from_slice(out.stdout.split(|&b| b == b'\n').next().unwrap()).unwrap();
    assert_eq!(summary["ok"], true);
    let r: Value = serde_json::from_slice(&std::fs::read(&report).unwrap()).unwrap();
    assert_eq!(
        (r["over_cap"].clone(), r["code"].clone()),
        (Value::Bool(false), 0.into()),
        "{r}"
    );

    let code = |script: &str| {
        cap()
            .args(["--memory-mb", "64", "--", "sh", "-c", script])
            .status()
            .unwrap()
    };
    assert_eq!(code("exit 7").code(), Some(7));
    assert_eq!(code("kill -TERM $$").signal(), Some(libc::SIGTERM));
    assert_eq!(code(r#"[ "$STEPV_CAP_MB" = 64 ]"#).code(), Some(0));
}

#[test]
fn its_own_failures_are_125_to_127() {
    for args in [
        &["--", "true"][..],
        &["--memory-mb", "0", "--", "true"],
        &["--memory-mb", "64"],
        &["--memory-mb", "64", "--bogus", "--", "true"],
    ] {
        let out = cap().args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(125), "{args:?}: {}", stderr(&out));
    }
    let missing = cap()
        .args(["--memory-mb", "64", "--", "/nonexistent/x"])
        .status()
        .unwrap();
    assert_eq!(missing.code(), Some(127));
}

/// What nextest runs per test when the per-test cap is on: the test binary
/// under stepv-cap, its test name on the command line.
#[test]
fn a_ballooning_test_fails_by_its_name() {
    let out = cap()
        .args(["--memory-mb", "64", "--"])
        .arg(std::env::current_exe().unwrap())
        .args(["balloon_test", "--exact", "--ignored", "--nocapture"])
        .env("STEPV_TEST_BALLOON_MB", "512")
        .output()
        .unwrap();
    let err = stderr(&out);
    assert_eq!(out.status.code(), Some(EXIT_OVER_CAP.into()), "{err}");
    assert!(err.contains("balloon_test --exact"), "{err}");
}

/// Holds `STEPV_TEST_BALLOON_MB` MiB for 5 s: the ballooning test above.
#[test]
#[ignore = "run by a_ballooning_test_fails_by_its_name"]
fn balloon_test() {
    let Some(mb) = std::env::var("STEPV_TEST_BALLOON_MB")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
    else {
        return;
    };
    let mut v = vec![0u8; mb << 20];
    for i in (0..v.len()).step_by(4096) {
        v[i] = 1;
    }
    std::thread::sleep(Duration::from_secs(5));
    assert_eq!(std::hint::black_box(&v)[0], 1);
}
