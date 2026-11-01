//! Paket J7 (#271): Ohne Bank-Ordner (und damit ohne Download) bleibt die Wahl erhalten,
//! die Spur spielt GeneralUser GS und `track_sound` meldet den Zustand `failed`.

use rmcp::model::{CallToolRequestParams, CallToolResult, ClientConfig};
use rmcp::{ClientHandler, ServiceExt};
use serde_json::json;
use woa_agent::WoaAgent;

#[derive(Clone, Default)]
struct TestClient;

impl ClientHandler for TestClient {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::default()
    }
}

fn ok(r: &CallToolResult) -> serde_json::Value {
    assert_ne!(r.is_error, Some(true), "tool error: {r:?}");
    serde_json::from_str(&r.content[0].as_text().unwrap().text).unwrap()
}

#[tokio::test]
async fn without_bank_folder_the_choice_stays_and_generaluser_plays() {
    let dir = tempfile::tempdir().unwrap();
    let agent = WoaAgent::open_or_create(dir.path().join("p")).unwrap();
    let (c_write, s_read) = tokio::io::duplex(1 << 16);
    let (s_write, c_read) = tokio::io::duplex(1 << 16);
    let server =
        tokio::spawn(async move { woa_agent::serve(agent, (s_read, s_write)).await.unwrap() });
    let client = TestClient.serve((c_read, c_write)).await.unwrap();
    let call = |name: &'static str, args: serde_json::Value| {
        let mut p = CallToolRequestParams::new(name);
        if let serde_json::Value::Object(m) = args {
            p = p.with_arguments(m);
        }
        client.call_tool(p)
    };
    let kit = ok(&call("add_track", json!({"name": "Kit", "kind": "midi"}))
        .await
        .unwrap())["track"]
        .as_u64()
        .unwrap();
    let r = ok(&call(
        "track_sound",
        json!({"track": kit, "preset": "Standard-Kit", "bank": "avl-red-zeppelin"}),
    )
    .await
    .unwrap());
    assert_eq!(r["sound"]["bank"], "avl-red-zeppelin");
    assert_eq!(r["bank_state"], "failed");
    assert!(r["bank_error"].as_str().unwrap().contains("Datenordner"));
    client.cancel().await.unwrap();
    server.await.unwrap();
}
