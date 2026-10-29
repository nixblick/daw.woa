//! Aussetzer (Xruns) der Ausgabe: Der Fehlerpfad des Stroms zählt lock-frei, geloggt
//! wird höchstens eine zusammengefasste Zeile je Minute (#51).

use std::time::{Duration, Instant};

/// Abstand zwischen zwei Log-Zeilen.
pub const LOG_EVERY: Duration = Duration::from_secs(60);

/// Takt der Zusammenfassung: merkt sich Zählerstand und Zeit der letzten Zeile.
#[derive(Debug)]
pub struct XrunLog {
    logged: u64,
    at: Instant,
}

impl XrunLog {
    pub fn new(now: Instant) -> Self {
        Self { logged: 0, at: now }
    }

    /// Mit dem aktuellen Zählerstand aufrufen. Ist eine Minute um, kommt die Zahl der
    /// Aussetzer seit der letzten Zeile zurück; der Aufrufer loggt sie mit seinem Kontext.
    pub fn due(&mut self, total: u64, now: Instant) -> Option<u64> {
        if now.duration_since(self.at) < LOG_EVERY {
            return None;
        }
        let new = total.saturating_sub(self.logged);
        self.logged = total;
        self.at = now;
        Some(new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_summary_per_minute() {
        let t0 = Instant::now();
        let mut log = XrunLog::new(t0);
        assert_eq!(log.due(5, t0 + Duration::from_secs(10)), None);
        assert_eq!(log.due(7, t0 + Duration::from_secs(61)), Some(7));
        assert_eq!(log.due(9, t0 + Duration::from_secs(90)), None);
        assert_eq!(log.due(9, t0 + Duration::from_secs(122)), Some(2));
        assert_eq!(log.due(9, t0 + Duration::from_secs(190)), Some(0));
    }
}
