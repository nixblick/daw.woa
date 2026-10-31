//! Stems (Paket 26): jede Spur und jeder Bus als eigene Datei, alle gleich lang, dazu der
//! Mixdown.
//!
//! Ein Stem ist das Signal einer Spur nach Fader und Pan (ein Bus: nach seinem Fader), also
//! vor dem Master, ohne den Bus, in den sie läuft. Gerechnet wird in einem einzigen
//! Offline-Durchgang über die Abgriffe des Graphen ([`crate::tap`]), damit alle Stems
//! sample-genau zum Mixdown passen.
//!
//! Was gilt: **Die Summe der obersten Stems ist der Mixdown.** Oberst sind Spuren und Busse,
//! deren Ausgang der Master ist ([`Stem::top_level`]). Eine Spur, die in einen Bus läuft, steckt
//! schon in dessen Stem; ihre eigene Datei ist ein Zusatz (etwa für eine andere DAW) und
//! gehört nicht noch einmal in die Summe. Stumme Spuren (und was Solo ausblendet) kommen nicht
//! als Datei, genau wie sie im Mixdown fehlen.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use woa_model::{Project, TrackId, TrackKind};

use crate::clip::clip_graph_inner;
use crate::export::{ExportSpec, export_with};
use crate::project::audibility;
use crate::{Engine, EngineError};

/// Ein Stem.
#[derive(Debug, Clone, PartialEq)]
pub struct Stem {
    pub track: TrackId,
    pub name: String,
    pub kind: TrackKind,
    /// Ausgang ist der Master (zählt in der Summe der Stems).
    pub top_level: bool,
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

/// Welche Stems gewünscht sind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StemSelect {
    /// Audio- und MIDI-Spuren.
    pub tracks: bool,
    /// Busse.
    pub buses: bool,
}

impl Default for StemSelect {
    fn default() -> Self {
        Self {
            tracks: true,
            buses: true,
        }
    }
}

/// Ergebnis von [`render_stems`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StemRender {
    pub mix_left: Vec<f32>,
    pub mix_right: Vec<f32>,
    pub stems: Vec<Stem>,
    /// Namen der Spuren, die nicht klingen und deshalb keine Datei bekommen.
    pub skipped: Vec<String>,
}

/// Rendert Mixdown und Stems über `seconds` in einem Durchgang.
#[tracing::instrument(level = "info", skip_all, name = "render_stems")]
pub fn render_stems(
    project: &Project,
    rate: u32,
    seconds: f64,
    select: StemSelect,
    load: &mut dyn FnMut(&str) -> Option<Arc<[f32]>>,
) -> Result<StemRender, EngineError> {
    render_stems_plugins(project, rate, seconds, select, load, &mut |_| {
        woa_dsp::plugin::TrackPlugins::default()
    })
}

/// Wie [`render_stems`], mit den Plugins der Spuren (Paket 31).
pub fn render_stems_plugins(
    project: &Project,
    rate: u32,
    seconds: f64,
    select: StemSelect,
    load: &mut dyn FnMut(&str) -> Option<Arc<[f32]>>,
    plugins: &mut dyn FnMut(&woa_model::Track) -> woa_dsp::plugin::TrackPlugins,
) -> Result<StemRender, EngineError> {
    const BLOCK: usize = 512;
    let (mut engine, mut processor) = Engine::new(16);
    let graph = clip_graph_inner(
        project,
        &engine.transport(),
        rate as f32,
        load,
        None,
        None,
        true,
        true,
        Some(plugins),
    );
    let mut taps = graph.taps;
    engine.set_graph(graph.builder, rate as f32, BLOCK)?;
    engine.set_tempo(project.tempo_bpm)?;

    let audible = audibility(project);
    let mut out = StemRender::default();
    let mut wanted: Vec<TrackId> = Vec::new();
    for (t, on) in project.tracks.iter().zip(&audible) {
        let wants = match t.kind {
            TrackKind::Bus => select.buses,
            _ => select.tracks,
        };
        if !wants {
            continue;
        }
        if !on {
            out.skipped.push(t.name.clone());
            continue;
        }
        if let Some(tap) = taps.get_mut(&Some(t.id)) {
            tap.set_enabled(true);
            wanted.push(t.id);
            out.stems.push(Stem {
                track: t.id,
                name: t.name.clone(),
                kind: t.kind,
                top_level: t.output.is_none(),
                left: Vec::new(),
                right: Vec::new(),
            });
        }
    }

    engine.play()?;
    let frames = (seconds.max(0.0) * f64::from(rate)) as usize;
    out.mix_left = vec![0.0; frames];
    out.mix_right = vec![0.0; frames];
    for stem in &mut out.stems {
        stem.left.reserve(frames);
        stem.right.reserve(frames);
    }
    let mut lost = 0u64;
    for (l, r) in out
        .mix_left
        .chunks_mut(BLOCK)
        .zip(out.mix_right.chunks_mut(BLOCK))
    {
        processor.process_stereo(l, r);
        // Nach jedem Block leeren: der Ring fasst nur gut eine Sekunde.
        for (id, stem) in wanted.iter().zip(&mut out.stems) {
            if let Some(tap) = taps.get_mut(&Some(*id)) {
                lost += tap.drain(&mut stem.left, &mut stem.right);
            }
        }
    }
    drop(processor);
    engine.collect();
    if lost > 0 {
        return Err(EngineError::Export(format!(
            "Stem-Abgriff hat {lost} Frames verloren"
        )));
    }
    for stem in &out.stems {
        if stem.left.len() != frames || stem.right.len() != frames {
            return Err(EngineError::Export(format!(
                "Stem {} hat {} statt {frames} Frames",
                stem.name,
                stem.left.len()
            )));
        }
    }
    Ok(out)
}

/// Dateiname ohne Endung aus einem Spurnamen: ohne Zeichen, die ein Dateisystem nicht mag,
/// ohne Punkte oder Leerzeichen am Ende, ohne reservierte Windows-Namen.
pub fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.replace("..", "_");
    let mut s = cleaned
        .trim_matches(|c: char| c == '.' || c.is_whitespace())
        .to_owned();
    if s.is_empty() {
        s = "Spur".into();
    }
    let stem_part = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem_part.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem_part.len() == 4
            && (stem_part.starts_with("COM") || stem_part.starts_with("LPT"))
            && stem_part.as_bytes()[3].is_ascii_digit());
    if reserved {
        s.insert(0, '_');
    }
    s.chars().take(80).collect()
}

/// Dateinamen der Stems: laufende Nummer vorn („01 Bass.wav“), damit gleiche Namen
/// nicht kollidieren und die Reihenfolge der Spuren erhalten bleibt.
pub fn stem_file_names(stems: &[Stem], ext: &str) -> Vec<String> {
    stems
        .iter()
        .enumerate()
        .map(|(i, s)| format!("{:02} {}.{ext}", i + 1, sanitize_name(&s.name)))
        .collect()
}

/// Name der Mixdown-Datei neben den Stems.
pub fn mixdown_file_name(ext: &str) -> String {
    format!("00 Mixdown.{ext}")
}

/// Rendert genau eine Spur (oder einen Bus) wie ihren Stem: nach Fader und Pan, vor dem
/// Master (Ticket 229). Eine stumme Spur ist ein Fehler, denn sie hätte keinen Ton.
pub fn render_track(
    project: &Project,
    rate: u32,
    seconds: f64,
    track: TrackId,
    load: &mut dyn FnMut(&str) -> Option<Arc<[f32]>>,
    plugins: &mut dyn FnMut(&woa_model::Track) -> woa_dsp::plugin::TrackPlugins,
) -> Result<Stem, EngineError> {
    let t = project
        .track(track)
        .ok_or_else(|| EngineError::Export("Spur nicht gefunden".into()))?;
    let bus = t.kind == TrackKind::Bus;
    let select = StemSelect {
        tracks: !bus,
        buses: bus,
    };
    let render = render_stems_plugins(project, rate, seconds, select, load, plugins)?;
    render
        .stems
        .into_iter()
        .find(|s| s.track == track)
        .ok_or_else(|| {
            EngineError::Export(format!("{} ist stumm, kein Ton zum Exportieren", t.name))
        })
}

fn scaled(x: &[f32], gain: f32) -> Vec<f32> {
    x.iter().map(|s| s * gain).collect()
}

/// Schreibt Stems (und auf Wunsch den Mixdown) in den Ordner `dir`, der schon da sein muss.
/// `gain_db` gilt für alle Dateien gleich (Lautheitsangleich des Mixdowns), damit die Summe
/// der Stems der Mixdown bleibt. Bei einem Fehler werden die in diesem Aufruf neu angelegten
/// Dateien wieder entfernt. Gibt die Pfade in Schreibreihenfolge zurück.
pub fn write_stems(
    dir: &Path,
    render: &StemRender,
    spec: &ExportSpec,
    rate: u32,
    gain_db: f64,
    with_mixdown: bool,
) -> Result<Vec<PathBuf>, EngineError> {
    let md = std::fs::metadata(dir)
        .map_err(|e| EngineError::Export(format!("{}: {e}", dir.display())))?;
    if !md.is_dir() {
        return Err(EngineError::Export(format!(
            "{}: kein Ordner",
            dir.display()
        )));
    }
    let ext = spec.format.extension();
    let gain = 10f64.powf(gain_db / 20.0) as f32;
    let mut jobs: Vec<(PathBuf, &[f32], &[f32])> = Vec::new();
    if with_mixdown {
        jobs.push((
            dir.join(mixdown_file_name(ext)),
            &render.mix_left,
            &render.mix_right,
        ));
    }
    for (stem, name) in render.stems.iter().zip(stem_file_names(&render.stems, ext)) {
        jobs.push((dir.join(name), &stem.left, &stem.right));
    }
    let mut written: Vec<(PathBuf, bool)> = Vec::new();
    for (path, l, r) in jobs {
        let existed = match std::fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_symlink() || !m.is_file() => {
                cleanup(&written);
                return Err(EngineError::Export(format!(
                    "{}: Ziel ist ein Symlink oder keine Datei",
                    path.display()
                )));
            }
            Ok(_) => true,
            Err(_) => false,
        };
        let result = if gain == 1.0 {
            export_with(&path, spec, l, r, rate)
        } else {
            export_with(&path, spec, &scaled(l, gain), &scaled(r, gain), rate)
        };
        if let Err(e) = result {
            cleanup(&written);
            return Err(e);
        }
        written.push((path, existed));
    }
    Ok(written.into_iter().map(|(p, _)| p).collect())
}

fn cleanup(written: &[(PathBuf, bool)]) {
    for (p, existed) in written {
        if !existed {
            let _ = std::fs::remove_file(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_names() {
        assert_eq!(sanitize_name("Bass/Drums: *neu*"), "Bass_Drums_ _neu_");
        assert_eq!(sanitize_name("..\\..\\etc"), "____etc");
        assert_eq!(sanitize_name("  "), "Spur");
        assert_eq!(sanitize_name("Name. "), "Name");
        assert_eq!(sanitize_name("CON"), "_CON");
        assert_eq!(sanitize_name("nul.txt"), "_nul.txt");
        assert_eq!(sanitize_name("COM1"), "_COM1");
        assert_eq!(sanitize_name("Comet"), "Comet");
        assert!(sanitize_name(&"x".repeat(500)).len() <= 80);
        assert!(!sanitize_name("a/../b").contains('/'));
    }

    #[test]
    fn numbered_names_keep_duplicates_apart() {
        let s = |n: &str| Stem {
            track: TrackId(1),
            name: n.into(),
            kind: TrackKind::Audio,
            top_level: true,
            left: vec![],
            right: vec![],
        };
        let names = stem_file_names(&[s("Gitarre"), s("Gitarre")], "wav");
        assert_eq!(names, ["01 Gitarre.wav", "02 Gitarre.wav"]);
        assert_eq!(mixdown_file_name("flac"), "00 Mixdown.flac");
    }
}
