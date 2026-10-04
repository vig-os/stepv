//! `stepv` — STEP/IGES/BREP preview and thumbnail generation.
//!
//! # What is here and what is not
//!
//! This crate is deliberately **kernel-agnostic at its seams**. The types below
//! are the contract between "something tessellated a CAD file" and "something
//! drew it", and they exist first precisely so the kernel decision stays
//! reversible: [`Scene`] is what the Plan A kernel (`occt-wasm`) must produce,
//! and it is also exactly what the Plan B fallback (native OCCT via C++) would
//! produce. See `plan.md` for both plans and the evidence behind them.
//!
//! The kernel itself is [`occt`]: native OCCT in a subprocess (Plan B — the
//! `occt-wasm` spike failed its gate, `plan.md` §5 "S1 result").

pub mod cache;
pub mod glb;
pub mod header;
pub mod occt;
pub mod render;

/// A triangle mesh for one part, in the file's own units.
///
/// Flat buffers rather than a vertex struct: both renderers downstream
/// (`SCNGeometrySource` on macOS, a software rasteriser on Linux) take
/// interleaved or planar slices directly, and a struct-of-vertices would be
/// re-flattened at both call sites.
///
/// `positions` and `normals` are `3 * vertex_count` long; `indices` is
/// `3 * triangle_count` long. `face_ids` is per-TRIANGLE
/// (`triangle_count` long), mirroring cxad's per-triangle face-id buffer so a
/// future shared viewer can key selection the same way.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
    pub face_ids: Vec<u32>,
}

impl Mesh {
    /// Triangle count implied by `indices`.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Vertex count implied by `positions`.
    #[must_use]
    pub fn vertex_count(&self) -> usize {
        self.positions.len() / 3
    }

    /// Whether the buffers are internally consistent.
    ///
    /// Worth having as a real check rather than a debug assert: a tessellator
    /// that silently emits a short normals buffer renders as black geometry,
    /// which reads as "the model is wrong" rather than "we have a bug".
    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        self.positions.len().is_multiple_of(3)
            && self.normals.len() == self.positions.len()
            && self.indices.len().is_multiple_of(3)
            && self.face_ids.len() == self.triangle_count()
            && self
                .indices
                .iter()
                .all(|&i| (i as usize) < self.vertex_count())
    }
}

/// Linear RGB in `0.0..=1.0`. No alpha: STEP carries none, and inventing one
/// would mean deciding transparency policy in the wrong layer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

/// How a face's triangles were obtained.
///
/// Exists so that a renderer can draw anything short of [`FaceStatus::Ok`]
/// with a warning treatment instead of passing it off as the model: a
/// previewer that silently shows wrong geometry is the failure `plan.md` §2
/// holds against Foxtrot. The order is severity, so a part's worst face is
/// its maximum. `Remeshed`, `Healed` and `Refined` are exact surfaces;
/// `Coarse` is exact but coarsely sampled; `Degenerate` is a zero-area
/// sliver with nothing to draw; `Approx` is a sample of the surface with a
/// jagged boundary; `Missing` has no triangles, only its outline in
/// [`Part::lines`]. The kernel's ladder is documented in
/// `kernel/stepv-occt.cpp`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum FaceStatus {
    Ok = 0,
    Remeshed = 1,
    Healed = 2,
    Refined = 3,
    Coarse = 4,
    Degenerate = 5,
    Approx = 6,
    Missing = 7,
}

impl FaceStatus {
    /// Decodes the kernel's byte; `None` for a value outside the contract.
    #[must_use]
    pub const fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => Self::Ok,
            1 => Self::Remeshed,
            2 => Self::Healed,
            3 => Self::Refined,
            4 => Self::Coarse,
            5 => Self::Degenerate,
            6 => Self::Approx,
            7 => Self::Missing,
            _ => return None,
        })
    }

    /// Whether the face is shown truthfully — exact triangles, or a sliver
    /// with nothing to show — so a renderer may draw it without a warning.
    #[must_use]
    pub const fn is_faithful(self) -> bool {
        (self as u8) <= (Self::Degenerate as u8)
    }
}

/// What a line segment represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LineKind {
    /// A curve of a faceless part in a file with no faces at all (a sketch or
    /// wireframe export): the file's whole content, so draw it.
    Sketch = 0,
    /// The boundary of a [`FaceStatus::Missing`] face, so the hole is drawn
    /// rather than hidden.
    MissingOutline = 1,
    /// A curve of a faceless part in a file that also has solids:
    /// construction geometry, axes, PMI leaders. Hide by default.
    Construction = 2,
}

/// Line segments, one `[x0 y0 z0 x1 y1 z1]` sextet per segment in
/// `positions`, with a [`LineKind`] per segment in `kinds`. Unindexed,
/// because both renderers take segment lists directly.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lines {
    pub positions: Vec<f32>,
    pub kinds: Vec<LineKind>,
}

impl Lines {
    #[must_use]
    pub fn segment_count(&self) -> usize {
        self.kinds.len()
    }

    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        self.positions.len() == 6 * self.kinds.len()
    }
}

/// One B-rep face: how its triangles were obtained, and its own colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Face {
    pub status: FaceStatus,
    /// The face's XCAF colour. Overrides [`Part::color`] when present: 81%
    /// of parts in the S1 corpus carry colour only per face.
    pub color: Option<Color>,
}

impl Face {
    /// A face with no colour of its own.
    #[must_use]
    pub const fn plain(status: FaceStatus) -> Self {
        Self {
            status,
            color: None,
        }
    }
}

/// One named, coloured part of an assembly.
#[derive(Debug, Clone)]
pub struct Part {
    /// Name from the XCAF label, when the exporter wrote one.
    pub name: Option<String>,
    /// Per-part colour from the XCAF colour tool, when present.
    pub color: Option<Color>,
    pub mesh: Mesh,
    /// One entry per B-rep face; `mesh.face_ids` indexes into it.
    pub faces: Vec<Face>,
    pub lines: Lines,
}

impl Part {
    /// [`Mesh::is_well_formed`], plus every `face_id` naming a face in
    /// `faces`, and well-formed lines.
    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        self.mesh.is_well_formed()
            && self.lines.is_well_formed()
            && self
                .mesh
                .face_ids
                .iter()
                .all(|&f| (f as usize) < self.faces.len())
    }

    /// The worst face status in the part, `None` for a part with no faces.
    #[must_use]
    pub fn worst_face(&self) -> Option<FaceStatus> {
        self.faces.iter().map(|f| f.status).max()
    }

    /// The colour to draw face `face_id` in: its own, else the part's.
    #[must_use]
    pub fn face_color(&self, face_id: u32) -> Option<Color> {
        self.faces
            .get(face_id as usize)
            .and_then(|f| f.color)
            .or(self.color)
    }
}

/// An axis-aligned bounding box in the file's own units.
///
/// Carried separately from the parts because it is available FIRST and far
/// cheaper: the previewer shows a box and the header metadata immediately, then
/// swaps in geometry when tessellation finishes (`plan.md` §4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

impl BBox {
    /// Diagonal length — the scale every deflection setting is relative to.
    ///
    /// This is the single most important number in the whole pipeline: a FIXED
    /// linear deflection is why most CAD previewers are either visibly
    /// faceted on small parts or hang on large assemblies.
    #[must_use]
    pub fn diagonal(&self) -> f64 {
        let d = [
            self.max[0] - self.min[0],
            self.max[1] - self.min[1],
            self.max[2] - self.min[2],
        ];
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    }
}

/// Tessellation quality, expressed the only way that survives contact with
/// real files: RELATIVE to the model's bounding-box diagonal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Deflection {
    /// Linear deflection as a FRACTION of the bbox diagonal (not an absolute
    /// length). 0.001..=0.005 is the useful band.
    pub linear_rel: f64,
    /// Angular deflection in degrees. 20..=30 is the useful band.
    pub angular_deg: f64,
}

impl Deflection {
    /// Thumbnail quality: coarse, fast, never the bottleneck.
    pub const THUMBNAIL: Self = Self {
        linear_rel: 0.005,
        angular_deg: 30.0,
    };

    /// Interactive preview quality.
    pub const PREVIEW: Self = Self {
        linear_rel: 0.001,
        angular_deg: 20.0,
    };

    /// Absolute linear deflection for a model of the given bbox diagonal.
    #[must_use]
    pub fn linear_abs(&self, bbox_diagonal: f64) -> f64 {
        self.linear_rel * bbox_diagonal
    }
}

/// Everything the front-ends need to draw a file.
#[derive(Debug, Clone)]
pub struct Scene {
    pub bbox: BBox,
    pub parts: Vec<Part>,
}

impl Scene {
    /// Total triangles across all parts — the number to watch in the harness.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.parts.iter().map(|p| p.mesh.triangle_count()).sum()
    }

    /// Total line segments across all parts.
    #[must_use]
    pub fn segment_count(&self) -> usize {
        self.parts.iter().map(|p| p.lines.segment_count()).sum()
    }

    /// The worst face status anywhere in the scene.
    #[must_use]
    pub fn worst_face(&self) -> Option<FaceStatus> {
        self.parts.iter().filter_map(Part::worst_face).max()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bbox_diagonal_is_euclidean() {
        let b = BBox {
            min: [0.0, 0.0, 0.0],
            max: [3.0, 4.0, 0.0],
        };
        assert!((b.diagonal() - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn deflection_scales_with_model_size() {
        // The whole point of relative deflection: the same setting yields a
        // 100x coarser absolute tolerance on a 100x larger model.
        let small = Deflection::PREVIEW.linear_abs(10.0);
        let large = Deflection::PREVIEW.linear_abs(1000.0);
        assert!((large / small - 100.0).abs() < 1e-9);
    }

    #[test]
    fn empty_mesh_is_well_formed() {
        assert!(Mesh::default().is_well_formed());
    }

    #[test]
    fn short_normals_buffer_is_rejected() {
        let m = Mesh {
            positions: vec![0.0; 9],
            normals: vec![0.0; 6],
            indices: vec![0, 1, 2],
            face_ids: vec![0],
        };
        assert!(!m.is_well_formed());
    }

    #[test]
    fn face_id_must_name_a_face() {
        let mesh = Mesh {
            positions: vec![0.0; 9],
            normals: vec![0.0; 9],
            indices: vec![0, 1, 2],
            face_ids: vec![1],
        };
        let mut part = Part {
            name: None,
            color: None,
            mesh,
            faces: vec![Face::plain(FaceStatus::Ok)],
            lines: Lines::default(),
        };
        assert!(!part.is_well_formed());
        part.faces.push(Face {
            status: FaceStatus::Approx,
            color: Some(Color {
                r: 1.0,
                g: 0.0,
                b: 0.0,
            }),
        });
        assert!(part.is_well_formed());
        assert_eq!(part.worst_face(), Some(FaceStatus::Approx));
        assert_eq!(
            part.face_color(1),
            Some(Color {
                r: 1.0,
                g: 0.0,
                b: 0.0
            })
        );
        assert_eq!(part.face_color(0), None);
    }

    #[test]
    fn face_status_order_is_severity() {
        assert!(FaceStatus::Ok < FaceStatus::Healed && FaceStatus::Approx < FaceStatus::Missing);
        assert!(FaceStatus::Coarse.is_faithful() && FaceStatus::Degenerate.is_faithful());
        assert!(!FaceStatus::Approx.is_faithful() && !FaceStatus::Missing.is_faithful());
        for v in 0..=7 {
            assert_eq!(FaceStatus::from_u8(v).map(|s| s as u8), Some(v));
        }
        assert_eq!(FaceStatus::from_u8(8), None);
    }

    #[test]
    fn out_of_range_index_is_rejected() {
        let m = Mesh {
            positions: vec![0.0; 9],
            normals: vec![0.0; 9],
            indices: vec![0, 1, 7],
            face_ids: vec![0],
        };
        assert!(!m.is_well_formed());
    }
}
