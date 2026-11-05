//! `woa-chip` (Design Fassung 7): der einzige Baustein für kleine Zustände. 26 px hoch (im
//! DJ-Kopf 28 px), Mono 12/700, Symbol 14 px in der Zustandsfarbe, Radius `radius-sm`.
//! Varianten `is-ok`, `is-warn`, `is-cue`, `is-alt`, `is-busy` (Symbol dreht sich); dazu die
//! Live-Farbe für ON AIR, Mikro und REC (Rot nur für live). Neutral: `well` mit
//! `border-strong`. Gefärbt: Rahmen und Symbol in der Farbe, Fläche 14–16 % davon.

use eframe::egui::{self, Color32, CornerRadius, Response, Sense, Stroke, StrokeKind};

use crate::theme;

/// Höhe eines Chips.
pub const CHIP_H: f32 = 26.0;
/// Höhe im DJ-Kopf.
pub const CHIP_H_DJ: f32 = 28.0;
/// Schriftgröße (Mono).
pub const CHIP_FONT: f32 = 12.0;
/// Symbolgröße.
pub const CHIP_ICON: f32 = 14.0;
/// Innenabstand links und rechts.
pub const CHIP_PAD_X: f32 = 9.0;
/// Abstand Symbol – Text.
pub const CHIP_GAP: f32 = 6.0;

/// Zustandsfarbe eines Chips.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChipTone {
    /// Ohne Zustand: `well`, `border-strong`, Symbol `text-dim`.
    Neutral,
    Ok,
    Warn,
    Cue,
    /// Zweitakzent (`accent-2`).
    Alt,
    /// Läuft gerade (Symbol dreht sich), Farbe `cue`.
    Busy,
    /// Live (ON AIR, Mikro offen, REC): `on-air`.
    Live,
    /// Eigene Farbe (Modus- oder Spurfarbe).
    Color(Color32),
}

impl ChipTone {
    /// Farbe von Rahmen und Symbol; `None` = neutral.
    pub fn color(self) -> Option<Color32> {
        match self {
            ChipTone::Neutral => None,
            ChipTone::Ok => Some(theme::ok()),
            ChipTone::Warn => Some(theme::warn()),
            ChipTone::Cue | ChipTone::Busy => Some(theme::cue()),
            ChipTone::Alt => Some(theme::accent_2()),
            ChipTone::Live => Some(theme::on_air()),
            ChipTone::Color(c) => Some(c),
        }
    }

    /// Anteil der Farbe an der Fläche (CSS: `is-ok` 16 %, sonst 14 %).
    fn fill_share(self) -> f32 {
        if self == ChipTone::Ok { 0.16 } else { 0.14 }
    }
}

/// Ein Chip. Ohne Symbol steht eine LED (7 px) in der Zustandsfarbe davor.
#[derive(Debug, Clone)]
pub struct StateChip<'a> {
    pub text: &'a str,
    pub icon: Option<&'a str>,
    pub tone: ChipTone,
    pub height: f32,
    /// Kleiner Vorsatz in Versalien (`<small>`), etwa „BPM“.
    pub small: Option<&'a str>,
    pub click: bool,
    /// Gefüllt (nur DJ-Modus, Moderator-Wunsch 05.11.2026): Fläche in der Zustandsfarbe, Schrift
    /// und Symbol in `lcd-bg`, damit die Badges im dunklen Club auffallen. Neutral bleibt neutral.
    pub filled: bool,
}

impl<'a> StateChip<'a> {
    pub fn new(text: &'a str, tone: ChipTone) -> Self {
        Self {
            text,
            icon: None,
            tone,
            height: CHIP_H,
            small: None,
            click: false,
            filled: false,
        }
    }

    pub fn icon(mut self, icon: &'a str) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Höhe im DJ-Kopf (28 px).
    pub fn dj(mut self) -> Self {
        self.height = CHIP_H_DJ;
        self
    }

    /// Gefüllte Fläche statt Rahmen (nur im DJ-Modus verwenden).
    pub fn filled(mut self) -> Self {
        self.filled = true;
        self
    }

    pub fn small(mut self, s: &'a str) -> Self {
        self.small = Some(s);
        self
    }

    pub fn clickable(mut self) -> Self {
        self.click = true;
        self
    }

    /// Fläche, Rahmen, Symbol/LED und Schrift. `hovered`: anklickbarer Chip unter der Maus.
    pub fn colors(&self, hovered: bool) -> (Color32, Color32, Color32, Color32) {
        let c = self.tone.color();
        match c {
            Some(c) if self.filled => (c, c, theme::lcd_bg(), theme::lcd_bg()),
            Some(c) => (
                theme::mix(c, theme::panel(), 1.0 - self.tone.fill_share()),
                c,
                c,
                theme::text(),
            ),
            None => (
                theme::well(),
                if hovered {
                    theme::text_dim()
                } else {
                    theme::border_strong()
                },
                theme::text_dim(),
                theme::text(),
            ),
        }
    }

    pub fn show(self, ui: &mut egui::Ui) -> Response {
        let font = egui::FontId::monospace(CHIP_FONT);
        let small_font = theme::label_font(10.0);
        let text_g =
            ui.painter()
                .layout_no_wrap(self.text.to_string(), font.clone(), theme::text());
        let small_g = self.small.map(|s| {
            ui.painter()
                .layout_no_wrap(s.to_uppercase(), small_font.clone(), theme::text_dim())
        });
        let lead = if self.icon.is_some() { CHIP_ICON } else { 7.0 };
        let mut w = CHIP_PAD_X * 2.0 + lead + CHIP_GAP + text_g.size().x;
        if let Some(g) = &small_g {
            w += g.size().x + 4.0;
        }
        let sense = if self.click {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, r) = ui.allocate_exact_size(egui::vec2(w, self.height), sense);
        let label = match self.small {
            Some(s) => format!("{s} {}", self.text),
            None => self.text.to_string(),
        };
        let wt = if self.click {
            egui::WidgetType::Button
        } else {
            egui::WidgetType::Label
        };
        r.widget_info(|| egui::WidgetInfo::labeled(wt, true, &label));
        if !ui.is_rect_visible(rect) {
            return r;
        }
        let p = ui.painter();
        let (fill, border, ink, text_ink) = self.colors(self.click && r.hovered());
        let radius = CornerRadius::same(theme::RADIUS_SM);
        p.rect_filled(rect, radius, fill);
        p.rect_stroke(rect, radius, Stroke::new(1.0, border), StrokeKind::Inside);
        let mut x = rect.left() + CHIP_PAD_X;
        let cy = rect.center().y;
        match self.icon {
            Some(icon) => {
                let pos = egui::pos2(x + CHIP_ICON / 2.0, cy);
                if self.tone == ChipTone::Busy && crate::widgets::motion(ui.ctx()) {
                    // Kreisel: das Symbol dreht sich (CSS `woa-spin`, 1 s je Runde).
                    let t = ui.input(|i| i.time) as f32;
                    let g = ui.painter().layout_no_wrap(
                        icon.to_string(),
                        egui::FontId::proportional(CHIP_ICON),
                        ink,
                    );
                    let angle = t * std::f32::consts::TAU;
                    let half = g.size() / 2.0;
                    let rot = egui::emath::Rot2::from_angle(angle);
                    let origin = pos - rot * half;
                    p.add(egui::epaint::TextShape::new(origin, g, ink).with_angle(angle));
                    ui.ctx().request_repaint();
                } else {
                    p.text(
                        pos,
                        egui::Align2::CENTER_CENTER,
                        icon,
                        egui::FontId::proportional(CHIP_ICON),
                        ink,
                    );
                }
                x += CHIP_ICON + CHIP_GAP;
            }
            None => {
                p.circle_filled(egui::pos2(x + 3.5, cy), 3.2, ink);
                x += 7.0 + CHIP_GAP;
            }
        }
        if let Some(g) = small_g {
            let gw = g.size().x;
            let small_ink = if text_ink == theme::text() {
                theme::text_dim()
            } else {
                text_ink
            };
            p.galley_with_override_text_color(egui::pos2(x, cy - g.size().y / 2.0), g, small_ink);
            x += gw + 4.0;
        }
        let th = text_g.size().y;
        p.galley_with_override_text_color(egui::pos2(x, cy - th / 2.0), text_g, text_ink);
        r
    }
}

/// Kurzform: Chip mit LED in der Farbe `color` (an) oder neutral (aus).
pub fn chip(ui: &mut egui::Ui, text: &str, color: Color32, active: bool) -> Response {
    let tone = if active {
        ChipTone::Color(color)
    } else {
        ChipTone::Neutral
    };
    StateChip::new(text, tone).show(ui)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size_of(chip: StateChip<'_>) -> egui::Vec2 {
        let ctx = theme::test_context();
        let mut out = egui::Vec2::ZERO;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            out = chip.clone().show(ui).rect.size();
        });
        out
    }

    /// DJ-Modus (Moderator 05.11.2026): gefüllte Badges, Neutral bleibt neutral; ohne
    /// `filled` bleibt jeder Chip umrandet.
    #[test]
    fn filled_chips_are_solid_only_when_asked() {
        let _ctx = theme::test_context();
        let on = StateChip::new("LÄUFT", ChipTone::Color(theme::accent())).filled();
        let (fill, border, ink, text) = on.colors(false);
        assert_eq!(fill, theme::accent());
        assert_eq!(border, theme::accent());
        assert_eq!((ink, text), (theme::lcd_bg(), theme::lcd_bg()));
        let plain = StateChip::new("LÄUFT", ChipTone::Color(theme::accent()));
        assert_ne!(plain.colors(false).0, theme::accent(), "ohne filled umrandet");
        let neutral = StateChip::new("LEER", ChipTone::Neutral).filled();
        assert_eq!(neutral.colors(false).0, theme::well());
    }

    #[test]
    fn chip_is_26_px_and_28_in_the_dj_head() {
        assert_eq!(size_of(StateChip::new("128.0", ChipTone::Ok)).y, CHIP_H);
        assert_eq!(
            size_of(StateChip::new("8A", ChipTone::Ok).dj()).y,
            CHIP_H_DJ
        );
        assert_eq!(CHIP_H, 26.0);
        assert_eq!(CHIP_H_DJ, 28.0);
    }

    #[test]
    fn live_is_the_only_red_tone() {
        for t in [
            ChipTone::Neutral,
            ChipTone::Ok,
            ChipTone::Warn,
            ChipTone::Cue,
            ChipTone::Alt,
            ChipTone::Busy,
        ] {
            assert_ne!(t.color(), Some(theme::on_air()), "{t:?} ist nicht rot");
        }
        assert_eq!(ChipTone::Live.color(), Some(theme::on_air()));
    }
}
