//! Ortszeit mit europäischer Sommerzeit, ohne Zeitzonen-Datenbank (Paket 29).
//!
//! Eine einzige Stelle für „wie spät ist es hier“: Stundenuhr ([`woa_library`]-Rotation) und
//! feste Zeit-Events rechnen beide damit, damit sie an der Zeitumstellung nicht auseinanderlaufen.
//!
//! Regel der EU: Sommerzeit beginnt am letzten Sonntag im März um 01:00 UTC und endet am letzten
//! Sonntag im Oktober um 01:00 UTC. Die Zone ist ein Grundversatz (Minuten gegenüber UTC, im
//! Winter) und ein Schalter für die EU-Regel; ohne Schalter bleibt der Versatz fest.

/// Montag = 0.
pub type Weekday = u8;

/// Eine Zone: Grundversatz im Winter plus optional die EU-Sommerzeit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Zone {
    /// Minuten gegenüber UTC im Winter (Mitteleuropa 60).
    pub base_min: i32,
    /// EU-Sommerzeit (+60 Minuten) anwenden.
    pub eu_dst: bool,
}

/// Ortszeit zu einem Zeitpunkt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Local {
    /// Ortstage seit 1970-01-01.
    pub day: i64,
    /// Montag = 0.
    pub weekday: Weekday,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    /// Ortsstunden seit 1970 (Schlüssel der Ortsstunde; in der doppelten Stunde im Herbst
    /// gleich für beide Durchläufe).
    pub hour_key: i64,
}

impl Zone {
    /// Mitteleuropa mit Sommerzeit.
    pub const CENTRAL_EUROPE: Zone = Zone {
        base_min: 60,
        eu_dst: true,
    };

    /// Fester Versatz ohne Sommerzeit.
    pub const fn fixed(base_min: i32) -> Zone {
        Zone {
            base_min,
            eu_dst: false,
        }
    }

    /// Versatz zu `unix` in Minuten.
    pub fn offset_min(&self, unix: i64) -> i32 {
        self.base_min
            + if self.eu_dst && eu_summer(unix) {
                60
            } else {
                0
            }
    }

    /// Ortszeit von `unix`.
    pub fn local(&self, unix: i64) -> Local {
        let t = unix + i64::from(self.offset_min(unix)) * 60;
        let day = t.div_euclid(86_400);
        let sod = t.rem_euclid(86_400);
        Local {
            day,
            // 1. Januar 1970 war ein Donnerstag.
            weekday: (day + 3).rem_euclid(7) as u8,
            hour: (sod / 3600) as u8,
            minute: (sod % 3600 / 60) as u8,
            second: (sod % 60) as u8,
            hour_key: t.div_euclid(3600),
        }
    }

    /// Erster Zeitpunkt, zu dem die Wanduhr `secs_of_day` am Ortstag `day` zeigt. `None`,
    /// wenn diese Uhrzeit im Frühjahr übersprungen wird. In der doppelten Stunde im Herbst
    /// gilt der erste Durchlauf.
    pub fn to_unix(&self, day: i64, secs_of_day: i64) -> Option<i64> {
        let local = day * 86_400 + secs_of_day;
        let mut best: Option<i64> = None;
        let offsets = [self.base_min + 60, self.base_min];
        for off in offsets {
            if off != self.base_min && !self.eu_dst {
                continue;
            }
            let u = local - i64::from(off) * 60;
            if self.offset_min(u) == off && best.is_none_or(|b| u < b) {
                best = Some(u);
            }
        }
        best
    }

    /// Zeitpunkt der Umstellung Winter nach Sommer, die eine übersprungene Ortszeit
    /// (`day`, `secs_of_day`) verschluckt hat. `None` ohne Sommerzeit oder wenn die Zeit
    /// existiert.
    pub fn gap_end(&self, day: i64, secs_of_day: i64) -> Option<i64> {
        if !self.eu_dst || self.to_unix(day, secs_of_day).is_some() {
            return None;
        }
        let u = day * 86_400 + secs_of_day - i64::from(self.base_min) * 60;
        let (start, _) = eu_window(civil_year(u.div_euclid(86_400)));
        Some(start)
    }
}

/// Tage seit 1970 für ein Datum (Howard Hinnant, gemeinfrei).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Jahr zu Tagen seit 1970.
pub fn civil_year(days: i64) -> i64 {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 { y + 1 } else { y }
}

/// Letzter Sonntag eines 31-Tage-Monats (März, Oktober) als Tage seit 1970.
fn last_sunday(y: i64, m: i64) -> i64 {
    let d31 = days_from_civil(y, m, 31);
    let weekday = (d31 + 3).rem_euclid(7);
    d31 - (weekday + 1) % 7
}

/// Beginn und Ende der EU-Sommerzeit im Jahr `y` (Unix-Sekunden, je 01:00 UTC).
pub fn eu_window(y: i64) -> (i64, i64) {
    (
        last_sunday(y, 3) * 86_400 + 3600,
        last_sunday(y, 10) * 86_400 + 3600,
    )
}

/// Gilt zu `unix` die EU-Sommerzeit?
pub fn eu_summer(unix: i64) -> bool {
    let (s, e) = eu_window(civil_year(unix.div_euclid(86_400)));
    unix >= s && unix < e
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPRING_2026: i64 = 1_774_746_000;
    const AUTUMN_2026: i64 = 1_792_890_000;

    #[test]
    fn switch_instants_match_the_calendar() {
        assert_eq!(eu_window(2026), (SPRING_2026, AUTUMN_2026));
        assert_eq!(eu_window(2027).0, 1_806_195_600);
        assert!(!eu_summer(SPRING_2026 - 1));
        assert!(eu_summer(SPRING_2026));
        assert!(eu_summer(AUTUMN_2026 - 1));
        assert!(!eu_summer(AUTUMN_2026));
    }

    #[test]
    fn local_time_follows_the_switch() {
        let z = Zone::CENTRAL_EUROPE;
        let before = z.local(SPRING_2026 - 1);
        assert_eq!((before.hour, before.minute, before.second), (1, 59, 59));
        let after = z.local(SPRING_2026);
        assert_eq!((after.hour, after.minute), (3, 0));
        let before = z.local(AUTUMN_2026 - 1);
        assert_eq!((before.hour, before.minute, before.second), (2, 59, 59));
        let after = z.local(AUTUMN_2026);
        assert_eq!((after.hour, after.minute), (2, 0));
        // Sonntag.
        assert_eq!(after.weekday, 6);
    }

    #[test]
    fn fixed_zone_ignores_summer_time() {
        let z = Zone::fixed(60);
        assert_eq!(z.local(SPRING_2026).hour, 2);
        assert_eq!(z.offset_min(AUTUMN_2026 - 1), 60);
    }

    #[test]
    fn wall_clock_to_unix_handles_gap_and_repeat() {
        let z = Zone::CENTRAL_EUROPE;
        let spring_day = z.local(SPRING_2026).day;
        // 02:30 gibt es am Frühjahrstag nicht.
        assert_eq!(z.to_unix(spring_day, 2 * 3600 + 1800), None);
        assert_eq!(z.gap_end(spring_day, 2 * 3600 + 1800), Some(SPRING_2026));
        // 03:30 gibt es.
        assert_eq!(
            z.to_unix(spring_day, 3 * 3600 + 1800),
            Some(SPRING_2026 + 1800)
        );
        // 02:30 im Herbst: erster Durchlauf (Sommerzeit).
        let autumn_day = z.local(AUTUMN_2026).day;
        let first = z.to_unix(autumn_day, 2 * 3600 + 1800);
        assert_eq!(first, Some(AUTUMN_2026 - 1800));
        // 00:00 ist normal.
        assert_eq!(z.to_unix(autumn_day, 0), Some(AUTUMN_2026 - 3 * 3600));
    }

    #[test]
    fn weekday_and_day_rollover() {
        let z = Zone::fixed(0);
        // 2026-10-30 ist ein Freitag.
        let l = z.local(1_793_318_400);
        assert_eq!(l.weekday, 4);
        let l = z.local(1_793_318_400 + 86_400 * 3 - 1);
        assert_eq!(l.weekday, 6);
        let l = z.local(1_793_318_400 + 86_400 * 3);
        assert_eq!(l.weekday, 0);
        assert_eq!((l.hour, l.minute), (0, 0));
    }
}
