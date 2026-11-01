//! Paket J5 (#285): `voice_over` legt die Sprachkette auf die Stimme und duckt das Bett
//! per Sidechain. Nur Offline-Render, kein Ton über Lautsprecher.

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

async fn start(dir: &std::path::Path) -> (Client, WoaAgent) {
    let agent = WoaAgent::open_or_create(dir.join("p")).unwrap();
    let (c_write, s_read) = tokio::io::duplex(1 << 16);
    let (s_write, c_read) = tokio::io::duplex(1 << 16);
    tokio::spawn({
        let agent = agent.clone();
        async move { woa_agent::serve(agent, (s_read, s_write)).await.unwrap() }
    });
    (TestClient.serve((c_read, c_write)).await.unwrap(), agent)
}

/// Bett: Sinus-Akkord über 8 Beats, Stimme: Sinus ab Beat 2 bis Beat 6.
async fn bed_and_voice(client: &Client) -> (u64, u64) {
    let bed = ok(&call(client, "add_track", json!({"name": "Bett"})).await)["track"]
        .as_u64()
        .unwrap();
    let voice = ok(&call(client, "add_track", json!({"name": "Stimme"})).await)["track"]
        .as_u64()
        .unwrap();
    ok(&call(
        client,
        "generate_notes",
        json!({"track": bed, "length_beats": 8.0, "notes": [
            {"beat": 0.0, "pitch": 48, "length_beats": 8.0},
            {"beat": 0.0, "pitch": 55, "length_beats": 8.0}]}),
    )
    .await);
    ok(&call(
        client,
        "generate_notes",
        json!({"track": voice, "start_beats": 2.0, "length_beats": 4.0, "notes": [
            {"beat": 0.0, "pitch": 69, "length_beats": 4.0}]}),
    )
    .await);
    (bed, voice)
}

async fn lufs(client: &Client, track: Option<u64>) -> f64 {
    let args = track.map_or(json!({}), |t| json!({"track": t}));
    let a = ok(&call(client, "analyze", args).await);
    a["lufs"].as_f64().unwrap_or_else(|| panic!("{a}"))
}

#[tokio::test]
async fn voice_over_ducks_the_bed_measurably_with_analyze() {
    let dir = tempfile::tempdir().unwrap();
    let (client, agent) = start(dir.path()).await;
    let (bed, voice) = bed_and_voice(&client).await;
    // Erst nur die Sprachkette, damit der Vergleich allein das Ducking misst.
    ok(&call(&client, "insert_voice_chain", json!({"track": voice})).await);
    let before = lufs(&client, None).await;
    let r = ok(&call(&client, "voice_over", json!({"voice": voice})).await);
    assert_eq!(r["chain"].as_array().map(Vec::len), Some(0), "{r}");
    let ducks = r["ducks"].as_array().unwrap();
    assert_eq!(ducks.len(), 1, "{r}");
    assert_eq!(ducks[0]["track"].as_u64(), Some(bed), "{r}");
    {
        let session = agent.session();
        let s = session.lock().unwrap();
        let t = s.project().track(woa_model::TrackId(bed)).unwrap();
        assert_eq!(t.inserts.len(), 1);
        assert_eq!(t.inserts[0].sidechain, Some(woa_model::TrackId(voice)));
    }
    // Stimme und Sprachkette gleich, nur das Bett wird unter der Stimme leiser.
    let after = lufs(&client, None).await;
    eprintln!("analyze lufs: ohne Ducking {before:.2}, mit Ducking {after:.2}");
    assert!(
        after < before - 1.0,
        "Mischung vorher {before}, nachher {after}"
    );

    // Zweiter Aufruf legt nichts doppelt an.
    let again = ok(&call(&client, "voice_over", json!({"voice": voice})).await);
    assert_eq!(again["chain"].as_array().map(Vec::len), Some(0), "{again}");
    assert_eq!(again["ducks"].as_array().map(Vec::len), Some(0), "{again}");

    // Ein Undo-Schritt nimmt die Ducker zurück, die Sprachkette bleibt.
    ok(&call(&client, "undo", json!({})).await);
    let session = agent.session();
    let s = session.lock().unwrap();
    let p = s.project();
    assert!(p.track(woa_model::TrackId(bed)).unwrap().inserts.is_empty());
    assert_eq!(p.track(woa_model::TrackId(voice)).unwrap().inserts.len(), 5);
}

#[tokio::test]
async fn voice_over_rejects_unknown_tracks_and_buses() {
    let dir = tempfile::tempdir().unwrap();
    let (client, _) = start(dir.path()).await;
    assert!(is_err(
        &call(&client, "voice_over", json!({"voice": 999})).await
    ));
    let bus = ok(&call(&client, "add_track", json!({"name": "Bus", "kind": "bus"})).await)["track"]
        .as_u64()
        .unwrap();
    assert!(is_err(
        &call(&client, "voice_over", json!({"voice": bus})).await
    ));
}
