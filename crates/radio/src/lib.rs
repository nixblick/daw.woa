//! # woa-radio: Sendeablauf
//!
//! Playlist mit Crossfade und Auto-Gain, Cart-Wall, Mikrofon mit Ducking, Mitschnitt,
//! Sendebus- und Mikro-Processing ([`dynamics`]).
//!
//! Aufteilung nach den Echtzeitregeln (CLAUDE.md):
//! - [`mixer::Mixer`] lebt im Audio-Thread. Er mischt Musik (zwei Decks für den
//!   Crossfade), Carts und Mikro; keine Allokation, kein Lock, kein I/O. Fertig dekodierte
//!   Puffer bekommt er als `basedrop::Shared` über eine `rtrb`-Queue, das Mikro über einen
//!   eigenen `rtrb`-Ring, den Mitschnitt gibt er über einen `rtrb`-Ring zurück.
//! - [`control::Controller`] lebt im Control-Thread: dekodiert, misst, reiht den nächsten
//!   Titel ein, schreibt den Mitschnitt und gibt alte Puffer frei.
//! - [`station::Station`] lässt den Controller in einem eigenen Thread laufen, damit die
//!   Sendung weiterläuft, auch wenn die Oberfläche gerade nicht zeichnet.
//! - [`party`]: Party-Layout ohne Fenster, Wunschliste und Tastenbefehle (Paket 35).
//! - [`voicetrack`]: Moderation zwischen zwei Titeln mit Ducking (Paket 30).
//! - [`show::RadioShow`] ist die gespeicherte Playlist und Cart-Belegung (`radio.json`).

use std::path::PathBuf;

pub mod audio;
pub mod control;
pub mod cues;
pub mod dynamics;
pub mod events;
pub mod level;
pub mod mixer;
pub mod party;
pub mod pfl;
pub mod playlists;
pub mod routing;
pub mod show;
pub mod station;
pub mod transition;
pub mod voicetrack;

pub use control::{Controller, render};
pub use cues::Cues;
pub use mixer::Mixer;
pub use show::{CartEntry, PlaylistEntry, RadioShow};
pub use woa_engine::pairs::PairBuf;

/// Felder der Cart-Wall (4×4).
pub const CARTS: usize = 16;
/// Zahl der Decks (A, B, C).
pub const DECKS: usize = 3;
/// Name der Datei im Datenordner.
pub const RADIO_FILE: &str = "radio.json";
/// Unterordner im Datenordner für Mitschnitte.
pub const RECORDINGS_DIR: &str = "mitschnitte";

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// `<basis>/woa`: unter Windows `%APPDATA%`, sonst `$xdg` oder `~/<fallback>`.
pub fn base_dir(xdg: &str, fallback: &str) -> PathBuf {
    if let Some(p) = std::env::var_os("APPDATA") {
        return PathBuf::from(p).join("woa");
    }
    std::env::var_os(xdg)
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(fallback))
        .join("woa")
}

/// Datenordner von WOA (Logs, Wiederherstellung, radio.json, Mitschnitte).
pub fn data_dir() -> PathBuf {
    base_dir("XDG_DATA_HOME", ".local/share")
}

/// Konfigurationsordner von WOA (`config.json`).
pub fn config_dir() -> PathBuf {
    base_dir("XDG_CONFIG_HOME", ".config")
}

/// Vorgabe für `radio.json`: im Datenordner. GUI und Agent nutzen dieselbe Datei.
pub fn default_radio_path() -> PathBuf {
    data_dir().join(RADIO_FILE)
}

/// Datum und Uhrzeit (UTC) aus Sekunden seit 1970, für Dateinamen:
/// `2026-10-29T153012Z`.
pub fn utc_stamp(unix_seconds: u64) -> String {
    let days = (unix_seconds / 86_400) as i64;
    let secs = unix_seconds % 86_400;
    // Howard Hinnant, civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}{:02}{:02}Z",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

/// Jetzt als [`utc_stamp`].
pub fn utc_now_stamp() -> String {
    let s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    utc_stamp(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_is_a_calendar_date() {
        assert_eq!(utc_stamp(0), "1970-01-01T000000Z");
        // 2026-10-29 15:30:12 UTC
        assert_eq!(utc_stamp(1_793_287_812), "2026-10-29T153012Z");
        assert_eq!(utc_stamp(951_782_400), "2000-02-29T000000Z");
    }
}
