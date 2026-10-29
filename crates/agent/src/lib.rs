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
                },
                index: None,
            })?;
            Ok(serde_json::json!({ "clip": id.0 }))
        })
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
                 Use project_info for ids, then edit; call save to write the project.",
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
