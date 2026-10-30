//! Export eines Stereo-Mixdowns: WAV (16/24 bit, 32 bit Float), FLAC (16/24 bit, `flacenc`),
//! MP3 (LAME, 320 kbit/s) und, über ein externes `ffmpeg` ([`crate::ffmpeg`]), Opus, Ogg Vorbis
//! und AAC. FLAC kennt kein Float-Format; 32 bit Float gibt es nur als WAV.

use std::io::{Seek, Write};
use std::path::{Path, PathBuf};

use crate::EngineError;
use crate::ffmpeg::{Codec, Ffmpeg};

/// Zielformat, aus der Dateiendung.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Wav,
    Flac,
    Mp3,
    /// Über ffmpeg.
    Opus,
    /// Ogg Vorbis, über ffmpeg.
    Ogg,
    /// AAC (ADTS), über ffmpeg.
    Aac,
}

impl ExportFormat {
    /// Alle Formate in Menü-Reihenfolge.
    pub const ALL: [ExportFormat; 6] = [
        Self::Wav,
        Self::Flac,
        Self::Mp3,
        Self::Opus,
        Self::Ogg,
        Self::Aac,
    ];

    /// Format zur Endung von `path` (Groß/klein egal), `None` bei unbekannter Endung.
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "wav" => Some(Self::Wav),
            "flac" => Some(Self::Flac),
            "mp3" => Some(Self::Mp3),
            "opus" => Some(Self::Opus),
            "ogg" | "oga" => Some(Self::Ogg),
            "aac" => Some(Self::Aac),
            _ => None,
        }
    }

    /// Dateiendung ohne Punkt.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Flac => "flac",
            Self::Mp3 => "mp3",
            Self::Opus => "opus",
            Self::Ogg => "ogg",
            Self::Aac => "aac",
        }
    }

    /// Name für Menüs.
    pub fn label(self) -> &'static str {
        match self {
            Self::Wav => "WAV",
            Self::Flac => "FLAC",
            Self::Mp3 => "MP3",
            Self::Opus => "Opus",
            Self::Ogg => "Ogg Vorbis",
            Self::Aac => "AAC",
        }
    }

    /// Der Codec, wenn das Format ffmpeg braucht.
    pub fn codec(self) -> Option<Codec> {
        match self {
            Self::Opus => Some(Codec::Opus),
            Self::Ogg => Some(Codec::Vorbis),
            Self::Aac => Some(Codec::Aac),
            _ => None,
        }
    }

    /// Welche Wortbreiten das Format kennt (ohne Wahl: leer).
    pub fn depths(self) -> &'static [BitDepth] {
        match self {
            Self::Wav => &[BitDepth::Int16, BitDepth::Int24, BitDepth::Float32],
            Self::Flac => &[BitDepth::Int16, BitDepth::Int24],
            _ => &[],
        }
    }

    /// Vorgabe-Bitrate in kbit/s für Formate über ffmpeg (sonst 0).
    pub fn default_kbps(self) -> u32 {
        match self {
            Self::Opus => 160,
            Self::Ogg | Self::Aac => 192,
            _ => 0,
        }
    }
}

/// Wortbreite der Samples bei WAV und FLAC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitDepth {
    Int16,
    Int24,
    /// 32 bit Fließkomma, nur WAV.
    Float32,
}

impl BitDepth {
    /// Name für Menüs.
    pub fn label(self) -> &'static str {
        match self {
            Self::Int16 => "16 Bit",
            Self::Int24 => "24 Bit",
            Self::Float32 => "32 Bit Float",
        }
    }
}

/// Alles, was ein Export außer den Samples braucht.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportSpec {
    pub format: ExportFormat,
    /// Nur WAV und FLAC; `None` nimmt die Vorgabe (WAV 32 bit Float, FLAC 24 bit).
    pub depth: Option<BitDepth>,
    /// Nur Opus, Ogg und AAC; 0 nimmt die Vorgabe des Formats.
    pub kbps: u32,
    /// Nur Opus, Ogg und AAC; `None` sucht [`Ffmpeg::locate`].
    pub ffmpeg: Option<PathBuf>,
}

impl ExportSpec {
    /// Vorgaben für `format`.
    pub fn new(format: ExportFormat) -> Self {
        Self {
            format,
            depth: None,
            kbps: 0,
            ffmpeg: None,
        }
    }

    fn effective_depth(&self) -> Result<BitDepth, EngineError> {
        let d = self.depth.unwrap_or(match self.format {
            ExportFormat::Flac => BitDepth::Int24,
            _ => BitDepth::Float32,
        });
        if self.format.depths().contains(&d) {
            Ok(d)
        } else {
            Err(fail(format!(
                "{} kennt {} nicht{}",
                self.format.label(),
                d.label(),
                if d == BitDepth::Float32 {
                    " (FLAC speichert nur ganze Zahlen, Float gibt es als WAV)"
                } else {
                    ""
                }
            )))
        }
    }

    fn locate_ffmpeg(&self) -> Result<Ffmpeg, EngineError> {
        match &self.ffmpeg {
            Some(p) => Ok(Ffmpeg::at(p)),
            None => Ffmpeg::locate().ok_or_else(|| fail(crate::ffmpeg::MISSING)),
        }
    }
}

fn fail(what: impl std::fmt::Display) -> EngineError {
    EngineError::Export(what.to_string())
}

/// Meldung für eine unbekannte Endung.
pub const EXTENSION_HINT: &str = "Endung muss .wav, .flac, .mp3, .opus, .ogg oder .aac sein";

/// Schreibt Stereo im Format der Dateiendung (Vorgaben des Formats).
pub fn export_stereo(
    path: &Path,
    left: &[f32],
    right: &[f32],
    rate: u32,
) -> Result<(), EngineError> {
    let format = ExportFormat::from_path(path).ok_or_else(|| fail(EXTENSION_HINT))?;
    export_with(path, &ExportSpec::new(format), left, right, rate)
}

/// Schreibt Stereo nach `path` mit den Einstellungen in `spec`. Scheitert der Export,
/// bleibt keine halbe Datei zurück (erst kodieren, dann die Datei anlegen).
#[tracing::instrument(level = "info", skip_all, name = "export")]
pub fn export_with(
    path: &Path,
    spec: &ExportSpec,
    left: &[f32],
    right: &[f32],
    rate: u32,
) -> Result<(), EngineError> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    export_with_to(&mut bytes, spec, left, right, rate)?;
    std::fs::write(path, bytes.into_inner()).map_err(fail)
}

/// Schreibt Stereo in ein schon geöffnetes Ziel (etwa eine mit `create_new` angelegte
/// Datei). So bleibt zwischen Prüfen und Schreiben kein Pfad, den jemand austauschen kann.
pub fn export_stereo_to<W: Write + Seek>(
    out: W,
    format: ExportFormat,
    left: &[f32],
    right: &[f32],
    rate: u32,
) -> Result<(), EngineError> {
    export_with_to(out, &ExportSpec::new(format), left, right, rate)
}

/// Wie [`export_stereo_to`], mit Einstellungen.
pub fn export_with_to<W: Write + Seek>(
    mut out: W,
    spec: &ExportSpec,
    left: &[f32],
    right: &[f32],
    rate: u32,
) -> Result<(), EngineError> {
    if left.len() != right.len() {
        return Err(fail("links und rechts sind verschieden lang"));
    }
    match spec.format {
        ExportFormat::Wav => write_wav(
            std::io::BufWriter::new(out),
            left,
            right,
            rate,
            spec.effective_depth()?,
        ),
        ExportFormat::Flac => {
            let bytes = encode_flac(left, right, rate, spec.effective_depth()?)?;
            out.write_all(&bytes).map_err(fail)?;
            out.flush().map_err(fail)
        }
        ExportFormat::Mp3 => {
            let bytes = encode_mp3(left, right, rate)?;
            out.write_all(&bytes).map_err(fail)?;
            out.flush().map_err(fail)
        }
        ExportFormat::Opus | ExportFormat::Ogg | ExportFormat::Aac => {
            let codec = spec.format.codec().ok_or_else(|| fail("kein Codec"))?;
            let ff = spec.locate_ffmpeg()?;
            let kbps = if spec.kbps == 0 {
                spec.format.default_kbps()
            } else {
                spec.kbps
            };
            ff.encode(codec, kbps, left, right, rate, &mut out)
                .map_err(fail)
        }
    }
}

/// Ganzzahl-Sample der Breite `bits` (16 oder 24): begrenzt und gerundet.
fn to_int(s: f32, bits: u32) -> i32 {
    let full = ((1u32 << (bits - 1)) - 1) as f32;
    (s.clamp(-1.0, 1.0) * full).round() as i32
}

/// WAV, Stereo: 16/24 bit ganzzahlig oder 32 bit Float.
fn write_wav<W: Write + Seek>(
    out: W,
    left: &[f32],
    right: &[f32],
    rate: u32,
    depth: BitDepth,
) -> Result<(), EngineError> {
    let (bits, format) = match depth {
        BitDepth::Int16 => (16u16, hound::SampleFormat::Int),
        BitDepth::Int24 => (24u16, hound::SampleFormat::Int),
        BitDepth::Float32 => (32u16, hound::SampleFormat::Float),
    };
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: bits,
        sample_format: format,
    };
    let mut w = hound::WavWriter::new(out, spec).map_err(EngineError::Wav)?;
    for (l, r) in left.iter().zip(right) {
        if depth == BitDepth::Float32 {
            w.write_sample(*l).map_err(EngineError::Wav)?;
            w.write_sample(*r).map_err(EngineError::Wav)?;
        } else {
            w.write_sample(to_int(*l, u32::from(bits)))
                .map_err(EngineError::Wav)?;
            w.write_sample(to_int(*r, u32::from(bits)))
                .map_err(EngineError::Wav)?;
        }
    }
    w.finalize().map_err(EngineError::Wav)
}

fn encode_flac(
    left: &[f32],
    right: &[f32],
    rate: u32,
    depth: BitDepth,
) -> Result<Vec<u8>, EngineError> {
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;

    let bits: u32 = if depth == BitDepth::Int16 { 16 } else { 24 };
    let samples: Vec<i32> = left
        .iter()
        .zip(right)
        .flat_map(|(l, r)| [*l, *r])
        .map(|s| to_int(s, bits))
        .collect();
    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|e| fail(format!("{e:?}")))?;
    let source =
        flacenc::source::MemSource::from_samples(&samples, 2, bits as usize, rate as usize);
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
        assert!(export_stereo(&tmp.path().join("a.xyz"), &s, &s, 48_000).is_err());
        assert!(
            !tmp.path().join("a.xyz").exists(),
            "no file for unknown format"
        );
        // Into an already open handle.
        let mut buf = std::io::Cursor::new(Vec::new());
        export_stereo_to(&mut buf, ExportFormat::Wav, &s, &s, 48_000).unwrap();
        assert!(buf.get_ref().starts_with(b"RIFF"));
    }
}
