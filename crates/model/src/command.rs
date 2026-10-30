//! The [`Command`] enum: the only way to change a [`Project`].

use serde::{Deserialize, Serialize};

use crate::automation::{AutoMode, AutoPoint, AutoTarget, AutomationLane};
use crate::effect::{Insert, InsertId};
use crate::error::{ModelError, Result};
use crate::plugin::{PluginParam, PluginRole, PluginSlot};
use crate::project::{
    Clip, ClipId, FadeCurve, MAX_GAIN_DB, MAX_TEMPO_BPM, MIN_GAIN_DB, MIN_TEMPO_BPM, Project, Send,
    Track, TrackId, TrackKind, check_send_level,
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
    /// Sets the shape of both fades of a clip (Paket 21).
    SetClipFadeCurves {
        /// Clip to change.
        clip: ClipId,
        /// Shape of the fade-in.
        fade_in: FadeCurve,
        /// Shape of the fade-out.
        fade_out: FadeCurve,
    },
    /// Sets the gain of a clip in dB (Paket 21), within the track gain range.
    SetClipGain {
        /// Clip to change.
        clip: ClipId,
        /// New gain in dB.
        gain_db: f64,
    },
    /// Time-stretches a clip (Paket 23): sets the new timeline length and the stretch factor
    /// together, so the audio shown stays the same. The fades must fit the new length.
    SetClipStretch {
        /// Clip to change.
        clip: ClipId,
        /// New length on the timeline in beats.
        length_beats: f64,
        /// New stretch factor, within [`crate::STRETCH_RANGE`].
        stretch: f64,
        /// Fade-in in beats after the change.
        fade_in_beats: f64,
        /// Fade-out in beats after the change.
        fade_out_beats: f64,
    },
    /// Sets the pitch shift of a clip in semitones (Paket 23), within [`crate::PITCH_RANGE`].
    SetClipPitch {
        /// Clip to change.
        clip: ClipId,
        /// New shift in semitones.
        semitones: f64,
    },
    /// Makes a clip follow the project tempo (`bpm`: the tempo its audio belongs to) or
    /// stops following (`None`) (Paket 23).
    SetClipFollowTempo {
        /// Clip to change.
        clip: ClipId,
        /// Tempo the audio's beats refer to, within [`crate::FOLLOW_TEMPO_RANGE`].
        bpm: Option<f64>,
    },
    /// Sets, replaces or removes (`folder: None`) the take folder `folder_id` of a track
    /// together with the clips that make up its comp (Paket 22). The clips of the old
    /// folder are removed first and `clips` are added in their place; their ids must be
    /// exactly `folder.clip_ids`. The inverse is the same command with the old state.
    SetTakeFolder {
        /// Track that holds the folder.
        track: TrackId,
        /// Id of the folder to replace or remove (a new id adds a folder).
        folder_id: u64,
        /// New folder, or `None` to remove it.
        folder: Option<crate::takes::TakeFolder>,
        /// The comp clips of the new folder.
        clips: Vec<Clip>,
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
    /// Puts a plugin (Paket 31) on a track: an instrument on a MIDI track (at most one) or an
    /// effect on an audio or MIDI track.
    AddPlugin {
        /// Target track.
        track: TrackId,
        /// The plugin; its id must be unused.
        plugin: PluginSlot,
        /// Position among the track's plugins (`None`: at the end).
        index: Option<usize>,
    },
    /// Removes a plugin from a track.
    RemovePlugin {
        /// Track holding the plugin.
        track: TrackId,
        /// The plugin entry.
        plugin: InsertId,
    },
    /// Sets one parameter value of a plugin; the value is applied after the stored state.
    SetPluginParam {
        /// Track holding the plugin.
        track: TrackId,
        /// The plugin entry.
        plugin: InsertId,
        /// CLAP parameter id.
        param: u32,
        /// New value in the plugin's range.
        value: f64,
    },
    /// Replaces the stored state and parameter values of a plugin (also used to take over the
    /// state of the running plugin before saving).
    SetPluginState {
        /// Track holding the plugin.
        track: TrackId,
        /// The plugin entry.
        plugin: InsertId,
        /// New state (base64), `None`: default state.
        state: Option<String>,
        /// New parameter values.
        params: Vec<PluginParam>,
    },
    /// Bypasses a plugin or switches it back on.
    SetPluginBypass {
        /// Track holding the plugin.
        track: TrackId,
        /// The plugin entry.
        plugin: InsertId,
        /// New state.
        bypass: bool,
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
    /// Sets the sidechain source of a compressor or gate (`None`: its own signal).
    SetInsertSidechain {
        /// Track holding the effect.
        track: TrackId,
        /// The effect.
        insert: InsertId,
        /// Track whose signal measures the level; it must not depend on `track`.
        source: Option<TrackId>,
    },
    /// Replaces the automation lane of one target (points and mode). No points and mode
    /// `Read` remove the lane.
    SetAutomation {
        /// Track holding the target.
        track: TrackId,
        /// What the lane controls; it must exist on the track.
        target: AutoTarget,
        /// The points; sorted on apply, values inside the range of the target.
        points: Vec<AutoPoint>,
        /// Read, write or latch.
        mode: AutoMode,
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
            Command::SetClipFadeCurves { .. } => "Set fade curves",
            Command::SetClipGain { .. } => "Set clip gain",
            Command::SetClipStretch { .. } => "Stretch clip",
            Command::SetClipPitch { .. } => "Set clip pitch",
            Command::SetClipFollowTempo { .. } => "Set clip tempo follow",
            Command::SetTakeFolder { .. } => "Set take folder",
            Command::SetNotes { .. } => "Edit notes",
            Command::SetOutput { .. } => "Set output",
            Command::AddSend { .. } => "Add send",
            Command::RemoveSend { .. } => "Remove send",
            Command::SetSend { .. } => "Set send",
            Command::AddPlugin { .. } => "Add plugin",
            Command::RemovePlugin { .. } => "Remove plugin",
            Command::SetPluginParam { .. } => "Set plugin parameter",
            Command::SetPluginState { .. } => "Set plugin state",
            Command::SetPluginBypass { .. } => "Bypass plugin",
            Command::AddInsert { .. } => "Add effect",
            Command::RemoveInsert { .. } => "Remove effect",
            Command::SetInsertParam { .. } => "Set effect parameter",
            Command::SetInsertBypass { .. } => "Bypass effect",
            Command::SetInsertSidechain { .. } => "Set sidechain",
            Command::SetAutomation { .. } => "Edit automation",
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
                for r in track
                    .output
                    .iter()
                    .chain(track.sends.iter().map(|s| &s.target))
                {
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
                for ins in &track.inserts {
                    if let Some(src) = ins.sidechain {
                        project.check_sidechain(&track, ins, src)?;
                        if src == track.id {
                            return Err(ModelError::RoutingCycle {
                                from: src,
                                to: track.id,
                            });
                        }
                    }
                }
                Project::check_automation(&track)?;
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
                if project.tracks.iter().any(|t| {
                    t.output == Some(track)
                        || t.sends.iter().any(|s| s.target == track)
                        || t.inserts.iter().any(|i| i.sidechain == Some(track))
                }) {
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
            Command::SetClipFadeCurves {
                clip,
                fade_in,
                fade_out,
            } => {
                let (ti, ci) = locate_clip(project, clip)?;
                let c = &mut project.tracks[ti].clips[ci];
                let old = Command::SetClipFadeCurves {
                    clip,
                    fade_in: c.fade_in_curve,
                    fade_out: c.fade_out_curve,
                };
                c.fade_in_curve = fade_in;
                c.fade_out_curve = fade_out;
                Ok(old)
            }
            Command::SetClipGain { clip, gain_db } => {
                check_gain(gain_db)?;
                let (ti, ci) = locate_clip(project, clip)?;
                let c = &mut project.tracks[ti].clips[ci];
                let old = std::mem::replace(&mut c.gain_db, gain_db);
                Ok(Command::SetClipGain { clip, gain_db: old })
            }
            Command::SetClipStretch {
                clip,
                length_beats,
                stretch,
                fade_in_beats,
                fade_out_beats,
            } => {
                check_range("stretch", stretch, crate::STRETCH_RANGE)?;
                let (ti, ci) = locate_clip(project, clip)?;
                let mut next = project.tracks[ti].clips[ci].clone();
                next.length_beats = length_beats;
                next.stretch = stretch;
                next.fade_in_beats = fade_in_beats;
                next.fade_out_beats = fade_out_beats;
                check_clip(&next)?;
                let old = std::mem::replace(&mut project.tracks[ti].clips[ci], next);
                Ok(Command::SetClipStretch {
                    clip,
                    length_beats: old.length_beats,
                    stretch: old.stretch,
                    fade_in_beats: old.fade_in_beats,
                    fade_out_beats: old.fade_out_beats,
                })
            }
            Command::SetClipPitch { clip, semitones } => {
                check_range("pitch_semitones", semitones, crate::PITCH_RANGE)?;
                let (ti, ci) = locate_clip(project, clip)?;
                let c = &mut project.tracks[ti].clips[ci];
                let old = std::mem::replace(&mut c.pitch_semitones, semitones);
                Ok(Command::SetClipPitch {
                    clip,
                    semitones: old,
                })
            }
            Command::SetClipFollowTempo { clip, bpm } => {
                if let Some(b) = bpm {
                    check_range("follow_tempo_bpm", b, crate::FOLLOW_TEMPO_RANGE)?;
                }
                let (ti, ci) = locate_clip(project, clip)?;
                let c = &mut project.tracks[ti].clips[ci];
                let old = std::mem::replace(&mut c.follow_tempo_bpm, bpm);
                Ok(Command::SetClipFollowTempo { clip, bpm: old })
            }
            Command::SetTakeFolder {
                track,
                folder_id,
                folder,
                clips,
            } => {
                let t = project
                    .track(track)
                    .ok_or(ModelError::TrackNotFound(track))?;
                if let Some(f) = &folder {
                    f.check()?;
                    crate::project::check_id(f.id)?;
                    if f.id != folder_id || t.kind != TrackKind::Audio {
                        return Err(ModelError::InvalidArgument(
                            "take folder needs its own id and an audio track".into(),
                        ));
                    }
                    if f.clip_ids.len() != clips.len()
                        || f.clip_ids.iter().zip(&clips).any(|(i, c)| *i != c.id)
                    {
                        return Err(ModelError::InvalidArgument(
                            "clips do not match the folder's clip ids".into(),
                        ));
                    }
                }
                let old_pos = t.take_folders.iter().position(|f| f.id == folder_id);
                let old_ids: Vec<ClipId> = old_pos
                    .map(|p| t.take_folders[p].clip_ids.clone())
                    .unwrap_or_default();
                // Ids may be reused from the old clips, but nothing else.
                let in_use = |raw: u64| {
                    project.id_in_use(raw)
                        && !old_ids.iter().any(|o| o.0 == raw)
                        && !(old_pos.is_some() && raw == folder_id)
                };
                if let Some(f) = &folder {
                    if old_pos.is_none() && project.id_in_use(f.id) {
                        return Err(ModelError::DuplicateId(f.id));
                    }
                    if let Some(c) = clips.iter().find(|c| c.audio.is_none()) {
                        return Err(ModelError::InvalidArgument(format!(
                            "comp clip {} has no audio",
                            c.id
                        )));
                    }
                    for (n, c) in clips.iter().enumerate() {
                        check_clip(c)?;
                        crate::project::check_id(c.id.0)?;
                        if c.id.0 == f.id
                            || in_use(c.id.0)
                            || clips[..n].iter().any(|o| o.id == c.id)
                        {
                            return Err(ModelError::DuplicateId(c.id.0));
                        }
                    }
                } else if old_pos.is_none() {
                    return Err(ModelError::InvalidArgument("take folder not found".into()));
                }
                // All checks passed; mutate.
                let (old_clips, at) = {
                    let t = find(project, track)?;
                    let first = t.clips.iter().position(|c| old_ids.contains(&c.id));
                    let mut removed = Vec::new();
                    t.clips.retain(|c| {
                        if old_ids.contains(&c.id) {
                            removed.push(c.clone());
                            false
                        } else {
                            true
                        }
                    });
                    (removed, first.unwrap_or(t.clips.len()))
                };
                for c in &clips {
                    project.bump_next_id(c.id.0);
                }
                let t = find(project, track)?;
                let old_folder = old_pos.map(|p| t.take_folders.remove(p));
                for (k, c) in clips.into_iter().enumerate() {
                    t.clips.insert((at + k).min(t.clips.len()), c);
                }
                if let Some(f) = folder {
                    project.bump_next_id(f.id);
                    let t = find(project, track)?;
                    let idx = old_pos
                        .unwrap_or(t.take_folders.len())
                        .min(t.take_folders.len());
                    t.take_folders.insert(idx, f);
                }
                Ok(Command::SetTakeFolder {
                    track,
                    folder_id,
                    folder: old_folder,
                    clips: old_clips,
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
                let t = project
                    .track(track)
                    .ok_or(ModelError::TrackNotFound(track))?;
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
            Command::AddPlugin {
                track,
                plugin,
                index,
            } => {
                plugin.check()?;
                crate::project::check_id(plugin.id.0)?;
                let kind = project
                    .track(track)
                    .ok_or(ModelError::TrackNotFound(track))?
                    .kind;
                let allowed = match plugin.role {
                    PluginRole::Instrument => kind == TrackKind::Midi,
                    PluginRole::Effect => kind != TrackKind::Bus,
                };
                if !allowed {
                    return Err(ModelError::InvalidArgument(format!(
                        "track {track} cannot take a plugin {}",
                        match plugin.role {
                            PluginRole::Instrument => "instrument (MIDI tracks only)",
                            PluginRole::Effect => "effect (not on buses)",
                        }
                    )));
                }
                if project.id_in_use(plugin.id.0) {
                    return Err(ModelError::DuplicateId(plugin.id.0));
                }
                let t = find(project, track)?;
                if plugin.role == PluginRole::Instrument
                    && t.plugins.iter().any(|p| p.role == PluginRole::Instrument)
                {
                    return Err(ModelError::InvalidArgument(format!(
                        "track {track} already has a plugin instrument"
                    )));
                }
                let id = plugin.id;
                let at = index
                    .filter(|&i| i <= t.plugins.len())
                    .unwrap_or(t.plugins.len());
                t.plugins.insert(at, plugin);
                project.bump_next_id(id.0);
                Ok(Command::RemovePlugin { track, plugin: id })
            }
            Command::RemovePlugin { track, plugin } => {
                let t = find(project, track)?;
                let at = t
                    .plugins
                    .iter()
                    .position(|p| p.id == plugin)
                    .ok_or(ModelError::InsertNotFound(plugin))?;
                let removed = t.plugins.remove(at);
                Ok(Command::AddPlugin {
                    track,
                    plugin: removed,
                    index: Some(at),
                })
            }
            Command::SetPluginParam {
                track,
                plugin,
                param,
                value,
            } => {
                if !value.is_finite() {
                    return Err(ModelError::InvalidValue {
                        field: "plugin parameter",
                        value,
                    });
                }
                let slot = find_plugin(project, track, plugin)?;
                let before = (slot.state.clone(), slot.params.clone());
                match slot.params.iter_mut().find(|p| p.id == param) {
                    Some(p) => p.value = value,
                    None => {
                        if slot.params.len() >= crate::plugin::MAX_PLUGIN_PARAMS {
                            return Err(ModelError::InvalidArgument(
                                "too many stored plugin parameters".into(),
                            ));
                        }
                        slot.params.push(PluginParam { id: param, value });
                    }
                }
                Ok(Command::SetPluginState {
                    track,
                    plugin,
                    state: before.0,
                    params: before.1,
                })
            }
            Command::SetPluginState {
                track,
                plugin,
                state,
                params,
            } => {
                let mut probe = find_plugin(project, track, plugin)?.clone();
                probe.state = state;
                probe.params = params;
                probe.check()?;
                let slot = find_plugin(project, track, plugin)?;
                let old_state = std::mem::replace(&mut slot.state, probe.state);
                let old_params = std::mem::replace(&mut slot.params, probe.params);
                Ok(Command::SetPluginState {
                    track,
                    plugin,
                    state: old_state,
                    params: old_params,
                })
            }
            Command::SetPluginBypass {
                track,
                plugin,
                bypass,
            } => {
                let slot = find_plugin(project, track, plugin)?;
                let old = std::mem::replace(&mut slot.bypass, bypass);
                Ok(Command::SetPluginBypass {
                    track,
                    plugin,
                    bypass: old,
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
            Command::SetInsertSidechain {
                track,
                insert,
                source,
            } => {
                let t = project
                    .track(track)
                    .ok_or(ModelError::TrackNotFound(track))?;
                let ins = t
                    .inserts
                    .iter()
                    .find(|i| i.id == insert)
                    .ok_or(ModelError::InsertNotFound(insert))?;
                if let Some(src) = source {
                    project.check_sidechain(t, ins, src)?;
                    if project.reaches(track, src) {
                        return Err(ModelError::RoutingCycle {
                            from: src,
                            to: track,
                        });
                    }
                } else if !ins.kind.supports_sidechain() {
                    return Err(ModelError::SidechainNotSupported(insert));
                }
                let t = find(project, track)?;
                let ins = t
                    .inserts
                    .iter_mut()
                    .find(|i| i.id == insert)
                    .ok_or(ModelError::InsertNotFound(insert))?;
                let old = std::mem::replace(&mut ins.sidechain, source);
                Ok(Command::SetInsertSidechain {
                    track,
                    insert,
                    source: old,
                })
            }
            Command::SetAutomation {
                track,
                target,
                mut points,
                mode,
            } => {
                let t = project
                    .track(track)
                    .ok_or(ModelError::TrackNotFound(track))?;
                let range = target.range_in(t)?;
                AutomationLane::normalise(&mut points, &range)?;
                let t = find(project, track)?;
                let blank = points.is_empty() && mode == AutoMode::Read;
                let at = t.automation.iter().position(|l| l.target == target);
                let old = match (at, blank) {
                    (Some(i), true) => t.automation.remove(i),
                    (Some(i), false) => {
                        let lane = &mut t.automation[i];
                        AutomationLane {
                            target,
                            mode: std::mem::replace(&mut lane.mode, mode),
                            points: std::mem::replace(&mut lane.points, points),
                        }
                    }
                    (None, true) => AutomationLane {
                        target,
                        mode: AutoMode::Read,
                        points: Vec::new(),
                    },
                    (None, false) => {
                        t.automation.push(AutomationLane {
                            target,
                            mode,
                            points,
                        });
                        AutomationLane {
                            target,
                            mode: AutoMode::Read,
                            points: Vec::new(),
                        }
                    }
                };
                Ok(Command::SetAutomation {
                    track,
                    target,
                    points: old.points,
                    mode: old.mode,
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

fn find_plugin(
    project: &mut Project,
    track: TrackId,
    plugin: InsertId,
) -> Result<&mut PluginSlot> {
    find(project, track)?
        .plugins
        .iter_mut()
        .find(|p| p.id == plugin)
        .ok_or(ModelError::InsertNotFound(plugin))
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

fn check_range(field: &'static str, value: f64, (lo, hi): (f64, f64)) -> Result<()> {
    if value.is_finite() && (lo..=hi).contains(&value) {
        Ok(())
    } else {
        Err(ModelError::InvalidValue { field, value })
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
    check_range("stretch", c.stretch, crate::STRETCH_RANGE)?;
    check_range("pitch_semitones", c.pitch_semitones, crate::PITCH_RANGE)?;
    if let Some(b) = c.follow_tempo_bpm {
        check_range("follow_tempo_bpm", b, crate::FOLLOW_TEMPO_RANGE)?;
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
