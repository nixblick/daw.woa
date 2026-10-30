//! The [`Command`] enum: the only way to change a [`Project`].

use serde::{Deserialize, Serialize};

use crate::effect::{Insert, InsertId};
use crate::error::{ModelError, Result};
use crate::project::{check_send_level, 
    Clip, ClipId, MAX_GAIN_DB, MAX_TEMPO_BPM, MIN_GAIN_DB, MIN_TEMPO_BPM, Project, Send, Track,
    TrackId,
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
    /// Changes length, audio offset and fades of a clip (trim, split, fades).
    EditClip {
        /// Clip to change.
        clip: ClipId,
        /// New length in beats (> 0).
        length_beats: f64,
        /// New offset into the audio in beats (>= 0).
        offset_beats: f64,
        /// New fade-in in beats (>= 0).
        fade_in_beats: f64,
        /// New fade-out in beats (>= 0); fade-in + fade-out <= length.
        fade_out_beats: f64,
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
    /// Replaces the notes and controller values of a MIDI clip (recording, import,
    /// agent, later the piano roll). The clip must already be a MIDI clip.
    SetNotes {
        /// Clip to change.
        clip: ClipId,
        /// New content; sorted on apply.
        midi: crate::midi::MidiData,
    },
    /// Routes the track's signal to a bus (`Some`) or the master (`None`).
    SetOutput {
        /// Track to route.
        track: TrackId,
        /// Destination bus; it must be a bus and must not lead back to `track`.
        output: Option<TrackId>,
    },
    /// Adds a send to a bus (one send per destination).
    AddSend {
        /// Source track.
        track: TrackId,
        /// The send.
        send: Send,
        /// Position in the send list (`None`: at the end).
        index: Option<usize>,
    },
    /// Removes the send to `target`.
    RemoveSend {
        /// Source track.
        track: TrackId,
        /// Destination bus of the send.
        target: TrackId,
    },
    /// Changes level and tap point of the send to `target`.
    SetSend {
        /// Source track.
        track: TrackId,
        /// Destination bus of the send.
        target: TrackId,
        /// Level in dB, `MIN_GAIN_DB..=MAX_SEND_DB`.
        level_db: f64,
        /// Tap before the fader.
        pre_fader: bool,
    },
    /// Inserts an effect into the chain of a track.
    AddInsert {
        /// Target track.
        track: TrackId,
        /// The effect; its id must be unused.
        insert: Insert,
        /// Position in the chain (`None`: at the end).
        index: Option<usize>,
    },
    /// Removes an effect from the chain.
    RemoveInsert {
        /// Track holding the effect.
        track: TrackId,
        /// Effect to remove.
        insert: InsertId,
    },
    /// Sets one parameter of an effect.
    SetInsertParam {
        /// Track holding the effect.
        track: TrackId,
        /// The effect.
        insert: InsertId,
        /// Index of the parameter, see `EffectKind::params`.
        param: usize,
        /// New value inside the parameter's range.
        value: f64,
    },
    /// Bypasses an effect or switches it back on.
    SetInsertBypass {
        /// Track holding the effect.
        track: TrackId,
        /// The effect.
        insert: InsertId,
        /// New state.
        bypass: bool,
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
            Command::EditClip { .. } => "Edit clip",
            Command::SetNotes { .. } => "Edit notes",
            Command::SetOutput { .. } => "Set output",
            Command::AddSend { .. } => "Add send",
            Command::RemoveSend { .. } => "Remove send",
            Command::SetSend { .. } => "Set send",
            Command::AddInsert { .. } => "Add effect",
            Command::RemoveInsert { .. } => "Remove effect",
            Command::SetInsertParam { .. } => "Set effect parameter",
            Command::SetInsertBypass { .. } => "Bypass effect",
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
                for r in track.output.iter().chain(track.sends.iter().map(|s| &s.target)) {
                    project.check_destination(track.id, *r)?;
                }
                for (i, s) in track.sends.iter().enumerate() {
                    check_send_level(s.level_db)?;
                    if track.sends[..i].iter().any(|o| o.target == s.target) {
                        return Err(ModelError::DuplicateSend {
                            track: track.id,
                            target: s.target,
                        });
                    }
                }
                for (i, ins) in track.inserts.iter().enumerate() {
                    ins.check()?;
                    crate::project::check_id(ins.id.0)?;
                    if ins.id.0 == track.id.0
                        || project.id_in_use(ins.id.0)
                        || track.clips.iter().any(|c| c.id.0 == ins.id.0)
                        || track.inserts[..i].iter().any(|o| o.id == ins.id)
                    {
                        return Err(ModelError::DuplicateId(ins.id.0));
                    }
                }
                let id = track.id;
                for ins in &track.inserts {
                    project.bump_next_id(ins.id.0);
                }
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
                if project
                    .tracks
                    .iter()
                    .any(|t| t.output == Some(track) || t.sends.iter().any(|s| s.target == track))
                {
                    return Err(ModelError::TrackInUse(track));
                }
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
            Command::EditClip {
                clip,
                length_beats,
                offset_beats,
                fade_in_beats,
                fade_out_beats,
            } => {
                let (ti, ci) = locate_clip(project, clip)?;
                let mut next = project.tracks[ti].clips[ci].clone();
                next.length_beats = length_beats;
                next.offset_beats = offset_beats;
                next.fade_in_beats = fade_in_beats;
                next.fade_out_beats = fade_out_beats;
                check_clip(&next)?;
                let old = std::mem::replace(&mut project.tracks[ti].clips[ci], next);
                Ok(Command::EditClip {
                    clip,
                    length_beats: old.length_beats,
                    offset_beats: old.offset_beats,
                    fade_in_beats: old.fade_in_beats,
                    fade_out_beats: old.fade_out_beats,
                })
            }
            Command::SetNotes { clip, mut midi } => {
                midi.check()?;
                let (ti, ci) = locate_clip(project, clip)?;
                let Some(old) = project.tracks[ti].clips[ci].midi.as_mut() else {
                    return Err(ModelError::NotMidiClip(clip));
                };
                midi.sort();
                let old = std::mem::replace(old, midi);
                Ok(Command::SetNotes { clip, midi: old })
            }
            Command::SetOutput { track, output } => {
                find(project, track)?;
                if let Some(to) = output {
                    project.check_destination(track, to)?;
                    if project.reaches(to, track) {
                        return Err(ModelError::RoutingCycle { from: track, to });
                    }
                }
                let t = find(project, track)?;
                let old = std::mem::replace(&mut t.output, output);
                Ok(Command::SetOutput { track, output: old })
            }
            Command::AddSend { track, send, index } => {
                check_send_level(send.level_db)?;
                let t = project.track(track).ok_or(ModelError::TrackNotFound(track))?;
                if t.sends.iter().any(|s| s.target == send.target) {
                    return Err(ModelError::DuplicateSend {
                        track,
                        target: send.target,
                    });
                }
                project.check_destination(track, send.target)?;
                if project.reaches(send.target, track) {
                    return Err(ModelError::RoutingCycle {
                        from: track,
                        to: send.target,
                    });
                }
                let target = send.target;
                let t = find(project, track)?;
                let at = index
                    .filter(|&i| i <= t.sends.len())
                    .unwrap_or(t.sends.len());
                t.sends.insert(at, send);
                Ok(Command::RemoveSend { track, target })
            }
            Command::RemoveSend { track, target } => {
                let t = find(project, track)?;
                let at = t
                    .sends
                    .iter()
                    .position(|s| s.target == target)
                    .ok_or(ModelError::SendNotFound { track, target })?;
                let removed = t.sends.remove(at);
                Ok(Command::AddSend {
                    track,
                    send: removed,
                    index: Some(at),
                })
            }
            Command::SetSend {
                track,
                target,
                level_db,
                pre_fader,
            } => {
                check_send_level(level_db)?;
                let t = find(project, track)?;
                let s = t
                    .sends
                    .iter_mut()
                    .find(|s| s.target == target)
                    .ok_or(ModelError::SendNotFound { track, target })?;
                let old_level = std::mem::replace(&mut s.level_db, level_db);
                let old_pre = std::mem::replace(&mut s.pre_fader, pre_fader);
                Ok(Command::SetSend {
                    track,
                    target,
                    level_db: old_level,
                    pre_fader: old_pre,
                })
            }
            Command::AddInsert {
                track,
                insert,
                index,
            } => {
                insert.check()?;
                crate::project::check_id(insert.id.0)?;
                if project.track(track).is_none() {
                    return Err(ModelError::TrackNotFound(track));
                }
                if project.id_in_use(insert.id.0) {
                    return Err(ModelError::DuplicateId(insert.id.0));
                }
                project.bump_next_id(insert.id.0);
                let id = insert.id;
                let t = find(project, track)?;
                let at = index
                    .filter(|&i| i <= t.inserts.len())
                    .unwrap_or(t.inserts.len());
                t.inserts.insert(at, insert);
                Ok(Command::RemoveInsert { track, insert: id })
            }
            Command::RemoveInsert { track, insert } => {
                let t = find(project, track)?;
                let at = t
                    .inserts
                    .iter()
                    .position(|i| i.id == insert)
                    .ok_or(ModelError::InsertNotFound(insert))?;
                let removed = t.inserts.remove(at);
                Ok(Command::AddInsert {
                    track,
                    insert: removed,
                    index: Some(at),
                })
            }
            Command::SetInsertParam {
                track,
                insert,
                param,
                value,
            } => {
                let t = find(project, track)?;
                let ins = t
                    .inserts
                    .iter_mut()
                    .find(|i| i.id == insert)
                    .ok_or(ModelError::InsertNotFound(insert))?;
                ins.kind.check_value(param, value)?;
                let slot = ins
                    .params
                    .get_mut(param)
                    .ok_or(ModelError::UnknownParam { index: param })?;
                let old = std::mem::replace(slot, value);
                Ok(Command::SetInsertParam {
                    track,
                    insert,
                    param,
                    value: old,
                })
            }
            Command::SetInsertBypass {
                track,
                insert,
                bypass,
            } => {
                let t = find(project, track)?;
                let ins = t
                    .inserts
                    .iter_mut()
                    .find(|i| i.id == insert)
                    .ok_or(ModelError::InsertNotFound(insert))?;
                let old = std::mem::replace(&mut ins.bypass, bypass);
                Ok(Command::SetInsertBypass {
                    track,
                    insert,
                    bypass: old,
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
    if !(c.length_beats.is_finite() && c.length_beats > 0.0) {
        return Err(ModelError::InvalidValue {
            field: "length_beats",
            value: c.length_beats,
        });
    }
    for (field, value) in [
        ("offset_beats", c.offset_beats),
        ("fade_in_beats", c.fade_in_beats),
        ("fade_out_beats", c.fade_out_beats),
    ] {
        if !(value.is_finite() && value >= 0.0) {
            return Err(ModelError::InvalidValue { field, value });
        }
    }
    if let Some(m) = &c.midi {
        m.check()?;
    }
    let fades = c.fade_in_beats + c.fade_out_beats;
    if fades > c.length_beats + 1e-9 {
        return Err(ModelError::InvalidValue {
            field: "fades",
            value: fades,
        });
    }
    Ok(())
}

fn check_track_values(t: &Track) -> Result<()> {
    check_gain(t.gain_db)?;
    check_pan(t.pan)
}
