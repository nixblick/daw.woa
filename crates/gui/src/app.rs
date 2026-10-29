//! The eframe window. Thin: reads [`AppState`], forwards user actions to it.

use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;
use woa_engine::record::{InputConfig, Recorder, next_take_path, start_recording};
use woa_model::{MAX_GAIN_DB, MIN_GAIN_DB, TrackId};

use crate::feedback::{FeedbackDialog, GuiConfig, Phase, encode_png};
use crate::state::{AppState, Status};

/// The desktop window.
pub struct WoaApp {
    state: AppState,
    config: GuiConfig,
    feedback: FeedbackDialog,
    /// Track being renamed and its edit buffer.
    rename: Option<(TrackId, String)>,
    /// Gain being dragged (committed as one undo step on release).
    gain_drag: Option<(TrackId, f64)>,
    /// Last title sent to the viewport.
    last_title: String,
    /// Where takes of unsaved projects go (`<data>/aufnahmen`).
    takes_dir: PathBuf,
    /// Running recording from the default input, if any.
    recorder: Option<Recorder>,
    /// Smoothed input level for the meter, 0..1.
    level: f32,
}

impl WoaApp {
    /// Builds the app; `recovery_dir` receives autosaves of unsaved projects.
    pub fn new(config: GuiConfig, recovery_dir: PathBuf) -> Self {
        let takes_dir = recovery_dir
            .parent()
            .map(|p| p.join("aufnahmen"))
            .unwrap_or_else(|| PathBuf::from("aufnahmen"));
        Self {
            takes_dir,
            recorder: None,
            level: 0.0,
            state: AppState::new(recovery_dir, Instant::now()),
            config,
            feedback: FeedbackDialog::default(),
            rename: None,
            gain_drag: None,
            last_title: String::new(),
        }
    }

    /// Starts or stops recording the default input into the project's `aufnahmen/`.
    fn toggle_record(&mut self) {
        if let Some(rec) = self.recorder.take() {
            self.level = 0.0;
            match rec.stop() {
                Ok(s) => {
                    tracing::info!(seconds = s.seconds(), overruns = s.overruns, "take saved");
                    let lost = if s.overruns > 0 {
                        format!(", {} Samples verloren", s.overruns)
                    } else {
                        String::new()
                    };
                    self.state.status = Status::Info(format!(
                        "Aufnahme gespeichert: {} ({:.1} s{lost})",
                        s.path.display(),
                        s.seconds()
                    ));
                }
                Err(e) => {
                    tracing::error!(error = %e, "take failed");
                    self.state.status = Status::Error(format!("Aufnahme fehlgeschlagen: {e}"));
                }
            }
            return;
        }
        let dir = match self.state.session().path() {
            Some(p) => p.join("aufnahmen"),
            None => self.takes_dir.clone(),
        };
        let started = next_take_path(&dir)
            .map_err(|e| e.to_string())
            .and_then(|path| {
                start_recording(&InputConfig::default(), &path, |e| {
                    tracing::warn!(error = %e, "input stream error");
                })
                .map_err(|e| e.to_string())
            });
        match started {
            Ok(rec) => {
                tracing::info!(path = %rec.path().display(), "take started");
                self.state.status = Status::Info(format!("Nimmt auf: {}", rec.path().display()));
                self.recorder = Some(rec);
            }
            Err(e) => {
                tracing::error!(error = %e, "take start failed");
                self.state.status = Status::Error(format!("Aufnahme nicht gestartet: {e}"));
            }
        }
    }

    fn record_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (label, fill) = match &self.recorder {
                Some(rec) => {
                    let t = rec.elapsed().as_secs();
                    (
                        format!("■ Stopp  {}:{:02}", t / 60, t % 60),
                        egui::Color32::from_rgb(200, 40, 40),
                    )
                }
                None => (
                    "● Aufnahme".to_string(),
                    ui.visuals().widgets.inactive.bg_fill,
                ),
            };
            let button = egui::Button::new(egui::RichText::new(label).strong()).fill(fill);
            if ui
                .add(button)
                .on_hover_text("Nimmt den Standard-Eingang (Mikrofon) als WAV auf (Strg+R)")
                .clicked()
            {
                self.toggle_record();
            }
            if let Some(rec) = &self.recorder {
                let fresh = rec.take_level();
                self.level = fresh.max(self.level * 0.85);
                let color = if self.level > 0.95 {
                    egui::Color32::RED
                } else {
                    egui::Color32::from_rgb(60, 180, 75)
                };
                ui.add(
                    egui::ProgressBar::new(self.level.clamp(0.0, 1.0))
                        .desired_width(240.0)
                        .fill(color),
                );
            }
        });
    }

    fn save(&mut self) {
        if self.state.needs_save_as() {
            self.save_as();
        } else {
            self.state.save();
        }
    }

    fn save_as(&mut self) {
        if let Some(dir) = rfd::FileDialog::new()
            .set_title("Projekt speichern")
            .set_file_name(format!("{}.woa", self.state.project().meta.name))
            .save_file()
        {
            self.state.save_as(&dir);
        }
    }

    fn open(&mut self) {
        if let Some(dir) = rfd::FileDialog::new()
            .set_title("Projektordner (.woa) öffnen")
            .pick_folder()
        {
            self.state.open(&dir);
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let (undo, redo, save, open, new, record) = ctx.input_mut(|i| {
            let m = egui::Modifiers::COMMAND;
            let sm = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
            (
                i.consume_key(m, egui::Key::Z),
                i.consume_key(m, egui::Key::Y) || i.consume_key(sm, egui::Key::Z),
                i.consume_key(m, egui::Key::S),
                i.consume_key(m, egui::Key::O),
                i.consume_key(m, egui::Key::N),
                i.consume_key(m, egui::Key::R),
            )
        });
        if record {
            self.toggle_record();
        }
        if undo {
            self.state.undo();
        }
        if redo {
            self.state.redo();
        }
        if save {
            self.save();
        }
        if open {
            self.open();
        }
        if new {
            self.state.new_project();
        }
    }

    fn take_screenshot(&mut self, ctx: &egui::Context) {
        if self.feedback.phase != Phase::Capturing {
            return;
        }
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let [w, h] = image.size;
            let png = match (u32::try_from(w), u32::try_from(h)) {
                (Ok(w), Ok(h)) => encode_png(w, h, image.as_raw())
                    .map_err(|e| tracing::warn!(error = %e, "screenshot encode failed"))
                    .ok(),
                _ => None,
            };
            self.feedback.set_screenshot(png);
        }
    }

    fn menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Datei", |ui| {
                if ui.button("Neu (Strg+N)").clicked() {
                    self.state.new_project();
                }
                if ui.button("Öffnen … (Strg+O)").clicked() {
                    self.open();
                }
                if ui.button("Speichern (Strg+S)").clicked() {
                    self.save();
                }
                if ui.button("Speichern unter …").clicked() {
                    self.save_as();
                }
            });
            ui.menu_button("Bearbeiten", |ui| {
                let h = self.state.session().history();
                let undo = format!("Rückgängig {}", h.undo_label().unwrap_or(""));
                let redo = format!("Wiederholen {}", h.redo_label().unwrap_or(""));
                if ui.button(undo).clicked() {
                    self.state.undo();
                }
                if ui.button(redo).clicked() {
                    self.state.redo();
                }
            });
            if ui.button("Feedback …").clicked() {
                self.feedback.open();
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            }
        });
    }

    fn tracks(&mut self, ui: &mut egui::Ui) {
        self.record_bar(ui);
        ui.separator();
        if ui.button("+ Spur").clicked() {
            self.state.add_track();
        }
        ui.separator();
        let tracks: Vec<_> = self
            .state
            .project()
            .tracks
            .iter()
            .map(|t| (t.id, t.name.clone(), t.gain_db, t.mute))
            .collect();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (id, name, gain, mute) in tracks {
                ui.horizontal(|ui| {
                    self.track_name(ui, id, &name);
                    let mut g = match self.gain_drag {
                        Some((d, v)) if d == id => v,
                        _ => gain,
                    };
                    let r = ui.add(
                        egui::Slider::new(&mut g, MIN_GAIN_DB..=MAX_GAIN_DB)
                            .suffix(" dB")
                            .text("Gain"),
                    );
                    if r.changed() {
                        self.gain_drag = Some((id, g));
                    }
                    if r.drag_stopped() || (r.changed() && !r.dragged()) {
                        self.gain_drag = None;
                        self.state.set_gain(id, g);
                    }
                    if ui.selectable_label(mute, "M").clicked() {
                        self.state.toggle_mute(id);
                    }
                });
            }
        });
    }

    fn track_name(&mut self, ui: &mut egui::Ui, id: TrackId, name: &str) {
        match &mut self.rename {
            Some((rid, buf)) if *rid == id => {
                let r = ui.add(egui::TextEdit::singleline(buf).desired_width(140.0));
                r.request_focus();
                if r.lost_focus() {
                    let new = buf.clone();
                    self.rename = None;
                    if !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.state.rename_track(id, &new);
                    }
                }
            }
            _ => {
                let r = ui
                    .add_sized([140.0, 20.0], egui::Button::new(name))
                    .on_hover_text("Doppelklick zum Umbenennen");
                if r.double_clicked() {
                    self.rename = Some((id, name.to_string()));
                }
            }
        }
    }

    fn feedback_window(&mut self, ctx: &egui::Context) {
        if matches!(self.feedback.phase, Phase::Closed | Phase::Capturing) {
            return;
        }
        let mut close = false;
        egui::Window::new("Feedback")
            .collapsible(false)
            .show(ctx, |ui| match self.feedback.phase.clone() {
                Phase::Editing => {
                    ui.label("Was ist passiert, was wünschst du dir?");
                    ui.add(egui::TextEdit::multiline(&mut self.feedback.text).desired_rows(6));
                    let has = self.feedback.screenshot.is_some();
                    ui.add_enabled(
                        has,
                        egui::Checkbox::new(&mut self.feedback.attach, "Screenshot anhängen"),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Senden").clicked() {
                            self.feedback.send(&self.config);
                        }
                        if ui.button("Abbrechen").clicked() {
                            close = true;
                        }
                    });
                }
                Phase::Sending => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Wird gesendet …");
                    });
                }
                Phase::Done(msg) => {
                    ui.label(msg);
                    if ui.button("Schließen").clicked() {
                        close = true;
                    }
                }
                Phase::Closed | Phase::Capturing => {}
            });
        if close {
            self.feedback.close();
        }
    }
}

impl eframe::App for WoaApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.shortcuts(ctx);
        self.take_screenshot(ctx);
        self.feedback.poll();
        self.state.tick_autosave(Instant::now());
        let title = self.state.title();
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
        // Keep the autosave timer and the feedback poll ticking while idle; the level
        // meter needs a faster beat while recording.
        let beat = if self.recorder.is_some() { 40 } else { 500 };
        ctx.request_repaint_after(std::time::Duration::from_millis(beat));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        egui::Panel::top("menu").show(ui, |ui| self.menu(ui, &ctx));
        egui::Panel::bottom("status").show(ui, |ui| match &self.state.status {
            Status::Idle => {
                ui.label("");
            }
            Status::Info(m) => {
                ui.label(m);
            }
            Status::Error(m) => {
                ui.colored_label(ui.visuals().error_fg_color, m);
            }
        });
        egui::CentralPanel::default().show(ui, |ui| self.tracks(ui));
        self.feedback_window(&ctx);
    }
}
