//! Einheitlicher Steuerknopf (André 31.10.2026): immer Symbol und Wort, eine Höhe, eine
//! Mindestbreite. Play und Pause leuchten in eigenen Farben aus den Theme-Tokens (`ok`, `warn`),
//! die in Hell und Dunkel gelten. Zeichnet über die beleuchtete Taste [`super::Key`].

use eframe::egui::{self, Response};

use crate::theme;

/// Höhe jedes Steuerknopfs.
pub const CTL_HEIGHT: f32 = 44.0;
/// Mindestbreite jedes Steuerknopfs.
pub const CTL_MIN_WIDTH: f32 = 108.0;
/// Schriftgröße der Beschriftung.
pub const CTL_FONT: f32 = 18.0;

/// Rolle des Knopfs, bestimmt die Leuchtfarbe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Normal,
    Play,
    Pause,
}

/// Beschriftung „Symbol Wort“.
pub fn ctl_label(icon: &str, word: &str) -> String {
    format!("{icon} {word}")
}

/// Ein Steuerknopf. `width` ist der Wunsch (z. B. ein Viertel der Reihe), nie unter der
/// Mindestbreite und nie schmaler als die Beschriftung.
pub fn ctl_key(
    ui: &mut egui::Ui,
    icon: &str,
    word: &str,
    tone: Tone,
    active: bool,
    width: f32,
) -> Response {
    let label = ctl_label(icon, word);
    let font = theme::label_font(CTL_FONT);
    let text_w = ui
        .painter()
        .layout_no_wrap(label.clone(), font.clone(), egui::Color32::WHITE)
        .size()
        .x;
    let w = width.max(CTL_MIN_WIDTH).max(text_w + 24.0);
    let (color, on) = match tone {
        Tone::Play => (theme::ok(), true),
        Tone::Pause => (theme::warn(), true),
        Tone::Normal => (theme::accent(), false),
    };
    super::Key::new(&label, egui::vec2(w, CTL_HEIGHT))
        .font(font)
        .color(color)
        .radius(theme::RADIUS_PARTY)
        .on(on)
        .lit(active)
        .ui(ui)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_has_symbol_and_word() {
        let l = ctl_label(theme::ic::PLAY, "Play");
        assert!(l.starts_with(theme::ic::PLAY) && l.ends_with(" Play"));
    }
}
