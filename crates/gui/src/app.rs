//! The eframe window. Thin: reads [`AppState`], forwards user actions to it.

use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;
use woa_engine::record::next_take_path;
use woa_model::{ClipId, MAX_GAIN_DB, MIN_GAIN_DB, TrackId};

use crate::player::Player;
use crate::sources::{self, SourceEntry, Take};
use crate::theme;

/// Pixel je Beat in der Zeitleiste.
/// Start-Zoom der Zeitleiste in Pixel je Beat.
const PX_PER_BEAT: f32 = 24.0;
/// Breite der Spurköpfe.
const HEADER_W: f32 = 250.0;
/// Höhe einer Spur.
const ROW_H: f32 = 88.0;
const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 2.0..=160.0;

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
    /// Laufende Aufnahmen, je Zielspur eine (leer = keine Aufnahme).
    takes: Vec<(Option<TrackId>, Take)>,
    /// Startpunkt der laufenden Aufnahme in Beats.
    rec_start: f64,
    /// Spuren, die bei der nächsten Aufnahme mitlaufen (leer = die ausgewählte Spur).
    armed: std::collections::BTreeSet<TrackId>,
    /// Gewählte Quelle je Spur (fehlt = Standard-Eingang).
    track_source: std::collections::HashMap<TrackId, String>,
    /// Spuren, deren Quelle beim Aufnehmen hörbar ist.
    track_monitor: std::collections::HashSet<TrackId>,
    /// Zuletzt abgefragte Quellen und wann.
    source_list: (Vec<SourceEntry>, Option<Instant>),
    /// Aufnahmepegel je Spur während der Aufnahme, 0..1.
    rec_levels: std::collections::HashMap<TrackId, f32>,
    /// Wiedergabe am Ende des letzten Clips anhalten.
    stop_at_end: bool,
    /// Playback engine at the default output (`None` if the device failed).
    player: Option<Player>,
    selected_track: Option<TrackId>,
    selected_clip: Option<ClipId>,
    /// Clip being dragged and its offset in beats.
    clip_drag: Option<(ClipId, f64)>,
    /// Fades being edited (committed as one undo step when editing ends).
    fade_edit: Option<(ClipId, f64, f64)>,
    /// Weitere markierte Clips (Strg/Umschalt+Klick), inklusive `selected_clip`.
    selected_clips: Vec<ClipId>,
    /// Export-Lautheit in LUFS, `None` = unverändert.
    export_lufs: Option<f64>,
    /// Gemessene Treiberlatenz beim Aufnehmen ausgleichen.
    latency_auto: bool,
    /// Zusätzlicher Latenzausgleich in Millisekunden (von Hand).
    latency_extra_ms: f64,
    /// Clip-Rand, der gerade gezogen wird: (Clip, linker Rand?, Versatz in Beats).
    trim: Option<(ClipId, bool, f64)>,
    /// Zoom der Zeitleiste in Pixel je Beat.
    zoom: f32,
    /// Zeitleisten-Flächen je Spur (dieses und letztes Bild), für das Ziehen auf andere Spuren.
    track_rects: Vec<(TrackId, egui::Rect)>,
    prev_track_rects: Vec<(TrackId, egui::Rect)>,
    /// Angezeigter Pegel je Spur (mit Abfall), 0..1.
    track_levels: std::collections::HashMap<TrackId, f32>,
    /// Smoothed input level for the meter, 0..1.
    level: f32,
    /// Thema schon gesetzt.
    themed: bool,
    /// Fotomodus (`WOA_SCREENSHOT=datei.png`): Zielpfad und gezählte Bilder.
    photo: Option<(PathBuf, u32)>,
}

impl WoaApp {
    /// Builds the app; `recovery_dir` receives autosaves of unsaved projects.
    pub fn new(config: GuiConfig, recovery_dir: PathBuf) -> Self {
        let mut state = AppState::new(recovery_dir, Instant::now());
        let player = match Player::open() {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::error!(error = %e, "no playback device");
                state.status = Status::Error(format!("Keine Wiedergabe: {e}"));
                None
            }
        };
        Self {
            takes: Vec::new(),
            rec_start: 0.0,
            armed: std::collections::BTreeSet::new(),
            track_source: std::collections::HashMap::new(),
            track_monitor: std::collections::HashSet::new(),
            source_list: (Vec::new(), None),
            rec_levels: std::collections::HashMap::new(),
            stop_at_end: true,
            player,
            selected_track: None,
            selected_clip: None,
            clip_drag: None,
            fade_edit: None,
            themed: false,
            photo: std::env::var_os("WOA_SCREENSHOT").map(|p| (PathBuf::from(p), 0)),
            selected_clips: Vec::new(),
            export_lufs: Some(-16.0),
            latency_auto: true,
            latency_extra_ms: 0.0,
            trim: None,
            zoom: PX_PER_BEAT,
            track_rects: Vec::new(),
            prev_track_rects: Vec::new(),
            track_levels: std::collections::HashMap::new(),
            level: 0.0,
            state,
            config,
            feedback: FeedbackDialog::default(),
            rename: None,
            gain_drag: None,
            last_title: String::new(),
        }
    }

    /// Startet oder beendet die Aufnahme. Jede scharf geschaltete Spur (sonst die
    /// ausgewählte) nimmt ihre eigene Quelle in `aufnahmen/` auf, alle ab demselben Punkt.
    fn toggle_record(&mut self) {
        if !self.takes.is_empty() {
            self.stop_record();
            return;
        }
        let dir = self.state.takes_dir();
        let tempo = self.state.project().tempo_bpm;
        let start = self
            .player
            .as_ref()
            .map_or(0.0, |p| p.position_beats(tempo));
        let targets: Vec<Option<TrackId>> = if self.armed.is_empty() {
            vec![self.selected_track]
        } else {
            self.armed.iter().copied().map(Some).collect()
        };
        let mut errors = Vec::new();
        for track in targets {
            let key = track
                .and_then(|t| self.track_source.get(&t).cloned())
                .unwrap_or_default();
            let monitor = track.is_some_and(|t| self.track_monitor.contains(&t));
            let started = next_take_path(&dir)
                .map_err(|e| e.to_string())
                .and_then(|path| Take::start(&key, &path, monitor));
            match started {
                Ok(take) => {
                    tracing::info!(path = %take.path().display(), source = %key, "take started");
                    self.takes.push((track, take));
                }
                Err(e) => {
                    tracing::error!(error = %e, source = %key, "take start failed");
                    errors.push(format!("{}: {e}", sources::short_label(&key)));
                }
            }
        }
        if self.takes.is_empty() {
            self.state.status =
                Status::Error(format!("Aufnahme nicht gestartet: {}", errors.join("; ")));
            return;
        }
        self.rec_start = start;
        self.state.status = if errors.is_empty() {
            Status::Info(format!("Nimmt auf: {} Spur(en)", self.takes.len()))
        } else {
            Status::Error(format!("Nimmt auf, aber: {}", errors.join("; ")))
        };
        // Overdub: die anderen Spuren spielen ab dem Kopf mit.
        if let Some(p) = self.player.as_mut() {
            p.play();
        }
    }

    fn stop_record(&mut self) {
        self.level = 0.0;
        self.rec_levels.clear();
        if let Some(p) = self.player.as_mut() {
            p.stop();
        }
        let start = self.rec_start;
        let mut saved = Vec::new();
        let mut errors = Vec::new();
        for (track, take) in std::mem::take(&mut self.takes) {
            match take.stop() {
                Ok((s, measured)) => {
                    tracing::info!(seconds = s.seconds(), overruns = s.overruns, "take saved");
                    if let Some(p) = self.player.as_mut() {
                        p.forget(&s.path);
                    }
                    let measured = if self.latency_auto {
                        measured.as_secs_f64()
                    } else {
                        0.0
                    };
                    let latency = measured + self.latency_extra_ms / 1000.0;
                    tracing::info!(latency_ms = latency * 1000.0, "latency compensation");
                    if let Some(clip) =
                        self.state
                            .add_take(track, &s.path, start, s.seconds(), latency)
                    {
                        self.selected_clip = Some(clip);
                        self.selected_track = self.state.project().clip(clip).map(|(t, _)| t.id);
                    }
                    let lost = if s.overruns > 0 {
                        format!(", {} Samples verloren", s.overruns)
                    } else {
                        String::new()
                    };
                    saved.push(format!("{:.1} s{lost}", s.seconds()));
                }
                Err(e) => {
                    tracing::error!(error = %e, "take failed");
                    errors.push(e);
                }
            }
        }
        self.state.status = if errors.is_empty() {
            Status::Info(format!(
                "Aufnahme gespeichert: {} Spur(en) ({})",
                saved.len(),
                saved.join(", ")
            ))
        } else {
            Status::Error(format!("Aufnahme fehlgeschlagen: {}", errors.join("; ")))
        };
    }

    /// Quellenliste, höchstens jede Sekunde neu abgefragt.
    fn sources(&mut self) -> &[SourceEntry] {
        let stale = self
            .source_list
            .1
            .is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(1));
        if stale {
            self.source_list = (sources::list(), Some(Instant::now()));
        }
        &self.source_list.0
    }

    fn rewind(&mut self) {
        let tempo = self.state.project().tempo_bpm;
        if let Some(p) = self.player.as_mut() {
            p.seek_beats(0.0, tempo);
        }
    }

    fn toggle_play(&mut self) {
        if !self.takes.is_empty() {
            self.toggle_record();
        } else {
            let end = self.audio_end();
            let tempo = self.state.project().tempo_bpm;
            let at_end = self.stop_at_end;
            if let Some(p) = self.player.as_mut() {
                // Am Ende angekommen: Abspielen beginnt wieder vorn.
                if at_end && !p.is_playing() && end > 0.0 && p.position_beats(tempo) >= end {
                    p.seek_beats(0.0, tempo);
                }
                p.toggle();
            }
        }
    }

    /// Transportleiste: Wiedergabe, große Zeitanzeige, Aufnahme mit Pegel, Latenz;
    /// rechts Zoom und Export-Lautheit.
    fn transport_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let playing = self.player.as_ref().is_some_and(Player::is_playing);
            let big = |t: &str| egui::RichText::new(t).size(18.0);
            if ui
                .add(egui::Button::new(big("⏮")).min_size(egui::vec2(40.0, 34.0)))
                .on_hover_text("Zum Anfang (Pos1)")
                .clicked()
            {
                self.rewind();
            }
            let play = egui::Button::new(big(if playing { "⏸" } else { "▶" }).color(if playing {
                theme::BG
            } else {
                ui.visuals().text_color()
            }))
            .min_size(egui::vec2(52.0, 34.0))
            .fill(if playing {
                theme::ACCENT
            } else {
                theme::SURFACE
            });
            if ui
                .add_enabled(self.player.is_some(), play)
                .on_hover_text("Abspielen / Pause (Leertaste)")
                .clicked()
            {
                self.toggle_play();
            }
            let end = egui::Button::new(egui::RichText::new("Stopp am Ende").small().color(
                if self.stop_at_end {
                    theme::BG
                } else {
                    theme::DIM
                },
            ))
            .min_size(egui::vec2(0.0, 34.0))
            .fill(if self.stop_at_end {
                theme::ACCENT
            } else {
                theme::SURFACE
            });
            if ui
                .add(end)
                .on_hover_text(
                    "An: die Wiedergabe hält am Ende des letzten Clips an. \
                     Aus: sie läuft weiter, z. B. zum Aufnehmen über das Ende hinaus.",
                )
                .clicked()
            {
                self.stop_at_end = !self.stop_at_end;
            }
            // Zeitanzeige: Minuten:Sekunden groß, Takt.Schlag klein.
            let secs = self.player.as_ref().map_or(0.0, Player::position_seconds);
            let tempo = self.state.project().tempo_bpm;
            let beats = self
                .player
                .as_ref()
                .map_or(0.0, |p| p.position_beats(tempo));
            egui::Frame::new()
                .fill(theme::BG)
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(12, 2))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(
                            egui::RichText::new(format!(
                                "{:02}:{:04.1}",
                                (secs / 60.0) as u64,
                                secs % 60.0
                            ))
                            .monospace()
                            .size(20.0)
                            .color(theme::ACCENT),
                        );
                        ui.label(
                            egui::RichText::new(format!(
                                "Takt {}.{}  ·  {tempo:.0} BPM",
                                (beats / 4.0) as u64 + 1,
                                (beats % 4.0) as u64 + 1
                            ))
                            .small()
                            .color(theme::DIM),
                        );
                    });
                });
            ui.add_space(12.0);
            let recording = !self.takes.is_empty();
            let label = match self.takes.first() {
                Some((_, rec)) => {
                    let t = rec.elapsed().as_secs();
                    format!("■  {}:{:02}", t / 60, t % 60)
                }
                None => "     Aufnahme".to_string(),
            };
            let rec_btn = egui::Button::new(egui::RichText::new(label).strong().size(15.0).color(
                if recording {
                    egui::Color32::WHITE
                } else {
                    theme::ON_AIR
                },
            ))
            .min_size(egui::vec2(120.0, 34.0))
            .fill(if recording {
                theme::ON_AIR
            } else {
                theme::SURFACE
            });
            let rec_resp = ui.add(rec_btn).on_hover_text(
                "Nimmt auf jeder scharfen Spur (roter Punkt) ihre Quelle auf, \
                     sonst auf der ausgewählten Spur (Strg+R)",
            );
            if !recording {
                // Die Schrift hat kein ●, also selbst zeichnen.
                let c = rec_resp.rect.left_center() + egui::vec2(22.0, 0.0);
                ui.painter().circle_filled(c, 6.0, theme::ON_AIR);
            }
            if rec_resp.clicked() {
                self.toggle_record();
            }
            if recording {
                let mut fresh = 0.0f32;
                for (track, take) in &self.takes {
                    let l = take.take_level();
                    fresh = fresh.max(l);
                    if let Some(t) = track {
                        let shown = self.rec_levels.entry(*t).or_insert(0.0);
                        *shown = l.max(*shown);
                    }
                }
                self.level = fresh.max(self.level * 0.85);
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(160.0, 10.0), egui::Sense::hover());
                level_bar(ui.painter(), rect, self.level);
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let out_ms = woa_engine::device::output_latency().as_secs_f64() * 1000.0;
                ui.checkbox(
                    &mut self.latency_auto,
                    egui::RichText::new("Latenz ausgleichen").small(),
                )
                .on_hover_text(format!(
                    "Verschiebt jede Aufnahme um die vom Treiber gemeldete Latenz nach vorn \
                         (Ausgang zuletzt {out_ms:.1} ms, Eingang wird bei der Aufnahme gemessen)"
                ));
                ui.add(
                    egui::DragValue::new(&mut self.latency_extra_ms)
                        .range(-500.0..=500.0)
                        .speed(0.5)
                        .prefix("Zusatz +")
                        .suffix(" ms"),
                )
                .on_hover_text(
                    "Zusätzlicher Ausgleich von Hand, falls Aufnahmen noch zu spät liegen",
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let name = |v: Option<f64>| match v {
                    None => "unverändert".to_string(),
                    Some(l) => format!("{l:.0} LUFS"),
                };
                egui::ComboBox::from_id_salt("export_lufs")
                    .selected_text(name(self.export_lufs))
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        for (v, hint) in [
                            (Some(-16.0), " · Web, Podcast"),
                            (Some(-23.0), " · Rundfunk, EBU R128"),
                            (None, ""),
                        ] {
                            ui.selectable_value(
                                &mut self.export_lufs,
                                v,
                                format!("{}{hint}", name(v)),
                            );
                        }
                    });
                ui.label(egui::RichText::new("Export").small().color(theme::DIM));
                ui.separator();
                if ui
                    .button("+")
                    .on_hover_text("Hineinzoomen (Strg+Mausrad)")
                    .clicked()
                {
                    self.zoom = (self.zoom * 1.5).clamp(*ZOOM_RANGE.start(), *ZOOM_RANGE.end());
                }
                if ui.button("−").on_hover_text("Herauszoomen").clicked() {
                    self.zoom = (self.zoom / 1.5).clamp(*ZOOM_RANGE.start(), *ZOOM_RANGE.end());
                }
                ui.label(egui::RichText::new("Zoom").small().color(theme::DIM));
            });
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

    /// Öffnet einen Projektordner (z. B. von der Kommandozeile).
    pub fn open_project(&mut self, dir: &std::path::Path) {
        self.state.open(dir);
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
        if !ctx.egui_wants_keyboard_input() {
            let (space, home, del, split, export) = ctx.input_mut(|i| {
                let n = egui::Modifiers::NONE;
                (
                    i.consume_key(n, egui::Key::Space),
                    i.consume_key(n, egui::Key::Home),
                    i.consume_key(n, egui::Key::Delete),
                    i.consume_key(n, egui::Key::S),
                    i.consume_key(egui::Modifiers::COMMAND, egui::Key::E),
                )
            });
            if split {
                self.split_at_playhead();
            }
            if export {
                self.export();
            }
            if space {
                self.toggle_play();
            }
            if home {
                self.rewind();
            }
            if del {
                self.delete_selected();
            }
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

    /// Fotomodus für Design-Prüfungen: nach ein paar Bildern ein Bildschirmfoto des Fensters
    /// als PNG speichern und beenden.
    fn photo(&mut self, ctx: &egui::Context) {
        let Some((path, frames)) = &mut self.photo else {
            return;
        };
        *frames += 1;
        if *frames == 20 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let [w, h] = image.size;
            let result = encode_png(w as u32, h as u32, image.as_raw())
                .map_err(|e| e.to_string())
                .and_then(|png| std::fs::write(&*path, png).map_err(|e| e.to_string()));
            match result {
                Ok(()) => tracing::info!(path = %path.display(), "photo saved"),
                Err(e) => tracing::error!(error = %e, "photo failed"),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
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
                ui.separator();
                if ui
                    .button("Exportieren (WAV, FLAC, MP3) … (Strg+E)")
                    .clicked()
                {
                    self.export();
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
        // Strg+Mausrad zoomt die Zeitleiste.
        let z = ui.ctx().input(|i| i.zoom_delta());
        if z != 1.0 {
            self.zoom = (self.zoom * z).clamp(*ZOOM_RANGE.start(), *ZOOM_RANGE.end());
        }
        self.prev_track_rects = std::mem::take(&mut self.track_rects);
        let tracks: Vec<_> = self
            .state
            .project()
            .tracks
            .iter()
            .map(|t| (t.id, t.name.clone(), t.gain_db, t.mute))
            .collect();
        egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.horizontal(|ui| {
                ui.add_space(HEADER_W + ui.spacing().item_spacing.x);
                self.ruler(ui);
            });
            for (i, (id, name, gain, mute)) in tracks.into_iter().enumerate() {
                ui.horizontal(|ui| {
                    self.track_header(ui, i, id, &name, gain, mute);
                    self.timeline(ui, id);
                });
            }
            ui.horizontal(|ui| {
                if ui
                    .add_sized([HEADER_W, 30.0], egui::Button::new("+  Spur hinzufügen"))
                    .clicked()
                {
                    self.state.add_track();
                }
            });
            if self.state.project().tracks.is_empty() {
                ui.add_space(24.0);
                ui.label(
                    egui::RichText::new(
                        "Noch keine Spur. Strg+R nimmt sofort auf und legt eine an.",
                    )
                    .color(theme::DIM),
                );
            }
        });
    }

    /// Lineal über der Zeitleiste: Taktnummern, Klick setzt den Kopf.
    fn ruler(&mut self, ui: &mut egui::Ui) {
        let px = self.zoom;
        let end = self.timeline_end();
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2((end as f32 + 16.0) * px, 20.0),
            egui::Sense::click_and_drag(),
        );
        let painter = ui.painter_at(rect);
        let every = if px < 4.0 {
            16
        } else if px < 8.0 {
            8
        } else {
            4
        };
        for beat in (0..=(end as usize + 16)).step_by(every) {
            let x = rect.left() + beat as f32 * px;
            painter.vline(
                x,
                rect.bottom() - 6.0..=rect.bottom(),
                egui::Stroke::new(1.0, theme::DIM),
            );
            painter.text(
                egui::pos2(x + 3.0, rect.top() + 2.0),
                egui::Align2::LEFT_TOP,
                format!("{}", beat / 4 + 1),
                egui::FontId::proportional(11.0),
                theme::DIM,
            );
        }
        let tempo = self.state.project().tempo_bpm;
        if let Some(p) = &self.player {
            let x = rect.left() + p.position_beats(tempo) as f32 * px;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(x - 6.0, rect.top() + 6.0),
                    egui::pos2(x + 6.0, rect.top() + 6.0),
                    egui::pos2(x, rect.bottom()),
                ],
                theme::ACCENT,
                egui::Stroke::NONE,
            ));
        }
        if (resp.clicked() || resp.dragged())
            && let (Some(pos), Some(p)) = (resp.interact_pointer_pos(), self.player.as_mut())
        {
            p.seek_beats(f64::from(((pos.x - rect.left()) / px).max(0.0)), tempo);
        }
    }

    /// Längste Spur in Beats, mindestens 64.
    /// Ende des letzten Clips in Beats (0 ohne Clips).
    fn audio_end(&self) -> f64 {
        self.state
            .project()
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.end_beats()))
            .fold(0.0f64, f64::max)
    }

    /// Hält die Wiedergabe am Ende des letzten Clips an, wenn „Stopp am Ende“ an ist.
    /// Beim Aufnehmen nie: da läuft die Zeit absichtlich über das Ende hinaus.
    fn stop_at_audio_end(&mut self) {
        if !self.stop_at_end || !self.takes.is_empty() {
            return;
        }
        let end = self.audio_end();
        let tempo = self.state.project().tempo_bpm;
        if let Some(p) = self.player.as_mut()
            && p.is_playing()
            && p.position_beats(tempo) >= end
        {
            p.stop();
            p.seek_beats(end, tempo);
        }
    }

    fn timeline_end(&self) -> f64 {
        self.state
            .project()
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.end_beats()))
            .fold(64.0f64, f64::max)
    }

    /// Spurkopf: Farbstreifen, Aufnahmeziel, Name, Mute, Pegel, Gain.
    fn track_header(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        id: TrackId,
        name: &str,
        gain: f64,
        mute: bool,
    ) {
        let color = theme::track_color(index);
        let armed = if self.armed.is_empty() {
            self.selected_track == Some(id)
        } else {
            self.armed.contains(&id)
        };
        let frame = egui::Frame::new()
            .fill(if armed {
                theme::mix(theme::SURFACE, color, 0.12)
            } else {
                theme::SURFACE
            })
            .corner_radius(6)
            .inner_margin(egui::Margin {
                left: 12,
                right: 6,
                top: 4,
                bottom: 4,
            });
        // Fest reserviertes Rechteck, damit Lineal und Zeitleisten bündig bleiben.
        let (slot, _) = ui.allocate_exact_size(egui::vec2(HEADER_W, ROW_H), egui::Sense::hover());
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(slot)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let resp = {
            let ui = &mut child;
            frame.show(ui, |ui| {
                ui.set_width(HEADER_W - 18.0);
                ui.set_max_width(HEADER_W - 18.0);
                ui.set_height(ROW_H - 8.0);
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
                ui.horizontal(|ui| {
                    let (dot, arm) =
                        ui.allocate_exact_size(egui::vec2(18.0, 22.0), egui::Sense::click());
                    ui.painter().circle(
                        dot.center(),
                        6.0,
                        if armed { theme::ON_AIR } else { theme::BG },
                        egui::Stroke::new(1.5, if armed { theme::ON_AIR } else { theme::DIM }),
                    );
                    if arm
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(
                            "Scharf schalten: nimmt bei der nächsten Aufnahme mit auf. \
                             Mehrere Spuren gleichzeitig möglich.",
                        )
                        .clicked()
                        && !self.armed.remove(&id)
                    {
                        self.armed.insert(id);
                    }
                    self.track_name(ui, id, name);
                    {
                        let m =
                            egui::Button::new(egui::RichText::new("M").strong().color(if mute {
                                theme::BG
                            } else {
                                theme::DIM
                            }))
                            .fill(if mute { theme::ACCENT } else { theme::PANEL })
                            .min_size(egui::vec2(26.0, 22.0));
                        if ui.add(m).on_hover_text("Stumm").clicked() {
                            self.state.toggle_mute(id);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    self.track_meter(ui, id, gain, mute);
                    let mut g = match self.gain_drag {
                        Some((d, v)) if d == id => v,
                        _ => gain,
                    };
                    let r = ui
                        .add(
                            egui::Slider::new(&mut g, MIN_GAIN_DB..=MAX_GAIN_DB)
                                .suffix(" dB")
                                .fixed_decimals(1),
                        )
                        .on_hover_text("Lautstärke der Spur (Doppelklick: 0 dB)");
                    if r.double_clicked() {
                        g = 0.0;
                        self.gain_drag = None;
                        self.state.set_gain(id, g);
                    } else {
                        if r.changed() {
                            self.gain_drag = Some((id, g));
                        }
                        if r.drag_stopped() || (r.changed() && !r.dragged()) {
                            self.gain_drag = None;
                            self.state.set_gain(id, g);
                        }
                    }
                });
                self.track_source_row(ui, id);
            })
        };
        let r = resp.response.rect;
        ui.painter().rect_filled(
            egui::Rect::from_min_size(r.min, egui::vec2(4.0, r.height())),
            egui::CornerRadius {
                nw: 6,
                sw: 6,
                ne: 0,
                se: 0,
            },
            color,
        );
    }

    /// Quellenwahl und Mithören einer Spur.
    fn track_source_row(&mut self, ui: &mut egui::Ui, id: TrackId) {
        let busy = !self.takes.is_empty();
        ui.horizontal(|ui| {
            let current = self.track_source.get(&id).cloned().unwrap_or_default();
            let mut chosen = None;
            ui.add_enabled_ui(!busy, |ui| {
                egui::ComboBox::from_id_salt(("source", id))
                    .selected_text(
                        egui::RichText::new(sources::short_label(&current))
                            .small()
                            .color(theme::DIM),
                    )
                    .width(150.0)
                    .truncate()
                    .show_ui(ui, |ui| {
                        for s in self.sources().to_vec() {
                            if ui.selectable_label(s.key == current, &s.label).clicked() {
                                chosen = Some(s.key);
                            }
                        }
                    })
                    .response
                    .on_hover_text(
                        "Aufnahmequelle der Spur: Mikrofon, Systemklang oder ein Programm",
                    );
            });
            if let Some(key) = chosen {
                if key == sources::DEFAULT_KEY {
                    self.track_source.remove(&id);
                    self.track_monitor.remove(&id);
                } else {
                    self.track_source.insert(id, key);
                }
            }
            let can = self.track_source.contains_key(&id);
            let mut on = self.track_monitor.contains(&id);
            let r = ui
                .add_enabled(
                    !busy && can,
                    egui::Checkbox::new(&mut on, egui::RichText::new("hören").small()),
                )
                .on_hover_text(
                    "Quelle beim Aufnehmen über den Ausgang hören. Für Mikrofone und \
                     Eingänge; Systemklang hört man ohnehin (Rückkopplung!).",
                )
                .on_disabled_hover_text("Erst eine Quelle aus der Liste wählen");
            if r.changed() {
                if on {
                    self.track_monitor.insert(id);
                } else {
                    self.track_monitor.remove(&id);
                }
            }
        });
    }

    /// Pegelanzeige am Spuranfang: Spitzenpegel nach Gain und Mute, dB-Skala -60..0,
    /// grün, ab -12 dB gelb, ab -3 dB rot; fällt langsam ab.
    fn track_meter(&mut self, ui: &mut egui::Ui, track: TrackId, gain_db: f64, mute: bool) {
        let raw = self.player.as_ref().map_or(0.0, |p| p.take_level(track));
        let project = self.state.project();
        let solo = project.tracks.iter().any(|t| t.solo);
        let audible = !mute && (!solo || project.track(track).is_some_and(|t| t.solo));
        let fresh = if audible {
            raw * 10f32.powf(gain_db as f32 / 20.0)
        } else {
            0.0
        };
        let fresh = fresh.max(self.rec_levels.insert(track, 0.0).unwrap_or(0.0));
        let shown = self.track_levels.entry(track).or_insert(0.0);
        *shown = fresh.max(*shown * 0.85);
        let level = *shown;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(60.0, 8.0), egui::Sense::hover());
        level_bar(ui.painter(), rect, level);
    }
}

/// Waagerechte Pegelanzeige, dB-Skala -60..0: grün, ab -12 dB gelb, ab -3 dB rot.
fn level_bar(painter: &egui::Painter, rect: egui::Rect, level: f32) {
    {
        painter.rect_filled(rect, 3.0, theme::BG);
        let db = 20.0 * level.max(1e-6).log10();
        let frac = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
        let color = if db > -3.0 {
            egui::Color32::from_rgb(220, 50, 50)
        } else if db > -12.0 {
            egui::Color32::from_rgb(230, 190, 40)
        } else {
            egui::Color32::from_rgb(60, 180, 75)
        };
        if frac > 0.0 {
            let mut bar = rect.shrink(2.0);
            bar.set_width(bar.width() * frac);
            painter.rect_filled(bar, 1.0, color);
        }
        for mark in [-12.0f32, -3.0] {
            let x = rect.left() + rect.width() * (mark + 60.0) / 60.0;
            painter.vline(x, rect.y_range(), egui::Stroke::new(1.0, theme::PANEL));
        }
    }
}

impl WoaApp {
    /// One track's strip: clips with waveform and fades, the playhead. Click: seek,
    /// click a clip: select, drag a clip: move it (snaps to 1/4 beat).
    fn timeline(&mut self, ui: &mut egui::Ui, track: TrackId) {
        let tempo = self.state.project().tempo_bpm;
        let px = self.zoom;
        let Some(t) = self.state.project().track(track) else {
            return;
        };
        let clips = t.clips.clone();
        let index = self
            .state
            .project()
            .tracks
            .iter()
            .position(|t| t.id == track)
            .unwrap_or(0);
        let color = theme::track_color(index);
        let end = self.timeline_end();
        let width = (end as f32 + 16.0) * px;
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, ROW_H), egui::Sense::click());
        self.track_rects.push((track, rect));
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 6.0, theme::BG);
        // Raster: jeder zweite Takt leicht aufgehellt, Schläge als feine Linien.
        let bars = end as usize / 4 + 4;
        for bar in 0..bars {
            let x = rect.left() + (bar * 4) as f32 * px;
            if bar % 2 == 1 {
                let r = egui::Rect::from_x_y_ranges(x..=x + 4.0 * px, rect.y_range());
                painter.rect_filled(r, 0.0, theme::mix(theme::BG, theme::PANEL, 0.45));
            }
            if px >= 10.0 {
                for b in 1..4 {
                    let xb = x + b as f32 * px;
                    painter.vline(
                        xb,
                        rect.y_range(),
                        egui::Stroke::new(1.0, theme::mix(theme::BG, theme::GRID, 0.4)),
                    );
                }
            }
            painter.vline(x, rect.y_range(), egui::Stroke::new(1.0, theme::GRID));
        }
        let ppb = self.player.as_ref().map(|p| p.peaks_per_beat(tempo));
        for c in &clips {
            let drag = match self.clip_drag {
                Some((id, d))
                    if id == c.id
                        || (self.selected_clips.contains(&id)
                            && self.selected_clips.contains(&c.id)) =>
                {
                    d
                }
                _ => 0.0,
            };
            // Gezogener Rand: Anzeige mit vorläufigen Werten.
            let (mut start, mut len, mut offset) =
                (c.start_beats + drag, c.length_beats, c.offset_beats);
            if let Some((id, left, d)) = self.trim
                && id == c.id
            {
                if left {
                    (start, len, offset) = (start + d, len - d, offset + d);
                } else {
                    len += d;
                }
            }
            let start = start.max(0.0);
            let x0 = rect.left() + start as f32 * px;
            let x1 = x0 + (len as f32 * px).max(2.0);
            let r = egui::Rect::from_x_y_ranges(x0..=x1, rect.top() + 3.0..=rect.bottom() - 3.0);
            let cresp = ui.interact(
                r,
                ui.id().with(("clip", c.id.0)),
                egui::Sense::click_and_drag(),
            );
            let base = if c.audio.is_some() { color } else { theme::DIM };
            let selected = self.selected_clip == Some(c.id) || self.selected_clips.contains(&c.id);
            painter.rect_filled(r, 5.0, theme::mix(base, theme::BG, 0.62));
            // Kopfzeile des Clips in der Spurfarbe, darin der Name.
            let head = egui::Rect::from_min_max(r.min, egui::pos2(r.right(), r.top() + 14.0));
            painter.rect_filled(
                head,
                egui::CornerRadius {
                    nw: 5,
                    ne: 5,
                    sw: 0,
                    se: 0,
                },
                theme::mix(base, theme::BG, if selected { 0.0 } else { 0.25 }),
            );
            // Waveform from the cached peaks.
            let file = c
                .audio
                .as_deref()
                .and_then(|rel| self.state.resolve_audio(rel));
            let peaks = match (file, self.player.as_mut()) {
                (Some(file), Some(p)) => p.peaks(&file),
                _ => None,
            };
            let audio_beats = peaks.as_ref().zip(ppb).map(|(p, ppb)| p.len() as f64 / ppb);
            if let (Some(ppb), Some(peaks)) = (ppb, peaks) {
                let mid = (r.top() + 14.0 + r.bottom()) / 2.0;
                let half = (r.bottom() - r.top() - 14.0) / 2.0 - 2.0;
                let stroke = egui::Stroke::new(1.0, theme::mix(base, egui::Color32::WHITE, 0.35));
                let mut x = r.left();
                while x < r.right() {
                    let b0 = offset + f64::from((x - r.left()) / px);
                    let b1 = b0 + f64::from(1.0 / px);
                    let (i0, i1) = (
                        (b0 * ppb) as usize,
                        ((b1 * ppb) as usize).max((b0 * ppb) as usize + 1),
                    );
                    let v = peaks
                        .get(i0.min(peaks.len())..i1.min(peaks.len()))
                        .map_or(0.0, |w| w.iter().fold(0.0f32, |m, s| m.max(*s)));
                    let local = f64::from((x - r.left()) / px);
                    let env = fade_gain(c, local) as f32;
                    let h = (v * env).min(1.0) * half;
                    painter.vline(x, (mid - h)..=(mid + h), stroke);
                    x += 1.0;
                }
            }
            // Fades as dark triangles.
            let shade = egui::Color32::from_black_alpha(130);
            if c.fade_in_beats > 0.0 {
                let fx = r.left() + c.fade_in_beats as f32 * px;
                painter.add(egui::Shape::convex_polygon(
                    vec![r.left_top(), egui::pos2(fx, r.top()), r.left_bottom()],
                    shade,
                    egui::Stroke::NONE,
                ));
            }
            if c.fade_out_beats > 0.0 {
                let fx = r.right() - c.fade_out_beats as f32 * px;
                painter.add(egui::Shape::convex_polygon(
                    vec![egui::pos2(fx, r.top()), r.right_top(), r.right_bottom()],
                    shade,
                    egui::Stroke::NONE,
                ));
            }
            if selected {
                painter.rect_stroke(
                    r,
                    5.0,
                    egui::Stroke::new(2.0, egui::Color32::WHITE),
                    egui::StrokeKind::Inside,
                );
            }
            painter.with_clip_rect(head.intersect(rect)).text(
                head.left_center() + egui::vec2(6.0, 0.0),
                egui::Align2::LEFT_CENTER,
                &c.name,
                egui::FontId::proportional(11.0),
                theme::BG,
            );
            if cresp.clicked() || cresp.drag_started() {
                let m = ui.input(|i| i.modifiers);
                if m.command || m.shift {
                    // Strg/Umschalt+Klick: zur Auswahl hinzufügen oder herausnehmen.
                    if let Some(p) = self.selected_clip
                        && !self.selected_clips.contains(&p)
                    {
                        self.selected_clips.push(p);
                    }
                    if cresp.clicked() && self.selected_clips.contains(&c.id) {
                        self.selected_clips.retain(|x| *x != c.id);
                    } else if !self.selected_clips.contains(&c.id) {
                        self.selected_clips.push(c.id);
                    }
                } else if !(cresp.drag_started() && self.selected_clips.contains(&c.id)) {
                    self.selected_clips = vec![c.id];
                }
                self.selected_clip = Some(c.id);
                self.selected_track = Some(track);
            }
            if cresp.dragged() {
                let d =
                    self.clip_drag.map_or(0.0, |(_, d)| d) + f64::from(cresp.drag_delta().x / px);
                self.clip_drag = Some((c.id, d));
            }
            if cresp.drag_stopped()
                && let Some((id, d)) = self.clip_drag.take()
            {
                let snapped = ((c.start_beats + d) * 4.0).round() / 4.0;
                if self.selected_clips.len() > 1 && self.selected_clips.contains(&id) {
                    let ids = self.selected_clips.clone();
                    self.state.shift_clips(&ids, snapped - c.start_beats);
                    continue;
                }
                // Losgelassen über einer anderen Spur: dorthin verschieben.
                let target = ui
                    .ctx()
                    .pointer_latest_pos()
                    .and_then(|pos| {
                        self.prev_track_rects
                            .iter()
                            .find(|(_, r)| r.y_range().contains(pos.y))
                    })
                    .map(|(t, _)| *t);
                self.state.move_clip_to(id, target, snapped);
            }
            cresp.on_hover_text(
                "Ziehen: verschieben (auch auf andere Spur) · Ränder ziehen: kürzen · S: am Kopf schneiden · Entf: löschen",
            );
            // Griffe an beiden Rändern zum Kürzen.
            for left in [true, false] {
                let hx = if left { r.left() } else { r.right() - 6.0 };
                let h = egui::Rect::from_x_y_ranges(hx..=hx + 6.0, r.y_range());
                let hresp = ui
                    .interact(h, ui.id().with(("trim", c.id.0, left)), egui::Sense::drag())
                    .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
                if hresp.drag_started() {
                    self.selected_clip = Some(c.id);
                }
                if hresp.dragged() {
                    let d =
                        self.trim.map_or(0.0, |(_, _, d)| d) + f64::from(hresp.drag_delta().x / px);
                    // Grenzen: mindestens 1/4 Beat lang, links nicht vor den Audio-Anfang,
                    // rechts nicht über das Audio-Ende (wenn bekannt).
                    let d = if left {
                        d.clamp(-c.offset_beats, c.length_beats - 0.25)
                    } else {
                        let max =
                            audio_beats.map_or(f64::MAX, |a| a - c.offset_beats - c.length_beats);
                        d.clamp(0.25 - c.length_beats, max.max(0.0))
                    };
                    self.trim = Some((c.id, left, d));
                }
                if hresp.drag_stopped()
                    && let Some((id, l, d)) = self.trim.take()
                {
                    let d = (d * 4.0).round() / 4.0;
                    if l {
                        self.state.trim_clip(
                            id,
                            c.start_beats + d,
                            c.length_beats - d,
                            c.offset_beats + d,
                        );
                    } else {
                        self.state
                            .trim_clip(id, c.start_beats, c.length_beats + d, c.offset_beats);
                    }
                }
            }
        }
        if let Some(p) = &self.player {
            let x = rect.left() + p.position_beats(tempo) as f32 * px;
            painter.vline(x, rect.y_range(), egui::Stroke::new(2.0, theme::ACCENT));
        }
        if resp.clicked() {
            self.selected_track = Some(track);
            self.selected_clip = None;
            self.selected_clips.clear();
            if let (Some(pos), Some(p)) = (resp.interact_pointer_pos(), self.player.as_mut()) {
                p.seek_beats(f64::from((pos.x - rect.left()) / px), tempo);
            }
        }
    }

    /// Bearbeitungszeile für den gewählten Clip: Fades, Schneiden, Löschen.
    fn clip_bar(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.selected_clip else {
            ui.label("Clip anklicken zum Bearbeiten · Strg+Klick: mehrere · Strg+Mausrad: Zoom · Strg+E: Export");
            return;
        };
        let Some((_, c)) = self.state.project().clip(id) else {
            self.selected_clip = None;
            self.selected_clips.clear();
            return;
        };
        let c = c.clone();
        ui.horizontal(|ui| {
            ui.strong(&c.name);
            let (mut fi, mut fo) = match self.fade_edit {
                Some((cid, a, b)) if cid == id => (a, b),
                _ => (c.fade_in_beats, c.fade_out_beats),
            };
            let r1 = ui.add(
                egui::DragValue::new(&mut fi)
                    .range(0.0..=c.length_beats)
                    .speed(0.05)
                    .prefix("Fade-in ")
                    .suffix(" Beats"),
            );
            let r2 = ui.add(
                egui::DragValue::new(&mut fo)
                    .range(0.0..=c.length_beats)
                    .speed(0.05)
                    .prefix("Fade-out ")
                    .suffix(" Beats"),
            );
            if r1.changed() || r2.changed() {
                self.fade_edit = Some((id, fi, fo));
            }
            let dragging = r1.dragged() || r2.dragged() || r1.has_focus() || r2.has_focus();
            if !dragging && let Some((cid, a, b)) = self.fade_edit.take() {
                self.state.set_fades(cid, a, b);
            }
            if ui.button("✂ Schneiden (S)").clicked() {
                self.split_at_playhead();
            }
            let n = self.selected_clips.len().max(1);
            let label = if n > 1 {
                format!("🗑 {n} Clips löschen (Entf)")
            } else {
                "🗑 Löschen (Entf)".to_string()
            };
            if ui.button(label).clicked() {
                self.delete_selected();
            }
        });
    }

    fn delete_selected(&mut self) {
        let mut ids = std::mem::take(&mut self.selected_clips);
        if let Some(c) = self.selected_clip.take()
            && !ids.contains(&c)
        {
            ids.push(c);
        }
        match ids.as_slice() {
            [] => {}
            [one] => {
                self.state.remove_clip(*one);
            }
            many => {
                self.state.remove_clips(many);
            }
        }
    }

    fn split_at_playhead(&mut self) {
        let Some(id) = self.selected_clip else { return };
        let tempo = self.state.project().tempo_bpm;
        let at = self
            .player
            .as_ref()
            .map_or(0.0, |p| p.position_beats(tempo));
        match self.state.split_clip(id, at) {
            Some(right) => self.selected_clip = Some(right),
            None => {
                self.state.status =
                    Status::Info("Zum Schneiden den roten Kopf in den Clip setzen".into());
            }
        }
    }

    fn export(&mut self) {
        let Some(out) = rfd::FileDialog::new()
            .set_title("Exportieren")
            .set_file_name(format!("{}.wav", self.state.project().meta.name))
            .add_filter("WAV", &["wav"])
            .add_filter("FLAC", &["flac"])
            .add_filter("MP3 (320 kbit/s)", &["mp3"])
            .save_file()
        else {
            return;
        };
        let out = if out.extension().is_none() {
            out.with_extension("wav")
        } else {
            out
        };
        let project = self.state.project().clone();
        let state = &self.state;
        let result = match self.player.as_mut() {
            Some(p) => p.export(
                &project,
                &|rel| state.resolve_audio(rel),
                &out,
                self.export_lufs,
            ),
            None => Err("keine Audio-Engine".into()),
        };
        self.state.status = match result {
            Ok((secs, norm)) => {
                tracing::info!(seconds = secs, ?norm, "export done");
                let loud = match norm {
                    Some(n) => format!(
                        ", Lautheit {:.1} → {:.1} LUFS{}",
                        n.before_lufs,
                        n.before_lufs + n.gain_db,
                        if n.peak_limited {
                            " (Spitzen begrenzen, leiser als Ziel)"
                        } else {
                            ""
                        }
                    ),
                    None => String::new(),
                };
                Status::Info(format!("Exportiert: {} ({secs:.1} s{loud})", out.display()))
            }
            Err(e) => {
                tracing::error!(error = %e, "export failed");
                Status::Error(format!("Export fehlgeschlagen: {e}"))
            }
        };
    }

    fn track_name(&mut self, ui: &mut egui::Ui, id: TrackId, name: &str) {
        match &mut self.rename {
            Some((rid, buf)) if *rid == id => {
                let r = ui.add(egui::TextEdit::singleline(buf).desired_width(130.0));
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
                    .add_sized(
                        [130.0, 22.0],
                        egui::Button::new(egui::RichText::new(name).strong())
                            .frame(false)
                            .truncate(),
                    )
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
        let project = self.state.project().clone();
        let state = &self.state;
        if let Some(p) = self.player.as_mut() {
            p.sync(&project, &|rel| state.resolve_audio(rel));
        }
        self.stop_at_audio_end();
        let busy = !self.takes.is_empty() || self.player.as_ref().is_some_and(Player::is_playing);
        let beat = if busy { 40 } else { 500 };
        ctx.request_repaint_after(std::time::Duration::from_millis(beat));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.photo(&ctx);
        if !self.themed {
            theme::apply(&ctx);
            self.themed = true;
        }
        egui::Panel::top("menu").show(ui, |ui| self.menu(ui, &ctx));
        egui::Panel::top("transport")
            .frame(
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ui, |ui| self.transport_bar(ui));
        egui::Panel::top("clipbar")
            .frame(
                egui::Frame::new()
                    .fill(theme::mix(theme::PANEL, theme::BG, 0.5))
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ui, |ui| ui.horizontal(|ui| self.clip_bar(ui)));
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::symmetric(12, 4)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    match &self.state.status {
                        Status::Idle => {
                            ui.label("");
                        }
                        Status::Info(m) => {
                            ui.label(egui::RichText::new(m).small().color(theme::DIM));
                        }
                        Status::Error(m) => {
                            ui.colored_label(ui.visuals().error_fg_color, m);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("WOA {}", env!("CARGO_PKG_VERSION")))
                                .small()
                                .color(theme::DIM),
                        );
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::mix(theme::PANEL, theme::BG, 0.5))
                    .inner_margin(egui::Margin::symmetric(12, 10)),
            )
            .show(ui, |ui| self.tracks(ui));
        self.feedback_window(&ctx);
    }
}

/// Fade-Hüllkurve eines Clips an Stelle `local` (Beats ab Clip-Anfang), 0..1.
fn fade_gain(c: &woa_model::Clip, local: f64) -> f64 {
    let mut g = 1.0f64;
    if c.fade_in_beats > 0.0 {
        g = g.min(local / c.fade_in_beats);
    }
    if c.fade_out_beats > 0.0 {
        g = g.min((c.length_beats - local) / c.fade_out_beats);
    }
    g.clamp(0.0, 1.0)
}
