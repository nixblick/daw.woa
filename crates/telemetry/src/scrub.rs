//! Scrubber: was zum Server geht, ist ein Bezeichner oder eine Zahl, sonst nichts.
//!
//! Der Server (`/ingest`) verlangt für `modul` und `art` `^[a-z0-9_.-]+$` mit 60 bzw. 80
//! Zeichen. Wir prüfen dasselbe hier, damit kein Freitext und kein Pfad die Maschine
//! verlässt. Pfade scheitern schon am `/` bzw. `\`; Großbuchstaben, Leerzeichen und
//! Umlaute ebenso. Zusätzlich verwerfen wir Bezeichner, die wie Pfade oder Dateinamen
//! mit Audio-/Projekt-Endung aussehen.

/// Maximale Länge von `modul` (Server: 60).
pub const MAX_MODUL: usize = 60;
/// Maximale Länge von `art` (Server: 80).
pub const MAX_ART: usize = 80;
/// Maximale Länge von `stufe` (Server: 20).
pub const MAX_STUFE: usize = 20;

const FILE_SUFFIXES: &[&str] = &[
    ".wav", ".mp3", ".flac", ".ogg", ".opus", ".aac", ".m4a", ".mid", ".midi", ".woa", ".json",
    ".txt", ".png", ".jpg",
];

/// Ist `s` ein zulässiger Bezeichner (a-z0-9_.-, nicht leer, höchstens `max` Zeichen,
/// kein `..`, keine Dateiendung)?
pub fn is_identifier(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-'))
        && !s.contains("..")
        && !s.starts_with('.')
        && !FILE_SUFFIXES.iter().any(|x| s.ends_with(x))
}

/// Macht aus einem tracing-Target (`woa_engine::graph`) ein Modul (`woa_engine.graph`).
/// Liefert `None`, wenn danach kein gültiger Bezeichner übrig bleibt.
pub fn module_from_target(target: &str) -> Option<String> {
    let m: String = target
        .replace("::", ".")
        .chars()
        .map(|c| c.to_ascii_lowercase())
        .collect();
    is_identifier(&m, MAX_MODUL).then_some(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_codes() {
        assert!(is_identifier("perf.cold.ms", MAX_ART));
        assert!(is_identifier("xrun", MAX_ART));
        assert!(is_identifier("stream.reconnect_2", MAX_ART));
    }

    #[test]
    fn rejects_text_and_paths() {
        for bad in [
            "",
            "Hallo Welt",
            "/home/user/song.wav",
            "C:\\Users\\x",
            "..",
            "a..b",
            ".hidden",
            "sendung.wav",
            "projekt.woa",
            "über",
            "UPPER",
            "mail@example.org",
        ] {
            assert!(!is_identifier(bad, MAX_ART), "{bad:?} must be rejected");
        }
        assert!(!is_identifier(&"a".repeat(81), MAX_ART));
    }

    #[test]
    fn target_to_module() {
        assert_eq!(
            module_from_target("woa_engine::graph").as_deref(),
            Some("woa_engine.graph")
        );
        assert_eq!(module_from_target("Woa").as_deref(), Some("woa"));
        assert_eq!(module_from_target("a b"), None);
    }
}
