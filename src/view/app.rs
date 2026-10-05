//! The GPU viewer's window: eframe around [`super::gpu`]'s offscreen render.
//!
//! The layout is a toolbar (view presets, display toggles, the backend and
//! sandbox pills, the theme switch), a properties panel on the right, a
//! status bar, and the viewport. A click picks a face through the GPU's id
//! pass (#29); the Selection panel names it from `--topology`, and the
//! Section panel cuts the model. #30 adds the model tree.
//!
//! Repaint discipline: eframe repaints on input only, and the viewport is
//! re-rendered only when what it shows changed ([`RenderKey`]): an idle
//! viewer draws nothing.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Key, PointerButton, Sense, ViewportCommand};
use eframe::egui_wgpu::{self, wgpu};
use egui_phosphor::regular as icon;

use super::controls::{Controls, Input};
use super::gpu::{GpuScene, IdTarget, Layout, PendingPick, Pick, Renderer, Section, Target, View};
use super::tree::Tree;
use super::widgets::{self, Tone};
use super::{Backend, Options, ThemePref, theme};
use crate::render::Camera;
use crate::topology::Topology;
use crate::{FaceStatus, Scene};

/// What a viewport frame depends on. Equal keys draw equal pixels, so an
/// equal key skips the render.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderKey {
    pub camera: Camera,
    pub show_construction: bool,
    /// The viewport in physical pixels, at most the device's texture size.
    pub size: [u32; 2],
    pub dark: bool,
    /// The approximated-face stripe width in physical pixels.
    pub stripe: u32,
    /// The section plane, when on.
    pub section: Option<[f32; 4]>,
    pub picked: Option<Pick>,
    /// The model tree's visibility generation.
    pub visibility: u64,
}

/// Whether a frame with `key` must be rendered: only when it differs from
/// the last one rendered, which it then becomes.
pub fn needs_render(last: &mut Option<RenderKey>, key: RenderKey) -> bool {
    if *last == Some(key) {
        return false;
    }
    *last = Some(key);
    true
}

/// What the properties panel shows about the model, kept after the scene's
/// CPU copy is dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub file: String,
    pub parts: usize,
    pub triangles: usize,
    pub faces: usize,
    pub approx: usize,
    pub missing: usize,
}

impl Summary {
    #[must_use]
    pub fn of(scene: &Scene, file: &str) -> Self {
        let count = |s| {
            scene
                .parts
                .iter()
                .flat_map(|p| &p.faces)
                .filter(|f| f.status == s)
                .count()
        };
        Self {
            file: file.to_owned(),
            parts: scene.parts.len(),
            triangles: scene.triangle_count(),
            faces: scene.parts.iter().map(|p| p.faces.len()).sum(),
            approx: count(FaceStatus::Approx),
            missing: count(FaceStatus::Missing),
        }
    }
}

/// The physical-pixel size of a viewport `points` big at `ppp`.
#[must_use]
pub fn physical(points: egui::Vec2, ppp: f32) -> [u32; 2] {
    [
        (points.x * ppp).round().max(1.0) as u32,
        (points.y * ppp).round().max(1.0) as u32,
    ]
}

/// Scroll (points) and pinch (factor) as [`Input::Scroll`] notches.
#[must_use]
pub fn zoom_notches(scroll_y: f32, pinch: f32) -> f32 {
    // One wheel notch is ~50 points of scroll on every platform egui
    // supports; a pinch of 1.12x is one notch (Controls::ZOOM).
    scroll_y / 50.0 + pinch.max(1e-3).ln() / 1.12f32.ln()
}

struct Viewer {
    renderer: Renderer,
    scene: GpuScene,
    target: Option<Target>,
    texture: Option<egui::TextureId>,
    last: Option<RenderKey>,
    controls: Controls,
    summary: Summary,
    backend: Backend,
    adapter: String,
    sandbox: Option<String>,
    sandboxed: bool,
    theme: ThemePref,
    screenshot: Option<PathBuf>,
    /// Frames painted, for the screenshot hook.
    frames: u32,
    /// When the window screenshot was requested.
    requested: Option<std::time::Instant>,
    /// Set once the screenshot is saved (or failed): close next frame.
    error: Arc<Mutex<Option<String>>>,
    /// The exact B-rep (#29's inspector), when the kernel wrote it.
    topology: Option<Topology>,
    picked: Option<Pick>,
    /// A click being resolved on the GPU.
    pending: Option<PendingPick>,
    /// The id pass's target, kept between clicks at the viewport's size.
    ids: Option<IdTarget>,
    cut: bool,
    section: Section,
    /// `STEPV_VIEW_PICK`: a click to make once the first frame is drawn.
    pick_at: Option<(f32, f32)>,
    /// That click is in flight: report what it hits on stderr.
    report_pick: bool,
    /// The model tree (#30).
    tree: Tree,
    /// The tree generation the GPU's visibility matches.
    uploaded: u64,
    /// Scroll the tree to the selection next frame (a pick in the view).
    reveal: bool,
}

impl Viewer {
    fn new(
        cc: &eframe::CreationContext<'_>,
        scene: &Scene,
        topology: Option<Topology>,
        opts: &Options,
        error: Arc<Mutex<Option<String>>>,
    ) -> Result<Self, String> {
        let rs = cc
            .wgpu_render_state
            .as_ref()
            .ok_or("eframe started without wgpu")?;
        // eframe's adapter went through gpu::select already; this guards the
        // contract if a future eframe ignores the selector.
        if !super::gpu::usable(&rs.adapter) {
            return Err(format!(
                "adapter {} cannot run the viewer",
                rs.adapter.get_info().name
            ));
        }
        theme::install(&cc.egui_ctx, opts.theme);
        let tree = match &topology {
            Some(t) => Tree::from_topology(t, scene.parts.len()),
            None => Tree::flat(
                &scene
                    .parts
                    .iter()
                    .map(|p| p.name.clone())
                    .collect::<Vec<_>>(),
            ),
        };
        let info = rs.adapter.get_info();
        // A validation error here (a driver that claims more than it does)
        // becomes an Err, and the software fallback, not a panic.
        let scope = rs.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let renderer = Renderer::new(&rs.device);
        let gpu_scene = GpuScene::upload(&rs.device, &renderer, Layout::new(scene));
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(format!("the GPU refused the viewer's pipelines: {e}"));
        }
        Ok(Self {
            renderer,
            scene: gpu_scene,
            target: None,
            texture: None,
            last: None,
            controls: Controls::for_scene(scene),
            summary: Summary::of(scene, &opts.file),
            backend: Backend::from_wgpu(info.backend),
            adapter: info.name,
            sandbox: opts.sandbox.clone(),
            sandboxed: opts.sandboxed,
            theme: opts.theme,
            screenshot: opts.screenshot.clone(),
            frames: 0,
            requested: None,
            error,
            topology,
            picked: None,
            pending: None,
            ids: None,
            pick_at: opts.pick_at,
            report_pick: false,
            tree,
            uploaded: 0,
            reveal: false,
            cut: false,
            section: Section {
                axis: 0,
                offset: 0.5,
                flip: false,
            },
        })
    }

    fn apply(&mut self, input: Input, short_edge: f32) {
        self.controls.apply(input, short_edge);
    }

    fn keys(&mut self, ctx: &egui::Context, short: f32) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let pressed = |k| ctx.input(|i| i.key_pressed(k));
        for (key, input) in [
            (Key::R, Input::Reset),
            (Key::F, Input::Front),
            (Key::T, Input::Top),
            (Key::C, Input::ToggleConstruction),
        ] {
            if pressed(key) {
                self.apply(input, short);
            }
        }
        // Esc clears a selection (or one being picked) first, then quits.
        if pressed(Key::Escape) && (self.picked.is_some() || self.pending.is_some()) {
            self.picked = None;
            self.pending = None;
        } else if pressed(Key::Q) || pressed(Key::Escape) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, short: f32) {
        ui.horizontal_centered(|ui| {
            let presets = [
                (icon::HOUSE, None, "Reset view (R)", Input::Reset),
                (icon::SQUARE, Some("Front"), "Front view (F)", Input::Front),
                (
                    icon::ARROW_FAT_LINE_DOWN,
                    Some("Top"),
                    "Top view (T)",
                    Input::Top,
                ),
            ];
            for (glyph, label, tip, input) in presets {
                if widgets::icon_button(ui, glyph, label, tip, false).clicked() {
                    self.apply(input, short);
                }
            }
            ui.separator();
            let on = self.controls.show_construction;
            if widgets::icon_button(ui, icon::RULER, None, "Construction curves (C)", on).clicked()
            {
                self.apply(Input::ToggleConstruction, short);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let dark = ui.visuals().dark_mode;
                let (glyph, tip) = if dark {
                    (icon::SUN, "Light theme")
                } else {
                    (icon::MOON, "Dark theme")
                };
                if widgets::icon_button(ui, glyph, None, tip, false).clicked() {
                    self.theme = if dark {
                        ThemePref::Light
                    } else {
                        ThemePref::Dark
                    };
                    theme::set_theme(ui.ctx(), self.theme);
                }
                ui.add_space(theme::space(1));
                let (text, tone, tip) = match (&self.sandbox, self.sandboxed) {
                    (Some(s), true) => (
                        format!("sandbox: {s}"),
                        Tone::Success,
                        "The kernel ran sandboxed",
                    ),
                    (Some(s), false) => (
                        format!("sandbox: {s}"),
                        Tone::Warning,
                        "The kernel ran without its full sandbox",
                    ),
                    (None, _) => (
                        "no sandbox".to_owned(),
                        Tone::Danger,
                        "The kernel reported no sandbox",
                    ),
                };
                widgets::status_pill(ui, &text, tone, tip);
                let tone = if self.backend == Backend::Gl {
                    Tone::Warning
                } else {
                    Tone::Neutral
                };
                widgets::status_pill(ui, self.backend.name(), tone, &self.adapter);
            });
        });
    }

    fn properties(&mut self, ui: &mut egui::Ui) {
        let s = &self.summary;
        widgets::section_header(ui, "Model");
        widgets::property_row(ui, "File", &s.file);
        widgets::property_row(ui, "Parts", &s.parts.to_string());
        widgets::property_row(ui, "Faces", &s.faces.to_string());
        widgets::property_row(ui, "Triangles", &s.triangles.to_string());
        if s.approx + s.missing > 0 {
            let t = widgets::tokens(ui);
            ui.label(
                egui::RichText::new(format!(
                    "{} approximated faces: {}, missing: {}",
                    icon::WARNING,
                    s.approx,
                    s.missing
                ))
                .color(t.warning),
            );
        }
        ui.separator();
        self.selection(ui);
        ui.separator();
        self.section_controls(ui);
        ui.separator();
        widgets::section_header(ui, "View");
        let c = &self.controls.camera;
        widgets::property_row(ui, "Azimuth", &format!("{:.0}°", c.azimuth_deg));
        widgets::property_row(ui, "Elevation", &format!("{:.0}°", c.elevation_deg));
        widgets::property_row(ui, "Zoom", &format!("{:.2}×", c.zoom));
        ui.separator();
        widgets::section_header(ui, "Display");
        widgets::property_row(ui, "Backend", self.backend.name());
        widgets::property_row(ui, "Adapter", &self.adapter);
    }

    /// The inspector: what the picked face is, from the exact B-rep.
    fn selection(&self, ui: &mut egui::Ui) {
        widgets::section_header(ui, "Selection");
        let t = widgets::tokens(ui);
        let Some(pick) = self.picked else {
            ui.label(egui::RichText::new("Click a face to inspect it").color(t.muted_foreground));
            return;
        };
        let (part, face) = (pick.part as usize, pick.face as usize);
        if pick.is_whole_part() {
            // Picked in the tree: the part, not one face of it.
            let name = self
                .topology
                .as_ref()
                .and_then(|t| t.parts.get(part).map(|p| p.name.clone()))
                .or_else(|| {
                    self.tree
                        .row_of(pick.part)
                        .map(|r| self.tree.rows()[r as usize].name.clone())
                })
                .unwrap_or_default();
            widgets::property_row(ui, "Part", &format!("{name} (#{part})"));
            if let Some(topo) = &self.topology {
                for r in super::inspect::part(topo, part) {
                    widgets::property_row(ui, r.key, &r.value);
                }
            }
            return;
        }
        match self.topology.as_ref().and_then(|topo| {
            super::inspect::face(topo, part, face)
                .map(|rows| (rows, super::inspect::part(topo, part)))
        }) {
            Some((face_rows, part_rows)) => {
                for r in face_rows.iter().chain(&part_rows) {
                    widgets::property_row(ui, r.key, &r.value);
                }
            }
            None => {
                // No topology (a kernel without --topology, or a file whose
                // topology failed): the mesh's numbering is all there is.
                widgets::property_row(ui, "Part", &format!("#{part}"));
                widgets::property_row(ui, "Face", &format!("#{face}"));
                ui.label(
                    egui::RichText::new("No exact topology for this file")
                        .color(t.muted_foreground),
                );
            }
        }
    }

    /// The section plane: on/off, axis, position, side.
    fn section_controls(&mut self, ui: &mut egui::Ui) {
        widgets::section_header(ui, "Section");
        ui.checkbox(&mut self.cut, "Cut the model");
        ui.add_enabled_ui(self.cut, |ui| {
            ui.horizontal(|ui| {
                for (k, name) in ["X", "Y", "Z"].iter().enumerate() {
                    ui.radio_value(&mut self.section.axis, k, *name);
                }
                ui.checkbox(&mut self.section.flip, "Flip");
            });
            ui.add(egui::Slider::new(&mut self.section.offset, 0.0..=1.0).show_value(false));
        });
    }

    /// The section plane, when it is on.
    fn plane(&self) -> Option<[f32; 4]> {
        let (lo, hi) = self.scene.bounds?;
        self.cut.then(|| self.section.plane(lo, hi))
    }

    /// What `key` draws.
    fn view(&self, key: &RenderKey, dark: bool) -> View {
        let t = theme::tokens(dark);
        let a = egui::Rgba::from(t.accent);
        View {
            camera: key.camera,
            show_construction: key.show_construction,
            clear: t.viewport.to_normalized_gamma_f32().map(f64::from),
            stripe: key.stripe,
            section: key.section,
            picked: key.picked,
            highlight: [a.r(), a.g(), a.b()],
        }
    }

    /// Starts resolving a click at `at` (points) in the viewport `rect`.
    fn start_pick(&mut self, rs: &egui_wgpu::RenderState, rect: egui::Rect, at: egui::Pos2) {
        let (Some(key), Some(target)) = (self.last, self.target.as_ref()) else {
            return;
        };
        let (w, h) = (target.width, target.height);
        if self
            .ids
            .as_ref()
            .is_none_or(|t| (t.width, t.height) != (w, h))
        {
            self.ids = Some(IdTarget::new(&rs.device, w, h));
        }
        let (x, y) = super::gpu::texel_at(
            [rect.min.x, rect.min.y],
            [rect.width(), rect.height()],
            [at.x, at.y],
            w,
            h,
        );
        let view = self.view(&key, key.dark);
        let ids = self.ids.as_ref().expect("created above");
        self.pending =
            Some(
                self.renderer
                    .pick(&rs.device, &rs.queue, &self.scene, ids, &view, (x, y)),
            );
    }

    /// Uploads the tree's visibility when it changed, and drops a selection
    /// whose part was hidden (#29's review).
    fn sync_visibility(&mut self, rs: &egui_wgpu::RenderState) {
        if self.tree.generation == self.uploaded {
            return;
        }
        self.uploaded = self.tree.generation;
        let shown = self.tree.shown_parts();
        let mut v = super::gpu::Visibility::all(shown.len());
        for (i, &on) in shown.iter().enumerate() {
            v.set(i, on);
        }
        self.scene.set_visibility(&rs.queue, v);
        if self
            .picked
            .is_some_and(|p| !shown.get(p.part as usize).copied().unwrap_or(false))
        {
            self.picked = None;
        }
    }

    /// The model tree panel.
    fn model_tree(&mut self, ui: &mut egui::Ui) {
        widgets::section_header(ui, "Model");
        let selected = self.picked.map(|p| p.part);
        let r = self
            .tree
            .panel(ui, selected, std::mem::take(&mut self.reveal));
        if let Some(part) = r.selected {
            self.picked = Some(Pick::part(part));
            self.pending = None;
        }
    }

    /// Collects a finished pick; asks for another frame while one is out.
    fn finish_pick(&mut self, ctx: &egui::Context, rs: &egui_wgpu::RenderState) {
        let Some(p) = &self.pending else { return };
        match p.poll(&rs.device) {
            Some(hit) => {
                // A part hidden while the pick was in flight is not picked:
                // the id pass saw the old visibility (#30 review).
                let shown = self.tree.shown_parts();
                let hit = hit.filter(|p| shown.get(p.part as usize).copied().unwrap_or(false));
                self.picked = hit;
                self.pending = None;
                self.reveal = hit.is_some();
                // The tree drew this frame before the pick landed: one more
                // frame reveals it and draws the highlight, without waiting
                // for the pointer to move.
                ctx.request_repaint();
                if std::mem::take(&mut self.report_pick) {
                    let what = hit.map_or("nothing".into(), |p| {
                        let surface = self.topology.as_ref().and_then(|t| {
                            let proto =
                                t.prototypes.get(t.parts.get(p.part as usize)?.prototype)?;
                            let f = proto.faces.get(p.face as usize)?;
                            Some(super::inspect::surface_name(&f.surface))
                        });
                        format!(
                            "part {} face {} ({})",
                            p.part,
                            p.face,
                            surface.unwrap_or("no topology")
                        )
                    });
                    eprintln!("stepv: STEPV_VIEW_PICK hit {what}");
                }
            }
            None => ctx.request_repaint(),
        }
    }

    /// Re-renders the viewport if `key` differs from the last frame's.
    fn render(&mut self, rs: &egui_wgpu::RenderState, key: RenderKey) {
        if !needs_render(&mut self.last, key) {
            return;
        }
        let [w, h] = key.size;
        if self
            .target
            .as_ref()
            .is_none_or(|t| (t.width, t.height) != (w, h))
        {
            let target = Target::new(&rs.device, w, h);
            let mut r = rs.renderer.write();
            match self.texture {
                Some(id) => r.update_egui_texture_from_wgpu_texture(
                    &rs.device,
                    &target.view,
                    wgpu::FilterMode::Linear,
                    id,
                ),
                None => {
                    self.texture = Some(r.register_native_texture(
                        &rs.device,
                        &target.view,
                        wgpu::FilterMode::Linear,
                    ));
                }
            }
            self.target = Some(target);
        }
        let view = self.view(&key, key.dark);
        let target = self.target.as_ref().expect("created above");
        let frame = self
            .renderer
            .render(&rs.device, &rs.queue, &self.scene, target, &view);
        rs.queue.submit([frame]);
    }

    fn viewport(&mut self, ui: &mut egui::Ui, rs: &egui_wgpu::RenderState) {
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let short = rect.width().min(rect.height());
        let shift = ui.input(|i| i.modifiers.shift);
        let d = resp.drag_delta();
        if d != egui::Vec2::ZERO {
            let pan = resp.dragged_by(PointerButton::Secondary)
                || resp.dragged_by(PointerButton::Middle)
                || (shift && resp.dragged_by(PointerButton::Primary));
            self.apply(
                if pan {
                    Input::Pan(d.x, d.y)
                } else {
                    Input::Orbit(d.x, d.y)
                },
                short,
            );
        }
        if resp.hovered() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let n = zoom_notches(scroll, pinch);
            if n != 0.0 {
                self.apply(Input::Scroll(n), short);
            }
        }
        let dark = ui.visuals().dark_mode;
        let ppp = ui.ctx().pixels_per_point();
        // Past the texture limit (a window across two 5K displays), render
        // at the limit and let egui scale it up rather than fail validation.
        let max = rs.device.limits().max_texture_dimension_2d;
        let key = RenderKey {
            camera: self.controls.camera,
            show_construction: self.controls.show_construction,
            size: physical(rect.size(), ppp).map(|v| v.min(max)),
            dark,
            stripe: (6.0 * ppp).round().max(1.0) as u32,
            section: self.plane(),
            picked: self.picked,
            visibility: self.tree.generation,
        };
        self.render(rs, key);
        // A click (not the end of a drag) picks what is under it, in the
        // frame just rendered; it lands a frame or two later.
        if resp.clicked()
            && let Some(at) = resp.interact_pointer_pos()
        {
            self.start_pick(rs, rect, at);
        }
        if let Some((x, y)) = self.pick_at.take() {
            self.report_pick = true;
            self.start_pick(rs, rect, rect.min + egui::vec2(x, y) * rect.size());
            ui.ctx().request_repaint();
        }
        self.finish_pick(ui.ctx(), rs);
        if let Some(id) = self.texture {
            ui.painter().image(
                id,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }

    /// The `STEPV_VIEW_SCREENSHOT` hook: once the window has settled, grab
    /// it, save it, and close.
    ///
    /// eframe captures a window only when it presents a frame, and an
    /// unpresented window (asleep display, a CI runner with no visible
    /// screen) never does. After [`SCREENSHOT_WAIT`] the hook saves the
    /// viewport's own render instead, saying so on stderr: that still proves
    /// the window's device drew the model, but not the panels around it.
    fn screenshot(&mut self, ctx: &egui::Context, rs: &egui_wgpu::RenderState) {
        let Some(path) = self.screenshot.clone() else {
            return;
        };
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let result = if let Some(image) = image {
            Some(save(&image, &path))
        } else if self
            .requested
            .is_some_and(|t| t.elapsed() > SCREENSHOT_WAIT)
        {
            Some(match &self.target {
                Some(target) => {
                    eprintln!(
                        "stepv: the window was never presented; saved the viewport's render instead"
                    );
                    let img = super::gpu::read_back(&rs.device, &rs.queue, target);
                    img.to_png().map_err(|e| e.to_string()).and_then(|png| {
                        std::fs::write(&path, png).map_err(|e| format!("{}: {e}", path.display()))
                    })
                }
                None => Err("no screenshot: the viewport never rendered".into()),
            })
        } else {
            None
        };
        if let Some(result) = result {
            if let Err(e) = result {
                *self.error.lock().unwrap() = Some(e);
            }
            self.screenshot = None;
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return;
        }
        // A few frames first: fonts, the theme and the first render land,
        // and a STEPV_VIEW_PICK click with its highlight.
        let settled = self.pick_at.is_none()
            && self.pending.is_none()
            && self.last.is_some_and(|k| k.picked == self.picked);
        if self.requested.is_none() && self.frames >= 3 && settled {
            ctx.send_viewport_cmd(ViewportCommand::Screenshot(Default::default()));
            self.requested = Some(std::time::Instant::now());
        }
        ctx.request_repaint();
    }
}

/// How long the screenshot hook waits for the window's own capture.
const SCREENSHOT_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

fn save(image: &egui::ColorImage, path: &Path) -> Result<(), String> {
    let img = crate::render::Image {
        width: image.size[0] as u32,
        height: image.size[1] as u32,
        rgba: image
            .pixels
            .iter()
            .flat_map(|c| c.to_srgba_unmultiplied())
            .collect(),
    };
    let png = img.to_png().map_err(|e| e.to_string())?;
    std::fs::write(path, png).map_err(|e| format!("{}: {e}", path.display()))
}

impl eframe::App for Viewer {
    fn ui(&mut self, root: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        let Some(rs) = frame.wgpu_render_state().cloned() else {
            return;
        };
        let short = ctx.content_rect().size().min_elem();
        self.keys(&ctx, short);
        let t = theme::tokens(root.visuals().dark_mode);
        let bar = |fill| {
            egui::Frame::NONE
                .fill(fill)
                .inner_margin(egui::Margin::symmetric(
                    theme::space(2) as i8,
                    theme::space(1) as i8,
                ))
                .stroke(egui::Stroke::new(1.0, t.border))
        };
        egui::Panel::top("toolbar")
            .frame(bar(t.panel))
            .exact_size(theme::space(10))
            .show(root, |ui| self.toolbar(ui, short));
        egui::Panel::bottom("status").frame(bar(t.panel)).show(root, |ui| {
            ui.label(
                egui::RichText::new(
                    "Drag to orbit · right- or shift-drag to pan · scroll to zoom · click to inspect · Esc clears · R reset · F front · T top · C construction",
                )
                .size(theme::size::SMALL)
                .color(t.muted_foreground),
            );
        });
        egui::Panel::left("tree")
            .frame(
                egui::Frame::NONE
                    .fill(t.panel)
                    .inner_margin(egui::Margin::same(theme::space(3) as i8))
                    .stroke(egui::Stroke::new(1.0, t.border)),
            )
            .default_size(260.0)
            .min_size(180.0)
            .show(root, |ui| self.model_tree(ui));
        self.sync_visibility(&rs);
        egui::Panel::right("properties")
            .frame(
                egui::Frame::NONE
                    .fill(t.panel)
                    .inner_margin(egui::Margin::same(theme::space(3) as i8))
                    .stroke(egui::Stroke::new(1.0, t.border)),
            )
            .default_size(260.0)
            .min_size(200.0)
            .show(root, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.properties(ui));
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(t.viewport))
            .show(root, |ui| self.viewport(ui, &rs));
        self.frames = self.frames.saturating_add(1);
        self.screenshot(&ctx, &rs);
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        theme::tokens(visuals.dark_mode)
            .panel
            .to_normalized_gamma_f32()
    }
}

/// Runs the GPU viewer. On failure before the window opened, hands the scene
/// back so the caller can fall back to the software viewer.
pub(super) fn run(
    scene: Scene,
    topology: Option<Topology>,
    title: &str,
    opts: &Options,
) -> Result<Backend, (String, Option<Scene>)> {
    let slot = Arc::new(Mutex::new(Some(scene)));
    let used = Arc::new(Mutex::new(None::<Backend>));
    let error = Arc::new(Mutex::new(None::<String>));
    let native = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title(title)
            .with_app_id("stepv")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([480.0, 320.0]),
        wgpu_options: egui_wgpu::WgpuConfiguration {
            wgpu_setup: egui_wgpu::WgpuSetup::CreateNew(egui_wgpu::WgpuSetupCreateNew {
                // Storage buffers in fragment shaders: what gpu.rs needs, not
                // eframe's WebGL2 limits for GL.
                // Only an adapter the viewer can use, and that presents to
                // the window: otherwise the window fails before it opens and
                // view::run falls back to software.
                native_adapter_selector: Some(Arc::new(|adapters, surface| {
                    if std::env::var_os("STEPV_VIEW_REJECT_ADAPTERS").is_some() {
                        return Err("STEPV_VIEW_REJECT_ADAPTERS is set".into());
                    }
                    super::gpu::select(adapters, surface)
                })),
                device_descriptor: Arc::new(|adapter| wgpu::DeviceDescriptor {
                    label: Some("stepv"),
                    required_limits: super::gpu::limits(adapter),
                    ..Default::default()
                }),
                ..egui_wgpu::WgpuSetupCreateNew::without_display_handle()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let (slot2, used2, error2, opts2) = (slot.clone(), used.clone(), error.clone(), opts.clone());
    let result = eframe::run_native(
        "stepv",
        native,
        Box::new(move |cc| {
            let scene = slot2.lock().unwrap().take().ok_or("no scene")?;
            match Viewer::new(cc, &scene, topology, &opts2, error2) {
                // The GPU has it; the CPU copy goes now (#28: the prototype
                // held both, 2.1 GB on the stress assembly).
                Ok(v) => {
                    *used2.lock().unwrap() = Some(v.backend);
                    drop(scene);
                    Ok(Box::new(v) as Box<dyn eframe::App>)
                }
                Err(e) => {
                    *slot2.lock().unwrap() = Some(scene);
                    Err(e.into())
                }
            }
        }),
    );
    let backend = *used.lock().unwrap();
    if let Some(e) = error.lock().unwrap().take() {
        return Err((e, None));
    }
    match (result, backend) {
        (Ok(()), Some(b)) => Ok(b),
        (Ok(()), None) => Err((
            "the viewer closed before it opened".into(),
            slot.lock().unwrap().take(),
        )),
        (Err(e), b) => Err((
            e.to_string(),
            if b.is_none() {
                slot.lock().unwrap().take()
            } else {
                None
            },
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_size_rounds_and_never_hits_zero() {
        assert_eq!(physical(egui::vec2(100.4, 50.6), 2.0), [201, 101]);
        assert_eq!(physical(egui::vec2(0.0, 0.0), 2.0), [1, 1]);
    }

    #[test]
    fn zoom_notches_match_the_wheel_and_pinch() {
        assert_eq!(zoom_notches(0.0, 1.0), 0.0);
        assert!((zoom_notches(50.0, 1.0) - 1.0).abs() < 1e-5);
        assert!((zoom_notches(0.0, 1.12) - 1.0).abs() < 1e-5);
        assert!(zoom_notches(-50.0, 1.0) < 0.0);
    }

    #[test]
    fn an_unchanged_view_is_not_re_rendered() {
        let key = RenderKey {
            camera: Camera::default(),
            show_construction: false,
            size: [100, 100],
            dark: false,
            stripe: 6,
            section: None,
            picked: None,
            visibility: 0,
        };
        let mut last = None;
        assert!(needs_render(&mut last, key), "the first frame renders");
        assert!(!needs_render(&mut last, key), "an idle frame does not");
        for changed in [
            RenderKey {
                size: [101, 100],
                ..key
            },
            RenderKey { dark: true, ..key },
            RenderKey { stripe: 12, ..key },
            RenderKey {
                section: Some([1.0, 0.0, 0.0, 5.0]),
                ..key
            },
            RenderKey {
                picked: Some(Pick { part: 0, face: 3 }),
                ..key
            },
            RenderKey {
                visibility: 1,
                ..key
            },
            RenderKey {
                show_construction: true,
                ..key
            },
            RenderKey {
                camera: Camera {
                    zoom: 1.1,
                    ..Camera::default()
                },
                ..key
            },
        ] {
            assert!(needs_render(&mut last, changed), "{changed:?}");
            assert!(needs_render(&mut last, key));
        }
    }

    #[test]
    fn summary_counts_faces_by_status() {
        use crate::{Face, Lines, Mesh, Part};
        let part = Part {
            name: None,
            color: None,
            mesh: Mesh::default(),
            faces: vec![
                Face::plain(FaceStatus::Ok),
                Face::plain(FaceStatus::Approx),
                Face::plain(FaceStatus::Missing),
            ],
            lines: Lines::default(),
        };
        let scene = Scene {
            bbox: crate::BBox {
                min: [0.0; 3],
                max: [1.0; 3],
            },
            parts: vec![part],
        };
        let s = Summary::of(&scene, "m.step");
        assert_eq!((s.parts, s.faces, s.approx, s.missing), (1, 3, 1, 1));
    }
}
