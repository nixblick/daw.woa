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
use woa_dsp::{Node, ProcessBuffers};

#[cfg(feature = "device")]
pub mod device;

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
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cycle => write!(f, "graph contains a cycle"),
            Self::UnknownNode(id) => write!(f, "unknown node {}", id.0),
            Self::InvalidConfig(what) => write!(f, "invalid config: {what}"),
            Self::QueueFull => write!(f, "command queue to audio thread is full"),
            Self::Wav(e) => write!(f, "wav: {e}"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<hound::Error> for EngineError {
    fn from(e: hound::Error) -> Self {
        Self::Wav(e)
    }
}

/// Graph-Beschreibung im Control-Thread (noch nicht kompiliert).
#[derive(Default)]
pub struct GraphBuilder {
    nodes: Vec<Box<dyn Node>>,
    edges: Vec<(NodeId, NodeId)>,
    output: Option<NodeId>,
}

impl GraphBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, node: impl Node + 'static) -> NodeId {
        self.nodes.push(Box::new(node));
        NodeId(self.nodes.len() - 1)
    }

    /// Verbindet den Ausgang von `from` mit dem Eingang von `to` (Eingänge werden summiert).
    pub fn connect(&mut self, from: NodeId, to: NodeId) {
        self.edges.push((from, to));
    }

    /// Legt fest, welcher Node auf den Engine-Ausgang geht.
    pub fn set_output(&mut self, node: NodeId) {
        self.output = Some(node);
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
        for &(a, b) in &self.edges {
            for id in [a, b] {
                if id.0 >= n {
                    return Err(EngineError::UnknownNode(id));
                }
            }
        }
        let output = match self.output {
            Some(id) if id.0 < n => Some(id.0),
            Some(id) => return Err(EngineError::UnknownNode(id)),
            None => None,
        };

        let mut indegree = vec![0usize; n];
        for &(_, b) in &self.edges {
            indegree[b.0] += 1;
        }
        let mut ready: Vec<usize> = (0..n).filter(|&i| indegree[i] == 0).collect();
        let mut order = Vec::with_capacity(n);
        while let Some(i) = ready.pop() {
            order.push(i);
            for &(a, b) in &self.edges {
                if a.0 == i {
                    indegree[b.0] -= 1;
                    if indegree[b.0] == 0 {
                        ready.push(b.0);
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
        let mut slots: Vec<Option<Box<dyn Node>>> = self.nodes.into_iter().map(Some).collect();
        let mut nodes = Vec::with_capacity(n);
        for &i in &order {
            if let Some(mut node) = slots[i].take() {
                node.prepare(sample_rate);
                nodes.push(node);
            }
        }
        let mut inputs = vec![Vec::new(); n];
        for &(a, b) in &self.edges {
            inputs[position[b.0]].push(position[a.0]);
        }

        let output = output.map(|i| position[i]);
        Ok(CompiledGraph {
            nodes,
            inputs,
            position,
            in_bufs: vec![vec![0.0; max_block]; n],
            out_bufs: vec![vec![0.0; max_block]; n],
            output,
            max_block,
            sample_rate,
        })
    }
}

/// Fertig kompilierter Graph; gehört nach dem Tausch dem Audio-Thread.
pub struct CompiledGraph {
    nodes: Vec<Box<dyn Node>>,
    /// Je Node (kompilierte Position) die Quellen.
    inputs: Vec<Vec<usize>>,
    /// Builder-Index -> kompilierte Position.
    position: Vec<usize>,
    in_bufs: Vec<Vec<f32>>,
    out_bufs: Vec<Vec<f32>>,
    output: Option<usize>,
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
            n.set_param(param, value);
        }
    }

    /// Rechnet `out.len()` Samples, in Stücken von höchstens `max_block`. Allokationsfrei.
    pub fn process(&mut self, out: &mut [f32]) {
        for chunk in out.chunks_mut(self.max_block) {
            self.process_block(chunk);
        }
    }

    fn process_block(&mut self, out: &mut [f32]) {
        let len = out.len();
        for pos in 0..self.nodes.len() {
            let in_buf = &mut self.in_bufs[pos][..len];
            in_buf.fill(0.0);
            for &src in &self.inputs[pos] {
                for (d, s) in in_buf.iter_mut().zip(self.out_bufs[src][..len].iter()) {
                    *d += *s;
                }
            }
            self.nodes[pos].process(ProcessBuffers {
                input: &self.in_bufs[pos][..len],
                output: &mut self.out_bufs[pos][..len],
            });
        }
        match self.output {
            Some(pos) => out.copy_from_slice(&self.out_bufs[pos][..len]),
            None => out.fill(0.0),
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
}

/// Control-Thread-Seite der Engine.
pub struct Engine {
    collector: Collector,
    commands: rtrb::Producer<Command>,
    info: Arc<ArcSwap<GraphInfo>>,
    generation: u64,
}

/// Audio-Thread-Seite der Engine.
pub struct AudioProcessor {
    commands: rtrb::Consumer<Command>,
    graph: Option<Owned<CompiledGraph>>,
}

impl Engine {
    /// Legt Engine und Audio-Prozessor an. `queue_capacity` = Kommandos im Flug.
    pub fn new(queue_capacity: usize) -> (Engine, AudioProcessor) {
        let (producer, consumer) = rtrb::RingBuffer::new(queue_capacity.max(1));
        (
            Engine {
                collector: Collector::new(),
                commands: producer,
                info: Arc::new(ArcSwap::from_pointee(GraphInfo::default())),
                generation: 0,
            },
            AudioProcessor {
                commands: consumer,
                graph: None,
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
        while let Ok(cmd) = self.commands.pop() {
            match cmd {
                Command::SetParam { node, param, value } => {
                    if let Some(g) = self.graph.as_mut() {
                        g.set_param(node, param, value);
                    }
                }
                // Der alte Graph fällt hier; basedrop reiht ihn nur beim Collector ein.
                Command::SwapGraph(new) => self.graph = Some(new),
            }
        }
        match self.graph.as_mut() {
            Some(g) => g.process(out),
            None => out.fill(0.0),
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
