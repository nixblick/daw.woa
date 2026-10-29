//! Upload-Senke: gebündelte Ereignisse an `POST <url>` (die `/ingest`-Route).
//!
//! Wire-Vertrag (kompatibel zu bretteins `ingest_zeilen`, siehe `docs/telemetrie.md`):
//!
//! ```json
//! {"install_id":"…","app_version":"0.1.0","os":"linux",
//!  "ereignisse":[{"zeit_ms":1690000000000,"stufe":"warn","modul":"woa_engine","art":"xrun","wert":1}]}
//! ```
//!
//! - Opt-in: ohne `enabled` oder mit leerer URL startet nichts.
//! - Nur Code/Stufe/Zahl; `modul` und `art` laufen durch [`crate::scrub`].
//! - HTTP (`ureq`, blockierend) nur im eigenen Thread. Erfassen ist ein `try_send` auf eine
//!   begrenzte Queue; ist sie voll, wird das Ereignis verworfen statt zu blockieren.
//! - 2xx heißt: Server hat gespeichert, Puffer leeren. Alles andere: Puffer behalten,
//!   später erneut versuchen (der Server antwortet 503, wenn er nicht speichern konnte).

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tracing::field::{Field, Visit};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

use crate::scrub::{MAX_ART, MAX_MODUL, is_identifier, module_from_target};

/// Stufe eines Ereignisses (Server erkennt `error`/`warn` als Fehler/Warnung).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
}

impl Level {
    fn from_tracing(l: &tracing::Level) -> Self {
        match *l {
            tracing::Level::ERROR => Level::Error,
            tracing::Level::WARN => Level::Warn,
            tracing::Level::INFO => Level::Info,
            _ => Level::Debug,
        }
    }
}

/// Ein Ereignis auf der Leitung. Felder können nur über [`Event::new`] entstehen,
/// das den Scrubber erzwingt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Event {
    zeit_ms: u64,
    stufe: Level,
    modul: String,
    art: String,
    wert: i64,
}

impl Event {
    /// Baut ein Ereignis; `None`, wenn `modul` oder `art` kein zulässiger Bezeichner ist.
    pub fn new(stufe: Level, modul: &str, art: &str, wert: i64) -> Option<Self> {
        if !is_identifier(modul, MAX_MODUL) || !is_identifier(art, MAX_ART) {
            return None;
        }
        Some(Self {
            zeit_ms: now_ms(),
            stufe,
            modul: modul.to_string(),
            art: art.to_string(),
            wert,
        })
    }

    pub fn art(&self) -> &str {
        &self.art
    }
    pub fn modul(&self) -> &str {
        &self.modul
    }
    pub fn wert(&self) -> i64 {
        self.wert
    }
    pub fn stufe(&self) -> Level {
        self.stufe
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Log-Endpunkt des eigenen Servers (bretteins.infra, Instanz `daw.woa` auf lfs001).
pub const DEFAULT_INGEST_URL: &str = "https://feedback.wironair.de/api/logs/ingest";

/// Neue zufällige Installations-ID im UUID-v4-Format, ohne Personenbezug.
///
/// Zufall aus dem zufällig gesäten `RandomState` der Standardbibliothek plus Zeit und
/// Prozess-ID (keine zusätzliche Abhängigkeit).
pub fn new_install_id() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut words = [0u64; 2];
    for (i, w) in words.iter_mut().enumerate() {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(now_ms());
        h.write_u32(std::process::id());
        h.write_usize(i);
        h.write_u128(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        );
        *w = h.finish();
    }
    let mut b = [0u8; 16];
    b[..8].copy_from_slice(&words[0].to_le_bytes());
    b[8..].copy_from_slice(&words[1].to_le_bytes());
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// Einstellungen der Upload-Senke.
#[derive(Debug, Clone)]
pub struct UploadConfig {
    /// Opt-in des Nutzers. `false` = keine Senke, kein Thread, kein Netz.
    pub enabled: bool,
    /// Ziel, z. B. `https://example.org/ingest`. Leer = aus (Standard).
    pub url: String,
    /// Zufällige Installations-ID (UUID), höchstens 64 Zeichen.
    pub install_id: String,
    pub app_version: String,
    pub os: String,
    /// Ab so vielen Ereignissen wird sofort gesendet.
    pub batch_size: usize,
    /// Spätestens nach dieser Zeit wird gesendet, wenn etwas im Puffer liegt.
    pub interval: Duration,
    /// Obergrenze des Puffers bei Serverausfall; älteste Ereignisse fallen weg.
    pub max_buffer: usize,
    /// Kapazität der Queue zwischen Erfassen und Upload-Thread.
    pub queue_capacity: usize,
    /// HTTP-Timeout je Anfrage.
    pub timeout: Duration,
}

impl Default for UploadConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: String::new(),
            install_id: String::new(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
            batch_size: 200,
            interval: Duration::from_secs(300),
            max_buffer: 2000,
            queue_capacity: 1024,
            timeout: Duration::from_secs(15),
        }
    }
}

#[derive(Serialize)]
struct Batch<'a> {
    #[serde(skip_serializing_if = "str::is_empty")]
    install_id: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    app_version: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    os: &'a str,
    ereignisse: &'a [Event],
}

enum Msg {
    Event(Event),
    Flush(mpsc::Sender<bool>),
}

/// Laufende Upload-Senke. Beim Drop wird ein letzter Upload versucht.
pub struct Uploader {
    tx: Option<SyncSender<Msg>>,
    handle: Option<JoinHandle<()>>,
}

impl Uploader {
    /// Startet den Upload-Thread; `None` ohne Opt-in oder ohne URL.
    pub fn start(cfg: UploadConfig) -> Option<Self> {
        if !cfg.enabled || cfg.url.trim().is_empty() {
            return None;
        }
        let (tx, rx) = mpsc::sync_channel(cfg.queue_capacity.max(1));
        let handle = std::thread::Builder::new()
            .name("woa-telemetry-upload".into())
            .spawn(move || worker(cfg, rx));
        match handle {
            Ok(h) => Some(Self {
                tx: Some(tx),
                handle: Some(h),
            }),
            Err(e) => {
                eprintln!("woa-telemetry: upload thread not started: {e}");
                None
            }
        }
    }

    /// Ereignis einreihen, blockiert nie. `false`, wenn die Queue voll ist oder das
    /// Ereignis den Scrubber nicht passiert.
    pub fn record(&self, stufe: Level, modul: &str, art: &str, wert: i64) -> bool {
        match Event::new(stufe, modul, art, wert) {
            Some(e) => self.send(e),
            None => false,
        }
    }

    fn send(&self, e: Event) -> bool {
        match &self.tx {
            Some(tx) => match tx.try_send(Msg::Event(e)) {
                Ok(()) => true,
                Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => false,
            },
            None => false,
        }
    }

    /// Jetzt senden und auf das Ergebnis warten (`true` = Server hat 2xx gegeben
    /// oder nichts war zu senden).
    pub fn flush(&self) -> bool {
        let Some(tx) = &self.tx else { return false };
        let (rtx, rrx) = mpsc::channel();
        if tx.send(Msg::Flush(rtx)).is_err() {
            return false;
        }
        rrx.recv().unwrap_or(false)
    }

    /// tracing-Layer, der WARN/ERROR und Ereignisse mit Feld `art` in die Senke speist.
    /// Nachricht und alle anderen Felder bleiben lokal.
    pub fn layer(&self) -> UploadLayer {
        UploadLayer {
            tx: self.tx.clone(),
        }
    }
}

impl Drop for Uploader {
    fn drop(&mut self) {
        // Sender schließen -> Worker sendet den Rest und endet.
        self.tx = None;
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn worker(cfg: UploadConfig, rx: Receiver<Msg>) {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(cfg.timeout))
        .http_status_as_error(false)
        .build()
        .into();
    let mut buf: Vec<Event> = Vec::new();
    let mut last = Instant::now();
    loop {
        let wait = cfg.interval.saturating_sub(last.elapsed());
        match rx.recv_timeout(wait) {
            Ok(Msg::Event(e)) => {
                if buf.len() >= cfg.max_buffer.max(1) {
                    buf.remove(0);
                }
                buf.push(e);
                if buf.len() >= cfg.batch_size.max(1) {
                    send_batch(&agent, &cfg, &mut buf);
                    last = Instant::now();
                }
            }
            Ok(Msg::Flush(reply)) => {
                let ok = send_batch(&agent, &cfg, &mut buf);
                last = Instant::now();
                let _ = reply.send(ok);
            }
            Err(RecvTimeoutError::Timeout) => {
                send_batch(&agent, &cfg, &mut buf);
                last = Instant::now();
            }
            Err(RecvTimeoutError::Disconnected) => {
                send_batch(&agent, &cfg, &mut buf);
                return;
            }
        }
    }
}

/// Sendet den Puffer; leert ihn nur bei 2xx. Liefert `true` bei Erfolg oder leerem Puffer.
fn send_batch(agent: &ureq::Agent, cfg: &UploadConfig, buf: &mut Vec<Event>) -> bool {
    if buf.is_empty() {
        return true;
    }
    let batch = Batch {
        install_id: &cfg.install_id,
        app_version: &cfg.app_version,
        os: &cfg.os,
        ereignisse: buf,
    };
    let body = match serde_json::to_string(&batch) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("woa-telemetry: batch not serializable: {e}");
            return false;
        }
    };
    match agent
        .post(&cfg.url)
        .header("Content-Type", "application/json")
        .send(body)
    {
        Ok(resp) if resp.status().is_success() => {
            buf.clear();
            true
        }
        // Kein tracing hier: das liefe in die eigene Senke zurück.
        Ok(_) | Err(_) => false,
    }
}

/// Siehe [`Uploader::layer`].
pub struct UploadLayer {
    tx: Option<SyncSender<Msg>>,
}

#[derive(Default)]
struct Fields {
    art: Option<String>,
    wert: i64,
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "art" {
            self.art = Some(value.to_string());
        }
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        if field.name() == "wert" {
            self.wert = value;
        }
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == "wert" {
            self.wert = i64::try_from(value).unwrap_or(i64::MAX);
        }
    }
    fn record_debug(&mut self, _field: &Field, _value: &dyn std::fmt::Debug) {}
}

impl<S: tracing::Subscriber> Layer<S> for UploadLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let Some(tx) = &self.tx else { return };
        let meta = event.metadata();
        let mut f = Fields::default();
        event.record(&mut f);
        let stufe = Level::from_tracing(meta.level());
        let art = match f.art {
            Some(a) => a,
            None => match stufe {
                Level::Error => "log.error".to_string(),
                Level::Warn => "log.warn".to_string(),
                _ => return,
            },
        };
        let Some(modul) = module_from_target(meta.target()) else {
            return;
        };
        if let Some(e) = Event::new(stufe, &modul, &art, f.wert) {
            let _ = tx.try_send(Msg::Event(e));
        }
    }
}
