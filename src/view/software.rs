//! `stepv view --software`: a minifb window over the software rasteriser.
//!
//! The pre-#28 viewer, kept as the fallback when there is no GPU adapter (a
//! headless VM, a remote X session without GLX, a broken driver). It reuses
//! [`crate::render`], the same pixels as the thumbnails, so it needs nothing
//! beyond a window.

use std::path::Path;

use super::controls::{Controls, Input, composite};
use crate::Scene;
use crate::render;

/// Opens a window on `scene` and runs until the user closes it. With
/// `screenshot`, it draws one frame, saves it there as PNG, and returns.
///
/// # Errors
/// When no window can be opened (no display) or the scene is empty.
pub fn run(scene: &Scene, title: &str, screenshot: Option<&Path>) -> Result<(), String> {
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
        if let Some(path) = screenshot {
            return save(&frame, w, h, path);
        }
    }
    Ok(())
}

/// Writes a 0RGB frame as an opaque PNG.
fn save(frame: &[u32], w: usize, h: usize, path: &Path) -> Result<(), String> {
    let rgba = frame
        .iter()
        .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8, 255])
        .collect();
    let img = render::Image {
        width: w as u32,
        height: h as u32,
        rgba,
    };
    let png = img.to_png().map_err(|e| e.to_string())?;
    std::fs::write(path, png).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn save_writes_the_frame_opaque() {
        let dir = std::env::temp_dir().join(format!("stepv-sw-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("f.png");
        let bg = (u32::from(super::super::controls::BACKGROUND[0]) << 16) | 0x00ff;
        super::save(&[0x00ff_0000, bg], 2, 1, &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
