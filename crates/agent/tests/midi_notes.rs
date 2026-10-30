//! Paket 12, Agent-Loop: Der Agent legt über MCP eine MIDI-Spur an, schreibt Noten als
//! MIDI-Clip (`generate_notes` mit `midi: true`), liest sie mit `read_notes` zurück,
//! exportiert den Mix (nicht still) und nimmt alles per Undo zurück. Dazu Fehlerfälle.

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

#[tokio::test]
async fn agent_writes_reads_and_exports_midi_notes() {
    let dir = tempfile::tempdir().unwrap();
    let agent = WoaAgent::open_or_create(dir.path()).unwrap();
    let session = agent.session();
    let (c_write, s_read) = tokio::io::duplex(1 << 16);
    let (s_write, c_read) = tokio::io::duplex(1 << 16);
    let server = tokio::spawn({
        let agent = agent.clone();
        async move { woa_agent::serve(agent, (s_read, s_write)).await.unwrap() }
    });
    let client = TestClient.serve((c_read, c_write)).await.unwrap();

    let tools = client.list_all_tools().await.unwrap();
    assert!(tools.iter().any(|t| t.name == "read_notes"));

    let piano = ok(&call(
        &client,
        "add_track",
        json!({"name": "Piano", "kind": "midi"}),
    )
    .await)["track"]
        .as_u64()
        .unwrap();
    let audio = ok(&call(&client, "add_track", json!({"name": "Audio"})).await)["track"]
        .as_u64()
        .unwrap();
    let notes = json!([
        {"beat": 1.0, "pitch": 64, "length_beats": 0.5, "velocity": 70},
        {"beat": 0.0, "pitch": 60, "length_beats": 0.5},
        {"beat": 3.5, "pitch": 67, "length_beats": 2.0, "velocity": 127},
    ]);
    let r = ok(&call(
        &client,
        "generate_notes",
        json!({"track": piano, "start_beats": 4.0, "length_beats": 4.0, "notes": notes, "midi": true}),
    )
    .await);
    assert_eq!(r["midi"], true);
    assert_eq!(r["notes"], 3);
    let clip = r["clip"].as_u64().unwrap();

    let n = ok(&call(&client, "read_notes", json!({"clip": clip})).await);
    assert_eq!(n["track"], piano);
    assert_eq!(n["start_beats"], 4.0);
    let got: Vec<(f64, f64, u64, u64)> = n["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| {
            (
                x["beat"].as_f64().unwrap(),
                x["length_beats"].as_f64().unwrap(),
                x["pitch"].as_u64().unwrap(),
                x["velocity"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        got,
        vec![(0.0, 0.5, 60, 100), (1.0, 0.5, 64, 70), (3.5, 0.5, 67, 127)],
        "sorted, default velocity, last note cut at the clip end"
    );
    assert!(
        !dir.path().join("aufnahmen").exists(),
        "MIDI clip writes no audio file"
    );

    let a = ok(&call(&client, "analyze", json!({})).await);
    assert!(a["lufs"].as_f64().is_some_and(|l| l > -40.0), "{a}");
    let e = ok(&call(&client, "export", json!({"path": "export/midi.wav"})).await);
    assert!(
        e["seconds"]
            .as_f64()
            .is_some_and(|s| (s - 4.0).abs() < 1e-6),
        "{e}"
    );
    let (l, _, _) = woa_engine::read_wav_stereo(&dir.path().join("export/midi.wav")).unwrap();
    let first = l.iter().position(|v| v.abs() > 1e-4).unwrap();
    // Clip ab Beat 4 bei 120 BPM = 2 s.
    assert!(first.abs_diff(96_000) <= 48, "first note at {first}");

    // Fehlerfälle: MIDI auf Audio-Spur, read_notes auf Nicht-MIDI-Clip, leere Noten.
    let before = session.lock().unwrap().project().clone();
    for (tool, args) in [
        (
            "generate_notes",
            json!({"track": audio, "length_beats": 1.0, "notes": [{"beat": 0.0, "pitch": 60, "length_beats": 1.0}], "midi": true}),
        ),
        (
            "generate_notes",
            json!({"track": piano, "length_beats": 1.0, "notes": [], "midi": true}),
        ),
        (
            "generate_notes",
            json!({"track": piano, "length_beats": 1.0, "notes": [{"beat": 2.0, "pitch": 60, "length_beats": 1.0}], "midi": true}),
        ),
        ("read_notes", json!({"clip": 9999})),
    ] {
        let r = call(&client, tool, args.clone()).await;
        assert_eq!(r.is_error, Some(true), "{tool} {args} should fail: {r:?}");
    }
    let placeholder = ok(&call(
        &client,
        "add_clip",
        json!({"track": audio, "name": "x", "start_beats": 0.0, "length_beats": 1.0}),
    )
    .await)["clip"]
        .as_u64()
        .unwrap();
    let r = call(&client, "read_notes", json!({"clip": placeholder})).await;
    assert_eq!(r.is_error, Some(true));
    ok(&call(&client, "undo", json!({})).await);
    assert_eq!(session.lock().unwrap().project().tracks, before.tracks);

    // Undo nimmt den MIDI-Clip zurück, Speichern hält die Noten.
    ok(&call(&client, "save", json!({})).await);
    let saved = std::fs::read_to_string(dir.path().join("project.json")).unwrap();
    assert!(saved.contains("\"midi\""), "notes in the project file");
    ok(&call(&client, "undo", json!({})).await);
    assert!(
        session
            .lock()
            .unwrap()
            .project()
            .clip(woa_model::ClipId(clip))
            .is_none()
    );

    client.cancel().await.unwrap();
    server.await.unwrap();
}
