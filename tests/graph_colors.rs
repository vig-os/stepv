//! `graph::colors` (ADR-0001 §2.2, G7): the graph's product and face colours
//! equal what OCCT's XCAF reports, leaf by leaf. Every kernel run goes
//! through `stepv-cap` at 4 GiB, one at a time.
//!
//! **The face join.** The kernel reports a part's faces in its transfer's
//! order with no STEP ids (those are M5's). [`Colors::faces`] lists a
//! product's faces in the order the transfer explores them, so a leaf's
//! faces are joined by position, and the join is *checked*, never assumed:
//! the counts must agree and every kernel mesh vertex of a face must lie in
//! the graph's conservative box of the face joined to it (G4). Healing can
//! split, drop or reorder faces; such a leaf fails the check and is compared
//! by its set of face colours instead, and counted apart.
//!
//! The fixtures are written here, one per covered style case. `STEPV_CORPUS`
//! (paths, `:`-separated, files or directories) adds the local corpus.

mod corpus;
mod stepgen;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use corpus::{apply, corpus_files, invert, kernel};
use stepgen::{F, part_file};
use stepv::Color;
use stepv::graph::{self, Bound, Graph};
use stepv::topology;

/// What the files gave.
#[derive(Default, Debug)]
struct Tally {
    leaves: usize,
    /// Leaves XCAF colours: a part colour or a face colour.
    coloured: usize,
    /// Multi-file leaves, whose parts the graph doesn't read.
    external: usize,
    /// Leaves XCAF splits into parts unlike the graph, compared by each
    /// face's drawn colour.
    split: usize,
    /// Parts whose faces joined, and the faces compared on them.
    joined: usize,
    faces: usize,
    /// Faces whose join a box checked; the rest only by count.
    boxed: usize,
    /// Parts compared by their sets of face colours.
    unjoined: usize,
    mismatches: Vec<String>,
}

/// The kernel's f32 against the graph's: equal to the float.
fn same(a: Option<Color>, b: Option<Color>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            let d = (a.r - b.r)
                .abs()
                .max((a.g - b.g).abs())
                .max((a.b - b.b).abs());
            d <= 1e-6
        }
        _ => false,
    }
}

fn key(c: Option<Color>) -> Option<[u32; 3]> {
    c.map(|c| [c.r, c.g, c.b].map(|x| (x * 1e5).round() as u32))
}

/// A kernel part's faces' mesh vertices, in its product's coordinates.
fn face_points(part: &stepv::Part, placement: &[f64; 12]) -> Vec<Vec<[f64; 3]>> {
    let back = invert(placement);
    let mut points = vec![Vec::new(); part.faces.len()];
    for (t, &f) in part.mesh.face_ids.iter().enumerate() {
        for &v in &part.mesh.indices[t * 3..t * 3 + 3] {
            let x = &part.mesh.positions[v as usize * 3..v as usize * 3 + 3];
            points[f as usize].push(apply(&back, [x[0], x[1], x[2]].map(f64::from)));
        }
    }
    points
}

/// Joins the kernel's faces (colour, mesh vertices) to the graph's (face,
/// colour) by position when the join checks out, else compares their sets
/// of colours; returns the mismatches.
fn compare(
    g: &Graph,
    b: &graph::Bounds,
    kernel: &[(Option<Color>, Vec<[f64; 3]>)],
    graph: &[(graph::Node, Option<Color>)],
    all: &mut Tally,
) -> Vec<String> {
    let inside = |face: graph::Node, pts: &[[f64; 3]]| match b.node(face) {
        Bound::Box(bb) => {
            let size = (0..3).map(|k| bb.max[k] - bb.min[k]).fold(0., f64::max);
            Some(pts.iter().all(|q| {
                let slack = 1e-6 * (size + corpus::norm(*q)).max(1.);
                (0..3).all(|k| q[k] >= bb.min[k] - slack && q[k] <= bb.max[k] + slack)
            }))
        }
        Bound::Unknown(_) => None,
    };
    let checks: Vec<Option<bool>> = if graph.len() == kernel.len() {
        graph
            .iter()
            .zip(kernel)
            .map(|(f, k)| inside(f.0, &k.1))
            .collect()
    } else {
        vec![Some(false)]
    };
    let mut out = Vec::new();
    if checks.iter().all(|c| *c != Some(false)) {
        all.joined += 1;
        all.faces += graph.len();
        all.boxed += checks.iter().filter(|c| c.is_some()).count();
        for ((face, mine), (theirs, _)) in graph.iter().zip(kernel) {
            if !same(*theirs, *mine) {
                let id = g.id(*face).0;
                out.push(format!("face #{id}: kernel {theirs:?}, graph {mine:?}"));
            }
        }
    } else {
        all.unjoined += 1;
        let kset: BTreeSet<_> = kernel.iter().map(|k| key(k.0)).collect();
        let gset: BTreeSet<_> = graph.iter().map(|f| key(f.1)).collect();
        if kset != gset {
            out.push(format!("face colours {kset:?}, graph {gset:?} (unjoined)"));
        }
    }
    out
}

fn check(input: &Path, all: &mut Tally) -> Result<(), String> {
    let g = Graph::open(input).map_err(|e| format!("graph: {e}"))?;
    let s = graph::structure(&g).map_err(|e| format!("structure: {e}"))?;
    let b = graph::bounds(&g, &s);
    let colors = graph::colors(&g);
    let (scene, topo) = kernel(input)?;
    if scene.parts.len() != topo.parts.len() {
        return Err("mesh and topology parts differ".into());
    }
    // A multi-file assembly's parts live in other files, which the kernel
    // reads and the graph doesn't (ADR §2.2).
    let multi_file = g.of_type("DOCUMENT_FILE").next().is_some();
    let file = input.file_name().unwrap().to_string_lossy().into_owned();
    for (leaf, parts) in topology::parts_per_leaf(&topo.tree).iter().enumerate() {
        let Some(path) = s.leaf_path(leaf as u64) else {
            return Err(format!("leaf {leaf} has no graph path"));
        };
        let p = path.0.last().unwrap().0;
        let mine = colors.parts(&g, &s, p);
        if multi_file && mine.iter().all(|m| m.faces.is_empty()) {
            all.external += 1;
            continue;
        }
        all.leaves += 1;
        let coloured = parts.iter().any(|&i| {
            let part = &scene.parts[i];
            part.color.is_some() || part.faces.iter().any(|f| f.color.is_some())
        });
        all.coloured += usize::from(coloured);
        let mut found = Vec::new();
        let kernel_faces = |i: usize, drawn: bool| {
            let part = &scene.parts[i];
            let points = face_points(part, &topo.parts[i].transform);
            let colour = move |c: Option<Color>| if drawn { c.or(part.color) } else { c };
            part.faces.iter().map(move |f| colour(f.color)).zip(points)
        };
        if mine.len() == parts.len() {
            if parts.len() == 1 && !same(scene.parts[parts[0]].color, s.products[p].color) {
                found.push(format!("product colour {:?}", s.products[p].color));
            }
            for (&i, m) in parts.iter().zip(&mine) {
                if !same(scene.parts[i].color, m.color) {
                    let k = scene.parts[i].color;
                    found.push(format!("part {i} colour {k:?}, graph {:?}", m.color));
                }
                let kernel: Vec<_> = kernel_faces(i, false).collect();
                let graph: Vec<_> = m.faces.iter().map(|f| (f.face, f.color)).collect();
                found.extend(compare(&g, &b, &kernel, &graph, all));
            }
        } else {
            // XCAF split the product in a way the graph doesn't model: every
            // face's drawn colour, its own or its part's, over the leaf.
            all.split += 1;
            let kernel: Vec<_> = parts.iter().flat_map(|&i| kernel_faces(i, true)).collect();
            let graph: Vec<_> = mine
                .iter()
                .flat_map(|m| m.faces.iter().map(|f| (f.face, f.color.or(m.color))))
                .collect();
            found.extend(compare(&g, &b, &kernel, &graph, all));
        }
        let prefix = format!("{file} leaf {leaf}: ");
        all.mismatches
            .extend(found.into_iter().map(|m| prefix.clone() + &m));
    }
    Ok(())
}

/// Runs `check` on `files` and reports on stderr. `Err` when a file isn't
/// compared.
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
    eprintln!(
        "colours: {} files; {} leaves ({} multi-file ones skipped), {} coloured by XCAF, {} split \
         unlike the graph (drawn colours compared); parts: {} joined ({} faces, {} checked by box), \
         {} compared by colour set; {} mismatches",
        files.len(),
        all.leaves,
        all.external,
        all.coloured,
        all.split,
        all.joined,
        all.faces,
        all.boxed,
        all.unjoined,
        all.mismatches.len(),
    );
    all
}

/// Fails on any mismatch.
fn parity(files: &[PathBuf]) -> Tally {
    let t = run(files);
    assert!(
        t.mismatches.is_empty(),
        "{} mismatches:\n{}",
        t.mismatches.len(),
        t.mismatches.join("\n")
    );
    t
}

/// The local corpus: every leaf's colours equal XCAF's.
#[test]
fn corpus_colours_equal_xcaf() {
    let Some(files) = corpus_files("graph_colors") else {
        return;
    };
    let t = parity(&files);
    assert!(t.leaves > 0 && t.coloured > 0, "no coloured leaf compared");
}

// ── Fixtures ────────────────────────────────────────────────────────────────

/// `[r, g, b]` in the file's sRGB, as linear RGB.
fn lin(c: [f64; 3]) -> Option<Color> {
    let l = |c: f64| {
        (if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }) as f32
    };
    Some(Color {
        r: l(c[0]),
        g: l(c[1]),
        b: l(c[2]),
    })
}

const BLUE: [f64; 3] = [0.2, 0.4, 0.6];
const RED: [f64; 3] = [1., 0., 0.];
const GREEN: [f64; 3] = [0., 1., 0.];

/// The STEP builder plus styles.
struct S(F);

impl S {
    fn new() -> Self {
        S(F::new())
    }

    fn rgb(&mut self, c: [f64; 3]) -> u32 {
        self.0
            .add(&format!("COLOUR_RGB('',{:?},{:?},{:?})", c[0], c[1], c[2]))
    }

    /// A `side` usage of a fill-area `colour`.
    fn usage(&mut self, colour: u32, side: &str) -> u32 {
        let f = &mut self.0;
        let fc = f.add(&format!("FILL_AREA_STYLE_COLOUR('',#{colour})"));
        let fa = f.add(&format!("FILL_AREA_STYLE('',(#{fc}))"));
        let sfa = f.add(&format!("SURFACE_STYLE_FILL_AREA(#{fa})"));
        let ss = f.add(&format!("SURFACE_SIDE_STYLE('',(#{sfa}))"));
        f.add(&format!("SURFACE_STYLE_USAGE(.{side}.,#{ss})"))
    }

    /// A styled item of `usages` on `item`.
    fn styled_by(&mut self, usages: &[u32], item: u32) -> u32 {
        let u: Vec<String> = usages.iter().map(|u| format!("#{u}")).collect();
        let p = self
            .0
            .add(&format!("PRESENTATION_STYLE_ASSIGNMENT(({}))", u.join(",")));
        self.0.add(&format!("STYLED_ITEM('',(#{p}),#{item})"))
    }

    /// A `.BOTH.` style of colour `c` (sRGB) on `item`.
    fn styled(&mut self, c: [f64; 3], item: u32) -> u32 {
        let c = self.rgb(c);
        let u = self.usage(c, "BOTH");
        self.styled_by(&[u], item)
    }

    /// The styled items, held as the exporters hold them.
    fn held(&mut self, items: &[u32]) {
        let u: Vec<String> = items.iter().map(|u| format!("#{u}")).collect();
        self.0.add(&format!(
            "MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION('',({}),#20)",
            u.join(",")
        ));
    }

    /// A closed, outward-oriented shaft at x: its lateral face, then its
    /// bottom and top caps; the shell and the solid. The healing keeps it as
    /// it is, so the transfer keeps its face order.
    fn solid(&mut self, x: f64) -> (u32, u32, [u32; 3]) {
        let f = &mut self.0;
        let a = f.ax([x, 0., 0.]);
        let cyl = f.add(&format!("CYLINDRICAL_SURFACE('',#{a},2.)"));
        let lo = f.circle_bound([x, 0., 0.], 2., true);
        let hi = f.circle_bound([x, 0., 10.], 2., false);
        let side = f.face(&[lo, hi], cyl);
        let mut caps = [0; 2];
        for (k, (z, b, sense)) in [(0., lo, ".F."), (10., hi, ".T.")].into_iter().enumerate() {
            let a = f.ax([x, 0., z]);
            let plane = f.add(&format!("PLANE('',#{a})"));
            caps[k] = f.add(&format!("ADVANCED_FACE('',(#{b}),#{plane},{sense})"));
        }
        let faces = [side, caps[0], caps[1]];
        let sh = f.add(&format!(
            "CLOSED_SHELL('',(#{},#{},#{}))",
            faces[0], faces[1], faces[2]
        ));
        let so = f.add(&format!("MANIFOLD_SOLID_BREP('',#{sh})"));
        (so, sh, faces)
    }

    fn write(&self, name: &str, items: &[u32]) -> PathBuf {
        let i: Vec<String> = items.iter().map(|i| format!("#{i}")).collect();
        part_file(name, 1e-7, &i.join(","), &self.0.out)
    }
}

/// The one product's colour and its faces' colours.
fn graph_colours(path: &Path) -> (Option<Color>, Vec<Option<Color>>) {
    let g = Graph::open(path).unwrap();
    let s = graph::structure(&g).unwrap();
    let parts = graph::colors(&g).parts(&g, &s, 0);
    let [part] = &parts[..] else {
        panic!("{} parts", parts.len());
    };
    assert_eq!(part.color, s.products[0].color);
    (part.color, part.faces.iter().map(|f| f.color).collect())
}

/// Asserts the graph's colours of `path` and their parity with XCAF, with
/// every face joined.
fn expect(path: &Path, product: Option<Color>, faces: &[Option<Color>]) {
    assert_eq!(graph_colours(path), (product, faces.to_vec()), "{path:?}");
    let t = parity(&[path.to_path_buf()]);
    assert_eq!((t.leaves, t.joined, t.faces), (1, 1, faces.len()), "{t:?}");
}

/// A `STYLED_ITEM` on the part's one `MANIFOLD_SOLID_BREP`, with a
/// `COLOUR_RGB` held in a presentation representation: the part's colour.
#[test]
fn a_styled_solid_colours_its_part() {
    let mut s = S::new();
    let (so, _, _) = s.solid(0.);
    let st = s.styled(BLUE, so);
    s.held(&[st]);
    expect(&s.write("solid.step", &[so]), lin(BLUE), &[None; 3]);
}

/// A `BREP_WITH_VOIDS`, styled: the part's colour, over all six faces.
#[test]
fn a_styled_solid_with_a_void_colours_its_part() {
    let mut s = S::new();
    let (_, outer, _) = s.solid(0.);
    let f = &mut s.0;
    let a = f.ax([0., 0., 2.]);
    let cyl = f.add(&format!("CYLINDRICAL_SURFACE('',#{a},1.)"));
    let lo = f.circle_bound([0., 0., 2.], 1., true);
    let hi = f.circle_bound([0., 0., 8.], 1., false);
    let side = f.face(&[lo, hi], cyl);
    let mut caps = Vec::new();
    for (z, b, sense) in [(2., lo, ".F."), (8., hi, ".T.")] {
        let a = f.ax([0., 0., z]);
        let plane = f.add(&format!("PLANE('',#{a})"));
        caps.push(f.add(&format!("ADVANCED_FACE('',(#{b}),#{plane},{sense})")));
    }
    let inner = f.add(&format!(
        "CLOSED_SHELL('',(#{side},#{},#{}))",
        caps[0], caps[1]
    ));
    let void = f.add(&format!("ORIENTED_CLOSED_SHELL('',*,#{inner},.F.)"));
    let solid = f.add(&format!("BREP_WITH_VOIDS('',#{outer},(#{void}))"));
    let st = s.styled(BLUE, solid);
    s.held(&[st]);
    expect(&s.write("voids.step", &[solid]), lin(BLUE), &[None; 6]);
}

/// Shells: a styled surface model of one shell, or its styled shell, is the
/// part's colour; a styled shell inside a solid colours its faces.
#[test]
fn a_styled_shell_colours_its_part_or_its_faces() {
    for (name, on_shell) in [("model.step", false), ("open-shell.step", true)] {
        let mut s = S::new();
        // The oriented shaft's faces, as an open shell (its own shell and
        // solid stay unreferenced).
        let (_, _, faces) = s.solid(0.);
        let item = s.0.item(&faces, false);
        let model: u32 = item[1..].parse().unwrap();
        let st = s.styled(BLUE, if on_shell { model - 1 } else { model });
        s.held(&[st]);
        expect(&s.write(name, &[model]), lin(BLUE), &[None; 3]);
    }
    let mut s = S::new();
    let (so, sh, _) = s.solid(0.);
    let st = s.styled(BLUE, sh);
    s.held(&[st]);
    expect(&s.write("closed-shell.step", &[so]), None, &[lin(BLUE); 3]);
}

/// Of two solids, the styled one colours its faces, not the part.
#[test]
fn a_styled_solid_among_two_colours_its_faces() {
    let mut s = S::new();
    let (a, _, _) = s.solid(0.);
    let (b, _, _) = s.solid(10.);
    let st = s.styled(BLUE, b);
    s.held(&[st]);
    let faces = [None, None, None, lin(BLUE), lin(BLUE), lin(BLUE)];
    expect(&s.write("two-solids.step", &[a, b]), None, &faces);
}

/// A `STYLED_ITEM` on an `ADVANCED_FACE` overrides its solid's colour.
#[test]
fn a_styled_face_overrides_its_solid() {
    let mut s = S::new();
    let (so, _, faces) = s.solid(0.);
    let a = s.styled(RED, so);
    let b = s.styled(BLUE, faces[1]);
    s.held(&[a, b]);
    expect(
        &s.write("face.step", &[so]),
        lin(RED),
        &[None, lin(BLUE), None],
    );
}

/// An `OVER_RIDING_STYLED_ITEM` on a face; of styles on one item, the last
/// in the file wins, overriding or not.
#[test]
fn an_overriding_style_colours_its_face_and_the_last_style_wins() {
    let mut s = S::new();
    let (so, _, faces) = s.solid(0.);
    let base = s.styled(GREEN, so);
    let c = s.rgb(BLUE);
    let u = s.usage(c, "BOTH");
    let p = s.0.add(&format!("PRESENTATION_STYLE_ASSIGNMENT((#{u}))"));
    let over = s.0.add(&format!(
        "OVER_RIDING_STYLED_ITEM('',(#{p}),#{},#{base})",
        faces[2]
    ));
    s.held(&[base, over]);
    let path = s.write("overriding.step", &[so]);
    expect(&path, lin(GREEN), &[None, None, lin(BLUE)]);

    let mut s = S::new();
    let (so, _, faces) = s.solid(0.);
    let base = s.styled(GREEN, so);
    let c = s.rgb(BLUE);
    let u = s.usage(c, "BOTH");
    let p = s.0.add(&format!("PRESENTATION_STYLE_ASSIGNMENT((#{u}))"));
    let over = s.0.add(&format!(
        "OVER_RIDING_STYLED_ITEM('',(#{p}),#{},#{base})",
        faces[0]
    ));
    let later = s.styled(RED, faces[0]);
    let twice = [s.styled(BLUE, so), s.styled(RED, so)];
    s.held(&[base, over, later, twice[0], twice[1]]);
    let path = s.write("last-wins.step", &[so]);
    expect(&path, lin(RED), &[lin(RED), None, None]);
}

/// Of a solid's and its shell's styles, the later in the file wins.
#[test]
fn the_later_of_a_solid_and_its_shell_wins() {
    for shell_last in [false, true] {
        let mut s = S::new();
        let (so, sh, _) = s.solid(0.);
        let (other, _, _) = s.solid(10.);
        let (first, second) = if shell_last { (so, sh) } else { (sh, so) };
        let a = s.styled(BLUE, first);
        let b = s.styled(RED, second);
        s.held(&[a, b]);
        let faces = [lin(RED), lin(RED), lin(RED), None, None, None];
        expect(&s.write("solid-shell.step", &[so, other]), None, &faces);
    }
}

/// A `DRAUGHTING_PRE_DEFINED_COLOUR`.
#[test]
fn a_predefined_colour_is_its_rgb() {
    let mut s = S::new();
    let (so, _, _) = s.solid(0.);
    let c = s.0.add("DRAUGHTING_PRE_DEFINED_COLOUR('magenta')");
    let u = s.usage(c, "BOTH");
    let st = s.styled_by(&[u], so);
    s.held(&[st]);
    let magenta = Some(Color {
        r: 1.,
        g: 0.,
        b: 1.,
    });
    expect(&s.write("predefined.step", &[so]), magenta, &[None; 3]);
}

/// A surface side and its colour.
type Usage = (&'static str, [f64; 3]);

/// `.POSITIVE.` or `.NEGATIVE.` alone colours; the last `.BOTH.` or
/// `.POSITIVE.` usage beats a `.NEGATIVE.` one, whatever the order.
#[test]
fn the_sides_resolve_as_xcaf_does() {
    let cases: [(&str, &[Usage], [f64; 3]); 5] = [
        ("positive.step", &[("POSITIVE", BLUE)], BLUE),
        ("negative.step", &[("NEGATIVE", BLUE)], BLUE),
        (
            "neg-pos.step",
            &[("NEGATIVE", RED), ("POSITIVE", BLUE)],
            BLUE,
        ),
        (
            "pos-neg.step",
            &[("POSITIVE", BLUE), ("NEGATIVE", RED)],
            BLUE,
        ),
        ("both-pos.step", &[("BOTH", RED), ("POSITIVE", BLUE)], BLUE),
    ];
    for (name, usages, want) in cases {
        let mut s = S::new();
        let (so, _, _) = s.solid(0.);
        let u: Vec<u32> = usages
            .iter()
            .map(|&(side, c)| {
                let c = s.rgb(c);
                s.usage(c, side)
            })
            .collect();
        let st = s.styled_by(&u, so);
        s.held(&[st]);
        expect(&s.write(name, &[so]), lin(want), &[None; 3]);
    }
}

/// Styles colour whether a `MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_
/// REPRESENTATION` holds them or not.
#[test]
fn a_style_needs_no_presentation_representation() {
    let mut s = S::new();
    let (so, _, _) = s.solid(0.);
    s.styled(BLUE, so);
    expect(&s.write("unheld.step", &[so]), lin(BLUE), &[None; 3]);
}

/// A style on the part's `SHAPE_REPRESENTATION` (#10): XCAF colours the
/// part only when some styled item styles a representation item, and a
/// style on the solid beats it.
#[test]
fn a_styled_shape_representation_colours_its_part() {
    let mut s = S::new();
    let (a, _, _) = s.solid(0.);
    let (b, _, _) = s.solid(10.);
    let st = s.styled(BLUE, 10);
    s.held(&[st]);
    expect(&s.write("rep-alone.step", &[a, b]), None, &[None; 6]);

    let mut s = S::new();
    let (a, _, _) = s.solid(0.);
    let (b, _, _) = s.solid(10.);
    let st = s.styled(BLUE, 10);
    let point = s.0.pt([0., 0., 0.]);
    let stray = s.styled(RED, point);
    s.held(&[st, stray]);
    expect(&s.write("rep.step", &[a, b]), lin(BLUE), &[None; 6]);

    let mut s = S::new();
    let (so, _, _) = s.solid(0.);
    let own = s.styled(RED, so);
    let rep = s.styled(BLUE, 10);
    s.held(&[own, rep]);
    expect(&s.write("rep-and-solid.step", &[so]), lin(RED), &[None; 3]);
}

/// The gate catches a colour XCAF gives and the graph doesn't: a
/// `SURFACE_STYLE_RENDERING`'s colour, outside the covered cases, is a
/// mismatch, never a silent default.
#[test]
fn a_colour_the_graph_misses_fails_the_gate() {
    let mut s = S::new();
    let (so, _, _) = s.solid(0.);
    let c = s.rgb(BLUE);
    let f = &mut s.0;
    let r = f.add(&format!("SURFACE_STYLE_RENDERING(.NORMAL_SHADING.,#{c})"));
    let side = f.add(&format!("SURFACE_SIDE_STYLE('',(#{r}))"));
    let u = f.add(&format!("SURFACE_STYLE_USAGE(.BOTH.,#{side})"));
    let st = s.styled_by(&[u], so);
    s.held(&[st]);
    let path = s.write("rendering.step", &[so]);
    assert_eq!(graph_colours(&path), (None, vec![None; 3]));
    let t = run(&[path]);
    assert_eq!(t.coloured, 1, "XCAF colours it");
    let part = t
        .mismatches
        .iter()
        .find(|m| m.contains("part 0 colour Some"));
    assert!(part.is_some(), "{:?}", t.mismatches);
}
