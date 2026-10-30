//! Window-independent app state: everything the UI does goes through here, so it can be
//! tested without a display. The UI only reads [`AppState`] and calls its methods.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::timeline::normalize_gain_db;
use woa_model::{
    AutoMode, AutoPoint, AutoTarget, Clip, ClipId, Command, EffectKind, FadeCurve, Insert,
    InsertId, ModelError, Project, Send, Session, TrackId, TrackKind,
};

/// Subfolder for recorded takes, inside the project or recovery folder.
pub const TAKES_DIR: &str = "aufnahmen";

/// Autosave interval (rule 7).
pub const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(10);

/// One line in the status bar.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    /// Nothing to report.
    Idle,
    /// Informational message.
    Info(String),
    /// Something failed; already logged.
    Error(String),
}

/// The desktop app state around one [`Session`].
pub struct AppState {
    session: Session,
    /// Where unsaved sessions autosave to (recovery folder).
    recovery_dir: PathBuf,
    last_autosave: Instant,
    /// Status bar text.
    pub status: Status,
}

impl AppState {
    /// A fresh, empty project. `recovery_dir` receives autosaves of unsaved projects.
    pub fn new(recovery_dir: PathBuf, now: Instant) -> Self {
        Self {
            session: Session::new("Neues Projekt"),
            recovery_dir,
            last_autosave: now,
            status: Status::Idle,
        }
    }

    /// The current project (read-only).
    pub fn project(&self) -> &Project {
        self.session.project()
    }

    /// The current session (read-only).
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// Window title: project name, folder, dirty marker.
    pub fn title(&self) -> String {
        let dirty = if self.session.is_dirty() { " *" } else { "" };
        format!("WOA – {}{}", self.project().meta.name, dirty)
    }

    fn report(&mut self, what: &str, result: Result<(), ModelError>) -> bool {
        match result {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!(error = %e, "{what} failed");
                self.status = Status::Error(format!("{what}: {e}"));
                false
            }
        }
    }

    fn apply(&mut self, command: Command) -> bool {
        let label = command.label();
        let result = self.session.apply(command);
        self.report(label, result)
    }

    /// Appends an audio track named "Spur N".
    pub fn add_track(&mut self) -> Option<TrackId> {
        let id = self.session.new_track_id();
        let name = format!("Spur {}", self.project().tracks.len() + 1);
        self.apply(Command::add_track(id, name, TrackKind::Audio))
            .then_some(id)
    }

    /// Appends a MIDI track named "MIDI N" (Paket 11: plays the keyboard when armed).
    pub fn add_midi_track(&mut self) -> Option<TrackId> {
        let id = self.session.new_track_id();
        let n = self
            .project()
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Midi)
            .count();
        let name = format!("MIDI {}", n + 1);
        self.apply(Command::add_track(id, name, TrackKind::Midi))
            .then_some(id)
    }

    /// Renames a track; empty or unchanged names are ignored.
    pub fn rename_track(&mut self, track: TrackId, name: &str) -> bool {
        let name = name.trim();
        let unchanged = self.project().track(track).is_some_and(|t| t.name == name);
        if name.is_empty() || unchanged {
            return false;
        }
        self.apply(Command::RenameTrack {
            track,
            name: name.to_string(),
        })
    }

    /// Sets the gain in dB (one undo step; the UI calls this when a drag ends).
    pub fn set_gain(&mut self, track: TrackId, gain_db: f64) -> bool {
        let unchanged = self
            .project()
            .track(track)
            .is_some_and(|t| t.gain_db == gain_db);
        if unchanged {
            return false;
        }
        self.apply(Command::SetGain { track, gain_db })
    }

    /// Toggles mute.
    pub fn toggle_mute(&mut self, track: TrackId) -> bool {
        let Some(mute) = self.project().track(track).map(|t| !t.mute) else {
            return false;
        };
        self.apply(Command::SetMute { track, mute })
    }

    /// Sets the pan (one undo step; the controller calls this when the knob rests).
    pub fn set_pan(&mut self, track: TrackId, pan: f64) -> bool {
        let unchanged = self.project().track(track).is_some_and(|t| t.pan == pan);
        if unchanged {
            return false;
        }
        self.apply(Command::SetPan { track, pan })
    }

    /// Toggles solo.
    pub fn toggle_solo(&mut self, track: TrackId) -> bool {
        let Some(solo) = self.project().track(track).map(|t| !t.solo) else {
            return false;
        };
        self.apply(Command::SetSolo { track, solo })
    }

    /// Removes a track with all its clips as one undo step.
    /// Sends and outputs that lead to the track are cut in the same step.
    pub fn remove_track(&mut self, track: TrackId) -> bool {
        let r = self.session.remove_track_routed(track);
        self.report("Remove track", r)
    }

    /// Appends a bus named `name` (empty: "Bus N").
    pub fn add_bus(&mut self, name: &str) -> Option<TrackId> {
        let id = self.session.new_track_id();
        let name = name.trim();
        let name = if name.is_empty() {
            let n = self
                .project()
                .tracks
                .iter()
                .filter(|t| t.kind == TrackKind::Bus)
                .count();
            format!("Bus {}", n + 1)
        } else {
            name.to_string()
        };
        self.apply(Command::add_track(id, name, TrackKind::Bus))
            .then_some(id)
    }

    /// Routes a track to a bus (`None`: master). A loop is refused and shown in the status bar.
    pub fn set_output(&mut self, track: TrackId, output: Option<TrackId>) -> bool {
        let unchanged = self
            .project()
            .track(track)
            .is_some_and(|t| t.output == output);
        if unchanged {
            return false;
        }
        self.apply(Command::SetOutput { track, output })
    }

    /// Creates the send to `target` or changes its level and tap point.
    pub fn set_send(
        &mut self,
        track: TrackId,
        target: TrackId,
        level_db: f64,
        pre_fader: bool,
    ) -> bool {
        let existing = self
            .project()
            .track(track)
            .and_then(|t| t.sends.iter().find(|s| s.target == target))
            .map(|s| (s.level_db, s.pre_fader));
        match existing {
            Some(cur) if cur == (level_db, pre_fader) => false,
            Some(_) => self.apply(Command::SetSend {
                track,
                target,
                level_db,
                pre_fader,
            }),
            None => self.apply(Command::AddSend {
                track,
                send: Send {
                    target,
                    level_db,
                    pre_fader,
                },
                index: None,
            }),
        }
    }

    /// Removes the send to `target`.
    pub fn remove_send(&mut self, track: TrackId, target: TrackId) -> bool {
        self.apply(Command::RemoveSend { track, target })
    }

    /// Appends an effect with default values to the track's chain.
    pub fn add_insert(&mut self, track: TrackId, kind: EffectKind) -> Option<InsertId> {
        let id = self.session.new_insert_id();
        self.apply(Command::AddInsert {
            track,
            insert: Insert::new(id, kind),
            index: None,
        })
        .then_some(id)
    }

    /// Removes an effect.
    pub fn remove_insert(&mut self, track: TrackId, insert: InsertId) -> bool {
        self.apply(Command::RemoveInsert { track, insert })
    }

    /// Switches an effect's bypass.
    pub fn toggle_insert_bypass(&mut self, track: TrackId, insert: InsertId) -> bool {
        let Some(bypass) = self
            .project()
            .track(track)
            .and_then(|t| t.inserts.iter().find(|i| i.id == insert))
            .map(|i| !i.bypass)
        else {
            return false;
        };
        self.apply(Command::SetInsertBypass {
            track,
            insert,
            bypass,
        })
    }

    /// Sets one effect parameter (one undo step; the UI calls this when a drag ends).
    pub fn set_insert_param(
        &mut self,
        track: TrackId,
        insert: InsertId,
        param: usize,
        value: f64,
    ) -> bool {
        let unchanged = self
            .project()
            .track(track)
            .and_then(|t| t.inserts.iter().find(|i| i.id == insert))
            .is_some_and(|i| i.params.get(param) == Some(&value));
        if unchanged {
            return false;
        }
        self.apply(Command::SetInsertParam {
            track,
            insert,
            param,
            value,
        })
    }

    /// Replaces the automation lane of one target (one undo step); `false` if nothing changed.
    pub fn set_automation(
        &mut self,
        track: TrackId,
        target: AutoTarget,
        points: Vec<AutoPoint>,
        mode: AutoMode,
    ) -> bool {
        let unchanged = self
            .project()
            .track(track)
            .and_then(|t| t.lane(&target))
            .map_or(points.is_empty() && mode == AutoMode::Read, |l| {
                l.points == points && l.mode == mode
            });
        if unchanged {
            return false;
        }
        self.apply(Command::SetAutomation {
            track,
            target,
            points,
            mode,
        })
    }

    /// Lets a compressor or gate measure its level from `source` (`None`: its own signal).
    pub fn set_sidechain(
        &mut self,
        track: TrackId,
        insert: InsertId,
        source: Option<TrackId>,
    ) -> bool {
        let unchanged = self
            .project()
            .track(track)
            .and_then(|t| t.inserts.iter().find(|i| i.id == insert))
            .is_some_and(|i| i.sidechain == source);
        if unchanged {
            return false;
        }
        self.apply(Command::SetInsertSidechain {
            track,
            insert,
            source,
        })
    }

    /// Sets all parameters of an effect to a preset (one undo step); `false` if nothing changed.
    pub fn apply_insert_preset(&mut self, track: TrackId, insert: InsertId, preset: &str) -> bool {
        let Some((kind, current)) = self
            .project()
            .track(track)
            .and_then(|t| t.inserts.iter().find(|i| i.id == insert))
            .map(|i| (i.kind, i.params.clone()))
        else {
            return false;
        };
        let Some(values) = kind.preset_values(preset) else {
            return false;
        };
        let changes: Vec<(usize, f64)> = values
            .iter()
            .enumerate()
            .filter(|(i, v)| current.get(*i) != Some(*v))
            .map(|(i, v)| (i, *v))
            .collect();
        if changes.is_empty() {
            return false;
        }
        let result = self.session.group("Effekt-Preset", |s| {
            for (param, value) in changes {
                s.apply(Command::SetInsertParam {
                    track,
                    insert,
                    param,
                    value,
                })?;
            }
            Ok(())
        });
        self.report("Effekt-Preset", result)
    }

    /// Appends the template "Sprachkette" (highpass, gate, EQ, compressor, de-esser) to the
    /// chain of a track as one undo step.
    pub fn add_voice_chain(&mut self, track: TrackId) -> bool {
        let result = self.session.group("Vorlage Sprachkette", |s| {
            for (kind, values) in woa_model::VOICE_CHAIN {
                let id = s.new_insert_id();
                s.apply(Command::AddInsert {
                    track,
                    insert: Insert::with_values(id, kind, values)?,
                    index: None,
                })?;
            }
            Ok(())
        });
        self.report("Vorlage Sprachkette", result)
    }

    /// Sets the project tempo in BPM (20..=400) as one undo step.
    pub fn set_tempo(&mut self, bpm: f64) -> bool {
        if (self.project().tempo_bpm - bpm).abs() < 1e-6 {
            return false;
        }
        self.apply(Command::SetTempo { bpm })
    }

    /// Creates an empty MIDI clip for composing in the Piano-Roll.
    pub fn create_empty_midi_clip(
        &mut self,
        track: Option<TrackId>,
        start_beats: f64,
        length_beats: f64,
    ) -> Option<ClipId> {
        self.add_midi_clip(
            track,
            "MIDI-Clip",
            start_beats,
            length_beats.max(1.0),
            woa_model::MidiData::default(),
        )
    }

    /// Where new takes are written: the project folder, or the recovery folder while unsaved.
    pub fn takes_dir(&self) -> PathBuf {
        self.session
            .path()
            .unwrap_or(&self.recovery_dir)
            .join(TAKES_DIR)
    }

    /// Resolves a clip's audio path (relative to the project): project folder first,
    /// then the recovery folder (takes of a project that was not saved yet).
    pub fn resolve_audio(&self, rel: &str) -> Option<PathBuf> {
        if !woa_model::is_safe_audio_path(rel) {
            return None;
        }
        self.session
            .path()
            .map(|p| p.join(rel))
            .into_iter()
            .chain(std::iter::once(self.recovery_dir.join(rel)))
            .find(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_file()))
    }

    /// Puts a recorded take on `track` (or on a new audio track) as one undo step.
    /// `file` must lie in [`AppState::takes_dir`].
    pub fn add_take(
        &mut self,
        track: Option<TrackId>,
        file: &Path,
        start_beats: f64,
        seconds: f64,
        latency_seconds: f64,
    ) -> Option<ClipId> {
        let name = file.file_stem()?.to_string_lossy().into_owned();
        let audio = format!("{TAKES_DIR}/{}", file.file_name()?.to_string_lossy());
        // Latenzausgleich: die ersten `latency` Sekunden der Datei gehören vor den Startpunkt,
        // also überspringen (Versatz) statt den Clip zu verschieben.
        let beats_per_s = self.project().tempo_bpm / 60.0;
        let latency = latency_seconds.clamp(0.0, (seconds - 1e-3).max(0.0));
        let offset_beats = latency * beats_per_s;
        let length_beats = ((seconds - latency) * beats_per_s).max(1e-3);
        let target = track.filter(|t| {
            self.project()
                .track(*t)
                .is_some_and(|t| t.kind == TrackKind::Audio)
        });
        self.session.begin_group("Aufnahme");
        let track = match target {
            Some(t) => Some(t),
            None => self.add_track(),
        };
        let id = self.session.new_clip_id();
        let ok = track.is_some_and(|track| {
            self.apply(Command::AddClip {
                track,
                clip: Clip {
                    id,
                    name,
                    start_beats: start_beats.max(0.0),
                    length_beats,
                    audio: Some(audio),
                    offset_beats,
                    fade_in_beats: 0.0,
                    fade_out_beats: 0.0,
                    midi: None,
                    fade_in_curve: woa_model::FadeCurve::Linear,
                    fade_out_curve: woa_model::FadeCurve::Linear,
                    gain_db: 0.0,
                },
                index: None,
            })
        });
        let closed = self.session.end_group();
        self.report("Aufnahme", closed);
        ok.then_some(id)
    }

    /// Puts recorded or imported notes as a MIDI clip on `track` (or on a new MIDI
    /// track) as one undo step (Paket 12).
    pub fn add_midi_clip(
        &mut self,
        track: Option<TrackId>,
        name: &str,
        start_beats: f64,
        length_beats: f64,
        midi: woa_model::MidiData,
    ) -> Option<ClipId> {
        let target = track.filter(|t| {
            self.project()
                .track(*t)
                .is_some_and(|t| t.kind == TrackKind::Midi)
        });
        self.session.begin_group("MIDI-Aufnahme");
        let track = match target {
            Some(t) => Some(t),
            None => {
                let id = self.session.new_track_id();
                let name = format!("MIDI {}", self.project().tracks.len() + 1);
                self.apply(Command::add_track(id, name, TrackKind::Midi))
                    .then_some(id)
            }
        };
        let id = self.session.new_clip_id();
        let ok = track.is_some_and(|track| {
            self.apply(Command::AddClip {
                track,
                clip: Clip::new_midi(id, name, start_beats.max(0.0), length_beats.max(1e-3), midi),
                index: None,
            })
        });
        let closed = self.session.end_group();
        self.report("MIDI-Aufnahme", closed);
        ok.then_some(id)
    }

    /// Moves a clip on its track to `start_beats` (clamped to >= 0).
    pub fn move_clip(&mut self, clip: ClipId, start_beats: f64) -> bool {
        self.move_clip_to(clip, None, start_beats)
    }

    /// Moves a clip to `start_beats` on `to_track` (`None`: same track).
    pub fn move_clip_to(
        &mut self,
        clip: ClipId,
        to_track: Option<TrackId>,
        start_beats: f64,
    ) -> bool {
        let Some((track, c)) = self.project().clip(clip) else {
            return false;
        };
        let (from, old) = (track.id, c.start_beats);
        let to_track = to_track.unwrap_or(from);
        let start_beats = start_beats.max(0.0);
        if to_track == from && (start_beats - old).abs() < 1e-9 {
            return false;
        }
        self.apply(Command::MoveClip {
            clip,
            to_track,
            start_beats,
            index: None,
        })
    }

    /// Trims a clip: new start, length and audio offset in one undo step.
    /// Fades are shortened to fit the new length.
    pub fn trim_clip(&mut self, clip: ClipId, start: f64, length: f64, offset: f64) -> bool {
        let Some((track, c)) = self.project().clip(clip) else {
            return false;
        };
        let (track, c) = (track.id, c.clone());
        let (start, offset) = (start.max(0.0), offset.max(0.0));
        if length < 1e-3
            || ((start - c.start_beats).abs() < 1e-9
                && (length - c.length_beats).abs() < 1e-9
                && (offset - c.offset_beats).abs() < 1e-9)
        {
            return false;
        }
        let fade_in = c.fade_in_beats.min(length);
        let fade_out = c.fade_out_beats.min(length - fade_in);
        self.session.begin_group("Trimmen");
        let mut ok = true;
        if (start - c.start_beats).abs() > 1e-9 {
            ok = self.apply(Command::MoveClip {
                clip,
                to_track: track,
                start_beats: start,
                index: None,
            });
        }
        ok = ok
            && self.apply(Command::EditClip {
                clip,
                length_beats: length,
                offset_beats: offset,
                fade_in_beats: fade_in,
                fade_out_beats: fade_out,
            });
        let closed = self.session.end_group();
        self.report("Trimmen", closed);
        ok
    }

    /// Sets the fades of a clip; too long fades are shortened to fit the clip.
    pub fn set_fades(&mut self, clip: ClipId, fade_in: f64, fade_out: f64) -> bool {
        let Some((_, c)) = self.project().clip(clip) else {
            return false;
        };
        let len = c.length_beats;
        let fade_in = fade_in.clamp(0.0, len);
        let fade_out = fade_out.clamp(0.0, len - fade_in);
        if fade_in == c.fade_in_beats && fade_out == c.fade_out_beats {
            return false;
        }
        let (length_beats, offset_beats) = (c.length_beats, c.offset_beats);
        self.apply(Command::EditClip {
            clip,
            length_beats,
            offset_beats,
            fade_in_beats: fade_in,
            fade_out_beats: fade_out,
        })
    }

    /// Sets the shape of both fades of a clip (Paket 21), one undo step.
    pub fn set_fade_curves(
        &mut self,
        clip: ClipId,
        fade_in: FadeCurve,
        fade_out: FadeCurve,
    ) -> bool {
        let Some((_, c)) = self.project().clip(clip) else {
            return false;
        };
        if c.fade_in_curve == fade_in && c.fade_out_curve == fade_out {
            return false;
        }
        self.apply(Command::SetClipFadeCurves {
            clip,
            fade_in,
            fade_out,
        })
    }

    /// Sets the gain of a clip in dB (Paket 21), clamped to the gain range, one undo step.
    pub fn set_clip_gain(&mut self, clip: ClipId, gain_db: f64) -> bool {
        let Some((_, c)) = self.project().clip(clip) else {
            return false;
        };
        let gain_db = gain_db.clamp(woa_model::MIN_GAIN_DB, woa_model::MAX_GAIN_DB);
        if gain_db == c.gain_db {
            return false;
        }
        self.apply(Command::SetClipGain { clip, gain_db })
    }

    /// Sets the clip gain so that the loudest sample (`peak`, linear, measured with the
    /// current gain NOT applied) reaches `target_db` dBFS. `false` for silence.
    pub fn normalize_clip(&mut self, clip: ClipId, peak: f32, target_db: f64) -> bool {
        match normalize_gain_db(peak, target_db) {
            Some(g) => self.set_clip_gain(clip, g),
            None => false,
        }
    }

    /// Splits a clip at `at_beats` (timeline) into two clips, one undo step.
    /// Returns the id of the right part; `None` if `at_beats` is not inside the clip.
    pub fn split_clip(&mut self, clip: ClipId, at_beats: f64) -> Option<ClipId> {
        let (track, c) = self.project().clip(clip)?;
        let (track, c) = (track.id, c.clone());
        let left = at_beats - c.start_beats;
        let right = c.end_beats() - at_beats;
        if left < 1e-3 || right < 1e-3 {
            return None;
        }
        let index = self
            .project()
            .track(track)?
            .clips
            .iter()
            .position(|x| x.id == clip)
            .map(|i| i + 1);
        let id = self.session.new_clip_id();
        let mut part = c.clone();
        part.id = id;
        part.start_beats = at_beats;
        part.length_beats = right;
        part.offset_beats = c.offset_beats + left;
        part.fade_in_beats = 0.0;
        part.fade_out_beats = c.fade_out_beats.min(right);
        self.session.begin_group("Schneiden");
        let ok = self.apply(Command::EditClip {
            clip,
            length_beats: left,
            offset_beats: c.offset_beats,
            fade_in_beats: c.fade_in_beats.min(left),
            fade_out_beats: 0.0,
        }) && self.apply(Command::AddClip {
            track,
            clip: part,
            index,
        });
        let closed = self.session.end_group();
        self.report("Schneiden", closed);
        ok.then_some(id)
    }

    /// Removes several clips as one undo step.
    pub fn remove_clips(&mut self, clips: &[ClipId]) -> bool {
        self.session.begin_group("Clips löschen");
        let ok = clips
            .iter()
            .all(|&c| self.apply(Command::RemoveClip { clip: c }));
        let closed = self.session.end_group();
        self.report("Clips löschen", closed);
        ok
    }

    /// Copies clips of the project for the clipboard (sorted by start).
    pub fn copy_clips(&self, ids: &[ClipId]) -> Vec<(TrackId, Clip)> {
        let mut out: Vec<(TrackId, Clip)> = ids
            .iter()
            .filter_map(|&id| self.project().clip(id).map(|(t, c)| (t.id, c.clone())))
            .collect();
        out.sort_by(|a, b| a.1.start_beats.total_cmp(&b.1.start_beats));
        out
    }

    /// Pastes copies of `items` so the earliest one starts at `at_beats`, one undo step.
    /// Every copy gets a fresh id. With `target` all clips land on that track if their kind
    /// fits (audio on audio, MIDI on MIDI); otherwise each goes back to its own track and
    /// clips whose track is gone are skipped. Returns the new ids (empty: nothing pasted).
    pub fn paste_clips(
        &mut self,
        items: &[(TrackId, Clip)],
        at_beats: f64,
        target: Option<TrackId>,
    ) -> Vec<ClipId> {
        let Some(first) = items.iter().map(|(_, c)| c.start_beats).reduce(f64::min) else {
            return Vec::new();
        };
        let delta = at_beats.max(0.0) - first;
        self.session.begin_group("Einfügen");
        let mut created = Vec::new();
        let mut ok = true;
        for (orig, clip) in items {
            let wants_midi = clip.midi.is_some();
            let fits = |p: &Project, t: TrackId| {
                p.track(t).is_some_and(|t| match t.kind {
                    TrackKind::Midi => wants_midi,
                    TrackKind::Audio => !wants_midi,
                    TrackKind::Bus => false,
                })
            };
            let dest = target
                .filter(|t| fits(self.project(), *t))
                .or_else(|| fits(self.project(), *orig).then_some(*orig));
            let Some(dest) = dest else { continue };
            let id = self.session.new_clip_id();
            let mut copy = clip.clone();
            copy.id = id;
            copy.start_beats = (clip.start_beats + delta).max(0.0);
            ok &= self.apply(Command::AddClip {
                track: dest,
                clip: copy,
                index: None,
            });
            created.push(id);
        }
        let closed = self.session.end_group();
        self.report("Einfügen", closed);
        if ok { created } else { Vec::new() }
    }

    /// Duplicates clips directly behind the selection (Strg+D), one undo step.
    pub fn duplicate_clips(&mut self, ids: &[ClipId]) -> Vec<ClipId> {
        let items = self.copy_clips(ids);
        let Some(first) = items.first().map(|(_, c)| c.start_beats) else {
            return Vec::new();
        };
        let end = items.iter().map(|(_, c)| c.end_beats()).fold(0.0, f64::max);
        self.paste_clips(&items, end.max(first), None)
    }

    /// Moves several clips on their tracks by `delta_beats`, one undo step.
    /// Nothing moves if one of them would start before 0.
    pub fn shift_clips(&mut self, clips: &[ClipId], delta_beats: f64) -> bool {
        let moves: Vec<(ClipId, TrackId, f64)> = clips
            .iter()
            .filter_map(|&c| {
                let (t, clip) = self.project().clip(c)?;
                Some((c, t.id, clip.start_beats + delta_beats))
            })
            .collect();
        if moves.is_empty() || delta_beats.abs() < 1e-9 || moves.iter().any(|m| m.2 < 0.0) {
            return false;
        }
        self.session.begin_group("Clips verschieben");
        let ok = moves.iter().all(|&(clip, to_track, start_beats)| {
            self.apply(Command::MoveClip {
                clip,
                to_track,
                start_beats,
                index: None,
            })
        });
        let closed = self.session.end_group();
        self.report("Clips verschieben", closed);
        ok
    }

    /// Removes a clip (undoable). The audio file stays on disk.
    pub fn remove_clip(&mut self, clip: ClipId) -> bool {
        self.apply(Command::RemoveClip { clip })
    }

    /// Replaces the notes of a MIDI clip (Piano-Roll), one undo step. Unchanged content
    /// is not applied, so a click without effect leaves the undo list alone.
    pub fn set_notes(&mut self, clip: ClipId, midi: woa_model::MidiData) -> bool {
        let same = self
            .project()
            .clip(clip)
            .and_then(|(_, c)| c.midi.as_ref())
            .is_some_and(|m| *m == midi);
        if same {
            return false;
        }
        self.apply(Command::SetNotes { clip, midi })
    }

    /// Undo; "nothing to undo" is not an error for the user.
    pub fn undo(&mut self) {
        match self.session.undo() {
            Ok(()) | Err(ModelError::NothingToUndo) => {}
            Err(e) => {
                self.report("Rückgängig", Err(e));
            }
        }
    }

    /// Redo; "nothing to redo" is not an error for the user.
    pub fn redo(&mut self) {
        match self.session.redo() {
            Ok(()) | Err(ModelError::NothingToRedo) => {}
            Err(e) => {
                self.report("Wiederholen", Err(e));
            }
        }
    }

    /// Starts a new, empty project.
    pub fn new_project(&mut self) {
        self.session = Session::new("Neues Projekt");
        self.status = Status::Info("Neues Projekt".into());
    }

    /// Opens a project folder.
    pub fn open(&mut self, dir: &Path) -> bool {
        match Session::open(dir) {
            Ok(session) => {
                self.session = session;
                self.status = Status::Info(format!("Geöffnet: {}", dir.display()));
                true
            }
            Err(e) => self.report("Öffnen", Err(e)),
        }
    }

    /// Saves to the current folder; `false` with [`ModelError::NoPath`] means the UI
    /// must ask for a folder ([`AppState::needs_save_as`]).
    pub fn save(&mut self) -> bool {
        let result = self.session.save();
        if self.report("Speichern", result) {
            self.status = Status::Info("Gespeichert".into());
            true
        } else {
            false
        }
    }

    /// Whether "save" must first ask for a folder.
    pub fn needs_save_as(&self) -> bool {
        self.session.path().is_none()
    }

    /// Saves into `dir` (appends `.woa` if missing) and adopts it as project folder.
    pub fn save_as(&mut self, dir: &Path) -> bool {
        let dir = with_woa_extension(dir);
        if let Err(e) = self.copy_takes(&dir) {
            tracing::error!(error = %e, "copying takes failed");
            self.status = Status::Error(format!("Aufnahmen kopieren: {e}"));
            return false;
        }
        let result = self.session.save_as(&dir);
        if self.report("Speichern", result) {
            self.status = Status::Info(format!("Gespeichert: {}", dir.display()));
            true
        } else {
            false
        }
    }

    /// Autosave timer hook: call every frame. Writes at most every
    /// [`AUTOSAVE_INTERVAL`], and only if something changed. Returns whether a file was
    /// written. Unsaved projects go to the recovery folder.
    pub fn tick_autosave(&mut self, now: Instant) -> bool {
        if now.duration_since(self.last_autosave) < AUTOSAVE_INTERVAL {
            return false;
        }
        self.last_autosave = now;
        let result = match self.session.path() {
            Some(_) => self.session.autosave(),
            None => self.session.autosave_to(&self.recovery_dir),
        };
        match result {
            Ok(written) => written,
            Err(e) => {
                self.report("Autosave", Err(e));
                false
            }
        }
    }
}

impl AppState {
    /// Copies every take the project uses into `dir`, so the new folder is complete.
    fn copy_takes(&self, dir: &Path) -> std::io::Result<()> {
        for clip in self.project().tracks.iter().flat_map(|t| &t.clips) {
            let Some(rel) = &clip.audio else { continue };
            let Some(src) = self.resolve_audio(rel) else {
                continue;
            };
            let dst = dir.join(rel);
            if dst == src || dst.exists() {
                continue;
            }
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&src, &dst)?;
        }
        Ok(())
    }
}

fn with_woa_extension(dir: &Path) -> PathBuf {
    if dir.extension().is_some_and(|e| e == "woa") {
        dir.to_path_buf()
    } else {
        let mut s = dir.as_os_str().to_owned();
        s.push(".woa");
        PathBuf::from(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(dir: &Path) -> (AppState, Instant) {
        let now = Instant::now();
        (AppState::new(dir.join("recovery"), now), now)
    }

    #[test]
    fn add_rename_gain_mute_with_undo_redo() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _) = state(tmp.path());
        let id = s.add_track().unwrap();
        assert_eq!(s.project().tracks[0].name, "Spur 1");
        assert!(s.rename_track(id, " Moderation "));
        assert!(!s.rename_track(id, "   "));
        assert!(!s.rename_track(id, "Moderation"));
        assert!(s.set_gain(id, -6.0));
        assert!(!s.set_gain(id, -6.0));
        assert!(s.toggle_mute(id));
        let t = &s.project().tracks[0];
        assert_eq!(
            (t.name.as_str(), t.gain_db, t.mute),
            ("Moderation", -6.0, true)
        );

        s.undo();
        assert!(!s.project().tracks[0].mute);
        s.undo();
        assert_eq!(s.project().tracks[0].gain_db, 0.0);
        s.redo();
        assert_eq!(s.project().tracks[0].gain_db, -6.0);
        s.undo();
        s.undo();
        s.undo();
        assert!(s.project().tracks.is_empty());
        s.undo(); // nothing left: no error shown
        assert_eq!(s.status, Status::Idle);
    }

    #[test]
    fn invalid_gain_shows_error_and_changes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _) = state(tmp.path());
        let id = s.add_track().unwrap();
        assert!(!s.set_gain(id, 1000.0));
        assert!(matches!(s.status, Status::Error(_)));
        assert_eq!(s.project().tracks[0].gain_db, 0.0);
    }

    #[test]
    fn save_as_open_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _) = state(tmp.path());
        assert!(s.needs_save_as());
        let id = s.add_track().unwrap();
        s.rename_track(id, "Musik");
        assert!(s.save_as(&tmp.path().join("show")));
        assert!(!s.needs_save_as());
        assert!(!s.session().is_dirty());
        assert!(tmp.path().join("show.woa").is_dir());

        s.new_project();
        assert!(s.project().tracks.is_empty());
        assert!(s.open(&tmp.path().join("show.woa")));
        assert_eq!(s.project().tracks[0].name, "Musik");
        s.toggle_mute(id);
        assert!(s.save());
        assert!(!s.open(&tmp.path().join("missing.woa")));
        assert!(matches!(s.status, Status::Error(_)));
    }

    #[test]
    fn autosave_every_ten_seconds_only_when_changed() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, t0) = state(tmp.path());
        s.add_track();
        assert!(!s.tick_autosave(t0 + Duration::from_secs(5)));
        assert!(s.tick_autosave(t0 + Duration::from_secs(10)));
        assert!(tmp.path().join("recovery").is_dir());
        // nothing changed since
        assert!(!s.tick_autosave(t0 + Duration::from_secs(21)));
        s.add_track();
        assert!(!s.tick_autosave(t0 + Duration::from_secs(25)));
        assert!(s.tick_autosave(t0 + Duration::from_secs(31)));

        // saved project autosaves into its own folder
        s.save_as(&tmp.path().join("p"));
        s.add_track();
        assert!(s.tick_autosave(t0 + Duration::from_secs(45)));
        assert!(
            woa_model::store::load_autosave(&tmp.path().join("p.woa"))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn take_lands_on_track_and_survives_save_as() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _) = state(&tmp.path().join("rec"));
        let dir = s.takes_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("aufnahme-001.wav");
        std::fs::write(&file, b"RIFF").unwrap();
        let clip = s.add_take(None, &file, 4.0, 2.0, 0.0).unwrap();
        assert_eq!(s.project().tracks.len(), 1);
        let (_, c) = s.project().clip(clip).unwrap();
        assert_eq!(c.audio.as_deref(), Some("aufnahmen/aufnahme-001.wav"));
        assert!(
            (c.length_beats - 4.0).abs() < 1e-9,
            "2 s at 120 bpm = 4 beats"
        );
        assert!(s.resolve_audio("aufnahmen/aufnahme-001.wav").is_some());
        assert!(s.resolve_audio("../x.wav").is_none());
        assert!(s.save_as(&tmp.path().join("show")));
        assert!(
            tmp.path()
                .join("show.woa/aufnahmen/aufnahme-001.wav")
                .is_file()
        );
        s.undo();
        assert!(
            s.project().tracks.is_empty(),
            "take + new track = one undo step"
        );
    }

    #[test]
    fn move_split_fade_with_undo() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _) = state(tmp.path());
        let dir = s.takes_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("aufnahme-001.wav");
        std::fs::write(&file, b"RIFF").unwrap();
        let a = s.add_take(None, &file, 0.0, 4.0, 0.0).unwrap(); // 8 beats
        assert!(s.move_clip(a, 2.0));
        let b = s.split_clip(a, 6.0).unwrap();
        let (_, ca) = s.project().clip(a).unwrap();
        let (_, cb) = s.project().clip(b).unwrap();
        assert_eq!((ca.start_beats, ca.length_beats), (2.0, 4.0));
        assert_eq!(
            (cb.start_beats, cb.length_beats, cb.offset_beats),
            (6.0, 4.0, 4.0)
        );
        assert!(s.split_clip(a, 1.0).is_none(), "outside the clip");
        assert!(s.set_fades(b, 1.0, 9.0));
        let (_, cb) = s.project().clip(b).unwrap();
        assert_eq!((cb.fade_in_beats, cb.fade_out_beats), (1.0, 3.0), "clamped");
        // Trim the left part from the left, move the right part to a new track.
        assert!(s.trim_clip(a, 3.0, 3.0, 1.0));
        let (_, ca) = s.project().clip(a).unwrap();
        assert_eq!(
            (ca.start_beats, ca.length_beats, ca.offset_beats),
            (3.0, 3.0, 1.0)
        );
        let t2 = s.add_track().unwrap();
        assert!(s.move_clip_to(b, Some(t2), 8.0));
        assert_eq!(s.project().clip(b).unwrap().0.id, t2);
        s.undo();
        s.undo();
        s.undo();
        s.undo();
        s.undo();
        assert!(s.project().clip(b).is_none(), "split is one undo step");
        assert_eq!(s.project().clip(a).unwrap().1.length_beats, 8.0);
    }

    #[test]
    fn latency_multi_shift_and_delete() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _) = state(tmp.path());
        let dir = s.takes_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("aufnahme-001.wav");
        std::fs::write(&file, b"RIFF").unwrap();
        // 120 BPM: 0.25 s Latenz = 0.5 Beats Versatz.
        let a = s.add_take(None, &file, 0.0, 4.0, 0.25).unwrap();
        let (_, ca) = s.project().clip(a).unwrap();
        assert_eq!((ca.offset_beats, ca.length_beats), (0.5, 7.5));
        let b = s.add_take(None, &file, 10.0, 1.0, 0.0).unwrap();
        assert!(!s.shift_clips(&[a, b], -1.0), "a would start before 0");
        assert!(s.shift_clips(&[a, b], 2.0));
        assert_eq!(s.project().clip(b).unwrap().1.start_beats, 12.0);
        assert!(s.remove_clips(&[a, b]));
        assert!(s.project().clip(a).is_none() && s.project().clip(b).is_none());
        s.undo();
        assert!(s.project().clip(a).is_some() && s.project().clip(b).is_some());
        s.undo();
        assert_eq!(s.project().clip(a).unwrap().1.start_beats, 0.0);
    }

    #[test]
    fn effect_presets_and_voice_chain_are_single_undo_steps() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _) = state(tmp.path());
        let t = s.add_track().unwrap();
        // Jede Art lässt sich einfügen; jede Voreinstellung setzt gültige Werte.
        for kind in EffectKind::ALL {
            let id = s.add_insert(t, kind).unwrap();
            for pr in kind.presets() {
                s.apply_insert_preset(t, id, pr.name);
                let ins = s
                    .project()
                    .track(t)
                    .unwrap()
                    .inserts
                    .last()
                    .unwrap()
                    .clone();
                assert_eq!(ins.params, kind.preset_values(pr.name).unwrap(), "{kind:?}");
            }
        }
        let before = s.project().track(t).unwrap().inserts.len();
        assert!(
            !s.apply_insert_preset(t, InsertId(9999), "x"),
            "unknown insert"
        );
        // Preset in einem Schritt zurück.
        let id = s.project().track(t).unwrap().inserts[0].id;
        let first = EffectKind::Eq.presets()[0].name;
        let last = EffectKind::Eq.presets().last().unwrap().name;
        s.apply_insert_preset(t, id, first);
        let snapshot = s.project().track(t).unwrap().inserts[0].params.clone();
        if first != last {
            assert!(s.apply_insert_preset(t, id, last));
            s.undo();
            assert_eq!(s.project().track(t).unwrap().inserts[0].params, snapshot);
        }
        // Sprachkette: fünf Effekte, ein Undo.
        assert!(s.add_voice_chain(t));
        assert_eq!(s.project().track(t).unwrap().inserts.len(), before + 5);
        s.undo();
        assert_eq!(s.project().track(t).unwrap().inserts.len(), before);
    }

    #[test]
    fn mixer_clicklist_bus_send_insert_undo_and_save() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _) = state(tmp.path());
        let t = s.add_track().unwrap();
        let bus = s.add_bus("Hall").unwrap();
        assert_eq!(s.project().track(bus).unwrap().name, "Hall");
        assert!(s.set_send(t, bus, -6.0, false));
        assert!(!s.set_send(t, bus, -6.0, false));
        let eq = s.add_insert(t, EffectKind::Eq).unwrap();
        assert!(s.set_insert_param(t, eq, 2, 3.0));
        assert!(s.toggle_insert_bypass(t, eq));
        // A bus onto itself is refused and nothing changes.
        assert!(!s.set_output(bus, Some(bus)));
        // Each routing command undoes on its own.
        s.undo();
        assert!(!s.project().track(t).unwrap().inserts[0].bypass);
        s.undo();
        s.undo();
        assert!(s.project().track(t).unwrap().inserts.is_empty());
        s.undo();
        assert!(s.project().track(t).unwrap().sends.is_empty());
        s.redo();
        s.redo();
        s.redo();
        s.redo();
        let tr = s.project().track(t).unwrap();
        assert_eq!(tr.sends.len(), 1);
        assert_eq!(tr.inserts[0].params[2], 3.0);
        assert!(tr.inserts[0].bypass);
        // Deleting the bus cuts the send in the same step; undo brings both back.
        assert!(s.remove_track(bus));
        assert!(s.project().track(t).unwrap().sends.is_empty());
        s.undo();
        assert_eq!(s.project().track(t).unwrap().sends.len(), 1);
    }

    #[test]
    fn title_marks_dirty() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _) = state(tmp.path());
        assert!(!s.title().ends_with('*'));
        s.add_track();
        assert!(s.title().ends_with('*'));
    }

    #[test]
    fn solo_pan_tempo_remove_and_empty_midi_clip() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _) = state(tmp.path());
        let id = s.add_track().unwrap();
        assert!(s.toggle_solo(id));
        assert!(s.project().track(id).unwrap().solo);
        assert!(s.set_pan(id, -0.5));
        assert!(!s.set_pan(id, -0.5));
        assert_eq!(s.project().track(id).unwrap().pan, -0.5);
        assert!(s.set_tempo(128.0));
        assert!(!s.set_tempo(128.0));
        assert_eq!(s.project().tempo_bpm, 128.0);
        let midi_clip = s.create_empty_midi_clip(None, 4.0, 8.0).unwrap();
        assert_eq!(s.project().tracks.len(), 2);
        assert!(s.project().clip(midi_clip).unwrap().1.midi.is_some());
        assert!(s.remove_track(id));
        assert_eq!(s.project().tracks.len(), 1);
        s.undo();
        assert_eq!(s.project().tracks.len(), 2);
    }
}
