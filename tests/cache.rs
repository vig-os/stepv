//! The cache (#62) end to end: only complete results are cached, an entry
//! that is truncated or of another version is regenerated, and a run killed
//! after its outline leaves the outline artifact but no entry. Run the real
//! kernel against `tests/data/`.

use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

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

    /// stepv with this HOME, not yet run.
    fn command(&self, args: &[&str], env: &[(&str, &str)]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_stepv"));
        cmd.args(args)
            .env("HOME", &self.0)
            .env("XDG_CACHE_HOME", self.0.join("xdg"))
            .envs(env.iter().copied());
        cmd
    }

    /// Runs stepv, returning its exit code and JSON line.
    fn stepv(&self, args: &[&str], env: &[(&str, &str)]) -> (i32, Value) {
        let out = self.command(args, env).output().expect("spawn stepv");
        result(&out)
    }
}

/// A finished stepv's exit code and JSON line.
fn result(out: &std::process::Output) -> (i32, Value) {
    let json = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .and_then(|l| serde_json::from_str(l).ok())
        .unwrap_or(Value::Null);
    (out.status.code().unwrap_or(-1), json)
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

#[test]
fn a_timed_out_run_caches_nothing_and_blocks_nothing() {
    kernel();
    let h = Home::new("timeout");
    let input = data("assembly.step");
    let out = h.0.join("o.png");
    let args = [
        input.to_str().unwrap(),
        "--png",
        out.to_str().unwrap(),
        "--size",
        "64",
    ];

    // A kernel that hangs; the 1 s cap kills it.
    let slow = h.0.join("slow-kernel");
    std::fs::write(&slow, "#!/bin/sh\nexec sleep 30\n").unwrap();
    std::fs::set_permissions(&slow, std::fs::Permissions::from_mode(0o755)).unwrap();
    let timed_out = [&args[..], &["--timeout", "1"]].concat();
    let (code, j) = h.stepv(&timed_out, &[("STEPV_OCCT", slow.to_str().unwrap())]);
    assert_eq!(code, 4, "{j}");
    assert!(
        h.entries("png").is_empty(),
        "a timed-out run cached an output"
    );
    assert!(
        h.entries("fail").is_empty(),
        "a timeout was cached as a failure"
    );

    // The next run, within its limits, completes and caches.
    let (code, j) = h.stepv(&args, &[]);
    assert_eq!((code, &j["cached"]), (0, &Value::Bool(false)), "{j}");
    assert_eq!(h.entries("png").len(), 1);
    assert_eq!(h.stepv(&args, &[]).1["cached"], true);
}

impl Home {
    /// Runs `stepv <input> --png <home>/o.png --size 64 <extra>`.
    fn png(&self, input: &Path, extra: &[&str], env: &[(&str, &str)]) -> (i32, Value) {
        let out = self.0.join("o.png");
        let args = [
            &[input.to_str().unwrap(), "--png", out.to_str().unwrap()],
            &["--size", "64"][..],
            extra,
        ]
        .concat();
        self.stepv(&args, env)
    }

    /// No output entry and no failure cached.
    fn assert_nothing_cached(&self, why: &str) {
        assert!(
            self.entries("png").is_empty(),
            "{why}: an output was cached"
        );
        assert!(
            self.entries("fail").is_empty(),
            "{why}: a failure was cached"
        );
    }
}

#[test]
fn a_memory_capped_run_caches_nothing() {
    kernel();
    let h = Home::new("memcap");
    let input = data("assembly.step");
    // The kernel's test hook holds 512 MiB; the 64 MiB cap kills it.
    let (code, j) = h.png(
        &input,
        &["--memory-mb", "64"],
        &[("STEPV_OCCT_TEST_BALLOON_MB", "512")],
    );
    assert_eq!((code, j["status"].as_str()), (3, Some("memory-cap")), "{j}");
    h.assert_nothing_cached("memory cap");
    assert_eq!(h.png(&input, &[], &[]).0, 0);
}

/// An executable `sh` script `body` under `h`, as a stand-in kernel.
fn script(h: &Home, name: &str, body: &str) -> PathBuf {
    let p = h.0.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

#[test]
fn a_kernel_exit_without_a_summary_is_an_error_and_not_cached() {
    let k = kernel();
    let h = Home::new("nosummary");
    // Exit 0: the real kernel writes the mesh; its summary, or its stream,
    // goes nowhere. Exit 3: a clean failure that leaves its summary out.
    let mute = script(
        &h,
        "mute-kernel",
        &format!("'{}' \"$@\" >/dev/null", k.display()),
    );
    let fail = script(&h, "fail-kernel", "exit 3");
    for kernel in [&mute, &fail] {
        let env = [("STEPV_OCCT", kernel.to_str().unwrap())];
        // Cached, so streamed; then plain.
        for extra in [&[][..], &["--no-cache"]] {
            let (code, j) = h.png(&data("assembly.step"), extra, &env);
            let why = format!("{} {extra:?}: {j}", kernel.display());
            assert_eq!((code, j["status"].as_str()), (3, Some("error")), "{why}");
            assert_eq!(
                j["error"], "bad kernel output: the kernel exited without a summary",
                "{why}"
            );
            h.assert_nothing_cached("no summary");
        }
    }
}

#[test]
fn an_unwritable_mesh_output_is_the_environment_and_not_cached() {
    kernel();
    let h = Home::new("readonly");
    // stepv's mesh goes to the temporary directory: make it read-only.
    let ro = h.0.join("ro");
    std::fs::create_dir(&ro).unwrap();
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
    let input = data("assembly.step");
    let (code, j) = h.png(&input, &[], &[("TMPDIR", ro.to_str().unwrap())]);
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(code, 3, "{j}");
    assert_eq!(j["kernel"]["cause"], "environment", "{j}");
    h.assert_nothing_cached("unwritable mesh");
    assert_eq!(h.png(&input, &[], &[]).0, 0);
}

#[test]
fn missing_part_files_are_the_environment_and_not_cached() {
    kernel();
    let h = Home::new("multifile");
    let dir = h.0.join("asm");
    std::fs::create_dir(&dir).unwrap();
    let top = dir.join("bracket.step");
    std::fs::copy(data("multifile/bracket.step"), &top).unwrap();
    let restore = |part: &str| {
        std::fs::copy(data(&format!("multifile/{part}")), dir.join(part)).unwrap();
    };

    // Neither part file: a failure, and not the file's.
    let (code, j) = h.png(&top, &[], &[]);
    assert_eq!(code, 3, "{j}");
    assert_eq!(j["kernel"]["cause"], "environment", "{j}");
    h.assert_nothing_cached("no part files");

    // One of two: a picture, incomplete, so not cached either.
    restore("pin.stp");
    let (code, j) = h.png(&top, &[], &[]);
    assert_eq!(
        (code, &j["kernel"]["external_missing"]),
        (0, &1.into()),
        "{j}"
    );
    h.assert_nothing_cached("one part file missing");

    // Both back: the whole assembly, cached.
    restore("plate.stp");
    let (code, j) = h.png(&top, &[], &[]);
    assert_eq!(
        (code, &j["kernel"]["parts"], &j["cached"]),
        (0, &3.into(), &false.into()),
        "{j}"
    );
    assert_eq!(h.png(&top, &[], &[]).1["cached"], true);
}

#[test]
fn a_bad_input_is_the_input_and_its_failure_is_cached() {
    kernel();
    let h = Home::new("badinput");
    let input = h.0.join("garbage.step");
    std::fs::write(&input, b"not a step file at all\n").unwrap();
    let (code, j) = h.png(&input, &[], &[]);
    assert_eq!(code, 3, "{j}");
    assert_eq!(j["kernel"]["cause"], "input", "{j}");
    assert_eq!(h.entries("fail").len(), 1);
    let (code, again) = h.png(&input, &[], &[]);
    assert_eq!((code, &again["cached"]), (3, &true.into()), "{again}");
    assert_eq!(again["error"], j["error"]);
}

#[test]
fn a_complete_run_has_no_cause() {
    kernel();
    let h = Home::new("ok");
    let (code, j) = h.png(&data("assembly.step"), &[], &[]);
    assert_eq!(code, 0, "{j}");
    assert!(j["kernel"]["cause"].is_null(), "{j}");
}

/// The kernel's C library: `$STEPV_OCCT_LIB`, else this checkout's
/// `target/kernel/` (`just kernel`), as `kernel()` resolves `$STEPV_OCCT`.
/// A missing file fails the test, with its path.
fn kernel_lib() -> PathBuf {
    let lib = std::env::var_os("STEPV_OCCT_LIB").map_or_else(
        || {
            let name = if cfg!(target_os = "macos") {
                "libstepvocct.dylib"
            } else {
                "libstepvocct.so"
            };
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/kernel")
                .join(name)
        },
        PathBuf::from,
    );
    assert!(
        lib.is_file(),
        "kernel library not found at {}: run `just kernel` or set STEPV_OCCT_LIB",
        lib.display()
    );
    lib
}

#[test]
fn an_input_the_host_will_not_open_is_the_environment() {
    kernel();
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("SKIPPING: root ignores mode bits, so the file would open");
        return;
    }
    let h = Home::new("unreadable");
    let input = h.0.join("m.step");
    std::fs::copy(data("assembly.step"), &input).unwrap();
    std::fs::set_permissions(&input, std::fs::Permissions::from_mode(0o000)).unwrap();
    let (code, j) = h.png(&input, &[], &[]);
    std::fs::set_permissions(&input, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(code, 3, "{j}");
    assert_eq!(j["kernel"]["cause"], "environment", "{j}");
    assert!(
        j["error"].as_str().unwrap().contains("cannot open input"),
        "{j}"
    );
    h.assert_nothing_cached("unreadable input");
    assert_eq!(
        h.png(&input, &[], &[]).0,
        0,
        "readable again, and not blocked"
    );
}

#[test]
fn an_occt_exception_is_the_input_but_its_out_of_memory_is_the_environment() {
    kernel();
    let input = data("assembly.step");
    // The kernel's test hook throws mid-run.
    let h = Home::new("throw");
    let (code, j) = h.png(&input, &[], &[("STEPV_OCCT_TEST_THROW", "failure")]);
    assert_eq!(code, 3, "{j}");
    assert_eq!(j["kernel"]["cause"], "input", "{j}");
    assert_eq!(h.entries("fail").len(), 1);

    let h = Home::new("throw-oom");
    let (code, j) = h.png(&input, &[], &[("STEPV_OCCT_TEST_THROW", "oom")]);
    assert_eq!((code, j["status"].as_str()), (3, Some("memory-cap")), "{j}");
    assert_eq!(j["kernel"]["cause"], "environment", "{j}");
    h.assert_nothing_cached("OCCT out of memory");
}

#[test]
fn a_file_with_no_geometry_is_the_input() {
    kernel();
    let h = Home::new("nogeometry");
    let input = h.0.join("empty.step");
    std::fs::write(
        &input,
        b"ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;\n",
    )
    .unwrap();
    let (code, j) = h.png(&input, &[], &[]);
    assert_eq!(code, 3, "{j}");
    assert_eq!(j["kernel"]["cause"], "input", "{j}");
    assert_eq!(h.entries("fail").len(), 1);
}

#[test]
fn invalid_arguments_are_the_environment() {
    use std::ffi::{CStr, CString, c_char, c_int};
    type Run =
        unsafe extern "C" fn(*const c_char, *const c_char, f64, f64, *mut c_int) -> *mut c_char;
    type Free = unsafe extern "C" fn(*mut c_char);
    let lib = CString::new(kernel_lib().into_os_string().into_encoded_bytes()).unwrap();
    // SAFETY: the kernel library's C ABI (kernel/stepv_occt.h), called with
    // the signatures it declares; the summary is freed with its own free.
    let json = unsafe {
        let h = libc::dlopen(lib.as_ptr(), libc::RTLD_NOW);
        assert!(!h.is_null(), "{:?}", CStr::from_ptr(libc::dlerror()));
        let run = libc::dlsym(h, c"stepv_occt_run".as_ptr());
        let free = libc::dlsym(h, c"stepv_occt_free".as_ptr());
        assert!(!run.is_null() && !free.is_null(), "the C API's symbols");
        let run: Run = std::mem::transmute(run);
        let free: Free = std::mem::transmute(free);
        let mut code = 0;
        // A zero deflection: what no caller should pass.
        let p = run(
            c"m.step".as_ptr(),
            std::ptr::null(),
            0.0,
            20.0,
            &raw mut code,
        );
        let json = CStr::from_ptr(p).to_string_lossy().into_owned();
        free(p);
        assert_eq!(code, 3, "{json}");
        json
    };
    let j: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        (&j["error"], &j["cause"]),
        (&"invalid arguments".into(), &"environment".into())
    );
}

#[test]
fn a_run_killed_after_its_outline_leaves_the_artifact_and_no_entry() {
    let k = kernel();
    let h = Home::new("killed");
    let input = data("assembly.step");
    let out = h.0.join("o.png");
    let args = [
        input.to_str().unwrap(),
        "--png",
        out.to_str().unwrap(),
        "--size",
        "64",
    ];

    // The kernel stalls 30 s once the outline is out; the 10 s cap kills it
    // there, with room for the parse on a loaded machine.
    let killed = [&args[..], &["--timeout", "10"]].concat();
    let (code, j) = h.stepv(&killed, &[("STEPV_OCCT_TEST_STALL_MS", "30000")]);
    assert_eq!(code, 4, "{j}");
    assert!(h.entries("png").is_empty(), "a killed run cached an output");
    assert!(
        h.entries("fail").is_empty(),
        "a timeout was cached as a failure"
    );

    let outline = occt::outline(&k, &input, occt::Limits::DEFAULT).unwrap();
    let a = cache::load_outline_in(&h.cache_dir(), &input).expect("no outline artifact");
    assert_eq!((&a.tree, a.leaves), (&outline.tree, outline.leaves));
    assert_eq!(a.header, stepv::header::read(&input));
    assert_eq!((a.bbox, a.boxes), (None, None));

    // The timeout blocks nothing: the next run completes and caches.
    let (code, j) = h.stepv(&args, &[]);
    assert_eq!((code, &j["cached"]), (0, &Value::Bool(false)), "{j}");
    assert_eq!(h.entries("png").len(), 1);
    let a = cache::load_outline_in(&h.cache_dir(), &input).unwrap();
    assert_eq!(a.tree, outline.tree);
    let bbox: [f64; 6] = serde_json::from_value(j["kernel"]["bbox"].clone()).unwrap();
    assert_eq!(a.bbox, Some(bbox));
}

#[test]
fn the_artifact_is_keyed_on_the_file_as_it_is_now() {
    let h = Home::new("stale");
    let input = h.0.join("m.step");
    std::fs::copy(data("assembly.step"), &input).unwrap();
    let outline = occt::outline(&kernel(), &input, occt::Limits::DEFAULT).unwrap();
    let a = cache::OutlineArtifact::new(stepv::header::read(&input), outline);
    let file = cache::FileId::of(&input).unwrap();
    cache::store_outline(&cache::outline_path(&h.cache_dir(), &file), &a).unwrap();
    assert_eq!(
        cache::load_outline_in(&h.cache_dir(), &input),
        Some(a.clone())
    );
    // Touched: the same length, another mtime, so another key.
    let mtime = std::fs::metadata(&input).unwrap().modified().unwrap();
    let f = std::fs::File::options().write(true).open(&input).unwrap();
    f.set_modified(mtime + Duration::from_secs(5)).unwrap();
    assert_eq!(cache::load_outline_in(&h.cache_dir(), &input), None);
    f.set_modified(mtime).unwrap();
    assert_eq!(cache::load_outline_in(&h.cache_dir(), &input), Some(a));
    // Rewritten: another length, so another key.
    let mut bytes = std::fs::read(&input).unwrap();
    bytes.extend_from_slice(b"\n");
    std::fs::write(&input, bytes).unwrap();
    assert_eq!(cache::load_outline_in(&h.cache_dir(), &input), None);
}

#[test]
fn a_file_saved_mid_run_gets_nothing_from_that_run() {
    kernel();
    let h = Home::new("savedmidrun");
    let input = h.0.join("m.step");
    std::fs::copy(data("assembly.step"), &input).unwrap();
    let out = h.0.join("o.png");
    let args = [
        input.to_str().unwrap(),
        "--png",
        out.to_str().unwrap(),
        "--size",
        "64",
    ];
    let before = cache::FileId::of(&input).unwrap();
    // The kernel holds the run 3 s once the outline is out.
    let child = h
        .command(&args, &[("STEPV_OCCT_TEST_STALL_MS", "3000")])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while h.entries("json").is_empty() {
        assert!(Instant::now() < deadline, "no outline artifact");
        std::thread::sleep(Duration::from_millis(10));
    }
    // Saved in place meanwhile: the same length, a later mtime.
    let mtime = std::fs::metadata(&input).unwrap().modified().unwrap();
    std::fs::File::options()
        .write(true)
        .open(&input)
        .unwrap()
        .set_modified(mtime + Duration::from_secs(5))
        .unwrap();
    let (code, j) = result(&child.wait_with_output().unwrap());
    assert_eq!(code, 0, "{j}");
    // Everything the run left is under the file as it was when it began.
    assert_eq!(
        cache::load_outline_in(&h.cache_dir(), &input),
        None,
        "the saved file was given the earlier run's outline"
    );
    // The run's artifact, its box included, is under the file it read.
    let bbox: [f64; 6] = serde_json::from_value(j["kernel"]["bbox"].clone()).unwrap();
    let a = cache::load_outline_at(&cache::outline_path(&h.cache_dir(), &before))
        .expect("no artifact under the file the run read");
    assert_eq!(a.bbox, Some(bbox));
    let (code, j) = h.stepv(&args, &[]);
    assert_eq!((code, &j["cached"]), (0, &Value::Bool(false)), "{j}");
}

#[test]
fn the_box_is_stored_only_once_the_mesh_has_decoded() {
    let k = kernel();
    let h = Home::new("baddecode");
    let input = data("assembly.step");
    // The real kernel, its stream intact, and then its mesh emptied.
    let spoil = script(
        &h,
        "spoil-kernel",
        &format!(
            "'{}' \"$@\"; rc=$?\nprev=\nfor a in \"$@\"; do [ \"$prev\" = --mesh ] && : >\"$a\"; prev=$a; done\nexit $rc",
            k.display()
        ),
    );
    let (code, j) = h.png(&input, &[], &[("STEPV_OCCT", spoil.to_str().unwrap())]);
    assert_eq!((code, j["status"].as_str()), (3, Some("error")), "{j}");
    assert!(
        j["error"]
            .as_str()
            .unwrap()
            .starts_with("bad kernel output"),
        "{j}"
    );
    h.assert_nothing_cached("an undecodable mesh");
    let a = cache::load_outline_in(&h.cache_dir(), &input).expect("no outline artifact");
    assert_eq!(a.bbox, None, "a box from a run that did not complete");
}

#[test]
fn a_later_run_killed_after_its_outline_keeps_the_box() {
    kernel();
    let h = Home::new("keepbox");
    let input = data("assembly.step");
    let (code, j) = h.png(&input, &[], &[]);
    assert_eq!(code, 0, "{j}");
    let bbox = cache::load_outline_in(&h.cache_dir(), &input).unwrap().bbox;
    assert!(bbox.is_some());
    let (code, j) = h.png(
        &input,
        // Another size: past the cached output, onto the same artifact.
        &["--size", "32", "--timeout", "10"],
        &[("STEPV_OCCT_TEST_STALL_MS", "30000")],
    );
    assert_eq!(code, 4, "{j}");
    assert_eq!(
        cache::load_outline_in(&h.cache_dir(), &input).unwrap().bbox,
        bbox
    );
}

/// The most a `view` test lets the viewer process hold (#86): its kernel
/// has `--memory-mb`, the viewer itself has no cap of its own.
const VIEW_FOOTPRINT_CAP: u64 = 2 << 30;
/// How long a `view` test may run before the watchdog kills it: well past
/// the `--timeout` `Home::view` gives stepv, which fires first.
const VIEW_DEADLINE: Duration = Duration::from_secs(120);

impl Home {
    /// `stepv view <input>` with `args`, held to what a test may cost (#86):
    /// the kernel under `--memory-mb 1024`, stepv's own `--timeout 30` unless
    /// `args` gives another, the screenshot hook set so a window that opens
    /// closes itself, and the viewer's own footprint polled.
    ///
    /// The watchdog is the backstop: past [`VIEW_FOOTPRINT_CAP`] or
    /// [`VIEW_DEADLINE`] it kills the run's whole process group, not stepv
    /// alone. The kernel's caps are stepv's own sampler, so a kernel orphaned
    /// by killing stepv would run on uncapped. However the run ends, nothing
    /// of its group may outlive it.
    fn view(&self, input: &Path, args: &[&str], env: &[(&str, &str)]) -> (i32, Value) {
        let (code, j, _) = self.view_within(input, args, env, VIEW_DEADLINE);
        (code, j)
    }

    /// [`Home::view`] with the watchdog's `deadline`, and stepv's stderr:
    /// what the window reports (`STEPV_VIEW_SCREENSHOT`'s lines).
    fn view_within(
        &self,
        input: &Path,
        args: &[&str],
        env: &[(&str, &str)],
        deadline: Duration,
    ) -> (i32, Value, String) {
        let shot = self.0.join("shot.png");
        // A later `--timeout` in `args` wins.
        let all = [
            &["view", input.to_str().unwrap(), "--timeout", "30"][..],
            args,
            &["--memory-mb", "1024"],
        ]
        .concat();
        let env = [env, &[("STEPV_VIEW_SCREENSHOT", shot.to_str().unwrap())]].concat();
        let mut child = self
            .command(&all, &env)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            // Its own group, the kernel included: one kill takes them all.
            .process_group(0)
            .spawn()
            .expect("spawn stepv view");
        // Read as it comes, so a chatty viewer never blocks on a full pipe.
        let mut err = child.stderr.take().unwrap();
        let stderr = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = std::io::Read::read_to_string(&mut err, &mut s);
            s
        });
        let group = child.id() as libc::pid_t;
        let start = Instant::now();
        let fired = loop {
            if child.try_wait().unwrap().is_some() {
                break None;
            }
            let fp = occt::footprint(child.id()).unwrap_or(0);
            if fp > VIEW_FOOTPRINT_CAP {
                break Some(format!(
                    "footprint {fp} bytes over the cap of {VIEW_FOOTPRINT_CAP}"
                ));
            }
            if start.elapsed() > deadline {
                break Some(format!(
                    "{:.1} s elapsed, past the deadline of {} s",
                    start.elapsed().as_secs_f64(),
                    deadline.as_secs()
                ));
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        if fired.is_some() {
            // SAFETY: a signal to the group this test spawned.
            unsafe { libc::killpg(group, libc::SIGKILL) };
        }
        let out = child.wait_with_output().unwrap();
        // Every member gone, the kernel included (stepv reaps it; one it
        // left would be reparented and reaped once dead).
        let gone = Instant::now() + Duration::from_secs(5);
        // SAFETY: signal 0 only asks whether the group has members.
        while unsafe { libc::killpg(group, 0) } == 0 {
            if Instant::now() > gone {
                unsafe { libc::killpg(group, libc::SIGKILL) };
                panic!("a process of stepv view's group outlived it");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let stderr = stderr.join().unwrap();
        eprint!("{stderr}");
        if let Some(why) = fired {
            panic!("stepv view and its group killed: {why}");
        }
        let (code, j) = result(&out);
        (code, j, stderr)
    }
}

// The tests that start `view` past its session check need a window session
// (#84): nix's build sandbox has none, so they are ignored there, and
// `just view-test` runs them (`--ignored`) on the macOS runner and under
// Xvfb in the Linux viewer job.

#[test]
fn a_viewer_without_a_window_session_fails_before_the_kernel() {
    let h = Home::new("viewnosession");
    // A kernel that leaves a mark if it is ever started.
    let mark = h.0.join("kernel-ran");
    let k = script(
        &h,
        "marking-kernel",
        &format!("touch '{}'\nexit 3", mark.display()),
    );
    let mut cmd = h.command(
        &[
            "view",
            data("assembly.step").to_str().unwrap(),
            "--memory-mb",
            "1024",
        ],
        &[
            ("STEPV_OCCT", k.to_str().unwrap()),
            ("STEPV_VIEW_TEST_NO_SESSION", "1"),
            // A window that opens anyway closes itself.
            (
                "STEPV_VIEW_SCREENSHOT",
                h.0.join("shot.png").to_str().unwrap(),
            ),
        ],
    );
    // Linux's real check, too: no display at all.
    cmd.env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY");
    let (code, j) = result(&cmd.output().expect("spawn stepv view"));
    assert_eq!((code, j["status"].as_str()), (3, Some("error")), "{j}");
    let e = j["error"].as_str().unwrap();
    assert!(e.starts_with("no window session: "), "{j}");
    assert!(!mark.exists(), "the kernel ran before the session check");
    // The report is the whole one, header metadata included.
    assert_eq!(
        j["info"],
        serde_json::to_value(stepv::header::read(&data("assembly.step"))).unwrap()
    );
}

#[test]
#[ignore = "needs a window session; run by just view-test"]
fn the_view_watchdog_kills_the_kernel_too() {
    let h = Home::new("viewdog");
    // A kernel that never finishes, and says who it is: stepv waits on it
    // for its 30 s timeout, past the watchdog's 10 s. The deadline must leave
    // room for start-up: macOS assesses a freshly written script on its first
    // launch, measured at up to 1.5 s, and the window opens meanwhile (#85).
    let pidfile = h.0.join("kernel.pid");
    let hang = script(
        &h,
        "hang-kernel",
        &format!("echo $$ > '{}'\nexec sleep 60", pidfile.display()),
    );
    let run = std::panic::catch_unwind(|| {
        h.view_within(
            &data("assembly.step"),
            &[],
            &[("STEPV_OCCT", hang.to_str().unwrap())],
            Duration::from_secs(10),
        )
    });
    let why = run.expect_err("the watchdog did not fire");
    let why = why
        .downcast_ref::<String>()
        .map_or("", String::as_str)
        .to_owned();
    assert!(why.contains("past the deadline of 10 s"), "{why}");
    let pid: libc::pid_t = std::fs::read_to_string(&pidfile)
        .expect("the kernel never started")
        .trim()
        .parse()
        .unwrap();
    // SAFETY: signal 0 only asks whether the process exists.
    let alive = unsafe { libc::kill(pid, 0) } == 0;
    assert!(!alive, "the kernel {pid} outlived the watchdog");
}

#[test]
#[ignore = "needs a window session; run by just view-test"]
fn a_viewer_killed_after_its_outline_leaves_the_artifact() {
    let k = kernel();
    let h = Home::new("view");
    let input = h.0.join("m.step");
    std::fs::copy(data("assembly.step"), &input).unwrap();
    // The snapshot the run must key on, taken before it starts.
    let before = cache::FileId::of(&input).unwrap();
    // The kernel stalls 30 s once the outline is out; the 10 s cap kills it
    // there, before the viewer would open a window. A viewer whose run was
    // not streamed opens one, and `Home::view` closes it again.
    let (code, j) = h.view(
        &input,
        &["--timeout", "10"],
        &[("STEPV_OCCT_TEST_STALL_MS", "30000")],
    );
    assert_eq!((code, j["status"].as_str()), (4, Some("timeout")), "{j}");
    let a = cache::load_outline_at(&cache::outline_path(&h.cache_dir(), &before))
        .expect("the viewer's run left no outline artifact");
    let outline = occt::outline(&k, &input, occt::Limits::DEFAULT).unwrap();
    assert_eq!((&a.tree, a.leaves), (&outline.tree, outline.leaves));
    assert_eq!(a.header, stepv::header::read(&input));
    assert_eq!(a.bbox, None, "a box from a run that did not complete");
}

#[test]
#[ignore = "needs a window session; run by just view-test"]
fn a_viewer_with_no_cache_leaves_no_artifact() {
    let k = kernel();
    let h = Home::new("viewnocache");
    let input = data("assembly.step");
    // The real kernel, then a crash: a streamed run would have its outline,
    // and no run gets as far as a window.
    let crash = script(
        &h,
        "crash-kernel",
        &format!("'{}' \"$@\"\nexit 1", k.display()),
    );
    let env = [("STEPV_OCCT", crash.to_str().unwrap())];
    let (code, j) = h.view(&input, &["--no-cache"], &env);
    assert_eq!((code, j["status"].as_str()), (3, Some("crashed")), "{j}");
    assert!(h.entries("json").is_empty(), "--no-cache wrote an artifact");
    // The same run, cached: the outline is left. Without this, the first
    // half could pass with a crash that comes before the outline.
    let (code, j) = h.view(&input, &[], &env);
    assert_eq!(code, 3, "{j}");
    assert_eq!(h.entries("json").len(), 1, "a cached view left no artifact");
}

#[test]
#[ignore = "needs a window session; run by just view-test"]
fn a_complete_view_adds_the_box() {
    kernel();
    let h = Home::new("viewbox");
    let input = h.0.join("m.step");
    std::fs::copy(data("assembly.step"), &input).unwrap();
    let before = cache::FileId::of(&input).unwrap();
    let (code, j) = h.view(&input, &[], &[]);
    // The box is stored once the mesh has decoded, before the window opens:
    // whether a window can open here (a headless runner) does not matter.
    assert!(
        j["status"] == "ok" || (code == 3 && j["status"] == "error"),
        "{j}"
    );
    let bbox: [f64; 6] = serde_json::from_value(j["kernel"]["bbox"].clone()).expect("a box");
    let a = cache::load_outline_at(&cache::outline_path(&h.cache_dir(), &before))
        .expect("the viewer's run left no outline artifact");
    assert_eq!(a.bbox, Some(bbox));
}

/// How many rows the viewer's tree has for `nodes`: one per node.
fn rows(nodes: &[stepv::topology::Node]) -> usize {
    nodes
        .iter()
        .map(|n| match n {
            stepv::topology::Node::Part { .. } => 1,
            stepv::topology::Node::Assembly { children, .. } => 1 + rows(children),
        })
        .sum()
}

/// Where in `err` the window reported its loading frame with `rows` rows
/// and no geometry; `None` if it did not.
fn loading_frame(err: &str, rows: usize) -> Option<usize> {
    let head = format!("stepv: STEPV_VIEW_SCREENSHOT loading: tree {rows} rows, ");
    let at = err.find(&head)?;
    let line = err[at..].lines().next()?;
    line.ends_with(", no geometry").then_some(at)
}

#[test]
#[ignore = "needs a window session; run by just view-test"]
fn the_viewer_shows_the_tree_before_the_geometry() {
    let k = kernel();
    let h = Home::new("viewtreefirst");
    let input = data("assembly.step");
    let outline = occt::outline(&k, &input, occt::Limits::DEFAULT).unwrap();
    // The kernel holds the run 8 s once the outline is out: the window is
    // open on the tree meanwhile, with nothing to draw yet.
    let (code, j, err) = h.view_within(
        &input,
        &[],
        &[("STEPV_OCCT_TEST_STALL_MS", "8000")],
        VIEW_DEADLINE,
    );
    assert_eq!((code, j["status"].as_str()), (0, Some("ok")), "{j}");
    let tree = loading_frame(&err, rows(&outline.tree)).expect("no loading frame with the tree");
    // Then the scene, swapped in on Done.
    let scene = err
        .find(&format!(
            "stepv: STEPV_VIEW_SCREENSHOT scene: {} parts",
            j["kernel"]["parts"]
        ))
        .expect("no scene");
    assert!(tree < scene, "{err}");
}

#[test]
#[ignore = "needs a window session; run by just view-test"]
fn a_cached_outline_shows_the_tree_before_any_kernel_event() {
    let k = kernel();
    let h = Home::new("viewcached");
    let input = h.0.join("m.step");
    std::fs::copy(data("assembly.step"), &input).unwrap();
    let outline = occt::outline(&k, &input, occt::Limits::DEFAULT).unwrap();
    let n = rows(&outline.tree);
    let a = cache::OutlineArtifact::new(stepv::header::read(&input), outline);
    let file = cache::FileId::of(&input).unwrap();
    cache::store_outline(&cache::outline_path(&h.cache_dir(), &file), &a).unwrap();
    // A kernel that never says a word: any tree came from the cache.
    let mute = script(&h, "mute-kernel", "exec sleep 60");
    let (code, j, err) = h.view_within(
        &input,
        &["--timeout", "5"],
        &[("STEPV_OCCT", mute.to_str().unwrap())],
        VIEW_DEADLINE,
    );
    assert!(loading_frame(&err, n).is_some(), "{err}");
    assert!(!err.contains("STEPV_VIEW_SCREENSHOT scene:"), "{err}");
    // The run timed out: reported as before (#89 keeps the window open).
    assert_eq!((code, j["status"].as_str()), (4, Some("timeout")), "{j}");
}

#[test]
#[ignore = "needs a window session; run by just view-test"]
fn closing_the_viewer_mid_load_cancels_the_kernel() {
    let k = kernel();
    let h = Home::new("viewcancel");
    // The real kernel, saying who it is, and stalled 30 s after the outline.
    let pidfile = h.0.join("kernel.pid");
    let stalled = script(
        &h,
        "stalled-kernel",
        &format!(
            "echo $$ > '{}'\nSTEPV_OCCT_TEST_STALL_MS=30000 exec '{}' \"$@\"",
            pidfile.display(),
            k.display()
        ),
    );
    let start = Instant::now();
    // The window closes itself once it has drawn the tree.
    let (code, j, _) = h.view_within(
        &data("assembly.step"),
        &[],
        &[
            ("STEPV_OCCT", stalled.to_str().unwrap()),
            ("STEPV_VIEW_CLOSE_LOADING", "1"),
        ],
        VIEW_DEADLINE,
    );
    assert_eq!((code, j["status"].as_str()), (0, Some("cancelled")), "{j}");
    assert!(
        start.elapsed() < Duration::from_secs(20),
        "the stall was waited out"
    );
    let pid: libc::pid_t = std::fs::read_to_string(&pidfile)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    // SAFETY: signal 0 only asks whether the process exists.
    assert!(
        unsafe { libc::kill(pid, 0) } != 0,
        "the kernel {pid} outlived the viewer"
    );
}
