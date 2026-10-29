//! Window-independent app state: everything the UI does goes through here, so it can be
//! tested without a display. The UI only reads [`AppState`] and calls its methods.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use woa_model::{Clip, ClipId, Command, ModelError, Project, Session, TrackId, TrackKind};

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
    ) -> Option<ClipId> {
        let name = file.file_stem()?.to_string_lossy().into_owned();
        let audio = format!("{TAKES_DIR}/{}", file.file_name()?.to_string_lossy());
        let length_beats = (seconds * self.project().tempo_bpm / 60.0).max(1e-3);
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
                    offset_beats: 0.0,
                    fade_in_beats: 0.0,
                    fade_out_beats: 0.0,
                },
                index: None,
            })
        });
        let closed = self.session.end_group();
        self.report("Aufnahme", closed);
        ok.then_some(id)
    }

    /// Moves a clip on its track to `start_beats` (clamped to >= 0).
    pub fn move_clip(&mut self, clip: ClipId, start_beats: f64) -> bool {
        let Some((track, c)) = self.project().clip(clip) else {
            return false;
        };
        let (track, old) = (track.id, c.start_beats);
        let start_beats = start_beats.max(0.0);
        if (start_beats - old).abs() < 1e-9 {
            return false;
        }
        self.apply(Command::MoveClip {
            clip,
            to_track: track,
            start_beats,
            index: None,
        })
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

    /// Removes a clip (undoable). The audio file stays on disk.
    pub fn remove_clip(&mut self, clip: ClipId) -> bool {
        self.apply(Command::RemoveClip { clip })
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
        let clip = s.add_take(None, &file, 4.0, 2.0).unwrap();
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
        let a = s.add_take(None, &file, 0.0, 4.0).unwrap(); // 8 beats
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
        s.undo();
        s.undo();
        assert!(s.project().clip(b).is_none(), "split is one undo step");
        assert_eq!(s.project().clip(a).unwrap().1.length_beats, 8.0);
    }

    #[test]
    fn title_marks_dirty() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _) = state(tmp.path());
        assert!(!s.title().ends_with('*'));
        s.add_track();
        assert!(s.title().ends_with('*'));
    }
}
