//! Aufnahme vom Eingabegerät in eine WAV-Datei (cpal), nur mit Feature `device`.
//!
//! Der Audio-Thread schiebt die Samples nur in eine lock-freie Queue (`rtrb`), zählt
//! Überläufe und merkt sich den Spitzenpegel als Atomic: keine Allokation, kein I/O,
//! kein Logging. Ein eigener Schreib-Thread holt die Samples ab und schreibt sie
//! fortlaufend als 32-bit-Float-WAV; alle halbe Sekunde aktualisiert er den Kopf, damit
//! die Datei nach einem Absturz bis kurz davor lesbar bleibt.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::device::{DeviceError, OutputDeviceInfo};

/// Sekunden Puffer zwischen Audio- und Schreib-Thread.
const QUEUE_SECONDS: usize = 4;
/// Wie oft der Schreib-Thread den WAV-Kopf aktualisiert.
const FLUSH_EVERY: Duration = Duration::from_millis(500);

/// Was beim Öffnen des Eingangs gewählt wird.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputConfig {
    /// `None` = Standard-Eingang.
    pub device_id: Option<String>,
}

/// Ergebnis einer beendeten Aufnahme.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordStats {
    pub path: PathBuf,
    pub sample_rate: u32,
    pub channels: u16,
    /// Geschriebene Frames (Samples je Kanal).
    pub frames: u64,
    /// Samples, die wegen voller Queue verloren gingen (sollte 0 sein).
    pub overruns: u64,
    /// Höchster Betrag eines Samples über die ganze Aufnahme, 0..1.
    pub peak: f32,
    /// Vom Treiber gemeldete Eingangslatenz (Aufnahme bis Callback), 0 wenn unbekannt.
    pub input_latency: Duration,
}

impl RecordStats {
    /// Länge in Sekunden.
    pub fn seconds(&self) -> f64 {
        self.frames as f64 / f64::from(self.sample_rate.max(1))
    }
}

/// Fehler beim Aufnehmen.
#[derive(Debug)]
pub enum RecordError {
    Device(DeviceError),
    Wav(String),
    WriterPanicked,
}

impl std::fmt::Display for RecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Device(e) => write!(f, "{e}"),
            Self::Wav(e) => write!(f, "wav: {e}"),
            Self::WriterPanicked => write!(f, "writer thread panicked"),
        }
    }
}

impl std::error::Error for RecordError {}

impl From<DeviceError> for RecordError {
    fn from(e: DeviceError) -> Self {
        Self::Device(e)
    }
}

impl From<cpal::Error> for RecordError {
    fn from(e: cpal::Error) -> Self {
        Self::Device(DeviceError::Backend(e))
    }
}

/// Zähler, die Audio-Thread, Schreib-Thread und Aufrufer teilen.
#[derive(Default)]
struct Meters {
    /// Spitzenpegel seit dem letzten [`Recorder::take_level`], als f32-Bits.
    level: AtomicU32,
    /// Spitzenpegel der ganzen Aufnahme, als f32-Bits.
    peak: AtomicU32,
    overruns: AtomicU64,
    /// Vom Schreib-Thread geschriebene Samples (alle Kanäle).
    written: AtomicU64,
    /// Zuletzt gemeldete Eingangslatenz in Mikrosekunden.
    latency_us: AtomicU64,
    stop: AtomicBool,
}

fn store_max(cell: &AtomicU32, value: f32) {
    let mut cur = cell.load(Ordering::Relaxed);
    while value > f32::from_bits(cur) {
        match cell.compare_exchange_weak(cur, value.to_bits(), Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => break,
            Err(seen) => cur = seen,
        }
    }
}

/// Eine laufende Aufnahme. Läuft, bis [`Recorder::stop`] aufgerufen oder der Wert fallen
/// gelassen wird (dann ohne Ergebnis).
pub struct Recorder {
    stream: Option<cpal::Stream>,
    writer: Option<JoinHandle<Result<(), String>>>,
    meters: Arc<Meters>,
    path: PathBuf,
    sample_rate: u32,
    channels: u16,
    started: Instant,
}

impl Recorder {
    /// Spitzenpegel seit dem letzten Aufruf (0..1), setzt ihn zurück. Für Pegelanzeigen.
    pub fn take_level(&self) -> f32 {
        f32::from_bits(self.meters.level.swap(0, Ordering::Relaxed))
    }

    /// Laufzeit seit dem Start.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Zieldatei.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Bisher verlorene Samples.
    pub fn overruns(&self) -> u64 {
        self.meters.overruns.load(Ordering::Relaxed)
    }

    /// Beendet die Aufnahme, wartet auf den Schreib-Thread und liefert das Ergebnis.
    pub fn stop(mut self) -> Result<RecordStats, RecordError> {
        // Erst den Stream schließen, dann kommen keine Samples mehr nach.
        drop(self.stream.take());
        self.meters.stop.store(true, Ordering::Release);
        let writer = self.writer.take().ok_or(RecordError::WriterPanicked)?;
        writer
            .join()
            .map_err(|_| RecordError::WriterPanicked)?
            .map_err(RecordError::Wav)?;
        let channels = u64::from(self.channels.max(1));
        Ok(RecordStats {
            path: self.path.clone(),
            sample_rate: self.sample_rate,
            channels: self.channels,
            frames: self.meters.written.load(Ordering::Acquire) / channels,
            overruns: self.meters.overruns.load(Ordering::Relaxed),
            peak: f32::from_bits(self.meters.peak.load(Ordering::Relaxed)),
            input_latency: Duration::from_micros(self.meters.latency_us.load(Ordering::Relaxed)),
        })
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        drop(self.stream.take());
        self.meters.stop.store(true, Ordering::Release);
        if let Some(w) = self.writer.take() {
            let _ = w.join();
        }
    }
}

/// Listet alle Eingabegeräte des Standard-Hosts.
pub fn list_input_devices() -> Result<Vec<OutputDeviceInfo>, DeviceError> {
    let host = cpal::default_host();
    let default_id = host.default_input_device().and_then(|d| d.id().ok());
    let mut list = Vec::new();
    for device in host.input_devices()? {
        let Ok(id) = device.id() else { continue };
        let name = device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_else(|_| id.to_string());
        list.push(OutputDeviceInfo {
            is_default: default_id.as_ref() == Some(&id),
            id: id.to_string(),
            name,
        });
    }
    Ok(list)
}

fn pick_input(config: &InputConfig) -> Result<cpal::Device, DeviceError> {
    let host = cpal::default_host();
    match &config.device_id {
        None => host
            .default_input_device()
            .ok_or(DeviceError::NoOutputDevice),
        Some(id) => {
            let parsed = cpal::DeviceId::from_str(id)
                .map_err(|_| DeviceError::DeviceNotFound(id.clone()))?;
            host.device_by_id(&parsed)
                .ok_or_else(|| DeviceError::DeviceNotFound(id.clone()))
        }
    }
}

/// Startet eine Aufnahme vom Eingang nach `path` (WAV, wird angelegt; Ordner muss
/// existieren). `on_error` bekommt Stream-Fehler (außerhalb des Audio-Threads loggen!).
pub fn start_recording(
    config: &InputConfig,
    path: &Path,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<Recorder, RecordError> {
    let device = pick_input(config)?;
    let supported = device.default_input_config()?;
    if supported.sample_format() != cpal::SampleFormat::F32 {
        return Err(DeviceError::UnsupportedFormat.into());
    }
    let stream_config = supported.config();
    let sample_rate = stream_config.sample_rate;
    let channels = stream_config.channels.max(1);

    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut wav =
        hound::WavWriter::create(path, spec).map_err(|e| RecordError::Wav(e.to_string()))?;

    let capacity = sample_rate as usize * usize::from(channels) * QUEUE_SECONDS;
    let (mut producer, mut consumer) = rtrb::RingBuffer::<f32>::new(capacity.max(1024));
    let meters = Arc::new(Meters::default());

    let m_audio = Arc::clone(&meters);
    let stream = device.build_input_stream(
        stream_config,
        move |data: &[f32], info: &cpal::InputCallbackInfo| {
            let ts = info.timestamp();
            let d = ts.callback.duration_since(ts.capture);
            m_audio
                .latency_us
                .store(d.as_micros() as u64, Ordering::Relaxed);
            let mut peak = 0.0f32;
            for &s in data {
                peak = peak.max(s.abs());
                if producer.push(s).is_err() {
                    m_audio.overruns.fetch_add(1, Ordering::Relaxed);
                }
            }
            store_max(&m_audio.level, peak);
            store_max(&m_audio.peak, peak);
        },
        on_error,
        None,
    )?;

    let m_writer = Arc::clone(&meters);
    let writer = std::thread::Builder::new()
        .name("woa-record-writer".into())
        .spawn(move || -> Result<(), String> {
            let mut last_flush = Instant::now();
            loop {
                let stopping = m_writer.stop.load(Ordering::Acquire);
                let mut n = 0u64;
                while let Ok(s) = consumer.pop() {
                    wav.write_sample(s).map_err(|e| e.to_string())?;
                    n += 1;
                }
                m_writer.written.fetch_add(n, Ordering::Release);
                if stopping {
                    break;
                }
                if last_flush.elapsed() >= FLUSH_EVERY {
                    wav.flush().map_err(|e| e.to_string())?;
                    last_flush = Instant::now();
                }
                if n == 0 {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            wav.finalize().map_err(|e| e.to_string())
        })
        .map_err(|e| RecordError::Wav(e.to_string()))?;

    stream.play()?;
    Ok(Recorder {
        stream: Some(stream),
        writer: Some(writer),
        meters,
        path: path.to_path_buf(),
        sample_rate,
        channels,
        started: Instant::now(),
    })
}

/// Nächster freier Name `aufnahme-NNN.wav` in `dir` (legt `dir` an).
pub fn next_take_path(dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    for n in 1..10_000u32 {
        let p = dir.join(format!("aufnahme-{n:03}.wav"));
        if !p.exists() {
            return Ok(p);
        }
    }
    Err(std::io::Error::other("no free take name"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_max_keeps_the_largest() {
        let c = AtomicU32::new(0);
        store_max(&c, 0.25);
        store_max(&c, 0.1);
        store_max(&c, 0.5);
        assert_eq!(f32::from_bits(c.load(Ordering::Relaxed)), 0.5);
    }

    #[test]
    fn take_names_count_up() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("aufnahmen");
        let a = next_take_path(&dir).unwrap();
        assert!(a.ends_with("aufnahme-001.wav"));
        std::fs::write(&a, b"x").unwrap();
        assert!(next_take_path(&dir).unwrap().ends_with("aufnahme-002.wav"));
    }
}
