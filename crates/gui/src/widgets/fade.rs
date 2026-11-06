//! Fade-Taste (`FadeButton`, Design Fassung 7; #320, #321, #274): blendet in der eingestellten
//! Dauer aus (Vorgabe 3 s, 1–10 s unter Einstellungen › Radio) und stoppt. Drei Formen: große Taste „Fade“ im Sende-Deck, quadratische Taste an den Decks A–C,
//! Symbol an Cart und Playlist-Zeile. Beim Ausblenden (`is-fading`) Rahmen `cue`, eine
//! `cue`-Füllung (28 %) wächst von links mit dem Fortschritt, rechts die Restzeit in Mono.
//! Läuft nichts, ist die Taste gesperrt. Die Zeit führt [`Fades`] im Oberflächen-Thread; das
//! Ausblenden selbst macht der Mixer ([`woa_radio::FADE_SECONDS`]).

use std::collections::HashMap;

use eframe::egui::{self, Color32, CornerRadius, Sense, Stroke, StrokeKind};

use crate::theme;

/// Vorgabe der Blende in Sekunden; die eingestellte Dauer führt [`Fades`].
pub const FADE_SECONDS: f64 = woa_radio::FADE_SECONDS;
/// Symbol der Fade-Taste (Phosphor `trend-down`, im Design `fadeout`).
pub const FADE_ICON: &str = egui_phosphor::regular::TREND_DOWN;
/// Tooltip und Hinweis (FadeButton-README) für die Dauer `secs`.
pub fn fade_tip(secs: f64) -> String {
    format!("Ausblenden ({})", secs_text(secs))
}

/// „3 s“, „2,5 s“ (deutsches Komma, ganze Sekunden ohne Nachkomma).
pub fn secs_text(secs: f64) -> String {
    if (secs - secs.round()).abs() < 0.05 {
        format!("{} s", secs.round() as i64)
    } else {
        format!("{secs:.1} s").replace('.', ",")
    }
}

/// Eine laufende Blende: Fortschritt 0..1 und ihre Gesamtdauer in Sekunden.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Running {
    pub progress: f32,
    pub total: f64,
}

/// Was ausgeblendet wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FadeKey {
    /// Sende-Deck: alles Laufende (Playlist, Decks, Carts).
    All,
    /// Die laufende Playlist-Zeile.
    Playlist,
    /// Deck A–C.
    Deck(usize),
    /// Cart der Cart-Wall.
    Cart(usize),
}

/// Laufende Blenden mit Startzeit (egui-Zeit in Sekunden) und Dauer.
#[derive(Debug)]
pub struct Fades {
    started: HashMap<FadeKey, (f64, f64)>,
    /// Eingestellte Dauer für die nächste Blende.
    seconds: f64,
}

impl Default for Fades {
    fn default() -> Self {
        Self {
            started: HashMap::new(),
            seconds: FADE_SECONDS,
        }
    }
}

impl Fades {
    /// Übernimmt die eingestellte Dauer (gilt ab dem nächsten Druck, wie im Mixer).
    pub fn set_seconds(&mut self, secs: f64) {
        if secs.is_finite() && secs > 0.0 {
            self.seconds = secs;
        }
    }

    /// Eingestellte Dauer.
    pub fn seconds(&self) -> f64 {
        self.seconds
    }

    /// Druck auf die Taste. `true`: die Blende beginnt; `false`: sie lief schon, der zweite
    /// Druck stoppt sofort (der Mixer macht das von selbst) und die Anzeige endet.
    pub fn press(&mut self, key: FadeKey, now: f64) -> bool {
        if self.progress(key, now).is_some() {
            self.started.remove(&key);
            false
        } else {
            self.started.insert(key, (now, self.seconds));
            if key == FadeKey::All {
                // Alles blendet gemeinsam aus: Einzelanzeigen gehen in dieser auf.
                self.started.retain(|k, _| *k == FadeKey::All);
            }
            true
        }
    }

    /// Fortschritt und Dauer, solange die Blende läuft; danach `None`.
    pub fn progress(&self, key: FadeKey, now: f64) -> Option<Running> {
        let (t0, total) = self
            .started
            .get(&key)
            .or_else(|| match key {
                // Decks blenden nur an ihrer eigenen Taste (Moderator 05.11.2026).
                FadeKey::Playlist | FadeKey::Cart(_) => self.started.get(&FadeKey::All),
                FadeKey::Deck(_) | FadeKey::All => None,
            })
            .copied()?;
        fade_progress(t0, now, total).map(|progress| Running { progress, total })
    }

    /// Räumt abgelaufene Blenden ab; `true`, solange noch eine läuft (für Neuzeichnen).
    pub fn tick(&mut self, now: f64) -> bool {
        self.started
            .retain(|_, (t0, total)| fade_progress(*t0, now, *total).is_some());
        !self.started.is_empty()
    }

    /// Die Quelle steht schon (etwa von Hand gestoppt): Anzeige beenden.
    pub fn clear(&mut self, key: FadeKey) {
        self.started.remove(&key);
    }
}

/// Fortschritt einer Blende über `total` Sekunden, die bei `t0` begann: 0..1 oder `None`,
/// wenn sie vorbei ist.
pub fn fade_progress(t0: f64, now: f64, total: f64) -> Option<f32> {
    let p = (now - t0) / total.max(0.01);
    (0.0..1.0).contains(&p).then_some(p as f32)
}

/// Restzeit als „1,1 s“ (deutsches Komma, eine Stelle).
pub fn rest_text(r: Running) -> String {
    let rest = (1.0 - f64::from(r.progress.clamp(0.0, 1.0))) * r.total;
    format!("{rest:.1} s").replace('.', ",")
}

/// Form der Taste.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FadeShape {
    /// Große Taste mit Wort (Sende-Deck), Höhe in px.
    Big(f32),
    /// Quadratische Taste (Decks A–C), Kantenlänge.
    Square(f32),
    /// Kleines Symbol (Cart, Playlist-Zeile), Kantenlänge.
    Icon(f32),
}

/// Restzeit im Inneren der quadratischen Deck-Taste (#539); nur dort, nicht an Icon-Tasten.
fn countdown_text(shape: FadeShape, running: Option<Running>) -> Option<String> {
    match (shape, running) {
        (FadeShape::Square(_), Some(run)) => Some(rest_text(run)),
        _ => None,
    }
}

/// Breite der quadratischen Taste: während der Blende breiter, damit „3,0 s“ passt.
fn square_width(s: f32, fading: bool) -> f32 {
    if fading { s + 24.0 } else { s }
}

/// Zeichnet die Fade-Taste. `running`: läuft gerade eine Blende. `enabled`: läuft etwas.
/// `secs`: eingestellte Dauer (für den Hinweis).
pub fn fade_button(
    ui: &mut egui::Ui,
    shape: FadeShape,
    label: &str,
    enabled: bool,
    running: Option<Running>,
    secs: f64,
) -> egui::Response {
    let progress = running.map(|r| r.progress);
    let enabled = enabled || progress.is_some();
    let word = "FADE";
    let font = theme::label_font(13.0);
    let size = match shape {
        FadeShape::Big(h) => {
            let w = ui
                .painter()
                .layout_no_wrap(word.into(), font.clone(), Color32::WHITE)
                .size()
                .x;
            let extra = if progress.is_some() { 44.0 } else { 0.0 };
            egui::vec2(16.0 + 8.0 + w + 28.0 + extra, h)
        }
        FadeShape::Square(s) => egui::vec2(square_width(s, progress.is_some()), s),
        FadeShape::Icon(s) => egui::vec2(s, s),
    };
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, r) = ui.allocate_exact_size(size, sense);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let radius = CornerRadius::same(theme::RADIUS_MD);
        let icon_only = matches!(shape, FadeShape::Icon(_));
        let fading = progress.is_some();
        let fill = if icon_only && !fading {
            if r.hovered() {
                theme::surface_hover()
            } else {
                Color32::TRANSPARENT
            }
        } else if r.hovered() && enabled {
            theme::surface_hover()
        } else {
            theme::surface()
        };
        p.rect_filled(rect, radius, fill);
        if let Some(pr) = progress {
            let mut f = rect;
            f.set_width(rect.width() * pr.clamp(0.0, 1.0));
            p.rect_filled(f, radius, crate::widgets::with_alpha(theme::cue(), 71));
        }
        let stroke = if fading {
            Stroke::new(1.5, theme::cue())
        } else if icon_only {
            Stroke::NONE
        } else {
            Stroke::new(1.0, theme::border_strong())
        };
        p.rect_stroke(rect, radius, stroke, StrokeKind::Inside);
        let ink = if !enabled {
            crate::widgets::with_alpha(theme::text_dim(), 110)
        } else if fading {
            theme::cue()
        } else {
            theme::text()
        };
        let icon_font = egui::FontId::proportional(match shape {
            FadeShape::Icon(s) => (s * 0.6).max(12.0),
            _ => 18.0,
        });
        match shape {
            FadeShape::Big(_) => {
                let mut x = rect.left() + 14.0;
                let g = p.text(
                    egui::pos2(x, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    FADE_ICON,
                    icon_font,
                    ink,
                );
                x = g.right() + 8.0;
                let t = p.text(
                    egui::pos2(x, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    word,
                    font,
                    if enabled {
                        theme::text()
                    } else {
                        theme::text_dim()
                    },
                );
                if let Some(run) = running {
                    p.text(
                        egui::pos2(t.right() + 10.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        rest_text(run),
                        egui::FontId::monospace(12.0),
                        theme::cue(),
                    );
                }
            }
            FadeShape::Square(_) | FadeShape::Icon(_) => {
                if let Some(text) = countdown_text(shape, running) {
                    p.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        text,
                        egui::FontId::monospace(12.0),
                        theme::cue(),
                    );
                } else {
                    p.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        FADE_ICON,
                        icon_font,
                        ink,
                    );
                }
            }
        }
    }
    let r = r.on_hover_text(match running {
        Some(run) => format!("Blendet aus, noch {} (Klick: sofort Stopp)", rest_text(run)),
        None => fade_tip(secs),
    });
    if progress.is_some() {
        ui.ctx().request_repaint();
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_runs_exactly_the_set_length() {
        assert_eq!(FADE_SECONDS, 3.0);
        assert_eq!(fade_progress(10.0, 10.0, 3.0), Some(0.0));
        assert_eq!(fade_progress(10.0, 11.5, 3.0), Some(0.5));
        assert!(fade_progress(10.0, 12.99, 3.0).is_some());
        assert_eq!(fade_progress(10.0, 13.0, 3.0), None, "nach 3 s vorbei");
        assert_eq!(fade_progress(10.0, 9.0, 3.0), None);
        assert!(fade_progress(10.0, 16.0, 8.0).is_some(), "8 s eingestellt");
    }

    /// #539: die quadratische Deck-Taste zählt herunter, Icon-Taste (Cart, Zeile) bleibt stumm.
    #[test]
    fn deck_key_shows_the_countdown_while_fading() {
        let run = Running {
            progress: 0.0,
            total: 3.0,
        };
        assert_eq!(
            countdown_text(FadeShape::Square(32.0), Some(run)).as_deref(),
            Some("3,0 s")
        );
        assert_eq!(countdown_text(FadeShape::Square(32.0), None), None);
        assert_eq!(countdown_text(FadeShape::Icon(24.0), Some(run)), None);
        assert!(square_width(32.0, true) > square_width(32.0, false));
    }

    #[test]
    fn rest_shows_german_decimal_comma() {
        let r = |progress, total| Running { progress, total };
        assert_eq!(rest_text(r(0.0, 3.0)), "3,0 s");
        assert_eq!(rest_text(r(0.62, 3.0)), "1,1 s");
        assert_eq!(rest_text(r(1.0, 3.0)), "0,0 s");
        assert_eq!(rest_text(r(0.5, 10.0)), "5,0 s");
        assert_eq!(fade_tip(3.0), "Ausblenden (3 s)");
        assert_eq!(secs_text(2.5), "2,5 s");
    }

    /// Moderator 05.11.2026: die Dauer ist einstellbar; eine laufende Blende behält ihre.
    #[test]
    fn a_running_fade_keeps_its_length_when_the_setting_changes() {
        let mut f = Fades::default();
        f.set_seconds(6.0);
        assert!(f.press(FadeKey::Cart(0), 0.0));
        f.set_seconds(1.0);
        let run = f.progress(FadeKey::Cart(0), 3.0).expect("läuft noch");
        assert_eq!(run.total, 6.0);
        assert!((run.progress - 0.5).abs() < 1e-6);
        assert!(f.press(FadeKey::Deck(0), 10.0));
        assert!(
            f.progress(FadeKey::Deck(0), 11.5).is_none(),
            "neue Blende 1 s"
        );
    }

    #[test]
    fn second_press_ends_the_fade_and_all_covers_every_source() {
        let mut f = Fades::default();
        assert!(f.press(FadeKey::Deck(1), 0.0));
        assert!(f.progress(FadeKey::Deck(1), 1.0).is_some());
        assert!(
            !f.press(FadeKey::Deck(1), 1.0),
            "zweiter Druck: sofort Stopp"
        );
        assert!(f.progress(FadeKey::Deck(1), 1.1).is_none());
        assert!(f.press(FadeKey::All, 5.0));
        assert!(
            f.progress(FadeKey::Cart(3), 6.0).is_some(),
            "Cart blendet mit"
        );
        assert!(f.progress(FadeKey::Playlist, 6.0).is_some());
        assert!(f.tick(7.9));
        assert!(!f.tick(8.0), "nach 3 s nichts mehr");
    }
}
