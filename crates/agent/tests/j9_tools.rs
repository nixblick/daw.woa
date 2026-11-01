//! Paket J9: `list_instruments`, `list_samples` und CLAP-Effekte über `insert_effect`.
//! Keine echte Plugin-Suche (`clap` bleibt aus), keine Nutzerdaten: Bibliothek und Ordner
//! liegen im Temp-Ordner.

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

#[tokio::test]
async fn list_instruments_shows_banks_presets_and_no_scan_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let (client, _) = start(dir.path()).await;
    let r = ok(&call(&client, "list_instruments", json!({})).await);
    let banks = r["banks"].as_array().unwrap();
    assert_eq!(r["default_bank"]["id"], "generaluser", "{r}");
    assert!(banks.iter().any(|b| b["id"] == "musescore-general"), "{r}");
    for b in banks {
        assert!(b["state"].is_string(), "bank without state: {b}");
        assert!(b["license"].is_string());
    }
    assert_eq!(r["presets"].as_array().unwrap().len(), 16);
    assert!(r["soundfont_available"].is_boolean());
    assert!(r["clap"].is_null(), "no plugin scan unless asked: {r}");
}

#[tokio::test]
async fn list_samples_searches_library_by_kind_and_name() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("bibliothek.sqlite3");
    {
        let lib = woa_library::Library::open(&db).unwrap();
        for (title, kind) in [
            ("Impact Heavy", woa_library::Kind::Sample),
            ("Impact Jingle", woa_library::Kind::Jingle),
            ("Morning Song", woa_library::Kind::Music),
            ("Riser Long", woa_library::Kind::Sample),
        ] {
            lib.add_track(&woa_library::NewTrack {
                path: format!("/fx/{title}.wav"),
                hash: title.into(),
                title: title.into(),
                duration: 1.5,
                kind,
            })
            .unwrap();
        }
    }
    let (client, _) = start(dir.path()).await;
    let lib = db.to_string_lossy().to_string();

    let r = ok(&call(
        &client,
        "list_samples",
        json!({"library": lib, "text": "impact"}),
    )
    .await);
    let titles: Vec<&str> = r["samples"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles.len(), 2, "{r}");
    assert!(!titles.contains(&"Morning Song"));

    let r = ok(&call(
        &client,
        "list_samples",
        json!({"library": lib, "kind": "sample"}),
    )
    .await);
    let s = r["samples"].as_array().unwrap();
    assert_eq!(s.len(), 2, "{r}");
    assert!(s.iter().all(|x| x["kind"] == "sample"));
    assert!(s[0]["path"].is_string() && s[0]["duration"].is_number());

    // Fehlende Bibliothek: leere Liste, keine neue Datei.
    let missing = dir.path().join("nichts").join("bibliothek.sqlite3");
    let r = ok(&call(
        &client,
        "list_samples",
        json!({"library": missing.to_string_lossy()}),
    )
    .await);
    assert_eq!(r["samples"].as_array().unwrap().len(), 0);
    assert!(!missing.exists());
}

#[tokio::test]
async fn list_samples_scans_a_folder_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let fx = dir.path().join("Jingle-FX");
    std::fs::create_dir_all(fx.join("impacts")).unwrap();
    std::fs::write(fx.join("impacts").join("impactPlate_heavy_000.wav"), b"x").unwrap();
    std::fs::write(fx.join("whoosh.ogg"), b"x").unwrap();
    std::fs::write(fx.join("readme.txt"), b"x").unwrap();
    let (client, _) = start(dir.path()).await;
    let folder = fx.to_string_lossy().to_string();
    let r = ok(&call(&client, "list_samples", json!({"folder": folder})).await);
    assert_eq!(r["samples"].as_array().unwrap().len(), 2, "{r}");
    let r = ok(&call(
        &client,
        "list_samples",
        json!({"folder": folder, "text": "IMPACT"}),
    )
    .await);
    let s = r["samples"].as_array().unwrap();
    assert_eq!(s.len(), 1, "{r}");
    assert!(s[0]["path"].as_str().unwrap().ends_with(".wav"));
    let r = call(
        &client,
        "list_samples",
        json!({"folder": folder, "library": "x"}),
    )
    .await;
    assert!(is_err(&r), "folder and library together");
}

#[tokio::test]
async fn insert_effect_takes_a_clap_effect_and_keeps_builtin_path() {
    let dir = tempfile::tempdir().unwrap();
    let (client, agent) = start(dir.path()).await;
    let audio = ok(&call(&client, "add_track", json!({"name": "Sprache"})).await)["track"]
        .as_u64()
        .unwrap();
    let bus = ok(&call(&client, "add_track", json!({"name": "Bus", "kind": "bus"})).await)["track"]
        .as_u64()
        .unwrap();

    // Eingebauter Weg unverändert.
    let eq = ok(&call(
        &client,
        "insert_effect",
        json!({"track": audio, "kind": "eq"}),
    )
    .await);
    assert!(eq["insert"].is_u64());

    let r = ok(&call(
        &client,
        "insert_effect",
        json!({"track": audio, "clap_id": "org.example.verb", "name": "Verb"}),
    )
    .await);
    let pid = r["insert"].as_u64().unwrap();
    {
        let s = agent.session();
        let s = s.lock().unwrap();
        let t = s.project().track(woa_model::TrackId(audio)).unwrap();
        assert_eq!(t.plugins.len(), 1);
        assert_eq!(t.plugins[0].clap_id, "org.example.verb");
        assert_eq!(t.plugins[0].name, "Verb");
        assert_eq!(t.plugins[0].role, woa_model::plugin::PluginRole::Effect);
        assert_eq!(t.plugins[0].id.0, pid);
    }

    // Bypass und Entfernen wirken auch auf den CLAP-Effekt.
    ok(&call(
        &client,
        "set_insert_bypass",
        json!({"insert": pid, "bypass": true}),
    )
    .await);
    {
        let s = agent.session();
        let s = s.lock().unwrap();
        assert!(
            s.project()
                .track(woa_model::TrackId(audio))
                .unwrap()
                .plugins[0]
                .bypass
        );
    }

    // Export rendert, auch wenn das Plugin hier nicht installiert ist.
    ok(&call(
        &client,
        "generate_drums",
        json!({"track": audio, "kick": "x...x...x...x..."}),
    )
    .await);
    ok(&call(&client, "export", json!({"path": "mix.wav"})).await);
    assert!(dir.path().join("p").join("mix.wav").is_file());

    ok(&call(&client, "remove_insert", json!({"insert": pid})).await);
    ok(&call(&client, "undo", json!({})).await);
    {
        let s = agent.session();
        let s = s.lock().unwrap();
        assert_eq!(
            s.project()
                .track(woa_model::TrackId(audio))
                .unwrap()
                .plugins
                .len(),
            1,
            "undo brings the plugin back"
        );
    }

    // Fehlerwege.
    for args in [
        json!({"track": bus, "clap_id": "org.example.verb"}),
        json!({"track": audio, "clap_id": "  "}),
        json!({"track": audio}),
        json!({"track": audio, "kind": "eq", "clap_id": "org.example.verb"}),
    ] {
        assert!(
            is_err(&call(&client, "insert_effect", args.clone()).await),
            "{args}"
        );
    }
}
