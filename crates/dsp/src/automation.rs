//! Automation im Audio-Thread (Paket 18): Kurven in Samples und ein Gain-Node, der sie
//! sample-genau abspielt.
//!
//! Eine [`AutoCurve`] kennt ihre Stützpunkte schon in Samples (der Control-Thread rechnet
//! Beats mit dem Projekttempo um). Knoten, die eine Kurve besitzen, bekommen von der Engine
//! vor jedem Abschnitt ihre Position über `locate` und zählen sie im Block selbst weiter;
//! deshalb ist das Ergebnis unabhängig von der Blockgröße. Kein `process` allokiert.

use crate::{DEFAULT_RAMP_SECONDS, Node, ProcessBuffers, Smoother};

/// Faktor von dB auf Amplitude im Exponenten (`ln(10) / 20`).
const DB_TO_NEPER: f32 = 0.115_129_25;

/// Stützpunkte in Samples mit Werten, zur Wiedergabe aufbereitet.
#[derive(Debug, Clone)]
pub struct AutoCurve {
    points: Vec<(u64, f32)>,
    /// Quelle in Beats, wenn die Kurve erst bei `prepare` auf die Abtastrate umgerechnet wird.
    beats: Vec<(f64, f32)>,
    tempo_bpm: f64,
    step: bool,
    cursor: usize,
}

impl AutoCurve {
    /// Neue Kurve. `points` müssen nach Sample aufsteigend sortiert sein; `step`: der Wert
    /// bleibt bis zum nächsten Punkt stehen, statt zu gleiten.
    pub fn new(points: Vec<(u64, f32)>, step: bool) -> Self {
        Self {
            points,
            beats: Vec::new(),
            tempo_bpm: 120.0,
            step,
            cursor: 0,
        }
    }

    /// Kurve in Beats; `prepare` rechnet mit Tempo und Abtastrate in Samples um (abgerundet,
    /// wie die Clip-Positionen). Bis dahin gilt 48 kHz.
    pub fn from_beats(points: &[(f64, f32)], tempo_bpm: f64, step: bool) -> Self {
        let mut c = Self::new(Vec::new(), step);
        c.beats = points.to_vec();
        c.tempo_bpm = tempo_bpm;
        c.prepare(48_000.0);
        c
    }

    /// Rechnet eine Beats-Kurve auf die Abtastrate um (Control-Thread; darf allokieren).
    pub fn prepare(&mut self, sample_rate: f32) {
        if self.beats.is_empty() {
            return;
        }
        let tempo = if self.tempo_bpm.is_finite() && self.tempo_bpm > 0.0 {
            self.tempo_bpm
        } else {
            120.0
        };
        let per_beat = 60.0 / tempo * f64::from(sample_rate.max(1.0));
        self.points = self
            .beats
            .iter()
            .map(|&(b, v)| ((b.max(0.0) * per_beat).floor() as u64, v))
            .collect();
        self.cursor = 0;
    }

    /// Ob die Kurve Punkte hat.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Wert bei Sample `s`: vor dem ersten und nach dem letzten Punkt gilt dieser Punkt,
    /// dazwischen die Gerade. Allokationsfrei; schnell bei aufsteigendem `s`.
    #[inline]
    pub fn value_at(&mut self, s: u64) -> f32 {
        let n = self.points.len();
        if n == 0 {
            return 0.0;
        }
        if self.cursor >= n || self.points[self.cursor].0 > s {
            // Rücksprung (Seek, Loop): neu suchen.
            self.cursor = self.points.partition_point(|p| p.0 <= s).saturating_sub(1);
        }
        while self.cursor + 1 < n && self.points[self.cursor + 1].0 <= s {
            self.cursor += 1;
        }
        let (s0, v0) = self.points[self.cursor];
        if s <= s0 || self.cursor + 1 >= n {
            return v0;
        }
        let (s1, v1) = self.points[self.cursor + 1];
        if self.step {
            return v0;
        }
        let t = (s - s0) as f64 / (s1 - s0) as f64;
        (f64::from(v0) + (f64::from(v1) - f64::from(v0)) * t) as f32
    }
}

/// Amplitude zu einem dB-Wert.
#[inline]
pub fn db_to_amp(db: f32) -> f32 {
    (db * DB_TO_NEPER).exp()
}

/// Welche Seite eines Panoramas ein [`AutoNode`] liefert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanSide {
    /// Linker Kanal (cos).
    Left,
    /// Rechter Kanal (sin).
    Right,
}

impl PanSide {
    /// Constant-Power-Faktor für Pan `-1..1`.
    #[inline]
    pub fn factor(self, pan: f32) -> f32 {
        let p = if pan.is_finite() {
            pan.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        let angle = (p + 1.0) * std::f32::consts::FRAC_PI_4;
        match self {
            PanSide::Left => angle.cos(),
            PanSide::Right => angle.sin(),
        }
    }
}

/// Parameter-IDs des [`AutoNode`].
pub mod auto_params {
    /// Fester Faktor (linear), geglättet; trägt Mute/Solo und alles, was nicht automatisiert ist.
    pub const FIXED: u32 = 0;
}

/// Gain mit Kurven: fester Faktor mal Pegelkurve (dB) mal Panoramakurve (Seite).
#[derive(Debug, Clone)]
pub struct AutoNode {
    fixed: Smoother,
    level: Option<AutoCurve>,
    pan: Option<(AutoCurve, PanSide)>,
    pos: u64,
    playing: bool,
}

impl AutoNode {
    /// Neuer Node. `fixed` ist der Startwert des festen Faktors.
    pub fn new(fixed: f32, level: Option<AutoCurve>, pan: Option<(AutoCurve, PanSide)>) -> Self {
        Self {
            fixed: Smoother::new(fixed, DEFAULT_RAMP_SECONDS),
            level,
            pan,
            pos: 0,
            playing: true,
        }
    }
}

impl Node for AutoNode {
    fn prepare(&mut self, sample_rate: f32) {
        self.fixed.prepare(sample_rate);
        if let Some(c) = &mut self.level {
            c.prepare(sample_rate);
        }
        if let Some((c, _)) = &mut self.pan {
            c.prepare(sample_rate);
        }
    }

    fn set_param(&mut self, param: u32, value: f32) {
        if param == auto_params::FIXED && value.is_finite() {
            self.fixed.set_target(value);
        }
    }

    fn locate(&mut self, position: u64, playing: bool) {
        self.pos = position;
        self.playing = playing;
    }

    fn process(&mut self, b: ProcessBuffers<'_>) {
        let mut pos = self.pos;
        for (o, &x) in b.output.iter_mut().zip(b.input) {
            let mut f = self.fixed.next_value();
            if let Some(c) = &mut self.level {
                f *= db_to_amp(c.value_at(pos));
            }
            if let Some((c, side)) = &mut self.pan {
                f *= side.factor(c.value_at(pos));
            }
            *o = x * f;
            if self.playing {
                pos += 1;
            }
        }
        self.pos = pos;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_is_exact_at_samples() {
        let mut c = AutoCurve::new(vec![(100, 0.0), (200, 10.0), (400, -10.0)], false);
        assert_eq!(c.value_at(0), 0.0);
        assert_eq!(c.value_at(100), 0.0);
        assert_eq!(c.value_at(150), 5.0);
        assert_eq!(c.value_at(200), 10.0);
        assert_eq!(c.value_at(300), 0.0);
        assert_eq!(c.value_at(9999), -10.0);
        // Rücksprung
        assert_eq!(c.value_at(150), 5.0);
        assert_eq!(c.value_at(0), 0.0);
    }

    #[test]
    fn step_curve_holds() {
        let mut c = AutoCurve::new(vec![(0, 1.0), (10, 3.0)], true);
        assert_eq!(c.value_at(9), 1.0);
        assert_eq!(c.value_at(10), 3.0);
    }

    #[test]
    fn node_follows_a_db_ramp_sample_exactly_in_any_block_size() {
        let curve = AutoCurve::new(vec![(0, 0.0), (480, -20.0)], false);
        let input = vec![1.0f32; 480];
        let mut reference = Vec::new();
        for block in [480usize, 64, 7, 1] {
            let mut n = AutoNode::new(1.0, Some(curve.clone()), None);
            n.prepare(48_000.0);
            n.locate(0, true);
            let mut out = vec![0.0f32; 480];
            for (i, o) in input.chunks(block).zip(out.chunks_mut(block)) {
                n.process(ProcessBuffers {
                    input: i,
                    output: o,
                });
            }
            if reference.is_empty() {
                reference = out.clone();
            }
            assert_eq!(out, reference, "block {block}");
        }
        assert!((reference[0] - 1.0).abs() < 1e-6);
        assert!((reference[240] - db_to_amp(-10.0)).abs() < 1e-5);
    }

    #[test]
    fn stopped_node_holds_the_value_at_the_cursor() {
        let curve = AutoCurve::new(vec![(0, 0.0), (100, -20.0)], false);
        let mut n = AutoNode::new(1.0, Some(curve), None);
        n.prepare(48_000.0);
        n.locate(50, false);
        let mut out = vec![0.0f32; 8];
        n.process(ProcessBuffers {
            input: &[1.0; 8],
            output: &mut out,
        });
        let want = db_to_amp(-10.0);
        assert!(out.iter().all(|v| (v - want).abs() < 1e-5));
    }

    #[test]
    fn pan_sides_are_constant_power() {
        for p in [-1.0f32, -0.3, 0.0, 0.7, 1.0] {
            let (l, r) = (PanSide::Left.factor(p), PanSide::Right.factor(p));
            assert!((l * l + r * r - 1.0).abs() < 1e-5);
        }
    }
}
