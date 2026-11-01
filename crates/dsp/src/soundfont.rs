//! SoundFont-Player (SF2, General MIDI) auf Basis von `rustysynth` (MIT).
//!
//! Die SoundFont wird im Control-Thread geladen ([`load_soundfont`]); der
//! Synthesizer samt Stereo-Zwischenpuffer wird in [`SoundFontPlayer::new`]
//! bzw. `prepare` angelegt. `render` mischt auf Mono und allokiert nicht.

use std::io::Read;
use std::sync::Arc;

pub use rustysynth::SoundFont;
use rustysynth::{Synthesizer, SynthesizerSettings};

use crate::instrument::{Instrument, NoteEvent, NoteEventKind};
use crate::{DEFAULT_RAMP_SECONDS, Smoother};

/// Parameter-IDs des SoundFont-Players.
pub mod soundfont_params {
    pub const GAIN: u32 = 5;
}

/// Fehler beim Laden oder Anlegen.
#[derive(Debug)]
pub enum SoundFontPlayerError {
    Load(rustysynth::SoundFontError),
    Synth(rustysynth::SynthesizerError),
}

impl std::fmt::Display for SoundFontPlayerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Load(e) => write!(f, "soundfont load failed: {e:?}"),
            Self::Synth(e) => write!(f, "synthesizer setup failed: {e:?}"),
        }
    }
}

impl std::error::Error for SoundFontPlayerError {}

/// Lädt eine SF2-Datei (Control-Thread, macht I/O).
pub fn load_soundfont<R: Read>(reader: &mut R) -> Result<Arc<SoundFont>, SoundFontPlayerError> {
    SoundFont::new(reader)
        .map(Arc::new)
        .map_err(SoundFontPlayerError::Load)
}

/// General-MIDI-Player als Instrument.
pub struct SoundFontPlayer {
    font: Arc<SoundFont>,
    synth: Synthesizer,
    sample_rate: i32,
    left: Box<[f32]>,
    right: Box<[f32]>,
    gain: Smoother,
}

fn make_synth(
    font: &Arc<SoundFont>,
    sample_rate: i32,
) -> Result<Synthesizer, SoundFontPlayerError> {
    let settings = SynthesizerSettings::new(sample_rate);
    Synthesizer::new(font, &settings).map_err(SoundFontPlayerError::Synth)
}

impl SoundFontPlayer {
    /// `chunk` ist die Größe des internen Stereo-Puffers (beliebige
    /// Blocklängen werden in Stücken dieser Größe gerechnet).
    pub fn new(
        font: Arc<SoundFont>,
        sample_rate: u32,
        chunk: usize,
    ) -> Result<Self, SoundFontPlayerError> {
        let sr = i32::try_from(sample_rate).unwrap_or(48_000);
        let synth = make_synth(&font, sr)?;
        let chunk = chunk.max(1);
        let mut gain = Smoother::new(1.0, DEFAULT_RAMP_SECONDS);
        gain.prepare(sr as f32);
        Ok(Self {
            font,
            synth,
            sample_rate: sr,
            left: vec![0.0; chunk].into_boxed_slice(),
            right: vec![0.0; chunk].into_boxed_slice(),
            gain,
        })
    }
}

impl Instrument for SoundFontPlayer {
    fn prepare(&mut self, sample_rate: f32) {
        // Control-Thread: neue Abtastrate = neuer Synthesizer. Schlägt das
        // fehl, bleibt der alte (Tonhöhe dann falsch, aber kein Absturz).
        let sr = sample_rate.round() as i32;
        if sr != self.sample_rate
            && let Ok(s) = make_synth(&self.font, sr)
        {
            self.synth = s;
            self.sample_rate = sr;
        }
        self.gain.prepare(sample_rate);
    }

    fn set_param(&mut self, param: u32, value: f32) {
        if param == soundfont_params::GAIN {
            self.gain.set_target(value);
        }
    }

    fn handle_event(&mut self, event: &NoteEvent) {
        let ch = i32::from(event.channel.min(15));
        match event.kind {
            NoteEventKind::NoteOn { note, velocity } => {
                self.synth
                    .note_on(ch, i32::from(note), i32::from(velocity.min(127)))
            }
            NoteEventKind::NoteOff { note } => self.synth.note_off(ch, i32::from(note)),
            NoteEventKind::AllNotesOff => self.synth.note_off_all_channel(ch, false),
            NoteEventKind::ProgramChange { program } => {
                self.synth
                    .process_midi_message(ch, 0xC0, i32::from(program.min(127)), 0)
            }
            NoteEventKind::Sustain { on } => {
                self.synth
                    .process_midi_message(ch, 0xB0, 64, if on { 127 } else { 0 })
            }
            NoteEventKind::PitchBend { value } => {
                let v = i32::from(value) + 8192;
                self.synth
                    .process_midi_message(ch, 0xE0, v & 0x7F, (v >> 7) & 0x7F)
            }
        }
    }

    fn render(&mut self, out: &mut [f32]) {
        let chunk = self.left.len();
        for part in out.chunks_mut(chunk) {
            let n = part.len();
            let (l, r) = (&mut self.left[..n], &mut self.right[..n]);
            self.synth.render(l, r);
            for ((o, a), b) in part.iter_mut().zip(l.iter()).zip(r.iter()) {
                *o = (a + b) * 0.5 * self.gain.next_value();
            }
        }
    }
}

// --- Test-SoundFont, im Speicher erzeugt (keine Datei im Repo).

fn chunk(id: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut v = id.to_vec();
    v.extend_from_slice(&(data.len() as u32).to_le_bytes());
    v.extend_from_slice(data);
    v
}

fn list(kind: &[u8; 4], parts: &[Vec<u8>]) -> Vec<u8> {
    let mut d = kind.to_vec();
    for p in parts {
        d.extend_from_slice(p);
    }
    chunk(b"LIST", &d)
}

fn name20(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.resize(20, 0);
    v
}

fn u16s(xs: &[u16]) -> Vec<u8> {
    xs.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// Kleinste gültige SF2 (ein Sinus-Sample, Preset 0) für Tests in allen Crates.
#[doc(hidden)]
pub fn tiny_sf2() -> Vec<u8> {
    let period = 100usize; // 480 Hz bei 48 kHz
    let len = period * 20;
    let mut smpl = Vec::new();
    for i in 0..len + 46 {
        let x = if i < len {
            ((i as f32 / period as f32 * std::f32::consts::TAU).sin() * 16000.0) as i16
        } else {
            0
        };
        smpl.extend_from_slice(&x.to_le_bytes());
    }
    let info = list(
        b"INFO",
        &[
            chunk(b"ifil", &u16s(&[2, 1])),
            chunk(b"INAM", &name20("woa test")),
        ],
    );
    let sdta = list(b"sdta", &[chunk(b"smpl", &smpl)]);

    let mut phdr = Vec::new();
    for (name, bag) in [("Sine", 0u16), ("EOP", 1)] {
        phdr.extend(name20(name));
        phdr.extend(u16s(&[0, 0, bag]));
        phdr.extend([0u8; 12]);
    }
    let pbag = u16s(&[0, 0, 1, 0]);
    let pmod = vec![0u8; 10];
    let pgen = u16s(&[41, 0, 0, 0]); // instrument 0, Terminator
    let mut inst = name20("Sine");
    inst.extend(u16s(&[0]));
    inst.extend(name20("EOI"));
    inst.extend(u16s(&[1]));
    let ibag = u16s(&[0, 0, 2, 0]);
    let imod = vec![0u8; 10];
    let igen = u16s(&[54, 1, 53, 0, 0, 0]); // Loop an, Sample 0, Terminator
    let mut shdr = name20("sine");
    for x in [0u32, len as u32, 0, len as u32, 48_000] {
        shdr.extend(x.to_le_bytes());
    }
    shdr.extend([69u8, 0]); // Grundton A4 (Sample ist 480 Hz, egal für den Test)
    shdr.extend(u16s(&[0, 1]));
    shdr.extend(name20("EOS"));
    shdr.extend([0u8; 26]);
    let pdta = list(
        b"pdta",
        &[
            chunk(b"phdr", &phdr),
            chunk(b"pbag", &pbag),
            chunk(b"pmod", &pmod),
            chunk(b"pgen", &pgen),
            chunk(b"inst", &inst),
            chunk(b"ibag", &ibag),
            chunk(b"imod", &imod),
            chunk(b"igen", &igen),
            chunk(b"shdr", &shdr),
        ],
    );
    let mut body = b"sfbk".to_vec();
    body.extend(info);
    body.extend(sdta);
    body.extend(pdta);
    chunk(b"RIFF", &body)
}
