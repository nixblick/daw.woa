//! Eingebettetes Cover-Bild einer Audiodatei lesen (ID3 APIC, MP4 `covr`, FLAC PICTURE,
//! Vorbis `METADATA_BLOCK_PICTURE`). Nur im Hintergrund-Thread aufrufen, nie im Audio-Thread.
//! Geliefert werden die kodierten Bildbytes (JPEG/PNG); dekodiert wird in der Oberfläche.

use std::fs::File;
use std::path::Path;

use symphonia::core::common::Limit;
use symphonia::core::formats::FormatOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::{MetadataOptions, MetadataRevision, StandardVisualKey, Visual};

/// Größtes Cover, das gelesen wird (Speicherschutz).
pub const MAX_COVER_BYTES: usize = 10 * 1024 * 1024;

/// Kodiertes Cover-Bild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cover {
    pub media_type: Option<String>,
    pub data: Vec<u8>,
}

/// Liest das Cover der Datei, bevorzugt „Cover vorne“, sonst das erste Bild.
/// `None`: kein Bild, zu groß, oder die Datei ist nicht lesbar.
pub fn read_cover(path: &Path) -> Option<Cover> {
    if !crate::decode::is_readable(path) {
        return None;
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_lowercase();
    let file = File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    hint.with_extension(&ext);
    let meta = MetadataOptions::default().limit_visual_bytes(Limit::Maximum(MAX_COVER_BYTES));
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), meta)
        .ok()?;
    let mut visuals: Vec<Visual> = Vec::new();
    let mut take = |rev: &MetadataRevision| {
        visuals.extend(rev.media.visuals.iter().cloned());
        for t in &rev.per_track {
            visuals.extend(t.metadata.visuals.iter().cloned());
        }
    };
    let mut m = format.metadata();
    loop {
        if let Some(rev) = m.current() {
            take(rev);
        }
        if m.pop().is_none() {
            break;
        }
    }
    pick(visuals)
}

fn pick(visuals: Vec<Visual>) -> Option<Cover> {
    let ok: Vec<Visual> = visuals
        .into_iter()
        .filter(|v| !v.data.is_empty() && v.data.len() <= MAX_COVER_BYTES)
        .collect();
    let best = ok
        .iter()
        .position(|v| v.usage == Some(StandardVisualKey::FrontCover))
        .unwrap_or(0);
    let v = ok.into_iter().nth(best)?;
    Some(Cover {
        media_type: v.media_type,
        data: v.data.into_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::export_stereo;

    /// Kleines gültiges PNG (1×1, rot), selbst erzeugt.
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x08, 0xd7, 0x63, 0xf8,
        0xcf, 0xc0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xdd, 0x8d, 0xb0, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    fn id3v23_apic(mime: &str, pic: &[u8]) -> Vec<u8> {
        let mut frame = vec![0u8];
        frame.extend_from_slice(mime.as_bytes());
        frame.push(0);
        frame.push(3);
        frame.push(0);
        frame.extend_from_slice(pic);
        let mut body = b"APIC".to_vec();
        body.extend_from_slice(&(frame.len() as u32).to_be_bytes());
        body.extend_from_slice(&[0, 0]);
        body.extend_from_slice(&frame);
        let n = body.len() as u32;
        let mut tag = b"ID3\x03\x00\x00".to_vec();
        tag.extend_from_slice(&[
            ((n >> 21) & 0x7f) as u8,
            ((n >> 14) & 0x7f) as u8,
            ((n >> 7) & 0x7f) as u8,
            (n & 0x7f) as u8,
        ]);
        tag.extend_from_slice(&body);
        tag
    }

    fn tone_file(dir: &Path, name: &str) -> std::path::PathBuf {
        let p = dir.join(name);
        let l: Vec<f32> = (0..4_410).map(|i| (i as f32 * 0.06).sin() * 0.3).collect();
        export_stereo(&p, &l, &l, 44_100).unwrap();
        p
    }

    #[test]
    fn mp3_apic_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let raw = tone_file(dir.path(), "raw.mp3");
        let mut b = id3v23_apic("image/png", PNG);
        b.extend_from_slice(&std::fs::read(&raw).unwrap());
        let p = dir.path().join("c.mp3");
        std::fs::write(&p, &b).unwrap();
        let c = read_cover(&p).expect("Cover");
        assert_eq!(c.data, PNG);
        assert_eq!(c.media_type.as_deref(), Some("image/png"));
    }

    #[test]
    fn file_without_cover_and_wav_give_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_cover(&tone_file(dir.path(), "x.mp3")).is_none());
        assert!(read_cover(&tone_file(dir.path(), "x.wav")).is_none());
        assert!(read_cover(&dir.path().join("fehlt.mp3")).is_none());
    }

    #[test]
    fn oversized_cover_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let raw = tone_file(dir.path(), "raw.mp3");
        let mut b = id3v23_apic("image/png", &vec![0u8; MAX_COVER_BYTES + 1]);
        b.extend_from_slice(&std::fs::read(&raw).unwrap());
        let p = dir.path().join("big.mp3");
        std::fs::write(&p, &b).unwrap();
        assert!(read_cover(&p).is_none());
    }

    /// FLAC (PICTURE-Block), M4A (`covr`) und OGG (METADATA_BLOCK_PICTURE) per ffmpeg.
    #[test]
    fn flac_m4a_ogg_covers_via_ffmpeg() {
        if std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .map_or(true, |o| !o.status.success())
        {
            eprintln!("ffmpeg fehlt, Cover-Test übersprungen");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let wav = tone_file(dir.path(), "src.wav");
        let png = dir.path().join("c.png");
        std::fs::write(&png, PNG).unwrap();
        for (name, codec) in [("c.flac", "flac"), ("c.m4a", "aac"), ("c.ogg", "libvorbis")] {
            let out = dir.path().join(name);
            let mut cmd = std::process::Command::new("ffmpeg");
            cmd.args(["-v", "error", "-y", "-i"]).arg(&wav);
            if name == "c.ogg" {
                // ffmpeg schreibt bei OGG kein Bild als Strom; Vorbis-Kommentar direkt.
                let block = flac_picture_block(PNG);
                let b64 = base64_encode(&block);
                cmd.args(["-c:a", codec, "-metadata"])
                    .arg(format!("METADATA_BLOCK_PICTURE={b64}"));
            } else {
                cmd.arg("-i").arg(&png).args([
                    "-map",
                    "0:a",
                    "-map",
                    "1",
                    "-c:a",
                    codec,
                    "-c:v",
                    "copy",
                    "-disposition:v",
                    "attached_pic",
                ]);
            }
            assert!(
                cmd.arg(&out).status().is_ok_and(|s| s.success()),
                "ffmpeg {name}"
            );
            let c = read_cover(&out).unwrap_or_else(|| panic!("{name}: kein Cover"));
            assert_eq!(c.data, PNG, "{name}");
        }
    }

    /// FLAC-PICTURE-Block (ohne Blockkopf), wie ihn Vorbis-Kommentare base64 tragen.
    fn flac_picture_block(pic: &[u8]) -> Vec<u8> {
        let mime = b"image/png";
        let mut b = Vec::new();
        b.extend_from_slice(&3u32.to_be_bytes());
        b.extend_from_slice(&(mime.len() as u32).to_be_bytes());
        b.extend_from_slice(mime);
        b.extend_from_slice(&0u32.to_be_bytes());
        for v in [1u32, 1, 24, 0] {
            b.extend_from_slice(&v.to_be_bytes());
        }
        b.extend_from_slice(&(pic.len() as u32).to_be_bytes());
        b.extend_from_slice(pic);
        b
    }

    fn base64_encode(x: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::new();
        for c in x.chunks(3) {
            let n = (u32::from(c[0]) << 16)
                | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
                | u32::from(*c.get(2).unwrap_or(&0));
            for i in 0..4 {
                if i <= c.len() {
                    s.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    s.push('=');
                }
            }
        }
        s
    }
}
