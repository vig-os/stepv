//! stepv builds OCCT 7.9.3 with one patch (#74,
//! nix/patches/occt-7.9.3-fixselfintersectwire-hoist.patch):
//! `ShapeFix_IntersectionTool::FixSelfIntersectWire` fetched `edge1` from the
//! wire's sequence on every inner iteration, an O(n) walk each time, so a long
//! wire healed in O(n^3). These run `occt-patch-check` (kernel/), built beside
//! the kernel against the same OCCT, and hold that OCCT to the patch: faster,
//! and otherwise identical.

use std::path::{Path, PathBuf};
use std::process::Command;

fn check() -> PathBuf {
    let k = stepv::occt::kernel_path().with_file_name("occt-patch-check");
    assert!(
        k.is_file(),
        "occt-patch-check not built next to the kernel: {} (run `just kernel`, which configures -DSTEPV_BUILD_TESTS=ON)",
        k.display()
    );
    k
}

fn run(args: &[&str]) -> String {
    let out = Command::new(check())
        .args(args)
        .output()
        .expect("spawn occt-patch-check");
    assert!(
        out.status.success(),
        "occt-patch-check {args:?}: {}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8")
}

/// The growth exponent of `FixSelfIntersectWire` from an 800-gon to a
/// 3200-gon: about 2 patched, about 3 unpatched (2.00 to 2.01 and 3.00 to 3.06
/// measured on OCCT 7.9.3 from nixpkgs). A ratio of two CPU times in one process, so load or a slow
/// runner moves both and not the exponent; 2.5 sits halfway.
#[test]
fn fix_self_intersect_wire_is_quadratic() {
    let line = run(&["scaling", "800"]);
    let exponent: f64 = line
        .split("\"exponent\":")
        .nth(1)
        .and_then(|s| s.trim().trim_end_matches('}').parse().ok())
        .unwrap_or_else(|| panic!("no exponent in {line}"));
    assert!(
        exponent < 2.5,
        "FixSelfIntersectWire grows as n^{exponent:.2}: this OCCT lacks the #74 patch ({line})"
    );
}

/// 200 generated self-intersecting wires, 91 of them through the big-segment
/// branch where the patch re-fetches `edge1`: the patched OCCT must fix each
/// exactly as the unpatched one did (`tests/occt-patch/wires.txt`, recorded
/// with nixpkgs' unpatched OCCT 7.9.3). A hoist without the re-fetch differs
/// on 39 of them.
#[test]
fn fix_self_intersect_wire_matches_unpatched_occt() {
    let reference = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/occt-patch/wires.txt"),
    )
    .expect("tests/occt-patch/wires.txt");
    let got = run(&["wires"]);
    let pairs: Vec<(&str, &str)> = reference
        .lines()
        .zip(got.lines())
        .filter(|(r, g)| r != g)
        .collect();
    let differ: Vec<&str> = pairs
        .iter()
        .map(|(r, _)| r.split(' ').next().unwrap_or(""))
        .collect();
    let first = pairs
        .first()
        .map(|(r, g)| format!("\nfirst difference:\n  reference {r}\n  got       {g}"))
        .unwrap_or_default();
    assert!(
        differ.is_empty() && reference.lines().count() == got.lines().count(),
        "FixSelfIntersectWire differs from unpatched OCCT on seeds {differ:?} \
         ({} lines, reference {}){first}",
        got.lines().count(),
        reference.lines().count()
    );
}
