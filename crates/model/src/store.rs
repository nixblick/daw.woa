//! Project folder format and atomic file writes.
//!
//! Layout of a project folder:
//!
//! ```text
//! <name>.woa/
//!   project.json     # ProjectFile { format_version, project }
//!   autosave.json    # optional, newer unsaved state (crash recovery)
//!   audio/           # recorded and imported audio (filled by later milestones)
//! ```
//!
//! Every file is written atomically: to a fresh temp file (O_EXCL, random name) in the
//! same folder, flushed to
//! disk, then renamed over the target. A crash therefore leaves either the old or
//! the new file, never a half-written one.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{ModelError, Result};
use crate::project::Project;

/// Current version of `project.json`. Bump on every incompatible change and add a
/// migration in [`from_json`]. Version 2 (Paket 16) adds `output`, `sends` and `inserts`
/// to tracks; version 1 files load unchanged because all three default to empty.
/// Version 3 (Paket 18) adds `automation` to tracks and `sidechain` to inserts. A file is
/// written with the lowest version that can hold it ([`required_version`]), so projects
/// without automation and sidechain stay readable by older builds; a build that does not
/// know version 3 refuses such a file instead of silently dropping the curves on re-save.
pub const FORMAT_VERSION: u32 = 3;

/// The lowest format version that can hold `project`: 3 with automation, a sidechain, a
/// clip gain or a fade curve, otherwise 2.
pub fn required_version(project: &Project) -> u32 {
    let v3 = project.tracks.iter().any(|t| {
        !t.automation.is_empty()
            || !t.take_folders.is_empty()
            || t.inserts.iter().any(|i| i.sidechain.is_some())
            || t.clips.iter().any(|c| {
                c.gain_db != 0.0 || !c.fade_in_curve.is_linear() || !c.fade_out_curve.is_linear()
            })
    });
    if v3 { 3 } else { 2 }
}
/// File name of the project inside the folder.
pub const PROJECT_FILE: &str = "project.json";
/// File name of the autosave inside the folder.
pub const AUTOSAVE_FILE: &str = "autosave.json";
/// Sub-folder for audio files.
pub const AUDIO_DIR: &str = "audio";

/// On-disk envelope around a [`Project`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectFile {
    /// Format version, see [`FORMAT_VERSION`].
    pub format_version: u32,
    /// The project itself.
    pub project: Project,
}

/// Serialises a project to the pretty JSON stored in `project.json`.
pub fn to_json(project: &Project) -> Result<String> {
    let file = ProjectFile {
        format_version: required_version(project),
        project: project.clone(),
    };
    serde_json::to_string_pretty(&file).map_err(|source| ModelError::Json {
        path: PathBuf::from(PROJECT_FILE),
        source,
    })
}

/// Parses `project.json` content, checking the format version.
pub fn from_json(json: &str, path: &Path) -> Result<Project> {
    #[derive(Deserialize)]
    struct Version {
        format_version: u32,
    }
    let json_err = |source| ModelError::Json {
        path: path.to_path_buf(),
        source,
    };
    let v: Version = serde_json::from_str(json).map_err(json_err)?;
    if v.format_version > FORMAT_VERSION {
        return Err(ModelError::UnsupportedVersion {
            found: v.format_version,
            supported: FORMAT_VERSION,
        });
    }
    let mut file: ProjectFile = serde_json::from_str(json).map_err(json_err)?;
    file.project.migrate();
    file.project.check_ids()?;
    file.project.check_routing()?;
    Ok(file.project)
}

/// Saves `project` into the folder `dir` (created with `audio/` if missing).
pub fn save(project: &Project, dir: &Path) -> Result<()> {
    create_dir(dir)?;
    create_dir(&dir.join(AUDIO_DIR))?;
    write_atomic(&dir.join(PROJECT_FILE), to_json(project)?.as_bytes())
}

/// Loads `project.json` from the folder `dir`.
pub fn load(dir: &Path) -> Result<Project> {
    read_project(&dir.join(PROJECT_FILE))
}

/// Writes `autosave.json` into `dir` (created if missing).
pub fn save_autosave(project: &Project, dir: &Path) -> Result<()> {
    create_dir(dir)?;
    write_atomic(&dir.join(AUTOSAVE_FILE), to_json(project)?.as_bytes())
}

/// Loads the autosave of `dir`, if one exists. After a crash the GUI offers this
/// state for recovery.
pub fn load_autosave(dir: &Path) -> Result<Option<Project>> {
    let path = dir.join(AUTOSAVE_FILE);
    if !path.exists() {
        return Ok(None);
    }
    read_project(&path).map(Some)
}

/// Removes the autosave of `dir`, if any (after a regular save).
pub fn remove_autosave(dir: &Path) -> Result<()> {
    let path = dir.join(AUTOSAVE_FILE);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(ModelError::Io { path, source }),
    }
}

/// Writes `bytes` to `path` atomically (temp file in the same folder, fsync, rename).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let io = |p: &Path| {
        let p = p.to_path_buf();
        move |source| ModelError::Io { path: p, source }
    };
    // A fresh, exclusively created temp file never follows a planted symlink.
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(io(parent))?;
    tmp.write_all(bytes).map_err(io(tmp.path()))?;
    tmp.as_file().sync_all().map_err(io(tmp.path()))?;
    tmp.persist(path).map_err(|e| io(path)(e.error))?;
    // Persist the rename itself; directories cannot be opened for sync on Windows.
    #[cfg(unix)]
    if let Some(parent) = path.parent()
        && let Ok(d) = fs::File::open(parent)
    {
        d.sync_all().map_err(io(parent))?;
    }
    Ok(())
}

fn read_project(path: &Path) -> Result<Project> {
    let json = fs::read_to_string(path).map_err(|source| ModelError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    from_json(&json, path)
}

fn create_dir(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).map_err(|source| ModelError::Io {
        path: dir.to_path_buf(),
        source,
    })
}
