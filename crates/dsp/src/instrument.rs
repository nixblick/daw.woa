//! Gemeinsame Bausteine der Instrumente: Note-Events, Event-Queue (`rtrb`),
//! ADSR-Hüllkurve und der [`InstrumentNode`], der ein Instrument in den
//! Audio-Graphen hängt.
//!
//! Echtzeitregel: `handle_event` und `render` allokieren nie. Stimmen und
//! Puffer werden im Konstruktor (Control-Thread) vorab angelegt.

use rtrb::{Consumer, Producer, RingBuffer};

use crate::{Node, ProcessBuffers};

/// Art eines Note-Events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteEventKind {
    /// Note an, `velocity` 1..=127 (0 wird wie Note aus behandelt).
    NoteOn { note: u8, velocity: u8 },
    /// Note aus (Release-Phase beginnt).
    NoteOff { note: u8 },
    /// Alle Noten in die Release-Phase.
    AllNotesOff,
    /// Programmwechsel (nur SoundFont-Player, General MIDI).
    ProgramChange { program: u8 },
    /// Haltepedal (CC 64): `true` ab Wert 64.
    Sustain { on: bool },
    /// Tonhöhenrad, -8192..=8191 (0 = Mitte).
    PitchBend { value: i16 },
}

/// Pitchbend-Bereich der eingebauten Instrumente in Halbtönen.
pub const PITCH_BEND_SEMITONES: f64 = 2.0;

/// Frequenzfaktor für einen Pitchbend-Wert (±[`PITCH_BEND_SEMITONES`]).
#[inline]
pub fn bend_factor(value: i16) -> f64 {
    let semis = f64::from(value) / 8192.0 * PITCH_BEND_SEMITONES;
    (semis / 12.0).exp2()
}

/// Ein Event mit Sample-Versatz innerhalb des Blocks, in dem es ankommt.
///
/// `offset` größer als der Block wird ans Blockende geklemmt (gilt dann ab dem
/// nächsten Block).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteEvent {
    pub offset: u32,
    pub channel: u8,
    pub kind: NoteEventKind,
}

impl NoteEvent {
    pub fn note_on(note: u8, velocity: u8) -> Self {
        Self {
            offset: 0,
            channel: 0,
            kind: NoteEventKind::NoteOn { note, velocity },
        }
    }

    pub fn note_off(note: u8) -> Self {
        Self {
            offset: 0,
            channel: 0,
            kind: NoteEventKind::NoteOff { note },
        }
    }

    /// Setzt den Sample-Versatz im Block.
    pub fn at(mut self, offset: u32) -> Self {
        self.offset = offset;
        self
    }

    /// Setzt den MIDI-Kanal (0..=15).
    pub fn on_channel(mut self, channel: u8) -> Self {
        self.channel = channel;
        self
    }
}

/// Lock-free Queue für Note-Events (Control-/MIDI-Thread -> Audio-Thread).
pub fn note_queue(capacity: usize) -> (Producer<NoteEvent>, Consumer<NoteEvent>) {
    RingBuffer::new(capacity.max(1))
}

/// Ein Klangerzeuger, der Note-Events versteht.
pub trait Instrument: Send {
    /// Control-Thread, vor dem ersten `render`.
    fn prepare(&mut self, sample_rate: f32);
    /// Parameter setzen; echtzeitsicher.
    fn set_param(&mut self, param: u32, value: f32);
    /// Ein Event anwenden; echtzeitsicher.
    fn handle_event(&mut self, event: &NoteEvent);
    /// Überschreibt `out` mit dem nächsten Abschnitt. Darf nicht allokieren.
    fn render(&mut self, out: &mut [f32]);
}

/// Hängt ein [`Instrument`] als Quelle in den Graphen und liest dabei die
/// Event-Queue sample-genau aus.
pub struct InstrumentNode<I: Instrument> {
    inner: I,
    events: Consumer<NoteEvent>,
}

impl<I: Instrument> InstrumentNode<I> {
    pub fn new(inner: I, events: Consumer<NoteEvent>) -> Self {
        Self { inner, events }
    }

    pub fn instrument(&self) -> &I {
        &self.inner
    }
}

impl<I: Instrument> Node for InstrumentNode<I> {
    fn prepare(&mut self, sample_rate: f32) {
        self.inner.prepare(sample_rate);
    }

    fn set_param(&mut self, param: u32, value: f32) {
        self.inner.set_param(param, value);
    }

    fn process(&mut self, buffers: ProcessBuffers<'_>) {
        let out = buffers.output;
        let len = out.len();
        let mut pos = 0usize;
        while let Ok(ev) = self.events.peek() {
            let at = (ev.offset as usize).min(len);
            if at > pos {
                self.inner.render(&mut out[pos..at]);
                pos = at;
            }
            if let Ok(ev) = self.events.pop() {
                self.inner.handle_event(&ev);
            }
        }
        if pos < len {
            self.inner.render(&mut out[pos..]);
        }
    }
}

/// Hüllkurven-Einstellungen in Sekunden bzw. Pegel (Sustain 0..=1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdsrParams {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

impl Default for AdsrParams {
    fn default() -> Self {
        Self {
            attack: 0.005,
            decay: 0.1,
            sustain: 0.8,
            release: 0.2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdsrStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Lineare ADSR-Hüllkurve, je Stimme.
#[derive(Debug, Clone)]
pub struct Adsr {
    stage: AdsrStage,
    level: f32,
    attack_inc: f32,
    decay_dec: f32,
    sustain: f32,
    release_samples: f32,
    release_dec: f32,
}

impl Default for Adsr {
    fn default() -> Self {
        Self {
            stage: AdsrStage::Idle,
            level: 0.0,
            attack_inc: 1.0,
            decay_dec: 1.0,
            sustain: 1.0,
            release_samples: 1.0,
            release_dec: 1.0,
        }
    }
}

fn samples(seconds: f32, sample_rate: f32) -> f32 {
    (seconds.max(0.0) * sample_rate).max(1.0)
}

impl Adsr {
    /// Startet die Hüllkurve am aktuellen Pegel (kein Sprung bei Retrigger).
    pub fn trigger(&mut self, p: &AdsrParams, sample_rate: f32) {
        self.sustain = p.sustain.clamp(0.0, 1.0);
        self.attack_inc = 1.0 / samples(p.attack, sample_rate);
        self.decay_dec = (1.0 - self.sustain) / samples(p.decay, sample_rate);
        self.release_samples = samples(p.release, sample_rate);
        self.stage = AdsrStage::Attack;
    }

    /// Wechselt in die Release-Phase.
    pub fn release(&mut self) {
        if self.stage != AdsrStage::Idle {
            self.release_dec = self.level / self.release_samples;
            self.stage = AdsrStage::Release;
        }
    }

    pub fn stage(&self) -> AdsrStage {
        self.stage
    }

    pub fn level(&self) -> f32 {
        self.level
    }

    pub fn is_active(&self) -> bool {
        self.stage != AdsrStage::Idle
    }

    /// Sofort stumm (nur für gestohlene Stimmen vor dem Neustart).
    pub fn kill(&mut self) {
        self.stage = AdsrStage::Idle;
        self.level = 0.0;
    }

    #[inline]
    pub fn next_value(&mut self) -> f32 {
        match self.stage {
            AdsrStage::Idle => {}
            AdsrStage::Attack => {
                self.level += self.attack_inc;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = AdsrStage::Decay;
                }
            }
            AdsrStage::Decay => {
                self.level -= self.decay_dec;
                if self.level <= self.sustain {
                    self.level = self.sustain;
                    self.stage = AdsrStage::Sustain;
                }
            }
            AdsrStage::Sustain => {}
            AdsrStage::Release => {
                self.level -= self.release_dec;
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.stage = AdsrStage::Idle;
                }
            }
        }
        self.level
    }
}

/// MIDI-Note in Hz (A4 = 69 = 440 Hz).
pub fn note_to_hz(note: u8) -> f64 {
    440.0 * 2f64.powf((f64::from(note) - 69.0) / 12.0)
}

/// Gemeinsame Stimmen-Verwaltung: wählt eine Stimme für eine neue Note.
///
/// Reihenfolge: dieselbe Note neu anschlagen, sonst freie Stimme, sonst die
/// leiseste Stimme in der Release-Phase, sonst die älteste.
pub(crate) fn pick_voice<V>(
    voices: &mut [V],
    note: u8,
    info: impl Fn(&V) -> (bool, u8, AdsrStage, f32, u64),
) -> Option<&mut V> {
    let mut same = None;
    let mut free = None;
    let mut quiet: Option<(usize, f32)> = None;
    let mut oldest: Option<(usize, u64)> = None;
    for (i, v) in voices.iter().enumerate() {
        let (active, n, stage, level, age) = info(v);
        if !active {
            if free.is_none() {
                free = Some(i);
            }
            continue;
        }
        if n == note && same.is_none() {
            same = Some(i);
        }
        if stage == AdsrStage::Release && quiet.is_none_or(|(_, l)| level < l) {
            quiet = Some((i, level));
        }
        if oldest.is_none_or(|(_, a)| age < a) {
            oldest = Some((i, age));
        }
    }
    let idx = same
        .or(free)
        .or(quiet.map(|(i, _)| i))
        .or(oldest.map(|(i, _)| i))?;
    voices.get_mut(idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adsr_shape() {
        let p = AdsrParams {
            attack: 0.01,
            decay: 0.01,
            sustain: 0.5,
            release: 0.01,
        };
        let mut e = Adsr::default();
        e.trigger(&p, 1000.0); // 10 samples each
        let mut peak = 0.0f32;
        for _ in 0..30 {
            peak = peak.max(e.next_value());
        }
        assert_eq!(peak, 1.0);
        assert_eq!(e.stage(), AdsrStage::Sustain);
        assert!((e.level() - 0.5).abs() < 1e-6);
        e.release();
        for _ in 0..11 {
            e.next_value();
        }
        assert!(!e.is_active());
    }

    #[test]
    fn note_to_hz_a4_and_octave() {
        assert!((note_to_hz(69) - 440.0).abs() < 1e-9);
        assert!((note_to_hz(81) - 880.0).abs() < 1e-9);
    }
}
