//! Wache für Eingänge (#594): Mikrofon, Pult-Eingang und Ducking-Auslöser.
//!
//! Fällt ein USB-Gerät kurz weg, meldet cpal einen Fehler im Strom. Die Ausgänge öffnet WOA
//! seit #291 selbst neu (`OutputWatch`); die Eingänge blieben bis zum Neustart still. Hier
//! dasselbe für Eingänge: Der Fehlerpfad (Audio-Thread) setzt nur ein Flag, der GUI-Takt
//! öffnet neu (1 s, 2 s, 4 s … 16 s) und sagt, was passiert ist.
//!
//! Reine Zeitrechnung mit eingespeistem `now` und Öffner, offline testbar ohne Gerät.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Was ein Takt der Wache getan hat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Poll {
    /// Nichts nötig.
    Idle,
    /// Der Strom ist ausgefallen und wurde wieder geöffnet.
    Reopened,
    /// Neu öffnen ging nicht; `first`: der erste Fehlschlag nach dem Ausfall (jetzt melden).
    Failed { first: bool, error: String },
    /// Nach so vielen Versuchen aufgegeben; die Meldung bleibt stehen.
    GaveUp,
}

/// Wache eines Eingangs.
#[derive(Debug)]
pub struct InputWatch {
    lost: Arc<AtomicBool>,
    retry: Option<Retry>,
}

#[derive(Debug, Clone, Copy)]
struct Retry {
    at: Instant,
    tries: u32,
}

impl Default for InputWatch {
    fn default() -> Self {
        Self {
            lost: Arc::new(AtomicBool::new(false)),
            retry: None,
        }
    }
}

impl InputWatch {
    /// So oft wird neu geöffnet (1+2+4+8+16+16+16 s ≈ 1 min), dann aufgegeben.
    pub const MAX_TRIES: u32 = 7;
    const MAX_WAIT: Duration = Duration::from_secs(16);

    /// Flag für den Fehlerpfad des Stroms (Audio-Thread). Dort nur setzen, wenn
    /// `woa_engine::device::needs_reopen` es verlangt (Aussetzer lassen den Strom laufen),
    /// kein Log (Regel 1).
    pub fn flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.lost)
    }

    /// Versucht die Wache gerade neu zu öffnen?
    pub fn retrying(&self) -> bool {
        self.retry.is_some()
    }

    /// Wache zurücksetzen (Eingang von Hand geschlossen oder neu gewählt).
    pub fn clear(&mut self) {
        self.lost.store(false, Ordering::Release);
        self.retry = None;
    }

    /// Ein Takt. `open`: öffnet den Eingang neu (der alte Strom ist dann schon zu).
    pub fn poll(&mut self, now: Instant, open: impl FnOnce() -> Result<(), String>) -> Poll {
        if self.lost.swap(false, Ordering::AcqRel) && self.retry.is_none() {
            self.retry = Some(Retry { at: now, tries: 0 });
        }
        let Some(r) = self.retry else {
            return Poll::Idle;
        };
        if now < r.at {
            return Poll::Idle;
        }
        match open() {
            Ok(()) => {
                self.retry = None;
                Poll::Reopened
            }
            Err(error) => {
                let tries = r.tries + 1;
                if tries >= Self::MAX_TRIES {
                    self.retry = None;
                    return Poll::GaveUp;
                }
                let wait = Duration::from_secs(1u64 << (tries - 1).min(4)).min(Self::MAX_WAIT);
                self.retry = Some(Retry {
                    at: now + wait,
                    tries,
                });
                Poll::Failed {
                    first: tries == 1,
                    error,
                }
            }
        }
    }
}

/// „Mikro offen, aber kein Signal“ (#594): Ist das Mikro offen und liegt sein Pegel länger
/// als [`NoSignal::AFTER`] unter −70 dBFS, gilt es als stumm.
#[derive(Debug, Default)]
pub struct NoSignal {
    since: Option<Instant>,
    alarm: bool,
}

impl NoSignal {
    pub const AFTER: Duration = Duration::from_secs(10);
    /// −70 dBFS linear.
    pub const SILENT: f32 = 0.000_316;

    pub fn step(&mut self, open: bool, peak: f32, now: Instant) -> bool {
        if open && peak < Self::SILENT {
            let since = *self.since.get_or_insert(now);
            self.alarm = now.duration_since(since) >= Self::AFTER;
        } else {
            self.since = None;
            self.alarm = false;
        }
        self.alarm
    }

    pub fn alarm(&self) -> bool {
        self.alarm
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lose(w: &InputWatch) {
        w.flag().store(true, Ordering::Release);
    }

    #[test]
    fn device_error_is_reopened_on_the_next_tick() {
        let t0 = Instant::now();
        let mut w = InputWatch::default();
        assert_eq!(
            w.poll(t0, || panic!("ohne Ausfall nicht öffnen")),
            Poll::Idle
        );
        lose(&w);
        let mut opened = 0;
        assert_eq!(
            w.poll(t0, || {
                opened += 1;
                Ok(())
            }),
            Poll::Reopened
        );
        assert_eq!(opened, 1);
        assert!(!w.retrying());
        assert_eq!(w.poll(t0, || panic!("nur einmal")), Poll::Idle);
    }

    #[test]
    fn failed_reopen_is_reported_once_retried_with_backoff_and_given_up() {
        let t0 = Instant::now();
        let mut w = InputWatch::default();
        lose(&w);
        let err = || Err("Gerät fehlt".to_string());
        assert_eq!(
            w.poll(t0, err),
            Poll::Failed {
                first: true,
                error: "Gerät fehlt".into()
            }
        );
        // Vor Ablauf der Wartezeit kein Versuch.
        assert_eq!(
            w.poll(t0 + Duration::from_millis(500), || panic!("zu früh")),
            Poll::Idle
        );
        let mut now = t0;
        let mut last = Poll::Idle;
        let mut tries = 1;
        while tries < InputWatch::MAX_TRIES {
            now += Duration::from_secs(17);
            last = w.poll(now, err);
            tries += 1;
            if tries < InputWatch::MAX_TRIES {
                assert!(
                    matches!(last, Poll::Failed { first: false, .. }),
                    "{last:?}"
                );
            }
        }
        assert_eq!(last, Poll::GaveUp);
        assert!(!w.retrying());
    }

    #[test]
    fn device_back_after_a_failure_reopens() {
        let t0 = Instant::now();
        let mut w = InputWatch::default();
        lose(&w);
        assert!(matches!(
            w.poll(t0, || Err("weg".into())),
            Poll::Failed { first: true, .. }
        ));
        assert_eq!(
            w.poll(t0 + Duration::from_secs(2), || Ok(())),
            Poll::Reopened
        );
    }

    #[test]
    fn no_signal_after_ten_seconds_only_while_open() {
        let t0 = Instant::now();
        let mut s = NoSignal::default();
        assert!(!s.step(true, 0.0, t0));
        assert!(!s.step(true, 0.0, t0 + Duration::from_secs(9)));
        assert!(s.step(true, 0.0, t0 + Duration::from_secs(10)));
        assert!(
            !s.step(true, 0.1, t0 + Duration::from_secs(11)),
            "Signal da"
        );
        assert!(
            !s.step(false, 0.0, t0 + Duration::from_secs(30)),
            "Mikro zu"
        );
        assert!(!s.step(false, 0.0, t0 + Duration::from_secs(60)));
    }
}
