//! Jogwheel des Party-Decks: dreht sich beim Abspielen mit 33⅓ Umdrehungen je Minute und
//! steht still, wenn nichts läuft oder Bewegung reduziert ist.

use eframe::egui::{self, Color32, Pos2, Sense, Stroke};

use super::with_alpha;
use crate::theme;

/// Umdrehungen je Sekunde einer Platte mit 33⅓ U/min.
pub const REV_PER_S: f32 = 100.0 / 3.0 / 60.0;

/// Neuer Winkel (Radiant, 0 ≤ w < 2π) nach `dt` Sekunden.
pub fn advance(angle: f32, dt: f32, playing: bool) -> f32 {
    if !playing {
        return angle;
    }
    (angle + dt.max(0.0) * REV_PER_S * std::f32::consts::TAU).rem_euclid(std::f32::consts::TAU)
}

/// Zeichnet das Jogwheel mit Durchmesser `diameter`. `id` hält den Winkel zwischen den Bildern.
pub fn jog(ui: &mut egui::Ui, id: egui::Id, diameter: f32, playing: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(diameter, diameter), Sense::hover());
    let spin = playing && super::motion(ui.ctx());
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    let angle = ui.data_mut(|d| {
        let a = d.get_temp_mut_or(id, 0.0f32);
        *a = advance(*a, dt, spin);
        *a
    });
    if spin {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(16));
    }
    let p = ui.painter();
    let c = rect.center();
    let r = diameter / 2.0;
    // Gehäuse, Rand, Platte mit Rillen.
    p.circle_filled(c, r, theme::surface());
    p.circle_stroke(c, r - 0.5, Stroke::new(1.0, theme::border_strong()));
    let platter = r * 0.88;
    p.circle_filled(c, platter, theme::lcd_bg());
    for k in 1..=5 {
        let rr = platter * (0.30 + 0.13 * k as f32);
        p.circle_stroke(c, rr, Stroke::new(1.0, with_alpha(Color32::WHITE, 12)));
    }
    // Markierungen am Rand drehen mit, damit man die Bewegung sieht.
    for k in 0..24 {
        let a = angle + k as f32 * std::f32::consts::TAU / 24.0;
        let (s, co) = a.sin_cos();
        let long = k % 6 == 0;
        let r0 = platter * if long { 0.80 } else { 0.90 };
        let col = if long {
            theme::lcd_ink()
        } else {
            theme::lcd_text()
        };
        p.line_segment(
            [
                Pos2::new(c.x + s * r0, c.y - co * r0),
                Pos2::new(c.x + s * platter * 0.97, c.y - co * platter * 0.97),
            ],
            Stroke::new(if long { 2.0 } else { 1.0 }, col),
        );
    }
    // Mittelkappe.
    p.circle_filled(c, platter * 0.30, theme::surface_hover());
    p.circle_stroke(c, platter * 0.30, Stroke::new(1.0, theme::border_strong()));
    p.circle_filled(
        c,
        platter * 0.07,
        if playing {
            theme::accent()
        } else {
            theme::led_off()
        },
    );
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stands_still_unless_playing() {
        assert_eq!(advance(1.0, 0.5, false), 1.0);
        assert!(advance(1.0, 0.5, true) > 1.0);
    }

    #[test]
    fn one_minute_makes_thirty_three_and_a_third_turns() {
        let mut a = 0.0f32;
        let mut total = 0.0f32;
        for _ in 0..3600 {
            let n = advance(a, 1.0 / 60.0, true);
            total += (n - a).rem_euclid(std::f32::consts::TAU);
            a = n;
        }
        let turns = total / std::f32::consts::TAU;
        assert!((turns - 33.333).abs() < 0.05, "{turns}");
    }

    #[test]
    fn angle_stays_in_one_turn() {
        let a = advance(6.2, 10.0, true);
        assert!((0.0..std::f32::consts::TAU).contains(&a));
    }
}
