//! Wo die App die General-MIDI-SoundFont findet (Paket J1, Issue #261).
//!
//! Die Bank (GeneralUser GS, `docs/sounds/README.md`) liegt nicht im Repo. Gesucht wird
//! in dieser Reihenfolge, die erste lesbare Datei gilt:
//!
//! 1. Umgebungsvariable `WOA_SOUNDFONT` (voller Pfad zu einer `.sf2`),
//! 2. `sounds/GeneralUser-GS.sf2` neben der ausführbaren Datei (Windows, AppImage),
//! 3. `../share/woa/sounds/GeneralUser-GS.sf2` relativ zur ausführbaren Datei (Linux-Pakete).
//!
//! Das Paket legt die Datei an Ort 2 bzw. 3 ab. Wird keine gefunden, spielen MIDI-Spuren
//! mit gewähltem GM-Klang weiter den eingebauten Synth. Geladen wird einmal je Prozess,
//! im Control-Thread, erst wenn eine Spur einen GM-Klang hat.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use woa_dsp::soundfont::{SoundFont, load_soundfont};

/// Dateiname der Standard-Bank.
pub const SOUNDFONT_FILE: &str = "GeneralUser-GS.sf2";
/// Umgebungsvariable für eine eigene Bank.
pub const SOUNDFONT_ENV: &str = "WOA_SOUNDFONT";

/// Suchorte in Prüfreihenfolge.
pub fn soundfont_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(p) = std::env::var_os(SOUNDFONT_ENV).filter(|p| !p.is_empty()) {
        out.push(PathBuf::from(p));
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(PathBuf::from))
    {
        out.push(dir.join("sounds").join(SOUNDFONT_FILE));
        out.push(dir.join("../share/woa/sounds").join(SOUNDFONT_FILE));
    }
    out
}

fn load_first() -> Option<Arc<SoundFont>> {
    for path in soundfont_candidates() {
        let Ok(file) = std::fs::File::open(&path) else {
            continue;
        };
        match load_soundfont(&mut std::io::BufReader::new(file)) {
            Ok(font) => {
                tracing::info!(path = %path.display(), "soundfont loaded");
                return Some(font);
            }
            Err(e) => tracing::warn!(path = %path.display(), error = %e, "soundfont unreadable"),
        }
    }
    tracing::info!("no soundfont found, MIDI tracks use the built-in synth");
    None
}

static FONT: OnceLock<Option<Arc<SoundFont>>> = OnceLock::new();

/// Die SoundFont des Prozesses, beim ersten Aufruf geladen; `None`: keine gefunden.
pub fn default_soundfont() -> Option<Arc<SoundFont>> {
    FONT.get_or_init(load_first).clone()
}
