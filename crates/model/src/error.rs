//! Error type of the model layer.

use std::path::PathBuf;

use crate::effect::InsertId;
use crate::project::{ClipId, TrackId};

/// Result alias used throughout `woa-model`.
pub type Result<T, E = ModelError> = std::result::Result<T, E>;

/// Everything that can go wrong when applying commands or touching project files.
///
/// A failed [`crate::Session::apply`] never leaves the project half-changed: commands
/// validate first and mutate afterwards.
#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    /// No track with this id exists.
    #[error("track {0} not found")]
    TrackNotFound(TrackId),
    /// No clip with this id exists.
    #[error("clip {0} not found")]
    ClipNotFound(ClipId),
    /// The clip carries no notes (audio or placeholder clip).
    #[error("clip {0} is not a MIDI clip")]
    NotMidiClip(ClipId),
    /// A track or clip with this id already exists.
    #[error("id {0} is already in use")]
    DuplicateId(u64),
    /// An id is above [`crate::project::MAX_ID`].
    #[error("id {0} is out of range")]
    IdOutOfRange(u64),
    /// A numeric parameter was out of range or not finite.
    #[error("invalid value for {field}: {value}")]
    InvalidValue {
        /// Name of the parameter, e.g. `"gain_db"`.
        field: &'static str,
        /// The rejected value.
        value: f64,
    },
    /// A track or project name was empty.
    #[error("name must not be empty")]
    EmptyName,
    /// A routing change would feed a bus back into itself.
    #[error("routing {from} -> {to} would create a cycle")]
    RoutingCycle {
        /// Source of the new connection.
        from: TrackId,
        /// Destination of the new connection.
        to: TrackId,
    },
    /// Outputs and sends can only go to bus tracks.
    #[error("track {0} is not a bus")]
    NotABus(TrackId),
    /// The track is still the destination of an output or send.
    #[error("track {0} is still a routing destination")]
    TrackInUse(TrackId),
    /// The track has no send to this destination.
    #[error("track {track} has no send to {target}")]
    SendNotFound {
        /// Track that should have the send.
        track: TrackId,
        /// Destination bus.
        target: TrackId,
    },
    /// The track already has a send to this destination.
    #[error("track {track} already has a send to {target}")]
    DuplicateSend {
        /// Track with the send.
        track: TrackId,
        /// Destination bus.
        target: TrackId,
    },
    /// No insert with this id on the track.
    #[error("insert {0} not found")]
    InsertNotFound(InsertId),
    /// The effect has no parameter with this index.
    #[error("effect has no parameter {index}")]
    UnknownParam {
        /// Rejected index.
        index: usize,
    },
    /// The effect has no parameter with this name.
    #[error("effect has no parameter named {0:?}")]
    UnknownParamName(String),
    /// The effect has no sidechain input.
    #[error("insert {0} has no sidechain input")]
    SidechainNotSupported(InsertId),
    /// An automation lane has too many points.
    #[error("an automation lane takes at most {max} points")]
    TooManyPoints {
        /// The limit.
        max: usize,
    },
    /// Two lanes of a track control the same target.
    #[error("track {track} has two automation lanes for the same target")]
    DuplicateAutomation {
        /// The track.
        track: TrackId,
    },
    /// There is nothing to undo.
    #[error("nothing to undo")]
    NothingToUndo,
    /// There is nothing to redo.
    #[error("nothing to redo")]
    NothingToRedo,
    /// Undo/redo is not allowed while a group is open.
    #[error("an undo group is still open")]
    GroupOpen,
    /// `end_group` was called without a matching `begin_group`.
    #[error("no undo group is open")]
    NoGroupOpen,
    /// The session has no project folder yet (needed for autosave/save).
    #[error("session has no project path; save it with save_as first")]
    NoPath,
    /// The file was written by a newer WOA version.
    #[error("project format version {found} is newer than supported version {supported}")]
    UnsupportedVersion {
        /// Version found in the file.
        found: u32,
        /// Highest version this build can read.
        supported: u32,
    },
    /// File system error, with the path involved.
    #[error("I/O error at {path}: {source}")]
    Io {
        /// File or folder that failed.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The JSON could not be parsed or produced.
    #[error("project JSON error at {path}: {source}")]
    Json {
        /// File that failed.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: serde_json::Error,
    },
}
