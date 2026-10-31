//! Signaturprüfung (minisign), fail-closed.

use minisign_verify::{PublicKey, Signature};
use sha2::{Digest, Sha256};

use crate::error::UpdateError;
use crate::version;

/// Öffentlicher Schlüssel, zur Bauzeit eingebrannt. Solange die Datei einen Platzhalter
/// enthält (kein gültiger minisign-Schlüssel), ist der Updater aus.
pub const PUBLIC_KEY: &str = include_str!("../../../packaging/windows/updater.pub");

/// `true`, wenn `key_text` ein gültiger öffentlicher minisign-Schlüssel ist.
pub fn key_configured(key_text: &str) -> bool {
    PublicKey::decode(key_text).is_ok()
}

/// Ist ein echter Schlüssel eingebaut?
pub fn configured() -> bool {
    key_configured(PUBLIC_KEY)
}

/// Prüft ein heruntergeladenes Paket. Erst wenn das `Ok` kommt, darf es installiert werden.
///
/// 1. Die Manifest-Version muss strikt neuer sein als `current` (Downgrade-Wächter).
/// 2. Die Signatur gilt über die **Bytes des MSI** und den Schlüssel in `pubkey`.
/// 3. Der signierte (vertrauenswürdige) Kommentar muss `woa <version>` lauten, mit genau
///    der Manifest-Version. So kann ein Angreifer mit gefälschtem Manifest kein altes,
///    echt signiertes MSI unter einer neuen Versionsnummer unterschieben.
///
/// Die `sha256` im Manifest ersetzt das nicht: sie liegt neben dem Paket (siehe [`verify_hash`]).
pub fn verify_package(
    pubkey: &str,
    msi: &[u8],
    sig_text: &str,
    manifest_version: &str,
    current: &str,
) -> Result<(), UpdateError> {
    version::ensure_newer(manifest_version, current)?;
    let key = PublicKey::decode(pubkey).map_err(|_| UpdateError::NotConfigured)?;
    let sig = Signature::decode(sig_text).map_err(|e| UpdateError::Signature(e.to_string()))?;
    // `false`: nur die moderne (vorgehashte) Signatur, keine Legacy-Variante.
    key.verify(msi, &sig, false)
        .map_err(|e| UpdateError::Signature(e.to_string()))?;
    let signed = signed_version(sig.trusted_comment())
        .ok_or_else(|| UpdateError::Signature("Kommentar ohne Version".into()))?;
    if version::parse(signed).is_none()
        || version::parse(signed) != version::parse(manifest_version)
    {
        return Err(UpdateError::Signature(format!(
            "signierte Version {signed} passt nicht zum Manifest {manifest_version}"
        )));
    }
    Ok(())
}

/// 64 Hex-Zeichen, klein.
pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// SHA-256 als kleingeschriebenes Hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Stufe A: Prüfsumme gegen das Manifest. Schützt vor beschädigten Downloads und vor
/// Adressen ausserhalb von GitHub, **nicht** vor jemandem, der Release-Dateien auf GitHub
/// austauschen kann (er tauscht MSI und Prüfsumme gemeinsam).
pub fn verify_hash(
    msi: &[u8],
    expected: &str,
    manifest_version: &str,
    current: &str,
) -> Result<(), UpdateError> {
    version::ensure_newer(manifest_version, current)?;
    if !is_sha256_hex(expected) {
        return Err(UpdateError::Manifest("sha256 fehlt oder ungültig".into()));
    }
    if sha256_hex(msi) != expected {
        return Err(UpdateError::Signature("sha256 passt nicht".into()));
    }
    Ok(())
}

/// Eine Stufe, kein Rückfall. Ist in `pubkey` ein echter Schlüssel, ist die Signatur Pflicht
/// (Stufe B, fail-closed): fehlt sie, wird abgelehnt, die Prüfsumme allein genügt nie. Steht
/// dort ein Platzhalter, gilt Stufe A: die Prüfsumme aus dem Manifest ist Pflicht.
pub fn verify_download(
    pubkey: &str,
    msi: &[u8],
    sig_text: Option<&str>,
    sha256: &str,
    manifest_version: &str,
    current: &str,
) -> Result<(), UpdateError> {
    if key_configured(pubkey) {
        let sig = sig_text.ok_or_else(|| UpdateError::Signature("Signatur fehlt".into()))?;
        verify_package(pubkey, msi, sig, manifest_version, current)?;
        // Steht trotzdem eine Prüfsumme im Manifest, muss sie stimmen.
        if !sha256.is_empty() {
            verify_hash(msi, sha256, manifest_version, current)?;
        }
        Ok(())
    } else {
        verify_hash(msi, sha256, manifest_version, current)
    }
}

/// Version aus dem Kommentar `woa X.Y.Z` (genau zwei Wörter).
fn signed_version(comment: &str) -> Option<&str> {
    let mut it = comment.split_whitespace();
    let (Some("woa"), Some(v), None) = (it.next(), it.next(), it.next()) else {
        return None;
    };
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Veto;

    const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/updater/");

    fn read(name: &str) -> String {
        std::fs::read_to_string(format!("{DIR}{name}")).unwrap()
    }
    fn payload() -> Vec<u8> {
        std::fs::read(format!("{DIR}payload.bin")).unwrap()
    }
    fn verify(sig: &str, manifest: &str, current: &str) -> Result<(), UpdateError> {
        verify_package(&read("test.pub"), &payload(), sig, manifest, current)
    }

    /// Ein halb eingefügter oder beschädigter Schlüssel in `updater.pub` würde den Updater
    /// stillschweigend auf Stufe A zurückfallen lassen. Darum: Die Datei ist entweder der
    /// Platzhalter (keine minisign-Kopfzeile) oder ein gültiger Schlüssel, nie etwas dazwischen.
    #[test]
    fn updater_pub_is_placeholder_or_valid_key() {
        let text = PUBLIC_KEY;
        if text.starts_with("untrusted comment:") {
            assert!(
                key_configured(text),
                "packaging/windows/updater.pub sieht nach minisign aus, ist aber kein gültiger Schlüssel"
            );
        } else {
            assert!(!key_configured(text), "Platzhalter darf nicht als Schlüssel gelten");
        }
    }

    #[test]
    fn broken_key_text_is_not_configured() {
        let good = read("test.pub");
        assert!(key_configured(&good));
        let cut: String = good.lines().take(1).collect::<Vec<_>>().join("\n");
        assert!(!key_configured(&cut));
        assert!(!key_configured(""));
    }

    #[test]
    fn good_signature_passes() {
        assert_eq!(verify(&read("payload.minisig"), "0.34.0", "0.33.4"), Ok(()));
    }

    #[test]
    fn tampered_bytes_fail() {
        let mut bytes = payload();
        bytes[100] ^= 1;
        let r = verify_package(
            &read("test.pub"),
            &bytes,
            &read("payload.minisig"),
            "0.34.0",
            "0.33.4",
        );
        assert!(matches!(r, Err(UpdateError::Signature(_))), "{r:?}");
    }

    #[test]
    fn foreign_key_fails() {
        let r = verify(&read("payload-foreign.minisig"), "0.34.0", "0.33.4");
        assert!(matches!(r, Err(UpdateError::Signature(_))), "{r:?}");
    }

    #[test]
    fn comment_version_must_equal_manifest_version() {
        // Echt signiert, aber für 0.33.0; das Manifest behauptet 0.34.0.
        let r = verify(&read("payload-oldver.minisig"), "0.34.0", "0.33.4");
        assert!(matches!(r, Err(UpdateError::Signature(_))), "{r:?}");
    }

    #[test]
    fn edited_trusted_comment_breaks_the_signature() {
        // Der Kommentar steht unter einer zweiten Signatur; umschreiben geht nicht.
        let forged = read("payload-oldver.minisig").replace("woa 0.33.0", "woa 0.34.0");
        assert_ne!(forged, read("payload-oldver.minisig"));
        let r = verify(&forged, "0.34.0", "0.33.4");
        assert!(matches!(r, Err(UpdateError::Signature(_))), "{r:?}");
    }

    #[test]
    fn downgrade_same_and_unreadable_are_vetoed_before_anything_else() {
        let sig = read("payload.minisig");
        assert_eq!(
            verify(&sig, "0.34.0", "0.34.0"),
            Err(Veto::SameVersion.into())
        );
        assert_eq!(
            verify(&sig, "0.34.0", "0.35.0"),
            Err(Veto::Downgrade.into())
        );
        assert_eq!(
            verify(&sig, "0.34.0", "kaputt"),
            Err(Veto::UnreadableVersion.into())
        );
        assert_eq!(
            verify(&sig, "neu", "0.33.4"),
            Err(Veto::UnreadableVersion.into())
        );
    }

    #[test]
    fn garbage_signature_and_missing_key_fail_closed() {
        let r = verify("das ist keine signatur", "0.34.0", "0.33.4");
        assert!(matches!(r, Err(UpdateError::Signature(_))), "{r:?}");
        let r = verify_package(
            "PLATZHALTER",
            &payload(),
            &read("payload.minisig"),
            "0.34.0",
            "0.33.4",
        );
        assert_eq!(r, Err(UpdateError::NotConfigured));
    }

    #[test]
    fn placeholder_key_means_not_configured() {
        assert!(!key_configured("PLATZHALTER: hier kommt der Schlüssel hin"));
        assert!(!key_configured(""));
        assert!(key_configured(&read("test.pub")));
    }

    fn hash_of(bytes: &[u8]) -> String {
        sha256_hex(bytes)
    }

    #[test]
    fn sha256_matches_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(is_sha256_hex(&sha256_hex(b"")));
        assert!(!is_sha256_hex("ABC"));
    }

    #[test]
    fn stage_a_needs_a_matching_hash() {
        let msi = payload();
        let h = hash_of(&msi);
        let ph = "PLATZHALTER";
        assert_eq!(
            verify_download(ph, &msi, None, &h, "0.34.0", "0.33.4"),
            Ok(())
        );
        let mut bad = msi.clone();
        bad[0] ^= 1;
        assert!(verify_download(ph, &bad, None, &h, "0.34.0", "0.33.4").is_err());
        assert!(matches!(
            verify_download(ph, &msi, None, "", "0.34.0", "0.33.4"),
            Err(UpdateError::Manifest(_))
        ));
    }

    #[test]
    fn stage_a_keeps_the_downgrade_guard() {
        let msi = payload();
        let h = hash_of(&msi);
        assert_eq!(
            verify_download("PLATZHALTER", &msi, None, &h, "0.33.0", "0.33.4"),
            Err(Veto::Downgrade.into())
        );
        assert_eq!(
            verify_download("PLATZHALTER", &msi, None, &h, "0.33.4", "0.33.4"),
            Err(Veto::SameVersion.into())
        );
    }

    #[test]
    fn stage_b_requires_the_signature_and_never_falls_back_to_the_hash() {
        let msi = payload();
        let h = hash_of(&msi);
        let key = read("test.pub");
        // Hash stimmt, Signatur fehlt: abgelehnt.
        let r = verify_download(&key, &msi, None, &h, "0.34.0", "0.33.4");
        assert!(matches!(r, Err(UpdateError::Signature(_))), "{r:?}");
        // Signatur schlecht, Hash stimmt: abgelehnt.
        let sig = read("payload-foreign.minisig");
        let r = verify_download(&key, &msi, Some(&sig), &h, "0.34.0", "0.33.4");
        assert!(matches!(r, Err(UpdateError::Signature(_))), "{r:?}");
        // Signatur gut, Hash leer: ok; Hash falsch: abgelehnt.
        let sig = read("payload.minisig");
        assert_eq!(
            verify_download(&key, &msi, Some(&sig), "", "0.34.0", "0.33.4"),
            Ok(())
        );
        assert_eq!(
            verify_download(&key, &msi, Some(&sig), &h, "0.34.0", "0.33.4"),
            Ok(())
        );
        let wrong = hash_of(b"anderes");
        assert!(verify_download(&key, &msi, Some(&sig), &wrong, "0.34.0", "0.33.4").is_err());
    }

    #[test]
    fn comment_format_is_strict() {
        assert_eq!(signed_version("woa 0.34.0"), Some("0.34.0"));
        assert_eq!(signed_version("woa"), None);
        assert_eq!(signed_version("woa 0.34.0 extra"), None);
        assert_eq!(signed_version("andere 0.34.0"), None);
    }
}
