//! `latest.json`: was die Release-Pipeline neben das MSI legt.

use serde::Deserialize;

use crate::error::{UpdateError, Veto};
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
    /// Adresse der minisign-Signatur. Pflicht: ohne sie wird das Update abgelehnt.
    #[serde(default)]
    pub msi_sig_url: String,
    /// SHA-256 des MSI als Hex. Nur Zusatz zur Signatur.
    #[serde(default)]
    pub msi_sha256: String,
}

/// Liest das Manifest und prüft Form und Version gegen die eingebaute.
///
/// - `Ok(Some(_))`: strikt neuer, Adressen liegen auf den erlaubten GitHub-Pfaden.
/// - `Ok(None)`: gleiche Version, nichts zu tun.
/// - `Err`: unlesbar oder älter (Downgrade-Veto).
pub fn evaluate(json: &[u8], current: &str) -> Result<Option<Available>, UpdateError> {
    let mut m: Available =
        serde_json::from_slice(json).map_err(|e| UpdateError::Manifest(e.to_string()))?;
    match version::ensure_newer(&m.version, current) {
        Ok(()) => {}
        Err(Veto::SameVersion) => return Ok(None),
        Err(v) => return Err(v.into()),
    }
    if !crate::hosts::start_allowed(&m.msi_url)
        || (!m.msi_sig_url.is_empty() && !crate::hosts::start_allowed(&m.msi_sig_url))
    {
        return Err(UpdateError::Manifest("Adresse nicht erlaubt".into()));
    }
    m.msi_sha256 = m.msi_sha256.trim().to_ascii_lowercase();
    if !m.msi_sha256.is_empty() && !crate::verify::is_sha256_hex(&m.msi_sha256) {
        return Err(UpdateError::Manifest("sha256 ungültig".into()));
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
