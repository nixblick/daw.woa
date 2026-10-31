//! Lautheit nach ITU-R BS.1770-4 / EBU R128 (momentan, kurzzeitig, integriert mit Gating)
//! und True Peak (4-fach überabgetastet).

use std::collections::VecDeque;

/// Biquad, direkte Form I, f64 für die Messung.
#[derive(Clone, Copy)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Biquad {
    fn new(b: [f64; 3], a: [f64; 2]) -> Self {
        Self {
            b,
            a,
            x: [0.0; 2],
            y: [0.0; 2],
        }
    }

    fn run(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1]
            - self.a[0] * self.y[0]
            - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// K-Bewertung (Höhen-Shelf plus Hochpass) für einen Kanal, für beliebige Abtastraten.
#[derive(Clone, Copy)]
pub struct KWeighting {
    stages: [Biquad; 2],
}

impl KWeighting {
    /// Filter für `rate` Hz.
    pub fn new(rate: u32) -> Self {
        let fs = f64::from(rate);
        let (f0, g, q) = (
            1_681.974_450_955_533,
            3.999_843_853_973_347,
            0.707_175_236_955_419_6,
        );
        let k = (std::f64::consts::PI * f0 / fs).tan();
        let vh = 10f64.powf(g / 20.0);
        let vb = vh.powf(0.499_666_774_154_541_6);
        let a0 = 1.0 + k / q + k * k;
        let shelf = Biquad::new(
            [
                (vh + vb * k / q + k * k) / a0,
                2.0 * (k * k - vh) / a0,
                (vh - vb * k / q + k * k) / a0,
            ],
            [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        );
        let (f0, q) = (38.135_470_876_024_44, 0.500_327_037_323_877_3);
        let k = (std::f64::consts::PI * f0 / fs).tan();
        let a0 = 1.0 + k / q + k * k;
        let hp = Biquad::new(
            [1.0, -2.0, 1.0],
            [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        );
        Self {
            stages: [shelf, hp],
        }
    }

    /// Filtert ein Sample.
    pub fn run(&mut self, x: f64) -> f64 {
        let y = self.stages[0].run(x);
        self.stages[1].run(y)
    }
}

/// Lautheit eines Mittelquadrats in LUFS.
pub fn lufs_of(mean_square: f64) -> f64 {
    -0.691 + 10.0 * mean_square.log10()
}

/// Gating nach BS.1770: absolut −70 LUFS, relativ −10 LU. `blocks` sind Mittelquadrate
/// der 400-ms-Blöcke. `None`, wenn nichts übrig bleibt.
pub fn gated_lufs(blocks: &[f64]) -> Option<f64> {
    let above: Vec<f64> = blocks
        .iter()
        .copied()
        .filter(|&z| z > 0.0 && lufs_of(z) > -70.0)
        .collect();
    if above.is_empty() {
        return None;
    }
    let rel = lufs_of(above.iter().sum::<f64>() / above.len() as f64) - 10.0;
    let kept: Vec<f64> = above.into_iter().filter(|&z| lufs_of(z) > rel).collect();
    if kept.is_empty() {
        return None;
    }
    Some(lufs_of(kept.iter().sum::<f64>() / kept.len() as f64))
}

/// Schritte je Sekunde: Die Blöcke überlappen zu 75 %, ein Schritt sind 100 ms.
const STEPS_PER_SECOND: usize = 10;
/// Schritte im kurzzeitigen Fenster (3 s).
const SHORT_STEPS: usize = 30;
/// Schritte im momentanen Fenster (400 ms).
const MOMENTARY_STEPS: usize = 4;

/// Laufende Lautheitsmessung: momentan (400 ms), kurzzeitig (3 s), integriert (gegatet) und
/// die größten Werte seit dem Zurücksetzen. Samples dürfen beliebig zerstückelt kommen.
pub struct LoudnessMeter {
    kl: KWeighting,
    kr: KWeighting,
    step_len: usize,
    step_pos: usize,
    step_acc: f64,
    /// Die letzten höchstens 30 Schritt-Energien (Summe der Quadrate).
    steps: VecDeque<f64>,
    /// Mittelquadrate aller 400-ms-Blöcke seit dem Zurücksetzen (für die integrierte Lautheit).
    blocks: Vec<f64>,
    max_momentary: Option<f64>,
    max_short: Option<f64>,
}

impl LoudnessMeter {
    /// Neue Messung für `rate` Hz.
    pub fn new(rate: u32) -> Self {
        let step_len = (rate as usize / STEPS_PER_SECOND).max(1);
        Self {
            kl: KWeighting::new(rate),
            kr: KWeighting::new(rate),
            step_len,
            step_pos: 0,
            step_acc: 0.0,
            steps: VecDeque::with_capacity(SHORT_STEPS + 1),
            blocks: Vec::new(),
            max_momentary: None,
            max_short: None,
        }
    }

    /// Fängt neu an (Filter, Fenster, integrierter Wert, Höchstwerte).
    pub fn reset(&mut self, rate: u32) {
        *self = Self::new(rate);
    }

    /// Nimmt Stereo-Samples; beide Slices werden auf die kürzere Länge gekürzt.
    pub fn push(&mut self, left: &[f32], right: &[f32]) {
        for (&l, &r) in left.iter().zip(right) {
            let l = self.kl.run(f64::from(l));
            let r = self.kr.run(f64::from(r));
            self.step_acc += l * l + r * r;
            self.step_pos += 1;
            if self.step_pos == self.step_len {
                self.finish_step();
            }
        }
    }

    fn finish_step(&mut self) {
        if self.steps.len() == SHORT_STEPS {
            self.steps.pop_front();
        }
        self.steps.push_back(self.step_acc);
        self.step_acc = 0.0;
        self.step_pos = 0;
        let Some(m) = self.window(MOMENTARY_STEPS) else {
            return;
        };
        // Ein neuer 400-ms-Block (75 % Überlappung) ist vollständig.
        // Gegatet wird erst beim Abfragen: hier über alle Blöcke zu gehen, machte die Messung
        // quadratisch in der Länge (#122, #120).
        self.blocks.push(m);
        if let Some(v) = self.momentary() {
            self.max_momentary = Some(self.max_momentary.map_or(v, |o| o.max(v)));
        }
        if let Some(v) = self.short_term() {
            self.max_short = Some(self.max_short.map_or(v, |o| o.max(v)));
        }
    }

    /// Mittelquadrat der letzten `n` Schritte; `None`, wenn weniger vorliegen.
    fn window(&self, n: usize) -> Option<f64> {
        if self.steps.len() < n {
            return None;
        }
        let sum: f64 = self.steps.iter().rev().take(n).sum();
        Some(sum / (n * self.step_len) as f64)
    }

    /// Momentane Lautheit (400 ms); `None` bei Stille oder zu wenig Material.
    pub fn momentary(&self) -> Option<f64> {
        self.window(MOMENTARY_STEPS)
            .filter(|&z| z > 0.0)
            .map(lufs_of)
    }

    /// Kurzzeit-Lautheit (3 s, am Anfang mit dem, was da ist, mindestens 400 ms).
    pub fn short_term(&self) -> Option<f64> {
        self.window(self.steps.len().clamp(MOMENTARY_STEPS, SHORT_STEPS))
            .filter(|&z| z > 0.0)
            .map(lufs_of)
    }

    /// Integrierte Lautheit mit Gating (geht einmal über alle Blöcke, linear).
    pub fn integrated(&self) -> Option<f64> {
        gated_lufs(&self.blocks)
    }

    /// Größte momentane Lautheit seit dem Zurücksetzen.
    pub fn max_momentary(&self) -> Option<f64> {
        self.max_momentary
    }

    /// Größte Kurzzeit-Lautheit seit dem Zurücksetzen.
    pub fn max_short_term(&self) -> Option<f64> {
        self.max_short
    }
}

/// Halbe Länge des Überabtastfilters in Eingangs-Samples.
const TP_HALF: usize = 12;
/// Überabtastfaktor (BS.1770 Anhang 2: 4-fach bei 48 kHz).
const TP_OVERSAMPLE: usize = 4;
const TP_TAPS: usize = 2 * TP_HALF + 1;

/// True-Peak-Messer für einen Kanal: gefensterter Sinc, 4-fach überabgetastet. Das Ergebnis
/// läuft [`TP_HALF`] Samples hinter dem Eingang her; [`TruePeak::flush`] holt den Rest.
#[derive(Clone)]
pub struct TruePeak {
    /// Koeffizienten je Phase, in zeitlicher Reihenfolge (ältestes Sample zuerst).
    phases: [[f64; TP_TAPS]; TP_OVERSAMPLE],
    /// Ringpuffer, jedes Sample doppelt (bei `pos` und `pos + TP_TAPS`): das Fenster der
    /// letzten [`TP_TAPS`] Samples liegt so immer zusammenhängend, ohne Umkopieren.
    ring: [f64; 2 * TP_TAPS],
    pos: usize,
    /// Größte Summe der Beträge einer Phase: |Ausgang| ≤ `gain` · größtes |Sample| im Fenster.
    gain: f64,
    /// So viele Samples lang kann das Fenster noch eine neue Spitze bringen. Liegt kein
    /// Sample im Fenster über `peak / gain`, ist die Rechnung überflüssig (exakt, kein
    /// Näherungswert): bei Musik fällt so der Großteil der Filterarbeit weg (André 31.10.2026,
    /// Analyse auf dem alten Windows-Notebook drei Viertel der Zeit im True Peak).
    live: usize,
    peak: f64,
}

impl Default for TruePeak {
    fn default() -> Self {
        Self::new()
    }
}

impl TruePeak {
    /// Neuer Messer.
    pub fn new() -> Self {
        let len = TP_OVERSAMPLE * 2 * TP_HALF + 1;
        let centre = (TP_OVERSAMPLE * TP_HALF) as f64;
        let h: Vec<f64> = (0..len)
            .map(|n| {
                let x = (n as f64 - centre) / TP_OVERSAMPLE as f64;
                let sinc = if x == 0.0 {
                    1.0
                } else {
                    (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
                };
                // Blackman-Fenster über die ganze Länge.
                let t = n as f64 / (len - 1) as f64;
                let w = 0.42 - 0.5 * (std::f64::consts::TAU * t).cos()
                    + 0.08 * (2.0 * std::f64::consts::TAU * t).cos();
                sinc * w
            })
            .collect();
        let mut phases = [[0.0; TP_TAPS]; TP_OVERSAMPLE];
        for (p, taps) in phases.iter_mut().enumerate() {
            for (j, c) in taps.iter_mut().rev().enumerate() {
                *c = h.get(TP_OVERSAMPLE * j + p).copied().unwrap_or(0.0);
            }
        }
        let gain = phases
            .iter()
            .map(|t| t.iter().map(|c| c.abs()).sum::<f64>())
            .fold(0.0, f64::max);
        Self {
            phases,
            ring: [0.0; 2 * TP_TAPS],
            pos: 0,
            gain,
            live: 0,
            peak: 0.0,
        }
    }

    fn step(&mut self, x: f64) {
        self.ring[self.pos] = x;
        self.ring[self.pos + TP_TAPS] = x;
        self.pos = (self.pos + 1) % TP_TAPS;
        if x.abs() * self.gain > self.peak {
            self.live = TP_TAPS;
        }
        if self.live == 0 {
            return;
        }
        self.live -= 1;
        // Ältestes Sample zuerst: das Fenster beginnt beim nächsten Schreibplatz.
        let w = &self.ring[self.pos..self.pos + TP_TAPS];
        for taps in &self.phases {
            let y: f64 = taps.iter().zip(w).map(|(c, h)| c * h).sum();
            self.peak = self.peak.max(y.abs());
        }
    }

    /// Nimmt Samples.
    pub fn push(&mut self, samples: &[f32]) {
        for &s in samples {
            self.step(f64::from(s));
        }
    }

    /// Schiebt Stille nach, damit die letzten Samples ganz in die Messung eingehen.
    pub fn flush(&mut self) {
        for _ in 0..TP_HALF {
            self.step(0.0);
        }
    }

    /// Größte Spitze (linear) seit dem Zurücksetzen.
    pub fn peak(&self) -> f64 {
        self.peak
    }

    /// Größte Spitze in dBTP; `None` bei Stille.
    pub fn dbtp(&self) -> Option<f64> {
        super::amp_to_db(self.peak)
    }

    /// Setzt die Spitze zurück (Historie bleibt).
    pub fn reset_peak(&mut self) {
        self.peak = 0.0;
        // Die Historie zählt wieder voll.
        self.live = TP_TAPS;
    }
}

/// True Peak eines Stereo-Signals in dBTP (größter beider Kanäle); `None` bei Stille.
pub fn true_peak_dbtp(left: &[f32], right: &[f32]) -> Option<f64> {
    let (mut l, mut r) = (TruePeak::new(), TruePeak::new());
    l.push(left);
    r.push(right);
    l.flush();
    r.flush();
    super::amp_to_db(l.peak().max(r.peak()))
}

/// Integrierte Lautheit in LUFS über ein ganzes Stück; `None` bei Stille oder unter 400 ms.
pub fn integrated_lufs(left: &[f32], right: &[f32], rate: u32) -> Option<f64> {
    let mut m = LoudnessMeter::new(rate);
    m.push(left, right);
    m.integrated()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, amp: f64, secs: f64, rate: u32) -> Vec<f32> {
        (0..(f64::from(rate) * secs) as usize)
            .map(|i| {
                (amp * (i as f64 * freq * std::f64::consts::TAU / f64::from(rate)).sin()) as f32
            })
            .collect()
    }

    fn dbfs(db: f64) -> f64 {
        10f64.powf(db / 20.0)
    }

    /// EBU Tech 3341, Fall 1 und 2: 1-kHz-Sinus auf beiden Kanälen, −23 und −33 dBFS.
    #[test]
    fn ebu_sine_levels() {
        for (level, expect) in [(-23.0, -23.0), (-33.0, -33.0)] {
            let s = sine(1000.0, dbfs(level), 20.0, 48_000);
            let mut m = LoudnessMeter::new(48_000);
            m.push(&s, &s);
            let i = m.integrated().unwrap();
            assert!((i - expect).abs() < 0.1, "integriert {i}");
            assert!((m.momentary().unwrap() - expect).abs() < 0.1);
            assert!((m.short_term().unwrap() - expect).abs() < 0.1);
            assert!((m.max_momentary().unwrap() - expect).abs() < 0.1);
        }
    }

    /// Ein Kanal allein ist 3 LU leiser als beide.
    #[test]
    fn single_channel_is_3_lu_quieter() {
        let s = sine(1000.0, dbfs(-23.0), 10.0, 48_000);
        let z = vec![0.0; s.len()];
        let both = integrated_lufs(&s, &s, 48_000).unwrap();
        let one = integrated_lufs(&s, &z, 48_000).unwrap();
        assert!((both - one - 3.01).abs() < 0.05, "{both} {one}");
    }

    /// EBU Tech 3341, Fall 3 (verkürzt): Leises unter dem relativen Gate zählt nicht.
    #[test]
    fn relative_gate_ignores_quiet_part() {
        let mut s = sine(1000.0, dbfs(-23.0), 10.0, 48_000);
        s.extend(sine(1000.0, dbfs(-43.0), 10.0, 48_000));
        let i = integrated_lufs(&s, &s, 48_000).unwrap();
        assert!((i + 23.0).abs() < 0.1, "{i}");
        // Unter −70 LUFS (absolutes Gate) bleibt nichts übrig.
        let quiet = sine(1000.0, dbfs(-80.0), 10.0, 48_000);
        assert!(integrated_lufs(&quiet, &quiet, 48_000).is_none());
    }

    #[test]
    fn works_at_other_rates_and_in_pieces() {
        let s = sine(1000.0, dbfs(-23.0), 8.0, 44_100);
        let mut m = LoudnessMeter::new(44_100);
        for chunk in s.chunks(333) {
            m.push(chunk, chunk);
        }
        assert!((m.integrated().unwrap() + 23.0).abs() < 0.15);
        let mut m2 = LoudnessMeter::new(44_100);
        m2.push(&[0.0; 44_100], &[0.0; 44_100]);
        assert!(m2.momentary().is_none() && m2.integrated().is_none());
    }

    /// EBU Tech 3341 sinngemäß: Ein Sinus bei fs/4 mit 45° Phase hat Sample-Spitzen bei
    /// 0,707, die wahre Spitze liegt bei 1,0 (+3 dB).
    /// Das Überspringen ändert nichts: gleich wie das volle Filter über jedes Sample.
    #[test]
    fn true_peak_skip_is_exact() {
        let reference = |x: &[f32]| {
            let tp = TruePeak::new();
            let mut hist = vec![0.0f64; TP_TAPS];
            let mut peak = 0.0f64;
            for &s in x.iter().chain(std::iter::repeat_n(&0.0, TP_HALF)) {
                hist.remove(0);
                hist.push(f64::from(s));
                for taps in &tp.phases {
                    let y: f64 = taps.iter().zip(&hist).map(|(c, h)| c * h).sum();
                    peak = peak.max(y.abs());
                }
            }
            peak
        };
        // Pseudozufällige Musik: leise Strecken, kurze laute Spitzen, Intersample-Spitzen.
        let mut seed = 12345u32;
        let x: Vec<f32> = (0..50_000)
            .map(|i| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5;
                let loud = if i % 7000 < 40 { 1.6 } else { 0.3 };
                noise * loud + 0.4 * ((i as f32) * 2.9).sin()
            })
            .collect();
        let mut tp = TruePeak::new();
        tp.push(&x);
        tp.flush();
        let want = reference(&x);
        assert!((tp.peak() - want).abs() < 1e-12, "{} vs {want}", tp.peak());
        // Nach dem Zurücksetzen zählt die Historie mit.
        tp.reset_peak();
        tp.push(&[0.0; 3]);
        assert!(tp.peak() > 0.0, "Historie nach reset_peak vergessen");
    }

    #[test]
    fn true_peak_finds_intersample_peak() {
        let s: Vec<f32> = (0..4800)
            .map(|i| {
                (std::f64::consts::FRAC_PI_4 + i as f64 * std::f64::consts::FRAC_PI_2).sin() as f32
            })
            .collect();
        let sample_peak = s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((sample_peak - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.001);
        let tp = true_peak_dbtp(&s, &s).unwrap();
        assert!(tp.abs() < 0.25, "{tp}");
    }

    #[test]
    fn true_peak_of_slow_sine_is_its_amplitude() {
        let s = sine(997.0, 0.5, 1.0, 48_000);
        let tp = true_peak_dbtp(&s, &s).unwrap();
        assert!((tp + 6.02).abs() < 0.05, "{tp}");
        assert!(true_peak_dbtp(&[0.0; 100], &[0.0; 100]).is_none());
    }

    /// #122/#120: Die integrierte Lautheit darf nicht mit jedem 100-ms-Schritt über alle bisherigen
    /// Blöcke neu gegatet werden (quadratisch: ein 30-Minuten-Titel brauchte Minuten). Zwei
    /// Stunden Material in Stücken müssen in Sekunden durch sein.
    #[test]
    fn long_material_is_measured_in_linear_time() {
        let rate = 4000;
        let s = sine(440.0, dbfs(-20.0), 1.0, rate);
        let t0 = std::time::Instant::now();
        let mut m = LoudnessMeter::new(rate);
        for _ in 0..2 * 3600 {
            m.push(&s, &s);
        }
        let i = m.integrated().unwrap();
        let took = t0.elapsed();
        assert!((i + 20.0).abs() < 0.5, "{i}");
        assert!(took < std::time::Duration::from_secs(20), "{took:?}");
    }
}
