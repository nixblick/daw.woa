//! Kern von `woa-streamd`: nimmt den Sendemix der Oberfläche an, spielt nach
//! `silence_seconds` ohne Ton selbst die Notfall-Playlist, kodiert MP3 und schickt es
//! über den [`Uplink`] an den Server.
//!
//! woa-streamd ist der Takt der Sendung: Er gibt Audio im Wanduhr-Takt aus (Blöcke zu
//! 20 ms). Der Sendemix der Oberfläche läuft über einen kleinen Puffer mit Vorlauf; läuft
//! er leer, wird mit Stille aufgefüllt, wird er zu voll (Uhren driften), wird gekürzt.
//! Wechsel zwischen Sendemix und Notfall-Playlist werden über 1 s überblendet.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use woa_radio::audio::{Stereo, is_supported, load_stereo, resample};

use crate::config::StreamConfig;
use crate::kennwort::Kennwort;
use crate::link::{
    HANDSHAKE, KIND_AUDIO, KIND_STOP, KIND_TITLE, LocalListener, LocalStream, MAX_FRAME,
    decode_audio, is_alive, link_path, read_frame, read_hello,
};
use crate::mp3::Mp3Encoder;
use crate::status::{self, StreamStatus};
use crate::uplink::{Phase, Uplink};

/// Unter diesem Pegel gilt der Sendemix als Stille (≈ −80 dBFS).
const SILENCE: f32 = 1e-4;
/// Überblendzeit Sendemix ↔ Notfall-Playlist.
const FADE_SECONDS: f32 = 1.0;
/// Vorlauf des Sendemix-Puffers nach einem Leerlauf.
const PREFILL_SECONDS: f32 = 0.1;
/// Mehr als so viel Puffer: auf den Vorlauf kürzen (Uhren-Drift).
const MAX_LIVE_SECONDS: f32 = 0.5;
const BLOCK_MS: u32 = 20;
const STATUS_EVERY: Duration = Duration::from_millis(250);
/// Höchstens so viele lokale Verbindungen gleichzeitig vor der Anmeldung. Kommt eine
/// weitere, wird die älteste wartende geschlossen (nicht die neue), damit stumme
/// Verbindungen die echte Oberfläche nicht aussperren.
const MAX_PENDING: usize = 4;

/// Was die Oberfläche geschickt hat.
#[derive(Default)]
struct Inbox {
    live: VecDeque<f32>,
    last_sound: Option<Instant>,
    ui_connected: bool,
    title: Option<String>,
    generation: u64,
}

/// Notfall-Playlist: Dateien der Reihe nach, endlos.
pub struct Emergency {
    files: Vec<PathBuf>,
    rate: u32,
    idx: usize,
    cur: Option<Stereo>,
    pos: usize,
    title: String,
    failed_round: usize,
    pause_until: Option<Instant>,
}

/// Ordner werden zu ihren lesbaren Dateien (sortiert), Dateien bleiben.
pub fn expand(entries: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in entries {
        if e.is_dir() {
            let mut files: Vec<PathBuf> = std::fs::read_dir(e)
                .map(|rd| {
                    rd.filter_map(Result::ok)
                        .map(|d| d.path())
                        .filter(|p| p.is_file() && is_supported(p))
                        .collect()
                })
                .unwrap_or_else(|err| {
                    tracing::warn!(dir = %e.display(), error = %err, "notfall: ordner unlesbar");
                    Vec::new()
                });
            files.sort();
            out.extend(files);
        } else if is_supported(e) {
            out.push(e.clone());
        } else {
            tracing::warn!(file = %e.display(), "notfall: format nicht lesbar, übersprungen");
        }
    }
    out
}

impl Emergency {
    pub fn new(entries: &[PathBuf], rate: u32) -> Self {
        let files = expand(entries);
        if files.is_empty() {
            tracing::warn!("notfall-playlist leer: bei Ausfall wird Stille gesendet");
        }
        Self {
            files,
            rate,
            idx: 0,
            cur: None,
            pos: 0,
            title: String::new(),
            failed_round: 0,
            pause_until: None,
        }
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    fn load_next(&mut self) {
        if self.files.is_empty() {
            return;
        }
        if let Some(t) = self.pause_until {
            if Instant::now() < t {
                return;
            }
            self.pause_until = None;
        }
        let path = self.files[self.idx % self.files.len()].clone();
        self.idx = (self.idx + 1) % self.files.len();
        match load_stereo(&path, self.rate) {
            Ok(s) if s.frames() > 0 => {
                self.title = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.cur = Some(s);
                self.pos = 0;
                self.failed_round = 0;
            }
            Ok(_) | Err(_) => {
                tracing::warn!(file = %path.display(), "notfall: datei nicht spielbar");
                self.failed_round += 1;
                if self.failed_round >= self.files.len() {
                    // Keine Datei spielbar: 10 s Stille, dann neu versuchen.
                    self.failed_round = 0;
                    self.pause_until = Some(Instant::now() + Duration::from_secs(10));
                }
            }
        }
    }

    /// Füllt `l`/`r` (Stille, wenn nichts spielbar ist).
    pub fn fill(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(r.len());
        let mut i = 0;
        let mut tries = 0;
        while i < n {
            let done = self.cur.as_ref().is_none_or(|c| self.pos >= c.frames());
            if done {
                self.cur = None;
                self.load_next();
                if self.cur.is_none() {
                    tries += 1;
                    if tries > self.files.len().max(1) {
                        break;
                    }
                    continue;
                }
            }
            if let Some(c) = self.cur.as_ref() {
                let k = (c.frames() - self.pos).min(n - i);
                l[i..i + k].copy_from_slice(&c.left[self.pos..self.pos + k]);
                r[i..i + k].copy_from_slice(&c.right[self.pos..self.pos + k]);
                self.pos += k;
                i += k;
            }
        }
        l[i..n].fill(0.0);
        r[i..n].fill(0.0);
    }
}

/// Startet woa-streamd im aktuellen Thread und kehrt erst zurück, wenn die Sendung endet
/// (`Stop` von der Oberfläche oder `stop` von außen).
pub fn run(cfg: StreamConfig, data_dir: &Path, stop: Arc<AtomicBool>) -> Result<(), String> {
    let status_path = status::status_path(data_dir);
    let started = Instant::now();
    let fail = |msg: String| {
        let s = StreamStatus {
            updated_unix: crate::unix_now(),
            pid: std::process::id(),
            server: cfg.label(),
            connection: "beendet".into(),
            last_error: Some(msg.clone()),
            ..StreamStatus::default()
        };
        let _ = status::write(&status_path, &s);
        msg
    };
    cfg.validate().map_err(&fail)?;
    let kw = Kennwort::from_file(&cfg.password_file).map_err(&fail)?;
    let link = link_path(data_dir);
    if is_alive(&link) {
        return Err(fail("woa-streamd läuft schon".into()));
    }

    let listener = LocalListener::bind(data_dir).map_err(|e| fail(e.to_string()))?;
    let token = crate::random_token();
    let info = listener
        .info(token.clone())
        .map_err(|e| fail(e.to_string()))?;
    info.write(&link)
        .map_err(|e| fail(format!("{}: {e}", link.display())))?;

    let rate = cfg.sample_rate;
    let inbox = Arc::new(Mutex::new(Inbox::default()));
    {
        let inbox = Arc::clone(&inbox);
        let stop = Arc::clone(&stop);
        std::thread::Builder::new()
            .name("woa-streamd-accept".into())
            .spawn(move || accept(listener, token, rate, inbox, stop))
            .map_err(|e| fail(e.to_string()))?;
    }

    let mut enc = Mp3Encoder::new(rate, cfg.bitrate_kbps).map_err(&fail)?;
    let uplink = Uplink::spawn(cfg.clone(), kw).map_err(&fail)?;
    let mut emergency = Emergency::new(&cfg.emergency, rate);
    tracing::info!(
        server = %cfg.label(),
        emergency_files = emergency.len(),
        "woa-streamd on air"
    );

    let block = (rate * BLOCK_MS / 1000) as usize;
    let fade_step = 1.0 / (FADE_SECONDS * rate as f32);
    let prefill = (PREFILL_SECONDS * rate as f32) as usize * 2;
    let max_live = (MAX_LIVE_SECONDS * rate as f32) as usize * 2;
    let silence = Duration::from_secs_f64(cfg.silence_seconds.max(0.1));
    let (mut ll, mut lr) = (vec![0.0f32; block], vec![0.0f32; block]);
    let (mut el, mut er) = (vec![0.0f32; block], vec![0.0f32; block]);
    let mut gain_live = 0.0f32;
    let mut buffering = true;
    let mut produced: u64 = 0;
    let mut sent_title = String::new();
    let mut next_status = Instant::now();
    let mut live_now = false;

    while !stop.load(Ordering::Relaxed) {
        let due = (started.elapsed().as_secs_f64() * f64::from(rate)) as u64;
        while produced + block as u64 <= due {
            // Sendemix holen.
            let (live_ok, ui_title) = {
                let mut ib = match inbox.lock() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                let live_ok = ib
                    .last_sound
                    .map_or(started.elapsed() < silence, |t| t.elapsed() < silence);
                if ib.live.len() > max_live {
                    let cut = (ib.live.len() - prefill) & !1;
                    ib.live.drain(..cut);
                }
                if buffering && ib.live.len() >= prefill {
                    buffering = false;
                }
                if !buffering && ib.live.len() < block * 2 {
                    buffering = true;
                }
                if buffering {
                    ll.fill(0.0);
                    lr.fill(0.0);
                } else {
                    for i in 0..block {
                        ll[i] = ib.live.pop_front().unwrap_or(0.0);
                        lr[i] = ib.live.pop_front().unwrap_or(0.0);
                    }
                }
                (live_ok, ib.title.clone())
            };
            live_now = live_ok;
            let target = if live_ok { 1.0 } else { 0.0 };
            if gain_live < 1.0 || !live_ok {
                emergency.fill(&mut el, &mut er);
            } else {
                el.fill(0.0);
                er.fill(0.0);
            }
            for i in 0..block {
                if gain_live < target {
                    gain_live = (gain_live + fade_step).min(1.0);
                } else if gain_live > target {
                    gain_live = (gain_live - fade_step).max(0.0);
                }
                let g = gain_live;
                ll[i] = ll[i] * g + el[i] * (1.0 - g);
                lr[i] = lr[i] * g + er[i] * (1.0 - g);
            }
            match enc.encode(&ll, &lr) {
                Ok(bytes) => uplink.send(bytes),
                Err(e) => tracing::warn!(error = %e, "encode failed"),
            }
            produced += block as u64;

            let title = if live_ok {
                ui_title.unwrap_or_else(|| cfg.name.clone())
            } else if emergency.title().is_empty() {
                cfg.name.clone()
            } else {
                emergency.title().to_string()
            };
            if title != sent_title {
                uplink.set_title(&title);
                sent_title = title;
            }
        }

        if Instant::now() >= next_status {
            next_status = Instant::now() + STATUS_EVERY;
            let ui = inbox.lock().map(|i| i.ui_connected).unwrap_or(false);
            let s = build_status(&cfg, &uplink, started, live_now, ui, &sent_title, "");
            if let Err(e) = status::write(&status_path, &s) {
                tracing::warn!(error = %e, "status not written");
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    if let Ok(bytes) = enc.flush() {
        uplink.send(bytes);
    }
    let s = build_status(&cfg, &uplink, started, false, false, &sent_title, "beendet");
    drop(uplink);
    let _ = std::fs::remove_file(&link);
    if !info.socket.as_os_str().is_empty() {
        let _ = std::fs::remove_file(&info.socket);
    }
    let _ = status::write(&status_path, &s);
    tracing::info!("woa-streamd off air");
    Ok(())
}

fn build_status(
    cfg: &StreamConfig,
    uplink: &Uplink,
    started: Instant,
    live: bool,
    ui: bool,
    title: &str,
    connection: &str,
) -> StreamStatus {
    let u = uplink.state();
    let connection = if connection.is_empty() {
        match u.phase {
            Phase::Connecting => "verbinde",
            Phase::Connected => "verbunden",
            Phase::Reconnecting => "verbinde neu",
        }
    } else {
        connection
    };
    StreamStatus {
        updated_unix: crate::unix_now(),
        pid: std::process::id(),
        server: cfg.label(),
        connection: connection.to_string(),
        connected: u.phase == Phase::Connected,
        source: if live { "Sendemix" } else { "Notfall-Playlist" }.to_string(),
        emergency: !live,
        ui_connected: ui,
        listeners: u.listeners,
        bitrate_kbps: cfg.bitrate_kbps,
        sample_rate: cfg.sample_rate,
        connected_seconds: u.connected_since.map_or(0, |t| t.elapsed().as_secs()),
        on_air_seconds: started.elapsed().as_secs(),
        title: title.to_string(),
        last_error: u.last_error,
        reconnects: u.reconnects,
        bytes_sent: u.bytes_sent,
        bytes_dropped: u.bytes_dropped,
    }
}

type Pending = Mutex<std::collections::VecDeque<(u64, LocalStream)>>;

fn accept(
    listener: LocalListener,
    token: String,
    rate: u32,
    inbox: Arc<Mutex<Inbox>>,
    stop: Arc<AtomicBool>,
) {
    let pending: Arc<Pending> = Arc::new(Mutex::new(std::collections::VecDeque::new()));
    let mut next_id = 0u64;
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok(s) => {
                next_id += 1;
                let id = next_id;
                let Ok(handle) = s.try_clone() else {
                    continue;
                };
                {
                    let mut p = match pending.lock() {
                        Ok(g) => g,
                        Err(e) => e.into_inner(),
                    };
                    while p.len() >= MAX_PENDING {
                        if let Some((_, old)) = p.pop_front() {
                            old.shutdown();
                        }
                    }
                    p.push_back((id, handle));
                }
                let (token, inbox, stop, p) = (
                    token.clone(),
                    Arc::clone(&inbox),
                    Arc::clone(&stop),
                    Arc::clone(&pending),
                );
                let spawned = std::thread::Builder::new()
                    .name("woa-streamd-ui".into())
                    .spawn(move || serve(s, id, &token, rate, &inbox, &stop, &p));
                if let Err(e) = spawned {
                    tracing::warn!(error = %e, "ui thread failed");
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                tracing::warn!(error = %e, "accept failed");
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
    listener.cleanup();
}

fn lock(inbox: &Mutex<Inbox>) -> std::sync::MutexGuard<'_, Inbox> {
    match inbox.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

/// Eine Verbindung der Oberfläche. Nach gültigem `Hello` löst sie jede ältere ab, es gibt
/// also höchstens eine aktive Oberfläche.
fn serve(
    mut s: LocalStream,
    id: u64,
    token: &str,
    out_rate: u32,
    inbox: &Mutex<Inbox>,
    stop: &AtomicBool,
    pending: &Pending,
) {
    let _ = s.set_nonblocking(false);
    let hello = read_hello(&mut s, token, Instant::now() + HANDSHAKE);
    // Atomar aus „wartend“ nehmen: Unter derselben Sperre, unter der `accept` verdrängt.
    // War die Verbindung schon verdrängt, gilt sie nicht; danach kann sie es nicht mehr werden.
    let still_pending = {
        let mut p = match pending.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        let before = p.len();
        p.retain(|(i, _)| *i != id);
        p.len() != before
    };
    let Some(in_rate) = hello.filter(|_| still_pending) else {
        return; // Prüfverbindung, Frist vorbei oder falsches Token.
    };
    let mut buf = Vec::new();
    if !(8_000..=384_000).contains(&in_rate) {
        tracing::warn!(rate = in_rate, "ui link: bad rate");
        return;
    }
    let my_gen = {
        let mut ib = lock(inbox);
        ib.generation += 1;
        ib.ui_connected = true;
        ib.live.clear();
        ib.generation
    };
    tracing::info!("ui connected");
    let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
    let mut samples = Vec::new();
    let (mut l, mut r) = (Vec::new(), Vec::new());
    while !stop.load(Ordering::Relaxed) && lock(inbox).generation == my_gen {
        match read_frame(&mut s, &mut buf, MAX_FRAME) {
            Ok(KIND_AUDIO) => {
                decode_audio(&buf, &mut samples);
                let loud = samples.iter().any(|x| x.abs() > SILENCE);
                let mut ib = lock(inbox);
                if ib.generation != my_gen {
                    break;
                }
                if loud {
                    ib.last_sound = Some(Instant::now());
                }
                if in_rate == out_rate {
                    ib.live.extend(samples.iter().copied());
                } else {
                    l.clear();
                    r.clear();
                    for f in samples.chunks_exact(2) {
                        l.push(f[0]);
                        r.push(f[1]);
                    }
                    let (rl, rr) = (
                        resample(&l, in_rate, out_rate),
                        resample(&r, in_rate, out_rate),
                    );
                    for (a, b) in rl.iter().zip(&rr) {
                        ib.live.push_back(*a);
                        ib.live.push_back(*b);
                    }
                }
                let max = out_rate as usize * 4;
                if ib.live.len() > max {
                    let cut = (ib.live.len() - max) & !1;
                    ib.live.drain(..cut);
                }
            }
            Ok(KIND_TITLE) => {
                lock(inbox).title = Some(String::from_utf8_lossy(&buf).into_owned());
            }
            Ok(KIND_STOP) => {
                tracing::info!("stop from ui");
                stop.store(true, Ordering::Relaxed);
                break;
            }
            Ok(_) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => break,
        }
    }
    let mut ib = lock(inbox);
    if ib.generation == my_gen {
        ib.ui_connected = false;
        tracing::info!("ui disconnected");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emergency_loops_and_survives_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("a.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut w = hound::WavWriter::create(&p, spec).unwrap();
        for _ in 0..1000 {
            w.write_sample(0.5f32).unwrap();
            w.write_sample(0.5f32).unwrap();
        }
        w.finalize().unwrap();
        std::fs::write(tmp.path().join("notiz.txt"), "x").unwrap();
        let mut e = Emergency::new(&[tmp.path().to_path_buf()], 44_100);
        assert_eq!(e.len(), 1);
        let (mut l, mut r) = (vec![0.0; 2500], vec![0.0; 2500]);
        e.fill(&mut l, &mut r);
        assert!(l.iter().all(|x| (*x - 0.5).abs() < 1e-6), "loops endlessly");
        assert_eq!(e.title(), "a");
        let mut none = Emergency::new(&[], 44_100);
        none.fill(&mut l, &mut r);
        assert!(l.iter().all(|x| *x == 0.0));
    }
}
