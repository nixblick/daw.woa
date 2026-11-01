//! # woa-model: Projekt, Spuren, Clips, Commands, Undo
//!
//! This crate is the single shared layer between every frontend (GUI, headless tool,
//! MCP agent) and the engine. Nobody mutates a [`Project`] directly: every change is a
//! [`Command`] sent through [`Session::apply`]. That gives us, for free:
//!
//! * **Undo/Redo** – every command returns its exact inverse when applied; the
//!   [`History`] keeps both and can group several commands into one undo step
//!   ([`Session::begin_group`] / [`Session::end_group`] or [`Session::group`]).
//! * **Project file** – a project is stored as a folder `<name>.woa/` holding
//!   `project.json` (with a `format_version`) and an `audio/` folder. Writes are
//!   atomic (temporary file, fsync, rename), see [`store`].
//! * **Autosave** – [`Session::autosave`] writes `autosave.json` next to
//!   `project.json` when something changed. The caller owns the timer (rule 7 in
//!   `CLAUDE.md`); this crate only provides the hook.
//!
//! ```
//! use woa_model::{Command, Session, TrackKind};
//!
//! let mut session = Session::new("Jingle");
//! let id = session.new_track_id();
//! session.apply(Command::add_track(id, "Voice", TrackKind::Audio))?;
//! session.apply(Command::SetGain { track: id, gain_db: -3.0 })?;
//! assert_eq!(session.project().track(id).map(|t| t.gain_db), Some(-3.0));
//!
//! session.undo()?;
//! assert_eq!(session.project().track(id).map(|t| t.gain_db), Some(0.0));
//! # Ok::<(), woa_model::ModelError>(())
//! ```
//!
//! No function in this crate panics on bad input; all failures are [`ModelError`].

pub mod automation;
pub mod chords;
mod command;
pub mod drums;
pub mod effect;
mod error;
mod history;
pub mod midi;
pub mod plugin;
mod project;
mod session;
pub mod sound;
pub mod store;
pub mod takes;
pub mod voice_over;

pub use automation::{
    AutoMode, AutoPoint, AutoRange, AutoTarget, AutomationLane, MAX_AUTOMATION_POINTS,
};
pub use chords::{CHORD_PROGRESSIONS, ChordProgression};
pub use command::Command;
pub use drums::{DRUM_PRESETS, DRUM_ROWS, DrumPreset, DrumRow, drum_preset};
pub use effect::{
    DELAY_SYNC_CHOICES, EffectKind, Insert, InsertId, ParamSpec, Preset, VOICE_CHAIN,
};
pub use error::{ModelError, Result};
pub use history::{History, HistoryEntry};
pub use midi::{CC_SUSTAIN, Edited, MIN_NOTE_BEATS, MidiCc, MidiData, MidiNote, grid_beats};
pub use plugin::{PluginParam, PluginRole, PluginSlot};
pub use project::{
    Clip, ClipId, DEFAULT_TEMPO_BPM, FOLLOW_TEMPO_RANGE, FadeCurve, MAX_GAIN_DB, MAX_ID,
    MAX_SEND_DB, MAX_TEMPO_BPM, MIN_GAIN_DB, MIN_TEMPO_BPM, PITCH_RANGE, Project, ProjectMeta,
    STRETCH_RANGE, Send, Track, TrackId, TrackKind, is_safe_audio_path,
};
pub use session::Session;
pub use sound::{SOUND_PRESETS, SoundBank, SoundPreset, SoundRole, TrackSound, preset_name};
pub use store::{FORMAT_VERSION, ProjectFile, required_version};
pub use takes::{
    ClipPlan, CompSegment, MAX_COMP_SEGMENTS, MAX_TAKES, RecordInput, RecordPlan, TakeEntry,
    TakeFolder, beats_for_samples, plan_recording,
};
pub use voice_over::{DUCK_COMPRESSOR, VoiceOver};
