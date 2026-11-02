//! ON-AIR-Lampe wie über der Studiotür: aus = dunkles Rotglas, an = leuchtend mit Glow.
//! Die Lampe sitzt in einem schwarzen Rahmen, die Schrift ohne Schatten.

use eframe::egui::{self, Align2, Color32, CornerRadius, Sense, Stroke, StrokeKind, Vec2};

use super::with_alpha;
use crate::theme;

/// Füll- und Schriftfarbe der Lampe.
pub fn lamp_colors(on: bool) -> (Color32, Color32) {
    if on {
        (theme::on_air_fill(), Color32::WHITE)
    } else {
        (
            theme::mix(theme::on_air_fill(), Color32::BLACK, 0.78),
            theme::mix(theme::on_air_fill(), Color32::BLACK, 0.45),
        )
    }
}

/// Zustand der Lampe in der Kopfleiste (`woa-onair--bar`, Fassung 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LampState {
    /// Nicht auf Sendung: dunkles Rotglas.
    Off,
    /// Auf Sendung: leuchtet mit Glut wie im Sende-Deck.
    On,
    /// Auf Sendung im Pult-Modus (#301, die Sendung kommt vom Mischpult): 72 % Füllung,
    /// kleiner Schein.
    Dim,
}

/// Zustand aus „auf Sendung“ und Pult-Modus.
pub fn onair_state(live: bool, pult: bool) -> LampState {
    match (live, pult) {
        (false, _) => LampState::Off,
        (true, false) => LampState::On,
        (true, true) => LampState::Dim,
    }
}

/// Füll- und Schriftfarbe je Zustand.
pub fn bar_colors(state: LampState) -> (Color32, Color32) {
    match state {
        LampState::Off => lamp_colors(false),
        LampState::On => lamp_colors(true),
        LampState::Dim => (
            theme::mix(theme::on_air_fill(), lamp_colors(false).0, 0.72),
            Color32::WHITE,
        ),
    }
}

/// Lampe in der Kopfleiste (`woa-onair--bar`): 28 px hoch, Rahmen 3 px schwarz, Barlow
/// Condensed 14, Glut außen wie im Sende-Deck (gedimmt: kleiner Schein). Klickbar.
pub fn onair_bar(ui: &mut egui::Ui, state: LampState) -> egui::Response {
    let font = theme::label_font(14.0);
    let g = ui
        .painter()
        .layout_no_wrap("ON AIR".into(), font.clone(), Color32::WHITE);
    let size = egui::vec2(g.size().x + 2.0 * 14.0, 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();
    let (fill, ink) = bar_colors(state);
    let glow: &[(f32, u8)] = match state {
        LampState::On => &[(6.0, 18), (3.5, 34), (1.5, 60)],
        LampState::Dim => &[(2.0, 30)],
        LampState::Off => &[],
    };
    for &(grow, a) in glow {
        p.rect_filled(
            rect.expand(grow),
            CornerRadius::same(theme::RADIUS_SM + 3),
            with_alpha(theme::on_air(), a),
        );
    }
    p.rect_filled(rect, CornerRadius::same(theme::RADIUS_SM), Color32::BLACK);
    p.rect_filled(rect.shrink(3.0), CornerRadius::same(2), fill);
    p.text(rect.center(), Align2::CENTER_CENTER, "ON AIR", font, ink);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Zeichnet die Lampe in `size`; `on` leuchtet sie.
pub fn onair_lamp(ui: &mut egui::Ui, size: Vec2, on: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter();
    let (fill, ink) = lamp_colors(on);
    let r = CornerRadius::same(theme::RADIUS_SM);
    if on {
        for (grow, a) in [(7.0, 18u8), (4.0, 34), (2.0, 60)] {
            p.rect_filled(
                rect.expand(grow),
                CornerRadius::same(theme::RADIUS_SM + 4),
                with_alpha(theme::on_air(), a),
            );
        }
    }
    p.rect_filled(rect, r, Color32::BLACK);
    let glass = rect.shrink(2.5);
    p.rect_filled(glass, CornerRadius::same(2), fill);
    if on {
        // Glanz oben auf dem Glas, keine Verläufe als Dekoration.
        let shine =
            egui::Rect::from_min_size(glass.min, egui::vec2(glass.width(), glass.height() * 0.28));
        p.rect_filled(shine, CornerRadius::same(2), with_alpha(Color32::WHITE, 28));
    }
    p.rect_stroke(
        rect,
        r,
        Stroke::new(1.0, theme::border_strong()),
        StrokeKind::Inside,
    );
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        "ON AIR",
        theme::label_font((size.y * 0.5).clamp(11.0, 26.0)),
        ink,
    );
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Label,
            true,
            if on { "ON AIR an" } else { "ON AIR aus" },
        )
    });
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_lamp_is_brighter_than_dark_glass() {
        let (on_fill, on_ink) = lamp_colors(true);
        let (off_fill, off_ink) = lamp_colors(false);
        let lum = |c: Color32| u32::from(c.r()) + u32::from(c.g()) + u32::from(c.b());
        assert!(lum(on_fill) > lum(off_fill));
        assert!(lum(on_ink) > lum(off_ink));
        assert_eq!(on_ink, Color32::WHITE);
    }

    #[test]
    fn bar_lamp_is_off_on_or_dimmed_in_pult_mode() {
        assert_eq!(onair_state(false, false), LampState::Off);
        assert_eq!(onair_state(false, true), LampState::Off);
        assert_eq!(onair_state(true, false), LampState::On);
        assert_eq!(onair_state(true, true), LampState::Dim);
        let lum = |c: Color32| u32::from(c.r()) + u32::from(c.g()) + u32::from(c.b());
        let (off, _) = bar_colors(LampState::Off);
        let (dim, dim_ink) = bar_colors(LampState::Dim);
        let (on, on_ink) = bar_colors(LampState::On);
        assert_eq!(on, lamp_colors(true).0, "gleiche Glut wie im Sende-Deck");
        assert!(
            lum(off) < lum(dim) && lum(dim) < lum(on),
            "gedimmt liegt dazwischen"
        );
        assert_eq!(on_ink, Color32::WHITE);
        assert_eq!(dim_ink, Color32::WHITE);
    }
}
