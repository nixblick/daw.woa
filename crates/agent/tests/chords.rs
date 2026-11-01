//! Paket J3: `generate_chords` legt dieselben drei Clips an wie die Oberfläche, `transpose_notes` wechselt die Tonart.

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
async fn chords_make_three_tracks_and_transpose_moves_them() {
    let dir = tempfile::tempdir().unwrap();
    let agent = WoaAgent::open_or_create(dir.path().join("p")).unwrap();
    let (c_write, s_read) = tokio::io::duplex(1 << 16);
    let (s_write, c_read) = tokio::io::duplex(1 << 16);
    let server = tokio::spawn({
        let agent = agent.clone();
        async move { woa_agent::serve(agent, (s_read, s_write)).await.unwrap() }
    });
    let client = TestClient.serve((c_read, c_write)).await.unwrap();
    let r = ok(&call(
        &client,
        "generate_chords",
        json!({"progression": "I-V-vi-IV", "key": "D", "bars": 4}),
    )
    .await);
    let want =
        woa_model::chords::generate(woa_model::chords::progression("I–V–vi–IV").unwrap(), 2, 4);
    let parts = r["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0]["role"], "chords");
    assert_eq!(parts[0]["notes"], want.chords.notes.len());
    assert_eq!(parts[1]["notes"], want.bass.notes.len());
    assert_eq!(parts[2]["notes"], want.arp.notes.len());
    assert_eq!(r["length_beats"], 16.0);
    let clips: Vec<u64> = parts.iter().map(|p| p["clip"].as_u64().unwrap()).collect();
    let t = ok(&call(
        &client,
        "transpose_notes",
        json!({"clips": clips, "semitones": -2}),
    )
    .await);
    assert_eq!(t["clips"], 3);
    let notes = ok(&call(&client, "read_notes", json!({"clip": clips[0]})).await);
    let first = notes["notes"][0]["pitch"].as_u64().unwrap();
    assert_eq!(first, u64::from(want.chords.notes[0].pitch) - 2);
    let bad = call(&client, "generate_chords", json!({"progression": "Polka"})).await;
    assert_eq!(bad.is_error, Some(true));
    let bad = call(
        &client,
        "generate_chords",
        json!({"progression": "I–IV", "key": "H"}),
    )
    .await;
    assert_eq!(bad.is_error, Some(true));
    client.cancel().await.unwrap();
    server.await.unwrap();
}
