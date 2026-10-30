//! Geräteauswahl und Ausgabe (cpal), nur mit Feature `device`.
//!
//! Der Callback läuft im Audio-Thread: er rechnet Stereo in zwei vorab angelegte
//! Puffer und verteilt auf die Gerätekanäle. Keine Allokation, kein Logging.
//! Unter- und Überläufe (`ErrorKind::Xrun`) zählt der Fehlerpfad im
//! [`TransportHandle`], bevor er den Fehler an `on_error` weitergibt.

use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::pairs::{PairBuf, interleave_pairs};
use crate::{AudioProcessor, TransportHandle};

/// Größter Block, den der Callback in einem Stück rechnet.
const SCRATCH_FRAMES: usize = 4096;

/// Puffergröße, die WOA standardmäßig anfordert (Abnahmekriterium M1).
pub const DEFAULT_BUFFER_FRAMES: u32 = 128;

pub use crate::RADIO_BUFFER_FRAMES;

/// Führt einen Gerätezugriff aus. Unter Windows auf einem frischen Thread: Das Fenster (winit,
/// Drag & Drop) richtet COM im GUI-Thread als STA ein, WASAPI braucht MTA und scheitert sonst
/// mit „Der Threadmodus kann nicht nach dem Einstellen geändert werden“ (RPC_E_CHANGED_MODE,
/// André 30.10.: keine Wiedergabe, Mikrofon startet nicht). Anderswo direkt.
#[cfg(windows)]
pub fn on_audio_thread<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| match s.spawn(f).join() {
        Ok(v) => v,
        Err(p) => std::panic::resume_unwind(p),
    })
}

/// Siehe die Windows-Fassung; hier gibt es kein COM, der Zugriff läuft direkt.
#[cfg(not(windows))]
pub fn on_audio_thread<T>(f: impl FnOnce() -> T) -> T {
    f()
}

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
    crate::device::on_audio_thread(move || -> String {
        pick_device(config)
            .ok()
            .and_then(|d| d.description().ok().map(|x| x.name().to_string()))
            .unwrap_or_else(|| "?".into())
    })
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
    /// Das neue Gerät läuft mit einer anderen Abtastrate als der Graph (Neuverbinden unmöglich).
    RateChanged {
        old: u32,
        new: u32,
    },
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
                write!(
                    f,
                    "device has only {n} channels, too few for the chosen channel pairs"
                )
            }
            Self::RateChanged { old, new } => {
                write!(f, "device rate changed from {old} to {new} Hz")
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
    crate::device::on_audio_thread(move || -> Result<Vec<OutputDeviceInfo>, DeviceError> {
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
    })
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
    crate::device::on_audio_thread(move || -> Result<u32, DeviceError> {
        let device = pick_device(config)?;
        let supported = device.default_output_config()?;
        Ok(supported.sample_rate())
    })
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
    crate::device::on_audio_thread(move || -> Result<u16, DeviceError> {
        let device = pick_device(config)?;
        Ok(device.default_output_config()?.channels())
    })
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
    crate::device::on_audio_thread(move || -> Result<cpal::Stream, DeviceError> {
        let device = pick_device(config)?;
        let supported = device.default_output_config()?;
        if supported.sample_format() != cpal::SampleFormat::F32 {
            return Err(DeviceError::UnsupportedFormat);
        }
        let mut stream_config = supported.config();
        // Windows (WASAPI, geteilter Modus): ein fester Puffer unter der Geräteperiode (~10 ms,
        // 128 Frames sind 2,7 ms) lässt das Öffnen scheitern, dann bleibt ▶ grau. Dort entscheidet
        // der Treiber (André 30.10., Windows-Notebook ohne Wiedergabe).
        if let Some(frames) = config.buffer_frames.filter(|_| !cfg!(windows)) {
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
    })
}

/// Was ein Ausgabegerät beim Öffnen anbietet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputProbe {
    /// Stabile Kennung des Geräts (auch beim Standardgerät die echte).
    pub id: String,
    /// Anzeigename.
    pub name: String,
    /// Abtastrate, mit der [`start_pairs_output`] öffnet.
    pub rate: u32,
    /// Kanäle, mit denen es öffnet.
    pub channels: u16,
}

/// Prüft ein Gerät, ohne es zu öffnen: Kennung, Name, Rate, Kanäle. Fehlt es, ein Fehler.
pub fn probe_output(config: &OutputConfig) -> Result<OutputProbe, DeviceError> {
    crate::device::on_audio_thread(move || -> Result<OutputProbe, DeviceError> {
        let device = pick_device(config)?;
        let supported = device.default_output_config()?;
        let id = device.id().map(|i| i.to_string()).unwrap_or_default();
        let name = device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_else(|_| id.clone());
        Ok(OutputProbe {
            id,
            name,
            rate: supported.sample_rate(),
            channels: supported.channels(),
        })
    })
}

/// Callback für [`start_pairs_output`]: bekommt die Paar-Puffer (schon auf Stille) und die
/// Zahl der Frames dieses Blocks und füllt sie. Läuft im Audio-Thread.
pub type PairsRender = Box<dyn FnMut(&mut [PairBuf], usize) + Send>;

/// Öffnet ein Gerät mit `pairs` Stereo-Paaren (Paar 0 = Kanal 1/2, Paar 1 = Kanal 3/4 …).
/// Jedes Gerät bekommt seinen eigenen Strom; mehrere Geräte laufen gleichzeitig, jedes mit
/// eigenem Takt (Ausgleich: [`crate::pairs::link`]). `master`: Dieses Gerät meldet seine
/// Latenz für den Latenzausgleich der Aufnahme. Hat das Gerät weniger als `2 * pairs`
/// Kanäle (bei `pairs == 1` genügt ein Kanal), kommt [`DeviceError::TooFewChannels`].
/// Unter Windows keine feste Puffergröße (WASAPI geteilt), siehe [`start_output_aux`].
pub fn start_pairs_output(
    config: &OutputConfig,
    pairs: usize,
    master: bool,
    mut render: PairsRender,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, DeviceError> {
    let pairs = pairs.max(1);
    crate::device::on_audio_thread(move || -> Result<cpal::Stream, DeviceError> {
        let device = pick_device(config)?;
        let supported = device.default_output_config()?;
        if supported.sample_format() != cpal::SampleFormat::F32 {
            return Err(DeviceError::UnsupportedFormat);
        }
        let mut stream_config = supported.config();
        if let Some(frames) = config.buffer_frames.filter(|_| !cfg!(windows)) {
            check_buffer_size(frames, supported.buffer_size())?;
            stream_config.buffer_size = cpal::BufferSize::Fixed(frames);
        }
        let channels = usize::from(stream_config.channels).max(1);
        if pairs > 1 && channels < 2 * pairs {
            return Err(DeviceError::TooFewChannels(stream_config.channels));
        }
        let mut bufs: Vec<PairBuf> = (0..pairs).map(|_| PairBuf::new(SCRATCH_FRAMES)).collect();
        let stream = device.build_output_stream(
            stream_config,
            move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
                if master {
                    let ts = info.timestamp();
                    let d = ts.playback.duration_since(ts.callback);
                    OUTPUT_LATENCY_US.store(d.as_micros() as u64, Ordering::Relaxed);
                }
                for chunk in data.chunks_mut(SCRATCH_FRAMES * channels) {
                    let frames = chunk.len() / channels;
                    for b in bufs.iter_mut() {
                        b.clear(frames);
                    }
                    render(&mut bufs, frames);
                    interleave_pairs(chunk, channels, &bufs, frames);
                }
            },
            on_error,
            None,
        )?;
        stream.play()?;
        Ok(stream)
    })
}

/// Fehlerklasse eines Gerätefehlers für die Telemetrie: nur ein fester Name, nie Freitext und
/// nie ein Gerätename.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    /// Der Strom ist ungültig geworden (Windows: Standardgerät gewechselt, Gerät gezogen).
    DeviceInvalidated,
    /// Das Gerät gibt es nicht mehr oder nicht mehr als Standard.
    DeviceNotAvailable,
    /// Das Gerät ist belegt.
    DeviceBusy,
    /// Format, Kanäle oder Rate passen nicht.
    Config,
    /// Rückmeldung des Treibers, die cpal nicht einordnen kann.
    Backend,
    /// Alles andere.
    Other,
}

impl ErrorClass {
    /// Einordnung eines cpal-Fehlers.
    pub fn of(err: &cpal::Error) -> Self {
        use cpal::ErrorKind as K;
        match err.kind() {
            K::StreamInvalidated => Self::DeviceInvalidated,
            K::DeviceNotAvailable => Self::DeviceNotAvailable,
            K::DeviceBusy => Self::DeviceBusy,
            K::UnsupportedConfig | K::InvalidInput | K::UnsupportedOperation => Self::Config,
            K::BackendError | K::HostUnavailable => Self::Backend,
            _ => Self::Other,
        }
    }

    /// Einordnung eines Fehlers beim Öffnen.
    pub fn of_device_error(err: &DeviceError) -> Self {
        match err {
            DeviceError::NoOutputDevice | DeviceError::DeviceNotFound(_) => {
                Self::DeviceNotAvailable
            }
            DeviceError::Backend(e) => Self::of(e),
            DeviceError::UnsupportedFormat
            | DeviceError::BufferSizeUnsupported { .. }
            | DeviceError::TooFewChannels(_)
            | DeviceError::RateChanged { .. } => Self::Config,
        }
    }

    /// Bezeichner für `art` im Telemetrie-Ereignis (`audio.output.<klasse>`).
    pub fn art(self) -> &'static str {
        match self {
            Self::DeviceInvalidated => "audio.output.device_invalidated",
            Self::DeviceNotAvailable => "audio.output.device_not_available",
            Self::DeviceBusy => "audio.output.device_busy",
            Self::Config => "audio.output.config",
            Self::Backend => "audio.output.backend",
            Self::Other => "audio.output.other",
        }
    }

    fn code(self) -> u8 {
        self as u8 + 1
    }

    fn from_code(c: u8) -> Option<Self> {
        [
            Self::DeviceInvalidated,
            Self::DeviceNotAvailable,
            Self::DeviceBusy,
            Self::Config,
            Self::Backend,
            Self::Other,
        ]
        .into_iter()
        .find(|k| k.code() == c)
    }
}

/// Muss der Strom nach diesem Fehler neu geöffnet werden? Aussetzer, eine selbst nachgeführte
/// Umleitung und die abgelehnte Echtzeit-Priorität lassen ihn laufen.
pub fn needs_reopen(err: &cpal::Error) -> bool {
    use cpal::ErrorKind as K;
    !matches!(
        err.kind(),
        K::Xrun | K::DeviceChanged | K::RealtimeDenied | K::PermissionDenied
    )
}

/// Vom Fehlerpfad gesetzt (darf im Audio-Thread laufen: nur Atomics), vom GUI-Takt gelesen.
#[derive(Default)]
struct Health {
    dead: AtomicBool,
    class: AtomicU8,
}

impl Health {
    fn report(&self, err: &cpal::Error) {
        if needs_reopen(err) {
            self.class
                .store(ErrorClass::of(err).code(), Ordering::Relaxed);
            self.dead.store(true, Ordering::Release);
        }
    }

    fn take(&self) -> Option<ErrorClass> {
        if self.dead.swap(false, Ordering::AcqRel) {
            Some(
                ErrorClass::from_code(self.class.load(Ordering::Relaxed))
                    .unwrap_or(ErrorClass::Other),
            )
        } else {
            None
        }
    }
}

/// Wartezeiten zwischen Öffnungsversuchen: 1 s, 2 s, 4 s … höchstens 16 s. Hielt der letzte
/// Strom länger als 30 s, beginnt es wieder bei 1 s. Reine Zeitrechnung, ohne Gerät.
#[derive(Debug)]
struct Backoff {
    wait: Duration,
    not_before: Option<Instant>,
}

impl Backoff {
    const MIN: Duration = Duration::from_secs(1);
    const MAX: Duration = Duration::from_secs(16);
    const STABLE: Duration = Duration::from_secs(30);

    fn new() -> Self {
        Self {
            wait: Self::MIN,
            not_before: None,
        }
    }

    fn ready(&self, now: Instant) -> bool {
        self.not_before.is_none_or(|t| now >= t)
    }

    /// Ein Versuch ist fehlgeschlagen oder ein laufender Strom ist ausgefallen, nachdem er
    /// `lived` lang lief.
    fn failed(&mut self, now: Instant, lived: Duration) {
        if lived > Self::STABLE {
            self.wait = Self::MIN;
        } else {
            self.wait = (self.wait * 2).min(Self::MAX);
        }
        self.not_before = Some(now + self.wait);
    }
}

type ErrorSink = Box<dyn FnMut(cpal::Error) + Send>;

/// Was ein Takt von [`ManagedOutput::poll`] getan hat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reconnect {
    /// Nichts nötig.
    Idle,
    /// Der Strom wurde neu geöffnet.
    Reopened,
    /// Der Versuch schlug fehl; der nächste folgt später.
    Failed(ErrorClass),
}

/// Ausgabestrom, der sich selbst neu öffnet. Die Render-Funktionen liegen hinter einem Mutex
/// außerhalb des Stroms und überleben ihn; der Audio-Thread nimmt sie nur mit `try_lock`
/// (kein Warten, bei Streit Stille). Neu geöffnet wird im Aufrufer von [`poll`](Self::poll),
/// also im GUI-Takt, nie im Audio-Thread.
///
/// Auslöser: ein Fehler im Strom (Windows: `StreamInvalidated` beim Umschalten des
/// Standardgeräts, `DeviceNotAvailable` beim Ziehen) oder, bei `device_id: None`, ein anderes
/// Standardgerät als beim Öffnen.
pub struct ManagedOutput {
    label: &'static str,
    config: OutputConfig,
    rate: u32,
    pairs: usize,
    master: bool,
    render: Arc<Mutex<PairsRender>>,
    sink: Arc<Mutex<ErrorSink>>,
    health: Arc<Health>,
    stream: Option<cpal::Stream>,
    paused: bool,
    opened_at: Instant,
    backoff: Backoff,
    default_id: Option<String>,
    default_checked: Instant,
    reopens: u32,
}

/// So oft fragt [`ManagedOutput::poll`] nach dem Standardgerät.
const DEFAULT_CHECK_EVERY: Duration = Duration::from_secs(2);

fn default_output_id() -> Option<String> {
    on_audio_thread(|| {
        cpal::default_host()
            .default_output_device()
            .and_then(|d| d.id().ok())
            .map(|i| i.to_string())
    })
}

impl ManagedOutput {
    /// Öffnet den Strom. `label` steht im Log (`radio`, `pfl`, `player`). `on_error` bekommt
    /// jeden Fehler des Stroms (auch Aussetzer) und darf im Audio-Thread laufen.
    pub fn open(
        label: &'static str,
        config: &OutputConfig,
        pairs: usize,
        master: bool,
        render: PairsRender,
        on_error: impl FnMut(cpal::Error) + Send + 'static,
    ) -> Result<Self, DeviceError> {
        let rate = probe_output(config)?.rate;
        let mut me = Self {
            label,
            config: config.clone(),
            rate,
            pairs,
            master,
            render: Arc::new(Mutex::new(render)),
            sink: Arc::new(Mutex::new(Box::new(on_error))),
            health: Arc::new(Health::default()),
            stream: None,
            paused: false,
            opened_at: Instant::now(),
            backoff: Backoff::new(),
            default_id: None,
            default_checked: Instant::now(),
            reopens: 0,
        };
        me.stream = Some(me.build()?);
        me.opened_at = Instant::now();
        if me.config.device_id.is_none() {
            me.default_id = default_output_id();
        }
        Ok(me)
    }

    /// Baut einen neuen Strom auf die gemeinsamen Render-Funktionen.
    fn build(&mut self) -> Result<cpal::Stream, DeviceError> {
        let now_rate = probe_output(&self.config)?.rate;
        if now_rate != self.rate {
            return Err(DeviceError::RateChanged {
                old: self.rate,
                new: now_rate,
            });
        }
        let health = Arc::new(Health::default());
        self.health = Arc::clone(&health);
        let shared = Arc::clone(&self.render);
        // Ein Aufruf des Audio-Threads nimmt die Sperre nur mit `try_lock`: bei Streit Stille
        // (die Puffer sind schon geleert), nie Warten.
        let render: PairsRender = Box::new(move |bufs, frames| {
            if let Ok(mut f) = shared.try_lock() {
                f(bufs, frames);
            }
        });
        let sink = Arc::clone(&self.sink);
        let on_error = move |e: cpal::Error| {
            health.report(&e);
            if let Ok(mut f) = sink.try_lock() {
                f(e);
            }
        };
        let stream = start_pairs_output(&self.config, self.pairs, self.master, render, on_error)?;
        if self.paused {
            stream.pause()?;
        }
        Ok(stream)
    }

    /// Abtastrate des Stroms (bleibt über das Neuöffnen gleich).
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Wie oft der Strom neu geöffnet wurde.
    pub fn reopens(&self) -> u32 {
        self.reopens
    }

    /// Läuft gerade ein Strom?
    pub fn is_open(&self) -> bool {
        self.stream.is_some()
    }

    /// Hält den Strom an (er bleibt dabei erhalten und überlebt ein Neuöffnen als angehalten).
    pub fn pause(&mut self) -> Result<(), DeviceError> {
        self.paused = true;
        match &self.stream {
            Some(s) => pause_output(s),
            None => Ok(()),
        }
    }

    /// Lässt den Strom weiterlaufen.
    pub fn resume(&mut self) -> Result<(), DeviceError> {
        self.paused = false;
        match &self.stream {
            Some(s) => resume_output(s),
            None => Ok(()),
        }
    }

    /// Rettungsknopf: sofort neu öffnen, ohne Wartezeit.
    pub fn reconnect_now(&mut self) -> Reconnect {
        self.backoff = Backoff::new();
        self.reopen(Instant::now(), None)
    }

    /// Ein GUI-Takt: prüft Fehler und Standardgerät und öffnet bei Bedarf neu. Loggt eine Zeile
    /// mit `art = audio.output.<klasse>` (nur die Klasse, kein Freitext, kein Gerätename).
    pub fn poll(&mut self) -> Reconnect {
        let now = Instant::now();
        let mut cause = self.health.take();
        if cause.is_none()
            && self.stream.is_some()
            && self.config.device_id.is_none()
            && now.duration_since(self.default_checked) >= DEFAULT_CHECK_EVERY
        {
            self.default_checked = now;
            let id = default_output_id();
            if id.is_some() && id != self.default_id {
                cause = Some(ErrorClass::DeviceInvalidated);
            }
        }
        if cause.is_none() && self.stream.is_some() {
            return Reconnect::Idle;
        }
        if self.stream.is_some() {
            // Frisch ausgefallen: Wartezeit nach der Lebensdauer des Stroms bemessen.
            let lived = now.duration_since(self.opened_at);
            self.backoff.failed(now, lived);
            self.backoff.not_before = Some(now);
            tracing::warn!(
                art = cause.unwrap_or(ErrorClass::Other).art(),
                output = self.label,
                "output stream lost, reopening"
            );
        }
        if !self.backoff.ready(now) {
            return Reconnect::Idle;
        }
        self.reopen(now, cause)
    }

    fn reopen(&mut self, now: Instant, cause: Option<ErrorClass>) -> Reconnect {
        // Erst den alten Strom freigeben, dann den neuen öffnen.
        self.stream = None;
        match self.build() {
            Ok(s) => {
                self.stream = Some(s);
                self.opened_at = Instant::now();
                self.backoff.not_before = None;
                self.reopens += 1;
                if self.config.device_id.is_none() {
                    self.default_id = default_output_id();
                }
                tracing::info!(
                    output = self.label,
                    reopens = self.reopens,
                    "output reopened"
                );
                Reconnect::Reopened
            }
            Err(e) => {
                let class = ErrorClass::of_device_error(&e);
                self.backoff.failed(now, Duration::ZERO);
                tracing::warn!(
                    art = class.art(),
                    output = self.label,
                    error = %e,
                    cause = ?cause,
                    "output reopen failed"
                );
                Reconnect::Failed(class)
            }
        }
    }
}

#[cfg(test)]
mod reconnect_tests {
    use super::*;

    fn err(kind: cpal::ErrorKind) -> cpal::Error {
        cpal::Error::new(kind)
    }

    #[test]
    fn device_errors_get_a_class_without_text() {
        assert_eq!(
            ErrorClass::of(&err(cpal::ErrorKind::StreamInvalidated)),
            ErrorClass::DeviceInvalidated
        );
        assert_eq!(
            ErrorClass::of(&err(cpal::ErrorKind::DeviceNotAvailable)),
            ErrorClass::DeviceNotAvailable
        );
        assert_eq!(
            ErrorClass::of(&err(cpal::ErrorKind::BackendError)),
            ErrorClass::Backend
        );
        assert_eq!(
            ErrorClass::of_device_error(&DeviceError::DeviceNotFound("secret-name".into())),
            ErrorClass::DeviceNotAvailable
        );
        for c in [
            ErrorClass::DeviceInvalidated,
            ErrorClass::DeviceNotAvailable,
            ErrorClass::DeviceBusy,
            ErrorClass::Config,
            ErrorClass::Backend,
            ErrorClass::Other,
        ] {
            assert!(c.art().starts_with("audio.output."));
            assert!(
                c.art()
                    .bytes()
                    .all(|b| matches!(b, b'a'..=b'z' | b'_' | b'.'))
            );
            assert_eq!(ErrorClass::from_code(c.code()), Some(c));
        }
    }

    #[test]
    fn simulated_device_error_marks_stream_dead_once() {
        let h = Health::default();
        h.report(&err(cpal::ErrorKind::Xrun));
        h.report(&err(cpal::ErrorKind::DeviceChanged));
        assert_eq!(
            h.take(),
            None,
            "Aussetzer und Umleitung lassen den Strom laufen"
        );
        h.report(&err(cpal::ErrorKind::StreamInvalidated));
        assert_eq!(h.take(), Some(ErrorClass::DeviceInvalidated));
        assert_eq!(h.take(), None, "ein Fehler löst genau ein Neuöffnen aus");
    }

    #[test]
    fn backoff_doubles_caps_and_resets_after_stable_run() {
        let t0 = Instant::now();
        let mut b = Backoff::new();
        assert!(b.ready(t0));
        b.failed(t0, Duration::ZERO);
        assert!(!b.ready(t0 + Duration::from_millis(1500)));
        assert!(b.ready(t0 + Duration::from_secs(2)));
        for _ in 0..10 {
            b.failed(t0, Duration::ZERO);
        }
        assert_eq!(b.wait, Backoff::MAX);
        b.failed(t0, Duration::from_secs(60));
        assert_eq!(b.wait, Backoff::MIN);
    }

    #[test]
    fn render_state_survives_for_the_next_stream() {
        // Dieselbe Hülle wie in `build`: zwei Ströme nacheinander auf einer Render-Funktion.
        let mut n = 0.0f32;
        let shared: Arc<Mutex<PairsRender>> =
            Arc::new(Mutex::new(Box::new(move |bufs, frames| {
                n += 1.0;
                bufs[0].l[..frames].fill(n);
            })));
        let wrap = |m: Arc<Mutex<PairsRender>>| -> PairsRender {
            Box::new(move |bufs, frames| {
                if let Ok(mut f) = m.try_lock() {
                    f(bufs, frames);
                }
            })
        };
        let mut bufs = vec![PairBuf::new(4)];
        let mut first = wrap(Arc::clone(&shared));
        bufs[0].clear(2);
        first(&mut bufs, 2);
        drop(first);
        let mut second = wrap(Arc::clone(&shared));
        bufs[0].clear(2);
        second(&mut bufs, 2);
        assert_eq!(bufs[0].l[..2], [2.0, 2.0]);
        // Hält jemand die Sperre, bleibt der Puffer still, der Callback wartet nicht.
        let _g = shared.lock().unwrap();
        bufs[0].clear(2);
        second(&mut bufs, 2);
        assert_eq!(bufs[0].l[..2], [0.0, 0.0]);
    }
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
