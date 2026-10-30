//! Logging, Upload, Feedback.
//!
//! - [`init`] richtet `tracing` ein: JSON-Logdatei je Tag im Datenverzeichnis, 30 Tage
//!   Aufbewahrung, Panic-Hook. Optional hängt die Upload-Senke ([`upload`]) mit dran.
//! - [`upload`] schickt gebündelt nur Ereigniscode, Stufe und Zahl an `/ingest`
//!   (Wire-Vertrag in `docs/telemetrie.md`), nie Freitext oder Pfade.
//! - [`feedback`] schickt Text plus Screenshot an `/api/feedback` desselben Servers.
//!
//! Nichts davon darf im Audio-Thread laufen: HTTP läuft in einem eigenen Thread, das
//! Erfassen eines Ereignisses ist ein `try_send` auf eine begrenzte Queue.

pub mod feedback;
pub mod scrub;
pub mod upload;
pub mod watch;

use std::path::{Path, PathBuf};

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

pub use feedback::{FeedbackError, FeedbackMeta, FeedbackType, submit_feedback};
pub use upload::{
    DEFAULT_INGEST_URL, Event, Level, UploadConfig, UploadLayer, Uploader, new_install_id,
};
pub use watch::FrameWatch;

/// Tage, die Logdateien liegen bleiben (eine Datei je Tag).
pub const LOG_RETENTION_DAYS: usize = 30;

/// Prefix der Logdateien: `woa.JJJJ-MM-TT.log`.
pub const LOG_PREFIX: &str = "woa";

/// Fehler beim Einrichten des Loggings.
#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("log directory {0:?} not usable: {1}")]
    Dir(PathBuf, std::io::Error),
    #[error("rolling appender: {0}")]
    Appender(String),
    #[error("global subscriber already set: {0}")]
    Subscriber(String),
}

/// Hält Datei-Writer und Uploader am Leben; beim Drop wird geflusht und hochgeladen.
pub struct Telemetry {
    _file_guard: WorkerGuard,
    uploader: Option<Uploader>,
}

impl Telemetry {
    /// Die Upload-Senke, falls Opt-in und Ziel-URL gesetzt sind.
    pub fn uploader(&self) -> Option<&Uploader> {
        self.uploader.as_ref()
    }
}

/// Baut den täglich rotierenden Datei-Appender mit 30 Tagen Aufbewahrung.
pub fn file_appender(log_dir: &Path) -> Result<RollingFileAppender, InitError> {
    std::fs::create_dir_all(log_dir).map_err(|e| InitError::Dir(log_dir.to_path_buf(), e))?;
    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_PREFIX)
        .filename_suffix("log")
        .max_log_files(LOG_RETENTION_DAYS)
        .build(log_dir)
        .map_err(|e| InitError::Appender(e.to_string()))
}

/// Richtet das globale Logging ein. `upload` ist `None` oder deaktiviert, solange der
/// Nutzer nicht zugestimmt hat (Opt-in). Filter per `WOA_LOG`, Standard `info`.
pub fn init(log_dir: &Path, upload: Option<UploadConfig>) -> Result<Telemetry, InitError> {
    let appender = file_appender(log_dir)?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter = EnvFilter::try_from_env("WOA_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let file_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_writer(writer)
        .with_filter(filter);

    let uploader = upload.and_then(Uploader::start);
    let upload_layer = uploader.as_ref().map(|u| u.layer());

    tracing_subscriber::registry()
        .with(file_layer)
        .with(upload_layer)
        .try_init()
        .map_err(|e| InitError::Subscriber(e.to_string()))?;

    install_panic_hook();
    Ok(Telemetry {
        _file_guard: guard,
        uploader,
    })
}

/// Panic-Hook: schreibt den Absturz samt Ort und Backtrace ins Log und erzeugt das
/// Upload-Ereignis `panic` (ohne Nachricht, die könnte Pfade oder Inhalte tragen).
/// Der vorige Hook (Standardausgabe) läuft danach weiter.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let backtrace = std::backtrace::Backtrace::force_capture();
        // Lokale Datei: volle Information. Upload: nur `art` und `modul` (Layer filtert).
        tracing::error!(
            target: "woa::panic",
            art = "panic",
            wert = 1u64,
            location = %location,
            message = %message,
            backtrace = %backtrace,
            "panic"
        );
        previous(info);
    }));
}
