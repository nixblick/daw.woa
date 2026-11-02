//! Probeton (Design-Runde 3): Note an/aus als Command auf eine Spur, ohne Ton testbar.
//!
//! Instrument-Kacheln, „Vorhören“ im Browser, das Drum-Raster und die Piano-Roll schicken hier
//! [`Probe`]-Commands hinein. Die App holt sie einmal je Bild ab ([`Probeton::take`]) und reicht
//! sie an die Note-Queue der Live-Spur weiter (dieselbe `rtrb`-Queue wie das Keyboard). Tests
//! lesen die Commands direkt, kein Ausgang nötig.
//!
//! Kein `woa_model::Command`: Ein Probeton ändert das Projekt nicht, gehört also weder in die
//! Undo-Kette noch in die Projektdatei.

use std::time::{Duration, Instant};

use woa_model::TrackId;

/// Ein Note-Command für die Spur `track`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    NoteOn {
        track: TrackId,
        pitch: u8,
        velocity: u8,
    },
    NoteOff {
        track: TrackId,
        pitch: u8,
    },
}

impl Probe {
    /// Die Spur des Commands.
    pub fn track(&self) -> TrackId {
        match *self {
            Self::NoteOn { track, .. } | Self::NoteOff { track, .. } => track,
        }
    }
}

/// Dauer eines kurzen Probetons (Kachel, Vorhören).
pub const TAP: Duration = Duration::from_millis(450);

/// Warteschlange der Probetöne mit zeitgesteuertem Loslassen.
#[derive(Debug, Default)]
pub struct Probeton {
    ready: Vec<Probe>,
    /// Später fällige Commands (Note an nach Klangwechsel, Note aus nach [`TAP`]).
    timed: Vec<(Instant, Probe)>,
    /// Gehaltene Töne (an, noch nicht aus), damit beim Schließen nichts hängen bleibt.
    held: Vec<(TrackId, u8)>,
}

impl Probeton {
    /// Note an, bis [`Probeton::note_off`] kommt (Klaviatur gedrückt halten).
    pub fn note_on(&mut self, track: TrackId, pitch: u8, velocity: u8) {
        self.ready.push(Probe::NoteOn {
            track,
            pitch,
            velocity: velocity.clamp(1, 127),
        });
        self.held.push((track, pitch));
    }

    /// Note aus.
    pub fn note_off(&mut self, track: TrackId, pitch: u8) {
        self.ready.push(Probe::NoteOff { track, pitch });
        self.held.retain(|h| *h != (track, pitch));
    }

    /// Kurzer Ton (oder Akkord): an ab `now + delay`, aus nach [`TAP`].
    pub fn tap(
        &mut self,
        track: TrackId,
        pitches: &[u8],
        velocity: u8,
        now: Instant,
        delay: Duration,
    ) {
        let on = now + delay;
        for &pitch in pitches {
            let velocity = velocity.clamp(1, 127);
            if delay.is_zero() {
                self.ready.push(Probe::NoteOn {
                    track,
                    pitch,
                    velocity,
                });
            } else {
                self.timed.push((
                    on,
                    Probe::NoteOn {
                        track,
                        pitch,
                        velocity,
                    },
                ));
            }
            self.timed.push((on + TAP, Probe::NoteOff { track, pitch }));
        }
    }

    /// Lässt alle gehaltenen und geplanten Töne los (Editor zu, Spur gewechselt).
    pub fn release_all(&mut self) {
        let offs: Vec<Probe> = self
            .timed
            .drain(..)
            .filter_map(|(_, p)| matches!(p, Probe::NoteOff { .. }).then_some(p))
            .chain(
                self.held
                    .drain(..)
                    .map(|(track, pitch)| Probe::NoteOff { track, pitch }),
            )
            .collect();
        self.ready.extend(offs);
    }

    /// Alle bis `now` fälligen Commands in Reihenfolge.
    pub fn take(&mut self, now: Instant) -> Vec<Probe> {
        let mut out = std::mem::take(&mut self.ready);
        let mut due: Vec<(Instant, Probe)> = Vec::new();
        self.timed.retain(|e| {
            if e.0 <= now {
                due.push(*e);
                false
            } else {
                true
            }
        });
        due.sort_by_key(|e| e.0);
        out.extend(due.into_iter().map(|e| e.1));
        out
    }

    /// Wann das nächste geplante Command fällig ist (für `request_repaint_after`).
    pub fn next_due(&self, now: Instant) -> Option<Duration> {
        self.timed
            .iter()
            .map(|e| e.0.saturating_duration_since(now))
            .min()
    }

    /// Wartet etwas (Tests und Anzeige)?
    pub fn is_idle(&self) -> bool {
        self.ready.is_empty() && self.timed.is_empty()
    }
}

/// Probeton-Tonhöhe einer Spur: Kick (36) für ein Kit, sonst C4.
pub fn pitch_for(sound: Option<woa_model::TrackSound>) -> u8 {
    if sound.is_some_and(|s| s.drums) {
        36
    } else {
        60
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: TrackId = TrackId(7);

    #[test]
    fn held_note_goes_on_and_off() {
        let mut p = Probeton::default();
        let now = Instant::now();
        p.note_on(T, 60, 100);
        p.note_off(T, 60);
        assert_eq!(
            p.take(now),
            vec![
                Probe::NoteOn {
                    track: T,
                    pitch: 60,
                    velocity: 100
                },
                Probe::NoteOff {
                    track: T,
                    pitch: 60
                }
            ]
        );
        assert!(p.is_idle());
    }

    #[test]
    fn tap_releases_after_the_tap_time() {
        let mut p = Probeton::default();
        let now = Instant::now();
        p.tap(T, &[60, 64, 67], 90, now, Duration::ZERO);
        let first = p.take(now);
        assert_eq!(first.len(), 3);
        assert!(first.iter().all(|c| matches!(c, Probe::NoteOn { .. })));
        assert!(p.take(now + TAP / 2).is_empty());
        let offs = p.take(now + TAP);
        assert_eq!(offs.len(), 3);
        assert!(offs.iter().all(|c| matches!(c, Probe::NoteOff { .. })));
        assert!(p.is_idle());
    }

    #[test]
    fn delayed_tap_starts_later() {
        let mut p = Probeton::default();
        let now = Instant::now();
        let d = Duration::from_millis(150);
        p.tap(T, &[36], 100, now, d);
        assert!(p.take(now).is_empty());
        assert_eq!(p.next_due(now), Some(d));
        assert!(matches!(
            p.take(now + d)[..],
            [Probe::NoteOn { pitch: 36, .. }]
        ));
    }

    #[test]
    fn release_all_leaves_nothing_hanging() {
        let mut p = Probeton::default();
        let now = Instant::now();
        p.note_on(T, 62, 100);
        p.tap(T, &[60], 100, now, Duration::ZERO);
        p.release_all();
        let out = p.take(now);
        let offs: Vec<u8> = out
            .iter()
            .filter_map(|c| match c {
                Probe::NoteOff { pitch, .. } => Some(*pitch),
                Probe::NoteOn { .. } => None,
            })
            .collect();
        assert!(offs.contains(&60) && offs.contains(&62));
        assert!(p.is_idle());
    }

    #[test]
    fn kit_tracks_probe_the_kick() {
        let kit = woa_model::TrackSound {
            program: 0,
            drums: true,
            bank: None,
        };
        assert_eq!(pitch_for(Some(kit)), 36);
        assert_eq!(pitch_for(None), 60);
    }
}
