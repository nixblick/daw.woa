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

pub use crate::RADIO_BUFFER_FRAMES;

/// Ein laufender Ausgabestrom; er spielt, solange er lebt.
pub type OutputStream = cpal::Stream;

/// Hält einen Strom an (das Gerät bleibt geöffnet, der Callback ruht).
pub fn pause_output(stream: &OutputStream) -> Result<(), DeviceError> {
    stream.pause().map_err(DeviceError::from)
}

/// Lässt einen angehaltenen Strom weiterlaufen.
pub fn resume_output(stream: &OutputStream) -> Result<(), DeviceError> {
    stream.play().map_err(DeviceError::from)
}

/// Anzeigename des Geräts, das `config` wählt (für Logs), `"?"` wenn unbekannt.
pub fn output_device_name(config: &OutputConfig) -> String {
    pick_device(config)
        .ok()
        .and_then(|d| d.description().ok().map(|x| x.name().to_string()))
        .unwrap_or_else(|| "?".into())
}

/// Ist der Fehler ein Unter- oder Überlauf (Xrun)?
pub fn is_xrun(err: &cpal::Error) -> bool {
    err.kind() == cpal::ErrorKind::Xrun
}

/// Fehler bei Geräteauswahl und Start.
#[derive(Debug)]
pub enum DeviceError {
    NoOutputDevice,
    DeviceNotFound(String),
    UnsupportedFormat,
    BufferSizeUnsupported {
        requested: u32,
        min: u32,
        max: u32,
    },
    Backend(cpal::Error),
    /// Kanal 3/4 gewünscht, das Gerät hat weniger Kanäle.
    TooFewChannels(u16),
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
            Self::TooFewChannels(n) => {
                write!(f, "device has {n} channels, channels 3/4 need at least 4")
            }
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
    let xrun = is_xrun(err);
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
    start_stereo_output(
        config,
        move |l, r| processor.process_stereo(l, r),
        move |err| {
            count_if_xrun(&err, &transport);
            on_error(err);
        },
    )
}

/// Startet die Ausgabe mit einem eigenen Stereo-Callback `render(links, rechts)`, der im
/// Audio-Thread läuft (keine Allokation, kein Lock, kein I/O). Die Rate liefert
/// [`output_sample_rate`] mit derselben `config`.
pub fn start_stereo_output(
    config: &OutputConfig,
    render: impl FnMut(&mut [f32], &mut [f32]) + Send + 'static,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, DeviceError> {
    start_output_aux(config, render, None, on_error)
}

/// Stereo-Callback für Kanal 3/4 (Vorhören, Paket 15).
pub type AuxRender = Box<dyn FnMut(&mut [f32], &mut [f32]) + Send>;

/// Kanäle, mit denen [`start_output_aux`] das Gerät öffnet.
pub fn output_channels(config: &OutputConfig) -> Result<u16, DeviceError> {
    let device = pick_device(config)?;
    Ok(device.default_output_config()?.channels())
}

/// Schreibt einen Block in verschachtelte Geräte-Frames: `main` auf Kanal 1/2 (bei Mono
/// gemittelt), `aux` auf Kanal 3/4, alle weiteren Kanäle still. `aux` erreicht nie
/// Kanal 1/2.
pub fn interleave(
    out: &mut [f32],
    channels: usize,
    main: (&[f32], &[f32]),
    aux: Option<(&[f32], &[f32])>,
) {
    for (i, frame) in out.chunks_mut(channels.max(1)).enumerate() {
        let (a, b) = (
            main.0.get(i).copied().unwrap_or(0.0),
            main.1.get(i).copied().unwrap_or(0.0),
        );
        if frame.len() == 1 {
            frame[0] = 0.5 * (a + b);
            continue;
        }
        frame.fill(0.0);
        frame[0] = a;
        frame[1] = b;
        if let Some((l, r)) = aux
            && frame.len() >= 4
        {
            frame[2] = l.get(i).copied().unwrap_or(0.0);
            frame[3] = r.get(i).copied().unwrap_or(0.0);
        }
    }
}

/// Wie [`start_stereo_output`], dazu optional `aux` auf Kanal 3/4 desselben Geräts.
/// Mit `aux` muss das Gerät mindestens vier Kanäle haben.
pub fn start_output_aux(
    config: &OutputConfig,
    mut render: impl FnMut(&mut [f32], &mut [f32]) + Send + 'static,
    mut aux: Option<AuxRender>,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
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
    if aux.is_some() && channels < 4 {
        return Err(DeviceError::TooFewChannels(stream_config.channels));
    }
    let mut left = vec![0.0f32; SCRATCH_FRAMES];
    let mut right = vec![0.0f32; SCRATCH_FRAMES];
    let mut aux_l = vec![0.0f32; SCRATCH_FRAMES];
    let mut aux_r = vec![0.0f32; SCRATCH_FRAMES];

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
                render(l, r);
                let side = match aux.as_mut() {
                    Some(f) => {
                        let (al, ar) = (&mut aux_l[..frames], &mut aux_r[..frames]);
                        f(al, ar);
                        Some((&al[..], &ar[..]))
                    }
                    None => None,
                };
                interleave(frame_chunk, channels, (l, r), side);
            }
        },
        on_error,
        None,
    )?;
    stream.play()?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aux_goes_to_channels_three_and_four_only() {
        let main = ([0.5f32, 0.25], [-0.5f32, -0.25]);
        let aux = ([0.9f32, 0.8], [0.7f32, 0.6]);
        let mut out = [1.0f32; 12];
        interleave(&mut out, 6, (&main.0, &main.1), Some((&aux.0, &aux.1)));
        assert_eq!(
            out,
            [
                0.5, -0.5, 0.9, 0.7, 0.0, 0.0, 0.25, -0.25, 0.8, 0.6, 0.0, 0.0
            ]
        );
        let mut st = [1.0f32; 4];
        interleave(&mut st, 2, (&main.0, &main.1), Some((&aux.0, &aux.1)));
        assert_eq!(st, [0.5, -0.5, 0.25, -0.25]);
    }

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
