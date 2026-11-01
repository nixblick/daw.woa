//! Auto-Updater: signierte MSI-Updates von GitHub Releases.
//!
//! Ablauf und Bedrohungsmodell stehen in `docs/updater.md`. Die minisign-Signatur ist Pflicht,
//! fail-closed: ohne gültigen eingebauten Schlüssel (`packaging/windows/updater.pub`) und ohne
//! passende Signatur wird nichts installiert, eine Prüfsumme allein genügt nie. Dazu der
//! Downgrade-Wächter (nur strikt neuere Version). Prüfung und Wächter sind reine Funktionen
//! ([`verify_download`], [`version::ensure_newer`], [`manifest::evaluate`]) und ohne Netz testbar.

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
    verify_package,
};
