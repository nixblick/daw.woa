//! DJ-Paket: Werkzeuge für die DJ-Ansicht der Party (`dj_state`, `dj_deck`, `dj_mixer`).
//!
//! Die Decks laufen nur in der App. Im Live-Modus ([`crate::live`]) reicht der Agent die
//! Kommandos über eine [`DjBridge`] an den GUI-Thread, der sie mit
//! `woa_radio::Controller::dj` ausführt und den Zustand zurückgibt. Ohne laufende App (reines
//! `woa-headless`) antworten die Werkzeuge mit einem Hinweis. Alles ist Live-Steuerung:
//! kein Rückgängig-Schritt, nichts wird in der Sendung gespeichert.

use std::sync::{Arc, mpsc};
use std::time::Duration;

use schemars::JsonSchema;
use serde::Deserialize;
use woa_radio::control::{Band, DjCommand};

/// So lange wartet ein Werkzeug auf die App.
const WAIT: Duration = Duration::from_secs(10);

/// Ein Auftrag an den GUI-Thread: Kommandos nacheinander, dann der Zustand als JSON.
pub struct DjRequest {
    pub cmds: Vec<DjCommand>,
    pub reply: mpsc::Sender<Result<serde_json::Value, String>>,
}

/// Agent-Seite der Verbindung zur App.
#[derive(Clone)]
pub struct DjBridge {
    tx: mpsc::Sender<DjRequest>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

/// GUI-Seite: nimmt die Aufträge an.
pub struct DjDesk {
    rx: mpsc::Receiver<DjRequest>,
}

/// Legt beide Seiten an; `wake` weckt die App (Neuzeichnen).
pub fn bridge(wake: Arc<dyn Fn() + Send + Sync>) -> (DjBridge, DjDesk) {
    let (tx, rx) = mpsc::channel();
    (DjBridge { tx, wake }, DjDesk { rx })
}

impl DjDesk {
    /// Bedient alle wartenden Aufträge mit `f` (Kommandos ausführen, Zustand liefern).
    pub fn serve(
        &self,
        mut f: impl FnMut(&[DjCommand]) -> Result<serde_json::Value, String>,
    ) -> usize {
        let mut n = 0;
        while let Ok(req) = self.rx.try_recv() {
            let _ = req.reply.send(f(&req.cmds));
            n += 1;
        }
        n
    }
}

impl DjBridge {
    /// Schickt die Kommandos an die App und wartet auf den Zustand (blockiert).
    pub fn call(&self, cmds: Vec<DjCommand>) -> Result<serde_json::Value, String> {
        let (reply, got) = mpsc::channel();
        self.tx
            .send(DjRequest { cmds, reply })
            .map_err(|_| "Die WOA-App ist geschlossen".to_string())?;
        (self.wake)();
        match got.recv_timeout(WAIT) {
            Ok(r) => r,
            Err(_) => {
                Err("Die WOA-App antwortet nicht (Fenster minimiert oder beschäftigt)".into())
            }
        }
    }
}

/// Hinweis ohne laufende App.
pub const NO_APP: &str = "Die DJ-Werkzeuge steuern die Party-Decks der laufenden WOA-App \
    (Live-Modus). Bitte die App öffnen und Party einschalten.";

/// Deck A oder B.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq)]
pub enum Deck {
    A,
    B,
}

impl Deck {
    fn side(self) -> usize {
        match self {
            Deck::A => 0,
            Deck::B => 1,
        }
    }
}

/// Aktion an einem Deck.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeckAction {
    /// Playlist-Titel `index` (0-basiert) laden; das Deck muss stehen.
    Load,
    Play,
    Pause,
    /// Cue-Taste antippen: läuft das Deck, zurück zum Cue und anhalten; steht es, Cue hierher.
    Cue,
    /// Sync an (`on` = true, Vorgabe) oder aus: Tempo und Takt ans andere Deck.
    Sync,
    /// Hot Cue `slot` (1–4): leer setzen, belegt anspringen; `on` = false löscht ihn.
    HotCue,
    /// Auto-Loop über `beats` (1, 2, 4, 8, 16); ohne `beats` aus.
    Loop,
    /// Tempo-Fader `value` −1..1 (= −8 … +8 %).
    Tempo,
    /// Keylock an/aus (`on`).
    Keylock,
    /// Springen auf `value` Sekunden.
    Seek,
}

/// Parameter von `dj_deck`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct DjDeckParams {
    pub deck: Deck,
    pub action: DeckAction,
    /// Für `load`: Playlist-Index.
    #[serde(default)]
    pub index: Option<usize>,
    /// Für `hot_cue`: 1–4.
    #[serde(default)]
    pub slot: Option<usize>,
    /// Für `loop`: Schläge.
    #[serde(default)]
    pub beats: Option<u32>,
    /// Für `tempo` (−1..1) und `seek` (Sekunden).
    #[serde(default)]
    pub value: Option<f64>,
    /// Für `sync`, `keylock`, `hot_cue` (false = löschen).
    #[serde(default)]
    pub on: Option<bool>,
}

/// Regler am Mixer.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MixerControl {
    /// Ansicht: 1 = DJ, 0 = Einfach.
    View,
    /// Trim in dB (−12 … +12), je Deck.
    Trim,
    /// EQ −1 (Kill) … 0 … +1 (+6 dB), je Deck.
    EqLo,
    EqMid,
    EqHi,
    /// Filter −1 (Tiefpass) … 0 … +1 (Hochpass), je Deck.
    Filter,
    /// Kanalfader 0..1, je Deck.
    Fader,
    /// Kopfhörer-Cue 1/0, je Deck (braucht einen Vorhör-Ausgang).
    Cue,
    /// Crossfader 0 (A) … 1 (B).
    Crossfader,
    /// Kurve: 1 = hart, 0 = weich.
    Curve,
    /// Master-Pegel der Party 0..1.
    Master,
    /// Kopfhörer-Mix 0 (Cue) … 1 (Master).
    PhonesMix,
    /// Kopfhörer-Pegel 0..1.
    PhonesLevel,
}

/// Parameter von `dj_mixer`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct DjMixerParams {
    pub control: MixerControl,
    pub value: f64,
    /// Für die Kanalregler: A oder B.
    #[serde(default)]
    pub deck: Option<Deck>,
}

/// Übersetzt `dj_deck` in ein Kommando (ohne App prüfbar).
pub fn deck_command(p: &DjDeckParams) -> Result<DjCommand, String> {
    let side = p.deck.side();
    let need = |v: Option<f64>, what: &str| v.ok_or_else(|| format!("`value` fehlt ({what})"));
    Ok(match p.action {
        DeckAction::Load => DjCommand::Load {
            side,
            index: p.index.ok_or("`index` fehlt (Playlist-Index)")?,
        },
        DeckAction::Play => DjCommand::Play { side },
        DeckAction::Pause => DjCommand::Pause { side },
        DeckAction::Cue => DjCommand::CueDown { side },
        DeckAction::Sync => DjCommand::Sync {
            side,
            on: p.on.unwrap_or(true),
        },
        DeckAction::HotCue => {
            let slot = p.slot.ok_or("`slot` fehlt (1–4)")?;
            if !(1..=4).contains(&slot) {
                return Err("`slot` ist 1–4".into());
            }
            DjCommand::HotCue {
                side,
                slot: slot - 1,
                clear: p.on == Some(false),
            }
        }
        DeckAction::Loop => DjCommand::Loop {
            side,
            beats: p.beats,
        },
        DeckAction::Tempo => DjCommand::Tempo {
            side,
            value: need(p.value, "−1..1")?,
        },
        DeckAction::Keylock => DjCommand::Keylock {
            side,
            on: p.on.unwrap_or(true),
        },
        DeckAction::Seek => DjCommand::Seek {
            side,
            seconds: need(p.value, "Sekunden")?,
        },
    })
}

/// Übersetzt `dj_mixer` in Kommandos. `phones` ist der aktuelle Kopfhörer-Stand (Mix, Pegel),
/// weil beide in einem Kommando stehen.
pub fn mixer_command(p: &DjMixerParams, phones: (f32, f32)) -> Result<DjCommand, String> {
    let v = p.value;
    if !v.is_finite() {
        return Err("`value` ist keine Zahl".into());
    }
    let side = || {
        p.deck
            .map(Deck::side)
            .ok_or_else(|| "`deck` fehlt (A oder B)".to_string())
    };
    let f = v as f32;
    Ok(match p.control {
        MixerControl::View => DjCommand::View { dj: v >= 0.5 },
        MixerControl::Trim => DjCommand::Trim {
            side: side()?,
            db: f,
        },
        MixerControl::EqLo => DjCommand::Eq {
            side: side()?,
            band: Band::Lo,
            value: f,
        },
        MixerControl::EqMid => DjCommand::Eq {
            side: side()?,
            band: Band::Mid,
            value: f,
        },
        MixerControl::EqHi => DjCommand::Eq {
            side: side()?,
            band: Band::Hi,
            value: f,
        },
        MixerControl::Filter => DjCommand::Filter {
            side: side()?,
            value: f,
        },
        MixerControl::Fader => DjCommand::Fader {
            side: side()?,
            value: f,
        },
        MixerControl::Cue => DjCommand::Cue {
            side: side()?,
            on: v >= 0.5,
        },
        MixerControl::Crossfader => DjCommand::Crossfader { value: f },
        MixerControl::Curve => DjCommand::Curve { hard: v >= 0.5 },
        MixerControl::Master => DjCommand::Master { value: f },
        MixerControl::PhonesMix => DjCommand::Phones {
            mix: f,
            level: phones.1,
        },
        MixerControl::PhonesLevel => DjCommand::Phones {
            mix: phones.0,
            level: f,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deck(json: &str) -> Result<DjCommand, String> {
        deck_command(&serde_json::from_str(json).unwrap())
    }

    #[test]
    fn deck_actions_map_to_commands() {
        assert_eq!(
            deck(r#"{"deck":"B","action":"load","index":3}"#),
            Ok(DjCommand::Load { side: 1, index: 3 })
        );
        assert_eq!(
            deck(r#"{"deck":"A","action":"hot_cue","slot":4}"#),
            Ok(DjCommand::HotCue {
                side: 0,
                slot: 3,
                clear: false
            })
        );
        assert_eq!(
            deck(r#"{"deck":"A","action":"hot_cue","slot":1,"on":false}"#),
            Ok(DjCommand::HotCue {
                side: 0,
                slot: 0,
                clear: true
            })
        );
        assert!(deck(r#"{"deck":"A","action":"hot_cue","slot":5}"#).is_err());
        assert_eq!(
            deck(r#"{"deck":"A","action":"tempo","value":0.5}"#),
            Ok(DjCommand::Tempo {
                side: 0,
                value: 0.5
            })
        );
        assert!(deck(r#"{"deck":"A","action":"tempo"}"#).is_err());
        assert_eq!(
            deck(r#"{"deck":"B","action":"sync"}"#),
            Ok(DjCommand::Sync { side: 1, on: true })
        );
        assert_eq!(
            deck(r#"{"deck":"B","action":"loop","beats":8}"#),
            Ok(DjCommand::Loop {
                side: 1,
                beats: Some(8)
            })
        );
    }

    #[test]
    fn mixer_controls_map_to_commands() {
        let m = |json: &str| mixer_command(&serde_json::from_str(json).unwrap(), (0.5, 0.8));
        assert_eq!(
            m(r#"{"control":"eq_lo","value":-1,"deck":"A"}"#),
            Ok(DjCommand::Eq {
                side: 0,
                band: Band::Lo,
                value: -1.0
            })
        );
        assert!(m(r#"{"control":"fader","value":1}"#).is_err(), "deck fehlt");
        assert_eq!(
            m(r#"{"control":"crossfader","value":0.25}"#),
            Ok(DjCommand::Crossfader { value: 0.25 })
        );
        assert_eq!(
            m(r#"{"control":"phones_level","value":0.3}"#),
            Ok(DjCommand::Phones {
                mix: 0.5,
                level: 0.3
            })
        );
        assert_eq!(
            m(r#"{"control":"view","value":1}"#),
            Ok(DjCommand::View { dj: true })
        );
    }

    #[test]
    fn bridge_round_trip() {
        let (b, desk) = bridge(Arc::new(|| {}));
        let t = std::thread::spawn(move || b.call(vec![DjCommand::View { dj: true }]));
        let mut served = 0;
        while served == 0 {
            served = desk.serve(|cmds| Ok(serde_json::json!({ "n": cmds.len() })));
            std::thread::yield_now();
        }
        assert_eq!(t.join().unwrap(), Ok(serde_json::json!({ "n": 1 })));
    }
}
