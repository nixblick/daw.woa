//! Ausgänge frei zuordnen: jede Quelle bekommt „Gerät + Kanalpaar“.
//!
//! Das Hauptgerät (das der Playlist) taktet den [`crate::Mixer`]. Jede andere Zuordnung ist
//! ein Ziel des Mixers: Liegt es auf dem Hauptgerät, geht es auf ein anderes Kanalpaar
//! desselben Stroms; liegt es auf einem anderen Gerät, läuft es über einen Ring
//! ([`woa_engine::pairs::link`]) in den eigenen Strom dieses Geräts. Der Pegel wird am Pult
//! gefahren: Die getrennten Ausgänge tragen das Signal vor dem Sendebus (Lautheitsregelung,
//! Kompressor, Limiter), nach den Reglern der Oberfläche.
//!
//! [`Plan::build`] löst eine [`OutputMap`] gegen die vorhandenen Geräte auf, ohne ein Gerät
//! zu öffnen (offline testbar). [`Router`] sitzt im Audio-Callback des Hauptgeräts.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use woa_engine::pairs::{LinkTx, PairBuf};

use crate::mixer::Mixer;

/// Eine Quelle, die einen eigenen Ausgang bekommen kann.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Playlist (Musik, Voice-Tracks, Mikro-Abhören, Gäste): taktet den Mixer, ist das
    /// Hauptgerät.
    Playlist,
    /// Cart-Wall.
    Carts,
}

/// Zahl der Quellen.
pub const SOURCES: usize = 2;

impl Source {
    /// Alle Quellen in der Reihenfolge der Oberfläche.
    pub const ALL: [Source; SOURCES] = [Source::Playlist, Source::Carts];

    pub fn index(self) -> usize {
        match self {
            Source::Playlist => 0,
            Source::Carts => 1,
        }
    }

    /// Name in der Oberfläche.
    pub fn label(self) -> &'static str {
        match self {
            Source::Playlist => "Playlist",
            Source::Carts => "Cart-Wall",
        }
    }
}

/// Wohin eine Quelle geht: Gerät und Kanalpaar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    /// Kennung des Geräts, `None` = Standardgerät.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    /// Name des Geräts bei der Wahl, nur für verständliche Meldungen, wenn es fehlt.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub device_name: String,
    /// Kanalpaar: 0 = Kanal 1/2, 1 = Kanal 3/4 …
    #[serde(default)]
    pub pair: u8,
}

impl Route {
    /// Name für Meldungen.
    pub fn device_label(&self) -> String {
        if self.device_name.is_empty() {
            match &self.device_id {
                Some(id) => id.clone(),
                None => "Standardgerät".into(),
            }
        } else {
            self.device_name.clone()
        }
    }
}

/// Die Zuordnung aller Quellen; gespeichert in `config.json`. Fehlt eine Quelle, gilt das
/// Standardgerät, Kanal 1/2.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OutputMap {
    routes: BTreeMap<Source, Route>,
}

impl OutputMap {
    pub fn is_default(&self) -> bool {
        self.routes.values().all(|r| *r == Route::default())
    }

    /// Route einer Quelle (Vorgabe, wenn nicht gesetzt).
    pub fn get(&self, s: Source) -> Route {
        self.routes.get(&s).cloned().unwrap_or_default()
    }

    pub fn set(&mut self, s: Source, r: Route) {
        if r == Route::default() {
            self.routes.remove(&s);
        } else {
            self.routes.insert(s, r);
        }
    }
}

/// Ziel-Nummer je Quelle für den Mixer. Ziel 0 ist der Hauptausgang (Playlist).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Routes {
    pub dest: [u8; SOURCES],
}

/// Was ein Gerät beim Prüfen anbietet (aus `woa_engine::device::probe_output`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub rate: u32,
    pub channels: u16,
}

impl DeviceInfo {
    /// Zahl der Stereo-Paare, die das Gerät hat (ein Kanal zählt als ein Paar).
    pub fn pairs(&self) -> usize {
        (usize::from(self.channels) / 2).max(1)
    }
}

/// Ein Gerät im Plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanDevice {
    /// Kennung, wie sie geöffnet wird (`None` = Standardgerät).
    pub open_id: Option<String>,
    pub info: DeviceInfo,
    /// Paare, die geöffnet werden (höchstes benutztes Paar + 1).
    pub pairs: usize,
    /// Ziele (Nummer) und ihr Paar auf diesem Gerät.
    pub dests: Vec<(usize, usize)>,
}

/// Aufgelöste Zuordnung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Gerät 0 ist das Hauptgerät.
    pub devices: Vec<PlanDevice>,
    pub routes: Routes,
    pub dest_count: usize,
    /// Meldungen für die Oberfläche (fehlende Geräte, ungültige Paare, Ersatz).
    pub warnings: Vec<String>,
}

/// Paar `pair` gibt es auf einem Gerät mit `channels` Kanälen (ein Kanal: nur Paar 0).
fn pair_fits(pair: usize, channels: u16) -> bool {
    if channels < 2 {
        pair == 0
    } else {
        2 * pair + 2 <= usize::from(channels)
    }
}

impl Plan {
    /// Löst `map` auf. `probe(id)` liefert das Gerät (`None` = Standardgerät) oder den
    /// Grund, warum es fehlt. `reserved_main`: Paare des Hauptgeräts, die schon belegt sind
    /// (Vorhören auf Kanal 3/4 belegt Paar 1).
    ///
    /// Fehlt das Gerät der Playlist, läuft die Sendung über das Standardgerät (mit Meldung).
    /// Fehlt das Gerät einer anderen Quelle oder passt das Paar nicht, läuft diese Quelle
    /// über den Hauptausgang mit der Playlist (mit Meldung), damit nichts verstummt.
    pub fn build(
        map: &OutputMap,
        probe: &dyn Fn(Option<&str>) -> Result<DeviceInfo, String>,
        reserved_main: &[usize],
    ) -> Plan {
        let mut warnings = Vec::new();
        let main_route = map.get(Source::Playlist);
        let mut main_pair = usize::from(main_route.pair);
        let (main_open, main_info) = match probe(main_route.device_id.as_deref()) {
            Ok(i) => (main_route.device_id.clone(), i),
            Err(e) => {
                warnings.push(format!(
                    "Playlist: {} fehlt ({e}). Die Sendung läuft über das Standardgerät.",
                    main_route.device_label()
                ));
                main_pair = 0;
                match probe(None) {
                    Ok(i) => (None, i),
                    Err(e2) => {
                        warnings.push(format!("Kein Standardgerät: {e2}"));
                        (
                            None,
                            DeviceInfo {
                                id: String::new(),
                                name: String::new(),
                                rate: 48_000,
                                channels: 2,
                            },
                        )
                    }
                }
            }
        };
        if !pair_fits(main_pair, main_info.channels) || reserved_main.contains(&main_pair) {
            warnings.push(format!(
                "Playlist: Kanal {}/{} geht auf {} nicht. Es gilt Kanal 1/2.",
                2 * main_pair + 1,
                2 * main_pair + 2,
                main_info.name
            ));
            main_pair = 0;
        }
        let mut devices = vec![PlanDevice {
            open_id: main_open,
            info: main_info,
            pairs: main_pair + 1,
            dests: vec![(0, main_pair)],
        }];
        let mut dest_count = 1usize;
        let mut routes = Routes::default();
        for s in Source::ALL {
            if s == Source::Playlist {
                continue;
            }
            let r = map.get(s);
            let pair = usize::from(r.pair);
            let info = match probe(r.device_id.as_deref()) {
                Ok(i) => i,
                Err(e) => {
                    warnings.push(format!(
                        "{}: {} fehlt ({e}). Sie läuft vorübergehend mit der Playlist.",
                        s.label(),
                        r.device_label()
                    ));
                    continue;
                }
            };
            if !pair_fits(pair, info.channels) {
                warnings.push(format!(
                    "{}: {} hat kein Kanal {}/{}. Sie läuft vorübergehend mit der Playlist.",
                    s.label(),
                    info.name,
                    2 * pair + 1,
                    2 * pair + 2
                ));
                continue;
            }
            let slot = match devices.iter().position(|d| d.info.id == info.id) {
                Some(i) => i,
                None => {
                    devices.push(PlanDevice {
                        open_id: r.device_id.clone(),
                        info,
                        pairs: 1,
                        dests: Vec::new(),
                    });
                    devices.len() - 1
                }
            };
            if slot == 0 && reserved_main.contains(&pair) {
                warnings.push(format!(
                    "{}: Kanal {}/{} ist fürs Vorhören reserviert. Sie läuft vorübergehend mit der Playlist.",
                    s.label(),
                    2 * pair + 1,
                    2 * pair + 2
                ));
                continue;
            }
            // Dasselbe Gerät und Paar wie ein anderes Ziel: dasselbe Ziel (die Quellen
            // werden dort addiert).
            let dev = &mut devices[slot];
            let dest = match dev.dests.iter().find(|(_, p)| *p == pair) {
                Some((d, _)) => *d,
                None => {
                    let d = dest_count;
                    dest_count += 1;
                    dev.dests.push((d, pair));
                    dev.pairs = dev.pairs.max(pair + 1);
                    d
                }
            };
            routes.dest[s.index()] = dest as u8;
        }
        // Ein Gerät ohne Ziel (nur durch Ausfälle möglich) fliegt raus; das Hauptgerät bleibt.
        devices.retain(|d| !d.dests.is_empty());
        Plan {
            devices,
            routes,
            dest_count,
            warnings,
        }
    }
}

/// Der Audio-Callback des Hauptgeräts: rechnet den Mixer in alle Ziele und verteilt sie auf
/// Kanalpaare des Hauptgeräts und auf die Ringe zu den anderen Geräten.
/// Keine Allokation, kein Lock.
pub struct Router {
    mixer: Mixer,
    bufs: Vec<PairBuf>,
    /// (Ziel, Paar auf dem Hauptgerät)
    local: Vec<(usize, usize)>,
    /// (Ziel, Ring zum anderen Gerät)
    remote: Vec<(usize, LinkTx)>,
}

/// Größter Block, den [`Router::render`] in einem Stück rechnet.
pub const MAX_BLOCK: usize = 4096;

impl Router {
    pub fn new(
        mixer: Mixer,
        dest_count: usize,
        local: Vec<(usize, usize)>,
        remote: Vec<(usize, LinkTx)>,
    ) -> Self {
        Self {
            mixer,
            bufs: (0..dest_count.max(1))
                .map(|_| PairBuf::new(MAX_BLOCK))
                .collect(),
            local,
            remote,
        }
    }

    /// Rechnet `frames` Frames; `pairs` sind die Paare des Hauptgeräts.
    pub fn render(&mut self, pairs: &mut [PairBuf], frames: usize) {
        let n = frames.min(MAX_BLOCK);
        self.mixer.process_multi(&mut self.bufs, n);
        for &(dest, pair) in &self.local {
            if let (Some(src), Some(dst)) = (self.bufs.get(dest), pairs.get_mut(pair)) {
                dst.l[..n].copy_from_slice(&src.l[..n]);
                dst.r[..n].copy_from_slice(&src.r[..n]);
            }
        }
        for (dest, tx) in self.remote.iter_mut() {
            if let Some(src) = self.bufs.get(*dest) {
                tx.push(&src.l[..n], &src.r[..n]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(id: &str, rate: u32, channels: u16) -> DeviceInfo {
        DeviceInfo {
            id: id.into(),
            name: format!("Karte {id}"),
            rate,
            channels,
        }
    }

    /// Geräte „1“ (Standard, 2 Kanäle) und „2“ (4 Kanäle); „weg“ fehlt.
    fn probe(id: Option<&str>) -> Result<DeviceInfo, String> {
        match id {
            None | Some("1") => Ok(dev("1", 48_000, 2)),
            Some("2") => Ok(dev("2", 44_100, 4)),
            Some(_) => Err("nicht gefunden".into()),
        }
    }

    fn route(id: Option<&str>, pair: u8) -> Route {
        Route {
            device_id: id.map(str::to_string),
            device_name: String::new(),
            pair,
        }
    }

    #[test]
    fn default_map_is_one_device_one_pair() {
        let p = Plan::build(&OutputMap::default(), &probe, &[]);
        assert_eq!(p.devices.len(), 1);
        assert_eq!(p.dest_count, 1);
        assert_eq!(p.routes, Routes::default());
        assert!(p.warnings.is_empty());
    }

    #[test]
    fn two_cards_run_in_parallel() {
        let mut m = OutputMap::default();
        m.set(Source::Playlist, route(Some("1"), 0));
        m.set(Source::Carts, route(Some("2"), 1));
        let p = Plan::build(&m, &probe, &[]);
        assert_eq!(p.devices.len(), 2);
        assert_eq!(p.devices[1].open_id.as_deref(), Some("2"));
        assert_eq!(p.devices[1].pairs, 2);
        assert_eq!(p.devices[1].dests, vec![(1, 1)]);
        assert_eq!(p.routes.dest, [0, 1]);
        assert!(p.warnings.is_empty());
    }

    #[test]
    fn same_device_other_pair_stays_on_the_main_stream() {
        let mut m = OutputMap::default();
        m.set(Source::Playlist, route(Some("2"), 0));
        m.set(Source::Carts, route(Some("2"), 1));
        let p = Plan::build(&m, &probe, &[]);
        assert_eq!(p.devices.len(), 1);
        assert_eq!(p.devices[0].pairs, 2);
        assert_eq!(p.routes.dest, [0, 1]);
        // Das Standardgerät ist Karte 1: dieselbe Karte unter anderem Namen zählt als eine.
        let mut m = OutputMap::default();
        m.set(Source::Carts, route(Some("1"), 0));
        let p = Plan::build(&m, &probe, &[]);
        assert_eq!(p.devices.len(), 1);
        assert_eq!(p.routes.dest, [0, 0]);
    }

    #[test]
    fn missing_device_is_reported_and_nothing_goes_silent() {
        let mut m = OutputMap::default();
        m.set(Source::Carts, route(Some("weg"), 0));
        let p = Plan::build(&m, &probe, &[]);
        assert_eq!(p.routes.dest, [0, 0]);
        assert_eq!(p.warnings.len(), 1);
        assert!(p.warnings[0].contains("Cart-Wall") && p.warnings[0].contains("fehlt"));
        // Fehlt das Gerät der Playlist, läuft sie über das Standardgerät.
        let mut m = OutputMap::default();
        m.set(Source::Playlist, route(Some("weg"), 0));
        let p = Plan::build(&m, &probe, &[]);
        assert_eq!(p.devices[0].open_id, None);
        assert!(p.warnings[0].contains("Playlist") && p.warnings[0].contains("Standardgerät"));
    }

    #[test]
    fn pair_beyond_the_channels_and_reserved_pair_are_refused() {
        let mut m = OutputMap::default();
        m.set(Source::Carts, route(Some("1"), 1));
        let p = Plan::build(&m, &probe, &[]);
        assert_eq!(p.routes.dest, [0, 0]);
        assert!(p.warnings[0].contains("Kanal 3/4"));
        let mut m = OutputMap::default();
        m.set(Source::Playlist, route(Some("2"), 0));
        m.set(Source::Carts, route(Some("2"), 1));
        let p = Plan::build(&m, &probe, &[1]);
        assert_eq!(p.routes.dest, [0, 0]);
        assert!(p.warnings[0].contains("Vorhören"));
    }

    #[test]
    fn map_round_trips_and_default_is_empty() {
        let mut m = OutputMap::default();
        assert!(m.is_default());
        assert_eq!(serde_json::to_string(&m).unwrap(), "{}");
        m.set(Source::Carts, route(Some("2"), 1));
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(json, r#"{"carts":{"device_id":"2","pair":1}}"#);
        assert_eq!(serde_json::from_str::<OutputMap>(&json).unwrap(), m);
        m.set(Source::Carts, Route::default());
        assert!(m.is_default());
    }
}
