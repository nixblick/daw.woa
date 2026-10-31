//! Cart-Pad der Cart-Wall: Tastennummer, Titel, Status-LED und Spurfarben-Balken. Das
//! laufende Pad leuchtet in der Modusfarbe und blinkt als einziges Pad. Belegte Pads sind
//! freundlich grün, leere grau; die Nummer steht groß vorn, der Titel daneben auf höchstens
//! zwei Zeilen (#97).

use eframe::egui::{self, Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind, Vec2};

use super::{blink_on, contrast_text, with_alpha};
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

/// Fläche je Zustand: laufend Modusfarbe, belegt Grün (`ok`) auf der Fläche, leer grau.
pub fn pad_fill(state: PadState, base: Color32) -> Color32 {
    match state {
        PadState::Playing => theme::accent(),
        PadState::Ready => theme::mix(theme::ok(), base, 0.55),
        PadState::Empty => theme::well(),
    }
}

/// Höhe der großen Nummer: fast so hoch wie das Pad.
pub fn number_size(pad_height: f32) -> f32 {
    pad_height * 0.62
}

/// Titel neben der Nummer: umbrochen auf `width`, höchstens zwei Zeilen, Rest mit „…“.
pub fn title_job(text: &str, width: f32, color: Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::simple(
        text.to_string(),
        theme::label_font(13.0),
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
        let r = CornerRadius::same(theme::RADIUS_LG);
        let pressed = resp.is_pointer_button_down_on();
        let p = ui.painter();
        let accent = theme::accent();
        let base = if resp.hovered() {
            theme::surface_hover()
        } else {
            theme::surface()
        };
        let fill = pad_fill(state, base);
        if state == PadState::Playing {
            for (grow, a) in [(4.0, 24u8), (2.0, 44)] {
                p.rect_filled(
                    rect.expand(grow),
                    CornerRadius::same(theme::RADIUS_LG + 3),
                    with_alpha(accent, a),
                );
            }
        }
        p.rect_filled(rect, r, fill);
        if state != PadState::Empty {
            let inner = rect.shrink(1.0);
            let (top, bottom) = if pressed || state == PadState::Playing {
                (
                    with_alpha(Color32::BLACK, 100),
                    with_alpha(Color32::WHITE, 18),
                )
            } else {
                (
                    with_alpha(Color32::WHITE, 40),
                    with_alpha(Color32::BLACK, 110),
                )
            };
            p.hline(inner.x_range(), inner.top() + 3.0, Stroke::new(1.0, top));
            p.hline(
                inner.x_range(),
                inner.bottom() - 5.0,
                Stroke::new(1.0, bottom),
            );
        }
        let edge = match state {
            PadState::Playing => accent,
            PadState::Ready => theme::mix(theme::ok(), theme::border_strong(), 0.45),
            PadState::Empty => theme::border(),
        };
        p.rect_stroke(
            rect,
            r,
            Stroke::new(if state == PadState::Playing { 2.0 } else { 1.0 }, edge),
            StrokeKind::Inside,
        );
        let ink = match state {
            PadState::Playing => contrast_text(accent),
            PadState::Ready => theme::text(),
            PadState::Empty => theme::text_dim(),
        };
        // Große Nummer vorn, Titel daneben (zwei Zeilen), Tastenschild rechts oben.
        let number = p.layout_no_wrap(
            self.number.to_string(),
            theme::label_font(number_size(rect.height())),
            ink,
        );
        let body_mid = rect.center().y - 2.0;
        let num_pos = egui::pos2(rect.left() + 8.0, body_mid - number.size().y / 2.0);
        let text_left = num_pos.x + number.size().x + 8.0;
        p.galley(num_pos, number, ink);
        // Countdown rechts, groß; der Titel rückt dafür nur zusammen (bleibt zweizeilig).
        let mut text_right = rect.right() - 8.0;
        let count = self.remaining.filter(|_| state == PadState::Playing).map(countdown);
        if let Some(t) = &count {
            let g = p.layout_no_wrap(t.clone(), theme::label_font(18.0), ink);
            let at = egui::pos2(rect.right() - 8.0 - g.size().x, rect.bottom() - 8.0 - g.size().y);
            text_right = at.x - 6.0;
            p.galley(at, g, ink);
        }
        let shown = self.title.unwrap_or("leer");
        let title = p.layout_job(title_job(shown, text_right - text_left, ink));
        let title_pos = egui::pos2(text_left, body_mid - title.size().y / 2.0 + 4.0);
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
        // Status-LED rechts oben: laufend blinkt, belegt leuchtet in der Spurfarbe.
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
            PadState::Playing if blink => theme::on_accent(),
            PadState::Playing => with_alpha(theme::on_accent(), 70),
            PadState::Ready => self.color,
            PadState::Empty => theme::led_off(),
        };
        p.circle_filled(egui::pos2(rect.right() - 10.0, rect.top() + 10.0), 3.5, led);
        // Spurfarben-Balken unten.
        if state != PadState::Empty {
            let bar = Rect::from_min_max(
                egui::pos2(rect.left() + 6.0, rect.bottom() - 6.0),
                egui::pos2(rect.right() - 6.0, rect.bottom() - 3.0),
            );
            p.rect_filled(
                bar,
                CornerRadius::same(theme::RADIUS_XS),
                if state == PadState::Playing {
                    theme::on_accent()
                } else {
                    self.color
                },
            );
        }
        let label = match &count {
            Some(t) => format!("Cart {}: {} {t}", self.number, shown),
            None => format!("Cart {}: {}", self.number, shown),
        };
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #97: belegt grün, leer grau, laufend Modusfarbe; Nummer fast padhoch, Titel auf
    /// höchstens zwei Zeilen.
    #[test]
    fn filled_pads_are_green_empty_grey_and_titles_two_lines() {
        let base = theme::surface();
        let ready = pad_fill(PadState::Ready, base);
        assert!(
            ready.g() > ready.r() + 20 && ready.g() > ready.b() + 10,
            "{ready:?}"
        );
        assert_eq!(pad_fill(PadState::Empty, base), theme::well());
        assert_eq!(pad_fill(PadState::Playing, base), theme::accent());
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
    fn playing_wins_over_filled_and_empty_is_unfilled() {
        assert_eq!(pad_state(true, true), PadState::Playing);
        assert_eq!(pad_state(false, true), PadState::Playing);
        assert_eq!(pad_state(true, false), PadState::Ready);
        assert_eq!(pad_state(false, false), PadState::Empty);
    }
}
