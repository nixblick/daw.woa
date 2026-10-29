//! Export eines Stereo-Mixdowns als WAV, FLAC (24 bit, `flacenc`) oder MP3 (LAME, 320 kbit/s).

use std::path::Path;

use crate::EngineError;
use crate::clip::write_wav_stereo;

/// Zielformat, aus der Dateiendung.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Wav,
    Flac,
    Mp3,
}

impl ExportFormat {
    /// Format zur Endung von `path` (Groß/klein egal), `None` bei unbekannter Endung.
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "wav" => Some(Self::Wav),
            "flac" => Some(Self::Flac),
            "mp3" => Some(Self::Mp3),
            _ => None,
        }
    }
}

fn fail(what: impl std::fmt::Display) -> EngineError {
    EngineError::Export(what.to_string())
}

/// Schreibt Stereo im Format der Dateiendung.
pub fn export_stereo(
    path: &Path,
    left: &[f32],
    right: &[f32],
    rate: u32,
) -> Result<(), EngineError> {
    match ExportFormat::from_path(path) {
        Some(ExportFormat::Wav) => write_wav_stereo(path, left, right, rate),
        Some(ExportFormat::Flac) => write_flac(path, left, right, rate),
        Some(ExportFormat::Mp3) => write_mp3(path, left, right, rate),
        None => Err(fail("Endung muss .wav, .flac oder .mp3 sein")),
    }
}

fn write_flac(path: &Path, left: &[f32], right: &[f32], rate: u32) -> Result<(), EngineError> {
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;

    const FULL: f32 = 8_388_607.0; // 24 bit
    let samples: Vec<i32> = left
        .iter()
        .zip(right)
        .flat_map(|(l, r)| [*l, *r])
        .map(|s| (s.clamp(-1.0, 1.0) * FULL).round() as i32)
        .collect();
    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|e| fail(format!("{e:?}")))?;
    let source = flacenc::source::MemSource::from_samples(&samples, 2, 24, rate as usize);
    let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
        .map_err(|e| fail(format!("{e:?}")))?;
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream
        .write(&mut sink)
        .map_err(|e| fail(format!("{e:?}")))?;
    std::fs::write(path, sink.as_slice()).map_err(fail)
}

fn write_mp3(path: &Path, left: &[f32], right: &[f32], rate: u32) -> Result<(), EngineError> {
    use mp3lame_encoder::{Bitrate, Builder, DualPcm, FlushNoGap, Quality};

    const CHUNK: usize = 8192;
    let mut b = Builder::new().ok_or_else(|| fail("LAME nicht verfügbar"))?;
    b.set_num_channels(2).map_err(fail)?;
    b.set_sample_rate(rate).map_err(fail)?;
    b.set_brate(Bitrate::Kbps320).map_err(fail)?;
    b.set_quality(Quality::Best).map_err(fail)?;
    let mut enc = b.build().map_err(fail)?;
    let mut out = Vec::new();
    for (l, r) in left.chunks(CHUNK).zip(right.chunks(CHUNK)) {
        out.reserve(mp3lame_encoder::max_required_buffer_size(l.len()));
        enc.encode_to_vec(DualPcm { left: l, right: r }, &mut out)
            .map_err(fail)?;
    }
    out.reserve(mp3lame_encoder::max_required_buffer_size(0).max(7200));
    enc.flush_to_vec::<FlushNoGap>(&mut out).map_err(fail)?;
    std::fs::write(path, out).map_err(fail)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone() -> Vec<f32> {
        (0..48_000)
            .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5)
            .collect()
    }

    #[test]
    fn exports_all_three_formats() {
        let tmp = tempfile::tempdir().unwrap();
        let s = tone();
        for (name, magic) in [
            ("a.wav", &b"RIFF"[..]),
            ("a.flac", &b"fLaC"[..]),
            ("a.MP3", &[][..]),
        ] {
            let p = tmp.path().join(name);
            export_stereo(&p, &s, &s, 48_000).unwrap();
            let bytes = std::fs::read(&p).unwrap();
            assert!(bytes.len() > 1000, "{name} too small");
            assert!(bytes.starts_with(magic), "{name} header");
        }
        // MP3: first frame sync word.
        let mp3 = std::fs::read(tmp.path().join("a.MP3")).unwrap();
        assert!(mp3.windows(2).any(|w| w[0] == 0xFF && w[1] & 0xE0 == 0xE0));
        assert!(export_stereo(&tmp.path().join("a.ogg"), &s, &s, 48_000).is_err());
    }
}
