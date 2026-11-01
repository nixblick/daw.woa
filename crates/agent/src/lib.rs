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
use woa_model::{
    AutoMode, AutoPoint, AutoTarget, Clip, ClipId, Command, EffectKind, FadeCurve, Insert,
    InsertId, ModelError, Send as SendRoute, Session, TrackId, TrackKind, TrackSound, VOICE_CHAIN,
    store,
};

pub mod audio;
pub mod freesound;
pub mod http;
pub mod jingle;
pub mod live;

use audio::{AgentFxKind, AgentNote, AgentWaveform};

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

/// Effect kind as seen by the agent.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentEffectKind {
    /// Highpass plus one bell band: highpass_hz (0 = off, up to 1000), bell_hz (40..16000),
    /// bell_db (-18..18), bell_q (0.1..10).
    Eq,
    /// Compressor: threshold_db (-60..0), ratio (1..20), attack_ms (0.1..200),
    /// release_ms (5..2000), makeup_db (0..24).
    Compressor,
    /// Peak limiter: ceiling_db (-24..0), release_ms (5..1000).
    Limiter,
    /// Noise gate: threshold_db (-80..0), range_db (0..80), attack_ms (0.1..50), hold_ms (0..500),
    /// release_ms (5..2000).
    Gate,
    /// De-esser: frequency_hz, threshold_db, ratio, max_reduction_db (see the parameter table).
    DeEsser,
    /// Reverb: decay_s, predelay_ms, damping_pct, mix_pct.
    Reverb,
    /// Delay, free or tempo-synced (`sync` = note value index): time_ms, sync, feedback_pct,
    /// tone_hz, pingpong, mix_pct.
    Delay,
    /// Chorus: rate_hz, depth_ms, delay_ms, mix_pct.
    Chorus,
    /// Saturation: drive_db, shape (0 soft, 1 tube, 2 hard), output_db, mix_pct.
    Saturation,
    /// Noise suppression (RNNoise, 48 kHz only): amount_pct.
    Denoise,
}

impl From<AgentEffectKind> for EffectKind {
    fn from(k: AgentEffectKind) -> Self {
        match k {
            AgentEffectKind::Eq => EffectKind::Eq,
            AgentEffectKind::Compressor => EffectKind::Compressor,
            AgentEffectKind::Limiter => EffectKind::Limiter,
            AgentEffectKind::Gate => EffectKind::Gate,
            AgentEffectKind::DeEsser => EffectKind::DeEsser,
            AgentEffectKind::Reverb => EffectKind::Reverb,
            AgentEffectKind::Delay => EffectKind::Delay,
            AgentEffectKind::Chorus => EffectKind::Chorus,
            AgentEffectKind::Saturation => EffectKind::Saturation,
            AgentEffectKind::Denoise => EffectKind::Denoise,
        }
    }
}

/// Parameters of `insert_voice_chain`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct InsertVoiceChainParams {
    /// Track id (audio, MIDI or bus).
    pub track: u64,
}

/// Parameters of `voice_over`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct VoiceOverParams {
    /// Track id of the voice (audio or MIDI track, recorded or imported).
    pub voice: u64,
}

/// Parameters of `list_effects`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListEffectsParams {
    /// Only this effect kind; omit for all.
    #[serde(default)]
    pub kind: Option<AgentEffectKind>,
}

/// Parameters of `insert_effect`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct InsertEffectParams {
    /// Track id (audio, MIDI or bus; CLAP effects not on buses).
    pub track: u64,
    /// Built-in effect kind. Give either `kind` or `clap_id`.
    #[serde(default)]
    pub kind: Option<AgentEffectKind>,
    /// CLAP plugin id of an effect (see `list_instruments` with `clap: true`), instead of `kind`.
    /// The project stores only the id; a plugin missing on this computer passes the signal through.
    #[serde(default)]
    pub clap_id: Option<String>,
    /// Display name of the CLAP effect. Default: the name from the last plugin scan, else the id.
    #[serde(default)]
    pub name: Option<String>,
    /// Position in the chain, 0 = first. Default: at the end.
    #[serde(default)]
    pub index: Option<usize>,
    /// Optional preset name (see `list_effects`); `params` are applied on top of it.
    #[serde(default)]
    pub preset: Option<String>,
    /// Optional start values by parameter name; unnamed parameters keep their defaults.
    #[serde(default)]
    pub params: std::collections::BTreeMap<String, f64>,
}

/// Parameters of `set_param`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetParamParams {
    /// Insert id (from `insert_effect` or `project_info`).
    pub insert: u64,
    /// Parameter name, e.g. `bell_db` (see `insert_effect` for the lists).
    pub param: String,
    /// New value within the parameter's range.
    pub value: f64,
}

/// Parameters of `set_insert_bypass`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetInsertBypassParams {
    /// Insert id.
    pub insert: u64,
    /// True bypasses the effect.
    pub bypass: bool,
}

/// Parameters of `remove_insert`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RemoveInsertParams {
    /// Insert id.
    pub insert: u64,
}

/// Parameters of `set_output`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetOutputParams {
    /// Track id (audio, MIDI or bus).
    pub track: u64,
    /// Destination bus id; omit or null for the master.
    #[serde(default)]
    pub output: Option<u64>,
}

/// Parameters of `set_send`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetSendParams {
    /// Source track id.
    pub track: u64,
    /// Destination bus id.
    pub target: u64,
    /// Send level in dB, -96 (off) to 12.
    pub level_db: f64,
    /// True taps before the fader (mute and solo still apply); default after it.
    #[serde(default)]
    pub pre_fader: bool,
}

/// Parameters of `remove_send`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RemoveSendParams {
    /// Source track id.
    pub track: u64,
    /// Destination bus id.
    pub target: u64,
}

/// What an automation lane controls.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentAutoTarget {
    /// Track volume in dB.
    Gain,
    /// Pan, -1 (left) to 1 (right).
    Pan,
    /// Level of the send to the bus `send_target`, in dB.
    Send,
    /// A parameter of the effect `insert`, named by `param`.
    Param,
}

/// Mode of an automation lane.
#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentAutoMode {
    /// The curve is played (default).
    #[default]
    Read,
    /// The next run records over the curve (controller or fader).
    Write,
    /// Recording starts at the first touch and lasts until stop.
    Latch,
}

impl From<AgentAutoMode> for AutoMode {
    fn from(m: AgentAutoMode) -> Self {
        match m {
            AgentAutoMode::Read => AutoMode::Read,
            AgentAutoMode::Write => AutoMode::Write,
            AgentAutoMode::Latch => AutoMode::Latch,
        }
    }
}

/// One automation point.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
pub struct AgentAutoPoint {
    /// Position in beats, 0 or later.
    pub beat: f64,
    /// Value within the range of the target (dB, pan, or the effect's parameter range).
    pub value: f64,
}

/// Selects the target of an automation lane.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AutomationTargetParams {
    /// Track id (audio, MIDI or bus) that owns the target.
    pub track: u64,
    /// What to automate.
    pub target: AgentAutoTarget,
    /// For `send`: the bus id of the send.
    #[serde(default)]
    pub send_target: Option<u64>,
    /// For `param`: the insert id of the effect.
    #[serde(default)]
    pub insert: Option<u64>,
    /// For `param`: the parameter name, e.g. `makeup_db` (see `list_effects`).
    #[serde(default)]
    pub param: Option<String>,
}

/// Parameters of `add_automation`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddAutomationParams {
    /// The target of the lane.
    #[serde(flatten)]
    pub target: AutomationTargetParams,
    /// The points; they are sorted, a second point on the same beat replaces the first.
    /// Between points the value glides linearly (volume and sends in dB); choice parameters
    /// hold until the next point; before the first and after the last point the nearest point
    /// holds. At most 4096 points.
    pub points: Vec<AgentAutoPoint>,
    /// `read` (default), `write` or `latch`.
    #[serde(default)]
    pub mode: AgentAutoMode,
}

/// Parameters of `set_sidechain`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetSidechainParams {
    /// Insert id of a compressor or gate on an audio or MIDI track.
    pub insert: u64,
    /// Track whose signal (after its fader, before its pan) measures the level; omit or null
    /// for the effect's own signal. A source that would form a loop is rejected.
    #[serde(default)]
    pub source: Option<u64>,
}

/// Resolves the target of an automation lane against the project.
fn resolve_auto_target(
    s: &Session,
    p: &AutomationTargetParams,
) -> Result<(TrackId, AutoTarget), ModelError> {
    let track = TrackId(p.track);
    let missing = |what: &str| ModelError::InvalidArgument(format!("{what} fehlt"));
    let target = match p.target {
        AgentAutoTarget::Gain => AutoTarget::Gain,
        AgentAutoTarget::Pan => AutoTarget::Pan,
        AgentAutoTarget::Send => AutoTarget::Send {
            target: TrackId(p.send_target.ok_or_else(|| missing("send_target"))?),
        },
        AgentAutoTarget::Param => {
            let insert = InsertId(p.insert.ok_or_else(|| missing("insert"))?);
            let name = p.param.as_deref().ok_or_else(|| missing("param"))?;
            let (owner, kind) = find_insert(s, insert)?;
            if owner != track {
                return Err(ModelError::InvalidArgument(format!(
                    "Effekt {} liegt nicht auf Spur {}",
                    insert.0, p.track
                )));
            }
            let param = kind
                .param_index(name)
                .ok_or_else(|| ModelError::UnknownParamName(name.to_string()))?;
            AutoTarget::InsertParam { insert, param }
        }
    };
    Ok((track, target))
}

/// Spur eines CLAP-Plugins mit dieser Kennung (teilt den Zahlenraum mit Inserts).
fn plugin_track(s: &Session, id: InsertId) -> Option<TrackId> {
    s.project()
        .tracks
        .iter()
        .find(|t| t.plugins.iter().any(|p| p.id == id))
        .map(|t| t.id)
}

/// Finds the track that holds `insert`, and the effect kind.
fn find_insert(s: &Session, insert: InsertId) -> Result<(TrackId, EffectKind), ModelError> {
    s.project()
        .tracks
        .iter()
        .find_map(|t| {
            t.inserts
                .iter()
                .find(|i| i.id == insert)
                .map(|i| (t.id, i.kind))
        })
        .ok_or(ModelError::InsertNotFound(insert))
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
    /// Track that gets the new clip: an audio track, or a MIDI track with `midi: true`.
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
    /// `true`: store the notes as a MIDI clip on a MIDI track (editable, played by the
    /// track instrument; `waveform` is ignored). Default `false`: render to audio.
    #[serde(default)]
    pub midi: bool,
}

/// Parameters of `generate_chords`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateChordsParams {
    /// Progression from the catalog (same as the app window "Akkordfolge"), e.g.
    /// "I–V–vi–IV", "I–IV–V–I", "vi–IV–I–V", "i–VI–III–VII"; "-" works for "–".
    /// An unknown name returns the list.
    pub progression: String,
    /// Key root: C, C#, D, Eb, E, F, F#, G, Ab, A, Bb, B (also Db, A#, ...). Default C.
    /// Minor progressions (lower-case i) use natural minor of this root.
    #[serde(default)]
    pub key: Option<String>,
    /// Length in 4/4 bars, one chord per bar, the progression repeats. Default 4.
    #[serde(default)]
    pub bars: Option<u32>,
    /// Clip start on the timeline in beats.
    #[serde(default)]
    pub start_beats: f64,
}

/// Parameters of `transpose_notes`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct TransposeNotesParams {
    /// MIDI clip ids, e.g. the three clips of generate_chords.
    pub clips: Vec<u64>,
    /// Semitones, -24..=24 (key change C to D = 2).
    pub semitones: i8,
}

/// Parameters of `edit_notes`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct EditNotesParams {
    /// MIDI clip id.
    pub clip: u64,
    /// The complete new note list (times relative to the clip start, inside the clip).
    /// Controller values (pedal) stay as they are.
    pub notes: Vec<AgentNote>,
}

/// Parameters of `quantize_notes`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct QuantizeNotesParams {
    /// MIDI clip id.
    pub clip: u64,
    /// Grid as note value: 4 = quarter, 8 = eighth, 16 = sixteenth, 32 = 32nd.
    pub division: u32,
    /// Triplet grid (two thirds of the step).
    #[serde(default)]
    pub triplet: bool,
    /// Also snap the note ends (default: keep the lengths).
    #[serde(default)]
    pub ends: bool,
    /// Indices (as returned by read_notes) of the notes to quantize; default all.
    #[serde(default)]
    pub notes: Option<Vec<usize>>,
}

/// Parameters of `read_notes`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadNotesParams {
    /// MIDI clip id.
    pub clip: u64,
}

/// Parameters of `generate_fx`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateFxParams {
    /// Audio track that gets the new clip.
    pub track: u64,
    /// riser (builds up before a hit), whoosh (transition), downlifter (falls after a hit).
    #[serde(default)]
    pub kind: AgentFxKind,
    /// Length in beats; default 4 (one 4/4 bar). The clip fits the project tempo.
    #[serde(default = "default_fx_beats")]
    pub length_beats: f64,
    /// Clip start on the timeline in beats.
    #[serde(default)]
    pub start_beats: f64,
    /// Clip name; default the kind ("Riser", "Whoosh", "Downlifter").
    #[serde(default)]
    pub name: Option<String>,
}

fn default_fx_beats() -> f64 {
    4.0
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
    /// `true`: write GM drum notes (36 kick, 38 snare, 42 hi-hat) as a MIDI clip on a
    /// MIDI track and give the track the GM standard kit (played by the SoundFont if one
    /// is installed). Default `false`: render synthetic drums to audio.
    #[serde(default)]
    pub midi: bool,
    /// Starter pattern instead of kick/snare/hihat (same presets as the drum grid in the
    /// app): "Pop", "Four on the Floor", "Rock", "Hip-Hop", "Nachrichten", "Party",
    /// "Halbes Tempo", "Shuffle". Implies `midi: true` and adds clap, open hi-hat and crash.
    #[serde(default)]
    pub preset: Option<String>,
    /// With `preset`: -1 less, 0 as is, 1 more (hi-hat on every 16th).
    #[serde(default)]
    pub density: i8,
    /// With `preset`: clip length in 4/4 bars, default 1.
    #[serde(default)]
    pub bars: Option<u32>,
}

/// Parameters of `track_sound`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct TrackSoundParams {
    /// MIDI track id.
    pub track: u64,
    /// Set this preset by name (see the returned `presets`, e.g. "E-Bass", "Standard-Kit").
    #[serde(default)]
    pub preset: Option<String>,
    /// Set this GM program 0..=127 (with `drums: true` the kit number, 0 = standard).
    #[serde(default)]
    pub program: Option<u8>,
    /// With `program`: drum kit on MIDI channel 10.
    #[serde(default)]
    pub drums: bool,
    /// `true`: back to the built-in synth.
    #[serde(default)]
    pub synth: bool,
    /// Sound bank (see the returned `banks`): "musescore-general", "avl-red-zeppelin" (drums only)
    /// or "generaluser" (the built-in default). A missing bank is downloaded in the background on
    /// first use (`bank_state`: downloading); until it is ready, and without network, the track
    /// plays GeneralUser GS. Without preset/program it keeps the current sound (piano or kit if none).
    #[serde(default)]
    pub bank: Option<String>,
}

/// Parameters of `import_audio`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImportAudioParams {
    /// Audio track that gets the new clip.
    pub track: u64,
    /// Absolute path of an audio file (WAV, FLAC, MP3, MP2, OGG/Vorbis, M4A/AAC/ALAC, AIFF or CAF); it is copied into the project folder.
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

/// Parameters of `split_clip`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SplitClipParams {
    /// Clip id.
    pub clip: u64,
    /// Cut position on the timeline in beats; must lie inside the clip.
    pub at_beats: f64,
}

/// Parameters of `set_fade_curves`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetFadeCurvesParams {
    /// Clip id.
    pub clip: u64,
    /// Shape of the fade-in: `linear`, `equal_power` or `s_curve`; default unchanged.
    #[serde(default)]
    pub fade_in: Option<String>,
    /// Shape of the fade-out: `linear`, `equal_power` or `s_curve`; default unchanged.
    #[serde(default)]
    pub fade_out: Option<String>,
}

/// Parameters of `set_clip_gain`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetClipGainParams {
    /// Clip id.
    pub clip: u64,
    /// Clip gain in dB (-96..24), before the track fader.
    pub gain_db: f64,
}

/// Parameters of `stretch_clip`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct StretchClipParams {
    /// Clip id.
    pub clip: u64,
    /// New length of the clip on the timeline in beats; the audio is stretched to fit
    /// (pitch stays). The resulting factor must be between 0.25 and 4.
    pub length_beats: f64,
}

/// Parameters of `set_clip_pitch`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetClipPitchParams {
    /// Clip id.
    pub clip: u64,
    /// Pitch shift in semitones (-24..24), independent of the length.
    pub semitones: f64,
}

/// Parameters of `set_clip_follow_tempo`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetClipFollowTempoParams {
    /// Clip id.
    pub clip: u64,
    /// `true`: the clip's audio belongs to the current project tempo and is stretched
    /// whenever the tempo changes; `false`: the audio keeps its own speed.
    pub follow: bool,
}

/// Parameters of `repeat_clip`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RepeatClipParams {
    /// Clip id.
    pub clip: u64,
    /// Number of copies placed right after the clip, one after another (1..=256).
    pub times: u32,
}

/// Parameters of `analyze`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct AnalyzeParams {
    /// Measure only this track (by id) instead of the whole mix.
    #[serde(default)]
    pub track: Option<u64>,
}

/// Parameters of `spectrogram_png`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SpectrogramParams {
    /// Target file, `.png`. Relative path inside the project folder; absolute paths and `..` are rejected.
    pub path: String,
    /// Draw only this track (by id) instead of the whole mix.
    #[serde(default)]
    pub track: Option<u64>,
    /// Image width in pixels, 16..=4096. Default 800.
    #[serde(default)]
    pub width: Option<u32>,
    /// Image height in pixels, 16..=2048. Default 400.
    #[serde(default)]
    pub height: Option<u32>,
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

/// Parameters of `export_jingle`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExportJingleParams {
    /// File name without extension and without a path, e.g. "Station Ident". An existing
    /// file is never overwritten: the next free name ("Station Ident 2") is used.
    pub name: String,
    /// Target loudness in LUFS. Default: the station's target (radio.json), the same value
    /// the playlist auto-gain aims for.
    #[serde(default)]
    pub target_lufs: Option<f64>,
    /// Cart button 1 to 16 that plays the jingle. Default: none.
    #[serde(default)]
    pub cart: Option<usize>,
}

/// Parameters of `build_playlist`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct BuildPlaylistParams {
    /// Audio files (WAV, FLAC, MP3, MP2, OGG/Vorbis, M4A/AAC/ALAC, AIFF or CAF) in play order. Relative paths are resolved against the project folder.
    pub files: Vec<String>,
    /// Append to the existing playlist instead of replacing it.
    #[serde(default)]
    pub append: bool,
    /// Where to write radio.json: a relative path inside the project folder (same rules as
    /// export: no `..`, no absolute path, no symlink). Default: radio.json in the WOA data folder.
    #[serde(default)]
    pub radio_json: Option<String>,
}

/// Parameters of `set_cue_points`. All times in seconds from the start of the file.
/// A point left out is cleared (the whole set is replaced).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetCuePointsParams {
    /// Title position in the playlist, starting at 1.
    pub position: usize,
    /// Cue-In: the title starts here on air.
    #[serde(default)]
    pub cue_in: Option<f64>,
    /// Cue-Out: the title ends here on air; the remaining time counts to this point.
    #[serde(default)]
    pub cue_out: Option<f64>,
    /// Ramp: end of the intro; the radio layout counts down to it for the presenter.
    #[serde(default)]
    pub ramp: Option<f64>,
    /// Hook: the catchy part, for previewing (PFL from hook).
    #[serde(default)]
    pub hook: Option<f64>,
    /// Fade-Out marker: the crossfade to the next title starts here and lasts until Cue-Out.
    #[serde(default)]
    pub fade_out: Option<f64>,
    /// Same rules as `build_playlist`. Default: radio.json in the WOA data folder.
    #[serde(default)]
    pub radio_json: Option<String>,
}

/// Parameters of `select_cart_set`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SelectCartSetParams {
    /// Cart-wall set 1 to 5.
    pub set: usize,
    /// Same rules as `build_playlist`. Default: radio.json in the WOA data folder.
    #[serde(default)]
    pub radio_json: Option<String>,
}

/// What `rename` renames.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RenameWhat {
    /// A cart-wall set (number 1 to 5).
    CartSet,
    /// A cart of the active set (number 1 to 16).
    Cart,
    /// A title of the active playlist (position from 1).
    Title,
}

/// Parameters of `rename`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RenameParams {
    pub what: RenameWhat,
    /// Set 1-5, cart 1-16 or playlist position from 1.
    pub number: usize,
    /// New name; empty restores the file name (or "Satz n" for a set).
    pub name: String,
    /// Same rules as `build_playlist`. Default: radio.json in the WOA data folder.
    #[serde(default)]
    pub radio_json: Option<String>,
}

/// Loads `radio`, applies `f`, saves. Same functions the app uses.
fn edit_show(
    radio: &Path,
    f: impl FnOnce(&mut woa_radio::RadioShow) -> Result<(), String>,
) -> Result<woa_radio::RadioShow, String> {
    let mut show = woa_radio::RadioShow::load(radio)?;
    f(&mut show)?;
    show.save(radio)?;
    Ok(show)
}

/// Applies a `rename` to `show`.
fn rename_in(show: &mut woa_radio::RadioShow, p: &RenameParams) -> Result<(), String> {
    let i = p
        .number
        .checked_sub(1)
        .ok_or_else(|| "number beginnt bei 1".to_string())?;
    match p.what {
        RenameWhat::CartSet => show.rename_cart_set(i, &p.name),
        RenameWhat::Cart => show.rename_cart(i, &p.name),
        RenameWhat::Title => show.rename_title(i, &p.name),
    }
}

fn cart_wall_json(show: &woa_radio::RadioShow) -> serde_json::Value {
    let carts: Vec<_> = show
        .carts
        .iter()
        .enumerate()
        .map(|(i, c)| {
            serde_json::json!({
                "cart": i + 1,
                "name": c.as_ref().map(|e| e.label.clone()),
                "path": c.as_ref().map(|e| shown(&e.path)),
            })
        })
        .collect();
    serde_json::json!({
        "cart_set": show.cart_set + 1,
        "cart_sets": show.cart_set_names(),
        "carts": carts,
    })
}

/// Parameters of `run_order`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RunOrderParams {
    /// Path of radio.json. Default: radio.json in the WOA data folder.
    #[serde(default)]
    pub radio_json: Option<String>,
}

/// Parameters of `list_instruments`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ListInstrumentsParams {
    /// `true`: also search the CLAP plugins on this computer (each file in its own process,
    /// takes a few seconds; the result is kept until the next `clap: true`).
    #[serde(default)]
    pub clap: bool,
}

/// Parameters of `list_samples`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ListSamplesParams {
    /// Words that must all appear in title, tags or path (prefix match, case does not matter).
    #[serde(default)]
    pub text: Option<String>,
    /// Library kind: `sample`, `jingle` or `music`. Default: all. Ignored with `folder`.
    #[serde(default)]
    pub kind: Option<String>,
    /// Library tag that must be set. Ignored with `folder`.
    #[serde(default)]
    pub tag: Option<String>,
    /// Library database file. Default: the library of the WOA data folder.
    #[serde(default)]
    pub library: Option<String>,
    /// Search this folder (and up to 4 levels below) for audio files instead of the library,
    /// e.g. a Jingle-FX folder. Not together with `library`.
    #[serde(default)]
    pub folder: Option<String>,
    /// Most results, default 100, at most 1000.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Parameters of `search_samples`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchSamplesParams {
    /// Search words, e.g. `riser` or `impact metal`.
    pub text: String,
    /// Most results, default 15, at most 50.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Parameters of `import_sample`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImportSampleParams {
    /// Freesound id from `search_samples`.
    pub id: u64,
    /// Audio track that gets the new clip.
    pub track: u64,
    /// Clip start in beats.
    #[serde(default)]
    pub start_beats: f64,
    /// Clip name; default the sound's name.
    #[serde(default)]
    pub name: Option<String>,
}

/// Parameters of `stream_status`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct StreamStatusParams {
    /// WOA data folder. Default: the standard data folder.
    #[serde(default)]
    pub data_dir: Option<String>,
}

/// Status of woa-streamd from `streamd-status.json`, plus `running`.
pub fn stream_status_json(data_dir: &Path) -> serde_json::Value {
    let path = woa_stream::status::status_path(data_dir);
    match woa_stream::status::read(&path) {
        Some(st) => {
            let running = st.is_running(woa_stream::unix_now());
            let summary = st.summary();
            let mut v = serde_json::to_value(&st).unwrap_or(serde_json::Value::Null);
            if let Some(o) = v.as_object_mut() {
                o.insert("running".into(), running.into());
                o.insert("summary".into(), summary.into());
            }
            v
        }
        None => serde_json::json!({
            "running": false,
            "summary": "nicht auf Sendung",
            "status_file": shown(&path),
        }),
    }
}

/// The MCP server around one project session.
#[derive(Clone)]
pub struct WoaAgent {
    session: Arc<Mutex<Session>>,
    dir: PathBuf,
    /// Takes written by `generate_*` this run; declared after `session` so it drops last.
    takes: Arc<Takes>,
    /// Freesound access for `search_samples` and `import_sample` (J10).
    samples: freesound::Source,
    // Read by the code generated from `#[tool_handler]`.
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

/// WAV files the agent rendered into `aufnahmen/` during this run. When the last handle
/// goes and nobody else holds the session any more (no GUI, no undo history alive), every
/// file that neither the saved `project.json` nor its autosave references is removed, so an
/// unsaved generation leaves nothing behind. Files the user brought are never listed here.
struct Takes {
    dir: PathBuf,
    session: std::sync::Weak<Mutex<Session>>,
    files: Mutex<Vec<(String, PathBuf)>>,
}

impl Takes {
    fn referenced(project: &woa_model::Project, rel: &str) -> bool {
        project
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .any(|c| c.audio.as_deref() == Some(rel))
    }

    /// Removes the unreferenced takes; returns how many.
    fn clean(&self) -> usize {
        if self.session.upgrade().is_some() {
            return 0;
        }
        let saved = store::load(&self.dir).ok();
        let autosave = store::load_autosave(&self.dir).ok().flatten();
        let files = std::mem::take(&mut *self.files.lock().unwrap_or_else(|e| e.into_inner()));
        let mut removed = 0;
        for (rel, abs) in files {
            let kept = [&saved, &autosave]
                .into_iter()
                .flatten()
                .any(|p| Self::referenced(p, &rel));
            if !kept && std::fs::remove_file(&abs).is_ok() {
                removed += 1;
            }
        }
        removed
    }
}

impl Drop for Takes {
    fn drop(&mut self) {
        self.clean();
    }
}

/// Answer of `undo`/`redo`: the step just done plus the next undo and redo step.
fn steps_json(done: &str, step: Option<String>, s: &Session) -> serde_json::Value {
    serde_json::json!({
        done: step,
        "undo": s.history().undo_label(),
        "redo": s.history().redo_label(),
    })
}

/// A path for the client: without the Windows verbatim prefix that `canonicalize` adds
/// (`\\?\C:\x` becomes `C:\x`, `\\?\UNC\host\share` becomes `\\host\share`).
fn shown(path: &Path) -> String {
    strip_verbatim(&path.display().to_string())
}

fn strip_verbatim(s: &str) -> String {
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s.to_string()
    }
}

/// Rate for radio measurements; LUFS does not depend on it.
const RADIO_RATE: u32 = 48_000;

/// Sets the cue points of title `position` (from 1) in `radio` and saves it.
fn set_cues_in(
    radio: &Path,
    position: usize,
    cues: woa_radio::Cues,
) -> Result<(woa_radio::RadioShow, woa_radio::Cues), String> {
    let mut show = woa_radio::RadioShow::load(radio)?;
    let i = position
        .checked_sub(1)
        .ok_or_else(|| "position beginnt bei 1".to_string())?;
    let before = show.set_cues(i, cues)?;
    show.save(radio)?;
    Ok((show, before))
}

fn run_order_json(show: &woa_radio::RadioShow, radio: &Path) -> serde_json::Value {
    let titles: Vec<_> = show
        .playlist
        .iter()
        .enumerate()
        .map(|(i, e)| {
            serde_json::json!({
                "position": i + 1,
                "title": e.title(),
                "path": shown(&e.path),
                "seconds": e.seconds,
                "lufs": e.lufs,
                "gain_db": e.gain_db(show.target_lufs),
                "cues": e.cues,
                "play_seconds": e.play_seconds(),
            })
        })
        .collect();
    serde_json::json!({
        "radio_json": shown(radio),
        "playlist": show.active_name(),
        "playlists": show.playlist_names(),
        "titles": titles,
        "count": show.playlist.len(),
        "total_seconds": show.total_seconds_from(0),
        "crossfade_seconds": show.crossfade_seconds,
        "target_lufs": show.target_lufs,
        "bus_processing": show.bus_processing,
        "mic_processing": show.mic_processing,
    })
}

/// Rendering and measuring block for seconds; they run on tokio's blocking pool so the
/// single-threaded MCP runtime keeps answering other requests meanwhile.
async fn off_runtime<F>(f: F) -> Result<CallToolResult, McpError>
where
    F: FnOnce() -> Result<CallToolResult, McpError> + Send + 'static,
{
    tokio::task::spawn_blocking(f).await.map_err(|e| {
        McpError::internal_error(format!("Hintergrundarbeit abgebrochen: {e}"), None)
    })?
}

fn lock_err() -> McpError {
    McpError::internal_error("Sitzung nicht verfügbar (vergiftete Sperre)", None)
}

/// Klang einer MIDI-Spur als JSON (`null`: eingebauter Synth).
fn sound_json(sound: Option<TrackSound>) -> serde_json::Value {
    match sound {
        None => serde_json::Value::Null,
        Some(x) => serde_json::json!({
            "program": x.program,
            "drums": x.drums,
            "preset": woa_model::preset_name(x),
            "bank": x.bank.map(woa_model::SoundBank::id),
        }),
    }
}

/// Lizenz einer Bank in einer Zeile (Volltext liegt neben der Datei, Paket J7).
fn bank_license(bank: woa_model::SoundBank) -> &'static str {
    match bank {
        woa_model::SoundBank::MusescoreGeneral => {
            "MIT (MuseScore General, S. Christian Collins; FluidR3 Frank Wen)"
        }
        woa_model::SoundBank::AvlRedZeppelin => "CC BY-SA 3.0 (AVL Drumkits, Glen MacArthur)",
        woa_model::SoundBank::Unknown => "",
    }
}

/// Presets, Bänke und SoundFont-Zustand: gemeinsam für `track_sound` und `list_instruments`.
fn sound_catalog_json() -> serde_json::Value {
    let presets: Vec<_> = woa_model::SOUND_PRESETS
        .iter()
        .map(|x| {
            serde_json::json!({
                "role": x.role.name(),
                "name": x.name,
                "program": x.sound.program,
                "drums": x.sound.drums,
            })
        })
        .collect();
    let banks: Vec<_> = woa_model::SoundBank::ALL
        .iter()
        .map(|&b| {
            let entry = woa_engine::banks::entry(b);
            serde_json::json!({
                "id": b.id(),
                "name": b.name(),
                "drums_only": b.drums_only(),
                "state": woa_engine::banks::state(b).name(),
                "bytes": entry.map(|e| e.bytes),
                "license": bank_license(b),
            })
        })
        .collect();
    let available = woa_engine::soundfont::default_soundfont().is_some();
    serde_json::json!({
        "default_bank": {
            "id": "generaluser",
            "name": "GeneralUser GS",
            "available": available,
            "license": "GeneralUser GS License v2.0",
        },
        "soundfont_available": available,
        "presets": presets,
        "banks": banks,
    })
}

/// Zuletzt gefundene CLAP-Plugins (`list_instruments` mit `clap: true`).
static CLAP_CATALOG: Mutex<Option<Vec<woa_plugins::PluginInfo>>> = Mutex::new(None);

/// Sucht CLAP-Plugins nur geschützt: je Datei ein eigener Prozess (`woa-plugin-host`), mit der
/// Sperrliste der App. Ohne Plugin-Prozess wird nicht gesucht; eine kaputte Bibliothek darf den
/// MCP-Server nicht mitreißen.
fn scan_clap() -> Result<Vec<woa_plugins::PluginInfo>, String> {
    let host = woa_plugins::HostConfig::discover()
        .ok_or("kein Plugin-Prozess (woa-plugin-host) gefunden, Suche ausgelassen")?;
    let path = woa_plugins::Blocklist::default_path(&woa_radio::config_dir());
    let mut blocklist = woa_plugins::Blocklist::load(&path);
    let catalog = woa_plugins::Catalog::scan_default_guarded(&host, &mut blocklist);
    Ok(catalog.plugins().to_vec())
}

fn plugin_json(p: &woa_plugins::PluginInfo) -> serde_json::Value {
    serde_json::json!({"clap_id": p.clap_id, "name": p.name, "vendor": p.vendor})
}

/// Audiodateien unter `root` (höchstens 4 Ebenen, 5000 Dateien), deren Name `words` enthält.
fn audio_files(root: &Path, words: &[String], limit: usize) -> Vec<PathBuf> {
    const EXT: [&str; 9] = [
        "wav", "flac", "mp3", "mp2", "ogg", "m4a", "aif", "aiff", "caf",
    ];
    let mut out = Vec::new();
    let mut seen = 0usize;
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            seen += 1;
            if seen > 5000 || out.len() >= limit {
                return out;
            }
            if path.is_dir() {
                if depth < 4 && !path.is_symlink() {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let ext_ok = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| EXT.contains(&e.to_ascii_lowercase().as_str()));
            let name = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_lowercase();
            if ext_ok && words.iter().all(|w| name.contains(w.as_str())) {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Liegt `path` unter einem der Ordner? `..` wird abgelehnt, Symlinks werden aufgelöst: der
/// nächste vorhandene Vorfahr zählt, die Datei selbst darf fehlen.
fn inside_any(path: &Path, roots: &[&Path]) -> bool {
    if path
        .components()
        .any(|c| c == std::path::Component::ParentDir)
    {
        return false;
    }
    let mut base = path;
    let mut rest = Vec::new();
    let resolved = loop {
        if let Ok(b) = base.canonicalize() {
            break rest.iter().rev().fold(b, |acc: PathBuf, n| acc.join(n));
        }
        match (base.parent(), base.file_name()) {
            (Some(p), Some(n)) => {
                rest.push(n.to_os_string());
                base = p;
            }
            _ => return false,
        }
    };
    roots
        .iter()
        .filter_map(|r| r.canonicalize().ok())
        .any(|r| resolved.starts_with(r))
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
        let session = Arc::new(Mutex::new(session));
        let takes = Arc::new(Takes {
            dir: dir.clone(),
            session: Arc::downgrade(&session),
            files: Mutex::new(Vec::new()),
        });
        Self {
            session,
            takes,
            dir,
            samples: freesound::Source::default(),
            tool_router: Self::tool_router(),
        }
    }

    /// Replaces the Freesound access (address and key source), e.g. for tests.
    pub fn with_sample_source(mut self, source: freesound::Source) -> Self {
        self.samples = source;
        self
    }

    /// Shared handle to the session, e.g. for a GUI that shows the agent's work live.
    pub fn session(&self) -> Arc<Mutex<Session>> {
        Arc::clone(&self.session)
    }

    fn build_playlist_at(
        &self,
        radio: &Path,
        files: &[String],
        append: bool,
    ) -> Result<woa_radio::RadioShow, String> {
        let mut show = woa_radio::RadioShow::load(radio)?;
        let mut entries = Vec::new();
        for f in files {
            let path = self.resolve_media(f);
            if !woa_radio::audio::is_supported(&path) {
                return Err(woa_radio::audio::unsupported(&path));
            }
            let measured = woa_radio::audio::measure_file(&path, RADIO_RATE)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            let mut entry = woa_radio::PlaylistEntry::new(path);
            entry.set_measured(measured);
            entries.push(entry);
        }
        if !append {
            show.playlist.clear();
        }
        show.playlist.extend(entries);
        show.save(radio)?;
        Ok(show)
    }

    /// Read path of radio.json (writing goes through `resolve_out`).
    fn radio_path(&self, given: Option<&str>) -> PathBuf {
        match given {
            Some(g) => self.resolve_media(g),
            None => woa_radio::default_radio_path(),
        }
    }

    /// Relative paths count from the project folder.
    fn resolve_media(&self, given: &str) -> PathBuf {
        let p = Path::new(given);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.dir.join(p)
        }
    }

    /// Renders the whole project, or one track alone when `track` is given.
    fn render_for(
        &self,
        project: &woa_model::Project,
        track: Option<u64>,
    ) -> Result<audio::Mix, String> {
        match track {
            None => audio::render(project, &self.dir),
            Some(id) => {
                if !project.tracks.iter().any(|t| t.id.0 == id) {
                    return Err(format!("Spur {id} gibt es nicht"));
                }
                audio::render_track(project, &self.dir, TrackId(id))
            }
        }
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
        self.takes
            .files
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((rel.clone(), abs));
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

    /// `generate_notes` with `midi: true`: a MIDI clip on a MIDI track (one undo step).
    fn add_midi_clip(s: &mut Session, p: GenerateNotesParams) -> CallToolResult {
        match s.project().track(TrackId(p.track)) {
            None => return fail(ModelError::TrackNotFound(TrackId(p.track))),
            Some(t) if t.kind != TrackKind::Midi => {
                return fail(format!("Spur {} ist keine MIDI-Spur", p.track));
            }
            Some(_) => {}
        }
        if !(p.start_beats.is_finite() && p.start_beats >= 0.0) {
            return fail(format!("start_beats muss >= 0 sein, war {}", p.start_beats));
        }
        let midi = match audio::notes_midi(&p.notes, p.length_beats) {
            Ok(m) => m,
            Err(e) => return fail(e),
        };
        let notes = midi.notes.len();
        let id = s.new_clip_id();
        let name = p.name.unwrap_or_else(|| "MIDI".into());
        let clip = Clip::new_midi(id, name, p.start_beats, p.length_beats, midi);
        let r = s.group(AGENT_LABEL, |s| {
            s.apply(Command::AddClip {
                track: TrackId(p.track),
                clip,
                index: None,
            })
        });
        match r {
            Ok(()) => ok_json(serde_json::json!({
                "clip": id.0,
                "midi": true,
                "notes": notes,
                "length_beats": audio::round(p.length_beats, 4),
            })),
            Err(e) => fail(e),
        }
    }

    fn add_drum_midi(s: &mut Session, p: GenerateDrumsParams) -> CallToolResult {
        let track = TrackId(p.track);
        let current = match s.project().track(track) {
            None => return fail(ModelError::TrackNotFound(track)),
            Some(t) if t.kind != TrackKind::Midi => {
                return fail(format!("Spur {} ist keine MIDI-Spur", p.track));
            }
            Some(t) => t.sound,
        };
        if !(p.start_beats.is_finite() && p.start_beats >= 0.0) {
            return fail(format!("start_beats muss >= 0 sein, war {}", p.start_beats));
        }
        let built = match p.preset.as_deref() {
            Some(name) => match woa_model::drum_preset(name) {
                Some(preset) => {
                    let length = f64::from(p.bars.unwrap_or(1).clamp(1, 64)) * 4.0;
                    Ok((
                        woa_model::drums::apply_preset(
                            &woa_model::MidiData::default(),
                            length,
                            preset,
                            p.density.clamp(-1, 1),
                        ),
                        length,
                    ))
                }
                None => Err(format!(
                    "unbekanntes Preset {name:?}, bekannt: {}",
                    woa_model::DRUM_PRESETS
                        .iter()
                        .map(|p| p.name)
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
            },
            None => audio::drums_midi(&p.kick, &p.snare, &p.hihat),
        };
        let (midi, length) = match built {
            Ok(m) => m,
            Err(e) => return fail(e),
        };
        let notes = midi.notes.len();
        let id = s.new_clip_id();
        let name = p.name.unwrap_or_else(|| "Drums".into());
        let clip = Clip::new_midi(id, name, p.start_beats, length, midi);
        let kit = current.filter(|k| k.drums).unwrap_or(TrackSound {
            program: 0,
            drums: true,
            bank: None,
        });
        let r = s.group(AGENT_LABEL, |s| {
            if current != Some(kit) {
                s.apply(Command::SetTrackSound {
                    track,
                    sound: Some(kit),
                })?;
            }
            s.apply(Command::AddClip {
                track,
                clip,
                index: None,
            })
        });
        match r {
            Ok(()) => ok_json(serde_json::json!({
                "clip": id.0,
                "midi": true,
                "notes": notes,
                "length_beats": audio::round(length, 4),
                "sound": sound_json(Some(kit)),
            })),
            Err(e) => fail(e),
        }
    }

    fn set_notes(
        s: &mut Session,
        clip: ClipId,
        midi: woa_model::MidiData,
        n: usize,
    ) -> CallToolResult {
        match s.group(AGENT_LABEL, |s| s.apply(Command::SetNotes { clip, midi })) {
            Ok(()) => ok_json(serde_json::json!({ "clip": clip.0, "notes": n })),
            Err(e) => fail(e),
        }
    }

    fn clip_of(s: &Session, clip: u64) -> Result<Clip, ModelError> {
        s.project()
            .clip(ClipId(clip))
            .map(|(_, c)| c.clone())
            .ok_or(ModelError::ClipNotFound(ClipId(clip)))
    }

    /// Zielpfad im Projektordner. Absolute Pfade und `..` sind verboten.
    /// Der tiefste vorhandene Vorfahr muss kanonisch im Projekt liegen, fehlende
    /// Ordner werden einzeln angelegt und sofort erneut geprüft, und das Ziel
    /// darf kein Symlink sein. Gibt den Zielpfad im geprüften Ordner zurück und die
    /// Ordner, die dabei neu angelegt wurden (äußerster zuerst), damit ein späterer
    /// Fehler sie wieder entfernen kann.
    fn resolve_out(&self, path: &str) -> Result<(PathBuf, Vec<PathBuf>), String> {
        let mut created = Vec::new();
        let r = self.resolve_out_inner(path, &mut created);
        if r.is_err() {
            remove_created(&created);
        }
        r.map(|out| (out, created))
    }

    fn resolve_out_inner(&self, path: &str, created: &mut Vec<PathBuf>) -> Result<PathBuf, String> {
        if !woa_model::is_safe_audio_path(path) {
            return Err(format!(
                "{path}: nur relative Pfade innerhalb des Projektordners erlaubt"
            ));
        }
        let leaves = || format!("{path}: Pfad verlässt den Projektordner");
        let root = self.dir.canonicalize().map_err(|e| e.to_string())?;
        let rel = Path::new(path);
        let name = rel.file_name().ok_or_else(leaves)?;
        let mut dir = root.clone();
        if let Some(parent) = rel.parent() {
            for comp in parent.components() {
                let std::path::Component::Normal(c) = comp else {
                    return Err(leaves());
                };
                let next = dir.join(c);
                match std::fs::symlink_metadata(&next) {
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        match std::fs::create_dir(&next) {
                            Ok(()) => created.push(next.clone()),
                            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                            Err(e) => return Err(format!("{}: {e}", next.display())),
                        }
                    }
                    Err(e) => return Err(format!("{}: {e}", next.display())),
                }
                let real = next.canonicalize().map_err(|e| e.to_string())?;
                if !real.starts_with(&root) || !real.is_dir() {
                    return Err(leaves());
                }
                dir = real;
            }
        }
        let out = dir.join(name);
        if let Ok(m) = std::fs::symlink_metadata(&out)
            && (m.file_type().is_symlink() || !m.is_file())
        {
            return Err(format!("{path}: Ziel ist ein Symlink oder keine Datei"));
        }
        Ok(out)
    }

    /// Schreibt den Mix in eine neu angelegte Temp-Datei im geprüften Ordner, und zwar
    /// in genau das Handle, das `create_new` geöffnet hat (kein zweites Öffnen über den
    /// Pfad), und setzt sie danach per `rename` an den Zielnamen. `rename` ersetzt einen
    /// inzwischen untergeschobenen Symlink, statt ihm zu folgen.
    fn write_mix(out: &Path, mix: &audio::Mix) -> Result<(), String> {
        let dir = out.parent().ok_or("kein Zielordner")?;
        let format = woa_engine::export::ExportFormat::from_path(out)
            .ok_or(woa_engine::export::EXTENSION_HINT)?;
        let ext = out.extension().and_then(|e| e.to_str()).unwrap_or("wav");
        let mut opened = None;
        for n in 0..100u32 {
            let p = dir.join(format!(".woa-export-{}-{n}.{ext}", std::process::id()));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&p)
            {
                Ok(f) => {
                    opened = Some((p, f));
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("{}: {e}", p.display())),
            }
        }
        let (tmp, file) = opened.ok_or("keine Temp-Datei frei")?;
        let r =
            woa_engine::export::export_stereo_to(&file, format, &mix.left, &mix.right, audio::RATE)
                .map_err(|e| e.to_string())
                .and_then(|()| file.sync_all().map_err(|e| e.to_string()))
                .and_then(|()| std::fs::rename(&tmp, out).map_err(|e| e.to_string()));
        drop(file);
        if r.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        r
    }

    /// Rendert, gleicht an und schreibt; alles nach `resolve_out`.
    fn export_to(
        &self,
        project: &woa_model::Project,
        out: &Path,
        target_lufs: Option<f64>,
    ) -> Result<(audio::Mix, serde_json::Value), String> {
        let mut mix = audio::render(project, &self.dir)?;
        let normalized = match target_lufs {
            Some(t) => audio::normalize_mix(&mut mix, t)?,
            None => serde_json::Value::Null,
        };
        Self::write_mix(out, &mix)?;
        Ok((mix, normalized))
    }
}

/// Entfernt neu angelegte Ordner wieder, innerster zuerst. `remove_dir` löscht nur leere
/// Ordner; was inzwischen jemand anderes hineingelegt hat, bleibt.
fn remove_created(created: &[PathBuf]) {
    for d in created.iter().rev() {
        let _ = std::fs::remove_dir(d);
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
            "path": shown(&self.dir),
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
                    midi: None,
                    fade_in_curve: woa_model::FadeCurve::Linear,
                    fade_out_curve: woa_model::FadeCurve::Linear,
                    gain_db: 0.0,
                    stretch: 1.0,
                    pitch_semitones: 0.0,
                    follow_tempo_bpm: None,
                },
                index: None,
            })?;
            Ok(serde_json::json!({ "clip": id.0 }))
        })
    }

    #[tool(
        description = "Render notes with the built-in synth into a new audio clip (WAV in the project's aufnahmen/ folder). Times in beats relative to the clip start; pitch is MIDI (60 = C4). With midi: true the notes become an editable MIDI clip on a MIDI track instead (played by the track instrument, readable with read_notes). Returns the clip id."
    )]
    async fn generate_notes(
        &self,
        Parameters(p): Parameters<GenerateNotesParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut s = self.lock()?;
        if p.midi {
            return Ok(Self::add_midi_clip(&mut s, p));
        }
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
        description = "Read the notes and controller values (sustain pedal = controller 64) of a MIDI clip: times in beats relative to the clip start, pitch is MIDI (60 = C4)."
    )]
    async fn read_notes(
        &self,
        Parameters(p): Parameters<ReadNotesParams>,
    ) -> Result<CallToolResult, McpError> {
        let s = self.lock()?;
        let Some((track, clip)) = s.project().clip(ClipId(p.clip)) else {
            return Ok(fail(ModelError::ClipNotFound(ClipId(p.clip))));
        };
        let Some(m) = &clip.midi else {
            return Ok(fail(ModelError::NotMidiClip(clip.id)));
        };
        let notes: Vec<_> = m
            .notes
            .iter()
            .map(|n| {
                serde_json::json!({
                    "beat": audio::round(n.beat, 4),
                    "length_beats": audio::round(n.length_beats, 4),
                    "pitch": n.pitch,
                    "velocity": n.velocity,
                })
            })
            .collect();
        let cc: Vec<_> =
            m.cc.iter()
                .map(|c| {
                    serde_json::json!({
                        "beat": audio::round(c.beat, 4),
                        "controller": c.controller,
                        "value": c.value,
                    })
                })
                .collect();
        Ok(ok_json(serde_json::json!({
            "clip": clip.id.0,
            "track": track.id.0,
            "start_beats": clip.start_beats,
            "length_beats": clip.length_beats,
            "notes": notes,
            "cc": cc,
        })))
    }

    #[tool(
        description = "Build chords, bass and arpeggio from a chord progression of the catalog (the same as the app window \"Akkordfolge\"): three new MIDI tracks (Akkorde = pad, Bass, Arpeggio = piano) with one MIDI clip each, all notes in the key, one chord per bar. Change the key afterwards with transpose_notes on the three clips. One undo step. Returns tracks and clips."
    )]
    async fn generate_chords(
        &self,
        Parameters(p): Parameters<GenerateChordsParams>,
    ) -> Result<CallToolResult, McpError> {
        use woa_model::chords;
        let Some(prog) = chords::progression(&p.progression) else {
            return Ok(fail(format!(
                "unbekannte Akkordfolge {:?}, bekannt: {}",
                p.progression,
                chords::CHORD_PROGRESSIONS
                    .iter()
                    .map(|c| c.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        };
        let key_name = p.key.as_deref().unwrap_or("C");
        let Some(key) = chords::key_index(key_name) else {
            return Ok(fail(format!(
                "unbekannte Tonart {key_name:?}, bekannt: {}",
                chords::KEYS.join(", ")
            )));
        };
        if !(p.start_beats.is_finite() && p.start_beats >= 0.0) {
            return Ok(fail(format!(
                "start_beats muss >= 0 sein, war {}",
                p.start_beats
            )));
        }
        let bars = p.bars.unwrap_or(4).clamp(1, 64);
        let length = f64::from(bars) * 4.0;
        let made = chords::generate(prog, key, bars);
        let mut s = self.lock()?;
        let mut parts = Vec::new();
        let r = s.group(AGENT_LABEL, |s| {
            for (role, name, sound_role, midi) in [
                ("chords", "Akkorde", woa_model::SoundRole::Pad, made.chords),
                ("bass", "Bass", woa_model::SoundRole::Bass, made.bass),
                ("arp", "Arpeggio", woa_model::SoundRole::Lead, made.arp),
            ] {
                let track = s.new_track_id();
                s.apply(Command::add_track(track, name.to_string(), TrackKind::Midi))?;
                let sound = woa_model::SOUND_PRESETS
                    .iter()
                    .find(|x| x.role == sound_role)
                    .map(|x| x.sound);
                s.apply(Command::SetTrackSound { track, sound })?;
                let clip = s.new_clip_id();
                let notes = midi.notes.len();
                s.apply(Command::AddClip {
                    track,
                    clip: Clip::new_midi(clip, name, p.start_beats, length, midi),
                    index: None,
                })?;
                parts.push(serde_json::json!({
                    "role": role,
                    "track": track.0,
                    "clip": clip.0,
                    "notes": notes,
                    "sound": sound_json(sound),
                }));
            }
            Ok::<(), ModelError>(())
        });
        Ok(match r {
            Ok(()) => ok_json(serde_json::json!({
                "progression": prog.name,
                "key": chords::KEYS[usize::from(key)],
                "minor": prog.minor,
                "length_beats": audio::round(length, 4),
                "parts": parts,
            })),
            Err(e) => fail(e),
        })
    }

    #[tool(
        description = "Transpose all notes of one or more MIDI clips by semitones (key change, e.g. the three clips of generate_chords). Pitches stay within 0..127. One undo step."
    )]
    async fn transpose_notes(
        &self,
        Parameters(p): Parameters<TransposeNotesParams>,
    ) -> Result<CallToolResult, McpError> {
        if !(-24..=24).contains(&p.semitones) {
            return Ok(fail(format!(
                "semitones muss -24..=24 sein, war {}",
                p.semitones
            )));
        }
        let mut s = self.lock()?;
        let mut todo = Vec::new();
        for &id in &p.clips {
            let clip = match Self::clip_of(&s, id) {
                Ok(c) => c,
                Err(e) => return Ok(fail(e)),
            };
            let Some(midi) = &clip.midi else {
                return Ok(fail(ModelError::NotMidiClip(clip.id)));
            };
            todo.push((clip.id, woa_model::chords::transpose(midi, p.semitones)));
        }
        let n = todo.len();
        let r = s.group(AGENT_LABEL, |s| {
            for (clip, midi) in todo {
                s.apply(Command::SetNotes { clip, midi })?;
            }
            Ok::<(), ModelError>(())
        });
        Ok(match r {
            Ok(()) => ok_json(serde_json::json!({ "clips": n, "semitones": p.semitones })),
            Err(e) => fail(e),
        })
    }

    #[tool(
        description = "Replace the notes of a MIDI clip (piano-roll edit: move, lengthen, delete, change velocity). Read them first with read_notes, change the list, send it back complete. Times in beats relative to the clip start; pedal values stay. One undo step."
    )]
    async fn edit_notes(
        &self,
        Parameters(p): Parameters<EditNotesParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut s = self.lock()?;
        let clip = match Self::clip_of(&s, p.clip) {
            Ok(c) => c,
            Err(e) => return Ok(fail(e)),
        };
        let Some(old) = &clip.midi else {
            return Ok(fail(ModelError::NotMidiClip(clip.id)));
        };
        let mut midi = match audio::notes_midi(&p.notes, clip.length_beats) {
            Ok(m) => m,
            Err(e) => return Ok(fail(e)),
        };
        midi.cc = old.cc.clone();
        let count = midi.notes.len();
        Ok(Self::set_notes(&mut s, clip.id, midi, count))
    }

    #[tool(
        description = "Quantize a MIDI clip: snap note starts (and with ends: true also the ends) to a grid, e.g. division 16 = sixteenth notes, triplet: true for triplets. Without notes all notes are quantized. One undo step."
    )]
    async fn quantize_notes(
        &self,
        Parameters(p): Parameters<QuantizeNotesParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut s = self.lock()?;
        if ![1, 2, 4, 8, 16, 32, 64].contains(&p.division) {
            return Ok(fail(format!(
                "division muss 1, 2, 4, 8, 16, 32 oder 64 sein, war {}",
                p.division
            )));
        }
        let clip = match Self::clip_of(&s, p.clip) {
            Ok(c) => c,
            Err(e) => return Ok(fail(e)),
        };
        let Some(old) = &clip.midi else {
            return Ok(fail(ModelError::NotMidiClip(clip.id)));
        };
        let sel = p.notes.unwrap_or_default();
        if let Some(bad) = sel.iter().find(|&&i| i >= old.notes.len()) {
            return Ok(fail(format!("Note {bad} gibt es nicht")));
        }
        let grid = woa_model::grid_beats(p.division, p.triplet);
        let (midi, _) = old.quantize(&sel, grid, p.ends);
        let count = if sel.is_empty() {
            midi.notes.len()
        } else {
            sel.len()
        };
        Ok(Self::set_notes(&mut s, clip.id, midi, count))
    }

    #[tool(
        description = "Read or set the sound of a MIDI track. Without preset/program/synth/bank it only reads. A sound is a General MIDI program from the SoundFont (GeneralUser GS, or a downloadable bank: see bank and the returned banks) or, with drums, a GM drum kit on channel 10; without a sound, or if no SoundFont is installed (soundfont_available: false), the track plays the built-in synth. Returns the current sound and the presets (four per role: lead, bass, pad, kit). One undo step."
    )]
    async fn track_sound(
        &self,
        Parameters(p): Parameters<TrackSoundParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut s = self.lock()?;
        let track = TrackId(p.track);
        // Bank (Paket J7): `Some(None)` = GeneralUser GS.
        let bank = match p.bank.as_deref() {
            None => None,
            Some(id) if id.eq_ignore_ascii_case("generaluser") || id.is_empty() => Some(None),
            Some(id) => match woa_model::SoundBank::from_id(id) {
                Some(b) => Some(Some(b)),
                None => return Ok(fail(format!("Bank {id:?} gibt es nicht"))),
            },
        };
        let current = s.project().track(track).and_then(|t| t.sound);
        let mut wanted = if p.synth {
            Some(None)
        } else if let Some(name) = &p.preset {
            match woa_model::SOUND_PRESETS
                .iter()
                .find(|x| x.name.eq_ignore_ascii_case(name))
            {
                Some(x) => Some(Some(x.sound)),
                None => return Ok(fail(format!("Preset {name:?} gibt es nicht"))),
            }
        } else {
            p.program.map(|program| {
                Some(TrackSound {
                    program,
                    drums: p.drums,
                    bank: None,
                })
            })
        };
        if !p.synth {
            match bank {
                Some(b) => {
                    let base = wanted.flatten().or(current).unwrap_or(TrackSound {
                        program: 0,
                        drums: b.is_some_and(woa_model::SoundBank::drums_only),
                        bank: None,
                    });
                    wanted = Some(Some(TrackSound { bank: b, ..base }));
                }
                // Neuer Klang ohne `bank`: die Bank bleibt, solange sie passt.
                None => {
                    if let Some(Some(w)) = &mut wanted {
                        w.bank = current
                            .and_then(|c| c.bank)
                            .filter(|b| w.drums || !b.drums_only());
                    }
                }
            }
        }
        if let Some(sound) = wanted
            && let Err(e) = s.group(AGENT_LABEL, |s| {
                s.apply(Command::SetTrackSound { track, sound })
            })
        {
            return Ok(fail(e));
        }
        let t = match s.project().track(track) {
            Some(t) if t.kind == TrackKind::Midi => t,
            Some(_) => return Ok(fail(ModelError::NotMidiTrack(track))),
            None => return Ok(fail(ModelError::TrackNotFound(track))),
        };
        // Erster Gebrauch: fehlt die Bank, lädt sie jetzt im Hintergrund.
        let state = t.sound.and_then(|x| x.bank).map(woa_engine::banks::ensure);
        let mut out = sound_catalog_json();
        out["track"] = t.id.0.into();
        out["sound"] = sound_json(t.sound);
        if let Some(st) = state {
            out["bank_state"] = st.name().into();
            match st {
                woa_engine::banks::BankState::Downloading { done, total } => {
                    out["bank_progress"] = serde_json::json!({"done": done, "total": total});
                }
                woa_engine::banks::BankState::Failed(e) => out["bank_error"] = e.into(),
                _ => {}
            }
        }
        Ok(ok_json(out))
    }

    #[tool(
        description = "Render a drum pattern (synthetic kick, snare, hi-hat) into a new audio clip. One character per 16th note: x = hit, o = soft hit, . = rest; 16 characters = one 4/4 bar. Clip length = longest pattern. With midi: true on a MIDI track the pattern becomes an editable MIDI clip with GM drum notes and the track gets the GM standard kit (see track_sound). preset picks a starter pattern of the in-app drum grid instead (MIDI, with density and bars). Returns the clip id."
    )]
    async fn generate_drums(
        &self,
        Parameters(p): Parameters<GenerateDrumsParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut s = self.lock()?;
        if p.midi || p.preset.is_some() {
            return Ok(Self::add_drum_midi(&mut s, p));
        }
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
        description = "Render a jingle effect from noise and a filter sweep into a new audio clip: riser (builds up into a hit, put it right before), whoosh (transition), downlifter (falls away after a hit). length_beats (default 4) follows the project tempo; peak -3 dBFS. Returns the clip id."
    )]
    async fn generate_fx(
        &self,
        Parameters(p): Parameters<GenerateFxParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut s = self.lock()?;
        if let Err(e) = Self::check_target(&s, p.track, p.start_beats) {
            return Ok(fail(e));
        }
        let buf = match audio::fx_buffer(p.kind, p.length_beats, s.project().tempo_bpm) {
            Ok(b) => b,
            Err(e) => return Ok(fail(e)),
        };
        let name = p.name.unwrap_or_else(|| {
            AgentFxKind::ALL
                .iter()
                .find(|(k, _)| *k == p.kind)
                .map_or("FX", |(_, n)| n)
                .to_owned()
        });
        Ok(self.add_take(&mut s, p.track, name, p.start_beats, p.length_beats, &buf))
    }

    #[tool(
        description = "Copy an audio file (WAV, FLAC, MP3, MP2, OGG/Vorbis, M4A/AAC/ALAC, AIFF or CAF) into the project folder (aufnahmen/) and add it as a clip on an audio track. Returns the clip id and its length in beats."
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
        let ext = src
            .extension()
            .and_then(|e| e.to_str())
            .map_or_else(|| "wav".to_owned(), str::to_ascii_lowercase);
        let (rel, abs) = audio::free_take_path(&self.dir, &stem, &ext);
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

    #[tool(
        description = "Search Freesound for samples (riser, whoosh, impact ...). Only CC0 and CC BY sounds are returned. Needs the user's own Freesound key, stored in WOA under Settings, KI. Returns id, name, author, license, duration, url and tags; put a hit on a track with import_sample."
    )]
    async fn search_samples(
        &self,
        Parameters(p): Parameters<SearchSamplesParams>,
    ) -> Result<CallToolResult, McpError> {
        let text = p.text.trim().to_string();
        if text.is_empty() {
            return Ok(fail("text ist leer"));
        }
        let limit = p.limit.unwrap_or(15).clamp(1, 50);
        let src = self.samples.clone();
        off_runtime(move || {
            Ok(match src.search(&text, limit) {
                Ok(hits) => ok_json(serde_json::json!({
                    "samples": hits.iter().map(freesound::Hit::json).collect::<Vec<_>>(),
                })),
                Err(e) => fail(e),
            })
        })
        .await
    }

    #[tool(
        description = "Download a Freesound sound (id from search_samples, CC0 or CC BY only) as MP3 preview into the project folder (aufnahmen/), add it as a clip on an audio track and write its credit line (title, author, license, link) to notizen.md in the project folder. Returns the clip id and its length in beats."
    )]
    async fn import_sample(
        &self,
        Parameters(p): Parameters<ImportSampleParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = Self::check_target(&*self.lock()?, p.track, p.start_beats) {
            return Ok(fail(e));
        }
        let (src, dir, session) = (self.samples.clone(), self.dir.clone(), self.session());
        off_runtime(move || {
            let hit = match src.sound(p.id) {
                Ok(h) => h,
                Err(e) => return Ok(fail(e)),
            };
            let (rel, abs) = audio::free_take_path(
                &dir,
                &format!("freesound-{}", hit.id),
                &freesound::preview_ext(&hit.preview),
            );
            if let Some(parent) = abs.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                return Ok(fail(format!("{}: {e}", parent.display())));
            }
            if let Err(e) = src.download(&hit, &abs) {
                return Ok(fail(e));
            }
            let mut s = session.lock().map_err(|_| lock_err())?;
            let added = Self::check_target(&s, p.track, p.start_beats).and_then(|()| {
                audio::wav_length_beats(&abs, s.project().tempo_bpm)
            });
            let length = match added {
                Ok(l) => l,
                Err(e) => {
                    let _ = std::fs::remove_file(&abs);
                    return Ok(fail(e));
                }
            };
            let id = s.new_clip_id();
            let name = p.name.unwrap_or_else(|| hit.name.clone());
            let r = Self::add_file_clip(&mut s, id, p.track, name, p.start_beats, length, rel);
            if r.is_error == Some(true) {
                let _ = std::fs::remove_file(&abs);
                return Ok(r);
            }
            if let Err(e) = freesound::append_credit(&dir, &hit) {
                return Ok(fail(format!(
                    "Clip angelegt, aber Herkunftszeile nicht geschrieben ({e}); bitte von Hand nachtragen: {}",
                    hit.credit_line()
                )));
            }
            Ok(r)
        })
        .await
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
        description = "Split a clip at a timeline position (beats, inside the clip) into two clips that play identically to the original. Returns the id of the right part."
    )]
    async fn split_clip(
        &self,
        Parameters(p): Parameters<SplitClipParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            let (track, c) = match s.project().clip(ClipId(p.clip)) {
                Some((t, c)) => (t.id, c.clone()),
                None => return Err(ModelError::ClipNotFound(ClipId(p.clip))),
            };
            let left = p.at_beats - c.start_beats;
            let right = c.end_beats() - p.at_beats;
            if !(left >= 1e-3 && right >= 1e-3) {
                return Err(ModelError::InvalidArgument(format!(
                    "at_beats {} liegt nicht im Clip ({}..{})",
                    p.at_beats,
                    c.start_beats,
                    c.end_beats()
                )));
            }
            let index = s
                .project()
                .track(track)
                .and_then(|t| t.clips.iter().position(|x| x.id == c.id))
                .map(|i| i + 1);
            let id = s.new_clip_id();
            let mut part = c.clone();
            part.id = id;
            part.start_beats = p.at_beats;
            part.length_beats = right;
            part.offset_beats = c.offset_beats + left / c.stretch;
            part.fade_in_beats = 0.0;
            part.fade_out_beats = c.fade_out_beats.min(right);
            s.apply(Command::EditClip {
                clip: c.id,
                length_beats: left,
                offset_beats: c.offset_beats,
                fade_in_beats: c.fade_in_beats.min(left),
                fade_out_beats: 0.0,
            })?;
            s.apply(Command::AddClip {
                track,
                clip: part,
                index,
            })?;
            Ok(serde_json::json!({ "clip": id.0 }))
        })
    }

    #[tool(
        description = "Set the shape of a clip's fades: linear, equal_power or s_curve (omit one to keep it). Overlapping audio clips cross-fade automatically with equal power."
    )]
    async fn set_fade_curves(
        &self,
        Parameters(p): Parameters<SetFadeCurvesParams>,
    ) -> Result<CallToolResult, McpError> {
        fn parse(v: Option<&str>, keep: FadeCurve) -> Result<FadeCurve, ModelError> {
            match v {
                None => Ok(keep),
                Some("linear") => Ok(FadeCurve::Linear),
                Some("equal_power") => Ok(FadeCurve::EqualPower),
                Some("s_curve") => Ok(FadeCurve::SCurve),
                Some(other) => Err(ModelError::InvalidArgument(format!(
                    "unbekannte Kurve {other:?} (linear, equal_power, s_curve)"
                ))),
            }
        }
        self.agent_step(|s| {
            let c = Self::clip_of(s, p.clip)?;
            let fade_in = parse(p.fade_in.as_deref(), c.fade_in_curve)?;
            let fade_out = parse(p.fade_out.as_deref(), c.fade_out_curve)?;
            s.apply(Command::SetClipFadeCurves {
                clip: c.id,
                fade_in,
                fade_out,
            })?;
            Ok(serde_json::json!({ "ok": true }))
        })
    }

    #[tool(description = "Set the gain of a clip in dB (-96..24), applied before the track fader.")]
    async fn set_clip_gain(
        &self,
        Parameters(p): Parameters<SetClipGainParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            s.apply(Command::SetClipGain {
                clip: ClipId(p.clip),
                gain_db: p.gain_db,
            })?;
            Ok(serde_json::json!({ "ok": true }))
        })
    }

    #[tool(
        description = "Time-stretch a clip to a new length in beats; the pitch stays. The stretch factor (new length / length of the audio shown) must be 0.25..4. Fades are shortened to fit."
    )]
    async fn stretch_clip(
        &self,
        Parameters(p): Parameters<StretchClipParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            let c = Self::clip_of(s, p.clip)?;
            if !(p.length_beats.is_finite() && p.length_beats > 0.0) {
                return Err(ModelError::InvalidArgument(format!(
                    "length_beats {} ungültig",
                    p.length_beats
                )));
            }
            let fade_in = c.fade_in_beats.min(p.length_beats);
            let fade_out = c.fade_out_beats.min(p.length_beats - fade_in);
            s.apply(Command::SetClipStretch {
                clip: c.id,
                length_beats: p.length_beats,
                stretch: c.stretch * p.length_beats / c.length_beats,
                fade_in_beats: fade_in,
                fade_out_beats: fade_out,
            })?;
            Ok(serde_json::json!({ "ok": true }))
        })
    }

    #[tool(
        description = "Shift the pitch of a clip in semitones (-24..24) without changing its length."
    )]
    async fn set_clip_pitch(
        &self,
        Parameters(p): Parameters<SetClipPitchParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            s.apply(Command::SetClipPitch {
                clip: ClipId(p.clip),
                semitones: p.semitones,
            })?;
            Ok(serde_json::json!({ "ok": true }))
        })
    }

    #[tool(
        description = "Let a clip follow the project tempo: with follow=true the audio is time-stretched whenever the project tempo changes (pitch stays); follow=false lets it play at its own speed."
    )]
    async fn set_clip_follow_tempo(
        &self,
        Parameters(p): Parameters<SetClipFollowTempoParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            let tempo = s.project().tempo_bpm;
            s.apply(Command::SetClipFollowTempo {
                clip: ClipId(p.clip),
                bpm: p.follow.then_some(tempo),
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

    #[tool(
        description = "Remove a track with all its clips. Sends and outputs that pointed at it are cut in the same undo step."
    )]
    async fn remove_track(
        &self,
        Parameters(p): Parameters<TrackParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            s.remove_track_routed(TrackId(p.track))?;
            Ok(serde_json::json!({ "ok": true }))
        })
    }

    #[tool(
        description = "Insert an effect into the insert chain of a track or bus (before the fader). Kinds: eq, compressor, limiter, gate, de_esser, reverb, delay, chorus, saturation, denoise. `list_effects` returns every parameter with name, range and default, and the presets of each kind. Optional `preset` starts from a named preset; optional `params` sets values by name on top. Returns the insert id for set_param."
    )]
    async fn insert_effect(
        &self,
        Parameters(p): Parameters<InsertEffectParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            let kind = match (p.kind, p.clap_id.as_deref()) {
                (Some(k), None) => EffectKind::from(k),
                (None, Some(clap_id)) => {
                    let clap_id = clap_id.trim();
                    if clap_id.is_empty() {
                        return Err(ModelError::InvalidArgument("clap_id is empty".into()));
                    }
                    let found = CLAP_CATALOG.lock().ok().and_then(|c| {
                        c.as_ref()?
                            .iter()
                            .find(|x| x.clap_id == clap_id)
                            .map(|x| x.name.clone())
                    });
                    let name = p
                        .name
                        .clone()
                        .filter(|n| !n.trim().is_empty())
                        .or(found)
                        .unwrap_or_else(|| clap_id.to_string());
                    let id = s.new_insert_id();
                    s.apply(Command::AddPlugin {
                        track: TrackId(p.track),
                        plugin: woa_model::PluginSlot::new(
                            id,
                            clap_id,
                            name,
                            woa_model::PluginRole::Effect,
                        ),
                        index: p.index,
                    })?;
                    return Ok(serde_json::json!({ "insert": id.0, "clap_id": clap_id }));
                }
                _ => {
                    return Err(ModelError::InvalidArgument(
                        "give either kind or clap_id".into(),
                    ));
                }
            };
            let id = s.new_insert_id();
            let mut insert = match &p.preset {
                Some(name) => Insert::with_preset(id, kind, name)?,
                None => Insert::new(id, kind),
            };
            for (name, value) in &p.params {
                let index = kind
                    .param_index(name)
                    .ok_or_else(|| ModelError::UnknownParamName(name.clone()))?;
                insert.params[index] = *value;
            }
            s.apply(Command::AddInsert {
                track: TrackId(p.track),
                insert,
                index: p.index,
            })?;
            Ok(serde_json::json!({ "insert": id.0 }))
        })
    }

    #[tool(
        description = "List the built-in effects: for each kind its parameters (name, unit, min, max, default, choices) and its presets. Optional `kind` limits the list."
    )]
    async fn list_effects(
        &self,
        Parameters(p): Parameters<ListEffectsParams>,
    ) -> Result<CallToolResult, McpError> {
        let only: Option<EffectKind> = p.kind.map(Into::into);
        let effects: Vec<serde_json::Value> = EffectKind::ALL
            .iter()
            .filter(|k| only.is_none_or(|o| o == **k))
            .map(|k| {
                let params: Vec<serde_json::Value> = k
                    .params()
                    .iter()
                    .map(|s| {
                        serde_json::json!({
                            "name": s.name, "unit": s.unit, "min": s.min, "max": s.max,
                            "default": s.default, "choices": s.choices,
                        })
                    })
                    .collect();
                let presets: Vec<&str> = k.presets().iter().map(|pr| pr.name).collect();
                serde_json::json!({
                    "kind": serde_json::to_value(k).unwrap_or_default(),
                    "label": k.label(), "params": params, "presets": presets,
                })
            })
            .collect();
        Ok(ok_json(serde_json::json!({ "effects": effects })))
    }

    #[tool(
        description = "List what can make sound: the SoundFont banks (default GeneralUser GS plus downloadable banks with state, size and license), the sound presets, and with clap: true the CLAP instruments and effects found on this computer (searched safely, one process per file). Set a sound with track_sound, a CLAP effect with insert_effect clap_id."
    )]
    async fn list_instruments(
        &self,
        Parameters(p): Parameters<ListInstrumentsParams>,
    ) -> Result<CallToolResult, McpError> {
        if p.clap {
            off_runtime(move || {
                let mut out = sound_catalog_json();
                match scan_clap() {
                    Ok(found) => {
                        out["clap"] = serde_json::json!({
                            "instruments": found.iter().filter(|x| x.is_instrument()).map(plugin_json).collect::<Vec<_>>(),
                            "effects": found.iter().filter(|x| x.is_effect()).map(plugin_json).collect::<Vec<_>>(),
                        });
                        if let Ok(mut c) = CLAP_CATALOG.lock() {
                            *c = Some(found);
                        }
                    }
                    Err(e) => out["clap_error"] = e.into(),
                }
                Ok(ok_json(out))
            })
            .await
        } else {
            let mut out = sound_catalog_json();
            out["clap"] = serde_json::Value::Null;
            Ok(ok_json(out))
        }
    }

    #[tool(
        description = "Search samples: the WOA library by kind (sample, jingle, music), tag and name, or with folder an audio folder (e.g. Jingle-FX) by file name. Read only; returns path, title, kind, duration and tags. Put a hit on a track with add_clip. For Freesound use search_samples."
    )]
    async fn list_samples(
        &self,
        Parameters(p): Parameters<ListSamplesParams>,
    ) -> Result<CallToolResult, McpError> {
        let limit = p.limit.unwrap_or(100).clamp(1, 1000);
        let text = p.text.clone().unwrap_or_default();
        if let Some(folder) = &p.folder {
            if p.library.is_some() {
                return Ok(fail("folder und library nicht zusammen"));
            }
            let root = self.resolve_media(folder);
            if !root.is_dir() {
                return Ok(fail(format!("Ordner {} gibt es nicht", shown(&root))));
            }
            let words: Vec<String> = text.split_whitespace().map(str::to_lowercase).collect();
            return off_runtime(move || {
                let samples: Vec<_> = audio_files(&root, &words, limit)
                    .iter()
                    .map(|f| {
                        serde_json::json!({
                            "path": f.to_string_lossy(),
                            "title": f.file_stem().map(|x| x.to_string_lossy().into_owned()),
                        })
                    })
                    .collect();
                Ok(ok_json(serde_json::json!({ "samples": samples })))
            })
            .await;
        }
        let data = woa_radio::data_dir();
        let db = match &p.library {
            Some(l) => {
                let db = self.resolve_media(l);
                if !inside_any(&db, &[&data, &self.dir]) {
                    return Ok(fail("library nur im WOA-Datenordner oder im Projektordner"));
                }
                db
            }
            None => data.join(woa_library::DB_FILE),
        };
        let kind = match p.kind.as_deref() {
            None => None,
            Some(k) => match woa_library::Kind::ALL.iter().find(|x| x.as_str() == k) {
                Some(&x) => Some(x),
                None => {
                    return Ok(fail(format!(
                        "Art {k:?} gibt es nicht (sample, jingle, music)"
                    )));
                }
            },
        };
        let tag = p.tag.clone();
        off_runtime(move || {
            // Nur lesen: eine fehlende Bibliothek wird nicht angelegt.
            if !db.is_file() {
                return Ok(ok_json(
                    serde_json::json!({ "samples": [], "library": false }),
                ));
            }
            let lib = match woa_library::Library::open_read_only(&db) {
                Ok(l) => l,
                Err(e) => return Ok(fail(e)),
            };
            let q = woa_library::Query {
                text,
                tag,
                kind,
                limit,
                ..woa_library::Query::default()
            };
            let hits = match lib.search(&q) {
                Ok(h) => h,
                Err(e) => return Ok(fail(e)),
            };
            let samples: Vec<_> = hits
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "path": t.path,
                        "title": t.title,
                        "kind": t.kind.as_str(),
                        "duration": t.duration,
                        "tags": t.tags,
                    })
                })
                .collect();
            Ok(ok_json(
                serde_json::json!({ "samples": samples, "library": true }),
            ))
        })
        .await
    }

    #[tool(
        description = "Insert the template 'Sprachkette' (voice chain) at the end of the insert chain of a track: highpass EQ, gate, EQ, compressor, de-esser. One undo step. Returns the five insert ids in order."
    )]
    async fn insert_voice_chain(
        &self,
        Parameters(p): Parameters<InsertVoiceChainParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            let mut ids = Vec::new();
            for (kind, values) in VOICE_CHAIN {
                let id = s.new_insert_id();
                let insert = Insert::with_values(id, kind, values)?;
                s.apply(Command::AddInsert {
                    track: TrackId(p.track),
                    insert,
                    index: None,
                })?;
                ids.push(id.0);
            }
            Ok(serde_json::json!({ "inserts": ids }))
        })
    }

    #[tool(
        description = "Voice over the bed (jingle step 'Stimme'): puts the voice chain on the voice track (unless it has one) and ducks every other audio and MIDI track without a voice chain under it with a compressor whose sidechain is the voice (buses cannot take a sidechain). One undo step; calling again adds nothing twice. Returns the chain insert ids and per bed track the ducker. Check the result with analyze."
    )]
    async fn voice_over(
        &self,
        Parameters(p): Parameters<VoiceOverParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            let r = s.voice_over(TrackId(p.voice))?;
            let ducks: Vec<_> = r
                .ducks
                .iter()
                .map(|(t, i)| serde_json::json!({ "track": t.0, "insert": i.0 }))
                .collect();
            let chain: Vec<u64> = r.chain.iter().map(|i| i.0).collect();
            Ok(serde_json::json!({ "chain": chain, "ducks": ducks }))
        })
    }

    #[tool(
        description = "Set one parameter of an insert effect by name (for example bell_db, threshold_db). The value must lie within the range listed at insert_effect; otherwise nothing changes."
    )]
    async fn set_param(
        &self,
        Parameters(p): Parameters<SetParamParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            let (track, kind) = find_insert(s, InsertId(p.insert))?;
            let param = kind
                .param_index(&p.param)
                .ok_or_else(|| ModelError::UnknownParamName(p.param.clone()))?;
            s.apply(Command::SetInsertParam {
                track,
                insert: InsertId(p.insert),
                param,
                value: p.value,
            })?;
            Ok(serde_json::json!({ "ok": true }))
        })
    }

    #[tool(description = "Bypass or re-enable an insert effect (smoothly, without a click).")]
    async fn set_insert_bypass(
        &self,
        Parameters(p): Parameters<SetInsertBypassParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            if let Some(track) = plugin_track(s, InsertId(p.insert)) {
                s.apply(Command::SetPluginBypass {
                    track,
                    plugin: InsertId(p.insert),
                    bypass: p.bypass,
                })?;
                return Ok(serde_json::json!({ "ok": true }));
            }
            let (track, _) = find_insert(s, InsertId(p.insert))?;
            s.apply(Command::SetInsertBypass {
                track,
                insert: InsertId(p.insert),
                bypass: p.bypass,
            })?;
            Ok(serde_json::json!({ "ok": true }))
        })
    }

    #[tool(description = "Remove an insert effect from its chain.")]
    async fn remove_insert(
        &self,
        Parameters(p): Parameters<RemoveInsertParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            if let Some(track) = plugin_track(s, InsertId(p.insert)) {
                s.apply(Command::RemovePlugin {
                    track,
                    plugin: InsertId(p.insert),
                })?;
                return Ok(serde_json::json!({ "ok": true }));
            }
            let (track, _) = find_insert(s, InsertId(p.insert))?;
            s.apply(Command::RemoveInsert {
                track,
                insert: InsertId(p.insert),
            })?;
            Ok(serde_json::json!({ "ok": true }))
        })
    }

    #[tool(
        description = "Route a track or bus to a bus (output = bus id) or back to the master (output omitted). A route that would form a loop is rejected."
    )]
    async fn set_output(
        &self,
        Parameters(p): Parameters<SetOutputParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            s.apply(Command::SetOutput {
                track: TrackId(p.track),
                output: p.output.map(TrackId),
            })?;
            Ok(serde_json::json!({ "ok": true }))
        })
    }

    #[tool(
        description = "Set the send of a track to a bus (created if missing, otherwise changed). level_db -96..12; pre_fader taps before the fader. A send that would form a loop is rejected."
    )]
    async fn set_send(
        &self,
        Parameters(p): Parameters<SetSendParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            let track = TrackId(p.track);
            let target = TrackId(p.target);
            let exists = s
                .project()
                .track(track)
                .is_some_and(|t| t.sends.iter().any(|x| x.target == target));
            if exists {
                s.apply(Command::SetSend {
                    track,
                    target,
                    level_db: p.level_db,
                    pre_fader: p.pre_fader,
                })?;
            } else {
                s.apply(Command::AddSend {
                    track,
                    send: SendRoute {
                        target,
                        level_db: p.level_db,
                        pre_fader: p.pre_fader,
                    },
                    index: None,
                })?;
            }
            Ok(serde_json::json!({ "ok": true }))
        })
    }

    #[tool(description = "Remove the send of a track to a bus.")]
    async fn remove_send(
        &self,
        Parameters(p): Parameters<RemoveSendParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            s.apply(Command::RemoveSend {
                track: TrackId(p.track),
                target: TrackId(p.target),
            })?;
            Ok(serde_json::json!({ "ok": true }))
        })
    }

    #[tool(
        description = "Set the automation curve of one parameter: track volume (gain, dB), pan, the level of a send (send_target = bus id) or a parameter of an effect (insert + param name). Replaces the whole curve of that target; points are beats with values inside the parameter's range; between points the value glides (volume and sends in dB). mode is read (default), write or latch (recording by fader or controller). One undo step. An empty points list with mode read removes the curve."
    )]
    async fn add_automation(
        &self,
        Parameters(p): Parameters<AddAutomationParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            let (track, target) = resolve_auto_target(s, &p.target)?;
            let n = p.points.len();
            s.apply(Command::SetAutomation {
                track,
                target,
                points: p
                    .points
                    .iter()
                    .map(|q| AutoPoint {
                        beat: q.beat,
                        value: q.value,
                    })
                    .collect(),
                mode: p.mode.into(),
            })?;
            Ok(serde_json::json!({ "ok": true, "points": n }))
        })
    }

    #[tool(
        description = "Remove the automation curve of one parameter (same selection as add_automation)."
    )]
    async fn remove_automation(
        &self,
        Parameters(p): Parameters<AutomationTargetParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            let (track, target) = resolve_auto_target(s, &p)?;
            s.apply(Command::SetAutomation {
                track,
                target,
                points: Vec::new(),
                mode: AutoMode::Read,
            })?;
            Ok(serde_json::json!({ "ok": true }))
        })
    }

    #[tool(
        description = "Sidechain: let a compressor or gate on an audio or MIDI track measure its level from another track (source = track id, signal after the fader and before the pan), for example to duck music under a voice. source omitted returns the effect to its own signal. Loops and effects without a sidechain input (and inserts on buses) are rejected."
    )]
    async fn set_sidechain(
        &self,
        Parameters(p): Parameters<SetSidechainParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_step(|s| {
            let (track, _) = find_insert(s, InsertId(p.insert))?;
            s.apply(Command::SetInsertSidechain {
                track,
                insert: InsertId(p.insert),
                source: p.source.map(TrackId),
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
        description = "Render the project offline (48 kHz, up to the end of the last clip) and measure it: integrated loudness (LUFS, ITU-R BS.1770), highest momentary and short-term loudness, sample peak and true peak (dBTP, 4x oversampled), clipping, length in seconds, the mean spectrum (peak frequency and octave-band levels in dBFS), the stereo image (correlation +1 mono .. -1 out of phase, balance, side-to-mid ratio), short notes on problems, and the peak of every audio track on its own. With `track`, measures that track alone (mixer settings kept) instead of the mix. Changes nothing."
    )]
    async fn analyze(
        &self,
        Parameters(p): Parameters<AnalyzeParams>,
    ) -> Result<CallToolResult, McpError> {
        let me = self.clone();
        off_runtime(move || me.analyze_blocking(p)).await
    }

    fn analyze_blocking(&self, p: AnalyzeParams) -> Result<CallToolResult, McpError> {
        let project = self.lock()?.project().clone();
        let mix = match self.render_for(&project, p.track) {
            Ok(m) => m,
            Err(e) => return Ok(fail(e)),
        };
        let mut result = audio::measure(&mix);
        if let (Some(extra), Some(map)) =
            (audio::analysis(&mix).as_object(), result.as_object_mut())
        {
            for (k, v) in extra {
                map.insert(k.clone(), v.clone());
            }
        }
        if p.track.is_some() {
            result["track"] = serde_json::json!(p.track);
            return Ok(ok_json(result));
        }
        let mut tracks = Vec::new();
        for t in project.tracks.iter().filter(|t| t.kind != TrackKind::Bus) {
            let peak = audio::render_track(&project, &self.dir, t.id)
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
        description = "Render the project offline and write its spectrogram as a PNG (time left to right, frequency logarithmic from 20 Hz at the bottom to the top, brighter = louder, -90..0 dBFS). `path` is a relative .png path inside the project folder. Optional `track` draws one track alone. Look at the picture to see tones, noise, cut-offs and gaps. Changes nothing in the project."
    )]
    async fn spectrogram_png(
        &self,
        Parameters(p): Parameters<SpectrogramParams>,
    ) -> Result<CallToolResult, McpError> {
        let me = self.clone();
        off_runtime(move || me.spectrogram_png_blocking(p)).await
    }

    fn spectrogram_png_blocking(&self, p: SpectrogramParams) -> Result<CallToolResult, McpError> {
        if !p.path.to_lowercase().ends_with(".png") {
            return Ok(fail("Endung muss .png sein"));
        }
        let (w, h) = (p.width.unwrap_or(800), p.height.unwrap_or(400));
        if !(16..=4096).contains(&w) || !(16..=2048).contains(&h) {
            return Ok(fail("width 16..=4096, height 16..=2048"));
        }
        let project = self.lock()?.project().clone();
        let mix = match self.render_for(&project, p.track) {
            Ok(m) => m,
            Err(e) => return Ok(fail(e)),
        };
        let (out, created) = match self.resolve_out(&p.path) {
            Ok(o) => o,
            Err(e) => return Ok(fail(e)),
        };
        let rgb = audio::spectrogram_rgb(&mix, w as usize, h as usize);
        if let Err(e) = audio::write_png(&out, &rgb, w, h) {
            remove_created(&created);
            return Ok(fail(e));
        }
        Ok(ok_json(serde_json::json!({
            "path": shown(&out),
            "width": w,
            "height": h,
            "seconds": audio::round(mix.seconds, 3),
        })))
    }

    #[tool(
        description = "Export the mixdown as WAV (32 bit float), FLAC (24 bit), MP3 (320 kbit/s), or Opus (.opus), Ogg Vorbis (.ogg) or AAC (.aac) through an installed ffmpeg, chosen by the file extension. Without ffmpeg the last three fail with a clear message. Optional target_lufs normalises the loudness (peaks stay below -1 dBFS). Returns loudness and peak of the written audio. If target_reached is false (normalized.peak_limited), the mix has too much peak for that loudness: lower the gain of the drum/percussive tracks by a few dB and export again."
    )]
    async fn export(
        &self,
        Parameters(p): Parameters<ExportParams>,
    ) -> Result<CallToolResult, McpError> {
        let me = self.clone();
        off_runtime(move || me.export_blocking(p)).await
    }

    fn export_blocking(&self, p: ExportParams) -> Result<CallToolResult, McpError> {
        let project = self.lock()?.project().clone();
        if woa_engine::export::ExportFormat::from_path(Path::new(&p.path)).is_none() {
            return Ok(fail(woa_engine::export::EXTENSION_HINT));
        }
        let (out, created) = match self.resolve_out(&p.path) {
            Ok(o) => o,
            Err(e) => return Ok(fail(e)),
        };
        let (mix, normalized) = match self.export_to(&project, &out, p.target_lufs) {
            Ok(v) => v,
            Err(e) => {
                remove_created(&created);
                return Ok(fail(e));
            }
        };
        let mut result = audio::measure(&mix);
        result["path"] = serde_json::Value::String(shown(&out));
        if let Some(t) = p.target_lufs {
            let reached = result["lufs"]
                .as_f64()
                .is_some_and(|l| (l - t).abs() <= 0.5);
            result["target_reached"] = serde_json::Value::Bool(reached);
        }
        result["normalized"] = normalized;
        Ok(ok_json(result))
    }

    #[tool(
        description = "Finish the mixdown as a radio jingle: gain to the target loudness, limiter at -1 dBFS (so the target is reached even with peaky drums), WAV into the WOA jingle folder (the one set in the app), entered in the library as kind Jingle, and optionally put on a cart (1-16) in radio.json. The app takes the cart over within seconds. Same path as the app's export dialog. Returns path, loudness, peak and cart."
    )]
    async fn export_jingle(
        &self,
        Parameters(p): Parameters<ExportJingleParams>,
    ) -> Result<CallToolResult, McpError> {
        let me = self.clone();
        off_runtime(move || {
            let project = me.lock()?.project().clone();
            let mut to = jingle::Target::standard(p.name);
            to.target_lufs = p.target_lufs;
            to.cart = p.cart;
            let result = jingle::check_name(&to.name)
                .and_then(|_| audio::render(&project, &me.dir))
                .and_then(|mut mix| {
                    jingle::publish(&mut mix.left, &mut mix.right, audio::RATE, &to)
                });
            Ok(match result {
                Ok(j) => ok_json(jingle::to_json(&j)),
                Err(e) => fail(e),
            })
        })
        .await
    }

    #[tool(description = "Undo the last step (the agent's or the human's).")]
    async fn undo(&self) -> Result<CallToolResult, McpError> {
        let mut s = self.lock()?;
        let step = s.history().undo_label().map(str::to_string);
        Ok(match s.undo() {
            Ok(()) => ok_json(steps_json("undone", step, &s)),
            Err(e) => fail(e),
        })
    }

    #[tool(description = "Redo the last undone step.")]
    async fn redo(&self) -> Result<CallToolResult, McpError> {
        let mut s = self.lock()?;
        let step = s.history().redo_label().map(str::to_string);
        Ok(match s.redo() {
            Ok(()) => ok_json(steps_json("redone", step, &s)),
            Err(e) => fail(e),
        })
    }

    #[tool(description = "Save the project to its folder (atomic write).")]
    async fn save(&self) -> Result<CallToolResult, McpError> {
        let mut s = self.lock()?;
        Ok(match s.save_as(&self.dir) {
            Ok(()) => ok_json(serde_json::json!({ "saved": shown(&self.dir) })),
            Err(e) => fail(e),
        })
    }

    #[tool(
        description = "Radio mode: build the active broadcast playlist (of several named ones) in radio.json from audio files (WAV). Replaces the playlist, or appends with append=true. Every title is measured (LUFS, peak, length) so auto-gain can bring it to the target loudness. Returns the run order."
    )]
    async fn build_playlist(
        &self,
        Parameters(p): Parameters<BuildPlaylistParams>,
    ) -> Result<CallToolResult, McpError> {
        let me = self.clone();
        off_runtime(move || me.build_playlist_blocking(p)).await
    }

    fn build_playlist_blocking(&self, p: BuildPlaylistParams) -> Result<CallToolResult, McpError> {
        // Write target: the default file, or a relative path under the same rules as export.
        let (radio, created) = match p.radio_json.as_deref() {
            None => (woa_radio::default_radio_path(), Vec::new()),
            Some(g) => match self.resolve_out(g) {
                Ok(x) => x,
                Err(e) => return Ok(fail(e)),
            },
        };
        let r = self.build_playlist_at(&radio, &p.files, p.append);
        if r.is_err() {
            remove_created(&created);
        }
        match r {
            Ok(show) => Ok(ok_json(run_order_json(&show, &radio))),
            Err(e) => Ok(fail(e)),
        }
    }

    #[tool(
        description = "Radio mode: set the cue points of one playlist title in radio.json (Cue-In, Cue-Out, Ramp, Hook, Fade-Out, seconds from file start). Replaces the title's whole set; omitted points are cleared, so call with no points to clear all. Checked: 0 <= Cue-In < Cue-Out <= length, the others between. Returns the previous points (call again with them to undo) and the new run order; a running WOA picks the file up within 2 s."
    )]
    async fn set_cue_points(
        &self,
        Parameters(p): Parameters<SetCuePointsParams>,
    ) -> Result<CallToolResult, McpError> {
        let (radio, created) = match p.radio_json.as_deref() {
            None => (woa_radio::default_radio_path(), Vec::new()),
            Some(g) => match self.resolve_out(g) {
                Ok(x) => x,
                Err(e) => return Ok(fail(e)),
            },
        };
        let cues = woa_radio::Cues {
            cue_in: p.cue_in,
            cue_out: p.cue_out,
            ramp: p.ramp,
            hook: p.hook,
            fade_out: p.fade_out,
        };
        match set_cues_in(&radio, p.position, cues) {
            Ok((show, before)) => {
                let mut v = run_order_json(&show, &radio);
                if let Some(o) = v.as_object_mut() {
                    o.insert("previous".into(), serde_json::json!(before));
                }
                Ok(ok_json(v))
            }
            Err(e) => {
                remove_created(&created);
                Ok(fail(e))
            }
        }
    }

    #[tool(
        description = "Radio mode: switch the cart wall to set 1-5 in radio.json (each set has its own name and 16 carts; a running cart keeps playing). Returns the set names and the carts of the active set; a running WOA picks the file up within 2 s."
    )]
    async fn select_cart_set(
        &self,
        Parameters(p): Parameters<SelectCartSetParams>,
    ) -> Result<CallToolResult, McpError> {
        let (radio, created) = match p.radio_json.as_deref() {
            None => (woa_radio::default_radio_path(), Vec::new()),
            Some(g) => match self.resolve_out(g) {
                Ok(x) => x,
                Err(e) => return Ok(fail(e)),
            },
        };
        let r = p
            .set
            .checked_sub(1)
            .ok_or_else(|| "set beginnt bei 1".to_string())
            .and_then(|i| edit_show(&radio, |s| s.switch_cart_set(i)));
        match r {
            Ok(show) => Ok(ok_json(cart_wall_json(&show))),
            Err(e) => {
                remove_created(&created);
                Ok(fail(e))
            }
        }
    }

    #[tool(
        description = "Radio mode: rename a cart-wall set (1-5), a cart of the active set (1-16) or a title of the active playlist (position from 1) in radio.json. Only the shown name changes, never the file; an empty name restores the file name. Same function as right-click > Umbenennen in the app. Project tracks: use rename_track."
    )]
    async fn rename(
        &self,
        Parameters(p): Parameters<RenameParams>,
    ) -> Result<CallToolResult, McpError> {
        let (radio, created) = match p.radio_json.as_deref() {
            None => (woa_radio::default_radio_path(), Vec::new()),
            Some(g) => match self.resolve_out(g) {
                Ok(x) => x,
                Err(e) => return Ok(fail(e)),
            },
        };
        match edit_show(&radio, |s| rename_in(s, &p)) {
            Ok(show) => {
                let mut v = cart_wall_json(&show);
                if let Some(o) = v.as_object_mut() {
                    o.insert("run_order".into(), run_order_json(&show, &radio));
                }
                Ok(ok_json(v))
            }
            Err(e) => {
                remove_created(&created);
                Ok(fail(e))
            }
        }
    }

    #[tool(
        description = "Broadcast: status of the stream process woa-streamd (running, connection, source live/emergency, listeners, bitrate, on-air seconds, last error). Read-only."
    )]
    async fn stream_status(
        &self,
        Parameters(p): Parameters<StreamStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        let dir = p
            .data_dir
            .map(PathBuf::from)
            .unwrap_or_else(woa_radio::data_dir);
        Ok(ok_json(stream_status_json(&dir)))
    }

    #[tool(
        description = "Radio mode: read the active playlist from radio.json (plus the names of all playlists) and return the run order with each title's length, loudness (LUFS) and auto-gain, plus the total length with crossfades. Changes nothing."
    )]
    async fn run_order(
        &self,
        Parameters(p): Parameters<RunOrderParams>,
    ) -> Result<CallToolResult, McpError> {
        let radio = self.radio_path(p.radio_json.as_deref());
        match woa_radio::RadioShow::load(&radio) {
            Ok(show) => Ok(ok_json(run_order_json(&show, &radio))),
            Err(e) => Ok(fail(e)),
        }
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
                 make sound (generate_notes with midi: true on a MIDI track keeps editable \
                 notes, read_notes reads them back), repeat_clip builds loops, analyze \
                 measures, export writes \
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

#[cfg(test)]
mod stream_tests {
    use super::*;

    #[test]
    fn stream_status_reads_file_or_reports_off() {
        let tmp = tempfile::tempdir().unwrap();
        let v = stream_status_json(tmp.path());
        assert_eq!(v["running"], false);
        let st = woa_stream::StreamStatus {
            updated_unix: woa_stream::unix_now(),
            connection: "verbunden".into(),
            listeners: Some(4),
            ..Default::default()
        };
        woa_stream::status::write(&woa_stream::status::status_path(tmp.path()), &st).unwrap();
        let v = stream_status_json(tmp.path());
        assert_eq!(v["running"], true);
        assert_eq!(v["listeners"], 4);
    }
}

#[cfg(test)]
mod path_tests {
    use super::strip_verbatim;

    #[test]
    fn verbatim_prefix_is_stripped() {
        assert_eq!(strip_verbatim(r"\\?\C:\Musik\mix.wav"), r"C:\Musik\mix.wav");
        assert_eq!(
            strip_verbatim(r"\\?\UNC\nas\musik\a.wav"),
            r"\\nas\musik\a.wav"
        );
        assert_eq!(
            strip_verbatim("/home/projekt/mix.wav"),
            "/home/projekt/mix.wav"
        );
        assert_eq!(strip_verbatim(r"C:\x.wav"), r"C:\x.wav");
    }
}

#[cfg(test)]
mod cart_set_tests {
    use super::*;

    #[test]
    fn select_set_and_rename_through_radio_json() {
        let tmp = tempfile::tempdir().unwrap();
        let radio = tmp.path().join("radio.json");
        let mut show = woa_radio::RadioShow::default();
        show.carts[1] = Some(woa_radio::CartEntry::new("jingles/id.wav"));
        show.playlist
            .push(woa_radio::PlaylistEntry::new("musik/song_v3.mp3"));
        show.save(&radio).unwrap();

        let p = |what, number, name: &str| RenameParams {
            what,
            number,
            name: name.into(),
            radio_json: None,
        };
        edit_show(&radio, |s| rename_in(s, &p(RenameWhat::Cart, 2, "Station"))).unwrap();
        edit_show(&radio, |s| {
            rename_in(s, &p(RenameWhat::CartSet, 1, "Morgen"))
        })
        .unwrap();
        let s = edit_show(&radio, |s| rename_in(s, &p(RenameWhat::Title, 1, "Song"))).unwrap();
        assert_eq!(s.playlist[0].title(), "Song");
        let v = cart_wall_json(&s);
        assert_eq!(v["cart_sets"][0], "Morgen");
        assert_eq!(v["carts"][1]["name"], "Station");
        assert!(edit_show(&radio, |s| rename_in(s, &p(RenameWhat::Cart, 0, "x"))).is_err());

        let s = edit_show(&radio, |s| s.switch_cart_set(2)).unwrap();
        assert_eq!(cart_wall_json(&s)["cart_set"], 3);
        assert!(s.carts.iter().all(Option::is_none));
        let back = woa_radio::RadioShow::load(&radio).unwrap();
        assert_eq!(back.cart_set, 2);
        let s = edit_show(&radio, |s| s.switch_cart_set(0)).unwrap();
        assert_eq!(s.carts[1].as_ref().unwrap().label, "Station");
    }
}
