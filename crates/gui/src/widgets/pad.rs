//! Cart-Pad der Cart-Wall: Tastennummer, Titel, Status-LED und Spurfarben-Balken. Das
//! laufende Pad leuchtet in der Modusfarbe und blinkt als einziges Pad.

use eframe::egui::{self, Align2, Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind, Vec2};

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

/// Ein Pad. `key` ist der Tastenname in der Ecke (nicht die Nummer), `capturing` zeigt „?“.
pub struct CartPad<'a> {
    pub number: usize,
    pub title: Option<&'a str>,
    pub key: Option<&'a str>,
    pub capturing: bool,
    pub playing: bool,
    pub color: Color32,
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
        let fill = match state {
            PadState::Playing => accent,
            PadState::Ready => theme::mix(self.color, base, 0.82),
            PadState::Empty => theme::well(),
        };
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
            PadState::Ready => theme::mix(self.color, theme::border_strong(), 0.45),
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
        // Große Nummer und Titel.
        p.text(
            rect.center() + egui::vec2(0.0, -8.0),
            Align2::CENTER_CENTER,
            self.number.to_string(),
            theme::label_font(26.0),
            ink,
        );
        let shown = self.title.unwrap_or("leer");
        let title_rect = Rect::from_center_size(
            rect.center() + egui::vec2(0.0, 16.0),
            egui::vec2(rect.width() - 10.0, 14.0),
        );
        p.with_clip_rect(title_rect).text(
            title_rect
                .left_center()
                .lerp(title_rect.right_center(), 0.5),
            Align2::CENTER_CENTER,
            shown,
            theme::label_font(13.0),
            ink,
        );
        // Tastenschild links oben.
        if self.capturing || self.key.is_some() {
            let text = if self.capturing {
                "?"
            } else {
                self.key.unwrap_or("")
            };
            let galley = p.layout_no_wrap(
                text.to_string(),
                theme::label_font(12.0),
                if self.capturing {
                    accent
                } else {
                    theme::text()
                },
            );
            let tag = Rect::from_min_size(
                rect.left_top() + egui::vec2(5.0, 5.0),
                galley.size() + egui::vec2(8.0, 2.0),
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
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playing_wins_over_filled_and_empty_is_unfilled() {
        assert_eq!(pad_state(true, true), PadState::Playing);
        assert_eq!(pad_state(false, true), PadState::Playing);
        assert_eq!(pad_state(true, false), PadState::Ready);
        assert_eq!(pad_state(false, false), PadState::Empty);
    }
}
