//! Plugin-Knoten (Paket 31): Instrument und Effekt als Stereo-Knoten im Graphen. Sie reichen
//! die Arbeit an einen [`StereoProcessor`] weiter (CLAP in `woa-plugins`, Attrappen in Tests).
//!
//! Echtzeit: vorab angelegte Listen, keine Sperre, kein Speicher, kein Log im Audio-Thread.

use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use rtrb::Consumer;
use woa_dsp::instrument::NoteEvent;
use woa_dsp::plugin::{BYPASS_PARAM, StereoProcessor, TrackPlugins};
use woa_dsp::{StereoBuffers, StereoNode};
use woa_model::{InsertId, PluginRole, Track};

use crate::midi_clip::{TimedEvent, track_events};

/// Platz für die Events eines Blocks.
const EVENT_CAP: usize = 2048;

/// Ersatz für ein fehlendes Instrument: Stille.
struct Mute;

impl StereoProcessor for Mute {
    fn set_param(&mut self, _id: u32, _value: f64) {}

    fn process(
        &mut self,
        _events: &[NoteEvent],
        _in_left: &[f32],
        _in_right: &[f32],
        out_left: &mut [f32],
        out_right: &mut [f32],
    ) {
        out_left.fill(0.0);
        out_right.fill(0.0);
    }

    fn reset(&mut self) {}
}

/// Instrument einer MIDI-Spur: spielt die Clip-Noten im Takt des Transports und die
/// Live-Noten des Keyboards.
pub struct PluginInstrumentNode {
    processor: Box<dyn StereoProcessor>,
    events: Vec<TimedEvent>,
    live: Option<Consumer<NoteEvent>>,
    pos: u64,
    playing: bool,
    /// Erwartete Position des nächsten Blocks; anders = Sprung (Seek, Loop, Start).
    expected: Option<u64>,
    cursor: usize,
    scratch: Vec<NoteEvent>,
    meter: Option<Arc<AtomicU32>>,
}

impl PluginInstrumentNode {
    /// Neuer Knoten.
    pub fn new(
        processor: Box<dyn StereoProcessor>,
        events: Vec<TimedEvent>,
        live: Option<Consumer<NoteEvent>>,
    ) -> Self {
        Self {
            processor,
            events,
            live,
            pos: 0,
            playing: false,
            expected: None,
            cursor: 0,
            scratch: Vec::with_capacity(EVENT_CAP),
            meter: None,
        }
    }

    /// Meldet den Spitzenpegel jedes Blocks in `meter`.
    pub fn with_meter(mut self, meter: Arc<AtomicU32>) -> Self {
        self.meter = Some(meter);
        self
    }
}

impl StereoNode for PluginInstrumentNode {
    fn prepare(&mut self, _sample_rate: f32) {}

    fn set_param(&mut self, param: u32, value: f32) {
        if param != BYPASS_PARAM {
            self.processor.set_param(param, f64::from(value));
        }
    }

    fn locate(&mut self, position: u64, playing: bool) {
        self.pos = position;
        self.playing = playing;
    }

    fn process(&mut self, b: StereoBuffers<'_>) {
        let len = b.out_left.len().min(b.out_right.len());
        self.scratch.clear();
        if let Some(live) = self.live.as_mut() {
            while let Ok(ev) = live.pop() {
                if self.scratch.len() < EVENT_CAP {
                    self.scratch.push(ev);
                }
            }
        }
        if self.playing {
            let pos = self.pos;
            if self.expected != Some(pos) {
                // Sprung: hängende Noten aus, ab der neuen Stelle weiterlesen.
                if self.expected.is_some() {
                    self.processor.reset();
                }
                self.cursor = self.events.partition_point(|e| e.sample < pos);
            }
            let end = pos + len as u64;
            while let Some(e) = self.events.get(self.cursor) {
                if e.sample >= end {
                    break;
                }
                if self.scratch.len() < EVENT_CAP {
                    let at = (e.sample.saturating_sub(pos) as usize).min(len.saturating_sub(1));
                    self.scratch.push(NoteEvent {
                        offset: at as u32,
                        channel: 0,
                        kind: e.kind,
                    });
                }
                self.cursor += 1;
            }
            self.expected = Some(end);
        } else if self.expected.take().is_some() {
            self.processor.reset();
        }
        // Live-Events stehen vorn mit Versatz 0; die Clip-Events sind nach Zeit geordnet.
        self.processor.process(
            &self.scratch,
            b.in_left,
            b.in_right,
            &mut b.out_left[..len],
            &mut b.out_right[..len],
        );
        if let Some(m) = &self.meter {
            crate::clip::store_peak(m, b.out_left);
        }
    }
}

/// Effekt-Plugin in der Kette einer Spur.
pub struct PluginFxNode {
    processor: Box<dyn StereoProcessor>,
    bypass: bool,
}

impl PluginFxNode {
    /// Neuer Knoten.
    pub fn new(processor: Box<dyn StereoProcessor>, bypass: bool) -> Self {
        Self { processor, bypass }
    }
}

impl StereoNode for PluginFxNode {
    fn prepare(&mut self, _sample_rate: f32) {}

    fn set_param(&mut self, param: u32, value: f32) {
        if param == BYPASS_PARAM {
            let on = value >= 0.5;
            if on != self.bypass {
                self.bypass = on;
                self.processor.reset();
            }
        } else {
            self.processor.set_param(param, f64::from(value));
        }
    }

    fn locate(&mut self, _position: u64, _playing: bool) {}

    fn process(&mut self, b: StereoBuffers<'_>) {
        if self.bypass {
            b.out_left.copy_from_slice(b.in_left);
            b.out_right.copy_from_slice(b.in_right);
        } else {
            self.processor
                .process(&[], b.in_left, b.in_right, b.out_left, b.out_right);
        }
    }
}

/// Die Plugin-Knoten einer Spur, bereit für den Graph-Bau.
pub struct PluginChain {
    /// Instrument-Knoten und Kennung seines Eintrags; ersetzt die Quelle der Spur.
    pub head: Option<(InsertId, Box<dyn StereoNode>)>,
    /// Effekte, erster zuerst.
    pub effects: Vec<(InsertId, Box<dyn StereoNode>)>,
}

impl PluginChain {
    /// Baut die Knoten aus den Plugins der Spur. `None`, wenn die Spur keinen Plugin-Platz hat.
    /// Ein Instrument-Platz ohne geladenes Plugin liefert einen stummen Knoten; ein fehlender
    /// Effekt wird durchgereicht (der Platz bleibt im Projekt).
    pub fn build(
        track: &Track,
        plugins: TrackPlugins,
        tempo_bpm: f64,
        rate: f32,
        live: Option<Consumer<NoteEvent>>,
        meter: Option<Arc<AtomicU32>>,
    ) -> Option<Self> {
        if track.plugins.is_empty() {
            return None;
        }
        let head = track
            .plugins
            .iter()
            .find(|s| s.role == PluginRole::Instrument)
            .map(|slot| {
                let processor = plugins
                    .instrument
                    .unwrap_or_else(|| Box::new(Mute) as Box<dyn StereoProcessor>);
                let events = track_events(&track.clips, tempo_bpm, rate);
                let mut node = PluginInstrumentNode::new(processor, events, live);
                if let Some(m) = meter {
                    node = node.with_meter(m);
                }
                (slot.id, Box::new(node) as Box<dyn StereoNode>)
            });
        let effects = plugins
            .effects
            .into_iter()
            .map(|e| {
                (
                    InsertId(e.id),
                    Box::new(PluginFxNode::new(e.processor, e.bypass)) as Box<dyn StereoNode>,
                )
            })
            .collect();
        Some(Self { head, effects })
    }
}
