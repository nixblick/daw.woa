//! Steuerseite der Gäste (Control-Thread): Gäste anlegen und entfernen, Fader, Stumm, Rückweg
//! wählen, Pegel und Zähler für die Anzeige, Aussetzer ins Log.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::MAX_GUESTS;
use crate::bank::{GuestBank, GuestCmd};
use crate::jitter::{GuestFeed, SlotStats, pair};
use crate::ret::{ReturnRx, pair as ret_pair};
use crate::source::{TestSignal, TestSource};

/// Befehle im Flug zur Bank.
const QUEUE: usize = 64;

/// Ein Gast aus dem Netz: Kanal, Schreibseite des Jitterpuffers und Leseseite des Rückwegs.
pub struct NetGuest {
    pub slot: usize,
    pub feed: GuestFeed,
    pub ret: ReturnRx,
}

impl std::fmt::Debug for NetGuest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NetGuest(slot {})", self.slot)
    }
}

/// Ein Gast in der Anzeige.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GuestView {
    pub slot: usize,
    pub name: String,
    pub source: String,
    pub gain_db: f32,
    pub muted: bool,
    /// Spitze seit dem letzten Bild (linear, nach dem Fader).
    pub peak: f32,
    /// Gepufferte Millisekunden.
    pub buffered_ms: f32,
    pub prebuffering: bool,
    pub underruns: u64,
    pub overflows: u64,
    /// Sein Rückweg liegt im Kopfhörer.
    pub monitored: bool,
    /// Verbindungswerte, wenn der Gast aus dem Browser kommt (Paket 38).
    pub net: Option<crate::web::NetView>,
}

struct Guest {
    name: String,
    source_label: String,
    gain_db: f32,
    muted: bool,
    source: Option<TestSource>,
    seen_underruns: u64,
    seen_overflows: u64,
}

/// Steuerseite aller Gast-Kanäle.
pub struct GuestHub {
    cmds: rtrb::Producer<GuestCmd>,
    rate: u32,
    stats: Vec<Arc<SlotStats>>,
    /// Freie Feeds; belegt = `None`.
    feeds: Vec<Option<GuestFeed>>,
    /// Freie Rückweg-Leser; belegt = `None`.
    returns: Vec<Option<ReturnRx>>,
    guests: Vec<Option<Guest>>,
    monitor: Option<usize>,
    next_number: usize,
    /// Zugang für Browser-Gäste (Paket 38), wenn eingeschaltet.
    pub(crate) web: Option<crate::web::WebGuests>,
}

/// Legt Hub und Bank für `rate` an. Die Bank gehört in den Audio-Thread.
pub fn channel(rate: u32) -> (GuestHub, GuestBank) {
    let (producer, consumer) = rtrb::RingBuffer::new(QUEUE);
    let mut stats = Vec::with_capacity(MAX_GUESTS);
    let mut feeds = Vec::with_capacity(MAX_GUESTS);
    let mut returns = Vec::with_capacity(MAX_GUESTS);
    let mut readers = Vec::with_capacity(MAX_GUESTS);
    for _ in 0..MAX_GUESTS {
        let st = Arc::new(SlotStats::default());
        let (feed, reader) = pair(rate, Arc::clone(&st));
        stats.push(Arc::clone(&st));
        feeds.push(Some(feed));
        let (ret_tx, ret_rx) = ret_pair(rate);
        returns.push(Some(ret_rx));
        readers.push((reader, st, ret_tx));
    }
    // Es sind genau MAX_GUESTS Leser; `new` scheitert nur bei anderer Zahl.
    let bank = GuestBank::new(consumer, readers, rate)
        .unwrap_or_else(|| unreachable!("MAX_GUESTS readers"));
    let hub = GuestHub {
        cmds: producer,
        rate,
        stats,
        feeds,
        returns,
        guests: (0..MAX_GUESTS).map(|_| None).collect(),
        monitor: None,
        next_number: 1,
        web: None,
    };
    (hub, bank)
}

impl GuestHub {
    fn send(&mut self, cmd: GuestCmd) {
        if self.cmds.push(cmd).is_err() {
            tracing::warn!(?cmd, "guest command queue full");
        }
    }

    /// Zahl der Gäste.
    pub fn count(&self) -> usize {
        self.guests.iter().flatten().count()
    }

    /// Belegt einen freien Kanal und schaltet ihn in der Bank an.
    fn claim(&mut self) -> Result<(usize, GuestFeed), String> {
        let slot = self
            .guests
            .iter()
            .zip(&self.feeds)
            .position(|(g, f)| g.is_none() && f.is_some())
            .ok_or_else(|| format!("Höchstens {MAX_GUESTS} Gäste"))?;
        let feed = self.feeds[slot]
            .take()
            .ok_or_else(|| "Kanal noch belegt".to_string())?;
        self.stats[slot].reset();
        self.send(GuestCmd::Activate { slot, on: true });
        self.send(GuestCmd::Gain { slot, gain: 1.0 });
        self.send(GuestCmd::Mute { slot, on: false });
        Ok((slot, feed))
    }

    /// Fügt einen Gast ohne eigene Quelle hinzu: der Aufrufer (später die WebRTC-Seite)
    /// schreibt selbst in den zurückgegebenen [`GuestFeed`]. Nach [`Self::remove`] gibt er den
    /// Feed mit [`Self::return_feed`] zurück, sonst bleibt der Kanal zu.
    pub fn add_external(&mut self, label: &str) -> Result<(usize, GuestFeed), String> {
        self.add_named(label, None)
    }

    fn add_named(&mut self, label: &str, name: Option<&str>) -> Result<(usize, GuestFeed), String> {
        let (slot, feed) = self.claim()?;
        let name = match name.map(crate::gate::clean_name).filter(|n| !n.is_empty()) {
            Some(n) => n,
            None => format!("Gast {}", self.next_number),
        };
        self.next_number += 1;
        tracing::info!(slot, name = %name, source = label, "guest added");
        self.guests[slot] = Some(Guest {
            name,
            source_label: label.to_string(),
            gain_db: 0.0,
            muted: false,
            source: None,
            seen_underruns: 0,
            seen_overflows: 0,
        });
        Ok((slot, feed))
    }

    /// Fügt einen Gast aus dem Netz hinzu (Browser per WebRTC): Kanal, Feed und Rückweg. Der
    /// Rückweg (Mix-Minus ohne den Gast) läuft sofort. Nach dem Abbau gibt der Aufrufer beides
    /// mit [`Self::return_network`] zurück.
    pub fn add_network(&mut self, label: &str, name: &str) -> Result<NetGuest, String> {
        if self.returns.iter().all(Option::is_none) {
            return Err("Kein Rückweg frei".into());
        }
        let (slot, feed) = self.add_named(label, Some(name))?;
        let Some(mut ret) = self.returns.get_mut(slot).and_then(Option::take) else {
            // Der Rückweg des Kanals ist noch bei einem alten Gast: Kanal sofort wieder frei.
            self.remove(slot);
            self.feeds[slot] = Some(feed);
            return Err("Rückweg dieses Kanals noch belegt".into());
        };
        // Reste des vorherigen Gasts im Rückweg dürfen nie zum neuen Gast durchkommen.
        ret.flush();
        self.send(GuestCmd::Return { slot, on: true });
        Ok(NetGuest { slot, feed, ret })
    }

    /// Gibt Feed und Rückweg eines abgebauten Netz-Gastes zurück; der Kanal wird wieder frei.
    pub fn return_network(&mut self, g: NetGuest) {
        let NetGuest {
            slot,
            feed,
            mut ret,
        } = g;
        ret.flush();
        self.return_feed(slot, feed);
        if self.guests.get(slot).is_some_and(Option::is_none)
            && let Some(r) = self.returns.get_mut(slot)
            && r.is_none()
        {
            *r = Some(ret);
        }
    }

    /// Gibt den Feed eines entfernten externen Gastes zurück, der Kanal wird wieder frei.
    pub fn return_feed(&mut self, slot: usize, feed: GuestFeed) {
        if self.guests.get(slot).is_some_and(Option::is_none)
            && let Some(f) = self.feeds.get_mut(slot)
            && f.is_none()
        {
            *f = Some(feed);
        }
    }

    /// Fügt einen Testgast hinzu (Sinus oder Datei). Gibt den Kanal zurück.
    pub fn add_test_guest(&mut self, signal: TestSignal) -> Result<usize, String> {
        let (slot, feed) = self.claim()?;
        let source = match TestSource::spawn(&signal, feed, self.rate) {
            Ok(s) => s,
            Err((e, feed)) => {
                self.feeds[slot] = feed;
                self.send(GuestCmd::Activate { slot, on: false });
                tracing::warn!(slot, error = %e, "test guest not started");
                return Err(e);
            }
        };
        let name = format!("Gast {}", self.next_number);
        self.next_number += 1;
        tracing::info!(slot, name = %name, source = %signal.label(), "guest added");
        self.guests[slot] = Some(Guest {
            name,
            source_label: signal.label(),
            gain_db: 0.0,
            muted: false,
            source: Some(source),
            seen_underruns: 0,
            seen_overflows: 0,
        });
        Ok(slot)
    }

    /// Entfernt einen Gast; sein Kanal wird frei. Ein externer Gast hört vorher auf zu schreiben.
    pub fn remove(&mut self, slot: usize) {
        let Some(g) = self.guests.get_mut(slot).and_then(Option::take) else {
            return;
        };
        // Ein Browser-Gast wird dabei auch im Netz getrennt.
        self.web_hangup_slot(slot);
        if self.monitor == Some(slot) {
            self.set_monitor(None);
        }
        // Erst die Quelle anhalten, dann den Kanal abschalten: die Bank leert den Ring beim
        // Abschalten, und danach darf nichts Altes mehr hineinkommen.
        if let Some(src) = g.source {
            self.feeds[slot] = src.stop();
            if self.feeds[slot].is_none() {
                tracing::warn!(slot, "guest source thread failed, channel stays closed");
            }
        }
        self.send(GuestCmd::Activate { slot, on: false });
        tracing::info!(slot, name = %g.name, "guest removed");
    }

    pub fn set_gain_db(&mut self, slot: usize, db: f32) {
        let db = db.clamp(-60.0, 12.0);
        if let Some(Some(g)) = self.guests.get_mut(slot) {
            g.gain_db = db;
            self.send(GuestCmd::Gain {
                slot,
                gain: 10f32.powf(db / 20.0),
            });
        }
    }

    pub fn set_mute(&mut self, slot: usize, on: bool) {
        if let Some(Some(g)) = self.guests.get_mut(slot) {
            g.muted = on;
            self.send(GuestCmd::Mute { slot, on });
        }
    }

    /// Rückweg dieses Gastes in den Kopfhörer (Abhörausgang); `None`: normaler Abhörmix.
    pub fn set_monitor(&mut self, slot: Option<usize>) {
        let slot = slot.filter(|&s| self.guests.get(s).is_some_and(Option::is_some));
        self.monitor = slot;
        self.send(GuestCmd::Monitor(slot));
    }

    pub fn monitor(&self) -> Option<usize> {
        self.monitor
    }

    /// Schreibt neue Aussetzer und Überläufe ins Log und gibt die letzte Meldung (für die
    /// Anzeige) zurück. Aufruf im Control-Takt.
    pub fn poll(&mut self) -> Option<String> {
        self.web_tick();
        let mut last = None;
        for (slot, g) in self.guests.iter_mut().enumerate() {
            let Some(g) = g else { continue };
            let st = &self.stats[slot];
            let u = st.underruns.load(Ordering::Relaxed);
            let o = st.overflows.load(Ordering::Relaxed);
            if u > g.seen_underruns {
                tracing::warn!(slot, name = %g.name, new = u - g.seen_underruns, total = u,
                    "guest buffer underrun (silence inserted)");
                last = Some(format!("{}: Aussetzer ({u})", g.name));
                g.seen_underruns = u;
            }
            if o > g.seen_overflows {
                tracing::warn!(slot, name = %g.name, new = o - g.seen_overflows, total = o,
                    dropped_frames = st.dropped_frames.load(Ordering::Relaxed),
                    "guest buffer overflow (audio dropped)");
                last = Some(format!("{}: Puffer übergelaufen ({o})", g.name));
                g.seen_overflows = o;
            }
        }
        last
    }

    /// Anzeige aller Gäste; holt dabei die Spitze ab.
    pub fn view(&self) -> Vec<GuestView> {
        self.guests
            .iter()
            .enumerate()
            .filter_map(|(slot, g)| {
                let g = g.as_ref()?;
                let st = &self.stats[slot];
                Some(GuestView {
                    slot,
                    name: g.name.clone(),
                    source: g.source_label.clone(),
                    gain_db: g.gain_db,
                    muted: g.muted,
                    peak: st.take_peak(),
                    buffered_ms: st.buffered.load(Ordering::Relaxed) as f32 * 1000.0
                        / self.rate.max(1) as f32,
                    prebuffering: st.prebuffering.load(Ordering::Relaxed),
                    underruns: st.underruns.load(Ordering::Relaxed),
                    overflows: st.overflows.load(Ordering::Relaxed),
                    monitored: self.monitor == Some(slot),
                    net: self.net_view(slot),
                })
            })
            .collect()
    }

    /// Zähler eines Kanals (für Tests und Anzeige).
    pub fn stats(&self, slot: usize) -> Option<&Arc<SlotStats>> {
        self.stats.get(slot)
    }
}

impl Drop for GuestHub {
    fn drop(&mut self) {
        for g in self.guests.iter_mut().flatten() {
            drop(g.source.take());
        }
    }
}
