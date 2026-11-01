//! Paket J10: `search_samples` und `import_sample` über Freesound. Kein Netz und kein
//! Schlüsselbund: ein lokaler Testserver spielt Freesound, der Schlüssel ist ein Platzhalter.

use rmcp::model::{CallToolRequestParams, CallToolResult, ClientConfig};
use rmcp::service::RunningService;
use rmcp::{ClientHandler, RoleClient, ServiceExt};
use serde_json::json;
use woa_agent::WoaAgent;

#[derive(Clone, Default)]
struct TestClient;

impl ClientHandler for TestClient {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::default()
    }
}

type Client = RunningService<RoleClient, TestClient>;

async fn call(client: &Client, name: &'static str, args: serde_json::Value) -> CallToolResult {
    let mut params = CallToolRequestParams::new(name);
    if let serde_json::Value::Object(map) = args {
        params = params.with_arguments(map);
    }
    client.call_tool(params).await.unwrap()
}

fn ok(r: &CallToolResult) -> serde_json::Value {
    assert_ne!(r.is_error, Some(true), "tool error: {r:?}");
    serde_json::from_str(&r.content[0].as_text().unwrap().text).unwrap()
}

fn is_err(r: &CallToolResult) -> bool {
    r.is_error == Some(true)
}

async fn start(dir: &std::path::Path, api: &str, key: Option<&'static str>) -> (Client, WoaAgent) {
    let agent = WoaAgent::open_or_create(dir.join("p"))
        .unwrap()
        .with_sample_source(woa_agent::freesound::Source {
            api: api.to_string(),
            key: std::sync::Arc::new(move || key.map(str::to_string)),
        });
    let (c_write, s_read) = tokio::io::duplex(1 << 16);
    let (s_write, c_read) = tokio::io::duplex(1 << 16);
    tokio::spawn({
        let agent = agent.clone();
        async move { woa_agent::serve(agent, (s_read, s_write)).await.unwrap() }
    });
    (TestClient.serve((c_read, c_write)).await.unwrap(), agent)
}

const KEY: &str = "test-key-placeholder";

/// Ein Mini-Freesound auf 127.0.0.1: Suche, Klang 11 (CC0), Klang 12 (CC BY-NC) und eine
/// WAV-Vorschau. Gibt die Adresse und die Liste der gesehenen Anfragezeilen zurück.
fn fake_freesound() -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    use std::io::{BufRead, BufReader, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let (b, s) = (base.clone(), seen.clone());
    let mut wav = std::io::Cursor::new(Vec::new());
    {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::new(&mut wav, spec).unwrap();
        for i in 0..48_000 {
            w.write_sample(((i as f32 * 0.05).sin() * 8000.0) as i16)
                .unwrap();
        }
        w.finalize().unwrap();
    }
    let wav = wav.into_inner();
    std::thread::spawn(move || {
        let snd = |id: u64, lic: &str| {
            json!({"id": id, "name": format!("Riser {id}"), "username": "maker",
                "license": lic, "duration": 1.0, "url": format!("https://freesound.org/s/{id}/"),
                "previews": {"preview-hq-mp3": format!("{b}/p/{id}.wav")}, "tags": ["riser"]})
        };
        for stream in l.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut r = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            let mut auth = String::new();
            loop {
                let mut h = String::new();
                if r.read_line(&mut h).unwrap() == 0 || h == "\r\n" {
                    break;
                }
                if h.to_ascii_lowercase().starts_with("authorization:") {
                    auth = h.trim().to_string();
                }
            }
            s.lock().unwrap().push(format!("{} {auth}", line.trim()));
            let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
            let (ctype, body): (&str, Vec<u8>) = if path.starts_with("/p/") {
                ("audio/wav", wav.clone())
            } else if path.starts_with("/search/text/") {
                let v = json!({"count": 3, "results": [
                    snd(11, "http://creativecommons.org/publicdomain/zero/1.0/"),
                    snd(12, "http://creativecommons.org/licenses/by-nc/4.0/"),
                    snd(13, "https://creativecommons.org/licenses/by/4.0/"),
                ]});
                ("application/json", v.to_string().into_bytes())
            } else if path.starts_with("/sounds/11/") {
                (
                    "application/json",
                    snd(11, "http://creativecommons.org/publicdomain/zero/1.0/")
                        .to_string()
                        .into_bytes(),
                )
            } else if path.starts_with("/sounds/12/") {
                (
                    "application/json",
                    snd(12, "http://creativecommons.org/licenses/by-nc/4.0/")
                        .to_string()
                        .into_bytes(),
                )
            } else {
                ("text/plain", b"nope".to_vec())
            };
            let status = if ctype == "text/plain" {
                "404 Not Found"
            } else {
                "200 OK"
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(&body);
        }
    });
    (base, seen)
}

#[tokio::test]
async fn search_returns_only_cc0_and_cc_by_and_sends_the_filter() {
    let dir = tempfile::tempdir().unwrap();
    let (api, seen) = fake_freesound();
    let (client, _) = start(dir.path(), &api, Some(KEY)).await;
    let r = ok(&call(&client, "search_samples", json!({"text": "riser"})).await);
    let ids: Vec<u64> = r["samples"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, [11, 13], "{r}");
    for h in r["samples"].as_array().unwrap() {
        assert!(h["license"] == "CC0" || h["license"] == "CC BY", "{h}");
    }
    assert!(
        !r.to_string().contains(KEY),
        "key leaked into the answer: {r}"
    );
    let req = seen.lock().unwrap().join("\n");
    assert!(req.contains("query=riser"), "{req}");
    assert!(req.contains("filter=license"), "{req}");
    assert!(req.contains(&format!("Token {KEY}")), "{req}");
}

#[tokio::test]
async fn import_adds_a_clip_and_a_credit_line_but_never_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let (api, _) = fake_freesound();
    let (client, _) = start(dir.path(), &api, Some(KEY)).await;
    let t = ok(&call(&client, "add_track", json!({"name": "FX", "kind": "audio"})).await);
    let track = t["track"].as_u64().unwrap();
    let r = ok(&call(
        &client,
        "import_sample",
        json!({"id": 11, "track": track, "start_beats": 4.0}),
    )
    .await);
    let audio = r["audio"].as_str().unwrap();
    assert!(dir.path().join("p").join(audio).is_file(), "{r}");
    assert!(
        (r["length_beats"].as_f64().unwrap() - 2.0).abs() < 0.01,
        "1 s at 120 BPM: {r}"
    );
    let notes = std::fs::read_to_string(dir.path().join("p").join("notizen.md")).unwrap();
    assert!(
        notes.contains("Riser 11") && notes.contains("maker") && notes.contains("CC0"),
        "{notes}"
    );
    assert!(notes.contains("https://freesound.org/s/11/"), "{notes}");
    ok(&call(&client, "save", json!({})).await);
    for e in std::fs::read_dir(dir.path().join("p")).unwrap() {
        let p = e.unwrap().path();
        if p.is_file() {
            let text = String::from_utf8_lossy(&std::fs::read(&p).unwrap()).into_owned();
            assert!(!text.contains(KEY), "key in {}", p.display());
        }
    }
}

#[tokio::test]
async fn import_refuses_non_commercial_and_works_without_key_only_with_a_hint() {
    let dir = tempfile::tempdir().unwrap();
    let (api, _) = fake_freesound();
    let (client, _) = start(dir.path(), &api, Some(KEY)).await;
    let t = ok(&call(&client, "add_track", json!({"name": "FX", "kind": "audio"})).await);
    let track = t["track"].as_u64().unwrap();
    let r = call(&client, "import_sample", json!({"id": 12, "track": track})).await;
    assert!(is_err(&r), "{r:?}");
    assert!(!dir.path().join("p").join("notizen.md").exists());
    let none = tempfile::tempdir().unwrap();
    let (client, _) = start(none.path(), &api, None).await;
    let r = call(&client, "search_samples", json!({"text": "riser"})).await;
    assert!(is_err(&r));
    assert!(
        r.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("Freesound-Schlüssel"),
        "{r:?}"
    );
}
