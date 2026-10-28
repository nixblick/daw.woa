//! Ausgabe auf das Standard-Audiogerät (cpal), nur mit Feature `device`.
//!
//! Der Callback läuft im Audio-Thread: er rechnet mono in einen vorab angelegten
//! Puffer und verteilt auf alle Kanäle. Keine Allokation, kein Logging.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::AudioProcessor;

/// Größter Block, den der Callback in einem Stück rechnet.
const SCRATCH_FRAMES: usize = 4096;

/// Fehler beim Öffnen des Geräts.
#[derive(Debug)]
pub enum DeviceError {
    NoOutputDevice,
    UnsupportedFormat,
    Backend(cpal::Error),
}

impl std::fmt::Display for DeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoOutputDevice => write!(f, "no default output device"),
            Self::UnsupportedFormat => write!(f, "default output is not f32"),
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

/// Startet die Ausgabe. Der Stream läuft, solange der Rückgabewert lebt.
/// Fehler im Callback meldet `on_error` (außerhalb des Audio-Threads loggen!).
pub fn start_output(
    mut processor: AudioProcessor,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, DeviceError> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or(DeviceError::NoOutputDevice)?;
    let supported = device.default_output_config()?;
    if supported.sample_format() != cpal::SampleFormat::F32 {
        return Err(DeviceError::UnsupportedFormat);
    }
    let config = supported.config();
    let channels = usize::from(config.channels).max(1);
    let mut scratch = vec![0.0f32; SCRATCH_FRAMES];

    let stream = device.build_output_stream(
        config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            for frame_chunk in data.chunks_mut(SCRATCH_FRAMES * channels) {
                let frames = frame_chunk.len() / channels;
                let mono = &mut scratch[..frames];
                processor.process(mono);
                for (frame, &s) in frame_chunk.chunks_mut(channels).zip(mono.iter()) {
                    frame.fill(s);
                }
            }
        },
        on_error,
        None,
    )?;
    stream.play()?;
    Ok(stream)
}
