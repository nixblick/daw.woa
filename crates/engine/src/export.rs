//! Export eines Stereo-Mixdowns als WAV, FLAC (24 bit, `flacenc`) oder MP3 (LAME, 320 kbit/s).

use std::io::{Seek, Write};
use std::path::Path;

use crate::EngineError;

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
    let format = ExportFormat::from_path(path)
        .ok_or_else(|| fail("Endung muss .wav, .flac oder .mp3 sein"))?;
    let file = std::fs::File::create(path).map_err(fail)?;
    export_stereo_to(file, format, left, right, rate)
}

/// Schreibt Stereo in ein schon geöffnetes Ziel (etwa eine mit `create_new` angelegte
/// Datei). So bleibt zwischen Prüfen und Schreiben kein Pfad, den jemand austauschen kann.
pub fn export_stereo_to<W: Write + Seek>(
    mut out: W,
    format: ExportFormat,
    left: &[f32],
    right: &[f32],
    rate: u32,
) -> Result<(), EngineError> {
    match format {
        ExportFormat::Wav => write_wav(std::io::BufWriter::new(out), left, right, rate),
        ExportFormat::Flac => {
            let bytes = encode_flac(left, right, rate)?;
            out.write_all(&bytes).map_err(fail)?;
            out.flush().map_err(fail)
        }
        ExportFormat::Mp3 => {
            let bytes = encode_mp3(left, right, rate)?;
            out.write_all(&bytes).map_err(fail)?;
            out.flush().map_err(fail)
        }
    }
}

/// 32-bit-Float-WAV, Stereo.
fn write_wav<W: Write + Seek>(
    out: W,
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
    let mut w = hound::WavWriter::new(out, spec).map_err(EngineError::Wav)?;
    for (l, r) in left.iter().zip(right) {
        w.write_sample(*l).map_err(EngineError::Wav)?;
        w.write_sample(*r).map_err(EngineError::Wav)?;
    }
    w.finalize().map_err(EngineError::Wav)
}

fn encode_flac(left: &[f32], right: &[f32], rate: u32) -> Result<Vec<u8>, EngineError> {
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
    let mut bytes = sink.as_slice().to_vec();
    fix_streaminfo_min_block(&mut bytes);
    Ok(bytes)
}

/// flacenc schreibt bei fester Blockgröße die Länge des (kürzeren) letzten Blocks als
/// Mindest-Blockgröße in STREAMINFO. Laut FLAC-Spezifikation zählt der letzte Block nicht mit;
/// strenge Leser (symphonia) halten den Strom sonst für variabel und finden keinen Frame.
/// STREAMINFO steht immer direkt hinter `fLaC` und dem 4-Byte-Blockkopf: min bei 8..10, max bei 10..12.
fn fix_streaminfo_min_block(bytes: &mut [u8]) {
    if bytes.len() >= 12 && &bytes[..4] == b"fLaC" && bytes[4] & 0x7f == 0 {
        let (min, max) = bytes[8..12].split_at_mut(2);
        min.copy_from_slice(max);
    }
}

fn encode_mp3(left: &[f32], right: &[f32], rate: u32) -> Result<Vec<u8>, EngineError> {
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
    Ok(out)
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
        assert!(
            !tmp.path().join("a.ogg").exists(),
            "no file for unknown format"
        );
        // Into an already open handle.
        let mut buf = std::io::Cursor::new(Vec::new());
        export_stereo_to(&mut buf, ExportFormat::Wav, &s, &s, 48_000).unwrap();
        assert!(buf.get_ref().starts_with(b"RIFF"));
    }
}
