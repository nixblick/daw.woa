//! Plain data types of a project. They carry no behaviour beyond lookups;
//! changes go through [`crate::Command`].

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::{ModelError, Result};

/// Tempo of a new project.
pub const DEFAULT_TEMPO_BPM: f64 = 120.0;
/// Lowest accepted tempo.
pub const MIN_TEMPO_BPM: f64 = 20.0;
/// Highest accepted tempo.
pub const MAX_TEMPO_BPM: f64 = 400.0;
/// Lowest accepted track gain (dB).
pub const MIN_GAIN_DB: f64 = -96.0;
/// Highest accepted track gain (dB).
pub const MAX_GAIN_DB: f64 = 24.0;

/// Stable id of a track. Ids are never reused within a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TrackId(pub u64);

/// Stable id of a clip. Shares the id space with [`TrackId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClipId(pub u64);

impl fmt::Display for TrackId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "T{}", self.0)
    }
}

impl fmt::Display for ClipId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "C{}", self.0)
    }
}

/// What a track carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    /// Recorded or imported audio.
    Audio,
    /// MIDI notes driving an instrument.
    Midi,
    /// Bus/group track without clips of its own.
    Bus,
}

/// Descriptive project data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectMeta {
    /// Display name, also the default folder name.
    pub name: String,
}

/// A clip on a track: position and length in beats, optionally backed by an audio file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    /// Stable id.
    pub id: ClipId,
    /// Display name.
    pub name: String,
    /// Start position in beats (>= 0).
    pub start_beats: f64,
    /// Length in beats (> 0).
    pub length_beats: f64,
    /// Audio file of the clip, relative to the project folder (e.g. `aufnahmen/x.wav`).
    /// `None`: placeholder clip without sound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
    /// Where in the audio file the clip begins, in beats (after a split).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub offset_beats: f64,
    /// Fade-in length in beats.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fade_in_beats: f64,
    /// Fade-out length in beats.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fade_out_beats: f64,
    /// Notes of a MIDI clip (Paket 12). `None`: audio or placeholder clip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub midi: Option<crate::midi::MidiData>,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl Clip {
    /// A clip without audio offset and fades.
    pub fn new(
        id: ClipId,
        name: impl Into<String>,
        start_beats: f64,
        length_beats: f64,
        audio: Option<String>,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            start_beats,
            length_beats,
            audio,
            offset_beats: 0.0,
            fade_in_beats: 0.0,
            fade_out_beats: 0.0,
            midi: None,
        }
    }

    /// A MIDI clip holding `midi` (no audio).
    pub fn new_midi(
        id: ClipId,
        name: impl Into<String>,
        start_beats: f64,
        length_beats: f64,
        midi: crate::midi::MidiData,
    ) -> Self {
        let mut c = Self::new(id, name, start_beats, length_beats, None);
        c.midi = Some(midi);
        c
    }

    /// End on the timeline in beats.
    pub fn end_beats(&self) -> f64 {
        self.start_beats + self.length_beats
    }
}

/// Whether a clip audio path is a plain relative path inside the project folder:
/// only normal components, no `..`, no root, no drive prefix.
pub fn is_safe_audio_path(rel: &str) -> bool {
    let p = std::path::Path::new(rel);
    !rel.is_empty()
        && p.components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
}

#[cfg(test)]
mod path_tests {
    #[test]
    fn audio_paths_stay_inside() {
        assert!(super::is_safe_audio_path("aufnahmen/a.wav"));
        for bad in [
            "",
            "../a.wav",
            "aufnahmen/../../a.wav",
            "/etc/passwd",
            "./a.wav",
        ] {
            assert!(!super::is_safe_audio_path(bad), "{bad}");
        }
    }
}

/// A track with its mixer settings and clips.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    /// Stable id.
    pub id: TrackId,
    /// Display name, never empty.
    pub name: String,
    /// What the track carries.
    pub kind: TrackKind,
    /// Gain in dB, within [`MIN_GAIN_DB`]..=[`MAX_GAIN_DB`].
    pub gain_db: f64,
    /// Pan from -1.0 (left) to 1.0 (right).
    pub pan: f64,
    /// Muted.
    pub mute: bool,
    /// Soloed.
    pub solo: bool,
    /// Clips in insertion order.
    pub clips: Vec<Clip>,
}

impl Track {
    /// A new track with neutral mixer settings and no clips.
    pub fn new(id: TrackId, name: impl Into<String>, kind: TrackKind) -> Self {
        Self {
            id,
            name: name.into(),
            kind,
            gain_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            clips: Vec::new(),
        }
    }
}

/// The whole project as it is saved to `project.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    /// Descriptive data.
    pub meta: ProjectMeta,
    /// Tempo in beats per minute.
    pub tempo_bpm: f64,
    /// Tracks in display order.
    pub tracks: Vec<Track>,
    /// Next free id for tracks and clips. Only grows.
    pub next_id: u64,
}

impl Project {
    /// An empty project at [`DEFAULT_TEMPO_BPM`].
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            meta: ProjectMeta { name: name.into() },
            tempo_bpm: DEFAULT_TEMPO_BPM,
            tracks: Vec::new(),
            next_id: 1,
        }
    }

    /// Looks up a track.
    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    /// Position of a track in [`Project::tracks`].
    pub fn track_index(&self, id: TrackId) -> Option<usize> {
        self.tracks.iter().position(|t| t.id == id)
    }

    /// Looks up a clip and the track it lives on.
    pub fn clip(&self, id: ClipId) -> Option<(&Track, &Clip)> {
        self.tracks
            .iter()
            .find_map(|t| t.clips.iter().find(|c| c.id == id).map(|c| (t, c)))
    }

    /// True if `raw` is used by any track or clip.
    pub fn id_in_use(&self, raw: u64) -> bool {
        self.tracks
            .iter()
            .any(|t| t.id.0 == raw || t.clips.iter().any(|c| c.id.0 == raw))
    }

    pub(crate) fn track_mut(&mut self, id: TrackId) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    /// Hands out a fresh id and advances the counter.
    ///
    /// Loading and every command keep all ids and `next_id` at or below
    /// [`MAX_ID`] + 1, so the loop ends and the counter cannot overflow.
    pub(crate) fn alloc_id(&mut self) -> u64 {
        // Skip anything already used, e.g. ids from hand-edited files.
        while self.id_in_use(self.next_id) {
            self.next_id = self.next_id.saturating_add(1);
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Rejects a loaded project whose ids or counter exceed [`MAX_ID`].
    pub(crate) fn check_ids(&self) -> Result<()> {
        if self.next_id > MAX_ID + 1 {
            return Err(ModelError::IdOutOfRange(self.next_id));
        }
        for t in &self.tracks {
            check_id(t.id.0)?;
            for c in &t.clips {
                check_id(c.id.0)?;
                if let Some(m) = &c.midi {
                    m.check()?;
                }
            }
        }
        Ok(())
    }

    /// Keeps `next_id` above every id that was inserted from outside.
    pub(crate) fn bump_next_id(&mut self, raw: u64) {
        if raw >= self.next_id {
            self.next_id = raw.saturating_add(1);
        }
    }
}

/// Largest id the model hands out or accepts (2^53, exact in JSON numbers).
/// The cap keeps the id counter far from overflow, even for hostile files.
pub const MAX_ID: u64 = 1 << 53;

/// Errors with [`ModelError::IdOutOfRange`] if `raw` is above [`MAX_ID`].
pub(crate) fn check_id(raw: u64) -> Result<()> {
    if raw > MAX_ID {
        return Err(ModelError::IdOutOfRange(raw));
    }
    Ok(())
}
