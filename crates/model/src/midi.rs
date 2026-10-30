//! Notes and controller data of a MIDI clip (Paket 12).
//!
//! Times are in beats relative to the clip start, so moving a clip moves its notes.
//! The engine plays them through the instrument of the track.

use serde::{Deserialize, Serialize};

use crate::error::{ModelError, Result};

/// Controller number of the sustain pedal.
pub const CC_SUSTAIN: u8 = 64;
/// Most notes or controller values one clip may hold (memory guard for hostile files).
pub const MAX_MIDI_EVENTS: usize = 1 << 20;

/// One note of a MIDI clip.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MidiNote {
    /// Start in beats from the clip start (>= 0).
    pub beat: f64,
    /// Length in beats (> 0).
    pub length_beats: f64,
    /// MIDI pitch 0..=127 (60 = C4).
    pub pitch: u8,
    /// Velocity 1..=127.
    pub velocity: u8,
}

/// One controller value (e.g. sustain pedal) of a MIDI clip.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MidiCc {
    /// Position in beats from the clip start (>= 0).
    pub beat: f64,
    /// Controller number 0..=127 ([`CC_SUSTAIN`] = sustain pedal).
    pub controller: u8,
    /// Value 0..=127.
    pub value: u8,
}

/// Content of a MIDI clip: notes sorted by start, controller values sorted by position.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MidiData {
    /// Notes.
    #[serde(default)]
    pub notes: Vec<MidiNote>,
    /// Controller values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cc: Vec<MidiCc>,
}

fn invalid(field: &'static str, value: f64) -> ModelError {
    ModelError::InvalidValue { field, value }
}

impl MidiData {
    /// Sorts notes by start (then pitch) and controller values by position (stable).
    pub fn sort(&mut self) {
        self.notes.sort_by(|a, b| {
            a.beat
                .total_cmp(&b.beat)
                .then(a.pitch.cmp(&b.pitch))
                .then(a.length_beats.total_cmp(&b.length_beats))
        });
        self.cc.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    }

    /// End of the last note in beats from the clip start (0 without notes).
    pub fn end_beats(&self) -> f64 {
        self.notes
            .iter()
            .map(|n| n.beat + n.length_beats)
            .fold(0.0, f64::max)
    }

    /// Checks every value; used by commands and when loading a project.
    pub fn check(&self) -> Result<()> {
        if self.notes.len() > MAX_MIDI_EVENTS || self.cc.len() > MAX_MIDI_EVENTS {
            return Err(invalid(
                "midi events",
                self.notes.len().max(self.cc.len()) as f64,
            ));
        }
        for n in &self.notes {
            if !(n.beat.is_finite() && n.beat >= 0.0) {
                return Err(invalid("note beat", n.beat));
            }
            if !(n.length_beats.is_finite() && n.length_beats > 0.0) {
                return Err(invalid("note length_beats", n.length_beats));
            }
            if n.pitch > 127 {
                return Err(invalid("note pitch", f64::from(n.pitch)));
            }
            if !(1..=127).contains(&n.velocity) {
                return Err(invalid("note velocity", f64::from(n.velocity)));
            }
        }
        for c in &self.cc {
            if !(c.beat.is_finite() && c.beat >= 0.0) {
                return Err(invalid("cc beat", c.beat));
            }
            if c.controller > 127 || c.value > 127 {
                return Err(invalid("cc", f64::from(c.controller.max(c.value))));
            }
        }
        Ok(())
    }

    /// A fingerprint that changes whenever a value changes (for "rebuild the graph?").
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for n in &self.notes {
            (
                n.beat.to_bits(),
                n.length_beats.to_bits(),
                n.pitch,
                n.velocity,
            )
                .hash(&mut h);
        }
        0xFFu8.hash(&mut h);
        for c in &self.cc {
            (c.beat.to_bits(), c.controller, c.value).hash(&mut h);
        }
        h.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_rejects_bad_values() {
        let good = MidiNote {
            beat: 0.0,
            length_beats: 1.0,
            pitch: 60,
            velocity: 100,
        };
        let mut d = MidiData {
            notes: vec![good],
            cc: vec![],
        };
        assert!(d.check().is_ok());
        for bad in [
            MidiNote { beat: -1.0, ..good },
            MidiNote {
                length_beats: 0.0,
                ..good
            },
            MidiNote { pitch: 128, ..good },
            MidiNote {
                velocity: 0,
                ..good
            },
            MidiNote {
                beat: f64::NAN,
                ..good
            },
        ] {
            d.notes = vec![bad];
            assert!(d.check().is_err(), "{bad:?}");
        }
        d.notes.clear();
        d.cc = vec![MidiCc {
            beat: 0.0,
            controller: 64,
            value: 200,
        }];
        assert!(d.check().is_err());
    }

    #[test]
    fn sort_and_end() {
        let n = |beat, pitch| MidiNote {
            beat,
            length_beats: 0.5,
            pitch,
            velocity: 90,
        };
        let mut d = MidiData {
            notes: vec![n(2.0, 60), n(0.0, 64), n(0.0, 60)],
            cc: vec![],
        };
        d.sort();
        assert_eq!(d.notes[0].pitch, 60);
        assert_eq!(d.notes[1].pitch, 64);
        assert!((d.end_beats() - 2.5).abs() < 1e-12);
        let f = d.fingerprint();
        d.notes[2].velocity = 91;
        assert_ne!(f, d.fingerprint());
    }
}
