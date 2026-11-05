//! Speichern, Öffnen, DAWproject, Rückfragen vor dem Ersetzen.

use super::*;

impl WoaApp {
    /// Übernimmt den Zustand der laufenden Plugins ins Projekt (vor Speichern, Autosave, Export).
    pub(super) fn capture_plugins(&mut self) {
        let Some(player) = self.player.as_mut() else {
            return;
        };
        let cmds = player.capture_plugin_state(self.state.project());
        self.state.apply_plugin_state(cmds);
    }

    pub(super) fn save(&mut self) {
        self.capture_plugins();
        if self.state.needs_save_as() {
            self.save_as();
        } else {
            self.state.save();
        }
    }

    pub(super) fn save_as(&mut self) {
        self.capture_plugins();
        if let Some(dir) = crate::ordner::dialog(crate::ordner::Art::Projekte)
            .set_title("Projekt speichern")
            .set_file_name(format!("{}.woa", self.state.project().meta.name))
            .save_file_modal()
        {
            self.save_into(&dir);
        }
    }

    /// Speichern unter `dir`: nie in einem anderen `.woa`-Paket; ein anderes bestehendes
    /// Projekt wird erst nach Rückfrage ersetzt (#409).
    pub(super) fn save_into(&mut self, dir: &std::path::Path) {
        let target = crate::state::save_target(dir);
        if self.state.overwrites_other_project(&target) {
            self.overwrite_ask = Some(target);
        } else {
            self.state.save_as(&target);
        }
    }

    /// Frage „Projekt überschreiben?“ (#409); Überschreiben legt `project.json.bak` an.
    pub(super) fn overwrite_confirm_window(&mut self, ctx: &egui::Context) {
        let Some(target) = self.overwrite_ask.clone() else {
            return;
        };
        let mut choice = None;
        let shown = egui::Window::new("Projekt überschreiben?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!(
                    "In {} liegt schon ein anderes Projekt. Ersetzen? Das alte bleibt als \
                     project.json.bak erhalten.",
                    target.display()
                ));
                ui.horizontal(|ui| {
                    if ui.button("Überschreiben").clicked() {
                        choice = Some(true);
                    }
                    if ui.button("Abbrechen").clicked() {
                        choice = Some(false);
                    }
                });
            });
        // #441: Esc = Abbrechen, über die gemeinsame Esc-Regel.
        if let Some(r) = &shown
            && crate::esc::frage(ctx, r.response.layer_id, "overwrite-confirm")
        {
            choice = Some(false);
        }
        match choice {
            Some(true) => {
                self.overwrite_ask = None;
                self.state.save_as(&target);
            }
            Some(false) => self.overwrite_ask = None,
            None => {}
        }
    }

    /// Schreibt das Projekt als `.dawproject` (Austausch mit Bitwig, Studio One, Cubase …).
    pub(super) fn export_dawproject(&mut self) {
        let Some(out) = crate::ordner::dialog(crate::ordner::Art::Exporte)
            .set_title("Als DAWproject exportieren")
            .add_filter("DAWproject", &["dawproject"])
            .set_file_name(format!("{}.dawproject", self.state.project().meta.name))
            .save_file_modal()
        else {
            return;
        };
        let project = self.state.project().clone();
        let resolve = |rel: &str| self.state.resolve_audio(rel);
        let probe = |p: &std::path::Path| {
            woa_io::dawproject::probe_wav(p).or_else(|| {
                let d = woa_engine::decode::decode_file(p).ok()?;
                Some(woa_io::dawproject::AudioInfo {
                    sample_rate: d.rate,
                    channels: u16::try_from(d.channels).unwrap_or(2),
                    seconds: d.frames() as f64 / f64::from(d.rate.max(1)),
                })
            })
        };
        match woa_io::dawproject::export::export_file(&project, &resolve, &probe, &out) {
            Ok(rep) => {
                tracing::info!(path = %out.display(), notes = rep.notes.len(), "dawproject exported");
                self.state.status = Status::Info(format!("DAWproject: {}", rep.summary()));
            }
            Err(e) => {
                tracing::error!(error = %e, "dawproject export failed");
                self.state.status = Status::Error(format!("DAWproject-Export: {e}"));
            }
        }
    }

    /// Liest eine `.dawproject`-Datei in einen neuen Projektordner `<name>.woa` neben der
    /// Datei und öffnet ihn.
    pub(super) fn open_dawproject(&mut self) {
        if self.ask_before_replace(Replace::Dawproject) {
            self.open_dawproject_now();
        }
    }

    pub(super) fn open_dawproject_now(&mut self) {
        let Some(file) = crate::ordner::dialog(crate::ordner::Art::Projekte)
            .set_title("DAWproject öffnen")
            .add_filter("DAWproject", &["dawproject"])
            .pick_file_modal()
        else {
            return;
        };
        let stem = file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Import")
            .to_owned();
        let parent = file.parent().unwrap_or_else(|| std::path::Path::new("."));
        let mut dir = parent.join(format!("{stem}.woa"));
        let mut n = 2;
        while dir.exists() {
            dir = parent.join(format!("{stem} ({n}).woa"));
            n += 1;
        }
        match woa_io::dawproject::import::import_to_folder(&file, &dir) {
            Ok((_, rep)) => {
                tracing::info!(path = %dir.display(), notes = rep.notes.len(), "dawproject imported");
                self.state.open(&dir);
                // Nach dem Öffnen überschreibt `open` die Statuszeile; die Zusammenfassung
                // ist das, was der Nutzer sehen soll.
                if !matches!(self.state.status, Status::Error(_)) {
                    self.state.status = Status::Info(format!("DAWproject: {}", rep.summary()));
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "dawproject import failed");
                self.state.status = Status::Error(format!("DAWproject-Import: {e}"));
            }
        }
    }

    /// Öffnet einen Projektordner (z. B. von der Kommandozeile).
    pub fn open_project(&mut self, dir: &std::path::Path) {
        self.state.open(dir);
    }

    /// Vor Neu/Öffnen (#408): bei ungespeicherten Änderungen erst fragen — Speichern,
    /// Nicht speichern, Abbrechen. `true`: sofort weiter. `false`: die Frage ist offen
    /// (ein zweiter Klick öffnet keine zweite) und führt `what` nach der Antwort aus.
    pub(super) fn ask_before_replace(&mut self, what: Replace) -> bool {
        if !self.state.session().is_dirty() {
            return true;
        }
        if self.replace_ask.is_none() {
            self.replace_ask = Some(what);
        }
        false
    }

    pub(super) fn replace_now(&mut self, what: Replace) {
        match what {
            Replace::New => self.state.new_project(),
            Replace::Open => self.open_now(),
            Replace::Dawproject => self.open_dawproject_now(),
        }
    }

    /// Frage „Projekt speichern?“ vor Neu und Öffnen (#408).
    pub(super) fn replace_confirm_window(&mut self, ctx: &egui::Context) {
        use crate::exit::Choice;
        let Some(what) = self.replace_ask else {
            return;
        };
        // Erst die Überschreiben-Frage beantworten.
        if self.overwrite_ask.is_some() {
            return;
        }
        let mut choice = None;
        let shown = egui::Window::new("Projekt speichern?")
            .id(egui::Id::new("replace-confirm"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("Das Projekt hat ungespeicherte Änderungen. Vorher speichern?");
                ui.horizontal(|ui| {
                    if ui.button("Speichern").clicked() {
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
            && crate::esc::frage(ctx, r.response.layer_id, "replace-confirm")
        {
            choice = Some(Choice::Cancel);
        }
        match choice {
            Some(Choice::Cancel) => self.replace_ask = None,
            Some(Choice::Discard) => {
                self.replace_ask = None;
                self.replace_now(what);
            }
            Some(Choice::Save) => {
                self.save();
                // Speichern-Dialog abgebrochen oder Überschreiben-Frage offen: Frage bleibt.
                if !self.state.session().is_dirty() {
                    self.replace_ask = None;
                    self.replace_now(what);
                }
            }
            None => {}
        }
    }

    /// Jingle-Assistent: ungespeicherte Änderungen erst sichern.
    /// `false` heißt: das Projekt blieb ungespeichert, der Aufrufer bricht ab.
    pub(super) fn save_before_replace(&mut self, what: &str) -> bool {
        if self.state.session().is_dirty() {
            self.save();
            if self.state.session().is_dirty() {
                self.state.status = Status::Error(format!(
                    "{what} abgebrochen: erst das Projekt speichern, dann noch einmal"
                ));
                return false;
            }
        }
        true
    }

    /// Datei › Beenden (Strg+Q): über dieselbe Frage wie Fenster schließen und Update.
    pub(super) fn quit(&mut self, ctx: &egui::Context) {
        if self.exit.request(
            crate::exit::ExitReason::Quit,
            self.state.session().is_dirty(),
        ) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Fenster schließen (X, Alt+F4): bei ungespeichertem Projekt erst fragen.
    pub(super) fn close_guard(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        if self.exit.request(
            crate::exit::ExitReason::Quit,
            self.state.session().is_dirty(),
        ) {
            // Einstellungen sichern, bevor das Fenster geht (#380): Radio-Einstellungen aus
            // radio.json und config.json, nicht erst beim nächsten Takt.
            self.flush_settings(ctx);
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
    }

    /// Schreibt alle Einstellungen sofort (#380): Sendepult (radio.json), Fenstergröße der
    /// Einstellungen und config.json.
    pub(super) fn flush_settings(&mut self, ctx: &egui::Context) {
        if let Some(r) = self.radio.as_mut() {
            r.flush();
        }
        self.remember_settings_size(ctx);
        self.save_config();
    }

    pub(super) fn new_project(&mut self) {
        if self.ask_before_replace(Replace::New) {
            self.state.new_project();
        }
    }

    pub(super) fn open(&mut self) {
        if self.ask_before_replace(Replace::Open) {
            self.open_now();
        }
    }

    pub(super) fn open_now(&mut self) {
        if let Some(dir) = crate::ordner::dialog(crate::ordner::Art::Projekte)
            .set_title("Projektordner (.woa) öffnen")
            .pick_folder_modal()
        {
            self.state.open(&dir);
        }
    }

    /// Opens the piano roll for a MIDI clip and selects its track (live instrument for
    /// the preview keyboard).
    pub(super) fn open_piano_roll(&mut self, clip: ClipId) {
        let Some((track, c)) = self.state.project().clip(clip) else {
            return;
        };
        if c.midi.is_none() {
            return;
        }
        self.selected_track = Some(track.id);
        self.selected_clip = Some(clip);
        self.track_focus = false;
        tracing::info!(clip = clip.0, "piano roll opened");
        self.piano_roll = Some(crate::widgets::piano_roll::PianoRoll::new(clip));
        if !self.piano_roll_floating {
            self.studio_dock = StudioDockTab::PianoRoll;
        }
    }

    /// Draws the open piano roll; closes it when its window or clip is gone.
    pub(super) fn piano_roll_window(&mut self, ctx: &egui::Context) {
        if !self.piano_roll_floating {
            return;
        }
        let playhead = self.playhead_beats();
        let Some(pr) = self.piano_roll.as_mut() else {
            return;
        };
        pr.playhead = playhead;
        if !pr.show(ctx, &mut self.state) {
            self.piano_roll = None;
        }
    }
}
