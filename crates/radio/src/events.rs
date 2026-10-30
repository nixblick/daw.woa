//! Feste Zeit-Events (Paket 29): Nachrichten, Werbung, Stundenjingle zu einer Uhrzeit oder nach
//! einem Muster, hart oder weich.
//!
//! - **Hart:** Das Event beginnt zur genauen Zeit (±2 s). Läuft gerade ein Titel, wird er unter
//!   dem Event ausgeblendet. Vorher plant der AutoDJ rückwärts, damit möglichst wenig
//!   abgeschnitten wird.
//! - **Weich:** Das Event kommt direkt nach dem laufenden Titel.
//!
//! Dieser Teil ist reine Rechnung und Zustandsmaschine ohne Ton und ohne Uhr: [`EventClock::tick`]
//! bekommt die Zeit als Zahl und liefert Aktionen, die der Controller ausführt. Dieselbe
//! Maschine treibt die Wochen-Simulation in den Tests.
//!
//! Das Prüfen im Takt mit Merken der erledigten Auslösung (damit nichts doppelt kommt) ist die
//! Idee von `cron.liq` in Liquidsoap; dort prüft ein Takt von 20 s mit Minutenmuster. Hier ist
//! der Takt der des Control-Threads, die Genauigkeit eine Sekunde, und eine verspätete oder
//! verpasste Auslösung wird gemeldet. Code ist nicht übernommen (`docs/herkunft.md`).
//!
//! Ortszeit und Zeitumstellung: [`woa_engine::localtime`], dieselbe Rechnung wie in der
//! Stundenuhr. Ein Muster mit festen Stunden („8, 12, 17“) gilt als Wanduhrzeit: im Herbst zählt
//! der erste Durchlauf der doppelten Stunde (einmal), im Frühjahr kommt ein Event in der
//! übersprungenen Stunde im Moment der Umstellung. Ein Muster mit „jede Stunde“ läuft dagegen
//! in jeder wirklichen Stunde, also im Herbst zweimal um 02:00.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use woa_engine::localtime::Zone;

/// Formatversion von `events.json`.
pub const VERSION: u32 = 1;
/// Name der Datei im Datenordner.
pub const FILE: &str = "events.json";
pub const MAX_EVENTS: usize = 200;
pub const MAX_NAME_CHARS: usize = 60;
/// So viele Sekunden vor der Zeit wird die Datei geladen.
pub const LEAD_SECONDS: f64 = 30.0;
/// So viele Sekunden vor einem harten Event bekommt der Mixer den Startbefehl (auf den Frame
/// genau, die Uhr des Mixers zählt).
pub const ARM_SECONDS: f64 = 1.0;
/// Ein hartes Event, das später als so viele Sekunden dran ist, wird nicht mehr gespielt.
pub const HARD_MAX_LATE: f64 = 20.0;
/// Ein weiches Event, das später als so viele Sekunden dran ist, wird nicht mehr gespielt.
pub const SOFT_MAX_LATE: f64 = 1800.0;
const ALL_HOURS: u32 = (1 << 24) - 1;
const ALL_DAYS: u8 = 0x7f;

/// Art des Events (nur zur Anzeige und zum Sortieren).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    News,
    Ad,
    #[default]
    Jingle,
    Other,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::News, Kind::Ad, Kind::Jingle, Kind::Other];

    pub fn label(self) -> &'static str {
        match self {
            Kind::News => "Nachrichten",
            Kind::Ad => "Werbung",
            Kind::Jingle => "Jingle",
            Kind::Other => "Sonstiges",
        }
    }
}

/// Hart oder weich.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Timing {
    #[default]
    Hard,
    Soft,
}

impl Timing {
    pub fn label(self) -> &'static str {
        match self {
            Timing::Hard => "hart",
            Timing::Soft => "weich",
        }
    }
}

/// Wann ein Event kommt: Wochentage, Stunden, Minute und Sekunde.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pattern {
    /// Bit 0 = Montag … Bit 6 = Sonntag.
    pub weekdays: u8,
    /// Bit 0 = 0 Uhr … Bit 23 = 23 Uhr.
    pub hours: u32,
    pub minute: u8,
    pub second: u8,
}

impl Pattern {
    /// Jeden Tag zu den Stunden von `hours` (Maske) zur Minute `minute`.
    pub fn daily(hours: u32, minute: u8) -> Self {
        Self {
            weekdays: ALL_DAYS,
            hours: hours & ALL_HOURS,
            minute,
            second: 0,
        }
    }

    /// Jede Stunde zur Minute `minute`.
    pub fn hourly(minute: u8) -> Self {
        Self::daily(ALL_HOURS, minute)
    }

    /// Gilt jede Stunde?
    pub fn is_hourly(&self) -> bool {
        self.hours & ALL_HOURS == ALL_HOURS
    }

    /// Erster Zeitpunkt ab `from` (Unix-Sekunden, einschließlich), `None`, wenn nie.
    pub fn next_at_or_after(&self, zone: Zone, from: i64) -> Option<i64> {
        if self.hours & ALL_HOURS == 0 || self.weekdays & ALL_DAYS == 0 {
            return None;
        }
        let in_hour = i64::from(self.minute.min(59)) * 60 + i64::from(self.second.min(59));
        if self.is_hourly() {
            // Jede wirkliche Stunde, auch die doppelte im Herbst.
            let mut h = from.div_euclid(3600) - 1;
            for _ in 0..(8 * 24 + 4) {
                let hs = h * 3600;
                h += 1;
                let off = i64::from(zone.offset_min(hs)) * 60;
                let local_in_hour = (hs + off).rem_euclid(3600);
                let cand = hs + (in_hour - local_in_hour).rem_euclid(3600);
                if cand < from {
                    continue;
                }
                if self.weekdays & (1 << zone.local(cand).weekday) != 0 {
                    return Some(cand);
                }
            }
            return None;
        }
        // Feste Wanduhrzeiten, der Reihe nach über Ortstage.
        let first_day = zone.local(from).day - 1;
        let mut best: Option<i64> = None;
        for day in first_day..first_day + 9 {
            let weekday = (day + 3).rem_euclid(7);
            if self.weekdays & (1 << weekday) == 0 {
                continue;
            }
            for hour in 0..24i64 {
                if self.hours & (1 << hour) == 0 {
                    continue;
                }
                let sod = hour * 3600 + in_hour;
                let cand = zone.to_unix(day, sod).or_else(|| zone.gap_end(day, sod));
                if let Some(c) = cand
                    && c >= from
                    && best.is_none_or(|b| c < b)
                {
                    best = Some(c);
                }
            }
            if best.is_some() {
                // Spätere Tage können nur später sein.
                break;
            }
        }
        best
    }
}

const DAY_NAMES: [&str; 7] = ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"];

/// Stunden als Text: `*` (jede), sonst Liste und Bereiche wie `7,8,12-14`.
pub fn format_hours(mask: u32) -> String {
    let mask = mask & ALL_HOURS;
    if mask == ALL_HOURS {
        return "*".into();
    }
    let mut parts = Vec::new();
    let mut h = 0;
    while h < 24 {
        if mask & (1 << h) == 0 {
            h += 1;
            continue;
        }
        let start = h;
        while h + 1 < 24 && mask & (1 << (h + 1)) != 0 {
            h += 1;
        }
        parts.push(if start == h {
            start.to_string()
        } else {
            format!("{start}-{h}")
        });
        h += 1;
    }
    parts.join(",")
}

/// Gegenstück zu [`format_hours`]. Leer ist ein Fehler (das Event würde nie kommen).
pub fn parse_hours(text: &str) -> Result<u32, String> {
    let t = text.trim();
    if t == "*" {
        return Ok(ALL_HOURS);
    }
    let mut mask = 0u32;
    for part in t.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (a.trim(), b.trim()),
            None => (part, part),
        };
        let (a, b): (u32, u32) = (
            a.parse()
                .map_err(|_| format!("„{part}“ ist keine Stunde"))?,
            b.parse()
                .map_err(|_| format!("„{part}“ ist keine Stunde"))?,
        );
        if a > 23 || b > 23 || a > b {
            return Err(format!("„{part}“: Stunden gehen von 0 bis 23, aufsteigend"));
        }
        for h in a..=b {
            mask |= 1 << h;
        }
    }
    if mask == 0 {
        return Err("keine Stunde angegeben (`*` für jede)".into());
    }
    Ok(mask)
}

/// Wochentage als Text: `*` oder Liste und Bereiche wie `Mo-Fr,So`.
pub fn format_weekdays(mask: u8) -> String {
    let mask = mask & ALL_DAYS;
    if mask == ALL_DAYS {
        return "*".into();
    }
    let mut parts = Vec::new();
    let mut d = 0;
    while d < 7 {
        if mask & (1 << d) == 0 {
            d += 1;
            continue;
        }
        let start = d;
        while d + 1 < 7 && mask & (1 << (d + 1)) != 0 {
            d += 1;
        }
        parts.push(if start == d {
            DAY_NAMES[start].to_string()
        } else {
            format!("{}-{}", DAY_NAMES[start], DAY_NAMES[d])
        });
        d += 1;
    }
    parts.join(",")
}

/// Gegenstück zu [`format_weekdays`].
pub fn parse_weekdays(text: &str) -> Result<u8, String> {
    let t = text.trim();
    if t == "*" {
        return Ok(ALL_DAYS);
    }
    let day = |s: &str| -> Result<usize, String> {
        DAY_NAMES
            .iter()
            .position(|n| n.eq_ignore_ascii_case(s.trim()))
            .ok_or_else(|| format!("„{s}“ ist kein Wochentag (Mo, Di, Mi, Do, Fr, Sa, So)"))
    };
    let mut mask = 0u8;
    for part in t.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (day(a)?, day(b)?),
            None => (day(part)?, day(part)?),
        };
        if a > b {
            return Err(format!("„{part}“: Bereich nur aufsteigend (Mo-So)"));
        }
        for d in a..=b {
            mask |= 1 << d;
        }
    }
    if mask == 0 {
        return Err("kein Wochentag angegeben (`*` für alle)".into());
    }
    Ok(mask)
}

#[derive(Serialize, Deserialize)]
struct PatternFile {
    #[serde(default = "star")]
    weekdays: String,
    #[serde(default = "star")]
    hours: String,
    #[serde(default)]
    minute: u8,
    #[serde(default)]
    second: u8,
}

fn star() -> String {
    "*".into()
}

impl Serialize for Pattern {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        PatternFile {
            weekdays: format_weekdays(self.weekdays),
            hours: format_hours(self.hours),
            minute: self.minute,
            second: self.second,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for Pattern {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let f = PatternFile::deserialize(d)?;
        let err = serde::de::Error::custom;
        Ok(Pattern {
            weekdays: parse_weekdays(&f.weekdays).map_err(err)?,
            hours: parse_hours(&f.hours).map_err(err)?,
            minute: f.minute,
            second: f.second,
        })
    }
}

/// Ein Event der Tabelle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimeEvent {
    /// Feste Kennung (bleibt beim Umsortieren).
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub kind: Kind,
    /// Audiodatei, die gespielt wird.
    pub path: PathBuf,
    pub pattern: Pattern,
    #[serde(default)]
    pub timing: Timing,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

fn default_offset() -> i32 {
    60
}

impl TimeEvent {
    pub fn is_hard(&self) -> bool {
        self.timing == Timing::Hard
    }
}

/// Die Event-Tabelle, gespeichert als `events.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventTable {
    #[serde(default = "version")]
    pub version: u32,
    /// Ortszeit gegenüber UTC im Winter in Minuten.
    #[serde(default = "default_offset")]
    pub utc_offset_min: i32,
    /// EU-Sommerzeit automatisch.
    #[serde(default = "yes")]
    pub auto_dst: bool,
    #[serde(default)]
    pub events: Vec<TimeEvent>,
}

fn version() -> u32 {
    VERSION
}

impl Default for EventTable {
    fn default() -> Self {
        Self {
            version: VERSION,
            utc_offset_min: default_offset(),
            auto_dst: true,
            events: Vec::new(),
        }
    }
}

impl EventTable {
    pub fn zone(&self) -> Zone {
        Zone {
            base_min: self.utc_offset_min,
            eu_dst: self.auto_dst,
        }
    }

    /// Prüft Grenzen und bringt die Tabelle in Ordnung. Gibt die Zahl der geänderten Werte
    /// zurück; ungültige Events (leerer Name oder Pfad) sind ein Fehler.
    pub fn sanitize(&mut self) -> Result<usize, String> {
        if self.version == 0 || self.version > VERSION {
            return Err(format!(
                "events.json Version {} wird nicht unterstützt (kann {VERSION})",
                self.version
            ));
        }
        self.version = VERSION;
        if self.events.len() > MAX_EVENTS {
            return Err(format!("höchstens {MAX_EVENTS} Events"));
        }
        let mut changed = 0;
        let off = self.utc_offset_min.clamp(-14 * 60, 14 * 60);
        if off != self.utc_offset_min {
            self.utc_offset_min = off;
            changed += 1;
        }
        let mut seen = HashMap::new();
        let mut next_id = self.events.iter().map(|e| e.id).max().unwrap_or(0) + 1;
        for e in &mut self.events {
            if e.name.trim().is_empty()
                || e.name.chars().count() > MAX_NAME_CHARS
                || e.name.chars().any(char::is_control)
            {
                return Err(format!(
                    "Event {}: Name leer, länger als {MAX_NAME_CHARS} Zeichen oder mit Steuerzeichen",
                    e.id
                ));
            }
            if e.path.as_os_str().is_empty() {
                return Err(format!("Event „{}“: keine Datei", e.name));
            }
            if seen.insert(e.id, ()).is_some() {
                e.id = next_id;
                next_id += 1;
                changed += 1;
            }
            let (m, s) = (e.pattern.minute.min(59), e.pattern.second.min(59));
            if (m, s) != (e.pattern.minute, e.pattern.second) {
                e.pattern.minute = m;
                e.pattern.second = s;
                changed += 1;
            }
            e.pattern.hours &= ALL_HOURS;
            e.pattern.weekdays &= ALL_DAYS;
        }
        Ok(changed)
    }

    /// Liest `path`. Fehlt die Datei, gibt es eine leere Tabelle.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let mut t: Self =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        t.sanitize()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(t)
    }

    /// Schreibt atomar (Temp-Datei im selben Ordner, dann `rename`). Ein Symlink als Ziel wird
    /// abgelehnt.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        use std::io::Write;
        if let Ok(m) = std::fs::symlink_metadata(path)
            && (m.file_type().is_symlink() || !m.is_file())
        {
            return Err(format!(
                "{}: Ziel ist ein Symlink oder keine Datei",
                path.display()
            ));
        }
        let dir = match path.parent() {
            Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
            _ => PathBuf::from("."),
        };
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut t = self.clone();
        t.sanitize()?;
        let text = serde_json::to_string_pretty(&t).map_err(|e| e.to_string())?;
        let mut opened = None;
        for n in 0..100u32 {
            let p = dir.join(format!(".events-{}-{n}.json", std::process::id()));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&p)
            {
                Ok(f) => {
                    opened = Some((p, f));
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("{}: {e}", p.display())),
            }
        }
        let (tmp, mut file) = opened.ok_or("keine Temp-Datei frei")?;
        let r = file
            .write_all(text.as_bytes())
            .and_then(|()| file.sync_all())
            .and_then(|()| std::fs::rename(&tmp, path))
            .map_err(|e| format!("{}: {e}", path.display()));
        drop(file);
        if r.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        r
    }

    /// Das Event mit Kennung `id`.
    pub fn get(&self, id: u32) -> Option<&TimeEvent> {
        self.events.iter().find(|e| e.id == id)
    }

    /// Nächste Auslösung von Event `e` ab `from`.
    pub fn next_of(&self, e: &TimeEvent, from: i64) -> Option<i64> {
        e.pattern.next_at_or_after(self.zone(), from)
    }

    /// Nächstes hartes Event ab `from` (Zeit, Name) – für die Rückwärtsplanung des AutoDJ.
    pub fn next_hard(&self, from: i64) -> Option<(i64, &str)> {
        self.events
            .iter()
            .filter(|e| e.enabled && e.is_hard())
            .filter_map(|e| self.next_of(e, from).map(|t| (t, e.name.as_str())))
            .min_by_key(|(t, _)| *t)
    }
}

/// Eine anstehende Auslösung.
#[derive(Debug, Clone, PartialEq)]
pub struct Upcoming {
    pub id: u32,
    pub at: i64,
    pub hard: bool,
    predecoded: bool,
}

/// Warum ein Event nicht gespielt wurde.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MissedWhy {
    /// Die Sendung lief nicht.
    NotOnAir,
    /// Es war schon mehr als die erlaubte Zeit vorbei (Sekunden).
    TooLate(f64),
}

/// Was der Controller tun soll.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Datei laden und bereithalten.
    Predecode {
        id: u32,
        at: i64,
        hard: bool,
    },
    /// Auslösen. Hart: der Mixer soll in `in_secs` beginnen (Sekunden ab jetzt, nie negativ).
    /// Weich: sofort hinter den laufenden Titel stellen.
    Fire {
        id: u32,
        at: i64,
        hard: bool,
        in_secs: f64,
    },
    Missed {
        id: u32,
        at: i64,
        why: MissedWhy,
    },
}

/// Die Zustandsmaschine hinter den Zeit-Events.
#[derive(Debug, Clone, Default)]
pub struct EventClock {
    table: EventTable,
    slots: Vec<Upcoming>,
    /// Zuletzt erledigte Auslösung je Event.
    done: HashMap<u32, i64>,
}

impl EventClock {
    pub fn new(table: EventTable) -> Self {
        Self {
            table,
            slots: Vec::new(),
            done: HashMap::new(),
        }
    }

    pub fn table(&self) -> &EventTable {
        &self.table
    }

    /// Neue Tabelle; die Planung beginnt von vorn.
    pub fn set_table(&mut self, table: EventTable) {
        self.table = table;
        self.slots.clear();
        self.done.clear();
    }

    /// Kennungen der Events, die noch auf ihre Auslösung warten.
    pub fn waiting_ids(&self) -> Vec<u32> {
        self.slots.iter().map(|s| s.id).collect()
    }

    /// Die nächste Auslösung (frühestens).
    pub fn upcoming(&self) -> Option<&Upcoming> {
        self.slots.iter().min_by_key(|s| s.at)
    }

    /// Ein Takt: `now` in Unix-Sekunden, `on_air`: die Sendung läuft und es spielt ein Titel.
    pub fn tick(&mut self, now: f64, on_air: bool) -> Vec<Action> {
        // Fehlende Plätze anlegen: von der letzten erledigten Auslösung an, sonst ab jetzt.
        let have: Vec<u32> = self.slots.iter().map(|s| s.id).collect();
        for e in self.table.events.iter().filter(|e| e.enabled) {
            if have.contains(&e.id) {
                continue;
            }
            let from = self.done.get(&e.id).map_or(now.floor() as i64, |d| d + 1);
            if let Some(at) = self.table.next_of(e, from) {
                self.slots.push(Upcoming {
                    id: e.id,
                    at,
                    hard: e.is_hard(),
                    predecoded: false,
                });
            }
        }
        // Gelöschte oder abgeschaltete Events fallen weg.
        let table = &self.table;
        self.slots
            .retain(|s| table.get(s.id).is_some_and(|e| e.enabled));

        let mut out = Vec::new();
        let mut finished = Vec::new();
        for s in &mut self.slots {
            let dt = s.at as f64 - now;
            let max_late = if s.hard { HARD_MAX_LATE } else { SOFT_MAX_LATE };
            if !s.predecoded && dt <= LEAD_SECONDS && dt >= -max_late && on_air {
                s.predecoded = true;
                out.push(Action::Predecode {
                    id: s.id,
                    at: s.at,
                    hard: s.hard,
                });
            }
            let due = if s.hard { dt <= ARM_SECONDS } else { dt <= 0.0 };
            if !due {
                continue;
            }
            finished.push(s.id);
            if -dt > max_late {
                out.push(Action::Missed {
                    id: s.id,
                    at: s.at,
                    why: MissedWhy::TooLate(-dt),
                });
            } else if !on_air {
                out.push(Action::Missed {
                    id: s.id,
                    at: s.at,
                    why: MissedWhy::NotOnAir,
                });
            } else {
                if !s.predecoded {
                    // Spät bemerkt (etwa gerade erst gestartet): erst laden, dann auslösen.
                    s.predecoded = true;
                    out.push(Action::Predecode {
                        id: s.id,
                        at: s.at,
                        hard: s.hard,
                    });
                }
                out.push(Action::Fire {
                    id: s.id,
                    at: s.at,
                    hard: s.hard,
                    in_secs: dt.max(0.0),
                });
            }
        }
        for id in finished {
            if let Some(i) = self.slots.iter().position(|s| s.id == id) {
                let s = self.slots.remove(i);
                self.done.insert(s.id, s.at);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CET: Zone = Zone::CENTRAL_EUROPE;

    fn event(id: u32, pattern: Pattern, timing: Timing) -> TimeEvent {
        TimeEvent {
            id,
            name: format!("Event {id}"),
            kind: Kind::Jingle,
            path: PathBuf::from("/x.wav"),
            pattern,
            timing,
            enabled: true,
        }
    }

    #[test]
    fn hours_and_weekdays_text_roundtrip() {
        assert_eq!(parse_hours("*"), Ok(ALL_HOURS));
        assert_eq!(parse_hours("7, 8,12-14"), Ok(0b111_0001_1000_0000));
        assert_eq!(format_hours(0b111_0001_1000_0000), "7-8,12-14");
        assert_eq!(parse_hours(&format_hours(0b1010_0101)), Ok(0b1010_0101));
        assert!(parse_hours("").is_err());
        assert!(parse_hours("24").is_err());
        assert!(parse_hours("5-3").is_err());
        assert!(parse_hours("x").is_err());
        assert_eq!(parse_weekdays("Mo-Fr,So"), Ok(0b101_1111));
        assert_eq!(format_weekdays(0b101_1111), "Mo-Fr,So");
        assert_eq!(parse_weekdays("*"), Ok(0x7f));
        assert!(parse_weekdays("Xx").is_err());
        assert!(parse_weekdays("Fr-Mo").is_err());
    }

    #[test]
    fn hourly_pattern_fires_every_real_hour() {
        let p = Pattern::hourly(0);
        // 2026-10-30 00:00 UTC = 01:00 Ortszeit; die nächste volle Stunde ist 01:00 UTC.
        let from = 1_793_318_400;
        assert_eq!(p.next_at_or_after(CET, from), Some(from));
        assert_eq!(p.next_at_or_after(CET, from + 1), Some(from + 3600));
        // Minute 30, Sekunde 15.
        let p = Pattern {
            minute: 30,
            second: 15,
            ..Pattern::hourly(0)
        };
        assert_eq!(p.next_at_or_after(CET, from), Some(from + 1815));
    }

    #[test]
    fn fixed_hours_midnight_and_weekdays() {
        // Montag bis Freitag 0 Uhr: von Freitag 12 Uhr aus kommt Montag 0 Uhr.
        let p = Pattern {
            weekdays: 0b001_1111,
            hours: 1,
            minute: 0,
            second: 0,
        };
        let z = Zone::fixed(0);
        // 2026-10-30 ist ein Freitag; 12:00 UTC.
        let fri_noon = 1_793_318_400 + 12 * 3600;
        let sat_midnight = 1_793_318_400 + 86_400;
        let mon_midnight = 1_793_318_400 + 3 * 86_400;
        assert_eq!(z.local(mon_midnight).weekday, 0);
        // Samstag 0 Uhr zählt nicht (kein Samstag), Montag 0 Uhr schon.
        assert_eq!(p.next_at_or_after(z, fri_noon), Some(mon_midnight));
        assert_ne!(p.next_at_or_after(z, fri_noon), Some(sat_midnight));
        // Genau auf der Zeit: einschließlich.
        assert_eq!(p.next_at_or_after(z, mon_midnight), Some(mon_midnight));
        assert_eq!(
            p.next_at_or_after(z, mon_midnight + 1),
            Some(mon_midnight + 86_400)
        );
    }

    #[test]
    fn no_hours_or_days_never_fires() {
        let p = Pattern {
            weekdays: 0,
            hours: ALL_HOURS,
            minute: 0,
            second: 0,
        };
        assert_eq!(p.next_at_or_after(CET, 0), None);
    }

    #[test]
    fn dst_spring_fixed_time_in_the_gap_fires_at_the_switch() {
        // 2026-03-29 (Sonntag): 02:30 gibt es nicht. Das Event kommt im Moment der Umstellung
        // (03:00 Sommerzeit = 01:00 UTC).
        let p = Pattern::daily(1 << 2, 30);
        let before = 1_774_746_000 - 6 * 3600;
        assert_eq!(p.next_at_or_after(CET, before), Some(1_774_746_000));
        // Ein Tag später wieder 02:30 Sommerzeit = 00:30 UTC.
        assert_eq!(
            p.next_at_or_after(CET, 1_774_746_001),
            Some(1_774_746_000 + 86_400 - 1800)
        );
    }

    #[test]
    fn dst_autumn_fixed_time_fires_once_hourly_twice() {
        // 2026-10-25: Sommerzeit endet 01:00 UTC (03:00 -> 02:00). 02:30 gibt es zweimal.
        let end = 1_792_890_000;
        let fixed = Pattern::daily(1 << 2, 30);
        let first = fixed.next_at_or_after(CET, end - 6 * 3600).unwrap();
        // Erster Durchlauf: 02:30 Sommerzeit = 00:30 UTC.
        assert_eq!(first, end - 1800);
        // Danach nicht noch einmal in dieser Nacht: nächstes Mal am Folgetag (02:30 Winterzeit).
        let second = fixed.next_at_or_after(CET, first + 1).unwrap();
        assert_eq!(second, end + 86_400 + 1800);
        // „Jede Stunde“: zweimal 02:00 Ortszeit (00:00 und 01:00 UTC).
        let hourly = Pattern::hourly(0);
        let a = hourly.next_at_or_after(CET, end - 3600).unwrap();
        let b = hourly.next_at_or_after(CET, a + 1).unwrap();
        assert_eq!((a, b), (end - 3600, end));
        assert_eq!(CET.local(a).hour, 2);
        assert_eq!(CET.local(b).hour, 2);
    }

    #[test]
    fn table_roundtrip_and_hostile_files() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE);
        assert_eq!(EventTable::load(&file).unwrap(), EventTable::default());
        let mut t = EventTable::default();
        t.events.push(event(1, Pattern::hourly(0), Timing::Hard));
        t.events.push(event(
            2,
            Pattern {
                weekdays: 0b001_1111,
                hours: (1 << 7) | (1 << 17),
                minute: 55,
                second: 30,
            },
            Timing::Soft,
        ));
        t.save(&file).unwrap();
        assert_eq!(EventTable::load(&file).unwrap(), t);
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            text.contains("\"Mo-Fr\"") && text.contains("\"7,17\""),
            "{text}"
        );
        // Kaputte Dateien werden gemeldet und nicht überschrieben.
        for bad in [
            "{",
            r#"{"version": 9}"#,
            r#"{"events":[{"id":1,"name":"","path":"/x","pattern":{}}]}"#,
            r#"{"events":[{"id":1,"name":"a","path":"","pattern":{}}]}"#,
            r#"{"events":[{"id":1,"name":"a","path":"/x","pattern":{"hours":"99"}}]}"#,
        ] {
            std::fs::write(&file, bad).unwrap();
            assert!(EventTable::load(&file).is_err(), "{bad}");
            assert_eq!(std::fs::read_to_string(&file).unwrap(), bad);
        }
        // Doppelte Kennungen und Minute 99 werden bereinigt.
        let mut t = EventTable::default();
        t.events.push(event(1, Pattern::hourly(99), Timing::Hard));
        t.events.push(event(1, Pattern::hourly(0), Timing::Hard));
        assert_eq!(t.sanitize(), Ok(2));
        assert_ne!(t.events[0].id, t.events[1].id);
        assert_eq!(t.events[0].pattern.minute, 59);
        // Symlink als Ziel.
        #[cfg(unix)]
        {
            let link = dir.path().join("l.json");
            std::os::unix::fs::symlink(&file, &link).unwrap();
            assert!(EventTable::default().save(&link).is_err());
        }
    }

    #[test]
    fn next_hard_ignores_soft_and_disabled() {
        let mut t = EventTable {
            utc_offset_min: 0,
            auto_dst: false,
            ..EventTable::default()
        };
        t.events.push(event(1, Pattern::hourly(0), Timing::Soft));
        let mut off = event(2, Pattern::hourly(10), Timing::Hard);
        off.enabled = false;
        t.events.push(off);
        t.events.push(event(3, Pattern::hourly(30), Timing::Hard));
        let from = 1_793_318_400 + 5;
        let (at, name) = t.next_hard(from).unwrap();
        assert_eq!(at, 1_793_318_400 + 1800);
        assert_eq!(name, "Event 3");
    }

    fn clock(events: Vec<TimeEvent>) -> EventClock {
        let t = EventTable {
            utc_offset_min: 0,
            auto_dst: false,
            events,
            ..EventTable::default()
        };
        EventClock::new(t)
    }

    #[test]
    fn state_machine_hard_event_predecodes_then_arms_once() {
        let base = 1_793_318_400; // 00:00 UTC
        let mut c = clock(vec![event(1, Pattern::hourly(0), Timing::Hard)]);
        // 10 s vor 01:00: noch nichts, die nächste ist 00:00 selbst? Wir starten bei 00:00:05.
        let mut now = base as f64 + 5.0;
        assert!(c.tick(now, true).is_empty());
        // Bei 00:59:29 ist es noch zu früh, bei 00:59:30 kommt das Laden.
        now = (base + 3600 - 31) as f64;
        assert!(c.tick(now, true).is_empty());
        now = (base + 3600 - 30) as f64;
        assert_eq!(
            c.tick(now, true),
            vec![Action::Predecode {
                id: 1,
                at: base + 3600,
                hard: true
            }]
        );
        // Noch einmal: nichts.
        assert!(c.tick(now + 1.0, true).is_empty());
        // 1 s vor der Zeit: Auslösen mit Rest.
        now = (base + 3600) as f64 - 0.75;
        let a = c.tick(now, true);
        assert_eq!(
            a,
            vec![Action::Fire {
                id: 1,
                at: base + 3600,
                hard: true,
                in_secs: 0.75
            }]
        );
        // Danach plant sie die nächste volle Stunde, ohne die alte zu wiederholen.
        assert!(c.tick(now + 0.5, true).is_empty());
        assert_eq!(c.upcoming().unwrap().at, base + 7200);
    }

    #[test]
    fn state_machine_soft_event_fires_at_the_time() {
        let base = 1_793_318_400;
        let mut c = clock(vec![event(7, Pattern::hourly(15), Timing::Soft)]);
        let at = base + 900;
        assert!(c.tick(base as f64 + 1.0, true).is_empty());
        let a = c.tick(at as f64 - 20.0, true);
        assert!(matches!(a[..], [Action::Predecode { hard: false, .. }]));
        assert!(c.tick(at as f64 - 0.5, true).is_empty());
        let a = c.tick(at as f64 + 0.01, true);
        assert!(matches!(a[..], [Action::Fire { hard: false, .. }]));
    }

    #[test]
    fn state_machine_reports_missed_events() {
        let base = 1_793_318_400;
        // Nicht auf Sendung.
        let mut c = clock(vec![event(1, Pattern::hourly(0), Timing::Hard)]);
        c.tick(base as f64 + 5.0, false);
        let a = c.tick((base + 3600) as f64, false);
        assert_eq!(
            a,
            vec![Action::Missed {
                id: 1,
                at: base + 3600,
                why: MissedWhy::NotOnAir
            }]
        );
        // Takt hängt 30 s: zu spät.
        let mut c = clock(vec![event(1, Pattern::hourly(0), Timing::Hard)]);
        c.tick(base as f64 + 5.0, true);
        let a = c.tick((base + 3630) as f64, true);
        assert!(matches!(
            a[..],
            [Action::Missed {
                why: MissedWhy::TooLate(_),
                ..
            }]
        ));
        // Gerade erst auf Sendung gegangen, 5 s vor der Zeit: Laden und Auslösen im selben Takt.
        let mut c = clock(vec![event(1, Pattern::hourly(0), Timing::Hard)]);
        c.tick(base as f64 + 5.0, false);
        let a = c.tick((base + 3600) as f64 - 0.5, true);
        assert!(matches!(
            a[..],
            [Action::Predecode { .. }, Action::Fire { .. }]
        ));
    }

    #[test]
    fn disabled_and_removed_events_do_not_fire() {
        let base = 1_793_318_400;
        let mut ev = event(1, Pattern::hourly(0), Timing::Hard);
        ev.enabled = false;
        let mut c = clock(vec![ev]);
        c.tick(base as f64 + 5.0, true);
        assert!(c.tick((base + 3600) as f64, true).is_empty());
        assert!(c.upcoming().is_none());
        // Mitten in der Planung ausgeschaltet.
        let mut c = clock(vec![event(1, Pattern::hourly(0), Timing::Hard)]);
        c.tick(base as f64 + 5.0, true);
        let mut t = c.table().clone();
        t.events[0].enabled = false;
        c.set_table(t);
        assert!(c.tick((base + 3600) as f64, true).is_empty());
    }

    #[test]
    fn a_week_of_events_fires_exactly_once_each_across_midnight_and_dst() {
        // Eine Woche um die Zeitumstellung im Herbst (Sonntag 25.10.2026 auf Montag):
        // Event A täglich 00:00 hart, B jede Stunde bei :00 weich, C Mo-Fr 02:30 hart.
        let table = EventTable {
            events: vec![
                event(1, Pattern::daily(1, 0), Timing::Hard),
                event(2, Pattern::hourly(0), Timing::Soft),
                event(
                    3,
                    Pattern {
                        weekdays: 0b001_1111,
                        ..Pattern::daily(1 << 2, 30)
                    },
                    Timing::Hard,
                ),
            ],
            ..EventTable::default()
        };
        let mut c = EventClock::new(table);
        // Start: Samstag 2026-10-24 12:00 Ortszeit (CEST) = 10:00 UTC.
        let start = 1_792_890_000 - 86_400 + 9 * 3600 - 24 * 3600 + 24 * 3600;
        let start = start - 15 * 3600 + 15 * 3600;
        let end = start + 7 * 86_400;
        let mut fired: HashMap<u32, Vec<i64>> = HashMap::new();
        let mut t = start as f64;
        while t < end as f64 {
            for a in c.tick(t, true) {
                if let Action::Fire { id, at, .. } = a {
                    fired.entry(id).or_default().push(at);
                }
            }
            t += 0.5;
        }
        // A: täglich 00:00 Ortszeit; sieben Tage -> 6 oder 7 Auslösungen, alle verschieden und
        // jeweils 00:00 lokal.
        for &(id, hour_minute) in &[(1u32, (0u8, 0u8)), (3u32, (2u8, 30u8))] {
            let v = &fired[&id];
            let mut sorted = v.clone();
            sorted.dedup();
            assert_eq!(&sorted, v, "keine Doppelten bei {id}");
            for &at in v {
                let l = CET.local(at);
                if id == 3 && l.day == CET.local(1_792_890_000).day {
                    continue; // Sonntag fällt weg (Mo-Fr)
                }
                assert_eq!((l.hour, l.minute), hour_minute, "{id} bei {at}");
            }
        }
        // Wochentage von C: nur Mo-Fr.
        assert!(fired[&3].iter().all(|&at| CET.local(at).weekday < 5));
        // C am Montag nach der Umstellung: 02:30 Winterzeit = 01:30 UTC.
        let mon_after = 1_792_890_000 + 86_400 + 1800;
        assert!(fired[&3].contains(&mon_after), "{:?}", fired[&3]);
        // B (jede Stunde, weich): in sieben Tagen 168 Stunden plus die doppelte im Herbst.
        let b = &fired[&2];
        assert!(b.len() == 168 || b.len() == 169, "{}", b.len());
        assert!(b.windows(2).all(|w| w[1] - w[0] == 3600), "gleichmäßig");
    }

    #[test]
    fn midnight_event_lands_on_the_second_and_next_day() {
        let z = Zone::fixed(60);
        // 23:59:59.9 -> 00:00:00 Ortszeit.
        let p = Pattern::daily(1, 0);
        let day = z.local(1_793_318_400).day;
        let midnight = z.to_unix(day + 1, 0).unwrap();
        assert_eq!(p.next_at_or_after(z, midnight - 1), Some(midnight));
        assert_eq!(p.next_at_or_after(z, midnight + 1), Some(midnight + 86_400));
    }
}
