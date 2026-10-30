//! DAWproject 1.0 (Paket 27): Austauschformat für Projekte zwischen DAWs (Bitwig, Studio One,
//! Cubase u. a.). Eine `.dawproject`-Datei ist ein ZIP mit `project.xml`, `metadata.xml` und
//! den Audiodateien. Dieses Modul schreibt ein WOA-Projekt in dieses Format
//! ([`export::export_file`]) und liest es zurück ([`import::import_to_folder`]).
//!
//! Was übertragen wird: Spuren (Audio, MIDI, Bus), Clips mit Position, Länge, Versatz,
//! Blenden, Dehnung, MIDI-Noten und Sustain-Controller, Tempo, Lautstärke, Pan, Mute, Solo,
//! Ausgänge, Sends, eigene Effekte (als `Device` mit der Kennung `woa:<art>`) und Automation
//! von Lautstärke, Pan, Sends und Effekt-Reglern. Was das Format nicht kennt (Tonhöhe, Clip-
//! Verstärkung, Blendenform, „Tempo folgen“) steht als kurze Notiz im Kommentar des Clips
//! (`woa: …`), damit es bei der Rückkehr nach WOA erhalten bleibt. Alles, was WOA nicht
//! kennt (fremde Plugins, Szenen, Video, Marker, Tempo-Automation), wird nicht übernommen,
//! aber im [`Report`] genannt und geloggt.
//!
//! Nichts hiervon läuft im Audio-Thread; es sind Dateivorgänge für Hintergrund oder Dialog.

pub mod export;
pub mod import;
pub mod xml;

use std::fmt;

use woa_model::EffectKind;

/// Name der Projektdatei im ZIP.
pub const PROJECT_ENTRY: &str = "project.xml";
/// Name der Metadaten im ZIP.
pub const METADATA_ENTRY: &str = "metadata.xml";
/// Ordner der Audiodateien im ZIP.
pub const AUDIO_PREFIX: &str = "audio/";
/// Versionsangabe, die wir schreiben.
pub const FORMAT_VERSION: &str = "1.0";
/// Vorsilbe der Geräte-Kennung eigener Effekte.
pub const DEVICE_PREFIX: &str = "woa:";
/// Vorsilbe der Clip-Notiz.
pub const CLIP_NOTE_PREFIX: &str = "woa:";

/// Fehler beim Schreiben oder Lesen einer DAWproject-Datei. Der Text enthält nie einen Pfad
/// aus der Datei selbst.
#[derive(Debug)]
pub enum DawError {
    /// XML nicht lesbar oder zu groß/tief.
    Xml(String),
    /// ZIP nicht lesbar oder verdächtig (zu viele Einträge, zu groß).
    Zip(String),
    /// Dateizugriff.
    Io(String),
    /// Inhalt passt nicht zu DAWproject 1.0 oder zu WOA.
    Invalid(String),
    /// Das gebaute Projekt besteht die Prüfung des Modells nicht.
    Model(String),
}

impl fmt::Display for DawError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DawError::Xml(m) => write!(f, "XML: {m}"),
            DawError::Zip(m) => write!(f, "ZIP: {m}"),
            DawError::Io(m) => write!(f, "Datei: {m}"),
            DawError::Invalid(m) => write!(f, "keine gültige DAWproject-Datei: {m}"),
            DawError::Model(m) => write!(f, "Projekt unzulässig: {m}"),
        }
    }
}

impl std::error::Error for DawError {}

impl From<std::io::Error> for DawError {
    fn from(e: std::io::Error) -> Self {
        DawError::Io(e.to_string())
    }
}

impl From<zip::result::ZipError> for DawError {
    fn from(e: zip::result::ZipError) -> Self {
        DawError::Zip(e.to_string())
    }
}

/// Was beim Schreiben oder Lesen nicht übertragen wurde, und ein paar Zahlen.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    /// Spuren im Projekt.
    pub tracks: usize,
    /// Clips im Projekt.
    pub clips: usize,
    /// Audiodateien, die ins ZIP kamen oder herausgelöst wurden.
    pub audio_files: usize,
    /// Alles, was nicht übernommen wurde (Klartext für Nutzer und Log).
    pub notes: Vec<String>,
}

/// Höchste Zahl Einzelmeldungen; danach zählt nur noch `omitted`.
const MAX_NOTES: usize = 200;

impl Report {
    /// Merkt eine nicht übertragene Sache und loggt sie.
    pub fn note(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        tracing::warn!(note = %msg, "dawproject: not transferred");
        if self.notes.len() < MAX_NOTES {
            self.notes.push(msg);
        } else if self.notes.len() == MAX_NOTES {
            self.notes.push("… weitere Hinweise ausgelassen".into());
        }
    }

    /// Eine Zeile für die Statusleiste.
    pub fn summary(&self) -> String {
        let n = self.notes.len();
        if n == 0 {
            format!(
                "{} Spuren, {} Clips, {} Audiodateien",
                self.tracks, self.clips, self.audio_files
            )
        } else {
            format!(
                "{} Spuren, {} Clips, {} Audiodateien; {n} Hinweise nicht übertragen (siehe Log)",
                self.tracks, self.clips, self.audio_files
            )
        }
    }
}

/// Kennwerte einer Audiodatei, die das Format verlangt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioInfo {
    /// Abtastrate in Hz.
    pub sample_rate: u32,
    /// Kanäle.
    pub channels: u16,
    /// Länge in Sekunden.
    pub seconds: f64,
}

/// Liest Rate, Kanäle und Länge aus dem Kopf einer WAV-Datei (ohne zu dekodieren). `None` bei
/// anderen Formaten oder kaputtem Kopf; die Oberfläche fragt dann den Decoder.
pub fn probe_wav(path: &std::path::Path) -> Option<AudioInfo> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut head = [0u8; 12];
    f.read_exact(&mut head).ok()?;
    if &head[0..4] != b"RIFF" || &head[8..12] != b"WAVE" {
        return None;
    }
    let (mut rate, mut channels, mut block, mut data) = (0u32, 0u16, 0u16, None::<u64>);
    let mut hdr = [0u8; 8];
    // Höchstens 64 Blöcke lesen; ein WAV-Kopf hat selten mehr als eine Handvoll.
    for _ in 0..64 {
        if f.read_exact(&mut hdr).is_err() {
            break;
        }
        let size = u64::from(u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]));
        match &hdr[0..4] {
            b"fmt " if size >= 16 => {
                let mut fmt = [0u8; 16];
                f.read_exact(&mut fmt).ok()?;
                channels = u16::from_le_bytes([fmt[2], fmt[3]]);
                rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
                block = u16::from_le_bytes([fmt[12], fmt[13]]);
                skip(&mut f, size - 16 + (size & 1))?;
            }
            b"data" => {
                data = Some(size);
                break;
            }
            _ => skip(&mut f, size + (size & 1))?,
        }
    }
    let data = data?;
    if rate == 0 || channels == 0 || block == 0 {
        return None;
    }
    #[allow(clippy::cast_precision_loss)]
    let seconds = (data / u64::from(block)) as f64 / f64::from(rate);
    Some(AudioInfo {
        sample_rate: rate,
        channels,
        seconds,
    })
}

fn skip(f: &mut std::fs::File, n: u64) -> Option<()> {
    use std::io::{Seek, SeekFrom};
    let n = i64::try_from(n).ok()?;
    f.seek(SeekFrom::Current(n)).ok().map(|_| ())
}

/// Stabile Kennung einer Effektart in der Datei.
pub fn effect_id(kind: EffectKind) -> &'static str {
    match kind {
        EffectKind::Eq => "eq",
        EffectKind::Compressor => "compressor",
        EffectKind::Limiter => "limiter",
        EffectKind::Gate => "gate",
        EffectKind::DeEsser => "de_esser",
        EffectKind::Reverb => "reverb",
        EffectKind::Delay => "delay",
        EffectKind::Chorus => "chorus",
        EffectKind::Saturation => "saturation",
        EffectKind::Denoise => "denoise",
    }
}

/// Effektart zu einer Kennung.
pub fn effect_from_id(id: &str) -> Option<EffectKind> {
    EffectKind::ALL.into_iter().find(|k| effect_id(*k) == id)
}

/// Linearer Faktor zu dB; unterhalb der Untergrenze Stille.
pub fn db_to_linear(db: f64) -> f64 {
    if db <= woa_model::MIN_GAIN_DB {
        0.0
    } else {
        10f64.powf(db / 20.0)
    }
}

/// dB zu linearem Faktor; 0 oder weniger ergibt die Untergrenze.
pub fn linear_to_db(lin: f64) -> f64 {
    if lin <= 0.0 {
        woa_model::MIN_GAIN_DB
    } else {
        20.0 * lin.log10()
    }
}

/// Zahl so schreiben, dass sie unverändert zurückgelesen wird.
pub(crate) fn num(v: f64) -> String {
    if v == 0.0 {
        "0".to_owned() // auch −0
    } else {
        format!("{v}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_ids_round_trip() {
        for k in EffectKind::ALL {
            assert_eq!(effect_from_id(effect_id(k)), Some(k));
        }
        assert_eq!(effect_from_id("nope"), None);
    }

    #[test]
    fn db_linear_inverse() {
        for db in [-60.0, -6.0, 0.0, 3.5, 12.0] {
            assert!((linear_to_db(db_to_linear(db)) - db).abs() < 1e-9);
        }
        assert_eq!(db_to_linear(-96.0), 0.0);
        assert_eq!(linear_to_db(0.0), -96.0);
    }

    #[test]
    fn report_caps_notes() {
        let mut r = Report::default();
        for i in 0..500 {
            r.note(format!("n{i}"));
        }
        assert_eq!(r.notes.len(), MAX_NOTES + 1);
        assert!(r.summary().contains("Hinweise"));
    }
}
