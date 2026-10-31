//! Stundenuhr und AutoDJ-Rotation (Paket 28).
//!
//! Eine **Stundenuhr** ist eine Folge von Slots, jeder Slot hat eine Kategorie (ein Tag der
//! Bibliothek, z. B. `hit`, `neu`, `jingle`) und optional eine Ersatzkategorie. Die Woche
//! (7 × 24 Stunden) ordnet jeder Stunde eine Uhr zu. Der Planer wählt für den nächsten Slot
//! einen Titel aus der Kategorie: gewichtet nach der Zeit seit dem letzten Spiel, mit
//! Sperrfristen für Titel und Interpret aus der Spielhistorie (`plays`). Ist der Slot leer
//! (keine Titel oder alle gesperrt), kommt die Ersatzkategorie, danach die Notfall-Kategorie.
//!
//! Idee der Kette Slot, Ersatz, Notfall nach `fallback`/`rotate` in Liquidsoap
//! (`src/libs/switches.liq`, GPLv2+) und dem Auffüllen einer Warteschlange wie im AutoDJ von
//! Mixxx (`src/library/autodj`, GPLv2+). Es ist kein Code übernommen, nur das Muster.
//!
//! Alles läuft im Control- oder Hintergrund-Thread, nie im Audio-Thread. Die Wahl ist
//! deterministisch: gleicher Startwert, gleiche Bibliothek, gleiche Historie, gleiche Folge.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use woa_engine::localtime::Zone;

use crate::{Error, Library, MAX_RESULTS, Query, Result, Track, normalize_tag};

/// Formatversion von `stundenuhr.json`.
pub const VERSION: u32 = 1;
/// Name der Datei im Datenordner.
pub const FILE: &str = "stundenuhr.json";
/// Stunden der Woche.
pub const WEEK_HOURS: usize = 168;
pub const MAX_CLOCKS: usize = 64;
pub const MAX_SLOTS: usize = 64;
pub const MAX_NAME_CHARS: usize = 40;
/// Obergrenze der Sperrfristen (Minuten): eine Woche.
pub const MAX_LOCKOUT_MIN: u32 = 7 * 24 * 60;
pub const MAX_LOOKAHEAD: usize = 20;
/// So alt (Stunden) zählt ein Titel höchstens für das Gewicht.
const MAX_AGE_HOURS: f64 = 168.0;

fn default_emergency() -> String {
    "notfall".into()
}
fn default_track_lock() -> u32 {
    180
}
fn default_artist_lock() -> u32 {
    60
}
fn default_offset() -> i32 {
    60
}
fn default_lookahead() -> usize {
    3
}
fn version() -> u32 {
    VERSION
}

/// Ein Platz in der Stunde.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slot {
    /// Tag, aus dem der Titel kommt.
    pub category: String,
    /// Tag, falls die Kategorie leer ist (leer: keiner).
    #[serde(default)]
    pub fallback: String,
}

impl Slot {
    pub fn new(category: &str) -> Self {
        Self {
            category: category.into(),
            fallback: String::new(),
        }
    }
}

/// Eine Stundenuhr.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HourClock {
    pub name: String,
    pub slots: Vec<Slot>,
}

/// Stundenuhren, Wochenzuordnung und Regeln. Gespeichert als `stundenuhr.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schedule {
    #[serde(default = "version")]
    pub version: u32,
    pub clocks: Vec<HourClock>,
    /// 168 Namen (Montag 0 Uhr zuerst), leer = keine Uhr in dieser Stunde.
    #[serde(default)]
    pub week: Vec<String>,
    /// Notfall-Kategorie (Tag), wenn Slot und Ersatz nichts liefern.
    #[serde(default = "default_emergency")]
    pub emergency: String,
    /// Sperrfrist für denselben Titel, Minuten.
    #[serde(default = "default_track_lock")]
    pub track_lockout_min: u32,
    /// Sperrfrist für denselben Interpreten, Minuten.
    #[serde(default = "default_artist_lock")]
    pub artist_lockout_min: u32,
    /// Ortszeit gegenüber UTC in Minuten im Winter (Mitteleuropa 60). Mit `auto_dst` kommt
    /// im Sommer eine Stunde dazu; ohne den Schalter gilt der Wert das ganze Jahr.
    #[serde(default = "default_offset")]
    pub utc_offset_min: i32,
    /// EU-Sommerzeit automatisch dazurechnen (letzter Sonntag im März bis Oktober). Dateien
    /// aus Paket 28 haben den Schalter nicht und behalten ihren festen Versatz.
    #[serde(default)]
    pub auto_dst: bool,
    /// Der AutoDJ hält so viele Titel im Voraus bereit.
    #[serde(default = "default_lookahead")]
    pub lookahead: usize,
    /// Startwert der Zufallsfolge.
    #[serde(default)]
    pub seed: u64,
    /// AutoDJ eingeschaltet.
    #[serde(default)]
    pub autodj: bool,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            version: VERSION,
            clocks: Vec::new(),
            week: vec![String::new(); WEEK_HOURS],
            emergency: default_emergency(),
            track_lockout_min: default_track_lock(),
            artist_lockout_min: default_artist_lock(),
            utc_offset_min: default_offset(),
            auto_dst: true,
            lookahead: default_lookahead(),
            seed: 0,
            autodj: false,
        }
    }
}

fn check_name(what: &str, s: &str) -> Result<()> {
    if s.is_empty() || s.chars().count() > MAX_NAME_CHARS || s.chars().any(char::is_control) {
        return Err(Error::Invalid(format!(
            "{what}: leer, länger als {MAX_NAME_CHARS} Zeichen oder mit Steuerzeichen"
        )));
    }
    Ok(())
}

impl Schedule {
    /// Die Zone der Stundenuhr.
    pub fn zone(&self) -> Zone {
        Zone {
            base_min: self.utc_offset_min,
            eu_dst: self.auto_dst,
        }
    }

    /// Eine Beispiel-Uhr, damit der Editor nicht leer beginnt.
    pub fn with_example() -> Self {
        let mut s = Self::default();
        s.clocks.push(HourClock {
            name: "Standard".into(),
            slots: vec![
                Slot {
                    category: "hit".into(),
                    fallback: "neu".into(),
                },
                Slot::new("hit"),
                Slot::new("neu"),
                Slot::new("jingle"),
            ],
        });
        s
    }

    /// Prüft Grenzen und bringt die Woche auf 168 Einträge; unbekannte Uhren in der Woche
    /// werden geleert. Gibt die Zahl der Korrekturen zurück.
    pub fn sanitize(&mut self) -> Result<usize> {
        if self.clocks.len() > MAX_CLOCKS {
            return Err(Error::Invalid(format!("Mehr als {MAX_CLOCKS} Uhren")));
        }
        for c in &self.clocks {
            check_name("Name der Uhr", &c.name)?;
            if c.slots.len() > MAX_SLOTS {
                return Err(Error::Invalid(format!(
                    "Uhr {}: mehr als {MAX_SLOTS} Slots",
                    c.name
                )));
            }
        }
        let mut fixed = 0;
        for c in &mut self.clocks {
            for s in &mut c.slots {
                s.category = normalize_tag(&s.category)?;
                if !s.fallback.trim().is_empty() {
                    s.fallback = normalize_tag(&s.fallback)?;
                } else {
                    s.fallback.clear();
                }
            }
        }
        self.emergency = if self.emergency.trim().is_empty() {
            String::new()
        } else {
            normalize_tag(&self.emergency)?
        };
        if self.week.len() != WEEK_HOURS {
            self.week.resize(WEEK_HOURS, String::new());
            fixed += 1;
        }
        let names: Vec<String> = self.clocks.iter().map(|c| c.name.clone()).collect();
        for w in &mut self.week {
            if !w.is_empty() && !names.contains(w) {
                w.clear();
                fixed += 1;
            }
        }
        self.track_lockout_min = self.track_lockout_min.min(MAX_LOCKOUT_MIN);
        self.artist_lockout_min = self.artist_lockout_min.min(MAX_LOCKOUT_MIN);
        self.utc_offset_min = self.utc_offset_min.clamp(-14 * 60, 14 * 60);
        self.lookahead = self.lookahead.clamp(1, MAX_LOOKAHEAD);
        Ok(fixed)
    }

    /// Lädt die Datei. Fehlt sie, kommt die Beispiel-Uhr. Kaputtes wird gemeldet, nie geraten.
    pub fn load(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::with_example()),
            Err(e) => return Err(e.into()),
        };
        let mut s: Self = serde_json::from_str(&text)
            .map_err(|e| Error::Invalid(format!("Stundenuhr: Datei nicht lesbar ({e})")))?;
        if s.version > VERSION {
            return Err(Error::Invalid(format!(
                "Stundenuhr stammt von einer neueren WOA-Version ({})",
                s.version
            )));
        }
        let fixed = s.sanitize()?;
        if fixed > 0 {
            tracing::warn!(fixed, "schedule corrected on load");
        }
        Ok(s)
    }

    /// Speichert atomar (Temp-Datei im selben Ordner, dann umbenennen). Ein Symlink als Ziel
    /// wird abgelehnt.
    pub fn save(&self, path: &Path) -> Result<()> {
        if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(Error::Invalid("Stundenuhr: Ziel ist ein Symlink".into()));
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| Error::Invalid(format!("Stundenuhr: {e}")))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Uhr einer Stunde (Wochentag Montag = 0).
    pub fn clock_for(&self, weekday: u8, hour: u8) -> Option<&HourClock> {
        let name = self
            .week
            .get(usize::from(weekday) * 24 + usize::from(hour))?;
        if name.is_empty() {
            return None;
        }
        self.clocks
            .iter()
            .find(|c| &c.name == name && !c.slots.is_empty())
    }

    /// Ordnet einer Stunde eine Uhr zu (`None` leert sie).
    pub fn set_hour(&mut self, weekday: u8, hour: u8, clock: Option<&str>) -> Result<()> {
        if weekday > 6 || hour > 23 {
            return Err(Error::Invalid("Stunde außerhalb der Woche".into()));
        }
        if self.week.len() != WEEK_HOURS {
            self.week.resize(WEEK_HOURS, String::new());
        }
        let name = match clock {
            Some(n) if self.clocks.iter().any(|c| c.name == n) => n.to_string(),
            Some(_) => return Err(Error::Invalid("Diese Uhr gibt es nicht".into())),
            None => String::new(),
        };
        self.week[usize::from(weekday) * 24 + usize::from(hour)] = name;
        Ok(())
    }
}

/// Ortszeit-Teile eines Zeitpunkts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalHour {
    /// Montag = 0.
    pub weekday: u8,
    pub hour: u8,
    /// Ortsstunden seit 1970 (Schlüssel der Stunde).
    pub key: i64,
}

/// Wochentag, Stunde und Stundenschlüssel für `unix` in Zone `zone`.
pub fn local_hour(unix: i64, zone: Zone) -> LocalHour {
    let l = zone.local(unix);
    LocalHour {
        weekday: l.weekday,
        hour: l.hour,
        key: l.hour_key,
    }
}

/// Beginn der Ortsstunde `key` als Unix-Sekunden (in der doppelten Stunde im Herbst der
/// erste Durchlauf; in der übersprungenen Stunde im Frühjahr der Moment der Umstellung).
pub fn hour_start(key: i64, zone: Zone) -> i64 {
    let day = key.div_euclid(24);
    let sod = key.rem_euclid(24) * 3600;
    zone.to_unix(day, sod)
        .or_else(|| zone.gap_end(day, sod))
        .unwrap_or(key * 3600 - i64::from(zone.base_min) * 60)
}

/// Interpret aus dem Titel nach dem Muster „Interpret - Titel“; ohne Trenner leer
/// (dann gilt nur die Titel-Sperrfrist).
pub fn artist_of(title: &str) -> String {
    title
        .split_once(" - ")
        .map(|(a, _)| {
            a.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
        })
        .unwrap_or_default()
}

/// Woher die Wahl kam.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Kategorie des Slots.
    Slot,
    /// Ersatzkategorie des Slots.
    Fallback,
    /// Notfall-Kategorie.
    Emergency,
}

impl Tier {
    pub fn label(self) -> &'static str {
        match self {
            Self::Slot => "Slot",
            Self::Fallback => "Ersatz",
            Self::Emergency => "Notfall",
        }
    }
}

/// Eine getroffene Wahl.
#[derive(Debug, Clone, PartialEq)]
pub struct Pick {
    pub track_id: i64,
    pub path: String,
    pub title: String,
    pub artist: String,
    pub seconds: f64,
    /// Kategorie, aus der der Titel kam.
    pub category: String,
    /// Kategorie, die der Slot wollte.
    pub wanted: String,
    pub tier: Tier,
    /// Nummer des Slots in der Uhr.
    pub slot: usize,
    pub clock: String,
    /// Voraussichtlicher Sendebeginn (Unix-Sekunden).
    pub at: i64,
}

impl Pick {
    /// Eine Logzeile für die Auswahl.
    pub fn describe(&self) -> String {
        format!(
            "Uhr {} Slot {} wollte {}: {} aus {} ({})",
            self.clock,
            self.slot + 1,
            self.wanted,
            self.title,
            self.category,
            self.tier.label()
        )
    }
}

/// Schon bereitliegender Titel (von Hand gesetzt oder früher geplant): belegt Zeit und
/// zählt für die Sperrfristen wie ein Spiel.
#[derive(Debug, Clone, Default)]
pub struct Ahead {
    pub track_id: Option<i64>,
    pub artist: String,
    pub seconds: f64,
}

/// Zustand des AutoDJ zwischen zwei Planungen.
#[derive(Debug, Clone)]
pub struct AutoDjState {
    hour_key: Option<i64>,
    count: usize,
    rng: u64,
}

impl AutoDjState {
    pub fn new(seed: u64) -> Self {
        Self {
            hour_key: None,
            count: 0,
            rng: seed,
        }
    }

    /// Ein Zufallswert in [0, 1) (SplitMix64, Sebastiano Vigna, gemeinfrei).
    fn next_f64(&mut self) -> f64 {
        self.rng = self.rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    }
}

struct Cand {
    id: i64,
    path: String,
    title: String,
    artist: String,
    seconds: f64,
}

fn load_category(lib: &Library, tag: &str) -> Result<Vec<Cand>> {
    if tag.is_empty() {
        return Ok(Vec::new());
    }
    let tracks: Vec<Track> = lib.search(&Query {
        text: String::new(),
        tag: Some(tag.to_string()),
        kind: None,
        limit: MAX_RESULTS,
        ..Query::default()
    })?;
    Ok(tracks
        .into_iter()
        .filter(|t| t.duration >= 1.0)
        .map(|t| Cand {
            id: t.id,
            artist: artist_of(&t.title),
            path: t.path,
            title: t.title,
            seconds: t.duration,
        })
        .collect())
}

struct History {
    track: HashMap<i64, i64>,
    artist: HashMap<String, i64>,
}

impl History {
    fn mark(&mut self, id: Option<i64>, artist: &str, at: i64) {
        if let Some(id) = id {
            self.track.insert(id, at);
        }
        if !artist.is_empty() {
            self.artist.insert(artist.to_string(), at);
        }
    }
}

struct Lockouts {
    track_s: i64,
    artist_s: i64,
}

/// Wählt aus `cands` nach Gewicht (Alter seit dem letzten Spiel). `strict`: Sperrfristen
/// gelten. Sonst (Notfall): erst ohne Sperre, ist alles gesperrt, der am längsten nicht
/// gespielte Titel.
fn choose(
    cands: &[Cand],
    hist: &History,
    vt: i64,
    lock: &Lockouts,
    strict: bool,
    fit: Option<f64>,
    st: &mut AutoDjState,
) -> Option<usize> {
    let free: Vec<usize> = (0..cands.len())
        .filter(|&i| {
            let c = &cands[i];
            hist.track.get(&c.id).is_none_or(|l| vt - l >= lock.track_s)
                && (c.artist.is_empty()
                    || hist
                        .artist
                        .get(&c.artist)
                        .is_none_or(|l| vt - l >= lock.artist_s))
        })
        .collect();
    if free.is_empty() {
        if strict || cands.is_empty() {
            return None;
        }
        return (0..cands.len()).min_by_key(|&i| (hist.track.get(&cands[i].id).copied(), i));
    }
    // Vor einem harten Zeit-Event: nur die drei Titel, die am besten in die Lücke passen.
    let free: Vec<usize> = match fit {
        Some(gap) => {
            let mut by_fit = free;
            by_fit.sort_by(|&a, &b| {
                (cands[a].seconds - gap)
                    .abs()
                    .total_cmp(&(cands[b].seconds - gap).abs())
            });
            by_fit.truncate(3);
            by_fit
        }
        None => free,
    };
    let weights: Vec<f64> = free
        .iter()
        .map(|&i| {
            let age = hist.track.get(&cands[i].id).map_or(MAX_AGE_HOURS, |l| {
                ((vt - l) as f64 / 3600.0).clamp(0.0, MAX_AGE_HOURS)
            });
            1.0 + age
        })
        .collect();
    let total: f64 = weights.iter().sum();
    let mut r = st.next_f64() * total;
    for (k, w) in weights.iter().enumerate() {
        if r < *w {
            return Some(free[k]);
        }
        r -= w;
    }
    free.last().copied()
}

/// Plant bis zu `n` weitere Titel ab `start` (Unix-Sekunden, Beginn des ersten Titels nach
/// `ahead`). `ahead` sind die schon bereitliegenden Titel in Reihenfolge; sie bleiben, wie
/// sie sind, und werden nur für Zeit und Sperrfristen mitgerechnet.
///
/// Liefert weniger als `n` Titel, wenn auch die Notfall-Kategorie leer ist; das wird als
/// Warnung geloggt, nie als Absturz. Jede Wahl steht im Log.
pub fn plan(
    lib: &Library,
    sched: &Schedule,
    st: &mut AutoDjState,
    start: i64,
    ahead: &[Ahead],
    n: usize,
) -> Result<Vec<Pick>> {
    plan_until(lib, sched, st, start, ahead, n, None)
}

/// Ein hartes Zeit-Event, auf das der AutoDJ rückwärts plant (Paket 29).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Deadline {
    /// Zeit des Events (Unix-Sekunden).
    pub at: i64,
    /// Überblendung zwischen zwei Titeln in Sekunden: jeder Titel schiebt die Uhr um seine
    /// Länge minus diese Zeit weiter (sonst läuft die Rechnung je Titel um ein paar Sekunden
    /// davon).
    pub overlap_s: i64,
}

/// Lücke vor einem harten Event, ab der der AutoDJ nach der Länge wählt (Sekunden).
pub const FIT_WINDOW_S: i64 = 480;

/// Wie [`plan`], aber mit einem harten Zeit-Event: Liegt die Zeit des nächsten Titels
/// höchstens [`FIT_WINDOW_S`] davor, wählt der AutoDJ unter den drei am besten passenden
/// Titeln, damit das Event pünktlich kommt und wenig abgeschnitten wird. Das Abschneiden
/// selbst macht der Mixer; die Planung verkleinert nur, was er abschneiden muss. Jede Wahl
/// nennt im Log die Lücke und was abgeschnitten wird.
pub fn plan_until(
    lib: &Library,
    sched: &Schedule,
    st: &mut AutoDjState,
    start: i64,
    ahead: &[Ahead],
    n: usize,
    deadline: Option<Deadline>,
) -> Result<Vec<Pick>> {
    let overlap = deadline.map_or(0, |d| d.overlap_s.max(0));
    let lock = Lockouts {
        track_s: i64::from(sched.track_lockout_min) * 60,
        artist_s: i64::from(sched.artist_lockout_min) * 60,
    };
    let mut hist = History {
        track: lib.last_plays_by_track()?,
        artist: lib.last_plays_by_artist()?,
    };
    let mut vt = start;
    for a in ahead {
        hist.mark(a.track_id, &a.artist, vt);
        vt += ((a.seconds.ceil() as i64) - overlap).max(1);
    }
    let mut cache: HashMap<String, Vec<Cand>> = HashMap::new();
    let mut out = Vec::new();
    for _ in 0..n {
        let lh = local_hour(vt, sched.zone());
        if st.hour_key != Some(lh.key) {
            // Neue Stunde, oder erster Aufruf nach dem Start: dann zählen die in dieser Stunde
            // schon gespielten Titel, damit die Folge der Slots nach einem Neustart weitergeht.
            st.count = if st.hour_key.is_none() {
                let from = hour_start(lh.key, sched.zone());
                lib.plays_between(from, vt)?
            } else {
                0
            };
            st.hour_key = Some(lh.key);
        }
        let clock = sched.clock_for(lh.weekday, lh.hour);
        let (slot_no, clock_name, wanted, fallback) = match clock {
            Some(c) => {
                let i = st.count % c.slots.len();
                st.count += 1;
                (
                    i,
                    c.name.clone(),
                    c.slots[i].category.clone(),
                    c.slots[i].fallback.clone(),
                )
            }
            None => (0, String::new(), String::new(), String::new()),
        };
        let tiers = [
            (Tier::Slot, wanted.clone()),
            (Tier::Fallback, fallback),
            (Tier::Emergency, sched.emergency.clone()),
        ];
        let mut chosen = None;
        for (tier, tag) in tiers {
            if tag.is_empty() {
                continue;
            }
            if !cache.contains_key(&tag) {
                cache.insert(tag.clone(), load_category(lib, &tag)?);
            }
            let cands = &cache[&tag];
            let gap = deadline
                .map(|d| d.at - vt)
                .filter(|g| (1..=FIT_WINDOW_S).contains(g))
                .map(|g| (g + overlap) as f64);
            if let Some(i) = choose(cands, &hist, vt, &lock, tier != Tier::Emergency, gap, st) {
                chosen = Some((tier, tag, i));
                break;
            }
        }
        let Some((tier, tag, i)) = chosen else {
            tracing::warn!(
                clock = %clock_name,
                slot = slot_no + 1,
                wanted = %wanted,
                "autodj: nothing to play, not even emergency"
            );
            break;
        };
        let c = &cache[&tag][i];
        let pick = Pick {
            track_id: c.id,
            path: c.path.clone(),
            title: c.title.clone(),
            artist: c.artist.clone(),
            seconds: c.seconds,
            category: tag,
            wanted,
            tier,
            slot: slot_no,
            clock: clock_name,
            at: vt,
        };
        tracing::info!(
            track = pick.track_id,
            tier = pick.tier.label(),
            "autodj pick: {}",
            pick.describe()
        );
        if let Some(d) = deadline
            && vt < d.at
        {
            let end = vt + pick.seconds.ceil() as i64 - overlap;
            if end > d.at {
                tracing::info!(
                    gap_s = d.at - vt,
                    cut_s = end - d.at,
                    "autodj: hard event {} s ahead, the pick runs {} s past it (the mixer cuts it)",
                    d.at - vt,
                    end - d.at
                );
            } else {
                tracing::info!(
                    gap_s = d.at - vt,
                    short_s = d.at - end,
                    "autodj: hard event {} s ahead, the pick ends {} s before it",
                    d.at - vt,
                    d.at - end
                );
            }
        }
        hist.mark(Some(pick.track_id), &pick.artist, vt);
        vt += ((pick.seconds.ceil() as i64) - overlap).max(1);
        out.push(pick);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AddOutcome, Kind, NewTrack};

    /// Ein Montag 0 Uhr (UTC).
    const MONDAY: i64 = (4 + 7 * 20_000) * 86_400;

    fn add(lib: &Library, i: usize, title: &str, secs: f64, tag: &str) -> i64 {
        let r = lib
            .add_track(&NewTrack {
                path: format!("/m/{i}.wav"),
                hash: format!("h{i}"),
                title: title.into(),
                duration: secs,
                kind: Kind::Music,
            })
            .unwrap();
        let AddOutcome::Added(id) = r else {
            panic!("{r:?}")
        };
        lib.add_tag(id, tag).unwrap();
        id
    }

    /// 500 Titel: 200 hit, 150 neu, 30 jingle, 120 notfall.
    fn fill(lib: &Library) {
        let mut n = 0;
        for i in 0..200 {
            add(
                lib,
                n,
                &format!("H{} - Song {i}", i % 40),
                170.0 + (i * 7 % 60) as f64,
                "hit",
            );
            n += 1;
        }
        for i in 0..150 {
            add(
                lib,
                n,
                &format!("N{} - Neu {i}", i % 30),
                150.0 + (i * 11 % 70) as f64,
                "neu",
            );
            n += 1;
        }
        for i in 0..30 {
            add(lib, n, &format!("Jingle {i}"), 8.0, "jingle");
            n += 1;
        }
        for i in 0..120 {
            add(
                lib,
                n,
                &format!("Z{} - Notfall {i}", i % 50),
                160.0,
                "notfall",
            );
            n += 1;
        }
        assert_eq!(lib.count().unwrap(), 500);
    }

    fn schedule(seed: u64) -> Schedule {
        let mut s = Schedule::with_example();
        s.utc_offset_min = 0;
        s.auto_dst = false;
        s.seed = seed;
        s.clocks[0].slots[0].fallback = "neu".into();
        for h in 0..WEEK_HOURS {
            s.week[h] = "Standard".into();
        }
        s.sanitize().unwrap();
        s
    }

    /// Spielt `hours` Stunden Sendezeit auf einer simulierten Uhr und trägt jede Wahl in die
    /// Historie ein.
    fn simulate(lib: &Library, s: &Schedule, from: i64, hours: i64) -> Vec<Pick> {
        let mut st = AutoDjState::new(s.seed);
        let mut vt = from;
        let mut all = Vec::new();
        while vt < from + hours * 3600 {
            let p = plan(lib, s, &mut st, vt, &[], 1).unwrap();
            let Some(p) = p.into_iter().next() else { break };
            lib.record_play(p.track_id, &p.artist, &p.category, vt)
                .unwrap();
            vt += p.seconds.ceil() as i64;
            all.push(p);
        }
        all
    }

    #[test]
    fn day_respects_lockouts_and_clock_order() {
        let lib = Library::open_in_memory().unwrap();
        fill(&lib);
        let s = schedule(7);
        let picks = simulate(&lib, &s, MONDAY, 24);
        assert!(picks.len() > 300, "{}", picks.len());
        let mut last_t: HashMap<i64, i64> = HashMap::new();
        let mut last_a: HashMap<String, i64> = HashMap::new();
        let mut hour_count: HashMap<i64, usize> = HashMap::new();
        for p in &picks {
            if let Some(l) = last_t.insert(p.track_id, p.at) {
                assert!(
                    p.at - l >= 180 * 60,
                    "Titel zu früh wieder: {}",
                    p.describe()
                );
            }
            if !p.artist.is_empty()
                && let Some(l) = last_a.insert(p.artist.clone(), p.at)
            {
                assert!(
                    p.at - l >= 60 * 60,
                    "Interpret zu früh wieder: {}",
                    p.describe()
                );
            }
            // Kategorienfolge: je Stunde die Slots der Uhr der Reihe nach.
            let key = local_hour(p.at, Zone::fixed(0)).key;
            let k = hour_count.entry(key).or_insert(0);
            assert_eq!(p.slot, *k % 4, "{}", p.describe());
            assert_eq!(p.wanted, ["hit", "hit", "neu", "jingle"][p.slot]);
            assert_eq!(p.tier, Tier::Slot, "{}", p.describe());
            assert_eq!(p.category, p.wanted);
            assert!(!p.describe().is_empty());
            *k += 1;
        }
        assert_eq!(hour_count.len(), 24);
    }

    #[test]
    fn planning_backward_to_a_hard_event_keeps_the_cut_small() {
        // Für viele Startwerte: Mit Frist ragt der Titel über die Zeit hinaus höchstens
        // um das, was ein Titel sinnvoll abweichen kann; ohne Frist ist das Ergebnis Zufall.
        let mut worst_with = 0;
        let mut worst_without = 0;
        for seed in 0..40u64 {
            let lib = Library::open_in_memory().unwrap();
            fill(&lib);
            let s = schedule(seed);
            let d = Deadline {
                at: MONDAY + 600,
                overlap_s: 3,
            };
            for (deadline, worst) in [(Some(d), &mut worst_with), (None, &mut worst_without)] {
                let mut st = AutoDjState::new(seed);
                let picks = plan_until(&lib, &s, &mut st, MONDAY, &[], 5, deadline).unwrap();
                let mut vt = MONDAY;
                let mut over = 0;
                for p in &picks {
                    let end = vt + p.seconds.ceil() as i64 - 3;
                    if vt < d.at && end >= d.at {
                        over = end - d.at;
                        break;
                    }
                    vt = end;
                }
                *worst = (*worst).max(over);
            }
        }
        // Titel dauern 150-230 s: ohne Planung ragt der Titel bis fast eine ganze Länge über.
        assert!(worst_with <= 100, "with planning: {worst_with} s");
        assert!(
            worst_with < worst_without,
            "{worst_with} vs {worst_without}"
        );
    }

    #[test]
    fn same_seed_same_day_other_seed_other_day() {
        let ids = |seed| {
            let lib = Library::open_in_memory().unwrap();
            fill(&lib);
            simulate(&lib, &schedule(seed), MONDAY, 6)
                .iter()
                .map(|p| p.track_id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(42), ids(42));
        assert_ne!(ids(42), ids(43));
    }

    #[test]
    fn empty_category_falls_back_then_emergency() {
        let lib = Library::open_in_memory().unwrap();
        fill(&lib);
        let mut s = schedule(1);
        s.clocks[0].slots = vec![
            Slot {
                category: "werbung".into(),
                fallback: "hit".into(),
            },
            Slot {
                category: "werbung".into(),
                fallback: "auch-leer".into(),
            },
        ];
        let picks = simulate(&lib, &s, MONDAY, 2);
        assert!(picks.len() > 10);
        for p in &picks {
            match p.slot {
                0 => assert_eq!((p.tier, p.category.as_str()), (Tier::Fallback, "hit")),
                _ => assert_eq!((p.tier, p.category.as_str()), (Tier::Emergency, "notfall")),
            }
        }
    }

    #[test]
    fn nothing_at_all_returns_nothing_without_panic() {
        let lib = Library::open_in_memory().unwrap();
        let s = schedule(1);
        let mut st = AutoDjState::new(1);
        assert!(plan(&lib, &s, &mut st, MONDAY, &[], 3).unwrap().is_empty());
    }

    #[test]
    fn emergency_ignores_lockout_when_everything_is_locked() {
        let lib = Library::open_in_memory().unwrap();
        let a = add(&lib, 0, "Solo - Eins", 100.0, "notfall");
        let mut s = schedule(1);
        s.clocks[0].slots = vec![Slot::new("leer")];
        let mut st = AutoDjState::new(1);
        lib.record_play(a, "solo", "notfall", MONDAY - 10).unwrap();
        let p = plan(&lib, &s, &mut st, MONDAY, &[], 1).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].tier, Tier::Emergency);
    }

    #[test]
    fn history_survives_restart() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("lib.sqlite3");
        let s = schedule(5);
        let (played, end) = {
            let lib = Library::open(&db).unwrap();
            fill(&lib);
            let picks = simulate(&lib, &s, MONDAY, 2);
            let end = picks
                .last()
                .map_or(MONDAY, |p| p.at + p.seconds.ceil() as i64);
            (picks, end)
        };
        let lib = Library::open(&db).unwrap();
        let by_track = lib.last_plays_by_track().unwrap();
        assert_eq!(by_track.len(), played.len());
        assert_eq!(lib.plays_since(MONDAY).unwrap().len(), played.len());
        // Nach dem Neustart wählt der Planer keinen Titel, der noch gesperrt ist.
        let mut st = AutoDjState::new(99);
        let next = plan(&lib, &s, &mut st, end, &[], 20).unwrap();
        for p in &next {
            assert!(
                by_track
                    .get(&p.track_id)
                    .is_none_or(|l| p.at - l >= 180 * 60),
                "{}",
                p.describe()
            );
        }
        // Die Slot-Folge der laufenden Stunde geht weiter (Zähler aus der Historie).
        let in_hour = played
            .iter()
            .filter(|p| local_hour(p.at, Zone::fixed(0)).key == local_hour(end, Zone::fixed(0)).key)
            .count();
        assert_eq!(next[0].slot, in_hour % 4);
    }

    #[test]
    fn hand_placed_tracks_are_kept_out_and_take_time() {
        let lib = Library::open_in_memory().unwrap();
        fill(&lib);
        let mut s = schedule(3);
        s.clocks[0].slots = vec![Slot::new("hit")];
        let hand = lib.search(&Query {
            tag: Some("hit".into()),
            limit: 500,
            ..Query::default()
        });
        let hand = hand.unwrap();
        let ahead: Vec<Ahead> = hand
            .iter()
            .take(100)
            .map(|t| Ahead {
                track_id: Some(t.id),
                artist: artist_of(&t.title),
                seconds: t.duration,
            })
            .collect();
        let mut st = AutoDjState::new(3);
        let picks = plan(&lib, &s, &mut st, MONDAY, &ahead, 5).unwrap();
        assert_eq!(picks.len(), 5);
        for p in &picks {
            assert!(!hand.iter().take(100).any(|t| t.id == p.track_id));
            assert!(p.at > MONDAY + 100 * 170);
        }
    }

    #[test]
    fn local_hour_and_week() {
        let l = local_hour(MONDAY + 3 * 3600 + 5, Zone::fixed(0));
        assert_eq!((l.weekday, l.hour), (0, 3));
        let l = local_hour(MONDAY + 23 * 3600, Zone::fixed(60));
        assert_eq!((l.weekday, l.hour), (1, 0));
        let l = local_hour(MONDAY - 1, Zone::fixed(0));
        assert_eq!((l.weekday, l.hour), (6, 23));
        assert_eq!(
            hour_start(local_hour(MONDAY, Zone::fixed(60)).key, Zone::fixed(60)),
            MONDAY
        );
    }

    #[test]
    fn schedule_file_roundtrip_and_hostile_input() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join(FILE);
        assert_eq!(Schedule::load(&f).unwrap().clocks.len(), 1);
        let mut s = schedule(9);
        s.set_hour(2, 14, Some("Standard")).unwrap();
        s.autodj = true;
        s.save(&f).unwrap();
        let back = Schedule::load(&f).unwrap();
        assert_eq!(back, s);
        assert!(back.clock_for(2, 14).is_some());
        assert!(s.set_hour(7, 0, None).is_err());
        assert!(s.set_hour(0, 0, Some("gibtsnicht")).is_err());
        std::fs::write(&f, "{kaputt").unwrap();
        assert!(Schedule::load(&f).is_err());
        std::fs::write(&f, r#"{"version":9,"clocks":[]}"#).unwrap();
        assert!(Schedule::load(&f).is_err());
        // Unbekannte Uhr in der Woche und falsche Länge werden bereinigt.
        std::fs::write(
            &f,
            r#"{"clocks":[{"name":"A","slots":[{"category":" HIT "}]}],"week":["A","B"],"lookahead":999}"#,
        )
        .unwrap();
        let l = Schedule::load(&f).unwrap();
        assert_eq!(l.week.len(), WEEK_HOURS);
        assert_eq!(l.week[0], "A");
        assert_eq!(l.week[1], "");
        assert_eq!(l.clocks[0].slots[0].category, "hit");
        assert_eq!(l.lookahead, MAX_LOOKAHEAD);
    }

    #[test]
    fn artist_guess() {
        assert_eq!(artist_of("  The  Band - Song - Live"), "the band");
        assert_eq!(artist_of("Nur ein Titel"), "");
    }
}
