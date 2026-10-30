//! Update-Hinweis: Zustand und Hintergrund-Threads (fensterunabhängig, testbar).
//!
//! Netzzugriff läuft nie im UI-Thread: Suche und Download sind eigene Threads, die ihr
//! Ergebnis über einen Kanal melden. [`UpdateState::poll`] holt es jedes Bild ab.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use woa_updater::{Available, UpdateError, Veto};

/// Höchstens einmal am Tag automatisch suchen.
pub const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

/// Ist die automatische Suche fällig? Eine Uhr, die rückwärts ging, zählt als fällig.
pub fn check_due(last: Option<u64>, now: u64) -> bool {
    match last {
        None => true,
        Some(t) => now < t || now - t >= CHECK_INTERVAL_SECS,
    }
}

/// Unix-Sekunden jetzt.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// Nichts zu zeigen.
    Idle,
    /// Suche läuft.
    Checking,
    /// Neuere Version gefunden, wartet auf den Klick.
    Available(Available),
    /// Paket wird geladen und geprüft.
    Downloading(Available),
    /// Geprüftes Paket liegt bereit (Windows startet danach den Installer).
    Ready(Available, PathBuf),
    /// Installation ging schief, mit Grund. Das Update bleibt wählbar.
    Failed(Available, String),
}

/// Was `poll` der Oberfläche meldet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    None,
    /// Suche beendet. `ok`: Antwort kam an (auch „aktuell“); dann den Zeitstempel merken.
    /// `message` ist ein Satz für die Statuszeile (nur bei manueller Suche gezeigt).
    Checked {
        ok: bool,
        message: String,
    },
    /// Geprüftes MSI liegt unter diesem Pfad.
    Downloaded(PathBuf),
}

enum Msg {
    Checked(Result<Option<Available>, UpdateError>),
    Downloaded(Result<PathBuf, UpdateError>),
}

pub struct UpdateState {
    pub phase: Phase,
    /// Für das Wort „Später“: bis zum Neustart ausgeblendet.
    pub dismissed: bool,
    manual: bool,
    rx: Option<Receiver<Msg>>,
}

impl Default for UpdateState {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            dismissed: false,
            manual: false,
            rx: None,
        }
    }
}

impl UpdateState {
    /// War die laufende Suche vom Nutzer ausgelöst? Nur dann melden Fehler sich laut.
    pub fn was_manual(&self) -> bool {
        self.manual
    }

    pub fn busy(&self) -> bool {
        self.rx.is_some()
    }

    /// Startet die Suche im Hintergrund. Läuft schon etwas, passiert nichts.
    pub fn start_check(&mut self, manual: bool, wake: impl Fn() + Send + 'static) {
        if self.busy() {
            return;
        }
        self.manual = manual;
        self.phase = Phase::Checking;
        let (tx, rx) = channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let r = woa_updater::check(env!("CARGO_PKG_VERSION"));
            let _ = tx.send(Msg::Checked(r));
            wake();
        });
    }

    /// Startet Download und Prüfung im Hintergrund (nur aus `Available`/`Failed`).
    pub fn start_download(&mut self, wake: impl Fn() + Send + 'static) {
        if self.busy() {
            return;
        }
        let (Phase::Available(a) | Phase::Failed(a, _)) = self.phase.clone() else {
            return;
        };
        self.phase = Phase::Downloading(a.clone());
        let (tx, rx) = channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let r = woa_updater::download_and_verify(&a, env!("CARGO_PKG_VERSION"));
            let _ = tx.send(Msg::Downloaded(r));
            wake();
        });
    }

    /// Holt ein fertiges Ergebnis ab (nicht blockierend).
    pub fn poll(&mut self) -> Event {
        let Some(rx) = &self.rx else {
            return Event::None;
        };
        let msg = match rx.try_recv() {
            Ok(m) => m,
            Err(TryRecvError::Empty) => return Event::None,
            Err(TryRecvError::Disconnected) => {
                // Thread ist ohne Antwort gestorben: nicht hängen bleiben.
                self.rx = None;
                self.fail_download("Thread abgebrochen");
                return Event::Checked {
                    ok: false,
                    message: "Update-Suche abgebrochen".into(),
                };
            }
        };
        self.rx = None;
        match msg {
            Msg::Checked(r) => self.finish_check(r),
            Msg::Downloaded(Ok(path)) => {
                if let Phase::Downloading(a) = self.phase.clone() {
                    self.phase = Phase::Ready(a, path.clone());
                }
                Event::Downloaded(path)
            }
            Msg::Downloaded(Err(e)) => {
                tracing::warn!(error = %e, "update download refused");
                self.fail_download(&e.to_string());
                Event::None
            }
        }
    }

    fn fail_download(&mut self, why: &str) {
        if let Phase::Downloading(a) | Phase::Ready(a, _) = self.phase.clone() {
            self.phase = Phase::Failed(a, why.to_string());
        } else if self.phase == Phase::Checking {
            self.phase = Phase::Idle;
        }
    }

    /// Installation scheiterte nach dem Download (z. B. Installer nicht startbar).
    pub fn install_failed(&mut self, why: &str) {
        self.fail_download(why);
    }

    /// Zurück zu „verfügbar“ (z. B. weil erst gespeichert werden muss).
    pub fn back_to_available(&mut self) {
        if let Phase::Downloading(a) | Phase::Ready(a, _) | Phase::Failed(a, _) = self.phase.clone()
        {
            self.phase = Phase::Available(a);
        }
    }

    fn finish_check(&mut self, r: Result<Option<Available>, UpdateError>) -> Event {
        match r {
            Ok(Some(a)) => {
                tracing::info!(version = %a.version, "update available");
                let message = format!("Update {} verfügbar", a.version);
                self.phase = Phase::Available(a);
                Event::Checked { ok: true, message }
            }
            Ok(None) => {
                self.phase = Phase::Idle;
                Event::Checked {
                    ok: true,
                    message: "WOA ist aktuell".into(),
                }
            }
            // Keine neuere Version (oder ein zurückgespieltes Manifest): Antwort kam an,
            // installiert wird nichts.
            Err(UpdateError::Veto(v)) => {
                tracing::warn!(veto = %v, "update manifest vetoed");
                self.phase = Phase::Idle;
                let message = match v {
                    Veto::Downgrade => "WOA ist aktuell (Manifest älter)".to_string(),
                    other => format!("Kein Update: {other}"),
                };
                Event::Checked { ok: true, message }
            }
            Err(e) => {
                tracing::info!(error = %e, "update check failed");
                self.phase = Phase::Idle;
                Event::Checked {
                    ok: false,
                    message: format!("Update-Suche fehlgeschlagen: {e}"),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn avail() -> Available {
        Available {
            version: "9.9.9".into(),
            notes: String::new(),
            msi_url: "https://example.org/a.msi".into(),
            msi_sig_url: "https://example.org/a.sig".into(),
            msi_sha256: String::new(),
        }
    }

    #[test]
    fn check_is_due_once_a_day() {
        assert!(check_due(None, 1_000));
        assert!(!check_due(Some(1_000), 1_000 + CHECK_INTERVAL_SECS - 1));
        assert!(check_due(Some(1_000), 1_000 + CHECK_INTERVAL_SECS));
        // Uhr ging zurück: lieber einmal zu viel fragen.
        assert!(check_due(Some(5_000), 1_000));
    }

    #[test]
    fn found_update_becomes_available() {
        let mut s = UpdateState::default();
        let e = s.finish_check(Ok(Some(avail())));
        assert!(matches!(e, Event::Checked { ok: true, .. }));
        assert_eq!(s.phase, Phase::Available(avail()));
    }

    #[test]
    fn up_to_date_and_veto_leave_nothing_to_show() {
        let mut s = UpdateState::default();
        assert!(matches!(
            s.finish_check(Ok(None)),
            Event::Checked { ok: true, .. }
        ));
        assert_eq!(s.phase, Phase::Idle);
        assert!(matches!(
            s.finish_check(Err(Veto::Downgrade.into())),
            Event::Checked { ok: true, .. }
        ));
        assert_eq!(s.phase, Phase::Idle);
    }

    #[test]
    fn network_error_is_not_ok_so_it_is_retried_next_start() {
        let mut s = UpdateState::default();
        let e = s.finish_check(Err(UpdateError::Net("offline".into())));
        assert!(matches!(e, Event::Checked { ok: false, .. }));
        assert_eq!(s.phase, Phase::Idle);
    }

    #[test]
    fn not_configured_does_no_harm() {
        let mut s = UpdateState::default();
        let e = s.finish_check(Err(UpdateError::NotConfigured));
        assert_eq!(
            e,
            Event::Checked {
                ok: false,
                message: "Update-Suche fehlgeschlagen: Updates nicht eingerichtet".into()
            }
        );
    }

    #[test]
    fn failed_install_stays_selectable() {
        let mut s = UpdateState {
            phase: Phase::Downloading(avail()),
            ..UpdateState::default()
        };
        s.install_failed("kaputt");
        assert_eq!(s.phase, Phase::Failed(avail(), "kaputt".into()));
        s.back_to_available();
        assert_eq!(s.phase, Phase::Available(avail()));
    }

    #[test]
    fn poll_without_work_is_quiet() {
        let mut s = UpdateState::default();
        assert_eq!(s.poll(), Event::None);
        assert!(!s.busy());
    }
}
