//! Wiedergabe im Fenster: eine Engine am Standard-Ausgang, die den Projekt-Graphen mit
//! [`ClipPlayer`]n je Audio-Spur spielt. Struktur-Änderungen (Spuren, Clips, Tempo) bauen
//! einen neuen Graphen, Mixer-Änderungen gehen als geglättete Parameter.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use std::time::Instant;
use woa_engine::clip::{
    PEAK_BLOCK, clip_graph, load_wav_mono, peaks, project_end_seconds, render_clips, take_peak,
};

use woa_engine::device::{
    OutputConfig, OutputStream, output_device_name, output_sample_rate, pause_output,
    resume_output, start_output_with,
};
use woa_engine::project::{ProjectGraph, TrackNodes};
use woa_engine::transport::{TransportHandle, beats_to_samples, samples_to_beats};
use woa_engine::{AudioProcessor, Engine};
use woa_model::{Project, TrackId};

use crate::xrun::XrunLog;

/// Was einen neuen Graphen verlangt.
type Structure = (u64, Vec<(TrackId, Vec<([u64; 5], Option<String>)>)>);
/// Was nur Parameter ändert.
type Mixer = Vec<(TrackId, u64, u64, bool, bool)>;

pub struct Player {
    engine: Engine,
    transport: TransportHandle,
    stream: OutputStream,
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
    peaks: HashMap<PathBuf, Arc<[f32]>>,
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
            tracing::warn!(file = %file.display(), error = %e, "clip load failed");
            None
        }
    }
}

impl Player {
    /// Öffnet den Standard-Ausgang.
    pub fn open() -> Result<Self, String> {
        let config = OutputConfig::default();
        let rate = output_sample_rate(&config).map_err(|e| e.to_string())?;
        let (engine, processor): (Engine, AudioProcessor) = Engine::new(256);
        let transport = engine.transport();
        // Der Fehlerpfad kann im Audio-Thread laufen: Xruns nur zählen (macht
        // `start_output_with` im Transport), geloggt wird je Minute in `sync`.
        let stream = start_output_with(processor, transport.clone(), &config, |_| {})
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
            peaks: HashMap::new(),
        })
    }

    /// Hält den Graphen beim Projekt. `resolve` macht aus einem Clip-Pfad eine Datei.
    /// Fehler (fehlende Datei) werden geloggt, der Clip bleibt dann stumm.
    pub fn sync(&mut self, project: &Project, resolve: &dyn Fn(&str) -> Option<PathBuf>) {
        self.engine.collect();
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
                device = %self.device,
                suspended = self.suspended,
                playing = self.is_playing(),
                "output underruns (last minute)"
            );
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
                                    c.start_beats,
                                    c.length_beats,
                                    c.offset_beats,
                                    c.fade_in_beats,
                                    c.fade_out_beats,
                                ]
                                .map(f64::to_bits),
                                c.audio.clone(),
                            )
                        })
                        .collect();
                    (t.id, clips)
                })
                .collect(),
        );
        if self.structure.as_ref() != Some(&structure) {
            let transport = self.transport.clone();
            let sr = self.rate;
            let cache = &mut self.cache;
            let mut load = |rel: &str| load_cached(cache, resolve(rel)?, sr);
            let graph = clip_graph(project, &transport, rate, &mut load, Some(&mut self.meters));
            match self.engine.set_graph(graph.builder, rate, 4096) {
                Ok(()) => {
                    self.nodes = graph.tracks;
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
            .map(|t| (t.id, t.gain_db.to_bits(), t.pan.to_bits(), t.mute, t.solo))
            .collect();
        if self.mixer.as_ref() != Some(&mixer) {
            match ProjectGraph::update(&self.nodes, &mut self.engine, project) {
                Ok(()) => self.mixer = Some(mixer),
                Err(e) => tracing::warn!(error = %e, "mixer update failed"),
            }
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
            pause_output(&self.stream)
        } else {
            resume_output(&self.stream)
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

    /// Rendert das ganze Projekt (bis zum Ende des letzten Clips) als Stereo-WAV.
    pub fn export(
        &mut self,
        project: &Project,
        resolve: &dyn Fn(&str) -> Option<PathBuf>,
        out: &Path,
        lufs: Option<f64>,
    ) -> Result<(f64, Option<woa_engine::loudness::Normalized>), String> {
        let seconds = project_end_seconds(project);
        if seconds <= 0.0 {
            return Err("keine Clips zum Exportieren".into());
        }
        let rate = self.rate;
        let cache = &mut self.cache;
        let (mut l, mut r) = render_clips(project, rate, seconds, &mut |rel: &str| {
            load_cached(cache, resolve(rel)?, rate)
        })
        .map_err(|e| e.to_string())?;
        let norm =
            lufs.and_then(|t| woa_engine::loudness::normalize(&mut l, &mut r, rate, t, -1.0));
        woa_engine::export::export_stereo(out, &l, &r, rate).map_err(|e| e.to_string())?;
        Ok((seconds, norm))
    }
}
