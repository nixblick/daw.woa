//! Kopfleisten-Block `status_bar` (TopBar-README, Design-Runde 2, André 01.11.2026): eine
//! eingelassene Leiste mit drei Gruppen (Modus · Status · Feedback), getrennt durch 1-px-Linien.
//! Jeder Eintrag hat denselben Stil: 28 px hoch, Versalien, LED-Punkt links. Nur die Farbe von
//! Punkt und Rahmen unterscheidet die Funktion. Aus: LED gedimmt, Text `text-dim`, keine Fläche.
//! An: Fläche `surface`, Rahmen und LED in der eigenen Farbe, Text `text`. Kein Glow, keine
//! gefüllten Farbflächen. Auf schmalem Fenster zeigt jeder Eintrag statt des Worts sein Symbol;
//! das Wort steht dann im Tooltip und bleibt für Bildschirmleser die Beschriftung.

use eframe::egui::{self, Color32, CornerRadius, Response, Sense, Stroke, StrokeKind};

use crate::theme;

/// Höhe jedes Eintrags (`.woa-sb`).
pub const TOPBAR_ITEM_H: f32 = 28.0;
/// Symbolgröße (Feedback, schmale Ansicht).
pub const TOPBAR_ICON: f32 = 14.0;
/// Schriftgröße des Worts.
pub const TOPBAR_FONT: f32 = 13.0;
/// Innenabstand links und rechts.
pub const TOPBAR_PAD_X: f32 = 11.0;
/// Abstand zwischen LED (oder Symbol) und Wort.
pub const TOPBAR_ICON_GAP: f32 = 7.0;
/// Durchmesser der LED.
pub const TOPBAR_LED: f32 = 7.0;
/// Radius jedes Eintrags.
pub const TOPBAR_RADIUS: u8 = theme::RADIUS_SM;
/// Abstand zwischen den Einträgen im Block.
pub const TOPBAR_GAP: f32 = 3.0;
/// Abstand zwischen den Segmenten des Modus-Umschalters (wie alle Einträge).
pub const TOPBAR_SEGMENT_GAP: f32 = TOPBAR_GAP;
/// Innenabstand des Blocks.
pub const TOPBAR_BAR_PAD: f32 = 3.0;
/// Abstand links und rechts der Trennlinie zwischen den Gruppen.
pub const TOPBAR_SEP_MARGIN: f32 = 4.0;
/// Abstand zwischen dem Block und der Update-Info links davon.
pub const TOPBAR_GROUP_GAP: f32 = theme::SPACE_6;
/// Unter dieser freien Breite zeigen die Einträge nur ihr Symbol.
pub const TOPBAR_COMPACT_BELOW: f32 = 760.0;

/// Aussehen eines Eintrags; die Farbe ist die eigene Farbe des Eintrags (Modus, MIDI, ON AIR …).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChipLook {
    /// Aus: LED gedimmt in der eigenen Farbe, keine Fläche.
    Off(Color32),
    /// An: Fläche `surface`, Rahmen und LED in der Farbe.
    On(Color32),
    /// Neutral mit Symbol statt LED (Feedback).
    Plain,
}

/// Ein Eintrag „LED Wort“ (`.woa-sb`). `icon` steht in der schmalen Ansicht statt des Worts und
/// bei [`ChipLook::Plain`] statt der LED. `click` macht ihn zum Knopf.
pub fn topbar_chip(
    ui: &mut egui::Ui,
    icon: &str,
    word: &str,
    look: ChipLook,
    compact: bool,
    click: bool,
) -> Response {
    Chip {
        icon,
        word,
        look,
        compact,
        click,
        suffix: None,
        blink: false,
    }
    .show(ui)
}

/// Eintrag mit allen Möglichkeiten: `suffix` steht klein in Mono hinter dem Wort (REC-Laufzeit),
/// `blink` lässt die LED im Sekundentakt blinken (REC läuft).
pub struct Chip<'a> {
    pub icon: &'a str,
    pub word: &'a str,
    pub look: ChipLook,
    pub compact: bool,
    pub click: bool,
    pub suffix: Option<&'a str>,
    pub blink: bool,
}

impl Chip<'_> {
    pub fn show(self, ui: &mut egui::Ui) -> Response {
        let (on, color) = match self.look {
            ChipLook::Off(c) => (false, c),
            ChipLook::On(c) => (true, c),
            ChipLook::Plain => (false, theme::text()),
        };
        let ink = if on { theme::text() } else { theme::text_dim() };
        let plain = self.look == ChipLook::Plain;
        // Vorn: LED oder (neutral bzw. schmal) das Symbol.
        let show_icon = plain || self.compact;
        let icon_g = show_icon.then(|| {
            ui.painter().layout_no_wrap(
                self.icon.to_string(),
                egui::FontId::proportional(TOPBAR_ICON),
                ink,
            )
        });
        let word_g = (!self.compact).then(|| {
            let mut job = egui::text::LayoutJob::default();
            job.append(
                &self.word.to_uppercase(),
                0.0,
                egui::TextFormat {
                    font_id: theme::label_font(TOPBAR_FONT),
                    color: ink,
                    extra_letter_spacing: 0.06 * TOPBAR_FONT,
                    ..Default::default()
                },
            );
            if let Some(s) = self.suffix {
                job.append(
                    s,
                    6.0,
                    egui::TextFormat {
                        font_id: egui::FontId::monospace(11.0),
                        color: theme::text_dim(),
                        valign: egui::Align::Center,
                        ..Default::default()
                    },
                );
            }
            ui.painter().layout_job(job)
        });
        let led = !plain;
        let mut w = TOPBAR_PAD_X * 2.0;
        if led {
            w += TOPBAR_LED;
        }
        if let Some(g) = &icon_g {
            w += g.size().x + if led { TOPBAR_ICON_GAP } else { 0.0 };
        }
        if let Some(g) = &word_g {
            w += TOPBAR_ICON_GAP + g.size().x;
        }
        let sense = if self.click {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, TOPBAR_ITEM_H), sense);
        if ui.is_rect_visible(rect) {
            let p = ui.painter();
            let r = CornerRadius::same(TOPBAR_RADIUS);
            if on {
                p.rect_filled(rect, r, theme::surface());
                p.rect_stroke(rect, r, Stroke::new(1.0, color), StrokeKind::Inside);
            } else if self.click && resp.hovered() {
                p.rect_filled(rect, r, theme::mix(theme::surface(), theme::well(), 0.55));
            }
            let cy = rect.center().y;
            let mut x = rect.left() + TOPBAR_PAD_X;
            if led {
                let lit = on && (!self.blink || super::blink_on(ui.input(|i| i.time), 1.0));
                let fill = if lit {
                    color
                } else {
                    theme::mix(color, theme::led_off(), 0.30)
                };
                p.circle_filled(egui::pos2(x + TOPBAR_LED / 2.0, cy), TOPBAR_LED / 2.0, fill);
                x += TOPBAR_LED;
                if icon_g.is_some() {
                    x += TOPBAR_ICON_GAP;
                }
            }
            if let Some(g) = icon_g {
                let gw = g.size().x;
                p.galley(egui::pos2(x, cy - g.size().y / 2.0), g, ink);
                x += gw;
            }
            if let Some(g) = word_g {
                x += TOPBAR_ICON_GAP;
                p.galley(egui::pos2(x, cy - g.size().y / 2.0), g, ink);
            }
        }
        if self.blink {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
        }
        let kind = if self.click {
            egui::WidgetType::Button
        } else {
            egui::WidgetType::Label
        };
        let label = self.word.to_string();
        resp.widget_info(|| egui::WidgetInfo::labeled(kind, true, label.clone()));
        resp
    }
}

/// Der eingelassene Block (`.woa-strip-bar`): `well`, 1-px-Rahmen, 3 px Innenabstand, Einträge
/// im Abstand [`TOPBAR_GAP`]. Gruppen trennt [`status_sep`].
pub fn status_bar<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(theme::well())
        .stroke(Stroke::new(1.0, theme::border()))
        .corner_radius(theme::RADIUS_MD)
        .inner_margin(egui::Margin::same(TOPBAR_BAR_PAD as i8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = TOPBAR_GAP;
                add(ui)
            })
            .inner
        })
        .inner
}

/// 1-px-Trennlinie zwischen zwei Gruppen im Block (20 px hoch).
pub fn status_sep(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(1.0 + 2.0 * TOPBAR_SEP_MARGIN, TOPBAR_ITEM_H),
        Sense::hover(),
    );
    let line = egui::Rect::from_center_size(rect.center(), egui::vec2(1.0, 20.0));
    ui.painter().rect_filled(line, 0.0, theme::border());
}
