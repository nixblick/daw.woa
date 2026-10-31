//! Das Gespräch: Nachrichten an das Modell, Werkzeug-Aufrufe an WOA und die externen
//! MCP-Server, bis das Modell mit Text antwortet. Läuft in einem Hintergrund-Thread, damit
//! die Oberfläche nie wartet.

use std::sync::mpsc::{Receiver, Sender, channel};

use serde_json::Value;

use crate::llm::{Message, Provider, ToolDef};
use crate::mcp_client::{McpClient, Spec};

/// Höchstens so viele Modell-Runden je Eingabe (Schutz vor Endlosschleifen).
const MAX_ROUNDS: usize = 10;
/// Längere Werkzeug-Ergebnisse werden gekürzt, bevor sie ans Modell gehen.
const MAX_RESULT: usize = 20_000;

const SYSTEM: &str = "Du bist der Assistent in WOA, einer Software für Radiosendungen \
(Spuren, Clips, Mischen, Sendeplan). Antworte kurz und auf Deutsch. Nutze die Werkzeuge, \
wenn der Nutzer etwas im Projekt ändern oder wissen will, und sage danach in einem Satz, \
was du getan hast.";

/// Was der Oberfläche gemeldet wird.
#[derive(Debug, Clone, PartialEq)]
pub enum Ev {
    /// Hinweis (z. B. ein Server, der sich nicht verbinden ließ).
    Notice(String),
    /// Bereit, mit so vielen Werkzeugen.
    Ready(usize),
    /// Ein Werkzeug wird aufgerufen.
    Tool(String),
    Reply(String),
    Error(String),
}

pub struct Assistant {
    provider: Provider,
    history: Vec<Message>,
    sources: Vec<(String, McpClient)>,
    tools: Vec<ToolDef>,
    /// (Name gegenüber dem Modell, Index der Quelle, echter Name)
    routes: Vec<(String, usize, String)>,
}

/// Name für das Modell: nur `[A-Za-z0-9_-]`, höchstens 64 Zeichen.
fn slug(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect()
}

fn clip(s: &str) -> String {
    if s.chars().count() <= MAX_RESULT {
        return s.to_string();
    }
    let mut out: String = s.chars().take(MAX_RESULT).collect();
    out.push_str("\n[gekürzt]");
    out
}

impl Assistant {
    /// Fragt die Werkzeuge aller verbundenen Quellen ab. Doppelte Namen bekommen den
    /// Namen der Quelle vorangestellt.
    pub fn new(provider: Provider, mut sources: Vec<(String, McpClient)>) -> (Self, Vec<String>) {
        let mut notes = Vec::new();
        let mut listed: Vec<(usize, String, ToolDef)> = Vec::new();
        for (i, (label, client)) in sources.iter_mut().enumerate() {
            match client.list_tools() {
                Ok(ts) => {
                    for t in ts {
                        listed.push((
                            i,
                            t.name.clone(),
                            ToolDef {
                                name: t.name,
                                description: t.description,
                                schema: t.schema,
                            },
                        ));
                    }
                }
                Err(e) => notes.push(format!("„{label}“: Werkzeuge nicht abrufbar ({e})")),
            }
        }
        let mut tools = Vec::new();
        let mut routes = Vec::new();
        for (i, real, mut def) in listed.clone() {
            let clash = listed.iter().filter(|(_, n, _)| *n == real).count() > 1
                || !real
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                || real.len() > 64;
            let exposed = if clash {
                slug(&format!("{}__{}", sources[i].0, real))
            } else {
                real.clone()
            };
            def.name = exposed.clone();
            tools.push(def);
            routes.push((exposed, i, real));
        }
        (
            Self {
                provider,
                history: Vec::new(),
                sources,
                tools,
                routes,
            },
            notes,
        )
    }

    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// Eine Eingabe des Nutzers; ruft Werkzeuge, bis das Modell mit Text antwortet.
    pub fn turn(&mut self, text: &str, on_tool: &mut dyn FnMut(&str)) -> Result<String, String> {
        let start = self.history.len();
        self.history.push(Message::User(text.to_string()));
        let result = self.run_rounds(on_tool);
        if let Err(e) = &result {
            // Werkzeuge, die schon liefen, haben das Projekt geändert: Ihre Runden bleiben im
            // Verlauf, nur der unvollständige Rest danach fällt weg, und eine Notiz schließt ab.
            match self.history[start..]
                .iter()
                .rposition(|m| matches!(m, Message::Tool { .. }))
            {
                None => self.history.truncate(start),
                Some(i) => {
                    self.history.truncate(start + i + 1);
                    self.history.push(Message::Assistant {
                        text: format!("(Zug abgebrochen: {e})"),
                        calls: Vec::new(),
                    });
                }
            }
        }
        result
    }

    fn run_rounds(&mut self, on_tool: &mut dyn FnMut(&str)) -> Result<String, String> {
        for _ in 0..MAX_ROUNDS {
            let reply = self.provider.complete(SYSTEM, &self.history, &self.tools)?;
            self.history.push(Message::Assistant {
                text: reply.text.clone(),
                calls: reply.calls.clone(),
            });
            if reply.calls.is_empty() {
                return Ok(reply.text);
            }
            for call in reply.calls {
                on_tool(&call.name);
                let (content, is_error) = self.call_tool(&call.name, &call.args);
                self.history.push(Message::Tool {
                    id: call.id,
                    content: clip(&content),
                    is_error,
                });
            }
        }
        Err(format!(
            "Abgebrochen nach {MAX_ROUNDS} Runden ohne Antwort. Formuliere die Bitte kleiner."
        ))
    }

    fn call_tool(&mut self, name: &str, args: &Value) -> (String, bool) {
        let Some((_, i, real)) = self.routes.iter().find(|(n, _, _)| n == name).cloned() else {
            return (format!("Unbekanntes Werkzeug „{name}“."), true);
        };
        match self.sources[i].1.call(&real, args) {
            Ok((text, is_error)) => (text, is_error),
            Err(e) => (e, true),
        }
    }
}

/// Befehle an den Hintergrund-Thread.
pub enum Cmd {
    Send(String),
}

/// Der Hintergrund-Thread samt Kanälen. Beim Ablegen endet der Thread, und die Kind-Prozesse
/// der Stdio-Server werden beendet.
pub struct Worker {
    pub tx: Sender<Cmd>,
    pub rx: Receiver<Ev>,
}

impl Worker {
    /// Verbindet alle Quellen (Name, Zugang) und wartet dann auf Eingaben.
    pub fn spawn(provider: Provider, specs: Vec<(String, Spec)>) -> Self {
        Self::spawn_with(move || (provider, specs))
    }

    /// Wie [`Worker::spawn`], aber Anbieter und Quellen entstehen erst im Hintergrund-Thread.
    /// So liest die Oberfläche nie selbst den Schlüsselbund.
    pub fn spawn_with<F>(setup: F) -> Self
    where
        F: FnOnce() -> (Provider, Vec<(String, Spec)>) + Send + 'static,
    {
        let (tx, cmd_rx) = channel::<Cmd>();
        let (ev_tx, rx) = channel::<Ev>();
        std::thread::spawn(move || {
            let (provider, specs) = setup();
            let mut sources = Vec::new();
            for (label, spec) in specs {
                match McpClient::connect(&spec) {
                    Ok(c) => sources.push((label, c)),
                    Err(e) => {
                        let _ = ev_tx.send(Ev::Notice(format!("„{label}“ nicht verbunden: {e}")));
                    }
                }
            }
            let (mut a, notes) = Assistant::new(provider, sources);
            for n in notes {
                let _ = ev_tx.send(Ev::Notice(n));
            }
            let _ = ev_tx.send(Ev::Ready(a.tool_count()));
            while let Ok(Cmd::Send(text)) = cmd_rx.recv() {
                let t = ev_tx.clone();
                let out = a.turn(&text, &mut |name| {
                    let _ = t.send(Ev::Tool(name.to_string()));
                });
                let _ = ev_tx.send(match out {
                    Ok(r) => Ev::Reply(r),
                    Err(e) => Ev::Error(e),
                });
            }
        });
        Self { tx, rx }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::Kind;
    use crate::testutil::MockServer;
    use serde_json::json;

    fn mcp_mock() -> MockServer {
        MockServer::start(vec![
            json!({"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}).to_string(),
            String::new(),
            json!({"jsonrpc":"2.0","id":2,"result":{"tools":[
                {"name":"add_track","description":"Spur","inputSchema":{"type":"object"}}]}})
            .to_string(),
            json!({"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"Spur 4"}]}})
                .to_string(),
        ])
    }

    fn llm_mock(kind: Kind) -> MockServer {
        match kind {
            Kind::OpenAiCompat => MockServer::start(vec![
                json!({"choices":[{"message":{"content":null,"tool_calls":[
                    {"id":"c1","type":"function","function":{"name":"add_track","arguments":"{}"}}]}}]})
                .to_string(),
                json!({"choices":[{"message":{"content":"Spur 4 ist angelegt."}}]}).to_string(),
            ]),
            Kind::Anthropic => MockServer::start(vec![
                json!({"content":[{"type":"tool_use","id":"c1","name":"add_track","input":{}}]})
                    .to_string(),
                json!({"content":[{"type":"text","text":"Spur 4 ist angelegt."}]}).to_string(),
            ]),
        }
    }

    fn run(kind: Kind) {
        let mcp = mcp_mock();
        let llm = llm_mock(kind);
        let client = McpClient::connect(&Spec::Http {
            url: mcp.url(),
            headers: vec![],
        })
        .unwrap();
        let (mut a, notes) = Assistant::new(
            Provider::new(kind, llm.url(), "m", None),
            vec![("WOA".into(), client)],
        );
        assert!(notes.is_empty());
        assert_eq!(a.tool_count(), 1);
        let mut seen = Vec::new();
        let r = a
            .turn("Lege eine Spur an", &mut |n| seen.push(n.to_string()))
            .unwrap();
        assert_eq!(r, "Spur 4 ist angelegt.");
        assert_eq!(seen, vec!["add_track"]);
        // Das Werkzeug-Ergebnis ging ans Modell zurück.
        let second = llm.requests()[1].body.clone();
        assert!(second.contains("Spur 4"), "{second}");
    }

    #[test]
    fn tool_loop_with_openai_format() {
        run(Kind::OpenAiCompat);
    }

    #[test]
    fn tool_loop_with_anthropic_format() {
        run(Kind::Anthropic);
    }

    #[test]
    fn unknown_tool_and_failed_turn_leave_history_clean() {
        let llm = MockServer::with_status(500, "{}".into());
        let (mut a, _) = Assistant::new(
            Provider::new(Kind::OpenAiCompat, llm.url(), "m", None),
            vec![],
        );
        assert!(
            a.turn("hallo", &mut |_| {})
                .unwrap_err()
                .starts_with("HTTP 500")
        );
        assert!(a.history.is_empty());
        assert!(a.call_tool("nix", &json!({})).1);
    }

    #[test]
    fn worker_reports_unreachable_server_and_answers() {
        let llm = MockServer::start(vec![
            json!({"choices":[{"message":{"content":"Hallo!"}}]}).to_string(),
        ]);
        let w = Worker::spawn(
            Provider::new(Kind::OpenAiCompat, llm.url(), "m", None),
            vec![(
                "kaputt".into(),
                Spec::Http {
                    url: "http://127.0.0.1:1".into(),
                    headers: vec![],
                },
            )],
        );
        w.tx.send(Cmd::Send("Hi".into())).unwrap();
        let mut got = Vec::new();
        while let Ok(e) = w.rx.recv_timeout(std::time::Duration::from_secs(20)) {
            let done = matches!(e, Ev::Reply(_) | Ev::Error(_));
            got.push(e);
            if done {
                break;
            }
        }
        assert!(matches!(&got[0], Ev::Notice(n) if n.contains("kaputt")));
        assert!(got.contains(&Ev::Ready(0)));
        assert_eq!(got.last(), Some(&Ev::Reply("Hallo!".into())));
    }

    #[test]
    fn failure_in_round_two_keeps_executed_tool_calls() {
        let mcp = mcp_mock();
        let llm = MockServer::start(vec![
            json!({"choices":[{"message":{"content":null,"tool_calls":[
                {"id":"c1","type":"function","function":{"name":"add_track","arguments":"{}"}}]}}]})
            .to_string(),
            "{}".into(),
        ]);
        let client = McpClient::connect(&Spec::Http {
            url: mcp.url(),
            headers: vec![],
        })
        .unwrap();
        let (mut a, _) = Assistant::new(
            Provider::new(Kind::OpenAiCompat, llm.url(), "m", None),
            vec![("WOA".into(), client)],
        );
        assert!(a.turn("Lege eine Spur an", &mut |_| {}).is_err());
        assert!(matches!(&a.history[0], Message::User(t) if t == "Lege eine Spur an"));
        assert!(matches!(&a.history[1], Message::Assistant { calls, .. } if calls.len() == 1));
        assert!(
            matches!(&a.history[2], Message::Tool { content, .. } if content.contains("Spur 4"))
        );
        assert!(
            matches!(&a.history[3], Message::Assistant { text, calls } if calls.is_empty() && text.contains("abgebrochen"))
        );
        assert_eq!(a.history.len(), 4);
    }

    #[test]
    fn tool_names_are_made_safe_and_unique() {
        assert_eq!(slug("Mein Server!"), "Mein_Server_");
    }
}
