//! The viewer's GPU renderer: wgpu, independent of egui.
//!
//! Geometry lives on the GPU in one struct-of-arrays set (positions, normals,
//! a face id per vertex, indices), with each part a range in it, drawn as its
//! own instance so the shader knows the part. What varies per face and per
//! part is in storage buffers the fragment shader reads: a [`Material`] per
//! face (colour and [`FaceStatus`]), each part's first face in that table, and
//! a visibility bitset. Recolouring a face, hiding a part or (#29) picking
//! never touches the geometry, and there is no per-vertex colour.
//!
//! [`Renderer::render`] draws into a [`Target`] the viewer owns (4x MSAA
//! colour and depth, resolved to a texture egui samples), not eframe's
//! surface attachments, so the same code renders headless for the tests and
//! for screenshots ([`Headless`]).
//!
//! The camera is the software rasteriser's ([`crate::render`]), same
//! orthographic projection and sphere fit, so the GPU and software windows
//! frame a model identically. The tests hold the two to the same silhouette.

use std::ops::Range;

use bytemuck::{Pod, Zeroable};
pub use eframe::egui_wgpu::wgpu;
use wgpu::util::DeviceExt;

use super::Backend;
use crate::render::{self, Camera};
use crate::{FaceStatus, LineKind, Scene};

/// The colour targets' format: gamma-encoded bytes, which is what egui
/// samples (egui-wgpu blends in gamma space and wants `Rgba8Unorm`). The
/// shaders encode sRGB themselves; an `Srgb` target with a `Unorm` view for
/// egui would need view formats, which wgpu's GL backend lacks.
pub const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// MSAA samples. 4 is the one count WebGPU guarantees for every format.
pub const SAMPLES: u32 = 4;
/// The id pass's format: `(part + 1, face)` per pixel, 0 for nothing. Two
/// channels, so neither count is squeezed into bits (#29 allowed
/// `part << 20 | face`, which caps an assembly at 4096 parts).
pub const ID: wgpu::TextureFormat = wgpu::TextureFormat::Rg32Uint;

/// One face's look, as the shader reads it.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Material {
    /// Linear RGB.
    pub color: [f32; 3],
    /// A [`FaceStatus`] as `u32`, for #31's overlay.
    pub status: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    /// Model space to clip space, column-major.
    clip: [[f32; 4]; 4],
    /// Model space to view space (rotation only), for normals.
    rot: [[f32; 4]; 4],
    /// The section plane: keep points with `dot(p, xyz) <= w`.
    section: [f32; 4],
    /// The picked face's highlight, linear RGB and opacity.
    highlight: [f32; 4],
    /// x: draw construction curves; y: the approximated-face stripe width
    /// in physical pixels; z: the section is on.
    flags: [u32; 4],
    /// The picked face: x = part + 1 (0 for none), y = face.
    pick: [u32; 4],
}

/// The bounding sphere [`render::render`]'s `Fit::Sphere` frames: the centre
/// and half-diagonal of the box around everything drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub mid: [f32; 3],
    pub radius: f32,
}

impl Fit {
    /// The fit of `points`, `None` when there are none.
    fn of(points: impl Iterator<Item = [f32; 3]>) -> Option<Self> {
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        let mut any = false;
        for p in points {
            any = true;
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        any.then(|| {
            let d = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
            Self {
                mid: [
                    (lo[0] + hi[0]) / 2.0,
                    (lo[1] + hi[1]) / 2.0,
                    (lo[2] + hi[2]) / 2.0,
                ],
                radius: ((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() / 2.0).max(f32::EPSILON),
            }
        })
    }
}

/// `render.rs`'s view rotation as rows: CAD Z-up to a Y-up view, azimuth
/// about the view's Y, then elevation about its X. `+z` points at the eye.
#[must_use]
pub fn rotation(cam: &Camera) -> [[f32; 3]; 3] {
    let (sa, ca) = cam.azimuth_deg.to_radians().sin_cos();
    let (se, ce) = cam.elevation_deg.to_radians().sin_cos();
    [
        [ca, -sa, 0.0],
        [sa * se, ca * se, ce],
        [-sa * ce, -ca * ce, se],
    ]
}

fn mul(m: &[[f32; 3]; 3], p: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2])
}

/// Model space to clip space for a `width` × `height` target: `render.rs`'s
/// screen mapping (fit margin, zoom, pan in short-edge units) as an
/// orthographic matrix. Depth maps the fit sphere into 0.25..0.75, nearer
/// smaller. Column-major, as WGSL takes it.
#[must_use]
pub fn clip_matrix(cam: &Camera, fit: &Fit, width: u32, height: u32) -> [[f32; 4]; 4] {
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    let short = w.min(h);
    let margin = 0.06 * short;
    let scale = (short - 2.0 * margin) / (2.0 * fit.radius) * cam.zoom.max(1e-3);
    let r = rotation(cam);
    let c = mul(&r, fit.mid);
    let (sx, sy) = (2.0 * scale / w, 2.0 * scale / h);
    let sz = -1.0 / (4.0 * fit.radius);
    let rows = [
        [
            sx * r[0][0],
            sx * r[0][1],
            sx * r[0][2],
            (2.0 / w) * (-scale * c[0] + cam.pan[0] * short),
        ],
        [
            sy * r[1][0],
            sy * r[1][1],
            sy * r[1][2],
            (2.0 / h) * (-scale * c[1] - cam.pan[1] * short),
        ],
        [sz * r[2][0], sz * r[2][1], sz * r[2][2], 0.5 - sz * c[2]],
        [0.0, 0.0, 0.0, 1.0],
    ];
    transpose(rows)
}

fn transpose(m: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    [0, 1, 2, 3].map(|c| [m[0][c], m[1][c], m[2][c], m[3][c]])
}

/// Per-part visibility, one bit per part, as the shader reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Visibility {
    words: Vec<u32>,
    len: usize,
}

impl Visibility {
    /// `len` parts, all shown.
    #[must_use]
    pub fn all(len: usize) -> Self {
        let mut words = vec![u32::MAX; len.div_ceil(32).max(1)];
        if !len.is_multiple_of(32) {
            *words.last_mut().unwrap() = (1u32 << (len % 32)) - 1;
        }
        if len == 0 {
            words[0] = 0;
        }
        Self { words, len }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[must_use]
    pub fn get(&self, part: usize) -> bool {
        part < self.len && self.words[part / 32] & (1 << (part % 32)) != 0
    }

    pub fn set(&mut self, part: usize, shown: bool) {
        if part < self.len {
            let bit = 1 << (part % 32);
            if shown {
                self.words[part / 32] |= bit;
            } else {
                self.words[part / 32] &= !bit;
            }
        }
    }

    /// The bitset, as uploaded: part `i` is bit `i % 32` of word `i / 32`.
    #[must_use]
    pub fn words(&self) -> &[u32] {
        &self.words
    }
}

/// Where one part sits in the shared buffers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartRange {
    /// Added to the part's (part-local) indices.
    pub base_vertex: i32,
    pub indices: Range<u32>,
    /// Line vertices, two per segment.
    pub lines: Range<u32>,
}

/// A [`Scene`] flattened into the GPU's layout, before upload. Built on the
/// CPU and tested without a GPU; [`GpuScene::upload`] consumes it, so the
/// CPU copy is gone once the GPU has it.
#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    /// The B-rep face of each vertex, part-local.
    pub faces: Vec<u32>,
    /// Part-local: add the part's `base_vertex`.
    pub indices: Vec<u32>,
    pub materials: Vec<Material>,
    /// Each part's first face in `materials`.
    pub face_base: Vec<u32>,
    pub line_positions: Vec<f32>,
    /// A [`LineKind`] per line vertex.
    pub line_kinds: Vec<u32>,
    pub parts: Vec<PartRange>,
    /// The fit without, and with, construction curves (`render.rs` frames
    /// only what it draws).
    pub fits: [Option<Fit>; 2],
    /// Vertices duplicated because triangles of two faces shared them.
    pub split_vertices: usize,
    /// The box around every mesh vertex and line point, for the section
    /// plane's range.
    pub bounds: Option<([f32; 3], [f32; 3])>,
}

impl Layout {
    #[must_use]
    pub fn new(scene: &Scene) -> Self {
        let mut l = Self::default();
        for part in &scene.parts {
            let m = &part.mesh;
            let base_vertex = (l.positions.len() / 3) as i32;
            // A face id per vertex. STEPVMSH writes each face's vertices
            // separately, so a vertex has one face; when an exporter's mesh
            // shares one across faces anyway, it is duplicated, never
            // mis-coloured.
            let n = m.positions.len() / 3;
            let mut face_of = vec![u32::MAX; n];
            // (vertex, face) -> its copy, and the copies in order.
            let mut copies: std::collections::HashMap<(u32, u32), u32> = Default::default();
            let mut extra: Vec<(u32, u32)> = Vec::new();
            let first_index = l.indices.len() as u32;
            for (tri, &f) in m.indices.chunks_exact(3).zip(&m.face_ids) {
                for &v in tri {
                    let vi = v as usize;
                    let idx = if face_of[vi] == u32::MAX || face_of[vi] == f {
                        face_of[vi] = f;
                        v
                    } else {
                        *copies.entry((v, f)).or_insert_with(|| {
                            extra.push((v, f));
                            (n + extra.len() - 1) as u32
                        })
                    };
                    l.indices.push(idx);
                }
            }
            l.positions.extend_from_slice(&m.positions);
            l.normals.extend_from_slice(&m.normals);
            l.faces
                .extend(face_of.iter().map(|&f| if f == u32::MAX { 0 } else { f }));
            for &(v, f) in &extra {
                let v = v as usize;
                l.positions
                    .extend_from_slice(&m.positions[v * 3..v * 3 + 3]);
                l.normals.extend_from_slice(&m.normals[v * 3..v * 3 + 3]);
                l.faces.push(f);
            }
            l.split_vertices += extra.len();

            l.face_base.push(l.materials.len() as u32);
            let faces = part.faces.len().max(1);
            l.materials.extend((0..faces).map(|f| {
                let c = part.face_color(f as u32).unwrap_or(render::DEFAULT_COLOR);
                Material {
                    color: [c.r, c.g, c.b],
                    status: part.faces.get(f).map_or(FaceStatus::Ok, |f| f.status) as u32,
                }
            }));

            let first_line = (l.line_positions.len() / 3) as u32;
            l.line_positions.extend_from_slice(&part.lines.positions);
            for &k in &part.lines.kinds {
                l.line_kinds.extend([k as u32, k as u32]);
            }
            l.parts.push(PartRange {
                base_vertex,
                indices: first_index..l.indices.len() as u32,
                lines: first_line..(l.line_positions.len() / 3) as u32,
            });
        }
        let points = |construction: bool| {
            let mesh = scene.parts.iter().flat_map(|p| {
                p.mesh
                    .indices
                    .iter()
                    .map(|&i| vtx(&p.mesh.positions, i as usize))
            });
            let lines = scene.parts.iter().flat_map(move |p| {
                p.lines
                    .kinds
                    .iter()
                    .enumerate()
                    .filter(move |(_, k)| construction || **k != LineKind::Construction)
                    .flat_map(|(s, _)| {
                        [
                            vtx(&p.lines.positions, 2 * s),
                            vtx(&p.lines.positions, 2 * s + 1),
                        ]
                    })
            });
            mesh.chain(lines)
        };
        l.fits = [Fit::of(points(false)), Fit::of(points(true))];
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for p in l
            .positions
            .chunks_exact(3)
            .chain(l.line_positions.chunks_exact(3))
        {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        l.bounds = (lo[0] <= hi[0]).then_some((lo, hi));
        l
    }
}

fn vtx(positions: &[f32], i: usize) -> [f32; 3] {
    [positions[3 * i], positions[3 * i + 1], positions[3 * i + 2]]
}

/// What to draw: the camera, the toggles, and the background.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub camera: Camera,
    pub show_construction: bool,
    /// sRGB-encoded RGBA, premultiplied (`Color32::to_normalized_gamma_f32`).
    pub clear: [f64; 4],
    /// The approximated-face stripes' width in physical pixels: render.rs's
    /// 6, times the display scale.
    pub stripe: u32,
    /// The section plane, as [`Section::plane`] gives it: everything with
    /// `dot(p, xyz) > w` is cut away, in the image, the picks and the
    /// highlight alike.
    pub section: Option<[f32; 4]>,
    /// The picked face (part, face), drawn highlighted.
    pub picked: Option<Pick>,
    /// The highlight's colour, linear RGB.
    pub highlight: [f32; 3],
}

impl View {
    /// `camera` with every toggle off, on a transparent background.
    #[must_use]
    pub fn new(camera: Camera) -> Self {
        Self {
            camera,
            show_construction: false,
            clear: [0.0; 4],
            stripe: 6,
            section: None,
            picked: None,
            highlight: [1.0, 0.75, 0.0],
        }
    }
}

/// What a pick hit: a B-rep face of a part, as the mesh and `--topology`
/// number them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pick {
    pub part: u32,
    pub face: u32,
}

/// A section plane across the model's bounding box: perpendicular to `axis`
/// (0 = x, 1 = y, 2 = z) at `offset` (0..1 of the box), keeping the side
/// below it, or above it when `flip`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Section {
    pub axis: usize,
    pub offset: f32,
    pub flip: bool,
}

impl Section {
    /// The plane `(n, w)` for the model bounds `lo..hi`: kept is
    /// `dot(p, n) <= w`.
    #[must_use]
    pub fn plane(&self, lo: [f32; 3], hi: [f32; 3]) -> [f32; 4] {
        let k = self.axis.min(2);
        let at = lo[k] + (hi[k] - lo[k]) * self.offset.clamp(0.0, 1.0);
        let mut n = [0.0; 4];
        if self.flip {
            n[k] = -1.0;
            n[3] = -at;
        } else {
            n[k] = 1.0;
            n[3] = at;
        }
        n
    }
}

/// The offscreen target the viewer owns.
pub struct Target {
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    /// The resolved colour, what egui (or a readback) samples.
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub width: u32,
    pub height: u32,
}

impl Target {
    #[must_use]
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let (width, height) = (width.max(1), height.max(1));
        let tex = |label, format, samples, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let attach = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let msaa = tex("stepv msaa", COLOR, SAMPLES, attach).create_view(&Default::default());
        let depth = tex("stepv depth", DEPTH, SAMPLES, attach).create_view(&Default::default());
        let texture = tex(
            "stepv view",
            COLOR,
            1,
            attach | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        );
        let view = texture.create_view(&Default::default());
        Self {
            msaa,
            depth,
            texture,
            view,
            width,
            height,
        }
    }
}

const SHADER: &str = r"
struct U {
  clip: mat4x4<f32>, rot: mat4x4<f32>, section: vec4<f32>, highlight: vec4<f32>,
  flags: vec4<u32>, pick: vec4<u32>,
};
struct Material { color: vec3<f32>, status: u32 };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var<storage, read> materials: array<Material>;
@group(0) @binding(2) var<storage, read> face_base: array<u32>;
@group(0) @binding(3) var<storage, read> visible: array<u32>;

// render.rs's srgb(): the targets are Rgba8Unorm, so encode here.
fn encode(c: vec3<f32>) -> vec4<f32> {
  let v = clamp(c, vec3(0.0), vec3(1.0));
  let lo = v * 12.92;
  let hi = 1.055 * pow(v, vec3(1.0 / 2.4)) - 0.055;
  return vec4(select(hi, lo, v <= vec3(0.0031308)), 1.0);
}

fn shown(part: u32) -> bool {
  return (visible[part >> 5u] & (1u << (part & 31u))) != 0u;
}

// Cut away by the section plane: every pass asks, so nothing is drawn,
// picked or highlighted through a section.
fn cut(world: vec3<f32>) -> bool {
  return u.flags.z == 1u && dot(world, u.section.xyz) > u.section.w;
}

struct MeshOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) nrm: vec3<f32>,
  @location(1) vpos: vec3<f32>,
  @location(2) @interpolate(flat) id: vec2<u32>,
  @location(3) world: vec3<f32>,
};

@vertex fn vs_mesh(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) face: u32,
                   @builtin(instance_index) part: u32) -> MeshOut {
  var o: MeshOut;
  o.pos = u.clip * vec4(p, 1.0);
  o.nrm = (u.rot * vec4(n, 0.0)).xyz;
  o.vpos = (u.rot * vec4(p, 1.0)).xyz;
  o.id = vec2(part, face);
  o.world = p;
  return o;
}


@fragment fn fs_mesh(i: MeshOut) -> @location(0) vec4<f32> {
  // Before any discard: derivatives need uniform control flow.
  let flat_n = cross(dpdx(i.vpos), dpdy(i.vpos));
  if (!shown(i.id.x) || cut(i.world)) { discard; }
  // The kernel's smooth normal, or the facet's where it wrote none.
  var n = i.nrm;
  if (dot(n, n) < 1e-12) { n = flat_n; }
  n = normalize(n);
  let m = materials[face_base[i.id.x] + i.id.y];
  // render.rs's two lights, in view space; two-sided, as there.
  let key = normalize(vec3(0.35, 0.75, 0.55));
  let fill = normalize(vec3(-0.6, 0.2, 0.4));
  let light = 0.22 + 0.62 * abs(dot(n, key)) + 0.16 * abs(dot(n, fill));
  var c = m.color * light;
  // Approximated faces: render.rs's diagonal amber stripes, 75% over the
  // shaded colour (#31 adds the rest of the overlay).
  if (m.status == STATUS_APPROX) {
    let band = u32(floor((i.pos.x + i.pos.y) / f32(u.flags.y))) % 2u == 0u;
    c = mix(c, select(APPROX_DARK, APPROX, band), 0.75);
  }
  return encode(c);
}

// The id pass (#29): which face each pixel shows. Single-sample, so a pixel
// is one face, never a blend of two.
@fragment fn fs_id(i: MeshOut) -> @location(0) vec2<u32> {
  if (!shown(i.id.x) || cut(i.world)) { discard; }
  return vec2(i.id.x + 1u, i.id.y);
}

// The picked face, over the shaded one: a depth-biased second pass of just
// that face's part, so it is a highlight and not a recolour.
@fragment fn fs_highlight(i: MeshOut) -> @location(0) vec4<f32> {
  if (!shown(i.id.x) || cut(i.world) || i.id.x + 1u != u.pick.x || i.id.y != u.pick.y) {
    discard;
  }
  return vec4(encode(u.highlight.rgb).rgb, u.highlight.a);
}

struct LineOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) @interpolate(flat) kind_part: vec2<u32>,
  @location(1) world: vec3<f32>,
};

@vertex fn vs_line(@location(0) p: vec3<f32>, @location(1) kind: u32,
                   @builtin(instance_index) part: u32) -> LineOut {
  var o: LineOut;
  o.pos = u.clip * vec4(p, 1.0);
  // Missing-face outlines draw over everything, as in render.rs.
  if (kind == 1u) { o.pos.z = 0.0; }
  // Hidden construction curves: outside the clip volume.
  if (kind == 2u && u.flags.x == 0u) { o.pos = vec4(2.0, 2.0, 2.0, 1.0); }
  o.kind_part = vec2(kind, part);
  o.world = p;
  return o;
}

@fragment fn fs_line(i: LineOut) -> @location(0) vec4<f32> {
  if (!shown(i.kind_part.y) || cut(i.world)) { discard; }
  switch i.kind_part.x {
    case 1u: { return encode(MISSING); }
    case 2u: { return encode(CONSTRUCTION); }
    default: { return encode(SKETCH); }
  }
}
";

/// The pipelines, built once per device.
pub struct Renderer {
    mesh: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    highlight: wgpu::RenderPipeline,
    id: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

/// How a pipeline writes: its colour target, sample count and depth state.
struct Output {
    target: wgpu::ColorTargetState,
    samples: u32,
    depth_write: bool,
    bias: wgpu::DepthBiasState,
}

impl Output {
    fn shaded() -> Self {
        Self {
            target: COLOR.into(),
            samples: SAMPLES,
            depth_write: true,
            bias: Default::default(),
        }
    }
}

impl Renderer {
    #[must_use]
    pub fn new(device: &wgpu::Device) -> Self {
        let rgb = |c: [f32; 3]| format!("vec3({:?}, {:?}, {:?})", c[0], c[1], c[2]);
        let consts = format!(
            "const MISSING = {};\nconst CONSTRUCTION = {};\nconst SKETCH = {};\n\
             const APPROX = {};\nconst APPROX_DARK = {};\nconst STATUS_APPROX = {}u;\n",
            rgb(render::MISSING),
            rgb(render::CONSTRUCTION),
            rgb(render::SKETCH),
            rgb(render::APPROX),
            rgb(render::APPROX_DARK),
            FaceStatus::Approx as u32
        );
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stepv view"),
            source: wgpu::ShaderSource::Wgsl((consts + SHADER).into()),
        });
        let storage = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("stepv scene"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                storage(1),
                storage(2),
                storage(3),
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("stepv"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline =
            |label, vs, fs, buffers: &[Option<wgpu::VertexBufferLayout>], topology, out: Output| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&pl),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some(vs),
                        compilation_options: Default::default(),
                        buffers,
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some(fs),
                        compilation_options: Default::default(),
                        targets: &[Some(out.target)],
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology,
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH,
                        depth_write_enabled: Some(out.depth_write),
                        depth_compare: Some(wgpu::CompareFunction::LessEqual),
                        stencil: Default::default(),
                        bias: out.bias,
                    }),
                    multisample: wgpu::MultisampleState {
                        count: out.samples,
                        ..Default::default()
                    },
                    multiview_mask: None,
                    cache: None,
                })
            };
        let vec3 = |location| {
            Some(wgpu::VertexBufferLayout {
                array_stride: 12,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: match location {
                    0 => &wgpu::vertex_attr_array![0 => Float32x3],
                    _ => &wgpu::vertex_attr_array![1 => Float32x3],
                },
            })
        };
        let u32_at = |location| {
            Some(wgpu::VertexBufferLayout {
                array_stride: 4,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: match location {
                    1 => &wgpu::vertex_attr_array![1 => Uint32],
                    _ => &wgpu::vertex_attr_array![2 => Uint32],
                },
            })
        };
        let mesh = pipeline(
            "stepv mesh",
            "vs_mesh",
            "fs_mesh",
            &[vec3(0), vec3(1), u32_at(2)],
            wgpu::PrimitiveTopology::TriangleList,
            Output::shaded(),
        );
        let lines = pipeline(
            "stepv lines",
            "vs_line",
            "fs_line",
            &[vec3(0), u32_at(1)],
            wgpu::PrimitiveTopology::LineList,
            Output::shaded(),
        );
        // Drawn over the shaded face at the same depth: pulled toward the
        // eye, it wins the depth test against its own face and loses to
        // anything genuinely in front.
        let highlight = pipeline(
            "stepv highlight",
            "vs_mesh",
            "fs_highlight",
            &[vec3(0), vec3(1), u32_at(2)],
            wgpu::PrimitiveTopology::TriangleList,
            Output {
                target: wgpu::ColorTargetState {
                    format: COLOR,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                },
                samples: SAMPLES,
                depth_write: false,
                bias: wgpu::DepthBiasState {
                    constant: -4,
                    slope_scale: -1.0,
                    clamp: 0.0,
                },
            },
        );
        let id = pipeline(
            "stepv id",
            "vs_mesh",
            "fs_id",
            &[vec3(0), vec3(1), u32_at(2)],
            wgpu::PrimitiveTopology::TriangleList,
            Output {
                target: ID.into(),
                samples: 1,
                depth_write: true,
                bias: Default::default(),
            },
        );
        Self {
            mesh,
            lines,
            highlight,
            id,
            layout,
        }
    }

    /// Encodes one frame of `scene` into `target`.
    #[must_use]
    pub fn render(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &GpuScene,
        target: &Target,
        view: &View,
    ) -> wgpu::CommandBuffer {
        queue.write_buffer(
            &scene.uniforms,
            0,
            bytemuck::bytes_of(&uniforms(scene, view, target.width, target.height)),
        );
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("stepv frame"),
        });
        {
            let [r, g, b, a] = view.clear;
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("stepv"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.msaa,
                    depth_slice: None,
                    resolve_target: Some(&target.view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &scene.bind, &[]);
            if scene.has_mesh {
                pass.set_pipeline(&self.mesh);
                draw_mesh(&mut pass, scene, None);
                if let Some(pick) = view.picked {
                    pass.set_pipeline(&self.highlight);
                    draw_mesh(&mut pass, scene, Some(pick.part as usize));
                }
            }
            if scene.has_lines {
                pass.set_pipeline(&self.lines);
                pass.set_vertex_buffer(0, scene.line_positions.slice(..));
                pass.set_vertex_buffer(1, scene.line_kinds.slice(..));
                for (i, p) in scene.parts.iter().enumerate() {
                    if scene.visibility.get(i) && !p.lines.is_empty() {
                        let i = i as u32;
                        pass.draw(p.lines.clone(), i..i + 1);
                    }
                }
            }
        }
        enc.finish()
    }
}

impl Renderer {
    /// Starts a pick at pixel `at` (x, y) of a `target`-sized view: renders
    /// the id pass and copies that one texel out. The result arrives
    /// asynchronously ([`PendingPick::poll`]), so a click never stalls a
    /// frame on the GPU.
    #[must_use]
    pub fn pick(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &GpuScene,
        target: &IdTarget,
        view: &View,
        at: (u32, u32),
    ) -> PendingPick {
        let (x, y) = (at.0.min(target.width - 1), at.1.min(target.height - 1));
        queue.write_buffer(
            &scene.uniforms,
            0,
            bytemuck::bytes_of(&uniforms(scene, view, target.width, target.height)),
        );
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("stepv pick"),
        });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("stepv id"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            if scene.has_mesh {
                pass.set_bind_group(0, &scene.bind, &[]);
                pass.set_pipeline(&self.id);
                draw_mesh(&mut pass, scene, None);
            }
        }
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("stepv pick"),
            size: u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
                    rows_per_image: Some(1),
                },
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([enc.finish()]);
        let state = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0));
        let done = state.clone();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            done.store(
                if r.is_ok() { 1 } else { 2 },
                std::sync::atomic::Ordering::Release,
            );
        });
        PendingPick { buffer, state }
    }
}

/// The uniforms for drawing `scene` as `view` into a `w` x `h` target.
fn uniforms(scene: &GpuScene, view: &View, w: u32, h: u32) -> Uniforms {
    let fit = scene.fits[usize::from(view.show_construction)]
        .or(scene.fits[1])
        .unwrap_or(Fit {
            mid: [0.0; 3],
            radius: 1.0,
        });
    let r = rotation(&view.camera);
    let rot = transpose([
        [r[0][0], r[0][1], r[0][2], 0.0],
        [r[1][0], r[1][1], r[1][2], 0.0],
        [r[2][0], r[2][1], r[2][2], 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);
    let [hr, hg, hb] = view.highlight;
    Uniforms {
        clip: clip_matrix(&view.camera, &fit, w, h),
        rot,
        section: view.section.unwrap_or([0.0; 4]),
        highlight: [hr, hg, hb, 0.55],
        flags: [
            u32::from(view.show_construction),
            view.stripe.max(1),
            u32::from(view.section.is_some()),
            0,
        ],
        pick: view.picked.map_or([0; 4], |p| [p.part + 1, p.face, 0, 0]),
    }
}

/// Draws every shown part's triangles, or only `only`'s.
fn draw_mesh(pass: &mut wgpu::RenderPass<'_>, scene: &GpuScene, only: Option<usize>) {
    pass.set_vertex_buffer(0, scene.positions.slice(..));
    pass.set_vertex_buffer(1, scene.normals.slice(..));
    pass.set_vertex_buffer(2, scene.faces.slice(..));
    pass.set_index_buffer(scene.indices.slice(..), wgpu::IndexFormat::Uint32);
    for (i, p) in scene.parts.iter().enumerate() {
        if only.is_some_and(|o| o != i) || !scene.visibility.get(i) || p.indices.is_empty() {
            continue;
        }
        let i = i as u32;
        pass.draw_indexed(p.indices.clone(), p.base_vertex, i..i + 1);
    }
}

/// The id pass's target: single-sample ids and depth.
pub struct IdTarget {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    depth: wgpu::TextureView,
    pub width: u32,
    pub height: u32,
}

impl IdTarget {
    #[must_use]
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let (width, height) = (width.max(1), height.max(1));
        let tex = |label, format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let attach = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let texture = tex("stepv ids", ID, attach | wgpu::TextureUsages::COPY_SRC);
        let view = texture.create_view(&Default::default());
        let depth = tex("stepv id depth", DEPTH, attach).create_view(&Default::default());
        Self {
            texture,
            view,
            depth,
            width,
            height,
        }
    }
}

/// A pick in flight.
pub struct PendingPick {
    buffer: wgpu::Buffer,
    /// 0 pending, 1 mapped, 2 failed.
    state: std::sync::Arc<std::sync::atomic::AtomicU8>,
}

impl PendingPick {
    /// `None` while the GPU is still busy; then what was hit, `Some(None)`
    /// for the background (or a failed readback). Never blocks.
    #[must_use]
    pub fn poll(&self, device: &wgpu::Device) -> Option<Option<Pick>> {
        let _ = device.poll(wgpu::PollType::Poll);
        match self.state.load(std::sync::atomic::Ordering::Acquire) {
            0 => None,
            1 => {
                let data = self.buffer.slice(..).get_mapped_range().ok()?;
                let word =
                    |i: usize| u32::from_le_bytes(data[4 * i..4 * i + 4].try_into().unwrap());
                let (part, face) = (word(0), word(1));
                Some((part != 0).then(|| Pick {
                    part: part - 1,
                    face,
                }))
            }
            _ => Some(None),
        }
    }

    /// Waits for the GPU: tests only, the viewer polls.
    #[must_use]
    pub fn wait(&self, device: &wgpu::Device) -> Option<Pick> {
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        self.poll(device).flatten()
    }
}

/// A scene on the GPU.
pub struct GpuScene {
    positions: wgpu::Buffer,
    normals: wgpu::Buffer,
    faces: wgpu::Buffer,
    indices: wgpu::Buffer,
    line_positions: wgpu::Buffer,
    line_kinds: wgpu::Buffer,
    uniforms: wgpu::Buffer,
    visible: wgpu::Buffer,
    bind: wgpu::BindGroup,
    has_mesh: bool,
    has_lines: bool,
    pub parts: Vec<PartRange>,
    pub fits: [Option<Fit>; 2],
    /// The model's box, for the section plane.
    pub bounds: Option<([f32; 3], [f32; 3])>,
    /// The CPU mirror of the GPU bitset: hidden parts are not drawn at all.
    visibility: Visibility,
}

impl GpuScene {
    /// Uploads `layout`, consuming it.
    #[must_use]
    pub fn upload(device: &wgpu::Device, renderer: &Renderer, layout: Layout) -> Self {
        let buf = |label, contents: &[u8], usage| {
            // wgpu refuses zero-sized bindings; one zero word stands in.
            let contents = if contents.is_empty() {
                &[0u8; 16][..]
            } else {
                contents
            };
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents,
                usage,
            })
        };
        let vertex = wgpu::BufferUsages::VERTEX;
        let storage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        let visibility = Visibility::all(layout.parts.len());
        let positions = buf("positions", bytemuck::cast_slice(&layout.positions), vertex);
        let normals = buf("normals", bytemuck::cast_slice(&layout.normals), vertex);
        let faces = buf("faces", bytemuck::cast_slice(&layout.faces), vertex);
        let indices = buf(
            "indices",
            bytemuck::cast_slice(&layout.indices),
            wgpu::BufferUsages::INDEX,
        );
        let line_positions = buf(
            "line positions",
            bytemuck::cast_slice(&layout.line_positions),
            vertex,
        );
        let line_kinds = buf(
            "line kinds",
            bytemuck::cast_slice(&layout.line_kinds),
            vertex,
        );
        let materials = buf(
            "materials",
            bytemuck::cast_slice(&layout.materials),
            storage,
        );
        let face_base = buf(
            "face base",
            bytemuck::cast_slice(&layout.face_base),
            storage,
        );
        let visible = buf(
            "visibility",
            bytemuck::cast_slice(visibility.words()),
            storage,
        );
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("stepv scene"),
            layout: &renderer.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: materials.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: face_base.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: visible.as_entire_binding(),
                },
            ],
        });
        Self {
            positions,
            normals,
            faces,
            indices,
            line_positions,
            line_kinds,
            uniforms,
            visible,
            bind,
            has_mesh: !layout.indices.is_empty(),
            has_lines: !layout.line_kinds.is_empty(),
            parts: layout.parts,
            fits: layout.fits,
            bounds: layout.bounds,
            visibility,
        }
    }

    #[must_use]
    pub fn visibility(&self) -> &Visibility {
        &self.visibility
    }

    /// Shows only the parts `vis` shows.
    pub fn set_visibility(&mut self, queue: &wgpu::Queue, vis: Visibility) {
        assert_eq!(vis.len(), self.visibility.len(), "one bit per part");
        queue.write_buffer(&self.visible, 0, bytemuck::cast_slice(vis.words()));
        self.visibility = vis;
    }
}

/// The device limits the viewer requests: WebGPU's downlevel set (which
/// has the storage buffers fragment shaders read here; WebGL2-class GL has
/// none), at the adapter's own texture size up to 8192.
#[must_use]
pub fn limits(adapter: &wgpu::Adapter) -> wgpu::Limits {
    wgpu::Limits {
        max_texture_dimension_2d: adapter.limits().max_texture_dimension_2d.min(8192),
        ..wgpu::Limits::downlevel_defaults()
    }
}

/// Whether `adapter` can run the viewer: it grants every limit [`limits`]
/// asks for (so the device request cannot then fail on them), reads storage
/// buffers in fragment shaders, and offsets indices by a base vertex (each
/// part's draw does).
#[must_use]
pub fn usable(adapter: &wgpu::Adapter) -> bool {
    let flags = adapter.get_downlevel_capabilities().flags;
    limits(adapter).check_limits(&adapter.limits())
        && flags.contains(wgpu::DownlevelFlags::FRAGMENT_STORAGE)
        && flags.contains(wgpu::DownlevelFlags::BASE_VERTEX)
}

/// eframe's adapter choice (#28 review): the first usable adapter that can
/// present to `surface`, a discrete GPU before an integrated one before a
/// CPU rasteriser. An error when there is none, so the window fails before
/// it opens and `stepv view` falls back to software.
///
/// # Errors
/// When no adapter is usable.
pub fn select(
    adapters: &[wgpu::Adapter],
    surface: Option<&wgpu::Surface<'_>>,
) -> Result<wgpu::Adapter, String> {
    let rank = |a: &wgpu::Adapter| match a.get_info().device_type {
        wgpu::DeviceType::DiscreteGpu => 0,
        wgpu::DeviceType::IntegratedGpu => 1,
        wgpu::DeviceType::VirtualGpu | wgpu::DeviceType::Other => 2,
        wgpu::DeviceType::Cpu => 3,
    };
    adapters
        .iter()
        .filter(|a| usable(a) && surface.is_none_or(|s| a.is_surface_supported(s)))
        .min_by_key(|a| rank(a))
        .cloned()
        .ok_or_else(|| "no usable GPU adapter for this window".to_owned())
}

fn instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::from_env()
            .unwrap_or(wgpu::Backends::PRIMARY | wgpu::Backends::GL),
        flags: wgpu::InstanceFlags::from_build_config().with_env(),
        backend_options: wgpu::BackendOptions::from_env_or_default(),
        memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
        display: None,
    })
}

fn adapter(instance: &wgpu::Instance) -> Option<wgpu::Adapter> {
    pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference:
            wgpu::PowerPreference::from_env().unwrap_or(wgpu::PowerPreference::HighPerformance),
        force_fallback_adapter: false,
        compatible_surface: None,
        apply_limit_buckets: false,
    }))
    .ok()
    .filter(usable)
}

/// The backend `stepv view` would get, `None` without a usable adapter.
/// Honours wgpu's `WGPU_BACKEND`, as the window does.
#[must_use]
pub fn probe() -> Option<Backend> {
    adapter(&instance()).map(|a| Backend::from_wgpu(a.get_info().backend))
}

/// A device without a window: the tests' and screenshots' renderer.
pub struct Headless {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub backend: Backend,
    pub adapter: String,
    renderer: Renderer,
}

impl Headless {
    /// `None` when there is no usable adapter.
    #[must_use]
    pub fn new() -> Option<Self> {
        let adapter = adapter(&instance())?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("stepv headless"),
            required_limits: limits(&adapter),
            ..Default::default()
        }))
        .ok()?;
        let info = adapter.get_info();
        let renderer = Renderer::new(&device);
        Some(Self {
            device,
            queue,
            backend: Backend::from_wgpu(info.backend),
            adapter: info.name,
            renderer,
        })
    }

    #[must_use]
    pub fn upload(&self, scene: &Scene) -> GpuScene {
        GpuScene::upload(&self.device, &self.renderer, Layout::new(scene))
    }

    /// Picks pixel (`x`, `y`) of a `width` × `height` view, waiting for it.
    #[must_use]
    pub fn pick(
        &self,
        scene: &GpuScene,
        view: &View,
        width: u32,
        height: u32,
        x: u32,
        y: u32,
    ) -> Option<Pick> {
        let target = IdTarget::new(&self.device, width, height);
        self.renderer
            .pick(&self.device, &self.queue, scene, &target, view, (x, y))
            .wait(&self.device)
    }

    /// Renders `scene` at `width` × `height` and reads it back.
    #[must_use]
    pub fn render(&self, scene: &GpuScene, view: &View, width: u32, height: u32) -> render::Image {
        let target = Target::new(&self.device, width, height);
        let frame = self
            .renderer
            .render(&self.device, &self.queue, scene, &target, view);
        self.queue.submit([frame]);
        read_back(&self.device, &self.queue, &target)
    }
}

/// The GPU tests' gate: a missing adapter skips them, loudly, unless
/// `STEPV_REQUIRE_GPU=1` says this machine has one (macOS CI; Linux CI once
/// #32 brings lavapipe), where it fails them. A suite that quietly skips its
/// subject is how a broken renderer stays green.
///
/// # Panics
/// With `STEPV_REQUIRE_GPU=1` and no usable adapter.
#[doc(hidden)]
#[must_use]
pub fn require_gpu(h: Option<Headless>) -> Option<Headless> {
    if h.is_none() {
        assert!(
            std::env::var_os("STEPV_REQUIRE_GPU").is_none_or(|v| v != "1"),
            "STEPV_REQUIRE_GPU=1 but there is no usable GPU adapter"
        );
        eprintln!("SKIPPING: no usable GPU adapter (set STEPV_REQUIRE_GPU=1 to fail instead)");
    }
    h
}

/// Copies `target`'s resolved colour into an [`render::Image`].
#[must_use]
pub fn read_back(device: &wgpu::Device, queue: &wgpu::Queue, target: &Target) -> render::Image {
    let (w, h) = (target.width, target.height);
    let row = (4 * w).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(row * h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        target.texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([enc.finish()]);
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("map the readback buffer"));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("wait for the GPU");
    let data = slice.get_mapped_range().expect("read the mapped buffer");
    let mut rgba = Vec::with_capacity((4 * w * h) as usize);
    for y in 0..h as usize {
        let start = y * row as usize;
        rgba.extend_from_slice(&data[start..start + 4 * w as usize]);
    }
    render::Image {
        width: w,
        height: h,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Face, Lines, Mesh, Part};

    /// An axis-aligned box with per-face vertices and outward normals, one
    /// B-rep face per side.
    fn cuboid(lo: [f32; 3], hi: [f32; 3], colors: [Option<Color>; 6]) -> Part {
        let mut m = Mesh::default();
        let mut faces = Vec::new();
        for (f, (axis, side)) in [(0, 0), (0, 1), (1, 0), (1, 1), (2, 0), (2, 1)]
            .into_iter()
            .enumerate()
        {
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let base = (m.positions.len() / 3) as u32;
            for (a, b) in [(0, 0), (1, 0), (1, 1), (0, 1)] {
                let mut p = [0.0; 3];
                p[axis] = if side == 0 { lo[axis] } else { hi[axis] };
                p[u] = if a == 0 { lo[u] } else { hi[u] };
                p[v] = if b == 0 { lo[v] } else { hi[v] };
                m.positions.extend_from_slice(&p);
                let mut n = [0.0; 3];
                n[axis] = if side == 0 { -1.0 } else { 1.0 };
                m.normals.extend_from_slice(&n);
            }
            m.indices
                .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            m.face_ids.extend([f as u32, f as u32]);
            faces.push(Face {
                status: FaceStatus::Ok,
                color: colors[f],
            });
        }
        Part {
            name: None,
            color: None,
            mesh: m,
            faces,
            lines: Lines::default(),
        }
    }

    fn scene(parts: Vec<Part>) -> Scene {
        Scene {
            bbox: crate::BBox {
                min: [0.0; 3],
                max: [1.0; 3],
            },
            parts,
        }
    }

    fn view(camera: Camera) -> View {
        View::new(camera)
    }

    fn gpu() -> Option<Headless> {
        require_gpu(Headless::new())
    }

    fn mask(img: &render::Image) -> Vec<bool> {
        img.rgba.chunks_exact(4).map(|p| p[3] > 127).collect()
    }

    fn iou(a: &[bool], b: &[bool]) -> f32 {
        let both = a.iter().zip(b).filter(|(x, y)| **x && **y).count();
        let any = a.iter().zip(b).filter(|(x, y)| **x || **y).count();
        both as f32 / any.max(1) as f32
    }

    #[test]
    fn layout_is_struct_of_arrays_with_a_face_per_vertex() {
        let red = Some(Color {
            r: 1.0,
            g: 0.0,
            b: 0.0,
        });
        let s = scene(vec![
            cuboid([0.0; 3], [1.0; 3], [None, red, None, None, None, None]),
            cuboid([2.0; 3], [3.0; 3], [None; 6]),
        ]);
        let l = Layout::new(&s);
        assert_eq!(l.positions.len(), 2 * 24 * 3);
        assert_eq!(l.normals.len(), l.positions.len());
        assert_eq!(l.faces.len(), 48);
        assert_eq!(l.split_vertices, 0);
        assert_eq!(l.face_base, [0, 6]);
        assert_eq!(l.materials.len(), 12);
        assert_eq!(l.materials[1].color, [1.0, 0.0, 0.0]);
        let d = render::DEFAULT_COLOR;
        assert_eq!(l.materials[7].color, [d.r, d.g, d.b]);
        assert_eq!(l.parts[1].base_vertex, 24);
        assert_eq!(l.parts[1].indices, 36..72);
        // Part-local indices: part 1's start from 0 again.
        assert_eq!(l.indices[36], 0);
        // Face 1's vertices say face 1.
        assert_eq!(&l.faces[4..8], &[1, 1, 1, 1]);
    }

    #[test]
    fn a_vertex_shared_by_two_faces_is_split() {
        // Two triangles of different faces sharing an edge (vertices 1, 2).
        let part = Part {
            name: None,
            color: None,
            mesh: Mesh {
                positions: vec![0., 0., 0., 1., 0., 0., 0., 1., 0., 1., 1., 0.],
                normals: vec![0., 0., 1., 0., 0., 1., 0., 0., 1., 0., 0., 1.],
                indices: vec![0, 1, 2, 1, 3, 2],
                face_ids: vec![0, 1],
            },
            faces: vec![Face::plain(FaceStatus::Ok), Face::plain(FaceStatus::Approx)],
            lines: Lines::default(),
        };
        let l = Layout::new(&scene(vec![part]));
        assert_eq!(l.split_vertices, 2);
        assert_eq!(l.faces.len(), 6);
        // Every triangle's corners carry that triangle's face.
        for (t, f) in [(0, 0), (1, 1)] {
            for &i in &l.indices[3 * t..3 * t + 3] {
                assert_eq!(l.faces[i as usize], f, "triangle {t}");
            }
        }
        assert_eq!(l.materials[1].status, FaceStatus::Approx as u32);
    }

    #[test]
    fn construction_curves_widen_the_fit_only_when_shown() {
        let mut p = cuboid([0.0; 3], [1.0; 3], [None; 6]);
        p.lines = Lines {
            positions: vec![0.0, 0.0, 0.0, 10.0, 0.0, 0.0],
            kinds: vec![LineKind::Construction],
        };
        let l = Layout::new(&scene(vec![p]));
        let [off, on] = l.fits.map(Option::unwrap);
        assert!((off.radius - 3f32.sqrt() / 2.0).abs() < 1e-5);
        assert!(on.radius > 4.0);
    }

    #[test]
    fn visibility_is_a_bitset() {
        let mut v = Visibility::all(33);
        assert_eq!(v.words(), &[u32::MAX, 1]);
        v.set(32, false);
        v.set(3, false);
        assert!(!v.get(3) && v.get(4) && !v.get(32) && !v.get(99));
        assert_eq!(v.words(), &[!(1 << 3), 0]);
        assert_eq!(Visibility::all(0).words(), &[0]);
        assert_eq!(Visibility::all(32).words(), &[u32::MAX]);
    }

    #[test]
    fn clip_matrix_centres_the_fit_and_honours_pan() {
        let fit = Fit {
            mid: [1.0, 2.0, 3.0],
            radius: 2.0,
        };
        let apply = |m: [[f32; 4]; 4], p: [f32; 3]| {
            [0, 1, 2].map(|r| m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r])
        };
        let cam = Camera::default();
        let c = apply(clip_matrix(&cam, &fit, 200, 100), fit.mid);
        assert!(c[0].abs() < 1e-5 && c[1].abs() < 1e-5, "{c:?}");
        assert!((c[2] - 0.5).abs() < 1e-5);
        // Nearer (toward the eye, view +z) is smaller depth, inside 0.25..0.75.
        let r = rotation(&cam);
        let toward = r[2];
        let near = apply(
            clip_matrix(&cam, &fit, 200, 100),
            [0, 1, 2].map(|k| fit.mid[k] + toward[k] * fit.radius),
        );
        assert!((near[2] - 0.25).abs() < 1e-5, "{near:?}");
        // Pan right by a tenth of the short edge (100 px) = 10 px of 200.
        let panned = Camera {
            pan: [0.1, 0.0],
            ..cam
        };
        let p = apply(clip_matrix(&panned, &fit, 200, 100), fit.mid);
        assert!((p[0] - 0.1).abs() < 1e-5, "{p:?}");
    }

    #[test]
    fn rotation_is_orthonormal() {
        let r = rotation(&Camera {
            azimuth_deg: 47.0,
            elevation_deg: -12.0,
            ..Camera::default()
        });
        for i in 0..3 {
            for j in 0..3 {
                let d: f32 = (0..3).map(|k| r[i][k] * r[j][k]).sum();
                assert!((d - f32::from(u8::from(i == j))).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn gpu_matches_the_software_silhouette() {
        let Some(g) = gpu() else { return };
        let s = scene(vec![
            cuboid([0.0; 3], [4.0, 2.0, 1.0], [None; 6]),
            cuboid([5.0, 0.0, 0.0], [6.0, 1.0, 3.0], [None; 6]),
        ]);
        let gs = g.upload(&s);
        for cam in [
            Camera::default(),
            Camera {
                azimuth_deg: 120.0,
                elevation_deg: -20.0,
                zoom: 1.7,
                pan: [0.1, -0.05],
            },
            Camera {
                azimuth_deg: 0.0,
                elevation_deg: 89.0,
                ..Camera::default()
            },
        ] {
            let (w, h) = (160, 120);
            let gpu = g.render(&gs, &view(cam), w, h);
            let cpu = render::render(
                &s,
                &render::Options {
                    width: w,
                    height: h,
                    show_construction: false,
                    supersample: 2,
                    camera: cam,
                    fit: render::Fit::Sphere,
                },
            )
            .unwrap();
            let score = iou(&mask(&gpu), &mask(&cpu));
            assert!(score > 0.95, "{cam:?}: silhouettes overlap only {score}");
        }
    }

    #[test]
    fn gpu_shades_like_the_software_renderer() {
        let Some(g) = gpu() else { return };
        let s = scene(vec![cuboid([0.0; 3], [1.0; 3], [None; 6])]);
        let gs = g.upload(&s);
        let cam = Camera::default();
        let gpu = g.render(&gs, &view(cam), 96, 96);
        let cpu = render::render(
            &s,
            &render::Options {
                width: 96,
                height: 96,
                show_construction: false,
                supersample: 1,
                camera: cam,
                fit: render::Fit::Sphere,
            },
        )
        .unwrap();
        // The middle of each visible face: flat faces, so smooth and flat
        // shading agree.
        for (x, y) in [(48, 30), (36, 58), (60, 58)] {
            let (a, b) = (gpu.pixel(x, y), cpu.pixel(x, y));
            for k in 0..4 {
                assert!(
                    a[k].abs_diff(b[k]) <= 3,
                    "({x}, {y}): GPU {a:?} vs software {b:?}"
                );
            }
        }
        assert_eq!(gpu.pixel(0, 0)[3], 0, "the background is the clear colour");
    }

    #[test]
    fn face_materials_come_from_the_storage_buffer() {
        let Some(g) = gpu() else { return };
        let red = Some(Color {
            r: 1.0,
            g: 0.0,
            b: 0.0,
        });
        // +z (face 5) is the top: seen from above, the whole box is red.
        let s = scene(vec![cuboid(
            [0.0; 3],
            [1.0; 3],
            [None, None, None, None, None, red],
        )]);
        let gs = g.upload(&s);
        let top = Camera {
            azimuth_deg: 0.0,
            elevation_deg: 89.0,
            ..Camera::default()
        };
        let p = g.render(&gs, &view(top), 64, 64).pixel(32, 32);
        assert!(p[0] > 150 && p[1] < 40 && p[2] < 40, "{p:?}");
    }

    #[test]
    fn hidden_parts_are_not_drawn() {
        let Some(g) = gpu() else { return };
        let s = scene(vec![
            cuboid([0.0; 3], [1.0; 3], [None; 6]),
            cuboid([3.0, 0.0, 0.0], [4.0, 1.0, 1.0], [None; 6]),
        ]);
        let mut gs = g.upload(&s);
        let front = Camera {
            azimuth_deg: 0.0,
            elevation_deg: 0.0,
            ..Camera::default()
        };
        let coverage = |img: &render::Image| mask(img).iter().filter(|m| **m).count();
        let both = coverage(&g.render(&gs, &view(front), 128, 64));
        let mut v = Visibility::all(2);
        v.set(1, false);
        gs.set_visibility(&g.queue, v);
        let one = g.render(&gs, &view(front), 128, 64);
        let left = coverage(&one);
        assert!(left > 0 && left * 3 < both * 2, "{left} of {both}");
        // The hidden part sits at the right of the fit: that side is empty.
        assert_eq!(one.pixel(110, 32)[3], 0);
    }

    #[test]
    fn sketches_draw_and_construction_waits_for_its_toggle() {
        let Some(g) = gpu() else { return };
        let part = |kind| Part {
            name: None,
            color: None,
            mesh: Mesh::default(),
            faces: vec![],
            lines: Lines {
                positions: vec![-1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                kinds: vec![kind],
            },
        };
        let front = Camera {
            azimuth_deg: 0.0,
            elevation_deg: 0.0,
            ..Camera::default()
        };
        let drawn = |img: render::Image| mask(&img).iter().filter(|m| **m).count();
        let sketch = g.upload(&scene(vec![part(LineKind::Sketch)]));
        assert!(drawn(g.render(&sketch, &view(front), 64, 64)) >= 40);
        let construction = g.upload(&scene(vec![part(LineKind::Construction)]));
        assert_eq!(drawn(g.render(&construction, &view(front), 64, 64)), 0);
        let shown = View {
            show_construction: true,
            ..view(front)
        };
        assert!(drawn(g.render(&construction, &shown, 64, 64)) >= 40);
    }

    #[test]
    fn approximated_faces_are_striped() {
        let Some(g) = gpu() else { return };
        let mut p = cuboid([0.0; 3], [1.0; 3], [None; 6]);
        for f in &mut p.faces {
            f.status = FaceStatus::Approx;
        }
        let gs = g.upload(&scene(vec![p]));
        let top = Camera {
            azimuth_deg: 0.0,
            elevation_deg: 89.0,
            ..Camera::default()
        };
        let img = g.render(&gs, &view(top), 64, 64);
        // Along a row through the middle: amber (red > blue) throughout, in
        // two alternating shades.
        let row: Vec<[u8; 4]> = (20..44).map(|x| img.pixel(x, 32)).collect();
        assert!(row.iter().all(|p| p[0] > p[2] + 40), "{row:?}");
        let (lo, hi) = row
            .iter()
            .fold((255, 0), |(lo, hi), p| (lo.min(p[0]), hi.max(p[0])));
        assert!(hi - lo > 40, "no stripes: red spans {lo}..{hi}");
    }

    #[test]
    fn no_adapter_is_selectable_from_none() {
        assert!(select(&[], None).is_err());
    }

    /// Where model point `p` lands in a `w` x `h` view of `gs` (pixels,
    /// top-left origin), from the same matrix the shaders use.
    fn pixel_of(gs: &GpuScene, cam: &Camera, w: u32, h: u32, p: [f32; 3]) -> (u32, u32) {
        let m = clip_matrix(cam, &gs.fits[0].unwrap(), w, h);
        let c = [0, 1].map(|r| m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r]);
        (
            ((c[0] + 1.0) / 2.0 * w as f32) as u32,
            ((1.0 - c[1]) / 2.0 * h as f32) as u32,
        )
    }

    fn two_boxes() -> Scene {
        scene(vec![
            cuboid([0.0; 3], [1.0; 3], [None; 6]),
            cuboid([3.0, 0.0, 0.0], [4.0, 1.0, 1.0], [None; 6]),
        ])
    }

    const FRONT: Camera = Camera {
        azimuth_deg: 0.0,
        elevation_deg: 0.0,
        zoom: 1.0,
        pan: [0.0, 0.0],
    };

    #[test]
    fn a_pick_names_the_part_and_face_under_the_pixel() {
        let Some(g) = gpu() else { return };
        let gs = g.upload(&two_boxes());
        let (w, h) = (160, 90);
        // From the front (looking along +y) the visible sides are y = 0:
        // face 2 of cuboid() (axis 1, low side).
        for (part, x) in [(0, 0.5), (1, 3.5)] {
            let (px, py) = pixel_of(&gs, &FRONT, w, h, [x, 0.0, 0.5]);
            let hit = g.pick(&gs, &View::new(FRONT), w, h, px, py);
            assert_eq!(hit, Some(Pick { part, face: 2 }), "at ({px}, {py})");
        }
        assert_eq!(
            g.pick(&gs, &View::new(FRONT), w, h, 2, 2),
            None,
            "background"
        );
    }

    #[test]
    fn hidden_parts_cannot_be_picked() {
        let Some(g) = gpu() else { return };
        let mut gs = g.upload(&two_boxes());
        let (w, h) = (160, 90);
        let (px, py) = pixel_of(&gs, &FRONT, w, h, [3.5, 0.0, 0.5]);
        let mut v = Visibility::all(2);
        v.set(1, false);
        gs.set_visibility(&g.queue, v);
        assert_eq!(g.pick(&gs, &View::new(FRONT), w, h, px, py), None);
    }

    #[test]
    fn the_section_plane_applies_to_picks() {
        let Some(g) = gpu() else { return };
        let gs = g.upload(&two_boxes());
        let (w, h) = (160, 90);
        let (px, py) = pixel_of(&gs, &FRONT, w, h, [0.5, 0.0, 0.5]);
        // Cut away y > 0.5: the front of box 0 goes, and the pick goes
        // through the cut to the inside of its back face (face 3, y = 1).
        let view = View {
            section: Some([0.0, 1.0, 0.0, 0.5]),
            ..View::new(FRONT)
        };
        // dot(p, (0,1,0)) > 0.5 is cut: the front face (y = 0) stays, so
        // flip the plane to cut y < 0.5 instead.
        let flipped = View {
            section: Some(
                Section {
                    axis: 1,
                    offset: 0.5,
                    flip: true,
                }
                .plane([0.0; 3], [1.0; 3]),
            ),
            ..View::new(FRONT)
        };
        assert_eq!(
            g.pick(&gs, &view, w, h, px, py),
            Some(Pick { part: 0, face: 2 })
        );
        assert_eq!(
            g.pick(&gs, &flipped, w, h, px, py),
            Some(Pick { part: 0, face: 3 })
        );
        // And the image agrees with the pick: through the cut, the (red)
        // back face shows.
        let red = Some(Color {
            r: 1.0,
            g: 0.0,
            b: 0.0,
        });
        let coloured = g.upload(&scene(vec![cuboid(
            [0.0; 3],
            [1.0; 3],
            [None, None, None, red, None, None],
        )]));
        let (px, py) = pixel_of(&coloured, &FRONT, w, h, [0.5, 0.0, 0.5]);
        let a = g.render(&coloured, &View::new(FRONT), w, h).pixel(px, py);
        let b = g.render(&coloured, &flipped, w, h).pixel(px, py);
        assert!(
            a[0] < 200 && b[0] > 150 && b[1] < 60,
            "front {a:?}, through the cut {b:?}"
        );
    }

    #[test]
    fn section_planes_span_the_box() {
        let s = Section {
            axis: 2,
            offset: 0.25,
            flip: false,
        };
        assert_eq!(s.plane([0.0; 3], [4.0, 4.0, 8.0]), [0.0, 0.0, 1.0, 2.0]);
        let f = Section { flip: true, ..s };
        // Kept: dot(p, (0,0,-1)) <= -2, i.e. z >= 2.
        assert_eq!(f.plane([0.0; 3], [4.0, 4.0, 8.0]), [0.0, 0.0, -1.0, -2.0]);
    }

    #[test]
    fn the_highlight_marks_only_the_picked_face() {
        let Some(g) = gpu() else { return };
        let gs = g.upload(&two_boxes());
        let (w, h) = (160, 90);
        let (ax, ay) = pixel_of(&gs, &FRONT, w, h, [0.5, 0.0, 0.5]);
        let (bx, by) = pixel_of(&gs, &FRONT, w, h, [3.5, 0.0, 0.5]);
        let plain = g.render(&gs, &View::new(FRONT), w, h);
        let lit = g.render(
            &gs,
            &View {
                picked: Some(Pick { part: 0, face: 2 }),
                highlight: [1.0, 0.0, 0.0],
                ..View::new(FRONT)
            },
            w,
            h,
        );
        let (p, q) = (plain.pixel(ax, ay), lit.pixel(ax, ay));
        assert!(
            q[0] > p[0] + 40 && q[2] + 40 < p[2],
            "not highlighted: {p:?} -> {q:?}"
        );
        assert_eq!(
            plain.pixel(bx, by),
            lit.pixel(bx, by),
            "the other box changed"
        );
    }

    #[test]
    fn an_empty_scene_renders_the_background() {
        let Some(g) = gpu() else { return };
        let gs = g.upload(&scene(vec![]));
        let img = g.render(
            &gs,
            &View {
                clear: [0.0, 0.0, 1.0, 1.0],
                ..view(Camera::default())
            },
            8,
            8,
        );
        assert_eq!(img.pixel(4, 4), [0, 0, 255, 255]);
    }
}
