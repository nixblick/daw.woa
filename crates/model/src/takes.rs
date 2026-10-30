//! Take folders and comping (Paket 22).
//!
//! Recording in a loop produces one take per round. The takes of one stretch live in a
//! [`TakeFolder`] on the track. The *comp* says which take plays in which part; it is
//! turned into ordinary clips of the track ([`TakeFolder::comp_clips`]), so the engine, the
//! export and every editor see plain clips and need no knowledge of takes.
//!
//! The planning functions ([`plan_recording`]) are pure arithmetic: they turn "what was
//! recorded, how late did it arrive, where was the loop and the punch range" into clip
//! positions, so the placement can be tested to the sample without any audio device.

use serde::{Deserialize, Serialize};

use crate::error::{ModelError, Result};
use crate::project::{Clip, ClipId, is_safe_audio_path};

/// Most takes in one folder.
pub const MAX_TAKES: usize = 64;
/// Most comp segments in one folder.
pub const MAX_COMP_SEGMENTS: usize = 256;
/// Upper bound for positions and lengths in a take folder, in beats (24 h at 400 BPM fits);
/// keeps a hostile project file from making drawing and rendering run unbounded (#88).
pub const MAX_TAKE_BEATS: f64 = 1_000_000.0;

/// One take: an audio file and where in it the folder start lies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TakeEntry {
    /// Display name ("Take 1").
    pub name: String,
    /// Audio file relative to the project folder.
    pub audio: String,
    /// Position in the audio file that belongs to the start of the folder, in beats.
    pub offset_beats: f64,
}

/// A stretch of the timeline played from one take.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CompSegment {
    /// Start on the timeline in beats.
    pub start_beats: f64,
    /// End on the timeline in beats (exclusive).
    pub end_beats: f64,
    /// Index into [`TakeFolder::takes`].
    pub take: usize,
}

/// The takes of one stretch of a track and the comp made from them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TakeFolder {
    /// Stable id, shares the id space with tracks and clips.
    pub id: u64,
    /// Display name.
    pub name: String,
    /// Start on the timeline in beats.
    pub start_beats: f64,
    /// Length in beats.
    pub length_beats: f64,
    /// The takes, oldest first.
    pub takes: Vec<TakeEntry>,
    /// Which take plays where; sorted, no overlaps. Parts without a segment are silent.
    pub comp: Vec<CompSegment>,
    /// The clips on the track that currently make up the comp.
    pub clip_ids: Vec<ClipId>,
}

fn bad(field: &'static str, value: f64) -> ModelError {
    ModelError::InvalidValue { field, value }
}

impl TakeFolder {
    /// End on the timeline in beats.
    pub fn end_beats(&self) -> f64 {
        self.start_beats + self.length_beats
    }

    /// Checks all values: finite, in range, sorted segments inside the folder, safe paths.
    pub fn check(&self) -> Result<()> {
        if !(self.start_beats.is_finite() && self.start_beats >= 0.0) {
            return Err(bad("take folder start", self.start_beats));
        }
        if !(self.length_beats.is_finite() && self.length_beats > 0.0) {
            return Err(bad("take folder length", self.length_beats));
        }
        if self.start_beats + self.length_beats > MAX_TAKE_BEATS {
            return Err(bad("take folder end", self.start_beats + self.length_beats));
        }
        if self.takes.is_empty() || self.takes.len() > MAX_TAKES {
            return Err(bad("take count", self.takes.len() as f64));
        }
        for t in &self.takes {
            if !is_safe_audio_path(&t.audio) {
                return Err(ModelError::InvalidArgument(format!(
                    "unsafe take path {:?}",
                    t.audio
                )));
            }
            if !(t.offset_beats.is_finite() && (0.0..=MAX_TAKE_BEATS).contains(&t.offset_beats)) {
                return Err(bad("take offset", t.offset_beats));
            }
        }
        if self.comp.len() > MAX_COMP_SEGMENTS {
            return Err(bad("comp segments", self.comp.len() as f64));
        }
        let mut prev_end = self.start_beats;
        let end = self.end_beats();
        for s in &self.comp {
            let ok = s.start_beats.is_finite()
                && s.end_beats.is_finite()
                && s.start_beats < s.end_beats
                && s.start_beats >= prev_end - 1e-9
                && s.end_beats <= end + 1e-9
                && s.take < self.takes.len();
            if !ok {
                return Err(bad("comp segment", s.start_beats));
            }
            prev_end = s.end_beats;
        }
        Ok(())
    }

    /// A comp that plays `take` over the whole folder.
    pub fn whole(&self, take: usize) -> Vec<CompSegment> {
        vec![CompSegment {
            start_beats: self.start_beats,
            end_beats: self.end_beats(),
            take,
        }]
    }

    /// Lets `take` play from `start` to `end` (clamped to the folder), replacing whatever
    /// played there. Segments cut by the new one are shortened or split, neighbours of the
    /// same take are merged. Returns the new comp; the folder itself stays unchanged.
    pub fn comp_with(&self, start: f64, end: f64, take: usize) -> Vec<CompSegment> {
        let start = start.max(self.start_beats);
        let end = end.min(self.end_beats());
        if !(start.is_finite() && end.is_finite()) || start >= end || take >= self.takes.len() {
            return self.comp.clone();
        }
        let mut out: Vec<CompSegment> = Vec::with_capacity(self.comp.len() + 2);
        for s in &self.comp {
            if s.end_beats <= start || s.start_beats >= end {
                out.push(*s);
                continue;
            }
            if s.start_beats < start {
                out.push(CompSegment {
                    end_beats: start,
                    ..*s
                });
            }
            if s.end_beats > end {
                out.push(CompSegment {
                    start_beats: end,
                    ..*s
                });
            }
        }
        out.push(CompSegment {
            start_beats: start,
            end_beats: end,
            take,
        });
        out.sort_by(|a, b| a.start_beats.total_cmp(&b.start_beats));
        let mut merged: Vec<CompSegment> = Vec::with_capacity(out.len());
        for s in out {
            match merged.last_mut() {
                Some(m) if m.take == s.take && (s.start_beats - m.end_beats).abs() < 1e-9 => {
                    m.end_beats = s.end_beats;
                }
                _ => merged.push(s),
            }
        }
        merged
    }

    /// Which take plays at `beat`, if any.
    pub fn take_at(&self, beat: f64) -> Option<usize> {
        self.comp
            .iter()
            .find(|s| beat >= s.start_beats && beat < s.end_beats)
            .map(|s| s.take)
    }

    /// The clips that realise the comp: one per segment, reading the take's file at the
    /// matching place. `next_id` hands out fresh clip ids.
    pub fn comp_clips(&self, mut next_id: impl FnMut() -> ClipId) -> Vec<Clip> {
        self.comp
            .iter()
            .filter_map(|s| {
                let take = self.takes.get(s.take)?;
                Some(Clip {
                    offset_beats: take.offset_beats + (s.start_beats - self.start_beats),
                    ..Clip::new(
                        next_id(),
                        take.name.clone(),
                        s.start_beats,
                        s.end_beats - s.start_beats,
                        Some(take.audio.clone()),
                    )
                })
            })
            .collect()
    }
}

/// Beats for a position of `samples` in a file at `rate`, nudged up by a quarter sample so
/// that the engine's rounding down (`beats_to_samples`) lands exactly on `samples`.
pub fn beats_for_samples(samples: u64, tempo_bpm: f64, rate: f64) -> f64 {
    (samples as f64 + 0.25) / rate * tempo_bpm / 60.0
}

/// What was recorded and under which conditions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordInput {
    /// Where the recording began on the timeline, in beats. With a loop this is the loop start.
    pub start_beats: f64,
    /// Project tempo.
    pub tempo_bpm: f64,
    /// Sample rate of the recorded file (= engine rate).
    pub rate: f64,
    /// Frames in the recorded file.
    pub frames: u64,
    /// Latency to compensate (input plus output plus manual offset), in frames.
    pub latency_frames: u64,
    /// Loop range in beats, if looping was on.
    pub loop_beats: Option<(f64, f64)>,
    /// Punch range in beats: only this part is kept.
    pub punch_beats: Option<(f64, f64)>,
}

/// Placement of one clip made from a recording.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipPlan {
    /// Start on the timeline in beats.
    pub start_beats: f64,
    /// Length in beats.
    pub length_beats: f64,
    /// Offset into the file in beats.
    pub offset_beats: f64,
}

/// Where a recording ends up.
#[derive(Debug, Clone, PartialEq)]
pub enum RecordPlan {
    /// One clip.
    Single(ClipPlan),
    /// One take per loop round; the folder covers `start_beats..start_beats + length_beats`,
    /// `offsets[i]` is the file offset of take `i` at the folder start.
    Folder {
        /// Folder start.
        start_beats: f64,
        /// Folder length.
        length_beats: f64,
        /// File offset per take, in beats.
        offsets: Vec<f64>,
    },
}

/// Turns a recording into clip placement. `None`: nothing to keep (empty file, punch range
/// outside the recording).
pub fn plan_recording(i: &RecordInput) -> Option<RecordPlan> {
    let bps = i.tempo_bpm / 60.0;
    if !(i.rate > 0.0 && bps > 0.0 && i.start_beats.is_finite()) || i.frames <= i.latency_frames {
        return None;
    }
    let usable = i.frames - i.latency_frames;
    let to_samples = |b: f64| (b.max(0.0) / bps * i.rate).floor() as u64;
    let to_beats = |s: u64| beats_for_samples(s, i.tempo_bpm, i.rate);
    if let Some((ls, le)) = i.loop_beats.filter(|(a, b)| a < b && *a >= 0.0) {
        let span = to_samples(le).saturating_sub(to_samples(ls));
        if span == 0 {
            return None;
        }
        let rounds = ((usable / span).max(1) as usize).min(MAX_TAKES);
        let (mut fs, mut fe) = (ls, le);
        if let Some((pi, po)) = i.punch_beats {
            let (a, b) = (pi.max(fs), po.min(fe));
            if a < b {
                (fs, fe) = (a, b);
            }
        }
        let shift = fs - ls;
        let offsets = (0..rounds as u64)
            .map(|k| to_beats(i.latency_frames + k * span) + shift)
            .collect();
        return Some(RecordPlan::Folder {
            start_beats: fs,
            length_beats: fe - fs,
            offsets,
        });
    }
    let len = usable as f64 / i.rate * bps;
    let mut plan = ClipPlan {
        start_beats: i.start_beats.max(0.0),
        length_beats: len,
        offset_beats: to_beats(i.latency_frames),
    };
    if let Some((pi, po)) = i.punch_beats {
        let a = pi.max(plan.start_beats);
        let b = po.min(plan.start_beats + plan.length_beats);
        if !(a.is_finite() && b.is_finite()) || a >= b {
            return None;
        }
        plan.offset_beats += a - plan.start_beats;
        plan.start_beats = a;
        plan.length_beats = b - a;
    }
    Some(RecordPlan::Single(plan))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder() -> TakeFolder {
        TakeFolder {
            id: 9,
            name: "Loop".into(),
            start_beats: 4.0,
            length_beats: 16.0,
            takes: (0..3)
                .map(|n| TakeEntry {
                    name: format!("Take {}", n + 1),
                    audio: format!("aufnahmen/a{n}.wav"),
                    offset_beats: 0.5 + 16.0 * n as f64,
                })
                .collect(),
            comp: Vec::new(),
            clip_ids: Vec::new(),
        }
    }

    #[test]
    fn comp_with_splits_replaces_and_merges() {
        let mut f = folder();
        f.comp = f.whole(2);
        f.comp = f.comp_with(8.0, 12.0, 0);
        assert_eq!(f.comp.len(), 3);
        assert_eq!(f.comp[1].take, 0);
        assert_eq!((f.comp[1].start_beats, f.comp[1].end_beats), (8.0, 12.0));
        // Putting take 2 back over the middle merges everything into one segment.
        f.comp = f.comp_with(8.0, 12.0, 2);
        assert_eq!(f.comp, f.whole(2));
        f.check().unwrap();
        // Outside the folder is clamped, nonsense is ignored.
        let c = f.comp_with(0.0, 6.0, 1);
        assert_eq!(c[0].start_beats, 4.0);
        assert_eq!(f.comp_with(9.0, 9.0, 1), f.comp);
        assert_eq!(f.comp_with(5.0, 6.0, 7), f.comp);
    }

    #[test]
    fn comp_clips_read_the_right_place() {
        let mut f = folder();
        f.comp = f.whole(1);
        f.comp = f.comp_with(8.0, 12.0, 2);
        let mut n = 100;
        let clips = f.comp_clips(|| {
            n += 1;
            ClipId(n)
        });
        assert_eq!(clips.len(), 3);
        assert_eq!(clips[1].audio.as_deref(), Some("aufnahmen/a2.wav"));
        assert_eq!(clips[1].start_beats, 8.0);
        assert_eq!(clips[1].length_beats, 4.0);
        // take 2 offset 32.5 at folder start 4.0, segment starts 4 beats later
        assert_eq!(clips[1].offset_beats, 36.5);
        assert_eq!(clips[0].offset_beats, 16.5);
    }

    #[test]
    fn check_rejects_bad_folders() {
        let mut f = folder();
        f.check().unwrap();
        f.length_beats = MAX_TAKE_BEATS;
        assert!(f.check().is_err(), "#88: end beyond the limit");
        let mut f = folder();
        f.takes[0].offset_beats = MAX_TAKE_BEATS * 2.0;
        assert!(f.check().is_err(), "#88: offset beyond the limit");
        let mut f = folder();
        f.takes[0].audio = "../x.wav".into();
        assert!(f.check().is_err());
        let mut f = folder();
        f.comp = vec![CompSegment {
            start_beats: 4.0,
            end_beats: 30.0,
            take: 0,
        }];
        assert!(f.check().is_err());
        f.comp = vec![CompSegment {
            start_beats: 4.0,
            end_beats: 8.0,
            take: 5,
        }];
        assert!(f.check().is_err());
        let mut f = folder();
        f.takes.clear();
        assert!(f.check().is_err());
    }

    fn input() -> RecordInput {
        RecordInput {
            start_beats: 8.0,
            tempo_bpm: 120.0,
            rate: 48_000.0,
            frames: 48_000 * 10,
            latency_frames: 960,
            loop_beats: None,
            punch_beats: None,
        }
    }

    #[test]
    fn single_take_skips_the_latency() {
        let Some(RecordPlan::Single(p)) = plan_recording(&input()) else {
            panic!("single expected");
        };
        assert_eq!(p.start_beats, 8.0);
        // 960 frames = 0.04 beats at 24 000 frames per beat.
        assert_eq!((p.offset_beats * 24_000.0).floor() as u64, 960);
        assert!((p.length_beats - (48_000.0 * 10.0 - 960.0) / 24_000.0).abs() < 1e-9);
    }

    #[test]
    fn punch_keeps_only_the_range() {
        let mut i = input();
        i.punch_beats = Some((10.0, 12.0));
        let Some(RecordPlan::Single(p)) = plan_recording(&i) else {
            panic!("single expected");
        };
        assert_eq!((p.start_beats, p.length_beats), (10.0, 2.0));
        assert_eq!((p.offset_beats * 24_000.0).floor() as u64, 960 + 2 * 24_000);
        // Outside the recording: nothing.
        i.punch_beats = Some((100.0, 101.0));
        assert!(plan_recording(&i).is_none());
    }

    #[test]
    fn loop_makes_one_take_per_round() {
        let mut i = input();
        i.start_beats = 4.0;
        i.loop_beats = Some((4.0, 8.0)); // 4 beats = 2 s = 96 000 frames
        i.frames = 96_000 * 3 + 960 + 5_000; // three full rounds and a stub
        let Some(RecordPlan::Folder {
            start_beats,
            length_beats,
            offsets,
        }) = plan_recording(&i)
        else {
            panic!("folder expected");
        };
        assert_eq!((start_beats, length_beats), (4.0, 4.0));
        assert_eq!(offsets.len(), 3);
        for (k, o) in offsets.iter().enumerate() {
            assert_eq!((o * 24_000.0).floor() as u64, 960 + k as u64 * 96_000);
        }
        // Less than one round still gives one take.
        i.frames = 30_000;
        let Some(RecordPlan::Folder { offsets, .. }) = plan_recording(&i) else {
            panic!("folder expected");
        };
        assert_eq!(offsets.len(), 1);
        // Punch inside the loop narrows the folder.
        i.frames = 96_000 * 2 + 960;
        i.punch_beats = Some((5.0, 7.0));
        let Some(RecordPlan::Folder {
            start_beats,
            length_beats,
            offsets,
        }) = plan_recording(&i)
        else {
            panic!("folder expected");
        };
        assert_eq!((start_beats, length_beats), (5.0, 2.0));
        assert_eq!(
            (offsets[1] * 24_000.0).floor() as u64,
            960 + 96_000 + 24_000
        );
    }

    #[test]
    fn nothing_recorded_gives_nothing() {
        let mut i = input();
        i.frames = 960;
        assert!(plan_recording(&i).is_none());
    }
}
