//! Paket 13, Agent-Weg der Piano-Roll: `edit_notes` und `quantize_notes` ändern einen
//! MIDI-Clip über denselben Befehl (`SetNotes`) wie die Oberfläche, je ein Undo-Schritt.

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

fn beats(n: &serde_json::Value) -> Vec<(f64, u64, u64)> {
    n["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| {
            (
                x["beat"].as_f64().unwrap(),
                x["pitch"].as_u64().unwrap(),
                x["velocity"].as_u64().unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
async fn agent_edits_and_quantizes_notes_with_undo() {
    let dir = tempfile::tempdir().unwrap();
    let agent = WoaAgent::open_or_create(dir.path()).unwrap();
    let (c_write, s_read) = tokio::io::duplex(1 << 16);
    let (s_write, c_read) = tokio::io::duplex(1 << 16);
    let server = tokio::spawn({
        let agent = agent.clone();
        async move { woa_agent::serve(agent, (s_read, s_write)).await.unwrap() }
    });
    let client = TestClient.serve((c_read, c_write)).await.unwrap();
    let tools = client.list_all_tools().await.unwrap();
    for t in ["edit_notes", "quantize_notes"] {
        assert!(tools.iter().any(|x| x.name == t), "{t}");
    }

    let piano = ok(&call(
        &client,
        "add_track",
        json!({"name": "Piano", "kind": "midi"}),
    )
    .await)["track"]
        .as_u64()
        .unwrap();
    let notes = json!([
        {"beat": 0.06, "pitch": 60, "length_beats": 0.5, "velocity": 100},
        {"beat": 1.1, "pitch": 64, "length_beats": 0.5, "velocity": 100},
        {"beat": 2.9, "pitch": 67, "length_beats": 0.5, "velocity": 100},
    ]);
    let clip = ok(&call(
        &client,
        "generate_notes",
        json!({"track": piano, "length_beats": 4.0, "notes": notes, "midi": true}),
    )
    .await)["clip"]
        .as_u64()
        .unwrap();

    // Nur die zweite Note auf Viertel, dann alle auf Achtel.
    ok(&call(
        &client,
        "quantize_notes",
        json!({"clip": clip, "division": 4, "notes": [1]}),
    )
    .await);
    let n = ok(&call(&client, "read_notes", json!({"clip": clip})).await);
    assert_eq!(
        beats(&n),
        vec![(0.06, 60, 100), (1.0, 64, 100), (2.9, 67, 100)]
    );
    ok(&call(
        &client,
        "quantize_notes",
        json!({"clip": clip, "division": 8}),
    )
    .await);
    let n = ok(&call(&client, "read_notes", json!({"clip": clip})).await);
    assert_eq!(
        beats(&n),
        vec![(0.0, 60, 100), (1.0, 64, 100), (3.0, 67, 100)]
    );

    // Velocity ändern und eine Note löschen über edit_notes.
    let edited = json!([
        {"beat": 0.0, "pitch": 60, "length_beats": 0.5, "velocity": 40},
        {"beat": 3.0, "pitch": 67, "length_beats": 0.5, "velocity": 100},
    ]);
    ok(&call(
        &client,
        "edit_notes",
        json!({"clip": clip, "notes": edited}),
    )
    .await);
    let n = ok(&call(&client, "read_notes", json!({"clip": clip})).await);
    assert_eq!(beats(&n), vec![(0.0, 60, 40), (3.0, 67, 100)]);

    // Fehlerfälle ändern nichts.
    for (tool, args) in [
        ("quantize_notes", json!({"clip": clip, "division": 3})),
        (
            "quantize_notes",
            json!({"clip": clip, "division": 4, "notes": [7]}),
        ),
        ("quantize_notes", json!({"clip": 9999, "division": 4})),
        (
            "edit_notes",
            json!({"clip": clip, "notes": [{"beat": 5.0, "pitch": 60, "length_beats": 1.0}]}),
        ),
    ] {
        let r = call(&client, tool, args.clone()).await;
        assert_eq!(r.is_error, Some(true), "{tool} {args} should fail: {r:?}");
    }
    let n = ok(&call(&client, "read_notes", json!({"clip": clip})).await);
    assert_eq!(beats(&n), vec![(0.0, 60, 40), (3.0, 67, 100)]);

    // Undo geht Schritt für Schritt zurück.
    ok(&call(&client, "undo", json!({})).await);
    let n = ok(&call(&client, "read_notes", json!({"clip": clip})).await);
    assert_eq!(beats(&n).len(), 3);
    ok(&call(&client, "undo", json!({})).await);
    ok(&call(&client, "undo", json!({})).await);
    let n = ok(&call(&client, "read_notes", json!({"clip": clip})).await);
    assert_eq!(beats(&n)[1], (1.1, 64, 100));

    client.cancel().await.unwrap();
    server.await.unwrap();
}
