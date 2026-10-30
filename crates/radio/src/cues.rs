//! Cue-Punkte je Titel (Paket 15): Cue-In, Cue-Out, Ramp, Hook, Fade-Out-Marker.
//!
//! Alle Werte in Sekunden ab Dateianfang, damit sie von der Geräterate unabhängig sind.
//! `None` heißt: nicht gesetzt (Cue-In = Anfang, Cue-Out = Ende, Überblendung nach der
//! globalen Einstellung).
//!
//! - **Cue-In / Cue-Out:** hier beginnt und endet der Titel in der Sendung.
//! - **Fade-Out-Marker:** hier beginnt die Überblendung zum nächsten Titel; die
//!   Überblendung dauert bis Cue-Out (statt der globalen Überblendlänge).
//! - **Ramp:** Ende des Intros; das Radio-Layout zählt bis dahin herunter.
//! - **Hook:** markante Stelle zum Vorhören (PFL ab Hook).
//!
//! Restzeit des Titels läuft bis Cue-Out.

use serde::{Deserialize, Serialize};

/// Cue-Punkte eines Titels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Cues {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cue_in: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cue_out: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade_out: Option<f64>,
}

/// Cue-Punkte in Frames für den Mixer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CueFrames {
    pub start: usize,
    pub end: usize,
    /// Überblendlänge ab Fade-Out-Marker bis Cue-Out (Frames), falls gesetzt.
    pub fade: Option<usize>,
}

impl Cues {
    pub fn is_empty(&self) -> bool {
        *self == Cues::default()
    }

    /// Anfang in Sekunden.
    pub fn start(&self) -> f64 {
        self.cue_in.unwrap_or(0.0)
    }

    /// Ende in Sekunden bei Titellänge `seconds`.
    pub fn end(&self, seconds: f64) -> f64 {
        self.cue_out.unwrap_or(seconds).min(seconds)
    }

    /// Gespielte Länge in Sekunden.
    pub fn play_seconds(&self, seconds: f64) -> f64 {
        (self.end(seconds) - self.start()).max(0.0)
    }

    /// Überblendlänge aus dem Fade-Out-Marker (Sekunden), falls gesetzt.
    pub fn fade_seconds(&self, seconds: f64) -> Option<f64> {
        self.fade_out.map(|f| (self.end(seconds) - f).max(0.0))
    }

    /// Prüft die Werte. `seconds` = Titellänge, falls bekannt.
    /// Regel: 0 ≤ Cue-In < Cue-Out ≤ Länge, Fade-Out, Ramp und Hook zwischen Cue-In und Cue-Out.
    pub fn validate(&self, seconds: Option<f64>) -> Result<(), String> {
        let named = [
            ("Cue-In", self.cue_in),
            ("Cue-Out", self.cue_out),
            ("Ramp", self.ramp),
            ("Hook", self.hook),
            ("Fade-Out", self.fade_out),
        ];
        for (name, v) in named {
            if let Some(v) = v {
                if !v.is_finite() || v < 0.0 {
                    return Err(format!("{name}: {v} ist keine gültige Zeit"));
                }
                if let Some(len) = seconds
                    && v > len + 1e-6
                {
                    return Err(format!(
                        "{name}: {v:.2} s liegt hinter dem Titelende ({len:.2} s)"
                    ));
                }
            }
        }
        let start = self.start();
        let end = self.cue_out.or(seconds).unwrap_or(f64::INFINITY);
        if start >= end {
            return Err(format!("Cue-In ({start:.2} s) muss vor Cue-Out liegen"));
        }
        for (name, v) in &named[2..] {
            if let Some(v) = *v
                && (v < start - 1e-6 || v > end + 1e-6)
            {
                return Err(format!(
                    "{name} ({v:.2} s) muss zwischen Cue-In und Cue-Out liegen"
                ));
            }
        }
        Ok(())
    }

    /// In Frames bei `rate`, begrenzt auf `total` Frames des dekodierten Puffers.
    pub fn frames(&self, rate: u32, total: usize) -> CueFrames {
        let f = |s: f64| ((s.max(0.0) * f64::from(rate)).round() as usize).min(total);
        let end = self.cue_out.map_or(total, f);
        let start = f(self.start()).min(end);
        let fade = self.fade_out.map(|x| end.saturating_sub(f(x).max(start)));
        CueFrames { start, end, fade }
    }
}

/// Ein Undo-Schritt: Cue-Punkte des Titels mit Laufzeit-Kennung `uid` vorher und nachher.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CueEdit {
    pub uid: u32,
    pub before: Cues,
    pub after: Cues,
}

/// Undo/Redo für Cue-Bearbeitungen (eigener Verlauf, nicht der des Projekts).
#[derive(Debug, Default)]
pub struct CueHistory {
    undo: Vec<CueEdit>,
    redo: Vec<CueEdit>,
}

/// Höchstens so viele Schritte.
const HISTORY: usize = 200;

impl CueHistory {
    pub fn push(&mut self, e: CueEdit) {
        self.undo.push(e);
        if self.undo.len() > HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn take_undo(&mut self) -> Option<CueEdit> {
        let e = self.undo.pop()?;
        self.redo.push(e);
        Some(e)
    }

    pub fn take_redo(&mut self) -> Option<CueEdit> {
        let e = self.redo.pop()?;
        self.undo.push(e);
        Some(e)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation() {
        let ok = Cues {
            cue_in: Some(1.0),
            cue_out: Some(9.0),
            ramp: Some(4.0),
            hook: Some(5.0),
            fade_out: Some(7.0),
        };
        assert!(ok.validate(Some(10.0)).is_ok());
        assert!(ok.validate(None).is_ok());
        assert!(Cues::default().validate(Some(10.0)).is_ok());
        let mut bad = ok;
        bad.cue_out = Some(11.0);
        assert!(bad.validate(Some(10.0)).is_err());
        let mut bad = ok;
        bad.cue_in = Some(9.5);
        assert!(bad.validate(Some(10.0)).is_err());
        let mut bad = ok;
        bad.ramp = Some(0.5);
        assert!(bad.validate(Some(10.0)).is_err());
        let mut bad = ok;
        bad.hook = Some(f64::NAN);
        assert!(bad.validate(None).is_err());
        let mut bad = ok;
        bad.fade_out = Some(9.5);
        assert!(bad.validate(Some(10.0)).is_err());
    }

    #[test]
    fn frames_and_lengths() {
        let c = Cues {
            cue_in: Some(1.0),
            cue_out: Some(9.0),
            fade_out: Some(7.0),
            ..Cues::default()
        };
        assert_eq!(
            c.frames(100, 1000),
            CueFrames {
                start: 100,
                end: 900,
                fade: Some(200)
            }
        );
        assert_eq!(c.play_seconds(10.0), 8.0);
        assert_eq!(c.fade_seconds(10.0), Some(2.0));
        let none = Cues::default().frames(100, 1000);
        assert_eq!((none.start, none.end, none.fade), (0, 1000, None));
        // Cue-Out hinter dem Puffer wird begrenzt.
        let long = Cues {
            cue_out: Some(50.0),
            ..Cues::default()
        };
        assert_eq!(long.frames(100, 1000).end, 1000);
    }

    #[test]
    fn history() {
        let mut h = CueHistory::default();
        let e = CueEdit {
            uid: 1,
            before: Cues::default(),
            after: Cues {
                hook: Some(2.0),
                ..Cues::default()
            },
        };
        h.push(e);
        assert_eq!(h.take_undo(), Some(e));
        assert!(!h.can_undo());
        assert_eq!(h.take_redo(), Some(e));
        assert!(h.take_redo().is_none());
    }
}
