//! Feedback dialog state and the background sender (window-independent, testable).

use std::path::Path;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use serde::{Deserialize, Serialize};
use woa_telemetry::{
    DEFAULT_INGEST_URL, FeedbackError, FeedbackMeta, UploadConfig, new_install_id, submit_feedback,
};

/// Message shown when no endpoint is configured.
pub const NOT_CONFIGURED: &str = "Server noch nicht eingerichtet";

/// App configuration (`config.json` in the config folder). Missing file = defaults.
/// Eigener Feedback-Server (bretteins.infra, Instanz `daw.woa` auf lfs001).
pub const DEFAULT_FEEDBACK_URL: &str = "https://feedback.wironair.de/api/feedback";

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct GuiConfig {
    /// Feedback endpoint; default is our own server. Empty string in `config.json` = off.
    #[serde(skip_serializing_if = "is_default_feedback_url")]
    pub feedback_url: String,
    /// Anonymous random install id (UUID), sent with feedback and log uploads.
    pub install_id: String,
    /// Opt-in for anonymous error codes. `None` = not asked yet (dialog on start).
    pub telemetry_enabled: Option<bool>,
    /// Log endpoint; default is our own server.
    #[serde(skip_serializing_if = "is_default_telemetry_url")]
    pub telemetry_url: String,
}

fn is_default_feedback_url(s: &str) -> bool {
    s == DEFAULT_FEEDBACK_URL
}

fn is_default_telemetry_url(s: &str) -> bool {
    s == DEFAULT_INGEST_URL
}

impl Default for GuiConfig {
    fn default() -> Self {
        Self {
            feedback_url: DEFAULT_FEEDBACK_URL.to_string(),
            install_id: String::new(),
            telemetry_enabled: None,
            telemetry_url: DEFAULT_INGEST_URL.to_string(),
        }
    }
}

impl GuiConfig {
    /// Creates a random install id if none is set. `true` = changed (save it).
    pub fn ensure_install_id(&mut self) -> bool {
        if self.install_id.trim().is_empty() {
            self.install_id = new_install_id();
            true
        } else {
            false
        }
    }

    /// Upload settings for [`woa_telemetry::init`]. Without opt-in `enabled` is false,
    /// so no thread and no network.
    pub fn upload_config(&self) -> UploadConfig {
        UploadConfig {
            enabled: self.telemetry_enabled == Some(true),
            url: self.telemetry_url.clone(),
            install_id: self.install_id.clone(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            ..UploadConfig::default()
        }
    }

    /// Writes `config.json` (creates the folder).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, text)
    }

    /// Reads `path`; a missing or broken file yields defaults (logged).
    pub fn load(path: &Path) -> Self {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                tracing::warn!(error = %e, "config unreadable, using defaults");
                return Self::default();
            }
        };
        serde_json::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "config invalid, using defaults");
            Self::default()
        })
    }
}

/// Where the dialog is.
#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    /// Closed.
    Closed,
    /// Waiting for the screenshot requested from the viewport.
    Capturing,
    /// Editing text.
    Editing,
    /// Request running in the background.
    Sending,
    /// Finished with a message (success or error).
    Done(String),
}

/// The feedback dialog.
pub struct FeedbackDialog {
    /// Current phase.
    pub phase: Phase,
    /// The user's text.
    pub text: String,
    /// Attach the screenshot.
    pub attach: bool,
    /// Captured PNG, if any.
    pub screenshot: Option<Vec<u8>>,
    pending: Option<Receiver<Result<Option<u64>, FeedbackError>>>,
}

impl Default for FeedbackDialog {
    fn default() -> Self {
        Self {
            phase: Phase::Closed,
            text: String::new(),
            attach: true,
            screenshot: None,
            pending: None,
        }
    }
}

impl FeedbackDialog {
    /// Opens the dialog; the UI must request a screenshot and call [`Self::set_screenshot`].
    pub fn open(&mut self) {
        *self = Self {
            phase: Phase::Capturing,
            ..Self::default()
        };
    }

    /// Stores the captured screenshot (or `None` if capture failed) and starts editing.
    pub fn set_screenshot(&mut self, png: Option<Vec<u8>>) {
        self.screenshot = png;
        if self.phase == Phase::Capturing {
            self.phase = Phase::Editing;
        }
    }

    /// Closes the dialog.
    pub fn close(&mut self) {
        *self = Self::default();
    }

    /// Sends in a background thread. Empty URL: no request, message [`NOT_CONFIGURED`].
    pub fn send(&mut self, config: &GuiConfig) {
        if config.feedback_url.trim().is_empty() {
            self.phase = Phase::Done(NOT_CONFIGURED.into());
            return;
        }
        if self.text.trim().is_empty() {
            self.phase = Phase::Done("Bitte zuerst einen Text schreiben".into());
            return;
        }
        let meta = FeedbackMeta {
            url: config.feedback_url.clone(),
            install_id: config.install_id.clone(),
            ..FeedbackMeta::default()
        };
        let text = self.text.clone();
        let png = if self.attach {
            self.screenshot.clone()
        } else {
            None
        };
        let (tx, rx) = channel();
        let spawned = std::thread::Builder::new()
            .name("woa-feedback".into())
            .spawn(move || {
                // Receiver gone = dialog closed; nothing to do.
                let _ = tx.send(submit_feedback(&text, png, &meta));
            });
        match spawned {
            Ok(_) => {
                self.pending = Some(rx);
                self.phase = Phase::Sending;
            }
            Err(e) => {
                tracing::error!(error = %e, "feedback thread failed");
                self.phase = Phase::Done(format!("Senden fehlgeschlagen: {e}"));
            }
        }
    }

    /// Polls the background request; call every frame.
    pub fn poll(&mut self) {
        let Some(rx) = &self.pending else { return };
        let result = match rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(FeedbackError::Transport("abgebrochen".into())),
        };
        self.pending = None;
        self.phase = Phase::Done(match result {
            Ok(Some(n)) => format!("Danke! Gemeldet als #{n}"),
            Ok(None) => "Danke! Gemeldet".into(),
            Err(FeedbackError::NoEndpoint) => NOT_CONFIGURED.into(),
            Err(e) => {
                tracing::warn!(error = %e, "feedback failed");
                format!("Senden fehlgeschlagen: {e}")
            }
        });
    }
}

/// Encodes an RGBA8 image as PNG.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, png::EncodingError> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header()?;
        writer.write_image_data(rgba)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_url_reports_not_configured_without_request() {
        let mut d = FeedbackDialog::default();
        d.open();
        assert_eq!(d.phase, Phase::Capturing);
        d.set_screenshot(None);
        assert_eq!(d.phase, Phase::Editing);
        d.text = "Knopf klemmt".into();
        let off = GuiConfig {
            feedback_url: String::new(),
            ..GuiConfig::default()
        };
        d.send(&off);
        assert_eq!(d.phase, Phase::Done(NOT_CONFIGURED.into()));
    }

    #[test]
    fn default_points_to_own_server() {
        assert_eq!(GuiConfig::default().feedback_url, DEFAULT_FEEDBACK_URL);
    }

    #[test]
    fn empty_text_is_refused() {
        let mut d = FeedbackDialog::default();
        d.open();
        d.set_screenshot(None);
        let cfg = GuiConfig {
            feedback_url: "http://127.0.0.1:9/api/feedback".into(),
            ..GuiConfig::default()
        };
        d.send(&cfg);
        assert!(matches!(d.phase, Phase::Done(ref m) if m.contains("Text")));
    }

    #[test]
    fn unreachable_server_ends_in_error_message() {
        let mut d = FeedbackDialog::default();
        d.open();
        d.set_screenshot(None);
        d.text = "Test".into();
        // Port 9 (discard) on localhost: connection refused quickly.
        let cfg = GuiConfig {
            feedback_url: "http://127.0.0.1:9/api/feedback".into(),
            ..GuiConfig::default()
        };
        d.send(&cfg);
        assert_eq!(d.phase, Phase::Sending);
        for _ in 0..300 {
            d.poll();
            if d.phase != Phase::Sending {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(matches!(d.phase, Phase::Done(ref m) if m.starts_with("Senden fehlgeschlagen")));
    }

    #[test]
    fn config_load_defaults_and_parse() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("config.json");
        assert_eq!(GuiConfig::load(&p), GuiConfig::default());
        std::fs::write(&p, r#"{"feedback_url":"https://x/api/feedback"}"#).unwrap();
        assert_eq!(GuiConfig::load(&p).feedback_url, "https://x/api/feedback");
        std::fs::write(&p, "kaputt").unwrap();
        assert_eq!(GuiConfig::load(&p), GuiConfig::default());
    }

    #[test]
    fn telemetry_is_opt_in_and_persisted() {
        let mut c = GuiConfig::default();
        assert_eq!(c.telemetry_enabled, None);
        assert!(!c.upload_config().enabled);
        assert_eq!(c.upload_config().url, DEFAULT_INGEST_URL);
        assert!(c.ensure_install_id());
        assert!(!c.ensure_install_id());
        c.telemetry_enabled = Some(true);
        assert!(c.upload_config().enabled);
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("sub/config.json");
        c.save(&p).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        // Defaults are not pinned in the file.
        assert!(!text.contains("feedback_url"), "{text}");
        assert_eq!(GuiConfig::load(&p), c);
    }

    #[test]
    fn png_encoding_has_magic() {
        let png = encode_png(2, 1, &[255; 8]).unwrap();
        assert!(png.starts_with(b"\x89PNG"));
    }
}
