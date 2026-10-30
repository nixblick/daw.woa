//! Insert-Effekte des Mischpults (Paket 16 und 17): voller EQ, Kompressor, Limiter, Gate,
//! De-Esser, Reverb, Delay, Chorus, Sättigung, Rauschunterdrückung.
//!
//! Ein [`Fx`] rechnet ein oder zwei Kanäle; [`FxNode`] hängt ihn mono, [`FxStereoNode`] stereo
//! in den Graphen. Bei zwei Kanälen sind Kompressor, Limiter, Gate und De-Esser verkoppelt
//! (gemeinsame Pegelmessung), damit das Stereobild nicht wandert.
//!
//! Parameter-IDs sind die Indizes der Parametertabelle im Modell (`EffectKind::params`);
//! [`BYPASS`] schaltet den Effekt geglättet aus und ein; ist er ausgeschaltet und eingeschwungen,
//! läuft das Eingangssignal bit-genau durch. Echtzeit: Alles wird in [`Fx::new`]/`prepare`
//! angelegt, `process` und `set_param` allokieren nicht.

use crate::dynamics::{Biquad, Compressor, Gate, Limiter, db_to_gain};
use crate::effects::chorus::Chorus;
use crate::effects::deesser::DeEsser;
use crate::effects::delay::DelayFx;
use crate::effects::denoise::Denoiser;
use crate::effects::reverb::Reverb;
use crate::effects::saturation::Saturator;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::automation::AutoCurve;
use crate::{DEFAULT_RAMP_SECONDS, Node, ProcessBuffers, Smoother, StereoBuffers, StereoNode};

/// Parameter-ID für Bypass (`0.0` = aktiv, `1.0` = umgangen).
pub const BYPASS: u32 = 100;

/// Höchste Zahl Parameter eines Effekts.
pub const MAX_PARAMS: usize = 12;

/// Schrittweite, in der Automation die Regler eines Effekts nachführt (Samples).
pub const AUTOMATION_STEP: usize = 64;

/// Effektart; die Reihenfolge der Parameter entspricht dem Modell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FxKind {
    /// highpass_hz, bell_hz, bell_db, bell_q, low_shelf_hz, low_shelf_db, high_shelf_hz,
    /// high_shelf_db, bell2_hz, bell2_db, bell2_q
    Eq,
    /// threshold_db, ratio, attack_ms, release_ms, makeup_db
    Compressor,
    /// ceiling_db, release_ms
    Limiter,
    /// threshold_db, range_db, attack_ms, hold_ms, release_ms
    Gate,
    /// freq_hz, threshold_db, ratio, max_cut_db
    DeEsser,
    /// decay_s, predelay_ms, damping_pct, mix_pct
    Reverb,
    /// time_ms, sync, feedback_pct, tone_hz, pingpong, mix_pct
    Delay,
    /// rate_hz, depth_ms, delay_ms, mix_pct
    Chorus,
    /// drive_db, shape, output_db, mix_pct
    Saturation,
    /// amount_pct
    Denoise,
}

/// Umgebung, in der ein Effekt läuft.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FxContext {
    /// Projekttempo für das synchronisierte Delay.
    pub tempo_bpm: f32,
    /// Export ohne Echtzeit: Die Rauschunterdrückung rechnet dann im Aufrufer-Thread.
    pub offline: bool,
}

impl Default for FxContext {
    fn default() -> Self {
        Self {
            tempo_bpm: 120.0,
            offline: false,
        }
    }
}

/// Alle Filter des EQs eines Kanals.
#[derive(Debug, Clone, Copy)]
struct EqChain {
    hp: Biquad,
    low: Biquad,
    bell: Biquad,
    bell2: Biquad,
    high: Biquad,
}

/// Unterhalb dieses Betrags (dB) gilt ein Band als flach und wird zum Durchgang.
const FLAT_DB: f32 = 0.005;

impl EqChain {
    fn design(p: &[f32; MAX_PARAMS], rate: f32) -> Self {
        let rate = f64::from(rate.max(8000.0));
        let top = rate * 0.45;
        let hp = if p[0] >= 10.0 {
            Biquad::highpass(
                f64::from(p[0]).min(top),
                std::f64::consts::FRAC_1_SQRT_2,
                rate,
            )
        } else {
            Biquad::identity()
        };
        let band = |hz: f32, db: f32, q: f32| {
            if db.abs() < FLAT_DB {
                Biquad::identity()
            } else {
                Biquad::peaking(
                    f64::from(hz).clamp(20.0, top),
                    f64::from(q).max(0.05),
                    f64::from(db),
                    rate,
                )
            }
        };
        let low = if p[5].abs() < FLAT_DB {
            Biquad::identity()
        } else {
            Biquad::low_shelf(f64::from(p[4]).clamp(20.0, top), f64::from(p[5]), rate)
        };
        let high = if p[7].abs() < FLAT_DB {
            Biquad::identity()
        } else {
            Biquad::high_shelf(f64::from(p[6]).clamp(20.0, top), f64::from(p[7]), rate)
        };
        Self {
            hp,
            low,
            bell: band(p[1], p[2], p[3]),
            bell2: band(p[8], p[9], p[10]),
            high,
        }
    }

    fn retune(&mut self, other: &EqChain) {
        self.hp.retune(&other.hp);
        self.low.retune(&other.low);
        self.bell.retune(&other.bell);
        self.bell2.retune(&other.bell2);
        self.high.retune(&other.high);
    }

    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        let x = self.hp.run(x);
        let x = self.low.run(x);
        let x = self.bell.run(x);
        let x = self.bell2.run(x);
        self.high.run(x)
    }
}

/// Zustand der Effekte mit eigenen Puffern.
#[derive(Debug, Clone)]
enum Extra {
    None,
    DeEsser(DeEsser),
    Reverb(Box<Reverb>),
    Delay(Box<DelayFx>),
    Chorus(Box<Chorus>),
    Saturation(Saturator),
    Denoise(Box<Denoiser>),
}

/// Ein Effekt für einen oder zwei Kanäle.
#[derive(Debug, Clone)]
pub struct Fx {
    kind: FxKind,
    p: [f32; MAX_PARAMS],
    rate: f32,
    ctx: FxContext,
    channels: usize,
    /// 1 = Effekt wirkt, 0 = umgangen.
    wet: Smoother,
    dirty: bool,
    eq: [EqChain; 2],
    eq_old: [EqChain; 2],
    fade_left: u32,
    fade_len: u32,
    comp: Compressor,
    makeup: Smoother,
    lim: Limiter,
    gate: Gate,
    extra: Extra,
    /// Automationskurven je Reglerindex (Paket 18).
    lanes: Vec<(u32, AutoCurve)>,
    pos: u64,
    playing: bool,
    /// Größte Absenkung seit dem letzten Abholen, als Bits eines f32 (>= 0).
    meter: Option<Arc<AtomicU32>>,
}

impl Fx {
    /// Legt den Effekt mit den Modellwerten `params` an (fehlende Werte bleiben 0).
    pub fn new(kind: FxKind, params: &[f64], bypass: bool) -> Self {
        Self::with_context(kind, params, bypass, FxContext::default(), 2)
    }

    /// Wie [`Fx::new`], mit Umgebung und Kanalzahl (1 = Spur-Insert, 2 = Bus-Insert).
    pub fn with_context(
        kind: FxKind,
        params: &[f64],
        bypass: bool,
        ctx: FxContext,
        channels: usize,
    ) -> Self {
        let mut p = [0.0f32; MAX_PARAMS];
        for (d, s) in p.iter_mut().zip(params) {
            *d = *s as f32;
        }
        let rate = 48_000.0;
        let eq = EqChain::design(&p, rate);
        let mut fx = Self {
            kind,
            p,
            rate,
            ctx,
            channels: channels.clamp(1, 2),
            wet: Smoother::new(if bypass { 0.0 } else { 1.0 }, DEFAULT_RAMP_SECONDS),
            dirty: false,
            eq: [eq; 2],
            eq_old: [eq; 2],
            fade_left: 0,
            fade_len: 1,
            comp: Compressor::new(-18.0, 2.0, 0.01, 0.12, rate),
            makeup: Smoother::new(1.0, DEFAULT_RAMP_SECONDS),
            lim: Limiter::new(-1.0, 0.05, rate),
            gate: Gate::new(-45.0, 40.0, rate),
            extra: Extra::None,
            lanes: Vec::new(),
            pos: 0,
            playing: true,
            meter: None,
        };
        fx.prepare(rate);
        fx
    }

    /// Hängt Automationskurven an (Control-Thread). Während sie laufen, setzen sie die Regler
    /// alle [`AUTOMATION_STEP`] Samples.
    pub fn set_lanes(&mut self, lanes: Vec<(u32, AutoCurve)>) {
        self.lanes = lanes;
    }

    /// Hängt den Absenkungsmesser an (Kompressor, Gate, Limiter, De-Esser).
    pub fn set_meter(&mut self, meter: Arc<AtomicU32>) {
        self.meter = Some(meter);
    }

    /// Siehe [`Node::locate`].
    pub fn locate(&mut self, position: u64, playing: bool) {
        self.pos = position;
        self.playing = playing;
    }

    /// Rechnet alle ratenabhängigen Größen aus den Parametern neu (Control-Thread; darf
    /// allokieren und Threads starten).
    pub fn prepare(&mut self, rate: f32) {
        self.rate = rate.max(1.0);
        for (_, c) in &mut self.lanes {
            c.prepare(self.rate);
        }
        self.wet.prepare(self.rate);
        self.makeup.prepare(self.rate);
        self.fade_len = ((DEFAULT_RAMP_SECONDS * self.rate).round() as u32).max(1);
        let eq = EqChain::design(&self.p, self.rate);
        self.eq = [eq; 2];
        self.eq_old = [eq; 2];
        self.fade_left = 0;
        let p = self.p;
        let (rate, tempo) = (self.rate, self.ctx.tempo_bpm);
        self.extra = Extra::None;
        match self.kind {
            FxKind::Eq => {}
            FxKind::Compressor => {
                self.comp = Compressor::new(p[0], p[1], p[2] / 1000.0, p[3] / 1000.0, rate);
                self.makeup.reset(db_to_gain(f64::from(p[4])));
            }
            FxKind::Limiter => {
                self.lim = Limiter::new(f64::from(p[0]), p[1] / 1000.0, rate);
            }
            FxKind::Gate => {
                self.gate = Gate::new(f64::from(p[0]), f64::from(p[1]), rate);
                self.set_gate();
            }
            FxKind::DeEsser => self.extra = Extra::DeEsser(DeEsser::new(&p, rate)),
            FxKind::Reverb => self.extra = Extra::Reverb(Box::new(Reverb::new(&p, rate))),
            FxKind::Delay => self.extra = Extra::Delay(Box::new(DelayFx::new(&p, rate, tempo))),
            FxKind::Chorus => self.extra = Extra::Chorus(Box::new(Chorus::new(&p, rate))),
            FxKind::Saturation => self.extra = Extra::Saturation(Saturator::new(&p, rate)),
            FxKind::Denoise => {
                let mut d = Denoiser::new(self.channels, self.ctx.offline, p[0]);
                d.prepare(rate);
                self.extra = Extra::Denoise(Box::new(d));
            }
        }
        self.dirty = false;
    }

    fn set_gate(&mut self) {
        let p = self.p;
        self.gate.set(
            f64::from(p[0]),
            f64::from(p[1]),
            p[2] / 1000.0,
            p[3] / 1000.0,
            p[4] / 1000.0,
            self.rate,
        );
    }

    /// Setzt einen Parameter (Index wie im Modell) oder [`BYPASS`]. Echtzeitsicher.
    pub fn set_param(&mut self, param: u32, value: f32) {
        if !value.is_finite() {
            return;
        }
        if param == BYPASS {
            self.wet.set_target(if value >= 0.5 { 0.0 } else { 1.0 });
        } else if let Some(slot) = self.p.get_mut(param as usize) {
            *slot = value;
            self.dirty = true;
        }
    }

    /// Wendet geänderte Parameter an; der EQ blendet 20 ms vom alten auf das neue Filter.
    fn apply_dirty(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let p = self.p;
        let (rate, tempo) = (self.rate, self.ctx.tempo_bpm);
        match self.kind {
            FxKind::Eq => {
                self.eq_old = self.eq;
                let new = EqChain::design(&p, rate);
                for c in &mut self.eq {
                    c.retune(&new);
                }
                self.fade_left = self.fade_len;
            }
            FxKind::Compressor => {
                self.comp
                    .set(p[0], p[1], p[2] / 1000.0, p[3] / 1000.0, rate);
                self.makeup.set_target(db_to_gain(f64::from(p[4])));
            }
            FxKind::Limiter => self.lim.set(f64::from(p[0]), p[1] / 1000.0, rate),
            FxKind::Gate => self.set_gate(),
            _ => match &mut self.extra {
                Extra::None => {}
                Extra::DeEsser(e) => e.set(&p, rate),
                Extra::Reverb(e) => e.set(&p, rate),
                Extra::Delay(e) => e.set(&p, rate, tempo),
                Extra::Chorus(e) => e.set(&p, rate),
                Extra::Saturation(e) => e.set(&p),
                Extra::Denoise(e) => e.set_amount(p[0]),
            },
        }
    }

    /// Zahl der Blöcke, die der Arbeits-Thread der Rauschunterdrückung nicht rechtzeitig lieferte.
    pub fn underruns(&self) -> u32 {
        match &self.extra {
            Extra::Denoise(d) => d.underruns(),
            _ => 0,
        }
    }

    /// Verzögerung des Effekts in Samples (für einen Latenzausgleich).
    pub fn latency_samples(&self) -> usize {
        match &self.extra {
            Extra::Denoise(d) if d.is_active() => crate::effects::denoise::LATENCY,
            _ => 0,
        }
    }

    /// Verarbeitet ein Sample-Paar. `stereo` = false: nur `l` zählt.
    #[inline]
    fn step(&mut self, l: f32, r: f32, stereo: bool, key: Option<f32>) -> (f32, f32) {
        match self.kind {
            FxKind::Eq => {
                let n = if stereo { 2 } else { 1 };
                let mut out = [l, r];
                let t = if self.fade_left > 0 {
                    self.fade_left -= 1;
                    Some(self.fade_left as f32 / self.fade_len as f32)
                } else {
                    None
                };
                for (c, o) in out.iter_mut().enumerate().take(n) {
                    let x = *o;
                    let new = self.eq[c].run(x);
                    *o = match t {
                        Some(old_weight) => {
                            let old = self.eq_old[c].run(x);
                            old * old_weight + new * (1.0 - old_weight)
                        }
                        None => new,
                    };
                }
                (out[0], out[1])
            }
            FxKind::Compressor => {
                let power = match key {
                    Some(k) => k * k,
                    None if stereo => 0.5 * (l * l + r * r),
                    None => l * l,
                };
                let g = self.comp.gain(power) * self.makeup.next_value();
                (l * g, r * g)
            }
            FxKind::Limiter => {
                let peak = if stereo {
                    l.abs().max(r.abs())
                } else {
                    l.abs()
                };
                let g = self.lim.gain(peak);
                let (l, r) = (l * g, r * g);
                // Harte Sicherung gegen Überschwinger beim Angriff.
                let c = self.lim.ceiling();
                (l.clamp(-c, c), r.clamp(-c, c))
            }
            FxKind::Gate => {
                let peak = match key {
                    Some(k) => k.abs(),
                    None if stereo => l.abs().max(r.abs()),
                    None => l.abs(),
                };
                let g = self.gate.gain(peak);
                (l * g, r * g)
            }
            _ => match &mut self.extra {
                Extra::None => (l, r),
                Extra::DeEsser(e) => e.step(l, r, stereo),
                Extra::Reverb(e) => e.step(l, r, stereo),
                Extra::Delay(e) => e.step(l, r, stereo),
                Extra::Chorus(e) => e.step(l, r, stereo),
                Extra::Saturation(e) => e.step(l, r, stereo),
                Extra::Denoise(e) => e.step(l, r, stereo),
            },
        }
    }

    fn run(
        &mut self,
        in_l: &[f32],
        in_r: Option<&[f32]>,
        out_l: &mut [f32],
        out_r: Option<&mut [f32]>,
        key: Option<&[f32]>,
    ) {
        if self.lanes.is_empty() {
            self.run_span(in_l, in_r, out_l, out_r, key);
        } else {
            // Automation: Regler in kleinen Schritten nachführen.
            let len = out_l.len();
            let mut out_r = out_r;
            let mut at = 0;
            while at < len {
                let end = (at + AUTOMATION_STEP).min(len);
                let pos = self.pos;
                for (param, curve) in &mut self.lanes {
                    let v = curve.value_at(pos);
                    if let Some(slot) = self.p.get_mut(*param as usize)
                        && v.is_finite()
                        && *slot != v
                    {
                        *slot = v;
                        self.dirty = true;
                    }
                }
                self.run_span(
                    &in_l[at..end],
                    in_r.map(|b| &b[at..end]),
                    &mut out_l[at..end],
                    out_r.as_deref_mut().map(|b| &mut b[at..end]),
                    key.map(|k| &k[at..end]),
                );
                if self.playing {
                    self.pos += (end - at) as u64;
                }
                at = end;
            }
        }
        if let Some(m) = &self.meter {
            let red = self.reduction_db().max(0.0);
            if red.is_finite() {
                m.fetch_max(red.to_bits(), Ordering::Relaxed);
            }
        }
    }

    fn run_span(
        &mut self,
        in_l: &[f32],
        in_r: Option<&[f32]>,
        out_l: &mut [f32],
        mut out_r: Option<&mut [f32]>,
        key: Option<&[f32]>,
    ) {
        self.apply_dirty();
        let stereo = in_r.is_some() && out_r.is_some();
        for i in 0..out_l.len() {
            let dl = in_l[i];
            let dr = in_r.map_or(dl, |b| b[i]);
            let w = self.wet.next_value();
            if w <= 0.0 && !self.wet.is_smoothing() {
                out_l[i] = dl;
                if let Some(o) = out_r.as_deref_mut() {
                    o[i] = dr;
                }
                continue;
            }
            let (wl, wr) = self.step(dl, dr, stereo, key.map(|k| k[i]));
            out_l[i] = dl + (wl - dl) * w;
            if let Some(o) = out_r.as_deref_mut() {
                o[i] = dr + (wr - dr) * w;
            }
        }
    }

    /// Absenkung von Kompressor, Limiter, Gate oder De-Esser in dB (für Anzeigen), sonst 0.
    pub fn reduction_db(&self) -> f32 {
        match (self.kind, &self.extra) {
            (FxKind::Compressor, _) => self.comp.reduction_db(),
            (FxKind::Limiter, _) => self.lim.reduction_db(),
            (FxKind::Gate, _) => self.gate.reduction_db(),
            (_, Extra::DeEsser(d)) => d.reduction_db(),
            _ => 0.0,
        }
    }
}

/// [`Fx`] als Mono-Node (Spur-Inserts).
#[derive(Debug, Clone)]
pub struct FxNode(Fx);

impl FxNode {
    /// Blöcke, die der Arbeits-Thread der Rauschunterdrückung nicht rechtzeitig lieferte.
    pub fn underruns(&self) -> u32 {
        self.0.underruns()
    }

    pub fn new(kind: FxKind, params: &[f64], bypass: bool) -> Self {
        Self(Fx::with_context(
            kind,
            params,
            bypass,
            FxContext::default(),
            1,
        ))
    }

    /// Mit Projekttempo und Betriebsart.
    pub fn with_context(kind: FxKind, params: &[f64], bypass: bool, ctx: FxContext) -> Self {
        Self(Fx::with_context(kind, params, bypass, ctx, 1))
    }

    /// Verzögerung in Samples.
    pub fn latency_samples(&self) -> usize {
        self.0.latency_samples()
    }

    /// Siehe [`Fx::set_lanes`].
    pub fn set_lanes(&mut self, lanes: Vec<(u32, AutoCurve)>) {
        self.0.set_lanes(lanes);
    }

    /// Siehe [`Fx::set_meter`].
    pub fn set_meter(&mut self, meter: Arc<AtomicU32>) {
        self.0.set_meter(meter);
    }
}

impl Node for FxNode {
    fn prepare(&mut self, sample_rate: f32) {
        self.0.prepare(sample_rate);
    }

    fn set_param(&mut self, param: u32, value: f32) {
        self.0.set_param(param, value);
    }

    fn locate(&mut self, position: u64, playing: bool) {
        self.0.locate(position, playing);
    }

    fn process(&mut self, buffers: ProcessBuffers<'_>) {
        self.0.run(buffers.input, None, buffers.output, None, None);
    }
}

/// [`Fx`] als Stereo-Node (Bus-Inserts), Kompressor und Limiter verkoppelt.
#[derive(Debug, Clone)]
pub struct FxStereoNode(Fx);

impl FxStereoNode {
    pub fn new(kind: FxKind, params: &[f64], bypass: bool) -> Self {
        Self(Fx::with_context(
            kind,
            params,
            bypass,
            FxContext::default(),
            2,
        ))
    }

    /// Mit Projekttempo und Betriebsart.
    pub fn with_context(kind: FxKind, params: &[f64], bypass: bool, ctx: FxContext) -> Self {
        Self(Fx::with_context(kind, params, bypass, ctx, 2))
    }

    /// Verzögerung in Samples.
    pub fn latency_samples(&self) -> usize {
        self.0.latency_samples()
    }

    /// Siehe [`Fx::set_lanes`].
    pub fn set_lanes(&mut self, lanes: Vec<(u32, AutoCurve)>) {
        self.0.set_lanes(lanes);
    }

    /// Siehe [`Fx::set_meter`].
    pub fn set_meter(&mut self, meter: Arc<AtomicU32>) {
        self.0.set_meter(meter);
    }
}

impl StereoNode for FxStereoNode {
    fn prepare(&mut self, sample_rate: f32) {
        self.0.prepare(sample_rate);
    }

    fn set_param(&mut self, param: u32, value: f32) {
        self.0.set_param(param, value);
    }

    fn locate(&mut self, position: u64, playing: bool) {
        self.0.locate(position, playing);
    }

    fn process(&mut self, b: StereoBuffers<'_>) {
        self.0.run(
            b.in_left,
            Some(b.in_right),
            b.out_left,
            Some(b.out_right),
            None,
        );
    }
}

/// Kompressor oder Gate einer Spur mit Sidechain: Kanal 0 ist das Signal, Kanal 1 das
/// Schlüsselsignal, das den Pegel misst. Ausgang ist nur Kanal 0.
#[derive(Debug, Clone)]
pub struct KeyedFxNode(Fx);

impl KeyedFxNode {
    /// Mit Projekttempo und Betriebsart (ein Kanal).
    pub fn with_context(kind: FxKind, params: &[f64], bypass: bool, ctx: FxContext) -> Self {
        Self(Fx::with_context(kind, params, bypass, ctx, 1))
    }

    /// Siehe [`Fx::set_lanes`].
    pub fn set_lanes(&mut self, lanes: Vec<(u32, AutoCurve)>) {
        self.0.set_lanes(lanes);
    }

    /// Siehe [`Fx::set_meter`].
    pub fn set_meter(&mut self, meter: Arc<AtomicU32>) {
        self.0.set_meter(meter);
    }
}

impl StereoNode for KeyedFxNode {
    fn prepare(&mut self, sample_rate: f32) {
        self.0.prepare(sample_rate);
    }

    fn set_param(&mut self, param: u32, value: f32) {
        self.0.set_param(param, value);
    }

    fn locate(&mut self, position: u64, playing: bool) {
        self.0.locate(position, playing);
    }

    fn process(&mut self, b: StereoBuffers<'_>) {
        self.0
            .run(b.in_left, None, b.out_left, None, Some(b.in_right));
        b.out_right.fill(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn sine(freq: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (std::f32::consts::TAU * freq * i as f32 / SR).sin())
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
    }

    fn run_mono(kind: FxKind, params: &[f64], input: &[f32]) -> Vec<f32> {
        let mut n = FxNode::new(kind, params, false);
        n.prepare(SR);
        let mut out = vec![0.0; input.len()];
        for (i, o) in input.chunks(256).zip(out.chunks_mut(256)) {
            n.process(ProcessBuffers {
                input: i,
                output: o,
            });
        }
        out
    }

    #[test]
    fn eq_highpass_cuts_rumble_and_bell_boosts() {
        let rumble = sine(30.0, 0.5, 48_000);
        let out = run_mono(FxKind::Eq, &[120.0, 1000.0, 0.0, 1.0], &rumble);
        assert!(rms(&out[24_000..]) < 0.25 * rms(&rumble[24_000..]));

        let tone = sine(1000.0, 0.25, 48_000);
        let out = run_mono(FxKind::Eq, &[0.0, 1000.0, 12.0, 1.0], &tone);
        let gain_db = 20.0 * (rms(&out[24_000..]) / rms(&tone[24_000..])).log10();
        assert!((gain_db - 12.0).abs() < 0.5, "{gain_db}");
        // Flach eingestellt ändert der EQ nichts.
        let flat = run_mono(FxKind::Eq, &[0.0, 1000.0, 0.0, 1.0], &tone);
        let err = flat
            .iter()
            .zip(&tone)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(err < 1e-4, "{err}");
    }

    #[test]
    fn compressor_lowers_loud_signal() {
        let loud = sine(440.0, 0.8, 48_000);
        let out = run_mono(FxKind::Compressor, &[-24.0, 4.0, 5.0, 100.0, 0.0], &loud);
        assert!(rms(&out[24_000..]) < 0.7 * rms(&loud[24_000..]));
        let quiet = sine(440.0, 0.01, 48_000);
        let out = run_mono(FxKind::Compressor, &[-24.0, 4.0, 5.0, 100.0, 0.0], &quiet);
        let r = rms(&out[24_000..]) / rms(&quiet[24_000..]);
        assert!((r - 1.0).abs() < 0.02, "{r}");
    }

    #[test]
    fn limiter_holds_the_ceiling() {
        let loud = sine(220.0, 1.5, 48_000);
        let out = run_mono(FxKind::Limiter, &[-6.0, 50.0], &loud);
        let ceiling = db_to_gain(-6.0);
        assert!(out.iter().all(|s| s.abs() <= ceiling + 1e-5));
    }

    #[test]
    fn stereo_compressor_is_linked() {
        // Nur links laut: rechts muss mit abgesenkt werden.
        let l = sine(440.0, 0.9, 48_000);
        let r = sine(440.0, 0.05, 48_000);
        let mut n = FxStereoNode::new(FxKind::Compressor, &[-24.0, 6.0, 2.0, 100.0, 0.0], false);
        n.prepare(SR);
        let (mut ol, mut or) = (vec![0.0; l.len()], vec![0.0; l.len()]);
        n.process(StereoBuffers {
            in_left: &l,
            in_right: &r,
            out_left: &mut ol,
            out_right: &mut or,
        });
        let g_r = rms(&or[24_000..]) / rms(&r[24_000..]);
        assert!(g_r < 0.8, "right not reduced: {g_r}");
    }

    #[test]
    fn bypass_is_smooth_and_exact_when_settled() {
        let tone = sine(1000.0, 0.25, 9600);
        let mut n = FxNode::new(FxKind::Eq, &[0.0, 1000.0, 12.0, 1.0], false);
        n.prepare(SR);
        n.set_param(BYPASS, 1.0);
        let mut out = vec![0.0; tone.len()];
        n.process(ProcessBuffers {
            input: &tone,
            output: &mut out,
        });
        // Nach der Rampe (20 ms = 960 Samples) identisch mit dem Eingang.
        assert_eq!(&out[2000..], &tone[2000..]);
        let step = out
            .windows(2)
            .zip(tone.windows(2))
            .map(|(o, t)| ((o[1] - o[0]) - (t[1] - t[0])).abs())
            .fold(0.0, f32::max);
        assert!(step < 0.1, "bypass click {step}");
    }

    #[test]
    fn eq_param_change_does_not_click() {
        let tone = sine(1000.0, 0.25, 9600);
        let mut n = FxNode::new(FxKind::Eq, &[0.0, 1000.0, 0.0, 1.0], false);
        n.prepare(SR);
        let mut out = vec![0.0; tone.len()];
        n.process(ProcessBuffers {
            input: &tone[..4800],
            output: &mut out[..4800],
        });
        n.set_param(2, 18.0);
        n.process(ProcessBuffers {
            input: &tone[4800..],
            output: &mut out[4800..],
        });
        let max_step = out
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        // Eine 1-kHz-Sinus mit Amplitude 0.25 hat höchstens 0.033 je Sample; mit +18 dB
        // wächst sie auf etwa 2.0, der Sprung je Sample bleibt ein Bruchteil davon.
        assert!(max_step < 0.5, "{max_step}");
    }

    #[test]
    fn no_alloc_in_process() {
        use assert_no_alloc::assert_no_alloc;
        let tone = sine(1000.0, 0.25, 512);
        for (kind, params) in [
            (FxKind::Eq, vec![80.0, 3000.0, 6.0, 2.0]),
            (FxKind::Compressor, vec![-20.0, 3.0, 5.0, 80.0, 3.0]),
            (FxKind::Limiter, vec![-3.0, 40.0]),
        ] {
            let mut n = FxStereoNode::new(kind, &params, false);
            n.prepare(SR);
            let mut ol = vec![0.0; 512];
            let mut or = vec![0.0; 512];
            assert_no_alloc(|| {
                n.set_param(0, 1.0);
                n.set_param(BYPASS, 1.0);
                n.process(StereoBuffers {
                    in_left: &tone,
                    in_right: &tone,
                    out_left: &mut ol,
                    out_right: &mut or,
                });
            });
        }
    }
}
