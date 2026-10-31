//! Studio-Uhr des Radio-Modus: 60 Sekunden-LEDs im Kreis, zwölf Stundenpunkte und die
//! Digitalzeit in DSEG7 in der Mitte. Wie echte Hardware steht sie auf `lcd_bg`.

use eframe::egui::{self, Align2, Pos2, Sense, Stroke};

use super::with_alpha;
use crate::theme;

/// Brennt die Sekunden-LED `i` (0 = oben) bei `sec` Sekunden? Die Kette füllt sich im
/// Uhrzeigersinn von 0 bis zur aktuellen Sekunde.
pub fn second_led_lit(i: usize, sec: u32) -> bool {
    i <= sec.min(59) as usize
}

/// Winkel der LED `i` in Radiant, 0 = oben, im Uhrzeigersinn.
pub fn led_angle(i: usize) -> f32 {
    (i % 60) as f32 * std::f32::consts::TAU / 60.0
}

/// Ort der LED `i` auf dem Kreis um `center` mit Radius `r` (Bildschirm: y nach unten).
pub fn led_pos(center: Pos2, r: f32, i: usize) -> Pos2 {
    let a = led_angle(i);
    Pos2::new(center.x + a.sin() * r, center.y - a.cos() * r)
}

/// `hh:mm:ss` mit führenden Nullen.
pub fn fmt_hms(h: u32, m: u32, s: u32) -> String {
    format!("{h:02}:{m:02}:{s:02}")
}

/// Platz der Digitalzeit: halbe Breite, die sie höchstens haben darf, damit sie mit
/// Abstand innerhalb der Stundenpunkte bleibt (#172).
pub fn digits_half_room(diameter: f32) -> f32 {
    let hour_ring = diameter / 2.0 - diameter * 0.15;
    let hour_dot = (diameter * 0.014).max(1.6);
    hour_ring - hour_dot - diameter * 0.05
}

/// Schriftgröße der Digitalzeit: Wunschgröße `0.15 · diameter`, verkleinert, bis die
/// gemessene Breite von „88:88:88“ in [`digits_half_room`] passt.
pub fn digits_size(ui: &egui::Ui, diameter: f32) -> f32 {
    let base = diameter * 0.15;
    let width = ui
        .painter()
        .layout_no_wrap(
            "88:88:88".to_string(),
            theme::seg_font(base),
            egui::Color32::WHITE,
        )
        .size()
        .x;
    let room = 2.0 * digits_half_room(diameter);
    if width > room && width > 0.0 {
        base * room / width
    } else {
        base
    }
}

/// Zeichnet die Uhr mit Durchmesser `diameter` für die Uhrzeit `h:m:s`.
pub fn studio_clock(ui: &mut egui::Ui, diameter: f32, h: u32, m: u32, s: u32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(diameter, diameter), Sense::hover());
    let p = ui.painter();
    let c = rect.center();
    let r = diameter / 2.0;
    p.circle_filled(c, r, theme::lcd_bg());
    p.circle_stroke(c, r - 0.5, Stroke::new(1.0, theme::border_strong()));
    let ring = r - diameter * 0.075;
    let dot = (diameter * 0.011).max(1.2);
    for i in 0..60 {
        let pos = led_pos(c, ring, i);
        if second_led_lit(i, s) {
            let head = i == s.min(59) as usize;
            if head {
                p.circle_filled(pos, dot * 2.6, with_alpha(theme::lcd_ink(), 60));
            }
            p.circle_filled(pos, dot * if head { 1.7 } else { 1.3 }, theme::lcd_ink());
        } else {
            p.circle_filled(pos, dot, theme::led_off());
        }
    }
    // Stundenpunkte innerhalb der Kette.
    for k in 0..12 {
        let pos = led_pos(c, ring - diameter * 0.075, k * 5);
        p.circle_filled(pos, (diameter * 0.014).max(1.6), theme::lcd_text());
    }
    // Digitalzeit: Geister-Segmente dahinter, dann die Ziffern.
    let size = digits_size(ui, diameter);
    let font = theme::seg_font(size);
    p.text(
        c,
        Align2::CENTER_CENTER,
        "88:88:88",
        font.clone(),
        with_alpha(theme::led_off(), 160),
    );
    p.text(
        c,
        Align2::CENTER_CENTER,
        fmt_hms(h, m, s),
        font,
        theme::lcd_ink(),
    );
    p.text(
        c + egui::vec2(0.0, diameter * 0.15 * 1.25),
        Align2::CENTER_CENTER,
        "STUDIO-UHR",
        theme::label_font((diameter * 0.065).max(9.0)),
        theme::lcd_text(),
    );
    resp.on_hover_text("Uhrzeit (Ortszeit der Stundenuhr)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_chain_fills_up_to_the_current_second() {
        assert!(second_led_lit(0, 0));
        assert!(!second_led_lit(1, 0));
        assert!(second_led_lit(30, 30));
        assert!(!second_led_lit(31, 30));
        assert!(second_led_lit(59, 59));
        // Schaltsekunde 60 begrenzt auf 59.
        assert!(second_led_lit(59, 60));
        assert_eq!((0..60).filter(|&i| second_led_lit(i, 44)).count(), 45);
    }

    #[test]
    fn leds_run_clockwise_from_the_top() {
        let c = Pos2::new(100.0, 100.0);
        let top = led_pos(c, 50.0, 0);
        assert!((top.x - 100.0).abs() < 1e-3 && (top.y - 50.0).abs() < 1e-3);
        let right = led_pos(c, 50.0, 15);
        assert!((right.x - 150.0).abs() < 1e-3 && (right.y - 100.0).abs() < 1e-3);
        let bottom = led_pos(c, 50.0, 30);
        assert!((bottom.y - 150.0).abs() < 1e-3);
        let left = led_pos(c, 50.0, 45);
        assert!((left.x - 50.0).abs() < 1e-3);
    }

    /// #172: Die Digitalzeit darf die Stundenpunkte nicht berühren, mit echter Schrift
    /// gemessen. Die alte feste Größe (0.15 · Durchmesser) lief in die Punkte bei 3 und 9.
    #[test]
    fn digital_time_keeps_distance_to_the_hour_dots() {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
        {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                for d in [120.0_f32, 152.0, 220.0] {
                    let w = |size: f32| {
                        ui.painter()
                            .layout_no_wrap(
                                "88:88:88".into(),
                                theme::seg_font(size),
                                egui::Color32::WHITE,
                            )
                            .size()
                            .x
                    };
                    let room = digits_half_room(d);
                    let size = digits_size(ui, d);
                    assert!(w(size) / 2.0 <= room + 0.5, "d={d}");
                    assert!(size >= d * 0.09, "d={d}: Zeit zu klein ({size})");
                    // Innerhalb der Stundenpunkte: Abstand zum Punkt bei 3 Uhr.
                    let hour_ring = d / 2.0 - d * 0.15;
                    assert!(w(size) / 2.0 < hour_ring - (d * 0.014).max(1.6));
                }
                // Ursache belegt: die alte Größe passte bei 152 px nicht.
                let old = ui
                    .painter()
                    .layout_no_wrap(
                        "88:88:88".into(),
                        theme::seg_font(22.8),
                        egui::Color32::WHITE,
                    )
                    .size()
                    .x;
                assert!(old / 2.0 > digits_half_room(152.0));
            });
        }
    }

    #[test]
    fn digital_time_is_zero_padded() {
        assert_eq!(fmt_hms(7, 5, 9), "07:05:09");
        assert_eq!(fmt_hms(23, 59, 58), "23:59:58");
    }
}
