//! The GPU viewer's window: eframe around [`super::gpu`]'s offscreen render.
//!
//! The layout is a toolbar (view presets, display toggles, the backend and
//! sandbox pills, the theme switch), a properties panel on the right, a
//! status bar, and the viewport. #29 adds picking and the inspector, #30 the
//! model tree.
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
use super::gpu::{GpuScene, Layout, Renderer, Target, View};
use super::widgets::{self, Tone};
use super::{Backend, Options, ThemePref, theme};
use crate::render::Camera;
use crate::{FaceStatus, Scene};

/// What a viewport frame depends on. Equal keys draw equal pixels, so an
/// equal key skips the render.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderKey {
    pub camera: Camera,
    pub show_construction: bool,
    /// The viewport in physical pixels.
    pub size: [u32; 2],
    pub dark: bool,
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
    /// Set once the screenshot is saved (or failed): close next frame.
    error: Arc<Mutex<Option<String>>>,
}

impl Viewer {
    fn new(
        cc: &eframe::CreationContext<'_>,
        scene: &Scene,
        opts: &Options,
        error: Arc<Mutex<Option<String>>>,
    ) -> Result<Self, String> {
        let rs = cc
            .wgpu_render_state
            .as_ref()
            .ok_or("eframe started without wgpu")?;
        theme::install(&cc.egui_ctx, opts.theme);
        let info = rs.adapter.get_info();
        let renderer = Renderer::new(&rs.device);
        let gpu_scene = GpuScene::upload(&rs.device, &renderer, Layout::new(scene));
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
            error,
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
        if pressed(Key::Q) || pressed(Key::Escape) {
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
                    theme::install(ui.ctx(), self.theme);
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

    fn properties(&self, ui: &mut egui::Ui) {
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
                    "{} {} faces approximated, {} missing",
                    icon::WARNING,
                    s.approx,
                    s.missing
                ))
                .color(t.warning),
            );
        }
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

    /// Re-renders the viewport if `key` differs from the last frame's.
    fn render(&mut self, rs: &egui_wgpu::RenderState, key: RenderKey, clear: egui::Color32) {
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
        let target = self.target.as_ref().expect("created above");
        let view = View {
            camera: key.camera,
            show_construction: key.show_construction,
            clear: clear.to_normalized_gamma_f32().map(f64::from),
        };
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
        let key = RenderKey {
            camera: self.controls.camera,
            show_construction: self.controls.show_construction,
            size: physical(rect.size(), ui.ctx().pixels_per_point()),
            dark,
        };
        self.render(rs, key, theme::tokens(dark).viewport);
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
    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else {
            return;
        };
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            if let Err(e) = save(&image, &path) {
                *self.error.lock().unwrap() = Some(e);
            }
            self.screenshot = None;
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return;
        }
        // A few frames first: fonts, the theme and the first render land.
        if self.frames == 3 {
            ctx.send_viewport_cmd(ViewportCommand::Screenshot(Default::default()));
        }
        ctx.request_repaint();
    }
}

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
                    "Drag to orbit · right-drag or shift-drag to pan · scroll to zoom · R reset · F front · T top · C construction",
                )
                .size(theme::size::SMALL)
                .color(t.muted_foreground),
            );
        });
        egui::Panel::right("properties")
            .frame(
                egui::Frame::NONE
                    .fill(t.panel)
                    .inner_margin(egui::Margin::same(theme::space(3) as i8))
                    .stroke(egui::Stroke::new(1.0, t.border)),
            )
            .default_size(260.0)
            .min_size(200.0)
            .show(root, |ui| self.properties(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(t.viewport))
            .show(root, |ui| self.viewport(ui, &rs));
        self.frames = self.frames.saturating_add(1);
        self.screenshot(&ctx);
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
            match Viewer::new(cc, &scene, &opts2, error2) {
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
