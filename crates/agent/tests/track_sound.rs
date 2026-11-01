//! Paket J1: Klang je MIDI-Spur über MCP (`track_sound`) lesen und setzen, Undo, Speichern
//! und Laden; `generate_drums` mit `midi: true` schreibt GM-Drumnoten und setzt das Kit.
//! Die SoundFont ist eine winzige, im Test erzeugte SF2 (über `WOA_SOUNDFONT`).

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
async fn agent_sets_reads_and_keeps_the_track_sound() {
    let dir = tempfile::tempdir().unwrap();
    let font = dir.path().join("tiny.sf2");
    std::fs::write(&font, woa_dsp::soundfont::tiny_sf2()).unwrap();
    // Einziger Test dieser Datei: kein anderer Thread liest die Umgebung.
    unsafe { std::env::set_var("WOA_SOUNDFONT", &font) };
    let proj = dir.path().join("p");
    let agent = WoaAgent::open_or_create(&proj).unwrap();
    let session = agent.session();
    let (c_write, s_read) = tokio::io::duplex(1 << 16);
    let (s_write, c_read) = tokio::io::duplex(1 << 16);
    let server = tokio::spawn({
        let agent = agent.clone();
        async move { woa_agent::serve(agent, (s_read, s_write)).await.unwrap() }
    });
    let client = TestClient.serve((c_read, c_write)).await.unwrap();

    let bass = ok(&call(
        &client,
        "add_track",
        json!({"name": "Bass", "kind": "midi"}),
    )
    .await)["track"]
        .as_u64()
        .unwrap();
    let audio = ok(&call(&client, "add_track", json!({"name": "Audio"})).await)["track"]
        .as_u64()
        .unwrap();

    let r = ok(&call(&client, "track_sound", json!({"track": bass})).await);
    assert!(r["sound"].is_null(), "new MIDI track plays the synth");
    assert_eq!(r["soundfont_available"], true);
    assert_eq!(r["presets"].as_array().unwrap().len(), 16);

    let r = ok(&call(
        &client,
        "track_sound",
        json!({"track": bass, "preset": "E-Bass"}),
    )
    .await);
    assert_eq!(r["sound"]["program"], 33);
    assert_eq!(r["sound"]["preset"], "E-Bass");
    let r = call(
        &client,
        "track_sound",
        json!({"track": audio, "program": 1}),
    )
    .await;
    assert_eq!(r.is_error, Some(true), "audio track has no GM sound");
    let r = call(
        &client,
        "track_sound",
        json!({"track": bass, "preset": "Tuba"}),
    )
    .await;
    assert_eq!(r.is_error, Some(true));

    // Drums als MIDI: GM-Noten, Spur bekommt das Standard-Kit.
    let kit =
        ok(&call(&client, "add_track", json!({"name": "Kit", "kind": "midi"})).await)["track"]
            .as_u64()
            .unwrap();
    let d = ok(&call(
        &client,
        "generate_drums",
        json!({"track": kit, "kick": "x...x...", "hihat": "x.x.x.x.", "midi": true}),
    )
    .await);
    assert_eq!(d["notes"], 6);
    assert_eq!(d["sound"]["drums"], true);
    let notes = ok(&call(&client, "read_notes", json!({"clip": d["clip"]})).await);
    let pitches: Vec<u64> = notes["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["pitch"].as_u64().unwrap())
        .collect();
    assert!(pitches.contains(&36) && pitches.contains(&42));

    ok(&call(&client, "save", json!({})).await);
    let loaded = woa_model::store::load(&proj).unwrap();
    let t = |id: u64| loaded.track(woa_model::TrackId(id)).unwrap().sound;
    assert_eq!(t(bass).map(|x| x.program), Some(33));
    assert_eq!(t(kit).map(|x| x.drums), Some(true));

    // Ein Undo nimmt die Drums samt Kit zurück, der Bass-Klang bleibt.
    ok(&call(&client, "undo", json!({})).await);
    {
        let s = session.lock().unwrap();
        let sound = |id: u64| s.project().track(woa_model::TrackId(id)).unwrap().sound;
        assert_eq!(sound(kit), None);
        let clip = woa_model::ClipId(d["clip"].as_u64().unwrap());
        assert!(s.project().clip(clip).is_none());
        assert_eq!(sound(bass).map(|x| x.program), Some(33));
    }
    let r = ok(&call(
        &client,
        "track_sound",
        json!({"track": bass, "program": 38}),
    )
    .await);
    assert_eq!(r["sound"]["preset"], "Synth-Bass");
    let r = ok(&call(
        &client,
        "track_sound",
        json!({"track": bass, "synth": true}),
    )
    .await);
    assert!(r["sound"].is_null());

    client.cancel().await.unwrap();
    server.await.unwrap();
}
