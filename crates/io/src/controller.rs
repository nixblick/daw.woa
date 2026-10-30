//! Controller und MIDI-Learn (Paket 14): Zuordnung `MIDI-Quelle → Ziel`, Lernen,
//! Takeover für Fader und LED-Rückmeldung.
//!
//! Threads: Der MIDI-Thread von `midir` übersetzt Bytes in [`CtrlMsg`] und schiebt sie in
//! eine `rtrb`-Queue ([`ControlSink`]). Die Oberfläche leert die Queue einmal je Bild und
//! wertet sie mit dem [`Mapper`] aus. Der Audio-Thread sieht davon nichts; was er bekommt,
//! kommt wie bei Maus und Tastatur als Mixer-Befehl über die vorhandenen Queues.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use midir::{MidiOutput, MidiOutputConnection};
use rtrb::{Consumer, Producer, RingBuffer};
use serde::{Deserialize, Serialize};

/// Platz in der Controller-Queue. Bei 60 Bildern je Sekunde reicht das für über
/// 60 000 Meldungen je Sekunde.
pub const QUEUE: usize = 1024;

/// Abstand, ab dem ein Fader den Wert der Oberfläche „erreicht“ hat (Takeover), in 0..1.
pub const TAKEOVER: f32 = 2.0 / 127.0;

/// Eine Controller-Meldung, wie sie aus dem MIDI-Thread kommt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CtrlMsg {
    /// MIDI-Kanal 0–15.
    pub channel: u8,
    pub kind: CtrlKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtrlKind {
    /// Control Change (Fader, Drehregler, Taster).
    Cc { cc: u8, value: u8 },
    /// Pad oder Taste gedrückt.
    NoteOn { note: u8, velocity: u8 },
    /// Pad oder Taste losgelassen (auch Note On mit Velocity 0).
    NoteOff { note: u8 },
}

impl CtrlMsg {
    /// Aus einer rohen Nachricht (Status, Daten 1, Daten 2); nur CC und Noten.
    pub fn from_raw((status, a, b): (u8, u8, u8)) -> Option<Self> {
        let channel = status & 0x0F;
        let kind = match status & 0xF0 {
            0x80 => CtrlKind::NoteOff { note: a },
            0x90 if b == 0 => CtrlKind::NoteOff { note: a },
            0x90 => CtrlKind::NoteOn {
                note: a,
                velocity: b,
            },
            0xB0 => CtrlKind::Cc { cc: a, value: b },
            _ => return None,
        };
        Some(Self { channel, kind })
    }

    /// Quelle dieser Meldung.
    pub fn source(&self) -> Source {
        let kind = match self.kind {
            CtrlKind::Cc { cc, .. } => SourceKind::Cc(cc),
            CtrlKind::NoteOn { note, .. } | CtrlKind::NoteOff { note } => SourceKind::Note(note),
        };
        Source {
            channel: self.channel,
            kind,
        }
    }
}

/// Wohin der MIDI-Thread Controller-Meldungen schiebt; `None` = keine Auswertung.
pub type ControlSink = Arc<Mutex<Option<Producer<CtrlMsg>>>>;

/// Leere Senke (keine Auswertung).
pub fn control_sink() -> ControlSink {
    Arc::new(Mutex::new(None))
}

/// Senke mit Queue; der Consumer gehört der Oberfläche.
pub fn control_queue() -> (ControlSink, Consumer<CtrlMsg>) {
    let (p, c) = RingBuffer::new(QUEUE);
    (Arc::new(Mutex::new(Some(p))), c)
}

/// Eine MIDI-Quelle auf dem Gerät: Kanal plus CC- oder Notennummer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Source {
    pub channel: u8,
    pub kind: SourceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Cc(u8),
    Note(u8),
}

impl Source {
    /// Für die Liste: „CC 7 · Kanal 1“, „Note 36 · Kanal 10“.
    pub fn label(&self) -> String {
        let ch = u16::from(self.channel) + 1;
        match self.kind {
            SourceKind::Cc(n) => format!("CC {n} · Kanal {ch}"),
            SourceKind::Note(n) => format!("Note {n} · Kanal {ch}"),
        }
    }
}

/// Pegel im Sendemixer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bus {
    Music,
    Carts,
    Mic,
}

impl Bus {
    pub const ALL: [Bus; 3] = [Bus::Music, Bus::Carts, Bus::Mic];

    pub fn label(&self) -> &'static str {
        match self {
            Bus::Music => "Musik",
            Bus::Carts => "Carts",
            Bus::Mic => "Mikro",
        }
    }
}

/// Layout, auf das ein Taster umschaltet (`set_mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    Produzent,
    Radio,
    /// Hin und her.
    Toggle,
}

/// Was eine Quelle steuert. Spuren zählen ab 0 in der Reihenfolge des Projekts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    TrackGain(usize),
    TrackPan(usize),
    TrackMute(usize),
    TrackSolo(usize),
    /// Pegel im Sendemixer.
    Level(Bus),
    /// Mikrofon auf/zu.
    Mic,
    /// Start bzw. Weiter.
    StartNext,
    Stop,
    /// Cart 0..N-1.
    Cart(usize),
    /// Layout umschalten.
    Mode(Layout),
}

impl Target {
    /// Fader oder Regler (Wert 0..1); sonst ein Taster (löst beim Drücken aus).
    pub fn is_continuous(&self) -> bool {
        matches!(
            self,
            Target::TrackGain(_) | Target::TrackPan(_) | Target::Level(_)
        )
    }

    /// Für die Liste, Spuren und Carts ab 1 gezählt.
    pub fn label(&self) -> String {
        match self {
            Target::TrackGain(t) => format!("Spur {} Fader", t + 1),
            Target::TrackPan(t) => format!("Spur {} Pan", t + 1),
            Target::TrackMute(t) => format!("Spur {} Stumm", t + 1),
            Target::TrackSolo(t) => format!("Spur {} Solo", t + 1),
            Target::Level(b) => format!("Pegel {}", b.label()),
            Target::Mic => "Mikro auf/zu".into(),
            Target::StartNext => "Start/Weiter".into(),
            Target::Stop => "Stopp".into(),
            Target::Cart(c) => format!("Cart {}", c + 1),
            Target::Mode(Layout::Produzent) => "Layout Produzent".into(),
            Target::Mode(Layout::Radio) => "Layout Radio".into(),
            Target::Mode(Layout::Toggle) => "Layout umschalten".into(),
        }
    }
}

/// Eine Zeile der Zuordnung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mapping {
    pub source: Source,
    pub target: Target,
}

/// Was die Oberfläche tun soll.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    /// Fader/Regler auf `value` (0..1) setzen.
    Set { target: Target, value: f32 },
    /// Taster gedrückt.
    Press(Target),
    /// Neue Zuordnung gelernt (schon eingetragen; speichern).
    Learned(Mapping),
}

#[derive(Debug, Default, Clone, Copy)]
struct SourceState {
    /// Letzter Rohwert des Reglers.
    last: Option<u8>,
    /// Fader hat den Wert der Oberfläche erreicht.
    picked: bool,
    /// Zuletzt gesendeter Wert (0..1).
    sent: Option<f32>,
}

/// Zuordnung eines Geräts samt Lernzustand und Takeover.
#[derive(Debug, Default, Clone)]
pub struct Mapper {
    maps: Vec<Mapping>,
    learning: Option<Target>,
    state: HashMap<Source, SourceState>,
}

impl Mapper {
    pub fn new(maps: Vec<Mapping>) -> Self {
        Self {
            maps,
            ..Self::default()
        }
    }

    pub fn mappings(&self) -> &[Mapping] {
        &self.maps
    }

    /// Ersetzt die Zuordnung (anderes Gerät gewählt); Takeover beginnt neu.
    pub fn set_mappings(&mut self, maps: Vec<Mapping>) {
        self.maps = maps;
        self.state.clear();
        self.learning = None;
    }

    /// Nächste Bewegung wird an `target` gebunden.
    pub fn learn(&mut self, target: Target) {
        self.learning = Some(target);
    }

    pub fn cancel_learn(&mut self) {
        self.learning = None;
    }

    pub fn learning(&self) -> Option<Target> {
        self.learning
    }

    /// Entfernt Zeile `i`.
    pub fn remove(&mut self, i: usize) -> Option<Mapping> {
        (i < self.maps.len()).then(|| self.maps.remove(i))
    }

    /// Quelle, die `target` steuert.
    pub fn source_of(&self, target: Target) -> Option<Source> {
        self.maps
            .iter()
            .find(|m| m.target == target)
            .map(|m| m.source)
    }

    /// Eine Meldung auswerten. `current` liefert den Wert, den die Oberfläche gerade für
    /// ein stetiges Ziel zeigt (0..1), `None` = unbekannt (dann kein Takeover).
    pub fn handle(
        &mut self,
        msg: CtrlMsg,
        current: impl Fn(&Target) -> Option<f32>,
    ) -> Option<Action> {
        let source = msg.source();
        if let Some(target) = self.learning {
            return self.try_learn(msg, source, target);
        }
        let target = self.maps.iter().find(|m| m.source == source)?.target;
        let st = self.state.entry(source).or_default();
        match msg.kind {
            CtrlKind::Cc { value, .. } if target.is_continuous() => {
                let v = f32::from(value) / 127.0;
                let cur = current(&target);
                if let (Some(c), Some(sent)) = (cur, st.sent)
                    && (c - sent).abs() > TAKEOVER
                {
                    // Die Oberfläche hat den Wert anders gestellt (Maus, Undo): neu abholen.
                    st.picked = false;
                }
                if !st.picked {
                    st.picked = match cur {
                        None => true,
                        Some(c) => {
                            let near = (v - c).abs() <= TAKEOVER;
                            let crossed = st.last.is_some_and(|l| {
                                let l = f32::from(l) / 127.0;
                                (l - c) * (v - c) <= 0.0
                            });
                            near || crossed
                        }
                    };
                }
                st.last = Some(value);
                if st.picked {
                    st.sent = Some(v);
                    Some(Action::Set { target, value: v })
                } else {
                    None
                }
            }
            CtrlKind::Cc { value, .. } => {
                // Taster als CC: nur die steigende Flanke löst aus.
                let was_down = st.last.is_some_and(|l| l >= 64);
                st.last = Some(value);
                (value >= 64 && !was_down).then_some(Action::Press(target))
            }
            CtrlKind::NoteOn { .. } if !target.is_continuous() => Some(Action::Press(target)),
            _ => None,
        }
    }

    /// Leert die Queue für ein Bild. Mehrere Fader-Werte für dasselbe Ziel werden zum
    /// letzten zusammengefasst (1000 CC/s werden so zu einem Wert je Bild), Taster und
    /// Gelerntes bleiben in ihrer Reihenfolge. Innerhalb des Bilds gilt für den Takeover
    /// der schon gesendete Wert als aktueller Wert der Oberfläche.
    pub fn pump(
        &mut self,
        rx: &mut Consumer<CtrlMsg>,
        current: impl Fn(&Target) -> Option<f32>,
    ) -> Vec<Action> {
        let mut out: Vec<Action> = Vec::new();
        while let Ok(msg) = rx.pop() {
            let overlay = |t: &Target| {
                out.iter()
                    .rev()
                    .find_map(|a| match a {
                        Action::Set { target, value } if target == t => Some(*value),
                        _ => None,
                    })
                    .or_else(|| current(t))
            };
            let Some(action) = self.handle(msg, overlay) else {
                continue;
            };
            if let Action::Set { target, .. } = action
                && let Some(slot) = out
                    .iter_mut()
                    .find(|a| matches!(a, Action::Set { target: t, .. } if *t == target))
            {
                *slot = action;
                continue;
            }
            out.push(action);
        }
        out
    }

    fn try_learn(&mut self, msg: CtrlMsg, source: Source, target: Target) -> Option<Action> {
        let ok = match msg.kind {
            CtrlKind::Cc { .. } => true,
            // Ein Fader lässt sich nur an einen Regler binden, nicht an ein Pad.
            CtrlKind::NoteOn { .. } => !target.is_continuous(),
            CtrlKind::NoteOff { .. } => false,
        };
        if !ok {
            return None;
        }
        self.maps
            .retain(|m| m.source != source && m.target != target);
        let mapping = Mapping { source, target };
        self.maps.push(mapping);
        self.learning = None;
        // Den Fader nach dem Lernen erst abholen lassen.
        self.state.insert(
            source,
            SourceState {
                last: match msg.kind {
                    CtrlKind::Cc { value, .. } => Some(value),
                    _ => None,
                },
                ..SourceState::default()
            },
        );
        Some(Action::Learned(mapping))
    }

    /// LED-Zustand je Pad (Quelle als Note, Ziel ein Taster): `lit` sagt, ob das Ziel
    /// gerade an ist (Cart läuft, Mikro offen, Spur stumm).
    pub fn leds(&self, lit: impl Fn(&Target) -> bool) -> Vec<(Source, bool)> {
        self.maps
            .iter()
            .filter(|m| matches!(m.source.kind, SourceKind::Note(_)) && !m.target.is_continuous())
            .map(|m| (m.source, lit(&m.target)))
            .collect()
    }
}

/// Note-Out an das Gerät für die LEDs der Pads. Sendet nur Änderungen.
pub struct LedOut {
    port: String,
    conn: MidiOutputConnection,
    shown: HashMap<Source, bool>,
}

/// Ausgang mit demselben Namen wie der Eingang (ohne ALSA-Nummern verglichen).
pub fn find_output(input: &str) -> Option<String> {
    let out = MidiOutput::new("woa-list").ok()?;
    let want = crate::midi::stable_name(input);
    out.ports()
        .iter()
        .filter_map(|p| out.port_name(p).ok())
        .filter(|n| !n.starts_with("woa"))
        .find(|n| crate::midi::stable_name(n) == want)
}

impl LedOut {
    /// Öffnet den Ausgang zum Eingang `input`; `None`, wenn das Gerät keinen hat.
    pub fn open(input: &str) -> Option<Self> {
        let name = find_output(input)?;
        let out = MidiOutput::new("woa").ok()?;
        let port = out
            .ports()
            .into_iter()
            .find(|p| out.port_name(p).ok().as_deref() == Some(name.as_str()))?;
        match out.connect(&port, "woa-led") {
            Ok(conn) => {
                tracing::info!(device = %name, "controller led output connected");
                Some(Self {
                    port: name,
                    conn,
                    shown: HashMap::new(),
                })
            }
            Err(e) => {
                tracing::warn!(device = %name, error = %e, "controller led output failed");
                None
            }
        }
    }

    pub fn port(&self) -> &str {
        &self.port
    }

    /// Schickt geänderte LED-Zustände als Note On (127 an, 0 aus).
    pub fn update(&mut self, leds: &[(Source, bool)]) {
        for &(src, on) in leds {
            if self.shown.get(&src) == Some(&on) {
                continue;
            }
            if let Some(msg) = led_message(src, on)
                && let Err(e) = self.conn.send(&msg)
            {
                tracing::warn!(error = %e, "controller led send failed");
                return;
            }
            self.shown.insert(src, on);
        }
    }
}

/// Die Bytes für eine LED: Note On auf Kanal und Note der Quelle.
pub fn led_message(src: Source, on: bool) -> Option<[u8; 3]> {
    match src.kind {
        SourceKind::Note(n) => Some([0x90 | (src.channel & 0x0F), n, if on { 127 } else { 0 }]),
        SourceKind::Cc(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cc(cc: u8, value: u8) -> CtrlMsg {
        CtrlMsg {
            channel: 0,
            kind: CtrlKind::Cc { cc, value },
        }
    }

    fn pad(note: u8) -> CtrlMsg {
        CtrlMsg {
            channel: 9,
            kind: CtrlKind::NoteOn {
                note,
                velocity: 100,
            },
        }
    }

    #[test]
    fn raw_messages() {
        assert_eq!(
            CtrlMsg::from_raw((0xB2, 7, 99)),
            Some(CtrlMsg {
                channel: 2,
                kind: CtrlKind::Cc { cc: 7, value: 99 }
            })
        );
        assert_eq!(
            CtrlMsg::from_raw((0x99, 36, 0)).map(|m| m.kind),
            Some(CtrlKind::NoteOff { note: 36 })
        );
        assert_eq!(CtrlMsg::from_raw((0xE0, 0, 64)), None);
    }

    #[test]
    fn learn_binds_next_move_and_replaces_old() {
        let mut m = Mapper::default();
        assert_eq!(m.handle(cc(7, 10), |_| None), None, "nichts gelernt");
        m.learn(Target::TrackGain(0));
        // Ein Pad passt nicht zu einem Fader: wird ignoriert, Lernen bleibt an.
        assert_eq!(m.handle(pad(36), |_| None), None);
        assert_eq!(m.learning(), Some(Target::TrackGain(0)));
        let got = m.handle(cc(7, 10), |_| None);
        assert!(matches!(got, Some(Action::Learned(_))));
        assert_eq!(m.learning(), None);
        // Dieselbe Quelle neu gelernt: alte Zeile weg.
        m.learn(Target::Level(Bus::Music));
        m.handle(cc(7, 20), |_| None);
        assert_eq!(m.mappings().len(), 1);
        assert_eq!(m.mappings()[0].target, Target::Level(Bus::Music));
        // Dasselbe Ziel mit anderer Quelle: alte Zeile weg.
        m.learn(Target::Level(Bus::Music));
        m.handle(cc(8, 20), |_| None);
        assert_eq!(m.mappings().len(), 1);
        assert_eq!(
            m.source_of(Target::Level(Bus::Music)).map(|s| s.kind),
            Some(SourceKind::Cc(8))
        );
        assert!(m.remove(0).is_some());
        assert!(m.mappings().is_empty());
    }

    #[test]
    fn pads_and_buttons_press_once() {
        let mut m = Mapper::default();
        m.learn(Target::Cart(2));
        m.handle(pad(36), |_| None);
        m.learn(Target::StartNext);
        m.handle(cc(20, 127), |_| None);
        assert_eq!(
            m.handle(pad(36), |_| None),
            Some(Action::Press(Target::Cart(2)))
        );
        let off = CtrlMsg {
            channel: 9,
            kind: CtrlKind::NoteOff { note: 36 },
        };
        assert_eq!(m.handle(off, |_| None), None);
        // Der Taster ist vom Lernen noch unten: erst loslassen.
        assert_eq!(m.handle(cc(20, 0), |_| None), None);
        // CC-Taster: 127 drückt, weiteres 127 nicht, 0 lässt los, 127 drückt wieder.
        assert_eq!(
            m.handle(cc(20, 127), |_| None),
            Some(Action::Press(Target::StartNext))
        );
        assert_eq!(m.handle(cc(20, 127), |_| None), None);
        assert_eq!(m.handle(cc(20, 0), |_| None), None);
        assert_eq!(
            m.handle(cc(20, 127), |_| None),
            Some(Action::Press(Target::StartNext))
        );
        // Andere Kanäle sind andere Quellen.
        let other = CtrlMsg {
            channel: 3,
            kind: CtrlKind::NoteOn {
                note: 36,
                velocity: 1,
            },
        };
        assert_eq!(m.handle(other, |_| None), None);
    }

    fn fader() -> Mapper {
        let mut m = Mapper::default();
        m.learn(Target::TrackGain(1));
        m.handle(cc(7, 0), |_| None);
        m
    }

    fn value(a: Option<Action>) -> Option<f32> {
        match a {
            Some(Action::Set { value, .. }) => Some(value),
            _ => None,
        }
    }

    #[test]
    fn takeover_waits_until_fader_reaches_value() {
        let mut m = fader();
        let ui = 0.5f32;
        // Fader unten, Oberfläche bei 0,5: nichts springt.
        for v in [5u8, 20, 40, 60] {
            assert_eq!(m.handle(cc(7, v), |_| Some(ui)), None, "wert {v}");
        }
        // 60 → 70 überquert 0,5 (63,5): ab hier folgt der Wert.
        assert_eq!(value(m.handle(cc(7, 70), |_| Some(ui))), Some(70.0 / 127.0));
        assert_eq!(
            value(m.handle(cc(7, 30), |_| Some(70.0 / 127.0))),
            Some(30.0 / 127.0)
        );
    }

    #[test]
    fn takeover_from_above_and_by_reaching() {
        let mut m = Mapper::default();
        m.learn(Target::TrackGain(1));
        m.handle(cc(7, 120), |_| None);
        // Fader oben, Oberfläche bei 0,25, langsam herunter: erst bei Erreichen.
        assert_eq!(m.handle(cc(7, 110), |_| Some(0.25)), None);
        assert_eq!(m.handle(cc(7, 50), |_| Some(0.25)), None);
        assert!(
            value(m.handle(cc(7, 33), |_| Some(0.25))).is_some(),
            "33/127 liegt nah an 0,25"
        );
    }

    #[test]
    fn takeover_again_after_mouse_moved_value() {
        let mut m = fader();
        assert!(value(m.handle(cc(7, 1), |_| Some(0.0))).is_some());
        assert!(value(m.handle(cc(7, 10), |_| Some(1.0 / 127.0))).is_some());
        // Maus stellt 0,8: Fader bei 10 darf den Wert nicht zurückreißen.
        assert_eq!(m.handle(cc(7, 12), |_| Some(0.8)), None);
        assert_eq!(m.handle(cc(7, 80), |_| Some(0.8)), None);
        assert!(value(m.handle(cc(7, 110), |_| Some(0.8))).is_some());
    }

    #[test]
    fn unknown_current_value_follows_immediately() {
        let mut m = fader();
        assert_eq!(value(m.handle(cc(7, 127), |_| None)), Some(1.0));
    }

    #[test]
    fn leds_only_for_pads() {
        let mut m = Mapper::default();
        m.learn(Target::Cart(0));
        m.handle(pad(36), |_| None);
        m.learn(Target::Mic);
        m.handle(cc(30, 127), |_| None);
        m.learn(Target::TrackGain(0));
        m.handle(cc(7, 0), |_| None);
        let leds = m.leds(|t| *t == Target::Cart(0));
        assert_eq!(leds.len(), 1);
        assert!(leds[0].1);
        assert_eq!(led_message(leds[0].0, true), Some([0x99, 36, 127]));
        assert_eq!(led_message(leds[0].0, false), Some([0x99, 36, 0]));
    }

    #[test]
    fn mappings_round_trip_json() {
        let maps = vec![
            Mapping {
                source: Source {
                    channel: 0,
                    kind: SourceKind::Cc(7),
                },
                target: Target::Level(Bus::Music),
            },
            Mapping {
                source: Source {
                    channel: 9,
                    kind: SourceKind::Note(36),
                },
                target: Target::Mode(Layout::Toggle),
            },
        ];
        let text = serde_json::to_string(&maps).expect("json");
        let back: Vec<Mapping> = serde_json::from_str(&text).expect("json");
        assert_eq!(back, maps);
    }
}
