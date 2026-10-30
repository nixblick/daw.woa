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

use woa_dsp::automation::{AutoCurve, AutoNode, PanSide, db_to_amp};
use woa_dsp::plugin::BYPASS_PARAM;
use woa_dsp::fx::{BYPASS, FxContext, FxKind, FxNode, FxStereoNode, KeyedFxNode};
use woa_dsp::{DEFAULT_RAMP_SECONDS, Gain, Node, Smoother, StereoBuffers, StereoNode, gain_params};
use woa_model::{
    AutoMode, AutoRange, AutoTarget, AutomationLane, EffectKind, Insert, InsertId, Project, Send,
    Track, TrackId, TrackKind,
};

use crate::clip::store_peak;
use crate::plugin_node::PluginChain;
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
    /// Automation (Paket 18): Pegelkurve in dB und Balancekurve.
    level: Option<AutoCurve>,
    pan: Option<AutoCurve>,
    pos: u64,
    playing: bool,
}

impl BusStrip {
    fn new(gain: f32, left: f32, right: f32, meter: Option<Arc<AtomicU32>>) -> Self {
        Self {
            gain: Smoother::new(gain, DEFAULT_RAMP_SECONDS),
            left: Smoother::new(left, DEFAULT_RAMP_SECONDS),
            right: Smoother::new(right, DEFAULT_RAMP_SECONDS),
            meter,
            level: None,
            pan: None,
            pos: 0,
            playing: true,
        }
    }

    fn with_lanes(mut self, level: Option<AutoCurve>, pan: Option<AutoCurve>) -> Self {
        self.level = level;
        self.pan = pan;
        self
    }
}

impl StereoNode for BusStrip {
    fn prepare(&mut self, sample_rate: f32) {
        self.gain.prepare(sample_rate);
        self.left.prepare(sample_rate);
        self.right.prepare(sample_rate);
        if let Some(c) = &mut self.level {
            c.prepare(sample_rate);
        }
        if let Some(c) = &mut self.pan {
            c.prepare(sample_rate);
        }
    }

    fn locate(&mut self, position: u64, playing: bool) {
        self.pos = position;
        self.playing = playing;
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
        let mut pos = self.pos;
        for i in 0..b.out_left.len() {
            let mut g = self.gain.next_value();
            let (mut bl, mut br) = (self.left.next_value(), self.right.next_value());
            if let Some(c) = &mut self.level {
                g *= db_to_amp(c.value_at(pos));
            }
            if let Some(c) = &mut self.pan {
                let (l, r) = balance(f64::from(c.value_at(pos)));
                bl *= l;
                br *= r;
            }
            b.out_left[i] = b.in_left[i] * g * bl;
            b.out_right[i] = b.in_right[i] * g * br;
            if self.playing {
                pos += 1;
            }
        }
        self.pos = pos;
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
    /// Rechter Spur-Gain, wenn die Spur Plugins hat: sie läuft dann in Stereo (`gain` ist
    /// der linke, `pan` die beiden Balance-Gains).
    pub gain_r: Option<NodeId>,
    /// Plugin-Knoten (Instrument und Effekte) mit der Kennung ihres Platzes.
    pub plugins: Vec<(InsertId, NodeId)>,
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
    /// Absenkungsmesser der dynamischen Effekte (Kompressor, Gate, Limiter, De-Esser) in dB,
    /// größter Wert seit dem Abholen als Bits eines `f32`; abholen mit [`crate::clip::take_peak`].
    pub reductions: HashMap<InsertId, Arc<AtomicU32>>,
}

/// Die Kurve eines Ziels, wenn sie abgespielt wird: Betriebsart Lesen, mindestens ein Punkt,
/// das Ziel (Effekt, Send, Regler) gibt es noch.
fn active_lane(t: &Track, target: AutoTarget) -> Option<(&AutomationLane, AutoRange)> {
    let lane = t.lane(&target)?;
    if lane.mode != AutoMode::Read || lane.points.is_empty() {
        return None;
    }
    let range = target.range_in(t).ok()?;
    Some((lane, range))
}

fn lane_curve(t: &Track, target: AutoTarget, tempo_bpm: f64) -> Option<AutoCurve> {
    let (lane, range) = active_lane(t, target)?;
    let pts: Vec<(f64, f32)> = lane
        .points
        .iter()
        .map(|p| (p.beat, p.value as f32))
        .collect();
    Some(AutoCurve::from_beats(&pts, tempo_bpm, range.step))
}

/// Kurven der Regler eines Effekts: (Reglerindex, Kurve).
fn insert_lanes(t: &Track, ins: &Insert, tempo_bpm: f64) -> Vec<(u32, AutoCurve)> {
    (0..ins.params.len())
        .filter_map(|param| {
            let target = AutoTarget::InsertParam {
                insert: ins.id,
                param,
            };
            lane_curve(t, target, tempo_bpm).map(|c| (param as u32, c))
        })
        .collect()
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
            // Mit abgespielter Kurve bleibt vom festen Faktor nur Mute/Solo (Gain) bzw. 1.
            let gain_lane = active_lane(t, AutoTarget::Gain).is_some();
            // Spuren mit Plugins laufen in Stereo; eine Panorama-Kurve gilt dort nicht.
            let pan_lane = n.gain_r.is_none() && active_lane(t, AutoTarget::Pan).is_some();
            let g = if gain_lane {
                if on { 1.0 } else { 0.0 }
            } else {
                g
            };
            match n.pan {
                Some((pl, pr)) => {
                    engine.set_param(n.gain, gain_params::GAIN, g)?;
                    if let Some(gr) = n.gain_r {
                        engine.set_param(gr, gain_params::GAIN, g)?;
                    }
                    let (l, r) = if pan_lane { (1.0, 1.0) } else { (l, r) };
                    engine.set_param(pl, gain_params::GAIN, l)?;
                    engine.set_param(pr, gain_params::GAIN, r)?;
                }
                None => {
                    let (bal_l, bal_r) = if pan_lane { (1.0, 1.0) } else { (bal_l, bal_r) };
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
                    // Eine abgespielte Kurve führt den Regler selbst.
                    let target = AutoTarget::InsertParam {
                        insert: ins.id,
                        param: i,
                    };
                    if active_lane(t, target).is_some() {
                        continue;
                    }
                    engine.set_param(node, i as u32, *v as f32)?;
                }
                engine.set_param(node, BYPASS, if ins.bypass { 1.0 } else { 0.0 })?;
            }
            for slot in &t.plugins {
                let Some(&(_, node)) = n.plugins.iter().find(|(id, _)| *id == slot.id) else {
                    continue;
                };
                for p in &slot.params {
                    engine.set_param(node, p.id, p.value as f32)?;
                }
                engine.set_param(node, BYPASS_PARAM, if slot.bypass { 1.0 } else { 0.0 })?;
            }
            for s in &t.sends {
                let Some(sn) = n.sends.iter().find(|x| x.target == s.target) else {
                    continue;
                };
                let (a, b) = send_fixed(t, s, on, n.pan.is_some());
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

/// Feste Faktoren der beiden Gains eines Sends. Vor dem Fader hat eine Spur noch kein
/// Panorama, also bekommt der Send das der Spur; nach dem Fader steckt es schon im Signal.
/// Spielt eine Pegel- oder Panoramakurve, rechnet der Node sie selbst und der feste Faktor
/// lässt den jeweiligen Teil weg.
fn send_fixed(track: &Track, s: &Send, audible: bool, is_track: bool) -> (f32, f32) {
    let level_lane = active_lane(track, AutoTarget::Send { target: s.target }).is_some();
    let pan_lane = active_lane(track, AutoTarget::Pan).is_some();
    let level = if level_lane { 1.0 } else { s.linear() as f32 };
    let f = if audible { level } else { 0.0 };
    if s.pre_fader && is_track && !pan_lane {
        let (_, l, r) = track_gains_with(track, true);
        (f * l, f * r)
    } else {
        (f, f)
    }
}

/// Die beiden Nodes eines Sends: ein Gain, oder ein [`AutoNode`] mit Pegel- und (vor dem
/// Fader einer Spur) Panoramakurve.
fn send_nodes(
    b: &mut GraphBuilder,
    t: &Track,
    s: &Send,
    audible: bool,
    is_track: bool,
    tempo_bpm: f64,
) -> (NodeId, NodeId) {
    let (a, c) = send_fixed(t, s, audible, is_track);
    let level = lane_curve(t, AutoTarget::Send { target: s.target }, tempo_bpm);
    let pan = if s.pre_fader && is_track {
        lane_curve(t, AutoTarget::Pan, tempo_bpm)
    } else {
        None
    };
    if level.is_none() && pan.is_none() {
        return (b.add_node(Gain::new(a)), b.add_node(Gain::new(c)));
    }
    let side = |side| pan.clone().map(|p| (p, side));
    let left = b.add_node(AutoNode::new(a, level.clone(), side(PanSide::Left)));
    let right = b.add_node(AutoNode::new(c, level, side(PanSide::Right)));
    (left, right)
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
    build_project_graph_plugins(project, source, offline, with_taps, None)
}

/// Wie [`build_project_graph_taps`]. `plugins` liefert je Spur die Plugin-Knoten (Paket 31);
/// eine Spur mit Plugins läuft ab dem Instrument oder dem ersten Plugin-Effekt in Stereo. Hat
/// sie ein Instrument, ersetzt es die Quelle (`source` wird für diese Spur nicht gerufen).
pub fn build_project_graph_plugins(
    project: &Project,
    source: &mut dyn FnMut(&Track) -> Option<Box<dyn Node>>,
    offline: bool,
    with_taps: bool,
    mut plugins: Option<&mut dyn FnMut(&Track) -> Option<PluginChain>>,
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
    let mut reductions: HashMap<InsertId, Arc<AtomicU32>> = HashMap::new();
    let tempo = project.tempo_bpm;
    // Sidechain-Eingänge (Node, Quellspur), nach dem Bau aller Züge verdrahtet.
    let mut pending_keys: Vec<(NodeId, TrackId)> = Vec::new();

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
            let mut fx =
                FxStereoNode::with_context(fx_kind(ins.kind), &ins.params, ins.bypass, ctx);
            fx.set_lanes(insert_lanes(t, ins, tempo));
            if let Some(m) = reduction_meter(ins.kind) {
                fx.set_meter(Arc::clone(&m));
                reductions.insert(ins.id, m);
            }
            let n = b.add_stereo(Box::new(fx));
            b.connect_ch(prev, 0, n, 0);
            b.connect_ch(prev, 1, n, 1);
            inserts.push((ins.id, n));
            prev = n;
        }
        let meter = Arc::new(AtomicU32::new(0));
        bus_meters.insert(t.id, Arc::clone(&meter));
        let gain_lane = lane_curve(t, AutoTarget::Gain, tempo);
        let pan_lane = lane_curve(t, AutoTarget::Pan, tempo);
        let (g, bal_l, bal_r) = (
            if gain_lane.is_some() {
                if on { 1.0 } else { 0.0 }
            } else {
                g
            },
            if pan_lane.is_some() { 1.0 } else { bal_l },
            if pan_lane.is_some() { 1.0 } else { bal_r },
        );
        let strip = b.add_stereo(Box::new(
            BusStrip::new(g, bal_l, bal_r, Some(meter)).with_lanes(gain_lane, pan_lane),
        ));
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
                gain_r: None,
                plugins: Vec::new(),
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
            let (left, right) = send_nodes(&mut b, t, s, on, false, tempo);
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
        let chain = plugins.as_mut().and_then(|f| f(t));
        let mut chain = chain.filter(|c| c.head.is_some() || !c.effects.is_empty());
        let head = chain.as_mut().and_then(|c| c.head.take());
        let stereo_head = head.is_some();
        let src = match head {
            Some((_, node)) => b.add_stereo(node),
            None => {
                let Some(src) = source(t) else { continue };
                b.add_boxed(src)
            }
        };
        let on = audible_of(t.id);
        let (g, l, r) = track_gains_with(t, on);
        let mut tail = src;
        let mut inserts = Vec::new();
        let mut plugin_nodes: Vec<(InsertId, NodeId)> = Vec::new();
        if let Some(slot) = t.plugins.iter().find(|s| s.role == woa_model::PluginRole::Instrument)
            && stereo_head
        {
            plugin_nodes.push((slot.id, src));
        }
        for ins in &t.inserts {
            let lanes = insert_lanes(t, ins, tempo);
            let meter = reduction_meter(ins.kind);
            let key = ins
                .sidechain
                .filter(|&s| ins.kind.supports_sidechain() && project.track(s).is_some());
            let n = if stereo_head {
                // Ab dem Plugin-Instrument läuft die Kette in Stereo (ohne Sidechain).
                let mut fx =
                    FxStereoNode::with_context(fx_kind(ins.kind), &ins.params, ins.bypass, ctx);
                fx.set_lanes(lanes);
                if let Some(m) = &meter {
                    fx.set_meter(Arc::clone(m));
                }
                let n = b.add_stereo(Box::new(fx));
                b.connect_ch(tail, 0, n, 0);
                b.connect_ch(tail, 1, n, 1);
                n
            } else if let Some(source) = key {
                // Sidechain: Kanal 0 Signal, Kanal 1 Schlüssel (siehe `KeyedFxNode`).
                let mut fx =
                    KeyedFxNode::with_context(fx_kind(ins.kind), &ins.params, ins.bypass, ctx);
                fx.set_lanes(lanes);
                if let Some(m) = &meter {
                    fx.set_meter(Arc::clone(m));
                }
                let n = b.add_stereo(Box::new(fx));
                b.connect_ch(tail, 0, n, 0);
                pending_keys.push((n, source));
                n
            } else {
                let mut fx = FxNode::with_context(fx_kind(ins.kind), &ins.params, ins.bypass, ctx);
                fx.set_lanes(lanes);
                if let Some(m) = &meter {
                    fx.set_meter(Arc::clone(m));
                }
                let n = b.add_boxed(Box::new(fx));
                b.connect(tail, n);
                n
            };
            if let Some(m) = meter {
                reductions.insert(ins.id, m);
            }
            inserts.push((ins.id, n));
            tail = n;
        }
        // Plugin-Effekte: Stereo, nach den eingebauten Effekten. Eine Mono-Kette speist beide
        // Eingänge des ersten Plugins.
        let effects = chain.map(|c| c.effects).unwrap_or_default();
        let stereo = stereo_head || !effects.is_empty();
        let mut tail_is_mono = !stereo_head;
        for (id, node) in effects {
            let n = b.add_stereo(node);
            if tail_is_mono {
                b.connect_ch(tail, 0, n, 0);
                b.connect_ch(tail, 0, n, 1);
                tail_is_mono = false;
            } else {
                b.connect_ch(tail, 0, n, 0);
                b.connect_ch(tail, 1, n, 1);
            }
            plugin_nodes.push((id, n));
            tail = n;
        }
        let gain_lane = lane_curve(t, AutoTarget::Gain, tempo);
        let pan_lane = if stereo {
            None
        } else {
            lane_curve(t, AutoTarget::Pan, tempo)
        };
        let make_gain = |b: &mut GraphBuilder| match &gain_lane {
            Some(c) => b.add_node(AutoNode::new(
                if on { 1.0 } else { 0.0 },
                Some(c.clone()),
                None,
            )),
            None => b.add_node(Gain::new(g)),
        };
        let gain = make_gain(&mut b);
        let gain_r = stereo.then(|| make_gain(&mut b));
        let (pan_left, pan_right) = match pan_lane {
            Some(c) => (
                b.add_node(AutoNode::new(1.0, None, Some((c.clone(), PanSide::Left)))),
                b.add_node(AutoNode::new(1.0, None, Some((c, PanSide::Right)))),
            ),
            None => (b.add_node(Gain::new(l)), b.add_node(Gain::new(r))),
        };
        match gain_r {
            Some(gr) => {
                // Stereo: links und rechts je durch Fader und Balance.
                b.connect_ch(tail, 0, gain, 0);
                b.connect_ch(tail, 1, gr, 0);
                b.connect(gain, pan_left);
                b.connect(gr, pan_right);
            }
            None => {
                b.connect(tail, gain);
                b.connect(gain, pan_left);
                b.connect(gain, pan_right);
            }
        }
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
            let (left, right) = send_nodes(&mut b, t, s, on, true, tempo);
            if s.pre_fader {
                if stereo && !tail_is_mono {
                    b.connect_ch(tail, 0, left, 0);
                    b.connect_ch(tail, 1, right, 0);
                } else {
                    b.connect(tail, left);
                    b.connect(tail, right);
                }
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
                gain_r,
                plugins: plugin_nodes,
            },
        );
    }
    // Sidechains: Schlüssel ist das Signal der Quellspur nach dem Fader, vor dem Panorama
    // (Bus: Mitte aus links und rechts).
    for (key, source) in pending_keys {
        let Some(src) = tracks.get(&source) else {
            continue;
        };
        if let Some(gr) = src.gain_r {
            // Plugin-Spur in Stereo: Mitte aus links und rechts.
            for node in [src.gain, gr] {
                let half = b.add_node(Gain::new(0.5));
                b.connect(node, half);
                b.connect_ch(half, 0, key, 1);
            }
        } else if src.pan.is_some() {
            b.connect_ch(src.gain, 0, key, 1);
        } else {
            for ch in 0..2u8 {
                let half = b.add_node(Gain::new(0.5));
                b.connect_ch(src.gain, ch, half, 0);
                b.connect_ch(half, 0, key, 1);
            }
        }
    }
    ProjectGraph {
        builder: b,
        tracks,
        bus_meters,
        taps,
        reductions,
    }
}

/// Messer für die Absenkung, wenn der Effekt eine kennt.
fn reduction_meter(kind: EffectKind) -> Option<Arc<AtomicU32>> {
    matches!(
        kind,
        EffectKind::Compressor | EffectKind::Gate | EffectKind::Limiter | EffectKind::DeEsser
    )
    .then(|| Arc::new(AtomicU32::new(0)))
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
