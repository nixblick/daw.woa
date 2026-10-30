//! Sättigung: Vorverstärkung, Kennlinie, Pegelausgleich, Mix. Drei Kennlinien: weich (tanh,
//! ungerade Obertöne), Röhre (verschobener Arbeitspunkt, auch gerade Obertöne) und hart (kubischer
//! Soft-Clip). Ein Gleichspannungsfilter nimmt den Versatz der Röhrenkennlinie wieder weg.

use crate::{DEFAULT_RAMP_SECONDS, Smoother};

/// Kennlinien in der Reihenfolge des Reglers `shape`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Soft,
    Tube,
    Hard,
}

const TUBE_BIAS: f32 = 0.2;

#[inline]
fn curve(shape: Shape, x: f32) -> f32 {
    match shape {
        Shape::Soft => x.tanh(),
        Shape::Tube => (x + TUBE_BIAS).tanh() - TUBE_BIAS.tanh(),
        Shape::Hard => {
            let c = x.clamp(-1.0, 1.0);
            1.5 * c - 0.5 * c * c * c
        }
    }
}

/// Sättigung für einen oder zwei Kanäle.
#[derive(Debug, Clone)]
pub struct Saturator {
    shape: Shape,
    drive: Smoother,
    out: Smoother,
    mix: Smoother,
    /// 1 / Kennlinie bei Vollaussteuerung der Vorverstärkung.
    norm: f32,
    dc_r: f32,
    x1: [f32; 2],
    y1: [f32; 2],
}

fn shape_of(v: f32) -> Shape {
    match v.round() as i32 {
        1 => Shape::Tube,
        2 => Shape::Hard,
        _ => Shape::Soft,
    }
}

fn db_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

impl Saturator {
    /// Legt die Sättigung an und stellt die Regler ein.
    pub fn new(p: &[f32], rate: f32) -> Self {
        let sm = |v: f32| {
            let mut s = Smoother::new(v, DEFAULT_RAMP_SECONDS);
            s.prepare(rate);
            s
        };
        let mut s = Self {
            shape: Shape::Soft,
            drive: sm(db_lin(p[0])),
            out: sm(db_lin(p[2])),
            mix: sm(p[3] / 100.0),
            norm: 1.0,
            dc_r: 1.0 - std::f32::consts::TAU * 10.0 / rate,
            x1: [0.0; 2],
            y1: [0.0; 2],
        };
        s.set(p);
        s
    }

    /// Neue Reglerwerte: drive_db, shape, output_db, mix_pct.
    pub fn set(&mut self, p: &[f32]) {
        self.shape = shape_of(p[1]);
        self.drive.set_target(db_lin(p[0]));
        self.out.set_target(db_lin(p[2]));
        self.mix.set_target(p[3] / 100.0);
        self.norm = 1.0 / curve(self.shape, db_lin(p[0])).abs().max(1e-3);
    }

    #[inline]
    fn channel(&mut self, c: usize, x: f32, drive: f32, out: f32) -> f32 {
        let y = curve(self.shape, x * drive) * self.norm;
        // Gleichspannung (Röhrenkennlinie) mit Hochpass bei etwa 10 Hz entfernen.
        let h = y - self.x1[c] + self.dc_r * self.y1[c];
        self.x1[c] = y;
        self.y1[c] = if h.abs() < 1e-20 { 0.0 } else { h };
        // Das Hochpass-Klingeln hinter starkem, einseitigem Übersteuern bleibt unter Vollaussteuerung.
        h.clamp(-1.0, 1.0) * out
    }

    /// Ein Sample-Paar; bei `stereo = false` zählt nur `l`.
    #[inline]
    pub fn step(&mut self, l: f32, r: f32, stereo: bool) -> (f32, f32) {
        let (drive, out, m) = (
            self.drive.next_value(),
            self.out.next_value(),
            self.mix.next_value(),
        );
        let wl = self.channel(0, l, drive, out);
        let wr = if stereo {
            self.channel(1, r, drive, out)
        } else {
            r
        };
        (l + (wl - l) * m, r + (wr - r) * m)
    }
}
