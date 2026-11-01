//! Zusatzbänke, die beim ersten Gebrauch geladen werden (Paket J7, Issue #271).
//!
//! Der Katalog (`docs/sounds/banks.tsv`) nennt je Bank Datei, Grösse, SHA-256, URL und
//! Lizenz; `packaging/fetch-soundfont.sh` liest dieselbe Datei. Geladen wird nur über
//! https, ohne Weiterleitungen, in eine `.part`-Datei, die erst nach stimmender Prüfsumme
//! ihren Namen bekommt. Liegt eine Bank im Bank-Ordner, ist sie also geprüft. Die Engine
//! lädt nie selbst: Oberfläche und MCP rufen [`ensure`], das in einem eigenen Thread lädt.
//! Bis die Bank da ist (oder ohne Netz), spielt GeneralUser GS
//! ([`crate::soundfont::soundfont_for`]).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use sha2::{Digest, Sha256};
use woa_model::SoundBank;

const CATALOG: &str = include_str!("../../../docs/sounds/banks.tsv");

/// Ein Eintrag des Katalogs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BankEntry {
    pub bank: SoundBank,
    /// Dateiname im Bank-Ordner.
    pub file: String,
    /// Genaue Grösse in Bytes.
    pub bytes: u64,
    /// SHA-256, klein geschrieben.
    pub sha256: String,
    /// Download-Adresse, nur https.
    pub url: String,
    /// Lizenzdatei, die neben die Bank gelegt wird.
    pub license_file: String,
}

/// Fehler beim Laden einer Bank.
#[derive(Debug)]
pub enum BankError {
    /// Die URL ist nicht https.
    NotHttps,
    /// Kein Bank-Ordner gesetzt ([`set_bank_dir`]).
    NoDir,
    /// Netz oder HTTP-Status.
    Net(String),
    /// Prüfsumme oder Grösse stimmen nicht.
    Hash {
        expected: String,
        got: String,
    },
    Io(std::io::Error),
}

impl std::fmt::Display for BankError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotHttps => write!(f, "Download nur über https"),
            Self::NoDir => write!(f, "kein Datenordner für Bänke"),
            Self::Net(e) => write!(f, "Download fehlgeschlagen: {e}"),
            Self::Hash { expected, got } => {
                write!(f, "Prüfsumme falsch (erwartet {expected}, erhalten {got})")
            }
            Self::Io(e) => write!(f, "Datei: {e}"),
        }
    }
}

impl std::error::Error for BankError {}

impl From<std::io::Error> for BankError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

fn parse(tsv: &str) -> Vec<BankEntry> {
    tsv.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            let [id, file, bytes, sha, url, license] = c.as_slice() else {
                tracing::warn!(line = l, "bank catalog line malformed");
                return None;
            };
            Some(BankEntry {
                bank: SoundBank::from_id(id)?,
                file: (*file).to_owned(),
                bytes: bytes.parse().ok()?,
                sha256: sha.to_ascii_lowercase(),
                url: (*url).to_owned(),
                license_file: (*license).to_owned(),
            })
        })
        .collect()
}

/// Der Katalog aller bekannten Bänke.
pub fn catalog() -> &'static [BankEntry] {
    static CAT: OnceLock<Vec<BankEntry>> = OnceLock::new();
    CAT.get_or_init(|| parse(CATALOG))
}

/// Der Katalogeintrag einer Bank; `None` für [`SoundBank::Unknown`].
pub fn entry(bank: SoundBank) -> Option<&'static BankEntry> {
    catalog().iter().find(|e| e.bank == bank)
}

/// Lizenztext, der neben die Bank gelegt wird (Volltext aus `docs/sounds/`).
pub fn license_text(bank: SoundBank) -> &'static str {
    match bank {
        SoundBank::MusescoreGeneral => {
            include_str!("../../../docs/sounds/MuseScore_General-LICENSE.md")
        }
        SoundBank::AvlRedZeppelin => include_str!("../../../docs/sounds/AVL-Drumkits-LICENSE.txt"),
        SoundBank::Unknown => "",
    }
}

static DIR: OnceLock<PathBuf> = OnceLock::new();

/// Setzt den Bank-Ordner des Prozesses (App: `<Datenordner>/banks`). Nur der erste Aufruf gilt.
pub fn set_bank_dir(dir: PathBuf) {
    let _ = DIR.set(dir);
}

/// Der Bank-Ordner des Prozesses, falls gesetzt.
pub fn bank_dir() -> Option<&'static Path> {
    DIR.get().map(PathBuf::as_path)
}

/// Pfad der Bank in `dir`.
pub fn bank_path(dir: &Path, entry: &BankEntry) -> PathBuf {
    dir.join(&entry.file)
}

/// Pfad der installierten Bank des Prozesses, wenn sie da ist.
pub fn installed(bank: SoundBank) -> Option<PathBuf> {
    let path = bank_path(bank_dir()?, entry(bank)?);
    path.is_file().then_some(path)
}

/// Schreibt `reader` als Bank nach `dir`, prüft Grösse und SHA-256 und legt die Lizenz
/// daneben. Bei falscher Prüfsumme bleibt nichts liegen. `progress` bekommt die gelesenen Bytes.
pub fn install(
    entry: &BankEntry,
    mut reader: impl Read,
    dir: &Path,
    mut progress: impl FnMut(u64),
) -> Result<PathBuf, BankError> {
    std::fs::create_dir_all(dir)?;
    let dest = bank_path(dir, entry);
    let part = dir.join(format!("{}.part", entry.file));
    let result = (|| {
        let mut out = std::io::BufWriter::new(std::fs::File::create(&part)?);
        let mut hash = Sha256::new();
        let mut buf = vec![0u8; 256 * 1024];
        let mut done = 0u64;
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            done += n as u64;
            if done > entry.bytes {
                return Err(BankError::Hash {
                    expected: entry.sha256.clone(),
                    got: format!("mehr als {} Bytes", entry.bytes),
                });
            }
            hash.update(&buf[..n]);
            out.write_all(&buf[..n])?;
            progress(done);
        }
        out.flush()?;
        let got: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if done != entry.bytes || got != entry.sha256 {
            return Err(BankError::Hash {
                expected: entry.sha256.clone(),
                got,
            });
        }
        Ok(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    std::fs::write(dir.join(&entry.license_file), license_text(entry.bank))?;
    std::fs::rename(&part, &dest)?;
    Ok(dest)
}

/// Lädt die Bank von ihrer URL nach `dir` und prüft sie ([`install`]). Blockiert; nur aus
/// einem eigenen Thread rufen. Nur https mit Zertifikatsprüfung, keine Weiterleitungen.
pub fn download(
    entry: &BankEntry,
    dir: &Path,
    progress: impl FnMut(u64),
) -> Result<PathBuf, BankError> {
    if !entry.url.starts_with("https://") {
        return Err(BankError::NotHttps);
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(20)))
        // Ein hängender Download gibt nach einer Stunde auf; `ensure` versucht es dann erneut.
        .timeout_global(Some(Duration::from_secs(3600)))
        .https_only(true)
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into();
    let mut resp = agent
        .get(&entry.url)
        .call()
        .map_err(|e| BankError::Net(e.to_string()))?;
    let status = resp.status().as_u16();
    if status != 200 {
        return Err(BankError::Net(format!("HTTP {status}")));
    }
    let reader = resp
        .body_mut()
        .with_config()
        .limit(entry.bytes + 1)
        .reader();
    install(entry, reader, dir, progress)
}

/// Zustand einer Bank im Prozess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BankState {
    /// Nicht da; [`ensure`] lädt sie.
    Missing,
    /// Wird geladen.
    Downloading { done: u64, total: u64 },
    /// Liegt geprüft im Bank-Ordner.
    Ready,
    /// Letzter Versuch gescheitert; [`ensure`] versucht es erneut. Bis dahin GeneralUser GS.
    Failed(String),
}

impl BankState {
    /// Kurzform für MCP: `missing`, `downloading`, `ready`, `failed`.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Downloading { .. } => "downloading",
            Self::Ready => "ready",
            Self::Failed(_) => "failed",
        }
    }
}

fn states() -> &'static Mutex<HashMap<SoundBank, BankState>> {
    static S: OnceLock<Mutex<HashMap<SoundBank, BankState>>> = OnceLock::new();
    S.get_or_init(Mutex::default)
}

fn set_state(bank: SoundBank, state: BankState) {
    if let Ok(mut s) = states().lock() {
        s.insert(bank, state);
    }
}

static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Zähler, der bei jeder fertig geladenen Bank steigt; die Oberfläche baut dann den Graphen neu.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Acquire)
}

/// Meldet eine Bank als da (nach [`install`]); der nächste Graph spielt sie.
pub fn mark_ready(bank: SoundBank) {
    set_state(bank, BankState::Ready);
    GENERATION.fetch_add(1, Ordering::AcqRel);
}

/// Zustand der Bank. Liegt die Datei im Bank-Ordner, ist sie `Ready`.
pub fn state(bank: SoundBank) -> BankState {
    let known = states().lock().ok().and_then(|s| s.get(&bank).cloned());
    match known {
        Some(s @ (BankState::Downloading { .. } | BankState::Ready)) => s,
        other if installed(bank).is_some() => {
            let _ = other;
            BankState::Ready
        }
        Some(s) => s,
        None => BankState::Missing,
    }
}

/// Erster Gebrauch: Fehlt die Bank, lädt ein eigener Thread sie (nur einer je Bank).
/// Gibt den Zustand danach zurück. Für [`SoundBank::Unknown`] `Failed`.
pub fn ensure(bank: SoundBank) -> BankState {
    let now = state(bank);
    if !matches!(now, BankState::Missing | BankState::Failed(_)) {
        return now;
    }
    let Some(entry) = entry(bank) else {
        return BankState::Failed("unbekannte Bank".into());
    };
    let Some(dir) = bank_dir() else {
        let s = BankState::Failed(BankError::NoDir.to_string());
        set_state(bank, s.clone());
        return s;
    };
    let total = entry.bytes;
    let start = BankState::Downloading { done: 0, total };
    set_state(bank, start.clone());
    let spawned = std::thread::Builder::new()
        .name(format!("bank-{}", bank.id()))
        .spawn(move || {
            tracing::info!(bank = bank.id(), url = %entry.url, "bank download started");
            let mut last = 0u64;
            let r = download(entry, dir, |done| {
                if done - last >= 1 << 20 || done == total {
                    last = done;
                    set_state(bank, BankState::Downloading { done, total });
                }
            });
            match r {
                Ok(path) => {
                    tracing::info!(bank = bank.id(), path = %path.display(), "bank installed");
                    // Hier einlesen, nicht beim nächsten Graphen im Oberflächen-Thread.
                    let _ = crate::soundfont::bank_font(bank);
                    mark_ready(bank);
                }
                Err(e) => {
                    tracing::warn!(bank = bank.id(), error = %e, "bank download failed, GeneralUser GS plays");
                    set_state(bank, BankState::Failed(e.to_string()));
                }
            }
        });
    if let Err(e) = spawned {
        let s = BankState::Failed(e.to_string());
        set_state(bank, s.clone());
        return s;
    }
    start
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_and_unknown_lines_are_skipped() {
        let tsv =
            "# x\nmusescore-general\ta.sf2\t10\tAB\thttps://x/a\tL\nnope\tb\t1\tc\td\te\nkaputt\n";
        let cat = parse(tsv);
        assert_eq!(cat.len(), 1);
        assert_eq!(cat[0].sha256, "ab");
        assert_eq!(cat[0].bank, SoundBank::MusescoreGeneral);
    }
}
