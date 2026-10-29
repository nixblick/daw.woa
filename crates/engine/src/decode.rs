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
/// Höchste Kanalzahl, die gelesen wird.
pub const MAX_CHANNELS: usize = 8;
/// Feste Obergrenze aller dekodierten Samples (alle Kanäle), unabhängig von Angaben aus der
/// Datei: 6 Stunden Stereo bei 48 kHz, gut 8 GB als f32. Gegen präparierte Dateien, deren
/// Kopf harmlos aussieht und deren Pakete dann mehr liefern.
pub const MAX_SAMPLES: usize = 6 * 3600 * 48_000 * 2;

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

/// Rate und Kanalzahl vor dem Dekodieren und für jeden Puffer prüfen.
fn check_format(rate: u32, channels: usize) -> Result<(), String> {
    if !(MIN_RATE..=MAX_RATE).contains(&rate) {
        return Err("Abtastrate außerhalb".into());
    }
    if !(1..=MAX_CHANNELS).contains(&channels) {
        return Err(format!("{channels} Kanäle (höchstens {MAX_CHANNELS})"));
    }
    Ok(())
}

/// Passen `more` weitere Samples noch unter [`MAX_SAMPLES`]?
fn check_total(have: usize, more: usize) -> Result<(), String> {
    if have.saturating_add(more) > MAX_SAMPLES {
        return Err("zu groß (Speicherschutz)".into());
    }
    Ok(())
}

fn decode_wav(path: &Path) -> Result<Decoded, String> {
    let e = |x: hound::Error| x.to_string();
    let mut reader = hound::WavReader::open(path).map_err(e)?;
    let spec = reader.spec();
    let channels = usize::from(spec.channels);
    check_format(spec.sample_rate, channels)?;
    check_len(u64::from(reader.duration()), spec.sample_rate)?;
    check_total(0, reader.len() as usize)?;
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
        channels,
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
    let rate = params.sample_rate.unwrap_or(0);
    let channels = params.channels.as_ref().map_or(0, |c| c.count());
    check_format(rate, channels)?;
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
                // Wechselt Rate oder Kanalzahl mitten in der Datei, wird sie abgelehnt.
                if buf.spec().rate() != rate || buf.spec().channels().count() != channels {
                    return Err("Abtastrate oder Kanalzahl wechselt in der Datei".into());
                }
                let more = buf.frames().saturating_mul(channels);
                check_total(samples.len(), more)?;
                buf.copy_to_vec_interleaved(&mut chunk);
                check_total(samples.len(), chunk.len())?;
                samples.extend_from_slice(&chunk);
            }
            // Ein kaputter Frame ist kein Abbruch (z. B. MP3 mit Fehlern).
            Err(SymError::DecodeError(_)) => continue,
            Err(x) => return Err(e(x)),
        }
        if let Some(frames) = samples.len().checked_div(channels) {
            check_len(frames as u64, rate)?;
        }
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
    fn flac_with_rate_zero_in_header_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("r0.flac");
        let (l, r) = tone(44_100);
        export_stereo(&p, &l, &r, 44_100).unwrap();
        let mut b = std::fs::read(&p).unwrap();
        // STREAMINFO: Rate sind die 20 Bit ab Byte 18.
        b[18] = 0;
        b[19] = 0;
        b[20] &= 0x0f;
        std::fs::write(&p, &b).unwrap();
        assert!(decode_file(&p).is_err());
    }

    #[test]
    fn wav_with_too_many_channels_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("c9.wav");
        let spec = hound::WavSpec {
            channels: 9,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&p, spec).unwrap();
        for _ in 0..9 {
            w.write_sample(0i16).unwrap();
        }
        w.finalize().unwrap();
        let e = decode_file(&p).unwrap_err();
        assert!(e.contains("Kanäle"), "{e}");
    }

    #[test]
    fn wav_claiming_huge_data_is_rejected_before_reading() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big.wav");
        // Kopf: 8 kHz mono 8 Bit, Datenblock angeblich fast 4 GB, real 4 Byte.
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&0xffff_fff0u32.to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&8_000u32.to_le_bytes());
        b.extend_from_slice(&8_000u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&8u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&0xffff_ffd0u32.to_le_bytes());
        b.extend_from_slice(&[128; 4]);
        std::fs::write(&p, &b).unwrap();
        assert!(decode_file(&p).is_err());
    }

    #[test]
    fn format_and_total_limits() {
        assert!(check_format(0, 2).is_err());
        assert!(check_format(48_000, 0).is_err());
        assert!(check_format(48_000, MAX_CHANNELS + 1).is_err());
        assert!(check_format(48_000, 2).is_ok());
        assert!(check_total(MAX_SAMPLES - 10, 10).is_ok());
        assert!(check_total(MAX_SAMPLES - 10, 11).is_err());
        assert!(check_total(usize::MAX, 1).is_err());
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
