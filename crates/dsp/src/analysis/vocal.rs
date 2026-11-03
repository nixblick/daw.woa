//! Gesangseinsatz schätzen (#299): wann im Titel zum ersten Mal gesungen wird.
//!
//! Eine einfache Heuristik ohne neue Abhängigkeit, deshalb in der Oberfläche immer als
//! „geschätzt“ gekennzeichnet. Gesang steht in fast jeder Stereo-Mischung in der Mitte und
//! liegt im Sprachband (300 Hz bis 3,4 kHz). Gemessen wird je 50 ms die Energie im Sprachband
//! der Mitte (L+R) abzüglich der Seite (L−R), bezogen auf die Gesamtenergie der Mitte. Steigt
//! dieser Wert nach dem Intro deutlich und für mindestens zwei Sekunden über seinen
//! Intro-Grundwert, gilt das als Einsatz.
//!
//! Grenzen: Ein mittig gemischtes Melodie-Instrument im Sprachband (Saxofon, Lead-Gitarre)
//! sieht aus wie Gesang; Titel, die sofort mit Gesang beginnen, haben keinen Intro-Grundwert
//! (dann kein Ergebnis); in Mono fehlt die Unterscheidung Mitte/Seite. Läuft offline in der
//! Analyse (Hintergrund-Thread), nie im Audio- oder UI-Thread.

use crate::dynamics::Biquad;

/// Gesucht wird nur in den ersten so vielen Sekunden.
pub const SEARCH_SECONDS: f64 = 90.0;
/// Messfenster.
const HOP_S: f64 = 0.05;
/// Glättung des Werts (Sekunden).
const SMOOTH_S: f64 = 1.0;
/// Intro-Grundwert: so viele Sekunden ab dem ersten Ton.
const BASE_S: f64 = 3.0;
/// Frühester Einsatz nach dem ersten Ton.
const MIN_INTRO_S: f64 = 2.0;
/// Der Wert muss so viele dB über dem Grundwert liegen …
const RISE_DB: f64 = 6.0;
/// … und danach so lange über Grundwert + `HOLD_DB` bleiben.
const HOLD_DB: f64 = 4.0;
const HOLD_S: f64 = 2.0;
/// Unter diesem Pegel (dBFS, Fenster-Effektivwert) gilt ein Fenster als still.
const QUIET_DB: f64 = -50.0;

/// Sekunde des ersten Gesangseinsatzes ab Dateianfang, `None`, wenn keiner erkennbar ist.
pub fn detect_vocal_start(left: &[f32], right: &[f32], rate: u32) -> Option<f64> {
    let rate_f = f64::from(rate.max(1));
    let n = left
        .len()
        .min(right.len())
        .min((SEARCH_SECONDS * rate_f) as usize);
    let hop = ((HOP_S * rate_f) as usize).max(1);
    if n < hop * 20 {
        return None;
    }
    // Sprachband: Hochpass 300 Hz, dann Tiefpass-Ersatz über Bandpass um 1 kHz.
    let mut hp_m = Biquad::highpass(300.0, 0.707, rate_f);
    let mut bp_m = Biquad::bandpass(1_000.0, 0.45, rate_f);
    let mut hp_s = Biquad::highpass(300.0, 0.707, rate_f);
    let mut bp_s = Biquad::bandpass(1_000.0, 0.45, rate_f);
    let mut score = Vec::with_capacity(n / hop + 1);
    let mut loud = Vec::with_capacity(n / hop + 1);
    let mut i = 0;
    while i + hop <= n {
        let (mut e_mid, mut e_vm, mut e_vs) = (0.0f64, 0.0f64, 0.0f64);
        for k in i..i + hop {
            let m = 0.5 * (left[k] + right[k]);
            let s = 0.5 * (left[k] - right[k]);
            let vm = bp_m.run(hp_m.run(m));
            let vs = bp_s.run(hp_s.run(s));
            e_mid += f64::from(m * m);
            e_vm += f64::from(vm * vm);
            e_vs += f64::from(vs * vs);
        }
        let rms_db = 10.0 * (e_mid / hop as f64 + 1e-20).log10();
        loud.push(rms_db > QUIET_DB);
        let centre = (e_vm - e_vs).max(e_mid * 1e-6);
        score.push(10.0 * (centre / (e_mid + 1e-20)).log10());
        i += hop;
    }
    let first = loud.iter().position(|&l| l)?;
    // Gleitender Mittelwert über `SMOOTH_S`.
    let w = ((SMOOTH_S / HOP_S) as usize).max(1);
    let smooth: Vec<f64> = (0..score.len())
        .map(|k| {
            let a = k.saturating_sub(w / 2);
            let b = (k + w / 2 + 1).min(score.len());
            score[a..b].iter().sum::<f64>() / (b - a) as f64
        })
        .collect();
    let base_end = first + (BASE_S / HOP_S) as usize;
    if base_end >= smooth.len() {
        return None;
    }
    let mut base: Vec<f64> = smooth[first..base_end].to_vec();
    base.sort_by(f64::total_cmp);
    let base = base[base.len() / 2];
    let hold = (HOLD_S / HOP_S) as usize;
    let from = first + (MIN_INTRO_S / HOP_S) as usize;
    (from..smooth.len().saturating_sub(hold)).find_map(|k| {
        let rises = smooth[k] > base + RISE_DB;
        let holds = smooth[k..k + hold].iter().all(|v| *v > base + HOLD_DB);
        // Der geglättete Wert steigt schon eine halbe Fensterbreite vor dem Einsatz.
        (rises && holds).then(|| {
            let start = k.saturating_sub(w / 4);
            (start as f64 * HOP_S * 10.0).round() / 10.0
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    /// Einfaches, wiederholbares Rauschen.
    struct Noise(u32);
    impl Noise {
        fn next(&mut self) -> f32 {
            self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (self.0 >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
        }
    }

    /// Breites Instrumental: unabhängiges Rauschen links und rechts plus ein Bass in der
    /// Mitte; ab `voice_at` eine Stimme (Grundton mit Obertönen, Silben) in der Mitte.
    fn song(secs: f64, voice_at: Option<f64>) -> (Vec<f32>, Vec<f32>) {
        let n = (secs * f64::from(RATE)) as usize;
        let (mut nl, mut nr) = (Noise(1), Noise(2));
        let mut l = Vec::with_capacity(n);
        let mut r = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f64 / f64::from(RATE);
            let bass = 0.3 * (std::f64::consts::TAU * 55.0 * t).sin() as f32;
            let mut v = 0.0f32;
            if voice_at.is_some_and(|a| t >= a) {
                let syll = (0.5 + 0.5 * (std::f64::consts::TAU * 3.0 * t).sin()).powi(2);
                let f0 = 220.0 + 6.0 * (std::f64::consts::TAU * 5.0 * t).sin();
                let mut h = 0.0;
                for k in 1..=12 {
                    h += (std::f64::consts::TAU * f0 * f64::from(k) * t).sin() / f64::from(k);
                }
                v = (0.25 * syll * h) as f32;
            }
            l.push(0.2 * nl.next() + bass + v);
            r.push(0.2 * nr.next() + bass + v);
        }
        (l, r)
    }

    #[test]
    fn finds_the_first_vocal_after_an_instrumental_intro() {
        let (l, r) = song(30.0, Some(12.0));
        let t = detect_vocal_start(&l, &r, RATE).expect("Einsatz erkannt");
        assert!((t - 12.0).abs() <= 1.0, "geschätzt {t} s statt 12 s");
    }

    #[test]
    fn instrumental_has_no_vocal_start() {
        let (l, r) = song(30.0, None);
        assert_eq!(detect_vocal_start(&l, &r, RATE), None);
    }

    #[test]
    fn silence_and_short_files_give_nothing() {
        assert_eq!(
            detect_vocal_start(&[0.0; 44_100], &[0.0; 44_100], RATE),
            None
        );
        assert_eq!(detect_vocal_start(&[0.1; 100], &[0.1; 100], RATE), None);
    }

    #[test]
    fn looks_only_into_the_first_ninety_seconds() {
        let (l, r) = song(100.0, Some(95.0));
        assert_eq!(detect_vocal_start(&l, &r, RATE), None);
    }
}
