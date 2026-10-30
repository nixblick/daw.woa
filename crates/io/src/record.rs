//! MIDI aufnehmen (Paket 12): Der MIDI-Thread stempelt jedes Event mit der
//! Transportposition aus der [`SampleClock`] und schiebt es in eine `rtrb`-Queue; die
//! Oberfläche leert die Queue ([`MidiTake::drain`]) und macht am Ende Noten daraus
//! ([`MidiTake::finish`]).
//!
//! Der Audio-Thread ist nicht beteiligt: Er meldet nur die Uhr. Gesperrt wird nur im
//! MIDI- und im Control-Thread (Tap tauschen), wie bei der [`crate::midi::EventSink`].

use std::sync::{Arc, Mutex};
use std::time::Instant;

use rtrb::{Consumer, Producer, RingBuffer};
use woa_dsp::clock::SampleClock;
use woa_dsp::instrument::{NoteEvent, NoteEventKind};
use woa_model::{CC_SUSTAIN, MidiCc, MidiData, MidiNote};

/// Plätze in der Aufnahme-Queue; die Oberfläche leert sie jeden Frame.
pub const TAKE_QUEUE: usize = 16_384;

/// Ein Event mit Transportposition in Samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamped {
    pub sample: u64,
    pub event: NoteEvent,
}

/// Was der MIDI-Thread zum Stempeln braucht.
pub struct TapInner {
    clock: Arc<SampleClock>,
    out: Producer<Stamped>,
    dropped: u64,
}

/// Abzweig der Aufnahme im MIDI-Eingang; `None` = keine Aufnahme.
pub type RecordTap = Arc<Mutex<Option<TapInner>>>;

/// Neuer, leerer Abzweig.
pub fn record_tap() -> RecordTap {
    Arc::new(Mutex::new(None))
}

/// Im MIDI-Thread: stempelt `ev` zur Zeit `at` und reicht es weiter. Ohne laufenden
/// Transport wird nichts aufgenommen.
pub fn tap_event(tap: &mut Option<TapInner>, at: Instant, ev: NoteEvent) {
    let Some(t) = tap.as_mut() else { return };
    let Some(sample) = t.clock.sample_at(at) else {
        return;
    };
    if t.out.push(Stamped { sample, event: ev }).is_err() {
        t.dropped += 1;
    }
}

/// Eine laufende MIDI-Aufnahme.
pub struct MidiTake {
    tap: RecordTap,
    clock: Arc<SampleClock>,
    events: Consumer<Stamped>,
    got: Vec<Stamped>,
    start_sample: u64,
}

/// Ergebnis einer Aufnahme.
#[derive(Debug, Clone, PartialEq)]
pub struct TakeResult {
    /// Noten und Pedal, Beats ab Aufnahmebeginn.
    pub midi: MidiData,
    /// Länge der Aufnahme in Beats (bis Stopp).
    pub length_beats: f64,
    /// Rohe Events mit Sample-Stempel (für Tests und Logs).
    pub events: usize,
    /// Wegen voller Queue verlorene Events.
    pub dropped: u64,
}

impl MidiTake {
    /// Startet die Aufnahme ab Transportposition `start_sample` und hängt den Abzweig ein.
    pub fn start(tap: &RecordTap, clock: Arc<SampleClock>, start_sample: u64) -> Self {
        let (out, events) = RingBuffer::new(TAKE_QUEUE);
        match tap.lock() {
            Ok(mut g) => {
                *g = Some(TapInner {
                    clock: Arc::clone(&clock),
                    out,
                    dropped: 0,
                })
            }
            Err(e) => tracing::warn!(error = %e, "midi record tap poisoned"),
        }
        tracing::info!(start_sample, "midi take started");
        Self {
            tap: Arc::clone(tap),
            clock,
            events,
            got: Vec::new(),
            start_sample,
        }
    }

    /// Holt angekommene Events ab; regelmäßig aufrufen (jeder Frame).
    pub fn drain(&mut self) {
        while let Ok(s) = self.events.pop() {
            self.got.push(s);
        }
    }

    /// Bisher aufgenommene Events.
    pub fn len(&self) -> usize {
        self.got.len()
    }

    pub fn is_empty(&self) -> bool {
        self.got.is_empty()
    }

    /// Hängt den Abzweig aus und macht Noten daraus. Offene Noten und Pedal enden beim
    /// Stopp; `tempo_bpm` rechnet Samples in Beats um.
    pub fn finish(mut self, tempo_bpm: f64) -> TakeResult {
        let stop = self.clock.sample_at(Instant::now());
        let rate = self.clock.read().map_or(48_000.0, |r| r.rate);
        let dropped = match self.tap.lock() {
            Ok(mut g) => g.take().map_or(0, |t| t.dropped),
            Err(_) => 0,
        };
        self.drain();
        let stop = stop
            .or_else(|| self.got.last().map(|s| s.sample))
            .unwrap_or(self.start_sample)
            .max(self.start_sample);
        let r = events_to_midi(&self.got, self.start_sample, stop, rate, tempo_bpm);
        tracing::info!(
            events = self.got.len(),
            notes = r.notes.len(),
            dropped,
            "midi take finished"
        );
        let length_beats = to_beats(stop - self.start_sample, rate, tempo_bpm)
            .max(r.end_beats())
            .max(1e-3);
        TakeResult {
            midi: r,
            length_beats,
            events: self.got.len(),
            dropped,
        }
    }
}

impl Drop for MidiTake {
    fn drop(&mut self) {
        if let Ok(mut g) = self.tap.lock() {
            *g = None;
        }
    }
}

fn to_beats(samples: u64, rate: f32, tempo_bpm: f64) -> f64 {
    samples as f64 / f64::from(rate.max(1.0)) * tempo_bpm / 60.0
}

/// Macht aus gestempelten Events Noten und Pedal-Werte, Beats ab `start`.
/// Note an ohne Note aus endet bei `stop`; All Notes Off beendet alle offenen Noten.
pub fn events_to_midi(
    events: &[Stamped],
    start: u64,
    stop: u64,
    rate: f32,
    tempo_bpm: f64,
) -> MidiData {
    let beat = |s: u64| to_beats(s.saturating_sub(start), rate, tempo_bpm);
    let mut open: Vec<(u8, u8, u64, u8)> = Vec::new(); // (Kanal, Note, Start, Velocity)
    let mut out = MidiData::default();
    let close = |out: &mut MidiData, (_, note, from, vel): (u8, u8, u64, u8), to: u64| {
        let len = beat(to.max(from)) - beat(from);
        out.notes.push(MidiNote {
            beat: beat(from),
            length_beats: len.max(1e-4),
            pitch: note.min(127),
            velocity: vel.clamp(1, 127),
        });
    };
    let mut sorted: Vec<Stamped> = events.to_vec();
    sorted.sort_by_key(|s| s.sample);
    for s in &sorted {
        let ch = s.event.channel;
        match s.event.kind {
            NoteEventKind::NoteOn { note, velocity } if velocity > 0 => {
                open.push((ch, note, s.sample, velocity));
            }
            NoteEventKind::NoteOn { note, .. } | NoteEventKind::NoteOff { note } => {
                if let Some(i) = open.iter().position(|o| o.0 == ch && o.1 == note) {
                    let o = open.remove(i);
                    close(&mut out, o, s.sample);
                }
            }
            NoteEventKind::AllNotesOff => {
                for o in std::mem::take(&mut open) {
                    close(&mut out, o, s.sample);
                }
            }
            NoteEventKind::Sustain { on } => out.cc.push(MidiCc {
                beat: beat(s.sample),
                controller: CC_SUSTAIN,
                value: if on { 127 } else { 0 },
            }),
            NoteEventKind::ProgramChange { .. } | NoteEventKind::PitchBend { .. } => {}
        }
    }
    for o in open {
        close(&mut out, o, stop);
    }
    if out.cc.last().is_some_and(|c| c.value >= 64) {
        out.cc.push(MidiCc {
            beat: beat(stop),
            controller: CC_SUSTAIN,
            value: 0,
        });
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(sample: u64, kind: NoteEventKind) -> Stamped {
        Stamped {
            sample,
            event: NoteEvent {
                offset: 0,
                channel: 0,
                kind,
            },
        }
    }

    #[test]
    fn pairs_notes_and_keeps_pedal() {
        // 120 bpm, 48 kHz: 1 Beat = 24 000 Samples; Aufnahme ab Sample 48 000.
        let ev = [
            st(
                48_000,
                NoteEventKind::NoteOn {
                    note: 60,
                    velocity: 100,
                },
            ),
            st(54_000, NoteEventKind::Sustain { on: true }),
            st(
                60_000,
                NoteEventKind::NoteOn {
                    note: 64,
                    velocity: 50,
                },
            ),
            st(60_000, NoteEventKind::NoteOff { note: 60 }),
            st(
                72_000,
                NoteEventKind::NoteOn {
                    note: 64,
                    velocity: 0,
                },
            ),
            st(
                84_000,
                NoteEventKind::NoteOn {
                    note: 67,
                    velocity: 90,
                },
            ),
        ];
        let m = events_to_midi(&ev, 48_000, 96_000, 48_000.0, 120.0);
        assert_eq!(m.notes.len(), 3);
        assert_eq!((m.notes[0].pitch, m.notes[0].beat), (60, 0.0));
        assert!((m.notes[0].length_beats - 0.5).abs() < 1e-9);
        assert_eq!(m.notes[1].velocity, 50);
        assert!((m.notes[1].length_beats - 0.5).abs() < 1e-9, "vel 0 = off");
        assert!((m.notes[2].length_beats - 0.5).abs() < 1e-9, "ends at stop");
        assert_eq!(m.cc.len(), 2, "pedal down plus closing pedal up");
        assert!((m.cc[0].beat - 0.25).abs() < 1e-9);
        assert_eq!(m.cc[1].value, 0);
    }
}
