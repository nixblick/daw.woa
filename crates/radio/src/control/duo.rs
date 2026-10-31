//! Party mit zwei Decks (André 31.10.2026): links und rechts spielen echte, gleichzeitig
//! laufende Seiten im Mixer ([`Cmd::PartyLoad`] usw.), der Crossfader blendet stufenlos mit
//! konstanter Leistung. Die Playlist bleibt das Modell: „läuft“ ist die hörbarere Seite,
//! „nächster“ die vorgeladene.
//!
//! - **Auto-Übergang an:** kurz vor dem Ende der laufenden Seite (Crossfade-Länge) startet die
//!   andere mit dem nächsten Titel, der Fader fährt im Mixer hinüber. Eine Handbewegung
//!   ersetzt die Fahrt sofort, die Automatik fährt nicht dagegen.
//! - **Aus:** der DJ startet jede Seite selbst und blendet von Hand.
//! - In beiden Fällen lädt eine Seite, die steht und nicht hörbar ist, den nächsten Titel nach
//!   der hörbaren vor; eine zu Ende gelaufene Seite startet nie von selbst.
//!
//! Läuft beim Betreten die Playlist, übernimmt die linke Seite an derselben Stelle; beim
//! Verlassen übernimmt die Playlist die hörbare Seite an ihrer Stelle.

use std::sync::atomic::Ordering;

use basedrop::Shared;

use super::Controller;
use crate::PARTY_SIDES;
use crate::audio::{Stereo, load_stereo, measure};
use crate::cues::CueFrames;
use crate::level::db_to_gain;
use crate::mixer::{Cmd, DECK_PLAYING, NONE};

/// Zustand der zwei Party-Seiten im Controller.
#[derive(Default)]
pub(super) struct Duo {
    /// Titel (uid) je Seite.
    uid: [Option<u32>; PARTY_SIDES],
    /// Puffer je Seite (für die Übergabe an die Playlist beim Verlassen).
    bufs: [Option<Shared<Stereo>>; PARTY_SIDES],
    /// Zuletzt gesehener „fertig“-Zähler je Seite.
    done_seen: [u32; PARTY_SIDES],
    /// Die Automatik hat für den laufenden Titel dieser Seite schon übergeben.
    fired: [bool; PARTY_SIDES],
    /// Die hörbare Seite (0 links, 1 rechts).
    live: usize,
}

/// Was die Oberfläche von einer Party-Seite zeigt.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PartySideView {
    /// Playlist-Index des Titels auf der Seite.
    pub index: Option<usize>,
    pub playing: bool,
    /// Stelle und Ende (Sekunden).
    pub position: f64,
    pub end: f64,
}

/// Schnappschuss der Party-Decks.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PartyDuoView {
    pub sides: [PartySideView; PARTY_SIDES],
    /// Die hörbare Seite.
    pub live: usize,
    /// Stellung des Crossfaders, wie der Mixer sie fährt (0 links, 1 rechts).
    pub fader: f32,
    /// Auto-Übergang an.
    pub auto: bool,
}

impl Controller {
    /// Party-Decks an: ab jetzt spielen die Seiten statt des Playlist-Abspielers. Läuft die
    /// Playlist (oder ist sie pausiert), übernimmt die linke Seite den Titel an seiner Stelle.
    pub fn party_enter(&mut self) {
        if self.duo.is_some() {
            return;
        }
        let mut duo = Duo::default();
        for (s, seen) in duo.done_seen.iter_mut().enumerate() {
            *seen = self.status.party_done[s].load(Ordering::Acquire);
        }
        self.duo = Some(duo);
        // Was gerade läuft bzw. pausiert ist, und an welcher Stelle.
        let cur = self.status.current.load(Ordering::Acquire);
        let take = if let Some((uid, pos, _)) = self.paused {
            Some((uid, pos, false))
        } else if self.running && cur != NONE {
            Some((cur, self.status.position.load(Ordering::Acquire), true))
        } else {
            None
        };
        self.send(Cmd::PartyFader { to: 0.0, frames: 0 });
        match take.and_then(|(uid, pos, play)| Some((self.index_of(uid)?, pos, play))) {
            Some((i, pos, play)) => {
                // Die Stelle nach dem Dekodieren nachmessen: so fehlt kaum etwas.
                let loaded = self.party_load(0, i, Some(pos));
                self.stop();
                if loaded && play {
                    let now = self.status.position.load(Ordering::Acquire);
                    if now > pos {
                        self.send(Cmd::PartySeek(0, (now - pos) as i64));
                    }
                    self.send(Cmd::PartyPlay(0));
                }
            }
            None => {
                let i = self
                    .show
                    .first_music_from(self.cue_index())
                    .unwrap_or(usize::MAX);
                if i < self.show.playlist.len() {
                    self.party_load(0, i, None);
                }
            }
        }
        self.party_sync_idle();
    }

    /// Party-Decks aus: spielt die hörbare Seite, übernimmt die Playlist sie an ihrer Stelle.
    pub fn party_leave(&mut self) {
        let Some(duo) = self.duo.take() else {
            return;
        };
        let l = duo.live;
        let playing = self.status.party_state[l].load(Ordering::Acquire) == DECK_PLAYING;
        let pos = self.status.party_pos[l].load(Ordering::Acquire) as usize;
        let index = duo.uid[l].and_then(|u| self.index_of(u));
        for s in 0..PARTY_SIDES {
            self.send(Cmd::PartyStop(s));
        }
        let Some(i) = index else {
            return;
        };
        self.set_cue(i);
        if let (true, Some(buf)) = (playing, duo.bufs[l].clone()) {
            let e = &self.show.playlist[i];
            let (uid, gain) = (e.uid, db_to_gain(e.gain_db(self.show.target_lufs)));
            let mut cues = e.cues.frames(self.rate, buf.frames());
            cues.start = pos.clamp(cues.start, cues.end);
            self.paused = None;
            self.resume_at = None;
            self.send(Cmd::ClearNext);
            self.send(Cmd::Queue {
                uid,
                buf,
                gain,
                cues,
                lead: None,
                hold: false,
            });
            self.pending = Some(uid);
            self.pending_plays = self.status.plays.load(Ordering::Acquire);
            self.failed.clear();
            self.running = true;
            self.send(Cmd::Play);
        }
    }

    /// Sind die Party-Decks an?
    pub fn party_active(&self) -> bool {
        self.duo.is_some()
    }

    /// Auto-Übergang an oder aus (wird mit der Sendung gespeichert).
    pub fn set_party_auto(&mut self, on: bool) {
        self.show.party_auto = on;
    }

    pub fn party_auto(&self) -> bool {
        self.show.party_auto
    }

    /// Crossfader von Hand (0 links, 1 rechts). Ersetzt eine laufende Automatik-Fahrt.
    pub fn party_fader(&mut self, pos: f32) {
        if self.duo.is_some() && pos.is_finite() {
            self.send(Cmd::PartyFader {
                to: pos.clamp(0.0, 1.0),
                frames: 0,
            });
        }
    }

    /// Seite `side` starten (▶ Play am Deck).
    pub fn party_play(&mut self, side: usize) {
        if self.duo.is_some() && side < PARTY_SIDES && self.party_loaded(side) {
            self.send(Cmd::PartyPlay(side));
        }
    }

    /// Seite `side` anhalten (die Stelle bleibt).
    pub fn party_pause(&mut self, side: usize) {
        if self.duo.is_some() && side < PARTY_SIDES {
            self.send(Cmd::PartyStop(side));
        }
    }

    /// Spielt irgendeine Seite?
    pub fn party_playing(&self) -> bool {
        self.duo.is_some() && (0..PARTY_SIDES).any(|s| self.party_side_playing(s))
    }

    /// Play/Pause der großen Taste: läuft etwas, halten alle Seiten an; sonst startet die
    /// Seite, auf die der Fader zeigt (sonst die hörbare).
    pub fn party_toggle(&mut self) {
        if self.duo.is_none() {
            return;
        }
        if self.party_playing() {
            for s in 0..PARTY_SIDES {
                self.send(Cmd::PartyStop(s));
            }
            return;
        }
        let side = self.party_live();
        let side = if self.party_loaded(side) {
            side
        } else {
            1 - side
        };
        if self.party_loaded(side) {
            if let Some(d) = self.duo.as_mut() {
                d.live = side;
            }
            self.send(Cmd::PartyFader {
                to: side as f32,
                frames: 0,
            });
            self.send(Cmd::PartyPlay(side));
        }
    }

    /// „Weiter“: die andere Seite startet jetzt, der Fader fährt in der Crossfade-Länge hin.
    /// Läuft nichts, startet die hörbare Seite.
    pub fn party_next(&mut self) {
        if self.duo.is_none() {
            return;
        }
        if !self.party_playing() {
            self.party_toggle();
            return;
        }
        let l = self.party_live();
        let o = 1 - l;
        if !self.party_loaded(o) {
            return;
        }
        let xf = self.crossfade_frames();
        self.party_transition(l, o, xf);
    }

    /// Vorheriger Titel: auf die andere Seite und hinüberblenden.
    pub fn party_previous(&mut self) {
        let Some(live_idx) = self.party_live_index() else {
            return;
        };
        let to = self.show.prev_music(live_idx).unwrap_or(live_idx);
        let l = self.party_live();
        let o = 1 - l;
        if self.party_load(o, to, None) {
            if self.party_playing() {
                let xf = self.crossfade_frames();
                self.party_transition(l, o, xf);
            } else if let Some(d) = self.duo.as_mut() {
                d.live = o;
                self.send(Cmd::PartyFader {
                    to: o as f32,
                    frames: 0,
                });
            }
        }
    }

    /// Die hörbare Seite beginnt von vorn.
    pub fn party_restart(&mut self) {
        let l = self.party_live();
        self.send(Cmd::PartySeek(l, i64::MIN / 2));
    }

    /// Auf der hörbaren Seite springen.
    pub fn party_seek(&mut self, seconds: f64) {
        if self.duo.is_none() || !seconds.is_finite() {
            return;
        }
        let l = self.party_live();
        let frames = (seconds * f64::from(self.rate)).round() as i64;
        self.send(Cmd::PartySeek(l, frames));
    }

    /// Playlist-Index des hörbaren Titels (auch wenn er steht).
    pub fn party_live_index(&self) -> Option<usize> {
        let d = self.duo.as_ref()?;
        d.uid[d.live].and_then(|u| self.index_of(u))
    }

    /// Schnappschuss für die Oberfläche (`None`, wenn die Party-Decks aus sind).
    pub fn party_view(&self) -> Option<PartyDuoView> {
        let d = self.duo.as_ref()?;
        let r = f64::from(self.rate);
        let sides = std::array::from_fn(|s| PartySideView {
            index: d.uid[s].and_then(|u| self.index_of(u)),
            playing: self.party_side_playing(s),
            position: self.status.party_pos[s].load(Ordering::Acquire) as f64 / r,
            end: self.status.party_end[s].load(Ordering::Acquire) as f64 / r,
        });
        Some(PartyDuoView {
            sides,
            live: d.live,
            fader: f32::from_bits(self.status.party_fader.load(Ordering::Acquire)),
            auto: self.show.party_auto,
        })
    }

    // ---- intern ------------------------------------------------------------------------

    fn party_live(&self) -> usize {
        self.duo.as_ref().map_or(0, |d| d.live)
    }

    fn party_side_playing(&self, s: usize) -> bool {
        self.status.party_state[s].load(Ordering::Acquire) == DECK_PLAYING
    }

    fn party_loaded(&self, s: usize) -> bool {
        self.duo.as_ref().is_some_and(|d| d.bufs[s].is_some())
    }

    /// Übergang von Seite `from` auf `to`: `to` startet, der Fader fährt in `frames` hin.
    fn party_transition(&mut self, from: usize, to: usize, frames: usize) {
        self.send(Cmd::PartyPlay(to));
        self.send(Cmd::PartyFader {
            to: to as f32,
            frames,
        });
        if let Some(d) = self.duo.as_mut() {
            d.fired[from] = true;
            d.fired[to] = false;
            d.live = to;
        }
        tracing::info!(from, to, frames, "party: transition");
    }

    /// Legt Playlist-Titel `index` auf Seite `side` (ab `from` Frames, sonst ab Cue-In).
    /// `false`, wenn er sich nicht laden ließ (er wird dann übersprungen).
    fn party_load(&mut self, side: usize, index: usize, from: Option<u64>) -> bool {
        let Some(e) = self.show.playlist.get(index) else {
            return false;
        };
        let (uid, path) = (e.uid, e.path.clone());
        match load_stereo(&path, self.rate) {
            Ok(s) => {
                if let Some(e) = self.show.playlist.get_mut(index)
                    && !e.is_measured()
                {
                    e.set_measured(measure(&s, self.rate));
                }
                let buf = Shared::new(&self.collector.handle(), s);
                let e = &self.show.playlist[index];
                let gain = db_to_gain(e.gain_db(self.show.target_lufs));
                let CueFrames { start, end, .. } = e.cues.frames(self.rate, buf.frames());
                let start = from.map_or(start, |p| (p as usize).clamp(start, end));
                self.send(Cmd::PartyLoad {
                    side,
                    buf: buf.clone(),
                    gain,
                    start,
                    end,
                });
                if let Some(d) = self.duo.as_mut() {
                    d.uid[side] = Some(uid);
                    d.bufs[side] = Some(buf);
                    d.fired[side] = false;
                }
                true
            }
            Err(err) => {
                tracing::warn!(error = %err, "party: load failed");
                self.failed.push(uid);
                self.message = Some(err);
                false
            }
        }
    }

    /// Die Seite, die nicht hörbar ist und steht, bekommt den nächsten Titel nach der hörbaren.
    fn party_sync_idle(&mut self) {
        let Some(d) = self.duo.as_ref() else {
            return;
        };
        let (l, uid_l, uid_o) = (d.live, d.uid[d.live], d.uid[1 - d.live]);
        let o = 1 - l;
        if self.party_side_playing(o) {
            return;
        }
        let cur = uid_l.and_then(|u| self.index_of(u));
        let keep = uid_o.and_then(|u| self.index_of(u));
        // Bei Zufall nicht jeden Takt neu würfeln: eine gültige Wahl bleibt liegen.
        if self.shuffle && keep.is_some() && keep != cur {
            return;
        }
        let after = cur.map_or_else(|| self.cue_index(), |i| i + 1);
        let failed = self.failed.clone();
        let want = self
            .candidates(cur, after, true)
            .into_iter()
            .find(|&i| !failed.contains(&self.show.playlist[i].uid));
        if want == keep && keep.is_some() {
            return;
        }
        match want {
            Some(i) => {
                self.party_load(o, i, None);
            }
            None if keep.is_none() && uid_o.is_some() => {
                // Der Titel ist weg und nichts folgt: die Seite leeren.
                self.send(Cmd::PartyEject(o));
                if let Some(d) = self.duo.as_mut() {
                    d.uid[o] = None;
                    d.bufs[o] = None;
                }
            }
            None => {}
        }
    }

    /// Takt der Party-Decks (aus [`Controller::pump`]).
    pub(super) fn party_tick(&mut self) {
        let Some(d) = self.duo.as_mut() else {
            return;
        };
        let mut ended = [false; PARTY_SIDES];
        for (s, e) in ended.iter_mut().enumerate() {
            let done = self.status.party_done[s].load(Ordering::Acquire);
            *e = done != d.done_seen[s];
            d.done_seen[s] = done;
        }
        // Hörbar: von den spielenden Seiten die, auf die der Fader zeigt.
        let playing: [bool; PARTY_SIDES] = std::array::from_fn(|s| {
            self.status.party_state[s].load(Ordering::Acquire) == DECK_PLAYING
        });
        let x = f32::from_bits(self.status.party_fader.load(Ordering::Acquire));
        let live = match playing {
            [true, false] => 0,
            [false, true] => 1,
            [true, true] if x < 0.5 => 0,
            [true, true] if x > 0.5 => 1,
            _ => d.live,
        };
        d.live = live;
        if self.show.party_auto {
            let o = 1 - live;
            let fired = d.fired[live];
            let has_o = d.uid[o].is_some();
            let rest = self.status.party_end[live]
                .load(Ordering::Acquire)
                .saturating_sub(self.status.party_pos[live].load(Ordering::Acquire))
                as usize;
            let xf = self.crossfade_frames();
            let due = if xf > 0 {
                playing[live] && rest <= xf
            } else {
                ended[live]
            };
            if due && !fired && has_o && !playing[o] {
                self.party_transition(live, o, rest.max(1));
            }
        }
        // Eine hörbare Seite ist zu Ende und steht: die andere ist jetzt dran (sie startet nicht
        // von selbst), die fertige lädt danach den Titel hinter ihr vor.
        if let Some(d) = self.duo.as_mut() {
            let l = d.live;
            if ended[l]
                && self.status.party_state[l].load(Ordering::Acquire) != DECK_PLAYING
                && d.bufs[1 - l].is_some()
                && self.status.party_state[1 - l].load(Ordering::Acquire) != DECK_PLAYING
            {
                d.live = 1 - l;
            }
        }
        self.party_sync_idle();
    }
}
