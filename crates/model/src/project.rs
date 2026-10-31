//! Plain data types of a project. They carry no behaviour beyond lookups;
//! changes go through [`crate::Command`].

use std::collections::{HashMap, HashSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::effect::{Insert, InsertId};
use crate::error::{ModelError, Result};

/// Tempo of a new project.
pub const DEFAULT_TEMPO_BPM: f64 = 120.0;
/// Lowest accepted tempo.
pub const MIN_TEMPO_BPM: f64 = 20.0;
/// Highest accepted tempo.
pub const MAX_TEMPO_BPM: f64 = 400.0;
/// Lowest accepted track gain (dB).
pub const MIN_GAIN_DB: f64 = -96.0;
/// Highest accepted track gain (dB).
pub const MAX_GAIN_DB: f64 = 24.0;
/// Highest accepted send level (dB). The lowest is [`MIN_GAIN_DB`] (= off).
pub const MAX_SEND_DB: f64 = 12.0;

/// Stable id of a track. Ids are never reused within a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TrackId(pub u64);

/// Stable id of a clip. Shares the id space with [`TrackId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClipId(pub u64);

impl fmt::Display for TrackId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "T{}", self.0)
    }
}

impl fmt::Display for ClipId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "C{}", self.0)
    }
}

/// What a track carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    /// Recorded or imported audio.
    Audio,
    /// MIDI notes driving an instrument.
    Midi,
    /// Bus/group track without clips of its own.
    Bus,
}

/// Descriptive project data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectMeta {
    /// Display name, also the default folder name.
    pub name: String,
}

/// A clip on a track: position and length in beats, optionally backed by an audio file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    /// Stable id.
    pub id: ClipId,
    /// Display name.
    pub name: String,
    /// Start position in beats (>= 0).
    pub start_beats: f64,
    /// Length in beats (> 0).
    pub length_beats: f64,
    /// Audio file of the clip, relative to the project folder (e.g. `aufnahmen/x.wav`).
    /// `None`: placeholder clip without sound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
    /// Where in the audio file the clip begins, in beats (after a split).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub offset_beats: f64,
    /// Fade-in length in beats.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fade_in_beats: f64,
    /// Fade-out length in beats.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fade_out_beats: f64,
    /// Notes of a MIDI clip (Paket 12). `None`: audio or placeholder clip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub midi: Option<crate::midi::MidiData>,
    /// Shape of the fade-in (Paket 21).
    #[serde(default, skip_serializing_if = "FadeCurve::is_linear")]
    pub fade_in_curve: FadeCurve,
    /// Shape of the fade-out (Paket 21).
    #[serde(default, skip_serializing_if = "FadeCurve::is_linear")]
    pub fade_out_curve: FadeCurve,
    /// Clip gain in dB, applied before the track fader (Paket 21).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub gain_db: f64,
    /// Time-stretch factor (Paket 23): `length_beats` shows `length / stretch` beats of the
    /// audio, so 1.1 plays the same audio 10 % longer at the same pitch. 1.0 = unchanged.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub stretch: f64,
    /// Pitch shift in semitones (Paket 23), independent of the length. 0 = unchanged.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub pitch_semitones: f64,
    /// Follow the project tempo (Paket 23): the tempo (BPM) the audio's beats refer to. When
    /// the project tempo differs, the audio is stretched so that it stays on the beat grid.
    /// `None`: the audio keeps its own speed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_tempo_bpm: Option<f64>,
}

fn one() -> f64 {
    1.0
}

fn is_one(v: &f64) -> bool {
    *v == 1.0
}

/// Range of the clip stretch factor.
pub const STRETCH_RANGE: (f64, f64) = (0.25, 4.0);
/// Range of the clip pitch shift in semitones.
pub const PITCH_RANGE: (f64, f64) = (-24.0, 24.0);
/// Range of a followed tempo in BPM.
pub const FOLLOW_TEMPO_RANGE: (f64, f64) = (20.0, 400.0);

/// Shape of a fade. `gain(t)` runs from 0 (start of the fade) to 1 (end); a fade-out
/// uses `gain(1 - t)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FadeCurve {
    /// Straight line.
    #[default]
    Linear,
    /// Equal power: `sin(t * pi / 2)`. Two clips fading against each other keep their
    /// summed power constant (for uncorrelated material).
    EqualPower,
    /// S-curve (raised cosine): soft start and end, the two sides sum to exactly 1.
    SCurve,
}

impl FadeCurve {
    /// All curves in menu order.
    pub const ALL: [FadeCurve; 3] = [FadeCurve::Linear, FadeCurve::EqualPower, FadeCurve::SCurve];

    /// Gain at position `t` (0..=1) of the fade. Values outside are clamped.
    /// Real-time safe: pure arithmetic, no allocation.
    pub fn gain(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            FadeCurve::Linear => t,
            FadeCurve::EqualPower => (t * std::f32::consts::FRAC_PI_2).sin(),
            FadeCurve::SCurve => 0.5 - 0.5 * (t * std::f32::consts::PI).cos(),
        }
    }

    /// True for the default curve (not written to the file).
    pub fn is_linear(&self) -> bool {
        *self == FadeCurve::Linear
    }

    /// Display name (German).
    pub fn label(self) -> &'static str {
        match self {
            FadeCurve::Linear => "Linear",
            FadeCurve::EqualPower => "Gleiche Leistung",
            FadeCurve::SCurve => "S-Kurve",
        }
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl Clip {
    /// A clip without audio offset and fades.
    pub fn new(
        id: ClipId,
        name: impl Into<String>,
        start_beats: f64,
        length_beats: f64,
        audio: Option<String>,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            start_beats,
            length_beats,
            audio,
            offset_beats: 0.0,
            fade_in_beats: 0.0,
            fade_out_beats: 0.0,
            midi: None,
            fade_in_curve: FadeCurve::Linear,
            fade_out_curve: FadeCurve::Linear,
            gain_db: 0.0,
            stretch: 1.0,
            pitch_semitones: 0.0,
            follow_tempo_bpm: None,
        }
    }

    /// Tempo (BPM) that maps the clip's source beats to seconds of the audio file.
    pub fn source_bpm(&self, project_tempo: f64) -> f64 {
        self.follow_tempo_bpm.unwrap_or(project_tempo)
    }

    /// Playing time of the audio divided by its original time: `stretch` times the tempo
    /// change of a followed tempo. 1.0 = the audio plays at its own speed and pitch-shifting
    /// aside needs no processing.
    pub fn time_ratio(&self, project_tempo: f64) -> f64 {
        self.stretch * self.source_bpm(project_tempo) / project_tempo
    }

    /// Whether the audio needs processing (stretch, tempo change or pitch).
    pub fn is_processed(&self, project_tempo: f64) -> bool {
        (self.time_ratio(project_tempo) - 1.0).abs() > 1e-9 || self.pitch_semitones != 0.0
    }

    /// Length of the audio shown by the clip, in source beats.
    pub fn source_length_beats(&self) -> f64 {
        self.length_beats / self.stretch
    }

    /// A MIDI clip holding `midi` (no audio).
    pub fn new_midi(
        id: ClipId,
        name: impl Into<String>,
        start_beats: f64,
        length_beats: f64,
        midi: crate::midi::MidiData,
    ) -> Self {
        let mut c = Self::new(id, name, start_beats, length_beats, None);
        c.midi = Some(midi);
        c
    }

    /// End on the timeline in beats.
    pub fn end_beats(&self) -> f64 {
        self.start_beats + self.length_beats
    }
}

/// Whether a clip audio path is a plain relative path inside the project folder:
/// only normal components, no `..`, no root, no drive prefix.
pub fn is_safe_audio_path(rel: &str) -> bool {
    let p = std::path::Path::new(rel);
    !rel.is_empty()
        && p.components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
}

#[cfg(test)]
mod path_tests {
    #[test]
    fn audio_paths_stay_inside() {
        assert!(super::is_safe_audio_path("aufnahmen/a.wav"));
        for bad in [
            "",
            "../a.wav",
            "aufnahmen/../../a.wav",
            "/etc/passwd",
            "./a.wav",
        ] {
            assert!(!super::is_safe_audio_path(bad), "{bad}");
        }
    }
}

/// A track with its mixer settings and clips.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    /// Stable id.
    pub id: TrackId,
    /// Display name, never empty.
    pub name: String,
    /// What the track carries.
    pub kind: TrackKind,
    /// Gain in dB, within [`MIN_GAIN_DB`]..=[`MAX_GAIN_DB`].
    pub gain_db: f64,
    /// Pan from -1.0 (left) to 1.0 (right).
    pub pan: f64,
    /// Muted.
    pub mute: bool,
    /// Soloed.
    pub solo: bool,
    /// Clips in insertion order.
    pub clips: Vec<Clip>,
    /// Destination bus of the track's signal; `None`: the master.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<TrackId>,
    /// Sends to buses (Paket 16).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sends: Vec<Send>,
    /// Effect chain, first insert first (Paket 16).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inserts: Vec<Insert>,
    /// Automation curves, at most one per target (Paket 18).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub automation: Vec<crate::automation::AutomationLane>,
    /// Take folders made by loop recording (Paket 22).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub take_folders: Vec<crate::takes::TakeFolder>,
    /// Fremd-Plugins (Paket 31): höchstens ein Instrument, dazu Effekte in Kettenreihenfolge.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<crate::plugin::PluginSlot>,
}

/// A send from a track to a bus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Send {
    /// Destination bus.
    pub target: TrackId,
    /// Level in dB, [`MIN_GAIN_DB`]..=[`MAX_SEND_DB`]; [`MIN_GAIN_DB`] means off.
    pub level_db: f64,
    /// Tapped before the fader (mute and solo still apply), otherwise after it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub pre_fader: bool,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(v: &bool) -> bool {
    !*v
}

impl Send {
    /// Linear factor of the send level; the lowest value is silence.
    pub fn linear(&self) -> f64 {
        if self.level_db <= MIN_GAIN_DB {
            0.0
        } else {
            10f64.powf(self.level_db / 20.0)
        }
    }
}

impl Track {
    /// A new track with neutral mixer settings and no clips.
    pub fn new(id: TrackId, name: impl Into<String>, kind: TrackKind) -> Self {
        Self {
            id,
            name: name.into(),
            kind,
            gain_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            clips: Vec::new(),
            output: None,
            sends: Vec::new(),
            inserts: Vec::new(),
            automation: Vec::new(),
            take_folders: Vec::new(),
            plugins: Vec::new(),
        }
    }

    /// The plugin entry with this id on this track.
    pub fn plugin(&self, id: InsertId) -> Option<&crate::plugin::PluginSlot> {
        self.plugins.iter().find(|p| p.id == id)
    }
}

/// The whole project as it is saved to `project.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    /// Descriptive data.
    pub meta: ProjectMeta,
    /// Tempo in beats per minute.
    pub tempo_bpm: f64,
    /// Tracks in display order.
    pub tracks: Vec<Track>,
    /// Next free id for tracks and clips. Only grows, on purpose: undo and redo never
    /// lower it, so an id is never handed out twice. A client (agent, GUI selection) that
    /// still holds the id of an undone track or clip gets "not found" instead of silently
    /// hitting a new object. Gaps in the ids are harmless; the counter is capped by
    /// [`MAX_ID`] (#243).
    pub next_id: u64,
}

impl Project {
    /// An empty project at [`DEFAULT_TEMPO_BPM`].
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            meta: ProjectMeta { name: name.into() },
            tempo_bpm: DEFAULT_TEMPO_BPM,
            tracks: Vec::new(),
            next_id: 1,
        }
    }

    /// Looks up a track.
    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    /// Position of a track in [`Project::tracks`].
    pub fn track_index(&self, id: TrackId) -> Option<usize> {
        self.tracks.iter().position(|t| t.id == id)
    }

    /// Looks up a clip and the track it lives on.
    pub fn clip(&self, id: ClipId) -> Option<(&Track, &Clip)> {
        self.tracks
            .iter()
            .find_map(|t| t.clips.iter().find(|c| c.id == id).map(|c| (t, c)))
    }

    /// True if `raw` is used by any track or clip.
    pub fn id_in_use(&self, raw: u64) -> bool {
        self.tracks.iter().any(|t| {
            t.id.0 == raw
                || t.clips.iter().any(|c| c.id.0 == raw)
                || t.inserts.iter().any(|i| i.id.0 == raw)
                || t.plugins.iter().any(|p| p.id.0 == raw)
                || t.take_folders.iter().any(|f| f.id == raw)
        })
    }

    pub(crate) fn track_mut(&mut self, id: TrackId) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    /// Hands out a fresh id and advances the counter.
    ///
    /// Loading and every command keep all ids and `next_id` at or below
    /// [`MAX_ID`] + 1, so the loop ends and the counter cannot overflow.
    pub(crate) fn alloc_id(&mut self) -> u64 {
        // Skip anything already used, e.g. ids from hand-edited files.
        while self.id_in_use(self.next_id) {
            self.next_id = self.next_id.saturating_add(1);
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Rejects a loaded project whose ids or counter exceed [`MAX_ID`].
    pub(crate) fn check_ids(&self) -> Result<()> {
        if self.next_id > MAX_ID + 1 {
            return Err(ModelError::IdOutOfRange(self.next_id));
        }
        for t in &self.tracks {
            check_id(t.id.0)?;
            for c in &t.clips {
                check_id(c.id.0)?;
                if let Some(m) = &c.midi {
                    m.check()?;
                }
            }
            for i in &t.inserts {
                check_id(i.id.0)?;
            }
            for p in &t.plugins {
                check_id(p.id.0)?;
            }
            for (n, f) in t.take_folders.iter().enumerate() {
                check_id(f.id)?;
                f.check()?;
                if t.take_folders[..n].iter().any(|o| o.id == f.id)
                    || self.tracks.iter().any(|o| o.id.0 == f.id)
                {
                    return Err(ModelError::DuplicateId(f.id));
                }
            }
        }
        Ok(())
    }

    /// Brings a file of an older format version up to date (Paket 16: missing effect
    /// parameters get their defaults).
    pub(crate) fn migrate(&mut self) {
        for t in &mut self.tracks {
            for i in &mut t.inserts {
                i.pad();
            }
        }
    }

    /// Checks outputs, sends, inserts, sidechains and automation of all tracks: destinations
    /// exist and are buses, no track feeds itself, no cycles, ids unique, values in range.
    /// Automation of a target that no longer exists (deleted effect or send) is kept; it
    /// comes back to life with Undo and the engine ignores it meanwhile.
    pub(crate) fn check_routing(&self) -> Result<()> {
        // Sets instead of linear scans: a hostile project file with many tracks, sends and
        // inserts must not make loading quadratic (#79).
        let track_ids: HashSet<u64> = self.tracks.iter().map(|t| t.id.0).collect();
        let mut seen: HashSet<u64> = HashSet::new();
        for t in &self.tracks {
            for r in t.output.iter().chain(t.sends.iter().map(|s| &s.target)) {
                self.check_destination(t.id, *r)?;
            }
            let mut targets: HashSet<TrackId> = HashSet::new();
            for s in &t.sends {
                check_send_level(s.level_db)?;
                if !targets.insert(s.target) {
                    return Err(ModelError::DuplicateSend {
                        track: t.id,
                        target: s.target,
                    });
                }
            }
            for i in &t.inserts {
                i.check()?;
                if let Some(src) = i.sidechain {
                    self.check_sidechain(t, i, src)?;
                }
                if track_ids.contains(&i.id.0) || !seen.insert(i.id.0) {
                    return Err(ModelError::DuplicateId(i.id.0));
                }
            }
        }
        for t in &self.tracks {
            for p in &t.plugins {
                p.check()?;
                if track_ids.contains(&p.id.0) || !seen.insert(p.id.0) {
                    return Err(ModelError::DuplicateId(p.id.0));
                }
            }
            if t.plugins
                .iter()
                .filter(|p| p.role == crate::plugin::PluginRole::Instrument)
                .count()
                > 1
            {
                return Err(ModelError::InvalidArgument(format!(
                    "track {} has more than one plugin instrument",
                    t.id
                )));
            }
        }
        for t in &self.tracks {
            Self::check_automation(t)?;
        }
        self.check_acyclic()
    }

    /// Every edge of the signal graph: outputs, sends, and sidechain source → listening track.
    fn edges(&self) -> HashMap<TrackId, Vec<TrackId>> {
        let mut out: HashMap<TrackId, Vec<TrackId>> = HashMap::with_capacity(self.tracks.len());
        for t in &self.tracks {
            let e = out.entry(t.id).or_default();
            e.extend(t.output);
            e.extend(t.sends.iter().map(|s| s.target));
            for i in &t.inserts {
                if let Some(src) = i.sidechain {
                    out.entry(src).or_default().push(t.id);
                }
            }
        }
        out
    }

    /// One pass over the whole graph (Kahn, O(V+E)) instead of one search per edge (#79).
    /// On a cycle the error names an edge between two tracks that are both on it.
    fn check_acyclic(&self) -> Result<()> {
        let edges = self.edges();
        let mut indegree: HashMap<TrackId, usize> = edges.keys().map(|k| (*k, 0)).collect();
        for to in edges.values().flatten() {
            *indegree.entry(*to).or_default() += 1;
        }
        let mut ready: Vec<TrackId> = indegree
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(k, _)| *k)
            .collect();
        while let Some(id) = ready.pop() {
            for to in edges.get(&id).into_iter().flatten() {
                if let Some(d) = indegree.get_mut(to) {
                    *d -= 1;
                    if *d == 0 {
                        ready.push(*to);
                    }
                }
            }
            indegree.remove(&id);
        }
        // What is left lies on a cycle or behind one; find an edge inside the rest.
        let mut left: Vec<TrackId> = indegree.keys().copied().collect();
        left.sort();
        for from in left {
            for to in edges.get(&from).into_iter().flatten() {
                if indegree.contains_key(to) {
                    return Err(ModelError::RoutingCycle { from, to: *to });
                }
            }
        }
        Ok(())
    }

    /// The sidechain source exists and the effect can listen to it.
    pub(crate) fn check_sidechain(
        &self,
        target: &Track,
        insert: &Insert,
        source: TrackId,
    ) -> Result<()> {
        // Bus-Inserts haben (noch) keinen Schlüsseleingang.
        if !insert.kind.supports_sidechain() || target.kind == TrackKind::Bus {
            return Err(ModelError::SidechainNotSupported(insert.id));
        }
        self.track(source)
            .map(|_| ())
            .ok_or(ModelError::TrackNotFound(source))
    }

    /// Lanes: one per target, at most [`crate::automation::MAX_AUTOMATION_POINTS`] points,
    /// sorted, finite. Values are checked against the range when the target exists.
    pub(crate) fn check_automation(t: &Track) -> Result<()> {
        for (i, lane) in t.automation.iter().enumerate() {
            if t.automation[..i].iter().any(|o| o.target == lane.target) {
                return Err(ModelError::DuplicateAutomation { track: t.id });
            }
            if lane.points.len() > crate::automation::MAX_AUTOMATION_POINTS {
                return Err(ModelError::TooManyPoints {
                    max: crate::automation::MAX_AUTOMATION_POINTS,
                });
            }
            let range = lane.target.range_in(t).ok();
            let mut prev = f64::NEG_INFINITY;
            for p in &lane.points {
                if !(p.beat.is_finite() && p.beat >= 0.0 && p.beat > prev && p.value.is_finite()) {
                    return Err(ModelError::InvalidValue {
                        field: "automation beat",
                        value: p.beat,
                    });
                }
                prev = p.beat;
                if let Some(r) = &range {
                    r.check(p.value)?;
                }
            }
        }
        Ok(())
    }

    /// The destination exists, is a bus and is not the source itself.
    pub(crate) fn check_destination(&self, from: TrackId, to: TrackId) -> Result<()> {
        let dest = self.track(to).ok_or(ModelError::TrackNotFound(to))?;
        if dest.kind != TrackKind::Bus {
            return Err(ModelError::NotABus(to));
        }
        if from == to {
            return Err(ModelError::RoutingCycle { from, to });
        }
        Ok(())
    }

    /// True if `goal` can be reached from `start` over outputs, sends and sidechains (or is
    /// `start`). O(V+E): the edges are built once per call (#79).
    pub fn reaches(&self, start: TrackId, goal: TrackId) -> bool {
        let edges = self.edges();
        let mut stack = vec![start];
        let mut visited: HashSet<TrackId> = HashSet::new();
        while let Some(id) = stack.pop() {
            if id == goal {
                return true;
            }
            if !visited.insert(id) {
                continue;
            }
            stack.extend(edges.get(&id).into_iter().flatten().copied());
        }
        false
    }

    /// Looks up an insert and its track.
    pub fn insert(&self, id: InsertId) -> Option<(&Track, &Insert)> {
        self.tracks
            .iter()
            .find_map(|t| t.inserts.iter().find(|i| i.id == id).map(|i| (t, i)))
    }

    /// Keeps `next_id` above every id that was inserted from outside.
    pub(crate) fn bump_next_id(&mut self, raw: u64) {
        if raw >= self.next_id {
            self.next_id = raw.saturating_add(1);
        }
    }
}

/// Errors if a send level is not finite or outside `MIN_GAIN_DB..=MAX_SEND_DB`.
pub(crate) fn check_send_level(v: f64) -> Result<()> {
    if v.is_finite() && (MIN_GAIN_DB..=MAX_SEND_DB).contains(&v) {
        Ok(())
    } else {
        Err(ModelError::InvalidValue {
            field: "level_db",
            value: v,
        })
    }
}

/// Largest id the model hands out or accepts (2^53, exact in JSON numbers).
/// The cap keeps the id counter far from overflow, even for hostile files.
pub const MAX_ID: u64 = 1 << 53;

/// Errors with [`ModelError::IdOutOfRange`] if `raw` is above [`MAX_ID`].
pub(crate) fn check_id(raw: u64) -> Result<()> {
    if raw > MAX_ID {
        return Err(ModelError::IdOutOfRange(raw));
    }
    Ok(())
}
