//! Audio-Clips abspielen: ein [`ClipPlayer`] je Spur liest die Transportposition
//! lock-frei aus dem [`TransportHandle`] und mischt alle Clips der Spur, die gerade laufen.
//!
//! Die Samples liegen fertig dekodiert (mono, Engine-Rate) in `Arc<[f32]>`; der Audio-Thread
//! liest nur, er allokiert nicht.

use std::path::Path;
use std::sync::Arc;

use woa_dsp::{Node, ProcessBuffers};

use crate::EngineError;
use crate::transport::TransportHandle;

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
    /// Mono-Samples in Engine-Rate.
    pub samples: Arc<[f32]>,
}

/// Spielt die Clips einer Spur ab, solange der Transport läuft.
pub struct ClipPlayer {
    clips: Vec<PlacedClip>,
    transport: TransportHandle,
}

impl ClipPlayer {
    pub fn new(clips: Vec<PlacedClip>, transport: TransportHandle) -> Self {
        Self { clips, transport }
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
            for (o, s) in out[out_off..out_off + n]
                .iter_mut()
                .zip(&clip.samples[src_off..src_off + n])
            {
                *o += *s;
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
        let mut b = GraphBuilder::new();
        let n = b.add_node(ClipPlayer::new(
            vec![PlacedClip { start: 50, samples }],
            engine.transport(),
        ));
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
