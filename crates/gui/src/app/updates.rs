//! Update-Hinweis (siehe `docs/updater.md`). Netz nur in Hintergrund-Threads.

use super::*;

impl WoaApp {
    pub(super) fn update_wake(ctx: &egui::Context) -> impl Fn() + Send + 'static {
        let ctx = ctx.clone();
        move || ctx.request_repaint()
    }

    /// Jedes Bild: beim Start und dann stündlich suchen, Ergebnisse abholen.
    pub(super) fn update_frame(&mut self, ctx: &egui::Context) {
        use crate::update::{Event, check_due, unix_now};
        if self.update_autostart {
            self.update_autostart = false;
            self.update_periodic = true;
            // #452/#497: alte Installation für alle Benutzer ablösen und Claudes Registrierung
            // umstellen. Beides liest Registry und Dateien (`.claude.json` kann groß sein), daher
            // im Hintergrund, damit das erste Bild nicht stockt.
            #[cfg(windows)]
            if self.photo.is_none() {
                let (tx, rx) = std::sync::mpsc::channel();
                let wake = Self::update_wake(ctx);
                let spawned = std::thread::Builder::new()
                    .name("woa-startup-repair".into())
                    .spawn(move || {
                        woa_updater::retire_per_machine(&woa_radio::data_dir());
                        let _ = tx.send(woa_updater::repair_claude_registration());
                        wake();
                    });
                match spawned {
                    Ok(_) => self.startup_repair = Some(rx),
                    Err(e) => tracing::warn!(error = %e, "startup repair thread not started"),
                }
            }
        }
        // Beim Start und danach stündlich, solange WOA läuft. Ein gescheiterter Versuch
        // wartet ebenfalls eine Stunde (`update_tried`).
        let interval = std::time::Duration::from_secs(crate::update::CHECK_INTERVAL_SECS);
        if self.config.auto_update
            && self.photo.is_none()
            && matches!(self.update.phase, crate::update::Phase::Idle)
            && self.update_periodic
            && self.update_tried.is_none_or(|t| t.elapsed() >= interval)
            && check_due(self.config.last_update_check, unix_now())
        {
            self.update_tried = Some(std::time::Instant::now());
            self.update.start_check(false, Self::update_wake(ctx));
        }
        if self.config.auto_update {
            ctx.request_repaint_after(std::time::Duration::from_secs(300));
        }
        if let Some(rx) = &self.startup_repair
            && let Ok(changed) = rx.try_recv()
        {
            self.startup_repair = None;
            if changed {
                self.state.status = Status::Info(
                    "Claude-Anbindung auf den neuen WOA-Pfad umgestellt. \
                     Claude einmal neu starten, dann verbindet es wieder."
                        .into(),
                );
            }
        }
        // Update-Download gescheitert: das Beenden fand nicht statt, beim nächsten Mal fragen.
        if self.exit.confirmed_for() == Some(crate::exit::ExitReason::Update)
            && !matches!(
                self.update.phase,
                crate::update::Phase::Downloading(_) | crate::update::Phase::Ready(..)
            )
        {
            self.exit.reset();
        }
        match self.update.poll() {
            Event::None => {}
            Event::Checked { ok, message } => {
                if ok {
                    self.config.last_update_check = Some(unix_now());
                    self.save_config();
                }
                if self.update.was_manual() {
                    self.state.status = if ok {
                        Status::Info(message)
                    } else {
                        Status::Error(message)
                    };
                }
            }
            #[cfg(windows)]
            Event::Downloaded(path) => self.update_install(ctx, &path),
            #[cfg(not(windows))]
            Event::Downloaded(_) => {}
        }
    }

    /// Geprüftes MSI liegt bereit: Projekt sichern, Installer starten, App beenden. Das MSI
    /// startet die neue Version nach der Installation selbst (`RESTARTWOA=1`).
    #[cfg(windows)]
    pub(super) fn update_install(&mut self, ctx: &egui::Context, msi: &std::path::Path) {
        // Während des Downloads geändert: dieselbe Frage; die Antwort installiert dann.
        if !self.exit.request(
            crate::exit::ExitReason::Update,
            self.state.session().is_dirty(),
        ) {
            return;
        }
        match woa_updater::launch_installer(msi) {
            Ok(()) => {
                tracing::info!("installer started, closing");
                self.exit.allow(crate::exit::ExitReason::Update);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Err(e) => {
                tracing::warn!(error = %e, "installer not started");
                self.exit.reset();
                self.update.install_failed(&e.to_string());
            }
        }
    }

    /// Update-Info in der Mitte der Kopfleiste, nur solange ein Update bereitliegt.
    /// `ui` ist der Rest der Leiste zwischen Menüs und rechter Gruppe (rechts-nach-links).
    pub(super) fn update_info(&mut self, ui: &mut egui::Ui) {
        use crate::update::Phase;
        if self.update.dismissed {
            return;
        }
        let (a, busy, err) = match self.update.phase.clone() {
            Phase::Available(a) => (a, false, None),
            Phase::Downloading(a) | Phase::Ready(a, _) => (a, true, None),
            Phase::Failed(a, e) => (a, false, Some(e)),
            Phase::Idle | Phase::Checking => return,
        };
        // Mittig im freien Raum: Breite aus dem letzten Bild, damit die Info zentriert sitzt.
        let id = egui::Id::new("topbar-update-w");
        let free = ui.available_rect_before_wrap();
        let w: f32 = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(260.0);
        let left = (free.center().x - w / 2.0).max(free.left());
        let slot = egui::Rect::from_min_max(
            egui::pos2(left, free.top()),
            egui::pos2(free.right(), free.bottom()),
        );
        let used = ui
            .scope_builder(
                egui::UiBuilder::new()
                    .max_rect(slot)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    egui::Frame::new()
                        .fill(theme::mix(theme::panel_header(), theme::accent(), 0.18))
                        .stroke(egui::Stroke::new(
                            1.0,
                            theme::mix(theme::border(), theme::accent(), 0.5),
                        ))
                        .corner_radius(5)
                        .inner_margin(egui::Margin::symmetric(10, 2))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!("Update {} verfügbar", a.version))
                                        .small()
                                        .color(theme::text()),
                                );
                                if !a.notes.trim().is_empty() {
                                    ui.menu_button("Neuerungen", |ui| {
                                        ui.set_max_width(420.0);
                                        ui.label(&a.notes);
                                    });
                                }
                                if let Some(e) = &err {
                                    ui.colored_label(
                                        theme::on_air(),
                                        format!("Fehlgeschlagen: {e}"),
                                    );
                                }
                                if busy {
                                    ui.spinner();
                                    ui.label(
                                        egui::RichText::new("Wird geladen und geprüft …").small(),
                                    );
                                    return;
                                }
                                #[cfg(windows)]
                                if ui.small_button("Jetzt installieren").clicked()
                                    && self.exit.request(
                                        crate::exit::ExitReason::Update,
                                        self.state.session().is_dirty(),
                                    )
                                {
                                    self.update.start_download(Self::update_wake(ui.ctx()));
                                }
                                #[cfg(not(windows))]
                                ui.hyperlink_to("Zur Release-Seite", woa_updater::RELEASES_PAGE);
                                if ui.small_button("Später").clicked() {
                                    self.update.dismissed = true;
                                }
                            });
                        })
                },
            )
            .response
            .rect
            .width();
        ui.ctx().data_mut(|d| d.insert_temp(id, used));
    }

    /// Frage „Projekt speichern?“ vor Beenden, Fenster schließen und Update (#365):
    /// Speichern, Nicht speichern, Abbrechen. Abbrechen lässt WOA offen.
    pub(super) fn exit_confirm_window(&mut self, ctx: &egui::Context) {
        use crate::exit::{Choice, ExitReason};
        let Some(reason) = self.exit.asking() else {
            return;
        };
        let (text, save_label) = match reason {
            ExitReason::Quit => (
                "Das Projekt hat ungespeicherte Änderungen. Vor dem Beenden speichern?",
                "Speichern und beenden",
            ),
            ExitReason::Update => (
                "Das Projekt hat ungespeicherte Änderungen. WOA wird für das Update beendet.",
                "Speichern und installieren",
            ),
        };
        let mut choice = None;
        let shown = egui::Window::new("Projekt speichern?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(text);
                ui.horizontal(|ui| {
                    if ui.button(save_label).clicked() {
                        choice = Some(Choice::Save);
                    }
                    if ui.button("Nicht speichern").clicked() {
                        choice = Some(Choice::Discard);
                    }
                    if ui.button("Abbrechen").clicked() {
                        choice = Some(Choice::Cancel);
                    }
                });
            });
        // #441: Esc = Abbrechen, über die gemeinsame Esc-Regel.
        if let Some(r) = &shown
            && crate::esc::frage(ctx, r.response.layer_id, "exit-confirm")
        {
            choice = Some(Choice::Cancel);
        }
        let Some(choice) = choice else {
            return;
        };
        if choice == Choice::Save {
            self.save();
        }
        let clean = !self.state.session().is_dirty();
        if choice == Choice::Save && !clean {
            self.state.status = Status::Error("Nicht gespeichert: bitte wählen".into());
        }
        match self.exit.answer(choice, clean) {
            Some(ExitReason::Quit) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Some(ExitReason::Update) => {
                #[cfg(windows)]
                if let crate::update::Phase::Ready(_, msi) = self.update.phase.clone() {
                    self.update_install(ctx, &msi);
                    return;
                }
                self.update.start_download(Self::update_wake(ctx));
            }
            None => {}
        }
    }

    /// Farbthema wählen (Hell oder Dunkel); die Modusfarbe folgt dem Layout von selbst.
    pub(super) fn theme_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Erscheinungsbild", |ui| {
            for t in [theme::Theme::Dunkel, theme::Theme::Hell] {
                if ui
                    .radio_value(&mut self.config.theme, t, t.label())
                    .on_hover_text("Die Akzentfarbe folgt dem Layout: Produzent, Radio oder Party")
                    .changed()
                {
                    self.save_config();
                }
            }
            ui.separator();
            ui.label(egui::RichText::new("Pegelansicht (Analyse-Fenster)").small());
            for v in crate::widgets::VuView::ALL {
                if ui
                    .radio_value(&mut self.config.vu_view, v, v.label())
                    .on_hover_text(
                        "Pegelansicht im Analyse-Fenster; der Transport zeigt immer die LED-Kette",
                    )
                    .changed()
                {
                    self.save_config();
                }
            }
        });
    }

    /// Einträge im Menü „Einstellungen“.
    pub(super) fn update_menu(&mut self, ui: &mut egui::Ui) {
        let mut on = self.config.auto_update;
        if ui
            .checkbox(&mut on, "Automatisch nach Updates suchen (stündlich)")
            .changed()
        {
            self.config.auto_update = on;
            self.save_config();
        }
        if ui
            .add_enabled(
                !self.update.busy(),
                egui::Button::new("Jetzt nach Updates suchen"),
            )
            .clicked()
        {
            self.check_updates_now(ui.ctx());
            ui.close();
        }
    }

    /// Sucht sofort nach einem neuen Release (Hilfe und Entwickler).
    pub(super) fn check_updates_now(&mut self, ctx: &egui::Context) {
        self.update.dismissed = false;
        self.update.start_check(true, Self::update_wake(ctx));
        self.state.status = Status::Info("Suche nach Updates …".into());
    }
}
