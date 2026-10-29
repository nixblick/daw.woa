//! # woa-stream: Sendeprozess
//!
//! - [`config::StreamConfig`]: Server, Stream-ID, Bitrate, Notfall-Playlist; das Passwort
//!   steht nie darin, nur der Pfad zur Datei ([`kennwort::Kennwort`]).
//! - [`mp3::Mp3Encoder`]: MP3 CBR über LAME.
//! - [`shoutcast`]: Quellclient für Shoutcast DNAS v2 im v1-kompatiblen Legacy-Modus
//!   (Port+1, Passwort `pass:#sid`), Titel per `admin.cgi?mode=updinfo`.
//! - [`uplink::Uplink`]: eigener Thread, der MP3 an den Server schickt, mit Wiederverbinden
//!   und begrenztem Puffer.
//! - [`link`]: lokale Verbindung Oberfläche → `woa-streamd` (TCP 127.0.0.1, Token-Datei)
//!   und der [`link::Feeder`] auf Seite der Oberfläche.
//! - [`daemon`]: der Kern von `woa-streamd` (Sendemix oder Notfall-Playlist, Takt, Status).
//! - [`status::StreamStatus`]: Zustand als JSON im Datenordner, für Oberfläche und Agent.

pub mod config;
pub mod daemon;
pub mod kennwort;
pub mod link;
pub mod mp3;
pub mod shoutcast;
pub mod status;
pub mod uplink;

pub use config::{Protocol, StreamConfig};
pub use kennwort::Kennwort;
pub use status::StreamStatus;

/// Zufällige Hex-Zeichenkette (ohne neue Abhängigkeit: `RandomState` ist pro Aufruf
/// zufällig geseedet). Für das lokale Token, kein kryptografisches Material nach außen.
pub fn random_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut s = String::with_capacity(64);
    for i in 0..4u64 {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(i);
        h.write_u32(std::process::id());
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        );
        s.push_str(&format!("{:016x}", h.finish()));
    }
    s
}

/// Sekunden seit 1970.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
