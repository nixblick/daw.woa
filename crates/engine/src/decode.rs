//! Audiodateien lesen (WAV, FLAC, MP3, MP2, OGG/Vorbis, M4A/AAC/ALAC, AIFF, CAF) – nur im Control- oder Hintergrund-Thread, nie im
//! Audio-Thread. Ergebnis: interleavte f32-Samples in Dateirate; Umrechnen auf die
//! Projektrate macht [`resample_linear`].

use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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

/// Endungen, die WOA lesen kann (klein geschrieben; Vergleich ohne Rücksicht auf die
/// Schreibweise). Opus fehlt: symphonia hat keinen Opus-Decoder.
pub const EXTENSIONS: [&str; 13] = [
    "wav", "flac", "mp3", "mp2", "ogg", "oga", "m4a", "mp4", "aac", "aif", "aiff", "aifc", "caf",
];

/// Filtertext für Dateidialoge.
pub const FILTER_NAME: &str = "Audio (MP3, FLAC, WAV, OGG, M4A, AAC, AIFF, CAF, MP2)";

/// Endungen für Dateidialoge: jede klein und groß (GTK filtert nach Schreibweise).
pub fn dialog_extensions() -> Vec<String> {
    EXTENSIONS
        .iter()
        .flat_map(|e| [(*e).to_owned(), e.to_ascii_uppercase()])
        .collect()
}

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

/// Befund einer Datei beim letzten Lesen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Damage {
    /// Gelesen, aber mit so vielen fehlerhaften Stellen (Frames).
    Damaged(u64),
    /// Nicht lesbar, mit Grund (ohne Pfad).
    Unreadable(String),
}

impl Damage {
    /// Klartext für Protokoll und Tooltip.
    pub fn text(&self, name: &str) -> String {
        match self {
            Self::Damaged(n) => format!(
                "Datei „{name}“ ist beschädigt ({n} fehlerhafte Stellen), spielt aber. \
                 Tipp: neu exportieren."
            ),
            Self::Unreadable(why) => {
                format!("Datei „{name}“ ist nicht lesbar ({why}). Tipp: neu exportieren.")
            }
        }
    }
}

/// Obergrenze der gemerkten Befunde.
const MAX_DAMAGE: usize = 10_000;

fn damage_map() -> &'static Mutex<HashMap<PathBuf, Damage>> {
    static M: std::sync::OnceLock<Mutex<HashMap<PathBuf, Damage>>> = std::sync::OnceLock::new();
    M.get_or_init(Default::default)
}

/// Befund zu `path` aus dem letzten Lesen, `None` bei heiler oder noch nicht gelesener Datei.
pub fn damage(path: &Path) -> Option<Damage> {
    damage_map().lock().ok()?.get(path).cloned()
}

fn note_damage(path: &Path, d: Option<Damage>) {
    let Ok(mut m) = damage_map().lock() else {
        return;
    };
    let Some(d) = d else {
        m.remove(path);
        return;
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Nur lokal: Pfad und Name gehen nie in den Upload (`woa_telemetry::upload_filter`).
    if m.get(path) != Some(&d) {
        tracing::warn!(
            target: woa_telemetry::LOCAL_TARGET,
            datei = %path.display(),
            "{}",
            d.text(&name)
        );
    }
    if m.len() < MAX_DAMAGE || m.contains_key(path) {
        m.insert(path.to_path_buf(), d);
    }
}

thread_local! {
    static DECODES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Wie oft dieser Thread [`decode_file`] aufgerufen hat (für Tests: „hier wird nicht dekodiert“).
pub fn decodes_on_this_thread() -> u64 {
    DECODES.with(std::cell::Cell::get)
}

/// Liest WAV (hound), FLAC oder MP3 (symphonia). Fehlertext ohne Pfad. Beschädigte oder
/// unlesbare Dateien landen mit Klartext im lokalen Protokoll und in [`damage`].
#[tracing::instrument(level = "info", skip_all, name = "decode_file")]
pub fn decode_file(path: &Path) -> Result<Decoded, String> {
    DECODES.with(|n| n.set(n.get() + 1));
    woa_telemetry::take_decoder_faults();
    let r = decode_inner(path);
    match &r {
        Ok((_, 0)) => note_damage(path, None),
        Ok((_, n)) => note_damage(path, Some(Damage::Damaged(*n))),
        Err(e) => note_damage(path, Some(Damage::Unreadable(e.clone()))),
    }
    r.map(|(d, _)| d)
}

/// Dekodiert und zählt fehlerhafte Frames (verworfene Pakete plus Warnungen des Dekoders).
fn decode_inner(path: &Path) -> Result<(Decoded, u64), String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let d = match ext.as_str() {
        "wav" => (decode_wav(path)?, 0),
        e if EXTENSIONS.contains(&e) => decode_symphonia(path, &ext)?,
        _ => return Err(format!("nicht lesbar, nur {FILTER_NAME}")),
    };
    if !(MIN_RATE..=MAX_RATE).contains(&d.0.rate) {
        return Err("Abtastrate außerhalb".into());
    }
    Ok((d.0, d.1 + woa_telemetry::take_decoder_faults()))
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

fn decode_symphonia(path: &Path, ext: &str) -> Result<(Decoded, u64), String> {
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
    let mut bad = 0u64;
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
            Err(SymError::DecodeError(_)) => {
                bad += 1;
                continue;
            }
            Err(x) => return Err(e(x)),
        }
        if let Some(frames) = samples.len().checked_div(channels) {
            check_len(frames as u64, rate)?;
        }
    }
    Ok((
        Decoded {
            samples,
            channels,
            rate,
        },
        bad,
    ))
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
        let p = dir.path().join("x.opus");
        std::fs::write(&p, b"nope").unwrap();
        assert!(!is_readable(&p));
        assert!(decode_file(&p).is_err());
        let p = dir.path().join("x.ogg");
        std::fs::write(&p, b"kein ogg").unwrap();
        assert!(is_readable(&p));
        assert!(decode_file(&p).is_err());
        let p = dir.path().join("x.mp3");
        std::fs::write(&p, b"kein mp3").unwrap();
        assert!(decode_file(&p).is_err());
    }

    /// ID3v2.3-Tag mit APIC-Bild der Größe `pic` Bytes (Inhalt egal, Bildtyp Cover vorne).
    fn id3v23_with_apic(pic: usize) -> Vec<u8> {
        let mut frame = vec![0u8]; // Textkodierung ISO-8859-1
        frame.extend_from_slice(b"image/jpeg\0");
        frame.push(3); // Cover vorne
        frame.push(0); // leere Beschreibung
        frame.extend(std::iter::repeat_n(0xa5u8, pic));
        let mut body = Vec::new();
        body.extend_from_slice(b"APIC");
        body.extend_from_slice(&(frame.len() as u32).to_be_bytes());
        body.extend_from_slice(&[0, 0]);
        body.extend_from_slice(&frame);
        let n = body.len() as u32;
        // Tag-Größe als Synchsafe-Integer (7 Bit je Byte).
        let size = [
            ((n >> 21) & 0x7f) as u8,
            ((n >> 14) & 0x7f) as u8,
            ((n >> 7) & 0x7f) as u8,
            (n & 0x7f) as u8,
        ];
        let mut tag = b"ID3\x03\x00\x00".to_vec();
        tag.extend_from_slice(&size);
        tag.extend_from_slice(&body);
        tag
    }

    /// Issue #50: MP3 mit 1,2 MB großem ID3v2.3-Tag (Cover) scheiterte im Probe.
    #[test]
    fn mp3_with_large_id3v2_cover_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("raw.mp3");
        let (l, r) = tone(44_100);
        export_stereo(&raw, &l, &r, 44_100).unwrap();
        let mut b = id3v23_with_apic(1_200_000);
        assert!(b.len() >= 500_000);
        b.extend_from_slice(&std::fs::read(&raw).unwrap());
        let p = dir.path().join("cover.mp3");
        std::fs::write(&p, &b).unwrap();
        let d = decode_file(&p).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(d.rate, 44_100);
        assert_eq!(d.channels, 2);
        let (dl, _) = d.stereo();
        assert!(dl.len().abs_diff(l.len()) <= 2_400, "{}", dl.len());
        assert!((rms(&dl) - rms(&l)).abs() < 0.01);
    }

    #[test]
    fn extensions_ignore_case() {
        for e in ["x.M4A", "x.Ogg", "x.AIFF", "x.mp3", "x.CAF"] {
            assert!(is_readable(Path::new(e)), "{e}");
        }
        assert!(!is_readable(Path::new("x.opus")));
        let d = dialog_extensions();
        assert!(d.contains(&"m4a".to_owned()) && d.contains(&"M4A".to_owned()));
    }

    /// ffmpeg vorhanden? Ohne ffmpeg werden die Formattests übersprungen (mit Meldung).
    fn has_ffmpeg() -> bool {
        let ok = std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .is_ok_and(|o| o.status.success());
        if !ok {
            eprintln!("ffmpeg fehlt, Formattest übersprungen");
        }
        ok
    }

    fn ffmpeg(input: &[&str], input_path: Option<&Path>, args: &[&str], out: &Path) -> bool {
        let mut c = std::process::Command::new("ffmpeg");
        c.args(["-v", "error", "-y"]).args(input);
        if let Some(i) = input_path {
            c.arg("-i").arg(i);
        }
        c.args(args).arg(out).status().is_ok_and(|s| s.success())
    }

    /// Alle neuen Formate, mit ffmpeg aus dem Testton erzeugt: Länge und Pegel nach dem Lesen.
    #[test]
    fn new_formats_keep_length_and_level() {
        if !has_ffmpeg() {
            return;
        }
        let cases: [(&str, &[&str], usize, f32); 8] = [
            ("t.ogg", &["-c:a", "libvorbis", "-q:a", "6"], 0, 0.01),
            ("t.oga", &["-c:a", "libvorbis", "-f", "ogg"], 0, 0.01),
            ("t.m4a", &["-c:a", "aac", "-b:a", "192k"], 2_400, 0.01),
            ("t.aac", &["-c:a", "aac", "-b:a", "192k"], 2_400, 0.01),
            ("a.m4a", &["-c:a", "alac"], 0, 1e-3),
            ("t.aiff", &["-c:a", "pcm_s16be"], 0, 1e-3),
            ("t.caf", &["-c:a", "pcm_s16le"], 0, 1e-3),
            ("t.mp2", &["-c:a", "mp2", "-b:a", "256k"], 2_400, 0.02),
        ];
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("src.wav");
        let (l, r) = tone(44_100);
        export_stereo(&wav, &l, &r, 44_100).unwrap();
        for (name, args, len_tol, rms_tol) in cases {
            let p = dir.path().join(name);
            assert!(ffmpeg(&[], Some(&wav), args, &p), "ffmpeg {name}");
            assert!(is_readable(&p), "{name}");
            let d = decode_file(&p).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(d.rate, 44_100, "{name}");
            assert_eq!(d.channels, 2, "{name}");
            let (dl, dr) = d.stereo();
            assert!(
                dl.len().abs_diff(l.len()) <= len_tol,
                "{name}: {} statt {}",
                dl.len(),
                l.len()
            );
            assert!(
                (rms(&dl) - rms(&l)).abs() < rms_tol,
                "{name} links {}",
                rms(&dl)
            );
            assert!(
                (rms(&dr) - rms(&r)).abs() < rms_tol,
                "{name} rechts {}",
                rms(&dr)
            );
        }
    }

    /// Speicherschutz aus #49 gilt auch für die neuen Formate: 9 Kanäle werden abgelehnt.
    #[test]
    fn new_formats_respect_channel_limit() {
        if !has_ffmpeg() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let src = ["-f", "lavfi", "-i", "sine=f=440:d=0.2:r=44100"];
        let pan = "pan=FL+FR+FC+LFE+BL+BR+FLC+FRC+BC|c0=c0|c1=c0|c2=c0|c3=c0|c4=c0|c5=c0|c6=c0|c7=c0|c8=c0";
        for (name, codec) in [("c9.aiff", "pcm_s16be"), ("c9.caf", "pcm_s16le")] {
            let p = dir.path().join(name);
            assert!(
                ffmpeg(&src, None, &["-af", pan, "-c:a", codec], &p),
                "ffmpeg {name}"
            );
            let e = decode_file(&p).unwrap_err();
            assert!(e.contains("Kanäle"), "{name}: {e}");
        }
    }
}
