//! Gesangseinsatz (#299) in Playlist und Decks: kleine Marke „Mikrofon + Rampzeit“ und ein
//! Strich auf dem Fortschrittsbalken. Von Hand gesetzt (Ramp-Marke im Cue-Editor) steht sie in
//! `accent-2`; aus der Analyse geschätzt in `text-dim` mit „≈“ davor.

use eframe::egui;
use woa_radio::show::VocalMark;

use crate::theme;

/// Breite der Marke in der Playlist-Zeile.
pub const VOCAL_W: f32 = 58.0;

/// Rampzeit (Sekunden von `start` bis zum Gesang) als „m:ss“, geschätzt mit „≈“.
pub fn vocal_text(m: &VocalMark, start: f64) -> String {
    let s = (m.seconds - start).max(0.0).round() as u64;
    let t = format!("{}:{:02}", s / 60, s % 60);
    if m.estimated { format!("≈{t}") } else { t }
}

/// Hinweis zur Marke.
pub fn vocal_hint(m: &VocalMark, start: f64) -> String {
    let t = vocal_text(m, start);
    let t = t.trim_start_matches('≈');
    if m.estimated {
        format!(
            "Gesang ab {t} (geschätzt aus der Analyse, kann danebenliegen). \
             Genau setzen: Rechtsklick › Cue-Punkte … › Rechtsklick in die Wellenform › \
             „Gesang beginnt hier“"
        )
    } else {
        format!("Gesang ab {t} (Ramp-Marke, von Hand gesetzt)")
    }
}

fn ink(m: &VocalMark) -> egui::Color32 {
    if m.estimated {
        theme::text_dim()
    } else {
        theme::accent_2()
    }
}

/// Die Marke (Symbol + Rampzeit) in fester Breite; ohne Gesangseinsatz bleibt der Platz leer.
pub fn vocal_tag(ui: &mut egui::Ui, m: Option<VocalMark>, start: f64, width: f32) {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 18.0), egui::Sense::hover());
    let Some(m) = m else { return };
    // Symbol aus der Phosphor-Schrift (proportional), Zeit in Mono.
    let t = ui.painter().text(
        rect.right_center(),
        egui::Align2::RIGHT_CENTER,
        vocal_text(&m, start),
        egui::FontId::monospace(11.0),
        ink(&m),
    );
    ui.painter().text(
        egui::pos2(t.left() - 3.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        theme::ic::MICROPHONE_STAGE,
        egui::FontId::proportional(12.0),
        ink(&m),
    );
    resp.on_hover_text(vocal_hint(&m, start));
}

/// Strich auf einem Fortschrittsbalken, der `start` … `start + length` zeigt.
pub fn vocal_tick(p: &egui::Painter, bar: egui::Rect, m: Option<VocalMark>, start: f64, len: f64) {
    let Some(m) = m else { return };
    if len <= 0.0 {
        return;
    }
    let f = ((m.seconds - start) / len).clamp(0.0, 1.0) as f32;
    let x = bar.left() + bar.width() * f;
    p.line_segment(
        [
            egui::pos2(x, bar.top() - 2.0),
            egui::pos2(x, bar.bottom() + 2.0),
        ],
        egui::Stroke::new(2.0, ink(&m)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramp_time_counts_from_cue_in_and_marks_estimates() {
        let est = VocalMark {
            seconds: 14.6,
            estimated: true,
        };
        assert_eq!(vocal_text(&est, 2.0), "≈0:13");
        let hand = VocalMark {
            seconds: 75.0,
            estimated: false,
        };
        assert_eq!(vocal_text(&hand, 0.0), "1:15");
        assert!(vocal_hint(&est, 0.0).contains("geschätzt"));
        assert!(!vocal_hint(&hand, 0.0).contains("geschätzt"));
    }
}
