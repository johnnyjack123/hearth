//! The web app's design tokens (`apps/web/src/styles/tokens.css`) as egui
//! visuals, so the desktop app looks like the chat: the same neutral greys,
//! the iris brand color, and the same status colors, light and dark.

use eframe::egui::{self, Color32, CornerRadius, Margin, Shadow, Stroke, Theme, Visuals};

/// One theme's semantic colors, named like the web tokens.
#[derive(Clone, Copy)]
pub struct Tokens {
    /// Page background.
    pub background: Color32,
    /// Text.
    pub foreground: Color32,
    /// Cards and panels.
    pub card: Color32,
    /// Quiet surfaces (code, inputs).
    pub muted: Color32,
    /// Secondary text.
    pub muted_foreground: Color32,
    /// Hover.
    pub accent: Color32,
    /// Lines.
    pub border: Color32,
    /// Brand color of buttons and selection.
    pub primary: Color32,
    /// Text on the brand color.
    pub primary_foreground: Color32,
    /// Something failed or is dangerous.
    pub destructive: Color32,
    /// Done, connected.
    pub success: Color32,
    /// Waits for the user.
    pub warning: Color32,
}

/// Converts `#rrggbb` written as a number.
const fn hex(value: u32) -> Color32 {
    Color32::from_rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

/// Light theme (`:root` in tokens.css).
pub const LIGHT: Tokens = Tokens {
    background: hex(0xfafafb),
    foreground: hex(0x0b0c10),
    card: hex(0xffffff),
    muted: hex(0xf3f4f7),
    muted_foreground: hex(0x565860),
    accent: hex(0xecedf0),
    border: hex(0xe2e3e7),
    primary: hex(0x5453e1),
    primary_foreground: hex(0xffffff),
    destructive: hex(0xcd1526),
    success: hex(0x008246),
    warning: hex(0xeba002),
};

/// Dark theme (`.dark` in tokens.css).
pub const DARK: Tokens = Tokens {
    background: hex(0x0b0c10),
    foreground: hex(0xfafafb),
    card: hex(0x16171c),
    muted: hex(0x1c1e23),
    muted_foreground: hex(0x9fa1a9),
    accent: hex(0x1c1e23),
    border: hex(0x26272e),
    primary: hex(0x8893fa),
    primary_foreground: hex(0x101230),
    destructive: hex(0xf3625d),
    success: hex(0x43c07a),
    warning: hex(0xf7b83d),
};

/// The tokens of the theme `ui` shows.
pub fn tokens(ctx: &egui::Context) -> Tokens {
    if ctx.global_style().visuals.dark_mode {
        DARK
    } else {
        LIGHT
    }
}

/// Corner radius of cards (`--radius`, 0.75 rem).
pub const RADIUS: u8 = 12;
/// Corner radius of buttons and inputs.
pub const CONTROL_RADIUS: u8 = 8;

fn visuals(theme: Theme, t: Tokens) -> Visuals {
    let mut v = match theme {
        Theme::Dark => Visuals::dark(),
        Theme::Light => Visuals::light(),
    };
    v.override_text_color = Some(t.foreground);
    v.weak_text_color = Some(t.muted_foreground);
    v.panel_fill = t.background;
    v.window_fill = t.card;
    v.window_stroke = Stroke::new(1.0, t.border);
    v.window_corner_radius = CornerRadius::same(RADIUS);
    v.menu_corner_radius = CornerRadius::same(CONTROL_RADIUS);
    v.window_shadow = Shadow {
        offset: [0, 6],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(if theme == Theme::Dark { 120 } else { 30 }),
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(if theme == Theme::Dark { 110 } else { 25 }),
    };
    v.faint_bg_color = t.muted;
    v.extreme_bg_color = t.muted;
    v.code_bg_color = t.muted;
    v.hyperlink_color = t.primary;
    v.warn_fg_color = t.warning;
    v.error_fg_color = t.destructive;
    v.selection.bg_fill = t.primary.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, t.primary);
    let radius = CornerRadius::same(CONTROL_RADIUS);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = t.card;
    w.noninteractive.weak_bg_fill = t.card;
    w.noninteractive.bg_stroke = Stroke::new(1.0, t.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, t.foreground);
    w.noninteractive.corner_radius = radius;
    for (state, fill) in [
        (&mut w.inactive, t.muted),
        (&mut w.hovered, t.accent),
        (&mut w.active, t.accent),
        (&mut w.open, t.accent),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0, t.border);
        state.fg_stroke = Stroke::new(1.0, t.foreground);
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
    w.hovered.bg_stroke = Stroke::new(1.0, t.muted_foreground.gamma_multiply(0.5));
    w.active.bg_stroke = Stroke::new(1.0, t.primary);
    v
}

/// Sets both themes; egui follows the system's choice.
pub fn apply(ctx: &egui::Context) {
    for (theme, tokens) in [(Theme::Light, LIGHT), (Theme::Dark, DARK)] {
        ctx.set_visuals_of(theme, visuals(theme, tokens));
        ctx.style_mut_of(theme, |style| {
            style.spacing.item_spacing = egui::vec2(8.0, 8.0);
            style.spacing.button_padding = egui::vec2(12.0, 6.0);
            style.spacing.interact_size.y = 30.0;
            style.spacing.window_margin = Margin::same(16);
            style.spacing.menu_margin = Margin::same(6);
            for (text_style, size) in [
                (egui::TextStyle::Body, 14.0),
                (egui::TextStyle::Button, 14.0),
                (egui::TextStyle::Small, 12.0),
                (egui::TextStyle::Monospace, 13.0),
                (egui::TextStyle::Heading, 20.0),
            ] {
                if let Some(font) = style.text_styles.get_mut(&text_style) {
                    font.size = size;
                }
            }
        });
    }
}

/// A card: the web app's `Card`.
pub fn card(ctx: &egui::Context) -> egui::Frame {
    let t = tokens(ctx);
    egui::Frame::new()
        .fill(t.card)
        .stroke(Stroke::new(1.0, t.border))
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::same(14))
}

/// A primary button: brand color, light text.
pub fn primary_button(ctx: &egui::Context, text: &str) -> egui::Button<'static> {
    let t = tokens(ctx);
    egui::Button::new(
        egui::RichText::new(text.to_owned())
            .color(t.primary_foreground)
            .strong(),
    )
    .fill(t.primary)
    .stroke(Stroke::NONE)
}

/// A small colored label, like the web app's `Badge`.
pub fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.16))
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).small().color(color));
        });
}
