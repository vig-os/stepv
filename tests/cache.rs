//! The cache (#62) end to end: only complete results are cached, an entry
//! that is truncated or of another version is regenerated, and a run killed
//! after its outline leaves the outline artifact but no entry. Run the real
//! kernel against `tests/data/`.

use std::os::unix::fs::PermissionsExt;
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

#[test]
fn a_kernel_exit_0_without_a_summary_fails_and_is_not_cached() {
    let k = kernel();
    let h = Home::new("nosummary");
    // The real kernel writes the mesh; its summary goes nowhere.
    let mute = h.0.join("mute-kernel");
    std::fs::write(
        &mute,
        format!("#!/bin/sh\n'{}' \"$@\" >/dev/null\n", k.display()),
    )
    .unwrap();
    std::fs::set_permissions(&mute, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (code, j) = h.png(
        &data("assembly.step"),
        &[],
        &[("STEPV_OCCT", mute.to_str().unwrap())],
    );
    // A cached run is streamed, and a stream that ends without its summary
    // breaks the contract: a crash (`occt::run_streaming`).
    assert_eq!((code, j["status"].as_str()), (3, Some("crashed")), "{j}");
    h.assert_nothing_cached("no summary");
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
    cache::store_outline_in(&h.cache_dir(), &input, &a).unwrap();
    assert_eq!(cache::load_outline_in(&h.cache_dir(), &input), Some(a));
    // Rewritten: another length, so another key.
    let mut bytes = std::fs::read(&input).unwrap();
    bytes.extend_from_slice(b"\n");
    std::fs::write(&input, bytes).unwrap();
    assert_eq!(cache::load_outline_in(&h.cache_dir(), &input), None);
}
