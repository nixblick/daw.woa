//! Segment eines Umschalters (Design `Segmented`, Design-Runde 3): Symbol plus Beschriftung,
//! gewählt = `accent-soft` mit Rand `accent` und Schrift `text`, sonst Mulde `well` mit Rand
//! `border` und Schrift `text-dim`; unter der Maus `surface-hover`. Die Dock-Reiter des
//! Produzenten nutzen ihn. Die Beschriftung für Bildschirmleser ist der Text ohne Symbol.

use eframe::egui::{self, CornerRadius, Response, Sense, Stroke, StrokeKind};

use super::icon_button::Ink;
use crate::theme;

/// Fläche, Rand und Schrift je Zustand.
pub fn segment_look(on: bool, hovered: bool) -> (Ink, Ink, Ink) {
    match (on, hovered) {
        (true, _) => (Ink::AccentSoft, Ink::Accent, Ink::Text),
        (false, true) => (Ink::Surface, Ink::Border, Ink::Text),
        (false, false) => (Ink::Well, Ink::Border, Ink::TextDim),
    }
}

/// `Segment::new(ic::PIANO_KEYS, "Piano-Roll").on(open).ui(ui)`.
pub struct Segment<'a> {
    icon: &'a str,
    label: &'a str,
    on: bool,
    height: f32,
}

impl<'a> Segment<'a> {
    pub fn new(icon: &'a str, label: &'a str) -> Self {
        Self {
            icon,
            label,
            on: false,
            height: 32.0,
        }
    }

    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    pub fn height(mut self, h: f32) -> Self {
        self.height = h;
        self
    }

    pub fn ui(self, ui: &mut egui::Ui) -> Response {
        let font = egui::FontId::proportional(13.0);
        let text = if self.icon.is_empty() {
            self.label.to_owned()
        } else {
            format!("{} {}", self.icon, self.label)
        };
        let galley = ui.painter().layout_no_wrap(text, font, theme::text());
        let size = egui::vec2(galley.size().x + 20.0, self.height);
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        let (fill, stroke, ink) = segment_look(self.on, resp.hovered());
        if ui.is_rect_visible(rect) {
            let r = CornerRadius::same(theme::RADIUS_SM);
            let p = ui.painter();
            p.rect_filled(rect, r, super::icon_button::ink_color(fill));
            p.rect_stroke(
                rect,
                r,
                Stroke::new(1.0, super::icon_button::ink_color(stroke)),
                StrokeKind::Inside,
            );
            p.galley(
                rect.center() - galley.size() / 2.0,
                galley,
                super::icon_button::ink_color(ink),
            );
        }
        let label = self.label.to_owned();
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, self.on, &label)
        });
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chosen_segment_is_accent_soft_with_accent_edge() {
        assert_eq!(
            segment_look(true, false),
            (Ink::AccentSoft, Ink::Accent, Ink::Text)
        );
        assert_eq!(segment_look(true, true).0, Ink::AccentSoft);
    }

    #[test]
    fn idle_segment_sits_in_the_well_dimmed() {
        assert_eq!(
            segment_look(false, false),
            (Ink::Well, Ink::Border, Ink::TextDim)
        );
        assert_eq!(segment_look(false, true).2, Ink::Text);
    }
}
