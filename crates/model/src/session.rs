//! [`Session`]: a project plus its history and file location.

use std::path::{Path, PathBuf};

use crate::command::Command;
use crate::effect::InsertId;
use crate::error::{ModelError, Result};
use crate::history::{History, HistoryEntry};
use crate::project::{ClipId, Project, TrackId};
use crate::store;

/// An open project. GUI, headless tool and agent all change the project only via
/// [`Session::apply`], [`Session::undo`] and [`Session::redo`].
///
/// The session is plain data (`Send`, no interior mutability); the caller decides on
/// which thread it lives. It must never be touched from the audio thread.
#[derive(Debug, Clone)]
pub struct Session {
    project: Project,
    history: History,
    path: Option<PathBuf>,
    revision: u64,
    saved_revision: u64,
    autosaved_revision: u64,
}

impl Session {
    /// A new, empty, unsaved project.
    pub fn new(name: impl Into<String>) -> Self {
        Self::from_project(Project::new(name), None)
    }

    /// Wraps an existing project, e.g. one loaded with [`store::load`]. `path` is the
    /// `<name>.woa` folder it belongs to, if any.
    pub fn from_project(project: Project, path: Option<PathBuf>) -> Self {
        Self {
            project,
            history: History::new(),
            path,
            revision: 0,
            saved_revision: 0,
            autosaved_revision: 0,
        }
    }

    /// Opens the project folder `dir` (`<name>.woa`).
    pub fn open(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let project = store::load(dir)?;
        Ok(Self::from_project(project, Some(dir.to_path_buf())))
    }

    /// Read access to the project.
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Read access to the undo history.
    pub fn history(&self) -> &History {
        &self.history
    }

    /// Replaces the history, e.g. with [`History::with_limit`]. Only allowed on a
    /// fresh session; existing steps are discarded.
    pub fn set_history(&mut self, history: History) {
        self.history = history;
    }

    /// Project folder, if the session was opened or saved.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// True if the project changed since the last [`Session::save`].
    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    /// Reserves a new track id. Ids are never handed out twice.
    pub fn new_track_id(&mut self) -> TrackId {
        TrackId(self.project.alloc_id())
    }

    /// Reserves a new clip id.
    pub fn new_clip_id(&mut self) -> ClipId {
        ClipId(self.project.alloc_id())
    }

    /// Reserves a new insert id.
    pub fn new_insert_id(&mut self) -> InsertId {
        InsertId(self.project.alloc_id())
    }

    /// Removes a track and first cuts every output, send and sidechain that leads to it, as
    /// one undo step. [`Command::RemoveTrack`] alone refuses a track that is still routed to.
    pub fn remove_track_routed(&mut self, track: TrackId) -> Result<()> {
        let mut steps = Vec::new();
        for t in &self.project.tracks {
            if t.output == Some(track) {
                steps.push(Command::SetOutput {
                    track: t.id,
                    output: None,
                });
            }
            if t.sends.iter().any(|s| s.target == track) {
                steps.push(Command::RemoveSend {
                    track: t.id,
                    target: track,
                });
            }
            for i in t.inserts.iter().filter(|i| i.sidechain == Some(track)) {
                steps.push(Command::SetInsertSidechain {
                    track: t.id,
                    insert: i.id,
                    source: None,
                });
            }
        }
        steps.push(Command::RemoveTrack { track });
        self.group("Remove track", |s| {
            for c in steps {
                s.apply(c)?;
            }
            Ok(())
        })
    }

    /// Applies `command` and records it as one undo step (or as part of the open
    /// group). Clears the redo stack. On error nothing changes.
    pub fn apply(&mut self, command: Command) -> Result<()> {
        let inverse = command.clone().apply(&mut self.project)?;
        self.history.record(command, inverse);
        self.revision += 1;
        Ok(())
    }

    /// Opens an undo group: all commands until the matching [`Session::end_group`]
    /// become one undo step labelled `label`. Groups nest; inner labels are ignored.
    pub fn begin_group(&mut self, label: impl Into<String>) {
        if self.history.open.is_none() {
            self.history.open = Some(HistoryEntry {
                label: label.into(),
                forward: Vec::new(),
                inverse: Vec::new(),
            });
        }
        self.history.depth += 1;
    }

    /// Closes the innermost group. An empty outermost group leaves no undo step.
    pub fn end_group(&mut self) -> Result<()> {
        if self.history.depth == 0 {
            return Err(ModelError::NoGroupOpen);
        }
        self.history.depth -= 1;
        if self.history.depth == 0
            && let Some(entry) = self.history.open.take()
            && !entry.forward.is_empty()
        {
            self.history.push(entry);
        }
        Ok(())
    }

    /// Runs `f` inside a group. The group is closed even if `f` fails; commands that
    /// succeeded before the error stay applied and form the undo step.
    pub fn group<T>(
        &mut self,
        label: impl Into<String>,
        f: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        self.begin_group(label);
        let result = f(self);
        self.end_group()?;
        result
    }

    /// Reverts the last undo step.
    pub fn undo(&mut self) -> Result<()> {
        if self.history.group_open() {
            return Err(ModelError::GroupOpen);
        }
        let entry = self.history.undo.pop().ok_or(ModelError::NothingToUndo)?;
        match replay(&self.project, entry.inverse.iter().rev()) {
            Ok(project) => {
                self.project = project;
                self.history.redo.push(entry);
                self.revision += 1;
                Ok(())
            }
            Err(e) => {
                self.history.undo.push(entry);
                Err(e)
            }
        }
    }

    /// Repeats the last undone step.
    pub fn redo(&mut self) -> Result<()> {
        if self.history.group_open() {
            return Err(ModelError::GroupOpen);
        }
        let entry = self.history.redo.pop().ok_or(ModelError::NothingToRedo)?;
        match replay(&self.project, entry.forward.iter()) {
            Ok(project) => {
                self.project = project;
                self.history.undo.push(entry);
                self.revision += 1;
                Ok(())
            }
            Err(e) => {
                self.history.redo.push(entry);
                Err(e)
            }
        }
    }

    /// Saves to the current project folder.
    pub fn save(&mut self) -> Result<()> {
        let dir = self.path.clone().ok_or(ModelError::NoPath)?;
        self.save_as(dir)
    }

    /// Saves to `dir` (a `<name>.woa` folder, created if missing) and makes it the
    /// session's folder. Removes a stale autosave there.
    pub fn save_as(&mut self, dir: impl AsRef<Path>) -> Result<()> {
        let dir = dir.as_ref();
        store::save(&self.project, dir)?;
        store::remove_autosave(dir)?;
        self.path = Some(dir.to_path_buf());
        self.saved_revision = self.revision;
        self.autosaved_revision = self.revision;
        Ok(())
    }

    /// Autosave hook (rule 7). Call it from a timer every few seconds; it writes
    /// `autosave.json` into the project folder only if something changed since the
    /// last save or autosave. Returns whether a file was written.
    ///
    /// Unsaved sessions without a folder return [`ModelError::NoPath`]; the caller
    /// may then autosave into a recovery folder with [`Session::autosave_to`].
    pub fn autosave(&mut self) -> Result<bool> {
        let dir = self.path.clone().ok_or(ModelError::NoPath)?;
        self.autosave_to(dir)
    }

    /// Like [`Session::autosave`], but into an explicit folder.
    pub fn autosave_to(&mut self, dir: impl AsRef<Path>) -> Result<bool> {
        if self.revision == self.autosaved_revision {
            return Ok(false);
        }
        store::save_autosave(&self.project, dir.as_ref())?;
        self.autosaved_revision = self.revision;
        Ok(true)
    }
}

/// Applies commands to a copy, so a failure halfway leaves the original intact.
fn replay<'a>(project: &Project, commands: impl Iterator<Item = &'a Command>) -> Result<Project> {
    let mut copy = project.clone();
    for c in commands {
        c.clone().apply(&mut copy)?;
    }
    Ok(copy)
}
