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
pub mod measuring;
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
    /// `--frames N`: orbit one degree a frame for N frames, then close and
    /// report the frame intervals (#32's CI smoke test and a quick bench).
    pub frames: Option<u32>,
    /// `STEPV_VIEW_PICK=x,y`: click there (fractions of the viewport) once
    /// the first frame is drawn. With `screenshot`, it waits for the pick.
    pub pick_at: Vec<PickAt>,
    /// `STEPV_VIEW_MEASURE=1`: start in measure mode, and report each
    /// measurement on stderr (the window tests' hook, with `pick_at`).
    pub measure: bool,
    /// `STEPV_VIEW_SECTION=axis,offset[,flip]` (axis 0..2, offset 0..1):
    /// open with the model cut there and capped (#34's screenshots and
    /// window test).
    pub section: Option<(usize, f32, bool)>,
    /// `STEPV_VIEW_CLOSE_LOADING=1`: close the window once it has drawn the
    /// tree, before the scene arrives (#85's test of closing mid-load).
    pub close_loading: bool,
    /// The file and the limits a measurement server (#33) reads it with.
    pub input: Option<PathBuf>,
    pub limits: Option<crate::occt::Limits>,
    /// The faces the kernel's run left unhealed (#51), for the measurement
    /// server to replay. `None`: the run did not say (an older kernel), and
    /// the server goes by its own clock.
    pub unhealed: Option<Vec<u64>>,
}

/// What a `--frames` run measured: wall-clock intervals between frames,
/// as the window loop ran them (vsync-bound on a real display).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameStats {
    pub count: u32,
    pub p50_ms: f64,
    pub p95_ms: f64,
}

impl FrameStats {
    /// The statistics of `ms`, `None` when empty.
    #[must_use]
    pub fn of(mut ms: Vec<f64>) -> Option<Self> {
        if ms.is_empty() {
            return None;
        }
        ms.sort_by(f64::total_cmp);
        let at = |q: f64| ms[((ms.len() - 1) as f64 * q).round() as usize];
        Some(Self {
            count: ms.len() as u32,
            p50_ms: at(0.5),
            p95_ms: at(0.95),
        })
    }
}

/// What a viewer run did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ran {
    pub backend: Backend,
    /// With `--frames`.
    pub frames: Option<FrameStats>,
}

/// Where a scripted click goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PickAt {
    /// Fractions of the viewport.
    Viewport(f32, f32),
    /// A model point, projected through the camera at the time: the same
    /// spot on the model whatever the window's size.
    Model([f32; 3]),
}

/// Parses `STEPV_VIEW_PICK`: clicks separated by `;`, each `x,y` (viewport
/// fractions, 0..1) or `@x,y,z` (a model point). Nothing, if any is
/// malformed.
#[must_use]
pub fn parse_pick_at(s: &str) -> Vec<PickAt> {
    let one = |c: &str| {
        let c = c.trim();
        let nums: Option<Vec<f32>> = c
            .trim_start_matches('@')
            .split(',')
            .map(|v| v.trim().parse().ok())
            .collect();
        match (c.starts_with('@'), nums?.as_slice()) {
            (true, &[x, y, z]) => Some(PickAt::Model([x, y, z])),
            (false, &[x, y]) if (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y) => {
                Some(PickAt::Viewport(x, y))
            }
            _ => None,
        }
    };
    s.split(';')
        .map(one)
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default()
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

/// Whether this process can open a window at all: a window-server (Aqua)
/// session on macOS, `DISPLAY` or `WAYLAND_DISPLAY` elsewhere on Unix. `view`
/// asks before it runs the kernel (#63, #84), so a user without one (ssh, a
/// build sandbox, a launchd daemon) is told at once instead of after the
/// whole run. Without a session, macOS's window layer aborts the process
/// outright, and the report on stdout would be lost with it.
///
/// `STEPV_VIEW_TEST_NO_SESSION=1` answers no, for the tests: a session cannot
/// be taken away from a process that has one on macOS.
///
/// # Errors
/// Without a session, saying why; always without the `viewer` feature.
pub fn session() -> Result<(), String> {
    #[cfg(not(feature = "viewer"))]
    return Err("this stepv was built without the `viewer` feature".into());
    #[cfg(feature = "viewer")]
    {
        let forced = std::env::var_os("STEPV_VIEW_TEST_NO_SESSION").is_some_and(|v| v == "1");
        if forced || !has_session() {
            return Err(no_session());
        }
        Ok(())
    }
}

#[cfg(feature = "viewer")]
fn no_session() -> String {
    if cfg!(target_os = "macos") {
        "no window session: this process has no window-server (Aqua) session, \
         as over ssh or in a build sandbox; run `stepv view` from a logged-in desktop"
            .into()
    } else {
        "no window session: neither DISPLAY nor WAYLAND_DISPLAY is set; run \
         `stepv view` in an X11 or Wayland session"
            .into()
    }
}

#[cfg(all(feature = "viewer", target_os = "macos"))]
fn has_session() -> bool {
    use std::ffi::c_void;
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        // NULL when the caller is not in a window-server (Quartz GUI) session.
        fn CGSessionCopyCurrentDictionary() -> *const c_void;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: *const c_void);
    }
    // SAFETY: no arguments; a non-null result is a dictionary we own.
    let d = unsafe { CGSessionCopyCurrentDictionary() };
    if d.is_null() {
        return false;
    }
    // SAFETY: released once, as the Copy rule asks.
    unsafe { CFRelease(d) };
    true
}

#[cfg(all(feature = "viewer", unix, not(target_os = "macos")))]
fn has_session() -> bool {
    ["DISPLAY", "WAYLAND_DISPLAY"]
        .iter()
        .any(|v| std::env::var_os(v).is_some_and(|s| !s.is_empty()))
}

#[cfg(all(feature = "viewer", not(unix)))]
fn has_session() -> bool {
    true
}

/// What the loader (`stepv view`'s kernel run, on its own thread) tells the
/// window as the run goes (#85).
#[derive(Debug)]
pub enum Load {
    /// The assembly tree and its leaf count: a cached outline artifact's
    /// before the kernel starts, then the run's `Event::Tree`.
    Tree(Vec<crate::topology::Node>, usize),
    Progress(crate::occt::Progress),
    Done(Box<Loaded>),
    /// The run did not produce a scene, and why: the report's status and
    /// error, as the CLI says them (#89). The window keeps the tree it has
    /// and says so.
    Failed(String),
}

/// A finished load: the scene and what came with it.
#[derive(Debug)]
pub struct Loaded {
    pub scene: crate::Scene,
    pub topology: Option<crate::topology::Topology>,
    /// The window's title once the parts are known.
    pub title: String,
    /// The kernel's `"sandbox"`, and the faces it left unhealed (#51).
    pub sandbox: Option<String>,
    pub unhealed: Option<Vec<u64>>,
}

impl Options {
    /// These options with what the finished load says about its kernel.
    #[must_use]
    pub fn loaded(&self, l: &Loaded) -> Self {
        Self {
            sandboxed: l.sandbox.as_deref().is_some_and(crate::occt::full_sandbox),
            sandbox: l.sandbox.clone(),
            unhealed: l.unhealed.clone(),
            ..self.clone()
        }
    }
}

/// The finished load from `load`, skipping the outline and progress; `None`
/// when it failed.
#[cfg(feature = "viewer")]
fn wait(load: &std::sync::mpsc::Receiver<Load>) -> Option<Box<Loaded>> {
    load.iter().find_map(|l| match l {
        Load::Done(l) => Some(Some(l)),
        Load::Failed(_) => Some(None),
        _ => None,
    })?
}

/// Opens the viewer at once and runs until the user closes it; returns the
/// backend that drew it. The GPU viewer shows the tree and the progress
/// from `load` until the scene arrives, then the scene (#85). The software
/// viewer has no tree: it waits for the scene before it opens (#90).
///
/// # Errors
/// When no window can be opened at all, or the scene is empty.
#[cfg(feature = "viewer")]
pub fn run(
    load: std::sync::mpsc::Receiver<Load>,
    title: &str,
    opts: &Options,
) -> Result<Ran, String> {
    let probe = if opts.software { None } else { gpu::probe() };
    let (backend, note) = choose(opts.software, probe);
    if let Some(note) = note {
        eprintln!("stepv: {note}");
    }
    let software = |back: app::Back| {
        let l = match back {
            app::Back::Load(load) => wait(&load).ok_or("the run produced no scene")?,
            app::Back::Loaded(l) => l,
        };
        software::run(&l.scene, &l.title, opts.screenshot.as_deref(), opts.frames).map(|frames| {
            Ran {
                backend: Backend::Software,
                frames,
            }
        })
    };
    if backend == Backend::Software {
        return software(app::Back::Load(load));
    }
    match app::run(load, title, opts) {
        Ok(ran) => Ok(ran),
        // The GPU cannot draw: the probe found an adapter the window could
        // not use (a surface it cannot present to, a device or pipelines it
        // refused), or the GPU refused the scene once it came.
        Err((e, Some(back))) => {
            eprintln!("stepv: the GPU viewer could not start ({e}); using the software viewer");
            software(back)
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
    _load: std::sync::mpsc::Receiver<Load>,
    _title: &str,
    _opts: &Options,
) -> Result<Ran, String> {
    Err("this stepv was built without the `viewer` feature".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "viewer")]
    #[test]
    fn a_loader_that_panics_ends_the_wait() {
        // The loader owns its sender (`run_view`): a panic drops it, and the
        // software viewer's wait ends instead of blocking for ever.
        let (tx, rx) = std::sync::mpsc::channel::<Load>();
        let loader = std::thread::spawn(move || {
            let _ = tx.send(Load::Tree(Vec::new(), 0));
            panic!("a loader bug");
        });
        assert!(loader.join().is_err());
        assert!(wait(&rx).is_none());
    }

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
    fn frame_stats_take_quantiles() {
        let s = FrameStats::of((1..=100).map(f64::from).collect()).unwrap();
        assert_eq!((s.count, s.p50_ms, s.p95_ms), (100, 51.0, 95.0));
        assert_eq!(FrameStats::of(vec![]), None);
        let one = FrameStats::of(vec![7.0]).unwrap();
        assert_eq!((one.p50_ms, one.p95_ms), (7.0, 7.0));
    }

    #[test]
    fn pick_at_parses_fractions_only() {
        assert_eq!(parse_pick_at("0.5, 0.25"), [PickAt::Viewport(0.5, 0.25)]);
        assert_eq!(
            parse_pick_at("0.1,0.2; @6,15.5,-1"),
            [PickAt::Viewport(0.1, 0.2), PickAt::Model([6.0, 15.5, -1.0])]
        );
        assert!(parse_pick_at("2,0").is_empty());
        assert!(parse_pick_at("@1,2").is_empty());
        assert!(parse_pick_at("0.1,0.2;x").is_empty());
    }

    #[test]
    fn theme_parses() {
        assert_eq!("dark".parse(), Ok(ThemePref::Dark));
        assert_eq!("auto".parse(), Ok(ThemePref::Auto));
        assert!("blue".parse::<ThemePref>().is_err());
    }
}
