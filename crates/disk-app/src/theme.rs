//! The "Soft Sky" visual system: a clean, minimal, pastel theme — plus font
//! loading so Hebrew filenames and symbol icons actually render (egui's default
//! font covers neither).

use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, FontId, Rounding, Stroke, TextStyle,
    Visuals,
};

// ── Soft Sky palette ────────────────────────────────────────────────────────
/// Window/treemap background (near-white, faintly cool).
pub const BG: Color32 = Color32::from_rgb(0xFB, 0xFC, 0xFE);
/// Chrome surfaces (toolbar, bars).
pub const PANEL: Color32 = Color32::from_rgb(0xF2, 0xF5, 0xFA);
/// Primary text.
pub const TEXT: Color32 = Color32::from_rgb(0x2B, 0x2F, 0x36);
/// Soft pastel accent (system-blue, lightened).
pub const ACCENT: Color32 = Color32::from_rgb(0x7F, 0xB3, 0xFF);
/// Deeper accent for outlines/links where contrast matters.
pub const ACCENT_DEEP: Color32 = Color32::from_rgb(0x4F, 0x90, 0xE8);
/// Hairline borders.
pub const BORDER: Color32 = Color32::from_rgb(0xE3, 0xE8, 0xEF);
/// Soft coral for destructive actions (kept gentle to fit the palette).
pub const DANGER: Color32 = Color32::from_rgb(0xE8, 0x80, 0x6B);
/// Compare-mode "added" / "removed" tones.
pub const ADDED: Color32 = Color32::from_rgb(0x5F, 0xB9, 0x8C);
pub const REMOVED: Color32 = Color32::from_rgb(0x6F, 0xA8, 0xD8);

/// Pastel treemap tiles, cycled by position.
pub const TILES: [Color32; 6] = [
    Color32::from_rgb(0xAE, 0xC8, 0xF0), // powder blue
    Color32::from_rgb(0xBF, 0xE3, 0xD0), // mint
    Color32::from_rgb(0xD9, 0xC7, 0xF0), // lavender
    Color32::from_rgb(0xFF, 0xD9, 0xC2), // peach
    Color32::from_rgb(0xFB, 0xE7, 0xA8), // butter
    Color32::from_rgb(0xC9, 0xE4, 0xF2), // sky
];
/// Readable text over the (always-light) pastel tiles.
pub const TILE_TEXT: Color32 = Color32::from_rgb(0x29, 0x32, 0x3D);

/// Apply fonts and the full style. Call once at startup.
pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(13.0, 7.0);
    style.text_styles = [
        (
            TextStyle::Heading,
            FontId::new(18.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(13.5, FontFamily::Proportional),
        ),
        (
            TextStyle::Small,
            FontId::new(11.5, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(12.5, FontFamily::Monospace),
        ),
    ]
    .into();
    style.visuals = build_visuals();
    ctx.set_style(style);
}

/// Load a Unicode-rich system font (Hebrew + symbols) as the primary
/// proportional face, keeping egui's bundled fonts as fallbacks.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let candidates = [
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        "/Library/Fonts/Arial Unicode.ttf",
        "/System/Library/Fonts/Supplemental/Tahoma.ttf",
    ];
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert("unicode".to_owned(), FontData::from_owned(bytes).into());
            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .insert(0, "unicode".to_owned());
            fonts
                .families
                .entry(FontFamily::Monospace)
                .or_default()
                .push("unicode".to_owned());
            break;
        }
    }
    ctx.set_fonts(fonts);
}

fn build_visuals() -> Visuals {
    let mut v = Visuals::light();
    v.override_text_color = Some(TEXT);
    v.panel_fill = PANEL;

    v.window_fill = Color32::WHITE;
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.window_rounding = Rounding::same(12.0);

    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);

    v.widgets.inactive.bg_fill = Color32::from_rgb(0xEA, 0xEF, 0xF7);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(0xEA, 0xEF, 0xF7);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.inactive.rounding = Rounding::same(8.0);

    v.widgets.hovered.bg_fill = Color32::from_rgb(0xE2, 0xEA, 0xF6);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0xE2, 0xEA, 0xF6);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.hovered.rounding = Rounding::same(8.0);

    v.widgets.active.bg_fill = ACCENT;
    v.widgets.active.weak_bg_fill = ACCENT;
    v.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    v.widgets.active.rounding = Rounding::same(8.0);

    v.selection.bg_fill = Color32::from_rgba_unmultiplied(0x7F, 0xB3, 0xFF, 90);
    v.selection.stroke = Stroke::new(1.0, ACCENT_DEEP);
    v.hyperlink_color = ACCENT_DEEP;
    v
}
