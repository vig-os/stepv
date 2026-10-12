//! `stepv-occt --outline` (#54): the assembly tree and names straight after
//! the STEP file is parsed, before any geometry is transferred. Its contract
//! is that it is the product-level tree (`topology::products`) of the one
//! `--topology` writes after the full transfer, so a front-end can show it
//! first. These run the real kernel against `tests/data/`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use stepv::Deflection;
use stepv::occt::{self, Limits, Outcome};
use stepv::topology::{self, Topology};

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

/// The tree `--topology` writes after a full run.
fn topology_of(input: &Path) -> Topology {
    let dir = std::env::temp_dir().join(format!(
        "stepv-outline-{}-{}",
        std::process::id(),
        input.file_name().unwrap().to_string_lossy()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let topo = dir.join("t.json");
    let run = occt::run(
        &kernel(),
        input,
        Deflection::PREVIEW,
        limits(),
        Some(&dir.join("m.bin")),
        Some(&topo),
        false,
    )
    .unwrap();
    assert_eq!(run.outcome, Outcome::Ok, "{input:?}: {:?}", run.summary);
    let t = Topology::parse(&std::fs::read(&topo).unwrap()).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    t
}

#[test]
fn the_outline_is_the_product_tree_topology_writes_after_the_transfer() {
    for f in [
        "assembly.step",
        "sketch.step",
        "multifile/plate.stp",
        "multifile/bracket.step",
        "multifile/pin.stp",
    ] {
        let input = data(f);
        let full = topology_of(&input);
        let outline =
            occt::outline(&kernel(), &input, limits()).unwrap_or_else(|e| panic!("{f}: {e}"));
        let products = topology::products(&full.tree);
        assert_eq!(outline.tree, products, "{f}: tree");
        let mut leaves = 0;
        fn count(n: &topology::Node, k: &mut usize) {
            match n {
                topology::Node::Part { .. } => *k += 1,
                topology::Node::Assembly { children, .. } => {
                    children.iter().for_each(|c| count(c, k))
                }
            }
        }
        products.iter().for_each(|n| count(n, &mut leaves));
        assert_eq!(outline.leaves, leaves, "{f}: leaf count");
    }
}

#[test]
fn the_outline_names_every_part_of_the_assembly() {
    // tests/data/assembly.step: a plate and two instances of one pin.
    let o = occt::outline(&kernel(), &data("assembly.step"), limits()).unwrap();
    assert_eq!(o.leaves, 3);
}

#[test]
fn only_step_has_an_outline() {
    // IGES and BREP carry no product structure to read before the transfer.
    for f in ["box.igs", "box.brep"] {
        let e = occt::outline(&kernel(), &data(f), limits()).unwrap_err();
        assert!(e.to_string().contains("STEP"), "{f}: {e}");
    }
}

#[test]
fn a_file_that_is_not_step_fails_cleanly() {
    let dir = std::env::temp_dir().join(format!("stepv-outline-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bad = dir.join("noise.step");
    std::fs::write(&bad, b"this is not ISO-10303-21 at all").unwrap();
    let e = occt::outline(&kernel(), &bad, limits()).unwrap_err();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(!e.to_string().is_empty());
}

#[test]
fn outline_takes_no_outputs_and_not_serve_either() {
    // Refused before anything is created: an output named here would
    // otherwise be truncated on the way to the usage error.
    let dir = std::env::temp_dir().join(format!("stepv-outline-args-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mesh = dir.join("never.bin");
    let input = data("assembly.step");
    for args in [
        vec!["--outline", "--serve"],
        vec!["--outline", "--mesh", mesh.to_str().unwrap()],
        vec!["--outline", "--topology", mesh.to_str().unwrap()],
    ] {
        let st = std::process::Command::new(kernel())
            .arg(&input)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert_eq!(st.status.code(), Some(2), "{args:?}");
        assert!(!mesh.exists(), "{args:?} created an output");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
