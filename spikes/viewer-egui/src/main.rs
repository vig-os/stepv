//! Spike A (#27): the #21 viewer on egui + wgpu.
//!
//!   viewer-egui <file.step>           interactive
//!   SPIKE_BENCH=300 viewer-egui <f>   orbit for 300 frames, print timings, exit
//!
//! Loads the mesh + topology through the sandboxed kernel, draws it with a
//! wgpu pipeline inside an egui paint callback (per-face colour, a clip
//! plane, the picked face highlighted), and offers a model tree with
//! show/hide, a section plane, and a face inspector fed by the exact topology.
//! Picking is a CPU raycast for the spike (timed); the product would use a
//! GPU id buffer.

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use eframe::egui_wgpu::{self, wgpu};
use glam::{Mat4, Vec3};
use stepv::occt::{self, Limits};
use stepv::topology::{Node, Surface, Topology};
use stepv::{Deflection, Scene};

// ── Data ────────────────────────────────────────────────────────────────────

struct Model {
    scene: Scene,
    topo: Topology,
    /// Per part: model-space AABB, for culling picks.
    part_boxes: Vec<(Vec3, Vec3)>,
    lo: Vec3,
    hi: Vec3,
    load: Duration,
    triangles: usize,
}

fn load(input: &std::path::Path) -> Result<Model, String> {
    let t0 = Instant::now();
    let dir = std::env::temp_dir();
    let (mesh, topo) = (dir.join("spike-a.msh"), dir.join("spike-a.json"));
    let limits = Limits { timeout: Duration::from_secs(300), memory: None };
    let run = occt::run(&occt::kernel_path(), input, Deflection::PREVIEW, limits, Some(&mesh), Some(&topo))
        .map_err(|e| e.to_string())?;
    if run.outcome != occt::Outcome::Ok {
        return Err(format!("kernel: {:?} {:?}", run.outcome, run.summary.and_then(|s| s.error)));
    }
    let scene = occt::read_mesh(&std::fs::read(&mesh).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let topo = Topology::parse(&std::fs::read(&topo).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    topo.check_against(&scene).map_err(|e| e.to_string())?;
    let mut part_boxes = Vec::new();
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    let mut triangles = 0;
    for p in &scene.parts {
        let (mut a, mut b) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for v in p.mesh.positions.chunks_exact(3) {
            let v = Vec3::from_slice(v);
            a = a.min(v);
            b = b.max(v);
        }
        lo = lo.min(a);
        hi = hi.max(b);
        part_boxes.push((a, b));
        triangles += p.mesh.indices.len() / 3;
    }
    Ok(Model { scene, topo, part_boxes, lo, hi, load: t0.elapsed(), triangles })
}

// ── GPU ─────────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 3],
    nrm: [f32; 3],
    color: [f32; 3],
    part: u32,
    face: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    clip: [f32; 4],
    /// x: clip on, y: selected part (+1, 0 = none), z: selected face.
    flags: [u32; 4],
}

struct Gpu {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    uniforms: wgpu::Buffer,
    bind: wgpu::BindGroup,
    /// Per part: index range.
    ranges: Vec<std::ops::Range<u32>>,
}

const SHADER: &str = r#"
struct U { view_proj: mat4x4<f32>, eye: vec4<f32>, clip: vec4<f32>, flags: vec4<u32> };
@group(0) @binding(0) var<uniform> u: U;
struct V { @builtin(position) pos: vec4<f32>, @location(0) world: vec3<f32>, @location(1) nrm: vec3<f32>,
           @location(2) color: vec3<f32>, @location(3) @interpolate(flat) id: vec2<u32> };
@vertex fn vs(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) c: vec3<f32>,
              @location(3) part: u32, @location(4) face: u32) -> V {
  var o: V; o.pos = u.view_proj * vec4(p, 1.0); o.world = p; o.nrm = n; o.color = c; o.id = vec2(part, face); return o;
}
@fragment fn fs(i: V, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
  if (u.flags.x == 1u && dot(i.world, u.clip.xyz) > u.clip.w) { discard; }
  var n = normalize(i.nrm); if (!front) { n = -n; }
  let l = normalize(u.eye.xyz - i.world);
  var c = i.color * (0.35 + 0.65 * abs(dot(n, l)));
  if (u.flags.y == i.id.x + 1u && u.flags.z == i.id.y) { c = mix(c, vec3(1.0, 0.85, 0.1), 0.6); }
  return vec4(c, 1.0);
}
"#;

const SAMPLES: u32 = 4;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

impl Gpu {
    fn new(rs: &egui_wgpu::RenderState, m: &Model) -> Self {
        let d = &rs.device;
        let mut verts = Vec::new();
        let mut idx = Vec::new();
        let mut ranges = Vec::new();
        let grey = [0.62, 0.66, 0.72];
        for (pi, p) in m.scene.parts.iter().enumerate() {
            let base = verts.len() as u32;
            let n = p.mesh.positions.len() / 3;
            // Vertices are per face in STEPVMSH: the face of any triangle using
            // a vertex is that vertex's face.
            let mut face_of = vec![0u32; n];
            for (t, tri) in p.mesh.indices.chunks_exact(3).enumerate() {
                for &v in tri {
                    face_of[v as usize] = p.mesh.face_ids[t];
                }
            }
            for v in 0..n {
                let f = face_of[v] as usize;
                let c = p.faces.get(f).and_then(|f| f.color).or(p.color).map_or(grey, |c| [c.r, c.g, c.b]);
                verts.push(Vertex {
                    pos: p.mesh.positions[v * 3..v * 3 + 3].try_into().unwrap(),
                    nrm: p.mesh.normals[v * 3..v * 3 + 3].try_into().unwrap(),
                    color: c,
                    part: pi as u32,
                    face: f as u32,
                });
            }
            let start = idx.len() as u32;
            idx.extend(p.mesh.indices.iter().map(|i| i + base));
            ranges.push(start..idx.len() as u32);
        }
        use wgpu::util::DeviceExt;
        let vertices = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("verts"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("idx"),
            contents: bytemuck::cast_slice(&idx),
            usage: wgpu::BufferUsages::INDEX,
        });
        let uniforms = d.create_buffer(&wgpu::BufferDescriptor {
            label: Some("u"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }],
        });
        let module = d.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pl = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cad"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 3 => Uint32, 4 => Uint32],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(rs.target_format.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
            multiview_mask: None,
            cache: None,
        });
        Self { pipeline, vertices, indices, uniforms, bind, ranges }
    }
}

struct Draw {
    uniforms: Uniforms,
    visible: Arc<Vec<bool>>,
}

impl egui_wgpu::CallbackTrait for Draw {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _sd: &egui_wgpu::ScreenDescriptor,
        _enc: &mut wgpu::CommandEncoder,
        res: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let gpu: &Gpu = res.get().unwrap();
        queue.write_buffer(&gpu.uniforms, 0, bytemuck::bytes_of(&self.uniforms));
        Vec::new()
    }

    fn paint(&self, _info: egui::PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>, res: &egui_wgpu::CallbackResources) {
        let gpu: &Gpu = res.get().unwrap();
        pass.set_pipeline(&gpu.pipeline);
        pass.set_bind_group(0, &gpu.bind, &[]);
        pass.set_vertex_buffer(0, gpu.vertices.slice(..));
        pass.set_index_buffer(gpu.indices.slice(..), wgpu::IndexFormat::Uint32);
        for (r, &v) in gpu.ranges.iter().zip(self.visible.iter()) {
            if v && !r.is_empty() {
                pass.draw_indexed(r.clone(), 0, 0..1);
            }
        }
    }
}

// ── Camera + picking ────────────────────────────────────────────────────────

struct Orbit {
    az: f32,
    el: f32,
    dist: f32,
    target: Vec3,
}

impl Orbit {
    /// CAD Z-up; render.rs's angles: toward the eye is
    /// (-sin a cos e, -cos a cos e, sin e) in CAD coordinates.
    fn toward(&self) -> Vec3 {
        let (a, e) = (self.az.to_radians(), self.el.to_radians());
        Vec3::new(-a.sin() * e.cos(), -a.cos() * e.cos(), e.sin())
    }
    fn up(&self) -> Vec3 {
        let (a, e) = (self.az.to_radians(), self.el.to_radians());
        Vec3::new(a.sin() * e.sin(), a.cos() * e.sin(), e.cos())
    }
    fn eye(&self) -> Vec3 {
        self.target + self.toward() * self.dist
    }
    fn view_proj(&self, aspect: f32, radius: f32) -> Mat4 {
        // wgpu's NDC: depth 0..1, Y up (glam's "directx" convention).
        let proj = glam::camera::rh::proj::directx::perspective(30f32.to_radians(), aspect, self.dist * 0.01, self.dist + radius * 4.0);
        proj * glam::camera::rh::view::look_at_mat4(self.eye(), self.target, self.up())
    }
}

/// Möller–Trumbore over every visible part whose box the ray hits.
fn pick(m: &Model, visible: &[bool], o: Vec3, d: Vec3, clip: Option<(Vec3, f32)>) -> Option<(usize, usize, f32)> {
    let mut best: Option<(usize, usize, f32)> = None;
    for (pi, p) in m.scene.parts.iter().enumerate() {
        if !visible[pi] {
            continue;
        }
        let (lo, hi) = m.part_boxes[pi];
        let inv = d.recip();
        let (t1, t2) = ((lo - o) * inv, (hi - o) * inv);
        let (tmin, tmax) = (t1.min(t2).max_element(), t1.max(t2).min_element());
        if tmax < tmin.max(0.0) || best.is_some_and(|b| tmin > b.2) {
            continue;
        }
        let pos = &p.mesh.positions;
        let v = |i: u32| Vec3::from_slice(&pos[i as usize * 3..i as usize * 3 + 3]);
        for (t, tri) in p.mesh.indices.chunks_exact(3).enumerate() {
            let (a, b, c) = (v(tri[0]), v(tri[1]), v(tri[2]));
            let (e1, e2) = (b - a, c - a);
            let h = d.cross(e2);
            let det = e1.dot(h);
            if det.abs() < 1e-12 {
                continue;
            }
            let f = 1.0 / det;
            let s = o - a;
            let uu = f * s.dot(h);
            if !(0.0..=1.0).contains(&uu) {
                continue;
            }
            let q = s.cross(e1);
            let vv = f * d.dot(q);
            if vv < 0.0 || uu + vv > 1.0 {
                continue;
            }
            let dist = f * e2.dot(q);
            if dist <= 0.0 || best.is_some_and(|b| dist >= b.2) {
                continue;
            }
            if let Some((n, w)) = clip
                && (o + d * dist).dot(n) > w
            {
                continue;
            }
            best = Some((pi, p.mesh.face_ids[t] as usize, dist));
        }
    }
    best
}

// ── App ─────────────────────────────────────────────────────────────────────

struct App {
    m: Model,
    orbit: Orbit,
    radius: f32,
    visible: Vec<bool>,
    section: bool,
    axis: usize,
    offset: f32,
    picked: Option<(usize, usize)>,
    pick_ms: f64,
    frames: Vec<f64>,
    last: Instant,
    bench: Option<usize>,
    started: Instant,
    first_frame: Option<Duration>,
}

impl App {
    fn tree(&mut self, ui: &mut egui::Ui, n: &Node) {
        match n {
            Node::Part { name, part } => {
                ui.checkbox(&mut self.visible[*part], format!("{name} #{part}"));
            }
            Node::Assembly { name, children } => {
                egui::CollapsingHeader::new(name.as_str()).default_open(true).id_salt(name.as_ptr()).show(ui, |ui| {
                    for c in children {
                        self.tree(ui, c);
                    }
                });
            }
        }
    }

    fn clip(&self) -> Option<(Vec3, f32)> {
        self.section.then(|| {
            let mut n = Vec3::ZERO;
            n[self.axis] = 1.0;
            let w = self.m.lo[self.axis] + (self.m.hi[self.axis] - self.m.lo[self.axis]) * self.offset;
            (n, w)
        })
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        let now = Instant::now();
        self.frames.push((now - self.last).as_secs_f64() * 1e3);
        self.last = now;
        if self.frames.len() > 120 {
            self.frames.remove(0);
        }
        let avg = self.frames.iter().sum::<f64>() / self.frames.len() as f64;

        egui::Panel::left("tree").default_size(240.0).show(root, |ui| {
            ui.heading("Model");
            egui::ScrollArea::vertical().show(ui, |ui| {
                let tree = self.m.topo.tree.clone();
                for n in &tree {
                    self.tree(ui, n);
                }
            });
        });
        egui::Panel::right("inspect").default_size(260.0).show(root, |ui| {
            ui.heading("Section");
            ui.checkbox(&mut self.section, "clip");
            ui.horizontal(|ui| {
                for (i, a) in ["X", "Y", "Z"].iter().enumerate() {
                    ui.radio_value(&mut self.axis, i, *a);
                }
            });
            ui.add(egui::Slider::new(&mut self.offset, 0.0..=1.0).text("offset"));
            ui.separator();
            ui.heading("Selection");
            match self.picked {
                Some((p, f)) => {
                    let part = &self.m.topo.parts[p];
                    let proto = &self.m.topo.prototypes[part.prototype];
                    let face = &proto.faces[f];
                    ui.label(format!("part {p}: {}", part.name));
                    ui.label(format!("face {f}: {}", surface(&face.surface)));
                    ui.label(format!("face area {:.3} mm²", face.area));
                    ui.label(format!("part volume {}", proto.volume.map_or("—".into(), |v| format!("{v:.3} mm³"))));
                    ui.label(format!("pick {:.1} ms", self.pick_ms));
                }
                None => {
                    ui.label("click a face");
                }
            }
        });
        egui::Panel::bottom("status").show(root, |ui| {
            ui.label(format!(
                "{} parts · {} triangles · load {:.2} s · frame {avg:.2} ms",
                self.m.scene.parts.len(),
                self.m.triangles,
                self.m.load.as_secs_f64()
            ));
        });
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(root, |ui| {
            let (rect, resp) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
            if resp.dragged_by(egui::PointerButton::Primary) {
                let d = resp.drag_delta();
                self.orbit.az = (self.orbit.az - d.x * 0.4).rem_euclid(360.0);
                self.orbit.el = (self.orbit.el + d.y * 0.4).clamp(-90.0, 90.0);
            }
            if resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) {
                let d = resp.drag_delta();
                let s = self.orbit.dist * 0.0015;
                let right = self.orbit.up().cross(self.orbit.toward()).normalize();
                self.orbit.target += (-right * d.x + self.orbit.up() * d.y) * s;
            }
            if resp.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                self.orbit.dist *= 1.0015f32.powf(-scroll);
            }
            if let Some(b) = self.bench.as_mut() {
                self.orbit.az = (self.orbit.az + 1.0) % 360.0;
                *b -= 1;
            }
            let aspect = rect.width() / rect.height().max(1.0);
            let vp = self.orbit.view_proj(aspect, self.radius);
            if resp.clicked()
                && let Some(pos) = resp.interact_pointer_pos()
            {
                let ndc = egui::vec2(
                    (pos.x - rect.left()) / rect.width() * 2.0 - 1.0,
                    1.0 - (pos.y - rect.top()) / rect.height() * 2.0,
                );
                let inv = vp.inverse();
                let a = inv.project_point3(Vec3::new(ndc.x, ndc.y, 0.0));
                let b = inv.project_point3(Vec3::new(ndc.x, ndc.y, 1.0));
                let t0 = Instant::now();
                let hit = pick(&self.m, &self.visible, a, (b - a).normalize(), self.clip());
                self.pick_ms = t0.elapsed().as_secs_f64() * 1e3;
                self.picked = hit.map(|(p, f, _)| (p, f));
            }
            let (clip, on) = self.clip().map_or(([0.0; 4], 0), |(n, w)| ([n.x, n.y, n.z, w], 1));
            let sel = self.picked.map_or([0, 0], |(p, f)| [p as u32 + 1, f as u32]);
            let e = self.orbit.eye();
            ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                rect,
                Draw {
                    uniforms: Uniforms {
                        view_proj: vp.to_cols_array_2d(),
                        eye: [e.x, e.y, e.z, 0.0],
                        clip,
                        flags: [on, sel[0], sel[1], 0],
                    },
                    visible: Arc::new(self.visible.clone()),
                },
            ));
        });
        if self.first_frame.is_none() {
            self.first_frame = Some(self.started.elapsed());
        }
        if let Some(b) = self.bench {
            if b == 0 {
                let mut f = self.frames.clone();
                f.sort_by(f64::total_cmp);
                println!(
                    "bench: {} parts, {} triangles, load {:.2} s, first frame {:.2} s, frame p50 {:.2} ms p95 {:.2} ms",
                    self.m.scene.parts.len(),
                    self.m.triangles,
                    self.m.load.as_secs_f64(),
                    self.first_frame.unwrap().as_secs_f64(),
                    f[f.len() / 2],
                    f[f.len() * 95 / 100]
                );
                // Picks over a 9x9 grid of the view: mean and worst latency.
                let inv = self.orbit.view_proj(1.6, self.radius).inverse();
                let (mut times, mut hits) = (Vec::new(), 0);
                for i in 0..81 {
                    let (x, y) = ((i % 9) as f32 / 4.0 - 1.0, (i / 9) as f32 / 4.0 - 1.0);
                    let (a, z) = (inv.project_point3(Vec3::new(x * 0.9, y * 0.9, 0.0)), inv.project_point3(Vec3::new(x * 0.9, y * 0.9, 1.0)));
                    let t0 = Instant::now();
                    hits += usize::from(pick(&self.m, &self.visible, a, (z - a).normalize(), None).is_some());
                    times.push(t0.elapsed().as_secs_f64() * 1e3);
                }
                let mean = times.iter().sum::<f64>() / times.len() as f64;
                let max = times.iter().copied().fold(0.0, f64::max);
                println!("bench: pick over 81 points ({hits} hits): mean {mean:.1} ms, max {max:.1} ms");
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        ctx.request_repaint();
    }
}

fn surface(s: &Surface) -> String {
    match s {
        Surface::Plane { normal, .. } => format!("plane, normal [{:.3}, {:.3}, {:.3}]", normal[0], normal[1], normal[2]),
        Surface::Cylinder { radius, .. } => format!("cylinder, r = {radius:.4} (⌀ {:.4})", radius * 2.0),
        Surface::Cone { radius, semi_angle_deg, .. } => format!("cone, r = {radius:.4}, half-angle {semi_angle_deg:.2}°"),
        Surface::Sphere { radius, .. } => format!("sphere, r = {radius:.4}"),
        Surface::Torus { major_radius, minor_radius, .. } => format!("torus, R = {major_radius:.4}, r = {minor_radius:.4}"),
        other => format!("{other:?}").to_lowercase(),
    }
}

fn main() -> eframe::Result {
    let started = Instant::now();
    let input = std::env::args().nth(1).expect("usage: viewer-egui <file>");
    let m = load(std::path::Path::new(&input)).unwrap_or_else(|e| panic!("{e}"));
    let cam = stepv::render::Camera::for_scene(&m.scene);
    let centre = (m.lo + m.hi) * 0.5;
    let radius = ((m.hi - m.lo).length() * 0.5).max(1e-3);
    let bench = std::env::var("SPIKE_BENCH").ok().and_then(|v| v.parse().ok());
    let parts = m.scene.parts.len();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        depth_buffer: 32,
        multisampling: SAMPLES as u16,
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]).with_title(format!("stepv spike A — {input}")),
        wgpu_options: egui_wgpu::WgpuConfiguration {
            surface: egui_wgpu::SurfaceConfig {
                present_mode: if bench.is_some() { wgpu::PresentMode::AutoNoVsync } else { wgpu::PresentMode::AutoVsync },
                // A 3D viewport is "a lot of extra GPU work".
                ..egui_wgpu::SurfaceConfig::HIGH_THROUGHPUT
            },
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native(
        "stepv-spike-a",
        options,
        Box::new(move |cc| {
            let rs = cc.wgpu_render_state.as_ref().expect("wgpu");
            let gpu = Gpu::new(rs, &m);
            rs.renderer.write().callback_resources.insert(gpu);
            Ok(Box::new(App {
                orbit: Orbit { az: cam.azimuth_deg, el: cam.elevation_deg, dist: radius * 3.5, target: centre },
                radius,
                visible: vec![true; parts],
                m,
                section: false,
                axis: 0,
                offset: 0.5,
                picked: None,
                pick_ms: 0.0,
                frames: Vec::new(),
                last: Instant::now(),
                bench,
                started,
                first_frame: None,
            }))
        }),
    )
}
