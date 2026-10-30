//! Audio-Clips abspielen: ein [`ClipPlayer`] je Spur liest die Transportposition
//! lock-frei aus dem [`TransportHandle`] und mischt alle Clips der Spur, die gerade laufen.
//!
//! Die Samples liegen fertig dekodiert (mono, Engine-Rate) in `Arc<[f32]>`; der Audio-Thread
//! liest nur, er allokiert nicht.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use woa_dsp::{Node, ProcessBuffers};

use woa_model::{Clip, FadeCurve, Project, TrackId, TrackKind};

use rtrb::Consumer;
use woa_dsp::instrument::NoteEvent;

use crate::midi_clip::midi_track_source;
use crate::plugin_node::PluginChain;
use crate::project::{ProjectGraph, build_project_graph_plugins, build_project_graph_taps};
use woa_dsp::plugin::TrackPlugins;
use crate::transport::{TransportHandle, beats_to_samples};
use crate::{Engine, EngineError};

use crate::decode::{MAX_RATE, MIN_RATE, decode_file, resample_linear};

/// Ein Clip auf der Zeitachse in Samples.
#[derive(Debug, Clone)]
pub struct PlacedClip {
    /// Start auf der Zeitachse (Samples).
    pub start: u64,
    /// Mono-Samples in Engine-Rate, schon auf Versatz und Länge zugeschnitten.
    pub samples: Arc<[f32]>,
    /// Fade-in-Länge in Samples.
    pub fade_in: u64,
    /// Fade-out-Länge in Samples.
    pub fade_out: u64,
    /// Form des Fade-in.
    pub fade_in_curve: FadeCurve,
    /// Form des Fade-out.
    pub fade_out_curve: FadeCurve,
    /// Clip-Gain als Faktor (1.0 = unverändert).
    pub gain: f32,
}

impl PlacedClip {
    /// Ohne Fades.
    pub fn plain(start: u64, samples: Arc<[f32]>) -> Self {
        Self {
            start,
            samples,
            fade_in: 0,
            fade_out: 0,
            fade_in_curve: FadeCurve::Linear,
            fade_out_curve: FadeCurve::Linear,
            gain: 1.0,
        }
    }

    /// Ende auf der Zeitachse (Samples, exklusiv).
    pub fn end(&self) -> u64 {
        self.start + self.samples.len() as u64
    }

    /// Hüllkurve (Fades, ohne Gain) an Stelle `i` des Clips. Echtzeitsicher.
    pub fn envelope(&self, i: u64) -> f32 {
        let len = self.samples.len() as u64;
        let mut g = 1.0f32;
        if self.fade_in > 0 && i < self.fade_in {
            g = g.min(self.fade_in_curve.gain(i as f32 / self.fade_in as f32));
        }
        let left = len.saturating_sub(i);
        if self.fade_out > 0 && left < self.fade_out {
            g = g.min(self.fade_out_curve.gain(left as f32 / self.fade_out as f32));
        }
        g
    }
}

/// Dezibel in einen linearen Faktor.
fn db_to_gain(db: f64) -> f32 {
    10f64.powf(db / 20.0) as f32
}

/// Automatische Überblendung (Paket 21): Überlappen sich zwei Clips einer Spur, blendet der
/// frühere über die Überlappung aus und der spätere ein, mit gleicher Leistung. Eigene,
/// längere Fades bleiben unberührt. Liegt der spätere Clip ganz im früheren, gibt es keine
/// Überblendung (er liegt dann einfach darüber). Läuft beim Bau des Graphen, nie im
/// Audio-Thread.
pub fn apply_crossfades(clips: &mut [PlacedClip]) {
    let mut order: Vec<usize> = (0..clips.len()).collect();
    order.sort_by_key(|&i| clips[i].start);
    for w in order.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (a_end, b_start, b_end) = (clips[a].end(), clips[b].start, clips[b].end());
        if b_start >= a_end || b_end <= a_end {
            continue;
        }
        let overlap = a_end - b_start;
        if overlap > clips[a].fade_out {
            clips[a].fade_out = overlap;
            clips[a].fade_out_curve = FadeCurve::EqualPower;
        }
        if overlap > clips[b].fade_in {
            clips[b].fade_in = overlap;
            clips[b].fade_in_curve = FadeCurve::EqualPower;
        }
    }
}

/// Ein bereits gedehntes Stück im Zwischenspeicher: Quelle (schwach), Ausschnitt, Verhältnis
/// und Halbtöne als Schlüssel.
struct StretchEntry {
    file: std::sync::Weak<[f32]>,
    from: usize,
    until: usize,
    ratio: u64,
    semitones: u64,
    result: Arc<[f32]>,
}

/// Kleiner Zwischenspeicher, damit ein erneuter Graphbau (jede Änderung am Projekt) nicht jedes
/// gedehnte Stück neu rechnet. Nur im Control-Thread benutzt.
static STRETCH_CACHE: std::sync::Mutex<Vec<StretchEntry>> = std::sync::Mutex::new(Vec::new());
const STRETCH_CACHE_LEN: usize = 6;

/// Dehnt und verschiebt die Tonhöhe von `file[from..until]` (Control-Thread, rechnet
/// gegebenenfalls Sekunden lang bei sehr langem Audio, daher mit Zwischenspeicher).
fn processed_samples(
    file: &Arc<[f32]>,
    from: usize,
    until: usize,
    ratio: f64,
    semitones: f64,
) -> Arc<[f32]> {
    let (rb, sb) = (ratio.to_bits(), semitones.to_bits());
    if let Ok(mut cache) = STRETCH_CACHE.lock() {
        cache.retain(|e| e.file.strong_count() > 0);
        if let Some(e) = cache.iter().find(|e| {
            e.from == from
                && e.until == until
                && e.ratio == rb
                && e.semitones == sb
                && e.file.upgrade().is_some_and(|f| Arc::ptr_eq(&f, file))
        }) {
            return Arc::clone(&e.result);
        }
    }
    let result: Arc<[f32]> =
        woa_dsp::stretch::stretch_and_pitch(&file[from..until], ratio, semitones).into();
    if let Ok(mut cache) = STRETCH_CACHE.lock() {
        if cache.len() >= STRETCH_CACHE_LEN {
            cache.remove(0);
        }
        cache.push(StretchEntry {
            file: Arc::downgrade(file),
            from,
            until,
            ratio: rb,
            semitones: sb,
            result: Arc::clone(&result),
        });
    }
    result
}

/// Setzt einen Modell-Clip auf die Zeitachse: Versatz und Länge schneiden die Samples zu,
/// Fades werden in Samples umgerechnet. Hat der Clip Dehnung, Tonhöhe oder folgt er dem
/// Tempo (Paket 23), wird das Stück hier fertig gerechnet; der Audio-Thread spielt es dann
/// wie jedes andere.
pub fn place_clip(clip: &Clip, file: &Arc<[f32]>, tempo_bpm: f64, rate: f32) -> PlacedClip {
    let to = |b: f64| beats_to_samples(b.max(0.0), tempo_bpm, rate);
    // Quell-Beats beziehen sich auf das Tempo der Datei (bei Tempo-Folgen das gemerkte).
    let src = |b: f64| beats_to_samples(b.max(0.0), clip.source_bpm(tempo_bpm), rate);
    // Beide Grenzen einzeln gerundet: zwei Teile eines geteilten Clips schließen so ohne
    // Lücke und ohne Doppelung an derselben Dateistelle an.
    let from = (src(clip.offset_beats) as usize).min(file.len());
    let until =
        (src(clip.offset_beats + clip.source_length_beats()) as usize).clamp(from, file.len());
    let samples: Arc<[f32]> = if clip.is_processed(tempo_bpm) {
        let ratio = clip.time_ratio(tempo_bpm);
        processed_samples(file, from, until, ratio, clip.pitch_semitones)
    } else if from == 0 && until == file.len() {
        Arc::clone(file)
    } else {
        file[from..until].into()
    };
    PlacedClip {
        start: to(clip.start_beats),
        samples,
        fade_in: to(clip.fade_in_beats),
        fade_out: to(clip.fade_out_beats),
        fade_in_curve: clip.fade_in_curve,
        fade_out_curve: clip.fade_out_curve,
        gain: db_to_gain(clip.gain_db),
    }
}

/// Baut den Projekt-Graphen mit einem [`ClipPlayer`] je Audio-Spur. `load` liefert die
/// dekodierten Samples zu einem Clip-Pfad (oder `None`: Clip bleibt stumm). `meters`
/// bekommt je Spur einen Pegelmesser, wenn gesetzt.
pub fn clip_graph(
    project: &Project,
    transport: &TransportHandle,
    rate: f32,
    load: &mut dyn FnMut(&str) -> Option<Arc<[f32]>>,
    meters: Option<&mut HashMap<TrackId, Arc<AtomicU32>>>,
) -> ProjectGraph {
    clip_graph_live(project, transport, rate, load, meters, None)
}

/// Wie [`clip_graph`], dazu die Live-Queue der scharfen MIDI-Spur (Paket 11) auf der
/// Spur `live.0`. MIDI-Spuren spielen ihre MIDI-Clips (Paket 12) über das Spur-Instrument;
/// die Live-Events gehen unabhängig vom Transport in dasselbe Instrument. Alles läuft
/// durch Fader, Pan, Mute und Solo der Spur.
pub fn clip_graph_live(
    project: &Project,
    transport: &TransportHandle,
    rate: f32,
    load: &mut dyn FnMut(&str) -> Option<Arc<[f32]>>,
    meters: Option<&mut HashMap<TrackId, Arc<AtomicU32>>>,
    live: Option<(TrackId, Consumer<NoteEvent>)>,
) -> ProjectGraph {
    clip_graph_mode(project, transport, rate, load, meters, live, false)
}

/// Wie [`clip_graph_live`], dazu Abgriffe an Master und Spuren fürs Analyse-Fenster
/// ([`ProjectGraph::taps`], Paket 19).
pub fn clip_graph_analysis(
    project: &Project,
    transport: &TransportHandle,
    rate: f32,
    load: &mut dyn FnMut(&str) -> Option<Arc<[f32]>>,
    meters: Option<&mut HashMap<TrackId, Arc<AtomicU32>>>,
    live: Option<(TrackId, Consumer<NoteEvent>)>,
) -> ProjectGraph {
    clip_graph_inner(project, transport, rate, load, meters, live, false, true, None)
}

/// Wie [`clip_graph_live`]; mit `offline` (Export) rechnen die Effekte ohne eigene Threads.
pub fn clip_graph_mode(
    project: &Project,
    transport: &TransportHandle,
    rate: f32,
    load: &mut dyn FnMut(&str) -> Option<Arc<[f32]>>,
    meters: Option<&mut HashMap<TrackId, Arc<AtomicU32>>>,
    live: Option<(TrackId, Consumer<NoteEvent>)>,
    offline: bool,
) -> ProjectGraph {
    clip_graph_inner(project, transport, rate, load, meters, live, offline, false, None)
}

/// Wie [`clip_graph_mode`] (mit Abgriffen, wenn `with_taps`), dazu die Plugins der Spuren
/// (Paket 31): `plugins` liefert je Spur ihre Plugin-Rechner (leer = keine Plugins). Ein
/// Instrument-Plugin ersetzt den eingebauten Synth der MIDI-Spur und nimmt die Live-Noten.
#[allow(clippy::too_many_arguments)]
pub fn clip_graph_plugins(
    project: &Project,
    transport: &TransportHandle,
    rate: f32,
    load: &mut dyn FnMut(&str) -> Option<Arc<[f32]>>,
    meters: Option<&mut HashMap<TrackId, Arc<AtomicU32>>>,
    live: Option<(TrackId, Consumer<NoteEvent>)>,
    offline: bool,
    with_taps: bool,
    plugins: &mut dyn FnMut(&woa_model::Track) -> TrackPlugins,
) -> ProjectGraph {
    clip_graph_inner(
        project,
        transport,
        rate,
        load,
        meters,
        live,
        offline,
        with_taps,
        Some(plugins),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn clip_graph_inner(
    project: &Project,
    transport: &TransportHandle,
    rate: f32,
    load: &mut dyn FnMut(&str) -> Option<Arc<[f32]>>,
    meters: Option<&mut HashMap<TrackId, Arc<AtomicU32>>>,
    live: Option<(TrackId, Consumer<NoteEvent>)>,
    offline: bool,
    with_taps: bool,
    plugins: Option<&mut dyn FnMut(&woa_model::Track) -> TrackPlugins>,
) -> ProjectGraph {
    let live = RefCell::new(live);
    let meters = RefCell::new(meters);
    let take_live = |t: &woa_model::Track| -> Option<Consumer<NoteEvent>> {
        let mut live = live.borrow_mut();
        match live.take() {
            Some((id, q)) if id == t.id => Some(q),
            other => {
                *live = other;
                None
            }
        }
    };
    let mut source = |t: &woa_model::Track| -> Option<Box<dyn Node>> {
        if t.kind == TrackKind::Midi {
            let queue = take_live(t);
            let mut player = midi_track_source(t, project.tempo_bpm, rate, transport, queue)?;
            if let Some(m) = meters.borrow_mut().as_deref_mut() {
                player = player.with_meter(Arc::clone(m.entry(t.id).or_default()));
            }
            return Some(Box::new(player));
        }
        if t.kind != TrackKind::Audio {
            return None;
        }
        let mut clips: Vec<PlacedClip> = t
            .clips
            .iter()
            .filter_map(|c| {
                let file = load(c.audio.as_deref()?)?;
                Some(place_clip(c, &file, project.tempo_bpm, rate))
            })
            .collect();
        apply_crossfades(&mut clips);
        let mut player = ClipPlayer::new(clips, transport.clone());
        if let Some(m) = meters.borrow_mut().as_deref_mut() {
            player = player.with_meter(Arc::clone(m.entry(t.id).or_default()));
        }
        Some(Box::new(player))
    };
    match plugins {
        None => build_project_graph_taps(project, &mut source, offline, with_taps),
        Some(f) => {
            let mut chain = |t: &woa_model::Track| -> Option<PluginChain> {
                if t.plugins.is_empty() {
                    return None;
                }
                let has_instrument = t
                    .plugins
                    .iter()
                    .any(|s| s.role == woa_model::PluginRole::Instrument);
                let queue = if has_instrument { take_live(t) } else { None };
                let meter = if has_instrument {
                    meters
                        .borrow_mut()
                        .as_deref_mut()
                        .map(|m| Arc::clone(m.entry(t.id).or_default()))
                } else {
                    None
                };
                PluginChain::build(t, f(t), project.tempo_bpm, rate, queue, meter)
            };
            build_project_graph_plugins(
                project,
                &mut source,
                offline,
                with_taps,
                Some(&mut chain),
            )
        }
    }
}

/// Ende des letzten Clips in Sekunden.
pub fn project_end_seconds(project: &Project) -> f64 {
    let beats = project
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .map(Clip::end_beats)
        .fold(0.0f64, f64::max);
    beats * 60.0 / project.tempo_bpm
}

/// Rendert das Projekt mit seinen Audio-Clips offline (links, rechts) über `seconds`.
pub fn render_clips(
    project: &Project,
    rate: u32,
    seconds: f64,
    load: &mut dyn FnMut(&str) -> Option<Arc<[f32]>>,
) -> Result<(Vec<f32>, Vec<f32>), EngineError> {
    render_clips_plugins(project, rate, seconds, load, &mut |_| TrackPlugins::default())
}

/// Wie [`render_clips`], mit den Plugins der Spuren (Paket 31).
pub fn render_clips_plugins(
    project: &Project,
    rate: u32,
    seconds: f64,
    load: &mut dyn FnMut(&str) -> Option<Arc<[f32]>>,
    plugins: &mut dyn FnMut(&woa_model::Track) -> TrackPlugins,
) -> Result<(Vec<f32>, Vec<f32>), EngineError> {
    const BLOCK: usize = 512;
    let (mut engine, mut processor) = Engine::new(16);
    let graph = clip_graph_plugins(
        project,
        &engine.transport(),
        rate as f32,
        load,
        None,
        None,
        true,
        false,
        plugins,
    );
    engine.set_graph(graph.builder, rate as f32, BLOCK)?;
    engine.set_tempo(project.tempo_bpm)?;
    engine.play()?;
    let frames = (seconds.max(0.0) * f64::from(rate)) as usize;
    let mut left = vec![0.0f32; frames];
    let mut right = vec![0.0f32; frames];
    for (l, r) in left.chunks_mut(BLOCK).zip(right.chunks_mut(BLOCK)) {
        processor.process_stereo(l, r);
    }
    drop(processor);
    engine.collect();
    Ok((left, right))
}

/// Schreibt Stereo als 32-bit-Float-WAV.
pub fn write_wav_stereo(
    path: &Path,
    left: &[f32],
    right: &[f32],
    rate: u32,
) -> Result<(), EngineError> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).map_err(EngineError::Wav)?;
    for (l, r) in left.iter().zip(right) {
        w.write_sample(*l).map_err(EngineError::Wav)?;
        w.write_sample(*r).map_err(EngineError::Wav)?;
    }
    w.finalize().map_err(EngineError::Wav)
}

/// Samples je Wellenform-Punkt.
pub const PEAK_BLOCK: usize = 256;

/// Spitzenwerte je [`PEAK_BLOCK`] Samples, für die Wellenform-Anzeige.
pub fn peaks(samples: &[f32]) -> Vec<f32> {
    samples
        .chunks(PEAK_BLOCK)
        .map(|c| c.iter().fold(0.0f32, |m, s| m.max(s.abs())))
        .collect()
}

/// Spielt die Clips einer Spur ab, solange der Transport läuft.
pub struct ClipPlayer {
    clips: Vec<PlacedClip>,
    transport: TransportHandle,
    meter: Option<Arc<AtomicU32>>,
}

/// Spitzenpegel seit dem letzten Abholen (f32-Bits), vom Audio-Thread geschrieben.
pub fn take_peak(meter: &AtomicU32) -> f32 {
    f32::from_bits(meter.swap(0, Ordering::Relaxed))
}

impl ClipPlayer {
    pub fn new(clips: Vec<PlacedClip>, transport: TransportHandle) -> Self {
        Self {
            clips,
            transport,
            meter: None,
        }
    }

    /// Meldet den Spitzenpegel jedes Blocks (vor Gain/Mute) in `meter`.
    pub fn with_meter(mut self, meter: Arc<AtomicU32>) -> Self {
        self.meter = Some(meter);
        self
    }
}

impl Node for ClipPlayer {
    fn prepare(&mut self, _sample_rate: f32) {}

    fn set_param(&mut self, _param: u32, _value: f32) {}

    fn process(&mut self, buffers: ProcessBuffers<'_>) {
        let out = buffers.output;
        out.fill(0.0);
        if !self.transport.is_playing() {
            return;
        }
        let pos = self.transport.position();
        let len = out.len() as u64;
        for clip in &self.clips {
            let end = clip.end();
            if end <= pos || clip.start >= pos + len {
                continue;
            }
            let from = pos.max(clip.start);
            let to = (pos + len).min(end);
            let out_off = (from - pos) as usize;
            let src_off = (from - clip.start) as usize;
            let n = (to - from) as usize;
            let faded = clip.fade_in > 0 || clip.fade_out > 0;
            let g = clip.gain;
            for (k, (o, s)) in out[out_off..out_off + n]
                .iter_mut()
                .zip(&clip.samples[src_off..src_off + n])
                .enumerate()
            {
                let v = if faded {
                    *s * clip.envelope((src_off + k) as u64)
                } else {
                    *s
                };
                // Gain 1.0 lässt das Signal bitgleich.
                *o += if g == 1.0 { v } else { v * g };
            }
        }
        if let Some(m) = &self.meter {
            store_peak(m, out);
        }
    }
}

/// Merkt sich den Spitzenpegel von `block` in `meter`, falls höher. Echtzeitsicher.
pub(crate) fn store_peak(meter: &AtomicU32, block: &[f32]) {
    let peak = block.iter().fold(0.0f32, |a, s| a.max(s.abs()));
    let mut cur = meter.load(Ordering::Relaxed);
    while peak > f32::from_bits(cur) {
        match meter.compare_exchange_weak(cur, peak.to_bits(), Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => break,
            Err(seen) => cur = seen,
        }
    }
}

/// Liest eine Audiodatei (alle `decode::EXTENSIONS`), mischt auf mono und rechnet linear auf
/// `sample_rate` um. Nur außerhalb des Audio-Threads aufrufen.
pub fn load_wav_mono(path: &Path, sample_rate: u32) -> Result<Arc<[f32]>, EngineError> {
    if !(MIN_RATE..=MAX_RATE).contains(&sample_rate) {
        return Err(EngineError::InvalidConfig("sample rate out of range"));
    }
    let d = decode_file(path).map_err(|e| EngineError::Read(format!("{}: {e}", path.display())))?;
    let mono = d.mono();
    if d.rate == sample_rate || mono.is_empty() {
        return Ok(mono.into());
    }
    Ok(resample_linear(&mono, d.rate, sample_rate).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Engine, GraphBuilder};

    #[test]
    fn plays_clip_at_its_position_only_while_playing() {
        let (mut engine, mut proc_) = Engine::new(16);
        let samples: Arc<[f32]> = vec![0.5; 100].into();
        let meter = Arc::new(AtomicU32::new(0));
        let mut b = GraphBuilder::new();
        let n = b.add_node(
            ClipPlayer::new(vec![PlacedClip::plain(50, samples)], engine.transport())
                .with_meter(Arc::clone(&meter)),
        );
        b.set_output(n);
        engine.set_graph(b, 48_000.0, 64).unwrap();
        let mut out = vec![0.0; 64];
        proc_.process(&mut out);
        assert!(out.iter().all(|&s| s == 0.0), "stopped = silent");
        engine.play().unwrap();
        let mut all = vec![0.0; 256];
        for c in all.chunks_mut(64) {
            proc_.process(c);
        }
        // Transport started at 0 after one stopped block (position stays 0 while stopped).
        assert_eq!(all[49], 0.0);
        assert_eq!(all[50], 0.5);
        assert_eq!(all[149], 0.5);
        assert_eq!(all[150], 0.0);
        assert_eq!(take_peak(&meter), 0.5);
        assert_eq!(take_peak(&meter), 0.0, "reset after take");
    }

    #[test]
    fn place_clip_trims_and_fades() {
        // 120 bpm at 4 Hz: one beat = 2 samples.
        let file: Arc<[f32]> = (0..20).map(|i| i as f32).collect::<Vec<_>>().into();
        let mut c = Clip::new(woa_model::ClipId(1), "c", 1.0, 3.0, None);
        c.offset_beats = 2.0;
        c.fade_in_beats = 1.0;
        let p = place_clip(&c, &file, 120.0, 4.0);
        assert_eq!(p.start, 2);
        assert_eq!(&p.samples[..], &[4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
        assert_eq!(p.fade_in, 2);
        assert_eq!(p.envelope(0), 0.0);
        assert_eq!(p.envelope(1), 0.5);
        assert_eq!(p.envelope(2), 1.0);
    }

    #[test]
    fn live_instrument_plays_on_its_midi_track_while_stopped() {
        use woa_dsp::instrument::{NoteEvent, note_queue};
        let mut project = Project::new("live");
        project
            .tracks
            .push(woa_model::Track::new(TrackId(1), "Piano", TrackKind::Midi));
        let mut quiet = woa_model::Track::new(TrackId(2), "Other", TrackKind::Midi);
        quiet.gain_db = -6.0;
        project.tracks.push(quiet);
        let (mut engine, mut proc_) = Engine::new(16);
        let (mut tx, rx) = note_queue(64);
        let graph = clip_graph_live(
            &project,
            &engine.transport(),
            48_000.0,
            &mut |_| None,
            None,
            Some((TrackId(1), rx)),
        );
        assert_eq!(
            graph.tracks.len(),
            1,
            "only the live MIDI track gets a source"
        );
        engine.set_graph(graph.builder, 48_000.0, 256).unwrap();
        tx.push(NoteEvent::note_on(69, 100)).unwrap();
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        let mut peak = 0.0f32;
        for _ in 0..20 {
            proc_.process_stereo(&mut l, &mut r);
            peak = l.iter().fold(peak, |m, x| m.max(x.abs()));
        }
        assert!(
            peak > 0.05,
            "live note audible in the mix without play: {peak}"
        );
    }

    #[test]
    fn render_clips_places_audio_and_exports() {
        let mut project = Project::new("r");
        let mut t = woa_model::Track::new(TrackId(1), "t", TrackKind::Audio);
        t.clips.push(Clip::new(
            woa_model::ClipId(2),
            "c",
            2.0,
            2.0,
            Some("a".into()),
        ));
        project.tracks.push(t);
        let file: Arc<[f32]> = vec![0.5; 48_000].into();
        let (l, _r) =
            render_clips(&project, 48_000, 3.0, &mut |_| Some(Arc::clone(&file))).unwrap();
        assert_eq!(l.len(), 144_000);
        assert_eq!(l[47_999], 0.0, "silent before the clip at 1 s");
        assert!(l[48_100] > 0.3, "clip plays at 1 s");
        assert!((project_end_seconds(&project) - 2.0).abs() < 1e-9);
        let tmp = tempfile::tempdir().unwrap();
        write_wav_stereo(&tmp.path().join("o.wav"), &l, &l, 48_000).unwrap();
        assert_eq!(peaks(&[0.1, -0.7, 0.2]), vec![0.7]);
    }

    #[test]
    fn wav_roundtrip_mono_and_resample() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("a.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 24_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut w = hound::WavWriter::create(&p, spec).unwrap();
        for _ in 0..100 {
            w.write_sample(1.0f32).unwrap();
            w.write_sample(0.0f32).unwrap();
        }
        w.finalize().unwrap();
        let s = load_wav_mono(&p, 24_000).unwrap();
        assert_eq!(s.len(), 100);
        assert!((s[0] - 0.5).abs() < 1e-6);
        assert_eq!(load_wav_mono(&p, 48_000).unwrap().len(), 200);
    }
}
