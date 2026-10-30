//! Automation (Paket 18): Kurven für Lautstärke, Pan, Sends und Effekt-Regler.
//!
//! Eine Spur hält je Ziel höchstens eine [`AutomationLane`]: Stützpunkte in Beats mit einem
//! Wert im Bereich des Ziels. Dazwischen wird linear überblendet, bei Auswahl-Reglern
//! (ganze Werte) bleibt der Wert bis zum nächsten Punkt stehen. Vor dem ersten und nach dem
//! letzten Punkt gilt der jeweils nächste Punkt. Lautstärke und Send rechnen in dB, Pan
//! linear; so rechnet auch die Engine.

use serde::{Deserialize, Serialize};

use crate::effect::InsertId;
use crate::error::{ModelError, Result};
use crate::project::{MAX_GAIN_DB, MAX_SEND_DB, MIN_GAIN_DB, Project, Track, TrackId};

/// Höchste Zahl Stützpunkte einer Kurve.
pub const MAX_AUTOMATION_POINTS: usize = 4096;

/// Was eine Kurve steuert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AutoTarget {
    /// Lautstärke der Spur (dB).
    Gain,
    /// Panorama der Spur (-1 links … 1 rechts).
    Pan,
    /// Pegel des Sends zu einem Bus (dB).
    Send {
        /// Ziel-Bus des Sends.
        target: TrackId,
    },
    /// Ein Regler eines Effekts der Spur.
    InsertParam {
        /// Der Effekt.
        insert: InsertId,
        /// Index des Reglers, siehe `EffectKind::params`.
        param: usize,
    },
}

/// Betriebsart einer Kurve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoMode {
    /// Lesen: die Kurve wird abgespielt.
    #[default]
    Read,
    /// Schreiben: ab Start der Wiedergabe wird der Reglerwert aufgenommen (überschreibt).
    Write,
    /// Latch: ab dem ersten Anfassen wird aufgenommen, bis die Wiedergabe stoppt.
    Latch,
}

impl AutoMode {
    /// Alle Arten in der Reihenfolge der Oberfläche.
    pub const ALL: [AutoMode; 3] = [AutoMode::Read, AutoMode::Write, AutoMode::Latch];

    /// Kurzname für Knöpfe (L, S, Latch).
    pub fn short(self) -> &'static str {
        match self {
            AutoMode::Read => "L",
            AutoMode::Write => "S",
            AutoMode::Latch => "Latch",
        }
    }

    /// Name für Hilfetexte und Meldungen.
    pub fn label(self) -> &'static str {
        match self {
            AutoMode::Read => "Lesen",
            AutoMode::Write => "Schreiben",
            AutoMode::Latch => "Latch",
        }
    }
}

/// Ein Stützpunkt.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AutoPoint {
    /// Position in Beats (>= 0).
    pub beat: f64,
    /// Wert im Bereich des Ziels.
    pub value: f64,
}

/// Eine Kurve für ein Ziel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationLane {
    /// Was die Kurve steuert.
    pub target: AutoTarget,
    /// Betriebsart; Lesen, wenn nicht gespeichert.
    #[serde(default, skip_serializing_if = "is_read")]
    pub mode: AutoMode,
    /// Stützpunkte, nach Beat sortiert, ein Wert je Beat.
    pub points: Vec<AutoPoint>,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_read(m: &AutoMode) -> bool {
    *m == AutoMode::Read
}

/// Bereich und Art eines Ziels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoRange {
    /// Kleinster Wert.
    pub min: f64,
    /// Größter Wert.
    pub max: f64,
    /// Auswahl-Regler: nur ganze Werte, Wert bleibt stehen statt zu gleiten.
    pub step: bool,
    /// Einheit für Anzeigen, z. B. `dB`.
    pub unit: &'static str,
}

impl AutoRange {
    /// Rechnet einen Wert 0..1 (Zeichenfläche, Controller) in den Bereich um.
    pub fn from_norm(&self, n: f64) -> f64 {
        let v = self.min + n.clamp(0.0, 1.0) * (self.max - self.min);
        if self.step { v.round() } else { v }
    }

    /// Rechnet einen Wert in 0..1 um.
    pub fn to_norm(&self, v: f64) -> f64 {
        if self.max > self.min {
            ((v - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// Prüft Wert gegen Bereich.
    pub fn check(&self, v: f64) -> Result<()> {
        let whole = !self.step || v.fract() == 0.0;
        if v.is_finite() && whole && (self.min..=self.max).contains(&v) {
            Ok(())
        } else {
            Err(ModelError::InvalidValue {
                field: "automation value",
                value: v,
            })
        }
    }
}

impl AutoTarget {
    /// Bereich des Ziels auf der Spur `track`; Fehler, wenn das Ziel dort nicht (mehr)
    /// existiert (Effekt, Send, Regler).
    pub fn range(&self, project: &Project, track: TrackId) -> Result<AutoRange> {
        let t = project
            .track(track)
            .ok_or(ModelError::TrackNotFound(track))?;
        self.range_in(t)
    }

    /// Wie [`AutoTarget::range`], für eine Spur, die (noch) nicht im Projekt liegt.
    pub fn range_in(&self, t: &Track) -> Result<AutoRange> {
        let track = t.id;
        Ok(match *self {
            AutoTarget::Gain => AutoRange {
                min: MIN_GAIN_DB,
                max: MAX_GAIN_DB,
                step: false,
                unit: "dB",
            },
            AutoTarget::Pan => AutoRange {
                min: -1.0,
                max: 1.0,
                step: false,
                unit: "",
            },
            AutoTarget::Send { target } => {
                if !t.sends.iter().any(|s| s.target == target) {
                    return Err(ModelError::SendNotFound { track, target });
                }
                AutoRange {
                    min: MIN_GAIN_DB,
                    max: MAX_SEND_DB,
                    step: false,
                    unit: "dB",
                }
            }
            AutoTarget::InsertParam { insert, param } => {
                let ins = t
                    .inserts
                    .iter()
                    .find(|i| i.id == insert)
                    .ok_or(ModelError::InsertNotFound(insert))?;
                let spec = ins
                    .kind
                    .params()
                    .get(param)
                    .ok_or(ModelError::UnknownParam { index: param })?;
                AutoRange {
                    min: spec.min,
                    max: spec.max,
                    step: !spec.choices.is_empty(),
                    unit: spec.unit,
                }
            }
        })
    }
}

impl AutomationLane {
    /// Kurve ohne Punkte im Lesen-Modus; sie zählt als nicht vorhanden.
    pub fn is_blank(&self) -> bool {
        self.points.is_empty() && self.mode == AutoMode::Read
    }

    /// Wert bei `beat` (Beats), `None` ohne Punkte. `step`: Wert bleibt bis zum nächsten
    /// Punkt stehen.
    pub fn value_at(&self, beat: f64, step: bool) -> Option<f64> {
        value_at(&self.points, beat, step)
    }

    /// Setzt einen Punkt (ersetzt den Punkt auf demselben Beat) und hält die Reihenfolge.
    pub fn with_point(mut self, beat: f64, value: f64) -> Self {
        self.points.retain(|p| p.beat != beat);
        self.points.push(AutoPoint { beat, value });
        self.points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        self
    }

    /// Sortiert und prüft die Punkte gegen den Bereich; doppelte Beats behalten den letzten.
    pub(crate) fn normalise(points: &mut Vec<AutoPoint>, range: &AutoRange) -> Result<()> {
        if points.len() > MAX_AUTOMATION_POINTS {
            return Err(ModelError::TooManyPoints {
                max: MAX_AUTOMATION_POINTS,
            });
        }
        for p in points.iter() {
            if !(p.beat.is_finite() && p.beat >= 0.0) {
                return Err(ModelError::InvalidValue {
                    field: "automation beat",
                    value: p.beat,
                });
            }
            range.check(p.value)?;
        }
        // Stabil sortieren, dann bei gleichem Beat den späteren Punkt behalten.
        points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        let mut out: Vec<AutoPoint> = Vec::with_capacity(points.len());
        for p in points.drain(..) {
            match out.last_mut() {
                Some(last) if last.beat == p.beat => *last = p,
                _ => out.push(p),
            }
        }
        *points = out;
        Ok(())
    }
}

/// Wert einer Punktliste bei `beat` (Beats). Die Punkte müssen nach Beat sortiert sein.
pub fn value_at(points: &[AutoPoint], beat: f64, step: bool) -> Option<f64> {
    let first = points.first()?;
    let last = points.last()?;
    if beat <= first.beat {
        return Some(first.value);
    }
    if beat >= last.beat {
        return Some(last.value);
    }
    let i = points.partition_point(|p| p.beat <= beat);
    let (a, b) = (points[i - 1], points[i]);
    if step {
        return Some(a.value);
    }
    let t = (beat - a.beat) / (b.beat - a.beat);
    Some(a.value + (b.value - a.value) * t)
}

impl Track {
    /// Die Kurve für `target`, falls vorhanden.
    pub fn lane(&self, target: &AutoTarget) -> Option<&AutomationLane> {
        self.automation.iter().find(|l| l.target == *target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pts(v: &[(f64, f64)]) -> Vec<AutoPoint> {
        v.iter()
            .map(|&(beat, value)| AutoPoint { beat, value })
            .collect()
    }

    #[test]
    fn interpolates_linearly_and_holds_the_ends() {
        let p = pts(&[(1.0, -20.0), (3.0, 0.0), (7.0, -10.0)]);
        assert_eq!(value_at(&p, 0.0, false), Some(-20.0));
        assert_eq!(value_at(&p, 1.0, false), Some(-20.0));
        assert_eq!(value_at(&p, 2.0, false), Some(-10.0));
        assert_eq!(value_at(&p, 3.0, false), Some(0.0));
        assert_eq!(value_at(&p, 5.0, false), Some(-5.0));
        assert_eq!(value_at(&p, 9.0, false), Some(-10.0));
        assert_eq!(value_at(&[], 1.0, false), None);
    }

    #[test]
    fn steps_hold_until_the_next_point() {
        let p = pts(&[(0.0, 1.0), (4.0, 3.0)]);
        assert_eq!(value_at(&p, 3.99, true), Some(1.0));
        assert_eq!(value_at(&p, 4.0, true), Some(3.0));
    }

    #[test]
    fn normalise_sorts_dedups_and_checks() {
        let range = AutoRange {
            min: -96.0,
            max: 24.0,
            step: false,
            unit: "dB",
        };
        let mut p = pts(&[(2.0, 0.0), (1.0, -3.0), (2.0, -6.0)]);
        AutomationLane::normalise(&mut p, &range).expect("ok");
        assert_eq!(p, pts(&[(1.0, -3.0), (2.0, -6.0)]));
        for bad in [
            pts(&[(-1.0, 0.0)]),
            pts(&[(f64::NAN, 0.0)]),
            pts(&[(0.0, 25.0)]),
            pts(&[(0.0, f64::INFINITY)]),
        ] {
            let mut b = bad;
            assert!(AutomationLane::normalise(&mut b, &range).is_err());
        }
        let mut many = (0..=MAX_AUTOMATION_POINTS)
            .map(|i| AutoPoint {
                beat: i as f64,
                value: 0.0,
            })
            .collect();
        assert!(AutomationLane::normalise(&mut many, &range).is_err());
    }

    #[test]
    fn range_norm_roundtrip() {
        let r = AutoRange {
            min: 0.0,
            max: 8.0,
            step: true,
            unit: "",
        };
        assert_eq!(r.from_norm(0.5), 4.0);
        assert_eq!(r.from_norm(0.26), 2.0);
        assert!((r.to_norm(2.0) - 0.25).abs() < 1e-12);
        assert!(r.check(2.5).is_err());
    }

    #[test]
    fn with_point_replaces_the_same_beat() {
        let lane = AutomationLane {
            target: AutoTarget::Gain,
            mode: AutoMode::Read,
            points: pts(&[(0.0, 0.0), (4.0, -6.0)]),
        }
        .with_point(2.0, -2.0)
        .with_point(4.0, -9.0);
        assert_eq!(lane.points, pts(&[(0.0, 0.0), (2.0, -2.0), (4.0, -9.0)]));
    }
}
