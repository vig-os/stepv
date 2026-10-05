//! The viewer's input state machine, shared by the GPU and software windows.
//!
//! Controls: drag to orbit, right-drag or shift-drag to pan, scroll to zoom,
//! `R` reset, `F` front, `T` top, `C` construction curves, `Q`/Esc quit.
//!
//! Pure: the camera behaviour is tested without a window, and both backends
//! drive the same [`Camera`] so a view looks the same on either.

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

/// The software window's background, as 0RGB.
pub(crate) const BACKGROUND: [u8; 3] = [0xe8, 0xea, 0xee];

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
