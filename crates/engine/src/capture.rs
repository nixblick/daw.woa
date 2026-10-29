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

use crate::live_peaks::{LivePeaks, live_peaks};
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
    let own = OwnProcess {
        pid: std::process::id().to_string(),
        exe: std::env::current_exe()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default(),
    };
    Ok(build_sources(&sources, &sinks, &inputs, &own))
}

/// Woran WOA die eigenen Wiedergabe-Streams erkennt.
#[derive(Debug, Clone, Default)]
pub struct OwnProcess {
    /// Prozess-ID als Text.
    pub pid: String,
    /// Dateiname des Programms (`woa`); pipewire-alsa nennt den Stream „PipeWire ALSA [woa]“
    /// und meldet keine Prozess-ID.
    pub exe: String,
}

/// Medientitel ohne Aussage, die nur „Wiedergabe“ heißen.
fn generic_media(m: &str) -> bool {
    let m = m.to_ascii_lowercase();
    m.starts_with("alsa ")
        || ["playback", "audio stream", "audiostream", "output"].contains(&m.as_str())
}

/// Baut die Quellenliste aus den Antworten des Servers (ohne I/O, testbar).
pub fn build_sources(
    sources: &[protocol::SourceInfo],
    sinks: &[protocol::SinkInfo],
    inputs: &[protocol::SinkInputInfo],
    own: &OwnProcess,
) -> Vec<CaptureSource> {
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
    // Laufende Streams zuerst, damit bei gleichem Programm der hörbare gewinnt.
    let mut inputs: Vec<&protocol::SinkInputInfo> = inputs.iter().collect();
    inputs.sort_by_key(|i| i.corked);
    for i in inputs {
        let app = prop(&i.props, Prop::ApplicationName)
            .or_else(|| prop(&i.props, Prop::ApplicationProcessBinary))
            .unwrap_or_else(|| text(&i.name));
        let pid = prop(&i.props, Prop::ApplicationProcessId);
        let alsa_own = !own.exe.is_empty() && app.ends_with(&format!("[{}]", own.exe));
        if pid.as_deref() == Some(own.pid.as_str()) || alsa_own {
            continue;
        }
        // Ohne passenden Ausgang (z. B. noch nicht verbunden) nicht verstecken, sondern den
        // ersten Ausgang nehmen; mitgeschnitten wird ohnehin direkt am Stream.
        let monitor = sinks
            .iter()
            .find(|k| k.index == i.sink_index)
            .or_else(|| sinks.first())
            .and_then(|k| k.monitor_source_name.as_deref().map(text));
        let Some(monitor) = monitor else {
            continue;
        };
        let key = format!("app:{app}");
        if out.iter().any(|s: &CaptureSource| s.key == key) {
            continue;
        }
        // Programme über pipewire-alsa heißen „PipeWire ALSA [vlc]“.
        let app = app
            .split_once('[')
            .and_then(|(_, r)| r.strip_suffix(']'))
            .map_or(app.clone(), str::to_string);
        let media = prop(&i.props, Prop::MediaName).filter(|m| *m != app && !generic_media(m));
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
    out
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
    peaks: LivePeaks,
}

impl CaptureRecorder {
    /// Hängt die seit dem letzten Aufruf fertigen Wellenform-Punkte an (je
    /// [`crate::clip::PEAK_BLOCK`] Frames bei [`RATE`]).
    pub fn drain_peaks(&mut self, out: &mut Vec<f32>) {
        self.peaks.drain_into(out);
    }

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
    let (mut collector, peaks) = live_peaks(CHANNELS);
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
                let (samples, _) = buf.as_chunks::<4>();
                for b in samples {
                    let s = f32::from_le_bytes(*b);
                    peak = peak.max(s.abs());
                    collector.push(s);
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
        peaks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(kv: &[(Prop, &str)]) -> protocol::Props {
        let mut p = protocol::Props::new();
        for (k, v) in kv {
            p.set(*k, CString::new(*v).unwrap());
        }
        p
    }

    fn sink() -> protocol::SinkInfo {
        protocol::SinkInfo {
            index: 54,
            name: CString::new("alsa_output.pci-0000_00_1f.3.analog-stereo").unwrap(),
            monitor_source_name: Some(
                CString::new("alsa_output.pci-0000_00_1f.3.analog-stereo.monitor").unwrap(),
            ),
            ..Default::default()
        }
    }

    fn input(idx: u32, sink: u32, corked: bool, kv: &[(Prop, &str)]) -> protocol::SinkInputInfo {
        protocol::SinkInputInfo {
            index: idx,
            name: CString::new("Playback").unwrap(),
            sink_index: sink,
            corked,
            props: props(kv),
            ..Default::default()
        }
    }

    /// Nachgebaut aus `wpctl inspect` und WirePlumber-`restore-stream` vom 29.10.2026:
    /// WOA selbst über pipewire-alsa (ohne Prozess-ID), Chromium über pipewire-pulse.
    fn fixture() -> Vec<protocol::SinkInputInfo> {
        let woa = [
            (Prop::ApplicationName, "PipeWire ALSA [woa]"),
            (Prop::MediaName, "ALSA Playback"),
        ];
        vec![
            input(1313, 54, false, &woa),
            input(1321, 54, true, &woa),
            input(
                1400,
                54,
                false,
                &[
                    (Prop::ApplicationName, "Chromium"),
                    (Prop::ApplicationProcessBinary, "chromium-browser"),
                    (Prop::ApplicationProcessId, "3034701"),
                    (Prop::MediaName, "Playback"),
                ],
            ),
        ]
    }

    fn own() -> OwnProcess {
        OwnProcess {
            pid: "3033057".into(),
            exe: "woa".into(),
        }
    }

    fn apps(list: &[CaptureSource]) -> Vec<(&str, &str, Option<u32>)> {
        list.iter()
            .filter(|s| s.kind == SourceKind::App)
            .map(|s| (s.label.as_str(), s.key.as_str(), s.app_input))
            .collect()
    }

    #[test]
    fn chromium_listed_own_streams_hidden() {
        let out = build_sources(&[], &[sink()], &fixture(), &own());
        assert_eq!(apps(&out), vec![("Chromium", "app:Chromium", Some(1400))]);
    }

    #[test]
    fn media_title_shown_and_binary_fallback() {
        let inputs = vec![input(
            7,
            54,
            false,
            &[
                (Prop::ApplicationProcessBinary, "firefox"),
                (Prop::MediaName, "Radio Bob – Live"),
            ],
        )];
        let out = build_sources(&[], &[sink()], &inputs, &own());
        assert_eq!(
            apps(&out),
            vec![("firefox – Radio Bob – Live", "app:firefox", Some(7))]
        );
    }

    #[test]
    fn unknown_sink_does_not_hide_stream() {
        let inputs = vec![input(
            9,
            u32::MAX,
            false,
            &[(Prop::ApplicationName, "Chromium")],
        )];
        let out = build_sources(&[], &[sink()], &inputs, &own());
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].source,
            "alsa_output.pci-0000_00_1f.3.analog-stereo.monitor"
        );
    }

    #[test]
    fn duplicate_app_keeps_running_stream() {
        let inputs = vec![
            input(1, 54, true, &[(Prop::ApplicationName, "Chromium")]),
            input(2, 54, false, &[(Prop::ApplicationName, "Chromium")]),
        ];
        let out = build_sources(&[], &[sink()], &inputs, &own());
        assert_eq!(apps(&out), vec![("Chromium", "app:Chromium", Some(2))]);
    }
}
