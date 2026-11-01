//! Paket J2: `generate_drums` mit `preset` legt dieselben Raster-Noten an wie die Oberfläche.

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
async fn preset_writes_the_same_notes_as_the_grid() {
    let dir = tempfile::tempdir().unwrap();
    let agent = WoaAgent::open_or_create(dir.path().join("p")).unwrap();
    let (c_write, s_read) = tokio::io::duplex(1 << 16);
    let (s_write, c_read) = tokio::io::duplex(1 << 16);
    let server = tokio::spawn({
        let agent = agent.clone();
        async move { woa_agent::serve(agent, (s_read, s_write)).await.unwrap() }
    });
    let client = TestClient.serve((c_read, c_write)).await.unwrap();
    let kit =
        ok(&call(&client, "add_track", json!({"name": "Kit", "kind": "midi"})).await)["track"]
            .as_u64()
            .unwrap();
    let d = ok(&call(
        &client,
        "generate_drums",
        json!({"track": kit, "preset": "pop", "bars": 2, "density": 1}),
    )
    .await);
    assert_eq!(d["length_beats"], 8.0);
    assert_eq!(d["sound"]["drums"], true);
    let want = woa_model::drums::apply_preset(
        &woa_model::MidiData::default(),
        8.0,
        woa_model::drum_preset("Pop").unwrap(),
        1,
    );
    assert_eq!(d["notes"], want.notes.len());
    let r = call(
        &client,
        "generate_drums",
        json!({"track": kit, "preset": "Polka"}),
    )
    .await;
    assert_eq!(r.is_error, Some(true));
    client.cancel().await.unwrap();
    server.await.unwrap();
}
