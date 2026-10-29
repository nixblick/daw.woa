//! Geräteauswahl und Ausgabe (cpal), nur mit Feature `device`.
//!
//! Der Callback läuft im Audio-Thread: er rechnet Stereo in zwei vorab angelegte
//! Puffer und verteilt auf die Gerätekanäle. Keine Allokation, kein Logging.
//! Unter- und Überläufe (`ErrorKind::Xrun`) zählt der Fehlerpfad im
//! [`TransportHandle`], bevor er den Fehler an `on_error` weitergibt.

use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::{AudioProcessor, TransportHandle};

/// Größter Block, den der Callback in einem Stück rechnet.
const SCRATCH_FRAMES: usize = 4096;

/// Puffergröße, die WOA standardmäßig anfordert (Abnahmekriterium M1).
pub const DEFAULT_BUFFER_FRAMES: u32 = 128;

/// Fehler bei Geräteauswahl und Start.
#[derive(Debug)]
pub enum DeviceError {
    NoOutputDevice,
    DeviceNotFound(String),
    UnsupportedFormat,
    BufferSizeUnsupported { requested: u32, min: u32, max: u32 },
    Backend(cpal::Error),
}

impl std::fmt::Display for DeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoOutputDevice => write!(f, "no default output device"),
            Self::DeviceNotFound(id) => write!(f, "output device not found: {id}"),
            Self::UnsupportedFormat => write!(f, "output device does not offer f32"),
            Self::BufferSizeUnsupported {
                requested,
                min,
                max,
            } => write!(
                f,
                "buffer size {requested} not supported (device allows {min}..={max})"
            ),
            Self::Backend(e) => write!(f, "audio backend: {e}"),
        }
    }
}

impl std::error::Error for DeviceError {}

impl From<cpal::Error> for DeviceError {
    fn from(e: cpal::Error) -> Self {
        Self::Backend(e)
    }
}

/// Ein Ausgabegerät für die Auswahl in GUI oder Agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputDeviceInfo {
    /// Stabile Kennung, für [`OutputConfig::device_id`].
    pub id: String,
    /// Anzeigename.
    pub name: String,
    /// Standardgerät des Systems.
    pub is_default: bool,
}

/// Was beim Öffnen gewählt wird.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputConfig {
    /// `None` = Standardgerät.
    pub device_id: Option<String>,
    /// `None` = Vorgabe des Treibers; sonst feste Puffergröße in Frames.
    pub buffer_frames: Option<u32>,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            device_id: None,
            buffer_frames: Some(DEFAULT_BUFFER_FRAMES),
        }
    }
}

/// Listet alle Ausgabegeräte des Standard-Hosts.
/// Zuletzt vom Treiber gemeldete Ausgangslatenz (Callback bis Lautsprecher) in Mikrosekunden,
/// 0 wenn unbekannt. Für den Latenzausgleich bei Aufnahmen.
pub static OUTPUT_LATENCY_US: AtomicU64 = AtomicU64::new(0);

/// [`OUTPUT_LATENCY_US`] als Dauer.
pub fn output_latency() -> std::time::Duration {
    std::time::Duration::from_micros(OUTPUT_LATENCY_US.load(Ordering::Relaxed))
}

pub fn list_output_devices() -> Result<Vec<OutputDeviceInfo>, DeviceError> {
    let host = cpal::default_host();
    let default_id = host.default_output_device().and_then(|d| d.id().ok());
    let mut list = Vec::new();
    for device in host.output_devices()? {
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

/// Prüft eine gewünschte Puffergröße gegen den Bereich des Geräts.
/// Bei unbekanntem Bereich entscheidet der Treiber beim Öffnen.
pub fn check_buffer_size(
    requested: u32,
    supported: &cpal::SupportedBufferSize,
) -> Result<(), DeviceError> {
    match *supported {
        cpal::SupportedBufferSize::Range { min, max } if requested < min || requested > max => {
            Err(DeviceError::BufferSizeUnsupported {
                requested,
                min,
                max,
            })
        }
        _ => Ok(()),
    }
}

/// Zählt Xruns im Zähler; gibt `true` zurück, wenn es einer war.
pub fn count_if_xrun(err: &cpal::Error, transport: &TransportHandle) -> bool {
    let xrun = err.kind() == cpal::ErrorKind::Xrun;
    if xrun {
        transport.count_underrun();
    }
    xrun
}

/// Wählt das Gerät nach `config.device_id` (`None` = Standardgerät).
fn pick_device(config: &OutputConfig) -> Result<cpal::Device, DeviceError> {
    let host = cpal::default_host();
    match &config.device_id {
        None => host
            .default_output_device()
            .ok_or(DeviceError::NoOutputDevice),
        Some(id) => {
            let parsed = cpal::DeviceId::from_str(id)
                .map_err(|_| DeviceError::DeviceNotFound(id.clone()))?;
            host.device_by_id(&parsed)
                .ok_or_else(|| DeviceError::DeviceNotFound(id.clone()))
        }
    }
}

/// Abtastrate, mit der [`start_output_with`] das Gerät öffnen wird. Der Graph muss
/// mit genau dieser Rate kompiliert werden.
pub fn output_sample_rate(config: &OutputConfig) -> Result<u32, DeviceError> {
    let device = pick_device(config)?;
    let supported = device.default_output_config()?;
    Ok(supported.sample_rate())
}

/// Startet die Ausgabe auf dem Standardgerät mit Treiber-Puffergröße.
pub fn start_output(
    processor: AudioProcessor,
    transport: TransportHandle,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, DeviceError> {
    start_output_with(
        processor,
        transport,
        &OutputConfig {
            device_id: None,
            buffer_frames: None,
        },
        on_error,
    )
}

/// Startet die Ausgabe nach `config`. Der Stream läuft, solange der Rückgabewert lebt.
/// Fehler im Callback meldet `on_error` (außerhalb des Audio-Threads loggen!).
pub fn start_output_with(
    mut processor: AudioProcessor,
    transport: TransportHandle,
    config: &OutputConfig,
    mut on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, DeviceError> {
    let device = pick_device(config)?;
    let supported = device.default_output_config()?;
    if supported.sample_format() != cpal::SampleFormat::F32 {
        return Err(DeviceError::UnsupportedFormat);
    }
    let mut stream_config = supported.config();
    if let Some(frames) = config.buffer_frames {
        check_buffer_size(frames, supported.buffer_size())?;
        stream_config.buffer_size = cpal::BufferSize::Fixed(frames);
    }
    let channels = usize::from(stream_config.channels).max(1);
    let mut left = vec![0.0f32; SCRATCH_FRAMES];
    let mut right = vec![0.0f32; SCRATCH_FRAMES];

    let stream = device.build_output_stream(
        stream_config,
        move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
            let ts = info.timestamp();
            let d = ts.playback.duration_since(ts.callback);
            OUTPUT_LATENCY_US.store(d.as_micros() as u64, Ordering::Relaxed);
            for frame_chunk in data.chunks_mut(SCRATCH_FRAMES * channels) {
                let frames = frame_chunk.len() / channels;
                let l = &mut left[..frames];
                let r = &mut right[..frames];
                processor.process_stereo(l, r);
                for ((frame, &a), &b) in
                    frame_chunk.chunks_mut(channels).zip(l.iter()).zip(r.iter())
                {
                    if channels == 1 {
                        frame[0] = 0.5 * (a + b);
                    } else {
                        frame.fill(0.0);
                        frame[0] = a;
                        frame[1] = b;
                    }
                }
            }
        },
        move |err| {
            count_if_xrun(&err, &transport);
            on_error(err);
        },
        None,
    )?;
    stream.play()?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_size_range_is_checked() {
        let range = cpal::SupportedBufferSize::Range { min: 64, max: 4096 };
        assert!(check_buffer_size(128, &range).is_ok());
        assert!(matches!(
            check_buffer_size(32, &range),
            Err(DeviceError::BufferSizeUnsupported { requested: 32, .. })
        ));
        assert!(check_buffer_size(128, &cpal::SupportedBufferSize::Unknown).is_ok());
    }

    #[test]
    fn xrun_is_counted() {
        let (engine, _p) = crate::Engine::new(4);
        let t = engine.transport();
        assert!(count_if_xrun(&cpal::Error::new(cpal::ErrorKind::Xrun), &t));
        assert!(!count_if_xrun(
            &cpal::Error::new(cpal::ErrorKind::DeviceBusy),
            &t
        ));
        assert_eq!(engine.underruns(), 1);
    }
}
