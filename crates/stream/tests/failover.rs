//! woa-streamd-Kern: Oberfläche weg → nach ≤ 2,5 s Notfall-Playlist, Oberfläche zurück →
//! Sendemix, `Stop` beendet die Sendung.

mod fake_server;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use fake_server::{start, wait_for};
use woa_stream::link::{self, KIND_AUDIO, KIND_STOP, KIND_TITLE, audio_payload, write_frame};
use woa_stream::status::{self, StreamStatus};
use woa_stream::{StreamConfig, daemon};

const KW: &str = "test-kw-456";
const SR: u32 = 44_100;

fn write_wav(path: &Path, seconds: f32) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SR,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..(seconds * SR as f32) as usize {
        let v = (i as f32 * 220.0 * std::f32::consts::TAU / SR as f32).sin() * 0.3;
        w.write_sample(v).unwrap();
        w.write_sample(v).unwrap();
    }
    w.finalize().unwrap();
}

/// Oberfläche spielen: 20-ms-Blöcke Ton im Takt, bis `stop`.
fn ui(link: std::path::PathBuf, stop: Arc<AtomicBool>, title: &'static str) {
    let mut s: link::LocalStream = link::connect(&link, SR).unwrap();
    write_frame(&mut s, KIND_TITLE, title.as_bytes()).unwrap();
    let n = SR as usize / 50;
    let mut k = 0usize;
    let mut bytes = Vec::new();
    let t0 = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        let block: Vec<f32> = (0..n)
            .flat_map(|i| {
                let v = ((k + i) as f32 * 440.0 * std::f32::consts::TAU / SR as f32).sin() * 0.5;
                [v, v]
            })
            .collect();
        audio_payload(&block, &mut bytes);
        if write_frame(&mut s, KIND_AUDIO, &bytes).is_err() {
            return;
        }
        k += n;
        let due = t0 + Duration::from_secs_f64(k as f64 / f64::from(SR));
        if let Some(d) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(d);
        }
    }
}

fn status_of(dir: &Path) -> StreamStatus {
    status::read(&status::status_path(dir)).unwrap_or_default()
}

#[test]
fn emergency_takes_over_and_hands_back() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let notfall = tmp.path().join("notfall");
    std::fs::create_dir_all(&notfall).unwrap();
    write_wav(&notfall.join("Notfall-Titel.wav"), 2.0);
    let kw_file = tmp.path().join("source.pass");
    std::fs::write(&kw_file, format!("{KW}\n")).unwrap();

    let fake = start(KW);
    let cfg = StreamConfig {
        host: "127.0.0.1".into(),
        port: fake.port - 1,
        admin_port: Some(fake.admin),
        stream_id: 1,
        sample_rate: SR,
        password_file: kw_file,
        emergency: vec![notfall.clone()],
        ..StreamConfig::default()
    };
    let stop_daemon = Arc::new(AtomicBool::new(false));
    let daemon = {
        let (cfg, data, stop) = (cfg.clone(), data.clone(), Arc::clone(&stop_daemon));
        std::thread::spawn(move || daemon::run(cfg, &data, stop))
    };
    let link = link::link_path(&data);
    assert!(wait_for(Duration::from_secs(3), || link::is_alive(&link)));

    // Oberfläche sendet.
    let ui_stop = Arc::new(AtomicBool::new(false));
    let ui1 = {
        let (l, s) = (link.clone(), Arc::clone(&ui_stop));
        std::thread::spawn(move || ui(l, s, "Live-Titel"))
    };
    assert!(wait_for(Duration::from_secs(3), || {
        let s = status_of(&data);
        s.connected && s.ui_connected && s.source == "Sendemix" && s.title == "Live-Titel"
    }));
    std::thread::sleep(Duration::from_millis(1000));
    assert_eq!(status_of(&data).source, "Sendemix");

    // Oberfläche „stürzt ab“.
    ui_stop.store(true, Ordering::Relaxed);
    ui1.join().unwrap();
    let gone = Instant::now();
    assert!(wait_for(Duration::from_secs(4), || status_of(&data).emergency));
    let took = gone.elapsed();
    assert!(took <= Duration::from_millis(2500), "Notfall nach {took:?}");
    let s = status_of(&data);
    assert_eq!(s.source, "Notfall-Playlist");
    assert!(s.connected, "server connection stays up");
    assert!(wait_for(Duration::from_secs(3), || {
        fake.seen
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.contains("song=Notfall-Titel"))
    }));

    // Oberfläche zurück.
    ui_stop.store(false, Ordering::Relaxed);
    let ui2 = {
        let (l, s) = (link.clone(), Arc::clone(&ui_stop));
        std::thread::spawn(move || ui(l, s, "Wieder da"))
    };
    assert!(wait_for(Duration::from_secs(2), || {
        let s = status_of(&data);
        !s.emergency && s.title == "Wieder da"
    }));
    ui_stop.store(true, Ordering::Relaxed);
    ui2.join().unwrap();

    // Daten kamen die ganze Zeit an, im Takt (≈ 16 kB/s bei 128 kbit/s).
    let bytes = fake.seen.bytes.lock().unwrap().len();
    assert!(bytes > 40_000, "{bytes}");
    assert_eq!(fake.seen.logins.load(Ordering::Relaxed), 1);

    // Stop von der Oberfläche beendet woa-streamd.
    let mut s = link::connect(&link, SR).unwrap();
    write_frame(&mut s, KIND_STOP, &[]).unwrap();
    let r = daemon.join().unwrap();
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(status_of(&data).connection, "beendet");
    assert!(!link.exists());
}

/// Wartet, bis der Daemon die Verbindung schließt; liefert die Zeit bis dahin.
fn closed_within(s: &mut link::LocalStream, limit: Duration) -> Option<Duration> {
    use std::io::Read;
    let t = Instant::now();
    s.set_read_timeout(Some(limit)).unwrap();
    let mut b = [0u8; 16];
    match s.read(&mut b) {
        Ok(0) => Some(t.elapsed()),
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => Some(t.elapsed()),
        _ => None,
    }
}

#[test]
fn local_link_rejects_strangers() {
    use std::io::Write;
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let kw_file = tmp.path().join("source.pass");
    std::fs::write(&kw_file, format!("{KW}\n")).unwrap();
    let fake = start(KW);
    let cfg = StreamConfig {
        host: "127.0.0.1".into(),
        port: fake.port - 1,
        password_file: kw_file,
        ..StreamConfig::default()
    };
    let stop = Arc::new(AtomicBool::new(false));
    let daemon = {
        let (cfg, data, stop) = (cfg, data.clone(), Arc::clone(&stop));
        std::thread::spawn(move || daemon::run(cfg, &data, stop))
    };
    let link = link::link_path(&data);
    assert!(wait_for(Duration::from_secs(3), || link::is_alive(&link)));
    let open = || link::open_raw(&link).unwrap().0;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let info = link::LinkInfo::read(&link).unwrap();
        let dir = info.socket.parent().unwrap();
        let mode = std::fs::metadata(dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "socket dir must be owner-only");
    }
    std::thread::sleep(Duration::from_millis(300));

    // Wrong token.
    let mut s = open();
    let bad = "0".repeat(link::TOKEN_LEN);
    write_frame(&mut s, link::KIND_HELLO, &link::hello_payload(SR, &bad)).unwrap();
    assert!(closed_within(&mut s, Duration::from_secs(1)).is_some());

    // Oversized hello header: closed at once, nothing allocated.
    let mut s = open();
    s.write_all(&[link::KIND_HELLO, 0xFF, 0xFF, 0xFF, 0x7F])
        .unwrap();
    assert!(closed_within(&mut s, Duration::from_secs(1)).is_some());

    // Silent peer: closed after the handshake deadline (200 ms), not later.
    let mut s = open();
    let t = closed_within(&mut s, Duration::from_secs(2)).expect("not closed");
    assert!(t <= Duration::from_millis(700), "{t:?}");

    // Several silent connections stay open; the real UI still gets in at once,
    // because the oldest waiting one is evicted, not the newcomer.
    let idle: Vec<link::LocalStream> = (0..12).map(|_| open()).collect();
    // The real UI still gets in.
    let mut s = link::connect(&link, SR).unwrap();
    assert!(wait_for(Duration::from_secs(2), || status_of(&data).ui_connected));
    drop(idle);
    write_frame(&mut s, KIND_STOP, &[]).unwrap();
    assert!(daemon.join().unwrap().is_ok());
}

#[test]
fn missing_password_file_is_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = StreamConfig {
        host: "127.0.0.1".into(),
        password_file: tmp.path().join("fehlt.pass"),
        ..StreamConfig::default()
    };
    let r = daemon::run(cfg, tmp.path(), Arc::new(AtomicBool::new(false)));
    let e = r.unwrap_err();
    assert!(e.contains("fehlt.pass"), "{e}");
    let s = status_of(tmp.path());
    assert_eq!(s.connection, "beendet");
    assert!(s.last_error.unwrap().contains("Kennwortdatei"));
}
