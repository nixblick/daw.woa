//! Dateien lesen und messen (Control-Thread): Stereo-Puffer in Engine-Rate, Lautheit.

use std::path::Path;

use woa_engine::loudness::integrated_lufs;

/// Längster Titel, der geladen wird (Speicherschutz).
const MAX_SECONDS: u64 = 6 * 3600;
const MIN_RATE: u32 = 8_000;
const MAX_RATE: u32 = 384_000;

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

/// Formate, die der Sendeablauf lesen kann. Heute nur WAV (wie die Engine).
pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
}

/// Fehlermeldung für eine Datei, die nicht gelesen werden kann.
pub fn unsupported(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "flac" | "mp3" => format!(
            "{}: {} kann WOA noch nicht lesen, bitte als WAV",
            path.display(),
            ext.to_ascii_uppercase()
        ),
        _ => format!("{}: nur WAV-Dateien", path.display()),
    }
}

/// Liest eine WAV-Datei als Stereo in `rate` (mono wird verdoppelt, mehr als zwei Kanäle:
/// die ersten beiden; Rate linear umgerechnet).
pub fn load_stereo(path: &Path, rate: u32) -> Result<Stereo, String> {
    if !is_supported(path) {
        return Err(unsupported(path));
    }
    let e = |x: hound::Error| format!("{}: {x}", path.display());
    let mut reader = hound::WavReader::open(path).map_err(e)?;
    let spec = reader.spec();
    if !(MIN_RATE..=MAX_RATE).contains(&spec.sample_rate) || !(MIN_RATE..=MAX_RATE).contains(&rate)
    {
        return Err(format!("{}: Abtastrate außerhalb", path.display()));
    }
    if u64::from(reader.duration()) / u64::from(spec.sample_rate) > MAX_SECONDS {
        return Err(format!("{}: länger als 6 Stunden", path.display()));
    }
    let channels = usize::from(spec.channels.max(1));
    let raw: Vec<f32> = match spec.sample_format {
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
    let frames = raw.len() / channels;
    let mut left = Vec::with_capacity(frames);
    let mut right = Vec::with_capacity(frames);
    for f in raw.chunks_exact(channels) {
        left.push(f[0]);
        right.push(if channels > 1 { f[1] } else { f[0] });
    }
    if spec.sample_rate == rate || left.is_empty() {
        return Ok(Stereo { left, right });
    }
    Ok(Stereo {
        left: resample(&left, spec.sample_rate, rate),
        right: resample(&right, spec.sample_rate, rate),
    })
}

/// Linear von `from` auf `to` Hz.
pub fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if x.is_empty() || from == to {
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
        let e = load_stereo(&tmp.path().join("x.mp3"), 48_000).unwrap_err();
        assert!(e.contains("MP3"), "{e}");
    }
}
