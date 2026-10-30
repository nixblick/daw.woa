//! Auto-Updater: signierte MSI-Updates von GitHub Releases.
//!
//! Ablauf und Bedrohungsmodell stehen in `docs/updater.md`. Zwei Stufen, nie gemischt:
//!
//! - **Stufe A** (`packaging/windows/updater.pub` ist ein Platzhalter): nur HTTPS auf feste
//!   GitHub-Hosts, SHA-256 aus `latest.json` gegen das MSI.
//! - **Stufe B** (echter minisign-Schlüssel eingebrannt): Signatur ist Pflicht, fail-closed,
//!   kein Rückfall auf A.
//!
//! In beiden gilt der Downgrade-Wächter (nur strikt neuere Version). Prüfung und Wächter sind
//! reine Funktionen ([`verify_download`], [`version::ensure_newer`], [`manifest::evaluate`])
//! und ohne Netz testbar.

mod error;
mod hosts;
pub mod manifest;
mod net;
mod verify;
pub mod version;

pub use error::{UpdateError, Veto};
pub use manifest::Available;
#[cfg(windows)]
pub use net::launch_installer;
pub use net::{MANIFEST_URL, RELEASES_PAGE, check, download_and_verify};
pub use verify::{
    PUBLIC_KEY, configured, is_sha256_hex, key_configured, sha256_hex, verify_download,
    verify_hash, verify_package,
};
