//! Aufnahme aus PipeWire/PulseAudio (nur Linux, Feature `device`): Eingänge, Systemklang
//! (der Monitor eines Ausgangs) und einzelne Programme.
//!
//! Spricht das Pulse-Protokoll direkt über den Socket (`pulseaudio`-Crate, reines Rust);
//! PipeWire stellt ihn über pipewire-pulse bereit. Der Server liefert immer 48 kHz Stereo
//! als 32-bit-Float, er wandelt selbst um. Gelesen wird in einem eigenen Thread, der die
//! WAV-Datei fortlaufend schreibt; das ist kein Echtzeit-Callback, der Server puffert.

use std::ffi::{CStr, CString};
use std::io::{BufReader, Read};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use pulseaudio::protocol::{self, Command, Prop};

use crate::record::RecordStats;

/// Abtastrate aller Aufnahmen über Pulse.
pub const RATE: u32 = 48_000;
/// Kanäle aller Aufnahmen über Pulse.
pub const CHANNELS: u16 = 2;
/// Bytes je Frame (Stereo, f32).
const FRAME_BYTES: u32 = 8;
/// Wie oft der Kopf der WAV-Datei aktualisiert wird.
const FLUSH_EVERY: Duration = Duration::from_millis(500);

/// Art einer Quelle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceKind {
    /// Mikrofon, Interface, Line-In.
    Input,
    /// Alles, was über einen Ausgang abgespielt wird.
    System,
    /// Ein einzelnes Programm (nur solange es Ton abspielt).
    App,
}

/// Eine aufnehmbare Quelle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureSource {
    pub kind: SourceKind,
    /// Anzeigename.
    pub label: String,
    /// Pulse-Name der Quelle (bei Programmen: Monitor ihres Ausgangs).
    pub source: String,
    /// Nur bei Programmen: Index des Wiedergabe-Streams.
    pub app_input: Option<u32>,
    /// Bleibender Schlüssel, um die Wahl einer Spur wiederzufinden.
    pub key: String,
}

/// Fehler beim Gespräch mit dem Audio-Server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureError(pub String);

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CaptureError {}

fn err(e: impl std::fmt::Display) -> CaptureError {
    CaptureError(e.to_string())
}

type Conn = (BufReader<UnixStream>, u16);

fn connect(client: &str) -> Result<Conn, CaptureError> {
    let path = pulseaudio::socket_path_from_env()
        .ok_or_else(|| err("kein PipeWire/PulseAudio gefunden"))?;
    let mut sock = BufReader::new(UnixStream::connect(path).map_err(err)?);
    let cookie = pulseaudio::cookie_path_from_env()
        .and_then(|p| std::fs::read(p).ok())
        .unwrap_or_default();
    let auth = protocol::AuthParams {
        version: protocol::MAX_VERSION,
        supports_shm: false,
        supports_memfd: false,
        cookie,
    };
    protocol::write_command_message(
        sock.get_mut(),
        0,
        &Command::Auth(auth),
        protocol::MAX_VERSION,
    )
    .map_err(err)?;
    let (_, reply) =
        protocol::read_reply_message::<protocol::AuthReply>(&mut sock, protocol::MAX_VERSION)
            .map_err(err)?;
    let version = protocol::MAX_VERSION.min(reply.version);
    let mut props = protocol::Props::new();
    props.set(Prop::ApplicationName, CString::new(client).map_err(err)?);
    protocol::write_command_message(sock.get_mut(), 1, &Command::SetClientName(props), version)
        .map_err(err)?;
    protocol::read_reply_message::<protocol::SetClientNameReply>(&mut sock, version)
        .map_err(err)?;
    Ok((sock, version))
}

fn text(c: &CStr) -> String {
    c.to_string_lossy().into_owned()
}

fn prop(props: &protocol::Props, p: Prop) -> Option<String> {
    let raw = props.get(p)?;
    let raw = raw.strip_suffix(&[0]).unwrap_or(raw);
    Some(String::from_utf8_lossy(raw).into_owned()).filter(|s| !s.is_empty())
}

fn ask<T: protocol::CommandReply>(
    conn: &mut Conn,
    seq: u32,
    cmd: Command,
) -> Result<T, CaptureError> {
    protocol::write_command_message(conn.0.get_mut(), seq, &cmd, conn.1).map_err(err)?;
    protocol::read_reply_message::<T>(&mut conn.0, conn.1)
        .map(|(_, r)| r)
        .map_err(err)
}

/// Listet Eingänge, Systemklang je Ausgang und die Programme, die gerade Ton abspielen.
/// Eigene Streams von WOA tauchen nicht auf.
pub fn list_sources() -> Result<Vec<CaptureSource>, CaptureError> {
    let mut conn = connect("WOA")?;
    let sources: protocol::SourceInfoList = ask(&mut conn, 2, Command::GetSourceInfoList)?;
    let sinks: protocol::SinkInfoList = ask(&mut conn, 3, Command::GetSinkInfoList)?;
    let inputs: protocol::SinkInputInfoList = ask(&mut conn, 4, Command::GetSinkInputInfoList)?;
    let own_pid = std::process::id().to_string();

    let mut out = Vec::new();
    for s in sources.iter().filter(|s| s.monitor_of_sink_index.is_none()) {
        let name = text(&s.name);
        out.push(CaptureSource {
            kind: SourceKind::Input,
            label: s.description.as_deref().map_or_else(|| name.clone(), text),
            key: format!("in:{name}"),
            source: name,
            app_input: None,
        });
    }
    for s in sources.iter().filter(|s| s.monitor_of_sink_index.is_some()) {
        let name = text(&s.name);
        let sink = sinks
            .iter()
            .find(|k| Some(k.index) == s.monitor_of_sink_index)
            .and_then(|k| k.description.as_deref().map(text));
        out.push(CaptureSource {
            kind: SourceKind::System,
            label: format!("Systemklang: {}", sink.unwrap_or_else(|| name.clone())),
            key: format!("sys:{name}"),
            source: name,
            app_input: None,
        });
    }
    for i in inputs.iter() {
        if prop(&i.props, Prop::ApplicationProcessId).as_deref() == Some(own_pid.as_str()) {
            continue;
        }
        let Some(monitor) = sinks
            .iter()
            .find(|k| k.index == i.sink_index)
            .and_then(|k| k.monitor_source_name.as_deref().map(text))
        else {
            continue;
        };
        let app = prop(&i.props, Prop::ApplicationName).unwrap_or_else(|| text(&i.name));
        let key = format!("app:{app}");
        // Programme über pipewire-alsa heißen „PipeWire ALSA [vlc]“.
        let app = app
            .split_once('[')
            .and_then(|(_, r)| r.strip_suffix(']'))
            .map_or(app.clone(), str::to_string);
        let media =
            prop(&i.props, Prop::MediaName).filter(|m| *m != app && !m.starts_with("ALSA "));
        out.push(CaptureSource {
            kind: SourceKind::App,
            label: match media {
                Some(m) => format!("{app} – {m}"),
                None => app.clone(),
            },
            key,
            source: monitor,
            app_input: Some(i.index),
        });
    }
    Ok(out)
}

#[derive(Default)]
struct Meters {
    level: AtomicU32,
    peak: AtomicU32,
    frames: AtomicU64,
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

/// Spielt die aufgenommenen Samples sofort wieder ab (Mithören).
struct Monitor {
    sock: UnixStream,
    channel: u32,
    drain: Option<JoinHandle<()>>,
}

impl Monitor {
    fn open() -> Result<Self, CaptureError> {
        let mut conn = connect("WOA Mithören")?;
        // Kleiner Puffer: lieber ein Knackser als hörbares Echo.
        let target = RATE / 1000 * 30 * FRAME_BYTES;
        let reply: protocol::CreatePlaybackStreamReply = ask(
            &mut conn,
            5,
            Command::CreatePlaybackStream(protocol::PlaybackStreamParams {
                sample_spec: spec(),
                channel_map: protocol::ChannelMap::stereo(),
                buffer_attr: protocol::stream::BufferAttr {
                    target_length: target,
                    pre_buffering: target / 3,
                    minimum_request_length: target / 3,
                    ..Default::default()
                },
                flags: protocol::stream::StreamFlags {
                    adjust_latency: true,
                    ..Default::default()
                },
                ..Default::default()
            }),
        )?;
        let sock = conn.0.get_ref().try_clone().map_err(err)?;
        // Der Server schickt laufend Anfragen nach Daten; wegräumen, damit nichts staut.
        let mut reader = conn.0;
        let drain = std::thread::Builder::new()
            .name("woa-monitor-drain".into())
            .spawn(move || {
                let mut buf = Vec::new();
                while let Ok(d) = protocol::read_descriptor(&mut reader) {
                    buf.resize(d.length as usize, 0);
                    if reader.read_exact(&mut buf).is_err() {
                        break;
                    }
                }
            })
            .map_err(err)?;
        Ok(Self {
            sock,
            channel: reply.channel,
            drain: Some(drain),
        })
    }

    fn send(&mut self, bytes: &[u8]) {
        let _ = protocol::write_memblock(&mut self.sock, self.channel, bytes, 0);
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        let _ = self.sock.shutdown(Shutdown::Both);
        if let Some(d) = self.drain.take() {
            let _ = d.join();
        }
    }
}

fn spec() -> protocol::SampleSpec {
    protocol::SampleSpec {
        format: protocol::SampleFormat::Float32Le,
        channels: CHANNELS as u8,
        sample_rate: RATE,
    }
}

/// Eine laufende Aufnahme aus Pulse. Endet mit [`CaptureRecorder::stop`] oder beim Fallenlassen.
pub struct CaptureRecorder {
    ctl: UnixStream,
    reader: Option<JoinHandle<Result<(), String>>>,
    meters: Arc<Meters>,
    path: PathBuf,
    started: Instant,
}

impl CaptureRecorder {
    /// Spitzenpegel seit dem letzten Aufruf (0..1), setzt ihn zurück.
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

    /// Beendet die Aufnahme und liefert das Ergebnis.
    pub fn stop(mut self) -> Result<RecordStats, CaptureError> {
        let _ = self.ctl.shutdown(Shutdown::Both);
        let reader = self
            .reader
            .take()
            .ok_or_else(|| err("Aufnahme lief nicht"))?;
        reader
            .join()
            .map_err(|_| err("Aufnahme-Thread abgestürzt"))?
            .map_err(CaptureError)?;
        Ok(RecordStats {
            path: self.path.clone(),
            sample_rate: RATE,
            channels: CHANNELS,
            frames: self.meters.frames.load(Ordering::Acquire),
            overruns: 0,
            peak: f32::from_bits(self.meters.peak.load(Ordering::Relaxed)),
            input_latency: Duration::ZERO,
        })
    }
}

impl Drop for CaptureRecorder {
    fn drop(&mut self) {
        let _ = self.ctl.shutdown(Shutdown::Both);
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
    }
}

/// Startet eine Aufnahme von `source` nach `path` (WAV 48 kHz Stereo f32). Mit `monitor`
/// wird die Quelle gleichzeitig über den Standard-Ausgang hörbar.
pub fn start_capture(
    source: &CaptureSource,
    path: &Path,
    monitor: bool,
) -> Result<CaptureRecorder, CaptureError> {
    let mut conn = connect("WOA")?;
    let mut props = protocol::Props::new();
    props.set(Prop::MediaName, CString::new("WOA Aufnahme").map_err(err)?);
    let reply: protocol::CreateRecordStreamReply = ask(
        &mut conn,
        2,
        Command::CreateRecordStream(protocol::RecordStreamParams {
            sample_spec: spec(),
            channel_map: protocol::ChannelMap::stereo(),
            source_name: Some(CString::new(source.source.as_str()).map_err(err)?),
            direct_on_input_index: source.app_input,
            buffer_attr: protocol::stream::BufferAttr {
                fragment_size: RATE / 1000 * 20 * FRAME_BYTES,
                ..Default::default()
            },
            flags: protocol::stream::StreamFlags {
                adjust_latency: true,
                ..Default::default()
            },
            props,
            ..Default::default()
        }),
    )?;
    let mut monitor = if monitor {
        Some(Monitor::open()?)
    } else {
        None
    };

    let wav_spec = hound::WavSpec {
        channels: CHANNELS,
        sample_rate: RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut wav = hound::WavWriter::create(path, wav_spec).map_err(err)?;
    let ctl = conn.0.get_ref().try_clone().map_err(err)?;
    let meters = Arc::new(Meters::default());
    let m = Arc::clone(&meters);
    let (mut sock, version) = conn;
    let channel = reply.channel;
    let reader = std::thread::Builder::new()
        .name("woa-capture".into())
        .spawn(move || -> Result<(), String> {
            let mut buf = Vec::new();
            let mut last_flush = Instant::now();
            // Endet, wenn stop() den Socket schließt.
            while let Ok(desc) = protocol::read_descriptor(&mut sock) {
                if desc.channel == u32::MAX {
                    if protocol::Command::read_tag_prefixed(&mut sock, version).is_err() {
                        break;
                    }
                    continue;
                }
                buf.resize(desc.length as usize, 0);
                if sock.read_exact(&mut buf).is_err() {
                    break;
                }
                if desc.channel != channel {
                    continue;
                }
                let mut peak = 0.0f32;
                let mut n = 0u64;
                for b in buf.chunks_exact(4) {
                    let s = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                    peak = peak.max(s.abs());
                    wav.write_sample(s).map_err(|e| e.to_string())?;
                    n += 1;
                }
                store_max(&m.level, peak);
                store_max(&m.peak, peak);
                m.frames
                    .fetch_add(n / u64::from(CHANNELS), Ordering::Release);
                if let Some(mon) = monitor.as_mut() {
                    mon.send(&buf);
                }
                if last_flush.elapsed() >= FLUSH_EVERY {
                    wav.flush().map_err(|e| e.to_string())?;
                    last_flush = Instant::now();
                }
            }
            drop(monitor);
            wav.finalize().map_err(|e| e.to_string())
        })
        .map_err(err)?;
    Ok(CaptureRecorder {
        ctl,
        reader: Some(reader),
        meters,
        path: path.to_path_buf(),
        started: Instant::now(),
    })
}
