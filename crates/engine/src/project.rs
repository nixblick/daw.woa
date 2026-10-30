//! Graph aus einem `woa-model`-Projekt bauen (Control-Thread) und offline rendern.
//!
//! Je Spur: Quelle -> Inserts (Mono-Effekte) -> Spur-Gain (dB, Mute/Solo) -> Pan links /
//! Pan rechts -> Ziel. Das Ziel ist der Master (Summen L/R) oder ein Bus (Paket 16).
//! Sends zweigen vor (nach den Inserts) oder nach dem Fader ab und gehen mit eigenem Pegel
//! auf einen Bus. Ein Bus ist ein Stereo-Zug: Eingang (Summe) -> Stereo-Inserts -> Fader und
//! Balance -> Ziel. Alles Veränderliche geht geglättet per `set_param`
//! ([`ProjectGraph::update`]), ohne neuen Graphen; Struktur (Inserts, Routing) braucht einen
//! neuen.
//!
//! Die Quellen liefert der Aufrufer: Clips tragen im Modell noch keinen Klang (M0-Platzhalter).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use woa_dsp::fx::{BYPASS, FxContext, FxKind, FxNode, FxStereoNode};
use woa_dsp::{DEFAULT_RAMP_SECONDS, Gain, Node, Smoother, StereoBuffers, StereoNode, gain_params};
use woa_model::{EffectKind, InsertId, Project, Track, TrackId, TrackKind};

use crate::clip::store_peak;
use crate::tap::{TapHandle, tap};
use crate::{Engine, EngineError, GraphBuilder, NodeId};

/// Parameter-IDs des Bus-Zugs ([`BusStrip`]).
pub mod bus_params {
    /// Fader (linear, inkl. Mute/Solo).
    pub const GAIN: u32 = 0;
    /// Linker Balance-Faktor.
    pub const BALANCE_LEFT: u32 = 1;
    /// Rechter Balance-Faktor.
    pub const BALANCE_RIGHT: u32 = 2;
}

/// Stereo-Durchgang: Eingang eines Busses (summiert dessen Zuläufe).
struct BusInput;

impl StereoNode for BusInput {
    fn prepare(&mut self, _sample_rate: f32) {}

    fn set_param(&mut self, _param: u32, _value: f32) {}

    fn process(&mut self, b: StereoBuffers<'_>) {
        b.out_left.copy_from_slice(b.in_left);
        b.out_right.copy_from_slice(b.in_right);
    }
}

/// Fader und Balance eines Busses, mit Pegelmesser am Ausgang.
struct BusStrip {
    gain: Smoother,
    left: Smoother,
    right: Smoother,
    meter: Option<Arc<AtomicU32>>,
}

impl BusStrip {
    fn new(gain: f32, left: f32, right: f32, meter: Option<Arc<AtomicU32>>) -> Self {
        Self {
            gain: Smoother::new(gain, DEFAULT_RAMP_SECONDS),
            left: Smoother::new(left, DEFAULT_RAMP_SECONDS),
            right: Smoother::new(right, DEFAULT_RAMP_SECONDS),
            meter,
        }
    }
}

impl StereoNode for BusStrip {
    fn prepare(&mut self, sample_rate: f32) {
        self.gain.prepare(sample_rate);
        self.left.prepare(sample_rate);
        self.right.prepare(sample_rate);
    }

    fn set_param(&mut self, param: u32, value: f32) {
        match param {
            bus_params::GAIN => self.gain.set_target(value),
            bus_params::BALANCE_LEFT => self.left.set_target(value),
            bus_params::BALANCE_RIGHT => self.right.set_target(value),
            _ => {}
        }
    }

    fn process(&mut self, b: StereoBuffers<'_>) {
        for i in 0..b.out_left.len() {
            let g = self.gain.next_value();
            b.out_left[i] = b.in_left[i] * g * self.left.next_value();
            b.out_right[i] = b.in_right[i] * g * self.right.next_value();
        }
        if let Some(m) = &self.meter {
            store_peak(m, b.out_left);
            store_peak(m, b.out_right);
        }
    }
}

/// Die beiden Gain-Nodes eines Sends (links, rechts) und was er speist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendNodes {
    pub target: TrackId,
    pub left: NodeId,
    pub right: NodeId,
}

/// Node-Ids einer Spur im Graphen.
///
/// Bei einer Audio- oder MIDI-Spur sind `gain`, `pan_left` und `pan_right` Mono-Gains; bei
/// einem Bus ist `gain` der [`BusStrip`] und `pan` leer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackNodes {
    pub gain: NodeId,
    pub pan: Option<(NodeId, NodeId)>,
    /// Effekte der Kette, in Reihenfolge.
    pub inserts: Vec<(InsertId, NodeId)>,
    pub sends: Vec<SendNodes>,
}

impl TrackNodes {
    /// Linker Pan-Gain (Spur) oder `None` (Bus).
    pub fn pan_left(&self) -> Option<NodeId> {
        self.pan.map(|p| p.0)
    }

    /// Rechter Pan-Gain (Spur) oder `None` (Bus).
    pub fn pan_right(&self) -> Option<NodeId> {
        self.pan.map(|p| p.1)
    }
}

/// Ergebnis von [`build_project_graph`]: Builder plus Zuordnung Spur -> Nodes.
pub struct ProjectGraph {
    pub builder: GraphBuilder,
    /// Audio-, MIDI- und Bus-Spuren, die einen Zug im Graphen haben.
    pub tracks: HashMap<TrackId, TrackNodes>,
    /// Pegelmesser der Busse (Spitze am Bus-Ausgang, abholen mit [`crate::clip::take_peak`]).
    pub bus_meters: HashMap<TrackId, Arc<AtomicU32>>,
    /// Abgriffe fürs Analyse-Fenster (Paket 19): `None` ist der Master, `Some` eine Spur nach
    /// Fader und Pan (Bus: nach dem Fader). Leer, wenn der Graph ohne Abgriffe gebaut wurde.
    pub taps: HashMap<Option<TrackId>, TapHandle>,
}

impl ProjectGraph {
    /// Schickt Gain/Pan/Mute/Solo, Sends und Effekt-Parameter aller bekannten Spuren als
    /// geglättete Parameter. Neue oder entfernte Spuren, Inserts, Sends und Ausgänge
    /// brauchen einen neuen Graphen.
    pub fn update(
        tracks: &HashMap<TrackId, TrackNodes>,
        engine: &mut Engine,
        project: &Project,
    ) -> Result<(), EngineError> {
        let audible = audibility(project);
        for (t, &on) in project.tracks.iter().zip(&audible) {
            let Some(n) = tracks.get(&t.id) else { continue };
            let (g, l, r) = track_gains_with(t, on);
            let (bal_l, bal_r) = balance(t.pan);
            match n.pan {
                Some((pl, pr)) => {
                    engine.set_param(n.gain, gain_params::GAIN, g)?;
                    engine.set_param(pl, gain_params::GAIN, l)?;
                    engine.set_param(pr, gain_params::GAIN, r)?;
                }
                None => {
                    engine.set_param(n.gain, bus_params::GAIN, g)?;
                    engine.set_param(n.gain, bus_params::BALANCE_LEFT, bal_l)?;
                    engine.set_param(n.gain, bus_params::BALANCE_RIGHT, bal_r)?;
                }
            }
            for ins in &t.inserts {
                let Some(&(_, node)) = n.inserts.iter().find(|(id, _)| *id == ins.id) else {
                    continue;
                };
                for (i, v) in ins.params.iter().enumerate() {
                    engine.set_param(node, i as u32, *v as f32)?;
                }
                engine.set_param(node, BYPASS, if ins.bypass { 1.0 } else { 0.0 })?;
            }
            for s in &t.sends {
                let Some(sn) = n.sends.iter().find(|x| x.target == s.target) else {
                    continue;
                };
                let (a, b) = send_gains(t, s.pre_fader, s.linear(), on, n.pan.is_some());
                engine.set_param(sn.left, gain_params::GAIN, a)?;
                engine.set_param(sn.right, gain_params::GAIN, b)?;
            }
        }
        Ok(())
    }
}

/// Linke und rechte Balance eines Busses: Mitte = beide 1, nach einer Seite wird die andere
/// leiser (linear).
fn balance(pan: f64) -> (f32, f32) {
    let p = if pan.is_finite() {
        pan.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    ((1.0 - p.max(0.0)) as f32, (1.0 + p.min(0.0)) as f32)
}

/// Faktoren der beiden Gains eines Sends. Vor dem Fader hat eine Spur noch kein Panorama,
/// also bekommt der Send das der Spur; nach dem Fader steckt es schon im Signal.
fn send_gains(track: &Track, pre: bool, level: f64, audible: bool, is_track: bool) -> (f32, f32) {
    let f = if audible { level as f32 } else { 0.0 };
    if pre && is_track {
        let (_, l, r) = track_gains_with(track, true);
        (f * l, f * r)
    } else {
        (f, f)
    }
}

/// Ob jede Spur klingt, in Reihenfolge der Projekt-Spuren.
///
/// Mute gewinnt immer. Ist irgendeine Spur auf Solo, klingen die Solo-Spuren und jeder Bus,
/// den eine Solo-Spur speist (Ausgang oder Send, auch über weitere Busse), außer er ist
/// stumm.
pub fn audibility(project: &Project) -> Vec<bool> {
    let solos: Vec<TrackId> = project
        .tracks
        .iter()
        .filter(|t| t.solo)
        .map(|t| t.id)
        .collect();
    project
        .tracks
        .iter()
        .map(|t| {
            if t.mute {
                return false;
            }
            if solos.is_empty() || t.solo {
                return true;
            }
            t.kind == TrackKind::Bus && solos.iter().any(|&s| project.reaches(s, t.id))
        })
        .collect()
}

/// Lineare Faktoren (Spur-Gain inkl. Mute/Solo, Pan links, Pan rechts).
///
/// Mute gewinnt immer; ist irgendeine Spur auf Solo, klingen nur Solo-Spuren (Busse siehe
/// [`audibility`]). Pan ist Constant-Power (cos/sin), Mitte = je -3 dB.
pub fn track_gains(track: &Track, any_solo: bool) -> (f32, f32, f32) {
    track_gains_with(track, !track.mute && (!any_solo || track.solo))
}

/// Wie [`track_gains`], mit fertig bestimmter Hörbarkeit.
pub fn track_gains_with(track: &Track, audible: bool) -> (f32, f32, f32) {
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

fn fx_kind(kind: EffectKind) -> FxKind {
    match kind {
        EffectKind::Eq => FxKind::Eq,
        EffectKind::Compressor => FxKind::Compressor,
        EffectKind::Limiter => FxKind::Limiter,
        EffectKind::Gate => FxKind::Gate,
        EffectKind::DeEsser => FxKind::DeEsser,
        EffectKind::Reverb => FxKind::Reverb,
        EffectKind::Delay => FxKind::Delay,
        EffectKind::Chorus => FxKind::Chorus,
        EffectKind::Saturation => FxKind::Saturation,
        EffectKind::Denoise => FxKind::Denoise,
    }
}

/// Ein- und Ausgänge eines Busses im Graphen.
struct BusPorts {
    /// Hier laufen die Zuläufe ein (Kanal 0 links, 1 rechts).
    head: NodeId,
    /// Letzter Knoten vor dem Fader (für Pre-Fader-Sends).
    pre: NodeId,
    /// Ausgang des Busses.
    strip: NodeId,
}

/// Baut den Graphen eines Projekts. `source` liefert je Spur die Quelle;
/// `None` lässt die Spur aus. Bus-Spuren brauchen keine Quelle.
pub fn build_project_graph(
    project: &Project,
    source: &mut dyn FnMut(&Track) -> Option<Box<dyn Node>>,
) -> ProjectGraph {
    build_project_graph_mode(project, source, false)
}

/// Wie [`build_project_graph`]. Mit `offline` (Export, Tests) rechnet die Rauschunterdrückung im
/// Aufrufer-Thread statt in ihrem Arbeits-Thread; das Ergebnis ist dasselbe.
pub fn build_project_graph_mode(
    project: &Project,
    source: &mut dyn FnMut(&Track) -> Option<Box<dyn Node>>,
    offline: bool,
) -> ProjectGraph {
    build_project_graph_taps(project, source, offline, false)
}

/// Wie [`build_project_graph_mode`]. Mit `with_taps` hängt der Graph an Master und jede Spur
/// einen Abgriff ([`ProjectGraph::taps`]); sie ändern das Signal nicht und sind ausgeschaltet,
/// bis die Oberfläche sie einschaltet.
pub fn build_project_graph_taps(
    project: &Project,
    source: &mut dyn FnMut(&Track) -> Option<Box<dyn Node>>,
    offline: bool,
    with_taps: bool,
) -> ProjectGraph {
    let ctx = FxContext {
        tempo_bpm: project.tempo_bpm as f32,
        offline,
    };
    let mut b = GraphBuilder::new();
    let sum_l = b.add_node(Gain::new(1.0));
    let sum_r = b.add_node(Gain::new(1.0));
    b.set_stereo_output(sum_l, sum_r);
    let audible = audibility(project);
    let audible_of = |id: TrackId| {
        project
            .tracks
            .iter()
            .position(|t| t.id == id)
            .is_some_and(|i| audible[i])
    };
    let mut tracks = HashMap::new();
    let mut bus_meters = HashMap::new();
    let mut taps = HashMap::new();
    if with_taps {
        let (sink, handle) = tap();
        let n = b.add_stereo(Box::new(sink));
        b.connect_ch(sum_l, 0, n, 0);
        b.connect_ch(sum_r, 0, n, 1);
        taps.insert(None, handle);
    }

    // Busse zuerst: Spuren und andere Busse brauchen ihre Eingänge.
    let mut buses: HashMap<TrackId, BusPorts> = HashMap::new();
    for t in project.tracks.iter().filter(|t| t.kind == TrackKind::Bus) {
        let on = audible_of(t.id);
        let (g, ..) = track_gains_with(t, on);
        let (bal_l, bal_r) = balance(t.pan);
        let head = b.add_stereo(Box::new(BusInput));
        let mut prev = head;
        let mut inserts = Vec::new();
        for ins in &t.inserts {
            let n = b.add_stereo(Box::new(FxStereoNode::with_context(
                fx_kind(ins.kind),
                &ins.params,
                ins.bypass,
                ctx,
            )));
            b.connect_ch(prev, 0, n, 0);
            b.connect_ch(prev, 1, n, 1);
            inserts.push((ins.id, n));
            prev = n;
        }
        let meter = Arc::new(AtomicU32::new(0));
        bus_meters.insert(t.id, Arc::clone(&meter));
        let strip = b.add_stereo(Box::new(BusStrip::new(g, bal_l, bal_r, Some(meter))));
        b.connect_ch(prev, 0, strip, 0);
        b.connect_ch(prev, 1, strip, 1);
        if with_taps {
            let (sink, handle) = tap();
            let n = b.add_stereo(Box::new(sink));
            b.connect_ch(strip, 0, n, 0);
            b.connect_ch(strip, 1, n, 1);
            taps.insert(Some(t.id), handle);
        }
        buses.insert(
            t.id,
            BusPorts {
                head,
                pre: prev,
                strip,
            },
        );
        tracks.insert(
            t.id,
            TrackNodes {
                gain: strip,
                pan: None,
                inserts,
                sends: Vec::new(),
            },
        );
    }
    // Ausgänge und Sends der Busse.
    for t in project.tracks.iter().filter(|t| t.kind == TrackKind::Bus) {
        let Some(ports) = buses.get(&t.id) else {
            continue;
        };
        match t.output.and_then(|o| buses.get(&o)) {
            Some(dest) => {
                b.connect_ch(ports.strip, 0, dest.head, 0);
                b.connect_ch(ports.strip, 1, dest.head, 1);
            }
            None => {
                b.connect_ch(ports.strip, 0, sum_l, 0);
                b.connect_ch(ports.strip, 1, sum_r, 0);
            }
        }
        let on = audible_of(t.id);
        let mut sends = Vec::new();
        for s in &t.sends {
            let Some(dest) = buses.get(&s.target) else {
                continue;
            };
            let tap = if s.pre_fader { ports.pre } else { ports.strip };
            let (a, c) = send_gains(t, s.pre_fader, s.linear(), on, false);
            let left = b.add_node(Gain::new(a));
            let right = b.add_node(Gain::new(c));
            b.connect_ch(tap, 0, left, 0);
            b.connect_ch(tap, 1, right, 0);
            b.connect_ch(left, 0, dest.head, 0);
            b.connect_ch(right, 0, dest.head, 1);
            sends.push(SendNodes {
                target: s.target,
                left,
                right,
            });
        }
        if let Some(n) = tracks.get_mut(&t.id) {
            n.sends = sends;
        }
    }

    for t in project.tracks.iter().filter(|t| t.kind != TrackKind::Bus) {
        let Some(src) = source(t) else { continue };
        let on = audible_of(t.id);
        let (g, l, r) = track_gains_with(t, on);
        let src = b.add_boxed(src);
        let mut tail = src;
        let mut inserts = Vec::new();
        for ins in &t.inserts {
            let n = b.add_boxed(Box::new(FxNode::with_context(
                fx_kind(ins.kind),
                &ins.params,
                ins.bypass,
                ctx,
            )));
            b.connect(tail, n);
            inserts.push((ins.id, n));
            tail = n;
        }
        let gain = b.add_node(Gain::new(g));
        let pan_left = b.add_node(Gain::new(l));
        let pan_right = b.add_node(Gain::new(r));
        b.connect(tail, gain);
        b.connect(gain, pan_left);
        b.connect(gain, pan_right);
        if with_taps {
            let (sink, handle) = tap();
            let n = b.add_stereo(Box::new(sink));
            b.connect_ch(pan_left, 0, n, 0);
            b.connect_ch(pan_right, 0, n, 1);
            taps.insert(Some(t.id), handle);
        }
        match t.output.and_then(|o| buses.get(&o)) {
            Some(dest) => {
                b.connect_ch(pan_left, 0, dest.head, 0);
                b.connect_ch(pan_right, 0, dest.head, 1);
            }
            None => {
                b.connect(pan_left, sum_l);
                b.connect(pan_right, sum_r);
            }
        }
        let mut sends = Vec::new();
        for s in &t.sends {
            let Some(dest) = buses.get(&s.target) else {
                continue;
            };
            let (a, c) = send_gains(t, s.pre_fader, s.linear(), on, true);
            let left = b.add_node(Gain::new(a));
            let right = b.add_node(Gain::new(c));
            if s.pre_fader {
                b.connect(tail, left);
                b.connect(tail, right);
            } else {
                b.connect(pan_left, left);
                b.connect(pan_right, right);
            }
            b.connect_ch(left, 0, dest.head, 0);
            b.connect_ch(right, 0, dest.head, 1);
            sends.push(SendNodes {
                target: s.target,
                left,
                right,
            });
        }
        tracks.insert(
            t.id,
            TrackNodes {
                gain,
                pan: Some((pan_left, pan_right)),
                inserts,
                sends,
            },
        );
    }
    ProjectGraph {
        builder: b,
        tracks,
        bus_meters,
        taps,
    }
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
    let graph = build_project_graph_mode(project, source, true);
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
