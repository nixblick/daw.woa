//! Externes `ffmpeg` als Prozess (Paket 26): Opus, Ogg Vorbis und AAC schreiben, Opus lesen.
//!
//! Nichts wird gelinkt: WOA ruft ein vorhandenes Programm auf (Argumente als Liste, keine
//! Shell). Fehlt es oder kann es einen Codec nicht, bekommt der Nutzer einen klaren Text
//! und die Oberfläche graut das Format aus. Alles hier läuft im Control- oder
//! Hintergrund-Thread, nie im Audio-Thread.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::decode::Decoded;

/// Umgebungsvariable, die den Pfad zu `ffmpeg` vorgibt.
pub const ENV_VAR: &str = "WOA_FFMPEG";

/// Hinweis für den Nutzer, wenn `ffmpeg` fehlt.
pub const MISSING: &str = "ffmpeg nicht gefunden: Opus, Ogg und AAC brauchen das Programm ffmpeg (installieren oder in den PATH legen, oder WOA_FFMPEG auf den Pfad setzen)";

/// Frames, die in einem Stück an ffmpeg gehen.
const CHUNK_FRAMES: usize = 16_384;
/// Mehr Fehlertext von ffmpeg wird nicht aufgehoben.
const STDERR_LIMIT: u64 = 16 * 1024;
/// Obergrenze für gelesene Ausgabe (Schutz vor einer entarteten Datei): 6 h Stereo, 48 kHz.
const MAX_DECODE_BYTES: u64 = 6 * 3600 * 48_000 * 2 * 4;

/// Audiocodec, den ffmpeg schreibt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// Opus im Ogg-Container.
    Opus,
    /// Vorbis im Ogg-Container.
    Vorbis,
    /// AAC-LC im ADTS-Strom (`.aac`; der Container `.m4a` bräuchte ein suchbares Ziel).
    Aac,
}

impl Codec {
    fn encoder(self) -> &'static str {
        match self {
            Self::Opus => "libopus",
            Self::Vorbis => "libvorbis",
            Self::Aac => "aac",
        }
    }

    fn container(self) -> &'static str {
        match self {
            Self::Opus | Self::Vorbis => "ogg",
            Self::Aac => "adts",
        }
    }

    /// Name für Meldungen.
    pub fn label(self) -> &'static str {
        match self {
            Self::Opus => "Opus",
            Self::Vorbis => "Ogg Vorbis",
            Self::Aac => "AAC",
        }
    }
}

/// Welche Codecs diese ffmpeg-Fassung schreiben kann.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Encoders {
    pub opus: bool,
    pub vorbis: bool,
    pub aac: bool,
}

impl Encoders {
    /// Kann `codec` geschrieben werden?
    pub fn supports(self, codec: Codec) -> bool {
        match codec {
            Codec::Opus => self.opus,
            Codec::Vorbis => self.vorbis,
            Codec::Aac => self.aac,
        }
    }

    /// Liest die Ausgabe von `ffmpeg -encoders`: Audio-Zeilen `A….. name  Beschreibung`.
    pub fn parse(listing: &str) -> Self {
        let mut out = Self::default();
        for line in listing.lines() {
            let mut it = line.split_whitespace();
            let (Some(flags), Some(name)) = (it.next(), it.next()) else {
                continue;
            };
            if !flags.starts_with('A') || flags.len() < 6 {
                continue;
            }
            match name {
                "libopus" => out.opus = true,
                "libvorbis" => out.vorbis = true,
                "aac" => out.aac = true,
                _ => {}
            }
        }
        out
    }
}

/// Ein auffindbares `ffmpeg`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ffmpeg {
    path: PathBuf,
}

impl Ffmpeg {
    /// Nimmt genau diesen Pfad (Tests, Einstellungen).
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Sucht `ffmpeg`: `WOA_FFMPEG`, neben dem Programm, dann im `PATH`.
    pub fn locate() -> Option<Self> {
        if let Some(p) = std::env::var_os(ENV_VAR).map(PathBuf::from)
            && p.is_file()
        {
            return Some(Self::at(p));
        }
        let name = if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        };
        let beside = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|d| d.join(name)));
        let on_path = std::env::var_os("PATH")
            .into_iter()
            .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
            .map(|d| d.join(name));
        beside
            .into_iter()
            .chain(on_path)
            .find(|p| p.is_file())
            .map(Self::at)
    }

    /// Pfad des Programms.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn command(&self) -> Command {
        let mut c = Command::new(&self.path);
        c.arg("-hide_banner").arg("-loglevel").arg("error");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        c
    }

    /// Fragt die Encoder ab. Ein Prozessstart: einmal aufrufen und merken, nie je Frame.
    pub fn encoders(&self) -> Result<Encoders, String> {
        let out = self
            .command()
            .args(["-encoders"])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| self.spawn_error(&e))?;
        if !out.status.success() {
            return Err(format!("ffmpeg -encoders: {}", out.status));
        }
        Ok(Encoders::parse(&String::from_utf8_lossy(&out.stdout)))
    }

    fn spawn_error(&self, e: &std::io::Error) -> String {
        if e.kind() == std::io::ErrorKind::NotFound {
            MISSING.to_owned()
        } else {
            format!(
                "ffmpeg ({}) lässt sich nicht starten: {e}",
                self.path.display()
            )
        }
    }

    /// Kodiert Stereo (f32) und schreibt die fertige Datei in `out`. `kbps` ist die
    /// Zielbitrate (32 bis 512), `rate` die Rate von `left`/`right`.
    pub fn encode(
        &self,
        codec: Codec,
        kbps: u32,
        left: &[f32],
        right: &[f32],
        rate: u32,
        out: &mut dyn Write,
    ) -> Result<(), String> {
        if !(32..=512).contains(&kbps) {
            return Err(format!("Bitrate {kbps} kbit/s außerhalb von 32 bis 512"));
        }
        if left.len() != right.len() {
            return Err("links und rechts sind verschieden lang".into());
        }
        let mut cmd = self.command();
        cmd.arg("-y")
            .args(["-f", "f32le", "-ar"])
            .arg(rate.to_string())
            .args(["-ac", "2", "-i", "pipe:0", "-c:a", codec.encoder()])
            .arg("-b:a")
            .arg(format!("{kbps}k"));
        if codec == Codec::Opus {
            // Opus kennt nur 8/12/16/24/48 kHz: ffmpeg rechnet vorher um.
            cmd.args(["-ar", "48000"]);
        }
        cmd.args(["-f", codec.container(), "pipe:1"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| self.spawn_error(&e))?;
        let mut stdin = child.stdin.take().ok_or("kein Eingang zu ffmpeg")?;
        let mut stdout = child.stdout.take().ok_or("keine Ausgabe von ffmpeg")?;
        let stderr = child
            .stderr
            .take()
            .ok_or("keine Fehlerausgabe von ffmpeg")?;

        let (bytes, write_ok, err_text) = std::thread::scope(|s| {
            // Eingang und Ausgabe laufen getrennt, sonst verklemmt sich ein voller Pipe-Puffer.
            let feeder = s.spawn(move || {
                let mut buf = Vec::with_capacity(CHUNK_FRAMES * 8);
                for (l, r) in left.chunks(CHUNK_FRAMES).zip(right.chunks(CHUNK_FRAMES)) {
                    buf.clear();
                    for (a, b) in l.iter().zip(r) {
                        buf.extend_from_slice(&a.to_le_bytes());
                        buf.extend_from_slice(&b.to_le_bytes());
                    }
                    if stdin.write_all(&buf).is_err() {
                        return false; // ffmpeg hat aufgehört: sein Fehlertext sagt warum
                    }
                }
                true // stdin schließt beim Verlassen: Ende der Eingabe
            });
            let errs = s.spawn(move || read_limited(stderr));
            let mut bytes = Vec::new();
            let read = stdout.read_to_end(&mut bytes);
            let ok = feeder.join().unwrap_or(false);
            let text = errs.join().unwrap_or_default();
            (read.map(|_| bytes), ok, text)
        });
        let status = child.wait().map_err(|e| format!("ffmpeg: {e}"))?;
        let bytes = bytes.map_err(|e| format!("Ausgabe von ffmpeg: {e}"))?;
        if !status.success() || !write_ok || bytes.is_empty() {
            return Err(self.failure(codec, status, &err_text));
        }
        out.write_all(&bytes).map_err(|e| e.to_string())?;
        out.flush().map_err(|e| e.to_string())
    }

    fn failure(&self, codec: Codec, status: std::process::ExitStatus, err: &str) -> String {
        let first = err.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
        if first.contains("Unknown encoder")
            || first.contains("Encoder") && first.contains("not found")
        {
            return format!(
                "dieses ffmpeg kann {} nicht schreiben (Encoder {} fehlt)",
                codec.label(),
                codec.encoder()
            );
        }
        format!("ffmpeg ({}) ist gescheitert: {first}", status)
    }

    /// Liest eine Datei, die symphonia nicht kann (Opus), über ffmpeg als Stereo-f32 in der
    /// Rate `rate`.
    pub fn decode(&self, path: &Path, rate: u32) -> Result<Decoded, String> {
        let mut child = self
            .command()
            .arg("-i")
            .arg(path)
            .args(["-f", "f32le", "-ac", "2", "-ar"])
            .arg(rate.to_string())
            .arg("pipe:1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| self.spawn_error(&e))?;
        let stdout = child.stdout.take().ok_or("keine Ausgabe von ffmpeg")?;
        let stderr = child
            .stderr
            .take()
            .ok_or("keine Fehlerausgabe von ffmpeg")?;
        let (raw, err_text) = std::thread::scope(|s| {
            let errs = s.spawn(move || read_limited(stderr));
            let mut raw = Vec::new();
            let r = stdout.take(MAX_DECODE_BYTES).read_to_end(&mut raw);
            (r.map(|_| raw), errs.join().unwrap_or_default())
        });
        let status = child.wait().map_err(|e| format!("ffmpeg: {e}"))?;
        let raw = raw.map_err(|e| format!("Ausgabe von ffmpeg: {e}"))?;
        if !status.success() || raw.is_empty() {
            let first = err_text
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("");
            return Err(format!("ffmpeg konnte die Datei nicht lesen: {first}"));
        }
        let samples = raw
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        Ok(Decoded {
            samples,
            channels: 2,
            rate,
        })
    }
}

fn read_limited(r: impl Read) -> String {
    let mut s = String::new();
    let _ = r.take(STDERR_LIMIT).read_to_string(&mut s);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_encoder_listing() {
        let listing = " Encoders:\n V..... = Video\n A....D aac                  AAC (Advanced Audio Coding)\n A..X.D opus                 Opus\n A....D libopus              libopus Opus (codec opus)\n V....D libx264              H.264\n";
        let e = Encoders::parse(listing);
        assert!(e.aac && e.opus);
        assert!(!e.vorbis, "libvorbis steht nicht in der Liste");
        // Eine Videozeile mit dem Namen eines Audio-Encoders zählt nicht.
        assert_eq!(Encoders::parse(" V....D libopus  x\n"), Encoders::default());
    }

    #[test]
    fn missing_program_gives_clear_message() {
        let f = Ffmpeg::at("/nicht/vorhanden/ffmpeg-gibt-es-nicht");
        let mut sink = Vec::new();
        let err = f
            .encode(
                Codec::Opus,
                128,
                &[0.0; 100],
                &[0.0; 100],
                48_000,
                &mut sink,
            )
            .unwrap_err();
        assert_eq!(err, MISSING);
        assert!(sink.is_empty());
        assert_eq!(f.encoders().unwrap_err(), MISSING);
        assert_eq!(f.decode(Path::new("x.opus"), 48_000).unwrap_err(), MISSING);
    }

    #[test]
    fn rejects_bad_arguments_before_starting() {
        let f = Ffmpeg::at("/nicht/vorhanden/ffmpeg");
        let mut sink = Vec::new();
        assert!(
            f.encode(Codec::Aac, 8, &[0.0], &[0.0], 48_000, &mut sink)
                .unwrap_err()
                .contains("Bitrate")
        );
        assert!(
            f.encode(Codec::Aac, 128, &[0.0, 0.0], &[0.0], 48_000, &mut sink)
                .unwrap_err()
                .contains("verschieden lang")
        );
    }
}
