//! Audio-Clips abspielen: ein [`ClipPlayer`] je Spur liest die Transportposition
//! lock-frei aus dem [`TransportHandle`] und mischt alle Clips der Spur, die gerade laufen.
//!
//! Die Samples liegen fertig dekodiert (mono, Engine-Rate) in `Arc<[f32]>`; der Audio-Thread
//! liest nur, er allokiert nicht.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use woa_dsp::{Node, ProcessBuffers};

use woa_model::{Clip, Project, TrackId, TrackKind};

use crate::project::{ProjectGraph, build_project_graph};
use crate::transport::{TransportHandle, beats_to_samples};
use crate::{Engine, EngineError};

/// Erlaubte Abtastraten beim Laden.
const MIN_RATE: u32 = 8_000;
const MAX_RATE: u32 = 384_000;
/// Längster Clip, der geladen wird (Speicherschutz gegen kaputte Dateien).
const MAX_CLIP_SECONDS: u64 = 6 * 3600;

/// Ein Clip auf der Zeitachse in Samples.
#[derive(Debug, Clone)]
pub struct PlacedClip {
    /// Start auf der Zeitachse (Samples).
    pub start: u64,
    /// Mono-Samples in Engine-Rate, schon auf Versatz und Länge zugeschnitten.
    pub samples: Arc<[f32]>,
    /// Fade-in-Länge in Samples.
    pub fade_in: u64,
    /// Fade-out-Länge in Samples.
    pub fade_out: u64,
}

impl PlacedClip {
    /// Ohne Fades.
    pub fn plain(start: u64, samples: Arc<[f32]>) -> Self {
        Self {
            start,
            samples,
            fade_in: 0,
            fade_out: 0,
        }
    }

    /// Lineare Hüllkurve an Stelle `i` des Clips.
    fn envelope(&self, i: u64) -> f32 {
        let len = self.samples.len() as u64;
        let mut g = 1.0f32;
        if self.fade_in > 0 && i < self.fade_in {
            g = g.min(i as f32 / self.fade_in as f32);
        }
        let left = len.saturating_sub(i);
        if self.fade_out > 0 && left < self.fade_out {
            g = g.min(left as f32 / self.fade_out as f32);
        }
        g
    }
}

/// Setzt einen Modell-Clip auf die Zeitachse: Versatz und Länge schneiden die Samples zu,
/// Fades werden in Samples umgerechnet.
pub fn place_clip(clip: &Clip, file: &Arc<[f32]>, tempo_bpm: f64, rate: f32) -> PlacedClip {
    let to = |b: f64| beats_to_samples(b.max(0.0), tempo_bpm, rate);
    let from = (to(clip.offset_beats) as usize).min(file.len());
    let until = (from + to(clip.length_beats) as usize).min(file.len());
    let samples: Arc<[f32]> = if from == 0 && until == file.len() {
        Arc::clone(file)
    } else {
        file[from..until].into()
    };
    PlacedClip {
        start: to(clip.start_beats),
        samples,
        fade_in: to(clip.fade_in_beats),
        fade_out: to(clip.fade_out_beats),
    }
}

/// Baut den Projekt-Graphen mit einem [`ClipPlayer`] je Audio-Spur. `load` liefert die
/// dekodierten Samples zu einem Clip-Pfad (oder `None`: Clip bleibt stumm). `meters`
/// bekommt je Spur einen Pegelmesser, wenn gesetzt.
pub fn clip_graph(
    project: &Project,
    transport: &TransportHandle,
    rate: f32,
    load: &mut dyn FnMut(&str) -> Option<Arc<[f32]>>,
    mut meters: Option<&mut HashMap<TrackId, Arc<AtomicU32>>>,
) -> ProjectGraph {
    let mut source = |t: &woa_model::Track| -> Option<Box<dyn Node>> {
        if t.kind != TrackKind::Audio {
            return None;
        }
        let clips = t
            .clips
            .iter()
            .filter_map(|c| {
                let file = load(c.audio.as_deref()?)?;
                Some(place_clip(c, &file, project.tempo_bpm, rate))
            })
            .collect();
        let mut player = ClipPlayer::new(clips, transport.clone());
        if let Some(m) = meters.as_deref_mut() {
            player = player.with_meter(Arc::clone(m.entry(t.id).or_default()));
        }
        Some(Box::new(player))
    };
    build_project_graph(project, &mut source)
}

/// Ende des letzten Clips in Sekunden.
pub fn project_end_seconds(project: &Project) -> f64 {
    let beats = project
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .map(Clip::end_beats)
        .fold(0.0f64, f64::max);
    beats * 60.0 / project.tempo_bpm
}

/// Rendert das Projekt mit seinen Audio-Clips offline (links, rechts) über `seconds`.
pub fn render_clips(
    project: &Project,
    rate: u32,
    seconds: f64,
    load: &mut dyn FnMut(&str) -> Option<Arc<[f32]>>,
) -> Result<(Vec<f32>, Vec<f32>), EngineError> {
    const BLOCK: usize = 512;
    let (mut engine, mut processor) = Engine::new(16);
    let graph = clip_graph(project, &engine.transport(), rate as f32, load, None);
    engine.set_graph(graph.builder, rate as f32, BLOCK)?;
    engine.set_tempo(project.tempo_bpm)?;
    engine.play()?;
    let frames = (seconds.max(0.0) * f64::from(rate)) as usize;
    let mut left = vec![0.0f32; frames];
    let mut right = vec![0.0f32; frames];
    for (l, r) in left.chunks_mut(BLOCK).zip(right.chunks_mut(BLOCK)) {
        processor.process_stereo(l, r);
    }
    drop(processor);
    engine.collect();
    Ok((left, right))
}

/// Schreibt Stereo als 32-bit-Float-WAV.
pub fn write_wav_stereo(
    path: &Path,
    left: &[f32],
    right: &[f32],
    rate: u32,
) -> Result<(), EngineError> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).map_err(EngineError::Wav)?;
    for (l, r) in left.iter().zip(right) {
        w.write_sample(*l).map_err(EngineError::Wav)?;
        w.write_sample(*r).map_err(EngineError::Wav)?;
    }
    w.finalize().map_err(EngineError::Wav)
}

/// Samples je Wellenform-Punkt.
pub const PEAK_BLOCK: usize = 256;

/// Spitzenwerte je [`PEAK_BLOCK`] Samples, für die Wellenform-Anzeige.
pub fn peaks(samples: &[f32]) -> Vec<f32> {
    samples
        .chunks(PEAK_BLOCK)
        .map(|c| c.iter().fold(0.0f32, |m, s| m.max(s.abs())))
        .collect()
}

/// Spielt die Clips einer Spur ab, solange der Transport läuft.
pub struct ClipPlayer {
    clips: Vec<PlacedClip>,
    transport: TransportHandle,
    meter: Option<Arc<AtomicU32>>,
}

/// Spitzenpegel seit dem letzten Abholen (f32-Bits), vom Audio-Thread geschrieben.
pub fn take_peak(meter: &AtomicU32) -> f32 {
    f32::from_bits(meter.swap(0, Ordering::Relaxed))
}

impl ClipPlayer {
    pub fn new(clips: Vec<PlacedClip>, transport: TransportHandle) -> Self {
        Self {
            clips,
            transport,
            meter: None,
        }
    }

    /// Meldet den Spitzenpegel jedes Blocks (vor Gain/Mute) in `meter`.
    pub fn with_meter(mut self, meter: Arc<AtomicU32>) -> Self {
        self.meter = Some(meter);
        self
    }
}

impl Node for ClipPlayer {
    fn prepare(&mut self, _sample_rate: f32) {}

    fn set_param(&mut self, _param: u32, _value: f32) {}

    fn process(&mut self, buffers: ProcessBuffers<'_>) {
        let out = buffers.output;
        out.fill(0.0);
        if !self.transport.is_playing() {
            return;
        }
        let pos = self.transport.position();
        let len = out.len() as u64;
        for clip in &self.clips {
            let end = clip.start + clip.samples.len() as u64;
            if end <= pos || clip.start >= pos + len {
                continue;
            }
            let from = pos.max(clip.start);
            let to = (pos + len).min(end);
            let out_off = (from - pos) as usize;
            let src_off = (from - clip.start) as usize;
            let n = (to - from) as usize;
            let faded = clip.fade_in > 0 || clip.fade_out > 0;
            for (k, (o, s)) in out[out_off..out_off + n]
                .iter_mut()
                .zip(&clip.samples[src_off..src_off + n])
                .enumerate()
            {
                *o += if faded {
                    *s * clip.envelope((src_off + k) as u64)
                } else {
                    *s
                };
            }
        }
        if let Some(m) = &self.meter {
            let peak = out.iter().fold(0.0f32, |a, s| a.max(s.abs()));
            let mut cur = m.load(Ordering::Relaxed);
            while peak > f32::from_bits(cur) {
                match m.compare_exchange_weak(
                    cur,
                    peak.to_bits(),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(seen) => cur = seen,
                }
            }
        }
    }
}

/// Liest eine WAV-Datei, mischt auf mono und rechnet linear auf `sample_rate` um.
pub fn load_wav_mono(path: &Path, sample_rate: u32) -> Result<Arc<[f32]>, EngineError> {
    let err = EngineError::Wav;
    let mut reader = hound::WavReader::open(path).map_err(err)?;
    let spec = reader.spec();
    if !(MIN_RATE..=MAX_RATE).contains(&spec.sample_rate)
        || !(MIN_RATE..=MAX_RATE).contains(&sample_rate)
    {
        return Err(EngineError::InvalidConfig("sample rate out of range"));
    }
    if u64::from(reader.duration()) / u64::from(spec.sample_rate) > MAX_CLIP_SECONDS {
        return Err(EngineError::InvalidConfig("clip longer than 6 hours"));
    }
    let channels = usize::from(spec.channels.max(1));
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(err)?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample.max(1) - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<Result<_, _>>()
                .map_err(err)?
        }
    };
    let mono: Vec<f32> = raw
        .chunks(channels)
        .map(|f| f.iter().sum::<f32>() / channels as f32)
        .collect();
    if spec.sample_rate == sample_rate || mono.is_empty() {
        return Ok(mono.into());
    }
    let ratio = f64::from(spec.sample_rate) / f64::from(sample_rate);
    let out_len = (mono.len() as f64 / ratio) as usize;
    let last = mono.len() - 1;
    let resampled: Vec<f32> = (0..out_len)
        .map(|i| {
            let x = i as f64 * ratio;
            let a = (x as usize).min(last);
            let b = (a + 1).min(last);
            let t = (x - a as f64) as f32;
            mono[a] * (1.0 - t) + mono[b] * t
        })
        .collect();
    Ok(resampled.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Engine, GraphBuilder};

    #[test]
    fn plays_clip_at_its_position_only_while_playing() {
        let (mut engine, mut proc_) = Engine::new(16);
        let samples: Arc<[f32]> = vec![0.5; 100].into();
        let meter = Arc::new(AtomicU32::new(0));
        let mut b = GraphBuilder::new();
        let n = b.add_node(
            ClipPlayer::new(vec![PlacedClip::plain(50, samples)], engine.transport())
                .with_meter(Arc::clone(&meter)),
        );
        b.set_output(n);
        engine.set_graph(b, 48_000.0, 64).unwrap();
        let mut out = vec![0.0; 64];
        proc_.process(&mut out);
        assert!(out.iter().all(|&s| s == 0.0), "stopped = silent");
        engine.play().unwrap();
        let mut all = vec![0.0; 256];
        for c in all.chunks_mut(64) {
            proc_.process(c);
        }
        // Transport started at 0 after one stopped block (position stays 0 while stopped).
        assert_eq!(all[49], 0.0);
        assert_eq!(all[50], 0.5);
        assert_eq!(all[149], 0.5);
        assert_eq!(all[150], 0.0);
        assert_eq!(take_peak(&meter), 0.5);
        assert_eq!(take_peak(&meter), 0.0, "reset after take");
    }

    #[test]
    fn place_clip_trims_and_fades() {
        // 120 bpm at 4 Hz: one beat = 2 samples.
        let file: Arc<[f32]> = (0..20).map(|i| i as f32).collect::<Vec<_>>().into();
        let mut c = Clip::new(woa_model::ClipId(1), "c", 1.0, 3.0, None);
        c.offset_beats = 2.0;
        c.fade_in_beats = 1.0;
        let p = place_clip(&c, &file, 120.0, 4.0);
        assert_eq!(p.start, 2);
        assert_eq!(&p.samples[..], &[4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
        assert_eq!(p.fade_in, 2);
        assert_eq!(p.envelope(0), 0.0);
        assert_eq!(p.envelope(1), 0.5);
        assert_eq!(p.envelope(2), 1.0);
    }

    #[test]
    fn render_clips_places_audio_and_exports() {
        let mut project = Project::new("r");
        let mut t = woa_model::Track::new(TrackId(1), "t", TrackKind::Audio);
        t.clips.push(Clip::new(
            woa_model::ClipId(2),
            "c",
            2.0,
            2.0,
            Some("a".into()),
        ));
        project.tracks.push(t);
        let file: Arc<[f32]> = vec![0.5; 48_000].into();
        let (l, _r) =
            render_clips(&project, 48_000, 3.0, &mut |_| Some(Arc::clone(&file))).unwrap();
        assert_eq!(l.len(), 144_000);
        assert_eq!(l[47_999], 0.0, "silent before the clip at 1 s");
        assert!(l[48_100] > 0.3, "clip plays at 1 s");
        assert!((project_end_seconds(&project) - 2.0).abs() < 1e-9);
        let tmp = tempfile::tempdir().unwrap();
        write_wav_stereo(&tmp.path().join("o.wav"), &l, &l, 48_000).unwrap();
        assert_eq!(peaks(&[0.1, -0.7, 0.2]), vec![0.7]);
    }

    #[test]
    fn wav_roundtrip_mono_and_resample() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("a.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 24_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut w = hound::WavWriter::create(&p, spec).unwrap();
        for _ in 0..100 {
            w.write_sample(1.0f32).unwrap();
            w.write_sample(0.0f32).unwrap();
        }
        w.finalize().unwrap();
        let s = load_wav_mono(&p, 24_000).unwrap();
        assert_eq!(s.len(), 100);
        assert!((s[0] - 0.5).abs() < 1e-6);
        assert_eq!(load_wav_mono(&p, 48_000).unwrap().len(), 200);
    }
}
