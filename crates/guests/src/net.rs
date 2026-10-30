//! Der Netz-Thread der Browser-Gäste: ein UDP-Socket, ein Thread, alle Gäste.
//!
//! Aufbau nach dem Muster der Sans-IO-Schleife in `str0m` (`examples/chat.rs`, MIT/Apache-2.0):
//! Pakete vom Socket an die passende `Rtc`, `poll_output` bis zum Timeout, Zeit weiterdrehen.
//! Eigener Code: Opus-Brücke, Rückweg, Statistik und Fehlerbehandlung ohne `expect`.
//!
//! - **Rein:** Opus-Pakete vom Browser werden dekodiert, auf die Rate der Engine gebracht und in
//!   den [`crate::GuestFeed`] des Kanals geschrieben (Jitterpuffer der Bank). Fehlt ein Paket,
//!   ersetzt Opus es (Packet Loss Concealment).
//! - **Raus:** Der Rückweg (Mix-Minus) kommt als PCM über einen `rtrb`-Ring von der Bank, wird in
//!   20-ms-Blöcke Opus kodiert und an den Browser geschickt.
//! - **Logs:** RTT und Verlust jeder Verbindung alle 10 Sekunden (bei schlechten Werten sofort).
//!
//! Läuft nie im Audio-Thread. Ohne TURN-Relay funktionieren nur direkte Verbindungen (gleiches
//! WLAN); das ist gewollt, ein Relay wäre ein bezahlter Dienst.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender, TryRecvError, channel, sync_channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use str0m::change::SdpOffer;
use str0m::format::Codec;
use str0m::media::{Frequency, MediaKind, MediaTime, Mid, Pt};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc};

use crate::NetGuest;
use crate::codec::{FRAME_48K, ReturnEncoder, VoiceDecoder};

/// Kennung eines Gastes im Netz-Thread.
pub type PeerId = u64;

/// Längste Wartezeit der Schleife (Rückweg-Takt).
const TICK: Duration = Duration::from_millis(5);
/// Rückweg-Vorrat, ab dem alter Ton verworfen wird (Sekunden). Sonst hörte der Gast sich spät.
const RETURN_MAX_SECONDS: f32 = 0.150;
/// Ab so viel Verlust (Anteil) oder RTT (ms) wird sofort gewarnt.
const WARN_LOSS: f32 = 0.05;
const WARN_RTT_MS: u32 = 300;
/// Abstand der regelmäßigen Statistikzeilen.
const LOG_EVERY: Duration = Duration::from_secs(10);

/// Einstellungen des Netz-Threads.
#[derive(Debug, Clone)]
pub struct NetConfig {
    /// Rate der Engine (Hz).
    pub engine_rate: u32,
    /// Lokale Adresse des UDP-Sockets (die LAN-Adresse; Port 0 wählt das System).
    pub bind: SocketAddr,
}

/// Messwerte einer Verbindung, lock-frei für die Anzeige.
#[derive(Debug, Default)]
pub struct PeerShared {
    /// Verbindung steht (ICE und DTLS fertig).
    pub connected: AtomicBool,
    /// Letzte Round-Trip-Zeit in Millisekunden, 0: unbekannt.
    pub rtt_ms: AtomicU32,
    /// Letzter Verlust des Gast-Mikros in Promille.
    pub loss_permille: AtomicU32,
    /// Empfangene Opus-Pakete.
    pub packets_in: AtomicU64,
    /// Gesendete Opus-Pakete des Rückwegs.
    pub packets_out: AtomicU64,
    /// Durch Opus ersetzte (verlorene) Pakete.
    pub concealed: AtomicU64,
}

/// Was der Netz-Thread meldet.
#[derive(Debug)]
pub enum NetEvent {
    /// Die Verbindung steht; der Gast ist zu hören.
    Connected { id: PeerId },
    /// Die Verbindung ist zu Ende. Der Gast (Kanal, Feed, Rückweg) kommt zurück.
    Gone {
        id: PeerId,
        reason: String,
        guest: Box<NetGuest>,
    },
}

type AddReply = Result<Arc<PeerShared>, (String, Box<NetGuest>)>;

enum NetCmd {
    Add {
        id: PeerId,
        offer: String,
        guest: Box<NetGuest>,
        reply: SyncSender<(Option<String>, AddReply)>,
    },
    Remove {
        id: PeerId,
    },
}

/// Fehler beim Annehmen: Text und, wenn noch vorhanden, der Gast (Kanal) zum Freigeben.
pub type AddError = (String, Option<NetGuest>);

/// Steuerseite des Netz-Threads.
pub struct GuestNet {
    cmds: Sender<NetCmd>,
    events: Receiver<NetEvent>,
    local: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl GuestNet {
    /// Öffnet den UDP-Port und startet den Thread.
    pub fn start(cfg: NetConfig) -> Result<Self, String> {
        let socket = UdpSocket::bind(cfg.bind).map_err(|e| format!("UDP-Port: {e}"))?;
        let local = socket.local_addr().map_err(|e| e.to_string())?;
        // Die Krypto-Bibliothek einmal je Prozess festlegen; ein zweiter Aufruf ist harmlos.
        str0m::crypto::from_feature_flags().install_process_default();
        let (ctx, crx) = channel();
        let (etx, erx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("woa-guest-net".into())
            .spawn(move || run(socket, local, cfg.engine_rate, crx, etx, flag))
            .map_err(|e| e.to_string())?;
        tracing::info!(%local, "guest net: started");
        Ok(Self {
            cmds: ctx,
            events: erx,
            local,
            stop,
            thread: Some(thread),
        })
    }

    /// Adresse des UDP-Sockets (mit dem echten Port).
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    /// Nimmt das SDP-Angebot eines Browsers an und gibt die Antwort zurück. Bei einem Fehler
    /// kommt der Gast (Kanal) zurück, damit er frei werden kann.
    pub fn add_peer(
        &self,
        id: PeerId,
        offer: &str,
        guest: NetGuest,
    ) -> Result<(String, Arc<PeerShared>), AddError> {
        let (tx, rx) = sync_channel(1);
        let cmd = NetCmd::Add {
            id,
            offer: offer.to_string(),
            guest: Box::new(guest),
            reply: tx,
        };
        if let Err(e) = self.cmds.send(cmd) {
            let guest = match e.0 {
                NetCmd::Add { guest, .. } => Some(*guest),
                NetCmd::Remove { .. } => None,
            };
            return Err(("Netz-Thread beendet".into(), guest));
        }
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok((Some(answer), Ok(shared))) => Ok((answer, shared)),
            Ok((_, Ok(_))) => Err(("Keine Antwort erzeugt".into(), None)),
            Ok((_, Err((e, g)))) => Err((e, Some(*g))),
            Err(_) => Err(("Netz-Thread antwortet nicht".into(), None)),
        }
    }

    /// Baut die Verbindung ab; der Gast kommt als [`NetEvent::Gone`] zurück.
    pub fn remove_peer(&self, id: PeerId) {
        let _ = self.cmds.send(NetCmd::Remove { id });
    }

    /// Holt gemeldete Ereignisse ab (ohne zu warten).
    pub fn poll(&self) -> Vec<NetEvent> {
        self.events.try_iter().collect()
    }

    /// Beendet den Thread; der Port ist danach zu. Noch laufende Gäste kommen als `Gone`.
    pub fn stop(&mut self) -> Vec<NetEvent> {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        self.poll()
    }
}

impl Drop for GuestNet {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Peer {
    id: PeerId,
    rtc: Rtc,
    mid: Option<Mid>,
    pt: Option<Pt>,
    guest: Option<Box<NetGuest>>,
    dec: VoiceDecoder,
    enc: ReturnEncoder,
    decoded: Vec<f32>,
    ret_buf: Vec<f32>,
    /// RTP-Zeit des nächsten Rückweg-Pakets (48-kHz-Einheiten).
    rtp_time: u64,
    connected: bool,
    shared: Arc<PeerShared>,
    last_log: Instant,
    warned_at: Option<Instant>,
    reason: Option<String>,
}

impl Peer {
    fn accepts(&self, input: &Input) -> bool {
        self.rtc.accepts(input)
    }

    fn handle_input(&mut self, input: Input) {
        if !self.rtc.is_alive() {
            return;
        }
        if let Err(e) = self.rtc.handle_input(input) {
            self.fail(format!("Eingabe: {e}"));
        }
    }

    fn fail(&mut self, reason: String) {
        if self.reason.is_none() {
            self.reason = Some(reason);
        }
        self.rtc.disconnect();
    }

    /// `poll_output` bis zum Timeout; sendet Pakete, wertet Ereignisse aus.
    fn drive(&mut self, socket: &UdpSocket, events: &Sender<NetEvent>) -> Instant {
        loop {
            if !self.rtc.is_alive() {
                return Instant::now();
            }
            match self.rtc.poll_output() {
                Ok(Output::Transmit(t)) => {
                    if let Err(e) = socket.send_to(&t.contents, t.destination)
                        && !matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::ConnectionReset)
                    {
                        tracing::debug!(error = %e, "guest net: send failed");
                    }
                }
                Ok(Output::Timeout(t)) => return t,
                Ok(Output::Event(e)) => self.on_event(e, events),
                Err(e) => {
                    self.fail(format!("Ausgabe: {e}"));
                    return Instant::now();
                }
            }
        }
    }

    fn on_event(&mut self, e: Event, events: &Sender<NetEvent>) {
        match e {
            Event::Connected => {
                self.connected = true;
                self.shared.connected.store(true, Ordering::Relaxed);
                // Was sich im Rückweg-Ring seit dem Anlegen angesammelt hat, ist alt.
                if let Some(g) = self.guest.as_mut() {
                    g.ret.flush();
                }
                tracing::info!(id = self.id, "guest net: connected");
                let _ = events.send(NetEvent::Connected { id: self.id });
            }
            Event::IceConnectionStateChange(IceConnectionState::Disconnected) => {
                self.fail("Verbindung verloren".into());
            }
            Event::MediaAdded(m) => {
                if m.kind == MediaKind::Audio && self.mid.is_none() {
                    self.mid = Some(m.mid);
                    self.pt = self.rtc.writer(m.mid).and_then(|w| {
                        w.payload_params()
                            .find(|p| p.spec().codec == Codec::Opus)
                            .map(|p| p.pt())
                    });
                    if self.pt.is_none() {
                        self.fail("Browser bietet kein Opus an".into());
                    }
                }
            }
            Event::MediaData(d) => {
                self.shared.packets_in.fetch_add(1, Ordering::Relaxed);
                self.decoded.clear();
                if !d.contiguous {
                    match self.dec.conceal(&mut self.decoded) {
                        Ok(_) => {
                            self.shared.concealed.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(e) => tracing::debug!(error = %e, "guest net: conceal failed"),
                    }
                }
                if let Err(e) = self.dec.decode(&d.data, &mut self.decoded) {
                    tracing::debug!(id = self.id, error = %e, "guest net: bad opus packet");
                    return;
                }
                if let Some(g) = self.guest.as_mut() {
                    g.feed.push(&self.decoded);
                }
            }
            Event::PeerStats(s) => {
                if let Some(r) = s.rtt {
                    self.shared.rtt_ms.store(
                        r.as_millis().min(u128::from(u32::MAX)) as u32,
                        Ordering::Relaxed,
                    );
                }
                let loss = s.ingress_loss_fraction.unwrap_or(0.0);
                self.shared
                    .loss_permille
                    .store((loss * 1000.0) as u32, Ordering::Relaxed);
                self.log_stats(loss);
            }
            _ => {}
        }
    }

    fn log_stats(&mut self, loss: f32) {
        let rtt = self.shared.rtt_ms.load(Ordering::Relaxed);
        let now = Instant::now();
        let bad = loss > WARN_LOSS || rtt > WARN_RTT_MS;
        let due = now.duration_since(self.last_log) >= LOG_EVERY;
        let warn_due = bad
            && self
                .warned_at
                .is_none_or(|t| now.duration_since(t) >= Duration::from_secs(3));
        if warn_due {
            self.warned_at = Some(now);
            tracing::warn!(
                id = self.id,
                rtt_ms = rtt,
                loss_pct = loss * 100.0,
                "guest net: poor connection"
            );
        } else if due {
            tracing::info!(
                id = self.id,
                rtt_ms = rtt,
                loss_pct = loss * 100.0,
                packets_in = self.shared.packets_in.load(Ordering::Relaxed),
                packets_out = self.shared.packets_out.load(Ordering::Relaxed),
                concealed = self.shared.concealed.load(Ordering::Relaxed),
                "guest net: stats"
            );
        }
        if due {
            self.last_log = now;
        }
    }

    /// Rückweg: PCM aus dem Ring, zu Opus, an den Browser.
    fn pump_return(
        &mut self,
        engine_rate: u32,
        now: Instant,
        socket: &UdpSocket,
        events: &Sender<NetEvent>,
    ) {
        if !self.connected || !self.rtc.is_alive() {
            return;
        }
        let (Some(mid), Some(pt)) = (self.mid, self.pt) else {
            return;
        };
        let Some(g) = self.guest.as_mut() else { return };
        let max_frames = (RETURN_MAX_SECONDS * engine_rate as f32) as usize;
        let avail = g.ret.frames();
        if avail > max_frames {
            // Zu viel Vorrat (Thread war blockiert): alten Ton verwerfen.
            let drop = avail - max_frames / 2;
            self.ret_buf.resize(drop * 2, 0.0);
            g.ret.pop_into(&mut self.ret_buf);
            tracing::debug!(
                id = self.id,
                dropped_frames = drop,
                "guest net: return backlog"
            );
        }
        let n = g.ret.frames();
        if n > 0 {
            self.ret_buf.resize(n * 2, 0.0);
            let got = g.ret.pop_into(&mut self.ret_buf);
            self.enc.push(&self.ret_buf[..got * 2]);
        }
        while let Some(p) = self.enc.next_packet() {
            let packet = match p {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(id = self.id, error = %e, "guest net: opus encode failed");
                    continue;
                }
            };
            let Some(w) = self.rtc.writer(mid) else {
                return;
            };
            let t = MediaTime::new(self.rtp_time, Frequency::FORTY_EIGHT_KHZ);
            self.rtp_time += FRAME_48K as u64;
            if let Err(e) = w.write(pt, now, t, packet) {
                tracing::debug!(id = self.id, error = %e, "guest net: write failed");
                return;
            }
            self.shared.packets_out.fetch_add(1, Ordering::Relaxed);
            // Eine Änderung je Runde: erst die Ausgabe leeren, dann das nächste Paket.
            self.drive(socket, events);
        }
    }
}

fn run(
    socket: UdpSocket,
    local: SocketAddr,
    engine_rate: u32,
    cmds: Receiver<NetCmd>,
    events: Sender<NetEvent>,
    stop: Arc<AtomicBool>,
) {
    let mut peers: HashMap<PeerId, Peer> = HashMap::new();
    let mut buf = vec![0u8; 2000];
    while !stop.load(Ordering::Relaxed) {
        // Befehle.
        loop {
            match cmds.try_recv() {
                Ok(NetCmd::Add {
                    id,
                    offer,
                    guest,
                    reply,
                }) => match add(id, &offer, guest, local, engine_rate) {
                    Ok(a) => {
                        let shared = Arc::clone(&a.peer.shared);
                        peers.insert(id, a.peer);
                        let _ = reply.send((Some(a.answer), Ok(shared)));
                    }
                    Err((e, g)) => {
                        let _ = reply.send((None, Err((e, g))));
                    }
                },
                Ok(NetCmd::Remove { id }) => {
                    if let Some(p) = peers.get_mut(&id) {
                        p.fail("Aufgelegt".into());
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    stop.store(true, Ordering::SeqCst);
                    break;
                }
            }
        }

        // Rückweg und Ausgabe.
        let now = Instant::now();
        let mut soonest = now + TICK;
        for p in peers.values_mut() {
            p.pump_return(engine_rate, now, &socket, &events);
            soonest = soonest.min(p.drive(&socket, &events));
        }

        // Beendete Verbindungen melden und freigeben.
        let dead: Vec<PeerId> = peers
            .iter()
            .filter(|(_, p)| !p.rtc.is_alive())
            .map(|(id, _)| *id)
            .collect();
        for id in dead {
            if let Some(p) = peers.remove(&id) {
                finish(p, &events);
            }
        }

        // Eingang.
        let wait = soonest
            .saturating_duration_since(Instant::now())
            .clamp(Duration::from_millis(1), TICK);
        if socket.set_read_timeout(Some(wait)).is_err() {
            std::thread::sleep(wait);
        }
        match socket.recv_from(&mut buf) {
            Ok((n, source)) => {
                if let Ok(contents) = buf[..n].try_into() {
                    let input = Input::Receive(
                        Instant::now(),
                        Receive {
                            proto: Protocol::Udp,
                            source,
                            destination: local,
                            contents,
                        },
                    );
                    match peers.values_mut().find(|p| p.accepts(&input)) {
                        Some(p) => {
                            p.handle_input(input);
                            let _ = p.drive(&socket, &events);
                        }
                        None => tracing::trace!(%source, "guest net: no peer accepts packet"),
                    }
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::ConnectionReset
                ) => {}
            Err(e) => {
                tracing::warn!(error = %e, "guest net: socket read failed");
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        let now = Instant::now();
        for p in peers.values_mut() {
            p.handle_input(Input::Timeout(now));
            let _ = p.drive(&socket, &events);
        }
    }
    for (_, mut p) in peers.drain() {
        p.fail("Programm beendet".into());
        finish(p, &events);
    }
    tracing::info!("guest net: stopped");
}

fn finish(mut p: Peer, events: &Sender<NetEvent>) {
    p.shared.connected.store(false, Ordering::Relaxed);
    let reason = p
        .reason
        .take()
        .unwrap_or_else(|| "Verbindung beendet".into());
    tracing::info!(id = p.id, %reason, "guest net: peer gone");
    if let Some(guest) = p.guest.take() {
        let _ = events.send(NetEvent::Gone {
            id: p.id,
            reason,
            guest,
        });
    }
}

struct Added {
    peer: Peer,
    answer: String,
}

fn add(
    id: PeerId,
    offer: &str,
    guest: Box<NetGuest>,
    local: SocketAddr,
    engine_rate: u32,
) -> Result<Added, (String, Box<NetGuest>)> {
    let build = || -> Result<(Rtc, String, VoiceDecoder, ReturnEncoder), String> {
        let offer = SdpOffer::from_sdp_string(offer).map_err(|e| format!("Angebot: {e}"))?;
        let mut rtc = Rtc::builder()
            .clear_codecs()
            .enable_opus(true, false)
            .set_stats_interval(Some(Duration::from_secs(1)))
            .build(Instant::now());
        let cand = Candidate::host(local, "udp").map_err(|e| format!("Kandidat: {e}"))?;
        rtc.add_local_candidate(cand);
        let answer = rtc
            .sdp_api()
            .accept_offer(offer)
            .map_err(|e| format!("Antwort: {e}"))?;
        let dec = VoiceDecoder::new(engine_rate)?;
        let enc = ReturnEncoder::new(engine_rate)?;
        Ok((rtc, answer.to_sdp_string(), dec, enc))
    };
    match build() {
        Ok((rtc, answer, dec, enc)) => Ok(Added {
            answer,
            peer: Peer {
                id,
                rtc,
                mid: None,
                pt: None,
                guest: Some(guest),
                dec,
                enc,
                decoded: Vec::with_capacity(4096),
                ret_buf: Vec::with_capacity(8192),
                rtp_time: 0,
                connected: false,
                shared: Arc::new(PeerShared::default()),
                last_log: Instant::now(),
                warned_at: None,
                reason: None,
            },
        }),
        Err(e) => Err((e, guest)),
    }
}
