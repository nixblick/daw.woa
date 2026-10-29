//! Aussehen: dunkles Studio-Thema, eine Akzentfarbe, Spurfarben.

use eframe::egui;
use egui::{Color32, CornerRadius, FontId, Stroke, TextStyle, vec2};

/// Akzent (Wiedergabe, Kopf, Auswahl).
pub const ACCENT: Color32 = Color32::from_rgb(255, 145, 64);
/// Aufnahme, „On Air“.
pub const ON_AIR: Color32 = Color32::from_rgb(232, 62, 72);
/// Tiefster Hintergrund (Zeitleiste).
pub const BG: Color32 = Color32::from_rgb(22, 24, 29);
/// Leisten und Spurköpfe.
pub const PANEL: Color32 = Color32::from_rgb(31, 34, 41);
/// Knöpfe und Felder.
pub const SURFACE: Color32 = Color32::from_rgb(42, 46, 55);
/// Rasterlinien.
pub const GRID: Color32 = Color32::from_rgb(48, 53, 63);
/// Gedämpfte Schrift.
pub const DIM: Color32 = Color32::from_rgb(140, 148, 162);

const TRACK_COLORS: [Color32; 6] = [
    Color32::from_rgb(82, 156, 255),
    Color32::from_rgb(92, 204, 142),
    Color32::from_rgb(196, 128, 255),
    Color32::from_rgb(255, 196, 77),
    Color32::from_rgb(77, 208, 225),
    Color32::from_rgb(255, 120, 160),
];

/// Farbe der Spur Nummer `i` (wiederholt sich nach sechs).
pub fn track_color(i: usize) -> Color32 {
    TRACK_COLORS[i % TRACK_COLORS.len()]
}

/// Mischt `c` mit `with` (0 = c, 1 = with).
pub fn mix(c: Color32, with: Color32, t: f32) -> Color32 {
    let m = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
    Color32::from_rgb(m(c.r(), with.r()), m(c.g(), with.g()), m(c.b(), with.b()))
}

/// Setzt Farben, Abstände und Schriftgrößen.
pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        let v = &mut s.visuals;
        v.panel_fill = PANEL;
        v.window_fill = PANEL;
        v.extreme_bg_color = BG;
        v.faint_bg_color = SURFACE;
        v.selection.bg_fill = mix(ACCENT, BG, 0.45);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.hyperlink_color = ACCENT;
        v.window_corner_radius = CornerRadius::same(10);
        v.menu_corner_radius = CornerRadius::same(8);
        let r = CornerRadius::same(6);
        for (w, fill) in [
            (&mut v.widgets.noninteractive, PANEL),
            (&mut v.widgets.inactive, SURFACE),
            (&mut v.widgets.hovered, mix(SURFACE, Color32::WHITE, 0.08)),
            (&mut v.widgets.active, mix(SURFACE, ACCENT, 0.25)),
            (&mut v.widgets.open, mix(SURFACE, Color32::WHITE, 0.05)),
        ] {
            w.corner_radius = r;
            w.weak_bg_fill = fill;
            w.bg_fill = fill;
        }
        // Nicht bedienbare Flächen (Rahmen, Separatoren) bleiben dunkel.
        v.widgets.noninteractive.bg_fill = PANEL;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, GRID);
        v.widgets.inactive.bg_stroke = Stroke::NONE;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, mix(SURFACE, ACCENT, 0.5));

        s.spacing.item_spacing = vec2(8.0, 6.0);
        s.spacing.button_padding = vec2(10.0, 4.0);
        s.spacing.interact_size.y = 26.0;
        s.spacing.slider_width = 90.0;
        s.visuals.slider_trailing_fill = true;
        s.visuals.widgets.inactive.bg_fill = BG;
        s.text_styles
            .insert(TextStyle::Body, FontId::proportional(14.0));
        s.text_styles
            .insert(TextStyle::Button, FontId::proportional(14.0));
        s.text_styles
            .insert(TextStyle::Monospace, FontId::monospace(14.0));
        s.text_styles
            .insert(TextStyle::Small, FontId::proportional(11.0));
        s.text_styles
            .insert(TextStyle::Heading, FontId::proportional(20.0));
    });
}
