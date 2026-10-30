//! The eframe window. Thin: reads [`AppState`], forwards user actions to it.

use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;
use woa_engine::clip::PEAK_BLOCK;
use woa_engine::record::next_take_path;
use woa_model::{ClipId, MAX_GAIN_DB, MAX_TEMPO_BPM, MIN_GAIN_DB, MIN_TEMPO_BPM, TrackId};

use crate::player::Player;
use crate::sources::{self, SourceEntry, Take};
use crate::theme;

/// Aktiver Reiter im angedockten unteren Studio-Panel (Ableton-Detailansicht).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StudioDockTab {
    Closed,
    Mixer,
    PianoRoll,
    Automation,
}

/// Pixel je Beat in der Zeitleiste.
/// Start-Zoom der Zeitleiste in Pixel je Beat.
const PX_PER_BEAT: f32 = 24.0;
/// Breite der Spurköpfe.
const HEADER_W: f32 = 264.0;
/// Höhe einer Spur.
const ROW_H: f32 = 92.0;
const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 2.0..=160.0;

use crate::controls::{
    ControlDesk, Surface, gain_from_norm, gain_to_norm, level_from_norm, level_to_norm,
    pan_from_norm, pan_to_norm,
};
use crate::feedback::{FeedbackDialog, GuiConfig, Mode, Phase, encode_png};
use crate::radio::RadioDesk;
use crate::state::{AppState, Status};
use crate::timeline::{Clipboard, Snap, TimeUnit, ruler_ticks};
use woa_io::controller::{Layout, Target};

/// The desktop window.
pub struct WoaApp {
    /// Frames since `frames_since` (only with `WOA_REPAINT_DEBUG`).
    frames: (u32, Instant),
    state: AppState,
    config: GuiConfig,
    /// Where `config.json` is saved when the user changes a setting (`None` = not saved).
    config_path: Option<PathBuf>,
    feedback: FeedbackDialog,
    /// Track being renamed and its edit buffer.
    rename: Option<(TrackId, String)>,
    /// Gain being dragged (committed as one undo step on release).
    gain_drag: Option<(TrackId, f64)>,
    /// Laufende Änderung eines Effekt-Reglers (Effekt, Regler, Wert), ein Undo-Schritt am Ende.
    fx_drag: Option<(woa_model::InsertId, usize, f64)>,
    /// Laufende Änderung eines Plugin-Reglers (Platz, Regler, Wert), ein Undo-Schritt am Ende.
    plugin_drag: Option<(woa_model::InsertId, u32, f64)>,
    /// Laufende Änderung eines Send-Pegels (Spur, Ziel, dB).
    send_drag: Option<(TrackId, TrackId, f64)>,
    /// Eingabefeld "+ Bus".
    new_bus_name: String,
    /// Stereo-Panorama beim Ziehen (wird beim Loslassen als ein Undo-Schritt übernommen).
    pan_drag: Option<(TrackId, f64)>,
    /// Tempo beim Ziehen (wird beim Loslassen als ein Undo-Schritt übernommen).
    tempo_drag: Option<f64>,
    /// Loop-Wiedergabe aktiv?
    loop_enabled: bool,
    /// Loop-Anfang in Takten (ab 1).
    loop_start_bar: u32,
    /// Loop-Ende in Takten (exklusiv, > `loop_start_bar`).
    loop_end_bar: u32,
    /// Punch-Bereich aktiv (Paket 22): von der Aufnahme bleibt nur dieser Teil?
    punch_enabled: bool,
    /// Punch-In in Takten (ab 1).
    punch_in_bar: u32,
    /// Punch-Out in Takten (exklusiv, > `punch_in_bar`).
    punch_out_bar: u32,
    /// Loop-Bereich in Beats, wie er beim Start der laufenden Aufnahme galt.
    rec_loop: Option<(f64, f64)>,
    /// Punch-Bereich in Beats, wie er beim Start der laufenden Aufnahme galt.
    rec_punch: Option<(f64, f64)>,
    /// Aufgeklappte Take-Ordner (Paket 22).
    open_folders: std::collections::HashSet<u64>,
    /// Laufendes Ziehen im Take-Ordner: Spur, Ordner, Take, Anfang und Ende in Beats.
    comp_drag: Option<(TrackId, u64, usize, f64, f64)>,
    /// Unteres Studio-Dock (Mixer-Konsole / Piano-Roll).
    studio_dock: StudioDockTab,
    /// Piano-Roll als schwebendes Fenster statt im unteren Dock?
    piano_roll_floating: bool,
    /// Analyse-Fenster (Paket 19) offen?
    analysis_open: bool,
    /// Rechnung und Zustand des Analyse-Fensters; entsteht beim ersten Öffnen.
    analysis: Option<crate::analysis::AnalysisView>,
    /// Abholpuffer für den Abgriff (links, rechts), wiederverwendet.
    analysis_buf: (Vec<f32>, Vec<f32>),
    /// Last title sent to the viewport.
    last_title: String,
    /// Laufende Aufnahmen, je Zielspur eine (leer = keine Aufnahme).
    takes: Vec<(Option<TrackId>, Take)>,
    /// Startpunkt der laufenden Aufnahme in Beats.
    rec_start: f64,
    /// Laufende MIDI-Aufnahmen (Paket 12): Zielspur und Take.
    midi_takes: Vec<(TrackId, woa_io::record::MidiTake)>,
    /// Wellenform der laufenden Aufnahme je Spur: Spitzen je `PEAK_BLOCK` Frames und deren
    /// Abtastrate (#43).
    live_peaks: std::collections::HashMap<TrackId, (Vec<f32>, u32)>,
    /// Seit Aufnahmestart ein Sample bei 0 dBFS oder darüber (Übersteuerungslampe).
    rec_clipped: bool,
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
    /// Fades being dragged at the clip corners (Paket 21): (clip, fade-in, fade-out) in beats.
    fade_handle: Option<(ClipId, f64, f64)>,
    /// Clip gain being dragged in the inspector: (clip, dB), committed when the drag ends.
    gain_preview: Option<(ClipId, f64)>,
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
    /// Stretch value (percent) being typed or dragged in the inspector.
    stretch_edit: Option<(ClipId, f64)>,
    /// Pitch value (semitones) being typed or dragged in the inspector.
    pitch_edit: Option<(ClipId, f64)>,
    /// Clip being time-stretched at an edge with Alt held (Paket 23): (clip, left edge, delta).
    stretch_drag: Option<(ClipId, bool, f64)>,
    /// Zoom der Zeitleiste in Pixel je Beat.
    zoom: f32,
    /// Raster beim Verschieben und Kürzen (Paket 20).
    snap: Snap,
    /// Beschriftung des Lineals: Takte oder Sekunden.
    time_unit: TimeUnit,
    /// Zwischenablage für Strg+C / Strg+X / Strg+V.
    clipboard: Clipboard,
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
    /// Party-Layout (Paket 35).
    party: crate::party::PartyView,
    /// Datenordner (radio.json, Mitschnitte).
    data_dir: PathBuf,
    /// Bibliotheks-Fenster (Paket 24).
    library: crate::library::LibraryWindow,
    /// Export-Fenster (Paket 26).
    export_dialog: crate::export_dialog::ExportDialog,
    clock_editor: crate::clock_editor::ClockEditor,
    event_editor: crate::event_editor::EventEditor,
    /// Sendeablauf; läuft weiter, auch wenn das Produzent-Layout zu sehen ist.
    radio: Option<RadioDesk>,
    /// MIDI-Eingang (Paket 11), spielt das Instrument der scharfen MIDI-Spur.
    midi: woa_io::midi::MidiIn,
    /// Letzter gesehener Event-Zähler und wann er sich zuletzt geändert hat (Lampe).
    midi_seen: (u64, Option<Instant>),
    /// Offene Piano-Roll (Paket 13), Doppelklick auf einen MIDI-Clip.
    piano_roll: Option<crate::pianoroll::PianoRoll>,
    /// Controller und MIDI-Learn (Paket 14).
    controls: ControlDesk,
    /// Waker für den Controller gesetzt.
    controls_woken: bool,
    /// Fader/Pan vom Controller, noch nicht als Undo-Schritt eingetragen.
    ctrl_pending: Option<Pending>,
    /// Controller-Werte dieses Bildes für Fader und Pan (füttern den Automation-Aufnehmer).
    ctrl_touches: Vec<(TrackId, woa_model::AutoTarget, f64)>,
    /// Laufende Automation-Aufnahmen (Schreiben/Latch) je Spur und Ziel.
    auto_rec: std::collections::HashMap<
        (TrackId, woa_model::AutoTarget),
        crate::automation::AutoRecorder,
    >,
    /// Editor im Studio-Dock.
    auto_ed: crate::automation::EditorState,
    /// Angezeigte Absenkung der dynamischen Effekte (fällt langsam ab).
    gr_shown: std::collections::HashMap<woa_model::InsertId, f32>,
}

/// Ein Controller-Wert, der erst nach einer Ruhepause als ein Undo-Schritt ins Projekt geht
/// (sonst wäre jeder CC ein eigener Schritt).
#[derive(Debug, Clone, Copy)]
struct Pending {
    target: Target,
    track: TrackId,
    value: f64,
    at: Instant,
    /// Die Spur nimmt gerade Automation auf: Der Wert geht in die Kurve, nicht ins Projekt.
    keep: bool,
}

/// Ruhepause, nach der ein Controller-Wert ins Projekt geht.
const CTRL_COMMIT: std::time::Duration = std::time::Duration::from_millis(300);

/// Die App, wie der Controller sie sieht (getrennte Felder, damit die Ausleihe passt).
struct AppSurface<'a> {
    state: &'a mut AppState,
    radio: Option<&'a mut RadioDesk>,
    gain_drag: &'a mut Option<(TrackId, f64)>,
    pan_drag: &'a mut Option<(TrackId, f64)>,
    /// Ziele, die gerade aufgenommen werden (Wiedergabe läuft, Kurve in Schreiben oder Latch).
    recording: &'a [(TrackId, woa_model::AutoTarget)],
    pending: &'a mut Option<Pending>,
    touches: &'a mut Vec<(TrackId, woa_model::AutoTarget, f64)>,
    mode: Mode,
    mode_request: Option<Layout>,
}

impl AppSurface<'_> {
    fn track(&self, i: usize) -> Option<&woa_model::Track> {
        self.state.project().tracks.get(i)
    }

    fn commit(state: &mut AppState, gain_drag: &mut Option<(TrackId, f64)>, p: Pending) {
        if p.keep {
            // Aufnahme: weder Undo-Schritt noch fester Reglerstand; die Kurve entsteht beim Stopp.
            return;
        }
        match p.target {
            Target::TrackGain(_) => {
                if gain_drag.is_some_and(|(t, _)| t == p.track) {
                    *gain_drag = None;
                }
                state.set_gain(p.track, p.value);
            }
            Target::TrackPan(_) => {
                state.set_pan(p.track, p.value);
            }
            _ => {}
        }
    }

    fn hold(&mut self, target: Target, track: TrackId, value: f64) {
        if let Some(old) = self.pending.take_if(|p| p.target != target) {
            Self::commit(self.state, self.gain_drag, old);
        }
        let auto = match target {
            Target::TrackGain(_) => Some(woa_model::AutoTarget::Gain),
            Target::TrackPan(_) => Some(woa_model::AutoTarget::Pan),
            _ => None,
        };
        let keep = auto.is_some_and(|a| self.recording.contains(&(track, a)));
        *self.pending = Some(Pending {
            target,
            track,
            value,
            at: Instant::now(),
            keep,
        });
        if matches!(target, Target::TrackGain(_)) {
            *self.gain_drag = Some((track, value));
        }
        if keep && matches!(target, Target::TrackPan(_)) {
            *self.pan_drag = Some((track, value));
        }
        match target {
            Target::TrackGain(_) => self
                .touches
                .push((track, woa_model::AutoTarget::Gain, value)),
            Target::TrackPan(_) => self
                .touches
                .push((track, woa_model::AutoTarget::Pan, value)),
            _ => {}
        }
    }
}

impl Surface for AppSurface<'_> {
    fn current(&self, target: &Target) -> Option<f32> {
        if let Some(p) = self.pending.as_ref().filter(|p| p.target == *target) {
            return Some(match target {
                Target::TrackPan(_) => pan_to_norm(p.value),
                _ => gain_to_norm(p.value),
            });
        }
        match *target {
            Target::TrackGain(i) => {
                let t = self.track(i)?;
                let g = match *self.gain_drag {
                    Some((d, v)) if d == t.id => v,
                    _ => t.gain_db,
                };
                Some(gain_to_norm(g))
            }
            Target::TrackPan(i) => {
                let t = self.track(i)?;
                let p = match *self.pan_drag {
                    Some((d, v)) if d == t.id => v,
                    _ => t.pan,
                };
                Some(pan_to_norm(p))
            }
            Target::Level(b) => Some(level_to_norm(self.radio.as_ref()?.level_db(b))),
            _ => None,
        }
    }

    fn set(&mut self, target: Target, value: f32) {
        match target {
            Target::TrackGain(i) => {
                if let Some(id) = self.track(i).map(|t| t.id) {
                    self.hold(target, id, gain_from_norm(value));
                }
            }
            Target::TrackPan(i) => {
                if let Some(id) = self.track(i).map(|t| t.id) {
                    self.hold(target, id, pan_from_norm(value));
                }
            }
            Target::Level(b) => {
                if let Some(r) = self.radio.as_mut() {
                    r.set_level(b, level_from_norm(value));
                }
            }
            _ => {}
        }
    }

    fn press(&mut self, target: Target) {
        tracing::debug!(target = %target.label(), "controller press");
        match target {
            Target::TrackMute(i) => {
                if let Some(id) = self.track(i).map(|t| t.id) {
                    self.state.toggle_mute(id);
                }
            }
            Target::TrackSolo(i) => {
                if let Some(id) = self.track(i).map(|t| t.id) {
                    self.state.toggle_solo(id);
                }
            }
            Target::Mode(l) => self.mode_request = Some(l),
            other => {
                if let Some(r) = self.radio.as_mut() {
                    r.press(other);
                }
            }
        }
    }

    fn lit(&self, target: &Target) -> bool {
        match *target {
            Target::TrackMute(i) => self.track(i).is_some_and(|t| t.mute),
            Target::TrackSolo(i) => self.track(i).is_some_and(|t| t.solo),
            Target::Mode(Layout::Radio) => self.mode == Mode::Radio,
            Target::Mode(Layout::Produzent) => self.mode == Mode::Produzent,
            ref other => self.radio.as_ref().is_some_and(|r| r.lit(other)),
        }
    }
}

impl WoaApp {
    /// Builds the app; `recovery_dir` receives autosaves of unsaved projects.
    pub fn new(config: GuiConfig, recovery_dir: PathBuf) -> Self {
        Self::build(config, recovery_dir, true)
    }

    /// Wie [`WoaApp::new`], aber ohne Audiogerät: nichts wird abgespielt. Für Oberflächentests
    /// ohne Ton (Paket 20).
    pub fn without_audio(config: GuiConfig, recovery_dir: PathBuf) -> Self {
        Self::build(config, recovery_dir, false)
    }

    fn build(config: GuiConfig, recovery_dir: PathBuf, open_audio: bool) -> Self {
        let mut state = AppState::new(recovery_dir, Instant::now());
        let player = if open_audio {
            match Player::open() {
                Ok(p) => Some(p),
                Err(e) => {
                    tracing::error!(error = %e, "no playback device");
                    state.status = Status::Error(format!("Keine Wiedergabe: {e}"));
                    None
                }
            }
        } else {
            None
        };
        let sink = player
            .as_ref()
            .map_or_else(woa_io::midi::event_sink, Player::midi_sink);
        let mut midi = woa_io::midi::MidiIn::new(sink);
        if config.midi_input.is_some() {
            midi.select(config.midi_input.clone());
        }
        let controls = ControlDesk::new(&config);
        Self {
            controls,
            controls_woken: false,
            ctrl_pending: None,
            ctrl_touches: Vec::new(),
            auto_rec: std::collections::HashMap::new(),
            auto_ed: crate::automation::EditorState::default(),
            gr_shown: std::collections::HashMap::new(),
            midi,
            midi_seen: (0, None),
            piano_roll: None,
            takes: Vec::new(),
            rec_start: 0.0,
            midi_takes: Vec::new(),
            live_peaks: std::collections::HashMap::new(),
            rec_clipped: false,
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
            fade_handle: None,
            gain_preview: None,
            themed: false,
            party: crate::party::PartyView::default(),
            photo: std::env::var_os("WOA_SCREENSHOT").map(|p| (PathBuf::from(p), 0)),
            selected_clips: Vec::new(),
            export_lufs: Some(-16.0),
            latency_auto: true,
            latency_extra_ms: 0.0,
            trim: None,
            stretch_edit: None,
            pitch_edit: None,
            stretch_drag: None,
            zoom: PX_PER_BEAT,
            snap: Snap::default(),
            time_unit: TimeUnit::default(),
            clipboard: Clipboard::default(),
            track_rects: Vec::new(),
            prev_track_rects: Vec::new(),
            track_levels: std::collections::HashMap::new(),
            level: 0.0,
            state,
            config,
            config_path: None,
            feedback: FeedbackDialog::default(),
            rename: None,
            gain_drag: None,
            fx_drag: None,
            plugin_drag: None,
            send_drag: None,
            new_bus_name: String::new(),
            pan_drag: None,
            tempo_drag: None,
            loop_enabled: false,
            loop_start_bar: 1,
            loop_end_bar: 5,
            punch_enabled: false,
            punch_in_bar: 2,
            punch_out_bar: 4,
            rec_loop: None,
            rec_punch: None,
            open_folders: std::collections::HashSet::new(),
            comp_drag: None,
            studio_dock: StudioDockTab::Mixer,
            piano_roll_floating: std::env::var_os("WOA_PIANOROLL").is_some(),
            analysis_open: false,
            analysis: None,
            analysis_buf: (Vec::new(), Vec::new()),
            last_title: String::new(),
            data_dir: woa_radio::data_dir(),
            library: crate::library::LibraryWindow::new(
                woa_radio::data_dir().join(crate::library::DB_FILE),
            ),
            export_dialog: crate::export_dialog::ExportDialog::new(),
            clock_editor: crate::clock_editor::ClockEditor::new(woa_radio::data_dir()),
            event_editor: crate::event_editor::EventEditor::new(woa_radio::data_dir()),
            radio: None,
            frames: (0, Instant::now()),
        }
    }

    /// Projektzustand (für Tests und Werkzeuge).
    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// Projektzustand zum Ändern (für Tests).
    pub fn state_mut(&mut self) -> &mut AppState {
        &mut self.state
    }

    /// Fläche der Zeitleiste je Spur im zuletzt gezeichneten Bild (Bildschirmpunkte).
    pub fn track_rects(&self) -> &[(TrackId, egui::Rect)] {
        &self.track_rects
    }

    /// Pixel je Beat der Zeitleiste.
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Sets the data folder (radio.json, recordings). Default: the WOA data folder.
    pub fn set_data_dir(&mut self, dir: PathBuf) {
        self.library.set_db_path(dir.join(crate::library::DB_FILE));
        self.clock_editor.set_dir(dir.clone());
        self.event_editor.set_dir(dir.clone());
        self.data_dir = dir;
    }

    fn set_mode(&mut self, mode: Mode) {
        if self.config.mode == mode {
            return;
        }
        self.config.mode = mode;
        if mode.uses_desk() {
            if let Some(p) = self.player.as_mut()
                && p.is_playing()
            {
                self.toggle_play();
            }
            self.ensure_radio();
        }
        if let Some(p) = &self.config_path
            && let Err(e) = self.config.save(p)
        {
            tracing::warn!(error = %e, "config not saved");
        }
    }

    fn save_config(&self) {
        if let Some(p) = &self.config_path
            && let Err(e) = self.config.save(p)
        {
            tracing::warn!(error = %e, "config not saved");
        }
    }

    /// Controller: Queue leeren, Aktionen ausführen, Werte nach Ruhepause ins Projekt.
    fn controller_frame(&mut self, ctx: &egui::Context) {
        if !self.controls_woken {
            self.controls.set_waker(ctx);
            self.controls_woken = true;
        }
        let now = Instant::now();
        self.controls.poll(now);
        if let Some(r) = self.radio.as_mut()
            && let Some(t) = r.learn.take()
        {
            self.controls.mapper.learn(t);
        }
        let playing = self.player.as_ref().is_some_and(Player::is_playing);
        let recording: Vec<(TrackId, woa_model::AutoTarget)> = if playing {
            self.state
                .project()
                .tracks
                .iter()
                .flat_map(|t| {
                    t.automation
                        .iter()
                        .filter(|l| {
                            l.mode != woa_model::AutoMode::Read
                                && matches!(
                                    l.target,
                                    woa_model::AutoTarget::Gain | woa_model::AutoTarget::Pan
                                )
                        })
                        .map(move |l| (t.id, l.target))
                })
                .collect()
        } else {
            Vec::new()
        };
        let mut surface = AppSurface {
            state: &mut self.state,
            radio: self.radio.as_mut(),
            gain_drag: &mut self.gain_drag,
            pan_drag: &mut self.pan_drag,
            recording: &recording,
            pending: &mut self.ctrl_pending,
            touches: &mut self.ctrl_touches,
            mode: self.config.mode,
            mode_request: None,
        };
        let before = self.config.controller_maps.clone();
        let any = self.controls.frame(&mut surface, &mut self.config);
        if let Some(p) = surface
            .pending
            .take_if(|p| now.duration_since(p.at) >= CTRL_COMMIT)
        {
            AppSurface::commit(surface.state, surface.gain_drag, p);
        }
        let request = surface.mode_request;
        self.record_automation();
        if before != self.config.controller_maps {
            self.save_config();
        }
        if let Some(l) = request {
            let mode = match l {
                Layout::Produzent => Mode::Produzent,
                Layout::Radio => Mode::Radio,
                Layout::Toggle if self.config.mode != Mode::Produzent => Mode::Produzent,
                Layout::Toggle => Mode::Radio,
            };
            self.set_mode(mode);
        }
        if any || self.ctrl_pending.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
    }

    /// Automation aufnehmen (Paket 18): Spuren mit einer Kurve im Modus Schreiben oder Latch
    /// folgen beim Abspielen dem Controller (Fader, Pan); beim Stopp wird die Fahrt eine
    /// Kurve im Lesen-Modus, als ein Undo-Schritt.
    fn record_automation(&mut self) {
        use woa_model::{AutoMode, AutoTarget};
        let touches = std::mem::take(&mut self.ctrl_touches);
        let project = self.state.project();
        let tempo = project.tempo_bpm;
        let armed: Vec<(
            TrackId,
            AutoTarget,
            AutoMode,
            Vec<woa_model::AutoPoint>,
            f64,
        )> = project
            .tracks
            .iter()
            .flat_map(|t| {
                t.automation
                    .iter()
                    .filter(|l| {
                        l.mode != AutoMode::Read
                            && matches!(l.target, AutoTarget::Gain | AutoTarget::Pan)
                    })
                    .map(move |l| {
                        let fader = if l.target == AutoTarget::Gain {
                            t.gain_db
                        } else {
                            t.pan
                        };
                        (t.id, l.target, l.mode, l.points.clone(), fader)
                    })
            })
            .collect();
        let dropped: Vec<TrackId> = self
            .auto_rec
            .keys()
            .filter(|k| !armed.iter().any(|(t, x, ..)| (*t, *x) == **k))
            .map(|k| k.0)
            .collect();
        self.auto_rec
            .retain(|k, _| armed.iter().any(|(t, x, ..)| (*t, *x) == *k));
        for t in dropped {
            self.release_held(t);
        }
        let playing = self.player.as_ref().is_some_and(Player::is_playing);
        let beat = self
            .player
            .as_ref()
            .map_or(0.0, |p| p.position_beats(tempo));
        for (track, target, mode, points, fader) in armed {
            if playing {
                let Some(range) = self
                    .state
                    .project()
                    .track(track)
                    .and_then(|t| target.range_in(t).ok())
                else {
                    continue;
                };
                let touch = touches
                    .iter()
                    .rev()
                    .find(|(t, x, _)| *t == track && *x == target)
                    .map(|(_, _, v)| *v);
                self.auto_rec
                    .entry((track, target))
                    .or_insert_with(|| crate::automation::AutoRecorder::new(mode, range))
                    .update(beat, touch, fader);
            } else if let Some(rec) = self.auto_rec.remove(&(track, target)) {
                self.release_held(track);
                let _ = self.ctrl_pending.take_if(|p| p.keep && p.track == track);
                if let Some(next) = rec.finish(&points) {
                    self.state
                        .set_automation(track, target, next, AutoMode::Read);
                }
            }
        }
    }

    /// Lässt die vom Controller gehaltenen Reglerwerte einer Spur los (Aufnahme beendet).
    fn release_held(&mut self, track: TrackId) {
        if self.gain_drag.is_some_and(|(t, _)| t == track) {
            self.gain_drag = None;
        }
        if self.pan_drag.is_some_and(|(t, _)| t == track) {
            self.pan_drag = None;
        }
    }

    /// Der Wert, den ein Lautstärke-Regler zeigt: beim Abspielen folgt er der Kurve (Lesen).
    fn shown_gain(&self, id: TrackId, gain: f64) -> f64 {
        use woa_model::{AutoMode, AutoTarget};
        let Some(p) = self.player.as_ref().filter(|p| p.is_playing()) else {
            return gain;
        };
        self.state
            .project()
            .track(id)
            .and_then(|t| t.lane(&AutoTarget::Gain))
            .filter(|l| l.mode == AutoMode::Read)
            .and_then(|l| l.value_at(p.position_beats(self.state.project().tempo_bpm), false))
            .unwrap_or(gain)
    }

    /// „MIDI lernen“ im Kontextmenü eines Widgets.
    fn learn_menu(&mut self, r: &egui::Response, t: Target) {
        let bound = self.controls.mapper.source_of(t).map(|s| s.label());
        r.context_menu(|ui| {
            if let Some(t) = crate::controls::learn_menu(ui, t, bound) {
                self.controls.mapper.learn(t);
            }
        });
    }

    fn ensure_radio(&mut self) {
        if self.radio.is_none() {
            let path = self.data_dir.join(woa_radio::RADIO_FILE);
            self.radio = Some(RadioDesk::open(
                path,
                self.data_dir.clone(),
                self.config.radio_buffer(),
                &self.config.pfl_output,
            ));
        }
    }

    /// Untermenü „Vorhören (PFL)“ (Paket 15): aus, Kanal 3/4 des Sendegeräts oder ein
    /// zweites Gerät. Gespeichert in `config.json`; die Radio-Ausgabe öffnet neu, sobald
    /// nichts läuft (sonst ab dem nächsten Öffnen).
    fn pfl_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Vorhören (PFL)", |ui| {
            ui.label(
                egui::RichText::new("Kopfhörer, nie auf Sendung")
                    .small()
                    .color(theme::DIM),
            );
            let mut choice = self.config.pfl_output.clone();
            ui.radio_value(&mut choice, crate::PflOutput::Aus, "Aus");
            ui.radio_value(
                &mut choice,
                crate::PflOutput::Kanal34,
                "Kanal 3/4 des Sendegeräts",
            );
            match woa_engine::device::list_output_devices() {
                Ok(list) => {
                    for d in list {
                        ui.radio_value(
                            &mut choice,
                            crate::PflOutput::Geraet(d.id.clone()),
                            format!("Gerät: {}", d.name),
                        );
                    }
                }
                Err(e) => {
                    ui.label(format!("Geräte nicht lesbar: {e}"));
                }
            }
            if choice != self.config.pfl_output {
                tracing::info!(?choice, "pfl output chosen");
                self.config.pfl_output = choice;
                self.save_config();
                if self.radio.as_ref().is_some_and(|r| !r.is_on_air()) {
                    self.radio = None;
                    if self.config.mode.uses_desk() {
                        self.ensure_radio();
                    }
                }
                ui.close();
            }
        });
    }

    /// Untermenü „MIDI-Eingang“: Aus oder ein Gerät; gespeichert in `config.json`.
    fn midi_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("MIDI-Eingang", |ui| {
            let wanted = self.midi.wanted().map(str::to_string);
            let mut ports: Vec<String> = self.midi.ports().to_vec();
            if let Some(w) = &wanted
                && !ports.contains(w)
            {
                ports.push(w.clone());
            }
            let mut choice = wanted.clone();
            ui.radio_value(&mut choice, None, "Aus");
            if ports.is_empty() {
                ui.weak("Kein MIDI-Gerät gefunden");
            }
            for p in ports {
                let missing = !self.midi.ports().contains(&p);
                let label = if missing {
                    format!("{p} (nicht verbunden)")
                } else {
                    p.clone()
                };
                ui.radio_value(&mut choice, Some(p), label);
            }
            ui.separator();
            ui.weak("Spielt die scharf geschaltete MIDI-Spur");
            if choice != wanted {
                self.midi.select(choice.clone());
                self.config.midi_input = choice;
                if let Some(p) = &self.config_path
                    && let Err(e) = self.config.save(p)
                {
                    tracing::warn!(error = %e, "config not saved");
                }
            }
        });
    }

    /// Die MIDI-Spur, die das Keyboard spielt: die erste scharf geschaltete MIDI-Spur
    /// (ohne scharfe Spuren: die ausgewählte, wenn sie eine MIDI-Spur ist).
    fn live_midi_track(&self) -> Option<TrackId> {
        self.state
            .project()
            .tracks
            .iter()
            .filter(|t| t.kind == woa_model::TrackKind::Midi)
            .map(|t| t.id)
            .find(|id| {
                if self.armed.is_empty() {
                    self.selected_track == Some(*id)
                } else {
                    self.armed.contains(id)
                }
            })
    }

    /// MIDI-Lampe: leuchtet kurz bei jedem Event, Tooltip nennt Gerät und Zustand.
    fn midi_lamp(&mut self, ui: &mut egui::Ui) {
        use woa_io::midi::MidiStatus;
        let now = Instant::now();
        let events = self
            .midi
            .counters()
            .events
            .load(std::sync::atomic::Ordering::Relaxed);
        if events != self.midi_seen.0 {
            self.midi_seen = (events, Some(now));
        }
        let lit = self
            .midi_seen
            .1
            .is_some_and(|t| now.duration_since(t).as_millis() < 150);
        let status = self.midi.status();
        let (color, active) = match (&status, lit) {
            (MidiStatus::Off, _) => (theme::DIM, false),
            (MidiStatus::Waiting(_), _) => (theme::ON_AIR, true),
            (MidiStatus::Connected(_), true) => (theme::OK_GREEN, true),
            (MidiStatus::Connected(_), false) => (theme::CUE_CYAN, false),
        };
        let resp = egui::Frame::new()
            .fill(if active {
                theme::mix(color, theme::WELL, 0.72)
            } else {
                theme::WELL
            })
            .stroke(egui::Stroke::new(
                1.0,
                if active { color } else { theme::BORDER },
            ))
            .corner_radius(4)
            .inner_margin(egui::Margin::symmetric(8, 3))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                ui.horizontal(|ui| {
                    let (dot, _) =
                        ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot.center(), 3.8, color);
                    ui.label(
                        egui::RichText::new("MIDI")
                            .size(11.0)
                            .strong()
                            .monospace()
                            .color(if active {
                                theme::TEXT_BRIGHT
                            } else {
                                theme::DIM
                            }),
                    );
                });
            })
            .response;
        let target = self
            .live_midi_track()
            .and_then(|id| self.state.project().track(id).map(|t| t.name.clone()));
        let text = match status {
            MidiStatus::Off => {
                "MIDI: kein Gerät gewählt (Einstellungen → MIDI-Eingang)".to_string()
            }
            MidiStatus::Waiting(n) => format!("MIDI: {n} nicht da, suche alle 2 s"),
            MidiStatus::Connected(n) => format!(
                "MIDI: {n}\nSpielt: {}",
                target.as_deref().unwrap_or("keine MIDI-Spur scharf")
            ),
        };
        resp.on_hover_text(text);
        if lit {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(60));
        }
    }

    fn mode_switch(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            self.midi_lamp(ui);
            let mut mode = self.config.mode;
            let on_air = self.radio.as_ref().is_some_and(RadioDesk::is_on_air);
            egui::Frame::new()
                .fill(theme::WELL)
                .stroke(egui::Stroke::new(1.0, theme::BORDER))
                .corner_radius(5)
                .inner_margin(egui::Margin::symmetric(4, 2))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let radio_active = mode == Mode::Radio;
                    let radio_btn = egui::Button::new(
                        egui::RichText::new(if on_air {
                            "● 📻 Radio"
                        } else {
                            "📻 Radio"
                        })
                        .strong()
                        .color(if radio_active || on_air {
                            theme::WELL
                        } else {
                            theme::DIM
                        }),
                    )
                    .fill(if on_air {
                        theme::ON_AIR
                    } else if radio_active {
                        theme::ACCENT
                    } else {
                        theme::WELL
                    })
                    .stroke(egui::Stroke::NONE);
                    if ui
                        .add(radio_btn)
                        .on_hover_text("Sendeablauf: Playlist, Uhr, Carts, Mikro")
                        .clicked()
                    {
                        mode = Mode::Radio;
                    }

                    let party_active = mode == Mode::Party;
                    let party_btn =
                        egui::Button::new(egui::RichText::new("🎉 Party").strong().color(
                            if party_active {
                                theme::WELL
                            } else {
                                theme::DIM
                            },
                        ))
                        .fill(if party_active {
                            theme::ACCENT
                        } else {
                            theme::WELL
                        })
                        .stroke(egui::Stroke::NONE);
                    if ui
                        .add(party_btn)
                        .on_hover_text("Abspielmodus: große Anzeige, Wunschliste, Vollbild")
                        .clicked()
                    {
                        mode = Mode::Party;
                    }

                    let prod_active = mode == Mode::Produzent;
                    let prod_btn = egui::Button::new(
                        egui::RichText::new("🎚 Produzent")
                            .strong()
                            .color(if prod_active { theme::WELL } else { theme::DIM }),
                    )
                    .fill(if prod_active {
                        theme::ACCENT
                    } else {
                        theme::WELL
                    })
                    .stroke(egui::Stroke::NONE);
                    if ui
                        .add(prod_btn)
                        .on_hover_text("Zeitleiste mit Spuren und Clips")
                        .clicked()
                    {
                        mode = Mode::Produzent;
                    }
                });
            self.set_mode(mode);
        });
    }

    /// Startet oder beendet die Aufnahme. Jede scharf geschaltete Spur (sonst die
    /// ausgewählte) nimmt ihre eigene Quelle in `aufnahmen/` auf, alle ab demselben Punkt.
    fn toggle_record(&mut self) {
        if self.recording() {
            self.stop_record();
            return;
        }
        let dir = self.state.takes_dir();
        let tempo = self.state.project().tempo_bpm;
        let mut start = self
            .player
            .as_ref()
            .map_or(0.0, |p| p.position_beats(tempo));
        // Paket 22: Mit Loop beginnt die Aufnahme am Loop-Anfang, jede Runde wird ein Take.
        let loop_range = self.loop_beats();
        let punch_range = self.punch_beats();
        let targets: Vec<Option<TrackId>> = if self.armed.is_empty() {
            vec![self.selected_track]
        } else {
            self.armed.iter().copied().map(Some).collect()
        };
        let mut errors = Vec::new();
        let project = self.state.project();
        let (midi_targets, targets): (Vec<_>, Vec<_>) = targets.into_iter().partition(|t| {
            t.and_then(|t| project.track(t))
                .is_some_and(|t| t.kind == woa_model::TrackKind::Midi)
        });
        if !midi_targets.is_empty() {
            match self.player.as_ref() {
                Some(p) => {
                    let tap = self.midi.record_tap();
                    let at = p.position_samples();
                    for t in midi_targets.into_iter().flatten() {
                        let take = woa_io::record::MidiTake::start(&tap, p.clock(), at);
                        tracing::info!(track = t.0, start_sample = at, "midi take started");
                        self.midi_takes.push((t, take));
                    }
                }
                None => errors.push("MIDI: keine Wiedergabe, keine Uhr".into()),
            }
        }
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
        if !self.recording() {
            self.state.status =
                Status::Error(format!("Aufnahme nicht gestartet: {}", errors.join("; ")));
            return;
        }
        if let Some((ls, _)) = loop_range
            && !self.takes.is_empty()
        {
            start = ls;
            if let Some(p) = self.player.as_mut() {
                p.seek_beats(ls, tempo);
            }
        }
        self.rec_loop = loop_range.filter(|_| !self.takes.is_empty());
        self.rec_punch = punch_range;
        tracing::info!(start_beats = start, loop = ?self.rec_loop, punch = ?self.rec_punch, "recording starts");
        self.rec_start = start;
        self.live_peaks.clear();
        self.rec_clipped = false;
        self.state.status = if errors.is_empty() {
            Status::Info(format!(
                "Nimmt auf: {} Spur(en)",
                self.takes.len() + self.midi_takes.len()
            ))
        } else {
            Status::Error(format!("Nimmt auf, aber: {}", errors.join("; ")))
        };
        // Overdub: die anderen Spuren spielen ab dem Kopf mit.
        if let Some(p) = self.player.as_mut() {
            p.play();
        }
    }

    /// Loop-Bereich in Beats, wenn die Schleife an ist.
    fn loop_beats(&self) -> Option<(f64, f64)> {
        (self.loop_enabled && self.loop_end_bar > self.loop_start_bar).then(|| {
            (
                f64::from(self.loop_start_bar.saturating_sub(1) * 4),
                f64::from(self.loop_end_bar.saturating_sub(1) * 4),
            )
        })
    }

    /// Punch-Bereich in Beats, wenn Punch an ist.
    fn punch_beats(&self) -> Option<(f64, f64)> {
        (self.punch_enabled && self.punch_out_bar > self.punch_in_bar).then(|| {
            (
                f64::from(self.punch_in_bar.saturating_sub(1) * 4),
                f64::from(self.punch_out_bar.saturating_sub(1) * 4),
            )
        })
    }

    fn stop_record(&mut self) {
        self.level = 0.0;
        self.rec_levels.clear();
        self.live_peaks.clear();
        if let Some(p) = self.player.as_mut() {
            p.stop();
        }
        let start = self.rec_start;
        let mut saved = Vec::new();
        let mut errors = Vec::new();
        let tempo = self.state.project().tempo_bpm;
        for (track, take) in std::mem::take(&mut self.midi_takes) {
            let r = take.finish(tempo);
            tracing::info!(
                events = r.events,
                notes = r.midi.notes.len(),
                dropped = r.dropped,
                "midi take finished"
            );
            if r.midi.notes.is_empty() && r.midi.cc.is_empty() {
                errors.push("MIDI: keine Noten gespielt".to_string());
                continue;
            }
            let n = r.midi.notes.len();
            if let Some(clip) =
                self.state
                    .add_midi_clip(Some(track), "MIDI-Take", start, r.length_beats, r.midi)
            {
                self.selected_clip = Some(clip);
                self.selected_track = Some(track);
            }
            let lost = if r.dropped > 0 {
                format!(", {} Events verloren", r.dropped)
            } else {
                String::new()
            };
            saved.push(format!("{n} Noten{lost}"));
        }
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
                    let rate = f64::from(s.sample_rate.max(1));
                    let latency_frames = (latency.max(0.0) * rate).round() as u64;
                    tracing::info!(
                        latency_ms = latency * 1000.0,
                        latency_frames,
                        frames = s.frames,
                        "latency compensation"
                    );
                    let plan = woa_model::plan_recording(&woa_model::RecordInput {
                        start_beats: start,
                        tempo_bpm: tempo,
                        rate,
                        frames: s.frames,
                        latency_frames,
                        loop_beats: self.rec_loop,
                        punch_beats: self.rec_punch,
                    });
                    let Some(plan) = plan else {
                        errors.push("Aufnahme zu kurz oder außerhalb des Punch-Bereichs".into());
                        continue;
                    };
                    if let woa_model::RecordPlan::Folder { offsets, .. } = &plan {
                        tracing::info!(takes = offsets.len(), "loop takes");
                    }
                    if let Some(clip) = self.state.add_recording(track, &s.path, &plan) {
                        self.selected_clip = Some(clip);
                        self.selected_track = self.state.project().clip(clip).map(|(t, _)| t.id);
                    }
                    if let woa_model::RecordPlan::Folder { offsets, .. } = &plan {
                        let n = offsets.len();
                        saved.push(format!("{n} Takes"));
                        continue;
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
        if self.recording() {
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
            let big = |t: &str| egui::RichText::new(t).size(16.0).strong();

            // 1. Hardware-Transport-Gruppe
            egui::Frame::new()
                .fill(theme::WELL)
                .stroke(egui::Stroke::new(1.0, theme::BORDER))
                .corner_radius(5)
                .inner_margin(egui::Margin::symmetric(4, 3))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if ui
                        .add(
                            egui::Button::new(big("⏮"))
                                .fill(theme::SURFACE)
                                .min_size(egui::vec2(38.0, 32.0)),
                        )
                        .on_hover_text("Zum Anfang (Pos1)")
                        .clicked()
                    {
                        self.rewind();
                    }
                    let play = egui::Button::new(
                        big(if playing { "⏸" } else { "▶" }).color(if playing {
                            theme::WELL
                        } else {
                            theme::TEXT_BRIGHT
                        }),
                    )
                    .min_size(egui::vec2(48.0, 32.0))
                    .fill(if playing {
                        theme::OK_GREEN
                    } else {
                        theme::SURFACE
                    })
                    .stroke(egui::Stroke::new(
                        1.0,
                        if playing {
                            theme::OK_GREEN
                        } else {
                            theme::BORDER
                        },
                    ));
                    if ui
                        .add_enabled(self.player.is_some(), play)
                        .on_hover_text("Abspielen / Pause (Leertaste)")
                        .clicked()
                    {
                        self.toggle_play();
                    }

                    let recording = self.recording();
                    let label = match self.takes.first() {
                        Some((_, rec)) => {
                            let t = rec.elapsed().as_secs();
                            format!("■  {}:{:02}", t / 60, t % 60)
                        }
                        None if recording => "■  MIDI".to_string(),
                        None => "     Aufnahme".to_string(),
                    };
                    let rec_btn = egui::Button::new(
                        egui::RichText::new(label)
                            .strong()
                            .size(14.0)
                            .color(if recording {
                                egui::Color32::WHITE
                            } else {
                                theme::TEXT_BRIGHT
                            }),
                    )
                    .min_size(egui::vec2(114.0, 32.0))
                    .fill(if recording {
                        theme::ON_AIR
                    } else {
                        theme::SURFACE
                    })
                    .stroke(egui::Stroke::new(
                        1.0,
                        if recording {
                            theme::ON_AIR
                        } else {
                            theme::BORDER
                        },
                    ));
                    let rec_resp = ui.add(rec_btn).on_hover_text(
                        "Nimmt auf jeder scharfen Spur (roter Punkt) ihre Quelle auf, \
                             sonst auf der ausgewählten Spur (Strg+R)",
                    );
                    if !recording {
                        // Die Schrift hat kein ●, also selbst zeichnen.
                        let c = rec_resp.rect.left_center() + egui::vec2(18.0, 0.0);
                        ui.painter().circle_filled(c, 5.5, theme::ON_AIR);
                    }
                    if rec_resp.clicked() {
                        self.toggle_record();
                    }

                    let end = egui::Button::new(
                        egui::RichText::new("⇥ Stopp am Ende")
                            .small()
                            .strong()
                            .color(if self.stop_at_end {
                                theme::WELL
                            } else {
                                theme::DIM
                            }),
                    )
                    .min_size(egui::vec2(0.0, 32.0))
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

                    let loop_btn = egui::Button::new(
                        egui::RichText::new("🔁 Loop")
                            .small()
                            .strong()
                            .color(if self.loop_enabled {
                                theme::WELL
                            } else {
                                theme::DIM
                            }),
                    )
                    .min_size(egui::vec2(0.0, 32.0))
                    .fill(if self.loop_enabled {
                        theme::CUE_CYAN
                    } else {
                        theme::SURFACE
                    });
                    if ui
                        .add(loop_btn)
                        .on_hover_text("Wiedergabe im gewählten Taktbereich im Kreis abspielen")
                        .clicked()
                    {
                        self.loop_enabled = !self.loop_enabled;
                    }
                    if self.loop_enabled {
                        ui.add(
                            egui::DragValue::new(&mut self.loop_start_bar)
                                .range(1..=self.loop_end_bar.saturating_sub(1).max(1))
                                .speed(0.2)
                                .prefix("Takt "),
                        )
                        .on_hover_text("Loop-Start (Takt)");
                        ui.add(
                            egui::DragValue::new(&mut self.loop_end_bar)
                                .range((self.loop_start_bar + 1)..=512)
                                .speed(0.2)
                                .prefix("– "),
                        )
                        .on_hover_text("Loop-Ende (Takt)");
                    }

                    // Paket 22: Punch-Bereich. Von der Aufnahme bleibt nur dieser Teil.
                    let punch_btn = egui::Button::new(
                        egui::RichText::new("⏺ Punch")
                            .small()
                            .strong()
                            .color(if self.punch_enabled {
                                theme::WELL
                            } else {
                                theme::DIM
                            }),
                    )
                    .min_size(egui::vec2(0.0, 32.0))
                    .fill(if self.punch_enabled {
                        theme::ON_AIR
                    } else {
                        theme::SURFACE
                    });
                    if ui
                        .add(punch_btn)
                        .on_hover_text(
                            "Punch-Bereich: von der Aufnahme bleibt nur der Teil zwischen \
                             Punch-In und Punch-Out. Mit Loop landet nur dieser Teil im Take-Ordner.",
                        )
                        .clicked()
                    {
                        self.punch_enabled = !self.punch_enabled;
                    }
                    if self.punch_enabled {
                        ui.add(
                            egui::DragValue::new(&mut self.punch_in_bar)
                                .range(1..=self.punch_out_bar.saturating_sub(1).max(1))
                                .speed(0.2)
                                .prefix("Takt "),
                        )
                        .on_hover_text("Punch-In (Takt)");
                        ui.add(
                            egui::DragValue::new(&mut self.punch_out_bar)
                                .range((self.punch_in_bar + 1)..=512)
                                .speed(0.2)
                                .prefix("– "),
                        )
                        .on_hover_text("Punch-Out (Takt)");
                    }
                });

            // 2. Tiefschwarzes LCD-Zeit-, Takt- & Tempo-Display
            let secs = self.player.as_ref().map_or(0.0, Player::position_seconds);
            let tempo = self.state.project().tempo_bpm;
            let beats = self
                .player
                .as_ref()
                .map_or(0.0, |p| p.position_beats(tempo));
            egui::Frame::new()
                .fill(theme::WELL)
                .stroke(egui::Stroke::new(1.0, theme::BORDER))
                .corner_radius(5)
                .inner_margin(egui::Margin::symmetric(12, 3))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.label(
                                egui::RichText::new(format!(
                                    "{:02}:{:04.1}",
                                    (secs / 60.0) as u64,
                                    secs % 60.0
                                ))
                                .monospace()
                                .strong()
                                .size(18.5)
                                .color(theme::ACCENT),
                            );
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 5.0;
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Takt {}.{}  ·",
                                        (beats / 4.0) as u64 + 1,
                                        (beats % 4.0) as u64 + 1
                                    ))
                                    .small()
                                    .monospace()
                                    .color(theme::DIM),
                                );
                                let mut bpm = self.tempo_drag.unwrap_or(tempo);
                                let r = ui
                                    .add(
                                        egui::DragValue::new(&mut bpm)
                                            .range(MIN_TEMPO_BPM..=MAX_TEMPO_BPM)
                                            .speed(0.5)
                                            .fixed_decimals(0)
                                            .suffix(" BPM"),
                                    )
                                    .on_hover_text(
                                        "Projekttempo in Schlägen pro Minute (Doppelklick: 120 BPM)",
                                    );
                                if r.double_clicked() {
                                    self.tempo_drag = None;
                                    self.state.set_tempo(120.0);
                                } else {
                                    if r.changed() {
                                        self.tempo_drag = Some(bpm);
                                    }
                                    if r.drag_stopped() || (r.changed() && !r.dragged()) {
                                        self.tempo_drag = None;
                                        self.state.set_tempo(bpm);
                                    }
                                }
                            });
                        });
                    });
                });

            // 3. Live-Aufnahme-Pegelanzeige (wenn Aufnahme aktiv)
            if self.recording() {
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
                    ui.allocate_exact_size(egui::vec2(140.0, 14.0), egui::Sense::hover());
                level_bar(ui.painter(), rect, self.level);
                if self.rec_clipped {
                    theme::status_pill(ui, "ÜBERSTEUERT", theme::ON_AIR, true);
                }
            }

            // 4. Latenz-Kapsel
            egui::Frame::new()
                .fill(theme::WELL)
                .stroke(egui::Stroke::new(1.0, theme::BORDER))
                .corner_radius(5)
                .inner_margin(egui::Margin::symmetric(8, 3))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
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
                });

            // 5. Rechtsbündig: Export-Ziel & Zoom
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let name = |v: Option<f64>| match v {
                    None => "unverändert".to_string(),
                    Some(l) => format!("{l:.0} LUFS"),
                };
                egui::ComboBox::from_id_salt("export_lufs")
                    .selected_text(name(self.export_lufs))
                    .width(106.0)
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
                ui.separator();
                egui::ComboBox::from_id_salt("snap")
                    .selected_text(self.snap.label())
                    .width(64.0)
                    .show_ui(ui, |ui| {
                        for s in Snap::ALL {
                            ui.selectable_value(&mut self.snap, s, s.label());
                        }
                    })
                    .response
                    .on_hover_text("Raster beim Verschieben und Kürzen von Clips");
                ui.label(egui::RichText::new("Raster").small().color(theme::DIM));
                let secs = self.time_unit == TimeUnit::Seconds;
                if ui
                    .selectable_label(secs, if secs { "Sekunden" } else { "Takte" })
                    .on_hover_text("Lineal: Takte oder Sekunden")
                    .clicked()
                {
                    self.time_unit = if secs {
                        TimeUnit::Bars
                    } else {
                        TimeUnit::Seconds
                    };
                }
            });
        });
    }

    /// Übernimmt den Zustand der laufenden Plugins ins Projekt (vor Speichern, Autosave, Export).
    fn capture_plugins(&mut self) {
        let Some(player) = self.player.as_mut() else {
            return;
        };
        let cmds = player.capture_plugin_state(self.state.project());
        self.state.apply_plugin_state(cmds);
    }

    fn save(&mut self) {
        self.capture_plugins();
        if self.state.needs_save_as() {
            self.save_as();
        } else {
            self.state.save();
        }
    }

    fn save_as(&mut self) {
        self.capture_plugins();
        if let Some(dir) = rfd::FileDialog::new()
            .set_title("Projekt speichern")
            .set_file_name(format!("{}.woa", self.state.project().meta.name))
            .save_file()
        {
            self.state.save_as(&dir);
        }
    }

    /// Schreibt das Projekt als `.dawproject` (Austausch mit Bitwig, Studio One, Cubase …).
    fn export_dawproject(&mut self) {
        let Some(out) = rfd::FileDialog::new()
            .set_title("Als DAWproject exportieren")
            .add_filter("DAWproject", &["dawproject"])
            .set_file_name(format!("{}.dawproject", self.state.project().meta.name))
            .save_file()
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
    fn open_dawproject(&mut self) {
        let Some(file) = rfd::FileDialog::new()
            .set_title("DAWproject öffnen")
            .add_filter("DAWproject", &["dawproject"])
            .pick_file()
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

    fn open(&mut self) {
        if let Some(dir) = rfd::FileDialog::new()
            .set_title("Projektordner (.woa) öffnen")
            .pick_folder()
        {
            self.state.open(&dir);
        }
    }

    /// Opens the piano roll for a MIDI clip and selects its track (live instrument for
    /// the preview keyboard).
    fn open_piano_roll(&mut self, clip: ClipId) {
        let Some((track, c)) = self.state.project().clip(clip) else {
            return;
        };
        if c.midi.is_none() {
            return;
        }
        self.selected_track = Some(track.id);
        self.selected_clip = Some(clip);
        tracing::info!(clip = clip.0, "piano roll opened");
        self.piano_roll = Some(crate::pianoroll::PianoRoll::new(clip));
        if !self.piano_roll_floating {
            self.studio_dock = StudioDockTab::PianoRoll;
        }
    }

    /// Draws the open piano roll; closes it when its window or clip is gone.
    fn piano_roll_window(&mut self, ctx: &egui::Context) {
        if !self.piano_roll_floating {
            return;
        }
        let Some(pr) = self.piano_roll.as_mut() else {
            return;
        };
        let track = self.state.project().clip(pr.clip).map(|(t, _)| t.id);
        let live = self
            .state
            .project()
            .tracks
            .iter()
            .filter(|t| t.kind == woa_model::TrackKind::Midi)
            .map(|t| t.id)
            .find(|id| {
                if self.armed.is_empty() {
                    self.selected_track == Some(*id)
                } else {
                    self.armed.contains(id)
                }
            });
        // Vorhören nur, wenn ein Ausgang läuft und die Spur des Clips das Live-Instrument hat.
        let sink = self
            .player
            .as_ref()
            .filter(|_| track.is_some() && track == live)
            .map(Player::midi_sink);
        if !pr.show(ctx, &mut self.state, sink.as_ref()) {
            self.piano_roll = None;
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if let Some(pr) = self.piano_roll.as_mut()
            && !pr.keys(ctx, &mut self.state)
        {
            self.piano_roll = None;
        }
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
            let (copy, cut, paste, dup) = ctx.input_mut(|i| {
                let has =
                    |i: &egui::InputState, f: fn(&egui::Event) -> bool| i.events.iter().any(f);
                (
                    has(i, |e| matches!(e, egui::Event::Copy)),
                    has(i, |e| matches!(e, egui::Event::Cut)),
                    has(i, |e| matches!(e, egui::Event::Paste(_))),
                    i.consume_key(egui::Modifiers::COMMAND, egui::Key::D),
                )
            });
            if copy || cut {
                self.copy_selected();
            }
            if cut {
                self.delete_selected();
            }
            if paste {
                self.paste_at_playhead();
            }
            if dup {
                self.duplicate_selected();
            }
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
        let Some((_, frames)) = &mut self.photo else {
            return;
        };
        *frames += 1;
        let first = *frames == 2;
        if first
            && std::env::var_os("WOA_CUE_EDITOR").is_some()
            && let Some(r) = self.radio.as_mut()
        {
            // Fotomodus des Cue-Editors: erster Titel der Playlist.
            r.open_cue_editor(0);
        }
        if first && std::env::var_os("WOA_CONTROLLER").is_some() {
            // Fotomodus der Zuordnungsliste.
            self.controls.window = true;
        }
        if first && std::env::var_os("WOA_PIANOROLL").is_some() {
            // Fotomodus der Piano-Roll: ersten MIDI-Clip öffnen, zwei Noten wählen.
            let clip = self
                .state
                .project()
                .tracks
                .iter()
                .flat_map(|t| &t.clips)
                .find(|c| c.midi.is_some())
                .map(|c| c.id);
            if let Some(clip) = clip {
                self.open_piano_roll(clip);
                if let Some(pr) = self.piano_roll.as_mut() {
                    pr.sel = vec![1, 2];
                }
            }
            ctx.request_repaint();
            return;
        }
        let Some((path, frames)) = &mut self.photo else {
            return;
        };
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
            self.menu_items(ui, ctx);
            self.mode_switch(ui);
        });
        if let Some(t) = self.controls.learning_text() {
            ui.horizontal(|ui| {
                ui.colored_label(theme::ACCENT, t);
                if ui.small_button("Abbrechen").clicked() {
                    self.controls.mapper.cancel_learn();
                }
            });
        }
    }

    fn menu_items(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        {
            egui::Frame::new()
                .fill(theme::ACCENT)
                .corner_radius(4)
                .inner_margin(egui::Margin::symmetric(8, 2))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("WOA")
                            .size(12.5)
                            .strong()
                            .monospace()
                            .color(theme::WELL),
                    );
                });
            ui.add_space(2.0);
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
                if ui.button("Exportieren … (Strg+E)").clicked() {
                    self.export();
                }
                if ui.button("Als DAWproject exportieren …").clicked() {
                    self.export_dawproject();
                }
                if ui.button("DAWproject öffnen …").clicked() {
                    self.open_dawproject();
                }
                ui.separator();
                if ui
                    .button("Audio-Datei importieren (WAV, FLAC, MP3, OGG) …")
                    .clicked()
                {
                    self.import_audio();
                }
                if ui.button("MIDI importieren (.mid) …").clicked() {
                    self.import_midi();
                }
                if ui.button("Leeren MIDI-Clip anlegen (4 Takte)").clicked() {
                    self.create_midi_clip_at_playhead(self.selected_track);
                }
                if ui.button("MIDI-Clip exportieren (.mid) …").clicked() {
                    self.export_midi();
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
            ui.menu_button("Einstellungen", |ui| {
                self.midi_menu(ui);
                if ui.button("Controller und MIDI-Learn …").clicked() {
                    self.controls.window = true;
                    ui.close();
                }
                self.pfl_menu(ui);
                ui.separator();
                let mut on = self.config.telemetry_enabled == Some(true);
                if ui
                    .checkbox(&mut on, "Fehlercodes anonym senden (ab nächstem Start)")
                    .on_hover_text(TELEMETRY_HINT)
                    .changed()
                {
                    self.set_telemetry(on);
                }
            });
            if ui
                .selectable_label(self.library.open, "Bibliothek")
                .on_hover_text(
                    "Musik, Samples und Jingles suchen, taggen, vorhören und hineinziehen",
                )
                .clicked()
            {
                self.library.open = !self.library.open;
            }
            if ui
                .selectable_label(self.clock_editor.open, "Stundenuhr")
                .on_hover_text("Stundenuhren, Wochenplan und AutoDJ für den Radio-Modus")
                .clicked()
            {
                self.clock_editor.toggle();
            }
            if ui
                .selectable_label(self.event_editor.open, "Zeit-Events")
                .on_hover_text("Nachrichten, Werbung und Stundenjingle zur festen Zeit")
                .clicked()
            {
                self.event_editor.toggle();
            }
            if ui.button("Feedback …").clicked() {
                self.feedback.open();
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            }
        }
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
            .map(|t| (t.id, t.name.clone(), t.gain_db, t.pan, t.mute, t.solo))
            .collect();
        egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 5.0;
            ui.horizontal(|ui| {
                let (corner_rect, _) =
                    ui.allocate_exact_size(egui::vec2(HEADER_W, 24.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(corner_rect, 4.0, theme::PANEL_HEADER);
                ui.painter().text(
                    corner_rect.left_center() + egui::vec2(10.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    format!("SPUREN ({})  ·  KANALZUG", tracks.len()),
                    egui::FontId::proportional(11.0),
                    theme::DIM,
                );
                self.ruler(ui);
            });
            for (i, (id, name, gain, pan, mute, solo)) in tracks.into_iter().enumerate() {
                ui.horizontal(|ui| {
                    self.track_header(ui, i, id, &name, gain, pan, mute, solo);
                    self.timeline(ui, id);
                });
                self.take_folder_rows(ui, id, i);
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui
                    .add_sized(
                        [136.0, 32.0],
                        egui::Button::new(
                            egui::RichText::new("+  Audio-Spur")
                                .strong()
                                .color(theme::TEXT_BRIGHT),
                        )
                        .fill(theme::SURFACE),
                    )
                    .clicked()
                {
                    self.state.add_track();
                }
                if ui
                    .add_sized(
                        [116.0, 32.0],
                        egui::Button::new(
                            egui::RichText::new("+  MIDI-Spur")
                                .strong()
                                .color(theme::CUE_CYAN),
                        )
                        .fill(theme::SURFACE)
                        .stroke(egui::Stroke::new(
                            1.0,
                            theme::mix(theme::CUE_CYAN, theme::BORDER, 0.5),
                        )),
                    )
                    .on_hover_text("Spur für das Keyboard; scharf schalten zum Spielen")
                    .clicked()
                {
                    self.state.add_midi_track();
                }
                if ui
                    .add_sized(
                        [154.0, 32.0],
                        egui::Button::new(
                            egui::RichText::new("📂 Audio importieren …")
                                .small()
                                .strong()
                                .color(theme::TEXT_BRIGHT),
                        )
                        .fill(theme::WELL),
                    )
                    .on_hover_text("WAV, FLAC, MP3, OGG oder M4A als Clip am Abspielkopf einfügen")
                    .clicked()
                {
                    self.import_audio();
                }
                if ui
                    .add_sized(
                        [146.0, 32.0],
                        egui::Button::new(
                            egui::RichText::new("🎹 + MIDI-Clip (4 T.)")
                                .small()
                                .strong()
                                .color(theme::CUE_CYAN),
                        )
                        .fill(theme::WELL),
                    )
                    .on_hover_text(
                        "Leeren 4-Takt-MIDI-Clip anlegen und direkt in der Piano-Roll öffnen (oder Doppelklick in eine MIDI-Spur)",
                    )
                    .clicked()
                {
                    self.create_midi_clip_at_playhead(self.selected_track);
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
            egui::vec2((end as f32 + 16.0) * px, 24.0),
            egui::Sense::click_and_drag(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, theme::WELL);
        painter.hline(
            rect.x_range(),
            rect.bottom() - 1.0,
            egui::Stroke::new(1.0, theme::BORDER),
        );
        let tempo = self.state.project().tempo_bpm;
        for tick in ruler_ticks(self.time_unit, tempo, px, end + 16.0) {
            let x = rect.left() + tick.beat as f32 * px;
            painter.vline(
                x,
                rect.top() + 4.0..=rect.bottom(),
                egui::Stroke::new(1.0, theme::BORDER),
            );
            painter.text(
                egui::pos2(x + 4.0, rect.top() + 3.0),
                egui::Align2::LEFT_TOP,
                &tick.label,
                egui::FontId::monospace(11.0),
                theme::DIM,
            );
        }
        if px >= 12.0 {
            // Schläge als feine Striche am unteren Rand.
            for beat in 0..=(end as usize + 16) {
                let x = rect.left() + beat as f32 * px;
                painter.vline(
                    x,
                    rect.bottom() - 4.0..=rect.bottom(),
                    egui::Stroke::new(1.0, theme::GRID),
                );
            }
        }
        if self.loop_enabled && self.loop_end_bar > self.loop_start_bar {
            let l0 = ((self.loop_start_bar - 1) * 4) as f32 * px;
            let l1 = ((self.loop_end_bar - 1) * 4) as f32 * px;
            let loop_rect = egui::Rect::from_x_y_ranges(
                (rect.left() + l0)..=(rect.left() + l1),
                rect.top()..=rect.bottom(),
            );
            painter.rect_filled(
                loop_rect,
                2.0,
                egui::Color32::from_rgba_unmultiplied(
                    theme::CUE_CYAN.r(),
                    theme::CUE_CYAN.g(),
                    theme::CUE_CYAN.b(),
                    48,
                ),
            );
            painter.hline(
                loop_rect.x_range(),
                rect.top() + 1.5,
                egui::Stroke::new(2.5, theme::CUE_CYAN),
            );
        }
        if let Some(p) = &self.player {
            let x = rect.left() + p.position_beats(tempo) as f32 * px;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(x - 6.0, rect.top() + 8.0),
                    egui::pos2(x + 6.0, rect.top() + 8.0),
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
    /// Beim Aufnehmen oder aktivem Loop nie.
    fn stop_at_audio_end(&mut self) {
        if !self.stop_at_end || self.recording() || self.loop_enabled {
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
            .max(self.live_end_beats())
    }

    /// Läuft eine Aufnahme (Audio oder MIDI)?
    fn recording(&self) -> bool {
        !self.takes.is_empty() || !self.midi_takes.is_empty()
    }

    /// Holt die gestempelten MIDI-Events der laufenden Aufnahmen ab.
    fn drain_midi_takes(&mut self) {
        for (_, take) in &mut self.midi_takes {
            take.drain();
        }
    }

    /// Importiert eine Audiodatei (WAV, FLAC, MP3, OGG, M4A …) in `aufnahmen/` und legt
    /// sie als Clip am Abspielkopf auf eine Audio-Spur.
    fn import_audio(&mut self) {
        let exts = woa_engine::decode::dialog_extensions();
        let ext_refs: Vec<&str> = exts.iter().map(String::as_str).collect();
        let Some(path) = rfd::FileDialog::new()
            .set_title("Audiodatei importieren")
            .add_filter(woa_engine::decode::FILTER_NAME, &ext_refs)
            .pick_file()
        else {
            return;
        };
        let tempo = self.state.project().tempo_bpm;
        let at = self
            .player
            .as_ref()
            .map_or(0.0, |p| p.position_beats(tempo));
        self.import_audio_file(&path, self.selected_track, at);
    }

    /// Kopiert eine Audiodatei ins Projekt und legt sie als Clip bei `at` auf `track`
    /// (`None`: neue Spur). Dialog und Ziehen-und-Ablegen nutzen denselben Weg.
    fn import_audio_file(&mut self, path: &std::path::Path, track: Option<TrackId>, at: f64) {
        let decoded = match woa_engine::decode::decode_file(path) {
            Ok(d) => d,
            Err(e) => {
                tracing::error!(error = %e, path = %path.display(), "audio import decode failed");
                self.state.status = Status::Error(format!("Audio-Import: {e}"));
                return;
            }
        };
        let secs = decoded.frames() as f64 / f64::from(decoded.rate.max(1));
        if secs <= 0.0 {
            self.state.status = Status::Error("Audio-Import: Datei ist leer".into());
            return;
        }
        let dir = self.state.takes_dir();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::error!(error = %e, dir = %dir.display(), "creating takes_dir failed");
            self.state.status = Status::Error(format!("Audio-Import: {e}"));
            return;
        }
        let stem: String = path
            .file_stem()
            .map_or_else(|| "audio".into(), |s| s.to_string_lossy().into_owned())
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("wav")
            .to_ascii_lowercase();
        let mut dst = dir.join(format!("{stem}.{ext}"));
        let mut n = 2u32;
        while dst.exists() && dst != path {
            dst = dir.join(format!("{stem}-{n}.{ext}"));
            n += 1;
        }
        if dst != path
            && let Err(e) = std::fs::copy(path, &dst)
        {
            tracing::error!(error = %e, "copying imported audio failed");
            self.state.status = Status::Error(format!("Audio-Import: {e}"));
            return;
        }
        if let Some(clip) = self.state.add_take(track, &dst, at, secs, 0.0) {
            self.selected_clip = Some(clip);
            self.selected_track = self.state.project().clip(clip).map(|(t, _)| t.id);
            self.state.status = Status::Info(format!("Audio importiert ({secs:.1} s)"));
        }
    }

    /// Legt einen leeren 4-Takt-MIDI-Clip am Abspielkopf an und öffnet die Piano-Roll.
    fn create_midi_clip_at_playhead(&mut self, target: Option<TrackId>) {
        let tempo = self.state.project().tempo_bpm;
        let at = self
            .player
            .as_ref()
            .map_or(0.0, |p| ((p.position_beats(tempo) / 4.0).floor()) * 4.0);
        if let Some(clip) = self.state.create_empty_midi_clip(target, at, 16.0) {
            self.open_piano_roll(clip);
            self.state.status =
                Status::Info("Leerer MIDI-Clip (4 Takte) angelegt – Piano-Roll bereit".into());
        }
    }

    /// Importiert eine `.mid`-Datei als MIDI-Clip am Kopf (Paket 12).
    fn import_midi(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("MIDI importieren")
            .add_filter("MIDI", &["mid", "midi"])
            .pick_file()
        else {
            return;
        };
        let read = std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|b| woa_io::smf::read_smf(&b).map_err(|e| e.to_string()));
        let smf = match read {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(error = %e, path = %path.display(), "midi import failed");
                self.state.status = Status::Error(format!("MIDI-Import: {e}"));
                return;
            }
        };
        let tempo = self.state.project().tempo_bpm;
        let at = self
            .player
            .as_ref()
            .map_or(0.0, |p| p.position_beats(tempo));
        let name = smf.name.clone().unwrap_or_else(|| {
            path.file_stem()
                .map_or_else(|| "MIDI".into(), |s| s.to_string_lossy().into_owned())
        });
        let n = smf.midi.notes.len();
        let target = self.selected_track;
        if let Some(clip) =
            self.state
                .add_midi_clip(target, &name, at, smf.end_beats.max(1.0), smf.midi)
        {
            self.selected_clip = Some(clip);
            self.selected_track = self.state.project().clip(clip).map(|(t, _)| t.id);
            self.state.status = Status::Info(format!("MIDI importiert: {n} Noten"));
        }
    }

    /// Schreibt den ausgewählten MIDI-Clip als `.mid` (Paket 12).
    fn export_midi(&mut self) {
        let clip = self
            .selected_clip
            .and_then(|c| self.state.project().clip(c))
            .and_then(|(_, c)| c.midi.clone().map(|m| (c.name.clone(), m)));
        let Some((name, midi)) = clip else {
            self.state.status = Status::Error("Erst einen MIDI-Clip auswählen".into());
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .set_title("MIDI exportieren")
            .set_file_name(format!("{name}.mid"))
            .add_filter("MIDI", &["mid"])
            .save_file()
        else {
            return;
        };
        let bytes = woa_io::smf::write_smf(&midi, self.state.project().tempo_bpm, &name);
        self.state.status = match std::fs::write(&path, bytes) {
            Ok(()) => Status::Info(format!("MIDI gespeichert: {}", path.display())),
            Err(e) => {
                tracing::error!(error = %e, path = %path.display(), "midi export failed");
                Status::Error(format!("MIDI-Export: {e}"))
            }
        };
    }

    /// Ende der laufenden Aufnahme in Beats (0 ohne Aufnahme).
    fn live_end_beats(&self) -> f64 {
        match self.takes.first() {
            Some((_, take)) => {
                self.rec_start
                    + take.elapsed().as_secs_f64() * self.state.project().tempo_bpm / 60.0
            }
            None => 0.0,
        }
    }

    /// Holt die neuen Wellenform-Punkte aller laufenden Aufnahmen ab (#43).
    fn drain_live_peaks(&mut self) {
        for (track, take) in &mut self.takes {
            let Some(t) = track else { continue };
            let rate = take.sample_rate();
            let (peaks, _) = self
                .live_peaks
                .entry(*t)
                .or_insert_with(|| (Vec::new(), rate));
            let before = peaks.len();
            take.drain_peaks(peaks);
            if peaks[before..].iter().any(|p| *p >= 1.0) {
                self.rec_clipped = true;
            }
        }
    }

    /// Spurkopf: Farbstreifen, Aufnahmeziel, Name, Mute/Solo, Pegel, Gain, Pan.
    #[allow(clippy::too_many_arguments)]
    fn track_header(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        id: TrackId,
        name: &str,
        gain: f64,
        pan: f64,
        mute: bool,
        solo: bool,
    ) {
        let color = theme::track_color(index);
        let is_midi = self
            .state
            .project()
            .track(id)
            .is_some_and(|t| t.kind == woa_model::TrackKind::Midi);
        let armed = if self.armed.is_empty() {
            self.selected_track == Some(id)
        } else {
            self.armed.contains(&id)
        };
        let is_selected = self.selected_track == Some(id);
        let frame = egui::Frame::new()
            .fill(if armed {
                theme::mix(theme::PANEL, color, 0.14)
            } else {
                theme::PANEL
            })
            .stroke(egui::Stroke::new(
                1.0,
                if armed {
                    theme::mix(color, theme::ON_AIR, 0.45)
                } else if is_selected {
                    color
                } else {
                    theme::BORDER
                },
            ))
            .corner_radius(5)
            .inner_margin(egui::Margin {
                left: 12,
                right: 8,
                top: 5,
                bottom: 5,
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
                ui.set_width(HEADER_W - 20.0);
                ui.set_max_width(HEADER_W - 20.0);
                ui.set_height(ROW_H - 10.0);
                ui.spacing_mut().item_spacing = egui::vec2(5.0, 5.0);
                ui.horizontal(|ui| {
                    let (dot, arm) =
                        ui.allocate_exact_size(egui::vec2(20.0, 22.0), egui::Sense::click());
                    ui.painter().rect_filled(
                        dot.shrink2(egui::vec2(1.0, 2.0)),
                        3.0,
                        if armed {
                            theme::mix(theme::ON_AIR, theme::WELL, 0.35)
                        } else {
                            theme::WELL
                        },
                    );
                    ui.painter().circle(
                        dot.center(),
                        5.2,
                        if armed { theme::ON_AIR } else { theme::WELL },
                        egui::Stroke::new(
                            1.5,
                            if armed {
                                egui::Color32::WHITE
                            } else {
                                theme::DIM
                            },
                        ),
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
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        let s_btn = egui::Button::new(
                            egui::RichText::new("S").size(11.5).strong().color(if solo {
                                theme::WELL
                            } else {
                                theme::DIM
                            }),
                        )
                        .fill(if solo { theme::CUE_CYAN } else { theme::WELL })
                        .stroke(egui::Stroke::new(
                            1.0,
                            if solo { theme::CUE_CYAN } else { theme::BORDER },
                        ))
                        .min_size(egui::vec2(22.0, 21.0));
                        if ui
                            .add(s_btn)
                            .on_hover_text("Solo: nur Solo-Spuren sind zu hören")
                            .clicked()
                        {
                            self.state.toggle_solo(id);
                        }
                        let m = egui::Button::new(
                            egui::RichText::new("M").size(11.5).strong().color(if mute {
                                theme::WELL
                            } else {
                                theme::DIM
                            }),
                        )
                        .fill(if mute { theme::ACCENT } else { theme::WELL })
                        .stroke(egui::Stroke::new(
                            1.0,
                            if mute { theme::ACCENT } else { theme::BORDER },
                        ))
                        .min_size(egui::vec2(22.0, 21.0));
                        let r = ui.add(m).on_hover_text("Stumm (Rechtsklick: MIDI lernen)");
                        if r.clicked() {
                            self.state.toggle_mute(id);
                        }
                        if let Some(i) = self.track_index(id) {
                            self.learn_menu(&r, Target::TrackMute(i));
                        }
                        if is_midi {
                            ui.label(
                                egui::RichText::new("MIDI")
                                    .size(9.5)
                                    .strong()
                                    .monospace()
                                    .color(theme::CUE_CYAN),
                            );
                        }
                    });
                });
                ui.horizontal(|ui| {
                    self.track_meter(ui, id, gain, mute);
                    let mut g = match self.gain_drag {
                        Some((d, v)) if d == id => v,
                        _ => self.shown_gain(id, gain),
                    };
                    let r = ui
                        .add(
                            egui::Slider::new(&mut g, MIN_GAIN_DB..=MAX_GAIN_DB)
                                .suffix(" dB")
                                .fixed_decimals(1),
                        )
                        .on_hover_text(
                            "Lautstärke der Spur (Doppelklick: 0 dB, Rechtsklick: MIDI lernen \
                             für Fader, Pan und Solo)",
                        );
                    if let Some(i) = self.track_index(id) {
                        let bound = |t| self.controls.mapper.source_of(t).map(|s| s.label());
                        let items = [
                            (Target::TrackGain(i), bound(Target::TrackGain(i))),
                            (Target::TrackPan(i), bound(Target::TrackPan(i))),
                            (Target::TrackSolo(i), bound(Target::TrackSolo(i))),
                        ];
                        let mut chosen = None;
                        r.context_menu(|ui| {
                            for (t, b) in items {
                                if let Some(t) = crate::controls::learn_menu(ui, t, b) {
                                    chosen = Some(t);
                                }
                            }
                        });
                        if let Some(t) = chosen {
                            self.controls.mapper.learn(t);
                        }
                    }
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
                self.track_source_row(ui, id, pan);
            })
        };
        let r = resp.response.rect;
        resp.response.context_menu(|ui| {
            if ui.button("✏ Spur umbenennen").clicked() {
                self.rename = Some((id, name.to_string()));
                ui.close();
            }
            if is_midi {
                if ui.button("🎹 + Leerer MIDI-Clip (4 Takte)").clicked() {
                    self.create_midi_clip_at_playhead(Some(id));
                    ui.close();
                }
            } else if ui.button("📂 Audio-Datei importieren …").clicked() {
                self.selected_track = Some(id);
                self.import_audio();
                ui.close();
            }
            ui.separator();
            if ui
                .button(egui::RichText::new("🗑 Spur löschen").color(theme::ON_AIR))
                .clicked()
            {
                self.armed.remove(&id);
                if self.selected_track == Some(id) {
                    self.selected_track = None;
                }
                self.state.remove_track(id);
                ui.close();
            }
        });
        ui.painter().rect_filled(
            egui::Rect::from_min_size(r.min, egui::vec2(5.0, r.height())),
            egui::CornerRadius {
                nw: 5,
                sw: 5,
                ne: 0,
                se: 0,
            },
            color,
        );
    }

    /// Quellenwahl, Mithören und Stereo-Panorama einer Spur.
    fn track_source_row(&mut self, ui: &mut egui::Ui, id: TrackId, pan: f64) {
        let busy = self.recording();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            let current = self.track_source.get(&id).cloned().unwrap_or_default();
            let mut chosen = None;
            let mut opened = false;
            ui.add_enabled_ui(!busy, |ui| {
                opened = egui::ComboBox::from_id_salt(("source", id))
                    .selected_text(
                        egui::RichText::new(sources::short_label(&current))
                            .small()
                            .color(theme::DIM),
                    )
                    .width(108.0)
                    .truncate()
                    .show_ui(ui, |ui| {
                        let list = self.sources().to_vec();
                        for s in &list {
                            if ui.selectable_label(s.key == current, &s.label).clicked() {
                                chosen = Some(s.key.clone());
                            }
                        }
                        if !sources::has_apps(&list) {
                            ui.weak(sources::NO_APPS_HINT);
                        }
                    })
                    .response
                    .on_hover_text(
                        "Aufnahmequelle der Spur: Mikrofon, Systemklang oder ein Programm",
                    )
                    .clicked();
            });
            if opened {
                // Beim Öffnen frisch abfragen, nicht erst nach Ablauf der Sekunde.
                self.source_list.1 = None;
            }
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
            let mut p = match self.pan_drag {
                Some((d, v)) if d == id => v,
                _ => pan,
            };
            let mut p_pct = (p * 100.0).round();
            let pr = ui
                .add(
                    egui::DragValue::new(&mut p_pct)
                        .range(-100.0..=100.0)
                        .speed(1.0)
                        .fixed_decimals(0)
                        .custom_formatter(|v, _| {
                            let iv = v.round() as i32;
                            if iv == 0 {
                                "Pan C".to_string()
                            } else if iv < 0 {
                                format!("L{}", -iv)
                            } else {
                                format!("R{iv}")
                            }
                        }),
                )
                .on_hover_text("Stereo-Panorama links/rechts (Doppelklick: Mitte)");
            if pr.double_clicked() {
                self.pan_drag = None;
                self.state.set_pan(id, 0.0);
            } else {
                p = (p_pct / 100.0).clamp(-1.0, 1.0);
                if pr.changed() {
                    self.pan_drag = Some((id, p));
                }
                if pr.drag_stopped() || (pr.changed() && !pr.dragged()) {
                    self.pan_drag = None;
                    self.state.set_pan(id, p);
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
        let (rect, _) = ui.allocate_exact_size(egui::vec2(72.0, 11.0), egui::Sense::hover());
        level_bar(ui.painter(), rect, level);
    }
}

/// Der wachsende Clip einer laufenden Aufnahme (#43): rot getönt, von `start` bis `end`
/// (Beats), darin die Wellenform, so weit Punkte da sind (`ppb` Punkte je Beat).
fn draw_live_take(
    painter: &egui::Painter,
    rect: egui::Rect,
    px: f32,
    start: f64,
    end: f64,
    peaks: &[f32],
    ppb: f64,
) {
    if end <= start || ppb <= 0.0 {
        return;
    }
    let x0 = rect.left() + start as f32 * px;
    let x1 = x0 + ((end - start) as f32 * px).max(2.0);
    let r = egui::Rect::from_x_y_ranges(x0..=x1, rect.top() + 3.0..=rect.bottom() - 3.0);
    painter.rect_filled(r, 5.0, theme::mix(theme::ON_AIR, theme::WELL, 0.65));
    let head = egui::Rect::from_min_max(r.min, egui::pos2(r.right(), r.top() + 16.0));
    painter.rect_filled(
        head,
        egui::CornerRadius {
            nw: 5,
            ne: 5,
            sw: 0,
            se: 0,
        },
        theme::ON_AIR,
    );
    painter.with_clip_rect(head.intersect(rect)).text(
        head.left_center() + egui::vec2(6.0, 0.0),
        egui::Align2::LEFT_CENTER,
        "● Aufnahme läuft",
        egui::FontId::proportional(11.0),
        egui::Color32::WHITE,
    );
    let mid = (r.top() + 16.0 + r.bottom()) / 2.0;
    let half = (r.bottom() - r.top() - 16.0) / 2.0 - 2.0;
    let stroke = egui::Stroke::new(1.0, theme::mix(theme::ON_AIR, egui::Color32::WHITE, 0.45));
    // Nur sichtbare Spalten zeichnen.
    let clip = painter.clip_rect();
    let mut x = r.left().max(clip.left().floor());
    let stop = r.right().min(clip.right());
    while x < stop {
        let b0 = f64::from((x - r.left()) / px);
        let i0 = (b0 * ppb) as usize;
        if i0 >= peaks.len() {
            break;
        }
        let i1 = (((b0 + f64::from(1.0 / px)) * ppb) as usize).clamp(i0 + 1, peaks.len());
        let v = peaks[i0..i1].iter().fold(0.0f32, |m, s| m.max(*s));
        let h = v.min(1.0) * half;
        painter.vline(x, (mid - h)..=(mid + h), stroke);
        x += 1.0;
    }
}

/// Mehrgliedrige Studio-LED-Pegelanzeige (-60..0 dB: Grün, ab -12 dB Gelb, ab -3 dB Rot).
fn level_bar(painter: &egui::Painter, rect: egui::Rect, level: f32) {
    theme::led_meter(painter, rect, level);
}

impl WoaApp {
    /// One track's strip: clips with waveform and fades, the playhead. Click: seek,
    /// click a clip: select, drag a clip: move it (snaps to 1/4 beat).
    /// Take-Ordner einer Spur (Paket 22): eine Zeile mit dem Comp, aufgeklappt eine Zeile je
    /// Take. Ziehen in einer Take-Zeile lässt diesen Take im gezogenen Bereich spielen, ein
    /// Klick wählt den Takt unter dem Zeiger.
    fn take_folder_rows(&mut self, ui: &mut egui::Ui, track: TrackId, index: usize) {
        let Some(t) = self.state.project().track(track) else {
            return;
        };
        let folders = t.take_folders.clone();
        if folders.is_empty() {
            return;
        }
        let tempo = self.state.project().tempo_bpm;
        let px = self.zoom;
        let color = theme::track_color(index);
        let width = (self.timeline_end() as f32 + 16.0) * px;
        let ppb = self.player.as_ref().map(|p| p.peaks_per_beat(tempo));
        for f in folders {
            let open = self.open_folders.contains(&f.id);
            ui.horizontal(|ui| {
                let (slot, _) =
                    ui.allocate_exact_size(egui::vec2(HEADER_W, 24.0), egui::Sense::hover());
                ui.painter().rect_filled(slot, 4.0, theme::PANEL_HEADER);
                let toggle = egui::Rect::from_min_size(slot.min, egui::vec2(HEADER_W - 36.0, 24.0));
                let label = format!(
                    "{} {}  ·  {} Takes",
                    if open { "▾" } else { "▸" },
                    f.name,
                    f.takes.len()
                );
                if ui
                    .put(
                        toggle,
                        egui::Button::new(
                            egui::RichText::new(label).small().color(theme::TEXT_BRIGHT),
                        )
                        .fill(egui::Color32::TRANSPARENT),
                    )
                    .on_hover_text("Takes auf- oder zuklappen")
                    .clicked()
                {
                    if open {
                        self.open_folders.remove(&f.id);
                    } else {
                        self.open_folders.insert(f.id);
                    }
                }
                let del = egui::Rect::from_min_size(
                    egui::pos2(slot.right() - 32.0, slot.top()),
                    egui::vec2(30.0, 24.0),
                );
                if ui
                    .put(
                        del,
                        egui::Button::new(egui::RichText::new("🗑").small().color(theme::DIM))
                            .fill(egui::Color32::TRANSPARENT),
                    )
                    .on_hover_text("Take-Ordner samt Comp löschen (Strg+Z holt ihn zurück)")
                    .clicked()
                {
                    self.state.remove_take_folder(track, f.id);
                }
                // Der Comp als Streifen: welcher Take wo spielt.
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(width, 24.0), egui::Sense::hover());
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 4.0, theme::WELL);
                let x_of = |b: f64| rect.left() + b as f32 * px;
                for s in &f.comp {
                    let r = egui::Rect::from_x_y_ranges(
                        x_of(s.start_beats)..=x_of(s.end_beats),
                        rect.top() + 2.0..=rect.bottom() - 2.0,
                    );
                    painter.rect_filled(r, 3.0, theme::mix(color, theme::WELL, 0.45));
                    painter.text(
                        r.left_center() + egui::vec2(5.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        format!("Take {}", s.take + 1),
                        egui::FontId::proportional(10.0),
                        theme::TEXT_BRIGHT,
                    );
                }
            });
            if !open {
                continue;
            }
            for (k, take) in f.takes.iter().enumerate() {
                let file = self.state.resolve_audio(&take.audio);
                let peaks = match (file, self.player.as_mut()) {
                    (Some(file), Some(p)) => p.peaks(&file),
                    _ => None,
                };
                ui.horizontal(|ui| {
                    let (slot, _) =
                        ui.allocate_exact_size(egui::vec2(HEADER_W, 22.0), egui::Sense::hover());
                    ui.painter().text(
                        slot.left_center() + egui::vec2(22.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        &take.name,
                        egui::FontId::proportional(11.0),
                        theme::DIM,
                    );
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(width, 22.0),
                        egui::Sense::click_and_drag(),
                    );
                    let painter = ui.painter_at(rect);
                    painter.rect_filled(rect, 3.0, theme::WELL);
                    let x_of = |b: f64| rect.left() + b as f32 * px;
                    let span = egui::Rect::from_x_y_ranges(
                        x_of(f.start_beats)..=x_of(f.end_beats()),
                        rect.y_range(),
                    );
                    painter.rect_filled(span, 3.0, theme::mix(color, theme::WELL, 0.85));
                    for s in f.comp.iter().filter(|s| s.take == k) {
                        let r = egui::Rect::from_x_y_ranges(
                            x_of(s.start_beats)..=x_of(s.end_beats),
                            rect.y_range(),
                        );
                        painter.rect_filled(r, 3.0, theme::mix(color, theme::WELL, 0.5));
                    }
                    if let (Some(ppb), Some(peaks)) = (ppb, peaks) {
                        let mid = rect.center().y;
                        let half = rect.height() / 2.0 - 2.0;
                        let stroke =
                            egui::Stroke::new(1.0, theme::mix(color, egui::Color32::WHITE, 0.35));
                        // Only the visible columns, and stop at the end of the peaks (#88).
                        let clip = ui.clip_rect();
                        let mut x = span.left().max(clip.left());
                        let right = span.right().min(clip.right());
                        while x < right {
                            let b0 = take.offset_beats + f64::from((x - span.left()) / px);
                            let i0 = (b0 * ppb) as usize;
                            if i0 >= peaks.len() {
                                break;
                            }
                            let i1 = (((b0 + f64::from(1.0 / px)) * ppb) as usize).max(i0 + 1);
                            let v = peaks
                                .get(i0.min(peaks.len())..i1.min(peaks.len()))
                                .map_or(0.0, |w| w.iter().fold(0.0f32, |m, s| m.max(*s)));
                            let h = v.min(1.0) * half;
                            painter.vline(x, (mid - h)..=(mid + h), stroke);
                            x += 1.0;
                        }
                    }
                    if let Some((tr, fid, tk, a, b)) = self.comp_drag
                        && tr == track
                        && fid == f.id
                        && tk == k
                    {
                        let r = egui::Rect::from_x_y_ranges(
                            x_of(a.min(b))..=x_of(a.max(b)),
                            rect.y_range(),
                        );
                        painter.rect_filled(r, 3.0, theme::CUE_CYAN.gamma_multiply(0.25));
                        painter.rect_stroke(
                            r,
                            3.0,
                            egui::Stroke::new(1.0, theme::CUE_CYAN),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let beat_at = |x: f32| f64::from((x - rect.left()) / px).max(0.0);
                    if resp.drag_started()
                        && let Some(pos) = resp.interact_pointer_pos()
                    {
                        let a = self.snap.apply(beat_at(pos.x));
                        self.comp_drag = Some((track, f.id, k, a, a));
                    } else if resp.dragged()
                        && let Some(pos) = resp.interact_pointer_pos()
                    {
                        let b = self.snap.apply(beat_at(pos.x));
                        if let Some(d) = self.comp_drag.as_mut() {
                            d.4 = b;
                        }
                    }
                    let mut chosen = None;
                    if resp.drag_stopped() {
                        if let Some((_, _, _, a, b)) = self.comp_drag.take() {
                            chosen = Some((a.min(b), a.max(b)));
                        }
                    } else if resp.clicked()
                        && let Some(pos) = resp.interact_pointer_pos()
                    {
                        chosen = Some((beat_at(pos.x), beat_at(pos.x)));
                    }
                    if let Some((a, b)) = chosen {
                        // Ein Klick (oder ein Zug ohne Länge) wählt den ganzen Takt.
                        let (a, b) = if b - a < 0.01 {
                            let bar = (a / 4.0).floor() * 4.0;
                            (bar, bar + 4.0)
                        } else {
                            (a, b)
                        };
                        if self.state.comp_range(track, f.id, a, b, k) {
                            tracing::info!(take = k, from = a, to = b, "comp range set");
                        }
                    }
                });
            }
        }
    }

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
        painter.rect_filled(rect, 5.0, theme::WELL);
        painter.rect_stroke(
            rect,
            5.0,
            egui::Stroke::new(1.0, theme::BORDER),
            egui::StrokeKind::Inside,
        );
        // Raster: jeder zweite Takt leicht aufgehellt, Schläge als feine Linien.
        let bars = end as usize / 4 + 4;
        for bar in 0..bars {
            let x = rect.left() + (bar * 4) as f32 * px;
            if bar % 2 == 1 {
                let r = egui::Rect::from_x_y_ranges(x..=x + 4.0 * px, rect.y_range());
                painter.rect_filled(r, 0.0, theme::mix(theme::WELL, theme::PANEL, 0.38));
            }
            if px >= 10.0 {
                for b in 1..4 {
                    let xb = x + b as f32 * px;
                    painter.vline(
                        xb,
                        rect.y_range(),
                        egui::Stroke::new(1.0, theme::mix(theme::WELL, theme::GRID, 0.45)),
                    );
                }
            }
            painter.vline(x, rect.y_range(), egui::Stroke::new(1.0, theme::GRID));
        }
        let ppb = self.player.as_ref().map(|p| p.peaks_per_beat(tempo));
        let auto_xf = crate::timeline::crossfade_overlaps(&clips);
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
            // Alt-Ziehen am Rand dehnt die Zeit: Länge ändert sich, der Ton wird mitgedehnt.
            let mut view_ratio = c.time_ratio(tempo);
            if let Some((id, left, d)) = self.stretch_drag
                && id == c.id
            {
                if left {
                    (start, len) = (start + d, len - d);
                } else {
                    len += d;
                }
                view_ratio *= len / c.length_beats;
            }
            let start = start.max(0.0);
            let x0 = rect.left() + start as f32 * px;
            let x1 = x0 + (len as f32 * px).max(2.0);
            let r = egui::Rect::from_x_y_ranges(x0..=x1, rect.top() + 3.0..=rect.bottom() - 3.0);
            // Anzeige-Kopie mit gezogenen und automatischen Fades (Überblendung).
            let mut cv = c.clone();
            cv.length_beats = len;
            if let Some((id, a, b)) = self.fade_handle
                && id == c.id
            {
                (cv.fade_in_beats, cv.fade_out_beats) = (a, b);
            }
            if let Some(&(ai, ao)) = auto_xf.get(&c.id) {
                if ai > cv.fade_in_beats {
                    (cv.fade_in_beats, cv.fade_in_curve) = (ai, woa_model::FadeCurve::EqualPower);
                }
                if ao > cv.fade_out_beats {
                    (cv.fade_out_beats, cv.fade_out_curve) = (ao, woa_model::FadeCurve::EqualPower);
                }
            }
            let gain_lin = 10f32.powf(c.gain_db as f32 / 20.0);
            let cresp = ui.interact(
                r,
                ui.id().with(("clip", c.id.0)),
                egui::Sense::click_and_drag(),
            );
            let base = if c.audio.is_some() || c.midi.is_some() {
                color
            } else {
                theme::DIM
            };
            let selected = self.selected_clip == Some(c.id) || self.selected_clips.contains(&c.id);
            painter.rect_filled(r, 5.0, theme::mix(base, theme::WELL, 0.68));
            painter.rect_stroke(
                r,
                5.0,
                egui::Stroke::new(1.0, theme::mix(base, theme::BORDER, 0.35)),
                egui::StrokeKind::Inside,
            );
            // Kopfzeile des Clips in der Spurfarbe, darin der Name.
            let head = egui::Rect::from_min_max(r.min, egui::pos2(r.right(), r.top() + 16.0));
            painter.rect_filled(
                head,
                egui::CornerRadius {
                    nw: 5,
                    ne: 5,
                    sw: 0,
                    se: 0,
                },
                theme::mix(base, theme::WELL, if selected { 0.0 } else { 0.18 }),
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
                let mid = (r.top() + 16.0 + r.bottom()) / 2.0;
                let half = (r.bottom() - r.top() - 16.0) / 2.0 - 2.0;
                let fill_stroke =
                    egui::Stroke::new(1.0, theme::mix(base, egui::Color32::WHITE, 0.25));
                let peak_col = theme::mix(base, egui::Color32::WHITE, 0.65);
                let mut x = r.left();
                while x < r.right() {
                    // Position im Ton in Takt-Einheiten des Projekttempos; Dehnung und
                    // Tempo-Folgen strecken die Zeitachse des Clips (Paket 23).
                    let b0 = offset * tempo / c.source_bpm(tempo)
                        + f64::from((x - r.left()) / px) / view_ratio;
                    let b1 = b0 + f64::from(1.0 / px) / view_ratio;
                    let (i0, i1) = (
                        (b0 * ppb) as usize,
                        ((b1 * ppb) as usize).max((b0 * ppb) as usize + 1),
                    );
                    let v = peaks
                        .get(i0.min(peaks.len())..i1.min(peaks.len()))
                        .map_or(0.0, |w| w.iter().fold(0.0f32, |m, s| m.max(*s)));
                    let local = f64::from((x - r.left()) / px);
                    let env = fade_gain(&cv, local) as f32 * gain_lin;
                    let h = (v * env).min(1.0) * half;
                    painter.vline(x, (mid - h)..=(mid + h), fill_stroke);
                    if h > 1.0 {
                        painter.circle_filled(egui::pos2(x, mid - h), 0.7, peak_col);
                        painter.circle_filled(egui::pos2(x, mid + h), 0.7, peak_col);
                    }
                    x += 1.0;
                }
            }
            // MIDI-Noten-Vorschau (Mini-Piano-Roll direkt im Clip wie in Ableton)
            if let Some(midi) = &c.midi
                && !midi.notes.is_empty()
            {
                let body = egui::Rect::from_min_max(
                    egui::pos2(r.left() + 2.0, r.top() + 18.0),
                    egui::pos2(r.right() - 2.0, r.bottom() - 3.0),
                );
                let min_p = midi.notes.iter().map(|n| n.pitch).min().unwrap_or(48);
                let max_p = midi.notes.iter().map(|n| n.pitch).max().unwrap_or(72);
                let span = f32::from((max_p.saturating_sub(min_p)).max(12)) + 2.0;
                let note_h = (body.height() / span).clamp(2.5, 6.0);
                let note_col = theme::mix(base, egui::Color32::WHITE, 0.55);
                let clip_view = painter.with_clip_rect(body.intersect(rect));
                for n in &midi.notes {
                    let ns = n.beat - offset;
                    let ne = ns + n.length_beats;
                    if ne <= 0.0 || ns >= len {
                        continue;
                    }
                    let nx0 = r.left() + (ns.max(0.0) as f32) * px;
                    let nx1 = r.left() + (ne.min(len) as f32) * px;
                    let rel = 1.0 - f32::from(n.pitch.saturating_sub(min_p)) / span;
                    let ny = body.top() + rel * (body.height() - note_h);
                    let nr = egui::Rect::from_min_max(
                        egui::pos2(nx0, ny),
                        egui::pos2(nx1.max(nx0 + 2.0), ny + note_h),
                    );
                    clip_view.rect_filled(nr, 1.5, note_col);
                }
            }
            // Fades als abgedunkelte Dreiecke + helle Hüllkurven-Diagonale.
            let shade = egui::Color32::from_black_alpha(135);
            let fade_line = egui::Stroke::new(1.5, theme::mix(base, egui::Color32::WHITE, 0.7));
            if c.audio.is_some() {
                let body_top = r.top() + 16.0;
                let h = r.bottom() - body_top;
                for (fade, curve, out) in [
                    (cv.fade_in_beats, cv.fade_in_curve, false),
                    (cv.fade_out_beats, cv.fade_out_curve, true),
                ] {
                    if fade <= 0.0 {
                        continue;
                    }
                    let w = (fade as f32 * px).min(r.width());
                    let x_start = if out { r.right() - w } else { r.left() };
                    let pts = crate::timeline::fade_points(curve, out, 16);
                    let at = |(x, g): (f32, f32)| egui::pos2(x_start + x * w, r.bottom() - g * h);
                    for pair in pts.windows(2) {
                        let (p, q) = (at(pair[0]), at(pair[1]));
                        let seg = egui::Rect::from_min_max(
                            egui::pos2(p.x, r.top()),
                            egui::pos2(q.x, (p.y + q.y) / 2.0),
                        );
                        painter.rect_filled(seg, 0.0, shade);
                    }
                    painter.add(egui::Shape::line(
                        pts.iter().map(|&p| at(p)).collect(),
                        fade_line,
                    ));
                }
                // Griffe an den oberen Ecken: ziehen ändert die Fade-Länge.
                let selected_audio = selected;
                for out in [false, true] {
                    let fade = if out {
                        cv.fade_out_beats
                    } else {
                        cv.fade_in_beats
                    };
                    let edge = (fade as f32 * px).clamp(10.0, (r.width() - 10.0).max(10.0));
                    let hx = if out {
                        r.right() - edge
                    } else {
                        r.left() + edge
                    };
                    let hp = egui::pos2(hx, body_top);
                    let hresp = ui
                        .interact(
                            egui::Rect::from_center_size(hp, egui::vec2(14.0, 14.0)),
                            ui.id().with(("fade", c.id.0, out)),
                            egui::Sense::drag(),
                        )
                        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
                        .on_hover_text(if out {
                            "Fade-out ziehen"
                        } else {
                            "Fade-in ziehen"
                        });
                    let hot = hresp.hovered() || hresp.dragged();
                    if hot || selected_audio {
                        painter.circle_filled(
                            hp,
                            if hot { 5.5 } else { 4.0 },
                            if hot {
                                theme::ACCENT
                            } else {
                                egui::Color32::WHITE
                            },
                        );
                    }
                    if hresp.drag_started() {
                        self.selected_clip = Some(c.id);
                        self.selected_clips = vec![c.id];
                    }
                    if hresp.dragged() {
                        let (mut fi, mut fo) = self
                            .fade_handle
                            .filter(|(id, _, _)| *id == c.id)
                            .map_or((c.fade_in_beats, c.fade_out_beats), |(_, a, b)| (a, b));
                        let d = f64::from(hresp.drag_delta().x / px);
                        if out {
                            fo = (fo - d).clamp(0.0, (len - fi).max(0.0));
                        } else {
                            fi = (fi + d).clamp(0.0, (len - fo).max(0.0));
                        }
                        self.fade_handle = Some((c.id, fi, fo));
                    }
                    if hresp.drag_stopped()
                        && let Some((id, fi, fo)) = self.fade_handle.take()
                    {
                        let round = |v: f64| (v * 100.0).round() / 100.0;
                        self.state.set_fades(id, round(fi), round(fo));
                    }
                }
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
                egui::FontId::proportional(11.5),
                theme::WELL,
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
                let snapped = self.snap.apply(c.start_beats + d);
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
            if cresp.double_clicked() && c.midi.is_some() {
                self.open_piano_roll(c.id);
            }
            cresp.on_hover_text(
                if c.midi.is_some() {
                    "Doppelklick: Piano-Roll · Ziehen: verschieben (auch auf andere Spur) · Ränder ziehen: kürzen · S: am Kopf schneiden · Strg+C/X/V: kopieren, ausschneiden, einfügen · Strg+D: verdoppeln · Entf: löschen"
                } else {
                    "Ziehen: verschieben (auch auf andere Spur) · Ränder ziehen: kürzen, mit Alt: Zeit dehnen · S: am Kopf schneiden · Strg+C/X/V: kopieren, ausschneiden, einfügen · Strg+D: verdoppeln · Entf: löschen"
                },
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
                let alt_stretch = c.audio.is_some()
                    && (self.stretch_drag.is_some_and(|(id, _, _)| id == c.id)
                        || (hresp.drag_started() && ui.input(|i| i.modifiers.alt)));
                if hresp.dragged() && alt_stretch {
                    let d = self.stretch_drag.map_or(0.0, |(_, _, d)| d)
                        + f64::from(hresp.drag_delta().x / px);
                    // Grenzen: Faktor im erlaubten Bereich, mindestens 1/4 Beat, links nicht
                    // vor den Anfang der Zeitachse.
                    let (lo, hi) = woa_model::STRETCH_RANGE;
                    let (min_len, max_len) = (
                        (c.length_beats * lo / c.stretch).max(0.25),
                        c.length_beats * hi / c.stretch,
                    );
                    let d = if left {
                        d.clamp(c.length_beats - max_len, c.length_beats - min_len)
                            .max(-c.start_beats)
                    } else {
                        d.clamp(min_len - c.length_beats, max_len - c.length_beats)
                    };
                    self.stretch_drag = Some((c.id, left, d));
                } else if hresp.dragged() {
                    let d =
                        self.trim.map_or(0.0, |(_, _, d)| d) + f64::from(hresp.drag_delta().x / px);
                    // Grenzen: mindestens 1/4 Beat lang, links nicht vor den Audio-Anfang,
                    // rechts nicht über das Audio-Ende (wenn bekannt). Die Zeitachse des
                    // Clips ist um `stretch` gedehnt, Versatz und Audio-Ende liegen in Quell-Beats.
                    let d = if left {
                        d.clamp(-c.offset_beats * c.stretch, c.length_beats - 0.25)
                    } else {
                        let max = audio_beats.map_or(f64::MAX, |a| {
                            (a * c.source_bpm(tempo) / tempo - c.offset_beats) * c.stretch
                                - c.length_beats
                        });
                        d.clamp(0.25 - c.length_beats, max.max(0.0))
                    };
                    self.trim = Some((c.id, left, d));
                }
                if hresp.drag_stopped()
                    && let Some((id, l, d)) = self.stretch_drag.take()
                {
                    let edge = if l {
                        c.start_beats + d
                    } else {
                        c.end_beats() + d
                    };
                    let edge = self.snap.apply(edge);
                    let length = if l {
                        c.end_beats() - edge
                    } else {
                        edge - c.start_beats
                    };
                    self.state.stretch_clip(id, length, l);
                } else if hresp.drag_stopped()
                    && let Some((id, l, d)) = self.trim.take()
                {
                    let d = self.snap_trim(c, l, d);
                    if l {
                        self.state.trim_clip(
                            id,
                            c.start_beats + d,
                            c.length_beats - d,
                            c.offset_beats + d / c.stretch,
                        );
                    } else {
                        self.state
                            .trim_clip(id, c.start_beats, c.length_beats + d, c.offset_beats);
                    }
                }
            }
        }
        if let Some((peaks, rate)) = self.live_peaks.get(&track) {
            draw_live_take(
                &painter,
                rect,
                px,
                self.rec_start,
                self.live_end_beats(),
                peaks,
                f64::from(*rate) * 60.0 / tempo / PEAK_BLOCK as f64,
            );
        }
        if let Some(p) = &self.player {
            let x = rect.left() + p.position_beats(tempo) as f32 * px;
            painter.vline(x, rect.y_range(), egui::Stroke::new(2.0, theme::ACCENT));
        }
        let is_midi_track = self
            .state
            .project()
            .track(track)
            .is_some_and(|t| t.kind == woa_model::TrackKind::Midi);
        if resp.double_clicked()
            && is_midi_track
            && let Some(pos) = resp.interact_pointer_pos()
        {
            let beat = f64::from(((pos.x - rect.left()) / px).max(0.0));
            let bar_start = (beat / 4.0).floor() * 4.0;
            if let Some(clip) = self
                .state
                .create_empty_midi_clip(Some(track), bar_start, 16.0)
            {
                self.open_piano_roll(clip);
            }
        } else if resp.clicked() {
            self.selected_track = Some(track);
            self.selected_clip = None;
            self.selected_clips.clear();
            if let (Some(pos), Some(p)) = (resp.interact_pointer_pos(), self.player.as_mut()) {
                p.seek_beats(f64::from((pos.x - rect.left()) / px), tempo);
            }
        }
    }

    /// Bearbeitungszeile für den gewählten Clip: Fades, Schneiden, Löschen, Piano-Roll.
    fn clip_bar(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.selected_clip else {
            ui.horizontal(|ui| {
                theme::status_pill(ui, "CLIP-INSPEKTOR", theme::DIM, false);
                ui.label(
                    egui::RichText::new(
                        "Clip anklicken zum Bearbeiten  ·  Doppelklick in MIDI-Spur: neuer MIDI-Clip  ·  S: Schneiden am Kopf  ·  Strg+C/V/D: Kopieren, Einfügen, Verdoppeln  ·  Audio-Datei hineinziehen  ·  Strg+Mausrad: Zoom",
                    )
                    .small()
                    .color(theme::DIM),
                );
            });
            return;
        };
        let Some((_, c)) = self.state.project().clip(id) else {
            self.selected_clip = None;
            self.selected_clips.clear();
            return;
        };
        let c = c.clone();
        ui.horizontal(|ui| {
            theme::status_pill(
                ui,
                if c.midi.is_some() {
                    "MIDI-CLIP"
                } else {
                    "AUDIO-CLIP"
                },
                if c.midi.is_some() {
                    theme::CUE_CYAN
                } else {
                    theme::ACCENT
                },
                true,
            );
            ui.strong(&c.name);
            ui.separator();
            if c.midi.is_some()
                && ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("🎹 Piano-Roll öffnen")
                                .strong()
                                .color(theme::CUE_CYAN),
                        )
                        .fill(theme::WELL),
                    )
                    .clicked()
            {
                self.open_piano_roll(id);
            }
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
            if ui.button("⧉ Duplizieren (Strg+D)").clicked() {
                self.duplicate_selected();
            }
            if ui.button("Kopieren").on_hover_text("Strg+C").clicked() {
                self.copy_selected();
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
        if c.audio.is_some() {
            self.clip_shape_bar(ui, &c);
        }
    }

    /// Zweite Zeile des Clip-Inspektors (Paket 21): Fade-Kurven, Clip-Gain, Normalisieren.
    fn clip_shape_bar(&mut self, ui: &mut egui::Ui, c: &woa_model::Clip) {
        use woa_model::FadeCurve;
        let id = c.id;
        ui.horizontal(|ui| {
            let (mut cin, mut cout) = (c.fade_in_curve, c.fade_out_curve);
            for (label, cur, salt) in [
                ("Einblend-Kurve", &mut cin, 0u8),
                ("Ausblend-Kurve", &mut cout, 1u8),
            ] {
                egui::ComboBox::from_id_salt(("fade_curve", id.0, salt))
                    .selected_text(format!("{label}: {}", cur.label()))
                    .show_ui(ui, |ui| {
                        for k in FadeCurve::ALL {
                            ui.selectable_value(cur, k, k.label());
                        }
                    });
            }
            if (cin, cout) != (c.fade_in_curve, c.fade_out_curve) {
                self.state.set_fade_curves(id, cin, cout);
            }
            ui.separator();
            let mut g = match self.gain_preview {
                Some((pid, pg)) if pid == id => pg,
                _ => c.gain_db,
            };
            let r = ui.add(
                egui::DragValue::new(&mut g)
                    .range(woa_model::MIN_GAIN_DB..=woa_model::MAX_GAIN_DB)
                    .speed(0.1)
                    .fixed_decimals(1)
                    .prefix("Clip-Gain ")
                    .suffix(" dB"),
            );
            if r.double_clicked() {
                g = 0.0;
            }
            if r.dragged() || r.has_focus() {
                if g != c.gain_db {
                    self.gain_preview = Some((id, g));
                }
            } else {
                self.gain_preview = None;
                if g != c.gain_db {
                    self.state.set_clip_gain(id, g);
                }
            }
            if ui
                .button("Normalisieren")
                .on_hover_text(
                    "Clip-Gain so setzen, dass die lauteste Stelle des Clips bei -1 dBFS liegt",
                )
                .clicked()
            {
                self.normalize_selected(c);
            }
            ui.label(
                egui::RichText::new(
                    "Überlappende Clips blenden automatisch über (gleiche Leistung).",
                )
                .small()
                .color(theme::DIM),
            );
        });
        self.clip_time_bar(ui, c);
    }

    /// Dritte Zeile des Clip-Inspektors (Paket 23): Dehnung, Tonhöhe, Tempo folgen.
    fn clip_time_bar(&mut self, ui: &mut egui::Ui, c: &woa_model::Clip) {
        let id = c.id;
        ui.horizontal(|ui| {
            theme::status_pill(ui, "ZEIT", theme::CUE_CYAN, c.is_processed(self.state.project().tempo_bpm));
            // Dehnung in Prozent der Länge: 110 = zehn Prozent länger, Tonhöhe bleibt.
            let mut pct = match self.stretch_edit {
                Some((cid, v)) if cid == id => v,
                _ => c.stretch * 100.0,
            };
            let r = ui
                .add(
                    egui::DragValue::new(&mut pct)
                        .range(woa_model::STRETCH_RANGE.0 * 100.0..=woa_model::STRETCH_RANGE.1 * 100.0)
                        .speed(0.5)
                        .fixed_decimals(1)
                        .prefix("Dehnung ")
                        .suffix(" %"),
                )
                .on_hover_text("Länge des Tons in Prozent, die Tonhöhe bleibt. Doppelklick: 100 %. Am Clip-Rand mit Alt ziehen geht auch.");
            if r.double_clicked() {
                pct = 100.0;
            }
            if r.dragged() || r.has_focus() {
                if (pct - c.stretch * 100.0).abs() > 1e-9 {
                    self.stretch_edit = Some((id, pct));
                }
            } else {
                self.stretch_edit = None;
                if (pct - c.stretch * 100.0).abs() > 1e-9 {
                    self.state.set_clip_stretch(id, pct / 100.0);
                }
            }
            ui.separator();
            let mut st = match self.pitch_edit {
                Some((cid, v)) if cid == id => v,
                _ => c.pitch_semitones,
            };
            let r = ui
                .add(
                    egui::DragValue::new(&mut st)
                        .range(woa_model::PITCH_RANGE.0..=woa_model::PITCH_RANGE.1)
                        .speed(0.05)
                        .fixed_decimals(1)
                        .prefix("Tonhöhe ")
                        .suffix(" Halbtöne"),
                )
                .on_hover_text("Tonhöhe in Halbtönen, die Länge bleibt. Doppelklick: 0.");
            if r.double_clicked() {
                st = 0.0;
            }
            if r.dragged() || r.has_focus() {
                if st != c.pitch_semitones {
                    self.pitch_edit = Some((id, st));
                }
            } else {
                self.pitch_edit = None;
                if st != c.pitch_semitones {
                    self.state.set_clip_pitch(id, st);
                }
            }
            ui.separator();
            let mut follow = c.follow_tempo_bpm.is_some();
            if ui
                .checkbox(&mut follow, "Tempo folgen")
                .on_hover_text(
                    "Ändert sich das Projekt-Tempo, wird der Ton gedehnt statt verschoben: er bleibt auf dem Raster, die Tonhöhe bleibt.",
                )
                .changed()
            {
                self.state.set_clip_follow_tempo(id, follow);
            }
            if let Some(b) = c.follow_tempo_bpm {
                ui.label(
                    egui::RichText::new(format!("Ton gehört zu {b:.1} BPM"))
                        .small()
                        .color(theme::DIM),
                );
            }
        });
    }

    /// Setzt den Gain des Clips so, dass seine Spitze bei -1 dBFS liegt.
    fn normalize_selected(&mut self, c: &woa_model::Clip) {
        let tempo = self.state.project().tempo_bpm;
        let file = c
            .audio
            .as_deref()
            .and_then(|rel| self.state.resolve_audio(rel));
        let peak = match (file, self.player.as_mut()) {
            (Some(file), Some(p)) => p.peaks(&file).map(|pk| {
                crate::timeline::region_peak(
                    &pk,
                    p.peaks_per_beat(tempo),
                    c.offset_beats * tempo / c.source_bpm(tempo),
                    c.source_length_beats() * tempo / c.source_bpm(tempo),
                )
            }),
            // Ohne Wiedergabe-Engine (Tests): Datei direkt lesen, sample-genau.
            (Some(file), None) => woa_engine::clip::load_wav_mono(&file, 48_000)
                .ok()
                .map(|s| {
                    let per_beat = 48_000.0 * 60.0 / c.source_bpm(tempo);
                    let a = ((c.offset_beats * per_beat) as usize).min(s.len());
                    let b = (((c.offset_beats + c.source_length_beats()) * per_beat).ceil()
                        as usize)
                        .clamp(a, s.len());
                    s[a..b].iter().fold(0.0f32, |m, v| m.max(v.abs()))
                }),
            _ => None,
        };
        let done = peak.is_some_and(|pk| {
            self.state
                .normalize_clip(c.id, pk, crate::timeline::NORMALIZE_TARGET_DB)
        });
        if !done {
            self.state.status = Status::Info(
                "Normalisieren nicht möglich (Stille oder keine Wiedergabe-Engine)".into(),
            );
        }
    }

    /// Rastet einen gezogenen Clip-Rand ein: die neue Kante liegt auf dem Raster.
    fn snap_trim(&self, c: &woa_model::Clip, left: bool, d: f64) -> f64 {
        let snapped = if left {
            self.snap.apply(c.start_beats + d) - c.start_beats
        } else {
            self.snap.apply(c.end_beats() + d) - c.end_beats()
        };
        if left {
            snapped.clamp(-c.offset_beats * c.stretch, c.length_beats - 0.25)
        } else {
            snapped.max(0.25 - c.length_beats)
        }
    }

    /// Ids der markierten Clips (Mehrfachauswahl oder der eine gewählte).
    fn selection_ids(&self) -> Vec<ClipId> {
        let mut ids = self.selected_clips.clone();
        if let Some(c) = self.selected_clip
            && !ids.contains(&c)
        {
            ids.push(c);
        }
        ids
    }

    /// Strg+C: markierte Clips in die Zwischenablage.
    fn copy_selected(&mut self) {
        let items = self.state.copy_clips(&self.selection_ids());
        if items.is_empty() {
            return;
        }
        self.state.status = Status::Info(format!("{} Clip(s) kopiert", items.len()));
        self.clipboard = Clipboard { items };
    }

    /// Strg+V: Zwischenablage am Abspielkopf einfügen, auf die gewählte Spur, wenn sie passt.
    fn paste_at_playhead(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        let tempo = self.state.project().tempo_bpm;
        let at = self
            .player
            .as_ref()
            .map_or(0.0, |p| p.position_beats(tempo));
        let items = self.clipboard.items.clone();
        let ids = self
            .state
            .paste_clips(&items, self.snap.apply(at), self.selected_track);
        self.select_new(ids);
    }

    /// Strg+D: markierte Clips direkt hinter sich verdoppeln.
    fn duplicate_selected(&mut self) {
        let ids = self.state.duplicate_clips(&self.selection_ids());
        self.select_new(ids);
    }

    fn select_new(&mut self, ids: Vec<ClipId>) {
        if let Some(&last) = ids.last() {
            self.selected_clip = Some(last);
            self.selected_track = self.state.project().clip(last).map(|(t, _)| t.id);
            self.selected_clips = ids;
        }
    }

    /// Audiodateien, die aufs Fenster gezogen wurden, als Clips ablegen: die erste auf die
    /// Spur unter dem Zeiger bei der Zeigerposition, jede weitere auf eine neue Spur
    /// an derselben Stelle.
    fn drop_files(&mut self, ctx: &egui::Context) {
        let files: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if files.is_empty() {
            return;
        }
        self.place_audio(&files, ctx.pointer_latest_pos());
    }

    /// Das Bibliotheks-Fenster zeichnen und seine Wünsche ausführen: Vorhören im Kopfhörer
    /// (derselbe PFL-Weg wie im Radio, nie auf Sendung) und Ablegen gezogener Titel.
    fn library_window(&mut self, ctx: &egui::Context) {
        let pfl = self.radio.as_ref().is_some_and(RadioDesk::pfl_available);
        let out = self.library.show(ctx, pfl);
        if let Some(path) = out.preview {
            match self.radio.as_ref() {
                Some(r) => r.preview_file(path),
                None => {
                    self.state.status = Status::Error(
                        "Vorhören braucht den Radio-Modus mit Kopfhörer-Ausgang (Einstellungen → Vorhören)".into(),
                    );
                }
            }
        }
        if out.stop_preview
            && let Some(r) = self.radio.as_ref()
        {
            r.preview_stop();
        }
        if let Some((path, pos)) = out.drop {
            if self.config.mode.uses_desk() {
                let done = self
                    .radio
                    .as_mut()
                    .is_some_and(|r| r.drop_library_path(path, pos));
                if !done {
                    self.state.status = Status::Info(
                        "Zum Ablegen auf die Playlist oder ein Cart-Pad ziehen".into(),
                    );
                }
            } else {
                self.place_audio(&[path], Some(pos));
            }
        }
    }

    /// Legt Audiodateien als Clips ab (Ziehen aufs Fenster oder aus der Bibliothek).
    fn place_audio(&mut self, files: &[PathBuf], pos: Option<egui::Pos2>) {
        let hit = pos.and_then(|p| self.track_rects.iter().find(|(_, r)| r.contains(p)));
        let (track, at) = match (pos, hit) {
            (Some(p), Some((t, r))) => (
                Some(*t),
                self.snap.apply(f64::from((p.x - r.left()) / self.zoom)),
            ),
            _ => {
                let tempo = self.state.project().tempo_bpm;
                (
                    self.selected_track,
                    self.player
                        .as_ref()
                        .map_or(0.0, |p| p.position_beats(tempo)),
                )
            }
        };
        for (n, f) in files.iter().enumerate() {
            self.import_audio_file(f, if n == 0 { track } else { None }, at);
        }
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

    /// Öffnet das Export-Fenster (Format, Stems); gerechnet wird nach dem Klick darin.
    fn export(&mut self) {
        self.export_dialog.open_now();
    }

    fn export_window(&mut self, ctx: &egui::Context) {
        if let Some(req) = self.export_dialog.show(ctx) {
            self.run_export(req);
        }
    }

    fn run_export(&mut self, req: crate::export_dialog::ExportRequest) {
        let ext = req.spec.format.extension();
        self.capture_plugins();
        let project = self.state.project().clone();
        let name = project.meta.name.clone();
        let status = match req.stems {
            None => self.export_file(&project, &name, ext, &req.spec),
            Some(opts) => self.export_stem_folder(&project, &name, &req.spec, opts),
        };
        if let Some(s) = status {
            self.state.status = s;
        }
    }

    /// Text zur Lautheit für die Statuszeile.
    fn loudness_note(norm: Option<woa_engine::loudness::Normalized>) -> String {
        match norm {
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
        }
    }

    fn export_file(
        &mut self,
        project: &woa_model::Project,
        name: &str,
        ext: &str,
        spec: &woa_engine::export::ExportSpec,
    ) -> Option<Status> {
        let out = rfd::FileDialog::new()
            .set_title("Exportieren")
            .set_file_name(format!("{name}.{ext}"))
            .add_filter(spec.format.label(), &[ext])
            .save_file()?;
        let out = if out.extension().is_none() {
            out.with_extension(ext)
        } else {
            out
        };
        let state = &self.state;
        let result = match self.player.as_mut() {
            Some(p) => p.export(
                project,
                &|rel| state.resolve_audio(rel),
                &out,
                spec,
                self.export_lufs,
            ),
            None => Err("keine Audio-Engine".into()),
        };
        Some(match result {
            Ok((secs, norm)) => {
                tracing::info!(seconds = secs, ?norm, "export done");
                Status::Info(format!(
                    "Exportiert: {} ({secs:.1} s{})",
                    out.display(),
                    Self::loudness_note(norm)
                ))
            }
            Err(e) => {
                tracing::error!(error = %e, "export failed");
                Status::Error(format!("Export fehlgeschlagen: {e}"))
            }
        })
    }

    fn export_stem_folder(
        &mut self,
        project: &woa_model::Project,
        name: &str,
        spec: &woa_engine::export::ExportSpec,
        opts: crate::export_dialog::StemOptions,
    ) -> Option<Status> {
        let parent = rfd::FileDialog::new()
            .set_title("Ordner für die Stems wählen")
            .pick_folder()?;
        // Eigener Unterordner „<Projekt> Stems“, damit nichts Fremdes überschrieben wird.
        let dir = parent.join(format!("{} Stems", woa_engine::stems::sanitize_name(name)));
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::error!(error = %e, "stem folder not created");
            return Some(Status::Error(format!(
                "Ordner {} nicht anlegbar: {e}",
                dir.display()
            )));
        }
        let state = &self.state;
        let result = match self.player.as_mut() {
            Some(p) => p.export_stems(
                project,
                &|rel| state.resolve_audio(rel),
                &dir,
                spec,
                opts,
                self.export_lufs,
            ),
            None => Err("keine Audio-Engine".into()),
        };
        Some(match result {
            Ok(done) => {
                tracing::info!(
                    files = done.files,
                    seconds = done.seconds,
                    "stem export done"
                );
                let skipped = if done.skipped.is_empty() {
                    String::new()
                } else {
                    format!(", ohne stumme Spuren: {}", done.skipped.join(", "))
                };
                Status::Info(format!(
                    "{} Dateien in {} ({:.1} s{}{skipped})",
                    done.files,
                    dir.display(),
                    done.seconds,
                    Self::loudness_note(done.norm)
                ))
            }
            Err(e) => {
                tracing::error!(error = %e, "stem export failed");
                // Ein leerer Ordner, den dieser Lauf angelegt hat, bleibt nicht liegen.
                let _ = std::fs::remove_dir(&dir);
                Status::Error(format!("Export fehlgeschlagen: {e}"))
            }
        })
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

    /// Sets where setting changes are saved (`config.json`).
    pub fn set_config_path(&mut self, path: PathBuf) {
        self.config_path = Some(path);
    }

    fn set_telemetry(&mut self, on: bool) {
        self.config.telemetry_enabled = Some(on);
        if let Some(p) = &self.config_path
            && let Err(e) = self.config.save(p)
        {
            tracing::warn!(error = %e, "config not saved");
        }
    }

    /// First start: ask once whether anonymous error codes may be sent (opt-in).
    fn telemetry_window(&mut self, ctx: &egui::Context) {
        if self.config.telemetry_enabled.is_some() || self.photo.is_some() {
            return;
        }
        let mut choice = None;
        egui::Window::new("Fehlercodes senden?")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(TELEMETRY_HINT);
                ui.horizontal(|ui| {
                    if ui.button("Ja, anonym senden").clicked() {
                        choice = Some(true);
                    }
                    if ui.button("Nein").clicked() {
                        choice = Some(false);
                    }
                });
            });
        if let Some(on) = choice {
            self.set_telemetry(on);
        }
    }

    fn feedback_window(&mut self, ctx: &egui::Context) {
        if matches!(self.feedback.phase, Phase::Closed | Phase::Capturing) {
            return;
        }
        let now = Instant::now();
        self.feedback.tick(now);
        if let Some(left) = self.feedback.thanks_left(now) {
            ctx.request_repaint_after(left.min(std::time::Duration::from_millis(100)));
        }
        let mut close = false;
        // Dim the app behind the dialog so it reads as modal.
        egui::Area::new(egui::Id::new("feedback-scrim"))
            .order(egui::Order::Middle)
            .fixed_pos(egui::Pos2::ZERO)
            .interactable(true)
            .show(ctx, |ui| {
                let screen = ctx.content_rect();
                ui.painter()
                    .rect_filled(screen, 0.0, egui::Color32::from_black_alpha(140));
                ui.allocate_rect(screen, egui::Sense::click());
            });
        egui::Window::new("Feedback")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .fixed_size(egui::vec2(420.0, 0.0))
            .frame(
                theme::card_frame()
                    .inner_margin(egui::Margin::same(20))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 8],
                        blur: 28,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(160),
                    }),
            )
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("Feedback")
                        .size(20.0)
                        .strong()
                        .color(theme::TEXT_BRIGHT),
                );
                ui.add_space(4.0);
                match self.feedback.phase.clone() {
                    Phase::Editing => {
                        ui.label(
                            egui::RichText::new("Was ist passiert, was wünschst du dir?")
                                .color(theme::DIM),
                        );
                        ui.add_space(10.0);
                        ui.add(
                            egui::TextEdit::multiline(&mut self.feedback.text)
                                .desired_rows(6)
                                .desired_width(f32::INFINITY)
                                .hint_text("Schreib einfach los …"),
                        );
                        ui.add_space(6.0);
                        let has = self.feedback.screenshot.is_some();
                        ui.add_enabled(
                            has,
                            egui::Checkbox::new(&mut self.feedback.attach, "Screenshot anhängen"),
                        );
                        ui.add_space(12.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let send = egui::Button::new(
                                egui::RichText::new("Senden").strong().color(theme::WELL),
                            )
                            .fill(theme::ACCENT)
                            .min_size(egui::vec2(96.0, 30.0));
                            if ui.add(send).clicked() {
                                self.feedback.send(&self.config);
                            }
                            let cancel =
                                egui::Button::new("Abbrechen").min_size(egui::vec2(96.0, 30.0));
                            if ui.add(cancel).clicked() {
                                close = true;
                            }
                        });
                    }
                    Phase::Sending => {
                        ui.add_space(16.0);
                        ui.vertical_centered(|ui| {
                            ui.spinner();
                            ui.label(egui::RichText::new("Wird gesendet …").color(theme::DIM));
                        });
                        ui.add_space(16.0);
                    }
                    Phase::Done(msg) if self.feedback.is_thanks() => {
                        ui.add_space(12.0);
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new("✔").size(40.0).color(theme::OK_GREEN));
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(msg)
                                    .size(16.0)
                                    .strong()
                                    .color(theme::TEXT_BRIGHT),
                            );
                            ui.label(
                                egui::RichText::new("Das Fenster schließt sich gleich.")
                                    .small()
                                    .color(theme::DIM),
                            );
                        });
                        // Thin bar that runs out with the countdown.
                        let left = self.feedback.thanks_left(now).unwrap_or_default();
                        let frac =
                            left.as_secs_f32() / crate::feedback::THANKS_VISIBLE.as_secs_f32();
                        ui.add_space(10.0);
                        let (r, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 3.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(r, 1.5, theme::WELL);
                        let mut bar = r;
                        bar.set_width(r.width() * frac.clamp(0.0, 1.0));
                        ui.painter().rect_filled(bar, 1.5, theme::OK_GREEN);
                    }
                    Phase::Done(msg) => {
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(msg).color(theme::ON_AIR));
                        ui.add_space(12.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new("Schließen").min_size(egui::vec2(96.0, 30.0)),
                                )
                                .clicked()
                            {
                                close = true;
                            }
                        });
                    }
                    Phase::Closed | Phase::Capturing => {}
                }
            });
        if close {
            self.feedback.close();
        }
    }
}

impl WoaApp {
    /// Stelle der Spur im Projekt (für Controller-Ziele, die Spuren ab 0 zählen).
    fn track_index(&self, id: TrackId) -> Option<usize> {
        self.state.project().tracks.iter().position(|t| t.id == id)
    }

    fn controller_window(&mut self, ctx: &egui::Context) {
        if self.controls.window(ctx, &mut self.config) {
            self.save_config();
        }
    }

    /// `WOA_REPAINT_DEBUG=1`: alle 5 s eine Logzeile mit Bildern je Sekunde und den
    /// Auslösern der Repaints (Fehlersuche bei CPU-Last, #56).
    fn repaint_debug(&mut self, ctx: &egui::Context) {
        if std::env::var_os("WOA_REPAINT_DEBUG").is_none() {
            return;
        }
        self.frames.0 += 1;
        let dt = self.frames.1.elapsed().as_secs_f32();
        if dt >= 5.0 {
            let causes: Vec<String> = ctx.repaint_causes().iter().map(|c| c.to_string()).collect();
            tracing::info!(fps = self.frames.0 as f32 / dt, causes = ?causes, "repaint debug");
            self.frames = (0, Instant::now());
        }
    }
}

impl eframe::App for WoaApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.repaint_debug(ctx);
        self.drain_midi_takes();
        if self.config.mode.uses_desk() {
            self.ensure_radio();
        }
        // Im Radio-Modus spielt nur der Sendemixer; der Player-Strom ruht (#51).
        if let Some(p) = self.player.as_mut() {
            p.set_suspended(self.config.mode.uses_desk());
        }
        let player_active = self.player.as_ref().is_some_and(Player::is_active);
        if let Some(r) = self.radio.as_mut() {
            r.tick(player_active);
            // Zeit-Events: Tabelle laden, Zone von der Stundenuhr, an den Sende-Thread reichen;
            // der AutoDJ plant rückwärts auf das nächste harte Event.
            self.event_editor.ensure_loaded();
            self.event_editor.set_zone(
                self.clock_editor.schedule.utc_offset_min,
                self.clock_editor.schedule.auto_dst,
            );
            if let Some(t) = self.event_editor.take_push() {
                self.clock_editor.set_events(t.clone());
                r.push_events(t);
            }
            // AutoDJ: Historie nachtragen und die Playlist hinten auffüllen.
            let add = self
                .clock_editor
                .autodj_tick(r.view(), crate::clock_editor::now_unix());
            // Auswahlen aus `moderation` werden Voice-Tracks, der Rest hängt hinten an; die
            // Reihenfolge bleibt, damit die Stimme vor ihrem Titel steht.
            for path in add {
                if self.clock_editor.take_voice_pick(&path) {
                    r.add_voice_file(path, None);
                } else {
                    r.append_files(vec![path]);
                }
            }
        }
        self.clock_editor.tick_save();
        self.event_editor.tick_save();
        if self.config.mode == Mode::Party {
            let cmds = crate::party::PartyView::shortcuts(ctx);
            if let Some(r) = self.radio.as_mut() {
                self.party.run(cmds, r, ctx);
            }
        } else if self.config.mode == Mode::Radio {
            if let Some(r) = self.radio.as_mut() {
                r.shortcuts(ctx);
            }
        } else {
            self.shortcuts(ctx);
        }
        self.take_screenshot(ctx);
        if self.config.mode == Mode::Produzent {
            self.drop_files(ctx);
        }
        self.feedback.poll();
        if self.state.autosave_due(Instant::now()) {
            self.capture_plugins();
        }
        self.state.tick_autosave(Instant::now());
        let title = self.state.title();
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
        // Keep the autosave timer and the feedback poll ticking while idle; the level
        // meter needs a faster beat while recording.
        self.plugin_frame(ctx);
        let project = self.state.project().clone();
        self.midi.poll(Instant::now());
        self.controller_frame(ctx);
        let live = self.live_midi_track();
        let state = &self.state;
        let loop_beats = self
            .loop_enabled
            .then(|| {
                let s = f64::from(self.loop_start_bar.saturating_sub(1) * 4);
                let e = f64::from(self.loop_end_bar.saturating_sub(1) * 4);
                (s < e).then_some((s, e))
            })
            .flatten();
        if let Some(p) = self.player.as_mut() {
            p.sync(&project, &|rel| state.resolve_audio(rel), live);
            p.set_loop_beats(loop_beats, project.tempo_bpm);
        }
        self.stop_at_audio_end();
        self.drain_live_peaks();
        self.analysis_frame();
        let busy = self.recording() || self.player.as_ref().is_some_and(Player::is_playing);
        // Radio: 5 Bilder/s, solange ein Titel läuft (Uhr, Pegel), sonst 2/s. Vorher 10/s
        // dauerhaft, 34 % CPU in Ruhe (Paket 8).
        let radio_playing = self.radio.as_ref().is_some_and(RadioDesk::is_on_air);
        let midi_on = matches!(self.midi.status(), woa_io::midi::MidiStatus::Connected(_))
            || matches!(
                self.controls.status(),
                woa_io::midi::MidiStatus::Connected(_)
            );
        let beat = if busy {
            40
        } else if midi_on {
            // Aktivitätslampe: 5 Bilder/s, solange ein Gerät verbunden ist.
            200
        } else if self.config.mode.uses_desk() && radio_playing {
            200
        } else {
            500
        };
        ctx.request_repaint_after(std::time::Duration::from_millis(beat));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.photo(&ctx);
        if !self.themed {
            theme::apply(&ctx);
            self.themed = true;
        }
        egui::Panel::top("menu")
            .frame(
                egui::Frame::new()
                    .fill(theme::PANEL_HEADER)
                    .inner_margin(egui::Margin::symmetric(12, 5)),
            )
            .show(ui, |ui| self.menu(ui, &ctx));
        if self.config.mode == Mode::Party
            && let Some(radio) = self.radio.as_mut()
        {
            let mut cmds = Vec::new();
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::new()
                        .fill(theme::BG)
                        .inner_margin(egui::Margin::symmetric(14, 12)),
                )
                .show(ui, |ui| cmds = self.party.ui(ui, radio));
            self.party.run(cmds, radio, &ctx);
            return;
        }
        if self.config.mode == Mode::Radio
            && let Some(radio) = self.radio.as_mut()
        {
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::new()
                        .fill(theme::BG)
                        .inner_margin(egui::Margin::symmetric(14, 12)),
                )
                .show(ui, |ui| radio.ui(ui));
            radio.cue_window(&ctx);
            radio.voice_window(&ctx);
            self.library_window(&ctx);
            self.clock_editor.show(&ctx);
            self.event_editor
                .show(&ctx, self.radio.as_ref().map(|r| r.view()));
            self.export_window(&ctx);
            self.controller_window(&ctx);
            self.feedback_window(&ctx);
            self.telemetry_window(&ctx);
            return;
        }
        egui::Panel::top("transport")
            .frame(
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(12, 7)),
            )
            .show(ui, |ui| self.transport_bar(ui));
        egui::Panel::top("clipbar")
            .frame(
                egui::Frame::new()
                    .fill(theme::mix(theme::PANEL, theme::BG, 0.5))
                    .inner_margin(egui::Margin::symmetric(12, 5)),
            )
            .show(ui, |ui| ui.vertical(|ui| self.clip_bar(ui)));
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(theme::WELL)
                    .inner_margin(egui::Margin::symmetric(12, 4)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    match &self.state.status {
                        Status::Idle => {
                            ui.label(
                                egui::RichText::new("● Engine bereit")
                                    .small()
                                    .color(theme::OK_GREEN),
                            );
                        }
                        Status::Info(m) => {
                            ui.label(egui::RichText::new(m).small().color(theme::TEXT_BRIGHT));
                        }
                        Status::Error(m) => {
                            ui.colored_label(theme::ON_AIR, m);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("WOA {}", env!("CARGO_PKG_VERSION")))
                                .small()
                                .monospace()
                                .color(theme::DIM),
                        );
                    });
                });
            });
        self.studio_dock_panel(ui);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::symmetric(12, 10)),
            )
            .show(ui, |ui| self.tracks(ui));
        self.piano_roll_window(&ctx);
        self.analysis_window(&ctx);
        self.library_window(&ctx);
        self.clock_editor.show(&ctx);
        self.event_editor
            .show(&ctx, self.radio.as_ref().map(|r| r.view()));
        self.export_window(&ctx);
        self.controller_window(&ctx);
        self.feedback_window(&ctx);
        self.telemetry_window(&ctx);
    }
}

impl WoaApp {
    /// Plugin-Fenster (Paket 32), jedes Bild: Reglerbewegungen aus den Fenstern werden ein
    /// Rückgängig-Schritt (ein Regler: sein neuer Wert; ein Preset-Wechsel: der ganze Zustand),
    /// die Plugins bekommen ihre Zeitgeber, und nach „Rückgängig“ gleicht das Rack die laufenden
    /// Plugins mit dem Projekt ab.
    fn plugin_frame(&mut self, ctx: &egui::Context) {
        let Some(player) = self.player.as_mut() else {
            return;
        };
        let edits = player.pump_plugins();
        for (insert, edit) in edits {
            let Some(track) = self
                .state
                .project()
                .tracks
                .iter()
                .find(|t| t.plugin(insert).is_some())
                .map(|t| t.id)
            else {
                continue;
            };
            match edit {
                woa_plugins::ParamEdit::Param { id, new, .. } => {
                    self.state.set_plugin_param(track, insert, id, new);
                }
                woa_plugins::ParamEdit::WholeState => {
                    if let Some(cmd) = player.capture_plugin_slot(self.state.project(), insert) {
                        self.state.apply_plugin_state(vec![cmd]);
                    }
                }
            }
        }
        player.reconcile_plugins(self.state.project());
        for insert in player.new_plugin_failures() {
            let name = self
                .state
                .project()
                .tracks
                .iter()
                .find_map(|t| t.plugin(insert))
                .map_or_else(|| "Plugin".to_string(), |s| s.name.clone());
            self.state.status = Status::Error(format!(
                "{name}: Plugin abgestürzt – neu laden. Der Rest läuft weiter."
            ));
        }
        if player.has_isolated_plugins() {
            // Die Aufsicht über die Plugin-Prozesse läuft in diesem Takt.
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        if player.any_plugin_gui_open() {
            // Offene Plugin-Fenster brauchen den Takt für Zeitgeber und Reglermeldungen.
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }

    /// Holt je Bild die Samples des Abgriffs fürs Analyse-Fenster. Ist es zu, sind alle
    /// Abgriffe aus.
    fn analysis_frame(&mut self) {
        let Some(player) = self.player.as_mut() else {
            return;
        };
        if !self.analysis_open {
            player.set_tap_source(None);
            return;
        }
        let rate = player.sample_rate();
        let view = self
            .analysis
            .get_or_insert_with(|| crate::analysis::AnalysisView::new(rate));
        view.set_rate(rate);
        player.set_tap_source(Some(view.source()));
        let (l, r) = &mut self.analysis_buf;
        l.clear();
        r.clear();
        view.add_lost(player.drain_tap(l, r));
        if l.is_empty() {
            view.idle();
        } else {
            view.feed(l, r);
        }
    }

    /// Das schwebende Analyse-Fenster.
    fn analysis_window(&mut self, ctx: &egui::Context) {
        if !self.analysis_open {
            return;
        }
        let tracks: Vec<(TrackId, String)> = self
            .state
            .project()
            .tracks
            .iter()
            .map(|t| (t.id, t.name.clone()))
            .collect();
        let playing = self.player.as_ref().is_some_and(Player::is_playing);
        let mut open = true;
        if let Some(view) = self.analysis.as_mut() {
            egui::Window::new("Analyse")
                .open(&mut open)
                .default_size([760.0, 520.0])
                .min_size([420.0, 340.0])
                .resizable(true)
                .frame(theme::card_frame())
                .show(ctx, |ui| view.ui(ui, &tracks, playing));
        }
        self.analysis_open = open;
    }

    /// Unteres Ableton-Style Studio-Dock: Umschaltbar zwischen Studio-Mixer-Konsole und
    /// angedockter Piano-Roll (oder eingeklappt für maximale Zeitleisten-Höhe).
    fn studio_dock_panel(&mut self, ui: &mut egui::Ui) {
        let is_open = self.studio_dock != StudioDockTab::Closed;
        let panel = egui::Panel::bottom("studio_dock")
            .resizable(is_open)
            .default_size(if is_open { 330.0 } else { 32.0 })
            .size_range(if is_open { 175.0..=440.0 } else { 32.0..=32.0 })
            .frame(
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .stroke(egui::Stroke::new(1.0, theme::BORDER))
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            );
        panel.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                theme::status_pill(ui, "STUDIO-DOCK", theme::ACCENT, is_open);

                let mixer_active = self.studio_dock == StudioDockTab::Mixer;
                let mixer_btn = egui::Button::new(
                    egui::RichText::new("🎚 Studio-Mixer")
                        .small()
                        .strong()
                        .color(if mixer_active {
                            theme::WELL
                        } else {
                            theme::TEXT_BRIGHT
                        }),
                )
                .fill(if mixer_active {
                    theme::ACCENT
                } else {
                    theme::WELL
                });
                if ui
                    .add(mixer_btn)
                    .on_hover_text("Vertikale Mischpult-Kanalzüge für alle Spuren")
                    .clicked()
                {
                    self.studio_dock = if mixer_active {
                        StudioDockTab::Closed
                    } else {
                        StudioDockTab::Mixer
                    };
                }

                let pr_active = self.studio_dock == StudioDockTab::PianoRoll;
                let pr_btn = egui::Button::new(
                    egui::RichText::new("🎹 Piano-Roll (Clip-Editor)")
                        .small()
                        .strong()
                        .color(if pr_active {
                            theme::WELL
                        } else {
                            theme::CUE_CYAN
                        }),
                )
                .fill(if pr_active {
                    theme::CUE_CYAN
                } else {
                    theme::WELL
                });
                if ui
                    .add(pr_btn)
                    .on_hover_text("Angedockter MIDI-Noten-Editor wie in Ableton Live")
                    .clicked()
                {
                    self.studio_dock = if pr_active {
                        StudioDockTab::Closed
                    } else {
                        StudioDockTab::PianoRoll
                    };
                }

                let auto_active = self.studio_dock == StudioDockTab::Automation;
                let auto_btn =
                    egui::Button::new(egui::RichText::new("〰 Automation").small().strong().color(
                        if auto_active {
                            theme::WELL
                        } else {
                            theme::ACCENT
                        },
                    ))
                    .fill(if auto_active {
                        theme::ACCENT
                    } else {
                        theme::WELL
                    });
                if ui
                    .add(auto_btn)
                    .on_hover_text(
                        "Kurven für Lautstärke, Pan, Sends und Effekt-Regler der gewählten Spur",
                    )
                    .clicked()
                {
                    self.studio_dock = if auto_active {
                        StudioDockTab::Closed
                    } else {
                        StudioDockTab::Automation
                    };
                }

                let an_btn =
                    egui::Button::new(egui::RichText::new("📊 Analyse").small().strong().color(
                        if self.analysis_open {
                            theme::WELL
                        } else {
                            theme::OK_GREEN
                        },
                    ))
                    .fill(if self.analysis_open {
                        theme::OK_GREEN
                    } else {
                        theme::WELL
                    });
                if ui
                    .add(an_btn)
                    .on_hover_text("Lautheit, True Peak, Spektrum, Spektrogramm und Stereo-Scope")
                    .clicked()
                {
                    self.analysis_open = !self.analysis_open;
                }

                if self.studio_dock == StudioDockTab::PianoRoll {
                    let float_label = if self.piano_roll_floating {
                        "⤡ Im Dock einbetten"
                    } else {
                        "⤢ Als Fenster lösen"
                    };
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(float_label).small().color(theme::DIM),
                            )
                            .fill(theme::SURFACE),
                        )
                        .clicked()
                    {
                        self.piano_roll_floating = !self.piano_roll_floating;
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let toggle_lbl = if is_open {
                        "▾ Dock einklappen"
                    } else {
                        "▴ Dock ausklappen"
                    };
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(toggle_lbl).small().color(theme::DIM),
                            )
                            .fill(theme::WELL),
                        )
                        .clicked()
                    {
                        self.studio_dock = if is_open {
                            StudioDockTab::Closed
                        } else {
                            StudioDockTab::Mixer
                        };
                    }
                });
            });

            if !is_open {
                return;
            }
            ui.add_space(4.0);
            match self.studio_dock {
                StudioDockTab::Closed => {}
                StudioDockTab::Mixer => self.mixer_console(ui),
                StudioDockTab::PianoRoll => self.docked_piano_roll(ui),
                StudioDockTab::Automation => self.docked_automation(ui),
            }
        });
    }

    /// Vertikale Mischpult-Konsole im unteren Studio-Dock.
    fn mixer_console(&mut self, ui: &mut egui::Ui) {
        let tracks: Vec<_> = self
            .state
            .project()
            .tracks
            .iter()
            .map(|t| {
                (
                    t.id,
                    t.name.clone(),
                    t.kind,
                    t.gain_db,
                    t.pan,
                    t.mute,
                    t.solo,
                )
            })
            .collect();
        if tracks.is_empty() {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(
                        "Noch keine Spuren im Mischpult. Lege eine Audio- oder MIDI-Spur an:",
                    )
                    .color(theme::DIM),
                );
                if ui.button("+ Audio-Spur").clicked() {
                    self.state.add_track();
                }
                if ui.button("+ MIDI-Spur").clicked() {
                    self.state.add_midi_track();
                }
            });
            return;
        }
        egui::ScrollArea::horizontal()
            .id_salt("studio_mixer_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    for (i, (id, name, kind, gain, pan, mute, solo)) in
                        tracks.into_iter().enumerate()
                    {
                        let color = theme::track_color(i);
                        let is_sel = self.selected_track == Some(id);
                        let armed = if self.armed.is_empty() {
                            is_sel
                        } else {
                            self.armed.contains(&id)
                        };
                        let level = self.track_levels.get(&id).copied().unwrap_or(0.0);

                        egui::Frame::new()
                            .fill(theme::WELL)
                            .stroke(egui::Stroke::new(
                                1.0,
                                if is_sel { color } else { theme::BORDER },
                            ))
                            .corner_radius(5)
                            .inner_margin(egui::Margin::symmetric(8, 6))
                            .show(ui, |ui| {
                                ui.set_width(118.0);
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 4.0;
                                    // Farbiger Kanal-Kopfstreifen
                                    let (bar_rect, bar_resp) = ui.allocate_exact_size(
                                        egui::vec2(118.0, 18.0),
                                        egui::Sense::click(),
                                    );
                                    ui.painter().rect_filled(bar_rect, 3.0, color);
                                    ui.painter().text(
                                        bar_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        &name,
                                        egui::FontId::proportional(11.5),
                                        theme::WELL,
                                    );
                                    if bar_resp.clicked() {
                                        self.selected_track = Some(id);
                                    }

                                    // M / S / Arm Tastenreihe
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 4.0;
                                        let m_btn = egui::Button::new(
                                            egui::RichText::new("M")
                                                .size(11.0)
                                                .strong()
                                                .color(if mute { theme::WELL } else { theme::DIM }),
                                        )
                                        .fill(if mute { theme::ACCENT } else { theme::SURFACE })
                                        .min_size(egui::vec2(28.0, 20.0));
                                        if ui.add(m_btn).on_hover_text("Stumm").clicked() {
                                            self.state.toggle_mute(id);
                                        }

                                        let s_btn = egui::Button::new(
                                            egui::RichText::new("S")
                                                .size(11.0)
                                                .strong()
                                                .color(if solo { theme::WELL } else { theme::DIM }),
                                        )
                                        .fill(if solo {
                                            theme::CUE_CYAN
                                        } else {
                                            theme::SURFACE
                                        })
                                        .min_size(egui::vec2(28.0, 20.0));
                                        if ui.add(s_btn).on_hover_text("Solo").clicked() {
                                            self.state.toggle_solo(id);
                                        }

                                        let arm_btn = egui::Button::new(
                                            egui::RichText::new("●").size(11.0).strong().color(
                                                if armed {
                                                    egui::Color32::WHITE
                                                } else {
                                                    theme::ON_AIR
                                                },
                                            ),
                                        )
                                        .fill(if armed { theme::ON_AIR } else { theme::SURFACE })
                                        .min_size(egui::vec2(28.0, 20.0));
                                        if ui
                                            .add(arm_btn)
                                            .on_hover_text("Aufnahme scharf schalten")
                                            .clicked()
                                            && !self.armed.remove(&id)
                                        {
                                            self.armed.insert(id);
                                        }

                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    egui::RichText::new("🗑")
                                                        .size(10.5)
                                                        .color(theme::DIM),
                                                )
                                                .fill(theme::SURFACE)
                                                .min_size(egui::vec2(22.0, 20.0)),
                                            )
                                            .on_hover_text("Spur löschen")
                                            .clicked()
                                        {
                                            self.armed.remove(&id);
                                            if self.selected_track == Some(id) {
                                                self.selected_track = None;
                                            }
                                            self.state.remove_track(id);
                                        }
                                    });

                                    // Vertikale LED-Pegelanzeige + Gain- & Pan-Steuerung
                                    ui.horizontal(|ui| {
                                        let (m_rect, _) = ui.allocate_exact_size(
                                            egui::vec2(14.0, 78.0),
                                            egui::Sense::hover(),
                                        );
                                        theme::led_meter_vertical(ui.painter(), m_rect, level);

                                        ui.vertical(|ui| {
                                            ui.spacing_mut().item_spacing.y = 4.0;
                                            ui.label(
                                                egui::RichText::new(
                                                    if kind == woa_model::TrackKind::Midi {
                                                        "MIDI-SYNTH"
                                                    } else if kind == woa_model::TrackKind::Bus {
                                                        "BUS"
                                                    } else {
                                                        "AUDIO-KANAL"
                                                    },
                                                )
                                                .size(9.5)
                                                .monospace()
                                                .color(if kind == woa_model::TrackKind::Midi {
                                                    theme::CUE_CYAN
                                                } else {
                                                    theme::DIM
                                                }),
                                            );
                                            let mut g = match self.gain_drag {
                                                Some((d, v)) if d == id => v,
                                                _ => self.shown_gain(id, gain),
                                            };
                                            let r = ui
                                                .add(
                                                    egui::DragValue::new(&mut g)
                                                        .range(MIN_GAIN_DB..=MAX_GAIN_DB)
                                                        .speed(0.2)
                                                        .fixed_decimals(1)
                                                        .prefix("Pegel ")
                                                        .suffix(" dB"),
                                                )
                                                .on_hover_text("Kanal-Gain (Doppelklick: 0 dB)");
                                            if r.double_clicked() {
                                                self.gain_drag = None;
                                                self.state.set_gain(id, 0.0);
                                            } else {
                                                if r.changed() {
                                                    self.gain_drag = Some((id, g));
                                                }
                                                if r.drag_stopped() || (r.changed() && !r.dragged())
                                                {
                                                    self.gain_drag = None;
                                                    self.state.set_gain(id, g);
                                                }
                                            }

                                            let mut p = match self.pan_drag {
                                                Some((d, v)) if d == id => v,
                                                _ => pan,
                                            };
                                            let mut p_pct = (p * 100.0).round();
                                            let pr = ui
                                                .add(
                                                    egui::DragValue::new(&mut p_pct)
                                                        .range(-100.0..=100.0)
                                                        .speed(1.0)
                                                        .fixed_decimals(0)
                                                        .custom_formatter(|v, _| {
                                                            let iv = v.round() as i32;
                                                            if iv == 0 {
                                                                "Pan Mitte".to_string()
                                                            } else if iv < 0 {
                                                                format!("Pan L{}", -iv)
                                                            } else {
                                                                format!("Pan R{iv}")
                                                            }
                                                        }),
                                                )
                                                .on_hover_text("Panorama (Doppelklick: Mitte)");
                                            if pr.double_clicked() {
                                                self.pan_drag = None;
                                                self.state.set_pan(id, 0.0);
                                            } else {
                                                p = (p_pct / 100.0).clamp(-1.0, 1.0);
                                                if pr.changed() {
                                                    self.pan_drag = Some((id, p));
                                                }
                                                if pr.drag_stopped()
                                                    || (pr.changed() && !pr.dragged())
                                                {
                                                    self.pan_drag = None;
                                                    self.state.set_pan(id, p);
                                                }
                                            }
                                        });
                                    });
                                    self.strip_routing(ui, id, kind);
                                });
                            });
                    }
                    self.add_bus_box(ui);
                });
            });
    }

    /// "+ Bus": Name eintragen, Knopf legt die Gruppenspur an.
    fn add_bus_box(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(theme::PANEL_HEADER)
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
            .corner_radius(5)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_width(118.0);
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new("NEUER BUS")
                            .size(9.5)
                            .monospace()
                            .color(theme::DIM),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_bus_name)
                            .hint_text("Hall")
                            .desired_width(100.0),
                    );
                    if ui
                        .add(egui::Button::new("+ Bus").fill(theme::SURFACE))
                        .on_hover_text("Gruppenspur für Sends und Gruppen anlegen")
                        .clicked()
                    {
                        let name = std::mem::take(&mut self.new_bus_name);
                        self.state.add_bus(&name);
                    }
                });
            });
    }

    /// Ausgang, Sends und Effektkette eines Mischpult-Zugs.
    fn strip_routing(&mut self, ui: &mut egui::Ui, id: TrackId, kind: woa_model::TrackKind) {
        let Some(track) = self.state.project().track(id) else {
            return;
        };
        let output = track.output;
        let sends = track.sends.clone();
        let inserts = track.inserts.clone();
        let buses: Vec<(TrackId, String)> = self
            .state
            .project()
            .tracks
            .iter()
            .filter(|b| b.kind == woa_model::TrackKind::Bus && b.id != id)
            .map(|b| (b.id, b.name.clone()))
            .collect();
        let bus_name = |t: TrackId| {
            buses
                .iter()
                .find(|(b, _)| *b == t)
                .map_or_else(|| "?".to_string(), |(_, n)| n.clone())
        };
        let small = |s: &str| {
            egui::RichText::new(s.to_owned())
                .size(9.5)
                .monospace()
                .color(theme::DIM)
        };

        ui.separator();
        // Ausgang
        let current = output.map_or_else(|| "Master".to_string(), bus_name);
        egui::ComboBox::from_id_salt(("out", id.0))
            .selected_text(format!("→ {current}"))
            .width(100.0)
            .show_ui(ui, |ui| {
                if ui.selectable_label(output.is_none(), "Master").clicked() {
                    self.state.set_output(id, None);
                }
                for (b, n) in &buses {
                    if ui.selectable_label(output == Some(*b), n).clicked() {
                        self.state.set_output(id, Some(*b));
                    }
                }
            });

        // Sends
        if !buses.is_empty() {
            ui.label(small("SENDS"));
        }
        for s in &sends {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                let mut db = match self.send_drag {
                    Some((t, d, v)) if t == id && d == s.target => v,
                    _ => s.level_db,
                };
                let r = ui
                    .add(
                        egui::DragValue::new(&mut db)
                            .range(woa_model::MIN_GAIN_DB..=woa_model::MAX_SEND_DB)
                            .speed(0.2)
                            .fixed_decimals(1)
                            .prefix(format!("{} ", bus_name(s.target)))
                            .suffix(" dB"),
                    )
                    .on_hover_text("Send-Pegel (Doppelklick: 0 dB)");
                if r.double_clicked() {
                    self.send_drag = None;
                    self.state.set_send(id, s.target, 0.0, s.pre_fader);
                } else {
                    if r.changed() {
                        self.send_drag = Some((id, s.target, db));
                    }
                    if r.drag_stopped() || (r.changed() && !r.dragged()) {
                        self.send_drag = None;
                        self.state.set_send(id, s.target, db, s.pre_fader);
                    }
                }
                let tap = if s.pre_fader { "PRE" } else { "POST" };
                if ui
                    .small_button(tap)
                    .on_hover_text("Vor (PRE) oder nach (POST) dem Fader abgreifen")
                    .clicked()
                {
                    self.state.set_send(id, s.target, s.level_db, !s.pre_fader);
                }
                if ui
                    .small_button("✕")
                    .on_hover_text("Send entfernen")
                    .clicked()
                {
                    self.state.remove_send(id, s.target);
                }
            });
        }
        let free: Vec<_> = buses
            .iter()
            .filter(|(b, _)| !sends.iter().any(|s| s.target == *b))
            .collect();
        if !free.is_empty() {
            ui.menu_button("+ Send", |ui| {
                for (b, n) in free {
                    if ui.button(n).clicked() {
                        self.state.set_send(id, *b, -12.0, false);
                        ui.close();
                    }
                }
            });
        }

        // Effektkette
        ui.label(small("EFFEKTE"));
        for ins in &inserts {
            let header = ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                let on = !ins.bypass;
                let b = egui::Button::new(
                    egui::RichText::new(ins.kind.label())
                        .size(10.5)
                        .color(if on { theme::WELL } else { theme::DIM }),
                )
                .fill(if on { theme::CUE_CYAN } else { theme::SURFACE })
                .min_size(egui::vec2(70.0, 18.0));
                if ui.add(b).on_hover_text("Ein/Aus (Bypass)").clicked() {
                    self.state.toggle_insert_bypass(id, ins.id);
                }
                if ui
                    .small_button("✕")
                    .on_hover_text("Effekt entfernen")
                    .clicked()
                {
                    self.state.remove_insert(id, ins.id);
                }
            });
            let _ = header;
            if ins.kind.supports_sidechain() {
                let sources: Vec<(TrackId, String)> = self
                    .state
                    .project()
                    .tracks
                    .iter()
                    .filter(|o| o.id != id)
                    .map(|o| (o.id, o.name.clone()))
                    .collect();
                let cur = ins
                    .sidechain
                    .and_then(|s| sources.iter().find(|(o, _)| *o == s))
                    .map_or_else(|| "eigenes Signal".to_string(), |(_, n)| n.clone());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.label(small("Sidechain"));
                    if kind == woa_model::TrackKind::Bus {
                        ui.label(small("(nur Spuren)"));
                        return;
                    }
                    egui::ComboBox::from_id_salt(("sc", ins.id.0))
                        .selected_text(cur)
                        .width(110.0)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(ins.sidechain.is_none(), "eigenes Signal")
                                .clicked()
                            {
                                self.state.set_sidechain(id, ins.id, None);
                            }
                            for (o, n) in &sources {
                                if ui.selectable_label(ins.sidechain == Some(*o), n).clicked() {
                                    self.state.set_sidechain(id, ins.id, Some(*o));
                                }
                            }
                        })
                        .response
                        .on_hover_text(
                            "Pegel einer anderen Spur (nach deren Fader) steuert diesen Effekt, \
                             zum Beispiel Musik unter Sprache absenken",
                        );
                });
            }
            if matches!(
                ins.kind,
                woa_model::EffectKind::Compressor
                    | woa_model::EffectKind::Gate
                    | woa_model::EffectKind::Limiter
                    | woa_model::EffectKind::DeEsser
            ) {
                let now = self
                    .player
                    .as_ref()
                    .map_or(0.0, |p| p.take_reduction(ins.id));
                let shown = self.gr_shown.entry(ins.id).or_insert(0.0);
                *shown = now.max(*shown * 0.9);
                let db = *shown;
                ui.horizontal(|ui| {
                    ui.label(small("Absenkung"));
                    crate::automation::reduction_bar(ui, db);
                });
            }
            egui::CollapsingHeader::new(small("Regler"))
                .id_salt(("fx", ins.id.0))
                .show(ui, |ui| {
                    ui.menu_button(small("Preset ▾"), |ui| {
                        for pr in ins.kind.presets() {
                            if ui.button(pr.name).clicked() {
                                self.fx_drag = None;
                                self.state.apply_insert_preset(id, ins.id, pr.name);
                                ui.close();
                            }
                        }
                    });
                    for (i, spec) in ins.kind.params().iter().enumerate() {
                        if !spec.choices.is_empty() {
                            let cur = ins.params.get(i).copied().unwrap_or(spec.default);
                            let idx = (cur.round().max(0.0) as usize).min(spec.choices.len() - 1);
                            egui::ComboBox::from_id_salt(("fxc", ins.id.0, i))
                                .selected_text(format!("{} {}", spec.label, spec.choices[idx]))
                                .show_ui(ui, |ui| {
                                    for (n, name) in spec.choices.iter().enumerate() {
                                        if ui.selectable_label(n == idx, *name).clicked() {
                                            self.state.set_insert_param(id, ins.id, i, n as f64);
                                        }
                                    }
                                });
                            continue;
                        }
                        let mut v = match self.fx_drag {
                            Some((f, p, v)) if f == ins.id && p == i => v,
                            _ => ins.params.get(i).copied().unwrap_or(spec.default),
                        };
                        let r = ui
                            .add(
                                egui::DragValue::new(&mut v)
                                    .range(spec.min..=spec.max)
                                    .speed((spec.max - spec.min) / 400.0)
                                    .max_decimals(2)
                                    .prefix(format!("{} ", spec.label))
                                    .suffix(format!(" {}", spec.unit)),
                            )
                            .on_hover_text("Doppelklick: Vorgabe");
                        if r.double_clicked() {
                            self.fx_drag = None;
                            self.state.set_insert_param(id, ins.id, i, spec.default);
                        } else {
                            if r.changed() {
                                self.fx_drag = Some((ins.id, i, v));
                            }
                            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                                self.fx_drag = None;
                                self.state.set_insert_param(id, ins.id, i, v);
                            }
                        }
                    }
                });
        }
        let _ = kind;
        ui.menu_button("+ Effekt", |ui| {
            for k in woa_model::EffectKind::ALL {
                if ui.button(k.label()).clicked() {
                    self.state.add_insert(id, k);
                    ui.close();
                }
            }
            ui.separator();
            if ui
                .button("Vorlage Sprachkette")
                .on_hover_text("Hochpass, Gate, EQ, Kompressor, De-Esser in einem Schritt")
                .clicked()
            {
                self.state.add_voice_chain(id);
                ui.close();
            }
        });
        self.strip_plugins(ui, id, kind);
    }

    /// CLAP-Plugins einer Spur (Paket 31): Instrument und Effekte, Regler, Browser.
    fn strip_plugins(&mut self, ui: &mut egui::Ui, id: TrackId, kind: woa_model::TrackKind) {
        use woa_model::PluginRole;
        let Some(track) = self.state.project().track(id) else {
            return;
        };
        let slots = track.plugins.clone();
        let small = |s: &str| {
            egui::RichText::new(s.to_owned())
                .size(9.5)
                .color(theme::DIM)
        };
        let has_instrument = slots.iter().any(|s| s.role == PluginRole::Instrument);
        if !slots.is_empty() {
            ui.label(small("PLUGINS"));
        }
        for slot in &slots {
            let status = self.player.as_ref().and_then(|p| p.plugin_status(slot.id));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                if slot.role == PluginRole::Effect {
                    let on = !slot.bypass;
                    let b = egui::Button::new(
                        egui::RichText::new(&slot.name).size(10.5).color(if on {
                            theme::WELL
                        } else {
                            theme::DIM
                        }),
                    )
                    .fill(if on { theme::CUE_CYAN } else { theme::SURFACE })
                    .min_size(egui::vec2(70.0, 18.0));
                    if ui.add(b).on_hover_text("Ein/Aus (Bypass)").clicked() {
                        self.state.toggle_plugin_bypass(id, slot.id);
                    }
                } else {
                    ui.label(
                        egui::RichText::new(format!("♪ {}", slot.name))
                            .size(10.5)
                            .color(theme::TEXT_BRIGHT),
                    );
                }
                if matches!(status, Some(woa_plugins::SlotStatus::Loaded)) {
                    let open = self
                        .player
                        .as_ref()
                        .is_some_and(|p| p.plugin_gui_open(slot.id));
                    let b = egui::Button::new(egui::RichText::new("Fenster").size(10.5).color(
                        if open { theme::WELL } else { theme::TEXT_BRIGHT },
                    ))
                    .fill(if open { theme::CUE_CYAN } else { theme::SURFACE })
                    .min_size(egui::vec2(52.0, 18.0));
                    if ui
                        .add(b)
                        .on_hover_text("Oberfläche des Plugins öffnen oder schließen")
                        .clicked()
                        && let Some(p) = self.player.as_mut()
                    {
                        if open {
                            p.close_plugin_gui(slot.id);
                        } else if let Err(e) = p.open_plugin_gui(slot.id, &slot.name) {
                            tracing::warn!(plugin = %slot.clap_id, error = %e, "plugin window not opened");
                            self.state.status = Status::Error(e);
                        }
                    }
                }
                if matches!(status, Some(woa_plugins::SlotStatus::Loaded))
                    && std::env::var_os("WOA_TESTMENU").is_some_and(|v| v == "1")
                    && self
                        .player
                        .as_ref()
                        .is_some_and(|p| p.plugin_isolated(slot.id))
                    && ui
                        .small_button("Absturz simulieren")
                        .on_hover_text("Testmenü: beendet den Plugin-Prozess hart")
                        .clicked()
                    && let Some(p) = self.player.as_mut()
                {
                    p.simulate_plugin_crash(slot.id);
                }
                if ui
                    .small_button("✕")
                    .on_hover_text("Plugin entfernen")
                    .clicked()
                {
                    self.state.remove_plugin(id, slot.id);
                }
            });
            match &status {
                Some(woa_plugins::SlotStatus::Missing) => {
                    ui.label(
                        egui::RichText::new(format!(
                            "nicht installiert ({}); der Platz bleibt im Projekt",
                            slot.clap_id
                        ))
                        .size(9.5)
                        .color(theme::ON_AIR),
                    );
                    continue;
                }
                Some(woa_plugins::SlotStatus::Failed(why)) => {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(why.as_str())
                                .size(9.5)
                                .color(theme::ON_AIR),
                        );
                        let isolated = self
                            .player
                            .as_ref()
                            .is_some_and(|p| p.plugin_isolated(slot.id));
                        if isolated
                            && ui
                                .small_button("Neu laden")
                                .on_hover_text("Startet das Plugin neu, mit seinem letzten Zustand")
                                .clicked()
                            && let Some(p) = self.player.as_mut()
                            && let Err(e) = p.reload_plugin(self.state.project(), slot.id)
                        {
                            tracing::warn!(plugin = %slot.clap_id, error = %e, "plugin reload failed");
                            self.state.status = Status::Error(format!("{}: {e}", slot.name));
                        }
                        if ui
                            .small_button("Sperren")
                            .on_hover_text(
                                "Die Plugin-Datei wird beim nächsten Suchen nicht mehr geladen \
                                 (unter „+ Plugin“ wieder freigeben)",
                            )
                            .clicked()
                            && let Some(p) = self.player.as_mut()
                            && let Some(file) = p.block_plugin(&slot.clap_id)
                        {
                            self.state.status = Status::Info(format!(
                                "{file} gesperrt; gilt ab dem nächsten Suchen"
                            ));
                        }
                    });
                    self.plugin_log_ui(ui, &slot.clap_id, slot.id);
                    continue;
                }
                _ => self.plugin_log_ui(ui, &slot.clap_id, slot.id),
            }
            let params: Vec<woa_plugins::ParamDesc> = self
                .player
                .as_ref()
                .map_or_else(Vec::new, |p| p.plugin_params(slot.id))
                .into_iter()
                .filter(|p| !p.hidden && !p.read_only)
                .collect();
            if params.is_empty() {
                continue;
            }
            egui::CollapsingHeader::new(small("Regler"))
                .id_salt(("plug", slot.id.0))
                .show(ui, |ui| {
                    const SHOWN: usize = 64;
                    for p in params.iter().take(SHOWN) {
                        let stored = slot.params.iter().find(|q| q.id == p.id).map(|q| q.value);
                        let mut v = match self.plugin_drag {
                            Some((f, q, v)) if f == slot.id && q == p.id => v,
                            _ => stored
                                .or_else(|| {
                                    self.player
                                        .as_mut()
                                        .and_then(|pl| pl.plugin_value(slot.id, p.id))
                                })
                                .unwrap_or(p.default),
                        };
                        let label = if p.module.is_empty() {
                            format!("{} ", p.name)
                        } else {
                            format!("{} / {} ", p.module, p.name)
                        };
                        let mut w = egui::DragValue::new(&mut v)
                            .range(p.min..=p.max)
                            .speed(((p.max - p.min) / 400.0).max(1e-6))
                            .max_decimals(3)
                            .prefix(label);
                        if p.stepped {
                            w = w.max_decimals(0).speed(0.05);
                        }
                        let r = ui.add(w).on_hover_text("Doppelklick: Vorgabe");
                        if r.double_clicked() {
                            self.plugin_drag = None;
                            self.state.set_plugin_param(id, slot.id, p.id, p.default);
                        } else {
                            if r.changed() {
                                self.plugin_drag = Some((slot.id, p.id, v));
                            }
                            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                                self.plugin_drag = None;
                                self.state.set_plugin_param(id, slot.id, p.id, v);
                            }
                        }
                    }
                    if params.len() > SHOWN {
                        ui.label(small(&format!(
                            "… und {} weitere (alle im Plugin-Fenster)",
                            params.len() - SHOWN
                        )));
                    }
                });
        }
        ui.menu_button("+ Plugin", |ui| {
            let list = self
                .player
                .as_mut()
                .map_or_else(Vec::new, |p| p.plugin_list());
            if list.is_empty() {
                ui.label(small(
                    "Keine CLAP-Plugins gefunden (Ordner ~/.clap, /usr/lib/clap oder CLAP_PATH).",
                ));
            }
            for info in &list {
                let as_instrument = info.is_instrument();
                let as_effect = info.is_effect() || !as_instrument;
                if as_instrument && kind == woa_model::TrackKind::Midi && !has_instrument {
                    let label = format!("♪ {}  {}", info.name, info.vendor);
                    if ui.button(label).on_hover_text("Als Instrument").clicked() {
                        self.state.add_plugin(
                            id,
                            &info.clap_id,
                            &info.name,
                            PluginRole::Instrument,
                        );
                        ui.close();
                    }
                }
                if as_effect && kind != woa_model::TrackKind::Bus {
                    let label = format!("fx {}  {}", info.name, info.vendor);
                    if ui.button(label).on_hover_text("Als Effekt").clicked() {
                        self.state
                            .add_plugin(id, &info.clap_id, &info.name, PluginRole::Effect);
                        ui.close();
                    }
                }
            }
            ui.separator();
            if ui
                .button("Neu suchen")
                .on_hover_text("Liest die Plugin-Ordner noch einmal")
                .clicked()
            {
                if let Some(p) = self.player.as_mut() {
                    p.rescan_plugins();
                }
                ui.close();
            }
            self.plugin_safety_ui(ui);
        });
    }

    /// Sperrliste und Suchprobleme im Plugin-Menü (Paket 34).
    fn plugin_safety_ui(&mut self, ui: &mut egui::Ui) {
        let small = |s: &str| {
            egui::RichText::new(s.to_owned())
                .size(9.5)
                .color(theme::DIM)
        };
        let Some(p) = self.player.as_mut() else {
            return;
        };
        let blocked = p.blocked_plugins();
        let problems: Vec<_> = p
            .plugin_scan_problems()
            .into_iter()
            .filter(|q| !blocked.iter().any(|b| b.file == q.path))
            .collect();
        let name = |f: &std::path::Path| {
            f.file_name().map_or_else(
                || f.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            )
        };
        if !problems.is_empty() {
            ui.label(small(&format!(
                "{} Datei(en) beim Suchen übersprungen:",
                problems.len()
            )));
            for q in problems.iter().take(8) {
                ui.label(
                    egui::RichText::new(format!("{}: {}", name(&q.path), q.reason))
                        .size(9.5)
                        .color(theme::DIM),
                );
            }
        }
        if blocked.is_empty() {
            return;
        }
        ui.separator();
        ui.label(small(&format!(
            "Gesperrt ({}): werden nicht geladen",
            blocked.len()
        )));
        let mut free: Option<std::path::PathBuf> = None;
        for b in blocked.iter().take(50) {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{}: {}", name(&b.file), b.reason))
                        .size(9.5)
                        .color(theme::ON_AIR),
                )
                .on_hover_text(b.file.display().to_string());
                if ui.small_button("Freigeben").clicked() {
                    free = Some(b.file.clone());
                }
            });
        }
        if let Some(f) = free {
            p.unblock_plugin(&f);
            self.state.status =
                Status::Info("Freigegeben; „Neu suchen“ lädt die Datei wieder".into());
        } else if blocked.len() > 1 && ui.small_button("Alle freigeben").clicked() {
            p.unblock_all_plugins();
            self.state.status =
                Status::Info("Alle Sperren aufgehoben; „Neu suchen“ lädt neu".into());
        }
    }

    /// Das Log eines Plugins im eigenen Prozess (Paket 34): die letzten Zeilen, eingeklappt.
    fn plugin_log_ui(&mut self, ui: &mut egui::Ui, clap_id: &str, id: woa_model::InsertId) {
        let small = |s: &str| {
            egui::RichText::new(s.to_owned())
                .size(9.5)
                .color(theme::DIM)
        };
        let Some(p) = self.player.as_ref().filter(|p| p.plugin_isolated(id)) else {
            return;
        };
        egui::CollapsingHeader::new(small("Log"))
            .id_salt(("plog", id.0))
            .show(ui, |ui| {
                let lines = p.plugin_log_tail(clap_id, 8);
                if lines.is_empty() {
                    ui.label(small("Noch nichts eingetragen."));
                }
                for l in &lines {
                    ui.label(
                        egui::RichText::new(l)
                            .monospace()
                            .size(9.0)
                            .color(theme::DIM),
                    );
                }
                ui.label(small(&format!("Ordner: {}", p.plugin_log_dir().display())));
            });
    }

    /// Kurven-Editor im unteren Studio-Dock für die gewählte Spur.
    fn docked_automation(&mut self, ui: &mut egui::Ui) {
        let Some(track) = self
            .selected_track
            .filter(|t| self.state.project().track(*t).is_some())
            .or_else(|| self.state.project().tracks.first().map(|t| t.id))
        else {
            ui.label(egui::RichText::new("Noch keine Spur.").color(theme::DIM));
            return;
        };
        if let Some(t) = self.state.project().track(track) {
            ui.label(
                egui::RichText::new(format!("Spur: {}", t.name))
                    .small()
                    .color(theme::DIM),
            );
        }
        let tempo = self.state.project().tempo_bpm;
        let playhead = self
            .player
            .as_ref()
            .map_or(0.0, |p| p.position_beats(tempo));
        crate::automation::show(ui, &mut self.state, &mut self.auto_ed, track, playhead);
    }

    /// Angedockte Piano-Roll im unteren Studio-Dock.
    fn docked_piano_roll(&mut self, ui: &mut egui::Ui) {
        if self.piano_roll_floating {
            ui.label(
                egui::RichText::new(
                    "Die Piano-Roll ist aktuell als schwebendes Fenster geöffnet. Klicke oben auf „⤡ Im Dock einbetten“, um sie hier unten anzudocken.",
                )
                .color(theme::DIM),
            );
            return;
        }
        // Wenn ein MIDI-Clip ausgewählt wurde, aber noch nicht in `piano_roll` liegt, automatisch laden:
        if let Some(sel) = self.selected_clip
            && self
                .state
                .project()
                .clip(sel)
                .is_some_and(|(_, c)| c.midi.is_some())
            && self.piano_roll.as_ref().is_none_or(|pr| pr.clip != sel)
        {
            self.piano_roll = Some(crate::pianoroll::PianoRoll::new(sel));
        }

        let live = self.live_midi_track();
        if let Some(mut pr) = self.piano_roll.take() {
            let track = self.state.project().clip(pr.clip).map(|(t, _)| t.id);
            let sink = self
                .player
                .as_ref()
                .filter(|_| track.is_some() && track == live)
                .map(Player::midi_sink);
            if pr.show_inline(ui, &mut self.state, sink.as_ref()) {
                self.piano_roll = Some(pr);
                return;
            }
        }

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(
                    "Kein MIDI-Clip ausgewählt. Doppelklicke auf einen MIDI-Clip in der Zeitleiste oder lege direkt einen neuen an:",
                )
                .color(theme::DIM),
            );
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("🎹 + Leeren MIDI-Clip (4 Takte) anlegen")
                            .strong()
                            .color(theme::CUE_CYAN),
                    )
                    .fill(theme::SURFACE),
                )
                .clicked()
            {
                self.create_midi_clip_at_playhead(self.selected_track);
            }
        });
    }
}

/// Text of the opt-in question and the settings hint.
const TELEMETRY_HINT: &str = "Fehlercodes anonym an den WOA-Server senden? Gesendet werden nur \
     Code, Stufe und Zahl (z. B. „Aussetzer: 1“), mit einer zufälligen Installations-ID. \
     Keine Texte, keine Dateinamen, keine Aufnahmen. Gilt ab dem nächsten Start, \
     jederzeit unter „Einstellungen“ abschaltbar.";

/// Fade-Hüllkurve eines Clips an Stelle `local` (Beats ab Clip-Anfang), 0..1.
fn fade_gain(c: &woa_model::Clip, local: f64) -> f64 {
    let mut g = 1.0f64;
    if c.fade_in_beats > 0.0 {
        g = g.min(f64::from(
            c.fade_in_curve.gain((local / c.fade_in_beats) as f32),
        ));
    }
    if c.fade_out_beats > 0.0 {
        g = g.min(f64::from(
            c.fade_out_curve
                .gain(((c.length_beats - local) / c.fade_out_beats) as f32),
        ));
    }
    g.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use woa_model::{AutoMode, AutoTarget};

    fn setup(mode: AutoMode) -> (AppState, TrackId, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = AppState::new(tmp.path().join("p.woa"), Instant::now());
        let t = state.add_track().unwrap();
        state.set_automation(t, AutoTarget::Gain, Vec::new(), mode);
        (state, t, tmp)
    }

    /// Ein Fader-Ruck über die echte Oberfläche des Controllers samt Übernahme nach der Ruhepause.
    fn ride(
        state: &mut AppState,
        t: TrackId,
        recording: &[(TrackId, AutoTarget)],
    ) -> Option<Pending> {
        let (mut gain_drag, mut pan_drag, mut pending, mut touches) =
            (None, None, None, Vec::new());
        {
            let mut surface = AppSurface {
                state,
                radio: None,
                gain_drag: &mut gain_drag,
                pan_drag: &mut pan_drag,
                recording,
                pending: &mut pending,
                touches: &mut touches,
                mode: Mode::Produzent,
                mode_request: None,
            };
            surface.set(Target::TrackGain(0), 0.25);
        }
        assert_eq!(touches.len(), 1);
        assert_eq!(touches[0].0, t);
        let p = pending.take();
        if let Some(p) = p {
            AppSurface::commit(state, &mut gain_drag, p);
        }
        p
    }

    #[test]
    fn a_ride_during_recording_leaves_gain_and_undo_alone() {
        let (mut state, t, _tmp) = setup(AutoMode::Write);
        let before = state.project().track(t).unwrap().gain_db;
        let p = ride(&mut state, t, &[(t, AutoTarget::Gain)]).unwrap();
        assert!(p.keep);
        assert_eq!(state.project().track(t).unwrap().gain_db, before);
        // Der letzte Schritt im Verlauf ist weiter das Setzen der Betriebsart.
        state.undo();
        assert!(
            state
                .project()
                .track(t)
                .unwrap()
                .lane(&AutoTarget::Gain)
                .is_none()
        );
    }

    #[test]
    fn without_recording_the_ride_sets_the_gain_as_before() {
        let (mut state, t, _tmp) = setup(AutoMode::Read);
        let p = ride(&mut state, t, &[]).unwrap();
        assert!(!p.keep);
        assert!(state.project().track(t).unwrap().gain_db < -20.0);
    }
}
