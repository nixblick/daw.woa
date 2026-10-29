//! Radio tools: `build_playlist` writes radio.json with measured titles, `run_order`
//! reports order, total length and loudness.

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

async fn start(dir: &std::path::Path) -> (Client, WoaAgent, tokio::task::JoinHandle<()>) {
    let agent = WoaAgent::open_or_create(dir).unwrap();
    let (c_write, s_read) = tokio::io::duplex(1 << 16);
    let (s_write, c_read) = tokio::io::duplex(1 << 16);
    let server = tokio::spawn({
        let agent = agent.clone();
        async move { woa_agent::serve(agent, (s_read, s_write)).await.unwrap() }
    });
    let client = TestClient.serve((c_read, c_write)).await.unwrap();
    (client, agent, server)
}

fn tone(path: &std::path::Path, seconds: u32, amp: f32) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 44_100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..seconds * 44_100 {
        let v = (i as f32 * 440.0 * std::f32::consts::TAU / 44_100.0).sin() * amp;
        let s = (v * 32_767.0) as i16;
        w.write_sample(s).unwrap();
        w.write_sample(s).unwrap();
    }
    w.finalize().unwrap();
}

#[tokio::test]
async fn build_playlist_and_run_order() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("p.woa");
    std::fs::create_dir_all(dir.join("musik")).unwrap();
    tone(&dir.join("musik/a.wav"), 10, 0.05);
    tone(&dir.join("musik/b.wav"), 8, 0.5);
    tone(&dir.join("musik/c.wav"), 6, 0.2);
    let radio = dir.join("sendung/radio.json");
    let radio_s = "sendung/radio.json";
    let (client, _agent, _server) = start(&dir).await;

    let r = ok(&call(
        &client,
        "build_playlist",
        json!({"files": ["musik/a.wav", "musik/b.wav"], "radio_json": radio_s}),
    )
    .await);
    assert_eq!(r["count"], 2, "{r}");
    let r = ok(&call(
        &client,
        "build_playlist",
        json!({"files": ["musik/c.wav"], "append": true, "radio_json": radio_s}),
    )
    .await);
    assert_eq!(r["count"], 3, "{r}");

    let o = ok(&call(&client, "run_order", json!({"radio_json": radio_s})).await);
    let titles = o["titles"].as_array().unwrap();
    assert_eq!(titles[0]["title"], "a");
    assert_eq!(titles[2]["title"], "c");
    // 10 + 8 + 6 minus two crossfades of 3 s.
    assert!(
        (o["total_seconds"].as_f64().unwrap() - 18.0).abs() < 0.01,
        "{o}"
    );
    let quiet = titles[0]["lufs"].as_f64().unwrap();
    let loud = titles[1]["lufs"].as_f64().unwrap();
    assert!(loud - quiet > 15.0, "{quiet} {loud}");
    assert!(titles[0]["gain_db"].as_f64().unwrap() > titles[1]["gain_db"].as_f64().unwrap());

    // Saved with format version, and readable by the radio crate.
    let show = woa_radio::RadioShow::load(&radio).unwrap();
    assert_eq!(show.playlist.len(), 3);
    assert!(show.playlist.iter().all(|e| e.is_measured()));

    // Replace, then errors: unsupported format, missing file. radio.json stays intact.
    let r = ok(&call(
        &client,
        "build_playlist",
        json!({"files": ["musik/b.wav"], "radio_json": radio_s}),
    )
    .await);
    assert_eq!(r["count"], 1);
    for f in ["musik/x.mp3", "musik/fehlt.wav"] {
        let e = call(
            &client,
            "build_playlist",
            json!({"files": [f], "radio_json": radio_s}),
        )
        .await;
        assert_eq!(e.is_error, Some(true), "{f}");
    }
    assert_eq!(
        woa_radio::RadioShow::load(&radio).unwrap().playlist.len(),
        1
    );

    // Write targets outside the project folder, or a symlink, are refused.
    std::fs::write(tmp.path().join("x.json"), "unberührt").unwrap();
    let outside = tmp.path().join("x.json").display().to_string();
    for bad in ["../x.json", outside.as_str(), "neu/../../x.json"] {
        let e = call(
            &client,
            "build_playlist",
            json!({"files": ["musik/a.wav"], "radio_json": bad}),
        )
        .await;
        assert_eq!(e.is_error, Some(true), "{bad}");
    }
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("x.json")).unwrap(),
        "unberührt"
    );
    assert!(!dir.join("neu").exists(), "no folder left behind");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(tmp.path().join("x.json"), dir.join("link.json")).unwrap();
        let e = call(
            &client,
            "build_playlist",
            json!({"files": ["musik/a.wav"], "radio_json": "link.json"}),
        )
        .await;
        assert_eq!(e.is_error, Some(true), "symlink target");
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("x.json")).unwrap(),
            "unberührt"
        );
    }
}
