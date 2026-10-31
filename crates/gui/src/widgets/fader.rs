//! Fader: Kanalfader (senkrecht) und Crossfader (waagerecht) mit geriffelter Kappe und
//! Modusfarben-Strich. Doppelklick setzt auf den Vorgabewert zurück.

use std::ops::RangeInclusive;

use eframe::egui::{
    self, Color32, CornerRadius, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Vec2,
};

use super::with_alpha;
use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orient {
    /// Kanalfader: oben = Maximum.
    Vertical,
    /// Crossfader: rechts = Maximum.
    Horizontal,
}

/// Wert → Stellung 0..1 (außerhalb des Bereichs begrenzt).
pub fn norm(value: f32, min: f32, max: f32) -> f32 {
    if max <= min {
        return 0.0;
    }
    ((value - min) / (max - min)).clamp(0.0, 1.0)
}

/// Stellung 0..1 → Wert.
pub fn denorm(frac: f32, min: f32, max: f32) -> f32 {
    min + frac.clamp(0.0, 1.0) * (max - min)
}

/// Stellung 0..1 aus der Zeigerposition im Schacht `track` (Kappenmitte folgt dem Zeiger).
pub fn frac_at(pos: Pos2, track: Rect, orient: Orient) -> f32 {
    match orient {
        Orient::Vertical => 1.0 - ((pos.y - track.top()) / track.height().max(1.0)).clamp(0.0, 1.0),
        Orient::Horizontal => ((pos.x - track.left()) / track.width().max(1.0)).clamp(0.0, 1.0),
    }
}

/// Fader mit Wert `value` im Bereich `range`; `default` ist das Ziel des Doppelklicks.
/// Liefert die Antwort; `changed()` ist gesetzt, wenn sich der Wert geändert hat.
pub fn fader(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: RangeInclusive<f32>,
    default: f32,
    size: Vec2,
    orient: Orient,
) -> Response {
    let (min, max) = (*range.start(), *range.end());
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let cap_len = 22.0;
    let cap_wid = match orient {
        Orient::Vertical => rect.width().min(30.0),
        Orient::Horizontal => rect.height().min(30.0),
    };
    // Weg der Kappenmitte.
    let track = match orient {
        Orient::Vertical => Rect::from_min_max(
            Pos2::new(rect.left(), rect.top() + cap_len / 2.0),
            Pos2::new(rect.right(), rect.bottom() - cap_len / 2.0),
        ),
        Orient::Horizontal => Rect::from_min_max(
            Pos2::new(rect.left() + cap_len / 2.0, rect.top()),
            Pos2::new(rect.right() - cap_len / 2.0, rect.bottom()),
        ),
    };
    let before = *value;
    if resp.double_clicked() {
        *value = default.clamp(min, max);
    } else if let Some(pos) = resp
        .interact_pointer_pos()
        .filter(|_| resp.dragged() || resp.clicked())
    {
        *value = denorm(frac_at(pos, track, orient), min, max);
    }
    if resp.has_focus() {
        let step = (max - min) / 100.0;
        let (dec, inc) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowUp) || i.key_pressed(egui::Key::ArrowRight),
            )
        });
        if dec {
            *value = (*value - step).clamp(min, max);
        }
        if inc {
            *value = (*value + step).clamp(min, max);
        }
    }
    if (*value - before).abs() > f32::EPSILON {
        resp.mark_changed();
    }

    let p = ui.painter();
    let frac = norm(*value, min, max);
    // Schacht: eingelassen.
    let slot = match orient {
        Orient::Vertical => {
            Rect::from_center_size(rect.center(), egui::vec2(6.0, rect.height() - 4.0))
        }
        Orient::Horizontal => {
            Rect::from_center_size(rect.center(), egui::vec2(rect.width() - 4.0, 6.0))
        }
    };
    p.rect_filled(slot, CornerRadius::same(theme::RADIUS_SM), theme::well());
    p.rect_stroke(
        slot,
        CornerRadius::same(theme::RADIUS_SM),
        Stroke::new(1.0, theme::border()),
        StrokeKind::Inside,
    );
    // Skalenstriche an den Enden und in der Mitte.
    for t in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
        let major = t == 0.0 || t == 0.5 || t == 1.0;
        let len = if major { 7.0 } else { 4.0 };
        let col = theme::text_dim();
        match orient {
            Orient::Vertical => {
                let y = track.bottom() - t * track.height();
                p.hline(
                    (slot.left() - len - 3.0)..=(slot.left() - 3.0),
                    y,
                    Stroke::new(1.0, col),
                );
                p.hline(
                    (slot.right() + 3.0)..=(slot.right() + len + 3.0),
                    y,
                    Stroke::new(1.0, col),
                );
            }
            Orient::Horizontal => {
                let x = track.left() + t * track.width();
                p.vline(
                    x,
                    (slot.top() - len - 3.0)..=(slot.top() - 3.0),
                    Stroke::new(1.0, col),
                );
                p.vline(
                    x,
                    (slot.bottom() + 3.0)..=(slot.bottom() + len + 3.0),
                    Stroke::new(1.0, col),
                );
            }
        }
    }
    // Kappe.
    let center = match orient {
        Orient::Vertical => Pos2::new(rect.center().x, track.bottom() - frac * track.height()),
        Orient::Horizontal => Pos2::new(track.left() + frac * track.width(), rect.center().y),
    };
    let cap = match orient {
        Orient::Vertical => Rect::from_center_size(center, egui::vec2(cap_wid, cap_len)),
        Orient::Horizontal => Rect::from_center_size(center, egui::vec2(cap_len, cap_wid)),
    };
    let held = resp.dragged();
    let body = if held {
        theme::surface_hover()
    } else {
        theme::surface()
    };
    let r = CornerRadius::same(theme::RADIUS_SM);
    if resp.has_focus() {
        p.rect_stroke(
            cap.expand(2.0),
            r,
            Stroke::new(1.5, theme::focus()),
            StrokeKind::Outside,
        );
    }
    p.rect_filled(
        cap.translate(egui::vec2(0.0, 1.5)),
        r,
        with_alpha(Color32::BLACK, 110),
    );
    p.rect_filled(cap, r, body);
    p.rect_stroke(
        cap,
        r,
        Stroke::new(1.0, theme::border_strong()),
        StrokeKind::Inside,
    );
    // Riffelung quer zur Bewegung, Modusfarben-Strich in der Mitte.
    let groove = Stroke::new(1.0, with_alpha(Color32::BLACK, 90));
    let hi = Stroke::new(1.0, with_alpha(Color32::WHITE, 28));
    match orient {
        Orient::Vertical => {
            for k in [-7.0f32, -4.0, 4.0, 7.0] {
                let y = cap.center().y + k;
                p.hline((cap.left() + 3.0)..=(cap.right() - 3.0), y, groove);
                p.hline((cap.left() + 3.0)..=(cap.right() - 3.0), y + 1.0, hi);
            }
            p.hline(
                (cap.left() + 2.0)..=(cap.right() - 2.0),
                cap.center().y,
                Stroke::new(2.0, theme::accent()),
            );
        }
        Orient::Horizontal => {
            for k in [-7.0f32, -4.0, 4.0, 7.0] {
                let x = cap.center().x + k;
                p.vline(x, (cap.top() + 3.0)..=(cap.bottom() - 3.0), groove);
                p.vline(x + 1.0, (cap.top() + 3.0)..=(cap.bottom() - 3.0), hi);
            }
            p.vline(
                cap.center().x,
                (cap.top() + 2.0)..=(cap.bottom() - 2.0),
                Stroke::new(2.0, theme::accent()),
            );
        }
    }
    resp.widget_info(|| egui::WidgetInfo::slider(true, f64::from(*value), ""));
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_and_denorm_roundtrip_and_clamp() {
        assert_eq!(norm(-60.0, -60.0, 12.0), 0.0);
        assert_eq!(norm(12.0, -60.0, 12.0), 1.0);
        assert!((denorm(norm(-6.0, -60.0, 12.0), -60.0, 12.0) + 6.0).abs() < 1e-4);
        assert_eq!(norm(99.0, 0.0, 1.0), 1.0);
        assert_eq!(norm(1.0, 2.0, 2.0), 0.0);
        assert_eq!(denorm(2.0, 0.0, 10.0), 10.0);
    }

    #[test]
    fn pointer_position_maps_to_fader_position() {
        let t = Rect::from_min_max(Pos2::new(0.0, 10.0), Pos2::new(20.0, 110.0));
        assert_eq!(frac_at(Pos2::new(5.0, 10.0), t, Orient::Vertical), 1.0);
        assert_eq!(frac_at(Pos2::new(5.0, 110.0), t, Orient::Vertical), 0.0);
        assert_eq!(frac_at(Pos2::new(5.0, 60.0), t, Orient::Vertical), 0.5);
        assert_eq!(frac_at(Pos2::new(0.0, 60.0), t, Orient::Horizontal), 0.0);
        assert_eq!(frac_at(Pos2::new(20.0, 60.0), t, Orient::Horizontal), 1.0);
        assert_eq!(frac_at(Pos2::new(500.0, 60.0), t, Orient::Horizontal), 1.0);
    }
}
