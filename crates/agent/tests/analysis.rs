//! Paket 19: `analyze` mit Spektrum, True Peak und Stereo-Bild, und `spectrogram_png`.
//! Gemessen wird gegen erzeugte Sinus-Signale, nichts davon wird abgespielt.

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

async fn start(dir: &std::path::Path) -> Client {
    let agent = WoaAgent::open_or_create(dir).unwrap();
    let (c_write, s_read) = tokio::io::duplex(1 << 16);
    let (s_write, c_read) = tokio::io::duplex(1 << 16);
    tokio::spawn(async move { woa_agent::serve(agent, (s_read, s_write)).await.unwrap() });
    TestClient.serve((c_read, c_write)).await.unwrap()
}

/// Spur mit einem Sinus (MIDI-Tonhöhe `pitch`), vier Beats lang, Lautstärke 100.
async fn tone_track(client: &Client, name: &str, pitch: u8, pan: f64) -> u64 {
    let t = ok(&call(client, "add_track", json!({ "name": name })).await)["track"]
        .as_u64()
        .unwrap();
    ok(&call(
        client,
        "generate_notes",
        json!({"track": t, "length_beats": 8.0, "waveform": "sine",
               "notes": [{"beat": 0.0, "pitch": pitch, "length_beats": 8.0}]}),
    )
    .await);
    ok(&call(client, "set_pan", json!({ "track": t, "pan": pan })).await);
    t
}

#[tokio::test(flavor = "current_thread")]
async fn analyze_reports_spectrum_stereo_and_true_peak() {
    let dir = tempfile::tempdir().unwrap();
    let client = start(dir.path()).await;
    // A4 = 440 Hz in der Mitte: Mono, Spektrum-Spitze bei 440 Hz.
    let a = tone_track(&client, "Ton", 69, 0.0).await;
    let r = ok(&call(&client, "analyze", json!({})).await);
    let hz = r["spectrum"]["peak_hz"].as_f64().unwrap();
    assert!((hz - 440.0).abs() < 12.0, "Spitze bei {hz} Hz");
    let band = r["spectrum"]["octave_bands"]
        .as_array()
        .unwrap()
        .iter()
        .max_by(|x, y| {
            x["db"]
                .as_f64()
                .unwrap()
                .total_cmp(&y["db"].as_f64().unwrap())
        })
        .unwrap();
    assert_eq!(band["hz"].as_f64(), Some(500.0), "{r}");
    assert!(
        (r["stereo"]["correlation"].as_f64().unwrap() - 1.0).abs() < 1e-3,
        "{r}"
    );
    assert!(
        r["true_peak_dbtp"].as_f64().unwrap() >= r["sample_peak_dbfs"].as_f64().unwrap() - 0.01
    );
    assert!(
        r["momentary_max_lufs"].as_f64().is_some() && r["short_term_max_lufs"].as_f64().is_some()
    );
    assert!(
        r["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n.as_str().unwrap().contains("mono")),
        "{r}"
    );

    // Zweiter Ton (A5) hart rechts, der erste hart links: breit, unkorreliert.
    ok(&call(&client, "set_pan", json!({ "track": a, "pan": -1.0 })).await);
    tone_track(&client, "Ton 2", 81, 1.0).await;
    let w = ok(&call(&client, "analyze", json!({})).await);
    assert!(
        w["stereo"]["correlation"].as_f64().unwrap().abs() < 0.1,
        "{w}"
    );
    assert!(
        w["stereo"]["side_to_mid_db"].as_f64().unwrap() > -3.0,
        "{w}"
    );

    // Eine Spur allein: nur ihr Ton.
    let s = ok(&call(&client, "analyze", json!({ "track": a })).await);
    assert!(
        (s["spectrum"]["peak_hz"].as_f64().unwrap() - 440.0).abs() < 12.0,
        "{s}"
    );
    assert_eq!(s["track"], json!(a));
    assert!(s.get("tracks").is_none());
    let bad = call(&client, "analyze", json!({ "track": 9999 })).await;
    assert_eq!(bad.is_error, Some(true));
}

#[tokio::test(flavor = "current_thread")]
async fn spectrogram_png_draws_the_tone_line() {
    let dir = tempfile::tempdir().unwrap();
    let client = start(dir.path()).await;
    tone_track(&client, "Ton", 69, 0.0).await;
    let r = ok(&call(
        &client,
        "spectrogram_png",
        json!({"path": "bilder/spektrogramm.png", "width": 200, "height": 128}),
    )
    .await);
    assert_eq!(
        (r["width"].as_u64(), r["height"].as_u64()),
        (Some(200), Some(128))
    );
    let file = std::fs::File::open(dir.path().join("bilder/spektrogramm.png")).unwrap();
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!((info.width, info.height), (200, 128));
    assert_eq!(info.color_type, png::ColorType::Rgb);
    let lum = |x: usize, y: usize| {
        let o = (y * 200 + x) * 3;
        u32::from(buf[o]) + u32::from(buf[o + 1]) + u32::from(buf[o + 2])
    };
    let row_sum = |y: usize| (0..200).map(|x| lum(x, y)).sum::<u32>();
    let brightest = (0..128).max_by_key(|&y| row_sum(y)).unwrap();
    // 440 Hz auf der Log-Achse 20 Hz .. 24 kHz.
    let t = (440.0f64 / 20.0).ln() / (24_000.0f64 / 20.0).ln();
    let expect = 127 - (t * 128.0) as usize;
    assert!(
        brightest.abs_diff(expect) <= 3,
        "{brightest} statt {expect}"
    );

    for bad in ["x.wav", "../x.png", "/tmp/x.png"] {
        let e = call(&client, "spectrogram_png", json!({ "path": bad })).await;
        assert_eq!(e.is_error, Some(true), "{bad}");
    }
    let e = call(
        &client,
        "spectrogram_png",
        json!({"path": "a.png", "width": 3}),
    )
    .await;
    assert_eq!(e.is_error, Some(true));
}
