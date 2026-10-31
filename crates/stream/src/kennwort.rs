//! Das Quell-Kennwort: nie in `config.json`, nie geloggt, nie in `Debug`.
//!
//! Ablage in dieser Reihenfolge:
//!
//! 1. Betriebssystem-Schlüsselbund (Windows: Anmeldeinformationsverwaltung, Linux: Secret
//!    Service, macOS: Schlüsselbund) unter Dienst `woa`, Eintrag `stream-source`.
//! 2. Fehlt ein Schlüsselbund (Server ohne Oberfläche, Container), eine Datei
//!    `stream-source.pass` im Datenordner mit Modus 0600.
//! 3. Eine in der Konfiguration eingetragene Kennwortdatei (`password_file`) für Skripte und
//!    die Kommandozeile; sie gewinnt, solange sie eingetragen ist. Fehlt sie, ist das ein Fehler.

use std::path::{Path, PathBuf};

use crate::config::StreamConfig;

/// Dienstname im Schlüsselbund.
const SERVICE: &str = "woa";
/// Eintragsname im Schlüsselbund.
const ACCOUNT: &str = "stream-source";
/// Name der Ersatzdatei im Datenordner.
pub const FALLBACK_FILE: &str = "stream-source.pass";

/// Wo das Kennwort liegt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ort {
    Schluesselbund,
    Datei,
}

impl Ort {
    /// Für die Anzeige.
    pub fn label(self) -> &'static str {
        match self {
            Self::Schluesselbund => "im Schlüsselbund des Systems",
            Self::Datei => {
                "in einer geschützten Datei im Datenordner (kein Schlüsselbund gefunden)"
            }
        }
    }
}

/// Ein Speicher für das Kennwort. Der echte ist der Schlüsselbund; Tests nehmen einen
/// im Speicher, damit sie nie den Schlüsselbund des Rechners anfassen.
pub trait Speicher {
    /// `Ok(None)`: kein Eintrag. `Err`: Schlüsselbund nicht benutzbar.
    fn lesen(&self) -> Result<Option<String>, String>;
    fn schreiben(&self, wert: &str) -> Result<(), String>;
    fn loeschen(&self) -> Result<(), String>;
}

/// Schlüsselbund des Betriebssystems.
pub struct Schluesselbund;

impl Schluesselbund {
    fn eintrag() -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| e.to_string())
    }
}

impl Speicher for Schluesselbund {
    fn lesen(&self) -> Result<Option<String>, String> {
        match Self::eintrag()?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn schreiben(&self, wert: &str) -> Result<(), String> {
        Self::eintrag()?
            .set_password(wert)
            .map_err(|e| e.to_string())
    }

    fn loeschen(&self) -> Result<(), String> {
        match Self::eintrag()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Pfad der Ersatzdatei.
pub fn fallback_path(data_dir: &Path) -> PathBuf {
    data_dir.join(FALLBACK_FILE)
}

fn read_fallback(data_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(fallback_path(data_dir)).ok()?;
    let line = text.lines().next()?.trim_end_matches('\r');
    (!line.is_empty()).then(|| line.to_string())
}

/// Schreibt die Ersatzdatei: Modus 0600 schon beim Anlegen, nie lesbar für andere.
fn write_fallback(data_dir: &Path, wert: &str) -> Result<(), String> {
    use std::io::Write;
    std::fs::create_dir_all(data_dir)
        .map_err(|e| format!("Datenordner {} nicht anlegbar: {e}", data_dir.display()))?;
    let path = fallback_path(data_dir);
    let tmp = path.with_extension("pass.neu");
    let _ = std::fs::remove_file(&tmp);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts
        .open(&tmp)
        .map_err(|e| format!("Kennwortdatei {} nicht anlegbar: {e}", tmp.display()))?;
    f.write_all(wert.as_bytes())
        .and_then(|()| f.write_all(b"\n"))
        .and_then(|()| f.sync_all())
        .map_err(|e| format!("Kennwortdatei {} nicht schreibbar: {e}", tmp.display()))?;
    drop(f);
    std::fs::rename(&tmp, &path)
        .map_err(|e| format!("Kennwortdatei {} nicht ersetzbar: {e}", path.display()))
}

/// Speichert das Kennwort: Schlüsselbund, sonst Ersatzdatei. Eine Ersatzdatei, die zu einem
/// früheren Stand gehört, verschwindet, sobald der Schlüsselbund das neue Kennwort hat.
/// Der Fehlertext enthält nie das Kennwort.
pub fn save_with(store: &dyn Speicher, data_dir: &Path, wert: &str) -> Result<Ort, String> {
    if wert.is_empty() || wert.chars().any(|c| c == '\r' || c == '\n') {
        return Err("Kennwort darf nicht leer sein und keinen Zeilenumbruch enthalten".into());
    }
    match store.schreiben(wert) {
        Ok(()) => {
            let _ = std::fs::remove_file(fallback_path(data_dir));
            Ok(Ort::Schluesselbund)
        }
        Err(e) => {
            tracing::warn!(error = %e, "keyring unavailable, using protected file");
            write_fallback(data_dir, wert)?;
            Ok(Ort::Datei)
        }
    }
}

/// [`save_with`] im Schlüsselbund des Systems.
pub fn save(data_dir: &Path, wert: &str) -> Result<Ort, String> {
    save_with(&Schluesselbund, data_dir, wert)
}

/// Wo liegt ein gespeichertes Kennwort (ohne es zu lesen)?
pub fn stored_with(store: &dyn Speicher, data_dir: &Path) -> Option<Ort> {
    if matches!(store.lesen(), Ok(Some(_))) {
        Some(Ort::Schluesselbund)
    } else if read_fallback(data_dir).is_some() {
        Some(Ort::Datei)
    } else {
        None
    }
}

pub fn stored(data_dir: &Path) -> Option<Ort> {
    stored_with(&Schluesselbund, data_dir)
}

/// Entfernt das gespeicherte Kennwort aus Schlüsselbund und Ersatzdatei.
pub fn clear_with(store: &dyn Speicher, data_dir: &Path) -> Result<(), String> {
    let kb = store.loeschen();
    let _ = std::fs::remove_file(fallback_path(data_dir));
    kb
}

pub fn clear(data_dir: &Path) -> Result<(), String> {
    clear_with(&Schluesselbund, data_dir)
}

/// Das gespeicherte Kennwort selbst (für „Anzeigen“ und „Verbindung testen“).
pub fn reveal_with(store: &dyn Speicher, data_dir: &Path) -> Option<String> {
    store
        .lesen()
        .ok()
        .flatten()
        .filter(|v| !v.is_empty())
        .or_else(|| read_fallback(data_dir))
}

pub fn reveal(data_dir: &Path) -> Option<String> {
    reveal_with(&Schluesselbund, data_dir)
}

/// Übernimmt eine alte Kennwortdatei in den sicheren Speicher, wenn dort noch nichts liegt.
/// Die Datei selbst bleibt unberührt. `Ok(Some(ort))` heißt: übernommen, die Konfiguration
/// kann `password_file` leeren.
pub fn import_legacy_with(
    store: &dyn Speicher,
    data_dir: &Path,
    file: &Path,
) -> Result<Option<Ort>, String> {
    if file.as_os_str().is_empty() || !file.is_file() {
        return Ok(None);
    }
    if let Some(ort) = stored_with(store, data_dir) {
        return Ok(Some(ort));
    }
    let kw = Kennwort::from_file(file)?;
    save_with(store, data_dir, kw.expose()).map(Some)
}

pub fn import_legacy(data_dir: &Path, file: &Path) -> Result<Option<Ort>, String> {
    import_legacy_with(&Schluesselbund, data_dir, file)
}

impl Kennwort {
    /// Das Kennwort für das Senden: eingetragene Kennwortdatei, sonst Schlüsselbund, sonst
    /// Ersatzdatei. Die Fehlermeldung nennt nie den Inhalt.
    pub fn load_with(
        store: &dyn Speicher,
        cfg: &StreamConfig,
        data_dir: &Path,
    ) -> Result<Self, String> {
        if !cfg.password_file.as_os_str().is_empty() {
            return Self::from_file(&cfg.password_file);
        }
        reveal_with(store, data_dir)
            .map(Self)
            .ok_or_else(|| "Kein Kennwort gespeichert (Einstellungen → Sendung)".into())
    }

    pub fn load(cfg: &StreamConfig, data_dir: &Path) -> Result<Self, String> {
        Self::load_with(&Schluesselbund, cfg, data_dir)
    }
}

/// Kennwort im Speicher. `Debug` zeigt nur `***`.
#[derive(Clone, PartialEq, Eq)]
pub struct Kennwort(String);

impl Kennwort {
    /// Liest die erste Zeile von `path` (ohne Zeilenende). Die Fehlermeldung nennt nur
    /// den Pfad, nie den Inhalt.
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("Kennwortdatei {} nicht lesbar: {e}", path.display()))?;
        let line = text.lines().next().unwrap_or("").trim_end_matches('\r');
        if line.is_empty() {
            return Err(format!("Kennwortdatei {} ist leer", path.display()));
        }
        Ok(Self(line.to_string()))
    }

    /// Aus einem Wert (Tests).
    pub fn from_value(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Der Wert, nur für das Protokoll.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Kennwort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Kennwort(***)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_first_line_and_hides_it() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("k");
        std::fs::write(&p, "abc123x\r\nzweite\n").unwrap();
        let k = Kennwort::from_file(&p).unwrap();
        assert_eq!(k.expose(), "abc123x");
        assert!(!format!("{k:?}").contains("abc123x"));
        std::fs::write(&p, "\n").unwrap();
        assert!(Kennwort::from_file(&p).is_err());
        let e = Kennwort::from_file(&tmp.path().join("fehlt")).unwrap_err();
        assert!(e.contains("fehlt"));
    }

    use std::cell::RefCell;

    /// Speicher im Arbeitsspeicher; `kaputt` spielt „kein Schlüsselbund“.
    #[derive(Default)]
    struct Mem {
        v: RefCell<Option<String>>,
        kaputt: bool,
    }

    impl Speicher for Mem {
        fn lesen(&self) -> Result<Option<String>, String> {
            if self.kaputt {
                return Err("kein Dienst".into());
            }
            Ok(self.v.borrow().clone())
        }
        fn schreiben(&self, wert: &str) -> Result<(), String> {
            if self.kaputt {
                return Err("kein Dienst".into());
            }
            *self.v.borrow_mut() = Some(wert.to_string());
            Ok(())
        }
        fn loeschen(&self) -> Result<(), String> {
            *self.v.borrow_mut() = None;
            Ok(())
        }
    }

    #[test]
    fn keyring_first_then_protected_file() {
        let tmp = tempfile::tempdir().unwrap();
        let ok = Mem::default();
        assert_eq!(
            save_with(&ok, tmp.path(), "geheim1").unwrap(),
            Ort::Schluesselbund
        );
        assert!(!fallback_path(tmp.path()).exists());
        let cfg = StreamConfig::default();
        assert_eq!(
            Kennwort::load_with(&ok, &cfg, tmp.path()).unwrap().expose(),
            "geheim1"
        );

        let kaputt = Mem {
            kaputt: true,
            ..Mem::default()
        };
        assert_eq!(
            save_with(&kaputt, tmp.path(), "geheim2").unwrap(),
            Ort::Datei
        );
        assert_eq!(stored_with(&kaputt, tmp.path()), Some(Ort::Datei));
        assert_eq!(
            Kennwort::load_with(&kaputt, &cfg, tmp.path())
                .unwrap()
                .expose(),
            "geheim2"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(fallback_path(tmp.path()))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // Ersetzen bleibt 0600, und ein später verfügbarer Schlüsselbund räumt die Datei ab.
        save_with(&kaputt, tmp.path(), "geheim3").unwrap();
        assert_eq!(reveal_with(&kaputt, tmp.path()).as_deref(), Some("geheim3"));
        save_with(&ok, tmp.path(), "geheim4").unwrap();
        assert!(!fallback_path(tmp.path()).exists());
        clear_with(&ok, tmp.path()).unwrap();
        assert_eq!(stored_with(&ok, tmp.path()), None);
        assert!(
            Kennwort::load_with(&ok, &cfg, tmp.path())
                .unwrap_err()
                .contains("Kein Kennwort")
        );
    }

    #[test]
    fn rejects_bad_values_and_never_names_them() {
        let tmp = tempfile::tempdir().unwrap();
        let m = Mem::default();
        assert!(save_with(&m, tmp.path(), "").is_err());
        assert!(save_with(&m, tmp.path(), "a\nb").is_err());
        let e = save_with(
            &Mem {
                kaputt: true,
                ..Mem::default()
            },
            &tmp.path().join("x\0y"),
            "streng-geheim",
        )
        .unwrap_err();
        assert!(!e.contains("streng-geheim"));
    }

    #[test]
    fn legacy_file_is_imported_once_and_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let alt = tmp.path().join("alt.pass");
        std::fs::write(&alt, "altes-kw\n").unwrap();
        let m = Mem::default();
        assert_eq!(
            import_legacy_with(&m, tmp.path(), &alt).unwrap(),
            Some(Ort::Schluesselbund)
        );
        assert_eq!(m.v.borrow().as_deref(), Some("altes-kw"));
        assert!(alt.exists(), "alte Datei bleibt liegen");
        // Ein neueres Kennwort im Speicher wird nicht überschrieben.
        *m.v.borrow_mut() = Some("neu".into());
        import_legacy_with(&m, tmp.path(), &alt).unwrap();
        assert_eq!(m.v.borrow().as_deref(), Some("neu"));
        assert_eq!(
            import_legacy_with(&m, tmp.path(), &tmp.path().join("fehlt")).unwrap(),
            None
        );
        // Eine eingetragene Datei gewinnt beim Senden.
        let cfg = StreamConfig {
            password_file: alt,
            ..StreamConfig::default()
        };
        assert_eq!(
            Kennwort::load_with(&m, &cfg, tmp.path()).unwrap().expose(),
            "altes-kw"
        );
    }
}
