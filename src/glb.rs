//! Binary glTF 2.0 (`.glb`) from a [`Scene`]: the `--glb` path.
//!
//! Hand-written rather than via a dependency: the writer side is small, and the
//! layout decisions are the interesting part.
//!
//! - **One node per part**, named from XCAF, under a root node that turns the
//!   CAD convention (Z-up, millimetres) into glTF's (Y-up, metres).
//! - **Primitives grouped by (colour, status)**, all sharing the part's vertex
//!   buffers, so per-face colour survives without duplicating vertices.
//! - **The broken-face overlay is in the file**: approximated faces get their
//!   own amber material named `stepv:approximated`, and every primitive's
//!   `extras.stepv_face_status` says which ladder rung produced it, so a
//!   viewer can find and flag them. Missing-face outlines and sketch curves
//!   are `LINES` primitives; construction curves are left out by default.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::{Color, FaceStatus, LineKind, Scene};

/// Writer options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Options {
    /// Include `LineKind::Construction` curves.
    pub show_construction: bool,
}

const DEFAULT_COLOR: Color = Color {
    r: 0.38,
    g: 0.43,
    b: 0.50,
};

/// A material key: colour (quantised so equal floats dedupe) plus a class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    Faithful,
    Approx,
    Line(u8),
}

fn q(c: Color) -> [u16; 3] {
    [c.r, c.g, c.b].map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16)
}

struct Writer {
    bin: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Writer {
    /// Appends `bytes` as a buffer view (4-byte aligned) and returns its index.
    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let offset = self.bin.len();
        self.bin.extend_from_slice(bytes);
        let mut v = json!({ "buffer": 0, "byteOffset": offset, "byteLength": bytes.len() });
        if let Some(t) = target {
            v["target"] = json!(t);
        }
        self.views.push(v);
        self.views.len() - 1
    }

    fn vec3(&mut self, data: &[f32], with_bounds: bool) -> usize {
        let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
        let view = self.view(&bytes, Some(34962));
        let mut a = json!({
            "bufferView": view, "componentType": 5126, "count": data.len() / 3, "type": "VEC3"
        });
        if with_bounds {
            // POSITION accessors must carry min/max (glTF 2.0 §3.6.2.5).
            let mut lo = [f32::INFINITY; 3];
            let mut hi = [f32::NEG_INFINITY; 3];
            for p in data.chunks_exact(3) {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
            a["min"] = json!(lo);
            a["max"] = json!(hi);
        }
        self.accessors.push(a);
        self.accessors.len() - 1
    }

    fn indices(&mut self, data: &[u32]) -> usize {
        let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
        let view = self.view(&bytes, Some(34963));
        self.accessors.push(json!({
            "bufferView": view, "componentType": 5125, "count": data.len(), "type": "SCALAR"
        }));
        self.accessors.len() - 1
    }
}

fn status_name(s: FaceStatus) -> &'static str {
    match s {
        FaceStatus::Ok => "ok",
        FaceStatus::Remeshed => "remeshed",
        FaceStatus::Healed => "healed",
        FaceStatus::Refined => "refined",
        FaceStatus::Coarse => "coarse",
        FaceStatus::Degenerate => "degenerate",
        FaceStatus::Approx => "approximated",
        FaceStatus::Missing => "missing",
    }
}

/// Serialises `scene` as a `.glb`.
#[must_use]
pub fn to_glb(scene: &Scene, opts: &Options) -> Vec<u8> {
    let mut w = Writer {
        bin: Vec::new(),
        views: Vec::new(),
        accessors: Vec::new(),
    };
    let mut materials: BTreeMap<(Class, [u16; 3]), usize> = BTreeMap::new();
    let mut material_list: Vec<Value> = Vec::new();
    let mut material = |class: Class, c: Color| -> usize {
        *materials.entry((class, q(c))).or_insert_with(|| {
            let (name, rough) = match class {
                Class::Faithful => ("stepv:surface", 0.6),
                Class::Approx => ("stepv:approximated", 0.9),
                Class::Line(_) => ("stepv:line", 1.0),
            };
            material_list.push(json!({
                "name": name,
                "pbrMetallicRoughness": {
                    "baseColorFactor": [c.r, c.g, c.b, 1.0],
                    "metallicFactor": 0.0,
                    "roughnessFactor": rough,
                },
                // CAD faces are not reliably oriented.
                "doubleSided": true,
            }));
            material_list.len() - 1
        })
    };

    let mut meshes = Vec::new();
    let mut nodes = vec![json!({})]; // root, filled below
    let mut children = Vec::new();
    for (pi, part) in scene.parts.iter().enumerate() {
        let mut primitives = Vec::new();
        let m = &part.mesh;
        if m.triangle_count() > 0 {
            let pos = w.vec3(&m.positions, true);
            let nrm = w.vec3(&m.normals, false);
            // Group triangles by (material, status).
            let mut groups: BTreeMap<(usize, FaceStatus), Vec<u32>> = BTreeMap::new();
            for (tri, &fid) in m.indices.chunks_exact(3).zip(&m.face_ids) {
                let status = part
                    .faces
                    .get(fid as usize)
                    .map_or(FaceStatus::Ok, |f| f.status);
                let mat = if status == FaceStatus::Approx {
                    material(
                        Class::Approx,
                        Color {
                            r: 0.96,
                            g: 0.62,
                            b: 0.12,
                        },
                    )
                } else {
                    material(
                        Class::Faithful,
                        part.face_color(fid).unwrap_or(DEFAULT_COLOR),
                    )
                };
                groups
                    .entry((mat, status))
                    .or_default()
                    .extend_from_slice(tri);
            }
            for ((mat, status), idx) in groups {
                let ind = w.indices(&idx);
                primitives.push(json!({
                    "attributes": { "POSITION": pos, "NORMAL": nrm },
                    "indices": ind,
                    "material": mat,
                    "mode": 4,
                    "extras": { "stepv_face_status": status_name(status) },
                }));
            }
        }
        // Lines, one primitive per kind.
        for kind in [
            LineKind::Sketch,
            LineKind::MissingOutline,
            LineKind::Construction,
        ] {
            if kind == LineKind::Construction && !opts.show_construction {
                continue;
            }
            let pts: Vec<f32> = part
                .lines
                .kinds
                .iter()
                .enumerate()
                .filter(|(_, k)| **k == kind)
                .flat_map(|(s, _)| part.lines.positions[s * 6..s * 6 + 6].iter().copied())
                .collect();
            if pts.is_empty() {
                continue;
            }
            let pos = w.vec3(&pts, true);
            let (c, tag) = match kind {
                LineKind::Sketch => ([0.12, 0.13, 0.15], "sketch"),
                LineKind::MissingOutline => ([0.85, 0.05, 0.10], "missing-face-outline"),
                LineKind::Construction => ([0.35, 0.45, 0.60], "construction"),
            };
            let mat = material(
                Class::Line(kind as u8),
                Color {
                    r: c[0],
                    g: c[1],
                    b: c[2],
                },
            );
            primitives.push(json!({
                "attributes": { "POSITION": pos },
                "material": mat,
                "mode": 1,
                "extras": { "stepv_line_kind": tag },
            }));
        }
        if primitives.is_empty() {
            continue;
        }
        meshes.push(json!({ "primitives": primitives }));
        let mut node = json!({ "mesh": meshes.len() - 1 });
        node["name"] = json!(
            part.name
                .clone()
                .unwrap_or_else(|| format!("part {}", pi + 1))
        );
        nodes.push(node);
        children.push(nodes.len() - 1);
    }
    // Z-up mm → Y-up m: rotate −90° about X, scale 1/1000.
    let h = std::f64::consts::FRAC_1_SQRT_2;
    nodes[0] = json!({
        "name": "stepv",
        "rotation": [-h, 0.0, 0.0, h],
        "scale": [0.001, 0.001, 0.001],
        "children": children,
    });

    while !w.bin.len().is_multiple_of(4) {
        w.bin.push(0);
    }
    let doc = json!({
        "asset": { "version": "2.0", "generator": concat!("stepv ", env!("CARGO_PKG_VERSION")) },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": nodes,
        "meshes": meshes,
        "materials": material_list,
        "accessors": w.accessors,
        "bufferViews": w.views,
        "buffers": [{ "byteLength": w.bin.len() }],
    });
    let mut json_bytes = serde_json::to_vec(&doc).expect("glTF JSON serialises");
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }

    let total = 12 + 8 + json_bytes.len() + 8 + w.bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&(w.bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&w.bin);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BBox, Face, Lines, Mesh, Part};

    fn scene() -> Scene {
        // Two triangles on two faces: face 0 red, face 1 approximated.
        let mesh = Mesh {
            positions: vec![0., 0., 0., 1., 0., 0., 0., 1., 0., 1., 1., 0.],
            normals: vec![0., 0., 1., 0., 0., 1., 0., 0., 1., 0., 0., 1.],
            indices: vec![0, 1, 2, 1, 3, 2],
            face_ids: vec![0, 1],
        };
        Scene {
            bbox: BBox {
                min: [0.0; 3],
                max: [1.0, 1.0, 0.0],
            },
            parts: vec![Part {
                edges: Default::default(),
                name: Some("bracket".into()),
                color: Some(Color {
                    r: 0.5,
                    g: 0.5,
                    b: 0.5,
                }),
                mesh,
                faces: vec![
                    Face {
                        status: FaceStatus::Ok,
                        color: Some(Color {
                            r: 1.0,
                            g: 0.0,
                            b: 0.0,
                        }),
                    },
                    Face::plain(FaceStatus::Approx),
                ],
                lines: Lines {
                    positions: vec![0., 0., 0., 1., 1., 1., 0., 0., 0., 0., 0., 1.],
                    kinds: vec![LineKind::MissingOutline, LineKind::Construction],
                },
            }],
        }
    }

    #[test]
    fn output_is_valid_gltf_and_loads() {
        let glb = to_glb(&scene(), &Options::default());
        let (doc, buffers, _) = gltf::import_slice(&glb).expect("valid glTF 2.0");
        assert_eq!(buffers.len(), 1);
        let part = doc
            .nodes()
            .find(|n| n.name() == Some("bracket"))
            .expect("part node");
        let prims: Vec<_> = part.mesh().unwrap().primitives().collect();
        // red faithful face, amber approximated face, missing outline; no construction.
        assert_eq!(prims.len(), 3);
        let names: Vec<_> = prims
            .iter()
            .map(|p| p.material().name().unwrap_or(""))
            .collect();
        assert!(names.contains(&"stepv:approximated"));
        assert!(prims.iter().any(|p| p.mode() == gltf::mesh::Mode::Lines));
        let red = prims.iter().find(|p| {
            p.material().pbr_metallic_roughness().base_color_factor()[..3] == [1.0, 0.0, 0.0]
        });
        assert!(red.is_some(), "per-face colour survives");
    }

    #[test]
    fn construction_lines_are_opt_in() {
        let glb = to_glb(
            &scene(),
            &Options {
                show_construction: true,
            },
        );
        let (doc, _, _) = gltf::import_slice(&glb).unwrap();
        let n = doc.meshes().next().unwrap().primitives().count();
        assert_eq!(n, 4);
    }

    #[test]
    fn root_converts_z_up_millimetres() {
        let glb = to_glb(&scene(), &Options::default());
        let (doc, _, _) = gltf::import_slice(&glb).unwrap();
        let root = doc.nodes().find(|n| n.name() == Some("stepv")).unwrap();
        let (_, rot, scale) = root.transform().decomposed();
        assert!((scale[0] - 0.001).abs() < 1e-9);
        assert!((rot[0] + std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    }

    #[test]
    fn empty_scene_is_still_a_valid_file() {
        let s = Scene {
            bbox: BBox {
                min: [0.0; 3],
                max: [0.0; 3],
            },
            parts: vec![],
        };
        assert!(gltf::import_slice(to_glb(&s, &Options::default())).is_ok());
    }
}
