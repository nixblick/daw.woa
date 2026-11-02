//! Spurkopf einer MIDI-Spur, Zeile 2 (TrackHeader Fassung 7): links der Instrument-Knopf
//! (`woa-inst`: Symbol der Kategorie in Spurfarbe, Name, Pfeil; öffnet die Klangauswahl im Rack),
//! rechts der MIDI-Eingang (`woa-midiin`, 104 px; Symbol leuchtet `cue` bei Noten, Rahmen
//! gestrichelt ohne Gerät). Beide 22 px hoch (Design 24).

use eframe::egui::{self, Color32, CornerRadius, Sense, Stroke, StrokeKind};

use crate::theme;

/// Höhe beider Knöpfe (Design 24 px; 22 px, damit der 64-px-Kopf unten eine freie Klickkante behält).
pub const HEAD_CTL_H: f32 = 22.0;
/// Breite des MIDI-Eingangs.
pub const MIDI_IN_W: f32 = 104.0;

/// Instrument-Knopf. `width`: verfügbare Breite.
pub fn instrument_button(
    ui: &mut egui::Ui,
    icon: &str,
    name: &str,
    color: Color32,
    width: f32,
) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(egui::vec2(width, HEAD_CTL_H), Sense::click());
    let label = format!("Instrument {name}");
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let radius = CornerRadius::same(theme::RADIUS_SM);
        p.rect_filled(
            rect,
            radius,
            if r.hovered() {
                theme::surface_hover()
            } else {
                theme::surface()
            },
        );
        p.rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, theme::border_strong()),
            StrokeKind::Inside,
        );
        let cy = rect.center().y;
        p.text(
            egui::pos2(rect.left() + 7.0, cy),
            egui::Align2::LEFT_CENTER,
            icon,
            egui::FontId::proportional(14.0),
            color,
        );
        let caret = p.text(
            egui::pos2(rect.right() - 6.0, cy),
            egui::Align2::RIGHT_CENTER,
            egui_phosphor::regular::CARET_DOWN,
            egui::FontId::proportional(11.0),
            theme::text_dim(),
        );
        let left = rect.left() + 7.0 + 14.0 + 6.0;
        let max_w = (caret.left() - 4.0 - left).max(0.0);
        let mut job = egui::text::LayoutJob::simple_singleline(
            name.to_string(),
            egui::FontId::proportional(12.0),
            theme::text(),
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(max_w);
        let g = ui.painter().layout_job(job);
        let h = g.size().y;
        ui.painter()
            .galley(egui::pos2(left, cy - h / 2.0), g, theme::text());
    }
    r.on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!("Instrument: {name} · Klick: Klang wählen"))
}

/// Zustand des MIDI-Eingangs im Spurkopf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiInLook {
    /// Kein Gerät gewählt oder nicht da: gestrichelter Rahmen.
    NoDevice,
    /// Gerät verbunden.
    Connected,
    /// Gerade kommen Noten an: Symbol leuchtet `cue`.
    Notes,
}

/// Breite des MIDI-Eingangs bei `width` für beide Knöpfe: 104 px, wenn Platz ist, sonst die
/// Hälfte, damit nichts in die Gain-/Pan-Felder ragt.
pub fn midi_in_width(width: f32, gap: f32) -> f32 {
    MIDI_IN_W.min(((width - gap) * 0.5).floor()).max(24.0)
}

/// MIDI-Eingang (`width` × 22). Der Klick öffnet die Gerätewahl (Aufrufer).
pub fn midi_in_button(
    ui: &mut egui::Ui,
    width: f32,
    text: &str,
    look: MidiInLook,
) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(egui::vec2(width, HEAD_CTL_H), Sense::click());
    let label = format!("MIDI-Eingang {text}");
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let radius = CornerRadius::same(theme::RADIUS_SM);
        p.rect_filled(
            rect,
            radius,
            if r.hovered() {
                theme::surface_hover()
            } else {
                theme::well()
            },
        );
        match look {
            MidiInLook::NoDevice => {
                let s = Stroke::new(1.0, theme::border_strong());
                let pts = [
                    rect.left_top(),
                    rect.right_top(),
                    rect.right_bottom(),
                    rect.left_bottom(),
                    rect.left_top(),
                ];
                for w in pts.windows(2) {
                    p.extend(egui::Shape::dashed_line(&[w[0], w[1]], s, 4.0, 3.0));
                }
            }
            MidiInLook::Connected | MidiInLook::Notes => {
                let c = if look == MidiInLook::Notes {
                    theme::cue()
                } else {
                    theme::border_strong()
                };
                p.rect_stroke(rect, radius, Stroke::new(1.0, c), StrokeKind::Inside);
            }
        }
        let cy = rect.center().y;
        let ic = if look == MidiInLook::Notes {
            theme::cue()
        } else {
            theme::text_dim()
        };
        p.text(
            egui::pos2(rect.left() + 6.0, cy),
            egui::Align2::LEFT_CENTER,
            egui_phosphor::regular::PIANO_KEYS,
            egui::FontId::proportional(14.0),
            ic,
        );
        let left = rect.left() + 6.0 + 14.0 + 5.0;
        let mut job = egui::text::LayoutJob::simple_singleline(
            text.to_string(),
            egui::FontId::proportional(11.0),
            theme::text_dim(),
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(rect.right() - 5.0 - left);
        let g = ui.painter().layout_job(job);
        let h = g.size().y;
        ui.painter()
            .galley(egui::pos2(left, cy - h / 2.0), g, theme::text_dim());
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midi_input_is_104_wide_and_both_fit_the_row() {
        let ctx = theme::test_context();
        let (mut a, mut b) = (egui::Rect::NOTHING, egui::Rect::NOTHING);
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            a = midi_in_button(ui, MIDI_IN_W, "Kein MIDI-Gerät", MidiInLook::NoDevice).rect;
            b = instrument_button(ui, "x", "Flügel", Color32::RED, 120.0).rect;
        });
        assert_eq!(a.size(), egui::vec2(104.0, HEAD_CTL_H));
        assert_eq!(b.height(), HEAD_CTL_H);
        assert_eq!(midi_in_width(400.0, 4.0), 104.0);
        assert_eq!(midi_in_width(110.0, 4.0), 53.0, "schmal: halbe Breite");
    }
}
