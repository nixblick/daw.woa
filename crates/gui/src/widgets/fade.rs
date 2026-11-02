//! Fade-Taste (`FadeButton`, Design Fassung 7; #320, #321, #274): blendet fest in 3 s aus und
//! stoppt. Drei Formen: große Taste „Fade“ im Sende-Deck, quadratische Taste an den Decks A–C,
//! Symbol an Cart und Playlist-Zeile. Beim Ausblenden (`is-fading`) Rahmen `cue`, eine
//! `cue`-Füllung (28 %) wächst von links mit dem Fortschritt, rechts die Restzeit in Mono.
//! Läuft nichts, ist die Taste gesperrt. Die Zeit führt [`Fades`] im Oberflächen-Thread; das
//! Ausblenden selbst macht der Mixer ([`woa_radio::FADE_SECONDS`]).

use std::collections::HashMap;

use eframe::egui::{self, Color32, CornerRadius, Sense, Stroke, StrokeKind};

use crate::theme;

/// Dauer der Blende in Sekunden (fest, an allen Stellen).
pub const FADE_SECONDS: f64 = woa_radio::FADE_SECONDS;
/// Symbol der Fade-Taste (Phosphor `trend-down`, im Design `fadeout`).
pub const FADE_ICON: &str = egui_phosphor::regular::TREND_DOWN;
/// Tooltip und Hinweis (FadeButton-README).
pub const FADE_TIP: &str = "Fade (3 s): blendet in 3 Sekunden aus und stoppt";

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

/// Laufende Blenden mit ihrer Startzeit (egui-Zeit in Sekunden).
#[derive(Debug, Default)]
pub struct Fades {
    started: HashMap<FadeKey, f64>,
}

impl Fades {
    /// Druck auf die Taste. `true`: die Blende beginnt; `false`: sie lief schon, der zweite
    /// Druck stoppt sofort (der Mixer macht das von selbst) und die Anzeige endet.
    pub fn press(&mut self, key: FadeKey, now: f64) -> bool {
        if self.progress(key, now).is_some() {
            self.started.remove(&key);
            false
        } else {
            self.started.insert(key, now);
            if key == FadeKey::All {
                // Alles blendet gemeinsam aus: Einzelanzeigen gehen in dieser auf.
                self.started.retain(|k, _| *k == FadeKey::All);
            }
            true
        }
    }

    /// Fortschritt 0..1, solange die Blende läuft; danach `None`.
    pub fn progress(&self, key: FadeKey, now: f64) -> Option<f32> {
        let t0 = self
            .started
            .get(&key)
            .or_else(|| match key {
                FadeKey::Playlist | FadeKey::Deck(_) | FadeKey::Cart(_) => {
                    self.started.get(&FadeKey::All)
                }
                FadeKey::All => None,
            })
            .copied()?;
        fade_progress(t0, now)
    }

    /// Räumt abgelaufene Blenden ab; `true`, solange noch eine läuft (für Neuzeichnen).
    pub fn tick(&mut self, now: f64) -> bool {
        self.started
            .retain(|_, t0| fade_progress(*t0, now).is_some());
        !self.started.is_empty()
    }

    /// Die Quelle steht schon (etwa von Hand gestoppt): Anzeige beenden.
    pub fn clear(&mut self, key: FadeKey) {
        self.started.remove(&key);
    }
}

/// Fortschritt einer Blende, die bei `t0` begann: 0..1 oder `None`, wenn sie vorbei ist.
pub fn fade_progress(t0: f64, now: f64) -> Option<f32> {
    let p = (now - t0) / FADE_SECONDS;
    (0.0..1.0).contains(&p).then_some(p as f32)
}

/// Restzeit als „1,1 s“ (deutsches Komma, eine Stelle).
pub fn rest_text(progress: f32) -> String {
    let rest = (1.0 - f64::from(progress.clamp(0.0, 1.0))) * FADE_SECONDS;
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

/// Zeichnet die Fade-Taste. `progress`: läuft gerade eine Blende. `enabled`: läuft etwas.
pub fn fade_button(
    ui: &mut egui::Ui,
    shape: FadeShape,
    label: &str,
    enabled: bool,
    progress: Option<f32>,
) -> egui::Response {
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
        FadeShape::Square(s) | FadeShape::Icon(s) => egui::vec2(s, s),
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
                if let Some(pr) = progress {
                    p.text(
                        egui::pos2(t.right() + 10.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        rest_text(pr),
                        egui::FontId::monospace(12.0),
                        theme::cue(),
                    );
                }
            }
            FadeShape::Square(_) | FadeShape::Icon(_) => {
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
    let r = r.on_hover_text(match progress {
        Some(pr) => format!("Blendet aus, noch {} (Klick: sofort Stopp)", rest_text(pr)),
        None => FADE_TIP.to_string(),
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
    fn fade_runs_exactly_three_seconds() {
        assert_eq!(FADE_SECONDS, 3.0);
        assert_eq!(fade_progress(10.0, 10.0), Some(0.0));
        assert_eq!(fade_progress(10.0, 11.5), Some(0.5));
        assert!(fade_progress(10.0, 12.99).is_some());
        assert_eq!(fade_progress(10.0, 13.0), None, "nach 3 s vorbei");
        assert_eq!(fade_progress(10.0, 9.0), None);
    }

    #[test]
    fn rest_shows_german_decimal_comma() {
        assert_eq!(rest_text(0.0), "3,0 s");
        assert_eq!(rest_text(0.62), "1,1 s");
        assert_eq!(rest_text(1.0), "0,0 s");
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
