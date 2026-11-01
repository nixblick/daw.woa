//! Cart-Pad der Cart-Wall (Design-Runde 2, CartPad-README): neutral auf `surface`, die Farbe
//! steckt nur im Balken unten und in der LED. Das laufende Pad hat `accent-soft` als Fläche,
//! `accent` als Rand und füllt den Balken mit seinem Fortschritt; leere Pads sind gestrichelt.
//! Die Nummer steht groß vorn, der Titel daneben auf höchstens zwei Zeilen (#97).

use eframe::egui::{self, Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind, Vec2};

use super::{blink_on, with_alpha};
use crate::theme;

/// Welcher Zustand ein Pad hat, für Farbe und Licht.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadState {
    Empty,
    Ready,
    Playing,
}

pub fn pad_state(filled: bool, playing: bool) -> PadState {
    if playing {
        PadState::Playing
    } else if filled {
        PadState::Ready
    } else {
        PadState::Empty
    }
}

/// Fläche je Zustand (Design-Runde 2, CartPad-README): belegt neutral `surface` (bzw. Hover),
/// laufend `accent-soft`, leer ohne Fläche (nur gestrichelter Rand).
pub fn pad_fill(state: PadState, base: Color32) -> Color32 {
    match state {
        PadState::Playing => theme::accent_soft(),
        PadState::Ready => base,
        PadState::Empty => Color32::TRANSPARENT,
    }
}

/// Farbbalken unten: Spurfarbe zu 35 % auf `surface`; beim Laufen füllt die volle Farbe den
/// Fortschritt darüber.
pub fn bar_color(color: Color32) -> Color32 {
    theme::mix(color, theme::surface(), 0.65)
}

/// Höhe des Farbbalkens.
pub const BAR_H: f32 = 5.0;

/// Höhe der großen Nummer: fast so hoch wie das Pad.
pub fn number_size(pad_height: f32) -> f32 {
    pad_height * 0.62
}

/// Schriftgröße des Cart-Namens (#292: größer, lesbarer).
pub const TITLE_SIZE: f32 = 15.0;

/// Titel neben der Nummer: umbrochen auf `width`, höchstens zwei Zeilen, Rest mit „…“.
pub fn title_job(text: &str, width: f32, color: Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::simple(
        text.to_string(),
        theme::label_font(TITLE_SIZE),
        color,
        width.max(1.0),
    );
    job.wrap.max_rows = 2;
    job.wrap.break_anywhere = false;
    job.wrap.overflow_character = Some('…');
    job
}

/// Ein Pad. `key` ist der Tastenname in der Ecke (nicht die Nummer), `capturing` zeigt „?“.
pub struct CartPad<'a> {
    pub number: usize,
    pub title: Option<&'a str>,
    pub key: Option<&'a str>,
    pub capturing: bool,
    pub playing: bool,
    pub color: Color32,
    /// Restzeit in Sekunden, solange das Pad läuft (Countdown rechts).
    pub remaining: Option<f64>,
    /// Gesamtdauer in Sekunden (#292), rechts unten solange das Pad steht.
    pub seconds: Option<f64>,
}

/// Gesamtdauer eines Carts: „0:12“, „1:30“.
pub fn duration(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Countdown eines laufenden Carts: „-12 s“, unter 10 s mit einer Nachkommastelle („-9,4 s“).
pub fn countdown(secs: f64) -> String {
    let s = secs.max(0.0);
    if s < 10.0 {
        format!("-{:.1} s", s).replace('.', ",")
    } else {
        format!("-{:.0} s", s.ceil())
    }
}

impl CartPad<'_> {
    pub fn ui(self, ui: &mut egui::Ui, size: Vec2) -> egui::Response {
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        let state = pad_state(self.title.is_some(), self.playing);
        let r = CornerRadius::same(theme::RADIUS_MD);
        let p = ui.painter();
        let accent = theme::accent();
        let base = if resp.hovered() {
            theme::surface_hover()
        } else {
            theme::surface()
        };
        p.rect_filled(rect, r, pad_fill(state, base));
        match state {
            PadState::Empty => {
                let s = Stroke::new(1.0, theme::border_strong());
                let q = rect.shrink(0.5);
                let pts = [
                    q.left_top(),
                    q.right_top(),
                    q.right_bottom(),
                    q.left_bottom(),
                    q.left_top(),
                ];
                p.extend(egui::Shape::dashed_line(&pts, s, 4.0, 3.0));
            }
            _ => {
                let edge = if state == PadState::Playing {
                    accent
                } else {
                    theme::border_strong()
                };
                p.rect_stroke(rect, r, Stroke::new(1.0, edge), StrokeKind::Inside);
            }
        }
        let ink = if state == PadState::Empty {
            theme::text_dim()
        } else {
            theme::text()
        };
        // Große Nummer vorn in `text-dim`, Titel in Versalien daneben, Länge rechts.
        let number = p.layout_no_wrap(
            self.number.to_string(),
            theme::label_font(number_size(rect.height())),
            theme::text_dim(),
        );
        let body_mid = rect.center().y - BAR_H / 2.0;
        let num_pos = egui::pos2(rect.left() + 8.0, body_mid - number.size().y / 2.0);
        let text_left = num_pos.x + number.size().x + 8.0;
        p.galley(num_pos, number, ink);
        let mut text_right = rect.right() - 18.0;
        let count = self
            .remaining
            .filter(|_| state == PadState::Playing)
            .map(countdown);
        let total = self
            .seconds
            .filter(|_| state == PadState::Ready)
            .map(duration);
        if let Some((t, size, c)) = count
            .as_ref()
            .map(|t| (t, 15.0, accent))
            .or(total.as_ref().map(|t| (t, 12.0, theme::text_dim())))
        {
            let g = p.layout_no_wrap(t.clone(), egui::FontId::monospace(size), c);
            let at = egui::pos2(
                rect.right() - 8.0 - g.size().x,
                rect.bottom() - BAR_H - 5.0 - g.size().y,
            );
            text_right = at.x - 6.0;
            p.galley(at, g, c);
        }
        let shown = self.title.unwrap_or("leer");
        let upper = shown.to_uppercase();
        let title = p.layout_job(title_job(&upper, text_right - text_left, ink));
        let title_pos = egui::pos2(text_left, body_mid - title.size().y / 2.0);
        p.with_clip_rect(rect.shrink(2.0))
            .galley(title_pos, title, ink);
        if self.capturing || self.key.is_some() {
            let text = if self.capturing {
                "?"
            } else {
                self.key.unwrap_or("")
            };
            let galley = p.layout_no_wrap(
                text.to_string(),
                theme::label_font(11.0),
                if self.capturing {
                    accent
                } else {
                    theme::text()
                },
            );
            let size = galley.size() + egui::vec2(8.0, 2.0);
            let tag = Rect::from_min_size(
                egui::pos2(rect.right() - 20.0 - size.x, rect.top() + 4.0),
                size,
            );
            p.rect_filled(tag, CornerRadius::same(theme::RADIUS_XS + 2), theme::well());
            p.galley(tag.min + egui::vec2(4.0, 1.0), galley, theme::text());
        }
        // Status-LED rechts oben: laufend blinkt sie in `accent`, belegt leuchtet die Spurfarbe.
        let motion = super::motion(ui.ctx());
        let blink = if state == PadState::Playing && motion {
            let t = ui.input(|i| i.time);
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(120));
            blink_on(t, 2.0)
        } else {
            true
        };
        let led = match state {
            PadState::Playing if blink => accent,
            PadState::Playing => with_alpha(accent, 70),
            PadState::Ready => self.color,
            PadState::Empty => theme::led_off(),
        };
        p.circle_filled(egui::pos2(rect.right() - 10.0, rect.top() + 10.0), 3.5, led);
        // Farbbalken unten: Spurfarbe 35 %, laufend der Fortschritt in voller Farbe.
        if state != PadState::Empty {
            let bar = Rect::from_min_max(
                egui::pos2(rect.left() + 1.0, rect.bottom() - 1.0 - BAR_H),
                egui::pos2(rect.right() - 1.0, rect.bottom() - 1.0),
            );
            let br = CornerRadius {
                nw: 0,
                ne: 0,
                sw: theme::RADIUS_MD - 1,
                se: theme::RADIUS_MD - 1,
            };
            p.rect_filled(bar, br, bar_color(self.color));
            if state == PadState::Playing
                && let (Some(rem), Some(tot)) = (self.remaining, self.seconds)
                && tot > 0.0
            {
                let done = (1.0 - rem / tot).clamp(0.0, 1.0) as f32;
                let mut fill = bar;
                fill.set_right(bar.left() + bar.width() * done);
                p.rect_filled(fill, br, self.color);
            }
        }
        let label = match (&count, &total) {
            (Some(t), _) | (None, Some(t)) => format!("Cart {}: {} {t}", self.number, shown),
            (None, None) => format!("Cart {}: {}", self.number, shown),
        };
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Design-Runde 2: belegt neutral (`surface`), laufend `accent-soft`, leer ohne Fläche; der
    /// Farbbalken unten trägt die Spurfarbe zu 35 %. Nummer fast padhoch, Titel höchstens zwei Zeilen.
    #[test]
    fn pads_are_neutral_with_colour_bar_and_titles_two_lines() {
        let base = theme::surface();
        assert_eq!(pad_fill(PadState::Ready, base), base);
        assert_eq!(pad_fill(PadState::Empty, base), Color32::TRANSPARENT);
        assert_eq!(pad_fill(PadState::Playing, base), theme::accent_soft());
        let red = Color32::from_rgb(255, 0, 0);
        assert_eq!(bar_color(red), theme::mix(red, theme::surface(), 0.65));
        assert!(number_size(68.0) >= 40.0);

        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let long = "Stationsjingle Morgenshow mit sehr langem Namen und noch mehr Text";
            let g = ui
                .painter()
                .layout_job(title_job(long, 120.0, Color32::WHITE));
            assert_eq!(g.rows.len(), 2);
            let short = ui
                .painter()
                .layout_job(title_job("Intro", 120.0, Color32::WHITE));
            assert_eq!(short.rows.len(), 1);
        });
    }

    #[test]
    fn duration_reads_minutes_and_seconds() {
        assert_eq!(duration(12.4), "0:12");
        assert_eq!(duration(90.0), "1:30");
        assert_eq!(duration(-1.0), "0:00");
        const { assert!(TITLE_SIZE > 13.0) };
    }

    #[test]
    fn playing_wins_over_filled_and_empty_is_unfilled() {
        assert_eq!(pad_state(true, true), PadState::Playing);
        assert_eq!(pad_state(false, true), PadState::Playing);
        assert_eq!(pad_state(true, false), PadState::Ready);
        assert_eq!(pad_state(false, false), PadState::Empty);
    }
}
