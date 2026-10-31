//! Bausteine des Design Systems (Paket D4): beleuchtete Taste, Fader, VU-Meter, Studio-Uhr,
//! ON-AIR-Lampe, Cart-Pad und Jogwheel. Jeder Baustein zeichnet mit den Tokens aus
//! [`crate::theme`] und folgt damit Thema und Modus. Das Rechnen (Winkel, Schwellen, Takt)
//! steckt in kleinen reinen Funktionen und ist ohne Fenster getestet.

pub mod clock;
pub mod ctl;
pub mod fader;
pub mod jog;
pub mod key;
pub mod lamp;
pub mod pad;
pub mod vu;

pub use clock::studio_clock;
pub use ctl::{CTL_HEIGHT, CTL_MIN_WIDTH, Tone, ctl_key};
pub use fader::{Orient, fader};
pub use jog::jog;
pub use key::Key;
pub use lamp::onair_lamp;
pub use pad::CartPad;
pub use vu::{VuView, vu_meter};

use eframe::egui::{self, Color32};

/// Dürfen sich Anzeigen bewegen? Nein, wenn egui-Animationen ausgeschaltet sind
/// (`animation_time` 0) oder die Umgebung `WOA_REDUCED_MOTION=1` verlangt. Egui kennt die
/// Systemeinstellung „Bewegung reduzieren“ nicht selbst.
pub fn motion(ctx: &egui::Context) -> bool {
    static ENV_OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let env_off = *ENV_OFF
        .get_or_init(|| std::env::var("WOA_REDUCED_MOTION").is_ok_and(|v| v == "1" || v == "true"));
    !env_off && ctx.global_style().animation_time > 0.0
}

/// Schrift- oder Symbolfarbe, die auf `bg` gut lesbar ist (hell auf dunkel, dunkel auf hell).
pub fn contrast_text(bg: Color32) -> Color32 {
    // Relative Helligkeit nach Rec. 709 auf den gespeicherten (gammakodierten) Werten.
    let l = 0.2126 * f32::from(bg.r()) + 0.7152 * f32::from(bg.g()) + 0.0722 * f32::from(bg.b());
    if l > 140.0 {
        Color32::from_rgb(0x16, 0x12, 0x0c)
    } else {
        Color32::WHITE
    }
}

/// Dieselbe Farbe mit Deckkraft `a` (0..255), für Lichtschein.
pub fn with_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

/// Blinktakt: an in der ersten Hälfte jeder Periode. Ohne Bewegung (`hz` 0) immer an.
pub fn blink_on(t: f64, hz: f64) -> bool {
    if hz <= 0.0 {
        return true;
    }
    (t * hz).fract() < 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_text_flips_with_brightness() {
        assert_eq!(contrast_text(Color32::BLACK), Color32::WHITE);
        assert_eq!(
            contrast_text(Color32::from_rgb(255, 152, 56)),
            contrast_text(Color32::WHITE)
        );
        assert_ne!(contrast_text(Color32::WHITE), Color32::WHITE);
    }

    #[test]
    fn blink_has_half_duty_and_stands_still_without_rate() {
        assert!(blink_on(0.0, 2.0));
        assert!(!blink_on(0.3, 2.0));
        assert!(blink_on(0.5, 2.0));
        assert!(blink_on(123.4, 0.0));
    }

    #[test]
    fn with_alpha_keeps_the_colour() {
        let c = with_alpha(Color32::from_rgb(10, 20, 30), 128);
        assert_eq!(c.a(), 128);
    }
}
