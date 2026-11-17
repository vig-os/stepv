//! `graph::fits` (ADR-0001 §2.8, G6c): a leaf's placeholder as a fitted
//! cylinder or sphere. Every fit must contain the exact geometry, as the
//! kernel triangulates it: every node of OCCT's mesh of the leaf and every
//! point of its face edges' polylines, placed, lies inside the leaf's fit.
//! Every kernel run goes through `stepv-cap` at 4 GiB, one at a time.
//!
//! The fixtures are written here: a ball (sphere), a shaft and a bracket
//! (the bracket assembly's pin and plate, from `kernel/fixture-gen.cpp`),
//! axes misaligned past 10× the uncertainty, and a shaft whose inscribed
//! volume would beat its box while its drawn prism doesn't. `STEPV_CORPUS`
//! (paths, `:`-separated, files or directories) adds the local corpus.

mod corpus;
mod stepgen;

use std::path::{Path, PathBuf};

use corpus::{corpus_files, kernel, points};
use stepgen::{F, part_file};
use stepv::graph::{self, Bound, Fit, Graph};
use stepv::topology;

/// What the files gave.
#[derive(Default)]
struct Tally {
    /// Leaves with a box, and those drawn as a fit.
    leaves: usize,
    cylinders: usize,
    spheres: usize,
    /// Mesh and edge points compared against a fit.
    points: usize,
    violations: Vec<String>,
}

fn check(input: &Path, all: &mut Tally) -> Result<(), String> {
    let g = Graph::open(input).map_err(|e| format!("graph: {e}"))?;
    let s = graph::structure(&g).map_err(|e| format!("structure: {e}"))?;
    let b = graph::bounds(&g, &s);
    let f = graph::fits(&g, &b);
    let (scene, topo) = kernel(input)?;
    if scene.parts.len() != topo.parts.len() {
        return Err("mesh and topology parts differ".into());
    }
    let file = input.file_name().unwrap().to_string_lossy().into_owned();
    for (leaf, parts) in topology::parts_per_leaf(&topo.tree).iter().enumerate() {
        if matches!(b.leaf(leaf as u64), Bound::Unknown(_)) {
            continue;
        }
        all.leaves += 1;
        let Some(fit) = f.leaf(&b, leaf as u64) else {
            continue;
        };
        let size = match fit {
            Fit::Cylinder { length, radius, .. } => {
                all.cylinders += 1;
                length + radius
            }
            Fit::Sphere { radius, .. } => {
                all.spheres += 1;
                radius
            }
        };
        for &part in parts {
            let proto = &topo.prototypes[topo.parts[part].prototype];
            for q in points(&scene, part, proto) {
                all.points += 1;
                // The mesh's f32 rounding, as the box gate allows it.
                let slack = 1e-6 * (size + corpus::norm(q)).max(1.);
                let over = fit.outside(q);
                if over > slack {
                    all.violations.push(format!(
                        "{file} leaf {leaf}: {q:?} {over:.3e} mm outside {fit:?}"
                    ));
                    break;
                }
            }
        }
    }
    Ok(())
}

/// Runs `check` on `files`, reports on stderr, and fails on any point
/// outside its fit or when no fit was compared.
fn run(files: &[PathBuf]) -> Tally {
    let mut all = Tally::default();
    for f in files {
        let stem = f.file_stem().unwrap_or_default();
        // The multi-file assemblies' top files: their parts live in other files.
        if stem == "s1-c5-214" || stem == "s1_pe_214" {
            continue;
        }
        if let Err(e) = check(f, &mut all) {
            eprintln!("{}: not compared: {e}", f.display());
        }
    }
    let fitted = all.cylinders + all.spheres;
    eprintln!(
        "fits: {} files; {fitted} fits ({} cylinders, {} spheres) on {} leaves with a box ({:.1}%), \
         {} points compared, {} violations",
        files.len(),
        all.cylinders,
        all.spheres,
        all.leaves,
        100. * fitted as f64 / all.leaves.max(1) as f64,
        all.points,
        all.violations.len(),
    );
    assert!(
        all.violations.is_empty(),
        "{} outside:\n{}",
        all.violations.len(),
        all.violations.join("\n")
    );
    assert!(fitted > 0, "no fit compared");
    all
}

/// A ball of radius 5 about (1, 2, 3): two hemispheres on one sphere,
/// bounded by the equator.
fn ball() -> PathBuf {
    let mut f = F::new();
    let a = f.ax([1., 2., 3.]);
    let sphere = f.add(&format!("SPHERICAL_SURFACE('',#{a},5.)"));
    let up = f.circle_bound([1., 2., 3.], 5., true);
    let down = f.circle_bound([1., 2., 3.], 5., false);
    let faces = [f.face(&[up], sphere), f.face(&[down], sphere)];
    let item = f.item(&faces, true);
    part_file("ball.step", 1e-7, &item, &f.out)
}

/// The fit and the box of a one-part file's single leaf, and the product's
/// fit before the size test.
fn one_leaf(path: &Path) -> (Option<Fit>, Option<Fit>, graph::Bound) {
    let g = Graph::open(path).unwrap();
    let s = graph::structure(&g).unwrap();
    let b = graph::bounds(&g, &s);
    let f = graph::fits(&g, &b);
    (f.leaf(&b, 0), f.product(0), b.leaf(0))
}

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

/// The ball is a sphere fit: the sphere's radius, inflated by the
/// uncertainty, about its centre; and it holds the kernel's mesh.
#[test]
fn a_ball_is_a_sphere() {
    let ball = ball();
    let (leaf, _, _) = one_leaf(&ball);
    let Some(Fit::Sphere { centre, radius }) = leaf else {
        panic!("{leaf:?}");
    };
    assert_eq!(centre, [1., 2., 3.]);
    assert!((radius - (5. + 1e-7)).abs() < 1e-12, "{radius}");
    let t = run(&[ball]);
    assert_eq!((t.spheres, t.cylinders), (1, 0));
}

/// The bracket assembly: its pin, instanced twice, is a shaft (a cylinder
/// fit: radius 2, length 15, on its own axis), and its plate, a box with a
/// hole whose sides aren't perpendicular to any axis, keeps its box. Every
/// fit holds the kernel's mesh.
#[test]
fn a_shaft_is_a_cylinder_and_a_bracket_keeps_its_box() {
    let path = data("assembly.step");
    let g = Graph::open(&path).unwrap();
    let s = graph::structure(&g).unwrap();
    let b = graph::bounds(&g, &s);
    let f = graph::fits(&g, &b);
    assert_eq!(s.leaves, 3);
    assert_eq!(f.leaf(&b, 0), None, "the plate keeps its box");
    for pin in [1, 2] {
        let Some(Fit::Cylinder { length, radius, .. }) = f.leaf(&b, pin) else {
            panic!("pin {pin}: {:?}", f.leaf(&b, pin));
        };
        assert!((radius - 2.).abs() < 1e-6 && (length - 15.).abs() < 1e-6);
        assert!(
            radius - 2. > 0.99e-7,
            "inflated by the uncertainty: {radius}"
        );
    }
    let t = run(&[path]);
    assert_eq!((t.cylinders, t.spheres, t.leaves), (2, 0, 3));
}

/// Two coaxial cylinders whose axes are `delta` apart, with an uncertainty
/// of 1e-7 mm: within 10× it is one fit, inflated by ε = delta + 1e-7;
/// past it, the axes don't really agree and the box stays.
#[test]
fn axes_misaligned_past_ten_times_the_uncertainty_are_rejected() {
    let stack = |name: &str, delta: f64| {
        let mut f = F::new();
        let mut faces = f.shaft([0., 0., 0.], 3., 10., false);
        faces.extend(f.shaft([delta, 0., 10.], 2., 20., false));
        let item = f.item(&faces, false);
        part_file(name, 1e-7, &item, &f.out)
    };
    let (leaf, product, _) = one_leaf(&stack("aligned.step", 5e-7));
    let Some(Fit::Cylinder { radius, length, .. }) = leaf else {
        panic!("{leaf:?}");
    };
    let eps = 5e-7 + 1e-7;
    assert!((radius - (3. + eps)).abs() < 1e-12, "{radius}");
    assert!((length - (20. + 2. * eps)).abs() < 1e-12, "{length}");
    assert_eq!(leaf, product, "a part at the identity");
    let (leaf, product, bound) = one_leaf(&stack("misaligned.step", 1e-5));
    assert_eq!((leaf, product), (None, None));
    assert!(matches!(bound, Bound::Box(_)), "the box stays: {bound:?}");
    // Either side of the threshold: ε = 9× the uncertainty is used, 11× not.
    let (_, nine, _) = one_leaf(&stack("nine.step", 8e-7));
    assert!(
        matches!(nine, Some(Fit::Cylinder { radius, .. }) if (radius - (3. + 9e-7)).abs() < 1e-12),
        "{nine:?}"
    );
    let (_, eleven, _) = one_leaf(&stack("eleven.step", 1e-6));
    assert_eq!(eleven, None);
}

/// A band of a cylinder of radius 10 whose axis is tilted 10° from the
/// fitted one, cut by the planes z = ±h beside a z cylinder of radius 1 of
/// the same height, at an uncertainty of 1e-3: (leaf fit, product fit, the
/// band's farthest points, its cut ellipse's major vertices).
fn tilted_band(name: &str, h: f64) -> (Option<Fit>, Option<Fit>, Vec<[f64; 3]>) {
    let (r, th) = (10f64, 10f64.to_radians());
    let d = [0., th.sin(), th.cos()];
    let mut f = F::new();
    let mut faces = f.shaft([0., 0., -h], 1., h, false);
    let ax = f.ax_to([0.; 3], d, [1., 0., 0.]);
    let cyl = f.add(&format!("CYLINDRICAL_SURFACE('',#{ax},{r:?})"));
    let major = r / th.cos();
    let (mut bounds, mut far) = (Vec::new(), Vec::new());
    for (z, sense) in [(-h, true), (h, false)] {
        let c = [0., z * th.tan(), z];
        let a = f.ax_to(c, [0., 0., 1.], [0., 1., 0.]);
        let e = f.add(&format!("ELLIPSE('',#{a},{major:?},{r:?})"));
        let p = f.pt([0., c[1] + major, z]);
        let v = f.add(&format!("VERTEX_POINT('',#{p})"));
        let ec = f.add(&format!("EDGE_CURVE('',#{v},#{v},#{e},.T.)"));
        let s = if sense { ".T." } else { ".F." };
        let oe = f.add(&format!("ORIENTED_EDGE('',*,*,#{ec},{s})"));
        let lp = f.add(&format!("EDGE_LOOP('',(#{oe}))"));
        bounds.push(f.add(&format!("FACE_BOUND('',#{lp},.T.)")));
        far.extend([1., -1.].map(|sign| [0., c[1] + sign * major, z]));
    }
    faces.push(f.face(&bounds, cyl));
    let item = f.item(&faces, false);
    let (leaf, product, _) = one_leaf(&part_file(name, 1e-3, &item, &f.out));
    (leaf, product, far)
}

/// Round 2's blocker (PR #195 review): the band at h = 0.01. Its axis
/// strays only 1.8e-3 over the extent, but its cut reaches r / cos 10° ≈
/// 10.154 from the fitted axis, so a radius of r + ε (f062231) left 0.15 mm
/// of it outside. Bounded by its edges, the fit holds it.
#[test]
fn a_tilted_cylinder_face_never_sticks_out() {
    let (leaf, product, far) = tilted_band("tilted-band.step", 0.01);
    let fit = product.expect("a drift of sin 10° · 0.02 is within 10× 1e-3");
    for fit in [Some(fit), leaf].into_iter().flatten() {
        for &q in &far {
            assert!(fit.outside(q) <= 0., "{q:?} outside {fit:?}");
        }
    }
}

/// §2.8's angle criterion: the same band at h = 0.0403 strays 7.1e-3 from
/// the fitted axis (ε = 8.1× the uncertainty, within 10×), but its axis
/// drifts sin 10° · 0.0806 = 1.4e-2 over the extent, past 10×: rejected.
#[test]
fn a_tilt_past_ten_times_the_uncertainty_is_rejected() {
    let (leaf, product, _) = tilted_band("tilted-far.step", 0.0403);
    assert_eq!((leaf, product), (None, None));
}

/// §2.8's size test compares what is drawn. A shaft of radius 1 and
/// length 1 with an uncertainty u: its fit is r + u by 1 + 2u, and its box
/// 2 × 2 × 1. With u chosen so that the cylinder of the fit's radius (π R²
/// h) is smaller than the box but the drawn prism (n R² tan(π/n) h) isn't,
/// the box stays; with u = 1e-7 the fit wins.
#[test]
fn the_size_test_compares_the_drawn_volume() {
    let n = 32.;
    let drawn = n * (std::f64::consts::PI / n).tan();
    // (1 + u)² (1 + 2u) midway between 4 / drawn and 4 / π.
    let target = (4. / drawn + 4. / std::f64::consts::PI) / 2.;
    let (mut lo, mut hi) = (0f64, 0.1f64);
    for _ in 0..60 {
        let u = (lo + hi) / 2.;
        if (1. + u).powi(2) * (1. + 2. * u) < target {
            lo = u;
        } else {
            hi = u;
        }
    }
    let shaft = |name: &str, u: f64| {
        let mut f = F::new();
        let faces = f.shaft([0., 0., 0.], 1., 1., true);
        let item = f.item(&faces, true);
        part_file(name, u, &item, &f.out)
    };
    let (leaf, product, bound) = one_leaf(&shaft("tolerant.step", lo));
    let Bound::Box(bb) = bound else {
        panic!("{bound:?}")
    };
    let boxed: f64 = (0..3).map(|i| bb.max[i] - bb.min[i]).product();
    assert!((boxed - 4.).abs() < 1e-9, "{boxed}");
    let fit = product.expect("within the tolerance, the shaft fits");
    let Fit::Cylinder { length, radius, .. } = fit else {
        panic!("{fit:?}")
    };
    let inscribed = std::f64::consts::PI * radius * radius * length;
    assert!(inscribed < boxed, "{inscribed} vs {boxed}");
    assert!(
        fit.drawn_volume() >= boxed,
        "{} vs {boxed}",
        fit.drawn_volume()
    );
    assert_eq!(leaf, None, "the drawn prism is larger than the box");
    let (leaf, _, _) = one_leaf(&shaft("precise.step", 1e-7));
    assert!(leaf.is_some_and(|f| f.drawn_volume() < boxed), "{leaf:?}");
}

/// The drawn shapes are what the size test measures, and circumscribed: a
/// prism's sides and a polyhedron's facets lie at least the fit's radius
/// from its axis or centre.
#[test]
fn the_drawn_shape_is_circumscribed() {
    let fits = [
        Fit::Cylinder {
            base: [1., 2., 3.],
            axis: [0., 0.6, 0.8],
            length: 7.,
            radius: 2.,
        },
        Fit::Sphere {
            centre: [-1., 0., 4.],
            radius: 3.,
        },
    ];
    for fit in fits {
        let (tris, lines) = fit.drawn();
        assert!(!tris.is_empty() && tris.len() % 3 == 0 && lines.len() % 2 == 0);
        let mut volume = 0.;
        let inside = match fit {
            Fit::Cylinder {
                base, axis, length, ..
            } => [0, 1, 2].map(|i| base[i] + axis[i] * length / 2.),
            Fit::Sphere { centre, .. } => centre,
        };
        for t in tris.chunks_exact(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|p| [0, 1, 2].map(|i| p[i] - inside[i]));
            let n = [
                (b[1] - a[1]) * (c[2] - a[2]) - (b[2] - a[2]) * (c[1] - a[1]),
                (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]),
                (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]),
            ];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            let d = (n[0] * a[0] + n[1] * a[1] + n[2] * a[2]).abs();
            volume += d / 6.;
            // A side lies at least the radius from the axis, a cap half the
            // length from the middle, a facet the radius from the centre.
            let need = match fit {
                Fit::Cylinder {
                    axis,
                    length,
                    radius,
                    ..
                } => {
                    let along = (n[0] * axis[0] + n[1] * axis[1] + n[2] * axis[2]).abs() / len;
                    if along > 0.5 { length / 2. } else { radius }
                }
                Fit::Sphere { radius, .. } => radius,
            };
            assert!(
                d / len >= need * (1. - 1e-12),
                "{fit:?}: a facet at {}",
                d / len
            );
        }
        let want = fit.drawn_volume();
        assert!((volume - want).abs() < 1e-9 * want, "{volume} vs {want}");
    }
}

/// The local corpus, when `STEPV_CORPUS` names it: every fit against the
/// kernel's mesh, failing on any point outside or when it compared no fit.
/// CI runs it on `tests/fixtures/nist-pmi` and `occt-import-js`
/// (`.github/workflows/kernel.yml`).
#[test]
fn the_corpus_fits_contain_the_kernels() {
    let Some(files) = corpus_files("the_corpus_fits_contain_the_kernels") else {
        return;
    };
    run(&files);
}
