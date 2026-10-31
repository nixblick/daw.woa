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
}
