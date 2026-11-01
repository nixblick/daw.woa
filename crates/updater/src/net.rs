//! Netz: Manifest holen, Paket laden und prüfen. Blockierend; die Oberfläche ruft das
//! nur aus einem eigenen Thread.
//!
//! Nur HTTPS mit Zertifikatsprüfung. Weiterleitungen folgt der Code selbst, jeden Sprung
//! gegen [`crate::hosts`] geprüft; alles ausserhalb von GitHub wird abgelehnt.

use std::path::PathBuf;
use std::time::Duration;

use crate::error::UpdateError;
use crate::hosts;
use crate::manifest::{self, Available};
use crate::verify;

/// Neuestes **veröffentlichtes** Release (keine Entwürfe, keine Vorabversionen).
pub const MANIFEST_URL: &str =
    "https://github.com/nixblick/daw.woa/releases/latest/download/latest.json";
/// Seite zum Nachlesen und für Systeme ohne automatische Installation.
pub const RELEASES_PAGE: &str = "https://github.com/nixblick/daw.woa/releases/latest";

const MANIFEST_LIMIT: u64 = 64 * 1024;
const SIG_LIMIT: u64 = 16 * 1024;
const MSI_LIMIT: u64 = 512 * 1024 * 1024;
const MAX_HOPS: usize = 5;

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        // Kein Rückfall auf http.
        .https_only(true)
        // Weiterleitungen macht `get` selbst, damit jeder Sprung geprüft wird.
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into()
}

fn get(agent: &ureq::Agent, url: &str, limit: u64) -> Result<Vec<u8>, UpdateError> {
    let mut url = url.to_string();
    for _ in 0..MAX_HOPS {
        let mut resp = agent
            .get(&url)
            .call()
            .map_err(|e| UpdateError::Net(e.to_string()))?;
        let status = resp.status().as_u16();
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            let next = resp
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| UpdateError::Net("Weiterleitung ohne Ziel".into()))?
                .to_string();
            if !hosts::redirect_allowed(&next) {
                return Err(UpdateError::Net(
                    "Weiterleitung auf fremden Host abgelehnt".into(),
                ));
            }
            url = next;
            continue;
        }
        if status != 200 {
            return Err(UpdateError::Net(format!("HTTP {status}")));
        }
        return resp
            .body_mut()
            .with_config()
            .limit(limit)
            .read_to_vec()
            .map_err(|e| UpdateError::Net(e.to_string()));
    }
    Err(UpdateError::Net("zu viele Weiterleitungen".into()))
}

/// Fragt GitHub, ob es eine strikt neuere Version gibt. `Ok(None)`: aktuell.
pub fn check(current: &str) -> Result<Option<Available>, UpdateError> {
    debug_assert!(hosts::start_allowed(MANIFEST_URL));
    let json = get(
        &agent(Duration::from_secs(20)),
        MANIFEST_URL,
        MANIFEST_LIMIT,
    )?;
    manifest::evaluate(&json, current)
}

/// Lädt das MSI samt Signatur, prüft es und legt es erst
/// **danach** in `%TEMP%` ab. Der Rückgabepfad zeigt also immer auf ein geprüftes Paket.
pub fn download_and_verify(update: &Available, current: &str) -> Result<PathBuf, UpdateError> {
    // Vor dem Download: kein Byte für ein Paket, das ohnehin abgelehnt würde.
    crate::version::ensure_newer(&update.version, current)?;
    if !verify::configured() {
        return Err(UpdateError::NotConfigured);
    }
    if update.msi_sig_url.is_empty() {
        return Err(UpdateError::Signature("Manifest ohne Signatur".into()));
    }
    let agent = agent(Duration::from_secs(20 * 60));
    let raw = get(&agent, &update.msi_sig_url, SIG_LIMIT)?;
    let sig = String::from_utf8(raw).map_err(|_| UpdateError::Signature("kein Text".into()))?;
    let msi = get(&agent, &update.msi_url, MSI_LIMIT)?;
    verify::verify_download(
        verify::PUBLIC_KEY,
        &msi,
        Some(&sig),
        &update.msi_sha256,
        &update.version,
        current,
    )?;
    let path = std::env::temp_dir().join(format!("woa-update-{}.msi", update.version));
    std::fs::write(&path, &msi).map_err(|e| UpdateError::Io(e.to_string()))?;
    tracing::info!(version = %update.version, "update downloaded and verified");
    Ok(path)
}

/// Startet den Windows Installer mit dem geprüften MSI und kehrt sofort zurück. Der Aufrufer
/// beendet danach die App. `RESTARTWOA=1` lässt das MSI die neue Version nach der
/// Installation starten (siehe `packaging/windows/woa.wxs`).
#[cfg(windows)]
pub fn launch_installer(msi: &std::path::Path) -> Result<(), UpdateError> {
    std::process::Command::new("msiexec")
        .arg("/i")
        .arg(msi)
        .args(["/passive", "/norestart", "RESTARTWOA=1"])
        .spawn()
        .map(|_| ())
        .map_err(|e| UpdateError::Io(e.to_string()))
}
