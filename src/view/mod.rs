//! `stepv view`: the interactive viewer (#21).
//!
//! On a GPU it is egui + wgpu (#27's verdict, plan.md "Viewer stack"):
//! [`gpu`] draws the scene into an offscreen target the viewer owns, and
//! `app` lays the window out around it, styled by [`theme`] and
//! [`widgets`]. With `--software`, or when there is no usable GPU adapter, it
//! is the minifb window over the software rasteriser ([`software`]), the
//! viewer before #28. Both drive the same [`Controls`].
//!
//! The run reports which one it got as `"backend"` (`metal`, `vulkan`, `gl`,
//! `dx12` or `software`), as the kernel reports `"sandbox"`.

#[cfg(feature = "viewer")]
mod app;
pub mod controls;
#[cfg(feature = "viewer")]
pub mod gpu;
pub mod inspect;
#[cfg(feature = "viewer")]
pub mod software;
#[cfg(feature = "viewer")]
pub mod theme;
pub mod tree;
#[cfg(feature = "viewer")]
pub mod widgets;

use std::path::PathBuf;

pub use controls::{Controls, Input};

/// What drew the viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Metal,
    Vulkan,
    Gl,
    Dx12,
    /// A wgpu backend this list does not name.
    Other,
    /// The minifb window over the software rasteriser.
    Software,
}

impl Backend {
    /// The name `"backend"` reports.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Metal => "metal",
            Self::Vulkan => "vulkan",
            Self::Gl => "gl",
            Self::Dx12 => "dx12",
            Self::Other => "other",
            Self::Software => "software",
        }
    }

    #[cfg(feature = "viewer")]
    #[must_use]
    pub fn from_wgpu(b: eframe::egui_wgpu::wgpu::Backend) -> Self {
        use eframe::egui_wgpu::wgpu::Backend as W;
        match b {
            W::Metal => Self::Metal,
            W::Vulkan => Self::Vulkan,
            W::Gl => Self::Gl,
            W::Dx12 => Self::Dx12,
            _ => Self::Other,
        }
    }
}

/// `--theme`: follow the OS, or force one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemePref {
    #[default]
    Auto,
    Light,
    Dark,
}

impl std::str::FromStr for ThemePref {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "auto" => Ok(Self::Auto),
            "light" => Ok(Self::Light),
            "dark" => Ok(Self::Dark),
            _ => Err(format!("--theme: expected auto, light or dark, got {s:?}")),
        }
    }
}

/// How to open the viewer.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// `--software`: skip the GPU.
    pub software: bool,
    pub theme: ThemePref,
    /// The file's name, for the properties panel.
    pub file: String,
    /// The kernel's `"sandbox"`, and whether that is the full sandbox.
    pub sandbox: Option<String>,
    pub sandboxed: bool,
    /// `STEPV_VIEW_SCREENSHOT`: draw, save the window as PNG here, and quit.
    /// The tests' and the PR screenshots' hook.
    pub screenshot: Option<PathBuf>,
    /// `STEPV_VIEW_PICK=x,y`: click there (fractions of the viewport) once
    /// the first frame is drawn. With `screenshot`, it waits for the pick.
    pub pick_at: Option<(f32, f32)>,
}

/// Parses `STEPV_VIEW_PICK`'s `x,y`, each 0..1.
#[must_use]
pub fn parse_pick_at(s: &str) -> Option<(f32, f32)> {
    let (x, y) = s.split_once(',')?;
    let (x, y): (f32, f32) = (x.trim().parse().ok()?, y.trim().parse().ok()?);
    ((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y)).then_some((x, y))
}

/// The backend to open, given `--software` and what [`gpu::probe`] found,
/// and the note to print when it is not what the user would expect.
#[must_use]
pub fn choose(software: bool, adapter: Option<Backend>) -> (Backend, Option<&'static str>) {
    match (software, adapter) {
        (true, _) => (Backend::Software, None),
        (false, Some(b)) => (b, None),
        (false, None) => (
            Backend::Software,
            Some("no usable GPU adapter; using the software viewer (--software)"),
        ),
    }
}

/// Opens the viewer on `scene` and runs until the user closes it; returns the
/// backend that drew it.
///
/// # Errors
/// When no window can be opened at all, or the scene is empty.
#[cfg(feature = "viewer")]
pub fn run(
    scene: crate::Scene,
    topology: Option<crate::topology::Topology>,
    title: &str,
    opts: &Options,
) -> Result<Backend, String> {
    let probe = if opts.software { None } else { gpu::probe() };
    let (backend, note) = choose(opts.software, probe);
    if let Some(note) = note {
        eprintln!("stepv: {note}");
    }
    if backend == Backend::Software {
        software::run(&scene, title, opts.screenshot.as_deref())?;
        return Ok(Backend::Software);
    }
    match app::run(scene, topology, title, opts) {
        Ok(b) => Ok(b),
        // The window never opened: the probe found an adapter the window
        // could not use (a surface it cannot present to, a device it refused).
        Err((e, Some(scene))) => {
            eprintln!("stepv: the GPU viewer could not start ({e}); using the software viewer");
            software::run(&scene, title, opts.screenshot.as_deref())?;
            Ok(Backend::Software)
        }
        Err((e, None)) => Err(e),
    }
}

/// Without the `viewer` feature there is no window to open.
///
/// # Errors
/// Always.
#[cfg(not(feature = "viewer"))]
pub fn run(
    _scene: crate::Scene,
    _topology: Option<crate::topology::Topology>,
    _title: &str,
    _opts: &Options,
) -> Result<Backend, String> {
    Err("this stepv was built without the `viewer` feature".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_adapter_falls_back_to_software_and_says_so() {
        let (b, note) = choose(false, None);
        assert_eq!(b, Backend::Software);
        assert!(note.unwrap().contains("software viewer"));
    }

    #[test]
    fn software_is_honoured_even_with_a_gpu() {
        assert_eq!(
            choose(true, Some(Backend::Metal)),
            (Backend::Software, None)
        );
        assert_eq!(
            choose(false, Some(Backend::Vulkan)),
            (Backend::Vulkan, None)
        );
    }

    #[test]
    fn backend_names_are_the_reported_strings() {
        let all = [
            Backend::Metal,
            Backend::Vulkan,
            Backend::Gl,
            Backend::Dx12,
            Backend::Other,
            Backend::Software,
        ];
        let names: Vec<_> = all.iter().map(|b| b.name()).collect();
        assert_eq!(
            names,
            ["metal", "vulkan", "gl", "dx12", "other", "software"]
        );
    }

    #[test]
    fn pick_at_parses_fractions_only() {
        assert_eq!(parse_pick_at("0.5, 0.25"), Some((0.5, 0.25)));
        assert_eq!(parse_pick_at("2,0"), None);
        assert_eq!(parse_pick_at("x"), None);
    }

    #[test]
    fn theme_parses() {
        assert_eq!("dark".parse(), Ok(ThemePref::Dark));
        assert_eq!("auto".parse(), Ok(ThemePref::Auto));
        assert!("blue".parse::<ThemePref>().is_err());
    }
}
