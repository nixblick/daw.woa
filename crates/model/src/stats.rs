//! Projektgröße als Zahlen für Log und Telemetrie (#585): wie viel hat die Oberfläche zu
//! zeichnen und die Engine zu spielen? Keine Namen, keine Inhalte.

use std::fmt;

use crate::Project;

/// Kennzahlen eines Projekts, reine Rechnung aus [`Project`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectStats {
    /// Spuren (auch Busse).
    pub tracks: u64,
    /// Clips auf allen Spuren.
    pub clips: u64,
    /// MIDI-Noten in allen Clips.
    pub midi_notes: u64,
    /// Eingebaute Effekte (Inserts) auf allen Spuren.
    pub inserts: u64,
    /// Plugin-Einträge auf allen Spuren.
    pub plugins: u64,
    /// Stützpunkte aller Automationskurven.
    pub automation_points: u64,
}

impl ProjectStats {
    /// Zählt alles in `project`.
    pub fn of(project: &Project) -> Self {
        let mut s = Self::default();
        for t in &project.tracks {
            s.tracks += 1;
            s.clips += t.clips.len() as u64;
            s.inserts += t.inserts.len() as u64;
            s.plugins += t.plugins.len() as u64;
            for c in &t.clips {
                if let Some(m) = &c.midi {
                    s.midi_notes += m.notes.len() as u64;
                }
            }
            for l in &t.automation {
                s.automation_points += l.points.len() as u64;
            }
        }
        s
    }
}

/// Eine Zeile fürs Log: `tracks=3 clips=12 midi_notes=480 …`.
impl fmt::Display for ProjectStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tracks={} clips={}", self.tracks, self.clips)?;
        write!(f, " midi_notes={}", self.midi_notes)?;
        write!(f, " inserts={}", self.inserts)?;
        write!(f, " plugins={}", self.plugins)?;
        write!(f, " automation_points={}", self.automation_points)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AutoMode, AutoPoint, AutoTarget, AutomationLane, Clip, ClipId, EffectKind, Insert,
        InsertId, MidiData, MidiNote, Track, TrackId, TrackKind,
    };

    fn notes(n: usize) -> MidiData {
        let note = MidiNote {
            beat: 0.0,
            length_beats: 0.5,
            pitch: 60,
            velocity: 100,
        };
        MidiData {
            notes: vec![note; n],
            cc: Vec::new(),
        }
    }

    fn point(beat: f64) -> AutoPoint {
        AutoPoint { beat, value: 0.0 }
    }

    #[test]
    fn counts_tracks_clips_notes_inserts_and_points() {
        let mut p = Project::new("Test");
        assert_eq!(ProjectStats::of(&p), ProjectStats::default());

        let mut midi = Track::new(TrackId(1), "Klavier", TrackKind::Midi);
        let a = Clip::new_midi(ClipId(2), "A", 0.0, 4.0, notes(3));
        let b = Clip::new_midi(ClipId(3), "B", 4.0, 4.0, notes(4));
        midi.clips = vec![a, b];
        midi.inserts = vec![Insert::new(InsertId(4), EffectKind::Eq)];
        let lane = AutomationLane {
            target: AutoTarget::Gain,
            mode: AutoMode::Read,
            points: vec![point(0.0), point(4.0)],
        };
        midi.automation = vec![lane];
        let mut audio = Track::new(TrackId(5), "Stimme", TrackKind::Audio);
        let take = Clip::new(ClipId(6), "Take", 0.0, 8.0, Some("audio/a.wav".into()));
        audio.clips = vec![take];
        audio.inserts = vec![Insert::new(InsertId(7), EffectKind::Compressor)];
        let bus = Track::new(TrackId(8), "Bus", TrackKind::Bus);
        p.tracks = vec![midi, audio, bus];

        let s = ProjectStats::of(&p);
        let want = ProjectStats {
            tracks: 3,
            clips: 3,
            midi_notes: 7,
            inserts: 2,
            plugins: 0,
            automation_points: 2,
        };
        assert_eq!(s, want);
        let line = "tracks=3 clips=3 midi_notes=7 inserts=2 plugins=0 automation_points=2";
        assert_eq!(s.to_string(), line);
    }
}
