//! Probe mit einem echten Browser ohne echtes Gerät: Chromium läuft kopflos mit **falschem
//! Mikrofon** (`--use-fake-device-for-media-stream`) und stummgeschaltetem Ausgang
//! (`--mute-audio`) gegen den Gast-Server auf der LAN-Adresse dieses Rechners. Es wird nie ein
//! echtes Mikrofon oder ein Lautsprecher benutzt.
//!
//! Läuft nur, wenn `WOA_CHROMIUM` auf die Browser-Datei zeigt (sonst gilt der Test als
//! bestanden und tut nichts), zum Beispiel:
//! `WOA_CHROMIUM=/usr/bin/chromium-browser cargo test -p woa-guests --test browser_probe -- --nocapture`
//!
//! Gelesen wird der Seitentitel über den Debug-Port von Chromium; die Seite schreibt ihren Stand
//! hinein (`WOA-GAST bereit/wartet/verbunden rx=N/ende`).

use std::io::{Read, Write};
use std::net::{IpAddr, TcpStream, UdpSocket};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;
use woa_guests::source::SineGen;
use woa_guests::{WebConfig, channel};

const RATE: u32 = 48_000;
const BLOCK: usize = 480;

struct Browser(Child);

impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn lan_ip() -> Option<IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_unspecified() && !ip.is_loopback()).then_some(ip)
}

/// Titel der Seite, die auf die Gast-Adresse zeigt (über `/json` des Debug-Ports).
fn page_title(debug_port: u16, host: &str) -> Option<String> {
    let mut s = TcpStream::connect(("127.0.0.1", debug_port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    s.write_all(b"GET /json HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .ok()?;
    // Chromium hält die Verbindung offen: nach Content-Length aufhören, nicht auf das Ende warten.
    let mut raw: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 4096];
    let (head_len, body_len) = loop {
        let n = s.read(&mut tmp).ok().filter(|&n| n > 0)?;
        raw.extend_from_slice(&tmp[..n]);
        if let Some(p) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&raw[..p]).to_ascii_lowercase();
            let len = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .and_then(|v| v.trim().parse::<usize>().ok())?;
            break (p + 4, len);
        }
    };
    while raw.len() < head_len + body_len {
        let n = s.read(&mut tmp).ok().filter(|&n| n > 0)?;
        raw.extend_from_slice(&tmp[..n]);
    }
    let v: Value = serde_json::from_slice(&raw[head_len..head_len + body_len]).ok()?;
    if std::env::var_os("WOA_PROBE_DEBUG").is_some() {
        eprintln!(
            "targets: {}",
            v.to_string().chars().take(900).collect::<String>()
        );
    }
    v.as_array()?
        .iter()
        .filter(|t| t["type"] == "page" && t["url"].as_str().is_some_and(|u| u.contains(host)))
        .find_map(|t| t["title"].as_str().map(str::to_string))
}

fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("port");
    l.local_addr().expect("addr").port()
}

#[test]
fn headless_browser_with_fake_microphone_joins_and_hears_the_return() {
    let Ok(chromium) = std::env::var("WOA_CHROMIUM") else {
        println!("WOA_CHROMIUM nicht gesetzt: Browser-Probe übersprungen");
        return;
    };
    let ip = lan_ip().expect("kein LAN gefunden");
    let (mut hub, mut bank) = channel(RATE);
    hub.web_start(&WebConfig {
        ip,
        udp_port: 0,
        https_port: 0,
        engine_rate: RATE,
    })
    .expect("web_start");
    let link = hub.web_invite().expect("invite");
    let host = link
        .strip_prefix("https://")
        .and_then(|r| r.split('/').next())
        .expect("host")
        .to_string();

    let debug_port = free_port();
    let profile = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("woa-browser-probe");
    let _ = std::fs::remove_dir_all(&profile);
    let url = format!("{link}&auto=1");
    let child = Command::new(&chromium)
        .args([
            "--headless=new",
            "--no-sandbox",
            "--disable-gpu",
            "--no-first-run",
            "--use-fake-device-for-media-stream",
            "--use-fake-ui-for-media-stream",
            "--mute-audio",
            "--ignore-certificate-errors",
            "--autoplay-policy=no-user-gesture-required",
        ])
        .arg(format!("--remote-debugging-port={debug_port}"))
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg(&url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("chromium starten");
    let _browser = Browser(child);

    let mut music = SineGen::new(880.0, RATE, 0.3);
    let mut mb = vec![0.0f32; BLOCK * 2];
    let start = Instant::now();
    let mut done_frames = 0u64;
    let mut accepted = false;
    let mut last_title = String::new();
    let mut peak = 0.0f32;
    let mut rx = 0u32;
    let mut titles: Vec<String> = Vec::new();
    while start.elapsed() < Duration::from_secs(40) {
        // Audio-Takt der Bank in Echtzeit (der Gast-Ton wird gemessen).
        let due = (start.elapsed().as_secs_f64() * f64::from(RATE)) as u64;
        while done_frames + BLOCK as u64 <= due {
            music.fill(&mut mb);
            bank.begin_block();
            for i in 0..BLOCK {
                let (gl, _) = bank.frame();
                bank.send_returns((mb[i * 2], mb[i * 2 + 1]), |l, r| (l, r));
                peak = peak.max(gl.abs());
            }
            bank.end_block();
            done_frames += BLOCK as u64;
        }
        hub.poll();
        if !accepted && let Some(w) = hub.web_view().waiting.first().cloned() {
            println!("Warteraum: {} von {}", w.name, w.peer);
            hub.web_accept(&w.ticket).expect("accept");
            accepted = true;
        }
        if let Some(t) = page_title(debug_port, &host)
            && t != last_title
        {
            println!("[{:>5.1} s] Titel: {t}", start.elapsed().as_secs_f32());
            titles.push(t.clone());
            last_title = t;
        }
        if let Some(n) = last_title.split("rx=").nth(1).and_then(|n| n.parse().ok()) {
            rx = n;
        }
        if accepted && rx >= 50 && peak > 0.01 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        titles.iter().any(|t| t.contains("wartet")),
        "Seite kam nie in den Warteraum: {titles:?}"
    );
    assert!(accepted, "niemand im Warteraum");
    assert!(rx >= 50, "Rückweg kam nicht an (rx={rx}): {titles:?}");
    assert!(
        peak > 0.01,
        "vom falschen Mikrofon kam nichts an (Spitze {peak})"
    );
    let net = hub.view()[0].net.clone().expect("net view");
    println!(
        "Verbunden: RTT {} ms, Verlust {} Promille, Spitze am Pult {peak:.2}, Rückweg-Pakete im Browser {rx}",
        net.rtt_ms, net.loss_permille
    );
    assert!(net.connected);

    // Moderator legt auf: die Seite zeigt "ende", der Kanal ist frei.
    let slot = hub.view()[0].slot;
    hub.remove(slot);
    let until = Instant::now() + Duration::from_secs(10);
    let mut ended = false;
    while Instant::now() < until {
        hub.poll();
        if page_title(debug_port, &host).is_some_and(|t| t.contains("ende")) {
            ended = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(ended, "Seite zeigt nach dem Auflegen nicht 'ende'");
    assert_eq!(hub.count(), 0);
    let _ = std::fs::remove_dir_all(&profile);
}
