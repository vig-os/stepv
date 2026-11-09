//! Kernel-free checks of the structure's rules; `tests/graph_structure.rs`
//! holds it to the kernel.

use super::*;
use crate::graph::StepId;

fn graph(data: &str) -> Graph {
    Graph::from_bytes(
        format!(
            "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('t','',(''),(''),'','','');\n\
             FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA;\n{data}\nENDSEC;\nEND-ISO-10303-21;\n"
        )
        .into_bytes(),
    )
    .unwrap()
}

/// A product at ids `b..`: its definition is `#b+2`; with a shape, a shape
/// representation at `#b+4` without items or context.
fn product(b: u32, name: &str, id: &str, shape: bool) -> String {
    let mut s = format!(
        "#{b}=PRODUCT('{id}','{name}','',());\n#{}=PRODUCT_DEFINITION_FORMATION('','',#{b});\n#{}=PRODUCT_DEFINITION('','',#{},$);\n",
        b + 1,
        b + 2,
        b + 1
    );
    if shape {
        s += &format!(
            "#{}=PRODUCT_DEFINITION_SHAPE('','',#{});\n#{}=SHAPE_REPRESENTATION('',(),$);\n#{}=SHAPE_DEFINITION_REPRESENTATION(#{},#{});\n",
            b + 3,
            b + 2,
            b + 4,
            b + 5,
            b + 3,
            b + 4
        );
    }
    s
}

fn nauo(n: u32, id: &str, name: &str, desc: &str, parent: u32, child: u32) -> String {
    format!(
        "#{n}=NEXT_ASSEMBLY_USAGE_OCCURRENCE('{id}','{name}',{desc},#{},#{},$);\n",
        parent + 2,
        child + 2
    )
}

fn names(t: &[topology::Node], out: &mut Vec<String>) {
    for n in t {
        match n {
            topology::Node::Part { name, .. } => out.push(name.clone()),
            topology::Node::Assembly { name, children } => {
                out.push(format!("{name}/"));
                names(children, out);
            }
        }
    }
}

#[test]
fn both_naming_rules() {
    // A product: its name, else its id, else its instance's name; an
    // instance: its description, else its name, else its id.
    let data = [
        product(10, "assy", "A", true),
        product(20, "bolt", "B", true),
        product(30, " ", "id-only", true),
        product(40, "", "", true),
        product(50, "", "", true),
        product(60, "", "", true),
        nauo(100, "n1", "n1-name", "'n1-desc'", 10, 20),
        nauo(101, "n2", "n2-name", "$", 10, 30),
        nauo(102, "n3", "n3-name", "'n3-desc'", 10, 40),
        nauo(103, "n4", "n4-name", "' '", 10, 50),
        nauo(104, "n5", "", "$", 10, 60),
    ]
    .concat();
    let g = graph(&data);
    let s = structure(&g).unwrap();
    let mut got = Vec::new();
    names(&s.tree(&g, Expand::default()).unwrap(), &mut got);
    assert_eq!(
        got,
        ["assy/", "bolt", "id-only", "n3-desc", "n4-name", "n5"]
    );
}

#[test]
fn shapeless_products_are_dropped_and_documents_are_no_roots() {
    let data = [
        product(10, "assy", "A", false),
        product(20, "part", "P", true),
        product(30, "material", "M", false),
        product(40, "empty-assy", "E", false),
        nauo(100, "n1", "", "$", 10, 20),
        nauo(101, "n2", "", "$", 10, 30),
        nauo(102, "n3", "", "$", 10, 40),
        nauo(103, "n4", "", "$", 40, 30),
        // A document: its formation is a document's equivalent.
        "#200=PRODUCT('doc','doc','',());\n#201=PRODUCT_DEFINITION_FORMATION('','',#200);\n\
         #202=PRODUCT_DEFINITION_WITH_ASSOCIATED_DOCUMENTS('','',#201,$,());\n\
         #203=DOCUMENT_PRODUCT_EQUIVALENCE('','',#204,#201);\n#204=DOCUMENT('d','d','',#205);\n#205=DOCUMENT_TYPE('');\n"
            .to_owned(),
    ]
    .concat();
    let g = graph(&data);
    let s = structure(&g).unwrap();
    let defs: Vec<u32> = s.roots.iter().map(|&r| g.id(s.products[r].def).0).collect();
    assert_eq!(defs, [12]);
    let mut got = Vec::new();
    names(&s.tree(&g, Expand::default()).unwrap(), &mut got);
    assert_eq!(got, ["assy/", "part"]);
    assert_eq!(s.leaves, 1);
    let shown: Vec<bool> = s.products.iter().map(|p| p.shown).collect();
    assert_eq!(shown, [true, true, false, false, false]);
}

#[test]
fn leaf_paths_count_shown_leaves_without_expanding() {
    // top → [sub, gap (shapeless), sub, bolt]; sub → [bolt, bolt].
    let data = [
        product(10, "top", "T", true),
        product(20, "sub", "S", true),
        product(30, "bolt", "B", true),
        product(40, "gap", "G", false),
        nauo(100, "a", "", "$", 10, 20),
        nauo(101, "b", "", "$", 10, 40),
        nauo(102, "c", "", "$", 10, 20),
        nauo(103, "d", "", "$", 10, 30),
        nauo(104, "e", "", "$", 20, 30),
        nauo(105, "f", "", "$", 20, 30),
    ]
    .concat();
    let g = graph(&data);
    let s = structure(&g).unwrap();
    assert_eq!(s.leaves, 5);
    let (top, sub, bolt) = (0, 1, 2);
    assert_eq!(
        s.leaf_path(3).unwrap(),
        Path(vec![(top, 0), (sub, 2), (bolt, 1)])
    );
    assert_eq!(s.leaf_path(4).unwrap(), Path(vec![(top, 0), (bolt, 3)]));
    assert!(s.leaf_path(5).is_none());
    let leaves: Vec<_> = s
        .occurrences(Expand::default())
        .map(Result::unwrap)
        .filter_map(|o| Some((o.leaf?, o.path)))
        .collect();
    assert_eq!(leaves.len(), 5);
    for (leaf, path) in leaves {
        assert_eq!(s.leaf_path(leaf).as_ref(), Some(&path));
        assert_eq!(s.leaf_of(&path), Some(leaf));
    }
    // Not leaves: an assembly, a hidden child, a wrong slot, a wrong root.
    assert_eq!(s.leaf_of(&Path(vec![(top, 0), (sub, 0)])), None);
    assert_eq!(s.leaf_of(&Path(vec![(top, 0), (3, 1)])), None);
    assert_eq!(s.leaf_of(&Path(vec![(top, 0), (bolt, 2)])), None);
    assert_eq!(s.leaf_of(&Path(vec![(top, 1)])), None);
    assert_eq!(s.leaf_of(&Path(vec![])), None);
}

#[test]
fn caps_read_as_the_outline() {
    let data = [
        product(10, "top", "T", true),
        product(20, "p", "P", true),
        nauo(100, "a", "", "$", 10, 20),
        nauo(101, "b", "", "$", 10, 20),
    ]
    .concat();
    let g = graph(&data);
    let s = structure(&g).unwrap();
    let cap = Expand {
        max_depth: 60,
        max_nodes: 2,
    };
    let all: Vec<_> = s.occurrences(cap).collect();
    assert_eq!(
        all.len(),
        3,
        "two occurrences, then the error, then nothing"
    );
    assert_eq!(all[2], Err(ExpandError::Nodes(2)));
    assert_eq!(
        s.tree(&g, cap).unwrap_err().to_string(),
        "outline larger than 2 nodes"
    );
    assert_eq!(
        ExpandError::Nodes(1_000_000).to_string(),
        "outline larger than a million nodes"
    );
    let shallow = Expand {
        max_depth: 0,
        max_nodes: 10,
    };
    assert_eq!(
        s.tree(&g, shallow).unwrap_err().to_string(),
        "assembly nesting deeper than 0"
    );
}

#[test]
fn a_cycle_names_a_product_on_it() {
    let data = [
        product(10, "a", "A", true),
        product(20, "b", "B", true),
        nauo(100, "x", "", "$", 10, 20),
        nauo(101, "y", "", "$", 20, 10),
    ]
    .concat();
    let g = graph(&data);
    let e = structure(&g).unwrap_err();
    assert!(
        matches!(e, StructureError::Cycle(n) if [12, 22].contains(&g.id(n).0)),
        "{e:?}"
    );
}

const UNITS: &str = "
#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));
#2=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#1);
#3=(CONVERSION_BASED_UNIT('INCH',#2) LENGTH_UNIT() NAMED_UNIT(#4));
#4=DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.);
#5=SI_UNIT(*,$,.METRE.);
#6=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.0254),#5);
#7=(CONVERSION_BASED_UNIT('INCH',#6) LENGTH_UNIT() NAMED_UNIT(#4));
#8=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT($,.METRE.));
#9=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));
#11=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#9)) REPRESENTATION_CONTEXT('',''));
#12=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#3,#9)) REPRESENTATION_CONTEXT('',''));
#13=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#7)) REPRESENTATION_CONTEXT('',''));
#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#8)) REPRESENTATION_CONTEXT('',''));
#15=GEOMETRIC_REPRESENTATION_CONTEXT('','',3);
#21=SHAPE_REPRESENTATION('',(),#11);
#22=SHAPE_REPRESENTATION('',(),#12);
#23=SHAPE_REPRESENTATION('',(),#13);
#24=SHAPE_REPRESENTATION('',(),#14);
#25=SHAPE_REPRESENTATION('',(),#15);
";

#[test]
fn units_scale_to_mm_as_occt_reads_them() {
    let g = graph(UNITS);
    let mm = |id| mm_per_unit(&g, g.node(StepId(id)).unwrap());
    // mm; an inch over mm; an inch over a simple metre; a metre; no units.
    let got = [mm(21), mm(22), mm(23), mm(24), mm(25)];
    assert!(
        got.iter()
            .zip([1., 25.4, 25.4, 1000., 1.])
            .all(|(a, b)| (a - b).abs() < 1e-12),
        "{got:?}"
    );
}

#[test]
fn frames_pick_occts_x_direction() {
    // No reference direction: gp_Ax2's pick; one parallel to the axis is
    // ignored the same way; an oblique one is made orthogonal.
    assert_eq!(default_x([0., 0., 1.]), [1., 0., 0.]);
    assert_eq!(default_x([1., 0., 0.]), [0., 0., 1.]);
    assert_eq!(default_x([0., 1., 0.]), [0., 0., 1.]);
    let g = graph(
        "#1=CARTESIAN_POINT('',(1.,2.,3.));#2=DIRECTION('',(1.,0.,0.));#3=DIRECTION('',(2.,0.,0.));#4=DIRECTION('',(1.,1.,0.));
         #5=AXIS2_PLACEMENT_3D('',#1,#2,#3);#6=AXIS2_PLACEMENT_3D('',#1,$,#4);#7=AXIS2_PLACEMENT_3D('',#1,#2,$);",
    );
    let f = |id| frame(&g, g.node(StepId(id)).unwrap(), 10.).unwrap();
    assert_eq!(f(5), f(7));
    assert_eq!(f(7), [0., 0., 1., 10., 0., -1., 0., 20., 1., 0., 0., 30.]);
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let x = f(6);
    assert!(
        [x[0], x[4], x[8]]
            .iter()
            .zip([h, h, 0.])
            .all(|(a, b)| (a - b).abs() < 1e-15),
        "{x:?}"
    );
}

#[test]
fn instances_without_a_trusted_placement_say_why() {
    let data = [
        product(10, "top", "T", true),
        product(20, "a", "A", true),
        product(30, "b", "B", true),
        product(40, "c", "C", true),
        // a: no placement relationship at all.
        nauo(100, "a", "", "$", 10, 20),
        // b: a cartesian transformation operator.
        nauo(110, "b", "", "$", 10, 30),
        "#111=PRODUCT_DEFINITION_SHAPE('','',#110);#112=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#113,#111);
         #113=(REPRESENTATION_RELATIONSHIP('','',#34,#14) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#114) SHAPE_REPRESENTATION_RELATIONSHIP());
         #114=CARTESIAN_TRANSFORMATION_OPERATOR_3D('','',$,#115,$,$,$,$);#115=CARTESIAN_POINT('',(0.,0.,0.));\n"
            .to_owned(),
        // c: a relationship without a transformation sits at the identity.
        nauo(120, "c", "", "$", 10, 40),
        "#121=PRODUCT_DEFINITION_SHAPE('','',#120);#122=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#123,#121);
         #123=SHAPE_REPRESENTATION_RELATIONSHIP('','',#44,#14);\n"
            .to_owned(),
    ]
    .concat();
    let g = graph(&data);
    let s = structure(&g).unwrap();
    let p: Vec<Placement> = s.products[0].children.iter().map(|c| c.placement).collect();
    assert_eq!(
        p,
        [
            Placement::Unplaced(Reason("no context-dependent shape representation")),
            Placement::Unplaced(Reason("cartesian transformation operator")),
            Placement::Placed(IDENTITY),
        ]
    );
    // Unplaced along a path stays unplaced in the model.
    let o: Vec<Placement> = s
        .occurrences(Expand::default())
        .map(|o| o.unwrap().placement)
        .collect();
    assert_eq!(o[1], p[0]);
}

#[test]
fn text_refs_resolve_without_a_borrow() {
    let g = graph("#1=PRODUCT('it''s','\\X2\\00E4\\X0\\','',());");
    let s = structure(&g).unwrap();
    assert!(s.products.is_empty());
    let n = g.node(StepId(1)).unwrap();
    let a = g.args(n).unwrap();
    let Value::Str(id) = a[0] else { panic!() };
    let t = g.text_ref(n, id).unwrap();
    assert_eq!(g.text(t).decode(), "it's");
    // Within the arguments, their opening parenthesis included.
    assert_eq!(t.span, (2, 7));
    let Value::Str(name) = a[1] else { panic!() };
    assert_eq!(g.text(g.text_ref(n, name).unwrap()).decode(), "ä");
    // Not a span of that node: no ref; a bogus ref reads as empty.
    let other = graph("#1=PRODUCT('x','y','',());");
    let o = other.node(StepId(1)).unwrap();
    assert!(
        g.text_ref(
            n,
            other
                .args(o)
                .unwrap()
                .first()
                .map(|v| match v {
                    Value::Str(s) => *s,
                    _ => panic!(),
                })
                .unwrap()
        )
        .is_none()
    );
    assert_eq!(
        g.text(TextRef {
            node: n,
            span: (5, 999)
        })
        .decode(),
        ""
    );
}
