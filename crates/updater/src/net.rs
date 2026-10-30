//! Netz: Manifest holen, Paket laden und prüfen. Blockierend; die Oberfläche ruft das
//! nur aus einem eigenen Thread.
//!
//! Nur HTTPS mit Zertifikatsprüfung. Weiterleitungen folgt der Code selbst, jeden Sprung
//! gegen [`crate::hosts`] geprüft; alles ausserhalb von GitHub wird abgelehnt.

use std::path::PathBuf;
use std::time::Duration;

use crate::Kind;
use crate::error::UpdateError;
use crate::hosts::{self, Policy};
use crate::manifest::{self, Available};
use crate::verify;

/// Neuestes **veröffentlichtes** Release (keine Entwürfe, keine Vorabversionen).
pub const MANIFEST_URL: &str =
    "https://github.com/nixblick/daw.woa/releases/latest/download/latest.json";
/// Seite zum Nachlesen und für Systeme ohne automatische Installation.
pub const RELEASES_PAGE: &str = "https://github.com/nixblick/daw.woa/releases/latest";

const MANIFEST_LIMIT: u64 = 64 * 1024;
const SIG_LIMIT: u64 = 16 * 1024;
const PACKAGE_LIMIT: u64 = 512 * 1024 * 1024;
const MAX_HOPS: usize = 5;

fn agent(timeout: Duration, policy: Policy) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        // Kein Rückfall auf http.
        .https_only(policy.https_only())
        // Weiterleitungen macht `get` selbst, damit jeder Sprung geprüft wird.
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into()
}

fn get(agent: &ureq::Agent, policy: Policy, url: &str, limit: u64) -> Result<Vec<u8>, UpdateError> {
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
            if !policy.redirect_allowed(&next) {
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
    check_with(Policy::Github, MANIFEST_URL, current)
}

fn check_with(
    policy: Policy,
    manifest_url: &str,
    current: &str,
) -> Result<Option<Available>, UpdateError> {
    let json = get(
        &agent(Duration::from_secs(20), policy),
        policy,
        manifest_url,
        MANIFEST_LIMIT,
    )?;
    manifest::evaluate_with(&json, current, policy)
}

/// Lädt das Paket für `kind` (MSI oder AppImage; mit eingebautem Schlüssel samt Signatur),
/// prüft es und legt es erst **danach** auf die Platte. Der Rückgabepfad zeigt also immer auf
/// ein geprüftes Paket.
///
/// - MSI: `%TEMP%\woa-update-<version>.msi`.
/// - AppImage: neben der laufenden Datei (gleiches Dateisystem, damit das Ersetzen atomar ist),
///   ausführbar, versteckt als `.woa-update-<version>.part`.
/// - Flatpak und Quellbau: `Err(NotSupported)`, die App zeigt nur den Hinweis.
pub fn download_and_verify(
    update: &Available,
    current: &str,
    kind: Kind,
) -> Result<PathBuf, UpdateError> {
    let dest = match kind {
        Kind::Msi => std::env::temp_dir().join(format!("woa-update-{}.msi", update.version)),
        Kind::AppImage => appimage_staging(update)?,
        Kind::Flatpak | Kind::Other => return Err(UpdateError::NotSupported),
    };
    download_with(Policy::Github, update, current, kind, &dest)?;
    Ok(dest)
}

fn appimage_staging(update: &Available) -> Result<PathBuf, UpdateError> {
    let target = Kind::appimage_path().ok_or(UpdateError::NotSupported)?;
    let dir = target
        .parent()
        .ok_or_else(|| UpdateError::Io("AppImage ohne Ordner".into()))?;
    Ok(dir.join(format!(".woa-update-{}.part", update.version)))
}

fn download_with(
    policy: Policy,
    update: &Available,
    current: &str,
    kind: Kind,
    dest: &std::path::Path,
) -> Result<(), UpdateError> {
    // Vor dem Download: kein Byte für ein Paket, das ohnehin abgelehnt würde.
    crate::version::ensure_newer(&update.version, current)?;
    let pkg = update.package(kind).ok_or(UpdateError::NotSupported)?;
    // Stufe B gilt, sobald ein Schlüssel eingebrannt ist. Die Tests mit Fake-Server laufen in
    // Stufe A (Platzhalter), Stufe B ist in `verify` abgedeckt.
    let signed = verify::configured();
    if signed && pkg.sig_url.is_empty() {
        return Err(UpdateError::Signature("Manifest ohne Signatur".into()));
    }
    if !signed && pkg.sha256.is_empty() {
        return Err(UpdateError::Manifest("Manifest ohne sha256".into()));
    }
    let agent = agent(Duration::from_secs(20 * 60), policy);
    let sig = if signed {
        let raw = get(&agent, policy, pkg.sig_url, SIG_LIMIT)?;
        Some(String::from_utf8(raw).map_err(|_| UpdateError::Signature("kein Text".into()))?)
    } else {
        None
    };
    let bytes = get(&agent, policy, pkg.url, PACKAGE_LIMIT)?;
    verify::verify_download(
        verify::PUBLIC_KEY,
        &bytes,
        sig.as_deref(),
        pkg.sha256,
        &update.version,
        current,
    )?;
    write_package(dest, &bytes, kind)?;
    tracing::info!(version = %update.version, signed, ?kind, "update downloaded and verified");
    Ok(())
}

fn write_package(dest: &std::path::Path, bytes: &[u8], kind: Kind) -> Result<(), UpdateError> {
    let io = |e: std::io::Error| UpdateError::Io(e.to_string());
    std::fs::write(dest, bytes).map_err(io)?;
    #[cfg(unix)]
    if kind == Kind::AppImage {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755)) {
            let _ = std::fs::remove_file(dest);
            return Err(io(e));
        }
    }
    let _ = kind;
    Ok(())
}

/// Ersetzt die laufende AppImage-Datei durch das geprüfte Paket (`rename`, atomar auf
/// demselben Dateisystem). Das laufende Programm behält seine alte Datei im Speicher; erst der
/// nächste Start nimmt die neue. Bei einem Fehler bleibt die alte Datei unverändert und das
/// Paket wird entfernt.
pub fn replace_appimage(
    staged: &std::path::Path,
    target: &std::path::Path,
) -> Result<(), UpdateError> {
    std::fs::rename(staged, target).map_err(|e| {
        let _ = std::fs::remove_file(staged);
        UpdateError::Io(format!("AppImage nicht ersetzbar: {e}"))
    })
}

/// Startet das AppImage neu (nach [`replace_appimage`]) und kehrt sofort zurück. Der Aufrufer
/// beendet danach die App.
#[cfg(unix)]
pub fn restart_appimage(target: &std::path::Path) -> Result<(), UpdateError> {
    std::process::Command::new(target)
        .env_remove("APPDIR")
        .env_remove("APPIMAGE")
        .env_remove("ARGV0")
        .spawn()
        .map(|_| ())
        .map_err(|e| UpdateError::Io(e.to_string()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Fake-Server auf 127.0.0.1: antwortet je Pfad mit fester Antwort, zählt Aufrufe.
    struct Fake {
        base: String,
        hits: Arc<AtomicUsize>,
    }

    /// `routes`: (Pfad, Status, Header `Location` oder leer, Körper).
    fn serve(routes: Vec<(&'static str, u16, String, Vec<u8>)>) -> Fake {
        serve_on(TcpListener::bind("127.0.0.1:0").unwrap(), routes)
    }

    fn manifest(base: &str, version: &str, sha: &str) -> Vec<u8> {
        format!(
            r#"{{"version":"{version}","notes":"Neu","msi_url":"{base}/a.msi","msi_sha256":"{sha}","appimage_url":"{base}/a.AppImage","appimage_sha256":"{sha}"}}"#
        )
        .into_bytes()
    }

    const PAYLOAD: &[u8] = b"pretend this is a package";

    fn fixture(version: &str, served_sha: &str) -> Fake {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
        let routes = vec![
            (
                "/latest.json",
                200,
                String::new(),
                manifest(&base, version, served_sha),
            ),
            ("/a.msi", 200, String::new(), PAYLOAD.to_vec()),
            ("/a.AppImage", 200, String::new(), PAYLOAD.to_vec()),
        ];
        serve_on(l, routes)
    }

    fn serve_on(l: TcpListener, routes: Vec<(&'static str, u16, String, Vec<u8>)>) -> Fake {
        let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        std::thread::spawn(move || {
            for conn in l.incoming() {
                let Ok(mut c) = conn else { return };
                h.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 4096];
                let n = c.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (status, loc, body) = match routes.iter().find(|r| r.0 == path) {
                    Some((_, s, l, b)) => (*s, l.clone(), b.clone()),
                    None => (404, String::new(), Vec::new()),
                };
                let mut head = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                if !loc.is_empty() {
                    head += &format!("Location: {loc}\r\n");
                }
                let _ = c.write_all(format!("{head}\r\n").as_bytes());
                let _ = c.write_all(&body);
            }
        });
        Fake { base, hits }
    }

    fn good_sha() -> String {
        verify::sha256_hex(PAYLOAD)
    }

    fn check_local(f: &Fake, current: &str) -> Result<Option<Available>, UpdateError> {
        check_with(Policy::Local, &format!("{}/latest.json", f.base), current)
    }

    #[test]
    fn newer_version_is_offered() {
        let f = fixture("0.35.0", &good_sha());
        let a = check_local(&f, "0.34.1").unwrap().unwrap();
        assert_eq!(a.version, "0.35.0");
        assert!(a.package(Kind::AppImage).is_some());
        assert!(a.package(Kind::Flatpak).is_none());
    }

    #[test]
    fn same_version_is_no_update() {
        let f = fixture("0.34.1", &good_sha());
        assert_eq!(check_local(&f, "0.34.1"), Ok(None));
    }

    #[test]
    fn older_manifest_is_vetoed() {
        let f = fixture("0.33.0", &good_sha());
        assert_eq!(
            check_local(&f, "0.34.1"),
            Err(UpdateError::Veto(crate::Veto::Downgrade))
        );
    }

    #[test]
    fn no_network_is_a_quiet_error() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l); // nichts lauscht mehr
        let r = check_with(
            Policy::Local,
            &format!("http://127.0.0.1:{port}/latest.json"),
            "0.1.0",
        );
        assert!(matches!(r, Err(UpdateError::Net(_))), "{r:?}");
    }

    #[test]
    fn server_error_and_garbage_are_errors() {
        let f = serve(vec![
            ("/latest.json", 500, String::new(), b"boom".to_vec()),
            ("/kaputt.json", 200, String::new(), b"<html>".to_vec()),
        ]);
        let r = check_with(Policy::Local, &format!("{}/latest.json", f.base), "0.1.0");
        assert!(matches!(r, Err(UpdateError::Net(_))));
        let r = check_with(Policy::Local, &format!("{}/kaputt.json", f.base), "0.1.0");
        assert!(matches!(r, Err(UpdateError::Manifest(_))));
    }

    #[test]
    fn redirect_to_foreign_host_is_refused() {
        let f = serve(vec![(
            "/latest.json",
            302,
            "http://evil.example/latest.json".into(),
            Vec::new(),
        )]);
        let r = check_with(Policy::Local, &format!("{}/latest.json", f.base), "0.1.0");
        assert!(matches!(r, Err(UpdateError::Net(m)) if m.contains("fremden Host")));
    }

    #[test]
    fn good_download_is_written_and_executable() {
        if verify::configured() {
            return; // Stufe B: Signatur nötig, siehe verify-Tests
        }
        let f = fixture("0.35.0", &good_sha());
        let a = check_local(&f, "0.34.1").unwrap().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join(".woa-update-0.35.0.part");
        download_with(Policy::Local, &a, "0.34.1", Kind::AppImage, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), PAYLOAD);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&dest).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111);
        }
    }

    #[test]
    fn wrong_checksum_writes_nothing() {
        if verify::configured() {
            return;
        }
        let f = fixture("0.35.0", &"0".repeat(64));
        let a = check_local(&f, "0.34.1").unwrap().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("x.part");
        let r = download_with(Policy::Local, &a, "0.34.1", Kind::AppImage, &dest);
        assert!(matches!(r, Err(UpdateError::Signature(_))), "{r:?}");
        assert!(!dest.exists());
    }

    #[test]
    fn flatpak_and_source_builds_only_report() {
        let f = fixture("0.35.0", &good_sha());
        let a = check_local(&f, "0.34.1").unwrap().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let before = f.hits.load(Ordering::SeqCst);
        for kind in [Kind::Flatpak, Kind::Other] {
            let r = download_with(Policy::Local, &a, "0.34.1", kind, &dir.path().join("x"));
            assert_eq!(r, Err(UpdateError::NotSupported));
            assert_eq!(
                download_and_verify(&a, "0.34.1", kind),
                Err(UpdateError::NotSupported)
            );
        }
        // Kein Byte geladen.
        assert_eq!(f.hits.load(Ordering::SeqCst), before);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn download_refuses_old_version_before_any_request() {
        let f = fixture("0.35.0", &good_sha());
        let a = check_local(&f, "0.34.1").unwrap().unwrap();
        let before = f.hits.load(Ordering::SeqCst);
        let dir = tempfile::tempdir().unwrap();
        let r = download_with(
            Policy::Local,
            &a,
            "0.35.0",
            Kind::AppImage,
            &dir.path().join("x"),
        );
        assert_eq!(r, Err(UpdateError::Veto(crate::Veto::SameVersion)));
        assert_eq!(f.hits.load(Ordering::SeqCst), before);
    }

    #[test]
    fn replace_swaps_the_file_and_keeps_old_on_error() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("WOA.AppImage");
        let staged = dir.path().join(".woa-update-0.35.0.part");
        std::fs::write(&target, b"old").unwrap();
        std::fs::write(&staged, b"new").unwrap();
        replace_appimage(&staged, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert!(!staged.exists());
        // Paket fehlt: Fehler, alte Datei bleibt.
        let r = replace_appimage(&dir.path().join("weg"), &target);
        assert!(matches!(r, Err(UpdateError::Io(_))));
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
    }
}
