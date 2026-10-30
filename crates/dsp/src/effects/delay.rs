//! Delay (Echo), frei in Millisekunden oder tempo-synchron, mit Tiefpass in der Rückkopplung
//! und optionalem Ping-Pong. Die Zeit wird weich nachgeführt (kein Knacken beim Drehen).

use super::{DelayLine, OnePole};
use crate::dynamics::coef;
use crate::{DEFAULT_RAMP_SECONDS, Smoother};

/// Längste Verzögerung in Sekunden (ein Puffer fester Größe).
pub const MAX_DELAY_S: f32 = 4.0;

/// Notenwerte der Synchronisation in Schlägen (Viertel = 1); Index 0 = frei.
pub const SYNC_BEATS: [f32; 9] = [0.0, 2.0, 1.0, 1.5, 0.5, 0.75, 0.25, 2.0 / 3.0, 1.0 / 3.0];

/// Verzögerung in Sekunden für Reglerwerte und Tempo.
pub fn delay_seconds(time_ms: f32, sync: f32, tempo_bpm: f32) -> f32 {
    let idx = sync.round().clamp(0.0, (SYNC_BEATS.len() - 1) as f32) as usize;
    if idx == 0 {
        time_ms * 0.001
    } else {
        SYNC_BEATS[idx] * 60.0 / tempo_bpm.clamp(20.0, 400.0)
    }
}

/// Delay für einen oder zwei Kanäle.
#[derive(Debug, Clone)]
pub struct DelayFx {
    line: [DelayLine; 2],
    tone: [OnePole; 2],
    target: f32,
    current: f32,
    slew: f32,
    feedback: f32,
    pingpong: bool,
    mix: Smoother,
}

impl DelayFx {
    /// Legt die Puffer an (Control-Thread) und stellt die Regler ein.
    pub fn new(p: &[f32], rate: f32, tempo_bpm: f32) -> Self {
        let size = (MAX_DELAY_S * rate) as usize;
        let mut mix = Smoother::new(p[5] / 100.0, DEFAULT_RAMP_SECONDS);
        mix.prepare(rate);
        let mut d = Self {
            line: [DelayLine::new(size), DelayLine::new(size)],
            tone: [OnePole::lowpass(6000.0, rate); 2],
            target: 1.0,
            current: 1.0,
            slew: coef(0.05, rate),
            feedback: 0.0,
            pingpong: false,
            mix,
        };
        d.set(p, rate, tempo_bpm);
        d.current = d.target;
        d
    }

    /// Neue Reglerwerte: time_ms, sync, feedback_pct, tone_hz, pingpong, mix_pct.
    pub fn set(&mut self, p: &[f32], rate: f32, tempo_bpm: f32) {
        let secs = delay_seconds(p[0], p[1], tempo_bpm);
        self.target = (secs * rate).clamp(1.0, self.line[0].max_delay());
        self.feedback = (p[2] / 100.0).clamp(0.0, 0.95);
        for t in &mut self.tone {
            t.set_lowpass(p[3], rate);
        }
        self.pingpong = p[4] >= 0.5;
        self.mix.set_target(p[5] / 100.0);
    }

    /// Ein Sample-Paar; bei `stereo = false` zählt nur `l`.
    #[inline]
    pub fn step(&mut self, l: f32, r: f32, stereo: bool) -> (f32, f32) {
        let m = self.mix.next_value();
        if self.current != self.target {
            self.current += (self.target - self.current) * self.slew;
            if (self.target - self.current).abs() < 0.01 {
                self.current = self.target;
            }
        }
        let yl = self.line[0].read(self.current);
        let yr = if stereo {
            self.line[1].read(self.current)
        } else {
            0.0
        };
        let fl = self.tone[0].run(yl) * self.feedback;
        if stereo {
            let fr = self.tone[1].run(yr) * self.feedback;
            if self.pingpong {
                let mono = 0.5 * (l + r);
                self.line[0].write(mono + fr);
                self.line[1].write(fl);
            } else {
                self.line[0].write(l + fl);
                self.line[1].write(r + fr);
            }
            (l + (yl - l) * m, r + (yr - r) * m)
        } else {
            self.line[0].write(l + fl);
            (l + (yl - l) * m, r)
        }
    }
}
