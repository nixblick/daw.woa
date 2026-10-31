//! Radio-Layout (Sendeablauf): Playlist, große Uhr, Start/Weiter, Stopp, Cart-Wall,
//! Mikrofon mit Ducking, Mitschnitt. Alle Logik steckt in `woa_radio`; hier wird nur
//! gezeichnet und weitergereicht.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use eframe::egui;
use woa_io::controller::{Bus, Target};
use woa_radio::control::PflSource;
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
/// Tooltip, wenn das Vorhören in den Einstellungen aus ist.
pub const PFL_OFF: &str = "Vorhören ist aus: Einstellungen → Vorhören (PFL)";

/// Knopf „Vorhören“: läuft gerade diese Quelle, wird er zu „⏹ Vorhören stoppen“ (hervorgehoben),
/// ein Klick darauf stoppt (#205).
fn pfl_button(active: bool) -> egui::Button<'static> {
    if active {
        egui::Button::new(theme::fill_label(
            theme::icf::STOP,
            "Vorhören stoppen",
            14.0,
            theme::text(),
        ))
        .selected(true)
    } else {
        egui::Button::new("Vorhören")
    }
}

type Stream = Box<dyn std::any::Any>;

/// Tasten, die schon etwas anderes tun und keinem Cart gehören können.
/// Breite der Restzeit-Spalte eines Decks (Uhr und Fortschrittsbalken).
const DECK_CLOCK_WIDTH: f32 = 110.0;

/// Wohin eine gezogene Playlist-Zeile fällt (#166).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowDrop {
    /// Auf Deck `k` laden.
    Deck(usize),
    /// In der Playlist an diese Stelle verschieben.
    Row(usize),
    /// Daneben oder auf sich selbst: nichts.
    Nothing,
}

/// Ziel einer von Zeile `from` gezogenen Playlist-Zeile, losgelassen bei `pos`.
fn row_drop_target(
    pos: egui::Pos2,
    from: usize,
    decks: &[egui::Rect],
    playlist: egui::Rect,
    rows: &[egui::Rect],
) -> RowDrop {
    if let Some(k) = decks.iter().position(|r| r.contains(pos)) {
        return RowDrop::Deck(k);
    }
    if !playlist.contains(pos) {
        return RowDrop::Nothing;
    }
    match rows.iter().position(|r| r.contains(pos)) {
        Some(to) if to != from => RowDrop::Row(to),
        _ => RowDrop::Nothing,
    }
}

/// Anzahl gespielter Titel am Anfang der Playlist (vor dem laufenden bzw. nächsten).
pub(crate) fn played_count(v: &View) -> usize {
    v.first_open.min(v.playlist.len())
}

const RESERVED_KEYS: [egui::Key; 2] = [egui::Key::M, egui::Key::Space];

/// Die laufende Sendung samt Gerät.
pub struct RadioDesk {
    /// Beginn des laufenden Mitschnitts (Dauer im REC-Tooltip).
    rec_since: Option<std::time::Instant>,
    radio_path: PathBuf,
    data_dir: PathBuf,
    station: Option<Station>,
    /// Geöffnete Ausgabeströme (Hauptgerät und weitere Geräte).
    outputs: crate::outputs::Opened,
    /// Meldungen aus dem Öffnen, die in den Einstellungen stehen bleiben.
    output_warnings: Vec<String>,
    mic: Option<Stream>,
    /// Telefon-Eingang (Anrufer): Strom, gewählte Einstellung, Meldung, Zähler, Pegel.
    phone: Option<Stream>,
    phone_cfg: crate::phone::PhoneInput,
    phone_error: Option<String>,
    phone_stats: Option<Arc<woa_engine::pairs::LinkStats>>,
    phone_lost: Arc<std::sync::atomic::AtomicBool>,
    phone_level: f32,
    /// LED-Meter neben den Fadern und Master (#216).
    meters: [MeterHold; 5],
    /// Gespielte ausblenden: lokal sofort, bis der Schnappschuss nachzieht.
    hide_played: Option<bool>,
    /// Wunschordner: Beobachter. Ohne Wahl liegt er im Datenordner der Sendung.
    wishes: Option<crate::wishes::WishWatcher>,
    wish_chosen: Option<PathBuf>,
    /// Party-Decks sind an (Party-Modus).
    party_on: bool,
    rate: u32,
    error: Option<String>,
    view: View,
    /// QR-Code der zuletzt gezeigten Einladung (Paket 38).
    invite_qr: Option<(String, woa_remote::qr::QrMatrix)>,
    /// „Trennen“ einmal geklickt: Kanal und Zeitpunkt; der zweite Klick binnen 3 s trennt.
    guest_hangup_armed: Option<(usize, Instant)>,
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
    levels_db: [f64; 4],
    /// „MIDI lernen“ per Rechtsklick gewählt; die App holt es ab.
    pub learn: Option<Target>,
    /// Fläche der Playlist und der Cart-Pads im letzten Bild (Ziel für Ziehen aus der Bibliothek).
    playlist_rect: egui::Rect,
    cart_rects: Vec<egui::Rect>,
    /// Flächen der Decks A bis C (Ziel für gezogene Dateien).
    deck_rects: [egui::Rect; woa_radio::DECKS],
    /// Zeilen der Playlist im letzten Bild (Ziel beim Umsortieren per Ziehen, #166).
    row_rects: Vec<egui::Rect>,
    /// Gezogene Playlist-Zeile (Index), solange die Maus gedrückt ist (#166).
    row_drag: Option<usize>,
    /// Cart, das die nächste gedrückte Taste als neue Taste bekommt.
    key_capture: Option<usize>,
    /// Ortszeit für die Uhrzeit-Anzeige (von der Stundenuhr).
    zone: Zone,
    /// Offener Cue-Editor (Paket 15).
    cue_editor: Option<CueEditor>,
    /// Titelinfos dieses Titels wünscht das Kontextmenü (öffnet die Bibliothek, App holt ab).
    tag_wish: Option<PathBuf>,
    /// Offener Voice-Track-Editor (Paket 30).
    voice_editor: Option<VoiceEditor>,
    /// Zuletzt in den Editor übernommene Cue-Punkte je Titel (erkennt Undo/Redo).
    cue_seen: std::collections::HashMap<usize, woa_radio::Cues>,
    /// Fenster „Verwalten“ und Auswahl der aktiven Playlist.
    manage: crate::manage::ManageWindow,
    /// Eigener Strom für das Vorhören auf einem zweiten Gerät.
    _pfl_output: Option<woa_engine::device::ManagedOutput>,
    /// Warum das Vorhören keinen Ausgang hat (aus oder Fehler beim Öffnen); `None` = offen.
    pfl_note: Option<String>,
}

fn level_bus(b: Bus) -> LevelBus {
    match b {
        Bus::Music => LevelBus::Music,
        Bus::Carts => LevelBus::Carts,
        Bus::Mic => LevelBus::Mic,
        Bus::Decks => LevelBus::Decks,
    }
}

fn bus_index(b: Bus) -> usize {
    match b {
        Bus::Music => 0,
        Bus::Carts => 1,
        Bus::Mic => 2,
        Bus::Decks => 3,
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

/// Reihenfolge und Namen der senkrechten Pegelregler neben den Decks.
pub(crate) const FADER_BUSES: [(Bus, &str); 4] = [
    (Bus::Music, "Playlist"),
    (Bus::Carts, "Carts"),
    (Bus::Decks, "Decks"),
    (Bus::Mic, "Mikro"),
];

/// Anzeige eines LED-Meters: fällt weich, die Spitze steht [`HOLD_S`] Sekunden.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct MeterHold {
    pub level: f32,
    pub hold: f32,
    hold_at: f64,
}

/// So lange bleibt die Spitze stehen (Sekunden).
const HOLD_S: f64 = 1.5;

impl MeterHold {
    /// Neue Spitze `peak` (linear) zur Zeit `now` (Sekunden).
    pub fn step(&mut self, peak: f32, now: f64) {
        self.level = peak.max(self.level * 0.85);
        if peak >= self.hold || now - self.hold_at > HOLD_S {
            self.hold = peak.max(self.level);
            self.hold_at = now;
        }
    }
}

fn now_s() -> f64 {
    static T0: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    T0.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// Ein schmales LED-Meter mit Namen für die Barrierefreiheit (und die Tests).
fn meter_strip(ui: &mut egui::Ui, m: MeterHold, name: &str, size: egui::Vec2) {
    let (rect, r) = ui.allocate_exact_size(size, egui::Sense::hover());
    theme::bus_meter(ui.painter(), rect, m.level, m.hold);
    let label = format!("Pegelanzeige {name}");
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ProgressIndicator, true, &label));
    r.on_hover_text(format!(
        "{name}: Spitze {} dBFS. Grün bis -18, Gelb bis -6, Rot darüber, Blau = zu leise.",
        crate::widgets::vu::fmt_db(20.0 * m.hold.max(1e-6).log10())
    ));
}

/// Senkrechte Pegelregler (wie der Kanal-Gain der Spuren): Name oben, Fader mit dB-Skala und
/// LED-Meter daneben, Wert darunter; als fünfte Spalte das Meter „Master“ (Sendemix, ohne
/// Fader). `dbs` in der Reihenfolge von [`FADER_BUSES`], `meters` dazu plus Master. Liefert je
/// Regler die Antwort (`changed()`, wenn der Wert sich geändert hat; Doppelklick = 0 dB).
pub(crate) fn level_faders(
    ui: &mut egui::Ui,
    dbs: &mut [f32; 4],
    meters: &[MeterHold; 5],
) -> Vec<egui::Response> {
    let mut out = Vec::with_capacity(4);
    let caption = |ui: &mut egui::Ui, t: &str| {
        ui.label(
            egui::RichText::new(t)
                .font(theme::label_font(11.0))
                .color(theme::text_dim()),
        );
    };
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (((_, name), db), m) in FADER_BUSES.iter().zip(dbs.iter_mut()).zip(meters) {
            ui.vertical(|ui| {
                ui.set_width(48.0);
                caption(ui, name);
                let r = ui
                    .horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let r = crate::widgets::fader(
                            ui,
                            db,
                            woa_radio::mixer::LEVEL_MIN_DB as f32
                                ..=woa_radio::mixer::LEVEL_MAX_DB as f32,
                            0.0,
                            egui::vec2(40.0, 150.0),
                            crate::widgets::Orient::Vertical,
                        );
                        meter_strip(ui, *m, name, egui::vec2(6.0, 150.0));
                        r
                    })
                    .inner;
                let label = format!("Pegel {name}");
                let value = f64::from(*db);
                r.widget_info(|| egui::WidgetInfo::slider(true, value, &label));
                ui.label(
                    egui::RichText::new(crate::widgets::vu::fmt_db(*db))
                        .font(theme::label_font(11.0))
                        .monospace()
                        .color(theme::text()),
                );
                out.push(r.on_hover_text(format!(
                    "Pegel {name}. Doppelklick: 0 dB. Rechtsklick: MIDI lernen. Ganz unten = aus."
                )));
            });
        }
        ui.vertical(|ui| {
            caption(ui, "Master");
            meter_strip(ui, meters[4], "Master", egui::vec2(12.0, 150.0));
        });
    });
    out
}

/// Knöpfe „Deck A/B/C starten“ im Playlist-Player: das Deck löst die Playlist ab.
/// Klick: mit Blende (eingestellter Crossfade), Umschalt+Klick oder Kontextmenü „Sofort“:
/// harter Schnitt. Leere Decks sind aus und sagen, warum. Liefert (Deck, mit Blende).
pub(crate) fn deck_take_buttons(
    ui: &mut egui::Ui,
    decks: &[woa_radio::station::DeckView],
    crossfade: f64,
) -> Option<(usize, bool)> {
    let mut hit = None;
    for (k, d) in decks.iter().enumerate().take(woa_radio::DECKS) {
        let name = woa_radio::show::deck_name(k);
        let loaded = d.title.is_some();
        let r = ui
            .add_enabled(
                loaded,
                egui::Button::new(
                    egui::RichText::new(format!("{} Deck {name}", theme::icf::PLAY))
                        .size(14.0)
                        .strong(),
                )
                .min_size(egui::vec2(70.0, 38.0)),
            )
            .on_hover_text(format!(
                "Deck {name} starten und die Playlist ablösen: Die Playlist blendet aus und \
                 pausiert, das Deck blendet ein ({crossfade:.1} s Crossfade).\n\
                 Umschalt+Klick oder Rechtsklick „Sofort“: harter Schnitt ohne Blende."
            ))
            .on_disabled_hover_text(format!(
                "Deck {name} ist leer: erst einen Titel aufs Deck laden (Rechtsklick auf \
                 einen Titel, „Auf Deck laden“)"
            ));
        if r.clicked() {
            let shift = ui.input(|i| i.modifiers.shift);
            hit = Some((k, !shift));
        }
        r.context_menu(|ui| {
            if ui
                .button(format!("Mit Blende ({crossfade:.1} s)"))
                .clicked()
            {
                hit = Some((k, true));
                ui.close();
            }
            if ui.button("Sofort (ohne Blende)").clicked() {
                hit = Some((k, false));
                ui.close();
            }
        });
    }
    hit
}

/// Umschalter „Zurück zur Playlist“ an einem Deck. Liefert die Antwort (`clicked()`).
pub(crate) fn deck_return_toggle(ui: &mut egui::Ui, deck: usize, on: bool) -> egui::Response {
    let name = woa_radio::show::deck_name(deck);
    ui.add(
        egui::Button::selectable(on, egui::RichText::new("↩ Playlist").strong())
            .min_size(egui::vec2(84.0, 30.0)),
    )
    .on_hover_text(format!(
        "Zurück zur Playlist ({}): Ist das an, läuft nach Deck {name} die Playlist mit dem \
         eingestellten Crossfade weiter (pausiert: ab der Pausenstelle). Aus: Nach dem Deck \
         bleibt es still, etwa zum Reden oder Telefonieren.",
        if on { "an" } else { "aus" }
    ))
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
    let (h, m, s) = time_parts(unix, zone);
    crate::widgets::clock::fmt_hms(h, m, s)
}

/// Stunde, Minute und Sekunde zur Unix-Zeit `unix` in `zone` (für die Studio-Uhr).
pub fn time_parts(unix: i64, zone: Zone) -> (u32, u32, u32) {
    let l = zone.local(unix);
    (l.hour as u32, l.minute as u32, l.second as u32)
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
        let mut pfl_note: Option<String> = None;
        let (aux, pfl_output) = match pfl {
            PflOutput::Aus => {
                pfl_note = Some(PFL_OFF.into());
                (None, None)
            }
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
                    pfl_note = Some(format!(
                        "Vorhören aus: Das Gerät hat nur {n} Kanäle, Kanal 3/4 gibt es nicht"
                    ));
                    error.clone_from(&pfl_note);
                    (None, None)
                }
                Err(e) => {
                    tracing::warn!(error = %e, "pfl: channel count");
                    pfl_note = Some(format!("Vorhören aus: Kanalzahl nicht lesbar ({e})"));
                    error.clone_from(&pfl_note);
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
                let render: woa_engine::device::PairsRender = Box::new(move |bufs, frames| {
                    if let Some(b) = bufs.first_mut() {
                        player.process(&mut b.l[..frames], &mut b.r[..frames]);
                    }
                });
                match woa_engine::device::ManagedOutput::open("pfl", &cfg, 1, false, render, |_| {})
                {
                    Ok(st) => {
                        tracing::info!(device = %id, rate = prate, "pfl on second device");
                        controller.set_pfl(Some(h));
                        (None, Some(st))
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, device = %id, "pfl output failed");
                        pfl_note = Some(format!("Vorhören aus: Kopfhörer-Gerät geht nicht ({e})"));
                        error.clone_from(&pfl_note);
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
            rec_since: None,
            radio_path,
            data_dir,
            station,
            outputs: opened,
            output_warnings,
            mic: None,
            phone: None,
            phone_cfg: crate::phone::PhoneInput::default(),
            phone_error: None,
            phone_stats: None,
            phone_lost: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            phone_level: 0.0,
            meters: Default::default(),
            hide_played: None,
            wishes: None,
            party_on: false,
            wish_chosen: None,
            rate,
            error,
            view: View::default(),
            invite_qr: None,
            guest_hangup_armed: None,
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
            levels_db: [0.0; 4],
            learn: None,
            playlist_rect: egui::Rect::NOTHING,
            cart_rects: Vec::new(),
            deck_rects: [egui::Rect::NOTHING; woa_radio::DECKS],
            row_rects: Vec::new(),
            row_drag: None,
            key_capture: None,
            zone: Zone::CENTRAL_EUROPE,
            cue_editor: None,
            tag_wish: None,
            voice_editor: None,
            cue_seen: std::collections::HashMap::new(),
            manage: crate::manage::ManageWindow::default(),
            _pfl_output: pfl_output,
            pfl_note,
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
        tracing::info!("sendung start");
        self.attach_feeder();
    }

    /// „Sendung beenden“: woa-streamd meldet sich beim Server ab und endet.
    pub fn go_off_air(&mut self) {
        tracing::info!("sendung ende");
        self.with(|c| c.set_stream_output(None));
        if let Some(f) = self.feeder.take() {
            f.stop_broadcast();
        }
    }

    /// Läuft woa-streamd gerade (frische Status-Datei)?
    pub fn stream_running(&self) -> bool {
        self.stream_status
            .as_ref()
            .is_some_and(|s| s.is_running(woa_stream::unix_now()))
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

    pub(crate) fn with(&self, f: impl FnOnce(&mut Controller) + Send + 'static) {
        if let Some(s) = &self.station {
            s.with(f);
        }
    }

    /// Rettungsknopf „Ausgabe neu verbinden“: öffnet Sendeausgänge und Vorhören sofort neu.
    /// Das Ergebnis steht oben im Sendepult.
    pub fn reconnect_outputs(&mut self) {
        let mut ok = self.outputs.reconnect_now();
        if let Some(o) = self._pfl_output.as_mut() {
            ok &= !matches!(o.reconnect_now(), woa_engine::device::Reconnect::Failed(_));
        }
        self.error = if ok {
            None
        } else {
            Some("Neu verbinden hat nicht geklappt, WOA versucht es weiter".into())
        };
    }

    /// Ein Takt: Schnappschuss holen, speichern, fremde Änderungen an radio.json laden.
    /// `player_active`: Der Player-Strom läuft gleichzeitig (für das Log).
    pub fn tick(&mut self, player_active: bool) {
        self.outputs.poll();
        if let Some(o) = self._pfl_output.as_mut() {
            o.poll();
        }
        let Some(st) = &self.station else { return };
        self.view = st.view();
        self.phone_level = self.view.phone_peak.max(self.phone_level * 0.9);
        for (m, v) in self.meters.iter_mut().zip(self.view.meters) {
            m.step(v, now_s());
        }
        if self.hide_played == Some(self.view.hide_played) {
            self.hide_played = None;
        }
        if self.phone_lost.swap(false, Ordering::Relaxed) {
            self.phone = None;
            self.phone_stats = None;
            self.with(|c| c.set_phone_input(None));
            self.phone_error = Some(format!(
                "Der Telefon-Eingang {} ist ausgefallen (abgezogen?). Bitte anstecken und im \
                 Telefon-Fenster „Übernehmen“ drücken.",
                self.phone_cfg.label()
            ));
        }
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
                    device = %woa_telemetry::short_device(&self.device),
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
        let mut check_now = false;
        if self.view.revision != self.saved_revision
            && now.duration_since(self.saved_at).as_millis() >= 700
        {
            let m = mtime(&self.radio_path);
            if m.is_some() && m != self.known_mtime {
                // Erst die fremde Änderung (build_playlist) übernehmen, nicht überschreiben.
                self.saved_revision = self.view.revision;
                self.error = Some(
                    "radio.json wurde außerhalb geändert: neuer Stand übernommen, \
                     die letzte Änderung hier nicht gespeichert"
                        .into(),
                );
                check_now = true;
            } else {
                match st.show().save(&self.radio_path) {
                    Ok(()) => {
                        self.saved_revision = self.view.revision;
                        self.known_mtime = mtime(&self.radio_path);
                    }
                    Err(e) => self.error = Some(format!("radio.json nicht gespeichert: {e}")),
                }
            }
            self.saved_at = now;
        }
        if check_now || now.duration_since(self.checked_at).as_secs() >= 2 {
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

    /// Live für die ON-AIR-Lampe: die Sendung läuft (Feeder) oder das Mikro ist offen.
    pub fn is_live(&self) -> bool {
        self.feeder.is_some() || self.view.mic_open
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
            Target::Stop => self.with(|c| c.stop_or_pause()),
            Target::Cart(slot) if slot < CARTS => self.with(move |c| c.trigger_cart(slot)),
            _ => {}
        }
    }

    /// Wirkung eines Party-Befehls auf die Sendung (Paket 35). Das Vollbild macht die App.
    /// Party-Decks an (Party-Modus) oder aus. Jeden Frame aufrufen; geschickt wird nur beim
    /// Wechsel. Beim Betreten übernimmt die linke Seite einen laufenden Titel, beim Verlassen
    /// die Playlist die hörbare Seite.
    pub fn set_party_mode(&mut self, on: bool) {
        if on != self.party_on {
            self.party_on = on;
            if on {
                self.with(|c| c.party_enter());
            } else {
                self.with(|c| c.party_leave());
            }
        }
    }

    /// Befehle der Party-Decks (Tasten, Handy) im Party-Modus.
    fn party_duo_effect(&mut self, e: woa_radio::party::Effect) {
        use woa_radio::party::Effect;
        match e {
            Effect::StartOrNext => self.with(|c| c.party_next()),
            Effect::Stop | Effect::Pause => self.with(|c| c.party_pause_all()),
            Effect::Play => self.with(|c| {
                if !c.party_playing() {
                    c.party_toggle();
                }
            }),
            Effect::Previous => self.with(|c| c.party_previous()),
            Effect::Restart => self.with(|c| c.party_restart()),
            Effect::Seek(s) => self.with(move |c| c.party_seek(s)),
            Effect::CycleRepeat => self.with(|c| c.cycle_repeat()),
            Effect::ToggleShuffle => self.with(|c| c.toggle_shuffle()),
            Effect::MusicLevel(db) => self.set_level(Bus::Music, db),
            Effect::InsertNext(path) => self.with(move |c| {
                // Hinter den hörbaren Titel: die freie Seite lädt ihn dann vor.
                let at = c.party_live_index().map_or(0, |i| i + 1);
                let at = at.min(c.show().playlist.len());
                c.add(&[path], Some(at));
            }),
            Effect::Fullscreen(_) => {}
        }
    }

    /// Crossfader der Party von Hand (0 links, 1 rechts).
    pub fn party_fader(&self, pos: f32) {
        self.with(move |c| c.party_fader(pos));
    }

    /// ▶ Play bzw. Pause an einer Party-Seite.
    pub fn party_side(&self, side: usize, play: bool) {
        self.with(move |c| {
            if play {
                c.party_play(side);
            } else {
                c.party_pause(side);
            }
        });
    }

    /// Schalter „Auto-Übergang“ (wird mit der Sendung gespeichert).
    pub fn set_party_auto(&self, on: bool) {
        self.with(move |c| c.set_party_auto(on));
    }

    /// Stand des Wunschtitel-Ordners (#134), falls einer beobachtet wird.
    pub fn wish_folder_list(&self) -> Option<crate::wishes::WishList> {
        self.wishes.as_ref().map(|w| w.list())
    }

    pub fn party_effect(&mut self, e: woa_radio::party::Effect) {
        use woa_radio::party::Effect;
        match e {
            _ if self.party_on => self.party_duo_effect(e),
            Effect::StartOrNext => self.with(|c| c.start_or_next()),
            Effect::Stop => self.with(|c| c.stop()),
            Effect::Pause => self.with(|c| c.pause()),
            Effect::Play => self.with(|c| c.play()),
            Effect::Previous => self.with(|c| c.previous()),
            Effect::Restart => self.with(|c| c.restart()),
            Effect::Seek(s) => self.with(move |c| c.seek(s)),
            Effect::CycleRepeat => self.with(|c| c.cycle_repeat()),
            Effect::ToggleShuffle => self.with(|c| c.toggle_shuffle()),
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

    /// Neue Einstellung für den Telefon-Eingang: schließt den alten und öffnet den neuen.
    /// Ein fehlendes Gerät gibt eine klare Meldung, die Sendung läuft weiter.
    pub fn set_phone_config(&mut self, cfg: crate::phone::PhoneInput) {
        self.phone = None;
        self.phone_stats = None;
        self.phone_error = None;
        self.phone_cfg = cfg;
        self.with(|c| c.set_phone_input(None));
        self.open_phone();
    }

    fn open_phone(&mut self) {
        let Some(config) = self.phone_cfg.input_config() else {
            return;
        };
        let lost = Arc::clone(&self.phone_lost);
        match woa_engine::record::start_pair_input(&config, self.rate, move |e| {
            tracing::warn!(error = %e, "phone input");
            lost.store(true, Ordering::Relaxed);
        }) {
            Ok((stream, rx, stats, rate)) => {
                tracing::info!(rate, device = %self.phone_cfg.label(), "phone input open");
                self.phone = Some(Box::new(stream));
                self.phone_stats = Some(stats);
                self.with(move |c| c.set_phone_input(Some(rx)));
            }
            Err(e) => {
                self.phone_error = Some(format!(
                    "Telefon-Eingang {} lässt sich nicht öffnen oder fehlt ({e}). Die Sendung \
                     läuft weiter. Bitte anstecken und im Telefon-Fenster „Übernehmen“ drücken.",
                    self.phone_cfg.label()
                ));
            }
        }
    }

    /// „Telefon auf Sendung“: der Anrufer liegt auf der Sendung, die Musik geht runter.
    fn toggle_phone(&mut self) {
        let on = !self.view.phone_on;
        if on && self.phone.is_none() {
            if self.phone_cfg.device_id.is_none() {
                self.phone_error = Some(
                    "Kein Telefon-Eingang gewählt. Einstellungen, Telefon …, dort Gerät und \
                     Kanalpaar wählen."
                        .into(),
                );
                return;
            }
            self.phone_error = None;
            self.open_phone();
            if self.phone.is_none() {
                return;
            }
        }
        self.view.phone_on = on;
        self.with(move |c| c.set_phone_on(on));
    }

    /// Zeilen fürs Telefon-Fenster: Meldung, Zähler der Taktverbindung.
    pub fn phone_lines(&self) -> Vec<String> {
        let mut v = Vec::new();
        if let Some(e) = &self.phone_error {
            v.push(e.clone());
        }
        if let Some(s) = &self.phone_stats {
            v.push(format!(
                "Eingang läuft. Aussetzer: {} (Ring leer {}, übersprungen {}, verworfen {})",
                s.faults(),
                s.underruns.load(Ordering::Relaxed),
                s.skipped.load(Ordering::Relaxed),
                s.dropped.load(Ordering::Relaxed),
            ));
        }
        v
    }

    /// Wunschordner wählen (`None` = der Ordner `Wunschtitel` im Datenordner der Sendung).
    pub fn set_wish_folder(&mut self, folder: Option<PathBuf>) {
        let dir = folder.unwrap_or_else(|| self.data_dir.join("Wunschtitel"));
        if self.wishes.as_ref().is_some_and(|w| w.dir() == dir) {
            return;
        }
        self.wishes = Some(crate::wishes::WishWatcher::start(
            dir,
            Duration::from_secs(1),
        ));
    }

    /// Der Wunschordner, den die App speichern soll, wenn er gewählt wurde.
    pub fn wish_folder(&self) -> Option<&Path> {
        self.wishes.as_ref().map(|w| w.dir())
    }

    /// Die Wunschtitel: Liste mit einem Klick in die Playlist oder auf ein Deck.
    fn wish_card(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("WUNSCHTITEL")
                    .size(11.5)
                    .strong()
                    .color(theme::text_dim()),
            );
            if ui
                .small_button("Ordner wählen …")
                .on_hover_text("Alles, was hier hineinkommt, erscheint sofort in der Liste")
                .clicked()
                && let Some(dir) = rfd::FileDialog::new().pick_folder()
            {
                self.set_wish_folder(Some(dir.clone()));
                self.wish_chosen = Some(dir);
            }
        });
        let Some(w) = self.wishes.as_ref() else {
            ui.label(
                egui::RichText::new("Kein Wunschordner.")
                    .small()
                    .color(theme::text_dim()),
            );
            return;
        };
        let list = w.list();
        ui.label(
            egui::RichText::new(w.dir().display().to_string())
                .small()
                .color(theme::text_dim()),
        );
        if let Some(e) = &list.error {
            ui.colored_label(theme::on_air(), e);
        }
        if list.files.is_empty() && list.error.is_none() {
            ui.label(
                egui::RichText::new("Noch kein Wunsch im Ordner.")
                    .small()
                    .color(theme::text_dim()),
            );
        }
        // Die Liste ändert sich ohne Eingabe: ab und zu neu zeichnen.
        ui.ctx().request_repaint_after(Duration::from_secs(1));
        let mut act: Option<(PathBuf, Option<usize>)> = None;
        egui::ScrollArea::vertical()
            .id_salt("wishes")
            .max_height(150.0)
            .show(ui, |ui| {
                for f in &list.files {
                    let fresh = list.fresh.contains(&f.path);
                    ui.horizontal(|ui| {
                        let mut name = egui::RichText::new(&f.name).color(if f.copying {
                            theme::text_dim()
                        } else {
                            theme::text()
                        });
                        if fresh {
                            name = name.strong();
                        }
                        ui.add(egui::Label::new(name).truncate())
                            .on_hover_text(f.path.display().to_string());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if f.copying {
                                ui.label(
                                    egui::RichText::new("wird kopiert …")
                                        .small()
                                        .color(theme::text_dim()),
                                );
                                return;
                            }
                            for k in (0..woa_radio::DECKS).rev() {
                                if ui
                                    .small_button(woa_radio::show::deck_name(k).to_string())
                                    .on_hover_text("Auf dieses Deck laden")
                                    .clicked()
                                {
                                    act = Some((f.path.clone(), Some(k)));
                                }
                            }
                            if ui
                                .small_button("In die Playlist")
                                .on_hover_text("Direkt nach dem laufenden Titel einreihen")
                                .clicked()
                            {
                                act = Some((f.path.clone(), None));
                            }
                            if fresh {
                                ui.label(egui::RichText::new("neu").small().color(theme::accent()));
                            }
                        });
                    });
                }
            });
        match act {
            Some((path, Some(k))) => self.deck_load_file(k, path),
            Some((path, None)) => self.party_effect(woa_radio::party::Effect::InsertNext(path)),
            None => {}
        }
    }

    /// Der Ordner, den der Nutzer eben gewählt hat (die App speichert ihn einmal).
    /// Titel, dessen Titelinfos im Kontextmenü der Playlist gewählt wurden (einmal).
    pub fn take_tag_wish(&mut self) -> Option<PathBuf> {
        self.tag_wish.take()
    }

    pub fn take_wish_choice(&mut self) -> Option<PathBuf> {
        self.wish_chosen.take()
    }

    /// Laufender Mitschnitt: Ziel-Datei und Beginn (für die REC-Pille der Kopfleiste).
    pub fn recording(&self) -> Option<(Option<&std::path::Path>, Option<std::time::Instant>)> {
        (self.view.recording_on || self.view.recording.is_some())
            .then_some((self.view.recording.as_deref(), self.rec_since))
    }

    /// Ordner der Mitschnitte.
    pub fn recordings_dir(&self) -> PathBuf {
        self.data_dir.join(woa_radio::RECORDINGS_DIR)
    }

    /// Startet oder beendet den Mitschnitt der Sendesumme (REC in der Kopfleiste).
    pub fn toggle_recording(&mut self) {
        let on = self.view.recording_on || self.view.recording.is_some();
        self.rec_since = if on {
            None
        } else {
            Some(std::time::Instant::now())
        };
        if on {
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
        // Abgelegte Dateien zuerst: ein Textfeld mit Fokus darf sie nicht schlucken (#166).
        self.drop_os_files(ctx);
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
            // Leertaste: ein Knopf, läuft es, dann Pause, sonst Play.
            let running = self.view.running;
            for e in woa_radio::party::PartyState::default()
                .apply(woa_radio::party::PartyCommand::PlayPause, running)
            {
                self.party_effect(e);
            }
        }
    }

    /// Aufs Fenster gezogene Dateien: auf ein Deck die erste, sonst alle in die Playlist.
    fn drop_os_files(&mut self, ctx: &egui::Context) {
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
                .fill(theme::mix(theme::on_air(), theme::well(), 0.78))
                .stroke(egui::Stroke::new(1.0, theme::on_air()))
                .corner_radius(5)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(theme::ic::WARNING)
                                .strong()
                                .color(theme::on_air()),
                        );
                        ui.colored_label(theme::text(), e);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button(theme::ic::X).clicked() {
                                self.error = None;
                            }
                        });
                    });
                });
            ui.add_space(6.0);
        }
        for m in &v.messages {
            ui.label(egui::RichText::new(m).small().color(theme::text_dim()));
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
            // Die rechte Spalte rollt: sonst liegen Rack (Crossfade, Ducking) und Gäste
            // unter dem Fensterrand und sind nicht erreichbar (#168).
            egui::ScrollArea::vertical()
                .id_salt("radio_right_column")
                .auto_shrink([false, false])
                .show(&mut cols[1], |ui| {
                    ui.spacing_mut().item_spacing.y = 10.0;
                    theme::card_frame().show(ui, |ui| {
                        self.deck_card(ui, &v);
                    });
                    theme::card_frame().show(ui, |ui| {
                        self.wish_card(ui);
                    });
                    theme::card_frame().show(ui, |ui| {
                        self.cart_wall(ui, &v);
                    });
                    theme::card_frame().show(ui, |ui| {
                        self.processing_rack(ui);
                    });
                    theme::card_frame().show(ui, |ui| {
                        self.guests_card(ui);
                    });
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
                    .color(theme::text_dim()),
            );
            if v.running {
                theme::status_pill(ui, "DECK LÄUFT", theme::ok(), true);
            } else {
                theme::status_pill(ui, "BEREIT / CUE", theme::accent(), false);
            }
            if v.mic_open {
                theme::status_pill(ui, "MIKRO OFFEN", theme::on_air(), true);
            }
            if v.recording.is_some() {
                theme::status_pill(ui, "REC LÄUFT", theme::on_air(), true);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Die ON-AIR-Lampe sitzt in der Kopfleiste neben MIDI (André 31.10.).
                let n = self.underruns();
                if n > 0 {
                    ui.label(
                        egui::RichText::new(format!("Aussetzer: {n}"))
                            .small()
                            .color(theme::on_air()),
                    )
                    .on_hover_text(
                        "Puffer-Unterläufe der Ausgabe seit dem Öffnen. Steigt die Zahl, \
                         in config.json radio_buffer_frames setzen, etwa 2048 oder 4096 (Vorgabe: 1024).",
                    );
                }
            });
        });
        ui.add_space(4.0);

        // Haupt-Display: Cover links, tiefschwarzes LCD-Cockpit rechts.
        ui.horizontal(|ui| {
            egui::Frame::new()
                .fill(theme::well())
                .stroke(egui::Stroke::new(1.0, theme::border()))
                .corner_radius(6)
                .inner_margin(egui::Margin::same(3))
                .show(ui, |ui| {
                    crate::covers::paint_square(ui, 152.0, now.as_ref(), before.as_ref(), t)
                        .on_hover_text("Cover des Titels (aus der Datei)");
                });

            // Studio-Uhr: 60 Sekunden-LEDs und Digitalzeit (Ortszeit der Stundenuhr).
            let (h, m, s) = time_parts(crate::clock_editor::now_unix(), self.zone);
            crate::widgets::studio_clock(ui, 152.0, h, m, s);

            theme::lcd_show(ui, |ui| {
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
                            theme::accent()
                        } else {
                            theme::cue()
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
                                            .color(theme::text_dim()),
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
                                .color(theme::text()),
                        )
                        .truncate(),
                    );
                    ui.add_space(2.0);

                    // Die Uhr zeigt die Studio-Uhr links; hier nur der Takt für ihre Sekunden.
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(250));

                    let urgent = v.running && v.remaining_title < 10.0;
                    let clock_col = if urgent {
                        theme::on_air()
                    } else {
                        theme::accent()
                    };
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(clock(v.remaining_title))
                                .font(theme::seg_font(54.0))
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
                                    .color(theme::accent()),
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
                                    .color(theme::text_dim()),
                            );
                            ui.label(
                                egui::RichText::new(clock(v.remaining_total))
                                    .size(24.0)
                                    .strong()
                                    .monospace()
                                    .color(theme::text()),
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
                                theme::on_air()
                            } else {
                                theme::text_dim()
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
                            .color(theme::text_dim()),
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
                    ui.painter().rect_filled(bar_rect, 3.0, theme::panel());
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
            let (go_icon, go_text) = if v.running {
                (theme::icf::PAUSE, "Pause")
            } else {
                (theme::icf::PLAY, "Play")
            };
            let go_btn = egui::Button::new(theme::fill_label(
                go_icon,
                go_text,
                15.0,
                if v.running {
                    theme::well()
                } else {
                    theme::text()
                },
            ))
            .fill(if v.running {
                theme::ok()
            } else {
                theme::mix(theme::ok(), theme::surface(), 0.72)
            })
            .stroke(egui::Stroke::new(1.0, theme::ok()))
            .min_size(egui::vec2(104.0, 38.0));
            let r = ui.add(go_btn).on_hover_text("Play / Pause (Leertaste)");
            if r.clicked() {
                for e in woa_radio::party::PartyState::default()
                    .apply(woa_radio::party::PartyCommand::PlayPause, v.running)
                {
                    self.party_effect(e);
                }
            }
            self.learn_menu(&r, Target::StartNext);

            let r = ui
                .add(
                    egui::Button::new(theme::fill_label(
                        theme::icf::STOP,
                        "Stopp",
                        15.0,
                        theme::text(),
                    ))
                    .fill(theme::surface())
                    .min_size(egui::vec2(88.0, 38.0)),
                )
                .on_hover_text("Stopp: 1× hält an wie Pause, 2× stoppt");
            if r.clicked() {
                self.with(|c| c.stop_or_pause());
            }
            self.learn_menu(&r, Target::Stop);

            // Deck startet und löst die Playlist ab (mit Blende, Umschalt: sofort).
            if let Some((k, fade)) = deck_take_buttons(ui, &v.decks, v.crossfade_seconds) {
                self.with(move |c| {
                    if let Err(e) = c.deck_take_over(k, fade) {
                        c.report(e);
                    }
                });
            }

            // Weitere Transportknöpfe: derselbe Weg wie im Party-Modus und am Handy
            // (`PartyState::apply` und die Effekte in `party_effect`).
            let mut state = woa_radio::party::PartyState::default();
            for (label, cmd, tip) in [
                (
                    theme::icf::SKIP_BACK,
                    woa_radio::party::PartyCommand::Previous,
                    "Vorheriger Titel (P)",
                ),
                (
                    theme::icf::SKIP_FORWARD,
                    woa_radio::party::PartyCommand::Next,
                    "Nächster Titel (N)",
                ),
                (
                    theme::icf::ARROW_COUNTER_CLOCKWISE,
                    woa_radio::party::PartyCommand::Restart,
                    "Titel von vorn",
                ),
                (
                    "−10 s",
                    woa_radio::party::PartyCommand::SeekBack,
                    "10 s zurück",
                ),
                (
                    "+10 s",
                    woa_radio::party::PartyCommand::SeekForward,
                    "10 s vor",
                ),
            ] {
                // Symbole (Fill-Stil) stehen in `theme::icf`, reiner Text ("−10 s") bleibt Text.
                let text: egui::WidgetText = if label
                    .chars()
                    .next()
                    .is_some_and(|c| ('\u{E000}'..='\u{F8FF}').contains(&c))
                {
                    theme::fill_label(label, "", 15.0, theme::text()).into()
                } else {
                    big(label).into()
                };
                let b = egui::Button::new(text)
                    .fill(theme::surface())
                    .min_size(egui::vec2(48.0, 38.0));
                if ui.add(b).on_hover_text(tip).clicked() {
                    for e in state.apply(cmd, v.running) {
                        self.party_effect(e);
                    }
                }
            }
            let (rep_label, rep_tip) = match v.repeat {
                woa_radio::control::Repeat::Off => (theme::icf::REPEAT, "Wiederholen: aus"),
                woa_radio::control::Repeat::All => {
                    (theme::icf::REPEAT, "Wiederholen: ganze Playlist")
                }
                woa_radio::control::Repeat::One => {
                    (theme::icf::REPEAT_ONCE, "Wiederholen: ein Titel")
                }
            };
            let on = v.repeat != woa_radio::control::Repeat::Off;
            let b = egui::Button::new(theme::fill_label(
                rep_label,
                "",
                15.0,
                if on { theme::well() } else { theme::text() },
            ))
            .fill(if on {
                theme::accent()
            } else {
                theme::surface()
            })
            .min_size(egui::vec2(48.0, 38.0));
            if ui.add(b).on_hover_text(rep_tip).clicked() {
                self.party_effect(woa_radio::party::Effect::CycleRepeat);
            }
            let b = egui::Button::new(theme::fill_label(
                theme::icf::SHUFFLE,
                "",
                15.0,
                if v.shuffle {
                    theme::well()
                } else {
                    theme::text()
                },
            ))
            .fill(if v.shuffle {
                theme::accent()
            } else {
                theme::surface()
            })
            .min_size(egui::vec2(48.0, 38.0));
            if ui
                .add(b)
                .on_hover_text(if v.shuffle {
                    "Zufall: an"
                } else {
                    "Zufall: aus"
                })
                .clicked()
            {
                self.party_effect(woa_radio::party::Effect::ToggleShuffle);
            }

            let mic = egui::Button::new(
                big(&format!("{} Mikro (M)", theme::ic::MICROPHONE)).color(if v.mic_open {
                    egui::Color32::WHITE
                } else {
                    theme::text()
                }),
            )
            .fill(if v.mic_open {
                theme::on_air()
            } else {
                theme::surface()
            })
            .stroke(egui::Stroke::new(
                1.0,
                if v.mic_open {
                    theme::emphasize(theme::on_air(), 0.35)
                } else {
                    theme::border()
                },
            ))
            .min_size(egui::vec2(108.0, 38.0));
            let r = ui.add(mic).on_hover_text("Taste M");
            if r.clicked() {
                self.toggle_mic();
            }
            self.learn_menu(&r, Target::Mic);

            let ph = v.phone_on;
            let phone = egui::Button::new(
                big(&format!(
                    "{} {}",
                    theme::ic::PHONE,
                    if ph { "Telefon auf Sendung" } else { "Telefon" }
                ))
                .color(if ph {
                    egui::Color32::WHITE
                } else {
                    theme::text()
                }),
            )
            .fill(if ph {
                theme::on_air()
            } else {
                theme::surface()
            })
            .stroke(egui::Stroke::new(
                1.0,
                if ph {
                    theme::mix(theme::on_air(), egui::Color32::WHITE, 0.35)
                } else {
                    theme::border()
                },
            ))
            .min_size(egui::vec2(176.0, 38.0));
            if ui
                .add(phone)
                .on_hover_text(
                    "Anrufer auf die Sendung legen. Der Pegel rechts zeigt ihn auch vorher; \
                     den Rückweg zum Anrufer wählst du unter Ausgänge.",
                )
                .clicked()
            {
                self.toggle_phone();
            }
            let (rect, _) = ui.allocate_exact_size(egui::vec2(70.0, 12.0), egui::Sense::hover());
            theme::led_meter(ui.painter(), rect, self.phone_level);
            if let Some(e) = &self.phone_error {
                ui.colored_label(theme::on_air(), e);
            }

            // Mitschnitt: seit 0.34.56 als REC-Pille in der Kopfleiste, in jedem Modus.
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
                    .color(theme::text_dim()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                theme::status_pill(
                    ui,
                    &format!("ZIEL {:.0} LUFS", v.target_lufs),
                    theme::accent(),
                    v.bus_processing,
                );
            });
        });
        ui.add_space(4.0);

        // Regler für Crossfade und Ducking in einem eingefassten Panel
        theme::lcd_show(ui, |ui| {
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
            let mut cart_duck = v.cart_duck;
            if ui
                .checkbox(&mut cart_duck, "auch bei Carts")
                .on_hover_text("Musik leiser, solange ein Cart spielt")
                .changed()
            {
                self.view.cart_duck = cart_duck;
                self.with(move |c| c.set_cart_duck(cart_duck));
            }
            // Stopp mit Ausblenden (#227): kein eigener Knopf, die Stopp-Knöpfe bleiben.
            let (mut on, mut secs) = (v.stop_fade, v.stop_fade_seconds);
            let mut changed = false;
            ui.horizontal(|ui| {
                changed |= ui
                    .checkbox(&mut on, "Stopp blendet aus")
                    .on_hover_text(
                        "An: Stopp am Sende-Player und an den Decks A/B/C blendet über die \
                         eingestellte Dauer aus (etwa um mitten im Titel auszusteigen). \
                         Aus: Stopp bricht sofort ab.",
                    )
                    .changed();
                changed |= ui
                    .add_enabled(
                        on,
                        egui::Slider::new(&mut secs, 0.5..=10.0)
                            .suffix(" s")
                            .text("Ausblenddauer"),
                    )
                    .changed();
            });
            if changed {
                self.view.stop_fade = on;
                self.view.stop_fade_seconds = secs;
                self.with(move |c| c.set_stop_fade(on, secs));
            }
        });
        // Die Pegelregler stehen senkrecht neben den Decks (`level_faders`).
        ui.add_space(4.0);
        self.processing_row(ui);
    }

    /// Sendebus- und Mikro-Processing: Schalter und digitale Messanzeige.
    fn processing_row(&mut self, ui: &mut egui::Ui) {
        let v = &self.view;
        let (mut bus, mut mic) = (v.bus_processing, v.mic_processing);
        let mut changed = false;
        theme::lcd_show(ui, |ui| {
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
                        ("Gate offen", theme::ok(), true)
                    } else {
                        ("Gate zu", theme::text_dim(), false)
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
                        .color(theme::accent()),
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
                    .color(theme::text_dim()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let full = guests.len() >= woa_guests::MAX_GUESTS;
                let file = ui
                    .add_enabled(
                        !full,
                        egui::Button::new(format!("{} Datei als Gast", theme::ic::PLUS)),
                    )
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
                    .add_enabled(
                        !full,
                        egui::Button::new(format!("{} Testgast", theme::ic::PLUS)),
                    )
                    .on_hover_text("Ein Sinuston als Gast, jeder Gast mit eigener Tonhöhe");
                if sine.clicked() {
                    let freq = 330.0 + 110.0 * guests.len() as f32;
                    self.add_guest(woa_guests::TestSignal::Sine { freq });
                }
            });
        });
        ui.add_space(4.0);
        theme::lcd_show(ui, |ui| {
            ui.set_width(ui.available_width());
            if guests.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "Kein Gast. Jeder Gast bekommt einen Kanal und einen Rückweg ohne die eigene Stimme.",
                    )
                    .small()
                    .color(theme::text_dim()),
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
                    .color(theme::text_dim()),
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
                        .add_enabled(!full, egui::Button::new(format!("{} Einladung", theme::ic::PLUS)))
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
        theme::lcd_show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !web.running {
                ui.label(
                    egui::RichText::new(
                        "Aus. Einschalten, Einladung erzeugen, Link ans Handy schicken: Der Gast \
                         spricht über den Browser, ohne App.",
                    )
                    .small()
                    .color(theme::text_dim()),
                );
                return;
            }
            for w in &web.waiting {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{} wartet seit {} s", w.name, w.waited_s))
                            .strong()
                            .color(theme::accent()),
                    )
                    .on_hover_text(&w.peer);
                    let ticket = w.ticket.clone();
                    if ui
                        .button(egui::RichText::new("Annehmen").color(theme::ok()))
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
                                    .color(theme::cue()),
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
                            .color(theme::text_dim()),
                        );
                    });
                });
                ui.add_space(4.0);
            }
            if web.invites.is_empty() && web.waiting.is_empty() {
                ui.label(
                    egui::RichText::new("Bereit. „Einladung“ erzeugt einen Link mit QR-Code.")
                        .small()
                        .color(theme::text_dim()),
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
            ui.label(egui::RichText::new(&g.name).strong().color(theme::text()))
                .on_hover_text(&g.source);
            // Pegel vor dem Fader: zeigt auch im Warteraum und stumm, ob der Gast spricht.
            let (rect, _) = ui.allocate_exact_size(egui::vec2(90.0, 12.0), egui::Sense::hover());
            theme::led_meter(ui.painter(), rect, g.input_peak);
            // Auf Sendung / Warteraum.
            let (air_text, air_col, air_tip) = if g.on_air {
                (
                    format!("{} Auf Sendung", theme::ic::BROADCAST),
                    theme::on_air(),
                    "Der Gast ist zu hören. Klick: zurück in den Warteraum (hört weiter die Sendung).",
                )
            } else {
                (
                    format!("{} Warteraum", theme::ic::PAUSE),
                    theme::text_dim(),
                    "Der Gast hört die Sendung, ist aber nicht zu hören. Klick: auf Sendung holen.",
                )
            };
            if ui
                .add(egui::Button::new(egui::RichText::new(air_text).strong().color(air_col)))
                .on_hover_text(air_tip)
                .clicked()
            {
                let on = !g.on_air;
                if let Some(x) = self.view.guests.iter_mut().find(|x| x.slot == slot) {
                    x.on_air = on;
                }
                self.with(move |c| c.guests().set_on_air(slot, on));
            }
            // Stumm (Moderator): nicht auf Sendung, hört die Sendung weiter.
            let mute = egui::Button::new(
                egui::RichText::new(format!("{} Stumm", theme::ic::MICROPHONE_SLASH)).color(
                    if g.muted {
                        theme::on_air()
                    } else {
                        theme::text_dim()
                    },
                ),
            )
            .selected(g.muted);
            if ui
                .add(mute)
                .on_hover_text("Gast stumm: nicht auf Sendung, hört die Sendung aber weiter")
                .clicked()
            {
                let on = !g.muted;
                if let Some(x) = self.view.guests.iter_mut().find(|x| x.slot == slot) {
                    x.muted = on;
                }
                self.with(move |c| c.guests().set_mute(slot, on));
            }
            // Lautstärke wie der Kanal-Gain: ziehen, Doppelklick = 0 dB.
            let mut db = g.gain_db;
            let r = ui
                .add(
                    egui::DragValue::new(&mut db)
                        .range(-60.0..=12.0)
                        .speed(0.2)
                        .fixed_decimals(1)
                        .suffix(" dB"),
                )
                .on_hover_text("Lautstärke des Gastes. Ziehen, Doppelklick = 0 dB");
            let set = if r.double_clicked() {
                Some(0.0)
            } else if r.changed() {
                Some(db)
            } else {
                None
            };
            if let Some(db) = set {
                if let Some(x) = self.view.guests.iter_mut().find(|x| x.slot == slot) {
                    x.gain_db = db;
                }
                self.with(move |c| c.guests().set_gain_db(slot, db));
            }
            let ret = egui::Button::new(egui::RichText::new(theme::ic::HEADPHONES).color(
                if g.monitored {
                    theme::cue()
                } else {
                    theme::text_dim()
                },
            ));
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
            // Trennen mit kurzer Bestätigung: zweiter Klick binnen 3 s.
            let armed = self
                .guest_hangup_armed
                .is_some_and(|(s, t)| s == slot && t.elapsed() < Duration::from_secs(3));
            let label = if armed {
                egui::RichText::new(format!("{} Wirklich trennen?", theme::ic::X))
                    .color(theme::on_air())
            } else {
                egui::RichText::new(theme::ic::X)
            };
            if ui
                .small_button(label)
                .on_hover_text("Gast trennen (zweimal klicken)")
                .clicked()
            {
                if armed {
                    self.guest_hangup_armed = None;
                    self.with(move |c| c.guests().remove(slot));
                } else {
                    self.guest_hangup_armed = Some((slot, Instant::now()));
                }
            }
            if armed {
                ui.ctx().request_repaint_after(Duration::from_millis(250));
            }
        });
        let mut line = format!("{} · Puffer {:.0} ms", g.source, g.buffered_ms);
        if g.prebuffering {
            line.push_str(" · füllt");
        }
        if let Some(n) = &g.net {
            if n.connected {
                let q = if n.loss_permille > 50 || n.rtt_ms > 300 {
                    "schlecht"
                } else if n.loss_permille > 10 || n.rtt_ms > 150 {
                    "mäßig"
                } else {
                    "gut"
                };
                line.push_str(&format!(
                    " · Leitung {q} · RTT {} ms · Verlust {:.1} %",
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
                    .color(theme::text_dim()),
            );
            if g.self_muted {
                ui.label(
                    egui::RichText::new(format!(
                        "{} hat sich selbst stumm geschaltet",
                        theme::ic::MICROPHONE_SLASH
                    ))
                    .small()
                    .color(theme::accent()),
                );
            }
            if g.underruns > 0 || g.overflows > 0 {
                ui.label(
                    egui::RichText::new(format!(
                        "Aussetzer {} · Überlauf {}",
                        g.underruns, g.overflows
                    ))
                    .small()
                    .color(theme::on_air()),
                );
            }
        });
    }

    fn on_air_row(&mut self, ui: &mut egui::Ui) {
        let now = woa_stream::unix_now();
        let st = self.stream_status.clone();
        let running = st.as_ref().is_some_and(|s| s.is_running(now));
        let on = self.feeder.is_some();
        theme::lcd_show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                let (label, fill, border) = if on {
                    (
                        format!("{} Sendung beenden", theme::ic::BROADCAST),
                        theme::on_air(),
                        theme::emphasize(theme::on_air(), 0.35),
                    )
                } else {
                    (
                        format!("{} Auf Sendung", theme::ic::BROADCAST),
                        theme::mix(theme::accent(), theme::surface(), 0.72),
                        theme::accent(),
                    )
                };
                let text = egui::RichText::new(&label)
                    .size(14.5)
                    .strong()
                    .color(if on {
                        egui::Color32::WHITE
                    } else {
                        theme::text()
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
                            theme::on_air()
                        } else {
                            theme::ok()
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
                        ui.label(
                            egui::RichText::new("starte woa-streamd …").color(theme::text_dim()),
                        );
                    }
                    _ => {
                        ui.label(
                            egui::RichText::new("Stream-Encoder bereit (Shoutcast / Icecast)")
                                .small()
                                .color(theme::text_dim()),
                        );
                    }
                }
            });
            if let Some(p) = st
                .as_ref()
                .filter(|_| running)
                .and_then(StreamStatus::problem)
            {
                ui.label(egui::RichText::new(p).small().color(theme::on_air()));
            }
            if let Some(line) = link_line(
                self.feeder
                    .as_ref()
                    .map(|f| (f.is_connected(), f.reconnects())),
                st.as_ref().filter(|_| running),
            ) {
                ui.label(egui::RichText::new(line).small().color(theme::on_air()));
            }
        });
    }

    fn playlist(&mut self, ui: &mut egui::Ui, v: &View) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("SENDE-PLAYLIST")
                    .size(11.5)
                    .strong()
                    .color(theme::text_dim()),
            );
            let acts = self
                .manage
                .header(ui, &v.playlists, v.active_playlist, v.running);
            self.run_actions(acts);
            let hide = self.hide_played.unwrap_or(v.hide_played);
            let played = played_count(v);
            let count = if hide && played > 0 {
                format!("({} offen, {played} gespielt)", v.playlist.len() - played)
            } else {
                format!("({} Titel)", v.playlist.len())
            };
            ui.label(egui::RichText::new(count).small().color(theme::text_dim()));
            let r = ui
                .selectable_label(hide, egui::RichText::new(theme::ic::EYE_SLASH).small())
                .on_hover_text(
                    "Gespielte ausblenden: Titel vor dem laufenden verschwinden aus der Liste \
                     (nicht gelöscht). „bis Playlist-Ende“ zählt nur die offenen.",
                );
            if r.clicked() {
                let on = !hide;
                self.hide_played = Some(on);
                self.with(move |c| c.set_hide_played(on));
            }
            if v.measuring > 0 {
                ui.spinner();
                ui.label(
                    egui::RichText::new("misst Lautheit …")
                        .small()
                        .color(theme::accent()),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("+ Titel …")
                                .strong()
                                .color(theme::text()),
                        )
                        .fill(theme::mix(theme::accent(), theme::surface(), 0.72))
                        .stroke(egui::Stroke::new(1.0, theme::accent())),
                    )
                    .clicked()
                {
                    self.add_dialog();
                }
            });
        });
        ui.add_space(4.0);

        if v.playlist.is_empty() {
            theme::lcd_show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new(
                            "Audiodateien (MP3, FLAC, WAV, OGG, M4A, AIFF …) hierher ziehen oder „+ Titel …“.",
                        )
                        .color(theme::text_dim()),
                    );
                    ui.add_space(12.0);
                });
            });
        } else {
            // Spaltenkopf wie in mAirList
            egui::Frame::new()
                .fill(theme::panel_header())
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
                        ui.label(
                            egui::RichText::new("#")
                                .small()
                                .strong()
                                .color(theme::text_dim()),
                        );
                        ui.add_space(30.0);
                        ui.label(
                            egui::RichText::new("TITEL & LAUTHEIT")
                                .small()
                                .strong()
                                .color(theme::text_dim()),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new("AKTION  ·  DAUER")
                                    .small()
                                    .strong()
                                    .color(theme::text_dim()),
                            );
                        });
                    });
                });
        }

        let mut released: Option<(usize, egui::Pos2)> = None;
        self.row_rects.clear();
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            let hide_before = if self.hide_played.unwrap_or(v.hide_played) {
                played_count(v)
            } else {
                0
            };
            for (i, e) in v.playlist.iter().enumerate() {
                if i < hide_before {
                    // Ausgeblendet: Platzhalter, damit die Zeilennummern fürs Ziehen stimmen.
                    self.row_rects.push(egui::Rect::NOTHING);
                    continue;
                }
                let on_air = v.current == Some(i);
                let is_cue = i == v.cue && !v.running;
                let row_fill = if on_air {
                    theme::mix(theme::on_air(), theme::panel(), 0.72)
                } else if is_cue {
                    theme::mix(theme::cue(), theme::panel(), 0.82)
                } else if i % 2 == 1 {
                    theme::mix(theme::well(), theme::panel(), 0.65)
                } else {
                    theme::well()
                };
                let stroke = if on_air {
                    egui::Stroke::new(1.0, theme::on_air())
                } else if is_cue {
                    egui::Stroke::new(1.0, theme::cue())
                } else {
                    egui::Stroke::new(1.0, theme::mix(theme::border(), theme::well(), 0.5))
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
                                    theme::on_air()
                                } else if is_cue {
                                    theme::cue()
                                } else {
                                    theme::text_dim()
                                }),
                        );
                        let thumb = self.covers.as_mut().and_then(|c| c.thumb(&e.path));
                        crate::covers::paint_square(ui, 34.0, thumb.as_ref(), None, 1.0);
                        crate::protokoll::damage_badge(ui, &e.path);
                        if let Some(vb) = e.voice {
                            ui.label(
                                egui::RichText::new("VOICE")
                                    .small()
                                    .strong()
                                    .color(theme::accent()),
                            );
                            if vb.check {
                                ui.label(
                                    egui::RichText::new("prüfen")
                                        .small()
                                        .strong()
                                        .color(theme::on_air()),
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
                                    egui::RichText::new(&e.title).strong().color(theme::text()),
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
                                .color(theme::accent()),
                            );
                            if let Some(lb) = lufs_badge {
                                ui.label(
                                    egui::RichText::new(lb)
                                        .size(10.5)
                                        .monospace()
                                        .color(theme::text_dim()),
                                );
                            }
                        });
                    });
                });
                // Ziehen (#166): Zeile auf ein Deck (laden) oder an eine andere Stelle der
                // Playlist (verschieben). Nur Ziehen: Klick, Doppelklick und Kontextmenü auf dem
                // Titel und die Knöpfe bleiben, wie sie sind.
                let row = resp.response.rect;
                self.row_rects.push(row);
                let drag = ui
                    .interact(row, ui.id().with(("playlist_row", i)), egui::Sense::drag())
                    .on_hover_cursor(egui::CursorIcon::Grab);
                if drag.drag_started() {
                    self.row_drag = Some(i);
                }
                if drag.dragged() && self.row_drag == Some(i) {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                    if let Some(p) = ui.ctx().pointer_latest_pos() {
                        egui::Area::new(egui::Id::new("playlist_drag_ghost"))
                            .order(egui::Order::Tooltip)
                            .fixed_pos(p + egui::vec2(12.0, 8.0))
                            .interactable(false)
                            .show(ui.ctx(), |ui| {
                                theme::card_frame().show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(&e.title).strong().color(theme::text()),
                                    );
                                });
                            });
                    }
                }
                if drag.drag_stopped() && self.row_drag == Some(i) {
                    self.row_drag = None;
                    if let Some(p) = drag
                        .interact_pointer_pos()
                        .or(ui.ctx().pointer_latest_pos())
                    {
                        released = Some((i, p));
                    }
                }
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
                            theme::on_air()
                        } else {
                            theme::cue()
                        },
                    );
                }
            }
        });
        if let Some((from, pos)) = released {
            match row_drop_target(
                pos,
                from,
                &self.deck_rects,
                self.playlist_rect,
                &self.row_rects,
            ) {
                RowDrop::Deck(k) => self.with(move |c| {
                    if let Err(e) = c.deck_load_title(k, from) {
                        c.report(e);
                    }
                }),
                RowDrop::Row(to) => self.with(move |c| {
                    c.move_entry(from, to);
                }),
                RowDrop::Nothing => {}
            }
        }
    }

    fn cart_wall(&mut self, ui: &mut egui::Ui, v: &View) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("CART-WALL (JINGLES & SOUNDS)")
                    .size(11.5)
                    .strong()
                    .color(theme::text_dim()),
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
                    .color(theme::text_dim()),
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
                    // Die Nummer ist die große Zahl; die Taste steht klein in der Ecke,
                    // sobald sie nicht die Nummer selbst ist.
                    let key = v.cart_keys.get(slot).cloned().flatten();
                    let key_text = match &key {
                        Some(k) if cart_number_key(slot).as_deref() == Some(k.as_str()) => None,
                        Some(k) => Some(k.clone()),
                        None => Some("–".to_string()),
                    };
                    let r = crate::widgets::CartPad {
                        number: slot + 1,
                        title: label.as_deref(),
                        key: key_text.as_deref(),
                        capturing: self.key_capture == Some(slot),
                        playing,
                        color: theme::track_color(slot),
                        remaining: v.cart_remaining.get(slot).copied().filter(|r| *r > 0.0),
                    }
                    .ui(ui, egui::vec2(w, 68.0));
                    if self.cart_rects.len() <= slot {
                        self.cart_rects.resize(slot + 1, egui::Rect::NOTHING);
                    }
                    self.cart_rects[slot] = r.rect;

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
                                .add_enabled(
                                    v.pfl_available,
                                    pfl_button(v.pfl_source == Some(PflSource::Cart(slot))),
                                )
                                .on_disabled_hover_text(self.pfl_hint())
                                .clicked()
                        {
                            self.with(move |c| {
                                if let Err(e) = c.pfl_toggle(PflSource::Cart(slot)) {
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
        // Doppelklick setzt hier schon „als Nächstes“, darum Titelinfos nur im Kontextmenü.
        if ui.button("Titelinfos bearbeiten …").clicked() {
            self.tag_wish = Some(e.path.clone());
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
        let here = v.pfl_source == Some(PflSource::Title(i));
        for hook in [false, true] {
            if hook && e.cues.hook.is_none() {
                continue;
            }
            let button = if hook {
                egui::Button::new("Vorhören ab Hook")
            } else {
                pfl_button(here)
            };
            if ui
                .add_enabled(v.pfl_available, button)
                .on_disabled_hover_text(self.pfl_hint())
                .clicked()
            {
                self.with(move |c| {
                    let r = if hook {
                        c.pfl_title(i, true)
                    } else {
                        c.pfl_toggle(PflSource::Title(i))
                    };
                    if let Err(e) = r {
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

    /// Fenster „Verwalten“, Rückfrage beim Wechsel und Undo-Hinweis (aus `app.rs` je Bild).
    pub fn manage_window(
        &mut self,
        ctx: &egui::Context,
        library: &mut crate::library::LibraryWindow,
    ) {
        let mut acts = Vec::new();
        // Neue aktive Playlist: ungemessene Titel im Hintergrund messen.
        if self.manage.seen_active != Some(self.view.active_playlist)
            && let Some(st) = &self.station
        {
            if self.manage.seen_active.is_some() {
                st.measure_missing(&st.show());
            }
            self.manage.seen_active = Some(self.view.active_playlist);
        }
        let names = self.view.playlists.clone();
        if let Some(a) = self.manage.confirm_ui(ctx, &names) {
            acts.push(a);
        }
        if self.manage.open
            && let Some(st) = &self.station
        {
            let show = st.show();
            let target = show
                .playlists
                .get(self.manage.selected)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            acts.extend(self.manage.window(ctx, &show, self.view.running, |ui| {
                crate::titel_tab::titel_tab(ui, library, &target)
            }));
        }
        if let Some(a) = self.manage.undo_toast(ctx) {
            acts.push(a);
        }
        self.run_actions(acts);
    }

    /// Schickt Aufträge aus Kopf und Fenster an den Controller.
    fn run_actions(&mut self, acts: Vec<crate::manage::Action>) {
        use crate::manage::{Action, UndoSlot};
        for a in acts {
            match a {
                Action::Select(i) => self.with(move |c| {
                    c.select_playlist(i);
                }),
                Action::New => {
                    let open = self.manage.open;
                    self.with(move |c| {
                        let i = c.new_playlist("");
                        if !open && !c.on_air() {
                            c.select_playlist(i);
                        }
                    });
                    // Im Fenster gleich auswählen (neue steht hinten).
                    self.manage.selected = self.view.playlists.len();
                    self.manage.marked = None;
                }
                Action::Rename(i, name) => self.with(move |c| {
                    if let Err(e) = c.rename_playlist(i, &name) {
                        c.report(e);
                    }
                }),
                Action::Duplicate(i) => {
                    self.manage.selected = i + 1;
                    self.with(move |c| {
                        c.duplicate_playlist(i);
                    });
                }
                Action::Clear(i) => self.with(move |c| {
                    c.clear_playlist(i);
                }),
                Action::Delete(i) => {
                    let slot = self.manage.undo.clone();
                    self.with(move |c| match c.delete_playlist(i) {
                        Ok(d) => {
                            if let Ok(mut s) = slot.lock() {
                                *s = Some(UndoSlot {
                                    deleted: d,
                                    until: None,
                                });
                            }
                        }
                        Err(e) => c.report(e),
                    });
                    self.manage.marked = None;
                }
                Action::Undo => {
                    let taken = self.manage.undo.lock().ok().and_then(|mut s| s.take());
                    if let Some(u) = taken {
                        self.manage.selected = u.deleted.index;
                        self.with(move |c| {
                            c.restore_playlist(u.deleted);
                        });
                    }
                }
                Action::Import(p) => self.with(move |c| {
                    if let Err(e) = c.import_m3u(&p) {
                        c.report(e);
                    }
                }),
                Action::Export(i, p) => self.with(move |c| {
                    if let Err(e) = c.export_m3u(i, &p) {
                        c.report(e);
                    }
                }),
                Action::Add(i, files) => {
                    if i == self.view.active_playlist
                        && let Some(s) = &self.station
                    {
                        // Wie „+ Titel …“: messen im Hintergrund.
                        s.add_files(files);
                    } else {
                        self.with(move |c| {
                            c.playlist_add(i, &files);
                        });
                    }
                }
                Action::Remove(i, k) => self.with(move |c| {
                    c.playlist_remove(i, k);
                }),
                Action::Move(i, from, to) => self.with(move |c| {
                    c.playlist_move(i, from, to);
                }),
            }
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

    /// Warum das Vorhören aus ist: Einstellung „Aus“ oder der Fehler beim Öffnen.
    pub fn pfl_note(&self) -> Option<&str> {
        self.pfl_note.as_deref()
    }

    /// Tooltip für deaktivierte Vorhören-Knöpfe.
    pub fn pfl_hint(&self) -> String {
        pfl_hint_for(self.pfl_note.as_deref())
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
                .color(theme::text_dim()),
        );
        ui.add_space(4.0);
        // Decks links, die senkrechten Pegelregler als eine Gruppe rechts daneben.
        ui.horizontal_top(|ui| {
            let w = (ui.available_width() - 270.0).max(260.0);
            ui.allocate_ui(egui::vec2(w, 0.0), |ui| {
                ui.vertical(|ui| self.deck_rows(ui, v));
            });
            self.level_fader_group(ui);
        });
    }

    /// Pegel Playlist, Carts, Decks, Mikro als senkrechte Fader (Werte wie bisher).
    fn level_fader_group(&mut self, ui: &mut egui::Ui) {
        let mut dbs = FADER_BUSES.map(|(b, _)| self.level_db(b) as f32);
        let rs = level_faders(ui, &mut dbs, &self.meters);
        for ((r, (b, _)), db) in rs.iter().zip(FADER_BUSES).zip(dbs) {
            if r.changed() {
                self.set_level(b, f64::from(db));
            }
            self.learn_menu(r, Target::Level(b));
        }
    }

    fn deck_rows(&mut self, ui: &mut egui::Ui, v: &View) {
        for k in 0..woa_radio::DECKS {
            let d = v.decks.get(k).cloned().unwrap_or_default();
            let name = woa_radio::show::deck_name(k);
            let loaded = d.title.is_some();
            let urgent = d.playing && d.remaining < 10.0;
            let fill = if d.playing {
                theme::mix(theme::accent(), theme::surface(), 0.2)
            } else {
                theme::well()
            };
            let frame = egui::Frame::new()
                .fill(fill)
                .stroke(egui::Stroke::new(
                    if d.playing { 2.0 } else { 1.0 },
                    if d.playing {
                        theme::accent()
                    } else {
                        theme::border()
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
                            .color(theme::text()),
                    );
                    ui.vertical(|ui| {
                        ui.set_min_width(DECK_CLOCK_WIDTH);
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
                                theme::on_air()
                            } else {
                                theme::text()
                            }),
                        );
                        let frac = if d.length > 0.0 {
                            (1.0 - d.remaining / d.length).clamp(0.0, 1.0) as f32
                        } else {
                            0.0
                        };
                        // Feste Breite: sonst nimmt der Balken die ganze Zeile und der Titel
                        // daneben bricht Zeichen für Zeichen um (#168).
                        ui.add(
                            egui::ProgressBar::new(frac)
                                .desired_width(DECK_CLOCK_WIDTH)
                                .desired_height(4.0),
                        );
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
                                        theme::text()
                                    } else {
                                        theme::text_dim()
                                    }),
                                )
                                .truncate()
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
                                    pfl_button(v.pfl_source == Some(PflSource::Deck(k)))
                                        .min_size(egui::vec2(70.0, 30.0)),
                                )
                                .on_disabled_hover_text(self.pfl_hint())
                                .clicked()
                            {
                                self.with(move |c| {
                                    if let Err(e) = c.pfl_toggle(PflSource::Deck(k)) {
                                        c.report(e);
                                    }
                                });
                            }
                            if deck_return_toggle(ui, k, d.back_to_playlist).clicked() {
                                let on = !d.back_to_playlist;
                                self.with(move |c| c.set_deck_return(k, on));
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
    fn dragged_row_lands_on_deck_row_or_nowhere() {
        use super::{RowDrop, row_drop_target};
        use eframe::egui::{Rect, pos2, vec2};
        let decks = [
            Rect::from_min_size(pos2(1000.0, 100.0), vec2(400.0, 80.0)),
            Rect::from_min_size(pos2(1000.0, 190.0), vec2(400.0, 80.0)),
            Rect::from_min_size(pos2(1000.0, 280.0), vec2(400.0, 80.0)),
        ];
        let playlist = Rect::from_min_size(pos2(0.0, 400.0), vec2(900.0, 500.0));
        let rows: Vec<Rect> = (0..5)
            .map(|k| Rect::from_min_size(pos2(10.0, 450.0 + 50.0 * k as f32), vec2(880.0, 46.0)))
            .collect();
        let t = |p| row_drop_target(p, 1, &decks, playlist, &rows);
        assert_eq!(t(pos2(1200.0, 220.0)), RowDrop::Deck(1));
        assert_eq!(t(pos2(500.0, 620.0)), RowDrop::Row(3));
        // Auf sich selbst, in die Lücke oder außerhalb der Playlist: nichts.
        assert_eq!(t(pos2(500.0, 520.0)), RowDrop::Nothing);
        assert_eq!(t(pos2(500.0, 548.0)), RowDrop::Nothing);
        assert_eq!(t(pos2(950.0, 50.0)), RowDrop::Nothing);
        // Zeilen, die aus der Playlist gerollt sind, zählen nicht.
        assert_eq!(
            row_drop_target(pos2(500.0, 620.0), 1, &decks, Rect::NOTHING, &rows),
            RowDrop::Nothing
        );
    }

    /// Ein Bild mit `f`, Rückgabe: (Beschriftung, Rechteck, deaktiviert) je Bedienelement.
    fn frame(
        ctx: &eframe::egui::Context,
        n: u32,
        events: Vec<eframe::egui::Event>,
        mut f: impl FnMut(&mut eframe::egui::Ui),
    ) -> Vec<(String, eframe::egui::Rect, bool)> {
        use eframe::egui;
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 800.0),
            )),
            time: Some(f64::from(n) * 0.05),
            events,
            ..Default::default()
        };
        if let Some(egui::Event::PointerButton { modifiers, .. }) = input.events.get(1) {
            input.modifiers = *modifiers;
        }
        if n == 0 {
            crate::theme::install_fonts(ctx);
        }
        input.viewports.entry(egui::ViewportId::ROOT).or_default();
        let out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| f(ui));
        });
        let mut nodes = Vec::new();
        if let Some(u) = out.platform_output.accesskit_update {
            for (_, node) in u.nodes {
                let label = node.label().map(str::to_string).unwrap_or_default();
                if let Some(b) = node.bounds() {
                    let r = egui::Rect::from_min_max(
                        egui::pos2(b.x0 as f32, b.y0 as f32),
                        egui::pos2(b.x1 as f32, b.y1 as f32),
                    );
                    nodes.push((label, r, node.is_disabled()));
                }
            }
        }
        nodes
    }

    fn click_at(p: eframe::egui::Pos2, shift: bool) -> Vec<eframe::egui::Event> {
        use eframe::egui::{Event, Modifiers, PointerButton};
        let modifiers = if shift {
            Modifiers::SHIFT
        } else {
            Modifiers::NONE
        };
        vec![
            Event::PointerMoved(p),
            Event::PointerButton {
                pos: p,
                button: PointerButton::Primary,
                pressed: true,
                modifiers,
            },
            Event::PointerButton {
                pos: p,
                button: PointerButton::Primary,
                pressed: false,
                modifiers,
            },
        ]
    }

    fn find<'a>(
        nodes: &'a [(String, eframe::egui::Rect, bool)],
        text: &str,
    ) -> &'a (String, eframe::egui::Rect, bool) {
        nodes
            .iter()
            .find(|(l, _, _)| l.contains(text))
            .unwrap_or_else(|| panic!("„{text}“ fehlt: {nodes:?}"))
    }

    #[test]
    fn deck_buttons_take_over_with_fade_or_hard_and_empty_is_off() {
        use woa_radio::station::DeckView;
        let ctx = eframe::egui::Context::default();
        ctx.enable_accesskit();
        let decks = vec![
            DeckView {
                title: Some("Jingle".into()),
                ..DeckView::default()
            },
            DeckView::default(),
            DeckView {
                title: Some("Bett".into()),
                ..DeckView::default()
            },
        ];
        let hit = std::cell::Cell::new(None);
        let mut ui = |ui: &mut eframe::egui::Ui| {
            ui.horizontal(|ui| {
                if let Some(h) = super::deck_take_buttons(ui, &decks, 3.0) {
                    hit.set(Some(h));
                }
            });
        };
        frame(&ctx, 0, vec![], &mut ui);
        let nodes = frame(&ctx, 1, vec![], &mut ui);
        assert!(!find(&nodes, "Deck A").2, "A geladen: an");
        assert!(find(&nodes, "Deck B").2, "B leer: aus");
        let a = find(&nodes, "Deck A").1.center();
        let c = find(&nodes, "Deck C").1.center();
        let b = find(&nodes, "Deck B").1.center();
        let _ = frame(&ctx, 2, click_at(a, false), &mut ui);
        assert_eq!(hit.take(), Some((0, true)), "Klick: mit Blende");
        let _ = frame(&ctx, 3, vec![], &mut ui);
        let _ = frame(&ctx, 4, click_at(c, true), &mut ui);
        assert_eq!(hit.take(), Some((2, false)), "Umschalt+Klick: sofort");
        let _ = frame(&ctx, 5, vec![], &mut ui);
        let _ = frame(&ctx, 6, click_at(b, false), &mut ui);
        assert_eq!(hit.take(), None, "leeres Deck startet nichts");
    }

    #[test]
    fn back_to_playlist_toggle_is_there_and_clicks() {
        let ctx = eframe::egui::Context::default();
        ctx.enable_accesskit();
        let mut clicked = false;
        let mut ui = |ui: &mut eframe::egui::Ui| {
            clicked |= super::deck_return_toggle(ui, 0, false).clicked();
        };
        frame(&ctx, 0, vec![], &mut ui);
        let nodes = frame(&ctx, 1, vec![], &mut ui);
        let p = find(&nodes, "↩ Playlist").1.center();
        let _ = frame(&ctx, 2, click_at(p, false), &mut ui);
        assert!(clicked);
    }

    #[test]
    fn vertical_faders_are_there_and_set_the_level() {
        let ctx = eframe::egui::Context::default();
        ctx.enable_accesskit();
        let dbs = std::cell::RefCell::new([0.0f32; 4]);
        let changed = std::cell::RefCell::new(Vec::new());
        let mut ui = |ui: &mut eframe::egui::Ui| {
            for (k, r) in super::level_faders(ui, &mut dbs.borrow_mut(), &Default::default())
                .iter()
                .enumerate()
            {
                if r.changed() {
                    changed.borrow_mut().push(k);
                }
            }
        };
        frame(&ctx, 0, vec![], &mut ui);
        let nodes = frame(&ctx, 1, vec![], &mut ui);
        for name in [
            "Pegel Playlist",
            "Pegel Carts",
            "Pegel Decks",
            "Pegel Mikro",
        ] {
            let r = find(&nodes, name).1;
            assert!(
                r.height() > 2.0 * r.width(),
                "{name} steht senkrecht: {r:?}"
            );
        }
        // Klick unten auf den Deck-Fader: Pegel sinkt.
        let r = find(&nodes, "Pegel Decks").1;
        let p = eframe::egui::pos2(r.center().x, r.bottom() - 15.0);
        let _ = frame(&ctx, 2, click_at(p, false), &mut ui);
        assert_eq!(*changed.borrow(), vec![2]);
        assert!(dbs.borrow()[2] < -30.0, "Decks leise: {}", dbs.borrow()[2]);
        assert_eq!(dbs.borrow()[0], 0.0);
        // Doppelklick: zurück auf 0 dB.
        let _ = frame(&ctx, 3, vec![], &mut ui);
        let mut ev = click_at(p, false);
        ev.extend(click_at(p, false));
        let _ = frame(&ctx, 4, ev, &mut ui);
        assert_eq!(dbs.borrow()[2], 0.0, "Doppelklick: 0 dB");
    }

    #[test]
    fn five_led_meters_stand_next_to_the_faders() {
        let ctx = eframe::egui::Context::default();
        ctx.enable_accesskit();
        let mut dbs = [0.0f32; 4];
        let meters: [super::MeterHold; 5] = Default::default();
        let mut ui = |ui: &mut eframe::egui::Ui| {
            let _ = super::level_faders(ui, &mut dbs, &meters);
        };
        frame(&ctx, 0, vec![], &mut ui);
        let nodes = frame(&ctx, 1, vec![], &mut ui);
        for name in ["Playlist", "Carts", "Decks", "Mikro", "Master"] {
            let r = find(&nodes, &format!("Pegelanzeige {name}")).1;
            assert!(
                r.height() > 5.0 * r.width(),
                "{name} senkrecht und schmal: {r:?}"
            );
        }
    }

    #[test]
    fn led_meter_colour_steps() {
        use crate::theme::{BusZone, bus_zone};
        assert_eq!(bus_zone(-1.0), BusZone::Red);
        assert_eq!(bus_zone(-5.9), BusZone::Red);
        assert_eq!(bus_zone(-6.0), BusZone::Yellow);
        assert_eq!(bus_zone(-12.0), BusZone::Yellow);
        assert_eq!(bus_zone(-18.0), BusZone::Green);
        assert_eq!(bus_zone(-30.0), BusZone::Green);
        assert_eq!(bus_zone(-50.0), BusZone::Blue, "zu leise");
    }

    #[test]
    fn meter_peak_holds_then_falls() {
        let mut m = super::MeterHold::default();
        m.step(0.8, 0.0);
        assert_eq!((m.level, m.hold), (0.8, 0.8));
        m.step(0.0, 1.0);
        assert!(m.level < 0.8, "fällt");
        assert_eq!(m.hold, 0.8, "Spitze hält");
        m.step(0.0, 2.0);
        assert!(m.hold < 0.8, "nach der Haltezeit fällt die Spitze");
    }

    #[test]
    fn running_cart_shows_its_countdown() {
        assert_eq!(crate::widgets::pad::countdown(12.2), "-13 s");
        assert_eq!(crate::widgets::pad::countdown(9.43), "-9,4 s");
        let ctx = eframe::egui::Context::default();
        ctx.enable_accesskit();
        let mut ui = |ui: &mut eframe::egui::Ui| {
            crate::widgets::CartPad {
                number: 3,
                title: Some("Station-ID"),
                key: None,
                capturing: false,
                playing: true,
                color: eframe::egui::Color32::RED,
                remaining: Some(12.0),
            }
            .ui(ui, eframe::egui::vec2(200.0, 68.0));
        };
        frame(&ctx, 0, vec![], &mut ui);
        let nodes = frame(&ctx, 1, vec![], &mut ui);
        find(&nodes, "Cart 3: Station-ID -12 s");
    }

    #[test]
    fn clock_format() {
        assert_eq!(super::clock(0.0), "00:00");
        assert_eq!(super::clock(83.4), "01:23");
        assert_eq!(super::clock(3725.0), "1:02:05");
        assert_eq!(super::clock(-3.0), "00:00");
    }
}

/// Zeile unter „Auf Sendung“ zur lokalen Verbindung Oberfläche → woa-streamd und zu
/// Lücken im Sendemix. `feeder`: (verbunden, Neuverbindungen), `None` = nicht auf Sendung.
pub fn link_line(feeder: Option<(bool, u32)>, status: Option<&StreamStatus>) -> Option<String> {
    let (connected, reconnects) = feeder?;
    if !connected {
        return Some("Verbindung zu woa-streamd unterbrochen, verbinde neu …".into());
    }
    let gaps = status.map_or(0, |s| s.live_underruns + s.live_trims);
    match (reconnects, gaps) {
        (0, 0) => None,
        (r, 0) => Some(format!("Verbindung zu woa-streamd {r}× neu aufgebaut")),
        (0, g) => Some(format!("Lücken im Sendestream: {g}")),
        (r, g) => Some(format!(
            "Verbindung zu woa-streamd {r}× neu aufgebaut · Lücken im Sendestream: {g}"
        )),
    }
}

#[cfg(test)]
mod link_line_tests {
    use super::*;

    #[test]
    fn link_line_shows_drop_reconnects_and_gaps() {
        assert_eq!(link_line(None, None), None);
        assert_eq!(link_line(Some((true, 0)), None), None);
        assert!(
            link_line(Some((false, 0)), None)
                .unwrap()
                .contains("verbinde neu")
        );
        assert_eq!(
            link_line(Some((true, 2)), None).as_deref(),
            Some("Verbindung zu woa-streamd 2× neu aufgebaut")
        );
        let s = StreamStatus {
            live_underruns: 3,
            live_trims: 1,
            ..StreamStatus::default()
        };
        assert_eq!(
            link_line(Some((true, 0)), Some(&s)).as_deref(),
            Some("Lücken im Sendestream: 4")
        );
    }
}

/// Tooltip-Text: der Grund, falls bekannt, sonst der allgemeine Hinweis.
pub fn pfl_hint_for(note: Option<&str>) -> String {
    note.map_or_else(|| NO_PFL.to_string(), str::to_string)
}

#[cfg(test)]
mod pfl_hint_tests {
    use super::*;

    #[test]
    fn hint_shows_open_error() {
        let e = "Vorhören aus: Kopfhörer-Gerät geht nicht (busy)";
        assert_eq!(pfl_hint_for(Some(e)), e);
        assert_eq!(pfl_hint_for(Some(PFL_OFF)), PFL_OFF);
        assert_eq!(pfl_hint_for(None), NO_PFL);
    }
}
