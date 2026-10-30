//! MIDI-Eingang (Paket 11): Eingänge auflisten, verbinden, nach dem Abziehen wieder
//! verbinden, Bytes in [`NoteEvent`]s übersetzen und in die `rtrb`-Queue der scharfen
//! MIDI-Spur schieben.
//!
//! Threads: `midir` ruft den Rückruf in seinem eigenen Thread auf (nicht im Audio-Thread).
//! Dort sitzt der Parser und der Producer der Queue. Der Producer liegt hinter einem
//! `Mutex`, weil die Oberfläche ihn beim Graphentausch ersetzt; gesperrt wird nur im
//! MIDI- und im Control-Thread, der Audio-Thread liest lock-free aus dem Consumer.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use midir::{Ignore, MidiInput, MidiInputConnection};
use rtrb::Producer;
use woa_dsp::instrument::{NoteEvent, NoteEventKind};

/// Abstand der Geräteabfrage (Hotplug).
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Wohin die Events gehen; `None` = keine scharfe MIDI-Spur, Events werden verworfen.
pub type EventSink = Arc<Mutex<Option<Producer<NoteEvent>>>>;

/// Neue, leere Senke.
pub fn event_sink() -> EventSink {
    Arc::new(Mutex::new(None))
}

/// Übersetzt einen MIDI-Bytestrom in [`NoteEvent`]s. Versteht Running Status,
/// überspringt SysEx und lässt Echtzeit-Bytes (Clock, Active Sensing) durch, ohne den
/// laufenden Status zu stören.
#[derive(Debug, Default, Clone)]
pub struct MidiParser {
    status: Option<u8>,
    data: [u8; 2],
    have: usize,
    sysex: bool,
}

fn data_len(status: u8) -> usize {
    match status & 0xF0 {
        0xC0 | 0xD0 => 1,
        _ => 2,
    }
}

impl MidiParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ein Byte; liefert ein Event, sobald eine Nachricht vollständig ist.
    pub fn feed(&mut self, byte: u8) -> Option<NoteEvent> {
        if byte >= 0xF8 {
            return None; // Echtzeit
        }
        if byte & 0x80 != 0 {
            self.have = 0;
            if byte >= 0xF0 {
                // System Common beendet den Running Status.
                self.status = None;
                self.sysex = byte == 0xF0;
            } else {
                self.status = Some(byte);
                self.sysex = false;
            }
            return None;
        }
        if self.sysex {
            return None;
        }
        let status = self.status?;
        self.data[self.have] = byte;
        self.have += 1;
        if self.have < data_len(status) {
            return None;
        }
        self.have = 0;
        let channel = status & 0x0F;
        let (a, b) = (self.data[0], self.data[1]);
        let kind = match status & 0xF0 {
            0x80 => NoteEventKind::NoteOff { note: a },
            0x90 if b == 0 => NoteEventKind::NoteOff { note: a },
            0x90 => NoteEventKind::NoteOn {
                note: a,
                velocity: b,
            },
            0xB0 => match a {
                64 => NoteEventKind::Sustain { on: b >= 64 },
                120 | 123 => NoteEventKind::AllNotesOff,
                _ => return None,
            },
            0xC0 => NoteEventKind::ProgramChange { program: a },
            0xE0 => NoteEventKind::PitchBend {
                value: ((i16::from(b) << 7) | i16::from(a)) - 8192,
            },
            _ => return None,
        };
        Some(NoteEvent {
            offset: 0,
            channel,
            kind,
        })
    }

    /// Alle Bytes einer Nachricht (oder eines Stroms).
    pub fn feed_all(&mut self, bytes: &[u8], mut out: impl FnMut(NoteEvent)) {
        for &b in bytes {
            if let Some(ev) = self.feed(b) {
                out(ev);
            }
        }
    }
}

/// Zähler für die Oberfläche (Aktivitätslampe) und für Logs.
#[derive(Debug, Default)]
pub struct MidiCounters {
    /// Empfangene Events.
    pub events: AtomicU64,
    /// Verworfen, weil die Queue voll war.
    pub dropped: AtomicU64,
}

/// Name ohne die ALSA-Nummern am Ende ("Gerät:Port 20:0" -> "Gerät:Port"), damit ein
/// wieder eingestecktes Gerät mit neuer Client-Nummer erkannt wird.
pub fn stable_name(name: &str) -> &str {
    if let Some((head, tail)) = name.rsplit_once(' ')
        && let Some((c, p)) = tail.split_once(':')
        && !c.is_empty()
        && !p.is_empty()
        && c.bytes().all(|b| b.is_ascii_digit())
        && p.bytes().all(|b| b.is_ascii_digit())
    {
        return head;
    }
    name
}

/// Listet die MIDI-Eingänge (ohne WOAs eigene Ports).
pub fn list_inputs() -> Result<Vec<String>, String> {
    let input = MidiInput::new("woa-list").map_err(|e| e.to_string())?;
    Ok(input
        .ports()
        .iter()
        .filter_map(|p| input.port_name(p).ok())
        .filter(|n| !n.starts_with("woa"))
        .collect())
}

/// Zustand für die Anzeige.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidiStatus {
    /// Kein Gerät gewählt.
    Off,
    /// Verbunden mit diesem Port.
    Connected(String),
    /// Gewählt, aber nicht da (abgezogen); wird alle 2 s neu gesucht.
    Waiting(String),
}

/// Der eine MIDI-Eingang der App. `poll` regelmäßig aus dem Control-Thread aufrufen.
pub struct MidiIn {
    wanted: Option<String>,
    conn: Option<(String, MidiInputConnection<()>)>,
    sink: EventSink,
    counters: Arc<MidiCounters>,
    last_poll: Option<Instant>,
    ports: Vec<String>,
    /// Letzter geloggter Fehler, damit nicht alle 2 s dasselbe im Log steht.
    last_error: Option<String>,
}

impl MidiIn {
    pub fn new(sink: EventSink) -> Self {
        Self {
            wanted: None,
            conn: None,
            sink,
            counters: Arc::new(MidiCounters::default()),
            last_poll: None,
            ports: Vec::new(),
            last_error: None,
        }
    }

    pub fn counters(&self) -> Arc<MidiCounters> {
        Arc::clone(&self.counters)
    }

    /// Zuletzt gesehene Eingänge.
    pub fn ports(&self) -> &[String] {
        &self.ports
    }

    pub fn wanted(&self) -> Option<&str> {
        self.wanted.as_deref()
    }

    pub fn status(&self) -> MidiStatus {
        match (&self.conn, &self.wanted) {
            (Some((n, _)), _) => MidiStatus::Connected(n.clone()),
            (None, Some(w)) => MidiStatus::Waiting(w.clone()),
            (None, None) => MidiStatus::Off,
        }
    }

    /// Wählt ein Gerät (`None` = aus) und verbindet sofort.
    pub fn select(&mut self, name: Option<String>) {
        if self.wanted == name {
            return;
        }
        tracing::info!(device = ?name, "midi input selected");
        self.disconnect("selection changed");
        self.wanted = name;
        self.last_error = None;
        self.poll_now();
    }

    /// Fragt höchstens alle [`POLL_INTERVAL`] die Geräte ab.
    pub fn poll(&mut self, now: Instant) {
        if self
            .last_poll
            .is_some_and(|t| now.duration_since(t) < POLL_INTERVAL)
        {
            return;
        }
        self.last_poll = Some(now);
        self.poll_now();
    }

    /// Geräteabfrage jetzt: Abziehen erkennen, gewähltes Gerät verbinden.
    pub fn poll_now(&mut self) {
        self.last_poll = Some(Instant::now());
        match list_inputs() {
            Ok(p) => self.ports = p,
            Err(e) => {
                self.log_error("midi port listing failed", &e);
                self.ports.clear();
            }
        }
        if let Some((name, _)) = &self.conn
            && !self.ports.iter().any(|p| p == name)
        {
            let name = name.clone();
            self.disconnect("device gone");
            tracing::warn!(device = %name, "midi input lost, waiting for it to return");
        }
        if self.conn.is_none()
            && let Some(want) = self.wanted.clone()
            && let Some(port) = self.find(&want)
        {
            self.connect(&port);
        }
    }

    fn find(&self, want: &str) -> Option<String> {
        self.ports
            .iter()
            .find(|p| p.as_str() == want)
            .or_else(|| {
                self.ports
                    .iter()
                    .find(|p| stable_name(p) == stable_name(want))
            })
            .cloned()
    }

    fn log_error(&mut self, what: &str, e: &str) {
        if self.last_error.as_deref() != Some(e) {
            tracing::warn!(error = %e, "{what}");
            self.last_error = Some(e.to_string());
        }
    }

    fn connect(&mut self, name: &str) {
        let mut input = match MidiInput::new("woa") {
            Ok(i) => i,
            Err(e) => return self.log_error("midi init failed", &e.to_string()),
        };
        input.ignore(Ignore::All);
        let Some(port) = input
            .ports()
            .into_iter()
            .find(|p| input.port_name(p).ok().as_deref() == Some(name))
        else {
            return;
        };
        let sink = Arc::clone(&self.sink);
        let counters = Arc::clone(&self.counters);
        let mut parser = MidiParser::new();
        let callback = move |_stamp: u64, bytes: &[u8], _: &mut ()| {
            let Ok(mut guard) = sink.lock() else {
                return;
            };
            parser.feed_all(bytes, |ev| {
                counters.events.fetch_add(1, Ordering::Relaxed);
                let pushed = guard.as_mut().is_some_and(|p| p.push(ev).is_ok());
                if !pushed {
                    counters.dropped.fetch_add(1, Ordering::Relaxed);
                }
            });
        };
        match input.connect(&port, "woa-in", callback, ()) {
            Ok(c) => {
                tracing::info!(device = %name, "midi input connected");
                self.last_error = None;
                self.conn = Some((name.to_string(), c));
            }
            Err(e) => self.log_error("midi connect failed", &e.to_string()),
        }
    }

    fn disconnect(&mut self, why: &str) {
        if let Some((name, c)) = self.conn.take() {
            c.close();
            tracing::info!(device = %name, reason = why, "midi input disconnected");
            release_all(&self.sink);
        }
    }
}

impl Drop for MidiIn {
    fn drop(&mut self) {
        self.disconnect("shutdown");
    }
}

/// Pedal los und alle Noten aus, damit nach dem Abziehen nichts hängt.
pub fn release_all(sink: &EventSink) {
    if let Ok(mut g) = sink.lock()
        && let Some(p) = g.as_mut()
    {
        for kind in [
            NoteEventKind::Sustain { on: false },
            NoteEventKind::AllNotesOff,
        ] {
            let _ = p.push(NoteEvent {
                offset: 0,
                channel: 0,
                kind,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(bytes: &[u8]) -> Vec<NoteEventKind> {
        let mut p = MidiParser::new();
        let mut v = Vec::new();
        p.feed_all(bytes, |e| v.push(e.kind));
        v
    }

    #[test]
    fn note_on_off_and_velocity() {
        assert_eq!(
            parse(&[0x90, 60, 100, 0x80, 60, 0]),
            vec![
                NoteEventKind::NoteOn {
                    note: 60,
                    velocity: 100
                },
                NoteEventKind::NoteOff { note: 60 }
            ]
        );
    }

    #[test]
    fn note_on_velocity_zero_is_off() {
        assert_eq!(
            parse(&[0x90, 64, 0]),
            vec![NoteEventKind::NoteOff { note: 64 }]
        );
    }

    #[test]
    fn running_status() {
        assert_eq!(
            parse(&[0x90, 60, 90, 62, 80, 60, 0]),
            vec![
                NoteEventKind::NoteOn {
                    note: 60,
                    velocity: 90
                },
                NoteEventKind::NoteOn {
                    note: 62,
                    velocity: 80
                },
                NoteEventKind::NoteOff { note: 60 }
            ]
        );
    }

    #[test]
    fn realtime_bytes_keep_running_status() {
        assert_eq!(
            parse(&[0x90, 60, 0xF8, 90, 0xFE, 62, 80]),
            vec![
                NoteEventKind::NoteOn {
                    note: 60,
                    velocity: 90
                },
                NoteEventKind::NoteOn {
                    note: 62,
                    velocity: 80
                }
            ]
        );
    }

    #[test]
    fn sustain_pitchbend_program() {
        assert_eq!(
            parse(&[
                0xB0, 64, 127, 0xB0, 64, 0, 0xE0, 0, 0x40, 0xE0, 0x7F, 0x7F, 0xE0, 0, 0, 0xC0, 5
            ]),
            vec![
                NoteEventKind::Sustain { on: true },
                NoteEventKind::Sustain { on: false },
                NoteEventKind::PitchBend { value: 0 },
                NoteEventKind::PitchBend { value: 8191 },
                NoteEventKind::PitchBend { value: -8192 },
                NoteEventKind::ProgramChange { program: 5 },
            ]
        );
    }

    #[test]
    fn channel_and_other_cc_ignored() {
        let mut p = MidiParser::new();
        let mut v = Vec::new();
        p.feed_all(&[0x93, 60, 1, 0xB3, 7, 100, 0xD3, 50], |e| v.push(e));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].channel, 3);
    }

    #[test]
    fn sysex_skipped() {
        assert_eq!(
            parse(&[0xF0, 0x43, 0x10, 0x4C, 0xF7, 0x90, 60, 1]),
            vec![NoteEventKind::NoteOn {
                note: 60,
                velocity: 1
            }]
        );
    }

    #[test]
    fn stable_names() {
        assert_eq!(
            stable_name("Digital Piano:Digital Piano MIDI 1 20:0"),
            "Digital Piano:Digital Piano MIDI 1"
        );
        assert_eq!(stable_name("USB Keyboard"), "USB Keyboard");
    }
}
