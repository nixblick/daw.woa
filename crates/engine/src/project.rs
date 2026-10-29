//! Graph aus einem `woa-model`-Projekt bauen (Control-Thread) und offline rendern.
//!
//! Je Spur: Quelle -> Spur-Gain (dB, Mute/Solo) -> Pan links / Pan rechts -> Summen L/R.
//! Alles sind [`Gain`]-Nodes, Änderungen an Gain, Pan, Mute und Solo gehen daher geglättet
//! per `set_param` ([`ProjectGraph::update`]), ohne neuen Graphen.
//!
//! Die Quellen liefert der Aufrufer: Clips tragen im Modell noch keinen Klang (M0-Platzhalter).

use std::collections::HashMap;

use woa_dsp::{Gain, Node, gain_params};
use woa_model::{Project, Track, TrackId};

use crate::{Engine, EngineError, GraphBuilder, NodeId};

/// Node-Ids einer Spur im Graphen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackNodes {
    pub gain: NodeId,
    pub pan_left: NodeId,
    pub pan_right: NodeId,
}

/// Ergebnis von [`build_project_graph`]: Builder plus Zuordnung Spur -> Nodes.
pub struct ProjectGraph {
    pub builder: GraphBuilder,
    pub tracks: HashMap<TrackId, TrackNodes>,
}

impl ProjectGraph {
    /// Schickt Gain/Pan/Mute/Solo aller bekannten Spuren als geglättete Parameter.
    /// Neue oder entfernte Spuren brauchen einen neuen Graphen.
    pub fn update(
        tracks: &HashMap<TrackId, TrackNodes>,
        engine: &mut Engine,
        project: &Project,
    ) -> Result<(), EngineError> {
        let any_solo = project.tracks.iter().any(|t| t.solo);
        for t in &project.tracks {
            if let Some(n) = tracks.get(&t.id) {
                let (g, l, r) = track_gains(t, any_solo);
                engine.set_param(n.gain, gain_params::GAIN, g)?;
                engine.set_param(n.pan_left, gain_params::GAIN, l)?;
                engine.set_param(n.pan_right, gain_params::GAIN, r)?;
            }
        }
        Ok(())
    }
}

/// Lineare Faktoren (Spur-Gain inkl. Mute/Solo, Pan links, Pan rechts).
///
/// Mute gewinnt immer; ist irgendeine Spur auf Solo, klingen nur Solo-Spuren.
/// Pan ist Constant-Power (cos/sin), Mitte = je -3 dB.
pub fn track_gains(track: &Track, any_solo: bool) -> (f32, f32, f32) {
    let audible = !track.mute && (!any_solo || track.solo);
    let gain = if audible {
        10f64.powf(track.gain_db / 20.0)
    } else {
        0.0
    };
    let pan = if track.pan.is_finite() {
        track.pan.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    let angle = (pan + 1.0) * std::f64::consts::FRAC_PI_4;
    (gain as f32, angle.cos() as f32, angle.sin() as f32)
}

/// Baut den Graphen eines Projekts. `source` liefert je Spur die Quelle;
/// `None` (z. B. Bus-Spuren) lässt die Spur aus.
pub fn build_project_graph(
    project: &Project,
    source: &mut dyn FnMut(&Track) -> Option<Box<dyn Node>>,
) -> ProjectGraph {
    let mut b = GraphBuilder::new();
    let sum_l = b.add_node(Gain::new(1.0));
    let sum_r = b.add_node(Gain::new(1.0));
    b.set_stereo_output(sum_l, sum_r);
    let any_solo = project.tracks.iter().any(|t| t.solo);
    let mut tracks = HashMap::new();
    for t in &project.tracks {
        let Some(src) = source(t) else { continue };
        let (g, l, r) = track_gains(t, any_solo);
        let src = b.add_boxed(src);
        let gain = b.add_node(Gain::new(g));
        let pan_left = b.add_node(Gain::new(l));
        let pan_right = b.add_node(Gain::new(r));
        b.connect(src, gain);
        b.connect(gain, pan_left);
        b.connect(gain, pan_right);
        b.connect(pan_left, sum_l);
        b.connect(pan_right, sum_r);
        tracks.insert(
            t.id,
            TrackNodes {
                gain,
                pan_left,
                pan_right,
            },
        );
    }
    ProjectGraph { builder: b, tracks }
}

/// Rendert ein Projekt offline in Stereo (links, rechts), ab Position 0, Transport läuft.
pub fn render_project(
    project: &Project,
    source: &mut dyn FnMut(&Track) -> Option<Box<dyn Node>>,
    sample_rate: f32,
    frames: usize,
    block: usize,
) -> Result<(Vec<f32>, Vec<f32>), EngineError> {
    let block = block.max(1);
    let (mut engine, mut processor) = Engine::new(16);
    let graph = build_project_graph(project, source);
    engine.set_graph(graph.builder, sample_rate, block)?;
    engine.set_tempo(project.tempo_bpm)?;
    engine.play()?;
    let mut left = vec![0.0; frames];
    let mut right = vec![0.0; frames];
    for (l, r) in left.chunks_mut(block).zip(right.chunks_mut(block)) {
        processor.process_stereo(l, r);
    }
    drop(processor);
    engine.collect();
    Ok((left, right))
}

#[cfg(test)]
mod tests {
    use super::*;
    use woa_model::TrackKind;

    fn track(gain_db: f64, pan: f64, mute: bool, solo: bool) -> Track {
        let mut t = Track::new(TrackId(1), "t", TrackKind::Audio);
        t.gain_db = gain_db;
        t.pan = pan;
        t.mute = mute;
        t.solo = solo;
        t
    }

    #[test]
    fn pan_is_constant_power() {
        let (g, l, r) = track_gains(&track(0.0, 0.0, false, false), false);
        assert!((g - 1.0).abs() < 1e-6);
        assert!((l * l + r * r - 1.0).abs() < 1e-6);
        let (_, l, r) = track_gains(&track(0.0, -1.0, false, false), false);
        assert!((l - 1.0).abs() < 1e-6 && r.abs() < 1e-6);
    }

    #[test]
    fn mute_and_solo() {
        assert_eq!(track_gains(&track(0.0, 0.0, true, true), true).0, 0.0);
        assert_eq!(track_gains(&track(0.0, 0.0, false, false), true).0, 0.0);
        assert!(track_gains(&track(0.0, 0.0, false, true), true).0 > 0.99);
        let (g, _, _) = track_gains(&track(-6.0, 0.0, false, false), false);
        assert!((g - 0.501_187).abs() < 1e-5);
    }
}
