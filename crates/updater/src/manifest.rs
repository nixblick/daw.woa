//! `latest.json`: was die Release-Pipeline neben das MSI legt.

use serde::Deserialize;

use crate::error::{UpdateError, Veto};
use crate::hosts::Policy;
use crate::version;

/// Längere Notizen werden gekürzt (Anzeige, kein Schutzziel).
const MAX_NOTES_CHARS: usize = 4000;

/// Ein verfügbares Update, wie es das Manifest beschreibt. Noch **nicht** vertrauenswürdig:
/// erst [`crate::verify_download`] macht aus den Bytes etwas Installierbares.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Available {
    pub version: String,
    #[serde(default)]
    pub notes: String,
    pub msi_url: String,
    /// Adresse der minisign-Signatur (Stufe B). Fehlt in Manifesten ohne Signatur.
    #[serde(default)]
    pub msi_sig_url: String,
    /// SHA-256 des MSI als Hex. Prüfwert in Stufe A; mit Signatur (Stufe B) nur Zusatz.
    #[serde(default)]
    pub msi_sha256: String,
    /// AppImage (Linux). Fehlt in Manifesten älterer Releases.
    #[serde(default)]
    pub appimage_url: String,
    #[serde(default)]
    pub appimage_sig_url: String,
    #[serde(default)]
    pub appimage_sha256: String,
}

impl Available {
    /// Adressen und Prüfwert des Pakets für diese Paketart; `None`, wenn das Manifest keines
    /// dafür kennt oder die Art sich nicht selbst ersetzen kann (Flatpak, Quellbau).
    pub fn package(&self, kind: crate::Kind) -> Option<Package<'_>> {
        match kind {
            crate::Kind::Msi => Some(Package {
                url: &self.msi_url,
                sig_url: &self.msi_sig_url,
                sha256: &self.msi_sha256,
            }),
            crate::Kind::AppImage if !self.appimage_url.is_empty() => Some(Package {
                url: &self.appimage_url,
                sig_url: &self.appimage_sig_url,
                sha256: &self.appimage_sha256,
            }),
            _ => None,
        }
    }
}

/// Ein Paket aus dem Manifest, geliehen.
#[derive(Debug, Clone, Copy)]
pub struct Package<'a> {
    pub url: &'a str,
    pub sig_url: &'a str,
    pub sha256: &'a str,
}

/// Liest das Manifest und prüft Form und Version gegen die eingebaute.
///
/// - `Ok(Some(_))`: strikt neuer, Adressen liegen auf den erlaubten GitHub-Pfaden.
/// - `Ok(None)`: gleiche Version, nichts zu tun.
/// - `Err`: unlesbar oder älter (Downgrade-Veto).
pub fn evaluate(json: &[u8], current: &str) -> Result<Option<Available>, UpdateError> {
    evaluate_with(json, current, Policy::Github)
}

pub(crate) fn evaluate_with(
    json: &[u8],
    current: &str,
    policy: Policy,
) -> Result<Option<Available>, UpdateError> {
    let mut m: Available =
        serde_json::from_slice(json).map_err(|e| UpdateError::Manifest(e.to_string()))?;
    match version::ensure_newer(&m.version, current) {
        Ok(()) => {}
        Err(Veto::SameVersion) => return Ok(None),
        Err(v) => return Err(v.into()),
    }
    let urls_ok = policy.start_allowed(&m.msi_url)
        && [&m.msi_sig_url, &m.appimage_url, &m.appimage_sig_url]
            .iter()
            .all(|u| u.is_empty() || policy.start_allowed(u));
    if !urls_ok {
        return Err(UpdateError::Manifest("Adresse nicht erlaubt".into()));
    }
    for h in [&mut m.msi_sha256, &mut m.appimage_sha256] {
        *h = h.trim().to_ascii_lowercase();
        if !h.is_empty() && !crate::verify::is_sha256_hex(h) {
            return Err(UpdateError::Manifest("sha256 ungültig".into()));
        }
    }
    if m.notes.chars().count() > MAX_NOTES_CHARS {
        m.notes = m.notes.chars().take(MAX_NOTES_CHARS).collect::<String>() + " …";
    }
    Ok(Some(m))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://github.com/nixblick/daw.woa/releases/download/v9/";
    const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn json(version: &str) -> Vec<u8> {
        format!(
            r#"{{"version":"{version}","notes":"Neu: Dinge","msi_url":"{BASE}a.msi","msi_sig_url":"{BASE}a.msi.minisig","msi_sha256":"{HASH}"}}"#
        )
        .into_bytes()
    }

    #[test]
    fn parses_a_newer_manifest() {
        let a = evaluate(&json("0.34.0"), "0.33.4").unwrap().unwrap();
        assert_eq!(a.version, "0.34.0");
        assert_eq!(a.notes, "Neu: Dinge");
        assert_eq!(a.msi_url, format!("{BASE}a.msi"));
        assert_eq!(a.msi_sig_url, format!("{BASE}a.msi.minisig"));
        assert_eq!(a.msi_sha256, HASH);
    }

    #[test]
    fn same_version_is_no_update() {
        assert_eq!(evaluate(&json("0.33.4"), "0.33.4"), Ok(None));
    }

    #[test]
    fn older_and_unreadable_versions_are_vetoed() {
        assert_eq!(
            evaluate(&json("0.33.3"), "0.33.4"),
            Err(UpdateError::Veto(Veto::Downgrade))
        );
        assert_eq!(
            evaluate(&json("neu"), "0.33.4"),
            Err(UpdateError::Veto(Veto::UnreadableVersion))
        );
        assert_eq!(
            evaluate(&json("0.34.0"), "kaputt"),
            Err(UpdateError::Veto(Veto::UnreadableVersion))
        );
    }

    #[test]
    fn rejects_garbage_and_missing_fields() {
        assert!(matches!(
            evaluate(b"<html>", "0.1.0"),
            Err(UpdateError::Manifest(_))
        ));
        assert!(matches!(
            evaluate(br#"{"version":"1.0.0"}"#, "0.1.0"),
            Err(UpdateError::Manifest(_))
        ));
    }

    #[test]
    fn rejects_foreign_hosts_http_and_odd_urls() {
        for url in [
            "http://github.com/nixblick/daw.woa/releases/a.msi",
            "https://example.org/a.msi",
            "https://github.com/other/repo/releases/a.msi",
            "file:///etc/passwd",
        ] {
            let j = format!(r#"{{"version":"1.0.0","msi_url":"{url}"}}"#);
            assert!(
                matches!(
                    evaluate(j.as_bytes(), "0.1.0"),
                    Err(UpdateError::Manifest(_))
                ),
                "{url}"
            );
        }
        let j = format!(
            r#"{{"version":"1.0.0","msi_url":"{BASE}a.msi","msi_sig_url":"https://example.org/s"}}"#
        );
        assert!(matches!(
            evaluate(j.as_bytes(), "0.1.0"),
            Err(UpdateError::Manifest(_))
        ));
    }

    #[test]
    fn sha256_is_normalized_and_checked_for_shape() {
        let up = HASH.to_ascii_uppercase();
        let j = format!(r#"{{"version":"1.0.0","msi_url":"{BASE}a.msi","msi_sha256":" {up} "}}"#);
        assert_eq!(
            evaluate(j.as_bytes(), "0.1.0").unwrap().unwrap().msi_sha256,
            HASH
        );
        let j = format!(r#"{{"version":"1.0.0","msi_url":"{BASE}a.msi","msi_sha256":"abc"}}"#);
        assert!(matches!(
            evaluate(j.as_bytes(), "0.1.0"),
            Err(UpdateError::Manifest(_))
        ));
    }

    #[test]
    fn sig_and_hash_are_optional_here_and_notes_are_capped() {
        let none = format!(r#"{{"version":"1.0.0","msi_url":"{BASE}a.msi"}}"#);
        let a = evaluate(none.as_bytes(), "0.1.0").unwrap().unwrap();
        assert_eq!(
            (
                a.notes.as_str(),
                a.msi_sig_url.as_str(),
                a.msi_sha256.as_str()
            ),
            ("", "", "")
        );
        let long = "x".repeat(10_000);
        let j = format!(r#"{{"version":"1.0.0","notes":"{long}","msi_url":"{BASE}a.msi"}}"#);
        let n = evaluate(j.as_bytes(), "0.1.0").unwrap().unwrap().notes;
        assert!(n.chars().count() < 4100);
    }
}
