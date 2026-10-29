//! # woa-agent: MCP-Server
//!
//! Lets an AI agent (Claude Code, Claude Desktop, Gemini CLI) work on a WOA project
//! through the Model Context Protocol. The agent never touches the project directly:
//! every tool call becomes one or more [`Command`]s sent through [`Session::apply`],
//! wrapped in an undo group labelled [`AGENT_LABEL`]. So each tool call is exactly one
//! undo step, and a human can revert whatever the agent did.
//!
//! The server runs on the control side only (never in the audio thread), so the
//! [`Mutex`] around the session is fine here.
//!
//! Tool parameter schemas are derived from the serde types below via `schemars`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ServerCapabilities, ServerConfig};
use rmcp::{ErrorData as McpError, ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use woa_model::{Clip, ClipId, Command, ModelError, Session, TrackId, TrackKind, store};

pub mod audio;

use audio::{AgentNote, AgentWaveform};

/// Undo label of every step an agent creates.
pub const AGENT_LABEL: &str = "Agent";

/// Errors when starting or running the server.
#[derive(Debug)]
pub enum AgentError {
    /// The project could not be opened.
    Model(ModelError),
    /// The MCP transport failed.
    Transport(String),
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Model(e) => write!(f, "Projekt: {e}"),
            Self::Transport(e) => write!(f, "MCP: {e}"),
        }
    }
}

impl std::error::Error for AgentError {}

impl From<ModelError> for AgentError {
    fn from(e: ModelError) -> Self {
        Self::Model(e)
    }
}

/// Track kind as seen by the agent.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentTrackKind {
    /// Audio track.
    #[default]
    Audio,
    /// MIDI track.
    Midi,
    /// Bus (group) track.
    Bus,
}

impl From<AgentTrackKind> for TrackKind {
    fn from(k: AgentTrackKind) -> Self {
        match k {
            AgentTrackKind::Audio => TrackKind::Audio,
            AgentTrackKind::Midi => TrackKind::Midi,
            AgentTrackKind::Bus => TrackKind::Bus,
        }
    }
}

/// Parameters of `add_track`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddTrackParams {
    /// Display name of the new track.
    pub name: String,
    /// Track kind; default `audio`.
    #[serde(default)]
    pub kind: AgentTrackKind,
}

/// Parameters of `rename_track`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RenameTrackParams {
    /// Track id (from `project_info` or `add_track`).
    pub track: u64,
    /// New, non-empty name.
    pub name: String,
}

/// Parameters of `set_gain`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetGainParams {
    /// Track id.
    pub track: u64,
    /// Gain in dB (0 = unchanged level).
    pub gain_db: f64,
}

/// Parameters of `set_pan`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetPanParams {
    /// Track id.
    pub track: u64,
    /// Pan from -1.0 (left) to 1.0 (right).
    pub pan: f64,
}

/// Parameters of `set_mute`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetMuteParams {
    /// Track id.
    pub track: u64,
    /// True mutes the track.
    pub mute: bool,
}

/// Parameters of `set_tempo`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetTempoParams {
    /// Beats per minute.
    pub bpm: f64,
}

/// Parameters of `add_clip`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddClipParams {
    /// Track id.
    pub track: u64,
    /// Display name of the clip.
    pub name: String,
    /// Start position in beats (>= 0).
    pub start_beats: f64,
    /// Length in beats (> 0).
    pub length_beats: f64,
}

/// Parameters of tools that only name a track.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct TrackParams {
    /// Track id.
    pub track: u64,
}

/// Parameters of tools that only name a clip.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ClipParams {
    /// Clip id.
    pub clip: u64,
}

/// Parameters of `set_solo`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetSoloParams {
    /// Track id.
    pub track: u64,
    /// True solos the track.
    pub solo: bool,
}

/// Parameters of `generate_notes`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateNotesParams {
    /// Audio track that gets the new clip.
    pub track: u64,
    /// Clip name; default "Synth".
    #[serde(default)]
    pub name: Option<String>,
    /// Clip start on the timeline in beats.
    #[serde(default)]
    pub start_beats: f64,
    /// Clip length in beats (> 0). Notes are cut at the end.
    pub length_beats: f64,
    /// Oscillator: sine, saw (default) or square.
    #[serde(default)]
    pub waveform: AgentWaveform,
    /// Notes, times relative to the clip start.
    pub notes: Vec<AgentNote>,
}

/// Parameters of `generate_drums`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateDrumsParams {
    /// Audio track that gets the new clip.
    pub track: u64,
    /// Clip name; default "Drums".
    #[serde(default)]
    pub name: Option<String>,
    /// Clip start on the timeline in beats.
    #[serde(default)]
    pub start_beats: f64,
    /// Kick pattern, one character per 16th note: `x` hit, `o` soft hit, `.` rest.
    /// Example one bar: "x...x...x...x...".
    #[serde(default)]
    pub kick: String,
    /// Snare pattern, same notation.
    #[serde(default)]
    pub snare: String,
    /// Hi-hat pattern, same notation.
    #[serde(default)]
    pub hihat: String,
}

/// Parameters of `import_audio`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImportAudioParams {
    /// Audio track that gets the new clip.
    pub track: u64,
    /// Absolute path of a WAV file; it is copied into the project folder.
    pub path: String,
    /// Clip start in beats.
    #[serde(default)]
    pub start_beats: f64,
    /// Clip name; default the file name.
    #[serde(default)]
    pub name: Option<String>,
}

/// Parameters of `move_clip`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MoveClipParams {
    /// Clip id.
    pub clip: u64,
    /// New start in beats.
    pub start_beats: f64,
    /// Destination track; default the current one.
    #[serde(default)]
    pub track: Option<u64>,
}

/// Parameters of `trim_clip`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct TrimClipParams {
    /// Clip id.
    pub clip: u64,
    /// New length in beats; default unchanged.
    #[serde(default)]
    pub length_beats: Option<f64>,
    /// New offset into the audio in beats; default unchanged.
    #[serde(default)]
    pub offset_beats: Option<f64>,
}

/// Parameters of `set_fades`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetFadesParams {
    /// Clip id.
    pub clip: u64,
    /// Fade-in in beats.
    pub fade_in_beats: f64,
    /// Fade-out in beats.
    pub fade_out_beats: f64,
}

/// Parameters of `repeat_clip`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RepeatClipParams {
    /// Clip id.
    pub clip: u64,
    /// Number of copies placed right after the clip, one after another (1..=256).
    pub times: u32,
}

/// Parameters of `export`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExportParams {
    /// Target file, `.wav`, `.flac` or `.mp3`. Relative path inside the project folder; absolute paths and `..` are rejected.
    pub path: String,
    /// Integrated loudness to normalise to, e.g. -14 (streaming) or -16 (podcast).
    /// Peaks stay below -1 dBFS. Default: unchanged.
    #[serde(default)]
    pub target_lufs: Option<f64>,
}

/// The MCP server around one project session.
#[derive(Clone)]
pub struct WoaAgent {
    session: Arc<Mutex<Session>>,
    dir: PathBuf,
    // Read by the code generated from `#[tool_handler]`.
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

fn lock_err() -> McpError {
    McpError::internal_error("Sitzung nicht verfügbar (vergiftete Sperre)", None)
}

fn fail(e: impl std::fmt::Display) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(e.to_string())])
}

fn ok_json(value: serde_json::Value) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(value.to_string())])
}

impl WoaAgent {
    /// Opens the project folder `dir`, or starts a new project there if it holds no
    /// `project.json` yet (written on the first `save`).
    pub fn open_or_create(dir: impl AsRef<Path>) -> Result<Self, AgentError> {
        let dir = dir.as_ref().to_path_buf();
        let session = if dir.join(store::PROJECT_FILE).exists() {
            Session::open(&dir)?
        } else {
            let name = dir
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Projekt".to_string());
            Session::new(name)
        };
        Ok(Self::with_session(session, dir))
    }

    /// Wraps an existing session; `save` writes to `dir`.
    pub fn with_session(session: Session, dir: PathBuf) -> Self {
        Self {
            session: Arc::new(Mutex::new(session)),
            dir,
            tool_router: Self::tool_router(),
        }
    }

    /// Shared handle to the session, e.g. for a GUI that shows the agent's work live.
    pub fn session(&self) -> Arc<Mutex<Session>> {
        Arc::clone(&self.session)
    }

    fn lock(&self) -> Result<MutexGuard<'_, Session>, McpError> {
        self.session.lock().map_err(|_| lock_err())
    }

    /// Runs `f` as one undo step labelled "Agent" and turns the outcome into a tool
    /// result. Model errors go back to the agent as tool errors, not protocol errors.
    fn agent_step(
        &self,
        f: impl FnOnce(&mut Session) -> Result<serde_json::Value, ModelError>,
    ) -> Result<CallToolResult, McpError> {
        let mut s = self.lock()?;
        Ok(match s.group(AGENT_LABEL, f) {
            Ok(v) => ok_json(v),
            Err(e) => fail(e),
        })
    }

    /// Checks that `track` is an audio track and `start` a valid position.
    fn check_target(s: &Session, track: u64, start: f64) -> Result<(), String> {
        match s.project().track(TrackId(track)) {
            None => return Err(ModelError::TrackNotFound(TrackId(track)).to_string()),
            Some(t) if t.kind != TrackKind::Audio => {
                return Err(format!("Spur {track} ist keine Audio-Spur"));
            }
            Some(_) => {}
        }
        if !(start.is_finite() && start >= 0.0) {
            return Err(format!("start_beats muss >= 0 sein, war {start}"));
        }
        Ok(())
    }

    /// Writes `buf` as a new take and adds an audio clip for it (one undo step).
    fn add_take(
        &self,
        s: &mut Session,
        track: u64,
        name: String,
        start_beats: f64,
        length_beats: f64,
        buf: &[f32],
    ) -> CallToolResult {
        let id = s.new_clip_id();
        let (rel, abs) = audio::free_take_path(&self.dir, &format!("agent-{}", id.0), "wav");
        if let Err(e) = audio::write_take(&abs, buf) {
            return fail(e);
        }
        Self::add_file_clip(s, id, track, name, start_beats, length_beats, rel)
    }

    fn add_file_clip(
        s: &mut Session,
        id: ClipId,
        track: u64,
        name: String,
        start_beats: f64,
        length_beats: f64,
        rel: String,
    ) -> CallToolResult {
        let clip = Clip::new(id, name, start_beats, length_beats, Some(rel.clone()));
        let r = s.group(AGENT_LABEL, |s| {
            s.apply(Command::AddClip {
                track: TrackId(track),
                clip,
                index: None,
            })
        });
        match r {
            Ok(()) => ok_json(serde_json::json!({
                "clip": id.0,
                "audio": rel,
                "length_beats": audio::round(length_beats, 4),
            })),
            Err(e) => fail(e),
        }
    }

    fn clip_of(s: &Session, clip: u64) -> Result<Clip, ModelError> {
        s.project()
            .clip(ClipId(clip))
            .map(|(_, c)| c.clone())
            .ok_or(ModelError::ClipNotFound(ClipId(clip)))
    }

    /// Zielpfad im Projektordner. Absolute Pfade und `..` sind verboten;
    /// ein vorhandener Elternordner muss kanonisch im Projekt liegen (Symlinks).
    fn resolve_out(&self, path: &str) -> Result<PathBuf, String> {
        if !woa_model::is_safe_audio_path(path) {
            return Err(format!(
                "{path}: nur relative Pfade innerhalb des Projektordners erlaubt"
            ));
        }
        let out = self.dir.join(path);
        if let Some(parent) = out.parent()
            && parent.exists()
        {
            let root = self.dir.canonicalize().map_err(|e| e.to_string())?;
            let real = parent.canonicalize().map_err(|e| e.to_string())?;
            if !real.starts_with(&root) {
                return Err(format!("{path}: Pfad verlässt den Projektordner"));
            }
        }
        Ok(out)
    }
}

#[tool_router]
impl WoaAgent {
    #[tool(
        description = "Project overview as JSON: name, tempo, tracks with ids, mixer settings and clips, undo/redo labels, unsaved changes."
    )]
    async fn project_info(&self) -> Result<CallToolResult, McpError> {
        let s = self.lock()?;
        let project = serde_json::to_value(s.project())
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(ok_json(serde_json::json!({
            "project": project,
            "path": self.dir.display().to_string(),
            "dirty": s.is_dirty(),
            "undo": s.history().undo_label(),
            "redo": s.history().redo_label(),
        })))
    }

    #[tool(description = "Add a track at the end. Returns its id.")]
    async fn add_track(
        &self,
        Parameters(p): Parameters<AddTrackParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            let id = s.new_track_id();
            s.apply(Command::add_track(id, p.name, p.kind.into()))?;
            Ok(serde_json::json!({ "track": id.0 }))
        })
    }

    #[tool(description = "Rename a track.")]
    async fn rename_track(
        &self,
        Parameters(p): Parameters<RenameTrackParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            s.apply(Command::RenameTrack {
                track: TrackId(p.track),
                name: p.name,
            })?;
            Ok(serde_json::json!({ "ok": true }))
        })
    }

    #[tool(description = "Set the gain of a track in dB.")]
    async fn set_gain(
        &self,
        Parameters(p): Parameters<SetGainParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            s.apply(Command::SetGain {
                track: TrackId(p.track),
                gain_db: p.gain_db,
            })?;
            Ok(serde_json::json!({ "ok": true }))
        })
    }

    #[tool(description = "Set the pan of a track (-1 left .. 1 right).")]
    async fn set_pan(
        &self,
        Parameters(p): Parameters<SetPanParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            s.apply(Command::SetPan {
                track: TrackId(p.track),
                pan: p.pan,
            })?;
            Ok(serde_json::json!({ "ok": true }))
        })
    }

    #[tool(description = "Mute or unmute a track.")]
    async fn set_mute(
        &self,
        Parameters(p): Parameters<SetMuteParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            s.apply(Command::SetMute {
                track: TrackId(p.track),
                mute: p.mute,
            })?;
            Ok(serde_json::json!({ "ok": true }))
        })
    }

    #[tool(description = "Set the project tempo in BPM.")]
    async fn set_tempo(
        &self,
        Parameters(p): Parameters<SetTempoParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            s.apply(Command::SetTempo { bpm: p.bpm })?;
            Ok(serde_json::json!({ "ok": true }))
        })
    }

    #[tool(description = "Add a clip to a track, position and length in beats. Returns its id.")]
    async fn add_clip(
        &self,
        Parameters(p): Parameters<AddClipParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            let id: ClipId = s.new_clip_id();
            s.apply(Command::AddClip {
                track: TrackId(p.track),
                clip: Clip {
                    id,
                    name: p.name,
                    start_beats: p.start_beats,
                    length_beats: p.length_beats,
                    audio: None,
                    offset_beats: 0.0,
                    fade_in_beats: 0.0,
                    fade_out_beats: 0.0,
                },
                index: None,
            })?;
            Ok(serde_json::json!({ "clip": id.0 }))
        })
    }

    #[tool(
        description = "Render notes with the built-in synth into a new audio clip (WAV in the project's aufnahmen/ folder). Times in beats relative to the clip start; pitch is MIDI (60 = C4). Returns the clip id."
    )]
    async fn generate_notes(
        &self,
        Parameters(p): Parameters<GenerateNotesParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut s = self.lock()?;
        if let Err(e) = Self::check_target(&s, p.track, p.start_beats) {
            return Ok(fail(e));
        }
        let bpm = s.project().tempo_bpm;
        let buf = match audio::notes_buffer(p.waveform, &p.notes, p.length_beats, bpm) {
            Ok(b) => b,
            Err(e) => return Ok(fail(e)),
        };
        let name = p.name.unwrap_or_else(|| "Synth".into());
        Ok(self.add_take(&mut s, p.track, name, p.start_beats, p.length_beats, &buf))
    }

    #[tool(
        description = "Render a drum pattern (synthetic kick, snare, hi-hat) into a new audio clip. One character per 16th note: x = hit, o = soft hit, . = rest; 16 characters = one 4/4 bar. Clip length = longest pattern. Returns the clip id."
    )]
    async fn generate_drums(
        &self,
        Parameters(p): Parameters<GenerateDrumsParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut s = self.lock()?;
        if let Err(e) = Self::check_target(&s, p.track, p.start_beats) {
            return Ok(fail(e));
        }
        let bpm = s.project().tempo_bpm;
        let (buf, length) = match audio::drums_buffer(&p.kick, &p.snare, &p.hihat, bpm) {
            Ok(b) => b,
            Err(e) => return Ok(fail(e)),
        };
        let name = p.name.unwrap_or_else(|| "Drums".into());
        Ok(self.add_take(&mut s, p.track, name, p.start_beats, length, &buf))
    }

    #[tool(
        description = "Copy a WAV file into the project folder (aufnahmen/) and add it as a clip on an audio track. Returns the clip id and its length in beats."
    )]
    async fn import_audio(
        &self,
        Parameters(p): Parameters<ImportAudioParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut s = self.lock()?;
        if let Err(e) = Self::check_target(&s, p.track, p.start_beats) {
            return Ok(fail(e));
        }
        let src = PathBuf::from(&p.path);
        let length = match audio::wav_length_beats(&src, s.project().tempo_bpm) {
            Ok(l) => l,
            Err(e) => return Ok(fail(e)),
        };
        let stem = src
            .file_stem()
            .map(|x| x.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (rel, abs) = audio::free_take_path(&self.dir, &stem, "wav");
        if let Some(parent) = abs.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            return Ok(fail(format!("{}: {e}", parent.display())));
        }
        if let Err(e) = std::fs::copy(&src, &abs) {
            return Ok(fail(format!("{}: {e}", src.display())));
        }
        let id = s.new_clip_id();
        let name = match p.name {
            Some(n) => n,
            None if stem.is_empty() => "Import".into(),
            None => stem,
        };
        Ok(Self::add_file_clip(
            &mut s,
            id,
            p.track,
            name,
            p.start_beats,
            length,
            rel,
        ))
    }

    #[tool(description = "Move a clip to another start (beats) and optionally another track.")]
    async fn move_clip(
        &self,
        Parameters(p): Parameters<MoveClipParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            let to_track = match p.track {
                Some(t) => TrackId(t),
                None => s
                    .project()
                    .clip(ClipId(p.clip))
                    .map(|(t, _)| t.id)
                    .ok_or(ModelError::ClipNotFound(ClipId(p.clip)))?,
            };
            s.apply(Command::MoveClip {
                clip: ClipId(p.clip),
                to_track,
                start_beats: p.start_beats,
                index: None,
            })?;
            Ok(serde_json::json!({ "ok": true }))
        })
    }

    #[tool(description = "Change a clip's length and/or its offset into the audio (beats).")]
    async fn trim_clip(
        &self,
        Parameters(p): Parameters<TrimClipParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            let c = Self::clip_of(s, p.clip)?;
            s.apply(Command::EditClip {
                clip: c.id,
                length_beats: p.length_beats.unwrap_or(c.length_beats),
                offset_beats: p.offset_beats.unwrap_or(c.offset_beats),
                fade_in_beats: c.fade_in_beats,
                fade_out_beats: c.fade_out_beats,
            })?;
            Ok(serde_json::json!({ "ok": true }))
        })
    }

    #[tool(
        description = "Set fade-in and fade-out of a clip in beats (together at most the clip length)."
    )]
    async fn set_fades(
        &self,
        Parameters(p): Parameters<SetFadesParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            let c = Self::clip_of(s, p.clip)?;
            s.apply(Command::EditClip {
                clip: c.id,
                length_beats: c.length_beats,
                offset_beats: c.offset_beats,
                fade_in_beats: p.fade_in_beats,
                fade_out_beats: p.fade_out_beats,
            })?;
            Ok(serde_json::json!({ "ok": true }))
        })
    }

    #[tool(
        description = "Repeat a clip: place `times` copies right after it, back to back on the same track (for loops). Returns the new clip ids."
    )]
    async fn repeat_clip(
        &self,
        Parameters(p): Parameters<RepeatClipParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut s = self.lock()?;
        if !(1..=256).contains(&p.times) {
            return Ok(fail(format!("times muss 1..=256 sein, war {}", p.times)));
        }
        let (track, c) = match s.project().clip(ClipId(p.clip)) {
            Some((t, c)) => (t.id, c.clone()),
            None => return Ok(fail(ModelError::ClipNotFound(ClipId(p.clip)))),
        };
        let r = s.group(AGENT_LABEL, |s| {
            let mut ids = Vec::new();
            for k in 1..=p.times {
                let id = s.new_clip_id();
                let mut copy = c.clone();
                copy.id = id;
                copy.start_beats = c.start_beats + f64::from(k) * c.length_beats;
                s.apply(Command::AddClip {
                    track,
                    clip: copy,
                    index: None,
                })?;
                ids.push(id.0);
            }
            Ok(ids)
        });
        Ok(match r {
            Ok(ids) => ok_json(serde_json::json!({ "clips": ids })),
            Err(e) => fail(e),
        })
    }

    #[tool(description = "Remove a clip (its audio file stays in the project folder).")]
    async fn remove_clip(
        &self,
        Parameters(p): Parameters<ClipParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            s.apply(Command::RemoveClip {
                clip: ClipId(p.clip),
            })?;
            Ok(serde_json::json!({ "ok": true }))
        })
    }

    #[tool(description = "Remove a track with all its clips.")]
    async fn remove_track(
        &self,
        Parameters(p): Parameters<TrackParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            s.apply(Command::RemoveTrack {
                track: TrackId(p.track),
            })?;
            Ok(serde_json::json!({ "ok": true }))
        })
    }

    #[tool(
        description = "Solo or unsolo a track. While any track is soloed, only soloed tracks sound."
    )]
    async fn set_solo(
        &self,
        Parameters(p): Parameters<SetSoloParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            s.apply(Command::SetSolo {
                track: TrackId(p.track),
                solo: p.solo,
            })?;
            Ok(serde_json::json!({ "ok": true }))
        })
    }

    #[tool(
        description = "Render the project offline (48 kHz, up to the end of the last clip) and measure it: integrated loudness (LUFS, ITU-R BS.1770), sample peak in dBFS, clipping, length in seconds, and the peak of every audio track on its own. Changes nothing."
    )]
    async fn analyze(&self) -> Result<CallToolResult, McpError> {
        let project = self.lock()?.project().clone();
        let mix = match audio::render(&project, &self.dir) {
            Ok(m) => m,
            Err(e) => return Ok(fail(e)),
        };
        let mut result = audio::measure(&mix);
        let mut tracks = Vec::new();
        for t in project.tracks.iter().filter(|t| t.kind == TrackKind::Audio) {
            // The track alone (soloed, unmuted), mixer settings kept.
            let mut alone = project.clone();
            for o in &mut alone.tracks {
                o.solo = o.id == t.id;
                if o.id == t.id {
                    o.mute = false;
                }
            }
            let peak = audio::render(&alone, &self.dir)
                .ok()
                .and_then(|m| audio::peak_db(&m.left, &m.right))
                .map(|v| audio::round(v, 2));
            tracks.push(serde_json::json!({
                "track": t.id.0,
                "name": t.name,
                "muted": t.mute,
                "peak_dbfs": peak,
            }));
        }
        result["tracks"] = serde_json::Value::Array(tracks);
        Ok(ok_json(result))
    }

    #[tool(
        description = "Export the mixdown as WAV, FLAC (24 bit) or MP3 (320 kbit/s), chosen by the file extension. Optional target_lufs normalises the loudness (peaks stay below -1 dBFS). Returns loudness and peak of the written audio. If target_reached is false (normalized.peak_limited), the mix has too much peak for that loudness: lower the gain of the drum/percussive tracks by a few dB and export again."
    )]
    async fn export(
        &self,
        Parameters(p): Parameters<ExportParams>,
    ) -> Result<CallToolResult, McpError> {
        let project = self.lock()?.project().clone();
        let out = match self.resolve_out(&p.path) {
            Ok(o) => o,
            Err(e) => return Ok(fail(e)),
        };
        if woa_engine::export::ExportFormat::from_path(&out).is_none() {
            return Ok(fail("Endung muss .wav, .flac oder .mp3 sein"));
        }
        let mut mix = match audio::render(&project, &self.dir) {
            Ok(m) => m,
            Err(e) => return Ok(fail(e)),
        };
        let normalized = match p.target_lufs.map(|t| audio::normalize_mix(&mut mix, t)) {
            Some(Err(e)) => return Ok(fail(e)),
            Some(Ok(v)) => v,
            None => serde_json::Value::Null,
        };
        if let Some(parent) = out.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            return Ok(fail(format!("{}: {e}", parent.display())));
        }
        if let Err(e) = woa_engine::export::export_stereo(&out, &mix.left, &mix.right, audio::RATE)
        {
            return Ok(fail(e));
        }
        let mut result = audio::measure(&mix);
        result["path"] = serde_json::Value::String(out.display().to_string());
        if let Some(t) = p.target_lufs {
            let reached = result["lufs"]
                .as_f64()
                .is_some_and(|l| (l - t).abs() <= 0.5);
            result["target_reached"] = serde_json::Value::Bool(reached);
        }
        result["normalized"] = normalized;
        Ok(ok_json(result))
    }

    #[tool(description = "Undo the last step (the agent's or the human's).")]
    async fn undo(&self) -> Result<CallToolResult, McpError> {
        let mut s = self.lock()?;
        Ok(match s.undo() {
            Ok(()) => ok_json(serde_json::json!({ "undo": s.history().undo_label() })),
            Err(e) => fail(e),
        })
    }

    #[tool(description = "Redo the last undone step.")]
    async fn redo(&self) -> Result<CallToolResult, McpError> {
        let mut s = self.lock()?;
        Ok(match s.redo() {
            Ok(()) => ok_json(serde_json::json!({ "redo": s.history().redo_label() })),
            Err(e) => fail(e),
        })
    }

    #[tool(description = "Save the project to its folder (atomic write).")]
    async fn save(&self) -> Result<CallToolResult, McpError> {
        let mut s = self.lock()?;
        Ok(match s.save_as(&self.dir) {
            Ok(()) => ok_json(serde_json::json!({ "saved": self.dir.display().to_string() })),
            Err(e) => fail(e),
        })
    }
}

#[tool_handler]
impl ServerHandler for WoaAgent {
    fn get_info(&self) -> ServerConfig {
        let mut info = rmcp::model::Implementation::from_build_env();
        info.name = "woa".to_string();
        info.version = env!("CARGO_PKG_VERSION").to_string();
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(info)
            .with_instructions(
                "WOA (Wir on Air) DAW. Every tool call is one undo step labelled \"Agent\". \
                 Use project_info for ids, then edit; generate_drums and generate_notes \
                 make sound, repeat_clip builds loops, analyze measures, export writes \
                 the mixdown (target_lufs normalises; if target_reached is false, lower \
                 the drum track gain a few dB and export again); call save to write the \
                 project.",
            )
    }
}

/// Serves `agent` over any async byte transport until the client disconnects.
pub async fn serve<R, W>(agent: WoaAgent, transport: (R, W)) -> Result<(), AgentError>
where
    R: tokio::io::AsyncRead + Send + Unpin + 'static,
    W: tokio::io::AsyncWrite + Send + Unpin + 'static,
{
    let running = agent
        .serve(transport)
        .await
        .map_err(|e| AgentError::Transport(e.to_string()))?;
    running
        .waiting()
        .await
        .map_err(|e| AgentError::Transport(e.to_string()))?;
    Ok(())
}

/// Opens (or starts) the project at `dir` and serves MCP over stdin/stdout. Blocks
/// until the client closes stdin. Nothing else may write to stdout meanwhile.
pub fn run_stdio(dir: impl AsRef<Path>) -> Result<(), AgentError> {
    let agent = WoaAgent::open_or_create(dir)?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| AgentError::Transport(e.to_string()))?;
    rt.block_on(serve(agent, rmcp::transport::stdio()))
}
