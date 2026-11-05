//! Meldungszeile am Fuß des Sende-Decks (`StatusLine`, Design-Fassung 8): eine 28-px-Zeile
//! statt roter Fließtexte. Vorn das Warnsymbol, dann der fette Kurztext und das Detail; bei
//! mehreren Meldungen rechts ein Zähler-Chip „n Hinweise“, der die Liste aufklappt.

use eframe::egui;

use crate::theme;
use crate::widgets::chip::{ChipTone, StateChip};

/// Höhe der Zeile.
pub const STATUS_LINE_H: f32 = 28.0;

/// Eine Meldung.
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    /// Fehler (rot) statt Hinweis (gelb).
    pub error: bool,
    /// Kurztext, fett, etwa „Stream: neu verbunden“.
    pub title: String,
    /// Detail dahinter, gedimmt.
    pub detail: String,
    /// Mit Schließen-Knopf (nur Meldungen, die der Nutzer wegklicken darf).
    pub dismiss: bool,
}

impl Notice {
    pub fn warn(title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            error: false,
            title: title.into(),
            detail: detail.into(),
            dismiss: false,
        }
    }

    pub fn error(title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            error: true,
            ..Self::warn(title, detail)
        }
    }
}

/// Was der Nutzer in der Zeile getan hat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StatusAction {
    /// Index der weggeklickten Meldung.
    pub dismissed: Option<usize>,
}

/// Fehler zuerst, sonst in der gegebenen Reihenfolge.
fn order(notices: &[Notice]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..notices.len()).collect();
    idx.sort_by_key(|&i| !notices[i].error);
    idx
}

/// Zeichnet die Zeile (immer 28 px, ohne Meldung leer und ruhig) und bei `open` die Liste
/// aller Meldungen darunter.
pub fn status_line(ui: &mut egui::Ui, notices: &[Notice], open: &mut bool) -> StatusAction {
    let mut act = StatusAction::default();
    let ord = order(notices);
    if notices.len() < 2 {
        *open = false;
    }
    let r = ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), STATUS_LINE_H),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_height(STATUS_LINE_H);
            ui.spacing_mut().item_spacing.x = 6.0;
            let Some(&first) = ord.first() else {
                ui.label(
                    egui::RichText::new(format!("{} Keine Meldungen", theme::ic::CHECK_CIRCLE))
                        .size(12.0)
                        .color(theme::text_dim()),
                );
                return;
            };
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if notices.len() > 1 {
                    let text = format!("{} Hinweise", notices.len());
                    let tone = if notices.iter().any(|n| n.error) {
                        ChipTone::Live
                    } else {
                        ChipTone::Warn
                    };
                    let mut chip = StateChip::new(&text, tone).clickable();
                    chip.height = 20.0;
                    let c = chip.show(ui).on_hover_text("Alle Meldungen zeigen");
                    if c.clicked() {
                        *open = !*open;
                    }
                }
                if notices[first].dismiss
                    && ui
                        .small_button(theme::ic::X)
                        .on_hover_text("Meldung schließen")
                        .clicked()
                {
                    act.dismissed = Some(first);
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    notice_row(ui, &notices[first]);
                });
            });
        },
    );
    let full = notices
        .iter()
        .map(|n| format!("{} {}", n.title, n.detail))
        .collect::<Vec<_>>()
        .join("\n");
    let label = ord
        .first()
        .map(|&i| format!("Meldung: {}", notices[i].title))
        .unwrap_or_else(|| "Meldung: keine".into());
    r.response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
    if !full.is_empty() {
        r.response.on_hover_text(full);
    }
    if *open {
        theme::lcd_show(ui, |ui| {
            ui.set_width(ui.available_width());
            for &i in &ord {
                ui.horizontal(|ui| {
                    notice_row(ui, &notices[i]);
                    if notices[i].dismiss
                        && ui
                            .small_button(theme::ic::X)
                            .on_hover_text("Meldung schließen")
                            .clicked()
                    {
                        act.dismissed = Some(i);
                    }
                });
            }
        });
    }
    act
}

fn notice_row(ui: &mut egui::Ui, n: &Notice) {
    let color = if n.error {
        theme::on_air()
    } else {
        theme::warn()
    };
    ui.label(
        egui::RichText::new(theme::icf::WARNING)
            .family(egui::FontFamily::Name(theme::FAMILY_ICON_FILL.into()))
            .size(14.0)
            .color(color),
    );
    ui.label(
        egui::RichText::new(&n.title)
            .size(12.0)
            .strong()
            .color(theme::text()),
    );
    if !n.detail.is_empty() {
        ui.add(
            egui::Label::new(
                egui::RichText::new(&n.detail)
                    .size(12.0)
                    .color(theme::text_dim()),
            )
            .truncate(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn errors_come_first() {
        let n = [
            Notice::warn("Stream", "neu verbunden"),
            Notice::error("Telefon", "Gerät fehlt"),
        ];
        assert_eq!(order(&n), vec![1, 0]);
    }

    #[test]
    fn one_line_and_counter_for_many() {
        let notices = vec![
            Notice::warn("Stream: neu verbunden", "41 Lücken seit Start"),
            Notice::error("Telefon", "Gerät fehlt"),
        ];
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(900.0, 200.0))
            .build_ui_state(
                move |ui, open: &mut bool| {
                    let ctx = ui.ctx().clone();
                    let id = egui::Id::new("test_fonts");
                    if ctx.data_mut(|m| m.get_temp::<bool>(id)).is_none() {
                        ctx.data_mut(|m| m.insert_temp(id, true));
                        theme::install_fonts(&ctx);
                        return;
                    }
                    let _ = status_line(ui, &notices, open);
                },
                false,
            );
        h.run_steps(3);
        h.get_by_label("Meldung: Telefon");
        h.get_by_label("2 Hinweise").click();
        h.run_steps(2);
        assert!(*h.state(), "Zähler klappt die Liste auf");
    }
}
