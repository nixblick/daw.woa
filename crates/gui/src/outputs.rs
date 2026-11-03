//! Ausgänge je Gerät: öffnet die Ströme nach einem [`Plan`] und zeigt ihren Zustand.
//!
//! Das Hauptgerät (Playlist) taktet den Mixer. Jedes weitere Gerät bekommt einen eigenen
//! Strom; ein Ring je Ziel bringt den Ton vom Takt des Hauptgeräts in den des anderen
//! (`woa_engine::pairs`). Alle Gerätezugriffe laufen über `woa_engine::device`
//! (`on_audio_thread`, unter Windows kein fester Puffer).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use woa_engine::device::{
    ManagedOutput, OutputConfig, OutputProbe, PairsRender, Reconnect, is_xrun, probe_output,
};
use woa_engine::pairs::{LinkStats, link};
use woa_radio::routing::{DeviceInfo, OutputMap, Plan, Pult, SOURCES};
use woa_radio::{Controller, Mixer};

/// Ein Gerät, das mitten in der Sendung ausfallen kann.
pub struct Fault {
    label: String,
    flag: Arc<AtomicBool>,
    reported: bool,
}

/// Ströme in einem eigenen Thread (#291): Prüfen des Standardgeräts, Schließen und Neuöffnen
/// eines ausgefallenen Geräts dauern unter Windows (WASAPI) mitunter viele Sekunden. Liefen sie
/// im GUI-Takt, stand die Oberfläche so lange still. Der Thread lebt, solange der Wächter lebt;
/// beim Fallenlassen schließt er die Ströme und endet.
#[derive(Default)]
pub struct OutputWatch {
    tx: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// Ergebnis des letzten „Neu verbinden“ (`true`: alle laufen), einmal abholbar.
    reconnected: Arc<std::sync::Mutex<Option<bool>>>,
    /// Gab es beim Öffnen einen ersten Strom (das Hauptgerät)?
    any: bool,
}

/// So oft prüft der Wächter die Ströme.
const WATCH_EVERY: std::time::Duration = std::time::Duration::from_millis(100);

impl OutputWatch {
    /// Übernimmt die Ströme. Ohne Ströme kein Thread.
    pub fn spawn(mut outs: Vec<ManagedOutput>) -> Self {
        if outs.is_empty() {
            return Self::default();
        }
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let reconnected = Arc::new(std::sync::Mutex::new(None));
        let done = Arc::clone(&reconnected);
        let thread = std::thread::Builder::new()
            .name("woa-output-watch".into())
            .spawn(move || {
                loop {
                    match rx.recv_timeout(WATCH_EVERY) {
                        Ok(()) => {
                            let ok = outs
                                .iter_mut()
                                .all(|o| !matches!(o.reconnect_now(), Reconnect::Failed(_)));
                            if let Ok(mut r) = done.lock() {
                                *r = Some(ok);
                            }
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            for o in &mut outs {
                                o.poll();
                            }
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
                // Ströme hier schließen, nicht im GUI-Thread.
                drop(outs);
            })
            .ok();
        Self {
            tx: Some(tx),
            thread,
            reconnected,
            any: true,
        }
    }

    /// „Neu verbinden“ anstoßen; das Ergebnis holt [`Self::take_reconnected`] ab.
    pub fn reconnect_now(&self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(());
        }
    }

    /// Ergebnis des letzten „Neu verbinden“, falls schon da.
    pub fn take_reconnected(&self) -> Option<bool> {
        self.reconnected.lock().ok().and_then(|mut r| r.take())
    }

    /// Läuft ein Wächter (es gab beim Öffnen einen Strom)?
    pub fn is_active(&self) -> bool {
        self.any
    }
}

impl Drop for OutputWatch {
    fn drop(&mut self) {
        // Erst den Kanal schließen, dann warten: das Gerät ist danach frei zum Neuöffnen.
        self.tx = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Was `open` geöffnet hat.
pub struct Opened {
    /// Hauptgerät (falls es aufging) und weitere Geräte, im eigenen Thread überwacht (#291).
    pub watch: OutputWatch,
    /// Meldungen für die Oberfläche: fehlende Geräte, Ersatz, Fehler beim Öffnen.
    pub warnings: Vec<String>,
    pub faults: Vec<Fault>,
    /// Zähler der Ringe zu den weiteren Geräten, mit Anzeigename.
    pub links: Vec<(String, Arc<LinkStats>)>,
}

impl Opened {
    /// Meldungen über Geräte, die seit dem letzten Aufruf ausgefallen sind (einmal je Ausfall).
    pub fn take_new_faults(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        for f in &mut self.faults {
            if !f.reported && f.flag.load(Ordering::Relaxed) {
                f.reported = true;
                out.push(format!(
                    "Ausgang {} ist ausgefallen (Gerät gewechselt oder abgezogen?). WOA \
                     verbindet automatisch neu; sonst: Einstellungen, Ausgabe neu verbinden.",
                    f.label
                ));
            }
        }
        out
    }

    /// Rettungsknopf: alle Ströme neu öffnen (im Wächter-Thread). `false`, wenn es gar keinen
    /// Strom gibt; das Ergebnis kommt über [`OutputWatch::take_reconnected`].
    pub fn reconnect_now(&self) -> bool {
        self.watch.reconnect_now();
        self.watch.is_active()
    }

    /// Eine Zeile je Ring: Füllstand und Ausfälle, für die Einstellungen.
    pub fn link_lines(&self) -> Vec<String> {
        self.links
            .iter()
            .map(|(name, s)| {
                format!(
                    "{name}: Polster {} Frames, verworfen {}, übersprungen {}, leer gelaufen {}",
                    s.fill.load(Ordering::Relaxed),
                    s.dropped.load(Ordering::Relaxed),
                    s.skipped.load(Ordering::Relaxed),
                    s.underruns.load(Ordering::Relaxed)
                )
            })
            .collect()
    }
}

fn to_info(p: OutputProbe) -> DeviceInfo {
    DeviceInfo {
        id: p.id,
        name: p.name,
        rate: p.rate,
        channels: p.channels,
    }
}

/// Fragt ein Gerät ab (`None` = Standardgerät).
pub fn probe(id: Option<&str>) -> Result<DeviceInfo, String> {
    let config = OutputConfig {
        device_id: id.map(str::to_string),
        buffer_frames: None,
    };
    probe_output(&config)
        .map(to_info)
        .map_err(|e| e.to_string())
}

/// Fragt ein Eingangsgerät ab (Pult-Eingang).
pub fn probe_in(id: Option<&str>) -> Result<DeviceInfo, String> {
    let config = woa_engine::record::InputConfig {
        device_id: id.map(str::to_string),
        pair: 0,
    };
    woa_engine::record::probe_input(&config)
        .map(|p| DeviceInfo {
            id: p.id,
            name: p.name,
            rate: p.rate,
            channels: p.channels,
        })
        .map_err(|e| e.to_string())
}

/// Löst die Zuordnung samt Pult-Modus gegen die vorhandenen Geräte auf.
pub fn plan(map: &OutputMap, pult: &Pult, reserved_main: &[usize]) -> Plan {
    Plan::build_with_pult(map, pult, &probe, &probe_in, reserved_main)
}

/// Öffnet alle Ströme des Plans. `aux`: Vorhören auf Kanal 3/4 des Hauptgeräts (Paar 1).
/// `xruns` zählt die Aussetzer des Hauptgeräts.
/// Vorgabe für die Puffergröße der Radio-Ausgabe (21 ms bei 48 kHz).
pub const RADIO_BUFFER_FRAMES: u32 = 1024;

pub fn open(
    controller: &mut Controller,
    mixer: Mixer,
    plan: &Plan,
    buffer_frames: Option<u32>,
    mut aux: Option<woa_engine::device::AuxRender>,
    xruns: Arc<AtomicU64>,
) -> Opened {
    let main = &plan.devices[0];
    let rate = main.info.rate;
    let mut warnings = plan.warnings.clone();
    let mut routes = plan.routes;
    let (mut followers, mut faults, mut links) = (Vec::new(), Vec::new(), Vec::new());
    let mut remote = Vec::new();

    for dev in plan.devices.iter().skip(1) {
        let config = OutputConfig {
            device_id: dev.open_id.clone(),
            buffer_frames: None,
        };
        let (mut txs, mut rxs) = (Vec::new(), Vec::new());
        for &(dest, pair) in &dev.dests {
            let (tx, rx, stats) = link(rate, dev.info.rate);
            txs.push((dest, tx));
            rxs.push((pair, rx));
            links.push((
                format!("{} Kanal {}/{}", dev.info.name, 2 * pair + 1, 2 * pair + 2),
                stats,
            ));
        }
        let failed = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&failed);
        let render: PairsRender = Box::new(move |pairs, frames| {
            for (pair, rx) in rxs.iter_mut() {
                if let Some(b) = pairs.get_mut(*pair) {
                    rx.pull(&mut b.l[..frames], &mut b.r[..frames]);
                }
            }
        });
        let result =
            ManagedOutput::open("radio-extra", &config, dev.pairs, false, render, move |e| {
                if !is_xrun(&e) {
                    failed.store(true, Ordering::Relaxed);
                }
            });
        match result {
            Ok(s) => {
                tracing::info!(device = %dev.info.name, rate = dev.info.rate, pairs = dev.pairs, "second output ready");
                followers.push(s);
                faults.push(Fault {
                    label: dev.info.name.clone(),
                    flag,
                    reported: false,
                });
                remote.extend(txs);
            }
            Err(e) => {
                tracing::warn!(error = %e, device = %dev.info.name, "second output failed");
                warnings.push(format!(
                    "{}: Das Gerät lässt sich nicht öffnen ({e}). Die Quellen dort laufen \
                     vorübergehend mit der Playlist.",
                    dev.info.name
                ));
                for slot in routes.dest.iter_mut().take(SOURCES) {
                    if dev.dests.iter().any(|(d, _)| *slot as usize == *d) {
                        *slot = 0;
                    }
                }
                links.retain(|(n, _)| !n.starts_with(&dev.info.name));
            }
        }
    }
    controller.set_routes(routes);

    let mut router =
        woa_radio::routing::Router::new(mixer, plan.dest_count, main.dests.clone(), remote);
    let pairs = main.pairs.max(if aux.is_some() { 2 } else { 1 });
    let render: PairsRender = Box::new(move |bufs, frames| {
        router.render(bufs, frames);
        if let Some(f) = aux.as_mut()
            && let Some(b) = bufs.get_mut(1)
        {
            f(&mut b.l[..frames], &mut b.r[..frames]);
        }
    });
    // Ohne Angabe ein fester, sicherer Puffer statt der Treibervorgabe (PipeWire/Pulse
    // wählten teils sehr kleine Perioden, Aussetzer 29.10.2026). Lehnt das Gerät ab,
    // fällt `ManagedOutput::open` auf den Treiber zurück. Unter Windows entscheidet
    // ohnehin der Treiber (siehe `woa_engine::device`).
    let buffer_frames = buffer_frames.or(Some(RADIO_BUFFER_FRAMES));
    let config = OutputConfig {
        device_id: main.open_id.clone(),
        buffer_frames,
    };
    let failed = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&failed);
    let master = match ManagedOutput::open("radio", &config, pairs, true, render, move |e| {
        // Kann im Audio-Thread laufen: Aussetzer nur zählen, geloggt wird in `tick`.
        if is_xrun(&e) {
            xruns.fetch_add(1, Ordering::Relaxed);
        } else {
            failed.store(true, Ordering::Relaxed);
        }
    }) {
        Ok(s) => {
            tracing::info!(rate, ?buffer_frames, device = %main.info.name, pairs, "radio output ready");
            faults.push(Fault {
                label: main.info.name.clone(),
                flag,
                reported: false,
            });
            Some(s)
        }
        Err(e) => {
            tracing::error!(error = %e, "radio output failed");
            warnings.push(format!("Keine Ausgabe: {e}"));
            None
        }
    };
    // Das Hauptgerät steht vorn: es gab einen Strom, wenn es aufging.
    let watch = match master {
        Some(m) => OutputWatch::spawn(std::iter::once(m).chain(followers).collect()),
        None => {
            drop(followers);
            OutputWatch::default()
        }
    };
    Opened {
        watch,
        warnings,
        faults,
        links,
    }
}

#[cfg(test)]
mod watch_tests {
    use super::*;

    /// #291: Die Ströme wandern in den Wächter-Thread; der GUI-Takt fasst kein Gerät mehr an.
    #[test]
    fn streams_live_in_their_own_thread() {
        fn is_send<T: Send>() {}
        is_send::<ManagedOutput>();
        let w = OutputWatch::spawn(Vec::new());
        assert!(!w.is_active());
        let t = std::time::Instant::now();
        w.reconnect_now();
        assert_eq!(w.take_reconnected(), None);
        drop(w);
        assert!(t.elapsed() < std::time::Duration::from_millis(50));
    }
}
