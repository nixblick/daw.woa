//! #608: Served to the network (HTTP without TLS), `import_audio` only reads from the
//! project folder and the WOA data folder. Locally (stdio, 127.0.0.1) any path works.

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests duerfen abbrechen (Regel 6 gilt fuer Produktivcode).

use rmcp::model::{CallToolRequestParams, CallToolResult, ClientConfig};
use rmcp::service::RunningService;
use rmcp::{ClientHandler, RoleClient, ServiceExt};
use serde_json::json;
use std::path::Path;
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

fn text(r: &CallToolResult) -> String {
    r.content[0].as_text().unwrap().text.clone()
}

async fn start(agent: WoaAgent) -> Client {
    let (c_write, s_read) = tokio::io::duplex(1 << 16);
    let (s_write, c_read) = tokio::io::duplex(1 << 16);
    tokio::spawn(async move { woa_agent::serve(agent, (s_read, s_write)).await.unwrap() });
    TestClient.serve((c_read, c_write)).await.unwrap()
}

fn wav(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for _ in 0..48_000 {
        w.write_sample(0i16).unwrap();
    }
    w.finalize().unwrap();
}

async fn import(client: &Client, path: &Path) -> CallToolResult {
    let t: serde_json::Value = serde_json::from_str(&text(
        &call(client, "add_track", json!({"name": "A"})).await,
    ))
    .unwrap();
    call(
        client,
        "import_audio",
        json!({"track": t["track"], "path": path.display().to_string()}),
    )
    .await
}

#[tokio::test]
async fn network_mode_limits_import_to_project_and_data_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let outside = tmp.path().join("musik").join("song.wav");
    wav(&outside);
    let project = tmp.path().join("p.woa");
    let inside = project.join("eigen").join("take.wav");
    wav(&inside);

    // Local: any absolute path is fine.
    let local = start(WoaAgent::open_or_create(&project).unwrap()).await;
    let r = import(&local, &outside).await;
    assert_ne!(r.is_error, Some(true), "local import failed: {r:?}");

    // Network: a foreign path is refused before it is read, the message names the way out.
    let net = start(WoaAgent::open_or_create(&project).unwrap().from_network()).await;
    let r = import(&net, &outside).await;
    assert_eq!(
        r.is_error,
        Some(true),
        "network import of {outside:?}: {r:?}"
    );
    assert!(text(&r).contains("SSH-Tunnel"), "{}", text(&r));
    // `..` out of the project does not get around it.
    let sneaky = project.join("..").join("musik").join("song.wav");
    let r = import(&net, &sneaky).await;
    assert_eq!(
        r.is_error,
        Some(true),
        "network import of {sneaky:?}: {r:?}"
    );

    // Network: files inside the project folder still work.
    let r = import(&net, &inside).await;
    assert_ne!(r.is_error, Some(true), "network import from project: {r:?}");
}
