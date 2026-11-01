//! Wiedergabe im Fenster: eine Engine am Standard-Ausgang, die den Projekt-Graphen mit
//! [`ClipPlayer`]n je Audio-Spur spielt. Struktur-Änderungen (Spuren, Clips, Tempo) bauen
//! einen neuen Graphen, Mixer-Änderungen gehen als geglättete Parameter.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use std::time::Instant;
use woa_dsp::instrument::note_queue;
use woa_engine::clip::{
    PEAK_BLOCK, clip_graph_plugins, load_wav_mono, peaks, project_end_seconds, take_peak,
};

use woa_engine::device::{
    ManagedOutput, OutputConfig, PairsRender, count_if_xrun, output_device_name, output_sample_rate,
};
use woa_engine::project::{ProjectGraph, TrackNodes};
use woa_engine::tap::TapHandle;
use woa_engine::transport::{LoopRange, TransportHandle, beats_to_samples, samples_to_beats};
use woa_engine::{AudioProcessor, Engine};
use woa_model::{InsertId, Project, TrackId};

use crate::export_dialog::StemOptions;
use crate::xrun::XrunLog;
use woa_engine::export::ExportSpec;

/// Ergebnis eines Stem-Exports.
#[derive(Debug, Clone, PartialEq)]
pub struct StemsDone {
    pub seconds: f64,
    pub files: usize,
    /// Spuren ohne Datei, weil sie nicht klingen.
    pub skipped: Vec<String>,
    pub norm: Option<woa_engine::loudness::Normalized>,
}

/// Was einen neuen Graphen verlangt (Tempo, Clips, scharfe MIDI-Spur).
/// Das letzte Feld ist das Routing (Art, Ausgang, Sends, Insert-Kette je Spur).
type Structure = (
    u64,
    Vec<(TrackId, Vec<([u64; 9], Option<String>, Option<u64>)>)>,
    Option<TrackId>,
    String,
);

/// Routing und Effektketten als Schlüssel: Ändert sich eines davon, braucht es einen neuen Graphen.
fn routing_key(project: &Project) -> String {
    use std::fmt::Write;
    let mut key = String::new();
    for t in &project.tracks {
        // Klang und Bank (Pakete J1, J7) wählen die SoundFont des Graphen.
        let _ = write!(
            key,
            "{}:{:?}:{:?}:{:?}",
            t.id.0,
            t.kind,
            t.output.map(|o| o.0),
            t.sound
        );
        for s in &t.sends {
            let _ = write!(key, "|s{}{}", s.target.0, u8::from(s.pre_fader));
        }
        for i in &t.inserts {
            let _ = write!(
                key,
                "|i{}{:?}{:?}",
                i.id.0,
                i.kind,
                i.sidechain.map(|s| s.0)
            );
        }
        // Kurven: jede Änderung (Punkt, Betriebsart) baut den Graphen neu, Ziehen schreibt
        // erst beim Loslassen.
        for l in &t.automation {
            let _ = write!(key, "|a{:?}{:?}", l.target, l.mode);
            for p in &l.points {
                let _ = write!(key, ",{}:{}", p.beat.to_bits(), p.value.to_bits());
            }
        }
        for p in &t.plugins {
            let _ = write!(key, "|p{}{:?}{}", p.id.0, p.role, p.clap_id);
        }
        key.push(';');
    }
    key
}

/// Plätze in der Note-Queue der scharfen MIDI-Spur.
const LIVE_QUEUE: usize = 1024;
/// Was nur Parameter ändert.
type Mixer = Vec<(TrackId, u64, u64, bool, bool, Vec<u64>)>;

pub struct Player {
    engine: Engine,
    transport: TransportHandle,
    stream: ManagedOutput,
    /// Strom ruht (Radio-Modus: dort spielt der Sendemixer, ein zweiter Strom auf
    /// demselben Gerät kostet nur Aussetzer, #51).
    suspended: bool,
    xruns: XrunLog,
    buffer_frames: Option<u32>,
    device: String,
    rate: u32,
    cache: HashMap<PathBuf, Arc<[f32]>>,
    nodes: HashMap<TrackId, TrackNodes>,
    structure: Option<Structure>,
    mixer: Option<Mixer>,
    meters: HashMap<TrackId, Arc<AtomicU32>>,
    /// Absenkungsmesser der dynamischen Effekte (Kompressor, Gate ...).
    reductions: HashMap<InsertId, Arc<AtomicU32>>,
    peaks: HashMap<PathBuf, Arc<[f32]>>,
    /// Producer der Note-Queue des Live-Instruments; der MIDI-Eingang schreibt hinein.
    midi_sink: woa_io::midi::EventSink,
    /// Zuletzt gesetzter Loop-Bereich in Samples.
    loop_range: Option<LoopRange>,
    /// Abgriffe des Graphen fürs Analyse-Fenster: `None` ist der Master.
    taps: HashMap<Option<TrackId>, TapHandle>,
    /// Was das Analyse-Fenster hören will (`None`: Fenster zu, alle Abgriffe aus).
    tap_source: Option<Option<TrackId>>,
    /// Geladene CLAP-Plugins des Projekts; entsteht erst, wenn ein Projekt Plugins hat oder der
    /// Nutzer den Plugin-Browser öffnet (die Suche lädt Fremdcode).
    rack: Option<woa_plugins::PluginRack>,
    /// Zählt Änderungen am Rack; geht in den Graph-Schlüssel ein.
    plugin_epoch: u64,
    /// Gesperrte Plugin-Dateien (Paket 34); die Suche lädt sie nicht.
    blocklist: woa_plugins::Blocklist,
    /// Log-Ordner je Plugin.
    plugin_logs: woa_plugins::PluginLogs,
}

/// Dekodiert einmal je Datei und merkt sich das Ergebnis; Fehler werden geloggt.
fn load_cached(
    cache: &mut HashMap<PathBuf, Arc<[f32]>>,
    file: PathBuf,
    rate: u32,
) -> Option<Arc<[f32]>> {
    if let Some(s) = cache.get(&file) {
        return Some(Arc::clone(s));
    }
    match load_wav_mono(&file, rate) {
        Ok(s) => {
            cache.insert(file, Arc::clone(&s));
            Some(s)
        }
        Err(e) => {
            // Ticket: nur die Meldung; Fehlertext (kann den Pfad tragen) und Datei nur lokal.
            tracing::warn!("clip load failed");
            tracing::warn!(target: woa_telemetry::LOCAL_TARGET, datei = %file.display(), error = %e, "clip load failed");
            None
        }
    }
}

impl Player {
    /// Öffnet den Standard-Ausgang.
    pub fn open() -> Result<Self, String> {
        let config = OutputConfig::default();
        let rate = output_sample_rate(&config).map_err(|e| e.to_string())?;
        let (engine, processor): (Engine, AudioProcessor) = Engine::new(2048);
        let transport = engine.transport();
        // Der Fehlerpfad kann im Audio-Thread laufen: Xruns nur zählen (macht
        // `start_output_with` im Transport), geloggt wird je Minute in `sync`.
        let xrun_transport = transport.clone();
        let mut processor = processor;
        let render: PairsRender = Box::new(move |bufs, frames| {
            if let Some(b) = bufs.first_mut() {
                processor.process_stereo(&mut b.l[..frames], &mut b.r[..frames]);
            }
        });
        let stream = ManagedOutput::open("player", &config, 1, true, render, move |e| {
            count_if_xrun(&e, &xrun_transport);
        })
        .map_err(|e| e.to_string())?;
        let device = output_device_name(&config);
        tracing::info!(rate, buffer_frames = ?config.buffer_frames, %device, "playback ready");
        Ok(Self {
            engine,
            transport,
            stream,
            suspended: false,
            xruns: XrunLog::new(Instant::now()),
            buffer_frames: config.buffer_frames,
            device,
            rate,
            cache: HashMap::new(),
            nodes: HashMap::new(),
            structure: None,
            mixer: None,
            meters: HashMap::new(),
            reductions: HashMap::new(),
            peaks: HashMap::new(),
            midi_sink: woa_io::midi::event_sink(),
            loop_range: None,
            taps: HashMap::new(),
            tap_source: None,
            rack: None,
            plugin_epoch: 0,
            blocklist: woa_plugins::Blocklist::load(&woa_plugins::Blocklist::default_path(
                &woa_radio::config_dir(),
            )),
            plugin_logs: woa_plugins::PluginLogs::new(
                woa_radio::data_dir().join("logs").join("plugins"),
            ),
        })
    }

    /// Rettungsknopf „Ausgabe neu verbinden“ für den Player-Strom.
    pub fn reconnect_output(&mut self) {
        self.stream.reconnect_now();
    }

    /// Sample-Uhr des Audio-Threads (MIDI-Aufnahme stempelt damit, Paket 12).
    pub fn clock(&self) -> Arc<woa_dsp::clock::SampleClock> {
        self.transport.clock()
    }

    /// Kopfposition in Samples.
    pub fn position_samples(&self) -> u64 {
        self.transport.position()
    }

    /// Senke für den MIDI-Eingang (Note-Queue der scharfen MIDI-Spur).
    pub fn midi_sink(&self) -> woa_io::midi::EventSink {
        Arc::clone(&self.midi_sink)
    }

    /// Hält den Graphen beim Projekt. `resolve` macht aus einem Clip-Pfad eine Datei.
    /// Fehler (fehlende Datei) werden geloggt, der Clip bleibt dann stumm. `live` ist die
    /// scharfe MIDI-Spur: Sie bekommt ein Instrument, das der MIDI-Eingang spielt.
    pub fn sync(
        &mut self,
        project: &Project,
        resolve: &dyn Fn(&str) -> Option<PathBuf>,
        live: Option<TrackId>,
    ) {
        self.engine.collect();
        if !self.suspended {
            self.stream.poll();
        }
        let total = self.engine.underruns();
        if let Some(new) = self.xruns.due(total, Instant::now())
            && new > 0
        {
            tracing::warn!(
                output = "player",
                underruns = new,
                total,
                buffer_frames = ?self.buffer_frames,
                rate = self.rate,
                device = %woa_telemetry::short_device(&self.device),
                suspended = self.suspended,
                playing = self.is_playing(),
                "output underruns (last minute)"
            );
        }
        if project.tracks.iter().any(|t| !t.plugins.is_empty()) {
            self.ensure_rack();
        }
        if let Some(rack) = self.rack.as_mut()
            && rack.sync(project)
        {
            self.plugin_epoch += 1;
        }
        let rate = self.rate as f32;
        let structure: Structure = (
            project.tempo_bpm.to_bits(),
            project
                .tracks
                .iter()
                .map(|t| {
                    let clips = t
                        .clips
                        .iter()
                        .map(|c| {
                            (
                                [
                                    c.start_beats.to_bits(),
                                    c.length_beats.to_bits(),
                                    c.offset_beats.to_bits(),
                                    c.fade_in_beats.to_bits(),
                                    c.fade_out_beats.to_bits(),
                                    // Gain und beide Kurven (Paket 21) ändern den Klang.
                                    c.gain_db.to_bits()
                                        ^ (u64::from(c.fade_in_curve as u8) << 1)
                                        ^ (u64::from(c.fade_out_curve as u8) << 3),
                                    // Dehnung, Tonhöhe und Tempo-Folgen (Paket 23).
                                    c.stretch.to_bits(),
                                    c.pitch_semitones.to_bits(),
                                    c.follow_tempo_bpm.map_or(0, f64::to_bits),
                                ],
                                c.audio.clone(),
                                c.midi.as_ref().map(woa_model::MidiData::fingerprint),
                            )
                        })
                        .collect();
                    (t.id, clips)
                })
                .collect(),
            live,
            // Eine fertig geladene Bank (Paket J7) baut den Graphen neu.
            format!(
                "{}#{}#{}",
                routing_key(project),
                self.plugin_epoch,
                woa_engine::banks::generation()
            ),
        );
        if self.structure.as_ref() != Some(&structure) {
            let (live_graph, producer) = match live {
                Some(id) => {
                    let (producer, consumer) = note_queue(LIVE_QUEUE);
                    (Some((id, consumer)), Some(producer))
                }
                None => (None, None),
            };
            let transport = self.transport.clone();
            let sr = self.rate;
            let cache = &mut self.cache;
            let mut load = |rel: &str| load_cached(cache, resolve(rel)?, sr);
            let rack = self.rack.as_ref();
            let graph = clip_graph_plugins(
                project,
                &transport,
                rate,
                &mut load,
                Some(&mut self.meters),
                live_graph,
                false,
                true,
                &mut |t| rack.map_or_else(Default::default, |r| r.track_plugins(t)),
            );
            match self.engine.set_graph(graph.builder, rate, 4096) {
                Ok(()) => {
                    if self.structure.as_ref().map(|s| s.2) != Some(live) {
                        tracing::info!(track = ?live.map(|t| t.0), "midi live track");
                    }
                    match self.midi_sink.lock() {
                        Ok(mut g) => *g = producer,
                        Err(e) => tracing::warn!(error = %e, "midi sink poisoned"),
                    }
                    for (id, m) in graph.bus_meters {
                        self.meters.insert(id, m);
                    }
                    self.reductions = graph.reductions;
                    self.nodes = graph.tracks;
                    self.taps = graph.taps;
                    self.apply_tap_source();
                    self.structure = Some(structure);
                    self.mixer = None;
                }
                Err(e) => tracing::warn!(error = %e, "graph swap failed"),
            }
            if let Err(e) = self.engine.set_tempo(project.tempo_bpm) {
                tracing::warn!(error = %e, "tempo failed");
            }
        }
        let mixer: Mixer = project
            .tracks
            .iter()
            .map(|t| {
                let mut extra: Vec<u64> = t.sends.iter().map(|s| s.level_db.to_bits()).collect();
                for i in &t.inserts {
                    extra.push(u64::from(i.bypass));
                    extra.extend(i.params.iter().map(|p| p.to_bits()));
                }
                for p in &t.plugins {
                    extra.push(u64::from(p.bypass));
                    for q in &p.params {
                        extra.push(u64::from(q.id));
                        extra.push(q.value.to_bits());
                    }
                }
                (
                    t.id,
                    t.gain_db.to_bits(),
                    t.pan.to_bits(),
                    t.mute,
                    t.solo,
                    extra,
                )
            })
            .collect();
        if self.mixer.as_ref() != Some(&mixer) {
            match ProjectGraph::update(&self.nodes, &mut self.engine, project) {
                Ok(()) => self.mixer = Some(mixer),
                Err(e) => tracing::warn!(error = %e, "mixer update failed"),
            }
        }
    }

    fn ensure_rack(&mut self) {
        if self.rack.is_none() {
            let isolation = plugin_isolation(&self.plugin_logs);
            let catalog = Arc::new(scan_plugins(&isolation, &mut self.blocklist));
            for p in catalog.problems() {
                tracing::warn!("plugin scan problem");
                tracing::warn!(target: woa_telemetry::LOCAL_TARGET, datei = %p.path.display(), error = %p.reason, "plugin scan problem");
            }
            tracing::info!(found = catalog.plugins().len(), "plugin scan done");
            self.rack = Some(
                woa_plugins::PluginRack::new(catalog, self.rate as f32).with_isolation(isolation),
            );
        }
    }

    /// Die Plugins dieses Rechners (sucht beim ersten Aufruf).
    pub fn plugin_list(&mut self) -> Vec<woa_plugins::PluginInfo> {
        self.ensure_rack();
        self.rack
            .as_ref()
            .map_or_else(Vec::new, |r| r.catalog().plugins().to_vec())
    }

    /// Sucht die Plugins neu; alle Plätze werden danach neu geladen.
    pub fn rescan_plugins(&mut self) {
        let isolation = match self.rack.as_ref() {
            Some(r) => r.isolation().clone(),
            None => plugin_isolation(&self.plugin_logs),
        };
        let catalog = Arc::new(scan_plugins(&isolation, &mut self.blocklist));
        tracing::info!(found = catalog.plugins().len(), "plugin rescan done");
        match self.rack.as_mut() {
            Some(r) => r.reset(catalog),
            None => {
                self.rack = Some(
                    woa_plugins::PluginRack::new(catalog, self.rate as f32)
                        .with_isolation(isolation),
                );
            }
        }
        self.plugin_epoch += 1;
    }

    /// Dateien, die beim Suchen ausfielen (Datei, Grund).
    pub fn plugin_scan_problems(&self) -> Vec<woa_plugins::ScanProblem> {
        self.rack
            .as_ref()
            .map_or_else(Vec::new, |r| r.catalog().problems().to_vec())
    }

    /// Die gesperrten Plugin-Dateien.
    pub fn blocked_plugins(&self) -> Vec<woa_plugins::BlockEntry> {
        self.blocklist.entries().to_vec()
    }

    /// Gibt eine Plugin-Datei frei; sie wird beim nächsten „Neu suchen“ wieder geladen.
    pub fn unblock_plugin(&mut self, file: &std::path::Path) {
        if self.blocklist.unblock(file) {
            self.plugin_logs
                .scan_line(&format!("{}: von Hand freigegeben", file.display()));
        }
    }

    /// Hebt alle Sperren auf.
    pub fn unblock_all_plugins(&mut self) {
        self.blocklist.clear();
        self.plugin_logs
            .scan_line("alle Sperren von Hand aufgehoben");
    }

    /// Sperrt die Datei dieses Plugins (wirkt ab dem nächsten Suchen oder Start; ein laufendes
    /// Plugin wird nicht angerührt). Liefert den Dateinamen für die Statuszeile.
    pub fn block_plugin(&mut self, clap_id: &str) -> Option<String> {
        let path = self.rack.as_ref()?.catalog().find(clap_id)?.path.clone();
        if self.blocklist.block(&path, "von Hand gesperrt") {
            self.plugin_logs.line(
                clap_id,
                &format!("Datei {} von Hand gesperrt", path.display()),
            );
        }
        path.file_name().map(|n| n.to_string_lossy().into_owned())
    }

    /// Die letzten Zeilen aus dem Log eines Plugins.
    pub fn plugin_log_tail(&self, clap_id: &str, n: usize) -> Vec<String> {
        self.plugin_logs.tail(clap_id, n)
    }

    /// Der Log-Ordner der Plugins.
    pub fn plugin_log_dir(&self) -> &std::path::Path {
        self.plugin_logs.dir()
    }

    /// Startet ein ausgefallenes Plugin neu (der Graph bleibt derselbe).
    pub fn reload_plugin(&mut self, project: &Project, id: InsertId) -> Result<(), String> {
        match self.rack.as_mut() {
            Some(r) => r.reload(project, id).map_err(|e| e.to_string()),
            None => Err("keine Plugins geladen".into()),
        }
    }

    /// Beendet den Plugin-Prozess hart (Absturztest, nur im Testmenü).
    pub fn simulate_plugin_crash(&mut self, id: InsertId) {
        if let Some(r) = self.rack.as_mut() {
            r.simulate_crash(id);
        }
    }

    /// Ob dieser Platz in einem eigenen Prozess läuft.
    pub fn plugin_isolated(&self, id: InsertId) -> bool {
        self.rack.as_ref().is_some_and(|r| r.is_isolated(id))
    }

    /// Ob irgendein Plugin im eigenen Prozess läuft.
    pub fn has_isolated_plugins(&self) -> bool {
        self.rack.as_ref().is_some_and(|r| r.has_isolated())
    }

    /// Plätze, die seit dem letzten Aufruf ausgefallen sind.
    pub fn new_plugin_failures(&mut self) -> Vec<InsertId> {
        self.rack
            .as_mut()
            .map_or_else(Vec::new, |r| r.new_failures())
    }

    /// Status eines Plugin-Platzes.
    pub fn plugin_status(&self, id: InsertId) -> Option<woa_plugins::SlotStatus> {
        self.rack.as_ref()?.status(id)
    }

    /// Die Regler eines geladenen Plugins.
    pub fn plugin_params(&self, id: InsertId) -> Vec<woa_plugins::ParamDesc> {
        self.rack
            .as_ref()
            .map_or_else(Vec::new, |r| r.params(id).to_vec())
    }

    /// Wert eines Reglers laut Plugin.
    pub fn plugin_value(&mut self, id: InsertId, param: u32) -> Option<f64> {
        self.rack.as_mut()?.param_value(id, param)
    }

    /// Ob das Plugin ein eigenes Fenster anbietet.
    pub fn plugin_has_gui(&mut self, id: InsertId) -> bool {
        self.rack.as_mut().is_some_and(|r| r.has_gui(id))
    }

    /// Ob das Fenster des Plugins offen ist.
    pub fn plugin_gui_open(&self, id: InsertId) -> bool {
        self.rack.as_ref().is_some_and(|r| r.gui_open(id))
    }

    /// Ob irgendein Plugin-Fenster offen ist.
    pub fn any_plugin_gui_open(&self) -> bool {
        self.rack.as_ref().is_some_and(|r| r.any_gui_open())
    }

    /// Öffnet das Plugin-Fenster; der Fehlertext ist für die Statuszeile.
    pub fn open_plugin_gui(&mut self, id: InsertId, title: &str) -> Result<(), String> {
        match self.rack.as_mut() {
            Some(r) => r.open_gui(id, title).map_err(|e| e.to_string()),
            None => Err("Plugin-Fenster: keine Plugins geladen".into()),
        }
    }

    /// Schließt das Plugin-Fenster.
    pub fn close_plugin_gui(&mut self, id: InsertId) {
        if let Some(r) = self.rack.as_mut() {
            r.close_gui(id);
        }
    }

    /// Jedes Bild: Reglerbewegungen aus den Plugin-Fenstern holen, dann die Plugins bedienen.
    pub fn pump_plugins(&mut self) -> Vec<(InsertId, woa_plugins::ParamEdit)> {
        let Some(r) = self.rack.as_mut() else {
            return Vec::new();
        };
        let edits = r.poll_edits();
        r.pump();
        edits
    }

    /// Gleicht die laufenden Plugins mit dem Projekt ab (nach Rückgängig/Wiederholen).
    pub fn reconcile_plugins(&mut self, project: &Project) {
        if let Some(r) = self.rack.as_mut() {
            r.reconcile(project);
        }
    }

    /// Der Zustand eines Plugins als Befehl (Preset-Wechsel im Fenster).
    pub fn capture_plugin_slot(
        &mut self,
        project: &Project,
        id: InsertId,
    ) -> Option<woa_model::Command> {
        self.rack.as_mut()?.capture_slot(project, id)
    }

    /// Befehle, die den Zustand der laufenden Plugins ins Projekt übernehmen.
    pub fn capture_plugin_state(&mut self, project: &Project) -> Vec<woa_model::Command> {
        self.rack
            .as_mut()
            .map_or_else(Vec::new, |r| r.capture_state(project))
    }

    /// Eigene Plugin-Instanzen für einen Export (stört die Wiedergabe nicht).
    fn offline_rack(&self, project: &Project) -> Option<woa_plugins::PluginRack> {
        if project.tracks.iter().all(|t| t.plugins.is_empty()) {
            return None;
        }
        let mut rack = self.rack.as_ref()?.fresh();
        rack.sync(project);
        Some(rack)
    }

    /// Abtastrate der Engine.
    pub fn sample_rate(&self) -> u32 {
        self.rate
    }

    /// Wählt, was das Analyse-Fenster mithört: `Some(None)` den Master, `Some(Some(spur))`
    /// eine Spur (nach Fader und Pan), `None` nichts (alle Abgriffe aus, kein Aufwand im
    /// Audio-Thread außer einem Atomic je Block).
    pub fn set_tap_source(&mut self, source: Option<Option<TrackId>>) {
        if self.tap_source != source {
            self.tap_source = source;
            self.apply_tap_source();
        }
    }

    fn apply_tap_source(&mut self) {
        for (id, tap) in &mut self.taps {
            tap.set_enabled(self.tap_source == Some(*id));
        }
    }

    /// Holt die Samples des gewählten Abgriffs (hängt an). Liefert die seit dem letzten
    /// Abholen verlorenen Frames.
    pub fn drain_tap(&mut self, left: &mut Vec<f32>, right: &mut Vec<f32>) -> u64 {
        match self.tap_source.and_then(|s| self.taps.get_mut(&s)) {
            Some(tap) => tap.drain(left, right),
            None => 0,
        }
    }

    /// Läuft der Ausgabestrom (nicht ruhend)?
    pub fn is_active(&self) -> bool {
        !self.suspended
    }

    /// Lässt den Ausgabestrom ruhen (`true`) oder wieder laufen (`false`). Beim Ruhen
    /// wird die Wiedergabe vorher angehalten.
    pub fn set_suspended(&mut self, suspend: bool) {
        if suspend == self.suspended {
            return;
        }
        if suspend && self.is_playing() {
            self.stop();
        }
        let r = if suspend {
            self.stream.pause()
        } else {
            self.stream.resume()
        };
        match r {
            Ok(()) => self.suspended = suspend,
            Err(e) => tracing::warn!(error = %e, suspend, "player stream pause/resume failed"),
        }
    }

    pub fn is_playing(&self) -> bool {
        self.transport.is_playing()
    }

    pub fn play(&mut self) {
        if let Err(e) = self.engine.play() {
            tracing::warn!(error = %e, "play failed");
        }
    }

    pub fn stop(&mut self) {
        if let Err(e) = self.engine.stop() {
            tracing::warn!(error = %e, "stop failed");
        }
    }

    pub fn toggle(&mut self) {
        if self.is_playing() {
            self.stop()
        } else {
            self.play()
        }
    }

    /// Springt an eine Position in Beats.
    pub fn seek_beats(&mut self, beats: f64, tempo_bpm: f64) {
        let pos = beats_to_samples(beats.max(0.0), tempo_bpm, self.rate as f32);
        if let Err(e) = self.engine.seek(pos) {
            tracing::warn!(error = %e, "seek failed");
        }
    }

    /// Setzt oder löscht den Loop-Bereich in Beats (`start_beats < end_beats`).
    pub fn set_loop_beats(&mut self, range: Option<(f64, f64)>, tempo_bpm: f64) {
        let next = range.and_then(|(s, e)| {
            let start = beats_to_samples(s.max(0.0), tempo_bpm, self.rate as f32);
            let end = beats_to_samples(e.max(0.0), tempo_bpm, self.rate as f32);
            LoopRange::new(start, end)
        });
        if self.loop_range == next {
            return;
        }
        match self.engine.set_loop(next) {
            Ok(()) => self.loop_range = next,
            Err(e) => tracing::warn!(error = %e, "set_loop failed"),
        }
    }

    /// Position in Beats.
    pub fn position_beats(&self, tempo_bpm: f64) -> f64 {
        samples_to_beats(self.transport.position(), tempo_bpm, self.rate as f32)
    }

    /// Position in Sekunden.
    pub fn position_seconds(&self) -> f64 {
        self.transport.position() as f64 / f64::from(self.rate.max(1))
    }

    /// Spitzenpegel einer Spur seit dem letzten Aufruf (vor Gain/Mute), 0..1+.
    pub fn take_level(&self, track: TrackId) -> f32 {
        self.meters.get(&track).map_or(0.0, |m| take_peak(m))
    }

    /// Größte Gain-Reduction in dB eines Effekts seit dem letzten Aufruf (0 ohne Messer).
    pub fn take_reduction(&self, insert: InsertId) -> f32 {
        self.reductions.get(&insert).map_or(0.0, |m| take_peak(m))
    }

    /// Vergisst eine dekodierte Datei (nach neuer Aufnahme unter gleichem Namen).
    pub fn forget(&mut self, file: &Path) {
        self.cache.remove(file);
        self.peaks.remove(file);
    }

    /// Wellenform einer Datei: Spitzenwerte je [`PEAK_BLOCK`] Samples in Engine-Rate.
    pub fn peaks(&mut self, file: &Path) -> Option<Arc<[f32]>> {
        if let Some(p) = self.peaks.get(file) {
            return Some(Arc::clone(p));
        }
        let samples = load_cached(&mut self.cache, file.to_path_buf(), self.rate)?;
        let p: Arc<[f32]> = peaks(&samples).into();
        self.peaks.insert(file.to_path_buf(), Arc::clone(&p));
        Some(p)
    }

    /// Wellenform-Punkte je Beat.
    pub fn peaks_per_beat(&self, tempo_bpm: f64) -> f64 {
        f64::from(self.rate) * 60.0 / tempo_bpm / PEAK_BLOCK as f64
    }

    /// Rendert das ganze Projekt (bis zum Ende des letzten Clips) und schreibt es nach `out`
    /// im Format von `spec`.
    pub fn export(
        &mut self,
        project: &Project,
        resolve: &dyn Fn(&str) -> Option<PathBuf>,
        out: &Path,
        spec: &ExportSpec,
        lufs: Option<f64>,
    ) -> Result<(f64, Option<woa_engine::loudness::Normalized>), String> {
        let (mut l, mut r, seconds) = self.render_mix(project, resolve)?;
        let rate = self.rate;
        let norm =
            lufs.and_then(|t| woa_engine::loudness::normalize(&mut l, &mut r, rate, t, -1.0));
        woa_engine::export::export_with(out, spec, &l, &r, rate).map_err(|e| e.to_string())?;
        Ok((seconds, norm))
    }

    /// Mixdown offline mit Plugins, wie [`Self::export`] ihn schreibt: links, rechts, Sekunden
    /// (Abtastrate der Engine, [`Self::sample_rate`]). Auch für das Jingle (J6).
    pub fn render_mix(
        &mut self,
        project: &Project,
        resolve: &dyn Fn(&str) -> Option<PathBuf>,
    ) -> Result<(Vec<f32>, Vec<f32>, f64), String> {
        let seconds = project_end_seconds(project);
        if seconds <= 0.0 {
            return Err("keine Clips zum Exportieren".into());
        }
        let rate = self.rate;
        let rack = self.offline_rack(project);
        let cache = &mut self.cache;
        let (l, r) = woa_engine::clip::render_clips_plugins(
            project,
            rate,
            seconds,
            &mut |rel: &str| load_cached(cache, resolve(rel)?, rate),
            &mut |t| {
                rack.as_ref()
                    .map_or_else(Default::default, |r| r.track_plugins(t))
            },
        )
        .map_err(|e| e.to_string())?;
        Ok((l, r, seconds))
    }

    /// Bereitet den Export einer Spur vor (Ticket 229): dekodiert die Audio-Dateien hier
    /// (meist schon im Cache) und gibt eine Arbeit zurück, die in einem eigenen Thread rendert
    /// und schreibt. Ergebnis: Länge in Sekunden.
    pub fn track_export_job(
        &mut self,
        project: &Project,
        resolve: &dyn Fn(&str) -> Option<PathBuf>,
        track: TrackId,
        out: PathBuf,
        spec: ExportSpec,
        tags: woa_engine::export::ExportTags,
    ) -> Result<impl FnOnce() -> Result<f64, String> + std::marker::Send + 'static, String> {
        let seconds = project_end_seconds(project);
        if seconds <= 0.0 {
            return Err("keine Clips zum Exportieren".into());
        }
        let rate = self.rate;
        let mut audio: HashMap<String, Arc<[f32]>> = HashMap::new();
        for t in &project.tracks {
            for c in &t.clips {
                if let Some(rel) = c.audio.as_deref()
                    && !audio.contains_key(rel)
                    && let Some(s) =
                        resolve(rel).and_then(|f| load_cached(&mut self.cache, f, rate))
                {
                    audio.insert(rel.to_string(), s);
                }
            }
        }
        let make_rack = if project.tracks.iter().all(|t| t.plugins.is_empty()) {
            None
        } else {
            self.rack.as_ref().map(|r| r.fresh_later())
        };
        let project = project.clone();
        Ok(move || {
            let rack = make_rack.map(|make| {
                let mut r = make();
                r.sync(&project);
                r
            });
            let stem = woa_engine::stems::render_track(
                &project,
                rate,
                seconds,
                track,
                &mut |rel: &str| audio.get(rel).cloned(),
                &mut |t| {
                    rack.as_ref()
                        .map_or_else(Default::default, |r| r.track_plugins(t))
                },
            )
            .map_err(|e| e.to_string())?;
            woa_engine::export::export_tagged(&out, &spec, &stem.left, &stem.right, rate, &tags)
                .map_err(|e| e.to_string())?;
            Ok(seconds)
        })
    }

    /// Rendert Stems (und den Mixdown) in einem Durchgang und schreibt sie in den Ordner
    /// `dir`. Der Lautheitsangleich rechnet auf dem Mixdown; sein Gain gilt für jede Datei
    /// gleich, damit die Summe der Stems der Mixdown bleibt.
    pub fn export_stems(
        &mut self,
        project: &Project,
        resolve: &dyn Fn(&str) -> Option<PathBuf>,
        dir: &Path,
        spec: &ExportSpec,
        opts: StemOptions,
        lufs: Option<f64>,
    ) -> Result<StemsDone, String> {
        let seconds = project_end_seconds(project);
        if seconds <= 0.0 {
            return Err("keine Clips zum Exportieren".into());
        }
        let rate = self.rate;
        let rack = self.offline_rack(project);
        let cache = &mut self.cache;
        let render = woa_engine::stems::render_stems_plugins(
            project,
            rate,
            seconds,
            opts.select,
            &mut |rel: &str| load_cached(cache, resolve(rel)?, rate),
            &mut |t| {
                rack.as_ref()
                    .map_or_else(Default::default, |r| r.track_plugins(t))
            },
        )
        .map_err(|e| e.to_string())?;
        if render.stems.is_empty() {
            return Err("keine Spur zum Exportieren (alles stumm oder nichts gewählt)".into());
        }
        let norm = lufs.and_then(|t| {
            let (mut l, mut r) = (render.mix_left.clone(), render.mix_right.clone());
            woa_engine::loudness::normalize(&mut l, &mut r, rate, t, -1.0)
        });
        let gain_db = norm.as_ref().map_or(0.0, |n| n.gain_db);
        let files = woa_engine::stems::write_stems(dir, &render, spec, rate, gain_db, opts.mixdown)
            .map_err(|e| e.to_string())?;
        Ok(StemsDone {
            seconds,
            files: files.len(),
            skipped: render.skipped,
            norm,
        })
    }
}

/// Wo Plugins laufen: im eigenen Prozess (Absturzschutz, Paket 33), wenn sich der Plugin-Prozess
/// finden lässt; sonst im Programm selbst. `WOA_PLUGINS_IN_PROCESS=1` schaltet den Schutz aus
/// (Fehlersuche).
fn plugin_isolation(logs: &woa_plugins::PluginLogs) -> woa_plugins::Isolation {
    if std::env::var_os("WOA_PLUGINS_IN_PROCESS").is_some_and(|v| v == "1") {
        tracing::warn!(
            "plugins run inside the app (WOA_PLUGINS_IN_PROCESS=1), no crash protection"
        );
        return woa_plugins::Isolation::InProcess;
    }
    match woa_plugins::HostConfig::discover().or_else(woa_plugins::HostConfig::multicall) {
        Some(cfg) => {
            tracing::info!(exe = %cfg.exe.display(), "plugins run in their own process");
            woa_plugins::Isolation::Process(cfg.with_logs(logs.clone()))
        }
        None => {
            tracing::warn!("no plugin host found, plugins run inside the app");
            woa_plugins::Isolation::InProcess
        }
    }
}

fn scan_plugins(
    isolation: &woa_plugins::Isolation,
    blocklist: &mut woa_plugins::Blocklist,
) -> woa_plugins::Catalog {
    match isolation {
        woa_plugins::Isolation::Process(cfg) => {
            woa_plugins::Catalog::scan_default_guarded(cfg, blocklist)
        }
        woa_plugins::Isolation::InProcess => woa_plugins::Catalog::scan_default_filtered(blocklist),
    }
}
