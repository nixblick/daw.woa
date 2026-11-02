//! Knopf nur mit Symbol (`IconButton`): Werkzeuge in Clip-Inspektor, Dock, Spurkopf, Rack.
//! Jeder hat einen Tooltip mit Name und Tastenkürzel. Ruhe ohne Fläche, Symbol `text-dim`;
//! Hover mit Fläche `surface` und Rahmen `border`; „an“ in `accent-soft` mit Rahmen und Symbol
//! `accent`; Gefahr wird erst bei Hover rot (`on-air`); nicht verfügbar mit 35 % Deckkraft.
//! Die Beschriftung für Bildschirmleser und Tests ist das Symbol selbst.

use eframe::egui::{self, Color32, CornerRadius, Response, Sense, Stroke, StrokeKind};

use super::with_alpha;
use crate::theme;

/// Größe nach Design: 24 px (Symbol 14), 28 px (16), 32 px (18).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconSize {
    Sm,
    Md,
    Lg,
}

impl IconSize {
    /// Kantenlänge des Knopfs und Größe des Symbols.
    pub fn px(self) -> (f32, f32) {
        match self {
            Self::Sm => (24.0, 14.0),
            Self::Md => (28.0, 16.0),
            Self::Lg => (32.0, 18.0),
        }
    }
}

/// Farbrolle einer Fläche, eines Rahmens oder des Symbols (ohne Farben, damit testbar).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    None,
    TextDim,
    Text,
    Surface,
    Border,
    Accent,
    AccentSoft,
    OnAir,
}

/// Aussehen je Zustand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IconLook {
    pub fill: Ink,
    pub stroke: Ink,
    pub icon: Ink,
}

/// Aussehen aus Hover, „an“, Gefahr und fester Kappe (`solid`).
pub fn icon_look(hovered: bool, on: bool, danger: bool, solid: bool) -> IconLook {
    if on {
        return IconLook {
            fill: Ink::AccentSoft,
            stroke: Ink::Accent,
            icon: Ink::Accent,
        };
    }
    let icon = match (hovered, danger) {
        (true, true) => Ink::OnAir,
        (true, false) => Ink::Text,
        (false, _) => Ink::TextDim,
    };
    if hovered || solid {
        IconLook {
            fill: Ink::Surface,
            stroke: Ink::Border,
            icon,
        }
    } else {
        IconLook {
            fill: Ink::None,
            stroke: Ink::None,
            icon,
        }
    }
}

fn color(i: Ink) -> Color32 {
    match i {
        Ink::None => Color32::TRANSPARENT,
        Ink::TextDim => theme::text_dim(),
        Ink::Text => theme::text(),
        Ink::Surface => theme::surface(),
        Ink::Border => theme::border(),
        Ink::Accent => theme::accent(),
        Ink::AccentSoft => theme::accent_soft(),
        Ink::OnAir => theme::on_air(),
    }
}

/// `IconButton::new(ic::SCISSORS, "Teilen am Kopf (S)").ui(ui)`.
pub struct IconButton<'a> {
    icon: &'a str,
    tip: &'a str,
    size: IconSize,
    on: bool,
    danger: bool,
    solid: bool,
    enabled: bool,
}

impl<'a> IconButton<'a> {
    pub fn new(icon: &'a str, tip: &'a str) -> Self {
        Self {
            icon,
            tip,
            size: IconSize::Md,
            on: false,
            danger: false,
            solid: false,
            enabled: true,
        }
    }

    pub fn size(mut self, s: IconSize) -> Self {
        self.size = s;
        self
    }

    /// Schalter eingeschaltet (Loop, Tempo folgen, Fenster offen).
    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    /// Gefahr: erst bei Hover rot. Steht zuletzt in seiner Gruppe, mit Abstand.
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    /// Mit Tastenkappe, nur wo der Knopf allein steht.
    pub fn solid(mut self) -> Self {
        self.solid = true;
        self
    }

    pub fn enabled(mut self, e: bool) -> Self {
        self.enabled = e;
        self
    }

    pub fn ui(self, ui: &mut egui::Ui) -> Response {
        let (edge, glyph) = self.size.px();
        let sense = if self.enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(edge, edge), sense);
        let hovered = self.enabled && resp.hovered();
        let look = icon_look(hovered, self.on, self.danger, self.solid);
        if ui.is_rect_visible(rect) {
            let fade = |c: Color32| {
                if self.enabled {
                    c
                } else {
                    with_alpha(c, (f32::from(c.a()) * 0.35) as u8)
                }
            };
            let r = CornerRadius::same(theme::RADIUS_SM);
            let p = ui.painter();
            if look.fill != Ink::None {
                p.rect_filled(rect, r, fade(color(look.fill)));
            }
            if look.stroke != Ink::None {
                p.rect_stroke(
                    rect,
                    r,
                    Stroke::new(1.0, fade(color(look.stroke))),
                    StrokeKind::Inside,
                );
            }
            p.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                self.icon,
                egui::FontId::proportional(glyph),
                fade(color(look.icon)),
            );
        }
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, self.enabled, self.on, self.icon)
        });
        if self.enabled {
            resp.on_hover_text(self.tip)
        } else {
            resp.on_disabled_hover_text(self.tip)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rest_has_no_surface_and_dim_symbol() {
        let l = icon_look(false, false, false, false);
        assert_eq!(
            (l.fill, l.stroke, l.icon),
            (Ink::None, Ink::None, Ink::TextDim)
        );
    }

    #[test]
    fn hover_shows_surface_and_danger_turns_red_only_then() {
        let h = icon_look(true, false, false, false);
        assert_eq!(
            (h.fill, h.stroke, h.icon),
            (Ink::Surface, Ink::Border, Ink::Text)
        );
        assert_eq!(icon_look(false, false, true, false).icon, Ink::TextDim);
        assert_eq!(icon_look(true, false, true, false).icon, Ink::OnAir);
    }

    #[test]
    fn on_is_accent_soft_with_accent() {
        let l = icon_look(false, true, false, false);
        assert_eq!(
            (l.fill, l.stroke, l.icon),
            (Ink::AccentSoft, Ink::Accent, Ink::Accent)
        );
    }

    #[test]
    fn sizes_follow_the_design() {
        assert_eq!(IconSize::Sm.px(), (24.0, 14.0));
        assert_eq!(IconSize::Md.px(), (28.0, 16.0));
        assert_eq!(IconSize::Lg.px(), (32.0, 18.0));
    }
}
