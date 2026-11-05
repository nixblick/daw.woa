//! The eframe window. Thin: reads [`AppState`], forwards user actions to it.

mod menu;
mod mixer;
mod project_io;
mod timeline;
mod tracks;
mod transport;
mod updates;

use crate::ordner::Modal;
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
    Chat,
}

/// Pixel je Beat in der Zeitleiste.
/// Start-Zoom der Zeitleiste in Pixel je Beat.
const PX_PER_BEAT: f32 = 24.0;
/// Breite der Spurköpfe.
const HEADER_W: f32 = 264.0;
/// Breite des Feedback-Fensters (vorher 420).
/// Quelltext von `app.rs` samt Untermodulen, für Bauregel-Tests anderer Module.
#[cfg(test)]
pub(crate) const APP_SOURCES: &[(&str, &str)] = &[
    ("app.rs", include_str!("app.rs")),
    ("app/transport.rs", include_str!("app/transport.rs")),
    ("app/project_io.rs", include_str!("app/project_io.rs")),
    ("app/menu.rs", include_str!("app/menu.rs")),
    ("app/tracks.rs", include_str!("app/tracks.rs")),
    ("app/timeline.rs", include_str!("app/timeline.rs")),
    ("app/mixer.rs", include_str!("app/mixer.rs")),
    ("app/updates.rs", include_str!("app/updates.rs")),
];

pub const FEEDBACK_W: f32 = 600.0;
/// Abstand zwischen den Elementen rechts in der Kopfleiste (André 31.10.: „bisl weiter auseinander“).
use crate::widgets::topbar::{
    Chip, ChipLook, TOPBAR_COMPACT_BELOW, TOPBAR_GROUP_GAP, status_bar, status_sep, topbar_chip,
};
/// Widmung im Über-Dialog (André 31.10.2026).
pub const ABOUT_TAGLINE: &str = "WOA – built with love, coffee and AI";
/// Andrés PayPal-Spendenlink, derselbe wie in bretteins.desktop und fotobrett.desktop (`app/ui/app-info.js`).
pub const DONATE_PAYPAL: &str = "https://www.paypal.com/qrcodes/p2pqrc/KUND7JXYHRGDN";
/// QR-Code des PayPal-Links für „Über WOA“ (#231), einmal gebaut.
pub fn donate_qr() -> Option<&'static woa_remote::qr::QrMatrix> {
    static QR: std::sync::OnceLock<Option<woa_remote::qr::QrMatrix>> = std::sync::OnceLock::new();
    QR.get_or_init(|| woa_remote::qr::qr(DONATE_PAYPAL))
        .as_ref()
}
/// Andrés Ko-fi-Seite, wie in bretteins.desktop und fotobrett.desktop.
pub const DONATE_KOFI: &str = "https://ko-fi.com/nixblick";
/// Höhe einer Spur (Design `TrackHeader`: 64 px, zwei Zeilen).
const ROW_H: f32 = 64.0;
/// Höhe der Akkordspur unter dem Lineal (Design `ChordTrack`: 26 px).
const CHORD_LANE_H: f32 = 26.0;
/// Pan auch im Spurkopf (zusätzlich zum Rack-Ausgang). Offene Entscheidung von André
/// (31.10.2026): `false` nimmt die Anzeige aus dem Spurkopf.
const HEADER_PAN: bool = true;
/// Platz für Gain- und Pan-Feld in der zweiten Zeile des Spurkopfs (#212).
const HEADER_FIELDS_W: f32 = if HEADER_PAN { 134.0 } else { 80.0 };
/// So lange steht der Hinweis „Rückgängig (Strg+Z)“ nach dem Löschen einer Spur.
const UNDO_TOAST_SECS: f64 = 8.0;
/// Clip-Inspektor: darunter entfallen Gruppentitel (Tooltips tragen die Namen).
const INSPECTOR_NARROW: f32 = 1040.0;

/// Hinweis nach dem Löschen einer Spur (Design `DeleteTrack`).
struct UndoToast {
    /// Gelöschte Spuren, solange der Hinweis steht.
    count: usize,
    /// Name der zuletzt gelöschten Spur.
    name: String,
    /// Bis zu dieser Zeit (egui-Zeit in Sekunden) bleibt er stehen.
    until: f64,
}

impl UndoToast {
    fn text(&self) -> String {
        if self.count > 1 {
            format!("{} Spuren gelöscht", self.count)
        } else {
            format!("Spur „{}“ gelöscht", self.name)
        }
    }
}
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
/// Was nach der Frage „Projekt speichern?“ vor Neu/Öffnen kommt (#408).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Replace {
    New,
    Open,
    Dawproject,
}

pub struct WoaApp {
    /// Frames since `frames_since` (only with `WOA_REPAINT_DEBUG`).
    frames: (u32, Instant),
    state: AppState,
    config: GuiConfig,
    /// Where `config.json` is saved when the user changes a setting (`None` = not saved).
    config_path: Option<PathBuf>,
    feedback: FeedbackDialog,
    /// Reports held back after 429, sent again later (also after a restart).
    feedback_outbox: crate::feedback::Outbox,
    /// Fehler beim Anhängen eines eigenen Bildes (Feedback).
    image_error: Option<String>,
    /// Vorschau-Texturen im Feedback-Fenster: (Art, Bytes), Textur (#211).
    feedback_thumbs: Vec<((&'static str, usize), egui::TextureHandle)>,
    /// Update-Hinweis (Suche und Download im Hintergrund).
    update: crate::update::UpdateState,
    /// Frage „Projekt speichern?“ vor Beenden, Fenster schließen und Update (#365, #362).
    exit: crate::exit::ExitGuard,
    /// Frage „Projekt speichern?“ vor Neu und Öffnen (#408): was danach kommt.
    replace_ask: Option<Replace>,
    /// Frage „Projekt überschreiben?“ beim Speichern über ein anderes Projekt (#409).
    overwrite_ask: Option<std::path::PathBuf>,
    /// Beim ersten Bild automatisch suchen (nur in der echten App, nicht in Tests).
    update_autostart: bool,
    /// Track being renamed and its edit buffer.
    /// Spur, Eingabe, und ob der Fokus schon gesetzt wurde (nur einmal, sonst kommt man
    /// aus dem Feld nicht mehr heraus, André 30.10.).
    rename: Option<(TrackId, String, bool)>,
    /// Gain being dragged (committed as one undo step on release).
    gain_drag: Option<(TrackId, f64)>,
    /// Laufende Änderung eines Effekt-Reglers (Effekt, Regler, Wert), ein Undo-Schritt am Ende.
    fx_drag: Option<(woa_model::InsertId, usize, f64)>,
    /// Laufende Änderung eines Plugin-Reglers (Platz, Regler, Wert), ein Undo-Schritt am Ende.
    plugin_drag: Option<(woa_model::InsertId, u32, f64)>,
    /// Laufende Änderung eines Send-Pegels (Spur, Ziel, dB).
    send_drag: Option<(TrackId, TrackId, f64)>,
    /// Hinweis „Rückgängig (Strg+Z)“ nach dem Löschen einer Spur.
    undo_toast: Option<UndoToast>,
    /// egui-Zeit dieses Bildes (Sekunden), für den Rückgängig-Hinweis.
    now: f64,
    /// Eingebaute Effekte, deren Regler im Slot aufgeklappt sind.
    fx_open: std::collections::HashSet<woa_model::InsertId>,
    /// Suchtext in der Liste „+ Effekt“.
    fx_search: String,
    /// Aufgeklappte Klangwahl (Design `SoundPicker`): Spur und gezeigte Rolle.
    sound_pick: Option<(TrackId, woa_model::SoundCategory)>,
    /// Suche in der Klangauswahl (SoundPicker Fassung 7).
    sound_search: String,
    /// Raster für „Quantisieren“ im Clip-Inspektor (Teiler: 4 = 1/4 … 32 = 1/32).
    quantize_div: u32,
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
    /// Die eine gewählte Spur (Klick auf Spurkopf oder Kanalzug); Insert-Rack und Hervorhebung
    /// folgen ihr.
    selected_track: Option<TrackId>,
    /// Die Spur selbst wurde zuletzt angeklickt (nicht ein Clip oder die Zeitleiste): Entf löscht
    /// dann ohne gewählten Clip die Spur.
    track_focus: bool,
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
    /// Gezeichneter Spurkopf je Spur im letzten Bild (#212: endet vor Takt 1).
    header_rects: Vec<(TrackId, egui::Rect)>,
    prev_track_rects: Vec<(TrackId, egui::Rect)>,
    /// Angezeigter Pegel je Spur (mit Abfall), 0..1.
    track_levels: std::collections::HashMap<TrackId, f32>,
    /// Smoothed input level for the meter, 0..1.
    level: f32,
    /// Thema schon gesetzt.
    /// Zuletzt eingestelltes Thema samt Modus; `None` = noch nicht gesetzt.
    themed: Option<(theme::Theme, theme::Mode)>,
    /// Fenster „Über WOA“ (Version, Lizenz, Schriften) offen.
    about_open: bool,
    /// Misst langsame Frames und Hänger für die Telemetrie (nur Dauer).
    frame_watch: woa_telemetry::FrameWatch,
    protokoll: crate::protokoll::ProtokollWindow,
    /// Fenster Einstellungen (Fassung 7: Bereiche, Filter).
    settings: crate::settings::SettingsWindow,
    /// Liegt ein Schlüssel für einen Dienst (Karten unter Dienste und Konten)? Im Hintergrund
    /// nachgesehen, beim Öffnen des Fensters neu.
    service_keys: crate::ki_setup::KeyStatus,
    /// Sprung, den eine Zeile im Fenster anfordert (nach dem Zeichnen ausgeführt).
    settings_goto: Option<crate::settings::SettingId>,
    settings_was_open: bool,
    /// Entwickler-Modus: Freischalt-Fenster und „Fragen und To-dos“.
    entwickler_tor: crate::fragen::EntwicklerTor,
    fragen: crate::fragen::FragenWindow,
    abnahme: crate::abnahme::AbnahmeWindow,
    hoerer: crate::hoerer::HoererWindow,
    hoerer_peak: crate::hoerer::SessionPeak,
    hoerer_chip: crate::hoerer::ChipStand,
    /// Fotomodus (`WOA_SCREENSHOT=datei.png`): Zielpfad und gezählte Bilder.
    photo: Option<(PathBuf, u32)>,
    /// Party-Layout (Paket 35).
    party: crate::party::PartyView,
    /// Datenordner (radio.json, Mitschnitte).
    data_dir: PathBuf,
    /// Sperre des ersten Starts; ein zweiter Start holt dieses Fenster nach vorn (Fassung 8).
    instance: Option<crate::instance::Primary>,
    /// Fenster nach dem ersten Bild per Befehl maximieren (#442), nicht schon beim Anlegen.
    maximize_on_start: bool,
    /// Bibliotheks-Fenster (Paket 24).
    library: crate::library::LibraryWindow,
    /// Live-MCP-Server: Claude arbeitet auf diesem Projekt (André 31.10.2026).
    mcp_live: crate::mcp_live::McpLive,
    /// Fenster „Produktionen“: Audio-Dateien des Projekts, neueste oben.
    produktionen: crate::produktionen::ProduktionenWindow,
    akkorde: crate::akkorde::AkkordFenster,
    /// Browser links im Produzenten-Modus (Design System 2).
    browser: crate::browser::Browser,
    jingle_fx: crate::jingle_fx::FxFenster,
    jingle_assistent: crate::widgets::wizard::JingleAssistent,
    /// Export-Fenster (Paket 26).
    export_dialog: crate::export_dialog::ExportDialog,
    /// „Spur exportieren“ (Ticket 229): offenes Fenster und laufender Export.
    track_export: Option<crate::track_export::TrackExportDialog>,
    track_export_job: Option<crate::track_export::TrackExportJob>,
    clock_editor: crate::clock_editor::ClockEditor,
    event_editor: crate::event_editor::EventEditor,
    /// Sendeablauf; läuft weiter, auch wenn das Produzent-Layout zu sehen ist.
    radio: Option<RadioDesk>,
    /// Neue Vorhören-Wahl, die erst greift, sobald nichts mehr läuft.
    pfl_switch: PflSwitch,
    /// Ausgänge/Pult geändert, während etwas lief (#399): Neuaufbau, sobald nichts mehr läuft.
    routing_switch: PflSwitch,
    /// MIDI-Eingang (Paket 11), spielt das Instrument der scharfen MIDI-Spur.
    midi: woa_io::midi::MidiIn,
    /// Letzter gesehener Event-Zähler und wann er sich zuletzt geändert hat (Lampe).
    midi_seen: (u64, Option<Instant>),
    /// Offene Piano-Roll (Paket 13), Doppelklick auf einen MIDI-Clip.
    piano_roll: Option<crate::widgets::piano_roll::PianoRoll>,
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
    /// Fenster „Ausgänge“ (Gerät und Kanalpaar je Quelle).
    outputs_ui: crate::outputs_ui::OutputsUi,
    stream_ui: crate::stream_ui::StreamUi,
    /// Fenster „KI & MCP“ und der MCP-Server im Hintergrund.
    ki_ui: crate::ki_ui::KiUi,
    chat_ui: crate::chat_ui::ChatUi,
    /// Der MCP-Server im Hintergrund ist nach der Konfiguration angelaufen.
    ki_started: bool,
    ordner_ui: crate::ordner::OrdnerUi,
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
            Target::DjCrossfader => self.radio.as_ref()?.dj_crossfader(),
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
            Target::DjCrossfader => {
                if let Some(r) = self.radio.as_mut() {
                    r.set_dj_crossfader(value.clamp(0.0, 1.0));
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

/// Geschätzte Breiten der Transport-Gruppen (#443), großzügig gerundet.
const TRANSPORT_LOOP_W: f32 = 340.0;
const TRANSPORT_BARS_W: f32 = 120.0;
const TRANSPORT_LCD_W: f32 = 360.0;
const TRANSPORT_LEVEL_W: f32 = 300.0;
const TRANSPORT_VOLUME_W: f32 = 120.0;
const TRANSPORT_LATENCY_W: f32 = 140.0;
const TRANSPORT_RIGHT_W: f32 = 480.0;
/// Höhe der rechten Transportgruppe (Tasten 32 px plus Luft).
const TRANSPORT_ROW_H: f32 = 46.0;

/// Beginnt in einer umbrechenden Zeile eine neue, wenn `need` px nicht mehr passen.
fn transport_fit(ui: &mut egui::Ui, need: f32) {
    if row_rest(ui) < need {
        ui.end_row();
    }
}

/// Restbreite der aktuellen Zeile einer umbrechenden Reihe. `available_width()` liefert dort
/// die ganze Zeilenbreite, nicht den Rest (#443).
fn row_rest(ui: &egui::Ui) -> f32 {
    (ui.max_rect().right() - ui.cursor().min.x).max(0.0)
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
                Ok(p) => {
                    p.set_monitor_volume(config.monitor_volume);
                    Some(p)
                }
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
        let piano_roll_floating =
            config.piano_roll_window || std::env::var_os("WOA_PIANOROLL").is_some();
        Self {
            controls,
            controls_woken: false,
            ctrl_pending: None,
            ctrl_touches: Vec::new(),
            auto_rec: std::collections::HashMap::new(),
            auto_ed: crate::automation::EditorState::default(),
            gr_shown: std::collections::HashMap::new(),
            outputs_ui: crate::outputs_ui::OutputsUi::default(),
            stream_ui: crate::stream_ui::StreamUi::default(),
            ki_ui: crate::ki_ui::KiUi::default(),
            chat_ui: crate::chat_ui::ChatUi::default(),
            ki_started: false,
            ordner_ui: crate::ordner::OrdnerUi::default(),
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
            track_focus: false,
            selected_clip: None,
            clip_drag: None,
            fade_edit: None,
            fade_handle: None,
            gain_preview: None,
            themed: None,
            about_open: false,
            frame_watch: woa_telemetry::FrameWatch::new(),
            protokoll: Default::default(),
            settings: Default::default(),
            service_keys: Default::default(),
            settings_goto: None,
            settings_was_open: false,
            entwickler_tor: Default::default(),
            fragen: Default::default(),
            abnahme: Default::default(),
            hoerer: Default::default(),
            hoerer_peak: Default::default(),
            hoerer_chip: Default::default(),
            party: crate::party::PartyView::with_panels(config.party_panels),
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
            header_rects: Vec::new(),
            prev_track_rects: Vec::new(),
            track_levels: std::collections::HashMap::new(),
            level: 0.0,
            state,
            config,
            config_path: None,
            feedback: FeedbackDialog::with_outbox(
                woa_radio::data_dir().join(crate::feedback::OUTBOX_DIR),
            ),
            feedback_outbox: crate::feedback::Outbox::new(
                woa_radio::data_dir().join(crate::feedback::OUTBOX_DIR),
            ),
            image_error: None,
            feedback_thumbs: Vec::new(),
            update: crate::update::UpdateState::default(),
            exit: crate::exit::ExitGuard::default(),
            replace_ask: None,
            overwrite_ask: None,
            update_autostart: open_audio,
            rename: None,
            gain_drag: None,
            fx_drag: None,
            plugin_drag: None,
            send_drag: None,
            undo_toast: None,
            now: 0.0,
            fx_open: std::collections::HashSet::new(),
            fx_search: String::new(),
            sound_pick: None,
            sound_search: String::new(),
            quantize_div: 16,
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
            piano_roll_floating,
            analysis_open: false,
            analysis: None,
            analysis_buf: (Vec::new(), Vec::new()),
            last_title: String::new(),
            data_dir: woa_radio::data_dir(),
            instance: None,
            maximize_on_start: false,
            library: crate::library::LibraryWindow::new(
                woa_radio::data_dir().join(crate::library::DB_FILE),
            ),
            mcp_live: crate::mcp_live::McpLive::new(woa_radio::data_dir(), open_audio),
            produktionen: crate::produktionen::ProduktionenWindow::default(),
            akkorde: crate::akkorde::AkkordFenster::default(),
            browser: crate::browser::Browser::default(),
            jingle_fx: crate::jingle_fx::FxFenster::default(),
            jingle_assistent: crate::widgets::wizard::JingleAssistent::default(),
            export_dialog: crate::export_dialog::ExportDialog::new(),
            track_export: None,
            track_export_job: None,
            clock_editor: crate::clock_editor::ClockEditor::new(woa_radio::data_dir()),
            event_editor: crate::event_editor::EventEditor::new(woa_radio::data_dir()),
            radio: None,
            pfl_switch: PflSwitch::default(),
            routing_switch: PflSwitch::default(),
            frames: (0, Instant::now()),
        }
    }

    /// Projektzustand (für Tests und Werkzeuge).
    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// Ist das Fenster „Produktionen“ offen? (Oberflächentests)
    pub fn produktionen_open(&self) -> bool {
        self.produktionen.open
    }

    /// Projektzustand zum Ändern (für Tests).
    pub fn state_mut(&mut self) -> &mut AppState {
        &mut self.state
    }

    /// Fläche der Zeitleiste je Spur im zuletzt gezeichneten Bild (Bildschirmpunkte).
    pub fn track_rects(&self) -> &[(TrackId, egui::Rect)] {
        &self.track_rects
    }

    /// Spurkopf je Spur im zuletzt gezeichneten Bild (Bildschirmpunkte).
    pub fn header_rects(&self) -> &[(TrackId, egui::Rect)] {
        &self.header_rects
    }

    /// Öffnet die Piano-Roll eines MIDI-Clips im Studio-Dock (für Oberflächentests).
    #[doc(hidden)]
    pub fn open_piano_roll_docked(&mut self, clip: ClipId) {
        self.piano_roll_floating = false;
        self.open_piano_roll(clip);
    }

    /// Ist eine Piano-Roll offen? (Oberflächentests)
    pub fn piano_roll_open(&self) -> bool {
        self.piano_roll.is_some()
    }

    /// Setzt die Eingangsquelle einer Spur ohne Auswahlmenü (für Oberflächentests).
    #[doc(hidden)]
    pub fn set_track_source(&mut self, id: TrackId, key: &str) {
        self.track_source.insert(id, key.to_owned());
    }

    /// Pixel je Beat der Zeitleiste.
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Die gewählte Spur (für Tests).
    pub fn selected_track(&self) -> Option<TrackId> {
        self.selected_track
    }

    /// Der gewählte Clip (für Tests).
    pub fn selected_clip(&self) -> Option<ClipId> {
        self.selected_clip
    }

    /// Wählt eine Spur per Klick auf Spurkopf oder Kanalzug (ein Auswahlmodell für beide).
    fn select_track(&mut self, id: TrackId) {
        self.selected_track = Some(id);
        self.track_focus = true;
    }

    /// Die markierte Spur: hervorgehoben in Spurkopf und Kanalzug und zugleich das Ziel für Entf.
    /// Ein Zustand für beides, damit Entf nie eine Spur löscht, die nicht sichtbar markiert ist.
    /// Ein Klick auf einen Clip oder die Zeitleiste hebt die Markierung auf.
    pub fn marked_track(&self) -> Option<TrackId> {
        self.selected_track
            .filter(|id| self.track_focus && self.state.project().track(*id).is_some())
    }

    /// Ein Dialog, Menü oder Popup ist offen: Entf und Co. gehören dann nicht der Zeitleiste.
    fn overlay_open(&self, ctx: &egui::Context) -> bool {
        egui::Popup::is_any_open(ctx)
            || ctx.any_popup_open()
            || self.about_open
            || self.export_dialog.open
            || self.feedback.phase != crate::feedback::Phase::Closed
    }

    /// Die Spur, deren Inhalt das Insert-Rack zeigt: die gewählte, sonst die erste (nicht markiert).
    fn rack_track(&self) -> Option<TrackId> {
        let p = self.state.project();
        self.selected_track
            .filter(|id| p.track(*id).is_some())
            .or_else(|| p.tracks.first().map(|t| t.id))
    }

    /// Maximizes the window by command on the first frame. `with_maximized` at window creation
    /// left Windows with a window drawn at 2/3 size whose right part got no mouse input (#442).
    pub fn maximize_on_start(&mut self) {
        self.maximize_on_start = true;
    }

    /// Hält die Einzelinstanz-Sperre und weckt das Fenster, wenn ein zweiter Start klopft.
    pub fn set_instance(&mut self, p: crate::instance::Primary) {
        self.instance = Some(p);
    }

    /// Sets the data folder (radio.json, recordings). Default: the WOA data folder.
    pub fn set_data_dir(&mut self, dir: PathBuf) {
        // Zusatzbänke (Paket J7) landen im Datenordner.
        woa_engine::banks::set_bank_dir(dir.join("banks"));
        self.library.set_db_path(dir.join(crate::library::DB_FILE));
        self.clock_editor.set_dir(dir.clone());
        self.event_editor.set_dir(dir.clone());
        self.data_dir = dir;
        if self.config_path.is_some() {
            self.import_stream_kennwort();
        }
    }

    /// Alte Kennwortdatei der Sendung (bis 0.34.24 in `config.json` eingetragen) in den
    /// Schlüsselbund übernehmen und aus der Konfiguration austragen. Die Datei bleibt liegen.
    fn import_stream_kennwort(&mut self) {
        let file = self.config.stream.password_file.clone();
        if file.as_os_str().is_empty() {
            return;
        }
        match woa_stream::kennwort::import_legacy(&self.data_dir, &file) {
            Ok(_) => {
                tracing::info!("stream password moved to the secret store");
                self.config.stream.forget_legacy_file();
                self.save_config();
            }
            Err(e) => tracing::warn!(error = %e, "stream password import failed"),
        }
    }

    /// Fenster „Sendung: Stream-Server“: speichert die Angaben, das Kennwort geht in den
    /// Schlüsselbund.
    fn stream_window(&mut self, ctx: &egui::Context) {
        let streaming = self.radio.as_ref().is_some_and(|r| r.stream_running());
        let Some(applied) = self.stream_ui.show(ctx, &self.data_dir, streaming) else {
            return;
        };
        tracing::info!(server = %applied.config.label(), "stream settings applied");
        self.config.stream = applied.config;
        if let Some(neu) = applied.neues_kennwort {
            if neu.is_empty() {
                if let Err(e) = woa_stream::kennwort::clear(&self.data_dir) {
                    tracing::warn!(error = %e, "stream password not removed");
                    self.stream_ui
                        .password_failed(&self.data_dir, &format!("nicht entfernt ({e})"));
                }
            } else {
                match woa_stream::kennwort::save(&self.data_dir, &neu) {
                    Ok(ort) => {
                        tracing::info!(ort = ort.label(), "stream password stored");
                        self.config.stream.forget_legacy_file();
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "stream password not stored");
                        self.stream_ui
                            .password_failed(&self.data_dir, &e.to_string());
                    }
                }
            }
        }
        self.save_config();
    }

    /// Fenster „KI & MCP“. Der HTTP-Server im Hintergrund startet beim ersten Bild nicht
    /// (#243), nur nach „Speichern“ mit Haken.
    fn ki_window(&mut self, ctx: &egui::Context) {
        if !self.ki_started {
            self.ki_started = true;
            self.ki_ui.app_started(&mut self.config.ki);
        }
        let project = self
            .state
            .session()
            .path()
            .map(std::path::Path::to_path_buf);
        if self
            .chat_ui
            .show(ctx, &self.config.ki, &self.data_dir, project.as_deref())
        {
            self.settings
                .goto(crate::settings::SettingId::Provider(0), self.now);
        }
        let applied = self.ki_ui.show(ctx, &self.data_dir, project.as_deref());
        if self.ki_ui.take_token_changed() {
            // Der Live-Server nimmt sonst weiter das alte Token an.
            self.mcp_live.restart();
        }
        let Some(applied) = applied else {
            return;
        };
        tracing::info!(
            enabled = applied.mcp_server.enabled,
            port = applied.mcp_server.port,
            "ki settings applied"
        );
        let server_changed = applied.mcp_server != self.config.ki.mcp_server;
        self.config.ki = applied;
        if server_changed {
            self.ki_ui.apply(&self.config.ki, &self.data_dir);
        }
        self.save_config();
    }

    fn set_mode(&mut self, mode: Mode) {
        if self.config.mode == mode {
            return;
        }
        tracing::info!(from = ?self.config.mode, to = ?mode, "mode");
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

    /// Gespeicherte Wahl aller Audio-Geräte, mit der das Fenster öffnet.
    fn audio_choice(&self) -> crate::outputs_ui::AudioChoice {
        crate::outputs_ui::AudioChoice {
            outputs: self.config.outputs.clone(),
            pult: self.config.pult.clone(),
            pfl: self.config.pfl_output.clone(),
            mic: self.config.mic.clone(),
            phone: self.config.phone.clone(),
        }
    }

    /// Öffnet das Fenster „Audio-Geräte“ (der eine Ort für Soundkarten).
    fn open_audio(&mut self) {
        let choice = self.audio_choice();
        self.outputs_ui.open(choice);
    }

    /// Fenster „Audio-Geräte“: Übernommenes wird gespeichert und nur angewendet, wo sich etwas
    /// geändert hat. Mikrofon und Telefon gelten sofort. Ausgänge und Pult öffnen die
    /// Radio-Ausgabe neu, sobald nichts läuft (sonst beim nächsten Öffnen); Vorhören wie bisher
    /// über [`PflSwitch`].
    fn outputs_window(&mut self, ctx: &egui::Context) {
        let status = self.radio.as_ref().map(|r| {
            let mut lines = r.output_warnings().to_vec();
            lines.extend(r.output_links());
            lines.extend(r.phone_lines());
            if self.routing_switch.pending {
                lines.push(
                    "Ausgänge: gelten ab Ende der Wiedergabe. Der Pult-Modus wurde sofort \
                     umgeschaltet."
                        .into(),
                );
            }
            if self.pfl_switch.pending {
                lines.push("Vorhören: wirkt, sobald nichts mehr läuft.".into());
            } else if let Some(note) = r.pfl_note()
                && note != crate::radio::PFL_OFF
            {
                lines.push(note.to_string());
            }
            (lines, r.is_on_air())
        });
        let (lines, on_air) = status.unwrap_or_default();
        let Some(choice) = self.outputs_ui.show(ctx, &lines, on_air) else {
            return;
        };
        let before = self.audio_choice();
        if choice == before {
            return;
        }
        tracing::info!(pult = choice.pult.on, "audio devices applied");
        let routing = choice.outputs != before.outputs || choice.pult != before.pult;
        let pfl = choice.pfl != before.pfl;
        self.config.outputs = choice.outputs;
        self.config.pult = choice.pult;
        self.config.pfl_output = choice.pfl;
        self.config.mic = choice.mic.clone();
        self.config.phone = choice.phone.clone();
        self.save_config();
        let running = self.radio.as_ref().is_some_and(RadioDesk::is_on_air);
        if routing && self.radio.is_some() && !running {
            self.routing_switch.pending = false;
            self.radio = None;
            if self.config.mode.uses_desk() {
                self.ensure_radio();
            }
        } else if routing && self.radio.is_some() {
            // #399: Pult-Modus sofort im Mixer umschalten; die Ausgänge folgen, sobald nichts
            // mehr läuft (vorher ging die Änderung bis zum Neustart verloren).
            if self.config.pult != before.pult
                && let Some(r) = self.radio.as_mut()
            {
                r.set_pult_live(&self.config.outputs, &self.config.pult);
            }
            self.routing_switch.choose(true);
        } else if pfl && pfl_live_swap(&before.pfl, &self.config.pfl_output) {
            // #373/#374: eigenes Kopfhörer-Gerät sofort, auch wenn die Playlist läuft.
            self.pfl_switch.pending = false;
            let choice = self.config.pfl_output.clone();
            if let Some(r) = self.radio.as_mut() {
                r.swap_pfl_device(&choice);
            }
        } else if pfl && self.radio.is_some() && self.pfl_switch.choose(running) {
            self.reopen_radio_for_pfl();
        }
        if let Some(r) = self.radio.as_mut() {
            r.set_mic_config(choice.mic);
            if choice.phone != before.phone {
                r.set_phone_config(choice.phone);
            }
        }
    }

    /// Fenster „Speicherorte“ (#258): jede Änderung wird gespeichert und angewendet.
    fn ordner_window(&mut self, ctx: &egui::Context) {
        let Some(ordner) = self.ordner_ui.show(ctx, &self.config.ordner) else {
            return;
        };
        tracing::info!("folder settings applied");
        crate::ordner::apply(&ordner);
        self.config.ordner = ordner;
        self.save_config();
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
                &self.config.outputs,
                &self.config.pult,
            ));
            if let Some(r) = self.radio.as_mut() {
                r.set_phone_config(self.config.phone.clone());
                r.set_mic_config(self.config.mic.clone());
                r.set_wish_folder(self.config.wish_folder.clone());
            }
        }
    }

    /// Öffnet die Radio-Ausgabe neu, damit die Vorhören-Wahl greift. Nur ohne laufende Sendung.
    fn reopen_radio_for_pfl(&mut self) {
        tracing::info!(choice = ?self.config.pfl_output, "pfl: reopening radio output");
        self.radio = None;
        if self.config.mode.uses_desk() {
            self.ensure_radio();
        }
        if let Some(note) = self.radio.as_ref().and_then(RadioDesk::pfl_note)
            && note != crate::radio::PFL_OFF
        {
            tracing::warn!(note, "pfl output not available");
        }
    }

    /// Untermenü „MIDI-Eingang“: Aus oder ein Gerät; gespeichert in `config.json`.
    fn midi_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("MIDI-Eingang", |ui| self.midi_choices(ui));
    }

    /// Gerätewahl des MIDI-Eingangs: im Menü Einstellungen und per Klick auf MIDI in der Kopfleiste.
    fn midi_choices(&mut self, ui: &mut egui::Ui) {
        {
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
        }
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

    /// Probetöne (Design-Runde 3) an die Note-Queue der Live-Spur. Ein Ton für eine andere
    /// MIDI-Spur wählt diese Spur (sie wird damit live) und kommt kurz danach noch einmal;
    /// Abspielkopf in Beats, nur während der Wiedergabe (Lauflicht im Drum-Raster).
    /// Spurhöhe: immer 64 px. Die 36 px bei offener Piano-Roll (Design `ScreenPianoRoll`)
    /// schnitten die zweite Zeile des Spurkopfs ab (#334, „nur halb so groß“).
    fn row_h(&self) -> f32 {
        ROW_H
    }

    fn playhead_beats(&self) -> Option<f64> {
        let tempo = self.state.project().tempo_bpm;
        self.player
            .as_ref()
            .filter(|p| p.is_playing())
            .map(|p| p.position_beats(tempo))
    }

    /// ist eine Spur scharf, bleibt es bei ihr. Ohne Ausgang bleibt es still.
    fn drain_probes(&mut self, ctx: &egui::Context) {
        use crate::probeton::Probe;
        let now = Instant::now();
        let probes = self.state.probeton.take(now);
        if let Some(wait) = self.state.probeton.next_due(now) {
            ctx.request_repaint_after(wait);
        }
        if probes.is_empty() {
            return;
        }
        let live = self.live_midi_track();
        let sink = self.player.as_ref().map(Player::midi_sink);
        for probe in probes {
            let track = probe.track();
            if Some(track) == live || track == crate::probeton::VOICE_TRACK {
                let ev = match probe {
                    Probe::NoteOn {
                        pitch, velocity, ..
                    } => woa_dsp::instrument::NoteEvent::note_on(pitch, velocity),
                    Probe::NoteOff { pitch, .. } => woa_dsp::instrument::NoteEvent::note_off(pitch),
                };
                if let Some(sink) = sink.as_ref()
                    && let Ok(mut g) = sink.lock()
                    && let Some(q) = g.as_mut()
                {
                    // Volle Queue: der Probeton fällt aus, nichts blockiert.
                    let _ = q.push(ev);
                }
            } else if self.armed.is_empty()
                && let Probe::NoteOn {
                    pitch, velocity, ..
                } = probe
                && self
                    .state
                    .project()
                    .track(track)
                    .is_some_and(|t| t.kind == woa_model::TrackKind::Midi)
            {
                self.selected_track = Some(track);
                self.state.probeton.tap(
                    track,
                    &[pitch],
                    velocity,
                    now,
                    std::time::Duration::from_millis(150),
                );
                ctx.request_repaint_after(std::time::Duration::from_millis(150));
            }
        }
    }

    /// MIDI-Lampe: leuchtet kurz bei jedem Event, Tooltip nennt Gerät und Zustand.
    fn midi_lamp(&mut self, ui: &mut egui::Ui, compact: bool) {
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
        // Eigene Farbe `cue`; leuchtet bei eingehenden Noten, `warn`, solange das Gerät fehlt.
        let look = match (&status, lit) {
            (MidiStatus::Waiting(_), _) => ChipLook::On(theme::warn()),
            (MidiStatus::Connected(_), true) => ChipLook::On(theme::cue()),
            _ => ChipLook::Off(theme::cue()),
        };
        let resp = topbar_chip(ui, theme::ic::PIANO_KEYS, "MIDI", look, compact, true);
        egui::Popup::menu(&resp).show(|ui| self.midi_choices(ui));
        let target = self
            .live_midi_track()
            .and_then(|id| self.state.project().track(id).map(|t| t.name.clone()));
        let text = match status {
            MidiStatus::Off => "MIDI: kein Gerät gewählt. Klick: Gerät wählen".to_string(),
            MidiStatus::Waiting(n) => format!("MIDI: {n} nicht da, suche alle 2 s"),
            MidiStatus::Connected(n) => format!(
                "MIDI: {n}\nSpielt: {}\nKlick: Gerät wählen",
                target.as_deref().unwrap_or("keine MIDI-Spur scharf")
            ),
        };
        resp.on_hover_text(text);
        if lit {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(60));
        }
    }

    /// Modus-Umschalter: Produzent · Radio · Party, jeder in seiner Modusfarbe (TopBar-README).
    /// Läuft die Sendung, zeigt Radio in der schmalen Ansicht das Sende-Symbol.
    fn mode_switch(&mut self, ui: &mut egui::Ui, compact: bool) {
        let mut mode = self.config.mode;
        let on_air = self.radio.as_ref().is_some_and(RadioDesk::is_on_air);
        let radio_icon = if on_air {
            theme::ic::BROADCAST
        } else {
            theme::ic::RADIO
        };
        for (m, icon, word, color, tip) in [
            (
                Mode::Produzent,
                theme::ic::SLIDERS_HORIZONTAL,
                "Produzent",
                theme::mode_studio(),
                "Produzent – Zeitleiste mit Spuren und Clips (Strg+1)",
            ),
            (
                Mode::Radio,
                radio_icon,
                "Radio",
                theme::mode_radio(),
                "Radio – Sendeablauf: Playlist, Uhr, Carts, Mikro (Strg+2)",
            ),
            (
                Mode::Party,
                theme::ic::CONFETTI,
                "Party",
                theme::mode_party(),
                "Party – Abspielmodus: große Anzeige, Wunschliste, Vollbild (Strg+3)",
            ),
        ]
        .into_iter()
        // Von rechts gefüllt: rückwärts, damit Produzent links steht.
        .rev()
        {
            let look = if mode == m {
                ChipLook::On(color)
            } else {
                ChipLook::Off(color)
            };
            if topbar_chip(ui, icon, word, look, compact, true)
                .on_hover_text(tip)
                .clicked()
            {
                mode = m;
            }
        }
        self.set_mode(mode);
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
    painter.rect_filled(r, 5.0, theme::mix(theme::on_air(), theme::well(), 0.65));
    let head = egui::Rect::from_min_max(r.min, egui::pos2(r.right(), r.top() + 16.0));
    painter.rect_filled(
        head,
        egui::CornerRadius {
            nw: 5,
            ne: 5,
            sw: 0,
            se: 0,
        },
        theme::on_air(),
    );
    painter.with_clip_rect(head.intersect(rect)).text(
        head.left_center() + egui::vec2(6.0, 0.0),
        egui::Align2::LEFT_CENTER,
        format!("{} Aufnahme läuft", theme::ic::RECORD),
        egui::FontId::proportional(11.0),
        egui::Color32::WHITE,
    );
    let mid = (r.top() + 16.0 + r.bottom()) / 2.0;
    let half = (r.bottom() - r.top() - 16.0) / 2.0 - 2.0;
    let stroke = egui::Stroke::new(1.0, theme::emphasize(theme::on_air(), 0.45));
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
                ui.painter().rect_filled(slot, 4.0, theme::panel_header());
                let toggle = egui::Rect::from_min_size(slot.min, egui::vec2(HEADER_W - 36.0, 24.0));
                let label = format!(
                    "{} {}  ·  {} Takes",
                    if open {
                        theme::ic::CARET_DOWN
                    } else {
                        theme::ic::CARET_RIGHT
                    },
                    f.name,
                    f.takes.len()
                );
                if ui
                    .put(
                        toggle,
                        egui::Button::new(egui::RichText::new(label).small().color(theme::text()))
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
                        egui::Button::new(
                            egui::RichText::new(theme::ic::TRASH)
                                .small()
                                .color(theme::text_dim()),
                        )
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
                painter.rect_filled(rect, 4.0, theme::well());
                let x_of = |b: f64| rect.left() + b as f32 * px;
                for s in &f.comp {
                    let r = egui::Rect::from_x_y_ranges(
                        x_of(s.start_beats)..=x_of(s.end_beats),
                        rect.top() + 2.0..=rect.bottom() - 2.0,
                    );
                    painter.rect_filled(r, 3.0, theme::mix(color, theme::well(), 0.45));
                    painter.text(
                        r.left_center() + egui::vec2(5.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        format!("Take {}", s.take + 1),
                        egui::FontId::proportional(10.0),
                        theme::text(),
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
                        theme::text_dim(),
                    );
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(width, 22.0),
                        egui::Sense::click_and_drag(),
                    );
                    let painter = ui.painter_at(rect);
                    painter.rect_filled(rect, 3.0, theme::well());
                    let x_of = |b: f64| rect.left() + b as f32 * px;
                    let span = egui::Rect::from_x_y_ranges(
                        x_of(f.start_beats)..=x_of(f.end_beats()),
                        rect.y_range(),
                    );
                    painter.rect_filled(span, 3.0, theme::mix(color, theme::well(), 0.85));
                    for s in f.comp.iter().filter(|s| s.take == k) {
                        let r = egui::Rect::from_x_y_ranges(
                            x_of(s.start_beats)..=x_of(s.end_beats),
                            rect.y_range(),
                        );
                        painter.rect_filled(r, 3.0, theme::mix(color, theme::well(), 0.5));
                    }
                    if let (Some(ppb), Some(peaks)) = (ppb, peaks) {
                        let mid = rect.center().y;
                        let half = rect.height() / 2.0 - 2.0;
                        let stroke = egui::Stroke::new(1.0, theme::emphasize(color, 0.35));
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
                        painter.rect_filled(r, 3.0, theme::cue().gamma_multiply(0.25));
                        painter.rect_stroke(
                            r,
                            3.0,
                            egui::Stroke::new(1.0, theme::cue()),
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
}

/// Zeilen des Protokoll-Fensters (älteste zuerst), wie „Kopieren“ sie liefert.
fn feedback_log_lines() -> Vec<String> {
    woa_telemetry::journal()
        .snapshot()
        .iter()
        .filter_map(crate::feedback::ticket_line)
        .collect()
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

impl WoaApp {
    /// App-Kennzahlen zur Telemetrie-Zusammenfassung (alle 120 s, nur Zahlen).
    fn report_metrics(&mut self) {
        use woa_telemetry::metric;
        let p = self.state.project();
        metric("project.tracks", p.tracks.len() as u64);
        metric(
            "project.clips",
            p.tracks.iter().map(|t| t.clips.len() as u64).sum(),
        );
        metric("project.dirty", u64::from(self.state.session().is_dirty()));
        metric("ui.zoom_pct", (self.zoom * 100.0) as u64);
        metric("player.open", u64::from(self.player.is_some()));
        if let Some(r) = &self.radio {
            metric("radio.underruns_total", r.underruns());
        }
    }
}

impl eframe::App for WoaApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // #441: Esc gehört zuerst dem obersten Fenster bzw. der offenen Frage.
        crate::esc::beginn(ctx);
        if std::mem::take(&mut self.maximize_on_start) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }
        if let Some(i) = &self.instance {
            if i.poll_show() {
                tracing::info!("second start: window to front");
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                    egui::UserAttentionType::Informational,
                ));
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        self.close_guard(ctx);
        self.repaint_debug(ctx);
        self.drain_midi_takes();
        if self.config.mode.uses_desk() {
            self.ensure_radio();
        }
        let running = self.radio.as_ref().is_some_and(RadioDesk::is_on_air);
        let routing_due = self.routing_switch.poll(running);
        if (self.pfl_switch.poll(running) || routing_due) && self.radio.is_some() {
            if routing_due {
                tracing::info!("audio devices: applying deferred routing");
            }
            self.reopen_radio_for_pfl();
        }
        // Im Radio-Modus spielt nur der Sendemixer; der Player-Strom ruht (#51).
        if let Some(p) = self.player.as_mut() {
            p.set_suspended(self.config.mode.uses_desk());
        }
        let player_active = self.player.as_ref().is_some_and(Player::is_active);
        if let Some(r) = self.radio.as_mut() {
            r.tick(player_active);
            if let Some(dir) = r.take_wish_choice() {
                self.config.wish_folder = Some(dir);
                if let Some(p) = &self.config_path
                    && let Err(e) = self.config.save(p)
                {
                    tracing::warn!(error = %e, "config not saved");
                }
            }
            // Zeit-Events: Tabelle laden, Zone von der Stundenuhr, an den Sende-Thread reichen;
            // der AutoDJ plant rückwärts auf das nächste harte Event.
            self.event_editor.ensure_loaded();
            self.event_editor.set_zone(
                self.clock_editor.schedule.utc_offset_min,
                self.clock_editor.schedule.auto_dst,
            );
            r.set_zone(
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
        if let Some(r) = self.radio.as_mut() {
            r.set_party_mode(self.config.mode == Mode::Party);
            self.party.pump_remote(r, ctx);
        }
        // Die App startet im Vollbild (main.rs); die Party-Taste zeigt den Zustand des Fensters.
        if let Some(on) = ctx.input(|i| i.viewport().fullscreen) {
            self.party.state.fullscreen = on;
        }
        if self.config.mode == Mode::Party {
            let cmds = crate::party::PartyView::shortcuts(ctx, self.overlay_open(ctx));
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
        self.fullscreen_key(ctx);
        self.take_screenshot(ctx);
        if self.config.mode == Mode::Produzent {
            self.drop_files(ctx);
        }
        self.feedback.poll();
        self.feedback_outbox.tick(Instant::now());
        // KI-Werkzeugaufrufe zwischen zwei Bildern, vor dem Abgleich mit dem Player.
        if self.mcp_live.frame(ctx, &mut self.state) > 0 {
            self.produktionen.mark_stale();
        }
        // Ausgangspegel des Radios für Agenten (nur lesend, André 02.11.2026).
        let radio_on = self.config.mode.uses_desk();
        let peaks = self
            .radio
            .as_ref()
            .filter(|_| radio_on)
            .map_or([0.0; 3], RadioDesk::output_levels);
        self.mcp_live
            .publish_levels(radio_on && self.radio.is_some(), peaks);
        self.mcp_live.serve_dj(self.radio.as_ref());
        self.update_frame(ctx);
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
        let mut project = project;
        let live = match self.state.probeton.voice(Instant::now()) {
            Some(sound) => {
                with_voice_track(&mut project, sound);
                Some(crate::probeton::VOICE_TRACK)
            }
            None => self.live_midi_track(),
        };
        let state = &self.state;
        let loop_beats = self
            .loop_enabled
            .then(|| {
                let s = f64::from(self.loop_start_bar.saturating_sub(1) * 4);
                let e = f64::from(self.loop_end_bar.saturating_sub(1) * 4);
                (s < e).then_some((s, e))
            })
            .flatten();
        // Erster Gebrauch (Paket J7): fehlt eine Bank des Projekts, lädt sie im Hintergrund.
        for b in project
            .tracks
            .iter()
            .filter_map(|t| t.sound.and_then(|s| s.bank))
        {
            if woa_engine::banks::state(b) == woa_engine::banks::BankState::Missing {
                woa_engine::banks::ensure(b);
            }
        }
        if let Some(p) = self.player.as_mut() {
            p.sync(&project, &|rel| state.resolve_audio(rel), live);
            p.set_loop_beats(loop_beats, project.tempo_bpm);
        }
        self.drain_probes(ctx);
        self.stop_at_audio_end();
        self.drain_live_peaks();
        // Schreibfehler (z. B. Platte voll) sofort melden, nicht erst beim Stopp.
        if self.takes.iter().any(|(_, t)| t.failed()) {
            self.stop_record();
        }
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
        let _frame_time = self.frame_watch.frame();
        if self.frame_watch.summary_due() {
            self.report_metrics();
        }
        let ctx = ui.ctx().clone();
        self.now = ctx.input(|i| i.time);
        self.protokoll
            .show(&ctx, &crate::protokoll::log_dir(&self.data_dir));
        self.hoerer
            .show(&ctx, &self.config.hoerer_url, self.photo.is_some());
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma)) {
            if self.settings.open {
                self.settings.open = false;
            } else {
                self.settings.open_area(self.mode_area());
            }
        }
        if self.settings.open && !self.settings_was_open {
            // Schlüssel-Status frisch nachsehen (im Hintergrund, nie im UI-Thread).
            self.service_keys = Default::default();
        }
        self.settings_was_open = self.settings.open;
        self.settings.dev = self.config.entwickler;
        let mut settings = std::mem::take(&mut self.settings);
        if settings.size.is_none() {
            settings.size = self.config.settings_size;
        }
        settings.missing_devices = self.radio.as_ref().map_or(0, |r| r.output_warnings().len());
        settings.show(&ctx, |ui, id| self.setting_row(ui, id));
        self.settings = settings;
        self.remember_settings_size(&ctx);
        if let Some(id) = self.settings_goto.take() {
            self.settings.goto(id, self.now);
        }
        // Die Schriften gelten erst ab dem nächsten Durchlauf (sonst fehlen die Familien
        // `label` und `seg`): beim allerersten Bild nur setzen und neu zeichnen lassen.
        if theme::install_fonts(&ctx) {
            ctx.request_repaint();
            return;
        }
        self.photo(&ctx);
        let want = (
            theme::env_override().unwrap_or(self.config.theme),
            theme::Mode::from(self.config.mode),
        );
        if self.themed != Some(want) {
            theme::apply(&ctx, want.0, want.1);
            self.themed = Some(want);
        }
        egui::Panel::top("menu")
            .frame(
                egui::Frame::new()
                    .fill(theme::panel_header())
                    .inner_margin(egui::Margin::symmetric(12, 5)),
            )
            .show(ui, |ui| self.menu(ui, &ctx));
        self.exit_confirm_window(&ctx);
        self.replace_confirm_window(&ctx);
        self.overwrite_confirm_window(&ctx);
        if self.config.mode == Mode::Party
            && let Some(radio) = self.radio.as_mut()
        {
            let mut cmds = Vec::new();
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::new()
                        .fill(theme::bg())
                        .inner_margin(egui::Margin::symmetric(14, 12)),
                )
                .show(ui, |ui| cmds = self.party.ui(ui, radio, &mut self.library));
            self.party.run(cmds, radio, &ctx);
            // #386: Ansicht → Bibliothek öffnet das Fenster auch in der Party.
            self.library_window(&ctx);
            // In der Party umgeschaltet (#361): merken.
            if self.party.panels != self.config.party_panels {
                self.config.party_panels = self.party.panels;
                self.save_config();
            }
            self.app_windows(&ctx);
            return;
        }
        if self.config.mode == Mode::Radio
            && let Some(radio) = self.radio.as_mut()
        {
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::new()
                        .fill(theme::bg())
                        .inner_margin(egui::Margin::symmetric(14, 12)),
                )
                .show(ui, |ui| radio.ui(ui));
            if let Some(path) = radio.take_tag_wish() {
                self.library.edit_tags_for_path(&path);
            }
            radio.cue_window(&ctx);
            radio.voice_window(&ctx);
            radio.manage_window(&ctx, &mut self.library);
            self.library_window(&ctx);
            self.clock_editor.show(&ctx);
            self.event_editor
                .show(&ctx, self.radio.as_ref().map(|r| r.view()));
            self.app_windows(&ctx);
            return;
        }
        egui::Panel::top("transport")
            .frame(
                egui::Frame::new()
                    .fill(theme::panel())
                    .inner_margin(egui::Margin::symmetric(12, 7)),
            )
            .show(ui, |ui| self.transport_bar(ui));
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(theme::well())
                    .inner_margin(egui::Margin::symmetric(12, 4)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    match &self.state.status {
                        Status::Idle => {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} Engine bereit",
                                    theme::ic::CHECK_CIRCLE
                                ))
                                .small()
                                .color(theme::ok()),
                            );
                        }
                        Status::Info(m) => {
                            ui.label(egui::RichText::new(m).small().color(theme::text()));
                        }
                        Status::Error(m) => {
                            ui.colored_label(theme::on_air(), m);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("WOA {}", env!("CARGO_PKG_VERSION")))
                                .small()
                                .monospace()
                                .color(theme::text_dim()),
                        );
                    });
                });
            });
        self.browser_panel(ui);
        egui::Panel::top("clipbar")
            .frame(
                egui::Frame::new()
                    .fill(theme::mix(theme::panel(), theme::bg(), 0.5))
                    .inner_margin(egui::Margin::symmetric(12, 5)),
            )
            .show(ui, |ui| ui.vertical(|ui| self.clip_bar(ui)));
        self.studio_dock_panel(ui);
        self.undo_toast_ui(&ctx);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::bg())
                    .inner_margin(egui::Margin::symmetric(12, 10)),
            )
            .show(ui, |ui| self.tracks(ui));
        self.piano_roll_window(&ctx);
        self.analysis_window(&ctx);
        self.library_window(&ctx);
        self.produktionen_window(&ctx);
        self.jingle_assistent_window(&ctx);
        self.clock_editor.show(&ctx);
        self.event_editor
            .show(&ctx, self.radio.as_ref().map(|r| r.view()));
        self.app_windows(&ctx);
    }
}

impl WoaApp {
    /// Fenster, die das Menü in jeder Ansicht öffnet (Feedback, Über, Ausgabe …). Auch die
    /// Party-Ansicht zeichnet sie; vorher blieb dort etwa Feedback unsichtbar (#288).
    fn app_windows(&mut self, ctx: &egui::Context) {
        self.export_window(ctx);
        self.controller_window(ctx);
        self.feedback_window(ctx);
        self.outputs_window(ctx);
        self.ordner_window(ctx);
        self.stream_window(ctx);
        self.ki_window(ctx);
        self.telemetry_window(ctx);
        self.about_window(ctx);
        self.entwickler_windows(ctx);
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

impl WoaApp {
    /// Dezenter Abhörregler (#358): Lautsprecher-Symbol und ein schmaler waagerechter Fader.
    /// Doppelklick setzt auf voll zurück. Ändert nur den Lautsprecher, nicht Mix, Pegel, Export.
    fn monitor_volume_ui(&mut self, ui: &mut egui::Ui) {
        let mut v = self.config.monitor_volume;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let icon = if v <= 0.0 {
                theme::ic::SPEAKER_SLASH
            } else if v < 0.5 {
                theme::ic::SPEAKER_LOW
            } else {
                theme::ic::SPEAKER_HIGH
            };
            ui.label(
                egui::RichText::new(icon)
                    .size(15.0)
                    .color(theme::text_dim()),
            );
            let r = crate::widgets::fader(
                ui,
                &mut v,
                0.0..=1.0,
                1.0,
                egui::vec2(72.0, 18.0),
                crate::widgets::Orient::Horizontal,
            );
            let label = "Abhörlautstärke";
            r.widget_info(|| egui::WidgetInfo::slider(true, f64::from(v), label));
            let r = r.on_hover_text(format!(
                "Abhörlautstärke {:.0} % (nur Lautsprecher; Mix, Pegel und Export bleiben). \
                 Doppelklick: voll",
                v * 100.0
            ));
            if r.drag_stopped() || r.double_clicked() {
                self.save_config();
            }
        });
        if (v - self.config.monitor_volume).abs() > f32::EPSILON {
            self.config.monitor_volume = v;
            if let Some(p) = &self.player {
                p.set_monitor_volume(v);
            }
        }
    }
}

/// Menüleiste (André, 02.11.2026): je Gruppe zwischen zwei Trennern die Einträge in der
/// Reihenfolge der Anzeige, als (Beschriftung, Stelle im Code von `menu_items`). Innerhalb einer
/// Gruppe alphabetisch nach [`de_sort_key`]; ohne „…“. Die Tests prüfen beides und dass der Code
/// dieser Reihenfolge folgt.
#[cfg(test)]
pub(crate) const MENU_GROUPS: &[(&str, &[(&str, &str)])] = &[
    (
        "Datei",
        &[
            ("Neu (Strg+N)", "\"Neu (Strg+N)\""),
            ("Öffnen (Strg+O)", "\"Öffnen (Strg+O)\""),
            ("Speichern (Strg+S)", "\"Speichern (Strg+S)\""),
            ("Speichern unter", "\"Speichern unter\""),
        ],
    ),
    (
        "Datei",
        &[
            ("Exportieren", "\"Exportieren\""),
            ("Importieren", "\"Importieren\""),
        ],
    ),
    ("Datei", &[("Beenden (Strg+Q)", "\"Beenden (Strg+Q)\"")]),
    (
        "Datei › Exportieren",
        &[
            ("Als DAWproject", "\"Als DAWproject\""),
            ("MIDI-Clip (.mid)", "\"MIDI-Clip (.mid)\""),
            (
                "Mixdown exportieren (Strg+E)",
                "\"Mixdown exportieren (Strg+E)\"",
            ),
        ],
    ),
    (
        "Datei › Importieren",
        &[
            (
                "Audio-Datei (WAV, FLAC, MP3, OGG)",
                "\"Audio-Datei (WAV, FLAC, MP3, OGG)\"",
            ),
            ("DAWproject öffnen", "\"DAWproject öffnen\""),
            ("MIDI-Datei (.mid)", "\"MIDI-Datei (.mid)\""),
        ],
    ),
    (
        "Bearbeiten",
        &[
            ("Rückgängig", "\"Rückgängig {}\""),
            ("Wiederholen", "\"Wiederholen {}\""),
        ],
    ),
    (
        "Bearbeiten",
        &[(
            "Leeren MIDI-Clip anlegen (4 Takte)",
            "\"Leeren MIDI-Clip anlegen (4 Takte)\"",
        )],
    ),
    (
        "Ansicht",
        &[
            ("Bibliothek", "\"Bibliothek\""),
            ("Browser", "\"Browser\""),
            ("Fernbedienung", "\"Fernbedienung\""),
            ("Hörer-Statistik", "\"Hörer-Statistik\""),
            ("Stundenuhr", "\"Stundenuhr\""),
            ("Wunschliste", "\"Wunschliste\""),
            ("Zeit-Events", "\"Zeit-Events\""),
        ],
    ),
    ("Ansicht", &[("Erscheinungsbild", "self.theme_menu(ui)")]),
    (
        "Entwickler",
        &[
            ("Entwickler-Modus beenden", "\"Entwickler-Modus beenden\""),
            ("Fragen und To-dos", "\"Fragen und To-dos\""),
            ("Nach Updates suchen", "\"Nach Updates suchen\""),
        ],
    ),
    (
        "Hilfe",
        &[
            (
                "Beim Start nach Updates suchen (einmal am Tag)",
                "self.update_menu(ui)",
            ),
            ("Jetzt nach Updates suchen", "self.update_menu(ui)"),
            ("Protokoll", "\"Protokoll\""),
        ],
    ),
    ("Hilfe", &[("Über WOA", "\"Über WOA\"")]),
];

/// Status einer Assistent-Karte aus `config.json` (ohne Schlüsselbund): gewählt, eingerichtet
/// oder nicht eingerichtet.
pub(crate) fn provider_state(
    ki: &woa_assistant::KiConfig,
    preset: usize,
) -> (&'static str, crate::widgets::ChipTone) {
    use crate::widgets::ChipTone;
    if ki.pick() == Some(woa_assistant::Pick::Preset(preset)) {
        return ("gewählt", ChipTone::Ok);
    }
    let Some((_, kind, url)) = woa_assistant::ProviderConfig::presets()
        .get(preset)
        .copied()
    else {
        return ("nicht eingerichtet", ChipTone::Neutral);
    };
    if ki
        .providers
        .iter()
        .any(|p| p.kind == kind && p.base_url == url)
    {
        // Eingerichtet heißt nutzbar: grün wie „gewählt“ (#377).
        ("eingerichtet", ChipTone::Ok)
    } else {
        ("nicht eingerichtet", ChipTone::Neutral)
    }
}

/// Sortierschlüssel nach DIN 5007-1: Groß/klein egal, Umlaute wie Grundbuchstabe, ß wie ss.
#[cfg(test)]
pub(crate) fn de_sort_key(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        match c {
            'ä' => out.push('a'),
            'ö' => out.push('o'),
            'ü' => out.push('u'),
            'ß' => out.push_str("ss"),
            c => out.push(c),
        }
    }
    out
}

/// Symbol-Knopf ohne Text; was er tut, steht im Tooltip.
/// Bypass-Schalter eines Slots (Power-Symbol): an = `accent`, umgangen = gedimmt.
fn bypass_button(ui: &mut egui::Ui, on: bool) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(theme::ic::POWER)
                .size(13.0)
                .color(if on {
                    theme::accent()
                } else {
                    theme::text_dim()
                }),
        )
        .frame_when_inactive(false)
        .min_size(egui::vec2(20.0, 20.0)),
    )
    .on_hover_text(if on {
        "An: Klick umgeht den Effekt (Bypass)"
    } else {
        "Umgangen: Klick schaltet den Effekt wieder ein"
    })
}

/// Ergebnis eines Zahlenfelds im Clip-Inspektor, erst beim Loslassen oder nach Enter.
#[derive(Clone, Copy)]
enum Mixed {
    /// Eingetippt oder Standard per Doppelklick.
    Set(f64),
    /// Gezogen: von `from` nach `to` (für relative Änderungen).
    Dragged { from: f64, to: f64 },
}

impl Mixed {
    fn value(self) -> f64 {
        match self {
            Self::Set(v) | Self::Dragged { to: v, .. } => v,
        }
    }
}

/// Zahlenfeld (Design `NumberField`) für einen oder mehrere Werte: gleiche Werte stehen da,
/// unterschiedliche als „—“. Ziehen, Klick = eintippen. Liefert den Wert erst, wenn das Ziehen
/// oder Tippen endet, damit eine Änderung ein Undo-Schritt bleibt.
fn mixed_field(
    ui: &mut egui::Ui,
    key: impl std::hash::Hash + std::fmt::Debug,
    values: &[f64],
    range: std::ops::RangeInclusive<f64>,
    speed: f64,
    fmt: impl Fn(f64) -> String,
    tip: &str,
) -> Option<Mixed> {
    let id = ui.id().with(key);
    let first = values.first().copied().unwrap_or(0.0);
    let mixed = values.iter().any(|v| (v - first).abs() > 1e-9);
    let edit: Option<(f64, f64, bool)> = ui.data(|d| d.get_temp(id));
    let mut v = edit.map_or(first, |e| e.1);
    let dash = mixed && edit.is_none();
    let r = ui
        .add(
            egui::DragValue::new(&mut v)
                .range(range)
                .speed(speed)
                .custom_formatter(move |x, _| if dash { "—".to_string() } else { fmt(x) })
                .custom_parser(parse_number),
        )
        .on_hover_text(tip);
    if r.changed() || r.dragged() {
        let (from, _, dragged) = edit.unwrap_or((first, first, false));
        ui.data_mut(|d| d.insert_temp(id, (from, v, dragged || r.dragged())));
    }
    if r.dragged() || r.has_focus() {
        return None;
    }
    let (from, cur, dragged) = ui.data(|d| d.get_temp::<(f64, f64, bool)>(id))?;
    ui.data_mut(|d| d.remove::<(f64, f64, bool)>(id));
    if (cur - from).abs() < 1e-9 && !mixed {
        return None;
    }
    Some(if dragged {
        Mixed::Dragged { from, to: cur }
    } else {
        Mixed::Set(cur)
    })
}

/// Liest eine Zahl aus getipptem Text, auch „1.2.3“ (Takt.Schlag.Sechzehntel, als Position)
/// und mit Einheit („-3 dB“).
fn parse_number(s: &str) -> Option<f64> {
    let t = s.trim().replace('−', "-").replace(',', ".");
    let parts: Vec<&str> = t.split('.').collect();
    if parts.len() == 3 {
        let n: Vec<f64> = parts.iter().filter_map(|p| p.trim().parse().ok()).collect();
        if n.len() == 3 {
            return Some(((n[0] - 1.0) * 4.0 + (n[1] - 1.0) + (n[2] - 1.0) / 4.0).max(0.0));
        }
    }
    let num: String = t
        .chars()
        .skip_while(|c| !(c.is_ascii_digit() || *c == '-' || *c == '.'))
        .take_while(|c| c.is_ascii_digit() || *c == '-' || *c == '.')
        .collect();
    num.parse().ok()
}

/// Beats als Takt.Schlag.Sechzehntel (4/4): Position ab 1.1.1, Länge ab 0.0.0.
fn bar_beat(beats: f64, position: bool) -> String {
    let b = beats.max(0.0);
    let one = u64::from(position);
    let bar = (b / 4.0).floor() as u64 + one;
    let beat = (b % 4.0).floor() as u64 + one;
    let six = ((b.fract()) * 4.0).floor() as u64 + one;
    format!("{bar}.{beat}.{six}")
}

/// Gruppentitel im Insert-Rack: Modulkopf wie überall (`panel-label`).
/// Pegelansicht im Analyse-Fenster: Umschalter Analog/Modern/Aus und die gewählte Anzeige
/// des Summenpegels (analoges VU nur hier, der Transport zeigt die LED-Kette).
fn level_view(ui: &mut egui::Ui, view: &mut crate::widgets::VuView, level: f32) {
    ui.horizontal(|ui| {
        ui.label(theme::panel_label("PEGELANSICHT"));
        for v in crate::widgets::VuView::ALL {
            ui.radio_value(view, v, v.label())
                .on_hover_text("Anzeige des Summenpegels hier im Analyse-Fenster");
        }
    });
    if *view != crate::widgets::VuView::Aus {
        theme::lcd_show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.label(theme::panel_label("PEGEL").color(theme::lcd_text()));
                let size = if *view == crate::widgets::VuView::Analog {
                    egui::vec2(150.0, 64.0)
                } else {
                    egui::vec2(260.0, 14.0)
                };
                crate::widgets::vu_meter(ui, egui::Id::new("analysis-vu"), *view, size, level);
                ui.label(
                    egui::RichText::new(level_db_text(level))
                        .monospace()
                        .size(12.0)
                        .color(theme::lcd_text()),
                );
            });
        });
    }
}

/// Hängt die flüchtige Vorhör-Stimme als unsichtbare MIDI-Spur an die Kopie des Projekts,
/// die der Player bekommt (nie ans Projekt selbst). Bei Solo anderer Spuren bleibt sie hörbar.
fn with_voice_track(project: &mut woa_model::Project, sound: woa_model::TrackSound) {
    let solo = project.tracks.iter().any(|t| t.solo);
    let mut t = woa_model::Track::new(
        crate::probeton::VOICE_TRACK,
        "Vorhören",
        woa_model::TrackKind::Midi,
    );
    t.sound = Some(sound);
    t.solo = solo;
    project.tracks.push(t);
}

fn rack_title(s: &str) -> egui::RichText {
    theme::panel_label(s)
}

/// Gefahr-Symbolknopf (Design `IconButton --danger`): ohne Fläche, Symbol gedimmt, erst bei Hover
/// rot. Das Symbol ist die Beschriftung, damit Tests und Bildschirmleser es finden.
fn danger_icon_button(ui: &mut egui::Ui, icon: &str, size: f32, tip: &str) -> egui::Response {
    let s = if size <= 14.0 {
        crate::widgets::IconSize::Sm
    } else {
        crate::widgets::IconSize::Md
    };
    crate::widgets::IconButton::new(icon, tip)
        .size(s)
        .danger()
        .ui(ui)
}

/// Werkzeug-Symbolknopf (Design `IconButton`, 24 px): Ruhe ohne Fläche, Hover mit Fläche.
fn icon_button(ui: &mut egui::Ui, icon: &str, tip: &str) -> egui::Response {
    crate::widgets::IconButton::new(icon, tip)
        .size(crate::widgets::IconSize::Sm)
        .ui(ui)
}

#[cfg(test)]
mod tests {
    #[test]
    fn bar_position_and_level_text() {
        assert_eq!(super::bar_position(0.0).trim(), "1.1.1");
        assert_eq!(super::bar_position(5.5).trim(), "2.2.3");
        assert_eq!(super::level_db_text(0.0), "−∞ dB");
        assert_eq!(super::level_db_text(1.0), "0.0 dB");
    }

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

/// Lässt sich die Vorhör-Wahl ohne Neuöffnen der Sendeausgänge tauschen? Ja, solange weder
/// die alte noch die neue Wahl Kanal 3/4 des Sendegeräts ist (#373/#374).
fn pfl_live_swap(before: &crate::feedback::PflOutput, after: &crate::feedback::PflOutput) -> bool {
    use crate::feedback::PflOutput;
    !matches!(before, PflOutput::Kanal34) && !matches!(after, PflOutput::Kanal34)
}

/// Wann eine neue Vorhören-Wahl greift: sofort, wenn nichts läuft, sonst nach dem Stopp.
/// Die laufende Sendung wird nie für das Vorhören unterbrochen.
#[derive(Debug, Default)]
struct PflSwitch {
    pending: bool,
}

impl PflSwitch {
    /// Neue Wahl. `true` heißt: jetzt neu öffnen.
    fn choose(&mut self, running: bool) -> bool {
        self.pending = running;
        !running
    }

    /// Je Frame. `true` heißt: die wartende Wahl jetzt anwenden.
    fn poll(&mut self, running: bool) -> bool {
        if self.pending && !running {
            self.pending = false;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod pfl_switch_tests {
    use super::PflSwitch;

    #[test]
    fn choice_while_running_waits_until_stop() {
        let mut s = PflSwitch::default();
        assert!(!s.choose(true));
        assert!(s.pending);
        assert!(!s.poll(true));
        assert!(s.pending);
        assert!(s.poll(false));
        assert!(!s.pending);
        assert!(!s.poll(false));
    }

    #[test]
    fn choice_while_idle_applies_now() {
        let mut s = PflSwitch::default();
        assert!(s.choose(false));
        assert!(!s.pending);
        assert!(!s.poll(false));
    }

    /// #373/#374: Ein eigenes Kopfhörer-Gerät wird auch bei laufender Playlist sofort
    /// getauscht; nur Kanal 3/4 hängt am Sendegerät und wartet, bis nichts mehr läuft.
    #[test]
    fn own_headphone_device_switches_live() {
        use crate::feedback::PflOutput;
        let dev = |s: &str| PflOutput::Geraet(s.into());
        assert!(super::pfl_live_swap(&dev("a"), &dev("b")));
        assert!(super::pfl_live_swap(&PflOutput::Aus, &dev("b")));
        assert!(super::pfl_live_swap(&dev("a"), &PflOutput::Aus));
        assert!(!super::pfl_live_swap(&PflOutput::Kanal34, &dev("b")));
        assert!(!super::pfl_live_swap(&dev("a"), &PflOutput::Kanal34));
    }
}

/// Zustand einer Bank als Zusatz im Menü (Paket J7): leer, wenn sie da ist.
/// Anzeige des MIDI-Eingangs im Spurkopf.
/// Position als Takt.Schlag.Sechzehntel (4/4), ab 1 gezählt: 0 Schläge = „1.1.1“.
fn bar_position(beats: f64) -> String {
    let b = beats.max(0.0);
    let bar = (b / 4.0) as u64 + 1;
    let beat = (b % 4.0) as u64 + 1;
    let six = ((b % 1.0) * 4.0) as u64 + 1;
    format!("{bar:>3}.{beat}.{six}")
}

/// Spitzenpegel (linear) als dB-Text, unter −60 dB „−∞“.
fn level_db_text(level: f32) -> String {
    if level <= 0.001 {
        "−∞ dB".to_string()
    } else {
        format!("{:.1} dB", 20.0 * level.log10())
    }
}

fn midi_input_label(status: &woa_io::midi::MidiStatus) -> String {
    use woa_io::midi::MidiStatus;
    match status {
        MidiStatus::Off => "Kein MIDI-Gerät".to_owned(),
        MidiStatus::Connected(n) => n.clone(),
        MidiStatus::Waiting(n) => format!("{n} (nicht verbunden)"),
    }
}

/// Klänge im `SoundPicker`: leere Suche = die Kategorie, sonst Treffer über alle Kategorien
/// (Name oder Kategoriename, ohne Groß/Klein).
fn sound_matches(
    query: &str,
    shown: woa_model::SoundCategory,
) -> Vec<&'static woa_model::SoundPreset> {
    let q = query.trim().to_lowercase();
    woa_model::SOUND_PRESETS
        .iter()
        .filter(|p| {
            if q.is_empty() {
                p.category == shown
            } else {
                p.name.to_lowercase().contains(&q) || p.category.name().to_lowercase().contains(&q)
            }
        })
        .collect()
}

/// Kategoriename und Symbol im `SoundPicker`.
fn category_label(c: woa_model::SoundCategory) -> (&'static str, &'static str) {
    use woa_model::SoundCategory as C;
    let ic = match c {
        C::Klavier => theme::ic::PIANO_KEYS,
        C::Bass => theme::ic::WAVE_SINE,
        C::Gitarre | C::Streicher | C::Blaeser => theme::ic::MUSIC_NOTE,
        C::Synth => theme::ic::CLOUD,
        C::Drums => theme::ic::GRID_FOUR,
    };
    (c.name(), ic)
}

fn bank_hint(state: &woa_engine::banks::BankState) -> String {
    use woa_engine::banks::BankState;
    match state {
        BankState::Ready => String::new(),
        BankState::Missing => " · wird beim Wählen geladen".to_owned(),
        BankState::Downloading { done, total } => {
            format!(" · lädt {} %", done * 100 / (*total).max(1))
        }
        BankState::Failed(e) => format!(" · Fehler: {e}"),
    }
}

#[cfg(test)]
mod party_feedback_tests {
    //! #288: In der Party-Ansicht öffnet die Feedback-Taste ihr Fenster wie im Radio.
    use super::*;

    struct Rig {
        ctx: egui::Context,
        app: WoaApp,
        frame: eframe::Frame,
        n: u32,
        nodes: Vec<(String, egui::Rect)>,
    }

    impl Rig {
        fn step(&mut self, events: Vec<egui::Event>) {
            self.n += 1;
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1600.0, 1000.0),
                )),
                time: Some(f64::from(self.n) * 0.05),
                events,
                ..Default::default()
            };
            input.viewports.entry(egui::ViewportId::ROOT).or_default();
            let (app, frame) = (&mut self.app, &mut self.frame);
            let out = self.ctx.run_ui(input, |ui| {
                eframe::App::logic(app, ui.ctx(), frame);
                eframe::App::ui(app, ui, frame);
            });
            self.nodes.clear();
            if let Some(u) = out.platform_output.accesskit_update {
                for (_, node) in u.nodes {
                    let label = node.label().map(str::to_string).unwrap_or_default();
                    if let Some(b) = node.bounds() {
                        let r = egui::Rect::from_min_max(
                            egui::pos2(b.x0 as f32, b.y0 as f32),
                            egui::pos2(b.x1 as f32, b.y1 as f32),
                        );
                        self.nodes.push((label, r));
                    }
                }
            }
        }

        fn find(&self, text: &str) -> Option<egui::Rect> {
            self.nodes
                .iter()
                .find(|(l, r)| l.contains(text) && r.center().y < 40.0 || l == text)
                .map(|(_, r)| *r)
        }

        fn has(&self, text: &str) -> bool {
            self.nodes.iter().any(|(l, _)| l.contains(text))
        }
    }

    fn rig(mode: Mode) -> (Rig, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let config = GuiConfig {
            telemetry_enabled: Some(false),
            ..GuiConfig::default()
        };
        let mut app = WoaApp::without_audio(config, tmp.path().join("rec"));
        app.radio = Some(RadioDesk::headless(tmp.path()));
        app.config.mode = mode;
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut rig = Rig {
            ctx,
            app,
            frame: eframe::Frame::_new_kittest(),
            n: 0,
            nodes: Vec::new(),
        };
        rig.step(Vec::new());
        rig.step(Vec::new());
        (rig, tmp)
    }

    fn feedback_opens(mode: Mode) -> bool {
        let (mut rig, _tmp) = rig(mode);
        let at = rig.find("Feedback").expect("Feedback-Taste oben").center();
        let ev = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        rig.step(vec![egui::Event::PointerMoved(at)]);
        rig.step(vec![ev(true)]);
        rig.step(vec![ev(false)]);
        rig.step(Vec::new());
        // Das Bildschirmfoto, das die Taste anfordert, kommt wie vom Backend zurück.
        let image =
            std::sync::Arc::new(egui::ColorImage::new([2, 2], vec![egui::Color32::BLACK; 4]));
        rig.step(vec![egui::Event::Screenshot {
            viewport_id: egui::ViewportId::ROOT,
            user_data: egui::UserData::default(),
            image,
        }]);
        rig.step(Vec::new());
        rig.step(Vec::new());
        assert_eq!(rig.app.config.mode, mode, "Ansicht bleibt");
        rig.has("Abschicken")
    }

    #[test]
    fn feedback_window_opens_in_party_view() {
        assert!(
            feedback_opens(Mode::Party),
            "Feedback-Fenster in der Party-Ansicht"
        );
    }

    #[test]
    fn feedback_window_opens_in_radio_view() {
        assert!(feedback_opens(Mode::Radio));
    }

    /// André 02.11.2026: Schließen-X im Feedback-Fenster.
    #[test]
    fn feedback_window_has_close_x() {
        let (mut rig, _tmp) = rig(Mode::Radio);
        rig.app.feedback.open();
        rig.step(Vec::new());
        rig.step(Vec::new());
        assert!(rig.has("Feedback schließen"), "Schließen-X fehlt");
    }
}

#[cfg(test)]
mod nacharbeit_tests {
    //! Design-Runde 3, Nacharbeit: Fades in ms auch bei Mehrfachauswahl, Werkzeuge und Dock nur
    //! als Symbol, Transport ohne `egui::Button`, F11 schaltet Vollbild.
    use super::*;

    struct Rig {
        ctx: egui::Context,
        app: WoaApp,
        frame: eframe::Frame,
        n: u32,
        /// Beschriftung, Wert und Fläche jedes Knotens.
        nodes: Vec<(String, String, egui::Rect)>,
        commands: Vec<egui::ViewportCommand>,
        /// Fenstergröße in Punkten (Standard 1920 × 1030).
        size: egui::Vec2,
        /// Skalierung wie unter Windows (1,0 = 100 %, 1,25 = 125 %).
        ppp: f32,
        _tmp: tempfile::TempDir,
    }

    impl Rig {
        fn new(mode: Mode) -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let config = GuiConfig {
                telemetry_enabled: Some(false),
                ..GuiConfig::default()
            };
            let mut app = WoaApp::without_audio(config, tmp.path().join("rec"));
            if mode != Mode::Produzent {
                app.radio = Some(RadioDesk::headless(tmp.path()));
            }
            app.config.mode = mode;
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            let mut rig = Self {
                ctx,
                app,
                frame: eframe::Frame::_new_kittest(),
                n: 0,
                nodes: Vec::new(),
                commands: Vec::new(),
                size: egui::vec2(1920.0, 1030.0),
                ppp: 1.0,
                _tmp: tmp,
            };
            rig.step(Vec::new());
            rig.step(Vec::new());
            rig
        }

        fn step(&mut self, events: Vec<egui::Event>) {
            self.n += 1;
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)),
                time: Some(f64::from(self.n) * 0.05),
                events,
                ..Default::default()
            };
            input
                .viewports
                .entry(egui::ViewportId::ROOT)
                .or_default()
                .native_pixels_per_point = Some(self.ppp);
            let (app, frame) = (&mut self.app, &mut self.frame);
            let out = self.ctx.run_ui(input, |ui| {
                eframe::App::logic(app, ui.ctx(), frame);
                eframe::App::ui(app, ui, frame);
            });
            if let Some(v) = out.viewport_output.get(&egui::ViewportId::ROOT) {
                self.commands.extend(v.commands.iter().cloned());
            }
            self.nodes.clear();
            if let Some(u) = out.platform_output.accesskit_update {
                for (_, node) in u.nodes {
                    let label = node.label().map(str::to_string).unwrap_or_default();
                    let value = node.value().map(str::to_string).unwrap_or_default();
                    if let Some(b) = node.bounds() {
                        let r = egui::Rect::from_min_max(
                            egui::pos2(b.x0 as f32, b.y0 as f32),
                            egui::pos2(b.x1 as f32, b.y1 as f32),
                        );
                        self.nodes.push((label, value, r));
                    }
                }
            }
        }

        fn any(&self, f: impl Fn(&str) -> bool) -> bool {
            self.nodes
                .iter()
                .any(|(l, v, r)| r.width() > 0.0 && (f(l) || f(v)))
        }
    }

    /// Rumpf einer Funktion in `app.rs`, für Bauregeln (kein `egui::Button` in Hardware).
    fn body_of<'a>(src: &'a str, head: &str) -> &'a str {
        let a = src.find(head).unwrap_or_else(|| panic!("{head} fehlt"));
        let rest = &src[a + head.len()..];
        let b = ["\n    fn ", "\n    pub(super) fn "]
            .iter()
            .filter_map(|p| rest.find(p))
            .min()
            .or_else(|| rest.find("\nfn "))
            .unwrap_or(rest.len());
        &rest[..b]
    }

    // Untermodule zuerst: `app.rs` enthält diese Tests samt den gesuchten Köpfen als Text.
    const SRC: &str = concat!(
        include_str!("app/transport.rs"),
        include_str!("app/project_io.rs"),
        include_str!("app/menu.rs"),
        include_str!("app/tracks.rs"),
        include_str!("app/timeline.rs"),
        include_str!("app/mixer.rs"),
        include_str!("app/updates.rs"),
        include_str!("app.rs"),
    );

    #[test]
    fn multi_selection_shows_fades_in_ms() {
        let mut rig = Rig::new(Mode::Produzent);
        let st = rig.app.state_mut();
        let t = st.add_midi_track().unwrap();
        let a = st
            .add_midi_clip(Some(t), "a", 0.0, 4.0, woa_model::MidiData::default())
            .unwrap();
        let b = st
            .add_midi_clip(Some(t), "b", 8.0, 4.0, woa_model::MidiData::default())
            .unwrap();
        st.set_fades(a, 1.0, 0.5);
        st.set_fades(b, 1.0, 0.25);
        rig.app.selected_clip = Some(a);
        rig.app.selected_clips = vec![a, b];
        rig.step(Vec::new());
        rig.step(Vec::new());
        // 120 BPM: ein Schlag sind 500 ms; Fade-out ist verschieden und steht als „—“.
        assert!(rig.any(|s| s == "500 ms"), "Fade-in aller Clips in ms");
        assert!(!rig.any(|s| s == "1.00"), "keine Schläge mehr im Feld");
    }

    #[test]
    fn inspector_tools_are_icon_buttons() {
        for head in [
            "fn icon_button(",
            "fn danger_icon_button(",
            "fn clip_notes(",
            "fn clip_time(",
        ] {
            let body = body_of(SRC, head);
            assert!(
                !body.contains("egui::Button"),
                "{head} zeichnet noch einen egui-Knopf"
            );
        }
        assert!(body_of(SRC, "fn icon_button(").contains("IconButton::new"));
    }

    #[test]
    fn dock_detach_and_collapse_are_symbols_only() {
        let mut rig = Rig::new(Mode::Produzent);
        rig.app.studio_dock = StudioDockTab::PianoRoll;
        rig.step(Vec::new());
        rig.step(Vec::new());
        for word in ["Dock einklappen", "Als Fenster lösen", "Im Dock einbetten"] {
            assert!(
                !rig.any(|s| s.contains(word)),
                "„{word}“ steht noch als Text"
            );
        }
        assert!(
            rig.any(|s| s == theme::ic::CARET_DOWN),
            "Einklappen als Symbol"
        );
        assert!(rig.any(|s| s == theme::ic::ARROWS_OUT), "Lösen als Symbol");
    }

    /// Rack-Höhe (André 02.11.2026): „Ausgang / Pan“ liegt bei 1920 × 1030 im sichtbaren Dock.
    #[test]
    fn rack_output_and_pan_fit_the_dock() {
        for (midi, effects) in [(false, 0), (true, 0), (true, 8)] {
            let mut rig = Rig::new(Mode::Produzent);
            let st = rig.app.state_mut();
            let t = if midi {
                st.add_midi_track().unwrap()
            } else {
                st.add_track().unwrap()
            };
            for _ in 0..effects {
                st.add_insert(t, woa_model::EffectKind::Eq).unwrap();
            }
            rig.app.select_track(t);
            rig.app.studio_dock = StudioDockTab::Mixer;
            for _ in 0..4 {
                rig.step(Vec::new());
            }
            let find = |f: &dyn Fn(&str, &str) -> bool| {
                rig.nodes
                    .iter()
                    .filter(|(l, v, r)| r.width() > 0.0 && f(l, v))
                    .map(|n| n.2)
                    .next()
            };
            let ausgang = find(&|l, v| l == "AUSGANG" || v == "AUSGANG").expect("AUSGANG");
            let pan = find(&|_, v| v == "Pan C").expect("Pan im Rack");
            let dock = find(&|l, v| l == "STUDIO-DOCK" || v == "STUDIO-DOCK").expect("Dock");
            eprintln!("midi={midi} dock={dock:?} ausgang={ausgang:?} pan={pan:?}");
            assert!(ausgang.top() > dock.bottom(), "AUSGANG im Dock");
            // Unter dem Dock liegt die Statuszeile (~24 px): Pan muss darüber enden.
            assert!(
                pan.bottom() <= 1030.0 - 24.0,
                "Pan unten abgeschnitten (midi={midi}, fx={effects}): {pan:?}"
            );
        }
    }

    /// #443: Rack mit offener Klangauswahl bei 1280 × 720, 1366 × 768 und 1920 × 1030: nichts
    /// ragt rechts aus dem Fenster („Strei…“, Vorhören-Knopf am Rand).
    #[test]
    fn sound_picker_fits_the_window() {
        for size in [(1280.0, 720.0), (1366.0, 768.0), (1920.0, 1030.0)] {
            let mut rig = Rig::new(Mode::Produzent);
            rig.size = egui::vec2(size.0, size.1);
            let t = rig.app.state_mut().add_midi_track().unwrap();
            rig.app.open_sound_picker(t);
            rig.app.studio_dock = StudioDockTab::Mixer;
            for _ in 0..4 {
                rig.step(Vec::new());
            }
            assert!(rig.any(|s| s.contains("Streicher")), "Klangauswahl offen");
            let bad: Vec<_> = rig
                .nodes
                .iter()
                .filter(|(_, _, r)| {
                    r.is_positive() && r.width() < size.0 * 0.8 && r.max.x > size.0 + 0.5
                })
                .collect();
            assert!(bad.is_empty(), "{size:?}: rechts abgeschnitten: {bad:?}");
        }
    }

    /// #443: Transport bei 1280 × 720, 1366 × 768 und 1920 × 1030, auch mit Loop und Punch an:
    /// keine zwei Bedienelemente liegen übereinander, die Pegel-LED-Kette ist da.
    #[test]
    fn transport_never_overlaps() {
        for size in [(1280.0, 720.0), (1366.0, 768.0), (1920.0, 1030.0)] {
            for extra in [false, true] {
                let mut rig = Rig::new(Mode::Produzent);
                rig.size = egui::vec2(size.0, size.1);
                rig.app.loop_enabled = extra;
                rig.app.punch_enabled = extra;
                for _ in 0..4 {
                    rig.step(Vec::new());
                }
                let dock = rig
                    .nodes
                    .iter()
                    .find(|(l, v, _)| l == "STUDIO-DOCK" || v == "STUDIO-DOCK")
                    .map(|n| n.2)
                    .expect("Dock");
                let pegel = rig
                    .nodes
                    .iter()
                    .find(|(l, v, _)| l == "PEGEL" || v == "PEGEL")
                    .map(|n| n.2)
                    .expect("Pegel-LCD im Transport");
                // Bedienelemente des Transports: zwischen Menüleiste und Arbeitsfläche.
                let items: Vec<_> = rig
                    .nodes
                    .iter()
                    .filter(|(l, v, r)| {
                        r.is_positive()
                            && r.top() > 44.0
                            && r.bottom() < dock.top().min(160.0)
                            && !(l.is_empty() && v.is_empty())
                    })
                    .collect();
                for (i, a) in items.iter().enumerate() {
                    for b in &items[i + 1..] {
                        let x = a.2.intersect(b.2);
                        let nested = a.2.contains_rect(b.2) || b.2.contains_rect(a.2);
                        assert!(
                            nested || !x.is_positive() || x.area() < 4.0,
                            "{size:?} loop/punch={extra}: {a:?} überdeckt {b:?}"
                        );
                    }
                }
                assert!(pegel.right() <= size.0, "{size:?}: Pegel außerhalb");
            }
        }
    }

    /// Transport-Pegel (André 02.11.2026): LED-Kette mit dB-Zahl, das analoge VU nur in der
    /// Pegelansicht des Analyse-Fensters, dort umschaltbar Analog/Modern/Aus.
    #[test]
    fn transport_level_is_a_led_chain_and_vu_lives_in_analysis() {
        let body = body_of(SRC, "fn transport_bar(");
        assert!(!body.contains("vu_meter("), "Mini-VU noch im Transport");
        assert!(body.contains("led_meter("), "LED-Kette fehlt im Transport");
        let lv = body_of(SRC, "fn level_view(");
        assert!(lv.contains("vu_meter(") && lv.contains("VuView::ALL"));
        assert!(body_of(SRC, "fn analysis_window(").contains("level_view("));
    }

    /// Piano-Roll öffnet im Dock; „Als Fenster lösen“ wird gemerkt (André 02.11.2026).
    #[test]
    fn piano_roll_opens_docked_and_window_choice_is_remembered() {
        let mut rig = Rig::new(Mode::Produzent);
        assert!(!rig.app.piano_roll_floating, "Vorgabe: Dock");
        let st = rig.app.state_mut();
        let t = st.add_midi_track().unwrap();
        let c = st
            .add_midi_clip(Some(t), "a", 0.0, 4.0, woa_model::MidiData::default())
            .unwrap();
        rig.app.open_piano_roll(c);
        assert_eq!(rig.app.studio_dock, StudioDockTab::PianoRoll);
        assert!(body_of(SRC, "fn studio_dock_panel(").contains("config.piano_roll_window ="));
        let config = GuiConfig {
            telemetry_enabled: Some(false),
            piano_roll_window: true,
            ..GuiConfig::default()
        };
        let tmp = tempfile::tempdir().unwrap();
        let app = WoaApp::without_audio(config, tmp.path().join("rec"));
        assert!(app.piano_roll_floating, "gemerkte Wahl: Fenster");
    }

    #[test]
    fn transport_export_latency_and_ruler_are_hardware_keys() {
        let body = body_of(SRC, "fn transport_bar(");
        let from = body.find("// 4. Latenz").expect("Latenz-Gruppe");
        assert!(
            !body[from..].contains("egui::Button") && !body[from..].contains("segment("),
            "Latenz, Export oder Takte/Zeit noch als egui-Knopf"
        );
    }

    #[test]
    fn whole_transport_has_no_egui_button() {
        let body = body_of(SRC, "fn transport_bar(");
        assert!(
            !body.contains("egui::Button"),
            "Transport noch mit egui::Button (Zurück, Play, Aufnahme, Loop, Punch, Stopp am Ende)"
        );
    }

    #[test]
    fn producer_segments_use_the_widget() {
        assert!(
            !SRC.contains("\nfn segment("),
            "Dock-Reiter noch über die alte segment()-Hilfe"
        );
        assert!(
            SRC.contains("widgets::Segment::new("),
            "Segment-Baustein fehlt"
        );
    }

    fn f11() -> Vec<egui::Event> {
        vec![egui::Event::Key {
            key: egui::Key::F11,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]
    }

    #[test]
    fn maximize_is_sent_once_by_command_on_first_frame() {
        let maximized = |rig: &Rig| {
            rig.commands
                .iter()
                .filter(|c| matches!(c, egui::ViewportCommand::Maximized(true)))
                .count()
        };
        let mut rig = Rig::new(Mode::Produzent);
        assert_eq!(maximized(&rig), 0);
        rig.app.maximize_on_start();
        rig.step(Vec::new());
        rig.step(Vec::new());
        assert_eq!(maximized(&rig), 1);
    }

    fn fullscreen_cmds(rig: &Rig) -> Vec<bool> {
        rig.commands
            .iter()
            .filter_map(|c| match c {
                egui::ViewportCommand::Fullscreen(on) => Some(*on),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn f11_toggles_fullscreen_in_producer_and_radio() {
        for mode in [Mode::Produzent, Mode::Radio] {
            let mut rig = Rig::new(mode);
            rig.step(f11());
            assert_eq!(fullscreen_cmds(&rig), vec![true], "{mode:?}");
        }
    }

    #[test]
    fn party_fullscreen_follows_the_window() {
        let mut rig = Rig::new(Mode::Party);
        // Das Fenster startet im Vollbild (main.rs): die Party-Taste zeigt das und F11 schaltet aus.
        let input_fs = |rig: &mut Rig, ev: Vec<egui::Event>| {
            rig.n += 1;
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1920.0, 1030.0),
                )),
                time: Some(f64::from(rig.n) * 0.05),
                events: ev,
                ..Default::default()
            };
            input
                .viewports
                .entry(egui::ViewportId::ROOT)
                .or_default()
                .fullscreen = Some(true);
            let (app, frame) = (&mut rig.app, &mut rig.frame);
            let out = rig.ctx.run_ui(input, |ui| {
                eframe::App::logic(app, ui.ctx(), frame);
                eframe::App::ui(app, ui, frame);
            });
            if let Some(v) = out.viewport_output.get(&egui::ViewportId::ROOT) {
                rig.commands.extend(v.commands.iter().cloned());
            }
        };
        input_fs(&mut rig, Vec::new());
        assert!(rig.app.party.state.fullscreen, "Party kennt das Vollbild");
        input_fs(&mut rig, f11());
        assert_eq!(fullscreen_cmds(&rig), vec![false]);
    }

    #[test]
    fn f11_cannot_become_a_cart_key() {
        assert!(crate::radio::RESERVED_KEYS.contains(&egui::Key::F11));
    }

    #[test]
    fn menu_groups_are_sorted_and_without_dots() {
        for (menu, group) in MENU_GROUPS {
            for (label, _) in *group {
                assert!(
                    !label.contains('…') && !label.contains("..."),
                    "{menu}: „{label}“ mit Punkten"
                );
            }
            let keys: Vec<String> = group.iter().map(|(l, _)| de_sort_key(l)).collect();
            let mut sorted = keys.clone();
            sorted.sort();
            assert_eq!(keys, sorted, "{menu} nicht alphabetisch");
        }
        assert!(de_sort_key("Öffnen") < de_sort_key("Speichern"));
        assert!(de_sort_key("Ausgabe") < de_sort_key("Ausgänge"));
    }

    #[test]
    fn menu_code_follows_the_sorted_groups() {
        let body = format!(
            "{}{}",
            body_of(SRC, "fn menu_items("),
            body_of(SRC, "fn entwickler_menu(")
        );
        for (menu, group) in MENU_GROUPS {
            let mut last = 0;
            for (label, needle) in *group {
                let at = body
                    .find(needle)
                    .unwrap_or_else(|| panic!("{menu}: „{label}“ fehlt im Menü ({needle})"));
                assert!(
                    at >= last,
                    "{menu}: „{label}“ steht nicht in der Reihenfolge"
                );
                last = at;
            }
        }
        for head in [
            "fn menu_items(",
            "fn entwickler_menu(",
            "fn update_menu(",
            "fn midi_menu(",
            "fn theme_menu(",
        ] {
            for line in body_of(SRC, head).lines() {
                let item = line.contains("button(") || line.contains("checkbox(");
                assert!(
                    !(item && (line.contains('…') || line.contains("..."))),
                    "Menüeintrag mit Punkten: {line}"
                );
            }
        }
    }

    /// #365/#362: Fenster schließen und Strg+Q fragen bei ungespeichertem Projekt mit
    /// Speichern · Nicht speichern · Abbrechen; Abbrechen lässt WOA offen.
    #[test]
    fn closing_with_unsaved_changes_asks_save_discard_cancel() {
        let mut rig = Rig::new(Mode::Produzent);
        rig.app.state_mut().add_midi_track().unwrap();
        assert!(rig.app.state.session().is_dirty());
        // Fenster schließen (X).
        rig.n += 1;
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1920.0, 1030.0),
            )),
            time: Some(f64::from(rig.n) * 0.05),
            ..Default::default()
        };
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .events
            .push(egui::ViewportEvent::Close);
        let (app, frame) = (&mut rig.app, &mut rig.frame);
        let out = rig.ctx.run_ui(input, |ui| {
            eframe::App::logic(app, ui.ctx(), frame);
            eframe::App::ui(app, ui, frame);
        });
        let cmds = &out.viewport_output[&egui::ViewportId::ROOT].commands;
        assert!(
            cmds.contains(&egui::ViewportCommand::CancelClose),
            "Schließen nicht angehalten: {cmds:?}"
        );
        rig.step(Vec::new());
        for b in ["Speichern und beenden", "Nicht speichern", "Abbrechen"] {
            assert!(rig.any(|l| l == b), "Knopf „{b}“ fehlt");
        }
        // Esc = Abbrechen: Frage weg, WOA bleibt offen.
        rig.commands.clear();
        rig.step(vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        rig.step(Vec::new());
        assert!(!rig.any(|l| l == "Nicht speichern"), "Frage nicht zu");
        assert!(!rig.commands.contains(&egui::ViewportCommand::Close));
        // Strg+Q fragt dasselbe.
        rig.step(vec![egui::Event::Key {
            key: egui::Key::Q,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }]);
        rig.step(Vec::new());
        assert!(rig.any(|l| l == "Nicht speichern"), "Strg+Q fragt nicht");
        assert!(!rig.commands.contains(&egui::ViewportCommand::Close));
    }

    /// #408: Neu und Öffnen mit ungespeicherten Änderungen fragen Speichern · Nicht
    /// speichern · Abbrechen; ein zweiter Klick öffnet keine zweite Frage, Verwerfen
    /// macht weiter, ohne zu speichern.
    #[test]
    fn new_with_unsaved_changes_offers_discard_and_asks_once() {
        let mut rig = Rig::new(Mode::Produzent);
        rig.app.state_mut().add_midi_track().unwrap();
        assert!(rig.app.state.session().is_dirty());
        rig.app.new_project();
        rig.app.new_project();
        assert_eq!(rig.app.replace_ask, Some(Replace::New));
        rig.step(Vec::new());
        rig.step(Vec::new());
        for b in ["Speichern", "Nicht speichern", "Abbrechen"] {
            assert!(rig.any(|l| l == b), "Knopf „{b}“ fehlt");
        }
        let n = rig
            .nodes
            .iter()
            .filter(|(l, _, r)| l == "Nicht speichern" && r.width() > 0.0)
            .count();
        assert_eq!(n, 1, "Frage doppelt");
        let r = rig
            .nodes
            .iter()
            .find(|(l, _, r)| l == "Nicht speichern" && r.width() > 0.0)
            .map(|(_, _, r)| *r)
            .unwrap();
        let pos = r.center();
        let ev = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        rig.step(vec![egui::Event::PointerMoved(pos)]);
        rig.step(vec![ev(true)]);
        rig.step(vec![ev(false)]);
        rig.step(Vec::new());
        assert!(rig.app.replace_ask.is_none(), "Frage nicht zu");
        assert!(rig.app.state.project().tracks.is_empty(), "nicht verworfen");
        assert!(rig.app.state.needs_save_as(), "doch gespeichert");
    }

    /// #408: Abbrechen lässt das Projekt, wie es ist.
    #[test]
    fn open_with_unsaved_changes_cancel_keeps_project() {
        let mut rig = Rig::new(Mode::Produzent);
        rig.app.state_mut().add_midi_track().unwrap();
        rig.app.open();
        rig.step(Vec::new());
        rig.step(vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert!(rig.app.replace_ask.is_none());
        assert_eq!(rig.app.state.project().tracks.len(), 1);
        assert!(rig.app.state.session().is_dirty());
    }

    /// #441: Esc schließt „Über WOA“ wie die übrigen Fenster.
    #[test]
    fn escape_closes_about_window() {
        let mut rig = Rig::new(Mode::Produzent);
        rig.app.about_open = true;
        rig.step(Vec::new());
        assert!(rig.app.about_open);
        rig.step(vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert!(!rig.app.about_open, "Esc schließt Über WOA nicht");
    }

    fn esc() -> egui::Event {
        egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// #441: Esc schließt die Bibliothek (Party › Ansicht › Bibliothek).
    #[test]
    fn escape_closes_library_in_party() {
        let mut rig = Rig::new(Mode::Party);
        rig.app.library.open = true;
        rig.step(Vec::new());
        rig.step(vec![esc()]);
        assert!(!rig.app.library.open, "Esc schließt die Bibliothek nicht");
    }

    /// #441: Esc schließt die Analyse als Fenster.
    #[test]
    fn escape_closes_analysis_window() {
        let mut rig = Rig::new(Mode::Produzent);
        rig.app.analysis_open = true;
        // Ohne Audio legt erst der Player die Ansicht an.
        rig.app.analysis = Some(crate::analysis::AnalysisView::new(48_000));
        rig.step(Vec::new());
        rig.step(Vec::new());
        assert!(rig.any(|l| l == "Analyse"), "Analyse-Fenster fehlt");
        rig.step(vec![esc()]);
        assert!(!rig.app.analysis_open, "Esc schließt die Analyse nicht");
    }

    /// #441/#408: Esc in „Projekt speichern?“ beim Beenden heißt Abbrechen, in jeder Ansicht.
    #[test]
    fn escape_cancels_exit_question_in_every_view() {
        for mode in [Mode::Produzent, Mode::Radio, Mode::Party] {
            let mut rig = Rig::new(mode);
            assert!(!rig.app.exit.request(crate::exit::ExitReason::Quit, true));
            rig.step(Vec::new());
            assert!(rig.any(|l| l == "Nicht speichern"), "{mode:?}: Frage fehlt");
            rig.step(vec![esc()]);
            assert!(
                rig.app.exit.asking().is_none(),
                "{mode:?}: Esc bricht nicht ab"
            );
            assert!(!rig.commands.contains(&egui::ViewportCommand::Close));
        }
    }

    /// #441: Esc schließt Einstellungen und „Fragen und To-dos“ (gemeinsame Regel).
    #[test]
    fn escape_closes_settings_and_questions() {
        let mut rig = Rig::new(Mode::Produzent);
        rig.app.settings.open = true;
        rig.step(Vec::new());
        rig.step(vec![esc()]);
        assert!(!rig.app.settings.open, "Einstellungen bleiben offen");
        rig.app.config.entwickler = true;
        rig.app.fragen.open = true;
        rig.step(Vec::new());
        rig.step(vec![esc()]);
        assert!(!rig.app.fragen.open, "Fragen und To-dos bleiben offen");
    }

    /// #441: Ein Esc schließt nur das oberste Fenster, nicht alle.
    #[test]
    fn escape_closes_only_the_top_window() {
        let mut rig = Rig::new(Mode::Produzent);
        rig.app.library.open = true;
        rig.step(Vec::new());
        rig.app.about_open = true;
        rig.step(Vec::new());
        rig.step(vec![esc()]);
        assert!(!rig.app.about_open, "oberstes Fenster bleibt offen");
        assert!(rig.app.library.open, "darunter liegendes Fenster mit zu");
        rig.step(vec![esc()]);
        assert!(
            !rig.app.library.open,
            "zweites Esc schließt die Bibliothek nicht"
        );
    }

    impl Rig {
        /// Linksklick auf `pos` (Bewegen, Drücken, Loslassen).
        fn click_at(&mut self, pos: egui::Pos2) {
            let ev = |pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            self.step(vec![egui::Event::PointerMoved(pos)]);
            self.step(vec![ev(true)]);
            self.step(vec![ev(false)]);
            self.step(Vec::new());
        }

        fn rect_of(&self, label: &str) -> egui::Rect {
            self.nodes
                .iter()
                .find(|(l, _, r)| l == label && r.width() > 0.0)
                .map(|(_, _, r)| *r)
                .unwrap_or_else(|| panic!("„{label}“ nicht zu sehen"))
        }
    }

    /// Einstellungen › Audio und Geräte › „In Audio-Geräte öffnen“ wie in der Abnahme.
    fn open_audio_from_settings(rig: &mut Rig) {
        rig.app.settings.open = true;
        rig.app.settings.open_area(crate::settings::Area::Audio);
        rig.step(Vec::new());
        rig.step(Vec::new());
        let knopf = format!("In {} öffnen", crate::outputs_ui::WINDOW);
        let pos = rig.rect_of(&knopf).center();
        rig.click_at(pos);
        assert!(rig.app.outputs_ui.is_open(), "Audio-Geräte nicht offen");
        assert!(rig.app.settings.open);
    }

    /// #441 (Abnahme 0.41.24): Einstellungen und Audio-Geräte offen: Esc schließt zuerst das
    /// oberste Fenster Audio-Geräte, das zweite Esc die Einstellungen.
    #[test]
    fn escape_closes_audio_devices_above_settings() {
        let mut rig = Rig::new(Mode::Produzent);
        open_audio_from_settings(&mut rig);
        rig.step(vec![esc()]);
        assert!(
            !rig.app.outputs_ui.is_open(),
            "Esc schließt Audio-Geräte nicht"
        );
        assert!(
            rig.app.settings.open,
            "Esc schließt die Einstellungen darunter"
        );
        rig.step(Vec::new());
        rig.step(vec![esc()]);
        assert!(
            !rig.app.settings.open,
            "zweites Esc schließt die Einstellungen nicht"
        );
    }

    /// #441: Audio-Geräte allein schließt mit Esc, auch nach einem Klick ins Fenster.
    #[test]
    fn escape_closes_audio_devices_alone_after_click() {
        for klick in [false, true] {
            let mut rig = Rig::new(Mode::Produzent);
            open_audio_from_settings(&mut rig);
            rig.app.settings.open = false;
            rig.step(Vec::new());
            rig.step(Vec::new());
            if klick {
                // Klick ins Fenster: „Geräte neu lesen“ liest nur die Liste, schließt nichts.
                let pos = rig.rect_of("Geräte neu lesen").center();
                rig.click_at(pos);
                assert!(rig.app.outputs_ui.is_open());
            }
            rig.step(vec![esc()]);
            assert!(
                !rig.app.outputs_ui.is_open(),
                "Esc schließt Audio-Geräte nicht (Klick: {klick})"
            );
        }
    }

    /// Kurzer Ton als WAV (nur Datei, nichts wird abgespielt).
    fn test_wav(path: &std::path::Path, hz: f32) {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..48_000 * 4 {
            let s = (i as f32 * hz * std::f32::consts::TAU / 48_000.0).sin();
            w.write_sample((s * 6000.0) as i16).unwrap();
            w.write_sample((s * 6000.0) as i16).unwrap();
        }
        w.finalize().unwrap();
    }

    /// #443 (Abnahme 0.41.24): Party › DJ mit zwei geladenen Titeln bei 1280 px Breite
    /// (Fenster 1280 × 800 und 1280 × 720, auch 125 % Skalierung wie unter Windows): kein
    /// Bedienelement ragt rechts oder unten aus dem Fenster, Deck B samt Jog-Scheibe und die
    /// Bibliothek rechts unten bleiben ganz sichtbar.
    #[test]
    fn party_dj_with_loaded_decks_fits_1280() {
        use woa_radio::control::DjCommand;
        let mut fails = Vec::new();
        for (w, h, ppp) in [
            (1280.0_f32, 800.0_f32, 1.0_f32),
            (1280.0, 720.0, 1.0),
            (1280.0, 800.0, 1.25),
            (1280.0, 720.0, 1.25),
        ] {
            let mut rig = Rig::new(Mode::Party);
            rig.ppp = ppp;
            rig.size = egui::vec2(w / ppp, h / ppp);
            let dir = rig._tmp.path().join("titel");
            std::fs::create_dir_all(&dir).unwrap();
            let files: Vec<_> = [
                ("Erster Testtitel mit langem Namen", 220.0),
                ("Zweiter Testtitel", 330.0),
            ]
            .iter()
            .map(|(n, hz)| {
                let p = dir.join(format!("{n}.wav"));
                test_wav(&p, *hz);
                p
            })
            .collect();
            {
                let r = rig.app.radio.as_ref().unwrap();
                r.append_files(files);
                r.dj(DjCommand::View { dj: true });
            }
            let t0 = std::time::Instant::now();
            let mut loaded = false;
            while t0.elapsed() < std::time::Duration::from_secs(8) {
                rig.step(Vec::new());
                let r = rig.app.radio.as_ref().unwrap();
                let v = r.view();
                if v.playlist.len() == 2 && !loaded {
                    r.dj(DjCommand::Load { side: 0, index: 0 });
                    r.dj(DjCommand::Load { side: 1, index: 1 });
                    loaded = true;
                }
                if v.dj
                    .as_ref()
                    .is_some_and(|d| d.dj && d.decks.iter().all(|k| k.index.is_some()))
                {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            // Auf das Ende der Analyse warten: „Analyse fehlgeschlagen“ samt Knopf ist der
            // breiteste Deck-Kopf (unter Last kam er erst spät, der Test war dann zufällig rot).
            let t1 = std::time::Instant::now();
            while t1.elapsed() < std::time::Duration::from_secs(20)
                && rig.any(|l| l.ends_with("Analyse"))
            {
                std::thread::sleep(std::time::Duration::from_millis(20));
                rig.step(Vec::new());
            }
            for _ in 0..4 {
                rig.step(Vec::new());
            }
            assert!(
                !rig.any(|l| l.ends_with("Analyse")),
                "{w}×{h} @{ppp}: Analyse läuft noch"
            );
            let dj = rig.app.radio.as_ref().unwrap().view().dj.clone();
            assert!(
                dj.as_ref()
                    .is_some_and(|d| d.dj && d.decks.iter().all(|k| k.index.is_some())),
                "{w}×{h} @{ppp}: Decks nicht geladen"
            );
            assert!(rig.any(|l| l == "DECK B"), "{w}×{h} @{ppp}: Deck B fehlt");
            let (sw, sh) = (rig.size.x, rig.size.y);
            let bad: Vec<_> = rig
                .nodes
                .iter()
                .filter(|(l, _, r)| {
                    // Waagerecht: nichts über den rechten Rand. Senkrecht nur, was immer zu
                    // sehen sein muss (Listen darunter rollen in ihrer Karte); bei 125 % sind
                    // es nur 576 bzw. 640 Punkte Höhe, dort rollt die DJ-Ansicht senkrecht.
                    let must = ppp == 1.0
                        && (l == "DECK B" || l.ends_with(" AN") || l.ends_with("WUNSCH"));
                    r.is_positive()
                        && r.width() < sw * 0.8
                        && (r.max.x > sw + 0.5 || (must && r.max.y > sh + 0.5))
                })
                .map(|(l, v, r)| (l.clone(), v.clone(), *r))
                .collect();
            if !bad.is_empty() {
                fails.push(format!("{w}×{h} @{ppp} ({sw}×{sh} pt): {bad:?}"));
            }
        }
        assert!(
            fails.is_empty(),
            "Party DJ abgeschnitten:\n{}",
            fails.join("\n")
        );
    }

    /// #441: Eine offene Frage geht vor; das Fenster darunter bleibt.
    #[test]
    fn escape_cancels_question_before_window() {
        let mut rig = Rig::new(Mode::Produzent);
        rig.app.library.open = true;
        rig.step(Vec::new());
        assert!(!rig.app.exit.request(crate::exit::ExitReason::Quit, true));
        rig.step(Vec::new());
        rig.step(vec![esc()]);
        assert!(rig.app.exit.asking().is_none(), "Frage bleibt");
        assert!(rig.app.library.open, "Bibliothek mit zu");
    }

    /// #442: Bei Skalierung 125 % und 150 % (maximiert 1920 × 1030 Pixel) trifft ein Klick den
    /// Knopf ganz rechts oben. Prüft die Umrechnung in egui; den Windows-Fensterfehler selbst
    /// zeigt nur das Gerät (Prüfpunkt `maximiert-klicks`).
    #[test]
    fn click_hits_right_edge_when_scaled() {
        for ppp in [1.25_f32, 1.5] {
            let mut rig = Rig::new(Mode::Radio);
            let step = |rig: &mut Rig, events: Vec<egui::Event>| {
                rig.n += 1;
                let mut input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1920.0 / ppp, 1030.0 / ppp),
                    )),
                    time: Some(f64::from(rig.n) * 0.05),
                    events,
                    ..Default::default()
                };
                input
                    .viewports
                    .entry(egui::ViewportId::ROOT)
                    .or_default()
                    .native_pixels_per_point = Some(ppp);
                let (app, frame) = (&mut rig.app, &mut rig.frame);
                let out = rig.ctx.run_ui(input, |ui| {
                    eframe::App::logic(app, ui.ctx(), frame);
                    eframe::App::ui(app, ui, frame);
                });
                rig.nodes.clear();
                if let Some(u) = out.platform_output.accesskit_update {
                    for (_, node) in u.nodes {
                        let label = node.label().map(str::to_string).unwrap_or_default();
                        if let Some(b) = node.bounds() {
                            // egui meldet AccessKit-Flächen in Punkten, wie die Ereignisse.
                            let r = egui::Rect::from_min_max(
                                egui::pos2(b.x0 as f32, b.y0 as f32),
                                egui::pos2(b.x1 as f32, b.y1 as f32),
                            );
                            rig.nodes.push((label, String::new(), r));
                        }
                    }
                }
            };
            step(&mut rig, Vec::new());
            step(&mut rig, Vec::new());
            assert!((rig.ctx.pixels_per_point() - ppp).abs() < 1e-3);
            let at = rig
                .nodes
                .iter()
                .filter(|(l, _, r)| l.contains("Party") && r.width() > 0.0 && r.center().y < 60.0)
                .map(|(_, _, r)| r.center())
                .max_by(|a, b| a.x.total_cmp(&b.x))
                .expect("Party-Knopf oben rechts");
            assert!(
                at.x > 1920.0 / ppp * 0.5,
                "{ppp}: Knopf nicht rechts: {at:?}"
            );
            let ev = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            step(&mut rig, vec![egui::Event::PointerMoved(at)]);
            step(&mut rig, vec![ev(true)]);
            step(&mut rig, vec![ev(false)]);
            step(&mut rig, Vec::new());
            assert_eq!(
                rig.app.config.mode,
                Mode::Party,
                "{ppp}: Klick rechts kam nicht an"
            );
        }
    }

    #[test]
    fn clean_project_quits_without_asking() {
        let mut rig = Rig::new(Mode::Produzent);
        rig.app.exit = crate::exit::ExitGuard::default();
        rig.step(vec![egui::Event::Key {
            key: egui::Key::Q,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }]);
        assert!(rig.commands.contains(&egui::ViewportCommand::Close));
    }

    #[test]
    fn developer_menu_only_when_unlocked_and_before_help() {
        let mut rig = Rig::new(Mode::Radio);
        assert!(rig.any(|l| l == "Hilfe"), "Menüleiste fehlt");
        assert!(
            !rig.any(|l| l == "Entwickler"),
            "Entwickler ohne Freischaltung sichtbar"
        );
        rig.app.config.entwickler = true;
        rig.step(Vec::new());
        rig.step(Vec::new());
        let x = |rig: &Rig, name: &str| {
            rig.nodes
                .iter()
                .find(|(l, _, r)| l == name && r.width() > 0.0)
                .map(|(_, _, r)| r.min.x)
        };
        let dev = x(&rig, "Entwickler").expect("Entwickler fehlt nach Freischaltung");
        let help = x(&rig, "Hilfe").expect("Hilfe");
        assert!(dev < help, "Entwickler steht nicht vor Hilfe");
    }

    /// #396: Das Studio-Dock bleibt so klein, wie man es zieht; ist der Inhalt höher, wird
    /// er beschnitten statt das Dock wieder nach oben zu schieben.
    #[test]
    fn studio_dock_keeps_the_dragged_size() {
        let mut rig = Rig::new(Mode::Produzent);
        for _ in 0..3 {
            rig.app.state_mut().add_midi_track().unwrap();
        }
        rig.step(Vec::new());
        rig.step(Vec::new());
        // Oberkante des Docks: Reiter „Studio-Mixer“ liegt 6 px Rand plus Rahmen darunter.
        let edge = |rig: &Rig| {
            rig.nodes
                .iter()
                .filter(|(l, _, r)| l == "Studio-Mixer" && r.width() > 0.0)
                .map(|(_, _, r)| egui::pos2(r.center().x + 300.0, r.top() - 7.0))
                .max_by(|a, b| a.y.total_cmp(&b.y))
                .expect("Kante des Studio-Docks")
        };
        let from = edge(&rig);
        let to = from + egui::vec2(0.0, 400.0);
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        rig.step(vec![egui::Event::PointerMoved(from)]);
        rig.step(vec![button(from, true)]);
        for i in 1..=8 {
            let p = from + egui::vec2(0.0, 50.0 * i as f32);
            rig.step(vec![egui::Event::PointerMoved(p)]);
        }
        rig.step(vec![button(to, false)]);
        let dropped = edge(&rig).y;
        for _ in 0..10 {
            rig.step(Vec::new());
        }
        let later = edge(&rig).y;
        // Kleinste Höhe 175: Die Kante liegt deutlich tiefer als vorher und bleibt dort.
        assert!(
            dropped > from.y + 100.0,
            "Ziehen wirkt nicht: {} → {dropped}",
            from.y
        );
        assert!(
            (later - dropped).abs() < 2.0,
            "Dock wandert wieder hoch: {dropped} → {later}"
        );
    }
}

#[cfg(test)]
mod sound_search_tests {
    use super::sound_matches;
    use woa_model::SoundCategory;

    #[test]
    fn empty_search_shows_the_category_and_text_searches_all() {
        let k = sound_matches("", SoundCategory::Bass);
        assert!(!k.is_empty());
        assert!(k.iter().all(|p| p.category == SoundCategory::Bass));
        let first = woa_model::SOUND_PRESETS[0];
        let hit = sound_matches(&first.name.to_uppercase(), SoundCategory::Drums);
        assert!(
            hit.iter().any(|p| p.name == first.name),
            "über Kategorien, ohne Groß/Klein"
        );
        assert!(sound_matches("zzzz-nichts", SoundCategory::Bass).is_empty());
    }
}
