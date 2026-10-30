//! Radio-Layout (Sendeablauf): Playlist, große Uhr, Start/Weiter, Stopp, Cart-Wall,
//! Mikrofon mit Ducking, Mitschnitt. Alle Logik steckt in `woa_radio`; hier wird nur
//! gezeichnet und weitergereicht.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime};

use eframe::egui;
use woa_io::controller::{Bus, Target};
use woa_radio::mixer::LevelBus;
use woa_radio::station::{Station, View, ViewEntry};
use woa_radio::{CARTS, Controller, RadioShow};
use woa_stream::StreamStatus;
use woa_stream::link::{Feeder, is_alive, link_path};

use crate::cue_editor::{CueAction, CueEditor};
use crate::feedback::PflOutput;
use crate::theme;
use crate::voice_editor::{VoiceAction, VoiceEditor, VoiceInput};
use crate::xrun::XrunLog;
use woa_engine::localtime::Zone;

const NO_PFL: &str = "Kein Kopfhörer-Ausgang: Einstellungen → Vorhören (PFL)";

type Stream = Box<dyn std::any::Any>;

/// Tasten, die schon etwas anderes tun und keinem Cart gehören können.
const RESERVED_KEYS: [egui::Key; 2] = [egui::Key::M, egui::Key::Space];

/// Die laufende Sendung samt Gerät.
pub struct RadioDesk {
    radio_path: PathBuf,
    data_dir: PathBuf,
    station: Option<Station>,
    /// Geöffnete Ausgabeströme (Hauptgerät und weitere Geräte).
    outputs: crate::outputs::Opened,
    /// Meldungen aus dem Öffnen, die in den Einstellungen stehen bleiben.
    output_warnings: Vec<String>,
    mic: Option<Stream>,
    rate: u32,
    error: Option<String>,
    view: View,
    /// QR-Code der zuletzt gezeigten Einladung (Paket 38).
    invite_qr: Option<(String, woa_remote::qr::QrMatrix)>,
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
    /// Pegel Musik, Carts, Mikro in dB (Paket 14); nicht gespeichert, Start bei 0 dB.
    levels_db: [f64; 3],
    /// „MIDI lernen“ per Rechtsklick gewählt; die App holt es ab.
    pub learn: Option<Target>,
    /// Fläche der Playlist und der Cart-Pads im letzten Bild (Ziel für Ziehen aus der Bibliothek).
    playlist_rect: egui::Rect,
    cart_rects: Vec<egui::Rect>,
    /// Flächen der Decks A bis C (Ziel für gezogene Dateien).
    deck_rects: [egui::Rect; woa_radio::DECKS],
    /// Cart, das die nächste gedrückte Taste als neue Taste bekommt.
    key_capture: Option<usize>,
    /// Ortszeit für die Uhrzeit-Anzeige (von der Stundenuhr).
    zone: Zone,
    /// Offener Cue-Editor (Paket 15).
    cue_editor: Option<CueEditor>,
    /// Offener Voice-Track-Editor (Paket 30).
    voice_editor: Option<VoiceEditor>,
    /// Zuletzt in den Editor übernommene Cue-Punkte je Titel (erkennt Undo/Redo).
    cue_seen: std::collections::HashMap<usize, woa_radio::Cues>,
    /// Eigener Strom für das Vorhören auf einem zweiten Gerät.
    _pfl_output: Option<Stream>,
}

fn level_bus(b: Bus) -> LevelBus {
    match b {
        Bus::Music => LevelBus::Music,
        Bus::Carts => LevelBus::Carts,
        Bus::Mic => LevelBus::Mic,
    }
}

fn bus_index(b: Bus) -> usize {
    match b {
        Bus::Music => 0,
        Bus::Carts => 1,
        Bus::Mic => 2,
    }
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

/// Uhrzeit `hh:mm:ss` zur Unix-Zeit `unix` in `zone` (zum Ansagen).
pub fn time_of_day(unix: i64, zone: Zone) -> String {
    let l = zone.local(unix);
    format!("{:02}:{:02}:{:02}", l.hour, l.minute, l.second)
}

/// Name der Zifferntaste, die zur Nummer von Cart `slot` passt (1–9, 0 für 10).
fn cart_number_key(slot: usize) -> Option<String> {
    woa_radio::show::default_cart_key(slot).map(str::to_string)
}

/// Egui-Taste zu einem gespeicherten Namen (`None`, wenn es sie nicht gibt oder sie reserviert ist).
fn cart_key_of(name: &str) -> Option<egui::Key> {
    egui::Key::from_name(name).filter(|k| !RESERVED_KEYS.contains(k))
}

impl RadioDesk {
    /// Zone der Uhrzeit-Anzeige: Versatz in Minuten im Winter, EU-Sommerzeit ja/nein.
    pub fn set_zone(&mut self, base_min: i32, eu_dst: bool) {
        self.zone = Zone { base_min, eu_dst };
    }

    /// Lädt `radio.json` und öffnet die Ausgabe. Geht das Gerät nicht, bleibt die
    /// Playlist bearbeitbar und die Meldung steht oben.
    /// `buffer_frames`: Puffergröße der Ausgabe (`None` = Treiber).
    pub fn open(
        radio_path: PathBuf,
        data_dir: PathBuf,
        buffer_frames: Option<u32>,
        pfl: &PflOutput,
        output_map: &woa_radio::routing::OutputMap,
    ) -> Self {
        let (show, mut error) = match RadioShow::load(&radio_path) {
            Ok(s) => (s, None),
            Err(e) => (RadioShow::default(), Some(e)),
        };
        // Ausgänge je Gerät: Das Gerät der Playlist taktet den Mixer. Vorhören auf Kanal 3/4
        // belegt Paar 1 des Hauptgeräts.
        let reserved: &[usize] = if matches!(pfl, PflOutput::Kanal34) {
            &[1]
        } else {
            &[]
        };
        let plan = crate::outputs::plan(output_map, reserved);
        let config = woa_engine::device::OutputConfig {
            device_id: plan.devices[0].open_id.clone(),
            buffer_frames,
        };
        let rate = plan.devices[0].info.rate;
        let (mut controller, mixer) = Controller::new(show, rate);
        let device = plan.devices[0].info.name.clone();
        let underruns = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&underruns);
        // Vorhören (Paket 15): Kanal 3/4 im selben Callback oder ein eigener Strom.
        // Der PFL-Spieler hat keinen Weg in den Mixer, den Mitschnitt oder den Sende-Ring.
        let (aux, pfl_output) = match pfl {
            PflOutput::Aus => (None, None),
            PflOutput::Kanal34 => match woa_engine::device::output_channels(&config) {
                Ok(n) if n >= 4 => {
                    let (h, mut player) = woa_radio::pfl::channel(rate);
                    controller.set_pfl(Some(h));
                    tracing::info!(channels = n, "pfl on channels 3/4");
                    let f: woa_engine::device::AuxRender =
                        Box::new(move |l, r| player.process(l, r));
                    (Some(f), None)
                }
                Ok(n) => {
                    tracing::warn!(channels = n, "pfl: device has too few channels");
                    error = Some(format!(
                        "Vorhören aus: Das Gerät hat nur {n} Kanäle, Kanal 3/4 gibt es nicht"
                    ));
                    (None, None)
                }
                Err(e) => {
                    tracing::warn!(error = %e, "pfl: channel count");
                    (None, None)
                }
            },
            PflOutput::Geraet(id) => {
                let cfg = woa_engine::device::OutputConfig {
                    device_id: Some(id.clone()),
                    buffer_frames: None,
                };
                let prate = woa_engine::device::output_sample_rate(&cfg).unwrap_or(48_000);
                let (h, mut player) = woa_radio::pfl::channel(prate);
                match woa_engine::device::start_stereo_output(
                    &cfg,
                    move |l, r| player.process(l, r),
                    |e| {
                        if !woa_engine::device::is_xrun(&e) {
                            tracing::warn!(error = %e, "pfl output");
                        }
                    },
                ) {
                    Ok(st) => {
                        tracing::info!(device = %id, rate = prate, "pfl on second device");
                        controller.set_pfl(Some(h));
                        (None, Some(Box::new(st) as Stream))
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, device = %id, "pfl output failed");
                        error = Some(format!("Vorhören aus: Kopfhörer-Gerät geht nicht ({e})"));
                        (None, None)
                    }
                }
            }
        };
        let opened = crate::outputs::open(
            &mut controller,
            mixer,
            &plan,
            buffer_frames,
            aux,
            Arc::clone(&counter),
        );
        let output_warnings = opened.warnings.clone();
        if !output_warnings.is_empty() {
            let all = output_warnings.join("\n");
            error = Some(match error {
                Some(e) => format!("{e}\n{all}"),
                None => all,
            });
        }
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
            outputs: opened,
            output_warnings,
            mic: None,
            rate,
            error,
            view: View::default(),
            invite_qr: None,
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
            levels_db: [0.0; 3],
            learn: None,
            playlist_rect: egui::Rect::NOTHING,
            cart_rects: Vec::new(),
            deck_rects: [egui::Rect::NOTHING; woa_radio::DECKS],
            key_capture: None,
            zone: Zone::CENTRAL_EUROPE,
            cue_editor: None,
            voice_editor: None,
            cue_seen: std::collections::HashMap::new(),
            _pfl_output: pfl_output,
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
        for m in self.outputs.take_new_faults() {
            tracing::warn!(message = %m, "output lost");
            self.error = Some(match self.error.take() {
                Some(e) => format!("{e}\n{m}"),
                None => m,
            });
        }
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

    /// Letzter Schnappschuss der Sendung (für den AutoDJ).
    pub fn view(&self) -> &View {
        &self.view
    }

    /// Reicht die Tabelle der Zeit-Events an den Sende-Thread (gleiche Tabelle: keine Wirkung).
    pub fn push_events(&self, table: woa_radio::events::EventTable) {
        if let Some(s) = &self.station {
            s.with(move |c| c.set_events(table));
        }
    }

    /// Hängt Dateien hinten an die Playlist (AutoDJ); vorhandene Einträge bleiben stehen.
    pub fn append_files(&self, paths: Vec<PathBuf>) {
        if let Some(s) = &self.station {
            s.add_files(paths);
        }
    }

    /// Meldungen aus dem Öffnen der Ausgänge (fehlende Geräte, Ersatz).
    pub fn output_warnings(&self) -> &[String] {
        &self.output_warnings
    }

    /// Zustand der Ringe zu den weiteren Geräten, eine Zeile je Ring.
    pub fn output_links(&self) -> Vec<String> {
        self.outputs.link_lines()
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

    /// Pegel eines Zweigs (dB).
    pub fn level_db(&self, b: Bus) -> f64 {
        self.levels_db[bus_index(b)]
    }

    /// Pegel eines Zweigs setzen (Fader am Controller oder Regler im Fenster).
    pub fn set_level(&mut self, b: Bus, db: f64) {
        let db = db.clamp(
            woa_radio::mixer::LEVEL_MIN_DB,
            woa_radio::mixer::LEVEL_MAX_DB,
        );
        if self.levels_db[bus_index(b)] == db {
            return;
        }
        self.levels_db[bus_index(b)] = db;
        let bus = level_bus(b);
        self.with(move |c| c.set_level(bus, db));
    }

    /// Taster/Pad vom Controller.
    pub fn press(&mut self, t: Target) {
        match t {
            Target::Mic => self.toggle_mic(),
            Target::StartNext => self.with(|c| c.start_or_next()),
            Target::Stop => self.with(|c| c.stop()),
            Target::Cart(slot) if slot < CARTS => self.with(move |c| c.trigger_cart(slot)),
            _ => {}
        }
    }

    /// Wirkung eines Party-Befehls auf die Sendung (Paket 35). Das Vollbild macht die App.
    pub fn party_effect(&mut self, e: woa_radio::party::Effect) {
        use woa_radio::party::Effect;
        match e {
            Effect::StartOrNext => self.with(|c| c.start_or_next()),
            Effect::Stop => self.with(|c| c.stop()),
            Effect::MusicLevel(db) => self.set_level(Bus::Music, db),
            Effect::InsertNext(path) => self.with(move |c| {
                // Hinter den laufenden Titel, sonst hinter den Titel, der als Nächstes startet.
                let at = match c.current_index() {
                    Some(i) => i + 1,
                    None if c.show().playlist.is_empty() => 0,
                    None => c.cue_index() + 1,
                };
                c.add(&[path], Some(at));
            }),
            Effect::Fullscreen(_) => {}
        }
    }

    /// LED-Zustand eines Ziels.
    pub fn lit(&self, t: &Target) -> bool {
        match t {
            Target::Mic => self.view.mic_open,
            Target::StartNext => self.view.running,
            Target::Cart(slot) => *slot < 32 && self.view.carts_playing & (1 << slot) != 0,
            _ => false,
        }
    }

    fn learn_menu(&mut self, r: &egui::Response, t: Target) {
        r.context_menu(|ui| {
            if let Some(t) = crate::controls::learn_menu(ui, t, None) {
                self.learn = Some(t);
            }
        });
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
        // Neue Taste für ein Cart: die nächste Taste gilt (Esc bricht ab, Entf nimmt die Taste weg).
        if let Some(slot) = self.key_capture {
            let pressed = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        repeat: false,
                        ..
                    } => Some(*key),
                    _ => None,
                })
            });
            if let Some(key) = pressed {
                self.key_capture = None;
                match key {
                    egui::Key::Escape => {}
                    egui::Key::Delete | egui::Key::Backspace => {
                        self.with(move |c| {
                            let _ = c.set_cart_key(slot, None);
                        });
                    }
                    k if RESERVED_KEYS.contains(&k) => {
                        self.error = Some(format!(
                            "Die Taste {} ist belegt (M: Mikro, Leertaste: Start/Weiter)",
                            k.name()
                        ));
                    }
                    k => {
                        let name = k.name();
                        self.with(move |c| {
                            if let Err(e) = c.set_cart_key(slot, Some(name)) {
                                c.report(e);
                            }
                        });
                    }
                }
            }
            ctx.input_mut(|i| {
                i.events
                    .retain(|e| !matches!(e, egui::Event::Key { .. } | egui::Event::Text(_)));
            });
            return;
        }
        let keys = self.view.cart_keys.clone();
        let (mic, go, carts) = ctx.input_mut(|i| {
            let carts: Vec<usize> = keys
                .iter()
                .enumerate()
                .filter_map(|(slot, k)| Some((slot, cart_key_of(k.as_deref()?)?)))
                .filter(|(_, k)| i.consume_key(n, *k))
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
        if !dropped.is_empty() {
            let pos = ctx.input(|i| i.pointer.latest_pos());
            let deck = pos.and_then(|p| self.deck_rects.iter().position(|r| r.contains(p)));
            if let Some(k) = deck {
                if let Some(path) = dropped.into_iter().next() {
                    self.deck_load_file(k, path);
                }
            } else if let Some(s) = &self.station {
                s.add_files(dropped);
            }
        }
    }

    /// Zeichnet das ganze Layout in `ui`.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let v = self.view.clone();
        if let Some(e) = self.error.clone() {
            egui::Frame::new()
                .fill(theme::mix(theme::ON_AIR, theme::WELL, 0.78))
                .stroke(egui::Stroke::new(1.0, theme::ON_AIR))
                .corner_radius(5)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("⚠").strong().color(theme::ON_AIR));
                        ui.colored_label(theme::TEXT_BRIGHT, e);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("✕").clicked() {
                                self.error = None;
                            }
                        });
                    });
                });
            ui.add_space(6.0);
        }
        for m in &v.messages {
            ui.label(egui::RichText::new(m).small().color(theme::DIM));
        }
        ui.columns(2, |cols| {
            cols[0].spacing_mut().item_spacing.y = 10.0;
            cols[1].spacing_mut().item_spacing.y = 10.0;
            theme::card_frame().show(&mut cols[0], |ui| {
                self.clock_and_controls(ui, &v);
            });
            self.playlist_rect = theme::card_frame()
                .show(&mut cols[0], |ui| {
                    self.playlist(ui, &v);
                })
                .response
                .rect;
            theme::card_frame().show(&mut cols[1], |ui| {
                self.deck_card(ui, &v);
            });
            theme::card_frame().show(&mut cols[1], |ui| {
                self.cart_wall(ui, &v);
            });
            theme::card_frame().show(&mut cols[1], |ui| {
                self.processing_rack(ui);
            });
            theme::card_frame().show(&mut cols[1], |ui| {
                self.guests_card(ui);
            });
        });
    }

    fn clock_and_controls(&mut self, ui: &mut egui::Ui, v: &View) {
        let idx = v.current.or((!v.running).then_some(v.cue));
        let entry = idx.and_then(|i| v.playlist.get(i));
        let title = v
            .current
            .and_then(|i| v.playlist.get(i))
            .map(|e| e.title.clone())
            .unwrap_or_else(|| {
                entry
                    .map(|e| format!("Bereit: {}", e.title))
                    .unwrap_or_else(|| "– Kein Titel geladen –".to_string())
            });
        let total_title_secs = entry.and_then(|e| e.seconds).unwrap_or(0.0);
        // Cover des laufenden Titels (vor dem Start: des Titels, der als Nächstes kommt).
        let cover_path = entry.map(|e| e.path.clone());
        let ctx = ui.ctx().clone();
        let covers = self
            .covers
            .get_or_insert_with(|| crate::covers::Covers::new(&ctx));
        covers.poll(&ctx);
        let (now, before, t) = covers.big(&ctx, cover_path.as_deref());

        // Modul-Kopfzeile: Status-Badges links, Sendestatus rechts.
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("SENDE-DECK & MASTER-UHR")
                    .size(11.5)
                    .strong()
                    .color(theme::DIM),
            );
            if v.running {
                theme::status_pill(ui, "DECK LÄUFT", theme::OK_GREEN, true);
            } else {
                theme::status_pill(ui, "BEREIT / CUE", theme::ACCENT, false);
            }
            if v.mic_open {
                theme::status_pill(ui, "MIKRO OFFEN", theme::ON_AIR, true);
            }
            if v.recording.is_some() {
                theme::status_pill(ui, "REC LÄUFT", theme::ON_AIR, true);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let n = self.underruns();
                if n > 0 {
                    ui.label(
                        egui::RichText::new(format!("Aussetzer: {n}"))
                            .small()
                            .color(theme::ON_AIR),
                    )
                    .on_hover_text(
                        "Puffer-Unterläufe der Ausgabe seit dem Öffnen. Steigt die Zahl, \
                         in config.json radio_buffer_frames setzen, etwa 4096 (Vorgabe: Treiber).",
                    );
                }
            });
        });
        ui.add_space(4.0);

        // Haupt-Display: Cover links, tiefschwarzes LCD-Cockpit rechts.
        ui.horizontal(|ui| {
            egui::Frame::new()
                .fill(theme::WELL)
                .stroke(egui::Stroke::new(1.0, theme::BORDER))
                .corner_radius(6)
                .inner_margin(egui::Margin::same(3))
                .show(ui, |ui| {
                    crate::covers::paint_square(ui, 152.0, now.as_ref(), before.as_ref(), t)
                        .on_hover_text("Cover des Titels (aus der Datei)");
                });

            theme::lcd_frame().show(ui, |ui| {
                ui.set_min_height(142.0);
                ui.set_width(ui.available_width());
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        let badge_text = if v.running {
                            "AKTUELL ON AIR"
                        } else {
                            "NÄCHSTER START (CUE)"
                        };
                        let badge_col = if v.running {
                            theme::ACCENT
                        } else {
                            theme::CUE_CYAN
                        };
                        ui.label(
                            egui::RichText::new(badge_text)
                                .size(10.5)
                                .strong()
                                .monospace()
                                .color(badge_col),
                        );
                        if let Some(lufs) = entry.and_then(|e| e.lufs) {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{lufs:.1} LUFS"))
                                            .size(11.0)
                                            .monospace()
                                            .color(theme::DIM),
                                    );
                                },
                            );
                        }
                    });
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(title)
                                .size(17.5)
                                .strong()
                                .color(theme::TEXT_BRIGHT),
                        )
                        .truncate(),
                    );
                    ui.add_space(2.0);

                    // Uhrzeit zum Ansagen, groß und ruhig.
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(250));
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(time_of_day(
                                crate::clock_editor::now_unix(),
                                self.zone,
                            ))
                            .size(40.0)
                            .strong()
                            .monospace()
                            .color(theme::TEXT_BRIGHT),
                        )
                        .on_hover_text("Uhrzeit (Ortszeit der Stundenuhr)");
                        ui.label(
                            egui::RichText::new("UHR")
                                .size(11.0)
                                .strong()
                                .color(theme::DIM),
                        );
                    });

                    let urgent = v.running && v.remaining_title < 10.0;
                    let clock_col = if urgent { theme::ON_AIR } else { theme::ACCENT };
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(clock(v.remaining_title))
                                .size(54.0)
                                .strong()
                                .monospace()
                                .color(clock_col),
                        )
                        .on_hover_text("Restzeit des laufenden Titels (bis Cue-Out)");
                        if let Some(r) = v.ramp_remaining {
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new(format!("RAMP {:.0}", r.ceil()))
                                    .size(28.0)
                                    .strong()
                                    .monospace()
                                    .color(theme::ACCENT),
                            )
                            .on_hover_text(
                                "Sekunden bis zum Intro-Ende (Ramp): bis dahin sprechen",
                            );
                        }

                        ui.add_space(10.0);
                        ui.vertical(|ui| {
                            ui.add_space(10.0);
                            ui.label(
                                egui::RichText::new("bis Playlist-Ende")
                                    .size(11.0)
                                    .color(theme::DIM),
                            );
                            ui.label(
                                egui::RichText::new(clock(v.remaining_total))
                                    .size(24.0)
                                    .strong()
                                    .monospace()
                                    .color(theme::TEXT_BRIGHT),
                            );
                        });
                    });

                    if let Some((name, secs, hard)) = &v.next_event {
                        let near = *hard && *secs < 30.0;
                        ui.label(
                            egui::RichText::new(format!(
                                "NÄCHSTES EVENT  {name}  in {}  ({})",
                                clock(*secs),
                                if *hard { "hart" } else { "weich" }
                            ))
                            .size(12.0)
                            .monospace()
                            .color(if near {
                                theme::ON_AIR
                            } else {
                                theme::DIM
                            }),
                        )
                        .on_hover_text("Zeit-Events: Fenster „Zeit-Events“");
                    }
                    if let Some((name, dev)) = &v.last_event {
                        ui.label(
                            egui::RichText::new(format!(
                                "ZULETZT  {name}  {dev:+.1} s gegen die Zeit"
                            ))
                            .size(11.0)
                            .monospace()
                            .color(theme::DIM),
                        );
                    }
                    // Visueller Restzeit-Balken im LCD
                    let progress = if total_title_secs > 0.1 {
                        ((total_title_secs - v.remaining_title) / total_title_secs).clamp(0.0, 1.0)
                            as f32
                    } else if v.running {
                        0.5
                    } else {
                        0.0
                    };
                    let (bar_rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 6.0),
                        egui::Sense::hover(),
                    );
                    ui.painter().rect_filled(bar_rect, 3.0, theme::PANEL);
                    if progress > 0.0 {
                        let mut fill_r = bar_rect;
                        fill_r.set_width(bar_rect.width() * progress);
                        ui.painter().rect_filled(fill_r, 3.0, clock_col);
                    }
                });
            });
        });

        ui.add_space(6.0);

        // Hardware-Tastenreihe (Broadcast-Konsole)
        ui.horizontal_wrapped(|ui| {
            let big = |t: &str| egui::RichText::new(t).size(15.0).strong();
            let go = if v.running { "▶ Weiter" } else { "▶ Start" };
            let go_btn = egui::Button::new(big(go).color(if v.running {
                theme::WELL
            } else {
                theme::TEXT_BRIGHT
            }))
            .fill(if v.running {
                theme::OK_GREEN
            } else {
                theme::mix(theme::OK_GREEN, theme::SURFACE, 0.72)
            })
            .stroke(egui::Stroke::new(1.0, theme::OK_GREEN))
            .min_size(egui::vec2(104.0, 38.0));
            let r = ui.add(go_btn).on_hover_text("Leertaste");
            if r.clicked() {
                self.with(|c| c.start_or_next());
            }
            self.learn_menu(&r, Target::StartNext);

            let r = ui.add(
                egui::Button::new(big("■ Stopp"))
                    .fill(theme::SURFACE)
                    .min_size(egui::vec2(88.0, 38.0)),
            );
            if r.clicked() {
                self.with(|c| c.stop());
            }
            self.learn_menu(&r, Target::Stop);

            let mic = egui::Button::new(big("🎙 Mikro (M)").color(if v.mic_open {
                egui::Color32::WHITE
            } else {
                theme::TEXT_BRIGHT
            }))
            .fill(if v.mic_open {
                theme::ON_AIR
            } else {
                theme::SURFACE
            })
            .stroke(egui::Stroke::new(
                1.0,
                if v.mic_open {
                    theme::mix(theme::ON_AIR, egui::Color32::WHITE, 0.35)
                } else {
                    theme::BORDER
                },
            ))
            .min_size(egui::vec2(108.0, 38.0));
            let r = ui.add(mic).on_hover_text("Taste M");
            if r.clicked() {
                self.toggle_mic();
            }
            self.learn_menu(&r, Target::Mic);

            let rec_on = v.recording.is_some();
            let rec = if rec_on {
                "● Mitschnitt läuft"
            } else {
                "● Mitschnitt"
            };
            if ui
                .add(
                    egui::Button::new(big(rec).color(if rec_on {
                        egui::Color32::WHITE
                    } else {
                        theme::TEXT_BRIGHT
                    }))
                    .fill(if rec_on {
                        theme::ON_AIR
                    } else {
                        theme::SURFACE
                    })
                    .stroke(egui::Stroke::new(
                        1.0,
                        if rec_on { theme::ON_AIR } else { theme::BORDER },
                    ))
                    .min_size(egui::vec2(126.0, 38.0)),
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

        ui.add_space(4.0);
        self.on_air_row(ui);
    }

    /// 19-Zoll-Broadcast-Processing- & Übergangs-Rack (rechts unter der Cart-Wall).
    fn processing_rack(&mut self, ui: &mut egui::Ui) {
        let v = self.view.clone();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("BROADCAST-PROCESSING & ÜBERGÄNGE")
                    .size(11.5)
                    .strong()
                    .color(theme::DIM),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                theme::status_pill(
                    ui,
                    &format!("ZIEL {:.0} LUFS", v.target_lufs),
                    theme::ACCENT,
                    v.bus_processing,
                );
            });
        });
        ui.add_space(4.0);

        // Regler für Crossfade und Ducking in einem eingefassten Panel
        theme::lcd_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut xf = v.crossfade_seconds;
            if ui
                .add(
                    egui::Slider::new(&mut xf, 0.0..=10.0)
                        .suffix(" s")
                        .text("Crossfade-Blende"),
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
                        .text("Musik-Ducking (bei Mikro)"),
                )
                .changed()
            {
                self.view.duck_db = duck;
                self.with(move |c| c.set_duck(duck));
            }
        });
        for b in Bus::ALL {
            let mut db = self.level_db(b);
            let r = ui
                .add(
                    egui::Slider::new(
                        &mut db,
                        woa_radio::mixer::LEVEL_MIN_DB..=woa_radio::mixer::LEVEL_MAX_DB,
                    )
                    .suffix(" dB")
                    .fixed_decimals(1)
                    .text(format!("Pegel {}", b.label())),
                )
                .on_hover_text("Rechtsklick: MIDI lernen. Ganz links = aus.");
            if r.changed() {
                self.set_level(b, db);
            }
            if r.double_clicked() {
                self.set_level(b, 0.0);
            }
            self.learn_menu(&r, Target::Level(b));
        }

        ui.add_space(4.0);
        self.processing_row(ui);
    }

    /// Sendebus- und Mikro-Processing: Schalter und digitale Messanzeige.
    fn processing_row(&mut self, ui: &mut egui::Ui) {
        let v = &self.view;
        let (mut bus, mut mic) = (v.bus_processing, v.mic_processing);
        let mut changed = false;
        theme::lcd_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                changed |= ui
                    .checkbox(&mut bus, egui::RichText::new("Sendebus").strong())
                    .on_hover_text(format!(
                        "Lautheitsregelung auf {:.0} LUFS (±12 dB, langsam), Kompressor und \
                         Limiter bei −1 dBFS. Wirkt auf Sendung, Mitschnitt und Abhören.",
                        v.target_lufs
                    ))
                    .changed();
                changed |= ui
                    .checkbox(&mut mic, egui::RichText::new("Mikro-Kette").strong())
                    .on_hover_text(
                        "Hochpass 80 Hz, Präsenz +3 dB bei 3 kHz, Gate ab −50 dBFS, \
                         Kompressor 4:1 ab −24 dBFS mit +6 dB.",
                    )
                    .changed();
                if v.mic_processing && v.mic_open {
                    let (t, c, active) = if v.mic_gate_open {
                        ("Gate offen", theme::OK_GREEN, true)
                    } else {
                        ("Gate zu", theme::DIM, false)
                    };
                    theme::status_pill(ui, t, c, active);
                }
            });

            if v.bus_processing {
                ui.add_space(4.0);
                let lufs = if v.bus_lufs > -60.0 {
                    format!("{:.1} LUFS", v.bus_lufs)
                } else {
                    "– LUFS".into()
                };
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "{lufs}  ·  Regelung {:+.1} dB  ·  Begrenzer −{:.1} dB",
                            v.bus_agc_db,
                            v.bus_reduction_db.max(0.0)
                        ))
                        .small()
                        .monospace()
                        .color(theme::ACCENT),
                    )
                    .on_hover_text("Kurzzeit-Lautheit am Sendeausgang, Verstärkung der Regelung, Absenkung durch Kompressor und Limiter");
                });
            }
        });
        if changed {
            self.view.bus_processing = bus;
            self.view.mic_processing = mic;
            self.with(move |c| c.set_processing(bus, mic));
        }
    }

    /// Gast-Kanäle (Paket 37): Testgäste, Fader, Pegel, Rückweg (Mix-Minus) im Kopfhörer.
    fn guests_card(&mut self, ui: &mut egui::Ui) {
        let guests = self.view.guests.clone();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("GÄSTE & MIX-MINUS")
                    .size(11.5)
                    .strong()
                    .color(theme::DIM),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let full = guests.len() >= woa_guests::MAX_GUESTS;
                let file = ui
                    .add_enabled(!full, egui::Button::new("＋ Datei als Gast"))
                    .on_hover_text(
                        "Eine Audiodatei in Schleife als Testgast (kein Netz, kein Mikrofon)",
                    );
                if file.clicked()
                    && let Some(p) = rfd::FileDialog::new()
                        .add_filter(
                            woa_engine::decode::FILTER_NAME,
                            &woa_engine::decode::dialog_extensions(),
                        )
                        .pick_file()
                {
                    self.add_guest(woa_guests::TestSignal::File(p));
                }
                let sine = ui
                    .add_enabled(!full, egui::Button::new("＋ Testgast"))
                    .on_hover_text("Ein Sinuston als Gast, jeder Gast mit eigener Tonhöhe");
                if sine.clicked() {
                    let freq = 330.0 + 110.0 * guests.len() as f32;
                    self.add_guest(woa_guests::TestSignal::Sine { freq });
                }
            });
        });
        ui.add_space(4.0);
        theme::lcd_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            if guests.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "Kein Gast. Jeder Gast bekommt einen Kanal und einen Rückweg ohne die eigene Stimme.",
                    )
                    .small()
                    .color(theme::DIM),
                );
            }
            for g in &guests {
                self.guest_row(ui, g);
                ui.add_space(2.0);
            }
        });
        ui.add_space(6.0);
        self.web_guests(ui);
    }

    /// Browser-Gäste (Paket 38): Zugang an/aus, Einladungen mit QR-Code, Warteraum.
    fn web_guests(&mut self, ui: &mut egui::Ui) {
        let web = self.view.web.clone();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("BROWSER-GÄSTE")
                    .size(11.5)
                    .strong()
                    .color(theme::DIM),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if web.running {
                    if ui
                        .button("Ausschalten")
                        .on_hover_text("Trennt alle Browser-Gäste und schließt die Ports")
                        .clicked()
                    {
                        self.with(|c| c.guests_web_set(false, None));
                    }
                    let full = self.view.guests.len() >= woa_guests::MAX_GUESTS;
                    if ui
                        .add_enabled(!full, egui::Button::new("＋ Einladung"))
                        .on_hover_text("Ein Link für ein Handy im selben WLAN, einmal benutzbar")
                        .clicked()
                    {
                        self.with(|c| {
                            if c.guests().web_invite().is_none() {
                                tracing::warn!("invite not created");
                            }
                        });
                    }
                } else if ui
                    .button("Browser-Zugang einschalten")
                    .on_hover_text(
                        "Öffnet zwei Ports im WLAN (HTTPS und UDP). Gäste brauchen nur einen Browser.",
                    )
                    .clicked()
                {
                    let ip = woa_remote::lan_ip();
                    self.with(move |c| c.guests_web_set(true, ip));
                }
            });
        });
        ui.add_space(4.0);
        theme::lcd_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !web.running {
                ui.label(
                    egui::RichText::new(
                        "Aus. Einschalten, Einladung erzeugen, Link ans Handy schicken: Der Gast \
                         spricht über den Browser, ohne App.",
                    )
                    .small()
                    .color(theme::DIM),
                );
                return;
            }
            for w in &web.waiting {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{} wartet seit {} s", w.name, w.waited_s))
                            .strong()
                            .color(theme::ACCENT),
                    )
                    .on_hover_text(&w.peer);
                    let ticket = w.ticket.clone();
                    if ui
                        .button(egui::RichText::new("Annehmen").color(theme::OK_GREEN))
                        .clicked()
                    {
                        self.with(move |c| {
                            if let Err(e) = c.guests().web_accept(&ticket) {
                                tracing::warn!(error = %e, "guest not accepted");
                            }
                        });
                    }
                    let ticket = w.ticket.clone();
                    if ui.button("Ablehnen").clicked() {
                        self.with(move |c| c.guests().web_reject(&ticket));
                    }
                });
            }
            for (link, inv) in &web.invites {
                if self.invite_qr.as_ref().map(|(l, _)| l) != Some(link) {
                    self.invite_qr = woa_remote::qr::qr(link).map(|m| (link.clone(), m));
                }
                ui.horizontal(|ui| {
                    if let Some((l, m)) = &self.invite_qr
                        && l == link
                    {
                        crate::remote::draw_qr(ui, m, 150.0);
                    }
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "Einladung, noch {} min gültig",
                                inv.left_s.div_ceil(60)
                            ))
                            .strong(),
                        );
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(link)
                                    .small()
                                    .monospace()
                                    .color(theme::CUE_CYAN),
                            )
                            .wrap()
                            .selectable(true),
                        );
                        ui.horizontal(|ui| {
                            if ui.small_button("Link kopieren").clicked() {
                                ui.ctx().copy_text(link.clone());
                            }
                            if ui.small_button("Zurückziehen").clicked() {
                                let token = inv.token.clone();
                                self.with(move |c| c.guests().web_revoke(&token));
                            }
                        });
                        ui.label(
                            egui::RichText::new(
                                "Das Handy zeigt eine Zertifikatswarnung (selbst ausgestellt): \
                                 „Erweitert“, dann „Weiter“.",
                            )
                            .small()
                            .color(theme::DIM),
                        );
                    });
                });
                ui.add_space(4.0);
            }
            if web.invites.is_empty() && web.waiting.is_empty() {
                ui.label(
                    egui::RichText::new("Bereit. „＋ Einladung“ erzeugt einen Link mit QR-Code.")
                        .small()
                        .color(theme::DIM),
                );
            }
        });
    }

    fn add_guest(&mut self, signal: woa_guests::TestSignal) {
        self.with(move |c| {
            if let Err(e) = c.guests().add_test_guest(signal) {
                tracing::warn!(error = %e, "guest not added");
            }
        });
    }

    fn guest_row(&mut self, ui: &mut egui::Ui, g: &woa_guests::GuestView) {
        let slot = g.slot;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(&g.name)
                    .strong()
                    .color(theme::TEXT_BRIGHT),
            )
            .on_hover_text(&g.source);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(90.0, 12.0), egui::Sense::hover());
            theme::led_meter(ui.painter(), rect, g.peak);
            let mut db = g.gain_db;
            if ui
                .add(
                    egui::Slider::new(&mut db, -60.0..=12.0)
                        .suffix(" dB")
                        .fixed_decimals(0),
                )
                .changed()
            {
                if let Some(x) = self.view.guests.iter_mut().find(|x| x.slot == slot) {
                    x.gain_db = db;
                }
                self.with(move |c| c.guests().set_gain_db(slot, db));
            }
            let mute = egui::Button::new(egui::RichText::new("M").strong().color(if g.muted {
                theme::ON_AIR
            } else {
                theme::DIM
            }));
            if ui.add(mute).on_hover_text("Gast stumm").clicked() {
                let on = !g.muted;
                if let Some(x) = self.view.guests.iter_mut().find(|x| x.slot == slot) {
                    x.muted = on;
                }
                self.with(move |c| c.guests().set_mute(slot, on));
            }
            let ret = egui::Button::new(egui::RichText::new("🎧").color(if g.monitored {
                theme::CUE_CYAN
            } else {
                theme::DIM
            }));
            if ui
                .add(ret)
                .on_hover_text(
                    "Rückweg dieses Gastes abhören: Musik, Moderator und alle anderen Gäste, \
                     nur nicht er selbst. Der Abhörausgang trägt dann diesen Rückweg.",
                )
                .clicked()
            {
                let to = (!g.monitored).then_some(slot);
                self.with(move |c| c.guests().set_monitor(to));
            }
            if ui
                .small_button("✕")
                .on_hover_text("Gast entfernen")
                .clicked()
            {
                self.with(move |c| c.guests().remove(slot));
            }
        });
        let mut line = format!("{} · Puffer {:.0} ms", g.source, g.buffered_ms);
        if g.prebuffering {
            line.push_str(" · füllt");
        }
        if let Some(n) = &g.net {
            if n.connected {
                line.push_str(&format!(
                    " · RTT {} ms · Verlust {:.1} %",
                    n.rtt_ms,
                    n.loss_permille as f32 / 10.0
                ));
            } else {
                line.push_str(" · verbindet …");
            }
        }
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(line)
                    .small()
                    .monospace()
                    .color(theme::DIM),
            );
            if g.underruns > 0 || g.overflows > 0 {
                ui.label(
                    egui::RichText::new(format!(
                        "Aussetzer {} · Überlauf {}",
                        g.underruns, g.overflows
                    ))
                    .small()
                    .color(theme::ON_AIR),
                );
            }
        });
    }

    fn on_air_row(&mut self, ui: &mut egui::Ui) {
        let now = woa_stream::unix_now();
        let st = self.stream_status.clone();
        let running = st.as_ref().is_some_and(|s| s.is_running(now));
        let on = self.feeder.is_some();
        theme::lcd_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                let (label, fill, border) = if on {
                    (
                        "📡 Sendung beenden",
                        theme::ON_AIR,
                        theme::mix(theme::ON_AIR, egui::Color32::WHITE, 0.35),
                    )
                } else {
                    (
                        "📡 Auf Sendung",
                        theme::mix(theme::ACCENT, theme::SURFACE, 0.72),
                        theme::ACCENT,
                    )
                };
                let text = egui::RichText::new(label).size(14.5).strong().color(if on {
                    egui::Color32::WHITE
                } else {
                    theme::TEXT_BRIGHT
                });
                if ui
                    .add(
                        egui::Button::new(text)
                            .fill(fill)
                            .stroke(egui::Stroke::new(1.0, border))
                            .min_size(egui::vec2(148.0, 34.0)),
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
                            theme::OK_GREEN
                        };
                        theme::status_pill(ui, &s.summary(), color, true);
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
                    _ => {
                        ui.label(
                            egui::RichText::new("Stream-Encoder bereit (Shoutcast / Icecast)")
                                .small()
                                .color(theme::DIM),
                        );
                    }
                }
            });
            if let Some(p) = st
                .as_ref()
                .filter(|_| running)
                .and_then(StreamStatus::problem)
            {
                ui.label(egui::RichText::new(p).small().color(theme::ON_AIR));
            }
        });
    }

    fn playlist(&mut self, ui: &mut egui::Ui, v: &View) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("SENDE-PLAYLIST")
                    .size(11.5)
                    .strong()
                    .color(theme::DIM),
            );
            ui.label(
                egui::RichText::new(format!("({} Titel)", v.playlist.len()))
                    .small()
                    .color(theme::DIM),
            );
            if v.measuring > 0 {
                ui.spinner();
                ui.label(
                    egui::RichText::new("misst Lautheit …")
                        .small()
                        .color(theme::ACCENT),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("+ Titel …")
                                .strong()
                                .color(theme::TEXT_BRIGHT),
                        )
                        .fill(theme::mix(theme::ACCENT, theme::SURFACE, 0.72))
                        .stroke(egui::Stroke::new(1.0, theme::ACCENT)),
                    )
                    .clicked()
                {
                    self.add_dialog();
                }
            });
        });
        ui.add_space(4.0);

        if v.playlist.is_empty() {
            theme::lcd_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new(
                            "Audiodateien (MP3, FLAC, WAV, OGG, M4A, AIFF …) hierher ziehen oder „+ Titel …“.",
                        )
                        .color(theme::DIM),
                    );
                    ui.add_space(12.0);
                });
            });
        } else {
            // Spaltenkopf wie in mAirList
            egui::Frame::new()
                .fill(theme::PANEL_HEADER)
                .corner_radius(egui::CornerRadius {
                    nw: 4,
                    ne: 4,
                    sw: 0,
                    se: 0,
                })
                .inner_margin(egui::Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("#").small().strong().color(theme::DIM));
                        ui.add_space(30.0);
                        ui.label(
                            egui::RichText::new("TITEL & LAUTHEIT")
                                .small()
                                .strong()
                                .color(theme::DIM),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new("AKTION  ·  DAUER")
                                    .small()
                                    .strong()
                                    .color(theme::DIM),
                            );
                        });
                    });
                });
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            for (i, e) in v.playlist.iter().enumerate() {
                let on_air = v.current == Some(i);
                let is_cue = i == v.cue && !v.running;
                let row_fill = if on_air {
                    theme::mix(theme::ON_AIR, theme::PANEL, 0.72)
                } else if is_cue {
                    theme::mix(theme::CUE_CYAN, theme::PANEL, 0.82)
                } else if i % 2 == 1 {
                    theme::mix(theme::WELL, theme::PANEL, 0.65)
                } else {
                    theme::WELL
                };
                let stroke = if on_air {
                    egui::Stroke::new(1.0, theme::ON_AIR)
                } else if is_cue {
                    egui::Stroke::new(1.0, theme::CUE_CYAN)
                } else {
                    egui::Stroke::new(1.0, theme::mix(theme::BORDER, theme::WELL, 0.5))
                };
                let frame = egui::Frame::new()
                    .fill(row_fill)
                    .stroke(stroke)
                    .corner_radius(4)
                    .inner_margin(egui::Margin::symmetric(8, 5));
                let resp = frame.show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("{:>2}", i + 1))
                                .monospace()
                                .strong()
                                .color(if on_air {
                                    theme::ON_AIR
                                } else if is_cue {
                                    theme::CUE_CYAN
                                } else {
                                    theme::DIM
                                }),
                        );
                        let thumb = self.covers.as_mut().and_then(|c| c.thumb(&e.path));
                        crate::covers::paint_square(ui, 34.0, thumb.as_ref(), None, 1.0);
                        if let Some(vb) = e.voice {
                            ui.label(
                                egui::RichText::new("VOICE")
                                    .small()
                                    .strong()
                                    .color(theme::ACCENT),
                            );
                            if vb.check {
                                ui.label(
                                    egui::RichText::new("prüfen")
                                        .small()
                                        .strong()
                                        .color(theme::ON_AIR),
                                )
                                .on_hover_text(
                                    "Ein Nachbar hat sich geändert: der Voice-Track geht erst \
                                     nach der Bestätigung im Editor auf Sendung",
                                );
                            }
                        }

                        // Rechte Seite zuerst reservieren (Steuerung + Dauer + LUFS-Badge),
                        // damit der Titel links sauber abschneidet.
                        let lufs_badge = e
                            .lufs
                            .map(|l| format!("{l:.1} LUFS ({:+.1} dB)", e.gain_db));
                        let title_resp = ui
                            .add(
                                egui::Label::new(
                                    egui::RichText::new(&e.title)
                                        .strong()
                                        .color(theme::TEXT_BRIGHT),
                                )
                                .truncate()
                                .sense(egui::Sense::click()),
                            )
                            .on_hover_text(format!(
                                "{}\n{}\nDoppelklick: als Nächstes beim Start",
                                e.path.display(),
                                e.lufs
                                    .map(|l| format!("{l:.1} LUFS, Auto-Gain {:+.1} dB", e.gain_db))
                                    .unwrap_or_else(|| "noch nicht gemessen".into())
                            ));
                        if title_resp.double_clicked() {
                            self.with(move |c| c.set_cue(i));
                        }
                        title_resp.context_menu(|ui| self.title_menu(ui, i, e, v));

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("x").on_hover_text("Entfernen").clicked() {
                                self.with(move |c| {
                                    c.remove(i);
                                });
                            }
                            if ui.small_button("v").on_hover_text("Nach unten").clicked() {
                                self.with(move |c| {
                                    c.move_entry(i, i + 1);
                                });
                            }
                            if ui.small_button("^").on_hover_text("Nach oben").clicked() && i > 0 {
                                self.with(move |c| {
                                    c.move_entry(i, i - 1);
                                });
                            }
                            ui.label(
                                egui::RichText::new(
                                    e.seconds
                                        .map(|s| clock(e.cues.play_seconds(s)))
                                        .unwrap_or_else(|| "--:--".into()),
                                )
                                .monospace()
                                .strong()
                                .color(theme::ACCENT),
                            );
                            if let Some(lb) = lufs_badge {
                                ui.label(
                                    egui::RichText::new(lb)
                                        .size(10.5)
                                        .monospace()
                                        .color(theme::DIM),
                                );
                            }
                        });
                    });
                });
                // Farbiger Indikator-Streifen am linken Zeilenrand bei aktivem oder gecuetem Track
                if on_air || is_cue {
                    let r = resp.response.rect;
                    let stripe = egui::Rect::from_min_size(r.min, egui::vec2(4.0, r.height()));
                    ui.painter().rect_filled(
                        stripe,
                        egui::CornerRadius {
                            nw: 4,
                            sw: 4,
                            ne: 0,
                            se: 0,
                        },
                        if on_air {
                            theme::ON_AIR
                        } else {
                            theme::CUE_CYAN
                        },
                    );
                }
            }
        });
    }

    fn cart_wall(&mut self, ui: &mut egui::Ui, v: &View) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("CART-WALL (JINGLES & SOUNDS)")
                    .size(11.5)
                    .strong()
                    .color(theme::DIM),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(match self.key_capture {
                        Some(n) => format!(
                            "Taste für Cart {} drücken · Esc: abbrechen · Entf: ohne Taste",
                            n + 1
                        ),
                        None => {
                            "Taste startet und stoppt · Rechtsklick: Belegen, Taste, Schnitt".into()
                        }
                    })
                    .small()
                    .color(theme::DIM),
                );
            });
        });
        ui.add_space(4.0);

        let w = ((ui.available_width() - 3.0 * 8.0) / 4.0).max(60.0);
        let mut cut: Option<usize> = None;
        egui::Grid::new("carts")
            .spacing(egui::vec2(8.0, 8.0))
            .show(ui, |ui| {
                for slot in 0..CARTS {
                    let label = v.carts.get(slot).cloned().flatten();
                    let playing = v.carts_playing & (1 << slot) != 0;
                    let pad_color = theme::track_color(slot);
                    // Die große Zahl ist die Nummer des Carts. Die Taste steht klein in der
                    // Ecke, sobald sie nicht die Nummer selbst ist.
                    let number = (slot + 1).to_string();
                    let key = v.cart_keys.get(slot).cloned().flatten();
                    let key_text = match &key {
                        Some(k) if cart_number_key(slot).as_deref() == Some(k.as_str()) => None,
                        Some(k) => Some(k.clone()),
                        None => Some("–".to_string()),
                    };
                    let text = format!(
                        "{number}\n{}",
                        label.clone().unwrap_or_else(|| "leer".into())
                    );
                    let fill = if playing {
                        theme::mix(theme::ACCENT, theme::SURFACE, 0.25)
                    } else if label.is_some() {
                        theme::mix(pad_color, theme::SURFACE, 0.80)
                    } else {
                        theme::WELL
                    };
                    let stroke = if playing {
                        egui::Stroke::new(2.0, theme::ACCENT)
                    } else if label.is_some() {
                        egui::Stroke::new(1.0, theme::mix(pad_color, theme::BORDER, 0.45))
                    } else {
                        egui::Stroke::new(1.0, theme::BORDER)
                    };
                    let b =
                        egui::Button::new(egui::RichText::new(text).strong().color(if playing {
                            theme::WELL
                        } else if label.is_some() {
                            theme::TEXT_BRIGHT
                        } else {
                            theme::DIM
                        }))
                        .fill(fill)
                        .stroke(stroke)
                        .min_size(egui::vec2(w, 68.0));
                    let r = ui.add(b);
                    if self.cart_rects.len() <= slot {
                        self.cart_rects.resize(slot + 1, egui::Rect::NOTHING);
                    }
                    self.cart_rects[slot] = r.rect;

                    // Hardware-Pad-Optik: LED oben rechts + Farb-Streifen unten
                    let pad_rect = r.rect;
                    if let Some(k) = key_text {
                        let capturing = self.key_capture == Some(slot);
                        let font = egui::FontId::monospace(11.0);
                        let text = if capturing { "?".to_string() } else { k };
                        let galley = ui.painter().layout_no_wrap(
                            text,
                            font,
                            if capturing {
                                theme::ACCENT
                            } else {
                                theme::TEXT_BRIGHT
                            },
                        );
                        let tag = egui::Rect::from_min_size(
                            pad_rect.left_top() + egui::vec2(4.0, 4.0),
                            galley.size() + egui::vec2(8.0, 2.0),
                        );
                        ui.painter().rect_filled(tag, 3.0, theme::WELL);
                        ui.painter().galley(
                            tag.min + egui::vec2(4.0, 1.0),
                            galley,
                            theme::TEXT_BRIGHT,
                        );
                    }
                    let led_col = if playing {
                        theme::WELL
                    } else if label.is_some() {
                        pad_color
                    } else {
                        theme::mix(theme::DIM, theme::WELL, 0.6)
                    };
                    ui.painter().circle_filled(
                        egui::pos2(pad_rect.right() - 9.0, pad_rect.top() + 9.0),
                        3.5,
                        led_col,
                    );
                    if label.is_some() {
                        let bottom_bar = egui::Rect::from_min_max(
                            egui::pos2(pad_rect.left() + 3.0, pad_rect.bottom() - 4.0),
                            egui::pos2(pad_rect.right() - 3.0, pad_rect.bottom() - 1.5),
                        );
                        ui.painter().rect_filled(
                            bottom_bar,
                            1.0,
                            if playing { theme::WELL } else { pad_color },
                        );
                    }

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
                        if let Some(t) = crate::controls::learn_menu(ui, Target::Cart(slot), None) {
                            self.learn = Some(t);
                        }
                        if label.is_some()
                            && ui
                                .add_enabled(v.pfl_available, egui::Button::new("Vorhören"))
                                .on_disabled_hover_text(NO_PFL)
                                .clicked()
                        {
                            self.with(move |c| {
                                if let Err(e) = c.pfl_cart(slot) {
                                    c.report(e);
                                }
                            });
                            ui.close();
                        }
                        if ui.button("Taste ändern …").clicked() {
                            self.key_capture = Some(slot);
                            ui.close();
                        }
                        if label.is_some() && ui.button("Start und Ende schneiden …").clicked() {
                            cut = Some(slot);
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
        if let Some(slot) = cut {
            self.open_cart_cue_editor(slot);
        }
    }

    /// Rechtsklick auf einen Titel: Cue-Punkte, Vorhören (Paket 15).
    fn title_menu(&mut self, ui: &mut egui::Ui, i: usize, e: &ViewEntry, v: &View) {
        if e.voice.is_some() {
            if ui.button("Voice-Track bearbeiten …").clicked() {
                self.open_voice_editor(i);
                ui.close();
            }
        } else if ui.button("Voice-Track danach einfügen …").clicked() {
            self.add_voice_dialog(i + 1);
            ui.close();
        }
        if ui.button("Cue-Punkte …").clicked() {
            self.cue_editor = Some(CueEditor::open(
                i,
                e.path.clone(),
                e.title.clone(),
                e.seconds,
                e.cues,
            ));
            ui.close();
        }
        for (label, hook) in [("Vorhören", false), ("Vorhören ab Hook", true)] {
            if hook && e.cues.hook.is_none() {
                continue;
            }
            if ui
                .add_enabled(v.pfl_available, egui::Button::new(label))
                .on_disabled_hover_text(NO_PFL)
                .clicked()
            {
                self.with(move |c| {
                    if let Err(e) = c.pfl_title(i, hook) {
                        c.report(e);
                    }
                });
                ui.close();
            }
        }
        if e.voice.is_none() {
            ui.menu_button("Auf Deck laden", |ui| {
                for k in 0..woa_radio::DECKS {
                    let label = format!("Deck {}", woa_radio::show::deck_name(k));
                    if ui.button(label).clicked() {
                        self.with(move |c| {
                            if let Err(e) = c.deck_load_title(k, i) {
                                c.report(e);
                            }
                        });
                        ui.close();
                    }
                }
            });
        }
        if v.pfl_playing && ui.button("Vorhören anhalten").clicked() {
            self.with(Controller::pfl_stop);
            ui.close();
        }
    }

    /// Wählt eine Sprachdatei und legt sie als Voice-Track bei `at` in die Playlist.
    fn add_voice_dialog(&self, at: usize) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(
                woa_engine::decode::FILTER_NAME,
                &woa_engine::decode::dialog_extensions(),
            )
            .pick_file()
        {
            self.add_voice_file(path, Some(at));
        }
    }

    /// Legt eine Sprachdatei als Voice-Track bei `at` (sonst hinten) in die Playlist.
    pub fn add_voice_file(&self, path: PathBuf, at: Option<usize>) {
        self.with(move |c| {
            if let Err(e) = c.add_voice(&path, at) {
                c.report(e);
            }
        });
    }

    /// Öffnet den Voice-Track-Editor für Eintrag `i` (falls es ein Voice-Track ist).
    pub fn open_voice_editor(&mut self, i: usize) {
        let v = &self.view;
        let Some(e) = v.playlist.get(i) else { return };
        let Some(vb) = e.voice else { return };
        let music = |k: Option<usize>| k.and_then(|k| v.playlist.get(k));
        let a = music((0..i).rev().find(|&k| v.playlist[k].voice.is_none()));
        let b = music((i + 1..v.playlist.len()).find(|&k| v.playlist[k].voice.is_none()));
        let input = VoiceInput {
            index: i,
            title: e.title.clone(),
            voice_path: e.path.clone(),
            voice_seconds: e.seconds,
            a_path: a.map(|x| x.path.clone()),
            a_title: a.map(|x| x.title.clone()).unwrap_or_else(|| "–".into()),
            a_seconds: a.and_then(|x| x.seconds),
            a_cues: a.map(|x| x.cues).unwrap_or_default(),
            b_path: b.map(|x| x.path.clone()),
            b_title: b.map(|x| x.title.clone()).unwrap_or_else(|| "–".into()),
            b_seconds: b.and_then(|x| x.seconds),
            b_cues: b.map(|x| x.cues).unwrap_or_default(),
            crossfade: v.crossfade_seconds,
            params: vb.params,
            check: vb.check,
        };
        self.voice_editor = Some(VoiceEditor::open(input));
    }

    /// Zeichnet den Voice-Track-Editor, falls offen, und reicht seine Aufträge weiter.
    pub fn voice_window(&mut self, ctx: &egui::Context) {
        let Some(ed) = self.voice_editor.as_mut() else {
            return;
        };
        let v = &self.view;
        // Eintrag verschoben oder entfernt: Editor schließen statt falschen zu ändern.
        let Some(cur) = v
            .playlist
            .get(ed.input.index)
            .filter(|e| e.path == ed.input.voice_path && e.voice.is_some())
        else {
            self.voice_editor = None;
            return;
        };
        if let Some(vb) = cur.voice {
            ed.sync(vb.params, vb.check);
        }
        let mut open = true;
        let actions = ed.ui(ctx, &mut open, (v.pfl_available, v.pfl_playing));
        if !open {
            self.voice_editor = None;
        }
        for a in actions {
            match a {
                VoiceAction::Apply(i, p) => self.with(move |c| {
                    if let Err(e) = c.set_voice(i, p) {
                        c.report(e);
                    }
                }),
                VoiceAction::Confirm(i) => self.with(move |c| {
                    if let Err(e) = c.confirm_voice(i) {
                        c.report(e);
                    }
                }),
                VoiceAction::Preview(i) => self.with(move |c| {
                    if let Err(e) = c.pfl_voice(i) {
                        c.report(e);
                    }
                }),
                VoiceAction::StopPreview => self.with(Controller::pfl_stop),
            }
        }
    }

    /// Öffnet den Cue-Editor für Titel `i` (falls es ihn gibt).
    pub fn open_cue_editor(&mut self, i: usize) {
        if let Some(e) = self.view.playlist.get(i) {
            self.cue_editor = Some(CueEditor::open(
                i,
                e.path.clone(),
                e.title.clone(),
                e.seconds,
                e.cues,
            ));
        }
    }

    /// Öffnet den Schnitt (Start und Ende) für Cart `slot`.
    pub fn open_cart_cue_editor(&mut self, slot: usize) {
        let path = self.view.cart_paths.get(slot).cloned().flatten();
        if let (Some(path), Some(Some(title))) = (path, self.view.carts.get(slot)) {
            let cues = self.view.cart_cues.get(slot).copied().unwrap_or_default();
            let mut ed = CueEditor::open(slot, path, title.clone(), None, cues);
            ed.cart = true;
            self.cue_editor = Some(ed);
        }
    }

    /// Zeichnet den Cue-Editor, falls offen, und reicht seine Aufträge weiter.
    pub fn cue_window(&mut self, ctx: &egui::Context) {
        let Some(ed) = self.cue_editor.as_mut() else {
            return;
        };
        let v = &self.view;
        // Titel verschoben oder entfernt: Editor schließen statt falschen Titel zu ändern.
        let gone = if ed.deck {
            v.decks.get(ed.index).is_none_or(|d| d.title.is_none())
        } else if ed.cart {
            v.carts.get(ed.index).is_none_or(Option::is_none)
        } else {
            v.playlist.get(ed.index).map(|e| &e.path) != Some(&ed.path)
        };
        if gone {
            self.cue_editor = None;
            return;
        }
        let mut open = true;
        let actions = ed.ui(
            ctx,
            &mut open,
            (v.pfl_available, v.pfl_playing),
            (v.cues_undo && !ed.cart, v.cues_redo && !ed.cart),
        );
        let cart = ed.cart;
        let deck = ed.deck;
        if !open {
            self.cue_editor = None;
        }
        for a in actions {
            match a {
                CueAction::Apply(i, cues) => self.with(move |c| {
                    let r = if deck {
                        c.set_deck_cues(i, cues)
                    } else if cart {
                        c.set_cart_cues(i, cues)
                    } else {
                        c.set_cues(i, cues)
                    };
                    if let Err(e) = r {
                        c.report(e);
                    }
                }),
                CueAction::Preview(i, from) => self.with(move |c| {
                    let r = if deck {
                        c.pfl_deck_at(i, from)
                    } else if cart {
                        c.pfl_cart_at(i, from)
                    } else {
                        c.pfl_title_at(i, from)
                    };
                    if let Err(e) = r {
                        c.report(e);
                    }
                }),
                CueAction::StopPreview => self.with(Controller::pfl_stop),
                CueAction::Undo => self.with(|c| {
                    c.undo_cues();
                }),
                CueAction::Redo => self.with(|c| {
                    c.redo_cues();
                }),
            }
        }
        // Nach Undo/Redo zeigt der Editor die Punkte aus der Playlist.
        if let Some(ed) = self.cue_editor.as_mut()
            && let Some(e) = self.view.playlist.get(ed.index)
            && self.cue_seen.get(&ed.index) != Some(&e.cues)
        {
            ed.draft = e.cues;
            self.cue_seen.insert(ed.index, e.cues);
        }
    }

    /// Ob das Vorhören (PFL) einen Ausgang hat.
    pub fn pfl_available(&self) -> bool {
        self.view.pfl_available
    }

    /// Hört eine Datei der Bibliothek im Kopfhörer vor (nie auf Sendung, derselbe PFL-Weg).
    pub fn preview_file(&self, path: PathBuf) {
        self.with(move |c| {
            if let Err(e) = c.pfl_file(&path) {
                c.report(e);
            }
        });
    }

    pub fn preview_stop(&self) {
        self.with(Controller::pfl_stop);
    }

    /// Eine aus der Bibliothek gezogene Datei bei `pos` ablegen: auf einem Cart-Pad wird sie
    /// dem Pad zugewiesen, auf der Playlist hinten angehängt. Gibt zurück, ob etwas geschah.
    pub fn drop_library_path(&mut self, path: PathBuf, pos: egui::Pos2) -> bool {
        if let Some(k) = self.deck_rects.iter().position(|r| r.contains(pos)) {
            self.deck_load_file(k, path);
            return true;
        }
        if let Some(slot) = self.cart_rects.iter().position(|r| r.contains(pos)) {
            self.with(move |c| {
                if let Err(e) = c.assign_cart(slot, Some(path)) {
                    c.report(e);
                }
            });
            return true;
        }
        if self.playlist_rect.contains(pos)
            && let Some(s) = &self.station
        {
            s.add_files(vec![path]);
            return true;
        }
        false
    }

    /// Legt eine Datei auf Deck `k` (steht danach am Cue-In).
    fn deck_load_file(&self, k: usize, path: PathBuf) {
        self.with(move |c| {
            if let Err(e) = c.deck_load(k, path, woa_radio::Cues::default()) {
                c.report(e);
            }
        });
    }

    fn pick_deck_file(&self, k: usize) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(
                woa_engine::decode::FILTER_NAME,
                &woa_engine::decode::dialog_extensions(),
            )
            .pick_file()
        {
            self.deck_load_file(k, path);
        }
    }

    /// Öffnet den Schnitt (Start und Ende) für Deck `k`.
    fn open_deck_cue_editor(&mut self, k: usize) {
        if let Some(d) = self.view.decks.get(k)
            && let (Some(path), Some(title)) = (d.path.clone(), d.title.clone())
        {
            let mut ed = CueEditor::open(k, path, title, None, d.cues);
            ed.cart = true;
            ed.deck = true;
            self.cue_editor = Some(ed);
        }
    }

    /// Die drei Decks: große Restzeit, Start und Stopp, Laden per Ziehen oder Menü.
    fn deck_card(&mut self, ui: &mut egui::Ui, v: &View) {
        ui.label(
            egui::RichText::new("DECKS A · B · C")
                .size(11.5)
                .strong()
                .color(theme::DIM),
        );
        ui.add_space(4.0);
        for k in 0..woa_radio::DECKS {
            let d = v.decks.get(k).cloned().unwrap_or_default();
            let name = woa_radio::show::deck_name(k);
            let loaded = d.title.is_some();
            let urgent = d.playing && d.remaining < 10.0;
            let fill = if d.playing {
                theme::mix(theme::ACCENT, theme::SURFACE, 0.2)
            } else {
                theme::WELL
            };
            let frame = egui::Frame::new()
                .fill(fill)
                .stroke(egui::Stroke::new(
                    if d.playing { 2.0 } else { 1.0 },
                    if d.playing {
                        theme::ACCENT
                    } else {
                        theme::BORDER
                    },
                ))
                .corner_radius(5)
                .inner_margin(egui::Margin::symmetric(8, 5));
            let mut title_r: Option<egui::Response> = None;
            let out = frame.show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(name.to_string())
                            .size(26.0)
                            .strong()
                            .color(theme::TEXT_BRIGHT),
                    );
                    ui.vertical(|ui| {
                        ui.set_min_width(110.0);
                        ui.label(
                            egui::RichText::new(if loaded {
                                clock(d.remaining)
                            } else {
                                "--:--".into()
                            })
                            .size(30.0)
                            .monospace()
                            .strong()
                            .color(if urgent {
                                theme::ON_AIR
                            } else {
                                theme::TEXT_BRIGHT
                            }),
                        );
                        let frac = if d.length > 0.0 {
                            (1.0 - d.remaining / d.length).clamp(0.0, 1.0) as f32
                        } else {
                            0.0
                        };
                        ui.add(egui::ProgressBar::new(frac).desired_height(4.0));
                    });
                    ui.vertical(|ui| {
                        title_r = Some(
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(
                                        d.title
                                            .clone()
                                            .unwrap_or_else(|| "leer: Datei hierher ziehen".into()),
                                    )
                                    .small()
                                    .color(if loaded {
                                        theme::TEXT_BRIGHT
                                    } else {
                                        theme::DIM
                                    }),
                                )
                                .sense(egui::Sense::click()),
                            ),
                        );
                        ui.horizontal(|ui| {
                            let go = if d.playing { "Stopp" } else { "Start" };
                            if ui
                                .add_enabled(
                                    loaded,
                                    egui::Button::new(egui::RichText::new(go).strong())
                                        .min_size(egui::vec2(70.0, 30.0)),
                                )
                                .clicked()
                            {
                                if d.playing {
                                    self.with(move |c| c.deck_stop(k));
                                } else {
                                    self.with(move |c| c.deck_play(k));
                                }
                            }
                            if ui
                                .add_enabled(
                                    loaded,
                                    egui::Button::new("Cue").min_size(egui::vec2(46.0, 30.0)),
                                )
                                .on_hover_text("Zurück an den Anfang")
                                .clicked()
                            {
                                self.with(move |c| c.deck_cue(k));
                            }
                            if ui
                                .add_enabled(
                                    loaded && v.pfl_available,
                                    egui::Button::new("Vorhören").min_size(egui::vec2(70.0, 30.0)),
                                )
                                .on_disabled_hover_text(NO_PFL)
                                .clicked()
                            {
                                self.with(move |c| {
                                    if let Err(e) = c.pfl_deck(k) {
                                        c.report(e);
                                    }
                                });
                            }
                        });
                    });
                });
            });
            self.deck_rects[k] = out.response.rect;
            let Some(r) = title_r else {
                continue;
            };
            r.context_menu(|ui| {
                if ui.button("Datei laden …").clicked() {
                    self.pick_deck_file(k);
                    ui.close();
                }
                if loaded && ui.button("Start und Ende schneiden …").clicked() {
                    self.open_deck_cue_editor(k);
                    ui.close();
                }
                if loaded && ui.button("Leeren").clicked() {
                    self.with(move |c| c.deck_eject(k));
                    ui.close();
                }
            });
            ui.add_space(4.0);
        }
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
    fn time_of_day_follows_the_zone() {
        use woa_engine::localtime::Zone;
        // 2026-12-31 22:59:58 UTC ist in Mitteleuropa (Winter) 23:59:58.
        let unix = 1_798_757_998;
        assert_eq!(super::time_of_day(unix, Zone::CENTRAL_EUROPE), "23:59:58");
        assert_eq!(
            super::time_of_day(unix + 2, Zone::CENTRAL_EUROPE),
            "00:00:00"
        );
        assert_eq!(super::time_of_day(unix, Zone::fixed(0)), "22:59:58");
    }

    #[test]
    fn cart_keys_map_to_egui_keys_and_skip_reserved_ones() {
        use eframe::egui::Key;
        assert_eq!(super::cart_key_of("1"), Some(Key::Num1));
        assert_eq!(super::cart_key_of("0"), Some(Key::Num0));
        assert_eq!(super::cart_key_of("F5"), Some(Key::F5));
        assert_eq!(super::cart_key_of("M"), None);
        assert_eq!(super::cart_key_of("Space"), None);
        assert_eq!(super::cart_key_of("gibtsnicht"), None);
        // Jeder Name, den egui liefert, kommt als Taste zurück.
        for k in [Key::Num7, Key::A, Key::F12, Key::Comma] {
            assert_eq!(Key::from_name(k.name()), Some(k));
        }
        assert_eq!(super::cart_number_key(9).as_deref(), Some("0"));
        assert_eq!(super::cart_number_key(10), None);
    }

    #[test]
    fn clock_format() {
        assert_eq!(super::clock(0.0), "00:00");
        assert_eq!(super::clock(83.4), "01:23");
        assert_eq!(super::clock(3725.0), "1:02:05");
        assert_eq!(super::clock(-3.0), "00:00");
    }
}
