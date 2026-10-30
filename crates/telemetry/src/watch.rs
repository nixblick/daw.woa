//! Hänger der Oberfläche messen, ohne Inhalte: nur Dauer in ms.
//!
//! Die GUI meldet Anfang und Ende jedes Frames ([`FrameWatch::frame`]). Daraus entstehen:
//! - `ui.frame_slow`: längster Frame ab [`SLOW_MS`], höchstens einmal je [`REPORT_EVERY`].
//! - `ui.hang_open`: ein Frame läuft seit [`HANG_MS`] und mehr, gemeldet aus einem Wächter-
//!   Thread, solange er hängt (sonst sähe man eingefrorene oder abgeschossene Fenster nie).
//!
//! Leerlauf zählt nicht: egui zeichnet ohne Eingabe keine Frames, gemessen wird nur, was
//! zwischen Anfang und Ende eines Frames liegt.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Ab dieser Dauer gilt ein Frame als langsam (spürbares Ruckeln).
pub const SLOW_MS: u64 = 100;
/// Ab dieser Dauer gilt ein laufender Frame als Hänger.
pub const HANG_MS: u64 = 2000;
/// Langsame Frames werden gesammelt und höchstens so oft gemeldet.
pub const REPORT_EVERY: Duration = Duration::from_secs(1);

struct Inner {
    /// Start des laufenden Frames in ms seit `epoch`, 0 = kein Frame läuft.
    busy_since: AtomicU64,
    epoch: Instant,
    /// Längster langsamer Frame seit der letzten Meldung und Zeit dieser Meldung.
    slow: Mutex<(u64, Instant)>,
}

/// Misst die Frames eines Fensters; einmal je App anlegen.
pub struct FrameWatch(Arc<Inner>);

impl FrameWatch {
    /// Startet den Wächter-Thread für offene Hänger.
    pub fn new() -> Self {
        let inner = Arc::new(Inner {
            busy_since: AtomicU64::new(0),
            epoch: Instant::now(),
            slow: Mutex::new((0, Instant::now())),
        });
        let w = inner.clone();
        let _ = std::thread::Builder::new()
            .name("woa-ui-watch".into())
            .spawn(move || watchdog(&w.busy_since, w.epoch));
        Self(inner)
    }

    /// Markiert den Beginn eines Frames; das Ende misst der Rückgabewert beim Drop.
    pub fn frame(&self) -> FrameGuard {
        let now = ms_since(self.0.epoch).max(1);
        self.0.busy_since.store(now, Ordering::Relaxed);
        FrameGuard {
            inner: self.0.clone(),
            start: Instant::now(),
        }
    }
}

impl Inner {
    fn end(&self, took: Duration) {
        self.busy_since.store(0, Ordering::Relaxed);
        let ms = took.as_millis() as u64;
        let Ok(mut slow) = self.slow.lock() else {
            return;
        };
        if ms >= SLOW_MS {
            slow.0 = slow.0.max(ms);
        }
        if slow.0 > 0 && slow.1.elapsed() >= REPORT_EVERY {
            tracing::info!(art = "ui.frame_slow", wert = slow.0, "slow frame");
            *slow = (0, Instant::now());
        }
    }
}

impl Default for FrameWatch {
    fn default() -> Self {
        Self::new()
    }
}

/// Beendet beim Drop den Frame, den [`FrameWatch::frame`] begonnen hat.
pub struct FrameGuard {
    inner: Arc<Inner>,
    start: Instant,
}

impl Drop for FrameGuard {
    fn drop(&mut self) {
        self.inner.end(self.start.elapsed());
    }
}

fn ms_since(epoch: Instant) -> u64 {
    epoch.elapsed().as_millis() as u64
}

/// Meldet einen laufenden Hänger einmal ab [`HANG_MS`] und danach alle 10 s erneut.
fn watchdog(busy_since: &AtomicU64, epoch: Instant) {
    let mut reported_for = 0u64;
    let mut next_at = HANG_MS;
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let start = busy_since.load(Ordering::Relaxed);
        if start == 0 {
            reported_for = 0;
            next_at = HANG_MS;
            continue;
        }
        if start != reported_for {
            reported_for = start;
            next_at = HANG_MS;
        }
        let open = ms_since(epoch).saturating_sub(start);
        if open >= next_at {
            tracing::warn!(art = "ui.hang_open", wert = open, "ui frame hangs");
            next_at = open + 10_000;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_between_frames_is_not_a_hang() {
        let w = FrameWatch::new();
        drop(w.frame());
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(w.0.busy_since.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_slow_frame_is_remembered_until_reported() {
        let w = FrameWatch::new();
        w.0.end(Duration::from_millis(SLOW_MS + 50));
        assert_eq!(w.0.slow.lock().unwrap().0, SLOW_MS + 50);
        w.0.end(Duration::from_millis(5));
        assert_eq!(w.0.slow.lock().unwrap().0, SLOW_MS + 50);
    }
}
