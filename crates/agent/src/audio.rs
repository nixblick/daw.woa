//! Klang für den Agenten: Noten und Drums offline erzeugen, Audio importieren,
//! das Projekt rendern, messen und exportieren. Alles im Control-Bereich, nie im
//! Audio-Thread.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use woa_dsp::analysis::loudness::{LoudnessMeter, true_peak_dbtp};
use woa_dsp::analysis::spectrogram::Spectrogram;
use woa_dsp::analysis::spectrum::{FLOOR_DB, bin_hz, octave_bands, peak_bin};
use woa_dsp::analysis::stereo::StereoMeter;
use woa_dsp::generate::{Drum, Note, render_drums, render_notes};
use woa_dsp::synth::Waveform;
use woa_engine::clip::{load_wav_mono, project_end_seconds, render_clips};
use woa_engine::loudness::{integrated_lufs, normalize};
use woa_model::Project;

/// Abtastrate aller erzeugten Dateien und Renders.
pub const RATE: u32 = 48_000;
/// Ordner im Projekt für Aufnahmen und erzeugte Clips (wie die GUI).
pub const TAKES_DIR: &str = "aufnahmen";
/// Spitzengrenze beim Angleichen der Lautheit (dBFS), wie GUI und `woa-headless render`.
pub const CEILING_DB: f64 = -1.0;
/// Längster erzeugter Clip in Beats (Speicherschutz).
pub const MAX_GENERATED_BEATS: f64 = 4096.0;

/// Wellenform des Synths.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentWaveform {
    /// Sinus.
    Sine,
    /// Sägezahn.
    #[default]
    Saw,
    /// Rechteck.
    Square,
}

impl From<AgentWaveform> for Waveform {
    fn from(w: AgentWaveform) -> Self {
        match w {
            AgentWaveform::Sine => Waveform::Sine,
            AgentWaveform::Saw => Waveform::Saw,
            AgentWaveform::Square => Waveform::Square,
        }
    }
}

/// Eine Note für `generate_notes`, Zeiten in Beats ab Clip-Anfang.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct AgentNote {
    /// Beginn in Beats ab Clip-Anfang (>= 0).
    pub beat: f64,
    /// MIDI-Note 0..=127 (60 = C4, 69 = A4).
    pub pitch: u8,
    /// Haltedauer in Beats (> 0).
    pub length_beats: f64,
    /// Anschlagstärke 1..=127; Vorgabe 100.
    #[serde(default = "default_velocity")]
    pub velocity: u8,
}

fn default_velocity() -> u8 {
    100
}

fn samples(beats: f64, bpm: f64) -> u64 {
    (beats * 60.0 / bpm * f64::from(RATE)).round() as u64
}

/// Prüft eine Länge in Beats.
pub fn check_length(length_beats: f64) -> Result<(), String> {
    if length_beats.is_finite() && length_beats > 0.0 && length_beats <= MAX_GENERATED_BEATS {
        Ok(())
    } else {
        Err(format!(
            "length_beats muss zwischen 0 und {MAX_GENERATED_BEATS} liegen, war {length_beats}"
        ))
    }
}

/// Prüft Länge und Noten (Beginn im Clip, Länge > 0, Tonhöhe, Anschlag).
pub fn check_notes(notes: &[AgentNote], length_beats: f64) -> Result<(), String> {
    check_length(length_beats)?;
    for (i, n) in notes.iter().enumerate() {
        if !(n.beat.is_finite() && n.beat >= 0.0 && n.beat < length_beats) {
            return Err(format!("Note {i}: beat {} liegt nicht im Clip", n.beat));
        }
        if !(n.length_beats.is_finite() && n.length_beats > 0.0) {
            return Err(format!("Note {i}: length_beats muss > 0 sein"));
        }
        if n.pitch > 127 || n.velocity == 0 || n.velocity > 127 {
            return Err(format!("Note {i}: pitch 0..=127, velocity 1..=127"));
        }
    }
    if notes.is_empty() {
        return Err("notes ist leer".into());
    }
    Ok(())
}

/// Noten als Inhalt eines MIDI-Clips (Paket 12); Noten über das Clip-Ende werden gekürzt.
pub fn notes_midi(notes: &[AgentNote], length_beats: f64) -> Result<woa_model::MidiData, String> {
    check_notes(notes, length_beats)?;
    let mut m = woa_model::MidiData {
        notes: notes
            .iter()
            .map(|n| woa_model::MidiNote {
                beat: n.beat,
                length_beats: n.length_beats.min(length_beats - n.beat),
                pitch: n.pitch,
                velocity: n.velocity,
            })
            .collect(),
        cc: Vec::new(),
    };
    m.sort();
    Ok(m)
}

/// Rendert Noten zu einem Mono-Puffer über `length_beats`.
pub fn notes_buffer(
    wave: AgentWaveform,
    notes: &[AgentNote],
    length_beats: f64,
    bpm: f64,
) -> Result<Vec<f32>, String> {
    check_notes(notes, length_beats)?;
    let mut out = Vec::with_capacity(notes.len());
    for n in notes {
        out.push(Note {
            start: samples(n.beat, bpm),
            length: samples(n.length_beats.min(length_beats - n.beat), bpm),
            pitch: n.pitch,
            velocity: n.velocity,
        });
    }
    let total = samples(length_beats, bpm) as usize;
    Ok(render_notes(wave.into(), &out, RATE as f32, total))
}

/// Rendert Drum-Muster (je Zeichen ein 16tel: `x` laut, `o` leise, sonst Pause).
/// Liefert Puffer und Länge in Beats (längstes Muster / 4).
pub fn drums_buffer(
    kick: &str,
    snare: &str,
    hihat: &str,
    bpm: f64,
) -> Result<(Vec<f32>, f64), String> {
    let steps = kick
        .chars()
        .count()
        .max(snare.chars().count())
        .max(hihat.chars().count());
    if steps == 0 {
        return Err("kein Muster: kick, snare oder hihat angeben".into());
    }
    let length_beats = steps as f64 / 4.0;
    check_length(length_beats)?;
    let mut hits = Vec::new();
    for (pattern, drum) in [
        (kick, Drum::Kick),
        (snare, Drum::Snare),
        (hihat, Drum::HiHat),
    ] {
        for (i, c) in pattern.chars().enumerate() {
            let vel = match c {
                'x' | 'X' => 1.0,
                'o' | 'O' => 0.5,
                '.' | '-' | '_' | ' ' => continue,
                other => return Err(format!("unbekanntes Zeichen {other:?} im Muster")),
            };
            hits.push((samples(i as f64 / 4.0, bpm), drum, vel));
        }
    }
    if hits.is_empty() {
        return Err("Muster enthält keinen Schlag (x oder o)".into());
    }
    let total = samples(length_beats, bpm) as usize;
    Ok((render_drums(&hits, RATE as f32, total), length_beats))
}

/// Freier Dateiname `aufnahmen/<stem>.<ext>` (mit Zähler, nie überschreiben).
pub fn free_take_path(dir: &Path, stem: &str, ext: &str) -> (String, PathBuf) {
    let clean: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let clean = if clean.is_empty() {
        "clip".to_string()
    } else {
        clean
    };
    let mut n = 1u32;
    loop {
        let name = if n == 1 {
            format!("{clean}.{ext}")
        } else {
            format!("{clean}-{n}.{ext}")
        };
        let rel = format!("{TAKES_DIR}/{name}");
        let abs = dir.join(TAKES_DIR).join(&name);
        if !abs.exists() {
            return (rel, abs);
        }
        n += 1;
    }
}

/// Schreibt einen Mono-Puffer als WAV nach `abs` (Ordner wird angelegt).
pub fn write_take(abs: &Path, buf: &[f32]) -> Result<(), String> {
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    woa_engine::write_wav(abs, buf, RATE).map_err(|e| e.to_string())
}

/// Länge einer WAV-Datei in Beats (prüft dabei, dass sie lesbar ist).
pub fn wav_length_beats(path: &Path, bpm: f64) -> Result<f64, String> {
    let s = load_wav_mono(path, RATE).map_err(|e| format!("{}: {e}", path.display()))?;
    if s.is_empty() {
        return Err(format!("{}: keine Samples", path.display()));
    }
    Ok(s.len() as f64 / f64::from(RATE) * bpm / 60.0)
}

/// Ein Stereo-Render des Projekts.
pub struct Mix {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub seconds: f64,
}

/// Rendert `project` mit den Clip-Dateien aus `dir` bis zum Ende des letzten Clips.
pub fn render(project: &Project, dir: &Path) -> Result<Mix, String> {
    let seconds = project_end_seconds(project);
    if seconds <= 0.0 {
        return Err("keine Clips zum Rendern".into());
    }
    let mut errors = Vec::new();
    let mut load = |rel: &str| -> Option<Arc<[f32]>> {
        if !woa_model::is_safe_audio_path(rel) {
            errors.push(format!("{rel}: Pfad verlässt den Projektordner"));
            return None;
        }
        load_wav_mono(&dir.join(rel), RATE)
            .map_err(|e| errors.push(format!("{rel}: {e}")))
            .ok()
    };
    let (left, right) =
        render_clips(project, RATE, seconds, &mut load).map_err(|e| e.to_string())?;
    if let Some(e) = errors.first() {
        return Err(e.clone());
    }
    Ok(Mix {
        left,
        right,
        seconds,
    })
}

/// Spitze in dBFS (−inf bei Stille als `None`).
pub fn peak_db(left: &[f32], right: &[f32]) -> Option<f64> {
    let p = left.iter().chain(right).fold(0.0f32, |m, s| m.max(s.abs()));
    (p > 0.0).then(|| 20.0 * f64::from(p).log10())
}

/// Messwerte eines Renders als JSON.
pub fn measure(mix: &Mix) -> serde_json::Value {
    serde_json::json!({
        "seconds": round(mix.seconds, 3),
        "lufs": integrated_lufs(&mix.left, &mix.right, RATE).map(|v| round(v, 2)),
        "sample_peak_dbfs": peak_db(&mix.left, &mix.right).map(|v| round(v, 2)),
        "clipping": mix.left.iter().chain(&mix.right).any(|s| s.abs() >= 1.0),
    })
}

/// Gleicht auf `target` LUFS an (Spitzen bleiben unter [`CEILING_DB`]).
pub fn normalize_mix(mix: &mut Mix, target: f64) -> Result<serde_json::Value, String> {
    if !(target.is_finite() && (-70.0..=0.0).contains(&target)) {
        return Err(format!(
            "target_lufs muss zwischen -70 und 0 liegen, war {target}"
        ));
    }
    let n = normalize(&mut mix.left, &mut mix.right, RATE, target, CEILING_DB)
        .ok_or("Mix ist still, nichts anzugleichen")?;
    Ok(serde_json::json!({
        "before_lufs": round(n.before_lufs, 2),
        "gain_db": round(n.gain_db, 2),
        "peak_limited": n.peak_limited,
    }))
}

/// Rendert nur eine Spur (solo, entstummt), Mixer-Einstellungen bleiben.
pub fn render_track(
    project: &Project,
    dir: &Path,
    track: woa_model::TrackId,
) -> Result<Mix, String> {
    let mut alone = project.clone();
    for o in &mut alone.tracks {
        o.solo = o.id == track;
        if o.id == track {
            o.mute = false;
        }
    }
    render(&alone, dir)
}

/// Fenstergröße und Schritt des Spektrums für Agent und Bild.
const FFT_SIZE: usize = 4096;
const FFT_HOP: usize = 2048;

/// Erweiterte Messung (Paket 19): True Peak, Höchstwerte von momentan und kurzzeitig,
/// mittleres Spektrum in Oktavbändern und Stereo-Bild, dazu kurze Bewertungen.
pub fn analysis(mix: &Mix) -> serde_json::Value {
    let mut meter = LoudnessMeter::new(RATE);
    meter.push(&mix.left, &mix.right);
    let tp = true_peak_dbtp(&mix.left, &mix.right);
    let spec = Spectrogram::of_stereo(&mix.left, &mix.right, RATE, FFT_SIZE, FFT_HOP);
    let mean = spec.mean_db();
    let silent = mean.iter().all(|&d| d <= FLOOR_DB);
    let bands: Vec<serde_json::Value> = if silent {
        Vec::new()
    } else {
        octave_bands(&mean, spec.size, RATE)
            .into_iter()
            .map(|(hz, db)| serde_json::json!({ "hz": hz, "db": round(f64::from(db), 1) }))
            .collect()
    };
    let peak_hz = (!silent)
        .then(|| peak_bin(&mean))
        .flatten()
        .map(|k| round(f64::from(bin_hz(k, spec.size, RATE)), 1));
    let mut st = StereoMeter::new(f64::INFINITY);
    st.push(&mix.left, &mix.right);
    let corr = st.correlation();
    let mut notes: Vec<String> = Vec::new();
    if let Some(t) = tp
        && t > -1.0
    {
        notes.push(format!(
            "true peak {t:.1} dBTP is above -1 dBTP: risk of clipping after lossy encoding"
        ));
    }
    match corr {
        Some(c) if c < 0.0 => notes.push(format!(
            "correlation {c:.2} is negative: parts cancel in mono (phase problem)"
        )),
        Some(c) if c > 0.99 => {
            notes.push("correlation is about +1: the mix is practically mono".into());
        }
        _ => {}
    }
    serde_json::json!({
        "true_peak_dbtp": tp.map(|v| round(v, 2)),
        "momentary_max_lufs": meter.max_momentary().map(|v| round(v, 2)),
        "short_term_max_lufs": meter.max_short_term().map(|v| round(v, 2)),
        "spectrum": { "peak_hz": peak_hz, "octave_bands": bands },
        "stereo": {
            "correlation": corr.map(|v| round(v, 3)),
            "balance_db": st.balance_db().map(|v| round(v, 2)),
            "side_to_mid_db": st.width_db().map(|v| round(v.clamp(-60.0, 60.0), 1)),
        },
        "notes": notes,
    })
}

/// Spektrogramm des Mixes als RGB-Bild (`width * height * 3` Bytes), Frequenz logarithmisch.
pub fn spectrogram_rgb(mix: &Mix, width: usize, height: usize) -> Vec<u8> {
    Spectrogram::of_stereo(&mix.left, &mix.right, RATE, FFT_SIZE, FFT_HOP).render_rgb(width, height)
}

/// Schreibt ein RGB-Bild als PNG.
pub fn write_png(path: &Path, rgb: &[u8], width: u32, height: u32) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| e.to_string())?;
    w.write_image_data(rgb).map_err(|e| e.to_string())?;
    w.finish().map_err(|e| e.to_string())
}

/// Rundet auf `digits` Nachkommastellen.
pub fn round(v: f64, digits: i32) -> f64 {
    let f = 10f64.powi(digits);
    (v * f).round() / f
}
