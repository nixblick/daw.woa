//! Audio-Graph, RT-Thread, Transport, Aufnahme
//!
//! Aufteilung nach den Echtzeitregeln (CLAUDE.md):
//! - [`Engine`] lebt im Control-Thread: baut Graphen ([`GraphBuilder`]), kompiliert sie
//!   (Topologie, Puffer vorab), schickt Kommandos über `rtrb` und gibt alte Graphen über den
//!   `basedrop`-Collector frei. Die aktuelle Graph-Beschreibung steht lock-free per `arc-swap`
//!   für Leser (GUI, Agent) bereit.
//! - [`AudioProcessor`] lebt im Audio-Thread: liest Kommandos, tauscht den Graphen per
//!   Zeigertausch, rechnet Blöcke. Keine Allokation, kein Mutex, kein I/O.

use std::fmt;
use std::path::Path;
use std::sync::Arc;

use arc_swap::ArcSwap;
use basedrop::{Collector, Owned};
use woa_dsp::{Node, ProcessBuffers, StereoBuffers, StereoNode};

#[cfg(all(feature = "device", target_os = "linux"))]
pub mod capture;
pub mod clip;
pub mod cover;
pub mod decode;
#[cfg(feature = "device")]
pub mod device;

/// Puffergröße für den Sendebetrieb: 0 = Vorgabe des Treibers. Im Radio zählt Stabilität,
/// nicht Latenz. Kurze Proben am Desktop (PipeWire, #51) mit `radio-play`: 128 Frames
/// mit vielen Aussetzern, Treiber-Vorgabe in 2 min ohne. Kein Dauertest.
/// Steht außerhalb von `device`, weil auch Builds ohne Gerät (`woa-headless`) sie als Vorgabe zeigen.
pub const RADIO_BUFFER_FRAMES: u32 = 0;

pub mod export;
pub mod ffmpeg;
pub mod live_peaks;
pub mod localtime;
pub mod loudness;
pub mod midi_clip;
pub mod project;
#[cfg(feature = "device")]
pub mod record;
pub mod stems;
pub mod tap;
pub mod transport;

pub use project::{
    ProjectGraph, TrackNodes, audibility, build_project_graph, build_project_graph_mode,
    build_project_graph_taps, render_project, track_gains,
};
pub use transport::{LoopRange, TransportHandle, beats_to_samples, samples_to_beats};
use transport::{Shared, Transport};

/// Kennung eines Nodes innerhalb eines [`GraphBuilder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub usize);

/// Fehler der Engine.
#[derive(Debug)]
pub enum EngineError {
    /// Der Graph enthält einen Zyklus.
    Cycle,
    /// Eine Kante oder der Ausgang verweist auf einen unbekannten Node.
    UnknownNode(NodeId),
    /// Ungültige Einstellung (Abtastrate, Blockgröße).
    InvalidConfig(&'static str),
    /// Die Kommando-Queue zum Audio-Thread ist voll.
    QueueFull,
    /// WAV schreiben fehlgeschlagen.
    Wav(hound::Error),
    /// Audiodatei lesen fehlgeschlagen (WAV/FLAC/MP3).
    Read(String),
    /// Export (FLAC/MP3) fehlgeschlagen.
    Export(String),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cycle => write!(f, "graph contains a cycle"),
            Self::UnknownNode(id) => write!(f, "unknown node {}", id.0),
            Self::InvalidConfig(what) => write!(f, "invalid config: {what}"),
            Self::QueueFull => write!(f, "command queue to audio thread is full"),
            Self::Wav(e) => write!(f, "wav: {e}"),
            Self::Read(e) => write!(f, "read: {e}"),
            Self::Export(e) => write!(f, "export: {e}"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<hound::Error> for EngineError {
    fn from(e: hound::Error) -> Self {
        Self::Wav(e)
    }
}

/// Ein Node im Graphen: mono (ein Ein-, ein Ausgang) oder stereo (zwei Ein-, zwei Ausgänge,
/// z. B. ein verkoppelter Bus-Kompressor).
enum Slot {
    Mono(Box<dyn Node>),
    Stereo(Box<dyn StereoNode>),
}

impl Slot {
    fn channels(&self) -> usize {
        match self {
            Slot::Mono(_) => 1,
            Slot::Stereo(_) => 2,
        }
    }

    fn prepare(&mut self, rate: f32) {
        match self {
            Slot::Mono(n) => n.prepare(rate),
            Slot::Stereo(n) => n.prepare(rate),
        }
    }

    fn locate(&mut self, position: u64, playing: bool) {
        match self {
            Slot::Mono(n) => n.locate(position, playing),
            Slot::Stereo(n) => n.locate(position, playing),
        }
    }
}

/// Eine Kante: Kanal `from_ch` von `from` geht auf Kanal `to_ch` von `to`.
#[derive(Debug, Clone, Copy)]
struct Edge {
    from: NodeId,
    from_ch: u8,
    to: NodeId,
    to_ch: u8,
}

/// Graph-Beschreibung im Control-Thread (noch nicht kompiliert).
#[derive(Default)]
pub struct GraphBuilder {
    nodes: Vec<Slot>,
    edges: Vec<Edge>,
    output: Option<(NodeId, u8)>,
    output_right: Option<(NodeId, u8)>,
}

impl GraphBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, node: impl Node + 'static) -> NodeId {
        self.nodes.push(Slot::Mono(Box::new(node)));
        NodeId(self.nodes.len() - 1)
    }

    /// Wie [`GraphBuilder::add_node`], für bereits geboxte Nodes.
    pub fn add_boxed(&mut self, node: Box<dyn Node>) -> NodeId {
        self.nodes.push(Slot::Mono(node));
        NodeId(self.nodes.len() - 1)
    }

    /// Fügt einen Stereo-Node ein (Kanäle 0 = links, 1 = rechts, ein- wie ausgangsseitig).
    pub fn add_stereo(&mut self, node: Box<dyn StereoNode>) -> NodeId {
        self.nodes.push(Slot::Stereo(node));
        NodeId(self.nodes.len() - 1)
    }

    /// Verbindet den Ausgang von `from` mit dem Eingang von `to` (Eingänge werden summiert).
    pub fn connect(&mut self, from: NodeId, to: NodeId) {
        self.connect_ch(from, 0, to, 0);
    }

    /// Wie [`GraphBuilder::connect`] mit Kanalwahl (0 = links/mono, 1 = rechts). Ein
    /// Mono-Node kennt nur Kanal 0; ungültige Kanäle meldet `compile`.
    pub fn connect_ch(&mut self, from: NodeId, from_ch: u8, to: NodeId, to_ch: u8) {
        self.edges.push(Edge {
            from,
            from_ch,
            to,
            to_ch,
        });
    }

    /// Legt fest, welcher Node auf den Engine-Ausgang geht.
    pub fn set_output(&mut self, node: NodeId) {
        self.output = Some((node, 0));
    }

    /// Stereo-Ausgang: `left` und `right` gehen auf die beiden Kanäle.
    /// Ohne rechten Ausgang kopiert [`CompiledGraph::process_stereo`] links nach rechts.
    pub fn set_stereo_output(&mut self, left: NodeId, right: NodeId) {
        self.output = Some((left, 0));
        self.output_right = Some((right, 0));
    }

    /// Stereo-Ausgang aus den beiden Kanälen eines Stereo-Nodes (z. B. des Masters).
    pub fn set_stereo_output_of(&mut self, node: NodeId) {
        self.output = Some((node, 0));
        self.output_right = Some((node, 1));
    }

    /// Kompiliert im Control-Thread: topologische Ordnung (Kahn), Puffer vorab.
    pub fn compile(self, sample_rate: f32, max_block: usize) -> Result<CompiledGraph, EngineError> {
        if !(sample_rate.is_finite() && sample_rate > 0.0) {
            return Err(EngineError::InvalidConfig("sample rate"));
        }
        if max_block == 0 {
            return Err(EngineError::InvalidConfig("block size"));
        }
        let n = self.nodes.len();
        let channel_ok = |id: NodeId, ch: u8| -> Result<(), EngineError> {
            match self.nodes.get(id.0) {
                None => Err(EngineError::UnknownNode(id)),
                Some(slot) if usize::from(ch) < slot.channels() => Ok(()),
                Some(_) => Err(EngineError::InvalidConfig("channel of a mono node")),
            }
        };
        for e in &self.edges {
            channel_ok(e.from, e.from_ch)?;
            channel_ok(e.to, e.to_ch)?;
        }
        let resolve = |o: Option<(NodeId, u8)>| -> Result<Option<(usize, u8)>, EngineError> {
            match o {
                Some((id, ch)) => {
                    channel_ok(id, ch)?;
                    Ok(Some((id.0, ch)))
                }
                None => Ok(None),
            }
        };
        let output = resolve(self.output)?;
        let output_right = resolve(self.output_right)?;

        let mut indegree = vec![0usize; n];
        for e in &self.edges {
            indegree[e.to.0] += 1;
        }
        let mut ready: Vec<usize> = (0..n).filter(|&i| indegree[i] == 0).collect();
        let mut order = Vec::with_capacity(n);
        while let Some(i) = ready.pop() {
            order.push(i);
            for e in &self.edges {
                if e.from.0 == i {
                    indegree[e.to.0] -= 1;
                    if indegree[e.to.0] == 0 {
                        ready.push(e.to.0);
                    }
                }
            }
        }
        if order.len() != n {
            return Err(EngineError::Cycle);
        }

        // builder index -> compiled position
        let mut position = vec![0usize; n];
        for (pos, &i) in order.iter().enumerate() {
            position[i] = pos;
        }
        let mut slots: Vec<Option<Slot>> = self.nodes.into_iter().map(Some).collect();
        let mut nodes = Vec::with_capacity(n);
        for &i in &order {
            if let Some(mut node) = slots[i].take() {
                node.prepare(sample_rate);
                nodes.push(node);
            }
        }
        let mut inputs: Vec<[Vec<(usize, u8)>; 2]> =
            (0..n).map(|_| [Vec::new(), Vec::new()]).collect();
        for e in &self.edges {
            inputs[position[e.to.0]][usize::from(e.to_ch)].push((position[e.from.0], e.from_ch));
        }
        let buf = |slot: &Slot| -> [Vec<f32>; 2] {
            [
                vec![0.0; max_block],
                if slot.channels() == 2 {
                    vec![0.0; max_block]
                } else {
                    Vec::new()
                },
            ]
        };
        let in_bufs: Vec<[Vec<f32>; 2]> = nodes.iter().map(buf).collect();
        let out_bufs: Vec<[Vec<f32>; 2]> = nodes.iter().map(buf).collect();

        let output = output.map(|(i, ch)| (position[i], ch));
        let output_right = output_right.map(|(i, ch)| (position[i], ch));
        Ok(CompiledGraph {
            nodes,
            inputs,
            position,
            in_bufs,
            out_bufs,
            output,
            output_right,
            max_block,
            sample_rate,
        })
    }
}

/// Fertig kompilierter Graph; gehört nach dem Tausch dem Audio-Thread.
pub struct CompiledGraph {
    nodes: Vec<Slot>,
    /// Je Node (kompilierte Position) und Eingangskanal die Quellen (Position, Kanal).
    inputs: Vec<[Vec<(usize, u8)>; 2]>,
    /// Builder-Index -> kompilierte Position.
    position: Vec<usize>,
    in_bufs: Vec<[Vec<f32>; 2]>,
    out_bufs: Vec<[Vec<f32>; 2]>,
    output: Option<(usize, u8)>,
    output_right: Option<(usize, u8)>,
    max_block: usize,
    sample_rate: f32,
}

impl CompiledGraph {
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    fn set_param(&mut self, node: NodeId, param: u32, value: f32) {
        if let Some(&pos) = self.position.get(node.0)
            && let Some(n) = self.nodes.get_mut(pos)
        {
            match n {
                Slot::Mono(n) => n.set_param(param, value),
                Slot::Stereo(n) => n.set_param(param, value),
            }
        }
    }

    /// Meldet allen Knoten die Transportposition am Anfang des nächsten Abschnitts
    /// (Automation, Paket 18). Allokationsfrei.
    pub fn locate(&mut self, position: u64, playing: bool) {
        for n in &mut self.nodes {
            n.locate(position, playing);
        }
    }

    /// Rechnet `out.len()` Samples, in Stücken von höchstens `max_block`. Allokationsfrei.
    pub fn process(&mut self, out: &mut [f32]) {
        for chunk in out.chunks_mut(self.max_block) {
            self.process_block(chunk);
        }
    }

    /// Rechnet Stereo in zwei gleich lange Puffer. Allokationsfrei.
    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32]) {
        let len = left.len().min(right.len());
        let mut done = 0;
        while done < len {
            let n = (len - done).min(self.max_block);
            self.run_nodes(n);
            let l = &mut left[done..done + n];
            let r = &mut right[done..done + n];
            match self.output {
                Some((pos, ch)) => l.copy_from_slice(&self.out_bufs[pos][usize::from(ch)][..n]),
                None => l.fill(0.0),
            }
            match self.output_right {
                Some((pos, ch)) => r.copy_from_slice(&self.out_bufs[pos][usize::from(ch)][..n]),
                None => r.copy_from_slice(l),
            }
            done += n;
        }
    }

    fn process_block(&mut self, out: &mut [f32]) {
        let len = out.len();
        self.run_nodes(len);
        match self.output {
            Some((pos, ch)) => out.copy_from_slice(&self.out_bufs[pos][usize::from(ch)][..len]),
            None => out.fill(0.0),
        }
    }

    fn run_nodes(&mut self, len: usize) {
        for pos in 0..self.nodes.len() {
            let channels = self.nodes[pos].channels();
            for ch in 0..channels {
                let in_buf = &mut self.in_bufs[pos][ch][..len];
                in_buf.fill(0.0);
                for &(src, src_ch) in &self.inputs[pos][ch] {
                    let from = &self.out_bufs[src][usize::from(src_ch)][..len];
                    for (d, s) in in_buf.iter_mut().zip(from.iter()) {
                        *d += *s;
                    }
                }
            }
            let [in_l, in_r] = &self.in_bufs[pos];
            let [out_l, out_r] = &mut self.out_bufs[pos];
            match &mut self.nodes[pos] {
                Slot::Mono(node) => node.process(ProcessBuffers {
                    input: &in_l[..len],
                    output: &mut out_l[..len],
                }),
                Slot::Stereo(node) => node.process(StereoBuffers {
                    in_left: &in_l[..len],
                    in_right: &in_r[..len],
                    out_left: &mut out_l[..len],
                    out_right: &mut out_r[..len],
                }),
            }
        }
    }
}

/// Lesbare Beschreibung des aktiven Graphen (für GUI/Agent, lock-free per `arc-swap`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GraphInfo {
    pub generation: u64,
    pub node_count: usize,
    pub sample_rate: f32,
}

/// Kommandos vom Control- an den Audio-Thread.
pub enum Command {
    SetParam {
        node: NodeId,
        param: u32,
        value: f32,
    },
    SwapGraph(Owned<CompiledGraph>),
    /// Transport starten.
    Play,
    /// Transport anhalten (Position bleibt).
    Stop,
    /// Position in Samples setzen.
    Seek(u64),
    /// Loop-Bereich setzen oder aufheben.
    SetLoop(Option<LoopRange>),
}

/// Control-Thread-Seite der Engine.
pub struct Engine {
    collector: Collector,
    commands: rtrb::Producer<Command>,
    info: Arc<ArcSwap<GraphInfo>>,
    generation: u64,
    transport: TransportHandle,
}

/// Audio-Thread-Seite der Engine.
pub struct AudioProcessor {
    commands: rtrb::Consumer<Command>,
    graph: Option<Owned<CompiledGraph>>,
    transport: Transport,
}

impl Engine {
    /// Legt Engine und Audio-Prozessor an. `queue_capacity` = Kommandos im Flug.
    pub fn new(queue_capacity: usize) -> (Engine, AudioProcessor) {
        let (producer, consumer) = rtrb::RingBuffer::new(queue_capacity.max(1));
        let shared = Arc::new(Shared::default());
        shared
            .tempo_bits
            .store(120.0f64.to_bits(), std::sync::atomic::Ordering::Release);
        (
            Engine {
                collector: Collector::new(),
                commands: producer,
                info: Arc::new(ArcSwap::from_pointee(GraphInfo::default())),
                generation: 0,
                transport: TransportHandle {
                    shared: Arc::clone(&shared),
                },
            },
            AudioProcessor {
                commands: consumer,
                graph: None,
                transport: Transport::new(shared),
            },
        )
    }

    /// Kompiliert den Graphen und schickt ihn zum Tausch an den Audio-Thread.
    pub fn set_graph(
        &mut self,
        builder: GraphBuilder,
        sample_rate: f32,
        max_block: usize,
    ) -> Result<(), EngineError> {
        let graph = builder.compile(sample_rate, max_block)?;
        let node_count = graph.node_count();
        let owned = Owned::new(&self.collector.handle(), graph);
        self.commands
            .push(Command::SwapGraph(owned))
            .map_err(|_| EngineError::QueueFull)?;
        self.generation += 1;
        self.info.store(Arc::new(GraphInfo {
            generation: self.generation,
            node_count,
            sample_rate,
        }));
        Ok(())
    }

    /// Setzt einen Parameter; der Node glättet den Sprung.
    pub fn set_param(&mut self, node: NodeId, param: u32, value: f32) -> Result<(), EngineError> {
        self.commands
            .push(Command::SetParam { node, param, value })
            .map_err(|_| EngineError::QueueFull)
    }

    fn send(&mut self, cmd: Command) -> Result<(), EngineError> {
        self.commands.push(cmd).map_err(|_| EngineError::QueueFull)
    }

    /// Startet den Transport.
    pub fn play(&mut self) -> Result<(), EngineError> {
        self.send(Command::Play)
    }

    /// Hält den Transport an; die Position bleibt stehen.
    pub fn stop(&mut self) -> Result<(), EngineError> {
        self.send(Command::Stop)
    }

    /// Springt an eine Position in Samples.
    pub fn seek(&mut self, position: u64) -> Result<(), EngineError> {
        self.send(Command::Seek(position))
    }

    /// Setzt den Loop-Bereich (Samples) oder hebt ihn mit `None` auf.
    pub fn set_loop(&mut self, range: Option<LoopRange>) -> Result<(), EngineError> {
        self.send(Command::SetLoop(range))
    }

    /// Setzt den Loop-Bereich in Beats beim aktuellen Tempo.
    pub fn set_loop_beats(
        &mut self,
        start_beats: f64,
        end_beats: f64,
        sample_rate: f32,
    ) -> Result<(), EngineError> {
        let tempo = self.transport.tempo_bpm();
        let range = LoopRange::new(
            beats_to_samples(start_beats, tempo, sample_rate),
            beats_to_samples(end_beats, tempo, sample_rate),
        )
        .ok_or(EngineError::InvalidConfig("loop range"))?;
        self.set_loop(Some(range))
    }

    /// Übernimmt das Tempo (z. B. aus `woa_model::Project::tempo_bpm`).
    pub fn set_tempo(&mut self, tempo_bpm: f64) -> Result<(), EngineError> {
        if !(tempo_bpm.is_finite() && tempo_bpm > 0.0) {
            return Err(EngineError::InvalidConfig("tempo"));
        }
        self.transport
            .shared
            .tempo_bits
            .store(tempo_bpm.to_bits(), std::sync::atomic::Ordering::Release);
        Ok(())
    }

    /// Lesezugriff auf Transport und Underrun-Zähler.
    pub fn transport(&self) -> TransportHandle {
        self.transport.clone()
    }

    /// Bisher gezählte Unterläufe.
    pub fn underruns(&self) -> u64 {
        self.transport.underruns()
    }

    /// Gibt im Audio-Thread verworfene Graphen frei. Regelmäßig im Control-Thread aufrufen.
    pub fn collect(&mut self) {
        self.collector.collect();
    }

    /// Lock-free lesbare Graph-Beschreibung.
    pub fn info(&self) -> Arc<ArcSwap<GraphInfo>> {
        Arc::clone(&self.info)
    }
}

impl AudioProcessor {
    /// Ein Audio-Callback: Kommandos abarbeiten, dann rechnen. Allokationsfrei.
    pub fn process(&mut self, out: &mut [f32]) {
        self.drain_commands();
        self.publish_clock();
        let mut done = 0;
        while done < out.len() {
            let n = self.transport.segment(out.len() - done);
            let chunk = &mut out[done..done + n];
            let (pos, playing) = (self.transport.position(), self.transport.is_playing());
            match self.graph.as_mut() {
                Some(g) => {
                    g.locate(pos, playing);
                    g.process(chunk);
                }
                None => chunk.fill(0.0),
            }
            self.transport.advance(n);
            done += n;
        }
    }

    /// Wie [`AudioProcessor::process`], aber Stereo in zwei Puffern. Allokationsfrei.
    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.drain_commands();
        self.publish_clock();
        let len = left.len().min(right.len());
        let mut done = 0;
        while done < len {
            let n = self.transport.segment(len - done);
            let l = &mut left[done..done + n];
            let r = &mut right[done..done + n];
            let (pos, playing) = (self.transport.position(), self.transport.is_playing());
            match self.graph.as_mut() {
                Some(g) => {
                    g.locate(pos, playing);
                    g.process_stereo(l, r);
                }
                None => {
                    l.fill(0.0);
                    r.fill(0.0);
                }
            }
            self.transport.advance(n);
            done += n;
        }
    }

    fn publish_clock(&self) {
        if let Some(g) = self.graph.as_ref() {
            self.transport.publish_clock(g.sample_rate());
        }
    }

    fn drain_commands(&mut self) {
        while let Ok(cmd) = self.commands.pop() {
            match cmd {
                Command::SetParam { node, param, value } => {
                    if let Some(g) = self.graph.as_mut() {
                        g.set_param(node, param, value);
                    }
                }
                // Der alte Graph fällt hier; basedrop reiht ihn nur beim Collector ein.
                Command::SwapGraph(new) => self.graph = Some(new),
                Command::Play => self.transport.play(),
                Command::Stop => self.transport.stop(),
                Command::Seek(pos) => self.transport.seek(pos),
                Command::SetLoop(range) => self.transport.set_loop(range),
            }
        }
    }
}

/// Rendert `frames` Samples offline (ohne Gerät) in einen Puffer.
pub fn render_offline(processor: &mut AudioProcessor, frames: usize, block: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames];
    for chunk in out.chunks_mut(block.max(1)) {
        processor.process(chunk);
    }
    out
}

/// Schreibt Mono-Samples als 32-bit-Float-WAV.
pub fn write_wav(path: &Path, samples: &[f32], sample_rate: u32) -> Result<(), EngineError> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for &s in samples {
        writer.write_sample(s)?;
    }
    writer.finalize()?;
    Ok(())
}

/// Schreibt Stereo (links/rechts getrennt) als 32-bit-Float-WAV.
pub fn write_wav_stereo(
    path: &Path,
    left: &[f32],
    right: &[f32],
    sample_rate: u32,
) -> Result<(), EngineError> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for (&l, &r) in left.iter().zip(right) {
        writer.write_sample(l)?;
        writer.write_sample(r)?;
    }
    writer.finalize()?;
    Ok(())
}

/// Liest ein Stereo-Float-WAV in zwei Kanäle.
pub fn read_wav_stereo(path: &Path) -> Result<(Vec<f32>, Vec<f32>, u32), EngineError> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    if spec.channels != 2 {
        return Err(EngineError::InvalidConfig("wav is not stereo"));
    }
    let samples = reader.samples::<f32>().collect::<Result<Vec<_>, _>>()?;
    let left = samples.iter().step_by(2).copied().collect();
    let right = samples.iter().skip(1).step_by(2).copied().collect();
    Ok((left, right, spec.sample_rate))
}

/// Liest ein Mono-Float-WAV (für Golden-Tests).
pub fn read_wav(path: &Path) -> Result<(Vec<f32>, u32), EngineError> {
    let mut reader = hound::WavReader::open(path)?;
    let rate = reader.spec().sample_rate;
    let samples = reader.samples::<f32>().collect::<Result<Vec<_>, _>>()?;
    Ok((samples, rate))
}

#[cfg(test)]
mod tests {
    use super::*;
    use woa_dsp::{Gain, SineOsc};

    #[test]
    fn cycle_is_rejected() {
        let mut b = GraphBuilder::new();
        let a = b.add_node(Gain::new(1.0));
        let c = b.add_node(Gain::new(1.0));
        b.connect(a, c);
        b.connect(c, a);
        assert!(matches!(b.compile(48_000.0, 64), Err(EngineError::Cycle)));
    }

    #[test]
    fn unknown_node_is_rejected() {
        let mut b = GraphBuilder::new();
        let a = b.add_node(SineOsc::new(1.0, 1.0));
        b.connect(a, NodeId(7));
        assert!(matches!(
            b.compile(48_000.0, 64),
            Err(EngineError::UnknownNode(NodeId(7)))
        ));
    }

    struct Swap;

    impl StereoNode for Swap {
        fn prepare(&mut self, _sample_rate: f32) {}
        fn set_param(&mut self, _param: u32, _value: f32) {}
        fn process(&mut self, b: StereoBuffers<'_>) {
            b.out_left.copy_from_slice(b.in_right);
            b.out_right.copy_from_slice(b.in_left);
        }
    }

    #[test]
    fn stereo_node_routes_channels_and_rejects_bad_ones() {
        let mut b = GraphBuilder::new();
        let src = b.add_node(Gain::new(1.0));
        let swap = b.add_stereo(Box::new(Swap));
        // Quelle nur auf den linken Eingang, Ausgang rechts -> Engine-Ausgang links.
        b.connect_ch(src, 0, swap, 0);
        b.set_stereo_output_of(swap);
        let mut g = b.compile(48_000.0, 16).expect("compiles");
        // Gain ohne Eingang liefert Stille; hier zählt nur, dass Kanal 1 getrennt bleibt.
        let (mut l, mut r) = ([1.0f32; 8], [1.0f32; 8]);
        g.process_stereo(&mut l, &mut r);
        assert!(l.iter().chain(r.iter()).all(|s| *s == 0.0));

        let mut b = GraphBuilder::new();
        let a = b.add_node(Gain::new(1.0));
        let c = b.add_node(Gain::new(1.0));
        b.connect_ch(a, 1, c, 0);
        assert!(matches!(
            b.compile(48_000.0, 16),
            Err(EngineError::InvalidConfig(_))
        ));

        let mut b = GraphBuilder::new();
        let s1 = b.add_stereo(Box::new(Swap));
        let s2 = b.add_stereo(Box::new(Swap));
        b.connect_ch(s1, 1, s2, 0);
        b.connect_ch(s2, 0, s1, 1);
        assert!(matches!(b.compile(48_000.0, 16), Err(EngineError::Cycle)));
    }

    #[test]
    fn info_is_published() {
        let (mut e, _p) = Engine::new(8);
        let mut b = GraphBuilder::new();
        let s = b.add_node(SineOsc::new(1.0, 1.0));
        b.set_output(s);
        e.set_graph(b, 48_000.0, 64).unwrap();
        let info = e.info().load_full();
        assert_eq!(info.generation, 1);
        assert_eq!(info.node_count, 1);
    }
}
