//! ZPadder's look: calm dark and light themes with one accent colour, rounded widgets, and depth
//! from panel shades rather than gradients.

use eframe::egui::{
    self, Color32, CornerRadius, Margin, RichText, Shadow, Stroke, Theme, Ui, Vec2, Visuals,
};

pub(crate) const ACCENT: Color32 = Color32::from_rgb(76, 132, 255);

/// Light theme panel and window backgrounds, which text colours are checked against.
const LIGHT_PANEL: Color32 = Color32::from_rgb(244, 245, 248);
const LIGHT_FAINT: Color32 = Color32::from_rgb(237, 239, 243);
const DARK_PANEL: Color32 = Color32::from_rgb(24, 26, 31);
const DARK_WINDOW: Color32 = Color32::from_rgb(30, 33, 40);

/// Text for things that are on and working, such as the layer active on the pad right now
/// (Xpadder lit its active set green too). Darker in the light theme, so it stays readable.
pub(crate) fn live(visuals: &Visuals) -> Color32 {
    if visuals.dark_mode { Color32::from_rgb(72, 199, 116) } else { Color32::from_rgb(22, 122, 62) }
}

/// Text for warnings and notices.
pub(crate) fn warn(visuals: &Visuals) -> Color32 {
    if visuals.dark_mode { Color32::from_rgb(240, 170, 60) } else { Color32::from_rgb(150, 88, 0) }
}

/// Text for errors and destructive actions.
pub(crate) fn error(visuals: &Visuals) -> Color32 {
    if visuals.dark_mode { Color32::from_rgb(235, 87, 87) } else { Color32::from_rgb(190, 34, 34) }
}

/// Text for mapping that is paused: quieter than on, but not as faint as off.
pub(crate) fn paused(visuals: &Visuals) -> Color32 {
    if visuals.dark_mode {
        Color32::from_rgb(150, 162, 182)
    } else {
        Color32::from_rgb(92, 102, 120)
    }
}

pub(crate) fn apply(ctx: &egui::Context) {
    fonts(ctx);
    ctx.style_mut_of(Theme::Dark, |style| {
        common(style);
        dark(&mut style.visuals);
    });
    ctx.style_mut_of(Theme::Light, |style| {
        common(style);
        light(&mut style.visuals);
    });
}

/// Uses Windows' own interface font (Segoe UI), with Segoe UI Symbol as a fallback for arrows
/// and symbols that egui's built-in fonts lack. Without them, egui's fonts are used.
fn fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let dir = std::env::var_os("WINDIR")
        .map_or_else(|| std::path::PathBuf::from(r"C:\Windows"), std::path::PathBuf::from)
        .join("Fonts");
    for (name, file, primary) in
        [("segoe-ui", "segoeui.ttf", true), ("segoe-symbol", "seguisym.ttf", false)]
    {
        let Ok(bytes) = std::fs::read(dir.join(file)) else { continue };
        fonts.font_data.insert(name.into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            let list = fonts.families.entry(family.clone()).or_default();
            if primary && family == egui::FontFamily::Proportional {
                list.insert(0, name.into());
            } else {
                list.push(name.into());
            }
        }
    }
    ctx.set_fonts(fonts);
}

fn common(style: &mut egui::Style) {
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(10.0, 4.0);
    style.spacing.interact_size.y = 24.0;
    let v = &mut style.visuals;
    for widget in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(6);
    }
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_shadow =
        Shadow { offset: [0, 6], blur: 24, spread: 0, color: Color32::from_black_alpha(90) };
    v.popup_shadow =
        Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(70) };
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.hyperlink_color = ACCENT;
    v.indent_has_left_vline = false;
}

fn dark(v: &mut Visuals) {
    v.panel_fill = DARK_PANEL;
    v.window_fill = DARK_WINDOW;
    v.extreme_bg_color = Color32::from_rgb(16, 17, 21);
    v.faint_bg_color = Color32::from_rgb(32, 35, 42);
    v.window_stroke = Stroke::new(1.0, Color32::from_rgb(50, 54, 64));
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(42, 46, 55));
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(40, 44, 53);
    v.widgets.inactive.bg_fill = Color32::from_rgb(40, 44, 53);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(52, 57, 69);
    v.widgets.hovered.bg_fill = Color32::from_rgb(52, 57, 69);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(62, 68, 82);
    v.widgets.active.bg_fill = Color32::from_rgb(62, 68, 82);
}

fn light(v: &mut Visuals) {
    v.panel_fill = LIGHT_PANEL;
    v.window_fill = Color32::WHITE;
    v.extreme_bg_color = Color32::WHITE;
    v.faint_bg_color = LIGHT_FAINT;
    v.window_stroke = Stroke::new(1.0, Color32::from_rgb(214, 218, 226));
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(222, 225, 232));
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(230, 232, 238);
    v.widgets.inactive.bg_fill = Color32::from_rgb(230, 232, 238);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(219, 223, 232);
    v.widgets.hovered.bg_fill = Color32::from_rgb(219, 223, 232);
}

/// Background behind the controller.
pub(crate) fn canvas_fill(visuals: &Visuals) -> Color32 {
    if visuals.dark_mode { Color32::from_rgb(19, 21, 25) } else { Color32::from_rgb(232, 235, 240) }
}

/// Background behind controller photos: lighter, so dark controllers stand out.
pub(crate) fn picture_canvas_fill(visuals: &Visuals) -> Color32 {
    if visuals.dark_mode { Color32::from_rgb(52, 57, 67) } else { Color32::from_rgb(222, 226, 233) }
}

fn card_fill(visuals: &Visuals) -> Color32 {
    if visuals.dark_mode { Color32::from_rgb(31, 34, 41) } else { Color32::WHITE }
}

/// A rounded card grouping related settings, with an optional title.
pub(crate) fn card<R>(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
    egui::Frame::new()
        .fill(card_fill(ui.visuals()))
        .stroke(stroke)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() {
                ui.label(RichText::new(title).strong());
                ui.add_space(2.0);
            }
            add(ui)
        })
        .inner
}

/// A small button showing only a symbol, such as ✖. `name` says what it does, as its tooltip and
/// as the name screen readers announce.
pub(crate) fn icon_button(ui: &mut Ui, enabled: bool, icon: &str, name: &str) -> egui::Response {
    let response = ui
        .add_enabled(enabled, egui::Button::new(icon).small())
        .on_hover_text(name)
        .on_disabled_hover_text(name);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, name));
    response
}

/// A link that opens `file` from the documentation installed with ZPadder, showing `text`. When
/// the documentation is not there, `text` is shown plainly.
pub(crate) fn docs_link(ui: &mut Ui, text: &str, file: &str) {
    match crate::files::docs_file(file) {
        Some(path) => {
            let response = ui.link(text).on_hover_text(path.display().to_string());
            if response.clicked()
                && let Err(e) = crate::files::open_in_shell(&path)
            {
                tracing::warn!("{e}");
            }
        }
        None => {
            ui.weak(text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG relative luminance.
    fn luminance(c: Color32) -> f64 {
        let channel = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn status_colours_are_readable_in_both_themes() {
        let light = Visuals::light();
        let dark = Visuals::dark();
        for (name, colour) in [
            ("live", live as fn(&Visuals) -> Color32),
            ("warn", warn),
            ("error", error),
            ("paused", paused),
        ] {
            for background in [LIGHT_PANEL, LIGHT_FAINT, Color32::WHITE] {
                let ratio = contrast(colour(&light), background);
                assert!(ratio >= 4.5, "light {name} on {background:?}: {ratio:.2}");
            }
            for background in [DARK_PANEL, DARK_WINDOW] {
                let ratio = contrast(colour(&dark), background);
                assert!(ratio >= 4.5, "dark {name} on {background:?}: {ratio:.2}");
            }
        }
    }
}
