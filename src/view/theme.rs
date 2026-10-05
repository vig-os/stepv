//! The viewer's look: shadcn-style design tokens, applied through egui's own
//! [`egui::Style`] and [`egui::Visuals`].
//!
//! No component kit (#28): the shadcn-like kits for egui (armas, egui-shadcn)
//! trail egui by three minors, and a one-person kit would gate every egui
//! upgrade. Tokens set through `Style` upgrade with egui instead. Every colour,
//! radius and size the viewer draws comes from this module; the in-house
//! components in [`super::widgets`] read [`Tokens`] rather than hard-coding.
//!
//! The palette is shadcn's neutral (zinc) with one accent (blue), in a light
//! and a dark variant; the window follows the OS unless `--theme` says
//! otherwise.

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Theme,
    Visuals, vec2,
};

use super::ThemePref;

/// One theme's colours. Named for their role, as shadcn's CSS variables are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tokens {
    pub dark: bool,
    /// Text edits and the deepest surfaces.
    pub background: Color32,
    pub foreground: Color32,
    /// Side panels, the toolbar and the status bar.
    pub panel: Color32,
    /// Windows, popups and menus.
    pub card: Color32,
    /// Quiet fills: buttons at rest, striped rows.
    pub muted: Color32,
    /// Secondary text: labels, hints, property keys.
    pub muted_foreground: Color32,
    /// Button fill under the pointer, and while pressed.
    pub hover: Color32,
    pub pressed: Color32,
    pub border: Color32,
    /// The one accent: selection, focus, toggled buttons, links.
    pub accent: Color32,
    /// Text and icons on [`Self::accent`].
    pub accent_foreground: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub destructive: Color32,
    /// Behind the model: the thumbnails' grey in light mode.
    pub viewport: Color32,
}

/// Corner radius of every widget, in points (shadcn's `--radius`).
pub const RADIUS: u8 = 6;

/// The spacing scale: `space(n)` is `4n` points.
#[must_use]
pub const fn space(n: u8) -> f32 {
    4.0 * n as f32
}

/// The type scale, in points.
pub mod size {
    pub const SMALL: f32 = 11.0;
    pub const BODY: f32 = 13.0;
    pub const HEADING: f32 = 16.0;
    pub const MONO: f32 = 12.0;
    /// Toolbar icons (Phosphor glyphs are drawn as text).
    pub const ICON: f32 = 16.0;
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

pub const LIGHT: Tokens = Tokens {
    dark: false,
    background: rgb(0xffffff),
    foreground: rgb(0x09090b),
    panel: rgb(0xfafafa),
    card: rgb(0xffffff),
    muted: rgb(0xf4f4f5),
    // zinc-500 (#71717a) is 4.4:1 on `muted`, under AA; a shade darker.
    muted_foreground: rgb(0x6b6b73),
    hover: rgb(0xe4e4e7),
    pressed: rgb(0xd4d4d8),
    border: rgb(0xe4e4e7),
    accent: rgb(0x2563eb),
    accent_foreground: rgb(0xffffff),
    success: rgb(0x15803d),
    warning: rgb(0xb45309),
    destructive: rgb(0xdc2626),
    viewport: rgb(0xe8eaee),
};

pub const DARK: Tokens = Tokens {
    dark: true,
    background: rgb(0x09090b),
    foreground: rgb(0xfafafa),
    panel: rgb(0x18181b),
    card: rgb(0x18181b),
    muted: rgb(0x27272a),
    muted_foreground: rgb(0xa1a1aa),
    hover: rgb(0x3f3f46),
    pressed: rgb(0x52525b),
    border: rgb(0x27272a),
    accent: rgb(0x60a5fa),
    accent_foreground: rgb(0x09090b),
    success: rgb(0x4ade80),
    warning: rgb(0xfbbf24),
    destructive: rgb(0xf87171),
    viewport: rgb(0x1f2024),
};

/// The tokens for a light or dark UI.
#[must_use]
pub const fn tokens(dark: bool) -> &'static Tokens {
    if dark { &DARK } else { &LIGHT }
}

/// The complete egui style for `t`.
#[must_use]
pub fn style(t: &Tokens) -> egui::Style {
    let mut s = egui::Style::default();
    let r = CornerRadius::same(RADIUS);
    s.text_styles = [
        (
            TextStyle::Small,
            FontId::new(size::SMALL, FontFamily::Proportional),
        ),
        (
            TextStyle::Body,
            FontId::new(size::BODY, FontFamily::Proportional),
        ),
        (
            TextStyle::Button,
            FontId::new(size::BODY, FontFamily::Proportional),
        ),
        (
            TextStyle::Heading,
            FontId::new(size::HEADING, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(size::MONO, FontFamily::Monospace),
        ),
    ]
    .into();

    let sp = &mut s.spacing;
    sp.item_spacing = vec2(space(2), space(1));
    sp.button_padding = vec2(space(2), space(1));
    sp.window_margin = Margin::same(space(3) as i8);
    sp.menu_margin = Margin::same(space(2) as i8);
    sp.indent = space(4);
    sp.interact_size = vec2(space(10), space(6));
    sp.icon_spacing = space(1);

    let mut v = if t.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.dark_mode = t.dark;
    v.override_text_color = None;
    v.hyperlink_color = t.accent;
    v.faint_bg_color = t.muted;
    v.extreme_bg_color = t.background;
    v.text_edit_bg_color = Some(t.background);
    v.code_bg_color = t.muted;
    v.warn_fg_color = t.warning;
    v.error_fg_color = t.destructive;
    v.panel_fill = t.panel;
    v.window_fill = t.card;
    v.window_stroke = Stroke::new(1.0, t.border);
    v.window_corner_radius = r;
    v.menu_corner_radius = r;
    let shadow = Shadow {
        offset: [0, 2],
        blur: 8,
        spread: 0,
        color: Color32::from_black_alpha(if t.dark { 96 } else { 24 }),
    };
    v.window_shadow = shadow;
    v.popup_shadow = shadow;
    v.selection.bg_fill = t.accent;
    v.selection.stroke = Stroke::new(1.0, t.accent_foreground);
    v.collapsing_header_frame = false;
    v.indent_has_left_vline = false;

    let w = &mut v.widgets;
    for (wv, fill) in [
        (&mut w.noninteractive, t.panel),
        (&mut w.inactive, t.muted),
        (&mut w.hovered, t.hover),
        (&mut w.active, t.pressed),
        (&mut w.open, t.hover),
    ] {
        wv.corner_radius = r;
        wv.bg_fill = fill;
        wv.weak_bg_fill = fill;
        wv.fg_stroke = Stroke::new(1.0, t.foreground);
        wv.bg_stroke = Stroke::NONE;
        wv.expansion = 0.0;
    }
    // Separators and frames draw with the noninteractive stroke.
    w.noninteractive.bg_stroke = Stroke::new(1.0, t.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, t.foreground);
    // A focus ring, as shadcn's `--ring`.
    w.hovered.bg_stroke = Stroke::new(1.0, t.border);
    s.visuals = v;
    s
}

/// Installs the fonts (with the Phosphor icons) and both styles on `ctx`, and
/// picks the theme `pref` asks for.
pub fn install(ctx: &egui::Context, pref: ThemePref) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);
    ctx.set_style_of(Theme::Light, style(&LIGHT));
    ctx.set_style_of(Theme::Dark, style(&DARK));
    ctx.set_theme(match pref {
        ThemePref::Auto => egui::ThemePreference::System,
        ThemePref::Light => egui::ThemePreference::Light,
        ThemePref::Dark => egui::ThemePreference::Dark,
    });
}

/// WCAG 2 contrast ratio between two opaque colours, 1 to 21.
#[must_use]
pub fn contrast(a: Color32, b: Color32) -> f32 {
    fn lum(c: Color32) -> f32 {
        let ch = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.040_45 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
    }
    let (la, lb) = (lum(a), lum(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTH: [&Tokens; 2] = [&LIGHT, &DARK];

    #[test]
    fn text_meets_wcag_aa_on_every_surface() {
        for t in BOTH {
            for surface in [t.background, t.panel, t.card, t.muted] {
                assert!(contrast(t.foreground, surface) >= 7.0, "{t:?}");
                assert!(
                    contrast(t.muted_foreground, surface) >= 4.5,
                    "muted text on {surface:?}, dark={}",
                    t.dark
                );
            }
            // Text on hover/pressed button fills stays legible too.
            for fill in [t.hover, t.pressed] {
                assert!(contrast(t.foreground, fill) >= 4.5, "dark={}", t.dark);
            }
            assert!(
                contrast(t.accent_foreground, t.accent) >= 4.5,
                "dark={}",
                t.dark
            );
        }
    }

    #[test]
    fn status_colours_are_readable_as_text() {
        for t in BOTH {
            for c in [t.success, t.warning, t.destructive, t.accent] {
                assert!(contrast(c, t.panel) >= 3.0, "{c:?} on dark={}", t.dark);
            }
        }
    }

    #[test]
    fn the_model_stands_out_from_the_viewport() {
        // render::DEFAULT_COLOR, lit at its darkest and brightest.
        let base = crate::render::DEFAULT_COLOR;
        for t in BOTH {
            let lit = |k: f32| {
                let c = |v: f32| crate::render::srgb(v * k);
                Color32::from_rgb(c(base.r), c(base.g), c(base.b))
            };
            let best = contrast(lit(0.22), t.viewport).max(contrast(lit(1.0), t.viewport));
            assert!(
                best >= 1.5,
                "dark={}: model blends into the viewport",
                t.dark
            );
        }
    }

    #[test]
    fn style_uses_the_tokens() {
        for t in BOTH {
            let s = style(t);
            assert_eq!(s.visuals.dark_mode, t.dark);
            assert_eq!(s.visuals.panel_fill, t.panel);
            assert_eq!(s.visuals.selection.bg_fill, t.accent);
            assert_eq!(
                s.visuals.widgets.inactive.corner_radius,
                CornerRadius::same(RADIUS)
            );
            assert_eq!(s.visuals.widgets.hovered.weak_bg_fill, t.hover);
            assert_eq!(s.visuals.window_corner_radius, CornerRadius::same(RADIUS));
            assert_eq!(s.text_styles[&TextStyle::Body].size, size::BODY);
        }
    }

    #[test]
    fn spacing_is_on_the_4pt_grid() {
        let s = style(&LIGHT).spacing;
        for v in [
            s.item_spacing.x,
            s.item_spacing.y,
            s.button_padding.x,
            s.button_padding.y,
            s.indent,
            s.interact_size.x,
            s.interact_size.y,
            f32::from(s.window_margin.left),
            f32::from(s.menu_margin.left),
        ] {
            assert_eq!(v % 4.0, 0.0, "{v} is off the 4-pt grid");
        }
    }

    #[test]
    fn install_follows_the_preference() {
        let ctx = egui::Context::default();
        install(&ctx, ThemePref::Dark);
        assert_eq!(ctx.theme(), Theme::Dark);
        install(&ctx, ThemePref::Light);
        assert_eq!(ctx.theme(), Theme::Light);
        assert_eq!(ctx.global_style().visuals.panel_fill, LIGHT.panel);
    }
}
