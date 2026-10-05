//! The viewer's few in-house components, drawn from [`super::theme`]'s tokens:
//! a toolbar icon button, a panel section header, a key/value property row
//! and a status pill.

use eframe::egui::{self, Color32, CornerRadius, Response, RichText, Sense, Ui, vec2};

use super::theme::{self, Tokens, size, space};

/// The tokens of the theme `ui` is drawn in.
#[must_use]
pub fn tokens(ui: &Ui) -> &'static Tokens {
    theme::tokens(ui.visuals().dark_mode)
}

/// A toolbar button showing a Phosphor `icon` (square), or an icon and a
/// short `label`, with a tooltip. `selected` draws it in the accent, for
/// toggles.
pub fn icon_button(
    ui: &mut Ui,
    icon: &str,
    label: Option<&str>,
    tooltip: &str,
    selected: bool,
) -> Response {
    let t = tokens(ui);
    let fg = if selected {
        t.accent_foreground
    } else {
        t.foreground
    };
    let mut job = egui::text::LayoutJob::default();
    let font = |s| egui::TextFormat::simple(egui::FontId::proportional(s), fg);
    job.append(icon, 0.0, font(size::ICON));
    if let Some(label) = label {
        job.append(label, space(1), font(size::BODY));
    }
    job.halign = egui::Align::Center;
    let mut b = egui::Button::new(job)
        .min_size(vec2(space(7), space(7)))
        .corner_radius(CornerRadius::same(theme::RADIUS));
    if selected {
        b = b.fill(t.accent);
    } else {
        // Quiet at rest, as shadcn's "ghost" button: the fill shows on hover.
        b = b.fill(Color32::TRANSPARENT);
    }
    ui.add(b).on_hover_text(tooltip)
}

/// A panel section's header: small, muted, upper case, with space above.
pub fn section_header(ui: &mut Ui, title: &str) {
    let t = tokens(ui);
    ui.add_space(space(2));
    ui.label(
        RichText::new(title.to_uppercase())
            .size(size::SMALL)
            .strong()
            .color(t.muted_foreground),
    );
    ui.add_space(space(1));
}

/// One `key  value` row: the key muted on the left, the value right-aligned
/// and selectable (so a measurement can be copied). A value too long for
/// the row is cut with an ellipsis, in full on hover.
pub fn property_row(ui: &mut Ui, key: &str, value: &str) {
    let t = tokens(ui);
    ui.horizontal(|ui| {
        ui.label(RichText::new(key).color(t.muted_foreground));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(
                egui::Label::new(RichText::new(value).color(t.foreground))
                    .selectable(true)
                    .truncate(),
            );
        });
    });
}

/// The tone of a [`status_pill`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Success,
    Warning,
    Danger,
}

impl Tone {
    /// The pill's text colour in `t`.
    #[must_use]
    pub const fn color(self, t: &Tokens) -> Color32 {
        match self {
            Self::Neutral => t.muted_foreground,
            Self::Success => t.success,
            Self::Warning => t.warning,
            Self::Danger => t.destructive,
        }
    }
}

/// A small rounded label: `backend: metal`, `sandbox: seatbelt`. Tinted by
/// `tone`, with a tooltip explaining it.
pub fn status_pill(ui: &mut Ui, text: &str, tone: Tone, tooltip: &str) -> Response {
    let t = tokens(ui);
    let fg = tone.color(t);
    let galley =
        ui.painter()
            .layout_no_wrap(text.to_owned(), egui::FontId::proportional(size::SMALL), fg);
    let pad = vec2(space(2), space(1) / 2.0);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + 2.0 * pad, Sense::hover());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::same(u8::MAX), fg.gamma_multiply(0.14));
        p.rect_stroke(
            rect,
            CornerRadius::same(u8::MAX),
            egui::Stroke::new(1.0, fg.gamma_multiply(0.45)),
            egui::StrokeKind::Inside,
        );
        p.galley(rect.min + pad, galley, fg);
    }
    resp.on_hover_text(tooltip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::ThemePref;

    /// Runs `f` inside one frame of a themed context.
    fn in_frame(dark: bool, f: impl FnOnce(&mut Ui)) {
        let ctx = egui::Context::default();
        theme::install(
            &ctx,
            if dark {
                ThemePref::Dark
            } else {
                ThemePref::Light
            },
        );
        let mut f = Some(f);
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            if let Some(f) = f.take() {
                f(ui);
            }
        });
        out.textures_delta.clear();
    }

    #[test]
    fn components_lay_out_in_both_themes() {
        for dark in [false, true] {
            in_frame(dark, |ui| {
                assert_eq!(tokens(ui).dark, dark);
                let b = icon_button(ui, egui_phosphor::regular::HOUSE, None, "Reset view", false);
                assert!(b.rect.width() >= space(7) && b.rect.height() >= space(7));
                let l = icon_button(
                    ui,
                    egui_phosphor::regular::SQUARE,
                    Some("Front"),
                    "Front",
                    true,
                );
                assert!(l.rect.width() > b.rect.width(), "a label widens the button");
                section_header(ui, "Model");
                property_row(ui, "Parts", "12");
                let p = status_pill(ui, "metal", Tone::Success, "GPU backend");
                assert!(
                    p.rect.width() > p.rect.height(),
                    "a pill is wider than tall"
                );
            });
        }
    }

    #[test]
    fn tones_map_to_their_tokens() {
        let t = &theme::LIGHT;
        assert_eq!(Tone::Success.color(t), t.success);
        assert_eq!(Tone::Warning.color(t), t.warning);
        assert_eq!(Tone::Danger.color(t), t.destructive);
        assert_eq!(Tone::Neutral.color(t), t.muted_foreground);
    }
}
