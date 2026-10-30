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

/// Grid step in beats for the note value 1/`division` (4 = quarter, 16 = sixteenth),
/// as a triplet two thirds of it. Divisions outside 1..=64 are clamped.
pub fn grid_beats(division: u32, triplet: bool) -> f64 {
    let step = 4.0 / f64::from(division.clamp(1, 64));
    if triplet { step * 2.0 / 3.0 } else { step }
}

/// Shortest note the editing helpers produce, in beats.
pub const MIN_NOTE_BEATS: f64 = 1.0 / 32.0;

/// Notes plus the indices of the selected ones after an edit (Piano-Roll, agent).
pub type Edited = (MidiData, Vec<usize>);

fn snap(x: f64, grid: f64) -> f64 {
    if grid > 0.0 && grid.is_finite() {
        (x / grid).round() * grid
    } else {
        x
    }
}

/// Editing helpers of the piano roll. Each returns new content; the caller sends it
/// with [`crate::Command::SetNotes`], so every edit is one undo step and the agent uses
/// the same code. `sel` are indices into `notes`; unknown indices are ignored.
impl MidiData {
    fn rebuild(&self, mut notes: Vec<(MidiNote, bool)>) -> Edited {
        notes.sort_by(|(a, _), (b, _)| {
            a.beat
                .total_cmp(&b.beat)
                .then(a.pitch.cmp(&b.pitch))
                .then(a.length_beats.total_cmp(&b.length_beats))
        });
        let sel = notes
            .iter()
            .enumerate()
            .filter_map(|(i, (_, s))| s.then_some(i))
            .collect();
        let data = MidiData {
            notes: notes.into_iter().map(|(n, _)| n).collect(),
            cc: self.cc.clone(),
        };
        (data, sel)
    }

    fn tagged(&self, sel: &[usize]) -> Vec<(MidiNote, bool)> {
        self.notes
            .iter()
            .enumerate()
            .map(|(i, n)| (*n, sel.contains(&i)))
            .collect()
    }

    /// Adds one note (values clamped to the valid range); it is the new selection.
    pub fn add_note(&self, note: MidiNote) -> Edited {
        let note = MidiNote {
            beat: if note.beat.is_finite() {
                note.beat.max(0.0)
            } else {
                0.0
            },
            length_beats: if note.length_beats.is_finite() {
                note.length_beats.max(MIN_NOTE_BEATS)
            } else {
                1.0
            },
            pitch: note.pitch.min(127),
            velocity: note.velocity.clamp(1, 127),
        };
        let mut all = self.tagged(&[]);
        all.push((note, true));
        self.rebuild(all)
    }

    /// Removes the selected notes.
    pub fn delete_notes(&self, sel: &[usize]) -> MidiData {
        MidiData {
            notes: self
                .notes
                .iter()
                .enumerate()
                .filter(|(i, _)| !sel.contains(i))
                .map(|(_, n)| *n)
                .collect(),
            cc: self.cc.clone(),
        }
    }

    /// Moves the selected notes by `d_beats` and `d_pitch` semitones. The shift is
    /// limited as a whole, so the selection keeps its shape (no note before 0, pitch
    /// stays in 0..=127).
    pub fn move_notes(&self, sel: &[usize], d_beats: f64, d_pitch: i32) -> Edited {
        let chosen: Vec<&MidiNote> = sel.iter().filter_map(|&i| self.notes.get(i)).collect();
        if chosen.is_empty() {
            return (self.clone(), Vec::new());
        }
        let min_beat = chosen.iter().map(|n| n.beat).fold(f64::MAX, f64::min);
        let lo = chosen.iter().map(|n| i32::from(n.pitch)).min().unwrap_or(0);
        let hi = chosen
            .iter()
            .map(|n| i32::from(n.pitch))
            .max()
            .unwrap_or(127);
        let d_beats = if d_beats.is_finite() {
            d_beats.max(-min_beat)
        } else {
            0.0
        };
        let d_pitch = d_pitch.clamp(-lo, 127 - hi);
        let mut all = self.tagged(sel);
        for (n, s) in &mut all {
            if *s {
                n.beat = (n.beat + d_beats).max(0.0);
                n.pitch = (i32::from(n.pitch) + d_pitch).clamp(0, 127) as u8;
            }
        }
        self.rebuild(all)
    }

    /// Changes the length of the selected notes by `d_beats` (at least
    /// [`MIN_NOTE_BEATS`]).
    pub fn resize_notes(&self, sel: &[usize], d_beats: f64) -> Edited {
        let d = if d_beats.is_finite() { d_beats } else { 0.0 };
        let mut all = self.tagged(sel);
        for (n, s) in &mut all {
            if *s {
                n.length_beats = (n.length_beats + d).max(MIN_NOTE_BEATS);
            }
        }
        self.rebuild(all)
    }

    /// Sets the velocity (clamped to 1..=127) of the selected notes.
    pub fn set_velocity(&self, sel: &[usize], velocity: u8) -> MidiData {
        let mut out = self.clone();
        for &i in sel {
            if let Some(n) = out.notes.get_mut(i) {
                n.velocity = velocity.clamp(1, 127);
            }
        }
        out
    }

    /// Snaps the starts of the selected notes (all, if `sel` is empty) to `grid` beats,
    /// keeping their lengths. With `ends`, the ends snap too (at least one grid step).
    /// Controller values stay where they are.
    pub fn quantize(&self, sel: &[usize], grid: f64, ends: bool) -> Edited {
        let every = sel.is_empty();
        let mut all = self.tagged(sel);
        for (n, s) in &mut all {
            if every || *s {
                let end = n.beat + n.length_beats;
                n.beat = snap(n.beat, grid).max(0.0);
                if ends {
                    n.length_beats = (snap(end, grid) - n.beat).max(grid.max(MIN_NOTE_BEATS));
                }
            }
        }
        self.rebuild(all)
    }
}

#[cfg(test)]
mod tests {
    fn note(beat: f64, pitch: u8) -> MidiNote {
        MidiNote {
            beat,
            length_beats: 0.5,
            pitch,
            velocity: 90,
        }
    }

    #[test]
    fn grid_values() {
        assert_eq!(grid_beats(4, false), 1.0);
        assert_eq!(grid_beats(16, false), 0.25);
        assert!((grid_beats(8, true) - 1.0 / 3.0).abs() < 1e-12);
        assert_eq!(grid_beats(0, false), 4.0);
    }

    #[test]
    fn quantize_moves_starts_keeps_lengths() {
        let d = MidiData {
            notes: vec![note(0.1, 60), note(1.13, 62), note(1.9, 64)],
            cc: vec![],
        };
        let (q, sel) = d.quantize(&[], 0.25, false);
        let beats: Vec<f64> = q.notes.iter().map(|n| n.beat).collect();
        assert_eq!(beats, vec![0.0, 1.25, 2.0]);
        assert!(q.notes.iter().all(|n| n.length_beats == 0.5));
        assert!(sel.is_empty());
        let (q, sel) = d.quantize(&[1], 1.0, false);
        assert_eq!(q.notes[1].beat, 1.0);
        assert_eq!(q.notes[0].beat, 0.1);
        assert_eq!(sel, vec![1]);
        let (q, _) = d.quantize(&[2], 0.5, true);
        assert_eq!((q.notes[2].beat, q.notes[2].length_beats), (2.0, 0.5));
    }

    #[test]
    fn move_keeps_shape_and_follows_selection() {
        let d = MidiData {
            notes: vec![note(0.0, 60), note(1.0, 126), note(2.0, 50)],
            cc: vec![],
        };
        let (m, sel) = d.move_notes(&[0, 1], -3.0, 5);
        assert_eq!(m.notes.len(), 3);
        let moved: Vec<(f64, u8)> = sel
            .iter()
            .map(|&i| (m.notes[i].beat, m.notes[i].pitch))
            .collect();
        assert_eq!(moved, vec![(0.0, 61), (1.0, 127)]);
        let (m, sel) = d.move_notes(&[0], 3.0, 0);
        assert_eq!(sel, vec![2]);
        assert_eq!((m.notes[2].beat, m.notes[2].pitch), (3.0, 60));
    }

    #[test]
    fn add_resize_delete_velocity() {
        let d = MidiData::default();
        let (d, sel) = d.add_note(note(1.0, 60));
        let (d, sel2) = d.add_note(note(0.0, 64));
        assert_eq!((sel, sel2), (vec![0], vec![0]));
        assert_eq!(d.notes[1].pitch, 60);
        let (r, _) = d.resize_notes(&[1], -10.0);
        assert_eq!(r.notes[1].length_beats, MIN_NOTE_BEATS);
        let v = d.set_velocity(&[0, 1], 200);
        assert!(v.notes.iter().all(|n| n.velocity == 127));
        let x = d.delete_notes(&[0]);
        assert_eq!(x.notes, vec![note(1.0, 60)]);
        assert!(r.check().is_ok() && v.check().is_ok());
    }

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
