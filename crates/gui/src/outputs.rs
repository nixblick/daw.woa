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

/// Was `open` geöffnet hat.
pub struct Opened {
    /// Strom des Hauptgeräts, `None` wenn es sich nicht öffnen ließ.
    pub _master: Option<ManagedOutput>,
    /// Ströme der weiteren Geräte (leben, solange das Feld lebt).
    pub _followers: Vec<ManagedOutput>,
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

    /// Öffnet ausgefallene Ströme neu und folgt einem gewechselten Standardgerät. Im GUI-Takt
    /// aufrufen, nie im Audio-Thread.
    pub fn poll(&mut self) {
        for out in self._master.iter_mut().chain(self._followers.iter_mut()) {
            out.poll();
        }
    }

    /// Rettungsknopf: alle Ströme sofort neu öffnen. `true`, wenn alle liefen.
    pub fn reconnect_now(&mut self) -> bool {
        let mut ok = self._master.is_some();
        for out in self._master.iter_mut().chain(self._followers.iter_mut()) {
            ok &= !matches!(out.reconnect_now(), Reconnect::Failed(_));
        }
        ok
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
    Opened {
        _master: master,
        _followers: followers,
        warnings,
        faults,
        links,
    }
}
