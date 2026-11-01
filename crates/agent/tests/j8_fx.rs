//! Paket J8 (#283): `generate_fx` erzeugt Riser, Whoosh und Downlifter als Audio-Clip.
//! Nur Offline-Render, kein Ton über Lautsprecher.

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

async fn audio_track(client: &Client) -> u64 {
    let r = ok(&call(client, "add_track", json!({"name": "FX"})).await);
    r["track"].as_u64().unwrap()
}

#[tokio::test]
async fn generate_fx_fits_the_tempo_and_does_not_clip() {
    let dir = tempfile::tempdir().unwrap();
    let (client, _) = start(dir.path()).await;
    ok(&call(&client, "set_tempo", json!({"bpm": 100.0})).await);
    let t = audio_track(&client).await;
    for (i, kind) in ["riser", "whoosh", "downlifter"].iter().enumerate() {
        let r = ok(&call(
            &client,
            "generate_fx",
            json!({"track": t, "kind": kind, "length_beats": 4.0, "start_beats": i as f64 * 4.0}),
        )
        .await);
        assert!(r["clip"].is_u64(), "{r}");
        assert_eq!(r["length_beats"].as_f64(), Some(4.0), "{r}");
        // 4 Beats bei 100 BPM = 2,4 s = 115 200 Samples.
        let wav = dir.path().join("p").join(r["audio"].as_str().unwrap());
        let l = woa_engine::clip::load_wav_mono(&wav, 48_000).unwrap();
        assert!(
            l.len().abs_diff(115_200) <= 1,
            "{kind}: {} samples",
            l.len()
        );
    }
    let a = ok(&call(&client, "analyze", json!({})).await);
    assert_eq!(a["clipping"], false, "{a}");
    assert!(a["lufs"].as_f64().is_some_and(|l| l > -40.0), "silent: {a}");
}

#[tokio::test]
async fn generate_fx_rejects_bad_input() {
    let dir = tempfile::tempdir().unwrap();
    let (client, _) = start(dir.path()).await;
    let t = audio_track(&client).await;
    for bad in [
        json!({"track": t, "kind": "riser", "length_beats": 0.0}),
        json!({"track": t, "kind": "riser", "length_beats": -2.0}),
        json!({"track": 999, "kind": "riser"}),
        json!({"track": t, "kind": "laser"}),
    ] {
        assert!(
            is_err(&call(&client, "generate_fx", bad.clone()).await),
            "{bad}"
        );
    }
}
