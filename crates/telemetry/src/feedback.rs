//! Feedback-Client: Text plus optionaler Screenshot an `POST <url>` (`/api/feedback`).
//!
//! Nutzlast wie bretteins (`feedback_req.rs::baue`, Server `handle_feedback`):
//!
//! ```json
//! {"kind":"feedback","title":"…","body":"…\n\nInstall: <id>","type":"bug","trust":"L0",
//!  "app":"0.1.0","appVersion":"0.1.0","os":"linux","installId":"…","sentAt":1690000000000,
//!  "images":[{"name":"screenshot.png","content_b64":"…"}]}
//! ```
//!
//! Die Anfrage ist eine Whitelist: nur diese Felder gehen raus. Der Text ist hier
//! bewusst Freitext — der Nutzer schreibt ihn selbst und schickt ihn ausdrücklich ab.
//! Blockierend; nie aus dem Audio- oder GUI-Thread aufrufen.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde_json::{Value, json};

/// Server-Grenze für Screenshots (bretteins `IMG_MAX_FILE`, 3 MB).
pub const MAX_SCREENSHOT_BYTES: usize = 3 * 1024 * 1024;
/// Höchstens so viele Bilder je Meldung (Bildschirmfoto und ein eigenes Bild).
pub const MAX_IMAGES: usize = 2;
/// Server-Grenze für den Text (bretteins kürzt bei 20000 Zeichen).
pub const MAX_TEXT_CHARS: usize = 20_000;

/// Art des Feedbacks (Server-Whitelist bug|idee|frage|lob, sonst `feedback`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FeedbackType {
    Bug,
    Idee,
    Frage,
    Lob,
    #[default]
    Allgemein,
}

impl FeedbackType {
    fn wire(self) -> &'static str {
        match self {
            FeedbackType::Bug => "bug",
            FeedbackType::Idee => "idee",
            FeedbackType::Frage => "frage",
            FeedbackType::Lob => "lob",
            FeedbackType::Allgemein => "",
        }
    }
}

/// Begleitdaten einer Meldung.
#[derive(Debug, Clone)]
pub struct FeedbackMeta {
    /// Feedback-Endpunkt, z. B. `https://example.org/api/feedback`. Leer = aus.
    pub url: String,
    pub kind: FeedbackType,
    pub install_id: String,
    pub app_version: String,
    pub os: String,
    pub timeout: Duration,
}

impl Default for FeedbackMeta {
    fn default() -> Self {
        Self {
            url: String::new(),
            kind: FeedbackType::default(),
            install_id: String::new(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
            timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FeedbackError {
    #[error("feedback endpoint not configured")]
    NoEndpoint,
    #[error("feedback text is empty")]
    EmptyText,
    #[error("screenshot too large ({0} bytes)")]
    ScreenshotTooLarge(usize),
    #[error("screenshot is not a PNG")]
    NotPng,
    #[error("too many images ({0})")]
    TooManyImages(usize),
    #[error("server answered {0}")]
    Status(u16),
    #[error("transport: {0}")]
    Transport(String),
}

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Baut die Nutzlast (rein, ohne Netz; für Tests sichtbar).
/// `images`: (Dateiname, PNG-Bytes), höchstens [`MAX_IMAGES`], je höchstens
/// [`MAX_SCREENSHOT_BYTES`].
pub fn build_payload(
    text: &str,
    images: &[(String, Vec<u8>)],
    meta: &FeedbackMeta,
) -> Result<Value, FeedbackError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(FeedbackError::EmptyText);
    }
    let text: String = text.chars().take(MAX_TEXT_CHARS).collect();
    let title: String = text
        .lines()
        .next()
        .unwrap_or("Feedback")
        .chars()
        .take(80)
        .collect();
    let body = format!("{}\n\nInstall: {}", text, meta.install_id);
    let sent_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(0))
        .unwrap_or(0);
    let mut req = json!({
        "kind": "feedback",
        "title": title,
        "body": body,
        "type": meta.kind.wire(),
        "trust": "L0",
        "app": meta.app_version,
        "appVersion": meta.app_version,
        "os": meta.os,
        "installId": meta.install_id,
        "sentAt": sent_at,
    });
    if images.len() > MAX_IMAGES {
        return Err(FeedbackError::TooManyImages(images.len()));
    }
    let mut list = Vec::new();
    for (name, png) in images {
        if png.len() > MAX_SCREENSHOT_BYTES {
            return Err(FeedbackError::ScreenshotTooLarge(png.len()));
        }
        if !png.starts_with(PNG_MAGIC) {
            return Err(FeedbackError::NotPng);
        }
        let b64 = base64::engine::general_purpose::STANDARD.encode(png);
        list.push(json!({ "name": name, "content_b64": b64 }));
    }
    if !list.is_empty() {
        req["images"] = Value::Array(list);
    }
    Ok(req)
}

/// Schickt eine Meldung ab. Liefert die Issue-Nummer, wenn der Server sie nennt.
pub fn submit_feedback(
    text: &str,
    images: Vec<(String, Vec<u8>)>,
    meta: &FeedbackMeta,
) -> Result<Option<u64>, FeedbackError> {
    if meta.url.trim().is_empty() {
        return Err(FeedbackError::NoEndpoint);
    }
    let payload = build_payload(text, &images, meta)?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(meta.timeout))
        .http_status_as_error(false)
        .build()
        .into();
    let body = payload.to_string();
    let mut resp = agent
        .post(&meta.url)
        .header("Content-Type", "application/json")
        .send(body)
        .map_err(|e| FeedbackError::Transport(e.to_string()))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        tracing::warn!(
            art = "feedback.http_error",
            wert = i64::from(status),
            "feedback rejected"
        );
        return Err(FeedbackError::Status(status));
    }
    let answer: Value = resp
        .body_mut()
        .read_to_string()
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null);
    Ok(answer.get("issue").and_then(Value::as_u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> FeedbackMeta {
        FeedbackMeta {
            url: "http://x".into(),
            kind: FeedbackType::Bug,
            install_id: "abc".into(),
            app_version: "0.1.0".into(),
            os: "linux".into(),
            ..FeedbackMeta::default()
        }
    }

    #[test]
    fn payload_whitelist() {
        let p = build_payload("Knackser\nim Kanal 2", &[], &meta()).unwrap();
        let obj = p.as_object().unwrap();
        let mut keys: Vec<_> = obj.keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "app",
                "appVersion",
                "body",
                "installId",
                "kind",
                "os",
                "sentAt",
                "title",
                "trust",
                "type"
            ]
        );
        assert_eq!(p["title"], "Knackser");
        assert_eq!(p["type"], "bug");
        assert_eq!(p["body"], "Knackser\nim Kanal 2\n\nInstall: abc");
    }

    #[test]
    fn rejects_empty_and_non_png() {
        assert!(matches!(
            build_payload("  ", &[], &meta()),
            Err(FeedbackError::EmptyText)
        ));
        assert!(matches!(
            build_payload("x", &[("a.gif".into(), b"GIF89a".to_vec())], &meta()),
            Err(FeedbackError::NotPng)
        ));
        let png = b"\x89PNG\r\n\x1a\n".to_vec();
        let three = vec![("a.png".to_string(), png); 3];
        assert!(matches!(
            build_payload("x", &three, &meta()),
            Err(FeedbackError::TooManyImages(3))
        ));
    }
}
