//! Paket J7 (#271): Bank je MIDI-Spur über MCP (`track_sound` mit `bank`). Ohne Netz: Die
//! Bank liegt schon im Bank-Ordner (winzige, im Test erzeugte SF2 unter dem Katalog-Namen).

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

async fn start(proj: &std::path::Path) -> (Client, tokio::task::JoinHandle<()>) {
    let agent = WoaAgent::open_or_create(proj).unwrap();
    let (c_write, s_read) = tokio::io::duplex(1 << 16);
    let (s_write, c_read) = tokio::io::duplex(1 << 16);
    let server =
        tokio::spawn(async move { woa_agent::serve(agent, (s_read, s_write)).await.unwrap() });
    (TestClient.serve((c_read, c_write)).await.unwrap(), server)
}

#[tokio::test]
async fn agent_picks_a_bank_that_is_installed_and_it_stays_after_save() {
    let dir = tempfile::tempdir().unwrap();
    let banks_dir = dir.path().join("banks");
    std::fs::create_dir_all(&banks_dir).unwrap();
    let entry = woa_engine::banks::entry(woa_model::SoundBank::MusescoreGeneral).unwrap();
    std::fs::write(banks_dir.join(&entry.file), woa_dsp::soundfont::tiny_sf2()).unwrap();
    woa_engine::banks::set_bank_dir(banks_dir);
    let proj = dir.path().join("p");
    let (client, server) = start(&proj).await;
    let keys = ok(&call(
        &client,
        "add_track",
        json!({"name": "Keys", "kind": "midi"}),
    )
    .await)["track"]
        .as_u64()
        .unwrap();

    let r = ok(&call(&client, "track_sound", json!({"track": keys})).await);
    let banks = r["banks"].as_array().unwrap();
    assert_eq!(banks.len(), 2);
    assert_eq!(banks[0]["id"], "musescore-general");
    assert_eq!(banks[0]["state"], "ready");
    assert_eq!(banks[1]["drums_only"], true);
    assert!(banks[1]["license"].as_str().unwrap().contains("CC BY-SA"));

    let r = ok(&call(
        &client,
        "track_sound",
        json!({"track": keys, "preset": "Klavier", "bank": "musescore-general"}),
    )
    .await);
    assert_eq!(r["sound"]["bank"], "musescore-general");
    assert_eq!(r["sound"]["preset"], "Klavier");
    assert_eq!(r["bank_state"], "ready");
    // Nur die Bank wechseln: der Klang bleibt.
    let r = ok(&call(
        &client,
        "track_sound",
        json!({"track": keys, "bank": "generaluser"}),
    )
    .await);
    assert!(r["sound"]["bank"].is_null());
    assert_eq!(r["sound"]["program"], 0);
    ok(&call(
        &client,
        "track_sound",
        json!({"track": keys, "bank": "musescore-general"}),
    )
    .await);
    // Eine reine Drum-Bank für ein Klavier ist ein Fehler; eine unbekannte auch.
    let r = call(
        &client,
        "track_sound",
        json!({"track": keys, "bank": "avl-red-zeppelin"}),
    )
    .await;
    assert_eq!(r.is_error, Some(true));
    let r = call(
        &client,
        "track_sound",
        json!({"track": keys, "bank": "gibt-es-nicht"}),
    )
    .await;
    assert_eq!(r.is_error, Some(true));
    // Bank ohne Klang: Klavier aus der Bank.
    let lead = ok(&call(
        &client,
        "add_track",
        json!({"name": "Lead", "kind": "midi"}),
    )
    .await)["track"]
        .as_u64()
        .unwrap();
    let r = ok(&call(
        &client,
        "track_sound",
        json!({"track": lead, "bank": "musescore-general"}),
    )
    .await);
    assert_eq!(r["sound"]["program"], 0);

    ok(&call(&client, "save", json!({})).await);
    let loaded = woa_model::store::load(&proj).unwrap();
    let sound = loaded
        .track(woa_model::TrackId(keys))
        .unwrap()
        .sound
        .unwrap();
    assert_eq!(sound.bank, Some(woa_model::SoundBank::MusescoreGeneral));

    client.cancel().await.unwrap();
    server.await.unwrap();
}
