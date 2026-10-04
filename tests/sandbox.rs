//! The kernel's sandbox (#18): everything a hostile file could make an
//! exploited kernel do is refused, and everything the kernel is there for
//! still works.
//!
//! Each test runs the REAL kernel with `STEPV_OCCT_TEST_ESCAPE`, which makes
//! it attempt one action from inside the sandbox and report the outcome on
//! stderr (`kernel/stepv-occt.cpp`). A refusal is asserted twice where it can
//! be: by the kernel's report and by the absence of its effect.

use std::net::{TcpListener, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// The layout every test runs in: the input and a sibling in `in/`, the mesh
/// in `out/`, and `elsewhere/` beside both, which the kernel must not touch.
struct Layout {
    root: PathBuf,
}

impl Layout {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("stepv-sandbox-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["in/sub", "out", "elsewhere"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
        std::fs::copy(data.join("assembly.step"), root.join("in/assembly.step")).unwrap();
        std::fs::write(root.join("in/sibling.step"), "ISO-10303-21;\n").unwrap();
        std::fs::write(root.join("in/sub/nested.step"), "ISO-10303-21;\n").unwrap();
        std::fs::write(root.join("elsewhere/canary"), "secret\n").unwrap();
        Self { root }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
}

impl Drop for Layout {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct KernelRun {
    code: i32,
    summary: Value,
    stderr: String,
}

impl KernelRun {
    /// The kernel's own report on the escape attempt: `Some(true)` allowed.
    fn escape(&self, kind: &str) -> Option<bool> {
        let prefix = format!("stepv-occt escape {kind}: ");
        self.stderr
            .lines()
            .find_map(|l| l.strip_prefix(prefix.as_str()))
            .map(|rest| rest.starts_with("allowed"))
    }

    /// Refused BY THE SANDBOX: EPERM (seccomp, Seatbelt) or EACCES
    /// (Landlock), not some other failure that would pass vacuously.
    fn assert_refused(&self, kind: &str) {
        let prefix = format!("stepv-occt escape {kind}: refused (");
        let why = self
            .stderr
            .lines()
            .find_map(|l| l.strip_prefix(prefix.as_str()));
        assert!(
            why.is_some_and(
                |w| w.starts_with("Operation not permitted") || w.starts_with("Permission denied")
            ),
            "{kind} must be refused by the sandbox; kernel stderr:\n{}",
            self.stderr
        );
        assert_eq!(self.summary["sandbox"], SANDBOX, "{}", self.summary);
    }

    /// The sandbox must not cost the kernel its actual job.
    fn assert_did_the_work(&self, b: &Layout) {
        assert_eq!(
            self.code, 0,
            "summary {}\nstderr:\n{}",
            self.summary, self.stderr
        );
        assert_eq!(self.summary["ok"], true, "{}", self.summary);
        let mesh = std::fs::metadata(b.path("out/assembly.msh")).expect("mesh written");
        assert!(mesh.len() > 0);
    }
}

/// Seatbelt does not nest, so inside nix's macOS build sandbox (`nix flake
/// check`) the kernel cannot add its own and reports `macos-outer`. Only
/// there may a test step aside, saying so: the Kernel workflow runs these
/// tests again outside it. Anywhere else, `macos-outer` fails the test.
pub fn nested_in_nix_build(summary: &Value) -> bool {
    let nested = summary["sandbox"] == "macos-outer" && std::env::var_os("NIX_BUILD_TOP").is_some();
    if nested {
        eprintln!("SKIPPING: inside nix's build sandbox, which Seatbelt cannot nest in");
    }
    nested
}

/// The sandbox a complete run reports on this platform.
const SANDBOX: &str = if cfg!(target_os = "macos") {
    "macos-profile"
} else {
    "landlock+seccomp"
};

fn kernel(b: &Layout, escape: Option<&str>) -> Option<KernelRun> {
    kernel_at(
        &b.path("in/assembly.step"),
        &b.path("out/assembly.msh"),
        escape,
        None,
    )
}

/// A run on any input and mesh path, with `$HOME` set to `home` if given.
fn kernel_at(
    input: &Path,
    mesh: &Path,
    escape: Option<&str>,
    home: Option<&Path>,
) -> Option<KernelRun> {
    let k = stepv::occt::kernel_path();
    if !k.is_file() {
        if std::env::var_os("STEPV_SKIP_KERNEL_TESTS").is_some_and(|v| v == "1") {
            eprintln!("SKIPPING: kernel not built and STEPV_SKIP_KERNEL_TESTS=1");
            return None;
        }
        panic!("kernel not found at {} — run `just kernel`", k.display());
    }
    let mut cmd = Command::new(k);
    cmd.arg(input).arg("--mesh").arg(mesh);
    if let Some(e) = escape {
        cmd.env("STEPV_OCCT_TEST_ESCAPE", e);
    }
    if let Some(h) = home {
        cmd.env("HOME", h);
    }
    let out = cmd.output().expect("spawn kernel");
    let run = KernelRun {
        code: out.status.code().unwrap_or(-1),
        summary: String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .and_then(|l| serde_json::from_str(l).ok())
            .unwrap_or(Value::Null),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    };
    (!nested_in_nix_build(&run.summary)).then_some(run)
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn reports_the_sandbox_in_effect() {
    let b = Layout::new("report");
    let Some(r) = kernel(&b, None) else { return };
    r.assert_did_the_work(&b);
    assert_eq!(r.summary["sandbox"], SANDBOX, "{}", r.summary);
}

#[test]
fn refuses_tcp_connections() {
    let b = Layout::new("tcp");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let Some(r) = kernel(&b, Some(&format!("connect:{addr}"))) else {
        return;
    };
    r.assert_refused("connect");
    listener.set_nonblocking(true).unwrap();
    assert!(
        listener.accept().is_err(),
        "the kernel reached the listener"
    );
    r.assert_did_the_work(&b);
}

#[test]
fn refuses_udp_datagrams() {
    let b = Layout::new("udp");
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = sock.local_addr().unwrap();
    let Some(r) = kernel(&b, Some(&format!("udp:{addr}"))) else {
        return;
    };
    r.assert_refused("udp");
    sock.set_nonblocking(true).unwrap();
    assert!(sock.recv(&mut [0; 8]).is_err(), "a datagram got out");
    r.assert_did_the_work(&b);
}

#[test]
fn refuses_exec() {
    let b = Layout::new("exec");
    let Some(r) = kernel(&b, Some("exec:/bin/sh")) else {
        return;
    };
    r.assert_refused("exec");
    r.assert_did_the_work(&b);
}

#[test]
fn refuses_writes_outside_the_output_file() {
    for target in ["elsewhere/probe", "out/beside-the-mesh", "in/planted.step"] {
        let b = Layout::new("write");
        let p = b.path(target);
        let Some(r) = kernel(&b, Some(&format!("write:{}", s(&p)))) else {
            return;
        };
        r.assert_refused("write");
        assert!(!p.exists(), "{target} was created");
        r.assert_did_the_work(&b);
    }
}

#[test]
fn refuses_overwriting_existing_files() {
    let b = Layout::new("overwrite");
    let canary = b.path("elsewhere/canary");
    let Some(r) = kernel(&b, Some(&format!("write:{}", s(&canary)))) else {
        return;
    };
    r.assert_refused("write");
    assert_eq!(std::fs::read_to_string(&canary).unwrap(), "secret\n");
}

#[test]
fn refuses_reads_outside_the_input_directory() {
    let b = Layout::new("read");
    let canary = b.path("elsewhere/canary");
    let Some(r) = kernel(&b, Some(&format!("read:{}", s(&canary)))) else {
        return;
    };
    r.assert_refused("read");
    r.assert_did_the_work(&b);
}

#[test]
fn allows_the_input_directory_and_below() {
    // Multi-file assemblies resolve external references beside the main file
    // (CAx-IF s1-c5-214), or below it.
    for target in ["in/sibling.step", "in/sub/nested.step"] {
        let b = Layout::new("siblings");
        let p = b.path(target);
        let Some(r) = kernel(&b, Some(&format!("read:{}", s(&p)))) else {
            return;
        };
        assert_eq!(r.escape("read"), Some(true), "{target}: {}", r.stderr);
        r.assert_did_the_work(&b);
    }
}

#[test]
fn refuses_signals_to_other_processes() {
    let b = Layout::new("kill");
    let mut victim = Command::new("sleep").arg("30").spawn().unwrap();
    let r = kernel(&b, Some(&format!("kill:{}", victim.id())));
    let alive = victim.try_wait().unwrap().is_none();
    let _ = victim.kill();
    let _ = victim.wait();
    let Some(r) = r else { return };
    r.assert_refused("kill");
    assert!(alive, "the kernel killed another process");
    r.assert_did_the_work(&b);
}

#[test]
fn an_uncreatable_mesh_still_runs_sandboxed() {
    // A grant that fails must cost that access, not the whole sandbox.
    let b = Layout::new("no-mesh-dir");
    let canary = b.path("elsewhere/canary");
    let Some(r) = kernel_at(
        &b.path("in/assembly.step"),
        &b.path("missing-dir/assembly.msh"),
        Some(&format!("read:{}", s(&canary))),
        None,
    ) else {
        return;
    };
    r.assert_refused("read");
    assert_eq!(r.code, 3, "no mesh can be written: {}", r.summary);
}

#[test]
fn a_symlinked_mesh_is_not_followed() {
    // Otherwise whatever the link points at becomes the writable file.
    let b = Layout::new("mesh-symlink");
    let canary = b.path("elsewhere/canary");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&canary, b.path("out/assembly.msh")).unwrap();
    let Some(r) = kernel(&b, None) else { return };
    assert_eq!(std::fs::read_to_string(&canary).unwrap(), "secret\n");
    assert_eq!(r.summary["sandbox"], SANDBOX, "{}", r.summary);
    assert_eq!(r.code, 3, "{}", r.summary);
}

#[test]
fn a_file_at_the_home_root_exposes_only_itself() {
    // Siblings of ~/part.step are the whole of $HOME, ~/.ssh among it.
    let b = Layout::new("home-root");
    let home = b.path("in");
    let sibling = b.path("in/sibling.step");
    let Some(r) = kernel_at(
        &b.path("in/assembly.step"),
        &b.path("out/assembly.msh"),
        Some(&format!("read:{}", s(&sibling))),
        Some(&home),
    ) else {
        return;
    };
    r.assert_refused("read");
    r.assert_did_the_work(&b);
}

#[test]
fn the_topology_is_the_one_other_writable_file() {
    // --topology (#21) is a second output: granted like the mesh, by its
    // descriptor's path, and nothing else becomes writable with it.
    let b = Layout::new("topology");
    let k = stepv::occt::kernel_path();
    if !k.is_file() {
        return;
    }
    let probe = b.path("out/beside-the-topology");
    let out = Command::new(k)
        .arg(b.path("in/assembly.step"))
        .arg("--mesh")
        .arg(b.path("out/assembly.msh"))
        .arg("--topology")
        .arg(b.path("out/topology.json"))
        .env("STEPV_OCCT_TEST_ESCAPE", format!("write:{}", s(&probe)))
        .output()
        .expect("spawn kernel");
    let r = KernelRun {
        code: out.status.code().unwrap_or(-1),
        summary: serde_json::from_slice(
            out.stdout.split(|&c| c == b'\n').next().unwrap_or_default(),
        )
        .unwrap_or(Value::Null),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    };
    if nested_in_nix_build(&r.summary) {
        return;
    }
    r.assert_refused("write");
    assert!(!probe.exists());
    r.assert_did_the_work(&b);
    let topo = std::fs::read(b.path("out/topology.json")).unwrap();
    assert!(stepv::topology::Topology::parse(&topo).is_ok());
}
