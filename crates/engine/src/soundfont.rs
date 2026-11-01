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
//! im Control-Thread, erst wenn eine Spur einen GM-Klang hat. Zusatzbänke (Paket J7) wählt
//! [`soundfont_for`]; fehlt die Bank, gilt diese Standard-Bank.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use woa_dsp::soundfont::{SoundFont, load_soundfont};
use woa_model::{SoundBank, TrackSound};

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

/// Die SoundFont für einen Klang (Paket J7): seine Bank, wenn sie installiert ist und zum
/// Klang passt (eine reine Drum-Bank nur für Drums), sonst GeneralUser GS. Eine geladene
/// Bank bleibt im Speicher; eine fehlende wird nicht gemerkt, damit ein späterer Download zählt.
/// Control-Thread (macht I/O).
pub fn soundfont_for(sound: TrackSound) -> Option<Arc<SoundFont>> {
    sound
        .bank
        .filter(|b| sound.drums || !b.drums_only())
        .and_then(bank_font)
        .or_else(default_soundfont)
}

pub(crate) fn bank_font(bank: SoundBank) -> Option<Arc<SoundFont>> {
    static BANKS: OnceLock<Mutex<HashMap<SoundBank, Arc<SoundFont>>>> = OnceLock::new();
    let cache = BANKS.get_or_init(Mutex::default);
    if let Some(f) = cache.lock().ok().and_then(|c| c.get(&bank).cloned()) {
        return Some(f);
    }
    let path = crate::banks::installed(bank)?;
    let file = std::fs::File::open(&path).ok()?;
    match load_soundfont(&mut std::io::BufReader::new(file)) {
        Ok(font) => {
            tracing::info!(bank = bank.id(), path = %path.display(), "sound bank loaded");
            if let Ok(mut c) = cache.lock() {
                c.insert(bank, Arc::clone(&font));
            }
            Some(font)
        }
        Err(e) => {
            tracing::warn!(bank = bank.id(), error = %e, "sound bank unreadable, GeneralUser GS plays");
            None
        }
    }
}
