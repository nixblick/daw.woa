//! Sprachmodell-Anbieter: Anthropic und alles, was die OpenAI-Schnittstelle spricht
//! (OpenAI, OpenRouter, Ollama, LM Studio, vLLM, …). Beide Formate werden auf dieselben
//! Nachrichten und Werkzeug-Aufrufe abgebildet, damit der Chat nichts über den Anbieter weiß.
//!
//! Der Schlüssel steht nur im Speicher dieses Werts; `Debug` zeigt ihn nie, und keine
//! Fehlermeldung nennt ihn.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Version der Anthropic-Schnittstelle.
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Obergrenze der Antwortlänge je Runde.
const MAX_TOKENS: u32 = 4096;

/// Welches Format der Anbieter spricht.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Anthropic,
    /// `/v1/chat/completions` mit `tools`: OpenAI und alle, die es nachbauen.
    #[default]
    OpenAiCompat,
}

impl Kind {
    pub const ALL: [Kind; 2] = [Kind::Anthropic, Kind::OpenAiCompat];

    pub fn label(self) -> &'static str {
        match self {
            Self::Anthropic => "Anthropic (Claude)",
            Self::OpenAiCompat => {
                "OpenAI-kompatibel (OpenAI, OpenRouter, Ollama, LM Studio, vLLM …)"
            }
        }
    }
}

/// Ein Werkzeug, das das Modell aufrufen darf.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    /// JSON-Schema der Parameter.
    pub schema: Value,
}

/// Ein Aufruf, den das Modell verlangt.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub args: Value,
}

/// Ein Eintrag im Gespräch.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    User(String),
    Assistant {
        text: String,
        calls: Vec<ToolCall>,
    },
    Tool {
        id: String,
        content: String,
        is_error: bool,
    },
}

/// Eine Antwort des Modells.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reply {
    pub text: String,
    pub calls: Vec<ToolCall>,
}

/// Ein Anbieter mit Adresse, Modell und (optional) Schlüssel.
#[derive(Clone)]
pub struct Provider {
    pub kind: Kind,
    pub base_url: String,
    pub model: String,
    auth: Option<String>,
}

impl std::fmt::Debug for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Provider")
            .field("kind", &self.kind)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("auth", &self.auth.as_ref().map(|_| "***"))
            .finish()
    }
}

/// Hängt `path` (z. B. `/chat/completions`) an die Basisadresse. Die Basis darf mit oder
/// ohne `/v1` (oder `/v1beta`, `/api/v1` …) enden, und wer gleich die ganze Endpunkt-
/// Adresse einträgt, bekommt dieselbe zurück.
pub fn endpoint(base: &str, path: &str) -> String {
    let mut b = base.trim().trim_end_matches('/');
    for tail in ["/chat/completions", "/messages", "/models"] {
        if let Some(rest) = b.strip_suffix(tail) {
            b = rest.trim_end_matches('/');
        }
    }
    let last = b.rsplit('/').next().unwrap_or("");
    let versioned = last.len() >= 2 && last.starts_with('v') && last.as_bytes()[1].is_ascii_digit();
    if versioned {
        format!("{b}{path}")
    } else {
        format!("{b}/v1{path}")
    }
}

/// Kürzt Fremdtext für Fehlermeldungen.
fn short(s: &str) -> String {
    let s = s.trim();
    let mut out: String = s.chars().take(240).collect();
    if s.chars().count() > 240 {
        out.push('…');
    }
    out
}

/// Fehlertext aus Status und Rumpf, ohne Schlüssel.
pub(crate) fn http_error(status: u16, body: &str) -> String {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v["error"]["message"]
                .as_str()
                .or_else(|| v["error"].as_str())
                .or_else(|| v["message"].as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| short(body));
    let hint = match status {
        401 | 403 => "Der Anbieter lehnt den Schlüssel ab oder verlangt einen.",
        404 => {
            "Adresse oder Modell nicht gefunden. Stimmt die Basis-Adresse, stimmt der Modellname?"
        }
        429 => "Zu viele Anfragen oder Guthaben aufgebraucht.",
        s if s >= 500 => "Der Anbieter meldet einen Fehler auf seiner Seite.",
        _ => "",
    };
    let detail = short(&detail);
    match (hint.is_empty(), detail.is_empty()) {
        (true, true) => format!("HTTP {status}"),
        (true, false) => format!("HTTP {status}: {detail}"),
        (false, true) => format!("HTTP {status}. {hint}"),
        (false, false) => format!("HTTP {status}. {hint} ({detail})"),
    }
}

pub(crate) fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(timeout))
        .build()
        .into()
}

pub(crate) fn connect_error(e: ureq::Error) -> String {
    format!("Keine Verbindung: {e}. Stimmt die Adresse, läuft der Server, blockiert eine Firewall?")
}

impl Provider {
    pub fn new(
        kind: Kind,
        base_url: impl Into<String>,
        model: impl Into<String>,
        auth: Option<String>,
    ) -> Self {
        Self {
            kind,
            base_url: base_url.into(),
            model: model.into(),
            auth: auth.filter(|k| !k.is_empty()),
        }
    }

    fn headers(&self) -> Vec<(&'static str, String)> {
        match (self.kind, &self.auth) {
            (Kind::Anthropic, key) => {
                let mut h = vec![("anthropic-version", ANTHROPIC_VERSION.to_string())];
                if let Some(k) = key {
                    h.push(("x-api-key", k.clone()));
                }
                h
            }
            (Kind::OpenAiCompat, Some(k)) => vec![("Authorization", format!("Bearer {k}"))],
            (Kind::OpenAiCompat, None) => vec![],
        }
    }

    fn get(&self, url: &str) -> Result<Value, String> {
        let mut req = agent(Duration::from_secs(30)).get(url);
        for (k, v) in self.headers() {
            req = req.header(k, &v);
        }
        let mut resp = req.call().map_err(connect_error)?;
        let status = resp.status().as_u16();
        let body = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())?;
        if !(200..300).contains(&status) {
            return Err(http_error(status, &body));
        }
        serde_json::from_str(&body)
            .map_err(|_| "Antwort ist kein JSON (stimmt die Adresse?)".into())
    }

    fn post(&self, url: &str, body: &Value) -> Result<Value, String> {
        let mut req = agent(Duration::from_secs(180))
            .post(url)
            .header("Content-Type", "application/json");
        for (k, v) in self.headers() {
            req = req.header(k, &v);
        }
        let mut resp = req.send(body.to_string()).map_err(connect_error)?;
        let status = resp.status().as_u16();
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())?;
        if !(200..300).contains(&status) {
            return Err(http_error(status, &text));
        }
        serde_json::from_str(&text)
            .map_err(|_| "Antwort ist kein JSON (stimmt die Adresse?)".into())
    }

    /// Modellnamen, die der Server nennt (leer, wenn er keine Liste hat).
    pub fn models(&self) -> Result<Vec<String>, String> {
        let v = self.get(&endpoint(&self.base_url, "/models"))?;
        let mut ids: Vec<String> = v["data"]
            .as_array()
            .or_else(|| v["models"].as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|m| m["id"].as_str().or_else(|| m["name"].as_str()))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        ids.sort();
        ids.dedup();
        Ok(ids)
    }

    /// Eine Antwort des Modells auf das Gespräch.
    pub fn complete(
        &self,
        system: &str,
        history: &[Message],
        tools: &[ToolDef],
    ) -> Result<Reply, String> {
        if self.model.trim().is_empty() {
            return Err("Kein Modell eingetragen".into());
        }
        match self.kind {
            Kind::Anthropic => {
                let v = self.post(
                    &endpoint(&self.base_url, "/messages"),
                    &anthropic_body(&self.model, system, history, tools),
                )?;
                parse_anthropic(&v)
            }
            Kind::OpenAiCompat => {
                let v = self.post(
                    &endpoint(&self.base_url, "/chat/completions"),
                    &openai_body(&self.model, system, history, tools),
                )?;
                parse_openai(&v)
            }
        }
    }

    /// „Verbindung testen“: erst die Modellliste; hat der Server keine, eine winzige Anfrage.
    /// Die Meldung sagt in einem Satz, was los ist.
    pub fn test(&self) -> Result<String, String> {
        match self.models() {
            Ok(list) if !list.is_empty() => {
                let m = self.model.trim();
                if m.is_empty() {
                    Ok(format!(
                        "Verbunden, {} Modelle verfügbar. Trage noch ein Modell ein.",
                        list.len()
                    ))
                } else if list.iter().any(|x| x == m) {
                    Ok(format!(
                        "Verbunden, Modell „{m}“ ist da ({} Modelle).",
                        list.len()
                    ))
                } else {
                    Ok(format!(
                        "Verbunden, aber „{m}“ steht nicht in der Liste der {} Modelle. \
                         Wähle eins aus der Liste.",
                        list.len()
                    ))
                }
            }
            Err(e)
                if e.starts_with("HTTP 401")
                    || e.starts_with("HTTP 403")
                    || e.starts_with("Keine Verbindung") =>
            {
                Err(e)
            }
            _ => self
                .complete(
                    "Antworte mit einem Wort.",
                    &[Message::User("Sag OK.".into())],
                    &[],
                )
                .map(|r| format!("Verbunden, das Modell antwortet: „{}“.", short(&r.text))),
        }
    }
}

fn anthropic_body(model: &str, system: &str, history: &[Message], tools: &[ToolDef]) -> Value {
    let mut messages: Vec<Value> = Vec::new();
    let mut results: Vec<Value> = Vec::new();
    let flush = |messages: &mut Vec<Value>, results: &mut Vec<Value>| {
        if !results.is_empty() {
            messages.push(json!({"role": "user", "content": std::mem::take(results)}));
        }
    };
    for m in history {
        match m {
            Message::User(t) => {
                flush(&mut messages, &mut results);
                messages.push(json!({"role": "user", "content": t}));
            }
            Message::Assistant { text, calls } => {
                flush(&mut messages, &mut results);
                let mut blocks = Vec::new();
                if !text.is_empty() {
                    blocks.push(json!({"type": "text", "text": text}));
                }
                for c in calls {
                    blocks.push(
                        json!({"type": "tool_use", "id": c.id, "name": c.name, "input": c.args}),
                    );
                }
                messages.push(json!({"role": "assistant", "content": blocks}));
            }
            Message::Tool {
                id,
                content,
                is_error,
            } => results.push(json!({
                "type": "tool_result", "tool_use_id": id, "content": content, "is_error": is_error
            })),
        }
    }
    flush(&mut messages, &mut results);
    let mut body = json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "system": system,
        "messages": messages,
    });
    if !tools.is_empty() {
        body["tools"] = tools
            .iter()
            .map(
                |t| json!({"name": t.name, "description": t.description, "input_schema": t.schema}),
            )
            .collect();
    }
    body
}

fn parse_anthropic(v: &Value) -> Result<Reply, String> {
    let blocks = v["content"]
        .as_array()
        .ok_or("Antwort ohne „content“ (kein Anthropic-Format?)")?;
    let mut reply = Reply::default();
    for b in blocks {
        match b["type"].as_str() {
            Some("text") => reply.text.push_str(b["text"].as_str().unwrap_or("")),
            Some("tool_use") => reply.calls.push(ToolCall {
                id: b["id"].as_str().unwrap_or("").to_string(),
                name: b["name"].as_str().unwrap_or("").to_string(),
                args: b["input"].clone(),
            }),
            _ => {}
        }
    }
    Ok(reply)
}

fn openai_body(model: &str, system: &str, history: &[Message], tools: &[ToolDef]) -> Value {
    let mut messages = vec![json!({"role": "system", "content": system})];
    for m in history {
        match m {
            Message::User(t) => messages.push(json!({"role": "user", "content": t})),
            Message::Assistant { text, calls } => {
                let mut msg = json!({"role": "assistant", "content": text});
                if !calls.is_empty() {
                    msg["tool_calls"] = calls
                        .iter()
                        .map(|c| {
                            json!({
                                "id": c.id,
                                "type": "function",
                                "function": {"name": c.name, "arguments": c.args.to_string()}
                            })
                        })
                        .collect();
                }
                messages.push(msg);
            }
            Message::Tool { id, content, .. } => {
                messages.push(json!({"role": "tool", "tool_call_id": id, "content": content}));
            }
        }
    }
    let mut body = json!({"model": model, "messages": messages});
    if !tools.is_empty() {
        body["tools"] = tools
            .iter()
            .map(|t| {
                json!({"type": "function", "function": {
                    "name": t.name, "description": t.description, "parameters": t.schema
                }})
            })
            .collect();
    }
    body
}

fn parse_openai(v: &Value) -> Result<Reply, String> {
    let msg = v["choices"]
        .get(0)
        .map(|c| &c["message"])
        .ok_or("Antwort ohne „choices“ (kein OpenAI-Format?)")?;
    let mut reply = Reply {
        text: msg["content"].as_str().unwrap_or("").to_string(),
        calls: Vec::new(),
    };
    for (i, c) in msg["tool_calls"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let args = match &c["function"]["arguments"] {
            Value::String(s) if s.trim().is_empty() => json!({}),
            Value::String(s) => serde_json::from_str(s).unwrap_or_else(|_| json!({})),
            Value::Null => json!({}),
            other => other.clone(),
        };
        reply.calls.push(ToolCall {
            id: c["id"]
                .as_str()
                .map_or_else(|| format!("call_{i}"), str::to_string),
            name: c["function"]["name"].as_str().unwrap_or("").to_string(),
            args,
        });
    }
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::MockServer;

    #[test]
    fn endpoints_with_and_without_version() {
        let e = endpoint;
        assert_eq!(
            e("https://api.anthropic.com", "/messages"),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            e("https://api.openai.com/v1", "/chat/completions"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            e("https://api.openai.com/v1/", "/models"),
            "https://api.openai.com/v1/models"
        );
        assert_eq!(
            e("http://localhost:11434", "/chat/completions"),
            "http://localhost:11434/v1/chat/completions"
        );
        assert_eq!(
            e("https://openrouter.ai/api/v1", "/models"),
            "https://openrouter.ai/api/v1/models"
        );
        assert_eq!(
            e("https://x.example/v1beta", "/models"),
            "https://x.example/v1beta/models"
        );
        // Die ganze Endpunkt-Adresse eingetragen:
        assert_eq!(
            e("http://h:1/v1/chat/completions", "/chat/completions"),
            "http://h:1/v1/chat/completions"
        );
        assert_eq!(
            e("http://h:1/v1/messages", "/models"),
            "http://h:1/v1/models"
        );
    }

    fn tools() -> Vec<ToolDef> {
        vec![ToolDef {
            name: "add_track".into(),
            description: "Spur anlegen".into(),
            schema: json!({"type": "object", "properties": {"name": {"type": "string"}}}),
        }]
    }

    #[test]
    fn debug_never_shows_the_key() {
        let p = Provider::new(
            Kind::Anthropic,
            "http://x",
            "m",
            Some("geheimwert-1".into()),
        );
        assert!(!format!("{p:?}").contains("geheimwert-1"));
    }

    #[test]
    fn anthropic_round_trip_with_one_tool_call() {
        let srv = MockServer::start(vec![
            json!({"content": [
                {"type": "text", "text": "Ich lege sie an."},
                {"type": "tool_use", "id": "tu_1", "name": "add_track", "input": {"name": "Moderation"}}
            ]})
            .to_string(),
            json!({"content": [{"type": "text", "text": "Fertig."}]}).to_string(),
        ]);
        let p = Provider::new(
            Kind::Anthropic,
            srv.url(),
            "modell-a",
            Some("wert-a".into()),
        );
        let r = p
            .complete("System", &[Message::User("Neue Spur".into())], &tools())
            .unwrap();
        assert_eq!(r.text, "Ich lege sie an.");
        assert_eq!(r.calls.len(), 1);
        assert_eq!(r.calls[0].name, "add_track");
        assert_eq!(r.calls[0].args["name"], "Moderation");

        let hist = vec![
            Message::User("Neue Spur".into()),
            Message::Assistant {
                text: r.text.clone(),
                calls: r.calls.clone(),
            },
            Message::Tool {
                id: "tu_1".into(),
                content: "{\"track\":1}".into(),
                is_error: false,
            },
        ];
        let r2 = p.complete("System", &hist, &tools()).unwrap();
        assert_eq!(r2.text, "Fertig.");

        let reqs = srv.requests();
        assert_eq!(reqs[0].path, "/v1/messages");
        assert_eq!(reqs[0].header("x-api-key").as_deref(), Some("wert-a"));
        assert_eq!(
            reqs[0].header("anthropic-version").as_deref(),
            Some("2023-06-01")
        );
        assert!(reqs[0].header("authorization").is_none());
        let b0: Value = serde_json::from_str(&reqs[0].body).unwrap();
        assert_eq!(b0["model"], "modell-a");
        assert_eq!(b0["system"], "System");
        assert_eq!(b0["tools"][0]["name"], "add_track");
        assert_eq!(b0["tools"][0]["input_schema"]["type"], "object");
        let b1: Value = serde_json::from_str(&reqs[1].body).unwrap();
        let m = b1["messages"].as_array().unwrap();
        assert_eq!(m.len(), 3);
        assert_eq!(m[1]["content"][1]["type"], "tool_use");
        assert_eq!(m[2]["role"], "user");
        assert_eq!(m[2]["content"][0]["type"], "tool_result");
        assert_eq!(m[2]["content"][0]["tool_use_id"], "tu_1");
    }

    #[test]
    fn openai_round_trip_with_one_tool_call_and_optional_key() {
        let srv = MockServer::start(vec![
            json!({"choices": [{"message": {"content": null, "tool_calls": [
                {"id": "call_9", "type": "function",
                 "function": {"name": "add_track", "arguments": "{\"name\":\"Jingle\"}"}}
            ]}}]})
            .to_string(),
            json!({"choices": [{"message": {"content": "Erledigt."}}]}).to_string(),
        ]);
        // Basis mit /v1, ohne Schlüssel (lokaler Server).
        let p = Provider::new(
            Kind::OpenAiCompat,
            format!("{}/v1", srv.url()),
            "llama3",
            None,
        );
        let r = p
            .complete("S", &[Message::User("Spur".into())], &tools())
            .unwrap();
        assert_eq!(r.calls[0].id, "call_9");
        assert_eq!(r.calls[0].args["name"], "Jingle");
        let hist = vec![
            Message::User("Spur".into()),
            Message::Assistant {
                text: String::new(),
                calls: r.calls.clone(),
            },
            Message::Tool {
                id: "call_9".into(),
                content: "ok".into(),
                is_error: false,
            },
        ];
        let r2 = p.complete("S", &hist, &tools()).unwrap();
        assert_eq!(r2.text, "Erledigt.");

        let reqs = srv.requests();
        assert_eq!(reqs[0].path, "/v1/chat/completions");
        assert!(
            reqs[0].header("authorization").is_none(),
            "ohne Schlüssel kein Kopf"
        );
        let b0: Value = serde_json::from_str(&reqs[0].body).unwrap();
        assert_eq!(b0["messages"][0]["role"], "system");
        assert_eq!(b0["tools"][0]["type"], "function");
        assert_eq!(b0["tools"][0]["function"]["name"], "add_track");
        let b1: Value = serde_json::from_str(&reqs[1].body).unwrap();
        let m = b1["messages"].as_array().unwrap();
        assert_eq!(
            m[2]["tool_calls"][0]["function"]["arguments"],
            "{\"name\":\"Jingle\"}"
        );
        assert_eq!(m[3]["role"], "tool");
        assert_eq!(m[3]["tool_call_id"], "call_9");
    }

    #[test]
    fn openai_sends_the_key_as_bearer() {
        let srv = MockServer::start(vec![
            json!({"data": [{"id": "b"}, {"id": "a"}]}).to_string(),
        ]);
        let p = Provider::new(Kind::OpenAiCompat, srv.url(), "a", Some("wert-b".into()));
        assert_eq!(p.models().unwrap(), vec!["a", "b"]);
        let reqs = srv.requests();
        assert_eq!(reqs[0].path, "/v1/models");
        assert_eq!(
            reqs[0].header("authorization").as_deref(),
            Some("Bearer wert-b")
        );
    }

    #[test]
    fn test_button_texts_say_what_is_wrong() {
        let srv = MockServer::start(vec![json!({"data": [{"id": "m1"}]}).to_string()]);
        let p = Provider::new(Kind::OpenAiCompat, srv.url(), "m1", None);
        assert!(p.test().unwrap().contains("Modell „m1“ ist da"));

        let srv = MockServer::start(vec![json!({"data": [{"id": "m1"}]}).to_string()]);
        let p = Provider::new(Kind::OpenAiCompat, srv.url(), "falsch", None);
        assert!(p.test().unwrap().contains("steht nicht in der Liste"));

        let srv =
            MockServer::with_status(401, json!({"error": {"message": "bad key"}}).to_string());
        let p = Provider::new(Kind::Anthropic, srv.url(), "m", Some("nie-zeigen-1".into()));
        let e = p.test().unwrap_err();
        assert!(e.contains("Schlüssel"), "{e}");
        assert!(!e.contains("nie-zeigen-1"));

        let p = Provider::new(Kind::OpenAiCompat, "http://127.0.0.1:1", "m", None);
        assert!(p.test().unwrap_err().starts_with("Keine Verbindung"));
    }

    #[test]
    fn server_without_model_list_is_tested_with_a_tiny_request() {
        let srv = MockServer::start_routes(vec![
            ("/v1/models", 404, "{}".to_string()),
            (
                "/v1/chat/completions",
                200,
                json!({"choices": [{"message": {"content": "OK"}}]}).to_string(),
            ),
        ]);
        let p = Provider::new(Kind::OpenAiCompat, srv.url(), "x", None);
        assert!(p.test().unwrap().contains("antwortet: „OK“"));
    }

    #[test]
    fn a_page_that_is_no_model_endpoint_is_reported() {
        let srv = MockServer::start(vec!["<html>hallo</html>".to_string()]);
        let p = Provider::new(Kind::OpenAiCompat, srv.url(), "x", None);
        assert!(
            p.complete("S", &[Message::User("x".into())], &[])
                .unwrap_err()
                .contains("kein JSON")
        );
    }
}
