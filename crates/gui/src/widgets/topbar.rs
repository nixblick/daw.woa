//! Einheitliche Kopfleisten-Pille (André 31.10.2026): MIDI, ON AIR, KI, Modus-Umschalter und
//! Feedback haben eine Höhe, eine Schrift, eine Symbolgröße, einen Radius und einen Innenabstand.
//! Rot nur für ON AIR, wenn live; der aktive Modus in der Akzentfarbe. Kein Glow (TopBar-README).
//! Auf schmalem Fenster zeigt jede Pille nur ihr Symbol; das Wort steht dann im Tooltip und
//! bleibt für Bildschirmleser die Beschriftung.

use eframe::egui::{self, Color32, CornerRadius, Response, Sense, Stroke, StrokeKind};

use crate::theme;

/// Höhe jedes Elements oben rechts.
pub const TOPBAR_ITEM_H: f32 = 24.0;
/// Symbolgröße.
pub const TOPBAR_ICON: f32 = 14.0;
/// Schriftgröße des Worts.
pub const TOPBAR_FONT: f32 = 12.0;
/// Innenabstand links und rechts.
pub const TOPBAR_PAD_X: f32 = theme::SPACE_3;
/// Abstand zwischen Symbol und Wort.
pub const TOPBAR_ICON_GAP: f32 = theme::SPACE_2;
/// Radius jeder Pille.
pub const TOPBAR_RADIUS: u8 = theme::RADIUS_LG;
/// Abstand zwischen Pillen einer Gruppe.
pub const TOPBAR_GAP: f32 = theme::SPACE_5;
/// Abstand zwischen den Segmenten des Modus-Umschalters.
pub const TOPBAR_SEGMENT_GAP: f32 = theme::SPACE_2;
/// Abstand zwischen den Gruppen (Status, Modus, Feedback).
pub const TOPBAR_GROUP_GAP: f32 = theme::SPACE_6;
/// Unter dieser freien Breite zeigen die Pillen nur Symbole.
pub const TOPBAR_COMPACT_BELOW: f32 = 760.0;

/// Aussehen einer Pille.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChipLook {
    /// Ruhig: Vertiefung, gedämpfte Schrift.
    Quiet,
    /// Leuchtet getönt in der Farbe (Status an).
    Lit(Color32),
    /// Voll gefüllt in der Farbe (aktiver Modus, ON AIR live, Feedback).
    Solid(Color32),
}

/// Eine Pille „Symbol Wort“. `compact` blendet das Wort aus. `click` macht sie zum Knopf.
pub fn topbar_chip(
    ui: &mut egui::Ui,
    icon: &str,
    word: &str,
    look: ChipLook,
    compact: bool,
    click: bool,
) -> Response {
    let dark = theme::is_dark();
    let (fill, stroke, ink) = match look {
        ChipLook::Quiet => (theme::well(), theme::border(), theme::text_dim()),
        ChipLook::Lit(c) => (
            theme::mix(c, theme::well(), if dark { 0.72 } else { 0.82 }),
            c,
            if dark {
                theme::mix(c, Color32::WHITE, 0.35)
            } else {
                theme::mix(c, Color32::BLACK, 0.25)
            },
        ),
        ChipLook::Solid(c) => (c, theme::emphasize(c, 0.25), Color32::WHITE),
    };
    let icon_g = ui.painter().layout_no_wrap(
        icon.to_string(),
        egui::FontId::proportional(TOPBAR_ICON),
        ink,
    );
    let word_g = (!compact).then(|| {
        ui.painter()
            .layout_no_wrap(word.to_string(), theme::label_font(TOPBAR_FONT), ink)
    });
    let w = TOPBAR_PAD_X * 2.0
        + icon_g.size().x
        + word_g
            .as_ref()
            .map_or(0.0, |g| TOPBAR_ICON_GAP + g.size().x);
    let sense = if click {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, TOPBAR_ITEM_H), sense);
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let r = CornerRadius::same(TOPBAR_RADIUS);
        let fill = if click && resp.hovered() {
            theme::emphasize(fill, 0.08)
        } else {
            fill
        };
        p.rect_filled(rect, r, fill);
        p.rect_stroke(rect, r, Stroke::new(1.0, stroke), StrokeKind::Inside);
        let mut x = rect.left() + TOPBAR_PAD_X;
        let cy = rect.center().y;
        p.galley(
            egui::pos2(x, cy - icon_g.size().y / 2.0),
            icon_g.clone(),
            ink,
        );
        x += icon_g.size().x + TOPBAR_ICON_GAP;
        if let Some(g) = word_g {
            p.galley(egui::pos2(x, cy - g.size().y / 2.0), g, ink);
        }
    }
    let kind = if click {
        egui::WidgetType::Button
    } else {
        egui::WidgetType::Label
    };
    let label = word.to_string();
    resp.widget_info(|| egui::WidgetInfo::labeled(kind, true, label.clone()));
    resp
}
