//! DJ-Paket: Bausteine für die Party-Ansicht „DJ“, alle echtzeitfest (keine Allokation, kein
//! Lock, kein I/O; alle Puffer liegen fest in den Strukturen).
//!
//! - [`DeckReader`] spielt einen Titel aus dem Speicher mit beliebigem Tempo: ohne Keylock als
//!   Varispeed (kubisch, auch rückwärts zum Scratchen), mit Keylock als laufendes WSOLA, das die
//!   Tonhöhe hält. Das ist dieselbe Idee wie [`crate::stretch`] (offline, beim Graph-Bau), hier
//!   aber Block für Block im Audio-Thread mit fester Fensterlänge.
//! - [`DjEq`]: Hi/Mid/Lo als Linkwitz-Riley-Bandteilung (4. Ordnung), so verschwindet ein Band
//!   bei Kill wirklich. In Mittelstellung aller Regler wird nach kurzer Blende ganz umgangen
//!   (bitgenau unverändert).
//! - [`DjFilter`]: ein Regler, links Tiefpass, rechts Hochpass, in der Mitte umgangen.
//! - [`xfader_gains`] (Kurve weich/hart) und [`headphone_gains`] (Kopfhörer Cue↔Master).

use std::f64::consts::PI;

/// Fensterlänge des Keylock-WSOLA (Frames). 21 ms bei 48 kHz: kurz genug für Schläge.
const WIN: usize = 1024;
/// Ausgabeschritt (halbes Fenster, Hann mit 50 % Überlappung summiert sich zu 1).
const HOP: usize = WIN / 2;
/// Suchweite für die Fensterlage nach beiden Seiten (Frames).
const TOL: isize = 256;
/// Schrittweite der groben Suche.
const DEC: usize = 4;

/// Steuerung des Lesers für einen Frame (vom Mixer je Seite gehalten).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReadCtl {
    /// Abspielgeschwindigkeit: 1 normal, 1,08 = +8 %, negativ = rückwärts (Scratch).
    pub speed: f64,
    /// Tonhöhe beim Tempoändern halten.
    pub keylock: bool,
    /// Spielbereich (Cue-In bis Cue-Out) in Frames.
    pub start: f64,
    pub end: f64,
    /// Aktive Schleife (Anfang, Ende) in Frames.
    pub looped: Option<(f64, f64)>,
}

impl ReadCtl {
    /// Ganzer Titel mit `len` Frames, normales Tempo, ohne Keylock und Schleife.
    pub fn whole(len: usize) -> Self {
        Self {
            speed: 1.0,
            keylock: false,
            start: 0.0,
            end: len as f64,
            looped: None,
        }
    }
}

/// Liest einen Stereo-Titel mit Tempo, Keylock, Scratch und Schleife.
pub struct DeckReader {
    /// Stelle im Titel (Frames, gebrochen) des nächsten Ausgabe-Frames.
    pos: f64,
    /// WSOLA hat gültigen Zustand (sonst beim nächsten Keylock-Frame neu ansetzen).
    wsola: bool,
    /// Anfang des zuletzt gewählten Fensters im Titel.
    prev_k: isize,
    queue_l: [f32; HOP],
    queue_r: [f32; HOP],
    qi: usize,
    tail_l: [f32; HOP],
    tail_r: [f32; HOP],
    win: [f32; WIN],
}

impl Default for DeckReader {
    fn default() -> Self {
        Self::new()
    }
}

#[inline]
fn at(x: &[f32], i: isize) -> f32 {
    if i >= 0 {
        x.get(i as usize).copied().unwrap_or(0.0)
    } else {
        0.0
    }
}

impl DeckReader {
    /// Neuer Leser am Anfang (im Control-Thread anlegen).
    pub fn new() -> Self {
        let mut win = [0.0f32; WIN];
        for (j, w) in win.iter_mut().enumerate() {
            *w = (0.5 - 0.5 * (2.0 * PI * j as f64 / WIN as f64).cos()) as f32;
        }
        Self {
            pos: 0.0,
            wsola: false,
            prev_k: 0,
            queue_l: [0.0; HOP],
            queue_r: [0.0; HOP],
            qi: HOP,
            tail_l: [0.0; HOP],
            tail_r: [0.0; HOP],
            win,
        }
    }

    /// Springt an Frame `pos` (Hot Cue, Cue, Klick in die Übersicht).
    pub fn seek(&mut self, pos: f64) {
        if pos.is_finite() {
            self.pos = pos.max(0.0);
            self.wsola = false;
        }
    }

    /// Stelle des nächsten Frames im Titel.
    pub fn position(&self) -> f64 {
        self.pos
    }

    /// Steht der Leser am Ende des Spielbereichs (vorwärts)?
    pub fn at_end(&self, c: &ReadCtl) -> bool {
        self.pos >= c.end
    }

    /// Nächster Stereo-Frame. Außerhalb des Titels Stille; am Ende bleibt die Stelle stehen.
    #[inline]
    pub fn next(&mut self, l: &[f32], r: &[f32], c: &ReadCtl) -> (f32, f32) {
        let len = l.len().min(r.len()) as f64;
        let end = c.end.min(len);
        let speed = if c.speed.is_finite() { c.speed } else { 1.0 };
        if speed >= 0.0 && self.pos >= end {
            self.pos = end.max(c.start);
            self.wsola = false;
            return (0.0, 0.0);
        }
        if speed < 0.0 && self.pos <= c.start {
            self.pos = c.start;
            self.wsola = false;
            return (0.0, 0.0);
        }
        let stretch = c.keylock && speed > 0.0 && (speed - 1.0).abs() > 1e-9;
        let out = if stretch {
            if !self.wsola {
                self.reset_wsola(l, r);
            }
            if self.qi >= HOP {
                self.hop(l, r);
            }
            let v = (self.queue_l[self.qi], self.queue_r[self.qi]);
            self.qi += 1;
            v
        } else {
            self.wsola = false;
            (cubic(l, self.pos), cubic(r, self.pos))
        };
        self.pos += speed;
        if let Some((a, b)) = c.looped
            && b > a
        {
            if speed > 0.0 && self.pos >= b {
                self.pos -= b - a;
                self.wsola = false;
            } else if speed < 0.0 && self.pos < a {
                self.pos += b - a;
                self.wsola = false;
            }
        }
        out
    }

    /// Setzt das WSOLA an der aktuellen Stelle an: das „vorige“ Fenster endet genau hier,
    /// so beginnt die Ausgabe nahtlos.
    fn reset_wsola(&mut self, l: &[f32], r: &[f32]) {
        let a = self.pos.round() as isize;
        self.prev_k = a - HOP as isize;
        for j in 0..HOP {
            let w = self.win[j + HOP];
            self.tail_l[j] = w * at(l, a + j as isize);
            self.tail_r[j] = w * at(r, a + j as isize);
        }
        self.qi = HOP;
        self.wsola = true;
    }

    /// Rechnet den nächsten Ausgabeschritt: Fensterlage nahe der Soll-Stelle so wählen, dass
    /// sie die Wellenform des vorigen Fensters fortsetzt, und überlappend addieren.
    fn hop(&mut self, l: &[f32], r: &[f32]) {
        let a = self.pos.round() as isize;
        let nat = self.prev_k + HOP as isize;
        let k = search(l, r, a, nat);
        for j in 0..HOP {
            let ji = j as isize;
            let w = self.win[j];
            self.queue_l[j] = self.tail_l[j] + w * at(l, k + ji);
            self.queue_r[j] = self.tail_r[j] + w * at(r, k + ji);
            let w2 = self.win[j + HOP];
            self.tail_l[j] = w2 * at(l, k + HOP as isize + ji);
            self.tail_r[j] = w2 * at(r, k + HOP as isize + ji);
        }
        self.prev_k = k;
        self.qi = 0;
    }
}

/// Ähnlichkeit (normierte Kreuzkorrelation, Mono) des Abschnitts ab `k` mit dem ab `nat`.
#[inline]
fn corr(l: &[f32], r: &[f32], k: isize, nat: isize, stride: usize) -> f32 {
    let (mut num, mut e) = (0.0f32, 1e-9f32);
    let mut j = 0isize;
    while j < HOP as isize {
        let m = at(l, k + j) + at(r, k + j);
        let n = at(l, nat + j) + at(r, nat + j);
        num += m * n;
        e += m * m;
        j += stride as isize;
    }
    num / e.sqrt()
}

/// Beste Fensterlage in `a ± TOL`: erst grob, dann fein. Bei Gleichstand gewinnt die
/// Soll-Stelle selbst (bei Tempo 1 ergibt das genau die Eingabe).
fn search(l: &[f32], r: &[f32], a: isize, nat: isize) -> isize {
    let mut best = (a, corr(l, r, a, nat, DEC));
    let mut k = a - TOL;
    while k <= a + TOL {
        let s = corr(l, r, k, nat, DEC);
        if s > best.1 + 1e-6 {
            best = (k, s);
        }
        k += DEC as isize;
    }
    let coarse = best.0;
    let mut fine = (coarse, corr(l, r, coarse, nat, 1));
    for k in coarse - DEC as isize + 1..coarse + DEC as isize {
        if k == coarse {
            continue;
        }
        let s = corr(l, r, k, nat, 1);
        if s > fine.1 + 1e-6 {
            fine = (k, s);
        }
    }
    fine.0
}

/// Kubische Interpolation (Catmull-Rom) an der gebrochenen Stelle `pos`.
#[inline]
fn cubic(x: &[f32], pos: f64) -> f32 {
    let i = pos.floor();
    let t = (pos - i) as f32;
    let i = i as isize;
    let p1 = at(x, i);
    if t == 0.0 {
        return p1;
    }
    let (p0, p2, p3) = (at(x, i - 1), at(x, i + 1), at(x, i + 2));
    let a = -0.5 * p0 + 1.5 * p1 - 1.5 * p2 + 0.5 * p3;
    let b = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
    let c = -0.5 * p0 + 0.5 * p2;
    ((a * t + b) * t + c) * t + p1
}

// ---- EQ und Filter --------------------------------------------------------------------------

/// Biquad (transponierte Direktform II), Zustand in f64.
#[derive(Debug, Clone, Copy, Default)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    /// Butterworth-Tief- oder Hochpass (Q = 1/√2), RBJ-Formeln.
    fn butter(rate: u32, hz: f64, high: bool) -> Self {
        let w = 2.0 * PI * hz / f64::from(rate.max(1));
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        let (b0, b1, b2) = if high {
            ((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0)
        } else {
            ((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0)
        };
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: -2.0 * c / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    #[inline]
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// Linkwitz-Riley 4. Ordnung: zwei gleiche Butterworth-Stufen hintereinander.
#[derive(Debug, Clone, Copy, Default)]
struct Lr4([Biquad; 2]);

impl Lr4 {
    fn new(rate: u32, hz: f64, high: bool) -> Self {
        let b = Biquad::butter(rate, hz, high);
        Self([b, b])
    }

    #[inline]
    fn run(&mut self, x: f64) -> f64 {
        let y = self.0[0].run(x);
        self.0[1].run(y)
    }
}

/// Bandteilung eines Kanals in Lo, Mid, Hi (phasengleich, die Summe ist ein Allpass).
#[derive(Debug, Clone, Copy, Default)]
struct Split {
    lo: Lr4,
    rest: Lr4,
    mid: Lr4,
    hi: Lr4,
    /// Allpass für Lo (LP + HP bei der oberen Trennfrequenz), damit Lo zu Mid/Hi passt.
    ap_lo: Lr4,
    ap_hi: Lr4,
}

/// Trennfrequenzen Lo/Mid und Mid/Hi.
const EQ_LOW_HZ: f64 = 250.0;
const EQ_HIGH_HZ: f64 = 2500.0;

impl Split {
    fn new(rate: u32) -> Self {
        Self {
            lo: Lr4::new(rate, EQ_LOW_HZ, false),
            rest: Lr4::new(rate, EQ_LOW_HZ, true),
            mid: Lr4::new(rate, EQ_HIGH_HZ, false),
            hi: Lr4::new(rate, EQ_HIGH_HZ, true),
            ap_lo: Lr4::new(rate, EQ_HIGH_HZ, false),
            ap_hi: Lr4::new(rate, EQ_HIGH_HZ, true),
        }
    }

    #[inline]
    fn run(&mut self, x: f64, g: [f32; 3]) -> f64 {
        let lo = self.lo.run(x);
        let lo = self.ap_lo.run(lo) + self.ap_hi.run(lo);
        let rest = self.rest.run(x);
        let mid = self.mid.run(rest);
        let hi = self.hi.run(rest);
        f64::from(g[0]) * lo + f64::from(g[1]) * mid + f64::from(g[2]) * hi
    }
}

/// Glättung der Regler (Sekunden, Zeitkonstante).
const SMOOTH_TAU: f64 = 0.02;

fn smooth_coef(rate: u32) -> f32 {
    (1.0 - (-1.0 / (SMOOTH_TAU * f64::from(rate.max(1)))).exp()) as f32
}

/// Eine Glättung auf `to`; nahe genug wird genau `to` (so endet eine Blende exakt).
#[inline]
fn approach(v: &mut f32, to: f32, k: f32) {
    *v += (to - *v) * k;
    if (to - *v).abs() < 1e-5 {
        *v = to;
    }
}

/// Reglerstellung des EQ (−1 links … 0 Mitte … +1 rechts) als linearer Faktor:
/// Mitte 0 dB, rechts +6 dB, links bis −40 dB, ganz links Kill (0).
pub fn eq_gain(knob: f32) -> f32 {
    let k = if knob.is_finite() {
        knob.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    if k <= -0.98 {
        0.0
    } else if k >= 0.0 {
        10f32.powf(6.0 * k / 20.0)
    } else {
        10f32.powf(40.0 * k / 20.0)
    }
}

/// Dreiband-EQ eines Decks (Stereo) mit Kill.
pub struct DjEq {
    split: [Split; 2],
    target: [f32; 3],
    gains: [f32; 3],
    /// Anteil des bearbeiteten Signals (0 = umgangen).
    wet: f32,
    k: f32,
}

impl DjEq {
    pub fn new(rate: u32) -> Self {
        Self {
            split: [Split::new(rate); 2],
            target: [1.0; 3],
            gains: [1.0; 3],
            wet: 0.0,
            k: smooth_coef(rate),
        }
    }

    /// Lineare Faktoren für Lo, Mid, Hi (siehe [`eq_gain`]).
    pub fn set(&mut self, lo: f32, mid: f32, hi: f32) {
        let ok = |g: f32| {
            if g.is_finite() {
                g.clamp(0.0, 4.0)
            } else {
                1.0
            }
        };
        self.target = [ok(lo), ok(mid), ok(hi)];
    }

    /// Springt ohne Glättung auf die Zielwerte (Laden, Tests).
    pub fn snap(&mut self) {
        self.gains = self.target;
        self.wet = if self.neutral() { 0.0 } else { 1.0 };
    }

    fn neutral(&self) -> bool {
        self.target == [1.0; 3]
    }

    /// Ein Stereo-Frame. Die Bänder laufen immer mit (warm), damit das Zuschalten nicht knackt.
    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let k = self.k;
        for i in 0..3 {
            approach(&mut self.gains[i], self.target[i], k);
        }
        let want = if self.neutral() { 0.0 } else { 1.0 };
        approach(&mut self.wet, want, k);
        let wl = self.split[0].run(f64::from(l), self.gains) as f32;
        let wr = self.split[1].run(f64::from(r), self.gains) as f32;
        if self.wet == 0.0 {
            (l, r)
        } else if self.wet == 1.0 {
            (wl, wr)
        } else {
            let d = 1.0 - self.wet;
            (d * l + self.wet * wl, d * r + self.wet * wr)
        }
    }
}

/// Unter diesem Betrag steht der Filter-Regler in der Mitte (umgangen).
const FILTER_DEAD: f32 = 0.02;

/// Zustandsvariablen-Filter (TPT nach Simper) für einen Kanal.
#[derive(Debug, Clone, Copy, Default)]
struct Svf {
    ic1: f64,
    ic2: f64,
}

impl Svf {
    /// Gibt (Tiefpass, Hochpass) zurück.
    #[inline]
    fn run(&mut self, x: f64, g: f64, k: f64) -> (f64, f64) {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = x - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        let lp = v2;
        let hp = x - k * v1 - lp;
        (lp, hp)
    }
}

/// DJ-Filter eines Decks: −1 … 0 Tiefpass (bis 100 Hz), 0 … +1 Hochpass (bis 10 kHz).
pub struct DjFilter {
    svf: [Svf; 2],
    rate: f64,
    target: f32,
    value: f32,
    wet: f32,
    k: f32,
}

impl DjFilter {
    pub fn new(rate: u32) -> Self {
        Self {
            svf: [Svf::default(); 2],
            rate: f64::from(rate.max(1)),
            target: 0.0,
            value: 0.0,
            wet: 0.0,
            k: smooth_coef(rate),
        }
    }

    /// Reglerstellung −1 … +1.
    pub fn set(&mut self, v: f32) {
        self.target = if v.is_finite() {
            v.clamp(-1.0, 1.0)
        } else {
            0.0
        };
    }

    pub fn snap(&mut self) {
        self.value = self.target;
        self.wet = if self.target.abs() < FILTER_DEAD {
            0.0
        } else {
            1.0
        };
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        approach(&mut self.value, self.target, self.k);
        let want = if self.target.abs() < FILTER_DEAD {
            0.0
        } else {
            1.0
        };
        approach(&mut self.wet, want, self.k);
        let v = f64::from(self.value);
        let high = v > 0.0;
        let fc = if high {
            20.0 * (10_000.0f64 / 20.0).powf(v)
        } else {
            20_000.0 * (100.0f64 / 20_000.0).powf(-v)
        };
        let fc = fc.min(0.45 * self.rate);
        let g = (PI * fc / self.rate).tan();
        // Leichte Resonanz wie an DJ-Mixern (Q ≈ 0,8).
        let k = 1.25;
        let pick = |(lp, hp): (f64, f64)| if high { hp } else { lp };
        let wl = pick(self.svf[0].run(f64::from(l), g, k)) as f32;
        let wr = pick(self.svf[1].run(f64::from(r), g, k)) as f32;
        if self.wet == 0.0 {
            (l, r)
        } else if self.wet == 1.0 {
            (wl, wr)
        } else {
            let d = 1.0 - self.wet;
            (d * l + self.wet * wl, d * r + self.wet * wr)
        }
    }
}

// ---- Crossfader und Kopfhörer ---------------------------------------------------------------

/// Kurve des Crossfaders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Curve {
    /// Konstante Leistung, für lange Übergänge.
    #[default]
    Soft,
    /// Beide voll bis kurz vor den Rand (Scratch-Kurve).
    Hard,
}

/// Breite der Blendzone der harten Kurve am Rand (Anteil des Wegs).
const HARD_EDGE: f32 = 0.06;

/// Faktoren (A, B) für die Crossfader-Stellung `x` (0 = A, 1 = B), exakte Endpunkte.
pub fn xfader_gains(x: f32, curve: Curve) -> (f32, f32) {
    let x = if x.is_finite() {
        x.clamp(0.0, 1.0)
    } else {
        0.5
    };
    match curve {
        Curve::Soft => {
            if x <= 0.0 {
                (1.0, 0.0)
            } else if x >= 1.0 {
                (0.0, 1.0)
            } else {
                let a = x * std::f32::consts::FRAC_PI_2;
                (a.cos(), a.sin())
            }
        }
        Curve::Hard => (
            ((1.0 - x) / HARD_EDGE).clamp(0.0, 1.0),
            (x / HARD_EDGE).clamp(0.0, 1.0),
        ),
    }
}

/// Kopfhörer-Mix (0 = nur Cue, 1 = nur Master): Faktoren (Cue, Master), konstante Leistung.
pub fn headphone_gains(mix: f32) -> (f32, f32) {
    xfader_gains(mix, Curve::Soft)
}
