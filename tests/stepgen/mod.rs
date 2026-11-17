//! Hand-written STEP fixtures for the graph's tests (G6c `graph_fits`, G7
//! `graph_colors`): one part in mm, its records built entity by entity with
//! the project's own geometry.

#![allow(dead_code)]

use std::path::PathBuf;

/// A one-part file in mm with length uncertainty `unc`, whose shape
/// representation lists `items`, written by `records` (ids from #50 up).
pub fn part_file(name: &str, unc: f64, items: &str, records: &str) -> PathBuf {
    let path = std::env::temp_dir()
        .join(format!("stepv-stepgen-{}", std::process::id()))
        .join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let text = format!(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('{name}','2026-11-15T00:00:00',(''),(''),'','','');\n\
         FILE_SCHEMA(('AUTOMOTIVE_DESIGN {{ 1 0 10303 214 1 1 1 1 }}'));\nENDSEC;\nDATA;\n\
         #1=APPLICATION_CONTEXT('mechanical design');\n\
         #2=APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#1);\n\
         #3=PRODUCT_CONTEXT('',#1,'mechanical');\n#4=PRODUCT('p','p','',(#3));\n\
         #5=PRODUCT_DEFINITION_FORMATION('','',#4);\n#6=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');\n\
         #7=PRODUCT_DEFINITION('design','',#5,#6);\n#8=PRODUCT_DEFINITION_SHAPE('','',#7);\n\
         #9=SHAPE_DEFINITION_REPRESENTATION(#8,#10);\n#10=SHAPE_REPRESENTATION('',({items}),#20);\n\
         #11=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n\
         #12=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\n\
         #13=(NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT());\n\
         #14=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE({unc:?}),#11,'distance_accuracy_value','');\n\
         #20=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#14)) \
         GLOBAL_UNIT_ASSIGNED_CONTEXT((#11,#12,#13)) REPRESENTATION_CONTEXT('',''));\n\
         {records}ENDSEC;\nEND-ISO-10303-21;\n"
    );
    std::fs::write(&path, text).unwrap();
    path
}

/// Records from #50 up.
pub struct F {
    pub out: String,
    pub next: u32,
}

impl F {
    pub fn new() -> Self {
        F {
            out: String::new(),
            next: 50,
        }
    }

    pub fn add(&mut self, record: &str) -> u32 {
        let id = self.next;
        self.next += 1;
        self.out += &format!("#{id}={record};\n");
        id
    }

    pub fn pt(&mut self, p: [f64; 3]) -> u32 {
        self.add(&format!(
            "CARTESIAN_POINT('',({:?},{:?},{:?}))",
            p[0], p[1], p[2]
        ))
    }

    /// An axis placement at `at`, z up, x along x.
    pub fn ax(&mut self, at: [f64; 3]) -> u32 {
        self.ax_to(at, [0., 0., 1.], [1., 0., 0.])
    }

    /// An axis placement at `at` with axis `z` and reference direction `x`.
    pub fn ax_to(&mut self, at: [f64; 3], z: [f64; 3], x: [f64; 3]) -> u32 {
        let p = self.pt(at);
        let z = self.add(&format!("DIRECTION('',({:?},{:?},{:?}))", z[0], z[1], z[2]));
        let x = self.add(&format!("DIRECTION('',({:?},{:?},{:?}))", x[0], x[1], x[2]));
        self.add(&format!("AXIS2_PLACEMENT_3D('',#{p},#{z},#{x})"))
    }

    /// A full circle of radius `r` about z through `at`, as a one-edge loop
    /// bound, oriented `sense`.
    pub fn circle_bound(&mut self, at: [f64; 3], r: f64, sense: bool) -> u32 {
        let a = self.ax(at);
        let c = self.add(&format!("CIRCLE('',#{a},{r:?})"));
        let p = self.pt([at[0] + r, at[1], at[2]]);
        let v = self.add(&format!("VERTEX_POINT('',#{p})"));
        let e = self.add(&format!("EDGE_CURVE('',#{v},#{v},#{c},.T.)"));
        let s = if sense { ".T." } else { ".F." };
        let oe = self.add(&format!("ORIENTED_EDGE('',*,*,#{e},{s})"));
        let lp = self.add(&format!("EDGE_LOOP('',(#{oe}))"));
        self.add(&format!("FACE_BOUND('',#{lp},.T.)"))
    }

    pub fn face(&mut self, bounds: &[u32], surface: u32) -> u32 {
        let b: Vec<String> = bounds.iter().map(|b| format!("#{b}")).collect();
        self.add(&format!(
            "ADVANCED_FACE('',({}),#{surface},.T.)",
            b.join(",")
        ))
    }

    /// A cylinder face of radius `r` on the z axis through `at`, from
    /// z = at[2] to `top`, and its two caps when `caps`.
    pub fn shaft(&mut self, at: [f64; 3], r: f64, top: f64, caps: bool) -> Vec<u32> {
        let a = self.ax(at);
        let cyl = self.add(&format!("CYLINDRICAL_SURFACE('',#{a},{r:?})"));
        let lo = self.circle_bound(at, r, true);
        let hi = self.circle_bound([at[0], at[1], top], r, false);
        let mut faces = vec![self.face(&[lo, hi], cyl)];
        if caps {
            for (z, b) in [(at[2], lo), (top, hi)] {
                let a = self.ax([at[0], at[1], z]);
                let plane = self.add(&format!("PLANE('',#{a})"));
                faces.push(self.face(&[b], plane));
            }
        }
        faces
    }

    /// `faces` as a shell's item: a solid when `closed`, else a surface
    /// model.
    pub fn item(&mut self, faces: &[u32], closed: bool) -> String {
        let f: Vec<String> = faces.iter().map(|f| format!("#{f}")).collect();
        let f = f.join(",");
        let item = if closed {
            let sh = self.add(&format!("CLOSED_SHELL('',({f}))"));
            self.add(&format!("MANIFOLD_SOLID_BREP('',#{sh})"))
        } else {
            let sh = self.add(&format!("OPEN_SHELL('',({f}))"));
            self.add(&format!("SHELL_BASED_SURFACE_MODEL('',(#{sh}))"))
        };
        format!("#{item}")
    }
}
