//! Wache für Eingänge (#594): Mikrofon, Pult-Eingang und Ducking-Auslöser.
//!
//! Fällt ein USB-Gerät kurz weg, meldet cpal einen Fehler im Strom. Die Ausgänge öffnet WOA
//! seit #291 selbst neu (`OutputWatch`); die Eingänge blieben bis zum Neustart still. Hier
//! dasselbe für Eingänge: Der Fehlerpfad (Audio-Thread) setzt nur ein Flag, der GUI-Takt
//! öffnet neu (1 s, 2 s, 4 s … 16 s) und sagt, was passiert ist. Danach prüft die Wache alle
//! 30 s weiter, bis das Gerät zurück ist (#605); „Übernehmen“ und „Neu verbinden“ stoßen
//! einen Versuch sofort an.
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
    /// Neu öffnen ging nicht; `report`: jetzt melden (erster Fehlschlag nach dem Ausfall,
    /// neuer Grund oder von Hand angestoßen).
    Failed { report: bool, error: String },
    /// Die schnellen Versuche sind vorbei; ab jetzt alle [`InputWatch::SLOW_WAIT`] (#605).
    GaveUp,
}

/// Wache eines Eingangs.
#[derive(Debug)]
pub struct InputWatch {
    lost: Arc<AtomicBool>,
    retry: Option<Retry>,
}

#[derive(Debug, Clone)]
struct Retry {
    at: Instant,
    tries: u32,
    /// Von Hand angestoßen: sofort versuchen und das Ergebnis melden.
    now: bool,
    /// Grund des letzten Fehlschlags (ohne Klammertexte), um einen neuen Grund zu melden.
    reason: Option<String>,
}

impl Default for InputWatch {
    fn default() -> Self {
        Self {
            lost: Arc::new(AtomicBool::new(false)),
            retry: None,
        }
    }
}

/// Wartezeit nach dem `tries`-ten Fehlschlag in der schnellen Phase (Sekunden).
const fn fast_wait(tries: u32) -> u64 {
    let shift = if tries - 1 < 4 { tries - 1 } else { 4 };
    let w = 1u64 << shift;
    if w < 16 { w } else { 16 }
}

/// Summe der Wartezeiten bis zum Aufgeben.
const fn fast_span(max_tries: u32) -> u64 {
    let mut s = 0;
    let mut t = 1;
    while t < max_tries {
        s += fast_wait(t);
        t += 1;
    }
    s
}

/// Grund ohne Text in Klammern (der Fehlertext des Systems wechselt von Versuch zu Versuch).
fn reason(error: &str) -> String {
    let mut out = String::with_capacity(error.len());
    let mut depth = 0u32;
    for c in error.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

impl InputWatch {
    /// So oft wird schnell neu geöffnet (sofort, dann nach 1, 2, 4, 8, 16, 16 s), danach
    /// nur noch alle [`Self::SLOW_WAIT`].
    pub const MAX_TRIES: u32 = 7;
    /// Abstand der Versuche nach der schnellen Phase (#605).
    pub const SLOW_WAIT: Duration = Duration::from_secs(30);
    /// Dauer der schnellen Phase (vom Ausfall bis zum letzten schnellen Versuch, 47 s).
    pub const FAST_SPAN: Duration = Duration::from_secs(fast_span(Self::MAX_TRIES));

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

    /// Nach der schnellen Phase (Versuche nur noch alle [`Self::SLOW_WAIT`])?
    pub fn slow(&self) -> bool {
        self.retry
            .as_ref()
            .is_some_and(|r| r.tries >= Self::MAX_TRIES)
    }

    /// Sofort neu versuchen (Übernehmen, Neu verbinden), statt die Wartezeit abzuwarten; das
    /// Ergebnis wird gemeldet. Ohne Ausfall tut es nichts.
    pub fn retry_now(&mut self) {
        if let Some(r) = &mut self.retry {
            r.now = true;
        }
    }

    /// Wache zurücksetzen (Eingang von Hand geschlossen oder neu gewählt).
    pub fn clear(&mut self) {
        self.lost.store(false, Ordering::Release);
        self.retry = None;
    }

    /// Ein Takt. `open`: öffnet den Eingang neu (der alte Strom ist dann schon zu).
    pub fn poll(&mut self, now: Instant, open: impl FnOnce() -> Result<(), String>) -> Poll {
        if self.lost.swap(false, Ordering::AcqRel) && self.retry.is_none() {
            self.retry = Some(Retry {
                at: now,
                tries: 0,
                now: false,
                reason: None,
            });
        }
        let Some(r) = &mut self.retry else {
            return Poll::Idle;
        };
        if now < r.at && !r.now {
            return Poll::Idle;
        }
        let kicked = std::mem::take(&mut r.now);
        match open() {
            Ok(()) => {
                self.retry = None;
                Poll::Reopened
            }
            Err(error) => {
                r.tries = r.tries.saturating_add(1);
                let why = reason(&error);
                let new_reason = r.reason.as_deref().is_some_and(|old| old != why);
                r.reason = Some(why);
                let report = r.tries == 1 || kicked || new_reason;
                if r.tries >= Self::MAX_TRIES {
                    r.at = now + Self::SLOW_WAIT;
                    if r.tries == Self::MAX_TRIES && !report {
                        return Poll::GaveUp;
                    }
                } else {
                    r.at = now + Duration::from_secs(fast_wait(r.tries));
                }
                Poll::Failed { report, error }
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
                report: true,
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
                    matches!(last, Poll::Failed { report: false, .. }),
                    "{last:?}"
                );
            }
        }
        assert_eq!(last, Poll::GaveUp);
        assert!(w.retrying(), "#605: prüft danach langsam weiter");
    }

    #[test]
    fn device_back_after_a_failure_reopens() {
        let t0 = Instant::now();
        let mut w = InputWatch::default();
        lose(&w);
        assert!(matches!(
            w.poll(t0, || Err("weg".into())),
            Poll::Failed { report: true, .. }
        ));
        assert_eq!(
            w.poll(t0 + Duration::from_secs(2), || Ok(())),
            Poll::Reopened
        );
    }

    /// #605: Nach dem Aufgeben prüft die Wache weiter (alle 30 s) und öffnet, sobald das
    /// Gerät zurück ist. Uhr eingespeist, kein sleep.
    #[test]
    fn after_giving_up_it_keeps_checking_slowly_and_reopens() {
        let t0 = Instant::now();
        let mut w = InputWatch::default();
        lose(&w);
        let mut now = t0;
        let mut gave_up = false;
        for _ in 0..InputWatch::MAX_TRIES {
            if w.poll(now, || Err("weg".into())) == Poll::GaveUp {
                gave_up = true;
            }
            now += Duration::from_secs(17);
        }
        assert!(gave_up);
        assert!(w.retrying(), "nach dem Aufgeben weiter wach");
        // Langsam: vor Ablauf von 30 s kein Versuch.
        let after = now;
        assert_eq!(w.poll(after, || panic!("zu früh")), Poll::Idle);
        let mut tries = 0;
        assert!(matches!(
            w.poll(after + InputWatch::SLOW_WAIT, || {
                tries += 1;
                Err("weg".into())
            }),
            Poll::Failed { report: false, .. }
        ));
        assert_eq!(tries, 1);
        assert_eq!(
            w.poll(
                after + InputWatch::SLOW_WAIT + Duration::from_secs(29),
                || panic!("zu früh")
            ),
            Poll::Idle
        );
        assert_eq!(
            w.poll(after + 2 * InputWatch::SLOW_WAIT, || Ok(())),
            Poll::Reopened
        );
        assert!(!w.retrying());
    }

    /// #605: „Übernehmen“ und „Neu verbinden“ versuchen sofort und melden das Ergebnis.
    #[test]
    fn retry_now_skips_the_wait_and_reports() {
        let t0 = Instant::now();
        let mut w = InputWatch::default();
        lose(&w);
        assert!(matches!(
            w.poll(t0, || Err("weg".into())),
            Poll::Failed { report: true, .. }
        ));
        w.retry_now();
        assert!(matches!(
            w.poll(t0, || Err("weg".into())),
            Poll::Failed { report: true, .. }
        ));
        w.retry_now();
        assert_eq!(w.poll(t0, || Ok(())), Poll::Reopened);
        // Ohne Ausfall tut retry_now nichts.
        w.retry_now();
        assert_eq!(w.poll(t0, || panic!("nichts ausgefallen")), Poll::Idle);
    }

    /// Ändert sich der Grund (Gerät fehlt → Kanalpaar fehlt), wird neu gemeldet; wechselnder
    /// Text in Klammern (Fehlertext des Systems) zählt nicht als neuer Grund.
    #[test]
    fn a_new_reason_is_reported_again() {
        let t0 = Instant::now();
        let mut w = InputWatch::default();
        lose(&w);
        let _ = w.poll(t0, || Err("Eingang X fehlt (Fehler 1).".into()));
        let r = w.poll(t0 + Duration::from_secs(1), || {
            Err("Eingang X fehlt (Fehler 2).".into())
        });
        assert!(matches!(r, Poll::Failed { report: false, .. }), "{r:?}");
        let r = w.poll(t0 + Duration::from_secs(3), || {
            Err("X hat keinen Eingang 3/4.".into())
        });
        assert!(matches!(r, Poll::Failed { report: true, .. }), "{r:?}");
    }

    /// Die Meldungen nennen die echte Dauer der schnellen Versuche.
    #[test]
    fn fast_span_matches_the_schedule() {
        let t0 = Instant::now();
        let mut w = InputWatch::default();
        lose(&w);
        let mut now = t0;
        loop {
            match w.poll(now, || Err("weg".into())) {
                Poll::GaveUp => break,
                Poll::Idle => now += Duration::from_millis(250),
                _ => {}
            }
        }
        assert_eq!(now - t0, InputWatch::FAST_SPAN);
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
