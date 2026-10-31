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
/// So oft geht eine Zusammenfassung raus (Frames, Speicher, Laufzeit), passend zum Upload.
pub const SUMMARY_EVERY: Duration = Duration::from_secs(120);

/// Frame-Statistik seit der letzten Zusammenfassung.
#[derive(Default)]
struct Stats {
    frames: u64,
    total_ms: u64,
    max_ms: u64,
    slow: u64,
}

struct Inner {
    /// Start des laufenden Frames in ms seit `epoch`, 0 = kein Frame läuft.
    busy_since: AtomicU64,
    epoch: Instant,
    /// Längster langsamer Frame seit der letzten Meldung und Zeit dieser Meldung.
    slow: Mutex<(u64, Instant)>,
    stats: Mutex<(Stats, Instant)>,
    started: Instant,
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
            stats: Mutex::new((Stats::default(), Instant::now())),
            started: Instant::now(),
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

    /// Schickt die Zusammenfassung, wenn [`SUMMARY_EVERY`] um ist; `true` heißt, die App
    /// soll jetzt ihre eigenen Kennzahlen dazulegen (siehe [`metric`]).
    pub fn summary_due(&self) -> bool {
        let Ok(mut st) = self.0.stats.lock() else {
            return false;
        };
        if st.1.elapsed() < SUMMARY_EVERY {
            return false;
        }
        let (s, since) = std::mem::replace(&mut *st, (Stats::default(), Instant::now()));
        drop(st);
        let secs = since.elapsed().as_secs().max(1);
        let m = Summary {
            frames: s.frames,
            fps: s.frames / secs,
            avg_ms: s.total_ms.checked_div(s.frames).unwrap_or(0),
            max_ms: s.max_ms,
            slow: s.slow,
            uptime_s: self.0.started.elapsed().as_secs(),
            rss_mb: rss_mb(),
            threads: thread_count(),
        };
        m.emit();
        true
    }
}

/// Ziel der Einzelkennzahlen: nur der Upload sieht sie, Datei und Protokoll bekommen die
/// eine Zeile `ui summary`.
pub const METRIC_TARGET: &str = "woa_metric";

/// Eine Kennzahl als Upload-Ereignis (`art`, Zahl), Stufe info.
pub fn metric(art: &'static str, wert: u64) {
    tracing::info!(target: METRIC_TARGET, art, wert, "metric");
}

/// Zusammenfassung eines Zeitraums: eine Logzeile, dazu je Wert ein Upload-Ereignis.
struct Summary {
    frames: u64,
    fps: u64,
    avg_ms: u64,
    max_ms: u64,
    slow: u64,
    uptime_s: u64,
    rss_mb: Option<u64>,
    threads: Option<u64>,
}

impl Summary {
    fn emit(&self) {
        tracing::info!(
            frames = self.frames,
            fps = self.fps,
            avg_ms = self.avg_ms,
            max_ms = self.max_ms,
            slow = self.slow,
            uptime_s = self.uptime_s,
            rss_mb = ?self.rss_mb,
            threads = ?self.threads,
            "ui summary"
        );
        metric("ui.frames", self.frames);
        metric("ui.fps", self.fps);
        metric("ui.frame_avg_ms", self.avg_ms);
        metric("ui.frame_max_ms", self.max_ms);
        metric("ui.frames_slow", self.slow);
        metric("app.uptime_s", self.uptime_s);
        if let Some(mb) = self.rss_mb {
            metric("proc.rss_mb", mb);
        }
        if let Some(n) = self.threads {
            metric("proc.threads", n);
        }
    }
}

/// Erste Zahl hinter `key` in einem `/proc/self/status`-Text (z. B. `VmRSS:  1234 kB`).
fn status_field(status: &str, key: &str) -> Option<u64> {
    status
        .lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|v| v.split_whitespace().next())
        .and_then(|v| v.parse().ok())
}

/// Belegter Arbeitsspeicher in MB aus `VmRSS` (kB, unabhängig von der Seitengröße).
fn rss_mb_from(status: &str) -> Option<u64> {
    status_field(status, "VmRSS:").map(|kb| kb / 1024)
}

/// Belegter Arbeitsspeicher in MB (Linux über `/proc`, sonst `None`).
fn rss_mb() -> Option<u64> {
    rss_mb_from(&std::fs::read_to_string("/proc/self/status").ok()?)
}

/// Anzahl Threads des Prozesses (Linux über `/proc`, sonst `None`).
fn thread_count() -> Option<u64> {
    status_field(
        &std::fs::read_to_string("/proc/self/status").ok()?,
        "Threads:",
    )
}

impl Inner {
    fn end(&self, took: Duration) {
        self.busy_since.store(0, Ordering::Relaxed);
        let ms = took.as_millis() as u64;
        if let Ok(mut st) = self.stats.lock() {
            st.0.frames += 1;
            st.0.total_ms += ms;
            st.0.max_ms = st.0.max_ms.max(ms);
            st.0.slow += u64::from(ms >= SLOW_MS);
        }
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
    fn rss_from_status_in_kb() {
        let st = "Name:\twoa\nVmRSS:\t  204800 kB\nThreads:\t7\n";
        assert_eq!(rss_mb_from(st), Some(200));
        assert_eq!(status_field(st, "Threads:"), Some(7));
        assert_eq!(rss_mb_from("Name: x\n"), None);
    }

    #[test]
    fn idle_between_frames_is_not_a_hang() {
        let w = FrameWatch::new();
        drop(w.frame());
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(w.0.busy_since.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn summary_is_one_line_in_file_and_journal() {
        use crate::{Journal, JournalLayer, journal_filter, noise_filter};
        use tracing_subscriber::Layer;
        use tracing_subscriber::layer::SubscriberExt;
        static J: std::sync::OnceLock<Journal> = std::sync::OnceLock::new();
        let j = J.get_or_init(Journal::default);
        struct Count(std::sync::Arc<std::sync::atomic::AtomicUsize>);
        impl<S: tracing::Subscriber> Layer<S> for Count {
            fn on_event(
                &self,
                _e: &tracing::Event<'_>,
                _c: tracing_subscriber::layer::Context<'_, S>,
            ) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }
        let n = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sub = tracing_subscriber::registry()
            .with(JournalLayer::new(j).with_filter(journal_filter()))
            .with(Count(n.clone()).with_filter(noise_filter()));
        tracing::subscriber::with_default(sub, || {
            Summary {
                frames: 10,
                fps: 1,
                avg_ms: 2,
                max_ms: 3,
                slow: 0,
                uptime_s: 120,
                rss_mb: Some(80),
                threads: Some(12),
            }
            .emit();
        });
        assert_eq!(j.len(), 1);
        assert_eq!(n.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn summary_waits_for_its_period() {
        let w = FrameWatch::new();
        drop(w.frame());
        assert!(!w.summary_due());
        assert_eq!(w.0.stats.lock().unwrap().0.frames, 1);
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
