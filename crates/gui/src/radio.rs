//! Radio-Layout (Sendeablauf): Playlist, große Uhr, Start/Weiter, Stopp, Cart-Wall,
//! Mikrofon mit Ducking, Mitschnitt. Alle Logik steckt in `woa_radio`; hier wird nur
//! gezeichnet und weitergereicht.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime};

use eframe::egui;
use woa_radio::station::{Station, View};
use woa_radio::{CARTS, Controller, RadioShow};
use woa_stream::StreamStatus;
use woa_stream::link::{Feeder, is_alive, link_path};

use crate::theme;
use crate::xrun::XrunLog;

type Stream = Box<dyn std::any::Any>;

/// Tasten für die ersten zehn Carts: 1–9, dann 0.
const CART_KEYS: [egui::Key; 10] = [
    egui::Key::Num1,
    egui::Key::Num2,
    egui::Key::Num3,
    egui::Key::Num4,
    egui::Key::Num5,
    egui::Key::Num6,
    egui::Key::Num7,
    egui::Key::Num8,
    egui::Key::Num9,
    egui::Key::Num0,
];

/// Die laufende Sendung samt Gerät.
pub struct RadioDesk {
    radio_path: PathBuf,
    data_dir: PathBuf,
    station: Option<Station>,
    _output: Option<Stream>,
    mic: Option<Stream>,
    rate: u32,
    error: Option<String>,
    view: View,
    saved_revision: u64,
    saved_at: Instant,
    known_mtime: Option<SystemTime>,
    checked_at: Instant,
    mic_dropped: Arc<AtomicU64>,
    covers: Option<crate::covers::Covers>,
    /// Sendung: Feeder zum woa-streamd (nur wenn „Auf Sendung“).
    feeder: Option<Feeder>,
    stream_status: Option<StreamStatus>,
    stream_checked_at: Instant,
    sent_title: String,
    /// Aussetzer der Ausgabe (vom Fehlerpfad des Stroms gezählt).
    underruns: Arc<AtomicU64>,
    xruns: XrunLog,
    buffer_frames: Option<u32>,
    device: String,
}

/// Pfad zu `woa-streamd` neben der laufenden App.
fn streamd_exe() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let name = if cfg!(windows) {
        "woa-streamd.exe"
    } else {
        "woa-streamd"
    };
    let p = exe
        .parent()
        .map(|d| d.join(name))
        .ok_or("Programmordner unbekannt")?;
    if p.is_file() {
        Ok(p)
    } else {
        Err(format!("{} fehlt", p.display()))
    }
}

/// Startet woa-streamd als eigenständigen Prozess: eigene Prozessgruppe, keine
/// Standard-Ein-/Ausgabe. So beendet weder Strg+C im Terminal noch ein Absturz der
/// Oberfläche die Sendung.
pub fn spawn_streamd(data_dir: &Path) -> Result<(), String> {
    use std::process::{Command, Stdio};
    let exe = streamd_exe()?;
    let mut cmd = Command::new(exe);
    cmd.arg("--data-dir")
        .arg(data_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    cmd.spawn()
        .map(|_child| ())
        .map_err(|e| format!("woa-streamd startet nicht: {e}"))
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// `mm:ss`, ab einer Stunde `h:mm:ss`.
pub fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

impl RadioDesk {
    /// Lädt `radio.json` und öffnet die Ausgabe. Geht das Gerät nicht, bleibt die
    /// Playlist bearbeitbar und die Meldung steht oben.
    /// `buffer_frames`: Puffergröße der Ausgabe (`None` = Treiber).
    pub fn open(radio_path: PathBuf, data_dir: PathBuf, buffer_frames: Option<u32>) -> Self {
        let (show, mut error) = match RadioShow::load(&radio_path) {
            Ok(s) => (s, None),
            Err(e) => (RadioShow::default(), Some(e)),
        };
        let config = woa_engine::device::OutputConfig {
            device_id: None,
            buffer_frames,
        };
        let rate = woa_engine::device::output_sample_rate(&config).unwrap_or(48_000);
        let (controller, mut mixer) = Controller::new(show, rate);
        let device = woa_engine::device::output_device_name(&config);
        let underruns = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&underruns);
        let output: Option<Stream> = match woa_engine::device::start_stereo_output(
            &config,
            move |l, r| mixer.process(l, r),
            // Kann im Audio-Thread laufen: Aussetzer nur zählen, geloggt wird in `tick`.
            move |e| {
                if woa_engine::device::is_xrun(&e) {
                    counter.fetch_add(1, Ordering::Relaxed);
                } else {
                    tracing::warn!(error = %e, "radio output");
                }
            },
        ) {
            Ok(s) => {
                tracing::info!(rate, ?buffer_frames, %device, "radio output ready");
                Some(Box::new(s))
            }
            Err(e) => {
                tracing::error!(error = %e, "radio output failed");
                error = Some(format!("Keine Ausgabe: {e}"));
                None
            }
        };
        let station = match Station::spawn(controller) {
            Ok(s) => {
                s.measure_missing(&s.show());
                Some(s)
            }
            Err(e) => {
                error = Some(format!("Sendesteuerung startet nicht: {e}"));
                None
            }
        };
        let known_mtime = mtime(&radio_path);
        Self {
            radio_path,
            data_dir,
            station,
            _output: output,
            mic: None,
            rate,
            error,
            view: View::default(),
            saved_revision: 0,
            saved_at: Instant::now(),
            known_mtime,
            checked_at: Instant::now(),
            mic_dropped: Arc::new(AtomicU64::new(0)),
            covers: None,
            feeder: None,
            stream_status: None,
            stream_checked_at: Instant::now(),
            sent_title: String::new(),
            underruns,
            xruns: XrunLog::new(Instant::now()),
            buffer_frames,
            device,
        }
        .reattach()
    }

    /// Läuft woa-streamd schon (Oberfläche nach Absturz neu gestartet), hängt sich die
    /// Oberfläche wieder an; woa-streamd blendet von der Notfall-Playlist zurück.
    fn reattach(mut self) -> Self {
        if self.station.is_some() && is_alive(&link_path(&self.data_dir)) {
            tracing::info!("streamd running, reattaching");
            self.attach_feeder();
        }
        self
    }

    fn attach_feeder(&mut self) {
        if self.feeder.is_some() || self.station.is_none() {
            return;
        }
        // Eine Sekunde Puffer; der Feeder leert ihn alle 20 ms.
        let (producer, consumer) = rtrb::RingBuffer::new(self.rate as usize * 2);
        match Feeder::spawn(consumer, self.rate, link_path(&self.data_dir)) {
            Ok(f) => {
                self.with(move |c| c.set_stream_output(Some(producer)));
                self.sent_title.clear();
                self.feeder = Some(f);
            }
            Err(e) => self.error = Some(format!("Sendung: {e}")),
        }
    }

    /// „Auf Sendung“: woa-streamd starten (falls nötig) und den Sendemix hinschicken.
    pub fn go_on_air(&mut self) {
        if self.station.is_none() {
            self.error = Some("Keine Sendesteuerung".into());
            return;
        }
        if !is_alive(&link_path(&self.data_dir))
            && let Err(e) = spawn_streamd(&self.data_dir)
        {
            tracing::error!(error = %e, "streamd spawn failed");
            self.error = Some(e);
            return;
        }
        self.attach_feeder();
    }

    /// „Sendung beenden“: woa-streamd meldet sich beim Server ab und endet.
    pub fn go_off_air(&mut self) {
        self.with(|c| c.set_stream_output(None));
        if let Some(f) = self.feeder.take() {
            f.stop_broadcast();
        }
    }

    /// Zustand der Sendung (Status-Datei von woa-streamd), wenn er läuft.
    pub fn stream_status(&self) -> Option<&StreamStatus> {
        self.stream_status.as_ref()
    }

    fn tick_stream(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.stream_checked_at).as_millis() >= 500 {
            self.stream_checked_at = now;
            let path = woa_stream::status::status_path(&self.data_dir);
            self.stream_status = woa_stream::status::read(&path);
        }
        if let Some(f) = &self.feeder {
            let title = self
                .view
                .current
                .and_then(|i| self.view.playlist.get(i))
                .map(|e| e.title.clone())
                .unwrap_or_default();
            if title != self.sent_title {
                f.set_title(&title);
                self.sent_title = title;
            }
        }
    }

    /// Aussetzer der Radio-Ausgabe seit dem Öffnen.
    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }

    fn with(&self, f: impl FnOnce(&mut Controller) + Send + 'static) {
        if let Some(s) = &self.station {
            s.with(f);
        }
    }

    /// Ein Takt: Schnappschuss holen, speichern, fremde Änderungen an radio.json laden.
    /// `player_active`: Der Player-Strom läuft gleichzeitig (für das Log).
    pub fn tick(&mut self, player_active: bool) {
        let Some(st) = &self.station else { return };
        self.view = st.view();
        self.tick_stream();
        let total = self.underruns();
        if let Some(new) = self.xruns.due(total, Instant::now()) {
            let v = &self.view;
            let title = v.current.and_then(|i| v.playlist.get(i));
            let format = title
                .and_then(|e| e.path.extension())
                .map(|x| x.to_string_lossy().to_lowercase());
            let line = (
                v.last_load.as_ref().map(|(_, ms)| *ms),
                v.last_load
                    .as_ref()
                    .and_then(|(p, _)| p.extension())
                    .map(|x| x.to_string_lossy().to_lowercase()),
            );
            let fields = (title.map(|e| e.title.clone()), format);
            if new > 0 {
                tracing::warn!(
                    output = "radio",
                    underruns = new,
                    total,
                    buffer_frames = ?self.buffer_frames,
                    rate = self.rate,
                    device = %self.device,
                    recording = v.recording.is_some(),
                    player_active,
                    playing = v.running,
                    title = ?fields.0,
                    format = ?fields.1,
                    last_load_ms = ?line.0,
                    last_load_format = ?line.1,
                    "output underruns (last minute)"
                );
            } else {
                tracing::info!(
                    output = "radio",
                    underruns = 0,
                    total,
                    buffer_frames = ?self.buffer_frames,
                    rate = self.rate,
                    device = %self.device,
                    recording = v.recording.is_some(),
                    player_active,
                    playing = v.running,
                    title = ?fields.0,
                    format = ?fields.1,
                    last_load_ms = ?line.0,
                    last_load_format = ?line.1,
                    "output underruns (last minute)"
                );
            }
        }
        let Some(st) = &self.station else { return };
        let now = Instant::now();
        if self.view.revision != self.saved_revision
            && now.duration_since(self.saved_at).as_millis() >= 700
        {
            match st.show().save(&self.radio_path) {
                Ok(()) => {
                    self.saved_revision = self.view.revision;
                    self.known_mtime = mtime(&self.radio_path);
                }
                Err(e) => self.error = Some(format!("radio.json nicht gespeichert: {e}")),
            }
            self.saved_at = now;
        }
        if now.duration_since(self.checked_at).as_secs() >= 2 {
            self.checked_at = now;
            let m = mtime(&self.radio_path);
            if m.is_some() && m != self.known_mtime {
                // Changed outside (agent tool build_playlist): take it over.
                self.known_mtime = m;
                match RadioShow::load(&self.radio_path) {
                    Ok(show) => {
                        if let Some(st) = &self.station {
                            st.measure_missing(&show);
                        }
                        self.with(move |c| c.replace_show(show));
                    }
                    Err(e) => self.error = Some(e),
                }
            }
        }
    }

    pub fn is_on_air(&self) -> bool {
        self.view.running
    }

    fn add_dialog(&self) {
        if let Some(files) = rfd::FileDialog::new()
            .add_filter(
                woa_engine::decode::FILTER_NAME,
                &woa_engine::decode::dialog_extensions(),
            )
            .pick_files()
            && let Some(s) = &self.station
        {
            s.add_files(files);
        }
    }

    fn toggle_mic(&mut self) {
        if self.mic.is_none() {
            let (producer, consumer) = rtrb::RingBuffer::new(self.rate as usize);
            match woa_engine::record::start_live_input(
                &woa_engine::record::InputConfig::default(),
                producer,
                Arc::clone(&self.mic_dropped),
                |e| tracing::warn!(error = %e, "mic input"),
            ) {
                Ok((stream, rate)) if rate == self.rate => {
                    self.mic = Some(Box::new(stream));
                    self.with(move |c| c.set_mic_input(Some(consumer)));
                }
                Ok((_, rate)) => {
                    self.error = Some(format!(
                        "Mikrofon läuft mit {rate} Hz, die Ausgabe mit {} Hz. Bitte im System \
                         gleich einstellen.",
                        self.rate
                    ));
                    return;
                }
                Err(e) => {
                    self.error = Some(format!("Kein Mikrofon: {e}"));
                    return;
                }
            }
        }
        let open = !self.view.mic_open;
        self.view.mic_open = open;
        self.with(move |c| c.set_mic_open(open));
    }

    fn toggle_recording(&mut self) {
        if self.view.recording.is_some() {
            self.with(|c| {
                c.stop_recording();
            });
        } else {
            let dir = self.data_dir.clone();
            self.with(move |c| c.start_recording(&dir));
        }
    }

    /// Tasten im Radio-Modus (nur, wenn kein Textfeld den Fokus hat).
    pub fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let n = egui::Modifiers::NONE;
        let (mic, go, carts) = ctx.input_mut(|i| {
            let carts: Vec<usize> = CART_KEYS
                .iter()
                .enumerate()
                .filter(|(_, k)| i.consume_key(n, **k))
                .map(|(slot, _)| slot)
                .collect();
            (
                i.consume_key(n, egui::Key::M),
                i.consume_key(n, egui::Key::Space),
                carts,
            )
        });
        for slot in carts {
            self.with(move |c| c.trigger_cart(slot));
        }
        if mic {
            self.toggle_mic();
        }
        if go {
            self.with(|c| c.start_or_next());
        }
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if !dropped.is_empty()
            && let Some(s) = &self.station
        {
            s.add_files(dropped);
        }
    }

    /// Zeichnet das ganze Layout in `ui`.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let v = self.view.clone();
        if let Some(e) = self.error.clone() {
            ui.horizontal(|ui| {
                ui.colored_label(ui.visuals().error_fg_color, e);
                if ui.small_button("x").clicked() {
                    self.error = None;
                }
            });
        }
        for m in &v.messages {
            ui.label(egui::RichText::new(m).small().color(theme::DIM));
        }
        ui.columns(2, |cols| {
            self.clock_and_controls(&mut cols[0], &v);
            cols[0].add_space(12.0);
            self.playlist(&mut cols[0], &v);
            self.cart_wall(&mut cols[1], &v);
        });
    }

    fn clock_and_controls(&mut self, ui: &mut egui::Ui, v: &View) {
        let title = v
            .current
            .and_then(|i| v.playlist.get(i))
            .map(|e| e.title.clone())
            .unwrap_or_else(|| "–".to_string());
        // Cover des laufenden Titels (vor dem Start: des Titels, der als Nächstes kommt).
        let cover_path = v
            .current
            .or((!v.running).then_some(v.cue))
            .and_then(|i| v.playlist.get(i))
            .map(|e| e.path.clone());
        let ctx = ui.ctx().clone();
        let covers = self
            .covers
            .get_or_insert_with(|| crate::covers::Covers::new(&ctx));
        covers.poll(&ctx);
        let (now, before, t) = covers.big(&ctx, cover_path.as_deref());
        ui.horizontal(|ui| {
            crate::covers::paint_square(ui, 168.0, now.as_ref(), before.as_ref(), t)
                .on_hover_text("Cover des Titels (aus der Datei)");
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(title).size(18.0));
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(clock(v.remaining_title))
                            .size(64.0)
                            .monospace()
                            .color(if v.running && v.remaining_title < 10.0 {
                                theme::ON_AIR
                            } else {
                                theme::ACCENT
                            }),
                    )
                    .on_hover_text("Restzeit des laufenden Titels");
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("bis Playlist-Ende").color(theme::DIM));
                        ui.label(
                            egui::RichText::new(clock(v.remaining_total))
                                .size(28.0)
                                .monospace(),
                        );
                    });
                    let n = self.underruns();
                    if n > 0 {
                        ui.label(
                            egui::RichText::new(format!("Aussetzer: {n}"))
                                .small()
                                .color(theme::DIM),
                        )
                        .on_hover_text(
                            "Puffer-Unterläufe der Ausgabe seit dem Öffnen. Steigt die Zahl, \
                             in config.json radio_buffer_frames setzen, etwa 4096 (Vorgabe: Treiber).",
                        );
                    }
                });
            });
        });
        ui.horizontal_wrapped(|ui| {
            let big = |t: &str| egui::RichText::new(t).size(17.0);
            let go = if v.running { "▶ Weiter" } else { "▶ Start" };
            if ui
                .add(egui::Button::new(big(go)).min_size(egui::vec2(100.0, 40.0)))
                .on_hover_text("Leertaste")
                .clicked()
            {
                self.with(|c| c.start_or_next());
            }
            if ui
                .add(egui::Button::new(big("■ Stopp")).min_size(egui::vec2(90.0, 40.0)))
                .clicked()
            {
                self.with(|c| c.stop());
            }
            let mic = egui::Button::new(big("Mikro (M)").color(if v.mic_open {
                theme::BG
            } else {
                ui.visuals().text_color()
            }))
            .fill(if v.mic_open {
                theme::ON_AIR
            } else {
                theme::SURFACE
            })
            .min_size(egui::vec2(90.0, 40.0));
            if ui.add(mic).on_hover_text("Taste M").clicked() {
                self.toggle_mic();
            }
            let rec = if v.recording.is_some() {
                "Mitschnitt läuft"
            } else {
                "Mitschnitt"
            };
            let rec_on = v.recording.is_some();
            if ui
                .add(
                    egui::Button::new(big(rec).color(if rec_on {
                        theme::BG
                    } else {
                        ui.visuals().text_color()
                    }))
                    .fill(if rec_on {
                        theme::ON_AIR
                    } else {
                        theme::SURFACE
                    })
                    .min_size(egui::vec2(120.0, 40.0)),
                )
                .on_hover_text(format!(
                    "Sendung als WAV nach {}",
                    self.data_dir.join(woa_radio::RECORDINGS_DIR).display()
                ))
                .clicked()
            {
                self.toggle_recording();
            }
        });
        self.on_air_row(ui);
        // Je Regler eine Zeile: Ein Slider ist ein Widget, `horizontal_wrapped` bricht nicht
        // in ihm um, und bei 900 px Fensterbreite wurde die zweite Beschriftung abgeschnitten.
        let mut xf = v.crossfade_seconds;
        if ui
            .add(
                egui::Slider::new(&mut xf, 0.0..=10.0)
                    .suffix(" s")
                    .text("Crossfade"),
            )
            .changed()
        {
            self.view.crossfade_seconds = xf;
            self.with(move |c| c.set_crossfade(xf));
        }
        let mut duck = v.duck_db;
        if ui
            .add(
                egui::Slider::new(&mut duck, -30.0..=0.0)
                    .suffix(" dB")
                    .text("Ducking"),
            )
            .changed()
        {
            self.view.duck_db = duck;
            self.with(move |c| c.set_duck(duck));
        }
    }

    fn on_air_row(&mut self, ui: &mut egui::Ui) {
        let now = woa_stream::unix_now();
        let st = self.stream_status.clone();
        let running = st.as_ref().is_some_and(|s| s.is_running(now));
        let on = self.feeder.is_some();
        ui.horizontal_wrapped(|ui| {
            let (label, fill) = if on {
                ("Sendung beenden", theme::ON_AIR)
            } else {
                ("Auf Sendung", theme::SURFACE)
            };
            let text = egui::RichText::new(label).size(17.0).color(if on {
                theme::BG
            } else {
                ui.visuals().text_color()
            });
            if ui
                .add(
                    egui::Button::new(text)
                        .fill(fill)
                        .min_size(egui::vec2(140.0, 40.0)),
                )
                .on_hover_text("Sendung an den eigenen Shoutcast (woa-streamd)")
                .clicked()
            {
                if on {
                    self.go_off_air();
                } else {
                    self.go_on_air();
                }
            }
            match &st {
                Some(s) if running => {
                    let color = if s.emergency || !s.connected {
                        theme::ON_AIR
                    } else {
                        theme::ACCENT
                    };
                    ui.label(egui::RichText::new(s.summary()).color(color))
                        .on_hover_text(format!("{} · {}", s.server, s.title));
                }
                Some(s) if s.last_error.is_some() && on => {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        s.last_error.clone().unwrap_or_default(),
                    );
                }
                _ if on => {
                    ui.label(egui::RichText::new("starte woa-streamd …").color(theme::DIM));
                }
                _ => {}
            }
        });
        if let Some(s) = st.as_ref().filter(|s| running && s.last_error.is_some()) {
            ui.label(
                egui::RichText::new(s.last_error.clone().unwrap_or_default())
                    .small()
                    .color(theme::DIM),
            );
        }
    }

    fn playlist(&mut self, ui: &mut egui::Ui, v: &View) {
        ui.horizontal(|ui| {
            ui.heading("Playlist");
            if ui.button("+ Titel …").clicked() {
                self.add_dialog();
            }
            if v.measuring > 0 {
                ui.spinner();
                ui.label(egui::RichText::new("misst Lautheit …").color(theme::DIM));
            }
        });
        if v.playlist.is_empty() {
            ui.label(
                egui::RichText::new(
                    "Audiodateien (MP3, FLAC, WAV, OGG, M4A, AIFF …) hierher ziehen oder „+ Titel …“.",
                )
                .color(theme::DIM),
            );
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, e) in v.playlist.iter().enumerate() {
                let on_air = v.current == Some(i);
                let frame = egui::Frame::new()
                    .fill(if on_air {
                        theme::mix(theme::ON_AIR, theme::PANEL, 0.6)
                    } else if i == v.cue && !v.running {
                        theme::SURFACE
                    } else {
                        theme::PANEL
                    })
                    .inner_margin(egui::Margin::symmetric(8, 4));
                frame.show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(format!("{:>2}", i + 1));
                        let thumb = self.covers.as_mut().and_then(|c| c.thumb(&e.path));
                        crate::covers::paint_square(ui, 32.0, thumb.as_ref(), None, 1.0);
                        if ui.small_button("^").on_hover_text("Nach oben").clicked() && i > 0 {
                            self.with(move |c| {
                                c.move_entry(i, i - 1);
                            });
                        }
                        if ui.small_button("v").on_hover_text("Nach unten").clicked() {
                            self.with(move |c| {
                                c.move_entry(i, i + 1);
                            });
                        }
                        if ui.small_button("x").on_hover_text("Entfernen").clicked() {
                            self.with(move |c| {
                                c.remove(i);
                            });
                        }
                        ui.label(
                            egui::RichText::new(
                                e.seconds.map(clock).unwrap_or_else(|| "--:--".into()),
                            )
                            .monospace(),
                        );
                        if ui
                            .add(
                                egui::Label::new(&e.title)
                                    .truncate()
                                    .sense(egui::Sense::click()),
                            )
                            .on_hover_text(format!(
                                "{}\n{}\nDoppelklick: als Nächstes beim Start",
                                e.path.display(),
                                e.lufs
                                    .map(|l| format!("{l:.1} LUFS, Auto-Gain {:+.1} dB", e.gain_db))
                                    .unwrap_or_else(|| "noch nicht gemessen".into())
                            ))
                            .double_clicked()
                        {
                            self.with(move |c| c.set_cue(i));
                        }
                    });
                });
            }
        });
    }

    fn cart_wall(&mut self, ui: &mut egui::Ui, v: &View) {
        ui.heading("Cart-Wall");
        ui.label(
            egui::RichText::new("Klick oder Taste 1–9, 0 spielt; Rechtsklick belegt.")
                .small()
                .color(theme::DIM),
        );
        let w = ((ui.available_width() - 3.0 * 8.0) / 4.0).max(60.0);
        egui::Grid::new("carts")
            .spacing(egui::vec2(8.0, 8.0))
            .show(ui, |ui| {
                for slot in 0..CARTS {
                    let label = v.carts.get(slot).cloned().flatten();
                    let playing = v.carts_playing & (1 << slot) != 0;
                    let key = match slot {
                        0..=8 => format!("{}", slot + 1),
                        9 => "0".into(),
                        _ => String::new(),
                    };
                    let text = format!("{key}\n{}", label.clone().unwrap_or_else(|| "leer".into()));
                    let b = egui::Button::new(egui::RichText::new(text).color(if playing {
                        theme::BG
                    } else if label.is_some() {
                        ui.visuals().text_color()
                    } else {
                        theme::DIM
                    }))
                    .fill(if playing {
                        theme::ACCENT
                    } else {
                        theme::SURFACE
                    })
                    .min_size(egui::vec2(w, 64.0));
                    let r = ui.add(b);
                    if r.clicked() {
                        if label.is_some() {
                            self.with(move |c| c.trigger_cart(slot));
                        } else {
                            self.assign_cart(slot);
                        }
                    }
                    r.context_menu(|ui| {
                        if ui.button("Belegen …").clicked() {
                            self.assign_cart(slot);
                            ui.close();
                        }
                        if label.is_some() && ui.button("Leeren").clicked() {
                            self.with(move |c| {
                                let _ = c.assign_cart(slot, None);
                            });
                            ui.close();
                        }
                    });
                    if slot % 4 == 3 {
                        ui.end_row();
                    }
                }
            });
    }

    fn assign_cart(&self, slot: usize) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(
                woa_engine::decode::FILTER_NAME,
                &woa_engine::decode::dialog_extensions(),
            )
            .pick_file()
        {
            self.with(move |c| {
                if let Err(e) = c.assign_cart(slot, Some(path)) {
                    tracing::warn!(error = %e, "cart not assigned");
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn clock_format() {
        assert_eq!(super::clock(0.0), "00:00");
        assert_eq!(super::clock(83.4), "01:23");
        assert_eq!(super::clock(3725.0), "1:02:05");
        assert_eq!(super::clock(-3.0), "00:00");
    }
}
