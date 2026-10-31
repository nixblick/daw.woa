//! Feedback dialog state and the background sender (window-independent, testable).

use std::path::Path;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use woa_telemetry::feedback::{MAX_SCREENSHOT_BYTES, MAX_TEXT_CHARS};
use woa_telemetry::{
    DEFAULT_INGEST_URL, FeedbackError, FeedbackMeta, UploadConfig, new_install_id, submit_feedback,
};

/// Message shown when no endpoint is configured.
pub const NOT_CONFIGURED: &str = "Server noch nicht eingerichtet";

/// App configuration (`config.json` in the config folder). Missing file = defaults.
/// Eigener Feedback-Server (eigener Server).
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
    /// Layout: Produzent (Zeitleiste) oder Radio (Sendeablauf).
    pub mode: Mode,
    /// Farbthema: Dunkel (Vorgabe) oder Hell. Die Modusfarbe folgt dem Layout.
    pub theme: crate::theme::Theme,
    /// Pegelansicht im Produzenten-Modus: Analog (VU), Modern (Balken) oder Aus.
    pub vu_view: crate::widgets::VuView,
    /// Sendung an den eigenen Shoutcast (liest auch woa-streamd). Ohne Kennwort.
    pub stream: woa_stream::StreamConfig,
    /// KI & MCP: WOA als MCP-Server (später der Assistent). Ohne Geheimnisse.
    #[serde(skip_serializing_if = "woa_assistant::KiConfig::is_default")]
    pub ki: woa_assistant::KiConfig,
    /// Puffergröße der Radio-Ausgabe in Frames. Fehlt oder 0 = Vorgabe des Treibers
    /// ([`woa_engine::RADIO_BUFFER_FRAMES`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radio_buffer_frames: Option<u32>,
    /// Gewählter MIDI-Eingang (Portname), `None` = aus.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub midi_input: Option<String>,
    /// Gewählter Controller (Paket 14), `None` = aus.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub controller_input: Option<String>,
    /// MIDI-Learn-Zuordnung je Controller (Gerätename ohne ALSA-Nummern).
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub controller_maps: std::collections::BTreeMap<String, Vec<woa_io::controller::Mapping>>,
    /// Ausgang für das Vorhören (PFL, Paket 15). Vorgabe: aus.
    #[serde(skip_serializing_if = "PflOutput::is_off")]
    pub pfl_output: PflOutput,
    /// Ausgänge je Gerät: Gerät und Kanalpaar je Quelle. Vorgabe: alles auf Standardgerät, Kanal 1/2.
    #[serde(skip_serializing_if = "woa_radio::routing::OutputMap::is_default")]
    pub outputs: woa_radio::routing::OutputMap,
    /// Telefon-Eingang (Anrufer): Gerät und Kanalpaar. Vorgabe: keiner.
    #[serde(skip_serializing_if = "crate::phone::PhoneInput::is_default")]
    pub phone: crate::phone::PhoneInput,
    /// Wunschordner (Wunschtitel). Vorgabe: `Wunschtitel` im Datenordner der Sendung.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wish_folder: Option<std::path::PathBuf>,
    /// Beim Start höchstens einmal am Tag nach einem neuen Release suchen. Vorgabe: an.
    pub auto_update: bool,
    /// Unix-Sekunden der letzten erfolgreichen Update-Suche.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_update_check: Option<u64>,
}

/// Wohin das Vorhören (PFL) geht. Nie auf Kanal 1/2 der Sendung.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PflOutput {
    #[default]
    Aus,
    /// Kanal 3/4 des Sendegeräts (Interface mit mindestens vier Ausgängen).
    Kanal34,
    /// Ein zweites Gerät (Kennung aus `list_output_devices`).
    Geraet(String),
}

impl PflOutput {
    pub fn is_off(&self) -> bool {
        *self == PflOutput::Aus
    }
}

impl GuiConfig {
    /// Puffergröße für die Radio-Ausgabe (`None` = Treiber entscheidet).
    pub fn radio_buffer(&self) -> Option<u32> {
        let n = self
            .radio_buffer_frames
            .unwrap_or(woa_engine::RADIO_BUFFER_FRAMES);
        (n > 0).then_some(n)
    }
}

/// Arbeitsmodus der Oberfläche.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Produzent,
    Radio,
    /// Party-Layout (Paket 35): spielt über denselben Sendemixer wie das Radio.
    Party,
}

impl Mode {
    /// Nutzt den Sendemixer (Radio und Party) statt des Projekt-Players.
    pub fn uses_desk(self) -> bool {
        matches!(self, Mode::Radio | Mode::Party)
    }
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
            mode: Mode::Produzent,
            theme: crate::theme::Theme::default(),
            vu_view: crate::widgets::VuView::default(),
            stream: woa_stream::StreamConfig::default(),
            ki: woa_assistant::KiConfig::default(),
            radio_buffer_frames: None,
            midi_input: None,
            controller_input: None,
            controller_maps: std::collections::BTreeMap::new(),
            pfl_output: PflOutput::Aus,
            outputs: woa_radio::routing::OutputMap::default(),
            phone: crate::phone::PhoneInput::default(),
            wish_folder: None,
            auto_update: true,
            last_update_check: None,
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
        let mut cfg: Self = serde_json::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "config invalid, using defaults");
            Self::default()
        });
        cfg.stream = cfg.stream.migrate();
        cfg
    }
}

/// How long the thank-you after a successful send stays before the dialog closes itself.
pub const THANKS_VISIBLE: Duration = Duration::from_secs(3);

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
    /// Attach the screenshot. Ticking it takes the screenshot (dialog hidden meanwhile).
    pub attach: bool,
    /// Captured PNG, if any.
    pub screenshot: Option<Vec<u8>>,
    /// The viewport screenshot was requested (one frame after the dialog vanished).
    pub capture_requested: bool,
    /// Own image chosen by the user: file name and PNG bytes.
    pub image: Option<(String, Vec<u8>)>,
    /// Send the session log (Protokoll window). On by default, again on every open (#211).
    pub send_log: bool,
    pending: Option<Receiver<Result<Option<u64>, FeedbackError>>>,
    /// Set when the send succeeded; the dialog closes [`THANKS_VISIBLE`] later.
    thanks_since: Option<Instant>,
}

impl Default for FeedbackDialog {
    fn default() -> Self {
        Self {
            phase: Phase::Closed,
            text: String::new(),
            attach: false,
            screenshot: None,
            capture_requested: false,
            image: None,
            send_log: true,
            pending: None,
            thanks_since: None,
        }
    }
}

/// Width of the small preview below the attachments (#211).
pub const PREVIEW_WIDTH: u32 = 160;

/// Small preview of a PNG, [`PREVIEW_WIDTH`] wide (narrower images keep their size).
pub fn thumbnail(png: &[u8]) -> Option<eframe::egui::ColorImage> {
    let img = image::load_from_memory(png).ok()?;
    let img = if img.width() > PREVIEW_WIDTH {
        img.resize(
            PREVIEW_WIDTH,
            u32::MAX,
            image::imageops::FilterType::Triangle,
        )
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Some(eframe::egui::ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    ))
}

/// Ctrl+Enter (Cmd+Enter on macOS) in the dialog: send with the ticked boxes (#211).
/// Consumes the key so the text field does not add a line break.
pub fn send_shortcut(input: &mut eframe::egui::InputState) -> bool {
    input.consume_key(eframe::egui::Modifiers::COMMAND, eframe::egui::Key::Enter)
}

/// Header line between the user's text and the attached log.
pub const LOG_HEADER: &str = "--- Protokoll ---";

/// Replaces the home folder (`/home/name`, `C:\\Users\\name`) by `~`, in both slash styles.
pub fn shorten_home(text: &str, home: Option<&Path>) -> String {
    let Some(home) = home.map(|h| {
        h.to_string_lossy()
            .trim_end_matches(['/', '\\'])
            .to_string()
    }) else {
        return text.to_string();
    };
    if home.len() < 2 {
        return text.to_string();
    }
    let mut out = text.to_string();
    for h in [
        home.clone(),
        home.replace('\\', "/"),
        home.replace('/', "\\"),
    ] {
        out = out.replace(&h, "~");
    }
    out
}

/// The user's home folder from the environment.
pub fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|h| !h.is_empty())
        .map(std::path::PathBuf::from)
}

/// Log lines (oldest first) as text, home folder shortened, newest lines kept so that
/// the result fits into `budget` characters.
pub fn log_text(lines: &[String], home: Option<&Path>, budget: usize) -> String {
    let mut kept: Vec<String> = Vec::new();
    let mut used = 0;
    for line in lines.iter().rev() {
        let l = shorten_home(line, home);
        let n = l.chars().count() + 1;
        if used + n > budget {
            break;
        }
        used += n;
        kept.push(l);
    }
    kept.reverse();
    kept.join("\n")
}

/// Turns any PNG/JPEG into a PNG within the server limit (downscaled if needed).
pub fn image_to_png(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("Bild nicht lesbar: {e}"))?;
    fit_png(img)
}

fn fit_png(mut img: image::DynamicImage) -> Result<Vec<u8>, String> {
    for _ in 0..4 {
        let rgba = img.to_rgba8();
        let png =
            encode_png(rgba.width(), rgba.height(), rgba.as_raw()).map_err(|e| e.to_string())?;
        if png.len() <= MAX_SCREENSHOT_BYTES {
            return Ok(png);
        }
        let (w, h) = (img.width() * 7 / 10, img.height() * 7 / 10);
        img = img.resize(w.max(1), h.max(1), image::imageops::FilterType::Triangle);
    }
    Err("Bild zu groß (höchstens 3 MB)".into())
}

impl FeedbackDialog {
    /// Opens the dialog for editing. No screenshot yet: it is taken when the user ticks the box.
    pub fn open(&mut self) {
        *self = Self {
            phase: Phase::Editing,
            ..Self::default()
        };
    }

    /// Hides the dialog and asks for a screenshot (the UI sends the viewport command
    /// on the next frame, see `capture_requested`).
    pub fn request_screenshot(&mut self) {
        self.phase = Phase::Capturing;
        self.capture_requested = false;
    }

    /// Stores the captured screenshot (or `None` if capture failed) and returns to editing.
    pub fn set_screenshot(&mut self, png: Option<Vec<u8>>) {
        let png = png.and_then(|p| {
            if p.len() <= MAX_SCREENSHOT_BYTES {
                Some(p)
            } else {
                image::load_from_memory(&p)
                    .ok()
                    .and_then(|i| fit_png(i).ok())
            }
        });
        self.attach = png.is_some();
        self.screenshot = png;
        self.capture_requested = false;
        if self.phase == Phase::Capturing {
            self.phase = Phase::Editing;
        }
    }

    /// Unticks the screenshot and drops it.
    pub fn remove_screenshot(&mut self) {
        self.attach = false;
        self.screenshot = None;
    }

    /// Attaches the user's own image (PNG or JPEG).
    pub fn set_image(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let png = image_to_png(bytes)?;
        let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
        let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
        let stem = if stem.trim().is_empty() { "bild" } else { stem };
        self.image = Some((format!("{stem}.png"), png));
        Ok(())
    }

    /// Removes the user's own image.
    pub fn remove_image(&mut self) {
        self.image = None;
    }

    /// What gets a preview: the ticked screenshot and the own image, as (key, PNG).
    pub fn previews(&self) -> Vec<(&'static str, &[u8])> {
        let mut v = Vec::new();
        if self.attach
            && let Some(s) = &self.screenshot
        {
            v.push(("screenshot", s.as_slice()));
        }
        if let Some((_, png)) = &self.image {
            v.push(("image", png.as_slice()));
        }
        v
    }

    /// Images that go out: only what the user ticked or attached.
    pub fn images(&self) -> Vec<(String, Vec<u8>)> {
        let mut v = Vec::new();
        if self.attach
            && let Some(s) = &self.screenshot
        {
            v.push(("screenshot.png".to_string(), s.clone()));
        }
        if let Some(i) = &self.image {
            v.push(i.clone());
        }
        v
    }

    /// Message body: the text, plus the log below [`LOG_HEADER`] only if ticked.
    /// `log_lines` are the Protokoll lines, oldest first. Fits [`MAX_TEXT_CHARS`].
    pub fn body(&self, log_lines: &[String], home: Option<&Path>) -> String {
        let text = self.text.trim().to_string();
        if !self.send_log || log_lines.is_empty() {
            return text;
        }
        // Room for the install line the payload adds.
        let budget = MAX_TEXT_CHARS.saturating_sub(text.chars().count() + LOG_HEADER.len() + 200);
        let log = log_text(log_lines, home, budget);
        if log.is_empty() {
            return text;
        }
        format!("{text}\n\n{LOG_HEADER}\n{log}")
    }

    /// Short list of what goes out, for the line above the send button.
    pub fn outgoing(&self, log_lines: usize) -> Vec<String> {
        let mut v = vec!["dein Text".to_string()];
        if self.attach && self.screenshot.is_some() {
            v.push("Bildschirmfoto".into());
        }
        if let Some((name, png)) = &self.image {
            v.push(format!("Bild {name} ({} KB)", png.len().div_ceil(1024)));
        }
        if self.send_log {
            v.push(format!("Protokoll ({log_lines} Zeilen)"));
        }
        v.push("Version, System, Installationskennung".into());
        v
    }

    /// Closes the dialog.
    pub fn close(&mut self) {
        *self = Self::default();
    }

    /// Sends in a background thread. Empty URL: no request, message [`NOT_CONFIGURED`].
    pub fn send(&mut self, config: &GuiConfig, log_lines: &[String]) {
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
        let text = self.body(log_lines, home_dir().as_deref());
        let images = self.images();
        let (tx, rx) = channel();
        let spawned = std::thread::Builder::new()
            .name("woa-feedback".into())
            .spawn(move || {
                // Receiver gone = dialog closed; nothing to do.
                let _ = tx.send(submit_feedback(&text, images, &meta));
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
        if result.is_ok() {
            self.thanks_since = Some(Instant::now());
        }
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

impl FeedbackDialog {
    /// True after a successful send (the message is a thank-you, not an error).
    pub fn is_thanks(&self) -> bool {
        self.thanks_since.is_some()
    }

    /// Time left until the thank-you closes itself, `None` if nothing is pending.
    pub fn thanks_left(&self, now: Instant) -> Option<Duration> {
        self.thanks_since
            .map(|t| THANKS_VISIBLE.saturating_sub(now.saturating_duration_since(t)))
    }

    /// Closes the dialog once the thank-you has been visible long enough; call every frame.
    pub fn tick(&mut self, now: Instant) {
        if self.thanks_left(now) == Some(Duration::ZERO) {
            self.close();
        }
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
    fn output_map_is_saved_only_when_set_and_old_configs_load() {
        use woa_radio::routing::{Route, Source};
        let mut cfg = GuiConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(!json.contains("outputs"), "{json}");
        let old: GuiConfig = serde_json::from_str(&json).unwrap();
        assert!(old.outputs.is_default());
        cfg.outputs.set(
            Source::Carts,
            Route {
                device_id: Some("card2".into()),
                device_name: "Karte 2".into(),
                pair: 1,
            },
        );
        let back: GuiConfig = serde_json::from_str(&serde_json::to_string(&cfg).unwrap()).unwrap();
        assert_eq!(back.outputs.get(Source::Carts).pair, 1);
        assert_eq!(back.outputs.get(Source::Playlist), Route::default());
    }

    #[test]
    fn thanks_close_after_three_seconds_errors_stay() {
        let t0 = Instant::now();
        let mut d = FeedbackDialog {
            phase: Phase::Done("Danke! Gemeldet".into()),
            thanks_since: Some(t0),
            ..Default::default()
        };
        d.tick(t0 + Duration::from_millis(2900));
        assert!(matches!(d.phase, Phase::Done(_)));
        d.tick(t0 + THANKS_VISIBLE);
        assert_eq!(d.phase, Phase::Closed);
        // An error never closes by itself.
        let mut e = FeedbackDialog {
            phase: Phase::Done("Senden fehlgeschlagen: x".into()),
            ..Default::default()
        };
        e.tick(t0 + Duration::from_secs(60));
        assert!(matches!(e.phase, Phase::Done(_)));
    }

    #[test]
    fn empty_url_reports_not_configured_without_request() {
        let mut d = FeedbackDialog::default();
        d.open();
        assert_eq!(d.phase, Phase::Editing);
        d.text = "Knopf klemmt".into();
        let off = GuiConfig {
            feedback_url: String::new(),
            ..GuiConfig::default()
        };
        d.send(&off, &[]);
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
        d.send(&cfg, &[]);
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
        d.send(&cfg, &[]);
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
    fn old_stream_settings_are_migrated_and_never_hold_a_password() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("config.json");
        // Stand bis 0.34.28: Quell-Port in `port`, Kennwortdatei eingetragen.
        std::fs::write(
            &p,
            r#"{"stream":{"host":"radio.example","port":8001,"password_file":"/x/kw.pass"}}"#,
        )
        .unwrap();
        let c = GuiConfig::load(&p);
        assert_eq!(c.stream.port, 8000);
        assert_eq!(c.stream.source_port(), 8001);
        c.save(&p).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("\"schema\": 2"), "{text}");
        assert_eq!(
            GuiConfig::load(&p).stream.port,
            8000,
            "stabil nach Speichern"
        );
    }

    #[test]
    fn theme_defaults_to_dark_and_is_persisted() {
        use crate::theme::Theme;
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("config.json");
        // Alte Konfiguration ohne Thema: Dunkel.
        std::fs::write(&p, r#"{"mode":"radio"}"#).unwrap();
        assert_eq!(GuiConfig::load(&p).theme, Theme::Dunkel);
        let mut c = GuiConfig::load(&p);
        c.theme = Theme::Hell;
        c.save(&p).unwrap();
        assert_eq!(GuiConfig::load(&p).theme, Theme::Hell);
        assert_eq!(GuiConfig::load(&p).mode, Mode::Radio);
    }

    #[test]
    fn level_view_defaults_to_analog_and_is_persisted() {
        use crate::widgets::VuView;
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("config.json");
        std::fs::write(&p, r#"{"mode":"produzent"}"#).unwrap();
        assert_eq!(GuiConfig::load(&p).vu_view, VuView::Analog);
        let mut c = GuiConfig::load(&p);
        c.vu_view = VuView::Modern;
        c.save(&p).unwrap();
        assert_eq!(GuiConfig::load(&p).vu_view, VuView::Modern);
        std::fs::write(&p, r#"{"vu_view":"aus"}"#).unwrap();
        assert_eq!(GuiConfig::load(&p).vu_view, VuView::Aus);
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
    fn screenshot_on_click_set_and_removed() {
        let mut d = FeedbackDialog::default();
        d.open();
        assert!(
            !d.attach && d.images().is_empty(),
            "nothing before the click"
        );
        d.request_screenshot();
        assert_eq!(d.phase, Phase::Capturing);
        d.set_screenshot(Some(encode_png(2, 1, &[255; 8]).unwrap()));
        assert_eq!(d.phase, Phase::Editing);
        assert!(d.attach);
        assert_eq!(d.images()[0].0, "screenshot.png");
        d.remove_screenshot();
        assert!(d.images().is_empty());
    }

    #[test]
    fn own_image_set_and_removed() {
        let mut d = FeedbackDialog::default();
        d.open();
        // A JPEG becomes a PNG.
        let mut jpg = Vec::new();
        image::DynamicImage::new_rgb8(4, 3)
            .write_to(
                &mut std::io::Cursor::new(&mut jpg),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        d.set_image("C:\\x\\Foto vom Pult.jpg", &jpg).unwrap();
        let imgs = d.images();
        assert_eq!(imgs.len(), 1);
        assert_eq!(imgs[0].0, "Foto vom Pult.png");
        assert!(imgs[0].1.starts_with(b"\x89PNG"));
        assert!(
            d.outgoing(0)
                .iter()
                .any(|s| s.starts_with("Bild Foto vom Pult.png"))
        );
        d.remove_image();
        assert!(d.images().is_empty());
        assert!(d.set_image("kaputt.png", b"nope").is_err());
        assert!(d.image.is_none());
    }

    #[test]
    fn home_paths_are_shortened() {
        let home = Path::new("/home/robert");
        assert_eq!(
            shorten_home("lade /home/robert/Musik/a.mp3 fehlgeschlagen", Some(home)),
            "lade ~/Musik/a.mp3 fehlgeschlagen"
        );
        let win = Path::new("C:\\Users\\Robert");
        assert_eq!(
            shorten_home(
                "[C:\\Users\\Robert\\Music\\b.wav] und C:/Users/Robert/x",
                Some(win)
            ),
            "[~\\Music\\b.wav] und ~/x"
        );
        assert_eq!(shorten_home("/home/robert/a", None), "/home/robert/a");
    }

    #[test]
    fn log_goes_only_when_ticked() {
        let home = Path::new("/home/robert");
        let lines = vec!["12:00 Fehler /home/robert/a.mp3 kaputt".to_string()];
        let mut d = FeedbackDialog::default();
        d.open();
        d.text = "Knackt".into();
        d.send_log = false;
        assert_eq!(d.body(&lines, Some(home)), "Knackt");
        assert!(!d.outgoing(1).iter().any(|s| s.contains("Protokoll")));
        d.send_log = true;
        let b = d.body(&lines, Some(home));
        assert!(b.contains(LOG_HEADER) && b.contains("~/a.mp3"), "{b}");
        assert!(!b.contains("robert"), "{b}");
        // Too long: newest lines kept, whole body within the server limit.
        let many: Vec<String> = (0..5000)
            .map(|i| format!("Zeile {i} {}", "x".repeat(40)))
            .collect();
        let b = d.body(&many, None);
        assert!(b.chars().count() <= MAX_TEXT_CHARS);
        assert!(b.ends_with(&many[4999]));
    }

    #[test]
    fn png_encoding_has_magic() {
        let png = encode_png(2, 1, &[255; 8]).unwrap();
        assert!(png.starts_with(b"\x89PNG"));
    }

    #[test]
    fn log_box_is_ticked_on_every_open() {
        let mut d = FeedbackDialog::default();
        d.open();
        assert!(d.send_log, "Protokoll an beim Öffnen");
        d.send_log = false;
        d.close();
        d.open();
        assert!(d.send_log, "nach dem Neuöffnen wieder an");
    }

    #[test]
    fn preview_appears_after_image_and_is_small() {
        let mut d = FeedbackDialog::default();
        d.open();
        assert!(d.previews().is_empty());
        let rgba = vec![200u8; 800 * 400 * 4];
        let png = encode_png(800, 400, &rgba).unwrap();
        d.set_screenshot(Some(png.clone()));
        d.set_image("eigen.jpg", &png).unwrap();
        let p = d.previews();
        assert_eq!(
            p.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
            ["screenshot", "image"]
        );
        let t = thumbnail(p[0].1).unwrap();
        assert_eq!(t.size, [PREVIEW_WIDTH as usize, 80]);
        d.remove_screenshot();
        d.remove_image();
        assert!(d.previews().is_empty());
    }

    #[test]
    fn ctrl_enter_is_the_send_shortcut() {
        let ctx = eframe::egui::Context::default();
        let press = |modifiers| eframe::egui::RawInput {
            events: vec![eframe::egui::Event::Key {
                key: eframe::egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }],
            modifiers,
            ..Default::default()
        };
        let mut hit = false;
        let _ = ctx.run_ui(press(eframe::egui::Modifiers::COMMAND), |ui| {
            hit = ui.ctx().input_mut(send_shortcut);
        });
        assert!(hit, "Strg+Enter sendet");
        let _ = ctx.run_ui(press(eframe::egui::Modifiers::NONE), |ui| {
            hit = ui.ctx().input_mut(send_shortcut);
        });
        assert!(!hit, "Enter allein macht eine neue Zeile");
    }

    #[test]
    fn shortcut_send_carries_the_ticked_log_to_a_fake_server() {
        use std::io::{Read, Write};
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/api/feedback", server.local_addr().unwrap());
        let got = std::thread::spawn(move || {
            let (mut s, _) = server.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            while let Ok(n) = s.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                if String::from_utf8_lossy(&buf).contains("Zeile eins") {
                    break;
                }
            }
            let _ = s.write_all(b"HTTP/1.1 500 x\r\ncontent-length: 0\r\n\r\n");
            String::from_utf8_lossy(&buf).to_string()
        });
        let mut d = FeedbackDialog::default();
        d.open();
        d.text = "Knopf klemmt".into();
        let cfg = GuiConfig {
            feedback_url: url,
            ..GuiConfig::default()
        };
        // What the window does on Ctrl+Enter: the same send as the button.
        d.send(&cfg, &["Zeile eins".to_string()]);
        assert_eq!(d.phase, Phase::Sending);
        let req = got.join().unwrap();
        assert!(req.contains("Knopf klemmt"), "{req}");
        assert!(
            req.contains(LOG_HEADER) && req.contains("Zeile eins"),
            "{req}"
        );
    }
}
