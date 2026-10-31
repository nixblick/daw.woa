//! Zwei Teilnehmer in einem Prozess über Loopback, ohne Gerät und ohne Ton: ein `str0m`-Client
//! (der "Browser") schickt einen Opus-kodierten Sinus von 440 Hz; der Netz-Thread nimmt ihn an,
//! die Bank mischt ihn, der Rückweg (Musik 880 Hz, ohne den Gast) geht per Opus zurück.
//! Pegel und Frequenz werden an beiden Enden gemessen (Goertzel).
//!
//! Der erste Test spricht den Netz-Thread direkt an, der zweite geht den ganzen Weg: Einladung,
//! HTTPS-Anmeldung, Warteraum, Annehmen, Sprechen, Auflegen.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use opus::{Application, Bitrate, Channels, Decoder, Encoder};
use serde_json::{Value, json};
use str0m::change::{SdpAnswer, SdpPendingOffer};
use str0m::media::{Direction, Frequency, MediaKind, MediaTime, Mid};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, Input, Output, Rtc};
use woa_guests::net::{GuestNet, NetConfig, NetEvent};
use woa_guests::source::SineGen;
use woa_guests::{GuestBank, GuestHub, WebConfig, channel};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const RATE: u32 = 48_000;
const BLOCK: usize = 480;

fn tone(x: &[f32], freq: f32) -> f32 {
    let w = std::f64::consts::TAU * f64::from(freq) / f64::from(RATE);
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, &v) in x.iter().enumerate() {
        re += f64::from(v) * (w * i as f64).cos();
        im += f64::from(v) * (w * i as f64).sin();
    }
    (2.0 * (re * re + im * im).sqrt() / x.len() as f64) as f32
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-12).log10()
}

/// Der "Browser": ein `Rtc` mit eigenem Socket.
struct Client {
    rtc: Rtc,
    sock: UdpSocket,
    mid: Mid,
    connected: bool,
    dec: Decoder,
    heard: Vec<f32>,
}

impl Client {
    /// Baut den Browser und sein Angebot.
    fn new() -> (Self, String, SdpPendingOffer) {
        let sock = UdpSocket::bind("127.0.0.1:0").expect("sock");
        sock.set_nonblocking(true).expect("nonblocking");
        str0m::crypto::from_feature_flags().install_process_default();
        let mut rtc = Rtc::builder()
            .clear_codecs()
            .enable_opus(true, false)
            .build(Instant::now());
        rtc.add_local_candidate(
            Candidate::host(sock.local_addr().expect("addr"), "udp").expect("cand"),
        );
        let mut api = rtc.sdp_api();
        let mid = api.add_media(MediaKind::Audio, Direction::SendRecv, None, None, None);
        let (offer, pending) = api.apply().expect("offer");
        let c = Self {
            rtc,
            sock,
            mid,
            connected: false,
            dec: Decoder::new(RATE, Channels::Stereo).expect("dec"),
            heard: Vec::new(),
        };
        (c, offer.to_sdp_string(), pending)
    }

    fn accept(&mut self, pending: SdpPendingOffer, answer: &str) {
        self.rtc
            .sdp_api()
            .accept_answer(pending, SdpAnswer::from_sdp_string(answer).expect("answer"))
            .expect("accept answer");
    }

    fn drive(&mut self) {
        loop {
            match self.rtc.poll_output().expect("poll_output") {
                Output::Transmit(t) => {
                    let _ = self.sock.send_to(&t.contents, t.destination);
                }
                Output::Timeout(_) => return,
                Output::Event(Event::Connected) => self.connected = true,
                Output::Event(Event::MediaData(d)) => {
                    let mut pcm = vec![0.0f32; 960 * 2];
                    let n = self
                        .dec
                        .decode_float(&d.data, &mut pcm, false)
                        .expect("decode");
                    for f in 0..n {
                        self.heard.push(pcm[f * 2]);
                    }
                }
                Output::Event(_) => {}
            }
        }
    }

    fn receive(&mut self) {
        let local = self.sock.local_addr().expect("addr");
        let mut buf = vec![0u8; 2000];
        while let Ok((n, source)) = self.sock.recv_from(&mut buf) {
            let input = Input::Receive(
                Instant::now(),
                Receive {
                    proto: Protocol::Udp,
                    source,
                    destination: local,
                    contents: buf[..n].try_into().expect("datagram"),
                },
            );
            if self.rtc.accepts(&input) {
                self.rtc.handle_input(input).expect("input");
                self.drive();
            }
        }
        self.rtc
            .handle_input(Input::Timeout(Instant::now()))
            .expect("timeout");
        self.drive();
    }
}

/// Spielt `secs` Sekunden Echtzeit: der Browser spricht 440 Hz, die Bank rechnet Blöcke mit
/// Musik 880 Hz und schreibt die Rückwege. Gibt zurück, was am Pult vom Gast ankam.
fn session(
    client: &mut Client,
    bank: &mut GuestBank,
    mut hub: Option<&mut GuestHub>,
    secs: u64,
) -> Vec<f32> {
    let mut enc = Encoder::new(RATE, Channels::Stereo, Application::Voip).expect("enc");
    enc.set_bitrate(Bitrate::Bits(48_000)).expect("bitrate");
    let mut mic = SineGen::new(440.0, RATE, 0.3);
    let mut music = SineGen::new(880.0, RATE, 0.3);
    let mut pcm = vec![0.0f32; 960 * 2];
    let mut packet = vec![0u8; 1500];
    let mut rtp = 0u64;
    let start = Instant::now();
    let mut next_audio = start;
    let mut bank_frames = 0u64;
    let mut at_bank: Vec<f32> = Vec::new();
    let mut mb = vec![0.0f32; BLOCK * 2];
    while start.elapsed() < Duration::from_secs(secs) {
        client.receive();
        let now = Instant::now();
        // Mikrofon des Browsers: alle 20 ms ein Opus-Paket, erst wenn verbunden.
        while client.connected && next_audio <= now {
            mic.fill(&mut pcm);
            let n = enc.encode_float(&pcm, &mut packet).expect("encode");
            let pt = client
                .rtc
                .writer(client.mid)
                .and_then(|w| w.payload_params().next().map(|p| p.pt()))
                .expect("pt");
            if let Some(w) = client.rtc.writer(client.mid) {
                w.write(
                    pt,
                    now,
                    MediaTime::new(rtp, Frequency::FORTY_EIGHT_KHZ),
                    packet[..n].to_vec(),
                )
                .expect("write");
            }
            client.drive();
            rtp += 960;
            next_audio += Duration::from_millis(20);
        }
        if !client.connected {
            next_audio = now;
        }
        // "Audio-Thread": so viele Blöcke, wie die Echtzeit hergibt.
        let due = (start.elapsed().as_secs_f64() * f64::from(RATE)) as u64;
        while bank_frames + BLOCK as u64 <= due {
            music.fill(&mut mb);
            bank.begin_block();
            for i in 0..BLOCK {
                let base = (mb[i * 2], mb[i * 2 + 1]);
                let (gl, _) = assert_no_alloc(|| bank.frame());
                assert_no_alloc(|| bank.send_returns(base, |l, r| (l, r)));
                at_bank.push(gl);
            }
            bank.end_block();
            bank_frames += BLOCK as u64;
        }
        if let Some(h) = hub.as_deref_mut() {
            h.poll();
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    at_bank
}

/// Prüft die Messwerte beider Enden.
fn check_audio(at_bank: &[f32], client: &Client) {
    let n = at_bank.len();
    let g = db(tone(&at_bank[n - 48_000..], 440.0));
    println!("Pult: 440 Hz {g:.1} dB");
    assert!(g > -14.0 && g < -8.0, "Gastton am Pult: {g} dB");
    let h = &client.heard;
    assert!(h.len() > 96_000, "zu wenig Rückweg: {}", h.len());
    let tail = &h[h.len() - 48_000..];
    let music_db = db(tone(tail, 880.0));
    let own_db = db(tone(tail, 440.0));
    println!("Rückweg: 880 Hz {music_db:.1} dB, eigener Ton 440 Hz {own_db:.1} dB");
    assert!(
        music_db > -14.0 && music_db < -8.0,
        "Musik im Rückweg: {music_db} dB"
    );
    assert!(own_db < -60.0, "eigener Ton im Rückweg: {own_db} dB");
}

#[test]
fn guest_tone_arrives_and_return_has_music_but_not_the_guest() {
    let (mut hub, mut bank) = channel(RATE);
    let guest = hub.add_network("Gast 1", "Anna").expect("slot");
    assert_eq!(hub.view()[0].name, "Anna");
    assert!(!hub.view()[0].on_air, "neuer Gast wartet");
    hub.set_on_air(guest.slot, true);
    let net = GuestNet::start(NetConfig {
        engine_rate: RATE,
        bind: "127.0.0.1:0".parse().expect("addr"),
    })
    .expect("net");

    let (mut client, offer, pending) = Client::new();
    let (answer, shared) = net
        .add_peer(1, &offer, guest)
        .map_err(|e| e.0)
        .expect("add_peer");
    client.accept(pending, &answer);
    let at_bank = session(&mut client, &mut bank, None, 5);

    assert!(client.connected, "Verbindung kam nicht zustande");
    assert!(shared.connected.load(Ordering::Relaxed));
    let pin = shared.packets_in.load(Ordering::Relaxed);
    let pout = shared.packets_out.load(Ordering::Relaxed);
    println!(
        "Pakete: Gast->WOA {pin}, WOA->Gast {pout}, RTT {} ms, Verlust {} Promille",
        shared.rtt_ms.load(Ordering::Relaxed),
        shared.loss_permille.load(Ordering::Relaxed)
    );
    assert!(pin > 100, "zu wenige Pakete vom Gast: {pin}");
    assert!(pout > 100, "zu wenige Pakete zum Gast: {pout}");
    check_audio(&at_bank, &client);

    // Aufhängen: der Gast kommt mit Kanal und Rückweg zurück.
    net.remove_peer(1);
    let mut got = None;
    for _ in 0..100 {
        for e in net.poll() {
            if let NetEvent::Gone { guest, .. } = e {
                got = Some(guest);
            }
        }
        if got.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let g = got.expect("Gone-Ereignis");
    hub.remove(g.slot);
    hub.return_network(*g);
    assert_eq!(hub.count(), 0);
    // Kanal und Rückweg sind zurück: ein neuer Gast bekommt sie wieder.
    assert!(hub.add_network("Gast 2", "Bert").is_ok());
}

// ---- der ganze Weg über HTTPS -------------------------------------------------------------

/// Nimmt jedes Zertifikat an: das Gast-Zertifikat ist selbst ausgestellt.
#[derive(Debug)]
struct AcceptAny(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for AcceptAny {
    fn verify_server_cert(
        &self,
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &[rustls::pki_types::CertificateDer<'_>],
        _: &rustls::pki_types::ServerName<'_>,
        _: &[u8],
        _: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        m: &[u8],
        c: &rustls::pki_types::CertificateDer<'_>,
        d: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(m, c, d, &self.0.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &rustls::pki_types::CertificateDer<'_>,
        d: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(m, c, d, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// Eine HTTPS-Anfrage wie vom Handy; gibt Status und JSON zurück.
fn https(addr: SocketAddr, method: &str, path: &str, body: &str) -> (u16, Value) {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let cfg = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .expect("versions")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAny(provider)))
        .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from("localhost").expect("name");
    let conn = rustls::ClientConnection::new(Arc::new(cfg), name).expect("conn");
    let tcp = TcpStream::connect(addr).expect("connect");
    tcp.set_read_timeout(Some(Duration::from_secs(10)))
        .expect("timeout");
    let mut tls = rustls::StreamOwned::new(conn, tcp);
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    tls.write_all(req.as_bytes()).expect("write");
    let mut raw = Vec::new();
    // Die Gegenseite schließt ohne close_notify nicht immer sauber; was da ist, reicht.
    let _ = tls.read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw).into_owned();
    let status: u16 = text
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .expect("status");
    let json = text
        .split_once("\r\n\r\n")
        .and_then(|(_, b)| serde_json::from_str(b).ok())
        .unwrap_or(Value::Null);
    (status, json)
}

fn web_cfg() -> WebConfig {
    WebConfig {
        ip: "127.0.0.1".parse().expect("ip"),
        udp_port: 0,
        https_port: 0,
        engine_rate: RATE,
    }
}

fn link_parts(link: &str) -> (SocketAddr, String) {
    let rest = link.strip_prefix("https://").expect("https");
    let (addr, tail) = rest.split_once("/gast?i=").expect("path");
    (addr.parse().expect("addr"), tail.to_string())
}

#[test]
fn invite_waiting_room_accept_talk_hangup() {
    let (mut hub, mut bank) = channel(RATE);
    assert!(hub.web_invite().is_none(), "ohne Zugang keine Einladung");
    hub.web_start(&web_cfg()).expect("web_start");
    assert!(hub.web_running());
    let link = hub.web_invite().expect("invite");
    let (addr, token) = link_parts(&link);

    // Die Seite kommt über HTTPS, die Einladung gilt.
    let (st, v) = https(addr, "GET", &format!("/gast/check?i={token}"), "");
    assert_eq!((st, v["ok"].clone()), (200, json!(true)));
    let (st, _) = https(addr, "GET", "/gast/check?i=00", "");
    assert_eq!(st, 200);

    // Der Gast tritt bei und steht im Warteraum.
    let (mut client, offer, pending) = Client::new();
    let body = json!({ "invite": token, "name": "Anna", "sdp": offer }).to_string();
    let (st, v) = https(addr, "POST", "/gast/join", &body);
    assert_eq!(st, 200, "join: {v}");
    let ticket = v["ticket"].as_str().expect("ticket").to_string();
    let (_, v) = https(addr, "POST", "/gast/join", &body);
    assert!(v["error"].is_string(), "Einladung nur einmal: {v}");
    let w = hub.web_view();
    assert_eq!(w.waiting.len(), 1);
    assert_eq!(w.waiting[0].name, "Anna");
    assert!(w.invites.is_empty(), "Einladung ist verbraucht");
    assert_eq!(hub.count(), 0, "noch kein Kanal vor dem Annehmen");
    let (_, v) = https(addr, "GET", &format!("/gast/state?t={ticket}"), "");
    assert_eq!(v["state"], "waiting");

    // Moderator nimmt an.
    hub.web_accept(&ticket).expect("accept");
    assert_eq!(hub.count(), 1);
    let (_, v) = https(addr, "GET", &format!("/gast/state?t={ticket}"), "");
    assert_eq!(v["state"], "accepted");
    assert_eq!(v["air"], "wait", "angenommen heißt erst Warteraum");
    client.accept(pending, v["sdp"].as_str().expect("sdp"));
    // Der Moderator holt ihn auf Sendung.
    assert!(!hub.view()[0].on_air);
    hub.set_on_air(hub.view()[0].slot, true);

    // Sprechen: 5 Sekunden, Messung an beiden Enden.
    let at_bank = session(&mut client, &mut bank, Some(&mut hub), 5);
    assert!(client.connected);
    check_audio(&at_bank, &client);
    let view = hub.view();
    assert_eq!(view[0].name, "Anna");
    let net = view[0].net.clone().expect("net view");
    assert!(net.connected);
    println!(
        "Anzeige: RTT {} ms, Verlust {} Promille",
        net.rtt_ms, net.loss_permille
    );
    let (_, v) = https(addr, "GET", &format!("/gast/state?t={ticket}"), "");
    assert_eq!(v["air"], "air", "die Seite sieht: auf Sendung");

    // Der Gast schaltet sich selbst stumm; der Moderator sieht es.
    let (st, _) = https(
        addr,
        "POST",
        "/gast/mute",
        &json!({ "t": ticket, "on": true }).to_string(),
    );
    assert_eq!(st, 200);
    hub.poll();
    assert!(hub.view()[0].self_muted);

    // Der Gast legt auf: Kanal frei, Ticket zu Ende.
    let (st, _) = https(
        addr,
        "POST",
        "/gast/leave",
        &json!({ "t": ticket }).to_string(),
    );
    assert_eq!(st, 200);
    for _ in 0..100 {
        hub.poll();
        if hub.count() == 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(hub.count(), 0, "Kanal nach dem Auflegen frei");
    let (_, v) = https(addr, "GET", &format!("/gast/state?t={ticket}"), "");
    assert_eq!(v["state"], "ended");

    // Der Kanal ist danach wieder benutzbar (Feed und Rückweg sind zurück).
    let g = hub.add_network("Browser", "Bert").expect("zweiter Gast");
    let slot = g.slot;
    hub.remove(slot);
    hub.return_network(g);

    // Ablehnen: der Gast erfährt es, es entsteht kein Kanal.
    let link = hub.web_invite().expect("invite");
    let (_, token) = link_parts(&link);
    let body = json!({ "invite": token, "name": "Carla", "sdp": "v=0\r\n" }).to_string();
    let (_, v) = https(addr, "POST", "/gast/join", &body);
    let t2 = v["ticket"].as_str().expect("ticket").to_string();
    hub.web_reject(&t2);
    let (_, v) = https(addr, "GET", &format!("/gast/state?t={t2}"), "");
    assert_eq!(v["state"], "rejected");
    assert_eq!(hub.count(), 0);

    // Kaputtes Angebot: Annehmen scheitert sauber, der Kanal bleibt frei.
    let link = hub.web_invite().expect("invite");
    let (_, token) = link_parts(&link);
    let body = json!({ "invite": token, "name": "Dirk", "sdp": "v=0\r\nmüll" }).to_string();
    let (_, v) = https(addr, "POST", "/gast/join", &body);
    let t3 = v["ticket"].as_str().expect("ticket").to_string();
    assert!(hub.web_accept(&t3).is_err());
    assert_eq!(hub.count(), 0);
    let (_, v) = https(addr, "GET", &format!("/gast/state?t={t3}"), "");
    assert_eq!(v["state"], "rejected");
    let g = hub
        .add_network("Browser", "Erik")
        .expect("Kanal wieder frei");
    let slot = g.slot;
    hub.remove(slot);
    hub.return_network(g);

    // Ausschalten: der Port ist zu.
    hub.web_stop();
    assert!(!hub.web_running());
    assert!(TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_err());
}
