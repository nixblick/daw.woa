//! Lautheit nach ITU-R BS.1770 / EBU R128 (integriert, mit Gating) und Angleichen beim Export.

/// Integrierte Lautheit in LUFS; `None` bei Stille oder weniger als 400 ms. Die Messung liegt in
/// `woa_dsp::analysis::loudness` (eine K-Bewertung für Export, Fenster und Agent).
pub use woa_dsp::analysis::loudness::integrated_lufs;

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

/// Ergebnis von [`master`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mastered {
    /// Gemessen vorher.
    pub before_lufs: f64,
    /// Gesamte Verstärkung vor dem Limiter in dB.
    pub gain_db: f64,
    /// Stärkste Absenkung durch den Limiter in dB (0 = nie eingegriffen).
    pub reduction_db: f32,
}

/// Fertiger Klang für ein Jingle (J6): Verstärkung auf `target_lufs`, dann Spitzenbegrenzer
/// ([`woa_dsp::dynamics::Limiter`]) auf `ceiling_db`. Anders als [`normalize`] erreicht das
/// auch spitzenreiche Mischungen; der Limiter kostet etwas Lautheit, deshalb bis zu vier
/// Durchgänge. `None` bei Stille. Nur offline (allokiert nicht, läuft aber über den ganzen Mix).
pub fn master(
    left: &mut [f32],
    right: &mut [f32],
    rate: u32,
    target_lufs: f64,
    ceiling_db: f64,
) -> Option<Mastered> {
    let before = integrated_lufs(left, right, rate)?;
    let ceiling = 10f64.powf(ceiling_db / 20.0) as f32;
    let mut now = before;
    let mut gain_db = 0.0;
    let mut reduction = 0.0f32;
    for _ in 0..4 {
        let step = target_lufs - now;
        if step.abs() < 0.1 {
            break;
        }
        gain_db += step;
        let g = 10f64.powf(step / 20.0) as f32;
        let mut lim = woa_dsp::dynamics::Limiter::new(ceiling_db, 0.05, rate as f32);
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let (a, b) = (*l * g, *r * g);
            let k = lim.gain(a.abs().max(b.abs()));
            reduction = reduction.max(lim.reduction_db());
            *l = (a * k).clamp(-ceiling, ceiling);
            *r = (b * k).clamp(-ceiling, ceiling);
        }
        now = integrated_lufs(left, right, rate)?;
    }
    Some(Mastered {
        before_lufs: before,
        gain_db,
        reduction_db: reduction,
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

    #[test]
    fn master_reaches_target_where_normalize_stops_at_the_peak() {
        // Kurze laute Spitzen auf leisem Grund: normalize bleibt an der Spitzengrenze hängen.
        let mut l = sine(0.02, 4);
        for i in (0..l.len()).step_by(12_000) {
            l[i] = 0.9;
        }
        let mut r = l.clone();
        let (mut nl, mut nr) = (l.clone(), r.clone());
        assert!(
            normalize(&mut nl, &mut nr, 48_000, -14.0, -1.0)
                .unwrap()
                .peak_limited
        );
        let m = master(&mut l, &mut r, 48_000, -14.0, -1.0).unwrap();
        let after = integrated_lufs(&l, &r, 48_000).unwrap();
        assert!((after + 14.0).abs() <= 0.5, "{after}");
        let peak = l.iter().chain(&r).fold(0.0f32, |a, s| a.max(s.abs()));
        assert!(20.0 * f64::from(peak).log10() <= -1.0 + 1e-3, "{peak}");
        assert!(m.reduction_db > 0.0);
        assert!(master(&mut [0.0; 48_000], &mut [0.0; 48_000], 48_000, -14.0, -1.0).is_none());
    }
}
