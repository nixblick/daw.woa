//! VU-Meter des Produzenten-Modus: analoges Zeigerinstrument mit hinterleuchtetem
//! Skalenblatt oder moderner LED-Balken. Die Pegelansicht ist umschaltbar (Analog, Modern,
//! Aus) und steht in den Einstellungen.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2,
};
use serde::{Deserialize, Serialize};

use crate::theme;

/// Wie der Pegel im Produzenten-Modus gezeigt wird.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VuView {
    #[default]
    Analog,
    Modern,
    Aus,
}

impl VuView {
    pub const ALL: [VuView; 3] = [VuView::Analog, VuView::Modern, VuView::Aus];

    pub fn label(self) -> &'static str {
        match self {
            VuView::Analog => "Analog (VU)",
            VuView::Modern => "Modern (Balken)",
            VuView::Aus => "Aus",
        }
    }
}

/// Untere und obere Skalengrenze des Zeigers in dB.
pub const VU_MIN_DB: f32 = -20.0;
pub const VU_MAX_DB: f32 = 3.0;
/// Halber Ausschlagwinkel des Zeigers in Grad (±50° um die Senkrechte).
pub const VU_SWEEP_DEG: f32 = 50.0;
/// Beschriftete Skalenstriche in dB.
pub const VU_TICKS: [f32; 7] = [-20.0, -10.0, -7.0, -5.0, -3.0, 0.0, 3.0];
/// Anzeigezeit des VU-Zeigers (Integrationszeit nach IEC 60268-17: 300 ms).
pub const VU_TAU_S: f32 = 0.3;

/// Pegel (linear) in dB, nach unten bei −120 begrenzt.
pub fn lin_to_db(level: f32) -> f32 {
    20.0 * level.max(1e-6).log10()
}

/// Winkel des Zeigers in Radiant gegen die Senkrechte (links negativ) für `db`.
pub fn db_to_angle(db: f32) -> f32 {
    let t = ((db - VU_MIN_DB) / (VU_MAX_DB - VU_MIN_DB)).clamp(0.0, 1.0);
    (-VU_SWEEP_DEG + t * 2.0 * VU_SWEEP_DEG).to_radians()
}

/// Farbzone einer LED oder Skala bei `db`: bis −12 Grün, bis −3 Gelb, darüber Rot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedZone {
    Green,
    Yellow,
    Red,
}

pub fn zone_for_db(db: f32) -> LedZone {
    if db > -3.0 {
        LedZone::Red
    } else if db > -12.0 {
        LedZone::Yellow
    } else {
        LedZone::Green
    }
}

/// Träge Anzeige: nähert `shown` in `dt` Sekunden dem Ziel `target` an (Zeitkonstante
/// [`VU_TAU_S`]). Überschwingt nie.
pub fn ballistics(shown: f32, target: f32, dt: f32) -> f32 {
    let k = 1.0 - (-dt.max(0.0) / VU_TAU_S).exp();
    shown + (target - shown) * k
}

/// Format der Messwertzeile: echtes Minuszeichen, eine Nachkommastelle.
pub fn fmt_db(db: f32) -> String {
    if db <= -99.0 {
        "−∞ dB".to_string()
    } else if db < 0.0 {
        format!("−{:.1} dB", -db)
    } else {
        format!("+{db:.1} dB")
    }
}

fn zone_color(z: LedZone) -> Color32 {
    match z {
        LedZone::Green => theme::led_green(),
        LedZone::Yellow => theme::led_yellow(),
        LedZone::Red => theme::led_red(),
    }
}

/// Zeigt den Pegel `level` (linear, 1.0 = 0 dBFS) in der gewählten Ansicht. `id` hält den
/// Zustand des Zeigers zwischen den Bildern. Bei `VuView::Aus` wird nichts belegt.
pub fn vu_meter(ui: &mut egui::Ui, id: egui::Id, view: VuView, size: Vec2, level: f32) {
    match view {
        VuView::Aus => {}
        VuView::Analog => analog(ui, id, size, level),
        VuView::Modern => modern(ui, size, level),
    }
}

fn analog(ui: &mut egui::Ui, id: egui::Id, size: Vec2, level: f32) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let target = lin_to_db(level).clamp(VU_MIN_DB, VU_MAX_DB);
    let animate = super::motion(ui.ctx());
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    let shown = ui.data_mut(|d| {
        let s = d.get_temp_mut_or(id, target);
        *s = if animate {
            ballistics(*s, target, dt)
        } else {
            target
        };
        *s
    });
    if animate && (shown - target).abs() > 0.05 {
        ui.ctx().request_repaint();
    }
    let p = ui.painter().with_clip_rect(rect);
    let r = CornerRadius::same(theme::RADIUS_SM);
    p.rect_filled(rect, r, theme::vu_face());
    p.rect_stroke(
        rect,
        r,
        Stroke::new(1.0, theme::border_strong()),
        StrokeKind::Inside,
    );
    let ink = theme::vu_ink();
    // Drehpunkt unter dem Blatt; der Bogen füllt die Breite.
    let pivot = Pos2::new(rect.center().x, rect.bottom() + rect.height() * 0.28);
    let radius = (rect.height() * 1.18).min(rect.width() * 0.62);
    let at = |db: f32, r: f32| {
        let a = db_to_angle(db);
        Pos2::new(pivot.x + a.sin() * r, pivot.y - a.cos() * r)
    };
    // Rote Zone 0 … +3 dB als dicker Bogen, der Rest als feine Linie.
    let arc = |from: f32, to: f32, r: f32, stroke: Stroke| {
        let n = 24;
        let pts: Vec<Pos2> = (0..=n)
            .map(|i| at(from + (to - from) * i as f32 / n as f32, r))
            .collect();
        p.add(egui::Shape::line(pts, stroke));
    };
    arc(VU_MIN_DB, 0.0, radius, Stroke::new(1.0, ink));
    arc(0.0, VU_MAX_DB, radius, Stroke::new(3.0, theme::on_air()));
    for db in VU_TICKS {
        let major = matches!(db as i32, -20 | -10 | -5 | 0 | 3);
        let len = if major { 8.0 } else { 5.0 };
        let col = if db >= 0.0 { theme::on_air() } else { ink };
        p.line_segment(
            [at(db, radius), at(db, radius - len)],
            Stroke::new(1.0, col),
        );
        if major {
            p.text(
                at(db, radius - len - 6.0),
                Align2::CENTER_CENTER,
                if db > 0.0 {
                    format!("+{db:.0}")
                } else {
                    format!("{db:.0}").replace('-', "−")
                },
                FontId::proportional(9.0),
                col,
            );
        }
    }
    p.text(
        Pos2::new(rect.center().x, rect.bottom() - 7.0),
        Align2::CENTER_CENTER,
        "VU",
        theme::label_font(12.0),
        ink,
    );
    // Zeiger.
    let tip = at(shown, radius * 0.96);
    p.line_segment([pivot, tip], Stroke::new(1.5, ink));
    resp.on_hover_text(format!("Pegel {}", fmt_db(lin_to_db(level))));
}

fn modern(ui: &mut egui::Ui, size: Vec2, level: f32) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let db = lin_to_db(level);
    let p = ui.painter();
    let r = CornerRadius::same(theme::RADIUS_SM);
    p.rect_filled(rect, r, theme::lcd_bg());
    p.rect_stroke(
        rect,
        r,
        Stroke::new(1.0, theme::border()),
        StrokeKind::Inside,
    );
    let inner = Rect::from_min_max(
        rect.min + egui::vec2(4.0, 4.0),
        Pos2::new(rect.max.x - 4.0, rect.max.y - 16.0),
    );
    // Balken −60 … 0 dB, Farbe nach Zone der Spitze.
    let frac = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
    let bar_h = inner.height().max(2.0);
    for (from, to) in [(-60.0f32, -12.0f32), (-12.0, -3.0), (-3.0, 0.0)] {
        let f0 = (from + 60.0) / 60.0;
        let f1 = (to + 60.0) / 60.0;
        let seg = Rect::from_min_max(
            Pos2::new(inner.left() + f0 * inner.width(), inner.top()),
            Pos2::new(inner.left() + f1 * inner.width(), inner.top() + bar_h),
        );
        let z = zone_for_db(to - 0.01);
        p.rect_filled(seg, 1.0, theme::mix(zone_color(z), theme::lcd_bg(), 0.86));
        let lit_to = f1.min(frac);
        if lit_to > f0 {
            let lit = Rect::from_min_max(
                seg.min,
                Pos2::new(inner.left() + lit_to * inner.width(), seg.max.y),
            );
            p.rect_filled(lit, 1.0, zone_color(z));
        }
    }
    p.text(
        Pos2::new(rect.right() - 5.0, rect.bottom() - 8.0),
        Align2::RIGHT_CENTER,
        fmt_db(db),
        FontId::monospace(10.5),
        theme::lcd_text(),
    );
    p.text(
        Pos2::new(rect.left() + 5.0, rect.bottom() - 8.0),
        Align2::LEFT_CENTER,
        "PEGEL",
        theme::label_font(11.0),
        theme::lcd_text(),
    );
    resp.on_hover_text("Pegel der Summe");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needle_angle_spans_the_sweep() {
        assert!((db_to_angle(VU_MIN_DB) + VU_SWEEP_DEG.to_radians()).abs() < 1e-6);
        assert!((db_to_angle(VU_MAX_DB) - VU_SWEEP_DEG.to_radians()).abs() < 1e-6);
        // Mitte der Skala: (−20 + 3) / 2 = −8,5 dB steht senkrecht.
        assert!(db_to_angle(-8.5).abs() < 1e-5);
        // Außerhalb begrenzt, nie rückwärts.
        assert_eq!(db_to_angle(-90.0), db_to_angle(VU_MIN_DB));
        assert_eq!(db_to_angle(30.0), db_to_angle(VU_MAX_DB));
        assert!(db_to_angle(-6.0) < db_to_angle(-3.0));
    }

    #[test]
    fn led_zones_switch_at_minus_12_and_minus_3() {
        assert_eq!(zone_for_db(-40.0), LedZone::Green);
        assert_eq!(zone_for_db(-12.0), LedZone::Green);
        assert_eq!(zone_for_db(-11.9), LedZone::Yellow);
        assert_eq!(zone_for_db(-3.0), LedZone::Yellow);
        assert_eq!(zone_for_db(-2.9), LedZone::Red);
        assert_eq!(zone_for_db(0.0), LedZone::Red);
    }

    #[test]
    fn ballistics_approaches_without_overshoot() {
        let mut s = 0.0f32;
        let mut last = s;
        for _ in 0..100 {
            s = ballistics(s, 1.0, 0.016);
            assert!(s >= last && s <= 1.0);
            last = s;
        }
        assert!(s > 0.9);
        // Nach 300 ms rund 63 % des Weges.
        let t = ballistics(0.0, 1.0, VU_TAU_S);
        assert!((t - 0.632).abs() < 0.01);
        assert_eq!(ballistics(0.4, 0.4, 0.05), 0.4);
    }

    #[test]
    fn levels_convert_to_db_with_a_floor() {
        assert!((lin_to_db(1.0)).abs() < 1e-6);
        assert!((lin_to_db(0.5) + 6.0206).abs() < 1e-3);
        assert!(lin_to_db(0.0) <= -119.0);
    }

    #[test]
    fn db_text_uses_the_real_minus_sign() {
        assert_eq!(fmt_db(-14.12), "−14.1 dB");
        assert_eq!(fmt_db(1.94), "+1.9 dB");
        assert_eq!(fmt_db(-120.0), "−∞ dB");
    }

    #[test]
    fn view_setting_roundtrips_and_defaults_to_analog() {
        assert_eq!(VuView::default(), VuView::Analog);
        for v in VuView::ALL {
            let s = serde_json::to_string(&v).unwrap();
            assert_eq!(serde_json::from_str::<VuView>(&s).unwrap(), v);
        }
        assert_eq!(serde_json::to_string(&VuView::Aus).unwrap(), "\"aus\"");
    }
}
