//! J6 acceptance (#261): `export_jingle` puts the finished jingle into the jingle folder,
//! the library (kind jingle) and onto a cart, exactly like the app.

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

#[tokio::test(flavor = "current_thread")]
async fn export_jingle_reaches_folder_library_and_cart() {
    let tmp = tempfile::tempdir().unwrap();
    // Eigener Datenordner nur für diesen Testlauf (eigenes Testprogramm, ein Test).
    unsafe { std::env::set_var("APPDATA", tmp.path().join("app")) };
    let woa = tmp.path().join("app").join("woa");
    std::fs::create_dir_all(&woa).unwrap();
    let jingles = tmp.path().join("eigene Jingles");
    std::fs::write(
        woa.join("config.json"),
        serde_json::to_string(&json!({"ordner": {"eigene": {"jingles": jingles}}})).unwrap(),
    )
    .unwrap();

    let (client, _agent, server) = start(&tmp.path().join("proj")).await;
    let t = ok(&call(&client, "add_track", json!({"name": "Signet"})).await)["track"]
        .as_u64()
        .unwrap();
    let notes = json!([
        {"beat": 0.0, "pitch": 72, "length_beats": 0.5, "velocity": 127},
        {"beat": 0.5, "pitch": 76, "length_beats": 0.5, "velocity": 127},
        {"beat": 1.0, "pitch": 79, "length_beats": 3.0, "velocity": 127}
    ]);
    ok(&call(
        &client,
        "generate_notes",
        json!({"track": t, "name": "Signet", "length_beats": 4.0, "waveform": "saw", "notes": notes}),
    )
    .await);

    for bad in [json!({"name": "../weg"}), json!({"name": "x", "cart": 17})] {
        let r = call(&client, "export_jingle", bad).await;
        assert_eq!(r.is_error, Some(true), "{r:?}");
    }

    let r = ok(&call(
        &client,
        "export_jingle",
        json!({"name": "Signet", "cart": 5, "target_lufs": -16.0}),
    )
    .await);
    let path = jingles.join("Signet.wav");
    assert_eq!(r["path"].as_str().unwrap(), path.to_str().unwrap(), "{r}");
    assert!(path.is_file());
    assert!((r["lufs"].as_f64().unwrap() + 16.0).abs() <= 0.5, "{r}");
    assert!(
        r["sample_peak_dbfs"].as_f64().unwrap() <= -1.0 + 1e-3,
        "{r}"
    );
    assert_eq!(r["cart"], 5);

    let lib = woa_library::Library::open(&woa.join(woa_library::DB_FILE)).unwrap();
    let row = lib.by_path(path.to_str().unwrap()).unwrap().unwrap();
    assert_eq!(row.kind, woa_library::Kind::Jingle);
    let show = woa_radio::RadioShow::load(&woa.join("radio.json")).unwrap();
    assert_eq!(show.carts[4].as_ref().unwrap().path, path);

    client.cancel().await.unwrap();
    server.await.unwrap();
}
