//! Audiodateien lesen (WAV, FLAC, MP3) – nur im Control- oder Hintergrund-Thread, nie im
//! Audio-Thread. Ergebnis: interleavte f32-Samples in Dateirate; Umrechnen auf die
//! Projektrate macht [`resample_linear`].

use std::fs::File;
use std::path::Path;

use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;

/// Kleinste und größte Dateirate, die gelesen wird.
pub const MIN_RATE: u32 = 8_000;
pub const MAX_RATE: u32 = 384_000;
/// Längste Datei, die gelesen wird (Speicherschutz).
pub const MAX_SECONDS: u64 = 6 * 3600;

/// Endungen, die WOA lesen kann (für Dateidialoge: beide Schreibweisen).
pub const EXTENSIONS: [&str; 6] = ["wav", "flac", "mp3", "WAV", "FLAC", "MP3"];

/// Kann WOA diese Datei (nach Endung) lesen?
pub fn is_readable(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// Dekodierte Datei: interleavte Samples in Dateirate.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Decoded {
    pub samples: Vec<f32>,
    pub channels: usize,
    pub rate: u32,
}

impl Decoded {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1)
    }

    /// Mittelwert aller Kanäle.
    pub fn mono(&self) -> Vec<f32> {
        let ch = self.channels.max(1);
        self.samples
            .chunks_exact(ch)
            .map(|f| f.iter().sum::<f32>() / ch as f32)
            .collect()
    }

    /// Links/rechts (mono wird verdoppelt, mehr als zwei Kanäle: die ersten beiden).
    pub fn stereo(&self) -> (Vec<f32>, Vec<f32>) {
        let ch = self.channels.max(1);
        let mut l = Vec::with_capacity(self.frames());
        let mut r = Vec::with_capacity(self.frames());
        for f in self.samples.chunks_exact(ch) {
            l.push(f[0]);
            r.push(if ch > 1 { f[1] } else { f[0] });
        }
        (l, r)
    }
}

/// Liest WAV (hound), FLAC oder MP3 (symphonia). Fehlertext ohne Pfad.
pub fn decode_file(path: &Path) -> Result<Decoded, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let d = match ext.as_str() {
        "wav" => decode_wav(path)?,
        "flac" | "mp3" => decode_symphonia(path, &ext)?,
        _ => return Err("nur WAV, FLAC und MP3".into()),
    };
    if !(MIN_RATE..=MAX_RATE).contains(&d.rate) {
        return Err("Abtastrate außerhalb".into());
    }
    Ok(d)
}

fn check_len(frames: u64, rate: u32) -> Result<(), String> {
    if rate > 0 && frames / u64::from(rate) > MAX_SECONDS {
        return Err("länger als 6 Stunden".into());
    }
    Ok(())
}

fn decode_wav(path: &Path) -> Result<Decoded, String> {
    let e = |x: hound::Error| x.to_string();
    let mut reader = hound::WavReader::open(path).map_err(e)?;
    let spec = reader.spec();
    if !(MIN_RATE..=MAX_RATE).contains(&spec.sample_rate) {
        return Err("Abtastrate außerhalb".into());
    }
    check_len(u64::from(reader.duration()), spec.sample_rate)?;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(e)?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample.clamp(1, 32) - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<Result<_, _>>()
                .map_err(e)?
        }
    };
    Ok(Decoded {
        samples,
        channels: usize::from(spec.channels.max(1)),
        rate: spec.sample_rate,
    })
}

fn decode_symphonia(path: &Path, ext: &str) -> Result<Decoded, String> {
    let e = |x: SymError| x.to_string();
    let file = File::open(path).map_err(|x| x.to_string())?;
    let mss = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    hint.with_extension(ext);
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(e)?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or("keine Tonspur")?;
    let track_id = track.id;
    let num_frames = track.num_frames;
    let Some(CodecParameters::Audio(params)) = track.codec_params.clone() else {
        return Err("keine Tonspur".into());
    };
    let mut rate = params.sample_rate.unwrap_or(0);
    let mut channels = params.channels.as_ref().map_or(0, |c| c.count());
    if let Some(n) = num_frames {
        check_len(n, rate)?;
    }
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(e)?;
    let mut samples = Vec::new();
    let mut chunk: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(SymError::IoError(x)) if x.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(x) => return Err(e(x)),
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buf) => {
                rate = buf.spec().rate();
                channels = buf.spec().channels().count();
                buf.copy_to_vec_interleaved(&mut chunk);
                samples.extend_from_slice(&chunk);
            }
            // Ein kaputter Frame ist kein Abbruch (z. B. MP3 mit Fehlern).
            Err(SymError::DecodeError(_)) => continue,
            Err(x) => return Err(e(x)),
        }
        if channels > 0 {
            check_len((samples.len() / channels) as u64, rate)?;
        }
    }
    if channels == 0 {
        return Err("keine Kanäle".into());
    }
    Ok(Decoded {
        samples,
        channels,
        rate,
    })
}

/// Linear von `from` auf `to` Hz (wie bisher bei WAV mit anderer Rate).
pub fn resample_linear(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if x.is_empty() || from == to || to == 0 {
        return x.to_vec();
    }
    let ratio = f64::from(from) / f64::from(to);
    let n = (x.len() as f64 / ratio) as usize;
    let last = x.len() - 1;
    (0..n)
        .map(|i| {
            let p = i as f64 * ratio;
            let a = (p as usize).min(last);
            let b = (a + 1).min(last);
            let t = (p - a as f64) as f32;
            x[a] * (1.0 - t) + x[b] * t
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::export_stereo;

    /// 1 s Sinus 440 Hz, links -6 dBFS, rechts halb so laut.
    fn tone(rate: u32) -> (Vec<f32>, Vec<f32>) {
        let l: Vec<f32> = (0..rate)
            .map(|i| 0.5 * (std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin())
            .collect();
        let r = l.iter().map(|v| v * 0.5).collect();
        (l, r)
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    fn roundtrip(ext: &str, len_tol: usize, rms_tol: f32) {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(format!("t.{ext}"));
        let (l, r) = tone(44_100);
        export_stereo(&p, &l, &r, 44_100).unwrap();
        assert!(is_readable(&p));
        let d = decode_file(&p).unwrap();
        assert_eq!(d.rate, 44_100);
        assert_eq!(d.channels, 2);
        let (dl, dr) = d.stereo();
        assert!(
            dl.len().abs_diff(l.len()) <= len_tol,
            "{ext}: {} statt {}",
            dl.len(),
            l.len()
        );
        assert!((rms(&dl) - rms(&l)).abs() < rms_tol, "{ext} links");
        assert!((rms(&dr) - rms(&r)).abs() < rms_tol, "{ext} rechts");
    }

    #[test]
    fn flac_roundtrip_keeps_length_and_level() {
        roundtrip("flac", 0, 1e-3);
    }

    #[test]
    fn mp3_roundtrip_keeps_length_and_level() {
        // MP3 hat Encoder-Verzögerung und Auffüllung; gapless kürzt das meiste weg.
        roundtrip("mp3", 2_400, 0.01);
    }

    #[test]
    fn mp3_and_flac_load_as_mono_in_project_rate() {
        let dir = tempfile::tempdir().unwrap();
        for ext in ["flac", "mp3"] {
            let p = dir.path().join(format!("m.{ext}"));
            let (l, r) = tone(44_100);
            export_stereo(&p, &l, &r, 44_100).unwrap();
            let m = crate::clip::load_wav_mono(&p, 48_000).unwrap();
            assert!(m.len().abs_diff(48_000) <= 2_700, "{ext}: {}", m.len());
            let want = rms(&l) * 0.75;
            assert!((rms(&m) - want).abs() < 0.01, "{ext}: {}", rms(&m));
        }
    }

    #[test]
    fn unknown_extension_and_garbage_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.ogg");
        std::fs::write(&p, b"nope").unwrap();
        assert!(!is_readable(&p));
        assert!(decode_file(&p).is_err());
        let p = dir.path().join("x.mp3");
        std::fs::write(&p, b"kein mp3").unwrap();
        assert!(decode_file(&p).is_err());
    }
}
