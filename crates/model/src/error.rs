//! Error type of the model layer.

use std::path::PathBuf;

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
