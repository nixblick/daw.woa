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

mod command;
mod error;
mod history;
mod project;
mod session;
pub mod store;

pub use command::Command;
pub use error::{ModelError, Result};
pub use history::{History, HistoryEntry};
pub use project::{
    Clip, ClipId, DEFAULT_TEMPO_BPM, MAX_GAIN_DB, MAX_TEMPO_BPM, MIN_GAIN_DB, MIN_TEMPO_BPM,
    Project, ProjectMeta, Track, TrackId, TrackKind,
};
pub use session::Session;
pub use store::{FORMAT_VERSION, ProjectFile};
