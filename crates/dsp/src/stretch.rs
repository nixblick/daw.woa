//! Time-Stretch und Tonhöhenänderung (Paket 23).
//!
//! Rein in Rust, ohne Abhängigkeit: WSOLA (Waveform Similarity Overlap-Add). Das Signal wird in
//! sich überlappenden Hann-Fenstern abgetastet; die Fensterlage wird je Schritt so gewählt,
//! dass sie die Wellenform des Vorgängers phasenrichtig fortsetzt. Die Tonhöhe bleibt beim
//! Dehnen erhalten. Die Tonhöhe selbst wird über Dehnen plus Neuabtastung geändert.
//!
//! Das alles läuft beim Bau des Graphen im Control-Thread, nie im Audio-Thread: der
//! Audio-Thread spielt nur das fertig gerechnete Ergebnis ab.

/// Fensterlänge in Samples (Hann, 50 % Überlappung).
const WINDOW: usize = 2048;
/// Schritt der Ausgabe (halbe Fensterlänge).
const HOP: usize = WINDOW / 2;
/// Suchweite für die Fensterlage in Samples (nach beiden Seiten).
const TOLERANCE: usize = 512;
/// Abwärtsfaktor der groben Suche.
const DECIM: usize = 4;

/// Kleinster und größter Dehnungsfaktor, den die Funktionen ausführen (außerhalb wird begrenzt).
pub const MIN_RATIO: f64 = 0.1;
/// Siehe [`MIN_RATIO`].
pub const MAX_RATIO: f64 = 10.0;

/// Dehnt `input` auf das `ratio`-fache der Länge (1,1 = zehn Prozent länger), die Tonhöhe bleibt.
/// Die Ausgabe hat genau `round(len * ratio)` Samples. Ein Verhältnis von 1 gibt die Eingabe
/// unverändert zurück.
pub fn time_stretch(input: &[f32], ratio: f64) -> Vec<f32> {
    let ratio = if ratio.is_finite() {
        ratio.clamp(MIN_RATIO, MAX_RATIO)
    } else {
        1.0
    };
    let target = (input.len() as f64 * ratio).round() as usize;
    if input.is_empty() || (ratio - 1.0).abs() < 1e-12 {
        return input.to_vec();
    }
    wsola(input, ratio, target)
}

/// Ändert die Tonhöhe um `semitones` und dehnt zugleich auf das `ratio`-fache der Länge.
/// Ergebnislänge genau `round(len * ratio)`. Bei 0 Halbtönen ist das [`time_stretch`].
pub fn stretch_and_pitch(input: &[f32], ratio: f64, semitones: f64) -> Vec<f32> {
    let ratio = if ratio.is_finite() {
        ratio.clamp(MIN_RATIO, MAX_RATIO)
    } else {
        1.0
    };
    let semitones = if semitones.is_finite() {
        semitones.clamp(-48.0, 48.0)
    } else {
        0.0
    };
    if semitones.abs() < 1e-9 {
        return time_stretch(input, ratio);
    }
    let target = (input.len() as f64 * ratio).round() as usize;
    if input.is_empty() {
        return Vec::new();
    }
    // Tonhöhe um den Faktor p heben: auf das ratio*p-fache dehnen (Tonhöhe bleibt), dann mit
    // p-facher Geschwindigkeit lesen (Tonhöhe mal p, Länge durch p).
    let p = 2f64.powf(semitones / 12.0);
    let stretched = wsola_len(input, ratio * p);
    let mut out = resample_cubic(&stretched, p, target);
    out.resize(target, 0.0);
    out
}

fn wsola_len(input: &[f32], ratio: f64) -> Vec<f32> {
    let ratio = ratio.clamp(MIN_RATIO, MAX_RATIO * 4.0);
    let target = (input.len() as f64 * ratio).round() as usize;
    if (ratio - 1.0).abs() < 1e-12 {
        return input.to_vec();
    }
    wsola(input, ratio, target)
}

/// Liest `x` mit Schrittweite `step` (kubisch, Catmull-Rom) und liefert höchstens `max` Werte.
fn resample_cubic(x: &[f32], step: f64, max: usize) -> Vec<f32> {
    let n = ((x.len() as f64 / step).floor() as usize).min(max.saturating_add(1));
    let at = |i: isize| -> f32 {
        if i < 0 || i as usize >= x.len() {
            0.0
        } else {
            x[i as usize]
        }
    };
    (0..n)
        .map(|k| {
            let pos = k as f64 * step;
            let i = pos.floor() as isize;
            let t = (pos - i as f64) as f32;
            let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
            0.5 * ((2.0 * p1)
                + (-p0 + p2) * t
                + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t
                + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t * t * t)
        })
        .collect()
}

fn wsola(input: &[f32], ratio: f64, target: usize) -> Vec<f32> {
    // Gepolsterte Kopie: vorn Platz für Fenster vor dem Anfang, hinten Nullen hinter dem Ende.
    let pad = (HOP + TOLERANCE).div_ceil(DECIM) * DECIM + DECIM;
    let tail = WINDOW + TOLERANCE + HOP + 2 * DECIM;
    let mut buf = vec![0.0f32; pad + input.len() + tail];
    buf[pad..pad + input.len()].copy_from_slice(input);
    // Grobe Fassung (Mittel über DECIM Samples) für die Suche.
    let dec: Vec<f32> = buf
        .chunks(DECIM)
        .map(|c| c.iter().sum::<f32>() / DECIM as f32)
        .collect();
    let window: Vec<f32> = (0..WINDOW)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / WINDOW as f64).cos() as f32)
        .collect();

    let mut out = vec![0.0f32; target + WINDOW];
    // Das erste Fenster beginnt eine halbe Länge vor dem Anfang (Ausgabe und Eingabe), damit
    // die Summe der Fenster vom ersten Sample an 1 ist.
    let mut prev = pad - HOP; // Lage des letzten Fensters in `buf`
    let mut k = 0usize;
    loop {
        let out_start = k as isize * HOP as isize - HOP as isize;
        if out_start >= target as isize {
            break;
        }
        let pos = if k == 0 {
            prev
        } else {
            let nominal = pad as f64 + (k - 1) as f64 * HOP as f64 / ratio;
            let nominal = nominal.round() as usize;
            let lo = nominal
                .saturating_sub(TOLERANCE)
                .max(pad.saturating_sub(HOP));
            let hi = (nominal + TOLERANCE).min(buf.len() - WINDOW - 1);
            best_position(&buf, &dec, prev + HOP, nominal.clamp(lo, hi), lo, hi)
        };
        for (i, w) in window.iter().enumerate() {
            let o = out_start + i as isize;
            if o >= 0 && (o as usize) < out.len() {
                out[o as usize] += w * buf[pos + i];
            }
        }
        prev = pos;
        k += 1;
    }
    out.truncate(target);
    out
}

/// Wählt die Lage in `lo..=hi`, deren erste Fensterhälfte der natürlichen Fortsetzung
/// (`template`, Lage in `buf`) am ähnlichsten ist; `nominal` gewinnt bei Gleichstand.
fn best_position(
    buf: &[f32],
    dec: &[f32],
    template: usize,
    nominal: usize,
    lo: usize,
    hi: usize,
) -> usize {
    let tmpl = &buf[template..template + HOP];
    // Grob: im abwärts abgetasteten Signal.
    let dl = HOP / DECIM;
    let dt = &dec[template / DECIM..template / DECIM + dl];
    let score_dec = |j: usize| -> f32 { ncc(dt, &dec[j..j + dl]) };
    let (jlo, jhi) = (lo.div_ceil(DECIM), hi / DECIM);
    let mut best_j = (nominal / DECIM).clamp(jlo, jhi.max(jlo));
    let mut best = score_dec(best_j);
    for j in jlo..=jhi {
        let s = score_dec(j);
        if s > best + 1e-6 {
            best = s;
            best_j = j;
        }
    }
    // Fein: um den groben Treffer herum auf Sample-Genauigkeit.
    let centre = best_j * DECIM;
    let (flo, fhi) = (
        centre.saturating_sub(DECIM).max(lo),
        (centre + DECIM).min(hi),
    );
    let mut best_p = centre.clamp(lo, hi);
    let mut best_s = ncc(tmpl, &buf[best_p..best_p + HOP]);
    for p in flo..=fhi {
        let s = ncc(tmpl, &buf[p..p + HOP]);
        if s > best_s + 1e-6 {
            best_s = s;
            best_p = p;
        }
    }
    best_p
}

/// Normierte Kreuzkorrelation zweier gleich langer Stücke (0 bei Stille).
fn ncc(a: &[f32], b: &[f32]) -> f32 {
    let (mut dot, mut ea, mut eb) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        ea += x * x;
        eb += y * y;
    }
    let d = (ea * eb).sqrt();
    if d > 1e-12 { dot / d } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use realfft::RealFftPlanner;

    const RATE: f64 = 48_000.0;

    fn sine(freq: f64, seconds: f64) -> Vec<f32> {
        (0..(seconds * RATE) as usize)
            .map(|i| (0.5 * (2.0 * std::f64::consts::PI * freq * i as f64 / RATE).sin()) as f32)
            .collect()
    }

    /// Frequenz der Spektrumspitze (Hann-Fenster, 2^16 Punkte aus der Mitte), parabolisch verfeinert.
    fn peak_freq(x: &[f32]) -> f64 {
        const N: usize = 1 << 16;
        assert!(x.len() >= N, "signal too short: {}", x.len());
        let from = (x.len() - N) / 2;
        let mut input: Vec<f32> = x[from..from + N]
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / N as f64).cos();
                v * w as f32
            })
            .collect();
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(N);
        let mut spec = fft.make_output_vec();
        fft.process(&mut input, &mut spec).expect("fft");
        let mag: Vec<f64> = spec.iter().map(|c| f64::from(c.norm())).collect();
        let (k, _) = mag
            .iter()
            .enumerate()
            .skip(1)
            .max_by(|a, b| a.1.total_cmp(b.1))
            .expect("peak");
        let (l, m, r) = (mag[k - 1].ln(), mag[k].ln(), mag[k + 1].ln());
        let delta = 0.5 * (l - r) / (l - 2.0 * m + r);
        (k as f64 + delta) * RATE / N as f64
    }

    #[test]
    fn stretch_keeps_frequency_and_hits_the_length() {
        let x = sine(440.0, 3.0);
        for ratio in [1.1, 0.9, 1.5, 0.5, 2.0] {
            let y = time_stretch(&x, ratio);
            let want = (x.len() as f64 * ratio).round() as usize;
            assert_eq!(y.len(), want, "length at ratio {ratio}");
            let f = peak_freq(&y);
            assert!((f - 440.0).abs() < 440.0 * 0.01, "ratio {ratio}: {f} Hz");
        }
    }

    #[test]
    fn stretch_of_one_is_identical() {
        let x = sine(440.0, 0.5);
        assert!(time_stretch(&x, 1.0) == x);
    }

    #[test]
    fn level_is_kept() {
        let x = sine(440.0, 3.0);
        let y = time_stretch(&x, 1.1);
        let rms =
            |s: &[f32]| (s.iter().map(|v| f64::from(v * v)).sum::<f64>() / s.len() as f64).sqrt();
        let mid = &y[y.len() / 4..y.len() * 3 / 4];
        let want = rms(&x);
        assert!(
            (rms(mid) - want).abs() < want * 0.03,
            "{} vs {want}",
            rms(mid)
        );
    }

    #[test]
    fn pitch_shift_moves_the_fft_peak() {
        let x = sine(440.0, 3.0);
        for semis in [2.0, -2.0, 7.0, -12.0, 12.0] {
            let y = stretch_and_pitch(&x, 1.0, semis);
            assert_eq!(y.len(), x.len(), "length at {semis}");
            let want = 440.0 * 2f64.powf(semis / 12.0);
            let f = peak_freq(&y);
            assert!(
                (f - want).abs() < want * 0.01,
                "{semis} st: {f} Hz, want {want}"
            );
        }
    }

    #[test]
    fn pitch_and_stretch_together() {
        let x = sine(330.0, 3.0);
        let y = stretch_and_pitch(&x, 1.25, 2.0);
        assert_eq!(y.len(), (x.len() as f64 * 1.25).round() as usize);
        let want = 330.0 * 2f64.powf(2.0 / 12.0);
        assert!((peak_freq(&y) - want).abs() < want * 0.01);
    }

    #[test]
    fn edge_cases_do_not_panic() {
        assert!(time_stretch(&[], 1.5).is_empty());
        assert!(stretch_and_pitch(&[], 1.5, 3.0).is_empty());
        let short = vec![0.3f32; 10];
        assert_eq!(time_stretch(&short, 2.0).len(), 20);
        assert_eq!(stretch_and_pitch(&short, 1.0, 5.0).len(), 10);
        let silence = vec![0.0f32; 20_000];
        let y = time_stretch(&silence, 1.3);
        assert_eq!(y.len(), 26_000);
        assert!(y.iter().all(|v| *v == 0.0));
        assert_eq!(time_stretch(&silence, f64::NAN).len(), silence.len());
    }
}
