//! Befunde aus dem Windows-Test des MCP-Servers: verwaiste Aufnahmen, undo/redo-Antwort,
//! MIDI-Spur in der Analyse. Nichts davon wird abgespielt.

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

fn text(r: &CallToolResult) -> String {
    r.content[0].as_text().unwrap().text.clone()
}

fn ok(r: &CallToolResult) -> serde_json::Value {
    assert_ne!(r.is_error, Some(true), "tool error: {r:?}");
    serde_json::from_str(&text(r)).unwrap()
}

async fn start(dir: &std::path::Path) -> (Client, tokio::task::JoinHandle<()>) {
    let agent = WoaAgent::open_or_create(dir).unwrap();
    let (c_write, s_read) = tokio::io::duplex(1 << 16);
    let (s_write, c_read) = tokio::io::duplex(1 << 16);
    let server =
        tokio::spawn(async move { woa_agent::serve(agent, (s_read, s_write)).await.unwrap() });
    (TestClient.serve((c_read, c_write)).await.unwrap(), server)
}

async fn close(client: Client, server: tokio::task::JoinHandle<()>) {
    client.cancel().await.unwrap();
    server.await.unwrap();
}

async fn tone(client: &Client, track: u64) -> String {
    ok(&call(
        client,
        "generate_notes",
        json!({"track": track, "length_beats": 2.0, "waveform": "sine",
               "notes": [{"beat": 0.0, "pitch": 69, "length_beats": 2.0}]}),
    )
    .await)["audio"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn audio_track(client: &Client) -> u64 {
    ok(&call(client, "add_track", json!({ "name": "Ton" })).await)["track"]
        .as_u64()
        .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn unsaved_generation_leaves_no_file_after_close() {
    let dir = tempfile::tempdir().unwrap();
    let (client, server) = start(dir.path()).await;
    let t = audio_track(&client).await;
    let rel = tone(&client, t).await;
    assert!(
        dir.path().join(&rel).exists(),
        "Take liegt während der Sitzung da"
    );
    close(client, server).await;
    assert!(
        !dir.path().join(&rel).exists(),
        "ungespeichert: {rel} bleibt liegen"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn saved_takes_stay_and_only_unreferenced_ones_go() {
    let dir = tempfile::tempdir().unwrap();
    let (client, server) = start(dir.path()).await;
    let t = audio_track(&client).await;
    let kept = tone(&client, t).await;
    ok(&call(&client, "save", json!({})).await);
    // Danach erzeugt und wieder rückgängig gemacht: nicht im gespeicherten Projekt.
    let gone = tone(&client, t).await;
    ok(&call(&client, "undo", json!({})).await);
    assert!(
        dir.path().join(&gone).exists(),
        "Undo löscht die Datei nicht sofort"
    );
    close(client, server).await;
    assert!(dir.path().join(&kept).exists(), "gespeicherter Take bleibt");
    assert!(!dir.path().join(&gone).exists(), "verwaister Take geht");
}

#[tokio::test(flavor = "current_thread")]
async fn undo_and_redo_name_the_step_just_done() {
    let dir = tempfile::tempdir().unwrap();
    let (client, server) = start(dir.path()).await;
    audio_track(&client).await;
    let u = ok(&call(&client, "undo", json!({})).await);
    let step = u["undone"].as_str().expect("undone").to_string();
    assert!(u["undo"].is_null(), "{u}");
    assert_eq!(u["redo"].as_str(), Some(step.as_str()), "{u}");
    let r = ok(&call(&client, "redo", json!({})).await);
    assert_eq!(r["redone"].as_str(), Some(step.as_str()), "{r}");
    assert_eq!(r["undo"].as_str(), Some(step.as_str()), "{r}");
    assert!(r["redo"].is_null(), "{r}");
    close(client, server).await;
}

#[tokio::test(flavor = "current_thread")]
async fn analyze_lists_the_midi_track() {
    let dir = tempfile::tempdir().unwrap();
    let (client, server) = start(dir.path()).await;
    let piano = ok(&call(
        &client,
        "add_track",
        json!({"name": "Piano", "kind": "midi"}),
    )
    .await)["track"]
        .as_u64()
        .unwrap();
    ok(&call(
        &client,
        "generate_notes",
        json!({"track": piano, "length_beats": 2.0, "midi": true,
               "notes": [{"beat": 0.0, "pitch": 60, "length_beats": 2.0}]}),
    )
    .await);
    let r = ok(&call(&client, "analyze", json!({})).await);
    let tracks = r["tracks"].as_array().unwrap();
    let entry = tracks
        .iter()
        .find(|t| t["track"].as_u64() == Some(piano))
        .unwrap_or_else(|| panic!("MIDI-Spur fehlt: {r}"));
    assert!(entry["peak_dbfs"].as_f64().is_some(), "{entry}");
    close(client, server).await;
}
