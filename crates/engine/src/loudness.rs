//! Lautheit nach ITU-R BS.1770 / EBU R128 (integriert, mit Gating) und Angleichen beim Export.

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

/// K-Filter (Höhen-Shelf + Hochpass) für beliebige Abtastraten.
fn k_filter(rate: u32) -> [Biquad; 2] {
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
    [shelf, hp]
}

/// Integrierte Lautheit in LUFS; `None` bei Stille oder weniger als 400 ms.
pub fn integrated_lufs(left: &[f32], right: &[f32], rate: u32) -> Option<f64> {
    let block = (rate as usize * 4) / 10;
    let step = rate as usize / 10;
    let n = left.len().min(right.len());
    if block == 0 || n < block {
        return None;
    }
    // Quadrate nach K-Filter, Summe je 100-ms-Abschnitt.
    let mut fl = k_filter(rate);
    let mut fr = k_filter(rate);
    let mut steps = Vec::with_capacity(n / step + 1);
    let mut acc = 0.0f64;
    for i in 0..n {
        let l = fl[0].run(f64::from(left[i]));
        let l = fl[1].run(l);
        let r = fr[0].run(f64::from(right[i]));
        let r = fr[1].run(r);
        acc += l * l + r * r;
        if (i + 1) % step == 0 {
            steps.push(acc);
            acc = 0.0;
        }
    }
    // 400-ms-Blöcke mit 75 % Überlappung.
    let blocks: Vec<f64> = steps
        .windows(4)
        .map(|w| w.iter().sum::<f64>() / block as f64)
        .collect();
    let lufs = |z: f64| -0.691 + 10.0 * z.log10();
    let gated: Vec<f64> = blocks
        .iter()
        .copied()
        .filter(|&z| z > 0.0 && lufs(z) > -70.0)
        .collect();
    if gated.is_empty() {
        return None;
    }
    let rel = lufs(gated.iter().sum::<f64>() / gated.len() as f64) - 10.0;
    let kept: Vec<f64> = gated.into_iter().filter(|&z| lufs(z) > rel).collect();
    if kept.is_empty() {
        return None;
    }
    Some(lufs(kept.iter().sum::<f64>() / kept.len() as f64))
}

/// Ergebnis von [`normalize`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Normalized {
    /// Gemessen vorher.
    pub before_lufs: f64,
    /// Angewandte Verstärkung in dB.
    pub gain_db: f64,
    /// `true`, wenn die Spitzengrenze die Verstärkung begrenzt hat.
    pub peak_limited: bool,
}

/// Gleicht auf `target_lufs` an, ohne dass ein Sample über `ceiling_db` (dBFS) kommt.
/// `None` (nichts verändert) bei Stille.
pub fn normalize(
    left: &mut [f32],
    right: &mut [f32],
    rate: u32,
    target_lufs: f64,
    ceiling_db: f64,
) -> Option<Normalized> {
    let before = integrated_lufs(left, right, rate)?;
    let peak = left
        .iter()
        .chain(right.iter())
        .fold(0.0f32, |m, s| m.max(s.abs()));
    let mut gain_db = target_lufs - before;
    let mut peak_limited = false;
    if peak > 0.0 {
        let room = ceiling_db - 20.0 * f64::from(peak).log10();
        if gain_db > room {
            gain_db = room;
            peak_limited = true;
        }
    }
    let g = 10f64.powf(gain_db / 20.0) as f32;
    for s in left.iter_mut().chain(right.iter_mut()) {
        *s *= g;
    }
    Some(Normalized {
        before_lufs: before,
        gain_db,
        peak_limited,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(amp: f32, secs: usize) -> Vec<f32> {
        (0..48_000 * secs)
            .map(|i| (i as f32 * 997.0 * std::f32::consts::TAU / 48_000.0).sin() * amp)
            .collect()
    }

    #[test]
    fn sine_measures_as_expected() {
        // 997 Hz, Amplitude 0.1 auf beiden Kanälen: Mittelquadrat 2 × 0.005 → etwa −20 LUFS.
        let s = sine(0.1, 3);
        let l = integrated_lufs(&s, &s, 48_000).unwrap();
        assert!((l + 20.0).abs() < 0.3, "{l}");
        assert!(integrated_lufs(&[0.0; 48_000], &[0.0; 48_000], 48_000).is_none());
    }

    #[test]
    fn normalize_hits_target_and_respects_ceiling() {
        let (mut l, mut r) = (sine(0.05, 3), sine(0.05, 3));
        let n = normalize(&mut l, &mut r, 48_000, -16.0, -1.0).unwrap();
        assert!(!n.peak_limited);
        let after = integrated_lufs(&l, &r, 48_000).unwrap();
        assert!((after + 16.0).abs() < 0.1, "{after}");
        // Ziel 0 LUFS geht nur bis zur Spitzengrenze.
        let n = normalize(&mut l, &mut r, 48_000, 0.0, -1.0).unwrap();
        assert!(n.peak_limited);
        let peak = l.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((20.0 * peak.log10() + 1.0).abs() < 0.01);
    }
}
