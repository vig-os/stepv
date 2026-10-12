//! `stepv-occt --stream` (#54): a run whose stdout is a stream of framed
//! events (the outline, then progress, then the summary) instead of one JSON
//! line, read by `occt::run_streaming`. These run the real kernel against
//! `tests/data/`, and fake kernels (shell scripts) for the broken streams a
//! real one never writes.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use stepv::Deflection;
use stepv::occt::{self, Event, Limits, Outcome, Run, Stage};

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

fn limits() -> Limits {
    Limits {
        timeout: Duration::from_secs(60),
        memory: Some(2 << 30),
    }
}

/// A scratch directory unique to one test, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("stepv-stream-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// An executable shell script standing in for the kernel.
    fn script(&self, name: &str, body: &str) -> PathBuf {
        let p = self.path(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn stream(kernel: &Path, input: &Path, limits: Limits, mesh: Option<&Path>) -> (Run, Vec<Event>) {
    let mut events = Vec::new();
    let run = occt::run_streaming(
        kernel,
        input,
        Deflection::PREVIEW,
        limits,
        mesh,
        None,
        false,
        &mut |e| events.push(e),
    )
    .unwrap();
    (run, events)
}

/// Progress only ever moves forward: stages in order, and within a stage
/// `done` never falls and never passes `total`.
fn assert_monotonic(events: &[Event]) {
    let mut last: Option<(Stage, u64)> = None;
    for e in events {
        let Event::Progress(p) = e else { continue };
        assert!(p.done <= p.total, "{p:?}");
        if let Some((stage, done)) = last {
            assert!(
                p.stage > stage || (p.stage == stage && p.done >= done),
                "{p:?} after {stage:?} {done}"
            );
        }
        last = Some((p.stage, p.done));
    }
}

#[test]
fn a_streamed_run_shows_the_outline_then_progress_then_the_summary() {
    let t = Scratch::new("assembly");
    let input = data("assembly.step");
    let (run, events) = stream(&kernel(), &input, limits(), Some(&t.path("m.bin")));
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.summary);

    // The outline comes first, and it is the one --outline reads.
    let Some(Event::Tree(tree)) = events.first() else {
        panic!("first event is not the outline: {events:?}");
    };
    assert_eq!(*tree, occt::outline(&kernel(), &input, limits()).unwrap());
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::Tree(_)))
            .count(),
        1
    );

    // Then progress through the transfer and the meshing, each finished.
    assert_monotonic(&events);
    for stage in [Stage::Transfer, Stage::Mesh] {
        let last = events
            .iter()
            .filter_map(|e| match e {
                Event::Progress(p) if p.stage == stage => Some(p),
                _ => None,
            })
            .next_back()
            .unwrap_or_else(|| panic!("no {stage:?} progress: {events:?}"));
        assert_eq!(last.done, last.total, "{stage:?} never finished");
    }

    // The summary and mesh are the ones a plain run produces.
    let plain = occt::run(
        &kernel(),
        &input,
        Deflection::PREVIEW,
        limits(),
        Some(&t.path("plain.bin")),
        None,
        false,
    )
    .unwrap();
    let (s, p) = (run.summary.unwrap(), plain.summary.unwrap());
    assert_eq!(
        (s.parts, s.faces, s.triangles, s.bbox, s.sandbox),
        (p.parts, p.faces, p.triangles, p.bbox, p.sandbox)
    );
    let a = occt::read_mesh(&std::fs::read(t.path("m.bin")).unwrap()).unwrap();
    let b = occt::read_mesh(&std::fs::read(t.path("plain.bin")).unwrap()).unwrap();
    assert_eq!(a.parts.len(), b.parts.len());
    assert_eq!(a.triangle_count(), b.triangle_count());
}

#[test]
fn a_file_without_product_structure_streams_no_outline() {
    for f in ["box.brep", "box.igs"] {
        let (run, events) = stream(&kernel(), &data(f), limits(), None);
        assert_eq!(run.outcome, Outcome::Ok, "{f}: {:?}", run.summary);
        assert!(
            !events.iter().any(|e| matches!(e, Event::Tree(_))),
            "{f}: {events:?}"
        );
        assert_monotonic(&events);
        assert!(run.summary.unwrap().ok, "{f}");
    }
}

#[test]
fn a_clean_failure_still_streams_its_summary() {
    let t = Scratch::new("noise");
    let input = t.path("noise.step");
    std::fs::write(&input, b"ISO-10303-21;\nnot really\n").unwrap();
    let (run, events) = stream(&kernel(), &input, limits(), None);
    assert_eq!(run.outcome, Outcome::Failed);
    let s = run.summary.expect("a failed run still has its summary");
    assert!(!s.ok && s.error.is_some(), "{s:?}");
    assert!(!events.iter().any(|e| matches!(e, Event::Tree(_))));
}

#[test]
fn a_run_killed_after_the_outline_keeps_the_outline_but_no_summary() {
    let t = Scratch::new("stall");
    // The real kernel, stalled for 30 s once the outline is out.
    let stalled = t.script(
        "stalled",
        &format!(
            "STEPV_OCCT_TEST_STALL_MS=30000 exec '{}' \"$@\"",
            kernel().display()
        ),
    );
    let input = data("assembly.step");
    let (run, events) = stream(
        &stalled,
        &input,
        Limits {
            timeout: Duration::from_secs(3),
            memory: None,
        },
        None,
    );
    assert_eq!(run.outcome, Outcome::Timeout);
    assert!(run.summary.is_none(), "a killed run is not complete");
    assert!(
        matches!(events.as_slice(), [Event::Tree(_)]),
        "only the outline arrived: {events:?}"
    );
    assert!(run.wall < Duration::from_secs(20), "{:?}", run.wall);
}

#[test]
fn a_broken_stream_is_a_crash_not_a_result() {
    let t = Scratch::new("broken");
    let input = data("assembly.step");
    for (name, body) in [
        ("silent", "exit 0"),
        ("garbage", "printf 'not a frame at all'; exit 0"),
        // A Done frame cut off inside its payload.
        (
            "cut",
            r#"printf 'SVEV\003\100\000\000\000{"ok":true'; exit 0"#,
        ),
        // A frame claiming 4 GiB: refused before it is allocated.
        ("huge", r"printf 'SVEV\002\377\377\377\377'; exit 0"),
        // A kind outside the contract.
        ("kind", r"printf 'SVEV\177\002\000\000\000{}'; exit 0"),
        // A clean failure exit, but its stream ends without the summary.
        ("nodone", "exit 3"),
    ] {
        let fake = t.script(name, body);
        let (run, _) = stream(&fake, &input, limits(), None);
        assert!(
            matches!(run.outcome, Outcome::Crashed { .. }),
            "{name}: {:?}",
            run.outcome
        );
        assert!(run.summary.is_none(), "{name}");
    }
}

#[test]
fn stream_refuses_serve_and_outline() {
    for flag in ["--serve", "--outline"] {
        let out = Command::new(kernel())
            .arg(data("assembly.step"))
            .arg("--stream")
            .arg(flag)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "--stream {flag}");
        assert!(out.stdout.is_empty(), "--stream {flag}");
    }
}

#[test]
fn a_slow_reader_still_sees_every_stage_finish() {
    // A front-end busy building its tree UI when the outline arrives.
    let mut events = Vec::new();
    let run = occt::run_streaming(
        &kernel(),
        &data("assembly.step"),
        Deflection::PREVIEW,
        limits(),
        None,
        None,
        false,
        &mut |e| {
            if matches!(e, Event::Tree(_)) {
                std::thread::sleep(Duration::from_millis(500));
            }
            events.push(e);
        },
    )
    .unwrap();
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.summary);
    assert_monotonic(&events);
    for stage in [Stage::Transfer, Stage::Mesh] {
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::Progress(p) if p.stage == stage && p.done == p.total)),
            "{stage:?} never finished: {events:?}"
        );
    }
}
