//! Titelinfos aus dem Datei-Kopf (Tags und Länge), ohne die Audiodaten zu dekodieren.
//!
//! Der Ordner-Import liest nur das hier; dekodiert wird erst in der Hintergrundanalyse. Ein
//! 5-Minuten-MP3 braucht so Millisekunden statt einer halben Sekunde (Telemetrie 0.34.34).

use std::path::Path;

use lofty::config::ParseOptions;
use lofty::prelude::*;
use lofty::probe::Probe;

/// Was der Kopf einer Datei verrät. Leere Felder fehlen in der Datei.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Header {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// Sekunden laut Kopf; 0, wenn die Datei keine Länge verrät (die Analyse trägt sie nach).
    pub duration: f64,
}

fn clean(s: Option<std::borrow::Cow<'_, str>>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// Liest Tags und Länge aus dem Kopf. Bilder werden übersprungen (schneller, weniger Speicher).
pub fn read_header(path: &Path) -> Result<Header, String> {
    let tagged = Probe::open(path)
        .map_err(|e| e.to_string())?
        .options(ParseOptions::new().read_cover_art(false))
        .read()
        .map_err(|e| e.to_string())?;
    let duration = tagged.properties().duration().as_secs_f64();
    let mut h = Header {
        duration,
        ..Header::default()
    };
    if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
        h.title = clean(tag.title());
        h.artist = clean(tag.artist());
        h.album = clean(tag.album());
    }
    Ok(h)
}
