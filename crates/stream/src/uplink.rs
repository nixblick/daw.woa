//! Eigener Thread zum Shoutcast-Server: MP3 senden, Titel setzen, Hörer zählen,
//! bei Abriss mit Backoff neu verbinden. Während des Neuaufbaus werden höchstens
//! [`BUFFER_SECONDS`] Sekunden gepuffert; Älteres fällt weg (die Sendung ist live).

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::config::StreamConfig;
use crate::kennwort::Kennwort;
use crate::shoutcast::{Source, StreamError, listeners, titles_possible, update_title};

/// Höchstens so viele Sekunden MP3 warten während des Neuaufbaus.
pub const BUFFER_SECONDS: usize = 5;
const BACKOFF_MIN: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(30);
/// Bei abgelehnter Anmeldung nicht hämmern.
const BACKOFF_REJECTED: Duration = Duration::from_secs(60);
const LISTENER_POLL: Duration = Duration::from_secs(15);

/// Verbindungszustand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Connecting,
    Connected,
    Reconnecting,
}

/// Lesbarer Zustand des Uplinks.
#[derive(Debug, Clone, Default)]
pub struct UplinkState {
    pub phase: Phase,
    pub last_error: Option<String>,
    /// Anmeldung abgelehnt (Kennwort oder Stream-ID).
    pub auth_failed: bool,
    pub listeners: Option<u32>,
    pub connected_since: Option<Instant>,
    pub bytes_sent: u64,
    pub bytes_dropped: u64,
    /// Neue Verbindungsversuche nach einem Fehlschlag oder Abriss.
    pub reconnects: u32,
    /// Nächster Verbindungsversuch, solange keine Verbindung steht.
    pub retry_at: Option<Instant>,
}

enum Msg {
    Data(Vec<u8>),
    Title(String),
    Quit,
}

pub struct Uplink {
    tx: Sender<Msg>,
    state: Arc<Mutex<UplinkState>>,
    thread: Option<JoinHandle<()>>,
}

impl Uplink {
    pub fn spawn(cfg: StreamConfig, kw: Kennwort) -> Result<Self, String> {
        let (tx, rx) = channel();
        let state = Arc::new(Mutex::new(UplinkState::default()));
        let st = Arc::clone(&state);
        let thread = std::thread::Builder::new()
            .name("woa-stream-uplink".into())
            .spawn(move || run(cfg, kw, rx, st))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            tx,
            state,
            thread: Some(thread),
        })
    }

    pub fn send(&self, bytes: &[u8]) {
        if !bytes.is_empty() {
            let _ = self.tx.send(Msg::Data(bytes.to_vec()));
        }
    }

    pub fn set_title(&self, title: &str) {
        let _ = self.tx.send(Msg::Title(title.to_string()));
    }

    pub fn state(&self) -> UplinkState {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

impl Drop for Uplink {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn set(state: &Mutex<UplinkState>, f: impl FnOnce(&mut UplinkState)) {
    if let Ok(mut s) = state.lock() {
        f(&mut s);
    }
}

fn run(cfg: StreamConfig, kw: Kennwort, rx: Receiver<Msg>, state: Arc<Mutex<UplinkState>>) {
    let cap = cfg.bitrate_kbps as usize * 125 * BUFFER_SECONDS;
    let mut buffer: VecDeque<u8> = VecDeque::with_capacity(cap);
    let mut source: Option<Source> = None;
    let mut backoff = BACKOFF_MIN;
    let mut next_try = Instant::now();
    let mut title: Option<String> = None;
    let mut title_pending = false;
    let mut next_poll = Instant::now();
    // Titel und Hörerzahl laufen über HTTP mit Fristen bis 15 s: eigener Thread,
    // damit der Sende-Pfad nie darauf wartet.
    let (job_tx, job_rx) = channel::<Job>();
    {
        let (cfg, kw, st) = (cfg.clone(), kw.clone(), Arc::clone(&state));
        let _ = std::thread::Builder::new()
            .name("woa-stream-meta".into())
            .spawn(move || meta(&cfg, &kw, &job_rx, &st));
    }
    loop {
        // Eingang sammeln.
        let mut msgs = Vec::new();
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(m) => msgs.push(m),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        while let Ok(m) = rx.try_recv() {
            msgs.push(m);
        }
        for m in msgs {
            match m {
                Msg::Data(d) => buffer.extend(d),
                Msg::Title(t) => {
                    if title.as_deref() != Some(t.as_str()) {
                        title = Some(t);
                        title_pending = true;
                    }
                }
                Msg::Quit => return,
            }
        }
        let dropped = trim_to_cap(&mut buffer, cap);
        if dropped > 0 {
            set(&state, |s| s.bytes_dropped += dropped as u64);
        }

        // Verbinden.
        if source.is_none() && Instant::now() >= next_try {
            match Source::connect(&cfg, &kw) {
                Ok(s) => {
                    tracing::info!(server = %cfg.label(), "stream connected");
                    source = Some(s);
                    backoff = BACKOFF_MIN;
                    title_pending = title.is_some();
                    next_poll = Instant::now();
                    set(&state, |s| {
                        s.phase = Phase::Connected;
                        s.last_error = None;
                        s.auth_failed = false;
                        s.connected_since = Some(Instant::now());
                        s.retry_at = None;
                    });
                }
                Err(e) => {
                    let rejected = matches!(e, StreamError::AuthFailed);
                    tracing::warn!(error = %e, "stream connect failed");
                    let wait = if rejected { BACKOFF_REJECTED } else { backoff };
                    next_try = Instant::now() + wait;
                    backoff = (backoff * 2).min(BACKOFF_MAX);
                    set(&state, |s| {
                        s.last_error = Some(e.to_string());
                        s.auth_failed = rejected;
                        s.retry_at = Some(next_try);
                        s.reconnects += 1;
                        if s.phase == Phase::Connected {
                            s.phase = Phase::Reconnecting;
                        }
                    });
                }
            }
        }

        // Senden.
        if let Some(src) = source.as_mut()
            && !buffer.is_empty()
        {
            let (a, b) = buffer.as_slices();
            let sent = a.len() + b.len();
            match src.send(a).and_then(|()| src.send(b)) {
                Ok(()) => {
                    buffer.clear();
                    set(&state, |s| s.bytes_sent += sent as u64);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "stream lost, reconnecting");
                    source = None;
                    next_try = Instant::now() + backoff;
                    set(&state, |s| {
                        s.phase = Phase::Reconnecting;
                        s.last_error = Some(e.to_string());
                        s.connected_since = None;
                        s.retry_at = Some(next_try);
                        s.reconnects += 1;
                    });
                }
            }
        }

        // Titel und Hörer (nur verbunden).
        if source.is_some() {
            if !titles_possible(&cfg) {
                title_pending = false;
            }
            if title_pending && let Some(t) = title.as_deref() {
                title_pending = false;
                let _ = job_tx.send(Job::Title(t.to_string()));
            }
            if cfg.admin_port.is_some() && Instant::now() >= next_poll {
                next_poll = Instant::now() + LISTENER_POLL;
                let _ = job_tx.send(Job::Listeners);
            }
        }
    }
}

enum Job {
    Title(String),
    Listeners,
}

/// Arbeitet Titel- und Hörerabfragen ab; endet, wenn der Uplink-Thread endet.
fn meta(cfg: &StreamConfig, kw: &Kennwort, rx: &Receiver<Job>, state: &Mutex<UplinkState>) {
    while let Ok(job) = rx.recv() {
        // Aufgestaute Hörerabfragen zusammenfassen, Titel nur den neuesten.
        let (mut title, mut poll) = (None, false);
        for j in std::iter::once(job).chain(rx.try_iter()) {
            match j {
                Job::Title(t) => title = Some(t),
                Job::Listeners => poll = true,
            }
        }
        if let Some(t) = title
            && let Err(e) = update_title(cfg, kw, &t)
        {
            tracing::warn!(error = %e, "title update failed");
            set(state, |s| s.last_error = Some(e.to_string()));
        }
        if poll {
            let n = listeners(cfg);
            set(state, |s| s.listeners = n);
        }
    }
}

/// Kürzt den Puffer auf `cap` Bytes und verwirft danach bis zum nächsten
/// MP3-Sync-Wort, damit der Strom an einer Frame-Grenze weitergeht.
fn trim_to_cap(buffer: &mut VecDeque<u8>, cap: usize) -> usize {
    if buffer.len() <= cap {
        return 0;
    }
    let mut drop = buffer.len() - cap;
    while drop + 1 < buffer.len() && !(buffer[drop] == 0xFF && buffer[drop + 1] & 0xE0 == 0xE0) {
        drop += 1;
    }
    if drop + 1 >= buffer.len() {
        drop = buffer.len();
    }
    buffer.drain(..drop);
    drop
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_keeps_frame_boundary() {
        let mut b: VecDeque<u8> = VecDeque::new();
        for _ in 0..10 {
            b.extend([0xFF, 0xFB, 1, 2, 3, 4, 5, 6]);
        }
        let dropped = trim_to_cap(&mut b, 75);
        assert_eq!(dropped, 8);
        assert_eq!((b[0], b[1]), (0xFF, 0xFB));
        assert_eq!(trim_to_cap(&mut b, 100), 0);
    }
}
