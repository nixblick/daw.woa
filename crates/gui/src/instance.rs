//! Nur ein WOA je Datenordner (Fassung 8, Punkt 0a).
//!
//! Ein zweiter Start öffnete bisher Audio, MIDI und Stream ein zweites Mal und störte so eine
//! laufende Sendung (05.11.2026). Jetzt hält der erste Start eine Sperre auf `woa.lock`
//! (Betriebssystem-Sperre, fällt bei Absturz weg, unter Windows und Linux gleich) und lauscht auf
//! `127.0.0.1` an einem freien Port, der in `woa.port` steht. Ein zweiter Start findet die Sperre
//! belegt, sagt dem ersten „show“ und endet, bevor irgendein Gerät geöffnet ist.
//! Nur `std`, keine neue Abhängigkeit.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const LOCK_FILE: &str = "woa.lock";
const PORT_FILE: &str = "woa.port";
const SHOW: &[u8] = b"show";

/// Ergebnis von [`acquire`].
pub enum Instance {
    /// Dieser Start ist der erste: Sperre halten, solange die App läuft.
    Primary(Primary),
    /// Ein WOA läuft schon; es wurde nach vorn geholt (`true`) oder war nicht erreichbar.
    Secondary { woken: bool },
}

/// Sperre und Weckruf des ersten Starts. Der Port wird in einem eigenen Thread bedient,
/// damit eine hängende Verbindung nie die Oberfläche blockiert.
pub struct Primary {
    _lock: Option<File>,
    woken: Arc<AtomicBool>,
}

impl Primary {
    /// `true`, wenn seit dem letzten Aufruf ein zweiter Start „show“ geschickt hat.
    /// Blockiert nie.
    pub fn poll_show(&self) -> bool {
        self.woken.swap(false, Ordering::Relaxed)
    }
}

fn serve(l: TcpListener, woken: Arc<AtomicBool>) {
    for s in l.incoming() {
        let Ok(mut s) = s else { continue };
        let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
        let mut buf = [0u8; 8];
        if let Ok(n) = s.read(&mut buf)
            && buf.get(..n) == Some(SHOW)
        {
            woken.store(true, Ordering::Relaxed);
        }
    }
}

/// Sperre im Datenordner nehmen oder den laufenden WOA wecken.
/// Fehler beim Anlegen der Sperre lassen den Start zu (lieber zwei WOA als keiner).
pub fn acquire(data_dir: &Path) -> Instance {
    if let Err(e) = std::fs::create_dir_all(data_dir) {
        tracing::warn!(error = %e, "single instance: no data dir");
    }
    let lock = match OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(data_dir.join(LOCK_FILE))
    {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!(error = %e, "single instance: lock file not opened");
            return Instance::Primary(Primary {
                _lock: None,
                woken: Arc::default(),
            });
        }
    };
    match lock.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            return Instance::Secondary {
                woken: wake(data_dir),
            };
        }
        Err(std::fs::TryLockError::Error(e)) => {
            tracing::warn!(error = %e, "single instance: lock failed, starting anyway");
        }
    }
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|l| {
            let port = l.local_addr()?.port();
            std::fs::write(data_dir.join(PORT_FILE), port.to_string())?;
            Ok(l)
        })
        .map_err(|e| tracing::warn!(error = %e, "single instance: no wake port"))
        .ok();
    let woken = Arc::<AtomicBool>::default();
    if let Some(l) = listener {
        let w = woken.clone();
        if let Err(e) = std::thread::Builder::new()
            .name("woa-instance".into())
            .spawn(move || serve(l, w))
        {
            tracing::warn!(error = %e, "single instance: no wake thread");
        }
    }
    Instance::Primary(Primary {
        _lock: Some(lock),
        woken,
    })
}

/// Dem laufenden WOA „show“ schicken.
fn wake(data_dir: &Path) -> bool {
    let Some(port) = std::fs::read_to_string(data_dir.join(PORT_FILE))
        .ok()
        .and_then(|s| s.trim().parse::<u16>().ok())
    else {
        return false;
    };
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    TcpStream::connect_timeout(&addr, Duration::from_secs(2))
        .and_then(|mut s| s.write_all(SHOW))
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("woa-instance-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn second_start_wakes_first_and_lock_frees_on_drop() {
        let d = dir("wake");
        let Instance::Primary(first) = acquire(&d) else {
            panic!("first start must be primary");
        };
        assert!(!first.poll_show());
        match acquire(&d) {
            Instance::Secondary { woken } => assert!(woken, "second start reaches the first"),
            Instance::Primary(_) => panic!("second start must not be primary"),
        }
        // Die Verbindung kommt an; ein paar Versuche, ohne feste Wartezeit-Grenze.
        let mut seen = false;
        for _ in 0..200 {
            if first.poll_show() {
                seen = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(seen, "first start gets the show request");
        drop(first);
        assert!(
            matches!(acquire(&d), Instance::Primary(_)),
            "lock free after exit"
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
