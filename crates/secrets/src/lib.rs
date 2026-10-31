//! Geheimnisse (Kennwörter, API-Schlüssel, Zugriffstoken): nie in `config.json`, nie geloggt,
//! nie in `Debug`.
//!
//! Ablage je Eintrag in dieser Reihenfolge:
//!
//! 1. Betriebssystem-Schlüsselbund (Windows: Anmeldeinformationsverwaltung, Linux: Secret
//!    Service, macOS: Schlüsselbund) unter Dienst `woa` und dem Eintragsnamen.
//! 2. Fehlt ein Schlüsselbund (Server ohne Oberfläche, Container), eine Datei
//!    `<eintrag>.pass` im Datenordner mit Modus 0600.
//!
//! Ein Eintragsname ist eine stabile Kennung (`stream-source`, `mcp-token`, `ki-anthropic`),
//! nie ein Anzeigename. Erlaubt sind Kleinbuchstaben, Ziffern und `-`, damit er auch als
//! Dateiname taugt. Die Konfiguration hält nur diese Kennungen, nie den Wert.

use std::path::{Path, PathBuf};

/// Dienstname im Schlüsselbund.
const SERVICE: &str = "woa";

/// Wo das Geheimnis liegt.
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

/// Ein Speicher für ein Geheimnis. Der echte ist der Schlüsselbund; Tests nehmen einen
/// im Speicher, damit sie nie den Schlüsselbund des Rechners anfassen.
pub trait Speicher {
    /// `Ok(None)`: kein Eintrag. `Err`: Schlüsselbund nicht benutzbar.
    fn lesen(&self) -> Result<Option<String>, String>;
    fn schreiben(&self, wert: &str) -> Result<(), String>;
    fn loeschen(&self) -> Result<(), String>;
}

/// Schlüsselbund des Betriebssystems für einen Eintrag.
pub struct Schluesselbund {
    account: String,
}

impl Schluesselbund {
    pub fn new(account: &str) -> Self {
        Self {
            account: account.to_string(),
        }
    }

    fn eintrag(&self) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, &self.account).map_err(|e| e.to_string())
    }
}

impl Speicher for Schluesselbund {
    fn lesen(&self) -> Result<Option<String>, String> {
        match self.eintrag()?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn schreiben(&self, wert: &str) -> Result<(), String> {
        self.eintrag()?
            .set_password(wert)
            .map_err(|e| e.to_string())
    }

    fn loeschen(&self) -> Result<(), String> {
        match self.eintrag()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Ist `account` eine zulässige Kennung (nicht leer, `a-z`, `0-9`, `-`, höchstens 64 Zeichen)?
pub fn valid_account(account: &str) -> bool {
    !account.is_empty()
        && account.len() <= 64
        && account
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Pfad der Ersatzdatei eines Eintrags.
pub fn fallback_path(data_dir: &Path, account: &str) -> PathBuf {
    data_dir.join(format!("{account}.pass"))
}

fn check_account(account: &str) -> Result<(), String> {
    if valid_account(account) {
        Ok(())
    } else {
        Err("ungültiger Name für den Eintrag im Schlüsselbund".into())
    }
}

fn read_fallback(data_dir: &Path, account: &str) -> Option<String> {
    if !valid_account(account) {
        return None;
    }
    let text = std::fs::read_to_string(fallback_path(data_dir, account)).ok()?;
    let line = text.lines().next()?.trim_end_matches('\r');
    (!line.is_empty()).then(|| line.to_string())
}

/// Schreibt die Ersatzdatei: Modus 0600 schon beim Anlegen, nie lesbar für andere.
fn write_fallback(data_dir: &Path, account: &str, wert: &str) -> Result<(), String> {
    use std::io::Write;
    std::fs::create_dir_all(data_dir)
        .map_err(|e| format!("Datenordner {} nicht anlegbar: {e}", data_dir.display()))?;
    let path = fallback_path(data_dir, account);
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

/// Speichert den Wert: Schlüsselbund, sonst Ersatzdatei. Eine Ersatzdatei, die zu einem
/// früheren Stand gehört, verschwindet, sobald der Schlüsselbund den neuen Wert hat.
/// Der Fehlertext enthält nie den Wert.
pub fn save_with(
    store: &dyn Speicher,
    data_dir: &Path,
    account: &str,
    wert: &str,
) -> Result<Ort, String> {
    check_account(account)?;
    if wert.is_empty() || wert.chars().any(|c| c == '\r' || c == '\n') {
        return Err("Wert darf nicht leer sein und keinen Zeilenumbruch enthalten".into());
    }
    match store.schreiben(wert) {
        Ok(()) => {
            let _ = std::fs::remove_file(fallback_path(data_dir, account));
            Ok(Ort::Schluesselbund)
        }
        Err(e) => {
            tracing::warn!(error = %e, account, "keyring unavailable, using protected file");
            // Ein älterer Schlüsselbund-Eintrag ginge beim Lesen vor und verdeckte den neuen Wert.
            let _ = store.loeschen();
            write_fallback(data_dir, account, wert)?;
            Ok(Ort::Datei)
        }
    }
}

/// Wo liegt ein gespeicherter Wert (ohne ihn zu lesen)?
pub fn stored_with(store: &dyn Speicher, data_dir: &Path, account: &str) -> Option<Ort> {
    if matches!(store.lesen(), Ok(Some(_))) {
        Some(Ort::Schluesselbund)
    } else if read_fallback(data_dir, account).is_some() {
        Some(Ort::Datei)
    } else {
        None
    }
}

/// Entfernt den Wert aus Schlüsselbund und Ersatzdatei.
pub fn clear_with(store: &dyn Speicher, data_dir: &Path, account: &str) -> Result<(), String> {
    check_account(account)?;
    let kb = store.loeschen();
    let _ = std::fs::remove_file(fallback_path(data_dir, account));
    kb
}

/// Der gespeicherte Wert selbst (für „Anzeigen“, „Verbindung testen“ und den Betrieb).
pub fn reveal_with(store: &dyn Speicher, data_dir: &Path, account: &str) -> Option<String> {
    store
        .lesen()
        .ok()
        .flatten()
        .filter(|v| !v.is_empty())
        .or_else(|| read_fallback(data_dir, account))
}

/// [`save_with`] im Schlüsselbund des Systems.
pub fn save(data_dir: &Path, account: &str, wert: &str) -> Result<Ort, String> {
    save_with(&Schluesselbund::new(account), data_dir, account, wert)
}

/// [`stored_with`] im Schlüsselbund des Systems.
pub fn stored(data_dir: &Path, account: &str) -> Option<Ort> {
    stored_with(&Schluesselbund::new(account), data_dir, account)
}

/// [`clear_with`] im Schlüsselbund des Systems.
pub fn clear(data_dir: &Path, account: &str) -> Result<(), String> {
    clear_with(&Schluesselbund::new(account), data_dir, account)
}

/// [`reveal_with`] im Schlüsselbund des Systems.
pub fn reveal(data_dir: &Path, account: &str) -> Option<String> {
    reveal_with(&Schluesselbund::new(account), data_dir, account)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Mem {
        v: RefCell<Option<String>>,
        kaputt: bool,
        schreib_kaputt: bool,
    }

    impl Speicher for Mem {
        fn lesen(&self) -> Result<Option<String>, String> {
            if self.kaputt {
                return Err("kein Dienst".into());
            }
            Ok(self.v.borrow().clone())
        }
        fn schreiben(&self, wert: &str) -> Result<(), String> {
            if self.kaputt || self.schreib_kaputt {
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
    fn account_names_are_file_safe() {
        assert!(valid_account("mcp-token"));
        assert!(valid_account("ki-anthropic"));
        assert!(!valid_account(""));
        assert!(!valid_account("../x"));
        assert!(!valid_account("A"));
        assert!(!valid_account("a b"));
        assert!(!valid_account(&"a".repeat(65)));
        let m = Mem::default();
        let tmp = tempfile::tempdir().unwrap();
        assert!(save_with(&m, tmp.path(), "../x", "w").is_err());
    }

    #[test]
    fn fallback_save_is_not_shadowed_by_old_keyring_value() {
        let tmp = tempfile::tempdir().unwrap();
        let kb = Mem {
            v: RefCell::new(Some("alt".into())),
            schreib_kaputt: true,
            ..Mem::default()
        };
        assert_eq!(
            save_with(&kb, tmp.path(), "mcp-token", "neu").unwrap(),
            Ort::Datei
        );
        assert_eq!(
            reveal_with(&kb, tmp.path(), "mcp-token").as_deref(),
            Some("neu")
        );
    }

    #[test]
    fn entries_are_independent_and_fall_back_to_0600_files() {
        let tmp = tempfile::tempdir().unwrap();
        let kaputt = Mem {
            kaputt: true,
            ..Mem::default()
        };
        assert_eq!(
            save_with(&kaputt, tmp.path(), "mcp-token", "eins").unwrap(),
            Ort::Datei
        );
        assert_eq!(
            save_with(&kaputt, tmp.path(), "ki-openai", "zwei").unwrap(),
            Ort::Datei
        );
        assert_eq!(
            reveal_with(&kaputt, tmp.path(), "mcp-token").as_deref(),
            Some("eins")
        );
        assert_eq!(
            reveal_with(&kaputt, tmp.path(), "ki-openai").as_deref(),
            Some("zwei")
        );
        assert!(tmp.path().join("ki-openai.pass").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(fallback_path(tmp.path(), "mcp-token"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        clear_with(&kaputt, tmp.path(), "mcp-token").unwrap();
        assert_eq!(stored_with(&kaputt, tmp.path(), "mcp-token"), None);
        assert_eq!(
            stored_with(&kaputt, tmp.path(), "ki-openai"),
            Some(Ort::Datei)
        );
    }
}
