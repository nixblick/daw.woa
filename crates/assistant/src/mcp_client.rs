//! Kleiner MCP-Client für die Werkzeuge des Assistenten: spricht mit beliebigen MCP-Servern,
//! lokal über Standard-Ein-/Ausgabe (Zeilen mit JSON-RPC) oder über HTTP (Streamable HTTP,
//! Antwort als JSON oder als Ereignisstrom). Nur das, was der Assistent braucht:
//! `initialize`, `tools/list`, `tools/call`.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::Duration;

use serde_json::{Value, json};

use crate::llm::{agent, connect_error, http_error};

const PROTOCOL: &str = "2025-06-18";
const CALL_TIMEOUT: Duration = Duration::from_secs(120);

/// Wie ein Server erreicht wird, mit schon eingesetztem Geheimnis.
#[derive(Clone)]
pub enum Spec {
    Stdio {
        command: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
    },
    Http {
        url: String,
        headers: Vec<(String, String)>,
    },
}

/// Ein Werkzeug eines Servers.
#[derive(Debug, Clone, PartialEq)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

enum Link {
    Stdio {
        child: Child,
        stdin: ChildStdin,
        lines: Receiver<String>,
    },
    Http {
        url: String,
        headers: Vec<(String, String)>,
        session: Option<String>,
    },
}

pub struct McpClient {
    link: Link,
    next_id: u64,
}

impl Drop for McpClient {
    fn drop(&mut self) {
        if let Link::Stdio { child, .. } = &mut self.link {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl McpClient {
    /// Verbindet und macht die Begrüßung (`initialize`).
    pub fn connect(spec: &Spec) -> Result<Self, String> {
        let link = match spec {
            Spec::Stdio { command, args, env } => {
                if command.trim().is_empty() {
                    return Err("Kein Startbefehl eingetragen".into());
                }
                let mut child = Command::new(command)
                    .args(args)
                    .envs(env.iter().map(|(k, v)| (k, v)))
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|e| format!("Start von „{command}“ fehlgeschlagen: {e}"))?;
                let stdin = child.stdin.take().ok_or("keine Eingabe")?;
                let out = child.stdout.take().ok_or("keine Ausgabe")?;
                let (tx, lines) = channel();
                std::thread::spawn(move || {
                    for l in BufReader::new(out).lines() {
                        let Ok(l) = l else { break };
                        if tx.send(l).is_err() {
                            break;
                        }
                    }
                });
                Link::Stdio {
                    child,
                    stdin,
                    lines,
                }
            }
            Spec::Http { url, headers } => {
                if url.trim().is_empty() {
                    return Err("Keine Adresse eingetragen".into());
                }
                Link::Http {
                    url: url.trim().to_string(),
                    headers: headers.clone(),
                    session: None,
                }
            }
        };
        let mut c = Self { link, next_id: 1 };
        c.request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL,
                "capabilities": {},
                "clientInfo": {"name": "woa", "version": env!("CARGO_PKG_VERSION")}
            }),
            Duration::from_secs(30),
        )?;
        c.notify("notifications/initialized")?;
        Ok(c)
    }

    fn notify(&mut self, method: &str) -> Result<(), String> {
        let msg = json!({"jsonrpc": "2.0", "method": method});
        match &mut self.link {
            Link::Stdio { stdin, .. } => writeln!(stdin, "{msg}")
                .and_then(|_| stdin.flush())
                .map_err(|e| format!("Server nicht mehr da: {e}")),
            Link::Http { .. } => self.http_post(&msg).map(|_| ()),
        }
    }

    fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let reply = match &mut self.link {
            Link::Stdio { stdin, lines, .. } => {
                writeln!(stdin, "{msg}")
                    .and_then(|_| stdin.flush())
                    .map_err(|e| format!("Server nicht mehr da: {e}"))?;
                loop {
                    let line = lines.recv_timeout(timeout).map_err(|e| match e {
                        RecvTimeoutError::Timeout => "Keine Antwort (Zeit abgelaufen)".to_string(),
                        RecvTimeoutError::Disconnected => {
                            "Server hat sich beendet (stimmt der Startbefehl?)".to_string()
                        }
                    })?;
                    if let Ok(v) = serde_json::from_str::<Value>(&line)
                        && v["id"] == json!(id)
                    {
                        break v;
                    }
                }
            }
            Link::Http { .. } => self
                .http_post(&msg)?
                .into_iter()
                .find(|v| v["id"] == json!(id))
                .ok_or("Antwort ohne passende Kennung")?,
        };
        if let Some(e) = reply.get("error") {
            return Err(format!(
                "Server meldet: {}",
                e["message"].as_str().unwrap_or("Fehler")
            ));
        }
        Ok(reply["result"].clone())
    }

    /// POST an einen HTTP-Server; liefert die JSON-RPC-Nachrichten der Antwort.
    fn http_post(&mut self, msg: &Value) -> Result<Vec<Value>, String> {
        let Link::Http {
            url,
            headers,
            session,
        } = &mut self.link
        else {
            return Err("kein HTTP".into());
        };
        let mut req = agent(CALL_TIMEOUT)
            .post(url.as_str())
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream");
        for (k, v) in headers.iter() {
            req = req.header(k.as_str(), v.as_str());
        }
        if let Some(s) = session.as_deref() {
            req = req.header("Mcp-Session-Id", s);
        }
        let mut resp = req.send(msg.to_string()).map_err(connect_error)?;
        let status = resp.status().as_u16();
        if let Some(s) = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
        {
            *session = Some(s.to_string());
        }
        let ctype = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())?;
        if !(200..300).contains(&status) {
            return Err(http_error(status, &body));
        }
        if body.trim().is_empty() {
            return Ok(vec![]);
        }
        if ctype.contains("text/event-stream") {
            Ok(body
                .lines()
                .filter_map(|l| l.strip_prefix("data:"))
                .filter_map(|d| serde_json::from_str(d.trim()).ok())
                .collect())
        } else {
            let v: Value = serde_json::from_str(&body)
                .map_err(|_| "Antwort ist kein JSON (stimmt die Adresse?)".to_string())?;
            Ok(match v {
                Value::Array(a) => a,
                one => vec![one],
            })
        }
    }

    pub fn list_tools(&mut self) -> Result<Vec<McpTool>, String> {
        let r = self.request("tools/list", json!({}), Duration::from_secs(30))?;
        Ok(r["tools"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|t| McpTool {
                        name: t["name"].as_str().unwrap_or("").to_string(),
                        description: t["description"].as_str().unwrap_or("").to_string(),
                        schema: t
                            .get("inputSchema")
                            .cloned()
                            .unwrap_or_else(|| json!({"type": "object"})),
                    })
                    .filter(|t| !t.name.is_empty())
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Ruft ein Werkzeug. Liefert Text und ob der Server einen Fehler meldet.
    pub fn call(&mut self, name: &str, args: &Value) -> Result<(String, bool), String> {
        let r = self.request(
            "tools/call",
            json!({"name": name, "arguments": args}),
            CALL_TIMEOUT,
        )?;
        let text = r["content"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|c| c["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let text = if text.is_empty() && !r["structuredContent"].is_null() {
            r["structuredContent"].to_string()
        } else {
            text
        };
        Ok((text, r["isError"].as_bool().unwrap_or(false)))
    }
}

/// „Verbindung testen“ für einen Server.
pub fn test(spec: &Spec) -> Result<String, String> {
    let mut c = McpClient::connect(spec)?;
    let tools = c.list_tools()?;
    Ok(match tools.len() {
        0 => "Verbunden, der Server bietet keine Werkzeuge an.".to_string(),
        n => {
            let names: Vec<_> = tools.iter().take(4).map(|t| t.name.as_str()).collect();
            format!(
                "Verbunden, {n} {} ({}{}).",
                if n == 1 { "Werkzeug" } else { "Werkzeuge" },
                names.join(", "),
                if n > 4 { ", …" } else { "" }
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::MockServer;

    fn init_ok() -> String {
        json!({"jsonrpc":"2.0","id":1,"result":{"protocolVersion":PROTOCOL,"capabilities":{},
            "serverInfo":{"name":"x","version":"1"}}})
        .to_string()
    }

    #[test]
    fn http_json_flow_with_header_and_tools() {
        // Reihenfolge: initialize, initialized (leer), tools/list, tools/call.
        let srv = MockServer::start(vec![
            init_ok(),
            String::new(),
            json!({"jsonrpc":"2.0","id":2,"result":{"tools":[
                {"name":"wetter","description":"Wetter","inputSchema":{"type":"object"}}]}})
            .to_string(),
            json!({"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"sonnig"}],
                "isError":false}})
            .to_string(),
        ]);
        let spec = Spec::Http {
            url: srv.url(),
            headers: vec![("Authorization".into(), "Bearer abc".into())],
        };
        let mut c = McpClient::connect(&spec).unwrap();
        let tools = c.list_tools().unwrap();
        assert_eq!(tools[0].name, "wetter");
        assert_eq!(
            c.call("wetter", &json!({"ort": "x"})).unwrap(),
            ("sonnig".into(), false)
        );
        let reqs = srv.requests();
        assert_eq!(
            reqs[0].header("authorization").as_deref(),
            Some("Bearer abc")
        );
        let b: Value = serde_json::from_str(&reqs[3].body).unwrap();
        assert_eq!(b["method"], "tools/call");
        assert_eq!(b["params"]["arguments"]["ort"], "x");
    }

    #[test]
    fn http_event_stream_answer_is_read() {
        let body = format!("event: message\ndata: {}\n\n", init_ok());
        let srv = MockServer::start_typed("text/event-stream", body);
        assert!(
            McpClient::connect(&Spec::Http {
                url: srv.url(),
                headers: vec![]
            })
            .is_ok()
        );
    }

    #[test]
    fn errors_are_readable() {
        let srv = MockServer::with_status(401, "{}".into());
        let e = test(&Spec::Http {
            url: srv.url(),
            headers: vec![],
        })
        .unwrap_err();
        assert!(e.starts_with("HTTP 401"), "{e}");
        let e = test(&Spec::Stdio {
            command: "gibt-es-nicht-woa".into(),
            args: vec![],
            env: vec![],
        })
        .unwrap_err();
        assert!(e.contains("Start von"), "{e}");
        assert!(
            test(&Spec::Http {
                url: String::new(),
                headers: vec![]
            })
            .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn stdio_flow_with_a_tiny_shell_server() {
        // Ein Mini-Server in sh: antwortet je Zeile mit passender Kennung.
        let script = r#"
while read line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"initialize"'*) echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"capabilities\":{}}}";;
    *'"tools/list"'*) echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"tools\":[{\"name\":\"echo\",\"inputSchema\":{\"type\":\"object\"}}]}}";;
    *'"tools/call"'*) echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"hallo\"}]}}";;
  esac
done
"#;
        let spec = Spec::Stdio {
            command: "sh".into(),
            args: vec!["-c".into(), script.into()],
            env: vec![],
        };
        let mut c = McpClient::connect(&spec).unwrap();
        assert_eq!(c.list_tools().unwrap()[0].name, "echo");
        assert_eq!(c.call("echo", &json!({})).unwrap().0, "hallo");
        assert!(test(&spec).unwrap().contains("1 Werkzeug"));
    }
}
