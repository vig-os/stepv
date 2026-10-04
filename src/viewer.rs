//! `stepv view`: an interactive window over the software rasteriser.
//!
//! The standalone viewer `plan.md` §1 asks for on Linux (the `.desktop` file
//! opens STEP/IGES/BREP with it), and usable on macOS too. It reuses
//! [`crate::render`] — the same pixels, overlay and colours as the
//! thumbnails — so there is one renderer to trust, not two.
//!
//! Controls: drag to orbit, right-drag or shift-drag to pan, scroll to zoom,
//! `R` reset, `F` front, `T` top, `C` construction curves, `Q`/Esc quit.
//!
//! Input handling is the pure [`Controls`] state machine, so the camera
//! behaviour is tested without a window; only [`run`] needs a display.

use crate::Scene;
use crate::render::{self, Camera, Fit};

/// What the user did since the last frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    /// Pointer moved by (dx, dy) pixels with the orbit button held.
    Orbit(f32, f32),
    /// Pointer moved by (dx, dy) pixels with the pan button held.
    Pan(f32, f32),
    /// Scroll wheel, positive = away from the user (zoom in).
    Scroll(f32),
    Reset,
    Front,
    Top,
    ToggleConstruction,
}

/// The viewer's state: the camera plus display toggles.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Controls {
    pub camera: Camera,
    /// Where Reset returns to: the view the window opened with.
    pub home: Camera,
    pub show_construction: bool,
}

impl Controls {
    /// Opening on `scene`'s own best view ([`Camera::for_scene`]): a flat
    /// part face-on, anything else from the default angle.
    #[must_use]
    pub fn for_scene(scene: &crate::Scene) -> Self {
        let home = Camera::for_scene(scene);
        Self {
            camera: home,
            home,
            show_construction: false,
        }
    }

    /// Degrees of orbit per pixel dragged.
    const ORBIT: f32 = 0.4;
    /// Zoom factor per scroll notch.
    const ZOOM: f32 = 1.12;

    /// Applies one input; `short_edge` is the window's shorter side in
    /// pixels, so panning tracks the pointer at any window size.
    pub fn apply(&mut self, input: Input, short_edge: f32) {
        let c = &mut self.camera;
        match input {
            Input::Orbit(dx, dy) => {
                c.azimuth_deg = (c.azimuth_deg - dx * Self::ORBIT).rem_euclid(360.0);
                c.elevation_deg = (c.elevation_deg + dy * Self::ORBIT).clamp(-89.0, 89.0);
            }
            Input::Pan(dx, dy) => {
                let s = short_edge.max(1.0);
                c.pan[0] += dx / s;
                c.pan[1] += dy / s;
            }
            Input::Scroll(notches) => {
                c.zoom = (c.zoom * Self::ZOOM.powf(notches)).clamp(0.05, 200.0);
            }
            Input::Reset => *c = self.home,
            Input::Front => {
                *c = Camera {
                    azimuth_deg: 0.0,
                    elevation_deg: 0.0,
                    ..Camera::default()
                }
            }
            Input::Top => {
                *c = Camera {
                    azimuth_deg: 0.0,
                    elevation_deg: 89.0,
                    ..Camera::default()
                }
            }
            Input::ToggleConstruction => self.show_construction = !self.show_construction,
        }
    }

    /// Render options for a `width` × `height` frame. `moving` drops
    /// supersampling so dragging stays responsive.
    #[must_use]
    pub fn options(&self, width: u32, height: u32, moving: bool) -> render::Options {
        render::Options {
            width,
            height,
            show_construction: self.show_construction,
            supersample: if moving { 1 } else { 2 },
            camera: self.camera,
            fit: Fit::Sphere,
        }
    }
}

/// The window background, as 0RGB.
const BACKGROUND: [u8; 3] = [0xe8, 0xea, 0xee];

/// Composites an RGBA render over the background into minifb's 0RGB.
#[must_use]
pub fn composite(img: &render::Image) -> Vec<u32> {
    img.rgba
        .chunks_exact(4)
        .map(|p| {
            let a = u32::from(p[3]);
            let mix = |c: u8, b: u8| (u32::from(c) * a + u32::from(b) * (255 - a)) / 255;
            (mix(p[0], BACKGROUND[0]) << 16)
                | (mix(p[1], BACKGROUND[1]) << 8)
                | mix(p[2], BACKGROUND[2])
        })
        .collect()
}

/// Opens a window on `scene` and runs until the user closes it.
///
/// # Errors
/// When no window can be opened (no display) or the scene is empty.
#[cfg(feature = "viewer")]
pub fn run(scene: &Scene, title: &str) -> Result<(), String> {
    use minifb::{Key, KeyRepeat, MouseButton, MouseMode, Window, WindowOptions};

    let mut window = Window::new(
        title,
        960,
        720,
        WindowOptions {
            resize: true,
            ..WindowOptions::default()
        },
    )
    .map_err(|e| format!("cannot open a window: {e}"))?;
    window.set_target_fps(60);

    let mut controls = Controls::for_scene(scene);
    let mut last_mouse: Option<(f32, f32)> = None;
    let mut dirty = true;
    let mut moving_frames = 0u32;
    let (mut w, mut h) = window.get_size();
    let mut frame: Vec<u32> = vec![0; w * h];

    while window.is_open() && !window.is_key_down(Key::Escape) && !window.is_key_down(Key::Q) {
        let size = window.get_size();
        if size != (w, h) && size.0 > 0 && size.1 > 0 {
            (w, h) = size;
            dirty = true;
        }
        let short = w.min(h) as f32;
        let mut moved = false;
        let mouse = window.get_mouse_pos(MouseMode::Pass);
        let left = window.get_mouse_down(MouseButton::Left);
        let right = window.get_mouse_down(MouseButton::Right);
        let shift = window.is_key_down(Key::LeftShift) || window.is_key_down(Key::RightShift);
        if let (Some((x, y)), true) = (mouse, left || right) {
            if let Some((lx, ly)) = last_mouse {
                let (dx, dy) = (x - lx, y - ly);
                if dx != 0.0 || dy != 0.0 {
                    let input = if right || shift {
                        Input::Pan(dx, dy)
                    } else {
                        Input::Orbit(dx, dy)
                    };
                    controls.apply(input, short);
                    moved = true;
                }
            }
            last_mouse = Some((x, y));
        } else {
            last_mouse = None;
        }
        if let Some((_, sy)) = window.get_scroll_wheel()
            && sy != 0.0
        {
            controls.apply(Input::Scroll(sy.signum()), short);
            moved = true;
        }
        for (key, input) in [
            (Key::R, Input::Reset),
            (Key::F, Input::Front),
            (Key::T, Input::Top),
            (Key::C, Input::ToggleConstruction),
        ] {
            if window.is_key_pressed(key, KeyRepeat::No) {
                controls.apply(input, short);
                dirty = true;
            }
        }
        if moved {
            moving_frames = 6;
        }
        // Draw cheaply while moving; once input settles, one sharp frame.
        if moved || dirty || moving_frames == 1 {
            let opts = controls.options(w as u32, h as u32, moved);
            frame = render::render(scene, &opts)
                .map(|img| composite(&img))
                .map_err(|e| e.to_string())?;
            dirty = false;
        }
        moving_frames = moving_frames.saturating_sub(1);
        window
            .update_with_buffer(&frame, w, h)
            .map_err(|e| format!("window update failed: {e}"))?;
    }
    Ok(())
}

/// Without the `viewer` feature there is no window to open.
#[cfg(not(feature = "viewer"))]
pub fn run(_scene: &Scene, _title: &str) -> Result<(), String> {
    Err("this stepv was built without the `viewer` feature".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orbit_wraps_azimuth_and_clamps_elevation() {
        let mut c = Controls::default();
        c.apply(Input::Orbit(-1000.0, 0.0), 500.0);
        assert!((0.0..360.0).contains(&c.camera.azimuth_deg));
        c.apply(Input::Orbit(0.0, 10_000.0), 500.0);
        assert_eq!(c.camera.elevation_deg, 89.0);
        c.apply(Input::Orbit(0.0, -10_000.0), 500.0);
        assert_eq!(c.camera.elevation_deg, -89.0);
    }

    #[test]
    fn scroll_zooms_multiplicatively_and_is_bounded() {
        let mut c = Controls::default();
        c.apply(Input::Scroll(1.0), 500.0);
        c.apply(Input::Scroll(-1.0), 500.0);
        assert!(
            (c.camera.zoom - 1.0).abs() < 1e-5,
            "in then out is identity"
        );
        for _ in 0..1000 {
            c.apply(Input::Scroll(1.0), 500.0);
        }
        assert_eq!(c.camera.zoom, 200.0);
    }

    #[test]
    fn pan_tracks_the_pointer_at_any_window_size() {
        let mut a = Controls::default();
        let mut b = Controls::default();
        a.apply(Input::Pan(50.0, 0.0), 500.0);
        b.apply(Input::Pan(100.0, 0.0), 1000.0);
        assert_eq!(a.camera.pan, b.camera.pan);
    }

    #[test]
    fn reset_and_presets() {
        let mut c = Controls::default();
        c.apply(Input::Orbit(123.0, 45.0), 500.0);
        c.apply(Input::Scroll(3.0), 500.0);
        c.apply(Input::Reset, 500.0);
        assert_eq!(c.camera, Camera::default());
        c.apply(Input::Top, 500.0);
        assert_eq!(c.camera.elevation_deg, 89.0);
        c.home = Camera {
            elevation_deg: 90.0,
            ..Camera::default()
        };
        c.apply(Input::Reset, 500.0);
        assert_eq!(c.camera, c.home, "reset returns to the opening view");
        c.apply(Input::ToggleConstruction, 500.0);
        assert!(c.show_construction);
    }

    #[test]
    fn moving_frames_skip_supersampling() {
        let c = Controls::default();
        assert_eq!(c.options(10, 10, true).supersample, 1);
        assert_eq!(c.options(10, 10, false).supersample, 2);
        assert_eq!(c.options(10, 10, false).fit, Fit::Sphere);
    }

    #[test]
    fn composite_blends_over_the_background() {
        let img = render::Image {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 0, 0, 0],
        };
        let px = composite(&img);
        assert_eq!(px[0], 0x00ff_0000, "opaque red stays red");
        let bg = (u32::from(BACKGROUND[0]) << 16)
            | (u32::from(BACKGROUND[1]) << 8)
            | u32::from(BACKGROUND[2]);
        assert_eq!(px[1], bg, "transparent shows the background");
    }
}
