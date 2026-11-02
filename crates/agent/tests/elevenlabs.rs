//! ElevenLabs-Anbindung ohne Netz und ohne Schlüsselbund: ein lokaler Testserver spielt
//! ElevenLabs (Konto, Stimmen, Sprache, Effekt, Musik, Fehlerfälle), der Schlüssel ist ein
//! Platzhalter. Kein echter Aufruf, kein Ton.

use std::io::{BufRead, BufReader, Read, Write};
use std::sync::{Arc, Mutex};

use rmcp::model::{CallToolRequestParams, CallToolResult, ClientConfig};
use rmcp::service::RunningService;
use rmcp::{ClientHandler, RoleClient, ServiceExt};
use serde_json::json;
use woa_agent::WoaAgent;
use woa_agent::elevenlabs::{
    self, Ablage, BettAuftrag, EffektAuftrag, Fehler, Klangerzeuger, Source, SprachAuftrag,
    Sprecher, TtsModell,
};

const KEY: &str = "platzhalter-schluessel-4711";

/// Eine gesehene Anfrage.
#[derive(Debug, Clone)]
struct Seen {
    line: String,
    key: String,
    body: String,
}

/// Wie der Testserver antwortet.
#[derive(Clone, Copy)]
enum Mode {
    Ok,
    Status(u16, &'static str),
}

fn fake(mode: Mode) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    // Echtes MP3 (wie ElevenLabs liefert), aus den Testdaten der Bibliothek.
    let audio = include_bytes!("../../library/tests/data/ton.mp3").to_vec();
    std::thread::spawn(move || {
        for stream in l.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut r = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if r.read_line(&mut line).unwrap_or(0) == 0 {
                continue;
            }
            let (mut key, mut len) = (String::new(), 0usize);
            loop {
                let mut h = String::new();
                if r.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                    break;
                }
                if let Some((k, v)) = h.split_once(':') {
                    let k = k.trim().to_ascii_lowercase();
                    if k == "xi-api-key" {
                        key = v.trim().to_string();
                    } else if k == "content-length" {
                        len = v.trim().parse().unwrap_or(0);
                    }
                }
            }
            let mut body = vec![0u8; len];
            let _ = r.read_exact(&mut body);
            let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
            s.lock().unwrap().push(Seen {
                line: line.trim().to_string(),
                key,
                body: String::from_utf8_lossy(&body).into_owned(),
            });
            let (status, ctype, out): (u16, &str, Vec<u8>) = match mode {
                Mode::Status(code, b) => (code, "application/json", b.as_bytes().to_vec()),
                Mode::Ok if path.starts_with("/v1/user/subscription") => (
                    200,
                    "application/json",
                    json!({"tier": "free", "character_count": 1200, "character_limit": 10000,
                           "next_character_count_reset_unix": 1_800_000_000})
                    .to_string()
                    .into_bytes(),
                ),
                Mode::Ok if path.starts_with("/v2/voices") => {
                    let v = if path.contains("next_page_token=seite2") {
                        json!({"voices": [{"voice_id": "Stimme2", "name": "Bert", "category": "premade"}],
                               "has_more": false, "total_count": 0, "next_page_token": null})
                    } else {
                        json!({"voices": [{"voice_id": "Stimme1", "name": "Anna", "category": "premade"},
                                          {"voice_id": "../boese", "name": "X"}],
                               "has_more": true, "total_count": 0, "next_page_token": "seite2"})
                    };
                    (200, "application/json", v.to_string().into_bytes())
                }
                Mode::Ok
                    if path.starts_with("/v1/text-to-speech/")
                        || path.starts_with("/v1/sound-generation")
                        || path.starts_with("/v1/music") =>
                {
                    (200, "audio/mpeg", audio.clone())
                }
                Mode::Ok => (404, "text/plain", b"nope".to_vec()),
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status} X\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                out.len()
            );
            let _ = stream.write_all(&out);
        }
    });
    (base, seen)
}

fn src(api: &str, key: Option<&'static str>) -> Source {
    Source {
        api: api.to_string(),
        key: Arc::new(move || key.map(str::to_string)),
    }
}

#[test]
fn account_and_voices_with_paging() {
    let (api, seen) = fake(Mode::Ok);
    let s = src(&api, Some(KEY));
    let k = s.konto().unwrap();
    assert_eq!(k.remaining(), 8800);
    assert_eq!(k.tier, "free");
    assert!(k.label().contains("8800"));
    let v = s.stimmen().unwrap();
    let names: Vec<_> = v.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Anna", "Bert"], "ungültige Kennung fällt raus");
    let seen = seen.lock().unwrap();
    assert!(seen[0].line.starts_with("GET /v1/user/subscription"));
    assert!(seen[1].line.contains("page_size=100"));
    assert!(seen[2].line.contains("next_page_token=seite2"));
    assert!(seen.iter().all(|r| r.key == KEY));
}

#[test]
fn speech_effect_and_bed_send_documented_fields() {
    let (api, seen) = fake(Mode::Ok);
    let s = src(&api, Some(KEY));
    let mut a = SprachAuftrag::new("Hallo, hier ist WOA!", "Stimme1");
    a.model = TtsModell::V3;
    a.stability = 0.3;
    let audio = s.sprechen(&a).unwrap();
    assert!(!audio.bytes.is_empty());
    s.effekt(&EffektAuftrag {
        text: "Rakete startet".into(),
        seconds: Some(45.0),
    })
    .unwrap();
    s.bett(&BettAuftrag {
        text: "fröhliches Bett".into(),
        seconds: 20.0,
    })
    .unwrap();
    let seen = seen.lock().unwrap();
    assert!(
        seen[0]
            .line
            .starts_with("POST /v1/text-to-speech/Stimme1?output_format=mp3_44100_128")
    );
    let b: serde_json::Value = serde_json::from_str(&seen[0].body).unwrap();
    assert_eq!(b["model_id"], "eleven_v3");
    assert_eq!(b["language_code"], "de");
    assert_eq!(b["voice_settings"]["stability"], 0.3);
    assert!(
        seen[1]
            .line
            .starts_with("POST /v1/sound-generation?output_format=")
    );
    let b: serde_json::Value = serde_json::from_str(&seen[1].body).unwrap();
    assert_eq!(b["duration_seconds"], 30.0);
    assert!(
        seen[2]
            .line
            .starts_with("POST /v1/music?output_format=mp3_44100_128")
    );
    let b: serde_json::Value = serde_json::from_str(&seen[2].body).unwrap();
    assert_eq!(b["music_length_ms"], 20_000);
    assert_eq!(b["force_instrumental"], true);
}

#[test]
fn errors_are_clear_german_and_never_carry_the_key() {
    type Want = fn(&Fehler) -> bool;
    let cases: [(u16, &str, Want); 5] = [
        (
            401,
            r#"{"detail":{"type":"authentication_error","code":"invalid_api_key","message":"platzhalter-schluessel-4711 is invalid"}}"#,
            |e| *e == Fehler::SchluesselAbgelehnt,
        ),
        (
            402,
            r#"{"detail":{"type":"payment_required","code":"insufficient_credits"}}"#,
            |e| *e == Fehler::Kontingent,
        ),
        (
            401,
            r#"{"detail":{"status":"quota_exceeded","message":"quota"}}"#,
            |e| *e == Fehler::Kontingent,
        ),
        (429, r#"{"detail":{"code":"rate_limit_exceeded"}}"#, |e| {
            *e == Fehler::ZuViele
        }),
        (
            422,
            r#"{"detail":[{"loc":["body","text"],"msg":"key platzhalter-schluessel-4711 too long","type":"x"}]}"#,
            |e| matches!(e, Fehler::Eingabe(_)),
        ),
    ];
    for (code, body, want) in cases {
        let (api, _) = fake(Mode::Status(code, body));
        let e = src(&api, Some(KEY))
            .sprechen(&SprachAuftrag::new("Hallo", "Stimme1"))
            .unwrap_err();
        assert!(want(&e), "HTTP {code}: {e:?}");
        let text = format!("{e} {e:?}");
        assert!(!text.contains(KEY), "Schlüssel im Fehler: {text}");
        assert!(text.chars().any(|c| "äöüÄÖÜß".contains(c)) || text.contains("ElevenLabs"));
    }
}

#[test]
fn no_key_and_no_network_stop_cleanly() {
    let (api, seen) = fake(Mode::Ok);
    let e = src(&api, None).konto().unwrap_err();
    assert_eq!(e, Fehler::KeinSchluessel);
    assert!(e.to_string().contains("Einstellungen"));
    assert!(
        seen.lock().unwrap().is_empty(),
        "ohne Schlüssel kein Aufruf"
    );

    // Geschlossener Port: Netzfehler, kein Absturz, kein Schlüssel.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let e = src(&format!("http://127.0.0.1:{port}"), Some(KEY))
        .konto()
        .unwrap_err();
    assert!(matches!(e, Fehler::Netz(_)), "{e:?}");
    assert!(!format!("{e} {e:?}").contains(KEY));

    // Nur https nach draußen.
    let e = src("http://api.elevenlabs.io", Some(KEY))
        .konto()
        .unwrap_err();
    assert!(matches!(e, Fehler::Netz(_)));
}

#[test]
fn source_debug_is_impossible_and_audio_debug_is_short() {
    let a = elevenlabs::Audio {
        bytes: vec![1, 2, 3],
        ext: "mp3",
    };
    assert_eq!(format!("{a:?}"), "Audio(3 Bytes .mp3)");
}

type Client = RunningService<RoleClient, TestClient>;

#[derive(Clone, Default)]
struct TestClient;

impl ClientHandler for TestClient {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::default()
    }
}

async fn call(client: &Client, name: &'static str, args: serde_json::Value) -> CallToolResult {
    let mut params = CallToolRequestParams::new(name);
    if let serde_json::Value::Object(map) = args {
        params = params.with_arguments(map);
    }
    client.call_tool(params).await.unwrap()
}

fn text(r: &CallToolResult) -> String {
    r.content[0].as_text().unwrap().text.clone()
}

async fn start(dir: &std::path::Path, source: Source, store: Option<Ablage>) -> (Client, WoaAgent) {
    let agent = WoaAgent::open_or_create(dir.join("p"))
        .unwrap()
        .with_voice_source(source, store);
    let (c_write, s_read) = tokio::io::duplex(1 << 16);
    let (s_write, c_read) = tokio::io::duplex(1 << 16);
    tokio::spawn({
        let agent = agent.clone();
        async move { woa_agent::serve(agent, (s_read, s_write)).await.unwrap() }
    });
    (TestClient.serve((c_read, c_write)).await.unwrap(), agent)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_tools_put_clips_on_tracks_and_store_origin() {
    let tmp = tempfile::tempdir().unwrap();
    let (api, seen) = fake(Mode::Ok);
    let store = Ablage {
        media_dir: tmp.path().join("medien"),
        db: tmp.path().join("bibliothek.sqlite3"),
    };
    let (client, agent) = start(tmp.path(), src(&api, Some(KEY)), Some(store.clone())).await;
    let t = call(
        &client,
        "add_track",
        json!({"name": "Sprecher", "kind": "audio"}),
    )
    .await;
    let track: serde_json::Value = serde_json::from_str(&text(&t)).unwrap();
    let track = track["track"].as_u64().or(track["id"].as_u64()).unwrap();

    let r = call(
        &client,
        "tts_generate",
        json!({"text": "Wir on Air!", "voice": "bert", "track": track}),
    )
    .await;
    assert_ne!(r.is_error, Some(true), "{}", text(&r));
    let v: serde_json::Value = serde_json::from_str(&text(&r)).unwrap();
    let file = std::path::PathBuf::from(v["file"].as_str().unwrap());
    assert!(file.is_file());
    assert!(v["length_beats"].as_f64().unwrap() > 0.0);
    assert!(v["library_id"].as_i64().is_some(), "{v}");
    assert!(std::path::Path::new(v["media_file"].as_str().unwrap()).starts_with(&store.media_dir));
    let h: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(elevenlabs::herkunft_path(&file)).unwrap())
            .unwrap();
    assert_eq!(h["anbieter"], "ElevenLabs");
    assert_eq!(h["text"], "Wir on Air!");
    assert_eq!(h["stimme"], "Bert");
    assert!(
        seen.lock()
            .unwrap()
            .iter()
            .any(|s| s.line.starts_with("POST /v1/text-to-speech/Stimme2"))
    );

    for (tool, args) in [
        (
            "sfx_generate",
            json!({"text": "Böller", "seconds": 2.0, "track": track, "start_beats": 4.0}),
        ),
        (
            "music_generate",
            json!({"text": "Sommerbett", "seconds": 10.0, "track": track, "start_beats": 8.0}),
        ),
    ] {
        let r = call(&client, tool, args).await;
        assert_ne!(r.is_error, Some(true), "{tool}: {}", text(&r));
        assert!(!text(&r).contains(KEY));
    }
    let clips: usize = agent
        .session()
        .lock()
        .unwrap()
        .project()
        .tracks
        .iter()
        .map(|t| t.clips.len())
        .sum();
    assert_eq!(clips, 3);
    let _ = client.cancel().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_tools_without_key_fail_in_german_without_network() {
    let tmp = tempfile::tempdir().unwrap();
    let (api, seen) = fake(Mode::Ok);
    let (client, _agent) = start(tmp.path(), src(&api, None), None).await;
    let t = call(
        &client,
        "add_track",
        json!({"name": "Sprecher", "kind": "audio"}),
    )
    .await;
    let track: serde_json::Value = serde_json::from_str(&text(&t)).unwrap();
    let track = track["track"].as_u64().or(track["id"].as_u64()).unwrap();
    for tool in ["tts_generate", "sfx_generate"] {
        let r = call(&client, tool, json!({"text": "x", "track": track})).await;
        assert_eq!(r.is_error, Some(true));
        assert!(
            text(&r).contains("Kein ElevenLabs-Schlüssel"),
            "{}",
            text(&r)
        );
    }
    let r = call(
        &client,
        "music_generate",
        json!({"text": "x", "seconds": 5.0, "track": track}),
    )
    .await;
    assert_eq!(r.is_error, Some(true));
    assert!(seen.lock().unwrap().is_empty());
    // 401 bei gesetztem Schlüssel: klare Meldung, kein Schlüssel in der Antwort.
    let (api, _) = fake(Mode::Status(
        401,
        r#"{"detail":{"code":"invalid_api_key"}}"#,
    ));
    let tmp2 = tempfile::tempdir().unwrap();
    let (client2, _a) = start(tmp2.path(), src(&api, Some(KEY)), None).await;
    let t = call(
        &client2,
        "add_track",
        json!({"name": "Sprecher", "kind": "audio"}),
    )
    .await;
    let track: serde_json::Value = serde_json::from_str(&text(&t)).unwrap();
    let track = track["track"].as_u64().or(track["id"].as_u64()).unwrap();
    let r = call(
        &client2,
        "sfx_generate",
        json!({"text": "x", "track": track}),
    )
    .await;
    assert_eq!(r.is_error, Some(true));
    assert!(text(&r).contains("401") && !text(&r).contains(KEY));
    let _ = client.cancel().await;
    let _ = client2.cancel().await;
}
