//! MIDI-Clips abspielen (Paket 12): ein [`MidiTrackPlayer`] je MIDI-Spur spielt die Noten
//! aller Clips der Spur sample-genau über das Instrument der Spur, solange der Transport
//! läuft, und dazu (auf der scharfen Spur) das Keyboard live.
//!
//! Die Events liegen fertig in Samples sortiert vor (Control-Thread). Der Audio-Thread
//! sucht nur mit Binärsuche, rendert und allokiert nicht.

use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use rtrb::Consumer;
use woa_dsp::instrument::{Instrument, NoteEvent, NoteEventKind};
use woa_dsp::soundfont::{SoundFont, SoundFontPlayer};
use woa_dsp::synth::{PolySynth, Waveform};
use woa_dsp::{Node, ProcessBuffers};
use woa_model::{CC_SUSTAIN, Clip, Track, TrackSound};

use crate::transport::{TransportHandle, beats_to_samples};

/// Stimmen des Spur-Instruments (wie das Live-Instrument aus Paket 11).
pub const TRACK_VOICES: usize = 16;
/// Wellenform des Spur-Instruments.
pub const TRACK_WAVEFORM: Waveform = Waveform::Saw;

/// Neues Instrument einer MIDI-Spur.
pub fn track_instrument() -> PolySynth {
    PolySynth::new(TRACK_VOICES, TRACK_WAVEFORM)
}

/// Instrument einer MIDI-Spur: eingebauter Synth oder GM-SoundFont (Paket J1).
pub enum TrackInstrument {
    Synth(PolySynth),
    SoundFont(Box<SoundFontPlayer>),
}

/// Puffergröße des SoundFont-Players (beliebige Blöcke werden darin gestückelt).
const SOUNDFONT_CHUNK: usize = 512;

impl TrackInstrument {
    /// Baut das Instrument (Control-Thread). Ohne Klang oder ohne `font`: der Synth.
    /// Mit Klang wird das GM-Programm auf dem Kanal des Klangs gesetzt.
    pub fn new(sound: Option<TrackSound>, font: Option<Arc<SoundFont>>, rate: f32) -> Self {
        let (Some(sound), Some(font)) = (sound, font) else {
            return Self::Synth(track_instrument());
        };
        match SoundFontPlayer::new(font, rate.round() as u32, SOUNDFONT_CHUNK) {
            Ok(mut p) => {
                p.handle_event(&NoteEvent {
                    offset: 0,
                    channel: sound.channel(),
                    kind: NoteEventKind::ProgramChange {
                        program: sound.program.min(127),
                    },
                });
                Self::SoundFont(Box::new(p))
            }
            Err(e) => {
                tracing::warn!(error = %e, "soundfont player failed, using the built-in synth");
                Self::Synth(track_instrument())
            }
        }
    }

    /// `true`, wenn die SoundFont spielt.
    pub fn is_soundfont(&self) -> bool {
        matches!(self, Self::SoundFont(_))
    }
}

impl Instrument for TrackInstrument {
    fn prepare(&mut self, sample_rate: f32) {
        match self {
            Self::Synth(s) => s.prepare(sample_rate),
            Self::SoundFont(s) => s.prepare(sample_rate),
        }
    }
    fn set_param(&mut self, param: u32, value: f32) {
        match self {
            Self::Synth(s) => s.set_param(param, value),
            Self::SoundFont(s) => s.set_param(param, value),
        }
    }
    fn handle_event(&mut self, event: &NoteEvent) {
        match self {
            Self::Synth(s) => s.handle_event(event),
            Self::SoundFont(s) => s.handle_event(event),
        }
    }
    fn render(&mut self, out: &mut [f32]) {
        match self {
            Self::Synth(s) => s.render(out),
            Self::SoundFont(s) => s.render(out),
        }
    }
}

/// Ein Event auf der Zeitachse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimedEvent {
    /// Position in Samples.
    pub sample: u64,
    pub kind: NoteEventKind,
}

fn order(k: &NoteEventKind) -> u8 {
    match k {
        NoteEventKind::NoteOff { .. } | NoteEventKind::AllNotesOff => 0,
        NoteEventKind::NoteOn { .. } => 2,
        _ => 1,
    }
}

/// Setzt die Noten der MIDI-Clips einer Spur auf die Zeitachse. Noten, die nach dem
/// Clip-Ende beginnen, fallen weg; längere enden am Clip-Ende. Pedal (CC 64) und
/// All Notes Off (CC 120/123) werden übernommen, ein gedrücktes Pedal endet am Clip-Ende.
pub fn track_events(clips: &[Clip], tempo_bpm: f64, rate: f32) -> Vec<TimedEvent> {
    let to = |b: f64| beats_to_samples(b.max(0.0), tempo_bpm, rate);
    let mut out = Vec::new();
    for c in clips {
        let Some(m) = &c.midi else { continue };
        let start = c.start_beats;
        let end = to(c.end_beats());
        for n in &m.notes {
            if !(n.beat.is_finite() && n.length_beats.is_finite()) || n.beat >= c.length_beats {
                continue;
            }
            let on = to(start + n.beat);
            let off = to(start + (n.beat + n.length_beats).min(c.length_beats)).max(on + 1);
            let note = n.pitch.min(127);
            out.push(TimedEvent {
                sample: on,
                kind: NoteEventKind::NoteOn {
                    note,
                    velocity: n.velocity.clamp(1, 127),
                },
            });
            out.push(TimedEvent {
                sample: off,
                kind: NoteEventKind::NoteOff { note },
            });
        }
        let mut pedal = false;
        for cc in &m.cc {
            if !cc.beat.is_finite() || cc.beat >= c.length_beats {
                continue;
            }
            let kind = match cc.controller {
                CC_SUSTAIN => {
                    pedal = cc.value >= 64;
                    NoteEventKind::Sustain { on: pedal }
                }
                120 | 123 => NoteEventKind::AllNotesOff,
                _ => continue,
            };
            out.push(TimedEvent {
                sample: to(start + cc.beat),
                kind,
            });
        }
        if pedal {
            out.push(TimedEvent {
                sample: end,
                kind: NoteEventKind::Sustain { on: false },
            });
        }
    }
    out.sort_by_key(|e| (e.sample, order(&e.kind)));
    out
}

/// Quelle einer MIDI-Spur: Clip-Noten im Takt des Transports, Live-Events sofort.
pub struct MidiTrackPlayer<I: Instrument = PolySynth> {
    inner: I,
    events: Vec<TimedEvent>,
    transport: TransportHandle,
    live: Option<Consumer<NoteEvent>>,
    /// Erwartete Position des nächsten Blocks; anders = Sprung (Seek, Loop, Start).
    expected: Option<u64>,
    cursor: usize,
    meter: Option<Arc<AtomicU32>>,
    /// MIDI-Kanal aller Events (9 = Drums auf Kanal 10).
    channel: u8,
}

impl<I: Instrument> MidiTrackPlayer<I> {
    pub fn new(
        inner: I,
        events: Vec<TimedEvent>,
        transport: TransportHandle,
        live: Option<Consumer<NoteEvent>>,
    ) -> Self {
        Self {
            inner,
            events,
            transport,
            live,
            expected: None,
            cursor: 0,
            meter: None,
            channel: 0,
        }
    }

    /// Spielt alle Events (Clip und live) auf diesem MIDI-Kanal.
    pub fn on_channel(mut self, channel: u8) -> Self {
        self.channel = channel.min(15);
        self
    }

    pub fn instrument(&self) -> &I {
        &self.inner
    }

    /// Meldet den Spitzenpegel jedes Blocks in `meter`.
    pub fn with_meter(mut self, meter: Arc<AtomicU32>) -> Self {
        self.meter = Some(meter);
        self
    }

    fn silence_all(&mut self) {
        for kind in [
            NoteEventKind::Sustain { on: false },
            NoteEventKind::AllNotesOff,
        ] {
            self.inner.handle_event(&NoteEvent {
                offset: 0,
                channel: self.channel,
                kind,
            });
        }
    }
}

impl<I: Instrument> Node for MidiTrackPlayer<I> {
    fn prepare(&mut self, sample_rate: f32) {
        self.inner.prepare(sample_rate);
    }

    fn set_param(&mut self, param: u32, value: f32) {
        self.inner.set_param(param, value);
    }

    fn process(&mut self, buffers: ProcessBuffers<'_>) {
        let out = buffers.output;
        let len = out.len();
        if let Some(live) = self.live.as_mut() {
            while let Ok(mut ev) = live.pop() {
                ev.channel = self.channel;
                self.inner.handle_event(&ev);
            }
        }
        let mut done = 0usize;
        if self.transport.is_playing() {
            let pos = self.transport.position();
            if self.expected != Some(pos) {
                // Sprung: hängende Noten aus, ab der neuen Stelle weiterlesen.
                if self.expected.is_some() {
                    self.silence_all();
                }
                self.cursor = self.events.partition_point(|e| e.sample < pos);
            }
            let end = pos + len as u64;
            while let Some(e) = self.events.get(self.cursor) {
                if e.sample >= end {
                    break;
                }
                let at = (e.sample.saturating_sub(pos) as usize).min(len);
                if at > done {
                    self.inner.render(&mut out[done..at]);
                    done = at;
                }
                let kind = e.kind;
                self.inner.handle_event(&NoteEvent {
                    offset: 0,
                    channel: self.channel,
                    kind,
                });
                self.cursor += 1;
            }
            self.expected = Some(end);
        } else if self.expected.take().is_some() {
            self.silence_all();
        }
        if done < len {
            self.inner.render(&mut out[done..]);
        }
        if let Some(m) = &self.meter {
            crate::clip::store_peak(m, out);
        }
    }
}

/// Baut die Quelle einer MIDI-Spur aus ihren MIDI-Clips; `None`, wenn die Spur weder
/// MIDI-Clips noch eine Live-Queue hat. Mit GM-Klang spielt die Bank des Klangs bzw. die
/// SoundFont des Prozesses ([`crate::soundfont::soundfont_for`]), fehlt sie, der eingebaute Synth.
pub fn midi_track_source(
    track: &Track,
    tempo_bpm: f64,
    rate: f32,
    transport: &TransportHandle,
    live: Option<Consumer<NoteEvent>>,
) -> Option<MidiTrackPlayer<TrackInstrument>> {
    let font = track.sound.and_then(crate::soundfont::soundfont_for);
    midi_track_source_with(track, tempo_bpm, rate, transport, live, font)
}

/// Wie [`midi_track_source`], mit ausdrücklich gegebener SoundFont.
pub fn midi_track_source_with(
    track: &Track,
    tempo_bpm: f64,
    rate: f32,
    transport: &TransportHandle,
    live: Option<Consumer<NoteEvent>>,
    font: Option<Arc<SoundFont>>,
) -> Option<MidiTrackPlayer<TrackInstrument>> {
    let events = track_events(&track.clips, tempo_bpm, rate);
    if events.is_empty() && live.is_none() {
        return None;
    }
    let inst = TrackInstrument::new(track.sound, font, rate);
    let channel = match (&inst, track.sound) {
        (TrackInstrument::SoundFont(_), Some(s)) => s.channel(),
        _ => 0,
    };
    Some(MidiTrackPlayer::new(inst, events, transport.clone(), live).on_channel(channel))
}

#[cfg(test)]
mod tests {
    use super::*;
    use woa_model::{ClipId, MidiCc, MidiData, MidiNote};

    #[test]
    fn events_are_placed_and_cut_at_clip_end() {
        let midi = MidiData {
            notes: vec![
                MidiNote {
                    beat: 0.5,
                    length_beats: 0.5,
                    pitch: 60,
                    velocity: 100,
                },
                MidiNote {
                    beat: 1.5,
                    length_beats: 4.0,
                    pitch: 62,
                    velocity: 100,
                },
                MidiNote {
                    beat: 2.5,
                    length_beats: 1.0,
                    pitch: 64,
                    velocity: 100,
                },
            ],
            cc: vec![MidiCc {
                beat: 0.0,
                controller: 64,
                value: 127,
            }],
        };
        // 120 bpm at 4 Hz: one beat = 2 samples. Clip at beat 2, 2 beats long.
        let c = Clip::new_midi(ClipId(1), "m", 2.0, 2.0, midi);
        let ev = track_events(&[c], 120.0, 4.0);
        let s: Vec<(u64, u8)> = ev.iter().map(|e| (e.sample, order(&e.kind))).collect();
        assert_eq!(
            s,
            vec![(4, 1), (5, 2), (6, 0), (7, 2), (8, 0), (8, 1)],
            "pedal, C on/off, D on, D cut at clip end, pedal up; E dropped"
        );
    }

    #[test]
    fn without_soundfont_a_gm_sound_keeps_the_synth() {
        let sound = Some(TrackSound {
            program: 33,
            drums: false,
            bank: None,
        });
        assert!(!TrackInstrument::new(sound, None, 48_000.0).is_soundfont());
        assert!(!TrackInstrument::new(None, None, 48_000.0).is_soundfont());
        let bytes = woa_dsp::soundfont::tiny_sf2();
        let font = woa_dsp::soundfont::load_soundfont(&mut bytes.as_slice()).ok();
        assert!(TrackInstrument::new(sound, font.clone(), 48_000.0).is_soundfont());
        assert!(!TrackInstrument::new(None, font, 48_000.0).is_soundfont());
    }
}
