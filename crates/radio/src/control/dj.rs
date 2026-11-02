//! DJ-Paket, Steuerseite: die DJ-Ansicht der Party als Kommandos ([`DjCommand`]).
//!
//! Oberfläche und Agent (MCP) schicken dieselben Kommandos an [`Controller::dj`]. Alles hier ist
//! Live-Steuerung und flüchtig wie der Probeton (kein Rückgängig, nicht in der Sendung
//! gespeichert); nur Hot Cues und die Schleife gehen über [`Controller::dj_take_marks`] in die
//! Bibliothek. Ohne Ton testbar: der Zustand liegt hier, der Mixer bekommt nur
//! [`DjCmd`]-Werte.
//!
//! - **Ansicht DJ:** Auto-Übergang und Vorladen der stehenden Seite sind aus (der DJ lädt mit
//!   „in A/B laden“ selbst). Die Einstellung „Auto-Übergang“ der Sendung bleibt unberührt.
//! - **Sync:** Tempo und Takt ans andere Deck. Liegt das Zieltempo außerhalb von ±8 %, wird
//!   zuerst das doppelte und halbe Tempo versucht (Halftime/Doubletime); passt auch das nicht,
//!   bleibt der Fader am Anschlag, Sync geht nicht an und es kommt ein Hinweis.
//! - **Cue:** stehend setzt die Taste den Cue-Punkt an die Stelle; halten spielt ab dem Cue,
//!   loslassen springt zurück und hält an. Läuft das Deck, springt Cue zurück und hält an.

use std::sync::atomic::Ordering;

use serde::{Deserialize, Serialize};
use woa_dsp::analysis::track::{camelot_compatible, camelot_of_name};
use woa_dsp::dj::Curve;

use super::Controller;
use crate::PARTY_SIDES;
use crate::djmix::DjCmd;
use crate::level::db_to_gain;
use crate::mixer::Cmd;

/// Tempo-Fader: voller Weg ±8 %.
pub const TEMPO_RANGE: f64 = 0.08;
/// Anschieben (Jogwheel-Rand, Pfeiltasten): voller Ausschlag ±4 %.
pub const NUDGE_RANGE: f64 = 0.04;
/// Auto-Loop-Längen in Schlägen.
pub const LOOP_BEATS: [u32; 5] = [1, 2, 4, 8, 16];
/// Anzahl der Hot Cues.
pub const HOT_CUES: usize = 4;
/// Trim-Bereich in dB.
pub const TRIM_DB: f32 = 12.0;

/// Ein Band des Kanal-EQs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Band {
    Lo,
    Mid,
    Hi,
}

/// Ein Kommando der DJ-Ansicht. `side` ist 0 (Deck A) oder 1 (Deck B).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum DjCommand {
    /// Ansicht umschalten: `true` DJ, `false` Einfach.
    View {
        dj: bool,
    },
    /// Playlist-Titel `index` (0-basiert) auf das Deck legen.
    Load {
        side: usize,
        index: usize,
    },
    Play {
        side: usize,
    },
    Pause {
        side: usize,
    },
    /// Cue-Taste gedrückt bzw. losgelassen.
    CueDown {
        side: usize,
    },
    CueUp {
        side: usize,
    },
    /// Sync an (Tempo und Takt ans andere Deck) oder aus.
    Sync {
        side: usize,
        on: bool,
    },
    /// Hot Cue `slot` (0–3): leer setzen, belegt anspringen; `clear` löscht ihn.
    HotCue {
        side: usize,
        slot: usize,
        #[serde(default)]
        clear: bool,
    },
    /// Auto-Loop über so viele Schläge (1, 2, 4, 8, 16) ab dem letzten Schlag; `None` = aus.
    Loop {
        side: usize,
        beats: Option<u32>,
    },
    /// Tempo-Fader −1..1 (= −8 … +8 %).
    Tempo {
        side: usize,
        value: f64,
    },
    /// Anschieben −1..1 (= −4 … +4 %), solange gehalten; 0 lässt los.
    Nudge {
        side: usize,
        value: f64,
    },
    /// Scratch: Geschwindigkeit (negativ rückwärts), `None` lässt los.
    Scratch {
        side: usize,
        speed: Option<f64>,
    },
    Keylock {
        side: usize,
        on: bool,
    },
    /// Springen auf die Stelle in Sekunden.
    Seek {
        side: usize,
        seconds: f64,
    },
    /// Trim in dB (−12 … +12).
    Trim {
        side: usize,
        db: f32,
    },
    /// EQ-Band −1..1 (−1 = Kill, 0 = neutral, +1 = +6 dB).
    Eq {
        side: usize,
        band: Band,
        value: f32,
    },
    /// Filter −1 (Tiefpass) … 0 (aus) … +1 (Hochpass).
    Filter {
        side: usize,
        value: f32,
    },
    /// Kanalfader 0..1.
    Fader {
        side: usize,
        value: f32,
    },
    /// Kanal in den Kopfhörer (braucht einen Vorhör-Ausgang).
    Cue {
        side: usize,
        on: bool,
    },
    /// Crossfader 0 (A) … 1 (B).
    Crossfader {
        value: f32,
    },
    /// Kurve des Crossfaders: `true` hart, `false` weich.
    Curve {
        hard: bool,
    },
    /// Master-Pegel der Party 0..1.
    Master {
        value: f32,
    },
    /// Kopfhörer: Mix 0 (Cue) … 1 (Master), Pegel 0..1.
    Phones {
        mix: f32,
        level: f32,
    },
}

/// Was die Analyse zum Titel eines Decks weiß (die Oberfläche liest es aus der Bibliothek).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DjTrackInfo {
    /// Playlist-Index, für den die Angaben gelten (sonst verworfen).
    pub index: usize,
    pub bpm: Option<f64>,
    /// Sekunde des ersten Schlags.
    pub beat_offset: Option<f64>,
    /// Tonart als Anzeigename („a-Moll“).
    pub key: Option<String>,
    /// Inhalts-Hash (Schlüssel der Marken in der Bibliothek).
    pub hash: Option<String>,
    pub hot: [Option<f64>; HOT_CUES],
    pub looped: Option<(f64, f64)>,
}

/// Geänderte Marken eines Titels für die Bibliothek.
#[derive(Debug, Clone, PartialEq)]
pub struct DjMarksChange {
    pub hash: String,
    pub hot: [Option<f64>; HOT_CUES],
    pub looped: Option<(f64, f64)>,
}

/// Steuerzustand eines Decks.
#[derive(Debug, Clone, PartialEq)]
struct DeckCtl {
    tempo: f64,
    nudge: f64,
    scratch: Option<f64>,
    keylock: bool,
    sync: bool,
    cue_point: Option<f64>,
    /// Cue gehalten (spielt ab Cue, Loslassen springt zurück).
    cue_held: bool,
    loop_beats: Option<u32>,
    trim_db: f32,
    eq: [f32; 3],
    filter: f32,
    fader: f32,
    cue: bool,
    info: Option<DjTrackInfo>,
}

impl Default for DeckCtl {
    fn default() -> Self {
        Self {
            tempo: 0.0,
            nudge: 0.0,
            scratch: None,
            keylock: false,
            sync: false,
            cue_point: None,
            cue_held: false,
            loop_beats: None,
            trim_db: 0.0,
            eq: [0.0; 3],
            filter: 0.0,
            fader: 1.0,
            cue: false,
            info: None,
        }
    }
}

/// Steuerzustand der DJ-Ansicht im Controller.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct DjCtl {
    pub(super) view: bool,
    decks: [DeckCtl; PARTY_SIDES],
    curve_hard: bool,
    master: f32,
    phones_mix: f32,
    phones_level: f32,
    dirty: Vec<DjMarksChange>,
}

impl Default for DjCtl {
    fn default() -> Self {
        Self {
            view: false,
            decks: Default::default(),
            curve_hard: false,
            master: 1.0,
            phones_mix: 0.5,
            phones_level: 0.8,
            dirty: Vec::new(),
        }
    }
}

/// Schnappschuss eines Decks für Oberfläche und Agent.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct DjDeckView {
    pub index: Option<usize>,
    pub playing: bool,
    /// Stelle und Ende in Sekunden.
    pub position: f64,
    pub end: f64,
    /// Tempo aus der Analyse und wie es gerade läuft.
    pub bpm: Option<f64>,
    pub bpm_now: Option<f64>,
    /// Tempo-Fader −1..1 und die Abweichung in Prozent (mit Anschieben).
    pub tempo: f64,
    pub tempo_percent: f64,
    pub key: Option<String>,
    pub camelot: Option<String>,
    /// Tonart passt zum anderen Deck (Camelot: gleich, ±1, Dur/Moll-Paar).
    pub key_match: bool,
    pub keylock: bool,
    pub sync: bool,
    /// Lage im Takt 0..1 (0 = auf dem Schlag), ohne Beatgrid `None`.
    pub phase: Option<f64>,
    pub cue_point: Option<f64>,
    pub hot: [Option<f64>; HOT_CUES],
    pub looped: Option<(f64, f64)>,
    pub loop_beats: Option<u32>,
    pub trim_db: f32,
    pub eq_lo: f32,
    pub eq_mid: f32,
    pub eq_hi: f32,
    pub filter: f32,
    pub fader: f32,
    pub cue: bool,
    /// Spitze nach dem Kanalfader (linear, seit dem letzten Lesen der Oberfläche).
    #[serde(skip)]
    pub level: f32,
}

/// Schnappschuss der DJ-Ansicht.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct DjView {
    pub dj: bool,
    pub decks: [DjDeckView; PARTY_SIDES],
    pub crossfader: f32,
    pub curve_hard: bool,
    pub master: f32,
    pub phones_mix: f32,
    pub phones_level: f32,
    /// Ein Vorhör-Ausgang ist da (sonst sind die Cue-Tasten gesperrt).
    pub cue_available: bool,
}

fn side_ok(side: usize) -> Result<(), String> {
    if side < PARTY_SIDES {
        Ok(())
    } else {
        Err(format!("Deck {side} gibt es nicht (0 = A, 1 = B)"))
    }
}

fn finite(v: f64, what: &str) -> Result<f64, String> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(format!("{what}: keine Zahl"))
    }
}

/// Lage im Takt 0..1 an `pos` Sekunden.
pub fn beat_phase(pos: f64, bpm: f64, offset: f64) -> f64 {
    let beats = (pos - offset) * bpm / 60.0;
    beats - beats.floor()
}

/// Wählt für Sync das Verhältnis Ziel/Eigen, das dem Originaltempo am nächsten liegt
/// (auch doppelt und halb). `None`, wenn keines in ±`range` liegt.
pub fn sync_ratio(own_bpm: f64, target_bpm: f64, range: f64) -> Option<f64> {
    if !(own_bpm > 0.0 && target_bpm > 0.0) {
        return None;
    }
    let r = target_bpm / own_bpm;
    [r, r * 2.0, r / 2.0]
        .into_iter()
        .min_by(|a, b| (a - 1.0).abs().total_cmp(&(b - 1.0).abs()))
        .filter(|x| (x - 1.0).abs() <= range + 1e-9)
}

impl Controller {
    /// Ist die DJ-Ansicht an?
    pub fn dj_active(&self) -> bool {
        self.dj.view
    }

    /// Ein Kommando der DJ-Ansicht ausführen. Fehler sind Hinweise für Mensch und Agent.
    pub fn dj(&mut self, cmd: DjCommand) -> Result<(), String> {
        if self.duo.is_none() {
            return Err("Party ist nicht an".into());
        }
        match cmd {
            DjCommand::View { dj } => self.dj_view_set(dj),
            DjCommand::Load { side, index } => {
                side_ok(side)?;
                if index >= self.show.playlist.len() {
                    return Err(format!("Titel {index} gibt es nicht in der Playlist"));
                }
                if self.party_side_playing_pub(side) {
                    return Err("Das Deck spielt gerade; erst anhalten".into());
                }
                if !self.party_load_side(side, index) {
                    return Err(self
                        .message
                        .clone()
                        .unwrap_or_else(|| "Laden ging nicht".into()));
                }
                Ok(())
            }
            DjCommand::Play { side } => {
                side_ok(side)?;
                self.dj.decks[side].cue_held = false;
                self.party_play(side);
                Ok(())
            }
            DjCommand::Pause { side } => {
                side_ok(side)?;
                self.dj.decks[side].cue_held = false;
                self.party_pause(side);
                Ok(())
            }
            DjCommand::CueDown { side } => {
                side_ok(side)?;
                self.dj_cue_down(side)
            }
            DjCommand::CueUp { side } => {
                side_ok(side)?;
                if std::mem::take(&mut self.dj.decks[side].cue_held) {
                    self.party_pause(side);
                    let c = self.dj_cue_point(side);
                    self.dj_jump(side, c);
                }
                Ok(())
            }
            DjCommand::Sync { side, on } => {
                side_ok(side)?;
                if on {
                    self.dj_sync(side)
                } else {
                    self.dj.decks[side].sync = false;
                    Ok(())
                }
            }
            DjCommand::HotCue { side, slot, clear } => {
                side_ok(side)?;
                if slot >= HOT_CUES {
                    return Err(format!("Hot Cue {} gibt es nicht (1–4)", slot + 1));
                }
                self.dj_hot_cue(side, slot, clear)
            }
            DjCommand::Loop { side, beats } => {
                side_ok(side)?;
                self.dj_loop(side, beats)
            }
            DjCommand::Tempo { side, value } => {
                side_ok(side)?;
                let v = finite(value, "Tempo")?.clamp(-1.0, 1.0);
                let d = &mut self.dj.decks[side];
                d.tempo = v;
                d.sync = false;
                self.dj_push_speed(side);
                Ok(())
            }
            DjCommand::Nudge { side, value } => {
                side_ok(side)?;
                self.dj.decks[side].nudge = finite(value, "Anschieben")?.clamp(-1.0, 1.0);
                self.dj_push_speed(side);
                Ok(())
            }
            DjCommand::Scratch { side, speed } => {
                side_ok(side)?;
                let speed = speed.map(|s| finite(s, "Scratch")).transpose()?;
                self.dj.decks[side].scratch = speed;
                self.send(Cmd::Dj(DjCmd::Scratch { side, speed }));
                Ok(())
            }
            DjCommand::Keylock { side, on } => {
                side_ok(side)?;
                self.dj.decks[side].keylock = on;
                self.send(Cmd::Dj(DjCmd::Keylock { side, on }));
                Ok(())
            }
            DjCommand::Seek { side, seconds } => {
                side_ok(side)?;
                let s = finite(seconds, "Stelle")?.max(0.0);
                self.dj_jump(side, s);
                Ok(())
            }
            DjCommand::Trim { side, db } => {
                side_ok(side)?;
                let db = if db.is_finite() { db } else { 0.0 }.clamp(-TRIM_DB, TRIM_DB);
                self.dj.decks[side].trim_db = db;
                self.send(Cmd::Dj(DjCmd::Trim {
                    side,
                    gain: db_to_gain(f64::from(db)),
                }));
                Ok(())
            }
            DjCommand::Eq { side, band, value } => {
                side_ok(side)?;
                let v = if value.is_finite() { value } else { 0.0 }.clamp(-1.0, 1.0);
                let d = &mut self.dj.decks[side];
                d.eq[band as usize] = v;
                let [lo, mid, hi] = d.eq;
                self.send(Cmd::Dj(DjCmd::Eq { side, lo, mid, hi }));
                Ok(())
            }
            DjCommand::Filter { side, value } => {
                side_ok(side)?;
                let v = if value.is_finite() { value } else { 0.0 }.clamp(-1.0, 1.0);
                self.dj.decks[side].filter = v;
                self.send(Cmd::Dj(DjCmd::Filter { side, value: v }));
                Ok(())
            }
            DjCommand::Fader { side, value } => {
                side_ok(side)?;
                let v = if value.is_finite() { value } else { 1.0 }.clamp(0.0, 1.0);
                self.dj.decks[side].fader = v;
                self.send(Cmd::Dj(DjCmd::Fader { side, gain: v }));
                Ok(())
            }
            DjCommand::Cue { side, on } => {
                side_ok(side)?;
                if on && !self.dj_cue_available() {
                    return Err(DJ_NO_CUE.into());
                }
                self.dj.decks[side].cue = on;
                self.send(Cmd::Dj(DjCmd::Cue { side, on }));
                Ok(())
            }
            DjCommand::Crossfader { value } => {
                if !value.is_finite() {
                    return Err("Crossfader: keine Zahl".into());
                }
                self.party_fader(value);
                Ok(())
            }
            DjCommand::Curve { hard } => {
                self.dj.curve_hard = hard;
                let c = if hard { Curve::Hard } else { Curve::Soft };
                self.send(Cmd::Dj(DjCmd::Curve(c)));
                Ok(())
            }
            DjCommand::Master { value } => {
                let v = if value.is_finite() { value } else { 1.0 }.clamp(0.0, 1.0);
                self.dj.master = v;
                self.send(Cmd::Dj(DjCmd::Master(v)));
                Ok(())
            }
            DjCommand::Phones { mix, level } => {
                let mix = if mix.is_finite() { mix } else { 0.5 }.clamp(0.0, 1.0);
                let level = if level.is_finite() { level } else { 0.8 }.clamp(0.0, 1.0);
                self.dj.phones_mix = mix;
                self.dj.phones_level = level;
                self.send(Cmd::Dj(DjCmd::Phones { mix, level }));
                Ok(())
            }
        }
    }

    /// Ist ein Vorhör-Ausgang für den Kopfhörer-Cue da?
    pub fn dj_cue_available(&self) -> bool {
        self.status.dj_cue_bus.load(Ordering::Acquire) || self.pfl.is_some()
    }

    /// Angaben aus der Analyse für das Deck `side` (Tempo, Beatgrid, Tonart, Marken).
    /// Gilt nur, wenn dort noch der Titel `info.index` liegt.
    pub fn dj_track_info(&mut self, side: usize, info: DjTrackInfo) {
        if side >= PARTY_SIDES || self.party_view_index(side) != Some(info.index) {
            return;
        }
        // Eine gespeicherte Schleife ist eine Marke, sie läuft nicht von selbst.
        self.dj.decks[side].info = Some(info);
    }

    /// Hot Cues und Schleifen, die sich seit dem letzten Aufruf geändert haben.
    pub fn dj_take_marks(&mut self) -> Vec<DjMarksChange> {
        std::mem::take(&mut self.dj.dirty)
    }

    /// Schnappschuss der DJ-Ansicht (`None`, wenn die Party aus ist).
    pub fn dj_view(&self) -> Option<DjView> {
        let pv = self.party_view()?;
        let decks = std::array::from_fn(|s| {
            let d = &self.dj.decks[s];
            let info = d
                .info
                .as_ref()
                .filter(|i| Some(i.index) == pv.sides[s].index);
            let bpm = info.and_then(|i| i.bpm);
            let pos = pv.sides[s].position;
            let key = info.and_then(|i| i.key.clone());
            let camelot = key.as_deref().and_then(camelot_of_name);
            let other = self.dj.decks[1 - s]
                .info
                .as_ref()
                .filter(|i| Some(i.index) == pv.sides[1 - s].index)
                .and_then(|i| i.key.as_deref())
                .and_then(camelot_of_name);
            let key_match =
                matches!((&camelot, &other), (Some(a), Some(b)) if camelot_compatible(a, b));
            let looped = self.dj_loop_seconds(s);
            DjDeckView {
                index: pv.sides[s].index,
                playing: pv.sides[s].playing,
                position: pos,
                end: pv.sides[s].end,
                bpm,
                bpm_now: bpm.map(|b| b * self.dj_speed_of(s)),
                tempo: d.tempo,
                tempo_percent: (self.dj_speed_of(s) - 1.0) * 100.0,
                key,
                camelot,
                key_match,
                keylock: d.keylock,
                sync: d.sync,
                phase: bpm
                    .map(|b| beat_phase(pos, b, info.and_then(|i| i.beat_offset).unwrap_or(0.0))),
                cue_point: d.cue_point,
                hot: info.map_or([None; HOT_CUES], |i| i.hot),
                looped,
                loop_beats: d.loop_beats.filter(|_| looped.is_some()),
                trim_db: d.trim_db,
                eq_lo: d.eq[0],
                eq_mid: d.eq[1],
                eq_hi: d.eq[2],
                filter: d.filter,
                fader: d.fader,
                cue: d.cue,
                level: f32::from_bits(self.status.dj_peak[s].load(Ordering::Acquire)),
            }
        });
        Some(DjView {
            dj: self.dj.view,
            decks,
            crossfader: pv.fader,
            curve_hard: self.dj.curve_hard,
            master: self.dj.master,
            phones_mix: self.dj.phones_mix,
            phones_level: self.dj.phones_level,
            cue_available: self.dj_cue_available(),
        })
    }

    /// Spitzen der Kanäle (A, B), des Masters und des Kopfhörers seit dem letzten Abholen.
    pub fn dj_take_levels(&self) -> [f32; 4] {
        let t = |a: &std::sync::atomic::AtomicU32| f32::from_bits(a.swap(0, Ordering::AcqRel));
        [
            t(&self.status.dj_peak[0]),
            t(&self.status.dj_peak[1]),
            t(&self.status.dj_master_peak),
            t(&self.status.dj_phones_peak),
        ]
    }

    // ---- intern ------------------------------------------------------------------------

    fn dj_view_set(&mut self, on: bool) -> Result<(), String> {
        if self.dj.view == on {
            return Ok(());
        }
        self.dj.view = on;
        self.send(Cmd::Dj(DjCmd::Mode(on)));
        if on {
            for s in 0..PARTY_SIDES {
                self.dj_push_speed(s);
                if self.dj.decks[s].cue_point.is_none() {
                    self.dj.decks[s].cue_point = Some(self.dj_pos(s));
                }
            }
        } else {
            // Zurück nach „Einfach“: Kopfhörer-Cue aus, Scratch und Anschieben los.
            for s in 0..PARTY_SIDES {
                let d = &mut self.dj.decks[s];
                d.cue_held = false;
                d.nudge = 0.0;
                d.scratch = None;
                if d.cue {
                    d.cue = false;
                    self.send(Cmd::Dj(DjCmd::Cue { side: s, on: false }));
                }
                self.send(Cmd::Dj(DjCmd::Scratch {
                    side: s,
                    speed: None,
                }));
            }
        }
        tracing::info!(dj = on, "party: view");
        Ok(())
    }

    fn dj_speed_of(&self, side: usize) -> f64 {
        let d = &self.dj.decks[side];
        1.0 + d.tempo * TEMPO_RANGE + d.nudge * NUDGE_RANGE
    }

    fn dj_push_speed(&mut self, side: usize) {
        let speed = self.dj_speed_of(side);
        self.send(Cmd::Dj(DjCmd::Speed { side, speed }));
    }

    fn dj_pos(&self, side: usize) -> f64 {
        self.status.party_pos[side].load(Ordering::Acquire) as f64 / f64::from(self.rate)
    }

    fn dj_jump(&mut self, side: usize, seconds: f64) {
        let frame = seconds.max(0.0) * f64::from(self.rate);
        self.send(Cmd::Dj(DjCmd::Jump { side, frame }));
    }

    fn dj_cue_point(&self, side: usize) -> f64 {
        self.dj.decks[side].cue_point.unwrap_or(0.0)
    }

    fn dj_cue_down(&mut self, side: usize) -> Result<(), String> {
        if !self.party_loaded_pub(side) {
            return Err("Auf dem Deck liegt kein Titel".into());
        }
        if self.party_side_playing_pub(side) && !self.dj.decks[side].cue_held {
            // Läuft: zurück zum Cue und anhalten.
            self.party_pause(side);
            let c = self.dj_cue_point(side);
            self.dj_jump(side, c);
            return Ok(());
        }
        let pos = self.dj_pos(side);
        let at_cue = self.dj.decks[side]
            .cue_point
            .is_some_and(|c| (c - pos).abs() < 0.02);
        if !at_cue {
            // Stehend an neuer Stelle: Cue hierher setzen.
            self.dj.decks[side].cue_point = Some(pos);
        }
        // Halten spielt ab Cue, Loslassen springt zurück.
        self.dj.decks[side].cue_held = true;
        self.party_play(side);
        Ok(())
    }

    fn dj_info(&self, side: usize) -> Option<&DjTrackInfo> {
        let idx = self.party_view_index(side)?;
        self.dj.decks[side].info.as_ref().filter(|i| i.index == idx)
    }

    fn dj_sync(&mut self, side: usize) -> Result<(), String> {
        let o = 1 - side;
        let (Some(own), Some(other)) = (self.dj_info(side).cloned(), self.dj_info(o).cloned())
        else {
            return Err("Sync braucht auf beiden Decks einen analysierten Titel".into());
        };
        let (Some(bpm), Some(bpm_o)) = (own.bpm, other.bpm) else {
            return Err("Sync braucht das Tempo beider Titel (Bibliothek analysieren)".into());
        };
        let target = bpm_o * self.dj_speed_of(o);
        let Some(ratio) = sync_ratio(bpm, target, TEMPO_RANGE) else {
            let d = &mut self.dj.decks[side];
            d.tempo = if target > bpm { 1.0 } else { -1.0 };
            d.sync = false;
            self.dj_push_speed(side);
            return Err(format!(
                "Tempo zu weit weg ({bpm:.1} gegen {target:.1} BPM): mehr als ±8 %, auch doppelt \
                 oder halb. Der Fader steht am Anschlag."
            ));
        };
        {
            let d = &mut self.dj.decks[side];
            d.tempo = ((ratio - 1.0) / TEMPO_RANGE).clamp(-1.0, 1.0);
            d.nudge = 0.0;
            d.sync = true;
        }
        self.dj_push_speed(side);
        // Takt: auf dieselbe Lage im Schlag wie das andere Deck (kürzester Weg).
        let pos = self.dj_pos(side);
        let pos_o = self.dj_pos(o);
        let ph = beat_phase(pos, bpm, own.beat_offset.unwrap_or(0.0));
        let ph_o = beat_phase(pos_o, bpm_o, other.beat_offset.unwrap_or(0.0));
        let mut d = ph_o - ph;
        d -= d.round();
        let shift = d * 60.0 / bpm;
        if shift.abs() > 1e-4 {
            self.dj_jump(side, (pos + shift).max(0.0));
        }
        tracing::info!(side, ratio, "party: sync");
        Ok(())
    }

    /// Sync hält das Tempo am anderen Deck (dessen Fader kann sich bewegen).
    pub(super) fn dj_tick(&mut self) {
        if !self.dj.view {
            return;
        }
        for s in 0..PARTY_SIDES {
            if !self.dj.decks[s].sync || self.dj.decks[1 - s].sync {
                continue;
            }
            let (Some(b), Some(bo)) = (
                self.dj_info(s).and_then(|i| i.bpm),
                self.dj_info(1 - s).and_then(|i| i.bpm),
            ) else {
                continue;
            };
            let target = bo * self.dj_speed_of(1 - s);
            if let Some(r) = sync_ratio(b, target, TEMPO_RANGE) {
                let t = ((r - 1.0) / TEMPO_RANGE).clamp(-1.0, 1.0);
                if (t - self.dj.decks[s].tempo).abs() > 1e-9 {
                    self.dj.decks[s].tempo = t;
                    self.dj_push_speed(s);
                }
            }
        }
    }

    fn dj_mark_dirty(&mut self, side: usize) {
        if let Some(i) = self.dj_info(side)
            && let Some(h) = i.hash.clone()
        {
            let ch = DjMarksChange {
                hash: h,
                hot: i.hot,
                looped: i.looped,
            };
            self.dj.dirty.retain(|c| c.hash != ch.hash);
            self.dj.dirty.push(ch);
        }
    }

    fn dj_info_mut(&mut self, side: usize) -> Option<&mut DjTrackInfo> {
        let idx = self.party_view_index(side)?;
        let d = &mut self.dj.decks[side];
        // Ohne Analyse: Marken trotzdem halten (nur nicht speicherbar).
        if d.info.as_ref().is_none_or(|i| i.index != idx) {
            d.info = Some(DjTrackInfo {
                index: idx,
                ..Default::default()
            });
        }
        d.info.as_mut()
    }

    fn dj_hot_cue(&mut self, side: usize, slot: usize, clear: bool) -> Result<(), String> {
        if !self.party_loaded_pub(side) {
            return Err("Auf dem Deck liegt kein Titel".into());
        }
        let pos = self.dj_pos(side);
        let Some(info) = self.dj_info_mut(side) else {
            return Err("Auf dem Deck liegt kein Titel".into());
        };
        if clear {
            info.hot[slot] = None;
            self.dj_mark_dirty(side);
            return Ok(());
        }
        match info.hot[slot] {
            Some(t) => self.dj_jump(side, t),
            None => {
                info.hot[slot] = Some(pos);
                self.dj_mark_dirty(side);
            }
        }
        Ok(())
    }

    fn dj_loop_seconds(&self, side: usize) -> Option<(f64, f64)> {
        self.dj.decks[side].loop_beats?;
        self.dj_info(side).and_then(|i| i.looped)
    }

    fn dj_loop(&mut self, side: usize, beats: Option<u32>) -> Result<(), String> {
        let r = f64::from(self.rate);
        let Some(n) = beats else {
            self.dj.decks[side].loop_beats = None;
            self.send(Cmd::Dj(DjCmd::Loop { side, frames: None }));
            return Ok(());
        };
        if !LOOP_BEATS.contains(&n) {
            return Err(format!(
                "Schleife über {n} Schläge gibt es nicht (1, 2, 4, 8, 16)"
            ));
        }
        let Some((bpm, off)) = self
            .dj_info(side)
            .and_then(|i| Some((i.bpm?, i.beat_offset.unwrap_or(0.0))))
        else {
            return Err("Auto-Loop braucht das Tempo des Titels (Bibliothek analysieren)".into());
        };
        let beat = 60.0 / bpm;
        let pos = self.dj_pos(side);
        let start = (off + ((pos - off) / beat).floor() * beat).max(0.0);
        let end = start + f64::from(n) * beat;
        self.dj.decks[side].loop_beats = Some(n);
        if let Some(i) = self.dj_info_mut(side) {
            i.looped = Some((start, end));
        }
        self.dj_mark_dirty(side);
        self.send(Cmd::Dj(DjCmd::Loop {
            side,
            frames: Some((start * r, end * r)),
        }));
        Ok(())
    }

    /// Ein neuer Titel liegt auf dem Deck: Cue an den Anfang, Schleife und Sync aus.
    pub(super) fn dj_on_load(&mut self, side: usize, start_frames: usize) {
        let d = &mut self.dj.decks[side];
        d.cue_point = Some(start_frames as f64 / f64::from(self.rate));
        d.cue_held = false;
        d.sync = false;
        d.loop_beats = None;
        d.info = None;
        self.send(Cmd::Dj(DjCmd::Loop { side, frames: None }));
    }
}

/// Hinweis an den gesperrten Cue-Tasten.
pub const DJ_NO_CUE: &str = "Kopfhörer-Cue braucht einen Vorhör-Ausgang (Einstellungen → \
    Vorhören: Kanal 3/4 oder ein eigenes Gerät)";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_ratio_prefers_double_and_half_time() {
        assert!((sync_ratio(120.0, 126.0, 0.08).unwrap() - 1.05).abs() < 1e-9);
        // 70 gegen 140: doppelt gezählt passt genau.
        assert!((sync_ratio(140.0, 70.0, 0.08).unwrap() - 1.0).abs() < 1e-9);
        assert!((sync_ratio(70.0, 140.0, 0.08).unwrap() - 1.0).abs() < 1e-9);
        assert_eq!(sync_ratio(100.0, 128.0, 0.08), None);
        assert_eq!(sync_ratio(0.0, 128.0, 0.08), None);
    }

    #[test]
    fn beat_phase_wraps() {
        assert!(beat_phase(0.5, 120.0, 0.0).abs() < 1e-9);
        assert!((beat_phase(0.75, 120.0, 0.0) - 0.5).abs() < 1e-9);
        assert!((beat_phase(0.1, 120.0, 0.35) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn commands_parse_from_json() {
        let c: DjCommand =
            serde_json::from_str(r#"{"action":"eq","side":1,"band":"lo","value":-1}"#).unwrap();
        assert_eq!(
            c,
            DjCommand::Eq {
                side: 1,
                band: Band::Lo,
                value: -1.0
            }
        );
        let c: DjCommand =
            serde_json::from_str(r#"{"action":"hot_cue","side":0,"slot":2}"#).unwrap();
        assert_eq!(
            c,
            DjCommand::HotCue {
                side: 0,
                slot: 2,
                clear: false
            }
        );
    }
}
