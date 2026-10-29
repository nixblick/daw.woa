//! Dateien lesen und messen (Control-Thread): Stereo-Puffer in Engine-Rate, Lautheit.

use std::path::Path;

use woa_engine::decode::{MAX_RATE, MIN_RATE, decode_file, is_readable, resample_linear};
use woa_engine::loudness::integrated_lufs;

/// Fertig dekodiertes Stereo in Mixer-Rate, beide Kanäle gleich lang.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stereo {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

impl Stereo {
    pub fn frames(&self) -> usize {
        self.left.len().min(self.right.len())
    }
}

/// Formate, die der Sendeablauf lesen kann (wie die Engine, `woa_engine::decode::EXTENSIONS`).
pub fn is_supported(path: &Path) -> bool {
    is_readable(path)
}

/// Fehlermeldung für eine Datei, die nicht gelesen werden kann.
pub fn unsupported(path: &Path) -> String {
    format!(
        "{}: nicht lesbar, nur {}",
        path.display(),
        woa_engine::decode::FILTER_NAME
    )
}

/// Liest jede lesbare Audiodatei als Stereo in `rate` (mono wird verdoppelt, mehr als zwei
/// Kanäle: die ersten beiden; Rate linear umgerechnet). Nie im Audio-Thread aufrufen.
pub fn load_stereo(path: &Path, rate: u32) -> Result<Stereo, String> {
    if !is_supported(path) {
        return Err(unsupported(path));
    }
    if !(MIN_RATE..=MAX_RATE).contains(&rate) {
        return Err(format!("{}: Abtastrate außerhalb", path.display()));
    }
    let d = decode_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (left, right) = d.stereo();
    if d.rate == rate || left.is_empty() {
        return Ok(Stereo { left, right });
    }
    Ok(Stereo {
        left: resample(&left, d.rate, rate),
        right: resample(&right, d.rate, rate),
    })
}

/// Linear von `from` auf `to` Hz.
pub fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    resample_linear(x, from, to)
}

/// Messwerte eines Titels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measured {
    pub lufs: Option<f64>,
    pub peak_db: Option<f64>,
    pub seconds: f64,
}

/// Misst integrierte Lautheit, Spitze und Länge.
pub fn measure(s: &Stereo, rate: u32) -> Measured {
    let peak = s
        .left
        .iter()
        .chain(&s.right)
        .fold(0.0f32, |m, v| m.max(v.abs()));
    Measured {
        lufs: integrated_lufs(&s.left, &s.right, rate),
        peak_db: (peak > 0.0).then(|| 20.0 * f64::from(peak).log10()),
        seconds: s.frames() as f64 / f64::from(rate.max(1)),
    }
}

/// Liest und misst eine Datei (für den Hintergrund-Thread beim Hinzufügen).
pub fn measure_file(path: &Path, rate: u32) -> Result<Measured, String> {
    load_stereo(path, rate).map(|s| measure(&s, rate))
}

/// Kennung des Dateistands (Größe und Änderungszeit), damit eine gecachte Messung
/// nach einer Änderung der Datei nicht mehr gilt.
pub fn file_stamp(path: &Path) -> Option<String> {
    let m = std::fs::metadata(path).ok()?;
    let mtime = m
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis();
    Some(format!("{}-{mtime}", m.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_mono_int_wav_as_stereo_and_resamples() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("a.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 24_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&p, spec).unwrap();
        for _ in 0..240 {
            w.write_sample(16_384i16).unwrap();
        }
        w.finalize().unwrap();
        let s = load_stereo(&p, 24_000).unwrap();
        assert_eq!(s.frames(), 240);
        assert!((s.left[3] - 0.5).abs() < 1e-6 && s.left == s.right);
        assert_eq!(load_stereo(&p, 48_000).unwrap().frames(), 480);
        assert!(file_stamp(&p).is_some());
        let e = load_stereo(&tmp.path().join("x.opus"), 48_000).unwrap_err();
        assert!(e.contains("MP3"), "{e}");
    }

    #[test]
    fn reads_mp3_and_flac_as_stereo_in_mixer_rate() {
        let tmp = tempfile::tempdir().unwrap();
        let l: Vec<f32> = (0..44_100)
            .map(|i| 0.5 * (std::f32::consts::TAU * 440.0 * i as f32 / 44_100.0).sin())
            .collect();
        let r: Vec<f32> = l.iter().map(|v| v * 0.5).collect();
        for ext in ["flac", "mp3"] {
            let p = tmp.path().join(format!("t.{ext}"));
            woa_engine::export::export_stereo(&p, &l, &r, 44_100).unwrap();
            assert!(is_supported(&p));
            let s = load_stereo(&p, 48_000).unwrap();
            assert!(
                s.frames().abs_diff(48_000) <= 2_700,
                "{ext}: {}",
                s.frames()
            );
            let m = measure(&s, 48_000);
            // Sinus -6 dBFS Spitze; MP3 darf leicht überschwingen.
            let peak = m.peak_db.unwrap();
            assert!((peak + 6.0).abs() < 0.5, "{ext}: {peak}");
        }
    }
}
