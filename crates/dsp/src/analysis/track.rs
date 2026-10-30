//! Titelanalyse (Paket 25): Tempo, Tonart, Stille und daraus Cue-Vorschläge.
//!
//! Alles hier läuft im Control-Thread oder in einem Hintergrund-Thread der Bibliothek, nie im
//! Audio-Thread. Verfahren ohne neue Abhängigkeit: Tempo aus der Autokorrelation der
//! Einsatzkurve, Tonart aus einem Chroma-Vektor gegen die Krumhansl-Profile. Beide liefern
//! `None`, wenn das Signal zu wenig hergibt (Sinus ohne Einsätze, Rauschen, zu kurz). Die
//! Treffsicherheit an echter Musik ist eine Vermutung, bis sie an echten Titeln gemessen ist.

use realfft::RealFftPlanner;

use super::loudness::{integrated_lufs, true_peak_dbtp};

/// Pegel in dBFS, unter dem ein Block als Stille gilt.
pub const SILENCE_DB: f64 = -50.0;
/// Länge eines Stille-Blocks in Sekunden.
const SILENCE_BLOCK_S: f64 = 0.01;
/// Kürzer als das bekommt keine Cue-Vorschläge.
const MIN_CUE_SECONDS: f64 = 2.0;
/// Längste automatische Ausblendung in Sekunden.
const MAX_FADE_SECONDS: f64 = 15.0;
/// Mindestlänge der Stille vorn bzw. hinten, ab der ein Cue vorgeschlagen wird.
const MIN_LEAD_SILENCE: f64 = 0.25;
const MIN_TAIL_SILENCE: f64 = 0.5;
/// Tempo wird erst ab dieser Länge versucht.
const MIN_TEMPO_SECONDS: f64 = 6.0;
const BPM_MIN: f64 = 70.0;
const BPM_MAX: f64 = 180.0;

/// Tempo mit Zuverlässigkeit (0..1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tempo {
    pub bpm: f64,
    pub confidence: f64,
}

/// Tonart: Grundton 0..11 (C = 0), Dur oder Moll, Zuverlässigkeit als Korrelation (−1..1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Key {
    pub tonic: u8,
    pub minor: bool,
    pub confidence: f64,
}

const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

impl Key {
    /// Anzeigename: „C-Dur“, „a-Moll“ (Moll klein geschrieben).
    pub fn name(&self) -> String {
        let n = NOTE_NAMES[usize::from(self.tonic % 12)];
        if self.minor {
            format!("{}-Moll", n.to_lowercase())
        } else {
            format!("{n}-Dur")
        }
    }
}

/// Stille vorn und hinten.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Silence {
    /// Sekunden Stille am Anfang.
    pub lead: f64,
    /// Sekunden Stille am Ende.
    pub tail: f64,
    /// Gesamtlänge in Sekunden.
    pub total: f64,
    /// Gar nichts hörbar.
    pub all_silent: bool,
}

/// Vorgeschlagene Cue-Punkte in Sekunden ab Dateianfang.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AutoCues {
    pub cue_in: Option<f64>,
    pub fade_out: Option<f64>,
    pub cue_out: Option<f64>,
}

impl AutoCues {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Ergebnis der ganzen Analyse eines Titels.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackAnalysis {
    pub seconds: f64,
    pub lufs: Option<f64>,
    pub true_peak_db: Option<f64>,
    pub peak_db: Option<f64>,
    pub tempo: Option<Tempo>,
    pub key: Option<Key>,
    pub silence: Silence,
    pub cues: AutoCues,
}

/// Verstärkung in dB, die `lufs` auf `target` bringt, ohne dass die Spitze über `ceiling`
/// (dBFS) kommt. Ohne gültige Lautheit 0 dB. Grenzen setzt der Aufrufer.
pub fn gain_to_target_db(
    lufs: Option<f64>,
    peak_db: Option<f64>,
    target: f64,
    ceiling: f64,
) -> f64 {
    let Some(l) = lufs.filter(|l| l.is_finite()) else {
        return 0.0;
    };
    let mut g = target - l;
    if let Some(p) = peak_db.filter(|p| p.is_finite()) {
        g = g.min(ceiling - p);
    }
    g
}

fn mono_of(left: &[f32], right: &[f32]) -> Vec<f32> {
    let n = left.len().min(right.len());
    (0..n).map(|i| 0.5 * (left[i] + right[i])).collect()
}

/// Spitze beider Kanäle in dBFS; `None` bei Stille.
fn peak_db_of(left: &[f32], right: &[f32]) -> Option<f64> {
    let peak = left.iter().chain(right).fold(0.0f32, |m, v| m.max(v.abs()));
    super::amp_to_db(f64::from(peak))
}

/// Stille vorn und hinten (Block-Spitze unter `threshold_db`).
pub fn detect_silence(left: &[f32], right: &[f32], rate: u32, threshold_db: f64) -> Silence {
    let n = left.len().min(right.len());
    let rate_f = f64::from(rate.max(1));
    let total = n as f64 / rate_f;
    let block = ((SILENCE_BLOCK_S * rate_f) as usize).max(1);
    let thr = 10f64.powf(threshold_db / 20.0) as f32;
    let loud = |b: usize| {
        let (a, z) = (b * block, ((b + 1) * block).min(n));
        left[a..z]
            .iter()
            .chain(&right[a..z])
            .any(|v| v.abs() >= thr)
    };
    let blocks = n.div_ceil(block);
    let Some(first) = (0..blocks).find(|&b| loud(b)) else {
        return Silence {
            lead: total,
            tail: 0.0,
            total,
            all_silent: true,
        };
    };
    let last = (first..blocks).rev().find(|&b| loud(b)).unwrap_or(first);
    let audible_end = (((last + 1) * block).min(n)) as f64 / rate_f;
    Silence {
        lead: (first * block) as f64 / rate_f,
        tail: (total - audible_end).max(0.0),
        total,
        all_silent: false,
    }
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// Cue-Vorschläge: Cue-In hinter der Stille am Anfang, Cue-Out vor der Stille am Ende,
/// Fade-Out dort, wo der Ausklang beginnt (Pegel fällt um 6 dB unter das laute Maß).
/// Gibt nur Werte zurück, die sich begründen lassen; sonst bleibt das Feld leer.
pub fn suggest_cues(left: &[f32], right: &[f32], rate: u32) -> AutoCues {
    let sil = detect_silence(left, right, rate, SILENCE_DB);
    if sil.all_silent || sil.total < MIN_CUE_SECONDS {
        return AutoCues::default();
    }
    let cue_in = (sil.lead >= MIN_LEAD_SILENCE).then(|| round2((sil.lead - 0.02).max(0.0)));
    let cue_out = (sil.tail >= MIN_TAIL_SILENCE)
        .then(|| round2((sil.total - sil.tail + 0.05).min(sil.total)));
    let start = cue_in.unwrap_or(0.0);
    let end = cue_out.unwrap_or(sil.total);
    if end - start < 1.0 {
        return AutoCues::default();
    }
    let fade_out =
        outro_start(left, right, rate, start, end).filter(|f| *f > start + 0.5 && *f < end - 0.5);
    AutoCues {
        cue_in,
        fade_out,
        cue_out,
    }
}

/// Beginn des Ausklangs zwischen `start` und `end` (Sekunden), falls es einen gibt.
fn outro_start(left: &[f32], right: &[f32], rate: u32, start: f64, end: f64) -> Option<f64> {
    let rate_f = f64::from(rate.max(1));
    let win = ((0.1 * rate_f) as usize).max(1);
    let n = left.len().min(right.len());
    let a = (start * rate_f) as usize / win;
    let z = ((end * rate_f) as usize).min(n) / win;
    if z <= a + 2 {
        return None;
    }
    let power: Vec<f64> = (a..z)
        .map(|w| {
            let s = w * win;
            let sum: f64 = (s..s + win)
                .map(|i| {
                    let m = 0.5 * f64::from(left[i] + right[i]);
                    m * m
                })
                .sum();
            sum / win as f64
        })
        .collect();
    let mut sorted = power.clone();
    sorted.sort_by(|x, y| x.total_cmp(y));
    let reference = sorted[(sorted.len() * 9 / 10).min(sorted.len() - 1)];
    if reference <= 0.0 {
        return None;
    }
    // Letztes Fenster, das noch „laut“ ist (höchstens 6 dB unter dem Bezug).
    let last_loud = power.iter().rposition(|p| *p >= reference * 0.25)?;
    let t = ((a + last_loud + 1) * win) as f64 / rate_f;
    if end - t < 1.0 {
        return None;
    }
    Some(round2(t.max(end - MAX_FADE_SECONDS)))
}

/// Tempo in BPM aus der Autokorrelation der Einsatzkurve (70 bis 180 BPM).
pub fn detect_bpm(mono: &[f32], rate: u32) -> Option<Tempo> {
    let rate_f = f64::from(rate.max(1));
    if (mono.len() as f64) < MIN_TEMPO_SECONDS * rate_f || rate < 4000 {
        return None;
    }
    // Einsatzkurve mit etwa 200 Werten je Sekunde.
    let hop = ((rate_f / 200.0).round() as usize).max(1);
    let env_rate = rate_f / hop as f64;
    // Pegel je Schritt über ein Fenster von vier Schritten (etwa 20 ms): so schwankt ein
    // gleichmäßiger Klang nicht mit der Phase, ein Einsatz bleibt aber scharf.
    let frames = (mono.len() / hop).saturating_sub(3);
    let mut prev = 0.0f64;
    let mut onset = Vec::with_capacity(frames);
    for f in 0..frames {
        let s = &mono[f * hop..(f + 4) * hop];
        let rms = (s.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / s.len() as f64).sqrt();
        let level = (1.0 + 1000.0 * rms).ln();
        onset.push((level - prev).max(0.0));
        prev = level;
    }
    // Ohne deutliche Einsätze (Lautstärkesprung auf mehr als das Doppelte in 5 ms) gibt es keinen
    // Schlag: Schwebungen und gleichmäßige Klänge sollen kein Tempo vortäuschen.
    let strong = onset.iter().filter(|o| **o > 0.7).count();
    if strong < (frames / 400).max(4) {
        return None;
    }
    let mean = onset.iter().sum::<f64>() / onset.len() as f64;
    for o in &mut onset {
        *o -= mean;
    }
    let ac0: f64 = onset.iter().map(|o| o * o).sum::<f64>() / onset.len() as f64;
    if ac0 < 1e-9 {
        return None;
    }
    let lag_min = ((60.0 * env_rate / BPM_MAX).floor() as usize).max(2);
    let lag_max = (60.0 * env_rate / BPM_MIN).ceil() as usize;
    let n = onset.len();
    if n <= lag_max * 4 + 2 {
        return None;
    }
    let ac = |lag: usize| -> f64 {
        if lag >= n {
            return 0.0;
        }
        let s: f64 = (0..n - lag).map(|i| onset[i] * onset[i + lag]).sum();
        s / (n - lag) as f64 / ac0
    };
    let acs: Vec<f64> = (0..=lag_max + 1).map(ac).collect();
    let mut best = (0usize, f64::MIN);
    for (lag, own) in acs.iter().enumerate().take(lag_max + 1).skip(lag_min) {
        // Vielfache des Schlags stützen den Kandidaten (Oktavfehler vermeiden).
        let score = own + 0.5 * ac(2 * lag) + 0.25 * ac(4 * lag);
        if score > best.1 {
            best = (lag, score);
        }
    }
    let (lag, _) = best;
    let confidence = acs[lag].clamp(0.0, 1.0);
    if lag == 0 || confidence < 0.05 {
        return None;
    }
    // Parabel durch die Nachbarn für eine Lage zwischen den Werten.
    let (y0, y1, y2) = (acs[lag - 1], acs[lag], acs[lag + 1]);
    let denom = y0 - 2.0 * y1 + y2;
    let frac = if denom.abs() > 1e-12 {
        (0.5 * (y0 - y2) / denom).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    let bpm = 60.0 * env_rate / (lag as f64 + frac);
    Some(Tempo { bpm, confidence })
}

/// Krumhansl-Kessler-Profile (Dur, Moll) für C bzw. a.
const MAJOR: [f64; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MINOR: [f64; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];
/// Höchstens so viele Fenster werden ausgewertet (gleichmäßig verteilt).
const MAX_KEY_FRAMES: usize = 400;

fn pearson(a: &[f64; 12], b: &[f64; 12]) -> f64 {
    let (ma, mb) = (a.iter().sum::<f64>() / 12.0, b.iter().sum::<f64>() / 12.0);
    let (mut num, mut da, mut db) = (0.0, 0.0, 0.0);
    for i in 0..12 {
        let (x, y) = (a[i] - ma, b[i] - mb);
        num += x * y;
        da += x * x;
        db += y * y;
    }
    if da <= 0.0 || db <= 0.0 {
        0.0
    } else {
        num / (da * db).sqrt()
    }
}

/// Tonart aus dem Chroma-Vektor (130 Hz bis 2,5 kHz). `None` bei Stille oder wenn keine
/// Tonart klar genug ist (Korrelation unter 0,4).
pub fn detect_key(mono: &[f32], rate: u32) -> Option<Key> {
    let size: usize = if rate >= 32_000 { 8192 } else { 4096 };
    if rate < 4000 || mono.len() < size {
        return None;
    }
    let df = f64::from(rate) / size as f64;
    let k_lo = (130.0 / df).ceil() as usize;
    let k_hi = ((2500.0 / df) as usize).min(size / 2 - 2);
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(size);
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();
    let mut scratch = fft.make_scratch_vec();
    let window: Vec<f32> = (0..size)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / size as f32).cos())
        .collect();
    let total_frames = (mono.len() - size) / (size / 2) + 1;
    let step = total_frames.div_ceil(MAX_KEY_FRAMES).max(1);
    let mut chroma = [0.0f64; 12];
    let mut mags = vec![0.0f32; size / 2 + 1];
    for f in (0..total_frames).step_by(step) {
        let s = &mono[f * (size / 2)..f * (size / 2) + size];
        for ((o, x), w) in input.iter_mut().zip(s).zip(&window) {
            *o = x * w;
        }
        if fft
            .process_with_scratch(&mut input, &mut spectrum, &mut scratch)
            .is_err()
        {
            continue;
        }
        for (m, c) in mags.iter_mut().zip(&spectrum) {
            *m = c.norm();
        }
        for k in k_lo.max(1)..=k_hi {
            let (a, b, c) = (mags[k - 1], mags[k], mags[k + 1]);
            if b <= a || b < c || b < 1e-3 {
                continue;
            }
            let denom = a - 2.0 * b + c;
            let delta = if denom.abs() > 1e-12 {
                f64::from(0.5 * (a - c) / denom).clamp(-0.5, 0.5)
            } else {
                0.0
            };
            let hz = (k as f64 + delta) * df;
            let midi = 69.0 + 12.0 * (hz / 440.0).log2();
            let pc = (midi.round() as i64).rem_euclid(12) as usize;
            chroma[pc] += f64::from(b).sqrt();
        }
    }
    if chroma.iter().sum::<f64>() <= 0.0 {
        return None;
    }
    let mut best: Option<Key> = None;
    for tonic in 0..12usize {
        let mut rotated = [0.0f64; 12];
        for (i, r) in rotated.iter_mut().enumerate() {
            *r = chroma[(tonic + i) % 12];
        }
        for (minor, profile) in [(false, &MAJOR), (true, &MINOR)] {
            let r = pearson(&rotated, profile);
            if best.is_none_or(|b| r > b.confidence) {
                best = Some(Key {
                    tonic: tonic as u8,
                    minor,
                    confidence: r,
                });
            }
        }
    }
    best.filter(|k| k.confidence >= 0.4)
}

/// Misst einen ganzen Titel.
pub fn analyze(left: &[f32], right: &[f32], rate: u32) -> TrackAnalysis {
    let n = left.len().min(right.len());
    let (left, right) = (&left[..n], &right[..n]);
    let mono = mono_of(left, right);
    TrackAnalysis {
        seconds: n as f64 / f64::from(rate.max(1)),
        lufs: integrated_lufs(left, right, rate),
        true_peak_db: true_peak_dbtp(left, right),
        peak_db: peak_db_of(left, right),
        tempo: detect_bpm(&mono, rate),
        key: detect_key(&mono, rate),
        silence: detect_silence(left, right, rate, SILENCE_DB),
        cues: suggest_cues(left, right, rate),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    fn click_track(bpm: f64, secs: f64, rate: u32) -> Vec<f32> {
        let n = (secs * f64::from(rate)) as usize;
        let mut x = vec![0.0f32; n];
        let period = 60.0 / bpm * f64::from(rate);
        let mut t = 0.0;
        while (t as usize) < n {
            let s = t as usize;
            for i in 0..(0.02 * f64::from(rate)) as usize {
                if s + i < n {
                    let env = (-(i as f32) / (0.004 * rate as f32)).exp();
                    x[s + i] +=
                        0.8 * env * (i as f32 * 1500.0 * std::f32::consts::TAU / rate as f32).sin();
                }
            }
            t += period;
        }
        x
    }

    fn chord(notes_hz: &[f32], secs: f32, rate: u32) -> Vec<f32> {
        (0..(secs * rate as f32) as usize)
            .map(|i| {
                notes_hz
                    .iter()
                    .map(|f| (i as f32 * f * std::f32::consts::TAU / rate as f32).sin())
                    .sum::<f32>()
                    * 0.2
            })
            .collect()
    }

    #[test]
    fn click_track_120_bpm() {
        let x = click_track(120.0, 20.0, RATE);
        let t = detect_bpm(&x, RATE).expect("tempo");
        assert!((t.bpm - 120.0).abs() <= 1.0, "{}", t.bpm);
        assert!(t.confidence > 0.3, "{}", t.confidence);
    }

    #[test]
    fn other_tempi_and_rates() {
        for (bpm, rate) in [(90.0, RATE), (128.0, 48_000), (150.0, 22_050), (75.0, 8000)] {
            let x = click_track(bpm, 24.0, rate);
            let t = detect_bpm(&x, rate).expect("tempo");
            assert!((t.bpm - bpm).abs() <= 1.5, "{bpm} @ {rate}: {}", t.bpm);
        }
    }

    #[test]
    fn steady_tone_and_silence_have_no_tempo() {
        let tone = chord(&[440.0], 10.0, RATE);
        assert!(detect_bpm(&tone, RATE).is_none());
        assert!(detect_bpm(&vec![0.0; 10 * RATE as usize], RATE).is_none());
        assert!(
            detect_bpm(&click_track(120.0, 2.0, RATE), RATE).is_none(),
            "too short"
        );
    }

    #[test]
    fn chords_give_their_key() {
        // C-Dur: C4 E4 G4; a-Moll: A3 C4 E4; D-Dur: D4 F#4 A4
        let c = detect_key(&chord(&[261.63, 329.63, 392.0], 6.0, RATE), RATE).expect("C");
        assert_eq!((c.tonic, c.minor), (0, false), "{}", c.name());
        let a = detect_key(&chord(&[220.0, 261.63, 329.63], 6.0, RATE), RATE).expect("a");
        assert_eq!((a.tonic, a.minor), (9, true), "{}", a.name());
        let d = detect_key(&chord(&[293.66, 369.99, 440.0], 6.0, RATE), RATE).expect("D");
        assert_eq!((d.tonic, d.minor), (2, false), "{}", d.name());
        assert_eq!(c.name(), "C-Dur");
        assert_eq!(a.name(), "a-Moll");
    }

    #[test]
    fn key_of_silence_and_short_signal_is_none() {
        assert!(detect_key(&vec![0.0; 100_000], RATE).is_none());
        assert!(detect_key(&[0.1; 100], RATE).is_none());
    }

    #[test]
    fn silence_front_and_back() {
        let mut x = vec![0.0f32; RATE as usize]; // 1 s Stille
        x.extend(chord(&[440.0], 3.0, RATE));
        x.extend(vec![0.0f32; 2 * RATE as usize]); // 2 s Stille
        let s = detect_silence(&x, &x, RATE, SILENCE_DB);
        assert!((s.lead - 1.0).abs() < 0.02, "{}", s.lead);
        assert!((s.tail - 2.0).abs() < 0.02, "{}", s.tail);
        assert!((s.total - 6.0).abs() < 1e-6);
        assert!(!s.all_silent);
        let z = vec![0.0f32; 1000];
        assert!(detect_silence(&z, &z, RATE, SILENCE_DB).all_silent);
        let e: [f32; 0] = [];
        assert!(detect_silence(&e, &e, RATE, SILENCE_DB).all_silent);
    }

    #[test]
    fn auto_cues_skip_silence_and_find_the_outro() {
        // 1,5 s Stille, 6 s laut, 4 s Ausklang (linear leiser), 1 s Stille.
        let r = RATE as usize;
        let mut x = vec![0.0f32; r * 3 / 2];
        x.extend(chord(&[440.0], 6.0, RATE));
        let outro = chord(&[440.0], 4.0, RATE);
        let m = outro.len();
        x.extend(
            outro
                .iter()
                .enumerate()
                .map(|(i, v)| v * (1.0 - i as f32 / m as f32)),
        );
        x.extend(vec![0.0f32; r]);
        let c = suggest_cues(&x, &x, RATE);
        let (ci, fo, co) = (c.cue_in.unwrap(), c.fade_out.unwrap(), c.cue_out.unwrap());
        assert!((ci - 1.48).abs() < 0.05, "{ci}");
        assert!((co - 11.55).abs() < 0.2, "{co}");
        // Pegel fällt ab 7,5 s; 6 dB unter dem Bezug etwa bei der Hälfte des Ausklangs.
        assert!(fo > 8.0 && fo < 10.5, "{fo}");
        assert!(ci < fo && fo < co);
    }

    #[test]
    fn no_cues_without_reason() {
        let x = chord(&[440.0], 5.0, RATE);
        assert!(suggest_cues(&x, &x, RATE).is_empty(), "nichts zu kürzen");
        let short = chord(&[440.0], 1.0, RATE);
        assert!(suggest_cues(&short, &short, RATE).is_empty());
        let z = vec![0.0f32; 5 * RATE as usize];
        assert!(suggest_cues(&z, &z, RATE).is_empty());
    }

    #[test]
    fn analysis_collects_everything() {
        let x = chord(&[261.63, 329.63, 392.0], 8.0, RATE);
        let a = analyze(&x, &x, RATE);
        assert!((a.seconds - 8.0).abs() < 1e-6);
        assert!(a.lufs.is_some() && a.true_peak_db.is_some() && a.peak_db.is_some());
        assert!(a.key.is_some());
        assert!(a.tempo.is_none(), "{:?}", a.tempo);
    }

    #[test]
    fn gain_reaches_target_and_respects_the_ceiling() {
        assert!((gain_to_target_db(Some(-23.0), Some(-12.0), -16.0, -1.0) - 7.0).abs() < 1e-9);
        // Spitze erlaubt nur 2 dB.
        assert!((gain_to_target_db(Some(-23.0), Some(-3.0), -16.0, -1.0) - 2.0).abs() < 1e-9);
        assert_eq!(gain_to_target_db(None, Some(-3.0), -16.0, -1.0), 0.0);
        assert_eq!(gain_to_target_db(Some(f64::NAN), None, -16.0, -1.0), 0.0);
    }
}
