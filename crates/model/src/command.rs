//! The [`Command`] enum: the only way to change a [`Project`].

use serde::{Deserialize, Serialize};

use crate::error::{ModelError, Result};
use crate::project::{
    Clip, ClipId, MAX_GAIN_DB, MAX_TEMPO_BPM, MIN_GAIN_DB, MIN_TEMPO_BPM, Project, Track, TrackId,
    TrackKind,
};

/// One change to a project.
///
/// Applying a command (via [`crate::Session::apply`]) validates it first and only then
/// mutates, so a failed command leaves the project untouched. A successful apply
/// returns the exact inverse command, which restores the previous state.
///
/// Commands are serde-serialisable so the agent (MCP) and remote frontends can send
/// them as JSON: `{"type": "set_gain", "track": 3, "gain_db": -6.0}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    /// Inserts a complete track (with its clips) at `index`, or at the end if `None`
    /// or out of range. Track and clip ids must be unused.
    AddTrack {
        /// The track to insert.
        track: Track,
        /// Position in the track list.
        index: Option<usize>,
    },
    /// Removes a track with all its clips.
    RemoveTrack {
        /// Track to remove.
        track: TrackId,
    },
    /// Renames a track. The name must not be empty.
    RenameTrack {
        /// Track to rename.
        track: TrackId,
        /// New name.
        name: String,
    },
    /// Sets the gain of a track in dB.
    SetGain {
        /// Target track.
        track: TrackId,
        /// Gain in dB, `MIN_GAIN_DB..=MAX_GAIN_DB`.
        gain_db: f64,
    },
    /// Sets the pan of a track.
    SetPan {
        /// Target track.
        track: TrackId,
        /// Pan, `-1.0..=1.0`.
        pan: f64,
    },
    /// Mutes or unmutes a track.
    SetMute {
        /// Target track.
        track: TrackId,
        /// New mute state.
        mute: bool,
    },
    /// Solos or unsolos a track.
    SetSolo {
        /// Target track.
        track: TrackId,
        /// New solo state.
        solo: bool,
    },
    /// Sets the project tempo.
    SetTempo {
        /// Beats per minute, `MIN_TEMPO_BPM..=MAX_TEMPO_BPM`.
        bpm: f64,
    },
    /// Inserts a clip on a track at `index` (or at the end).
    AddClip {
        /// Target track.
        track: TrackId,
        /// The clip; its id must be unused.
        clip: Clip,
        /// Position in the clip list.
        index: Option<usize>,
    },
    /// Removes a clip.
    RemoveClip {
        /// Clip to remove.
        clip: ClipId,
    },
    /// Moves a clip to another start position and/or another track.
    MoveClip {
        /// Clip to move.
        clip: ClipId,
        /// Destination track (may be the current one).
        to_track: TrackId,
        /// New start in beats (>= 0).
        start_beats: f64,
        /// Position in the destination clip list. `None`: keep the position on the
        /// same track, append at the end on another track.
        #[serde(default)]
        index: Option<usize>,
    },
}

impl Command {
    /// Convenience: add a fresh track with default mixer settings at the end.
    pub fn add_track(id: TrackId, name: impl Into<String>, kind: TrackKind) -> Self {
        Command::AddTrack {
            track: Track::new(id, name, kind),
            index: None,
        }
    }

    /// Short human-readable label, used as default undo label.
    pub fn label(&self) -> &'static str {
        match self {
            Command::AddTrack { .. } => "Add track",
            Command::RemoveTrack { .. } => "Remove track",
            Command::RenameTrack { .. } => "Rename track",
            Command::SetGain { .. } => "Set gain",
            Command::SetPan { .. } => "Set pan",
            Command::SetMute { .. } => "Mute",
            Command::SetSolo { .. } => "Solo",
            Command::SetTempo { .. } => "Set tempo",
            Command::AddClip { .. } => "Add clip",
            Command::RemoveClip { .. } => "Remove clip",
            Command::MoveClip { .. } => "Move clip",
        }
    }

    /// Applies the command to `project` and returns its inverse.
    ///
    /// On error the project is unchanged.
    pub(crate) fn apply(self, project: &mut Project) -> Result<Command> {
        match self {
            Command::AddTrack { track, index } => {
                if track.name.trim().is_empty() {
                    return Err(ModelError::EmptyName);
                }
                check_track_values(&track)?;
                crate::project::check_id(track.id.0)?;
                if project.id_in_use(track.id.0) {
                    return Err(ModelError::DuplicateId(track.id.0));
                }
                for (i, c) in track.clips.iter().enumerate() {
                    check_clip(c)?;
                    crate::project::check_id(c.id.0)?;
                    if c.id.0 == track.id.0
                        || project.id_in_use(c.id.0)
                        || track.clips[..i].iter().any(|o| o.id == c.id)
                    {
                        return Err(ModelError::DuplicateId(c.id.0));
                    }
                }
                let id = track.id;
                project.bump_next_id(id.0);
                for c in &track.clips {
                    project.bump_next_id(c.id.0);
                }
                let at = index
                    .filter(|&i| i <= project.tracks.len())
                    .unwrap_or(project.tracks.len());
                project.tracks.insert(at, track);
                Ok(Command::RemoveTrack { track: id })
            }
            Command::RemoveTrack { track } => {
                let at = project
                    .track_index(track)
                    .ok_or(ModelError::TrackNotFound(track))?;
                let removed = project.tracks.remove(at);
                Ok(Command::AddTrack {
                    track: removed,
                    index: Some(at),
                })
            }
            Command::RenameTrack { track, name } => {
                if name.trim().is_empty() {
                    return Err(ModelError::EmptyName);
                }
                let t = find(project, track)?;
                let old = std::mem::replace(&mut t.name, name);
                Ok(Command::RenameTrack { track, name: old })
            }
            Command::SetGain { track, gain_db } => {
                check_gain(gain_db)?;
                let t = find(project, track)?;
                let old = std::mem::replace(&mut t.gain_db, gain_db);
                Ok(Command::SetGain {
                    track,
                    gain_db: old,
                })
            }
            Command::SetPan { track, pan } => {
                check_pan(pan)?;
                let t = find(project, track)?;
                let old = std::mem::replace(&mut t.pan, pan);
                Ok(Command::SetPan { track, pan: old })
            }
            Command::SetMute { track, mute } => {
                let t = find(project, track)?;
                let old = std::mem::replace(&mut t.mute, mute);
                Ok(Command::SetMute { track, mute: old })
            }
            Command::SetSolo { track, solo } => {
                let t = find(project, track)?;
                let old = std::mem::replace(&mut t.solo, solo);
                Ok(Command::SetSolo { track, solo: old })
            }
            Command::SetTempo { bpm } => {
                if !(bpm.is_finite() && (MIN_TEMPO_BPM..=MAX_TEMPO_BPM).contains(&bpm)) {
                    return Err(ModelError::InvalidValue {
                        field: "bpm",
                        value: bpm,
                    });
                }
                let old = std::mem::replace(&mut project.tempo_bpm, bpm);
                Ok(Command::SetTempo { bpm: old })
            }
            Command::AddClip { track, clip, index } => {
                check_clip(&clip)?;
                crate::project::check_id(clip.id.0)?;
                if project.track(track).is_none() {
                    return Err(ModelError::TrackNotFound(track));
                }
                if project.id_in_use(clip.id.0) {
                    return Err(ModelError::DuplicateId(clip.id.0));
                }
                project.bump_next_id(clip.id.0);
                let id = clip.id;
                let t = find(project, track)?;
                let at = index
                    .filter(|&i| i <= t.clips.len())
                    .unwrap_or(t.clips.len());
                t.clips.insert(at, clip);
                Ok(Command::RemoveClip { clip: id })
            }
            Command::RemoveClip { clip } => {
                let (ti, ci) = locate_clip(project, clip)?;
                let t = &mut project.tracks[ti];
                let removed = t.clips.remove(ci);
                Ok(Command::AddClip {
                    track: t.id,
                    clip: removed,
                    index: Some(ci),
                })
            }
            Command::MoveClip {
                clip,
                to_track,
                start_beats,
                index,
            } => {
                check_start(start_beats)?;
                let dest = project
                    .track_index(to_track)
                    .ok_or(ModelError::TrackNotFound(to_track))?;
                let (ti, ci) = locate_clip(project, clip)?;
                let from_track = project.tracks[ti].id;
                let mut c = project.tracks[ti].clips.remove(ci);
                let old = std::mem::replace(&mut c.start_beats, start_beats);
                let target = &mut project.tracks[dest].clips;
                let at = match index {
                    Some(i) if i <= target.len() => i,
                    // Same track without index: keep the clip where it was.
                    None if ti == dest => ci,
                    _ => target.len(),
                };
                target.insert(at, c);
                Ok(Command::MoveClip {
                    clip,
                    to_track: from_track,
                    start_beats: old,
                    index: Some(ci),
                })
            }
        }
    }
}

fn find(project: &mut Project, id: TrackId) -> Result<&mut Track> {
    project.track_mut(id).ok_or(ModelError::TrackNotFound(id))
}

fn locate_clip(project: &Project, id: ClipId) -> Result<(usize, usize)> {
    project
        .tracks
        .iter()
        .enumerate()
        .find_map(|(ti, t)| t.clips.iter().position(|c| c.id == id).map(|ci| (ti, ci)))
        .ok_or(ModelError::ClipNotFound(id))
}

fn check_gain(v: f64) -> Result<()> {
    if v.is_finite() && (MIN_GAIN_DB..=MAX_GAIN_DB).contains(&v) {
        Ok(())
    } else {
        Err(ModelError::InvalidValue {
            field: "gain_db",
            value: v,
        })
    }
}

fn check_pan(v: f64) -> Result<()> {
    if v.is_finite() && (-1.0..=1.0).contains(&v) {
        Ok(())
    } else {
        Err(ModelError::InvalidValue {
            field: "pan",
            value: v,
        })
    }
}

fn check_start(v: f64) -> Result<()> {
    if v.is_finite() && v >= 0.0 {
        Ok(())
    } else {
        Err(ModelError::InvalidValue {
            field: "start_beats",
            value: v,
        })
    }
}

fn check_clip(c: &Clip) -> Result<()> {
    check_start(c.start_beats)?;
    if c.length_beats.is_finite() && c.length_beats > 0.0 {
        Ok(())
    } else {
        Err(ModelError::InvalidValue {
            field: "length_beats",
            value: c.length_beats,
        })
    }
}

fn check_track_values(t: &Track) -> Result<()> {
    check_gain(t.gain_db)?;
    check_pan(t.pan)
}
