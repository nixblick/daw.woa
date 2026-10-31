//! Beleuchtete Hardware-Taste (`Key`): erhaben, gedrückt eingerastet, `lit` = aktiv,
//! `on` = voll an. Der Zustand zeigt die Funktion, nicht die Maus.

use eframe::egui::{
    self, Color32, CornerRadius, FontId, Response, Sense, Stroke, StrokeKind, Vec2,
};

use super::{contrast_text, with_alpha};
use crate::theme;

/// Aussehen einer Taste je Zustand (ohne Farben, damit es testbar bleibt).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyLook {
    /// Steht die Taste hoch (Schatten unten) oder liegt sie tief (Schatten oben)?
    pub raised: bool,
    /// Anteil der Leuchtfarbe in der Fläche, 0 = Tastenfarbe, 1 = ganz Leuchtfarbe.
    pub fill: f32,
    /// Lichtschein um die Taste.
    pub glow: bool,
}

/// Aussehen für `lit` (aktiv), `on` (voll an, eingerastet) und `pressed` (gerade gedrückt).
pub fn key_look(lit: bool, on: bool, pressed: bool) -> KeyLook {
    KeyLook {
        raised: !pressed && !on,
        fill: if on {
            1.0
        } else if lit {
            0.38
        } else {
            0.0
        },
        glow: lit || on,
    }
}

/// Taste mit Beschriftung. `Key::new("Start", vec2(80.0, 32.0)).lit(true).ui(ui)`.
pub struct Key<'a> {
    label: &'a str,
    size: Vec2,
    lit: bool,
    on: bool,
    color: Option<Color32>,
    font: Option<FontId>,
    radius: u8,
    enabled: bool,
}

impl<'a> Key<'a> {
    pub fn new(label: &'a str, size: Vec2) -> Self {
        Self {
            label,
            size,
            lit: false,
            on: false,
            color: None,
            font: None,
            radius: theme::RADIUS_SM,
            enabled: true,
        }
    }

    /// Aktiv: getönte Fläche mit Lichtschein.
    pub fn lit(mut self, lit: bool) -> Self {
        self.lit = lit;
        self
    }

    /// Voll an: eingerastet in der Leuchtfarbe.
    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    /// Leuchtfarbe (Vorgabe: Modusfarbe).
    pub fn color(mut self, c: Color32) -> Self {
        self.color = Some(c);
        self
    }

    pub fn font(mut self, f: FontId) -> Self {
        self.font = Some(f);
        self
    }

    pub fn radius(mut self, r: u8) -> Self {
        self.radius = r;
        self
    }

    pub fn enabled(mut self, e: bool) -> Self {
        self.enabled = e;
        self
    }

    pub fn ui(self, ui: &mut egui::Ui) -> Response {
        let sense = if self.enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, resp) = ui.allocate_exact_size(self.size, sense);
        let pressed = self.enabled && resp.is_pointer_button_down_on();
        let look = key_look(self.lit, self.on, pressed);
        let lit_col = self.color.unwrap_or_else(theme::accent);
        let base = if resp.hovered() && self.enabled {
            theme::surface_hover()
        } else {
            theme::surface()
        };
        let fill = theme::mix(base, lit_col, look.fill);
        let r = CornerRadius::same(self.radius);
        let p = ui.painter();
        if look.glow && ui.is_rect_visible(rect) {
            for (grow, a) in [(4.0, 22u8), (2.0, 38)] {
                p.rect_filled(
                    rect.expand(grow),
                    CornerRadius::same(self.radius + 3),
                    with_alpha(lit_col, a),
                );
            }
        }
        p.rect_filled(rect, r, fill);
        // Kanten: erhaben = Licht oben, Schatten unten; tief = Schatten oben.
        let inner = rect.shrink(1.0);
        let (top, bottom) = if look.raised {
            (
                with_alpha(Color32::WHITE, 46),
                with_alpha(Color32::BLACK, 120),
            )
        } else {
            (
                with_alpha(Color32::BLACK, 120),
                with_alpha(Color32::WHITE, 20),
            )
        };
        p.hline(inner.x_range(), inner.top(), Stroke::new(1.0, top));
        p.hline(
            inner.x_range(),
            inner.bottom(),
            Stroke::new(if look.raised { 2.0 } else { 1.0 }, bottom),
        );
        let edge = if look.glow {
            lit_col
        } else {
            theme::border_strong()
        };
        p.rect_stroke(rect, r, Stroke::new(1.0, edge), StrokeKind::Inside);
        let nudge = if look.raised { 0.0 } else { 1.0 };
        let ink = if !self.enabled {
            theme::text_dim()
        } else if look.fill > 0.9 {
            contrast_text(fill)
        } else {
            theme::text()
        };
        let font = self.font.clone().unwrap_or_else(|| theme::label_font(14.0));
        p.text(
            rect.center() + egui::vec2(0.0, nudge),
            egui::Align2::CENTER_CENTER,
            self.label,
            font,
            ink,
        );
        resp.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Button,
                self.enabled,
                self.on || self.lit,
                self.label,
            )
        });
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_key_stands_up_without_light() {
        let k = key_look(false, false, false);
        assert!(k.raised && !k.glow);
        assert_eq!(k.fill, 0.0);
    }

    #[test]
    fn pressed_and_latched_keys_sit_low() {
        assert!(!key_look(false, false, true).raised);
        let on = key_look(true, true, false);
        assert!(!on.raised && on.glow);
        assert_eq!(on.fill, 1.0);
    }

    #[test]
    fn lit_is_between_idle_and_on() {
        let lit = key_look(true, false, false);
        assert!(lit.raised && lit.glow);
        assert!(lit.fill > 0.0 && lit.fill < 1.0);
    }
}
