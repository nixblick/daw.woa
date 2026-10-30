//! Voice-Tracking (Paket 30): Moderation vorab einsprechen oder importieren und zwischen dem
//! Ausklang des einen und dem Intro des nächsten Titels einpassen.
//!
//! Ein Voice-Track ist ein Eintrag der Playlist (`PlaylistEntry::voice` ist gesetzt), der nie
//! als Deck spielt: Er liegt zwischen zwei Titeln A und B und wird beim Übergang A nach B
//! über die Musik gelegt, während die Musik abgesenkt wird („Ducking“).
//!
//! **Zeitachse.** Null ist der Beginn der Überblendung, also der Moment, in dem B einsetzt und A
//! auszublenden beginnt. `offset` ist der Beginn der Stimme auf dieser Achse: negativ heißt,
//! die Stimme beginnt noch im Ausklang von A, positiv, sie beginnt erst über dem Intro von B.
//!
//! **Ducking.** Die Musik wird vor der Stimme in `attack` Sekunden auf `duck_db` abgesenkt,
//! bleibt so lange die Stimme läuft und kommt danach in `release` Sekunden zurück. Die
//! Hüllkurve ist reine Rechnung ([`duck_gain`]): Der Mixer rechnet sie je Frame ohne
//! Speicher, der Editor zeichnet dieselbe Funktion. Die Idee (Träger auf einen Anteil
//! absenken, Sprecher darüber, danach zurück) kennt `smooth_add` in Liquidsoap
//! (`src/libs/extra/audio.liq`, GPL-2.0-or-later); es ist kein Code übernommen
//! (siehe `docs/herkunft.md`).
//!
//! **Bezug zu den Nachbarn.** Der Eintrag merkt sich Pfad, Länge und Cue-Punkte von A und B
//! ([`Neighbor`]). Ändert sich einer davon (anderer Titel, andere Länge, andere Marken, Titel
//! entfernt), steht der Voice-Track auf „prüfen“ und wird nicht gesendet, bis der Mensch
//! bestätigt: Eine Moderation, die auf den falschen Titel zeigt, soll nicht auf Sendung gehen.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::cues::Cues;
use crate::show::{PlaylistEntry, RadioShow};

/// Weiteste Verschiebung der Stimme gegen den Übergang (Sekunden, nach beiden Seiten).
pub const MAX_OFFSET: f64 = 60.0;
/// Längste Moderation (Sekunden).
pub const MAX_VOICE_SECONDS: f64 = 300.0;
/// Längste Absenk- und Rückholzeit (Sekunden).
pub const MAX_RAMP: f64 = 10.0;
/// Weich ein- und ausblenden der Stimme selbst (Sekunden), gegen Knacken.
pub const EDGE_SECONDS: f64 = 0.005;

fn default_attack() -> f64 {
    0.4
}
fn default_release() -> f64 {
    0.8
}
fn default_duck() -> f64 {
    -12.0
}

/// Die einstellbaren Werte eines Voice-Tracks.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VoiceParams {
    /// Beginn der Stimme gegen den Beginn der Überblendung (Sekunden, negativ = früher).
    #[serde(default)]
    pub offset: f64,
    /// Zusätzliche Verstärkung der Stimme (dB) zur automatischen Angleichung an die Ziel-Lautheit.
    #[serde(default)]
    pub gain_db: f64,
    /// Absenkung der Musik unter der Stimme (dB, höchstens 0).
    #[serde(default = "default_duck")]
    pub duck_db: f64,
    /// Zeit der Absenkung vor der Stimme (Sekunden).
    #[serde(default = "default_attack")]
    pub attack: f64,
    /// Zeit der Rückkehr nach der Stimme (Sekunden).
    #[serde(default = "default_release")]
    pub release: f64,
}

impl Default for VoiceParams {
    fn default() -> Self {
        Self {
            offset: 0.0,
            gain_db: 0.0,
            duck_db: default_duck(),
            attack: default_attack(),
            release: default_release(),
        }
    }
}

impl VoiceParams {
    /// Bringt alle Werte in gültige Bereiche (nicht endliche Werte fallen auf die Vorgabe).
    pub fn sanitized(mut self) -> Self {
        let fix = |v: f64, d: f64| if v.is_finite() { v } else { d };
        self.offset = fix(self.offset, 0.0).clamp(-MAX_OFFSET, MAX_OFFSET);
        self.gain_db = fix(self.gain_db, 0.0).clamp(-24.0, 12.0);
        self.duck_db = fix(self.duck_db, default_duck()).clamp(-40.0, 0.0);
        self.attack = fix(self.attack, default_attack()).clamp(0.0, MAX_RAMP);
        self.release = fix(self.release, default_release()).clamp(0.0, MAX_RAMP);
        self
    }

    /// Ducking-Tiefe als linearer Faktor (1.0 = keine Absenkung).
    pub fn depth(&self) -> f32 {
        10f32.powf(self.duck_db as f32 / 20.0)
    }

    /// In Frames bei `rate` für den Mixer, `len` = Länge der Stimme in Frames.
    pub fn frames(&self, rate: u32, len: usize) -> VoiceFrames {
        let r = f64::from(rate);
        VoiceFrames {
            offset: (self.offset * r).round() as i64,
            len: len as i64,
            attack: (self.attack * r).round() as i64,
            release: (self.release * r).round() as i64,
            depth: self.depth(),
        }
    }
}

/// Werte in Frames, wie der Mixer sie braucht.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoiceFrames {
    pub offset: i64,
    pub len: i64,
    pub attack: i64,
    pub release: i64,
    pub depth: f32,
}

/// Faktor für die Musik an Frame `pos` der Stimme (`pos` 0 = erster Sample der Stimme, negativ
/// = Absenkung vor der Stimme). Reine Rechnung ohne Speicher: gilt im Audio-Thread genauso wie im
/// Editor. Absenken und Zurückholen laufen auf einer Cosinus-Rampe (keine Knicke).
#[inline]
pub fn duck_gain(pos: i64, len: i64, attack: i64, release: i64, depth: f32) -> f32 {
    if pos < -attack || pos >= len + release {
        return 1.0;
    }
    let ramp = |x: f32| 0.5 - 0.5 * (std::f32::consts::PI * x.clamp(0.0, 1.0)).cos();
    let s = if pos < 0 {
        ramp((pos + attack) as f32 / attack.max(1) as f32)
    } else if pos < len {
        1.0
    } else {
        1.0 - ramp((pos - len) as f32 / release.max(1) as f32)
    };
    1.0 - (1.0 - depth) * s
}

/// Weiches Ein- und Ausblenden der Stimme an ihren Rändern (`fade` Frames).
#[inline]
pub fn edge_gain(pos: i64, len: i64, fade: i64) -> f32 {
    if fade <= 0 {
        return 1.0;
    }
    let from_start = (pos + 1).min(fade) as f32 / fade as f32;
    let from_end = (len - pos).min(fade) as f32 / fade as f32;
    from_start.min(from_end).clamp(0.0, 1.0)
}

/// Absenkungskurve zum Zeichnen: `(Sekunde auf der Zeitachse, Faktor)` von `from` bis `to`
/// in Schritten von `step` Sekunden. Die Zeitachse hat ihren Nullpunkt am Beginn der
/// Überblendung, die Stimme beginnt bei `p.offset`.
pub fn duck_curve(
    p: &VoiceParams,
    voice_seconds: f64,
    from: f64,
    to: f64,
    step: f64,
) -> Vec<(f64, f32)> {
    const R: i64 = 1000;
    let f = p.frames(R as u32, (voice_seconds * 1000.0).round() as usize);
    let step = step.max(0.001);
    let n = ((to - from) / step).ceil().max(0.0) as usize;
    (0..=n)
        .map(|i| {
            let t = from + i as f64 * step;
            let pos = (t * 1000.0).round() as i64 - f.offset;
            (t, duck_gain(pos, f.len, f.attack, f.release, f.depth))
        })
        .collect()
}

/// Was von einem Nachbarn gemerkt wird.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Neighbor {
    pub path: PathBuf,
    /// Länge in Millisekunden; `None`, solange der Titel noch nicht gemessen war.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Cues::is_empty")]
    pub cues: Cues,
}

/// Vergleich des gemerkten mit dem jetzigen Nachbarn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cmp {
    Same,
    /// Gleicher Titel, der beim Merken noch nicht gemessen war: Werte stillschweigend nachziehen.
    Rebind,
    Changed,
}

impl Neighbor {
    pub fn of(e: &PlaylistEntry) -> Self {
        Self {
            path: e.path.clone(),
            ms: e.seconds.map(|s| (s * 1000.0).round() as i64),
            cues: e.cues,
        }
    }

    fn compare(&self, now: &Neighbor) -> Cmp {
        if self.path != now.path {
            Cmp::Changed
        } else if self.ms.is_none() {
            if now.ms.is_some() || now.cues != self.cues {
                Cmp::Rebind
            } else {
                Cmp::Same
            }
        } else if self.ms != now.ms || self.cues != now.cues {
            Cmp::Changed
        } else {
            Cmp::Same
        }
    }
}

/// Der Voice-Track-Teil eines Playlist-Eintrags.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceTrack {
    #[serde(flatten)]
    pub params: VoiceParams,
    /// Titel davor (A) und danach (B), wie sie beim Einpassen waren.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<Neighbor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<Neighbor>,
    /// Ein Nachbar hat sich geändert: der Voice-Track wird nicht gesendet, bis bestätigt.
    #[serde(default, skip_serializing_if = "is_false")]
    pub check: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl VoiceTrack {
    pub fn new(params: VoiceParams) -> Self {
        Self {
            params: params.sanitized(),
            before: None,
            after: None,
            check: false,
        }
    }
}

/// Vorschlag für den Beginn der Stimme.
///
/// Mit Ramp-Marke in B (Ende des Intros, `ramp_rel` Sekunden nach dem Beginn von B) endet die
/// Stimme genau dort; ohne Marke 2 s nach dem Beginn von B. Die Stimme beginnt nie weiter als
/// [`MAX_OFFSET`] vor dem Übergang.
pub fn suggest_offset(voice_seconds: f64, ramp_rel: Option<f64>) -> f64 {
    let end = match ramp_rel {
        Some(r) if r > 0.0 => r,
        _ => 2.0,
    };
    (end - voice_seconds).clamp(-MAX_OFFSET, MAX_OFFSET)
}

impl RadioShow {
    /// Letzter Titel (kein Voice-Track) vor `i`.
    pub fn prev_music(&self, i: usize) -> Option<usize> {
        (0..i.min(self.playlist.len()))
            .rev()
            .find(|&k| self.playlist[k].voice.is_none())
    }

    /// Nächster Titel (kein Voice-Track) nach `i`.
    pub fn next_music(&self, i: usize) -> Option<usize> {
        (i + 1..self.playlist.len()).find(|&k| self.playlist[k].voice.is_none())
    }

    /// Erster Titel (kein Voice-Track) ab `i`.
    pub fn first_music_from(&self, i: usize) -> Option<usize> {
        (i..self.playlist.len()).find(|&k| self.playlist[k].voice.is_none())
    }

    /// Gleicht alle Voice-Tracks mit ihren Nachbarn ab. Neue Nachbarn (der Voice-Track hatte
    /// noch keinen) und nachgetragene Messwerte werden stillschweigend übernommen; ein anderer
    /// Titel, eine andere Länge oder andere Marken setzen den Voice-Track auf „prüfen“.
    /// Gibt die Indizes zurück, die dabei neu auf „prüfen“ gegangen sind.
    pub fn relink_voices(&mut self) -> Vec<usize> {
        let mut flagged = Vec::new();
        for i in 0..self.playlist.len() {
            if self.playlist[i].voice.is_none() {
                continue;
            }
            let now = [
                self.prev_music(i).map(|k| Neighbor::of(&self.playlist[k])),
                self.next_music(i).map(|k| Neighbor::of(&self.playlist[k])),
            ];
            let Some(v) = self.playlist[i].voice.as_mut() else {
                continue;
            };
            let mut changed = false;
            for (slot, now) in [&mut v.before, &mut v.after].into_iter().zip(now) {
                match (slot.as_ref(), now) {
                    (None, Some(n)) => *slot = Some(n),
                    (Some(_), None) => changed = true,
                    (Some(old), Some(n)) => match old.compare(&n) {
                        Cmp::Same => {}
                        Cmp::Rebind => *slot = Some(n),
                        Cmp::Changed => changed = true,
                    },
                    (None, None) => {}
                }
            }
            if changed && !v.check {
                v.check = true;
                flagged.push(i);
            }
        }
        flagged
    }

    /// Bestätigt einen Voice-Track auf „prüfen“: die jetzigen Nachbarn gelten als geprüft.
    pub fn confirm_voice(&mut self, i: usize) -> Result<(), String> {
        let before = self.prev_music(i).map(|k| Neighbor::of(&self.playlist[k]));
        let after = self.next_music(i).map(|k| Neighbor::of(&self.playlist[k]));
        let v = self
            .playlist
            .get_mut(i)
            .and_then(|e| e.voice.as_mut())
            .ok_or_else(|| format!("Eintrag {} ist kein Voice-Track", i + 1))?;
        v.before = before;
        v.after = after;
        v.check = false;
        Ok(())
    }

    /// Setzt die Werte von Voice-Track `i` (geprüft und begrenzt). Das Bestätigen bleibt
    /// davon unberührt: wer die Werte ändert, hat sich die Nachbarn angesehen, deshalb
    /// bestätigt der Editor ausdrücklich ([`Self::confirm_voice`]).
    pub fn set_voice_params(&mut self, i: usize, p: VoiceParams) -> Result<VoiceParams, String> {
        if [p.offset, p.gain_db, p.duck_db, p.attack, p.release]
            .iter()
            .any(|v| !v.is_finite())
        {
            return Err("Voice-Track: Werte müssen Zahlen sein".into());
        }
        let v = self
            .playlist
            .get_mut(i)
            .and_then(|e| e.voice.as_mut())
            .ok_or_else(|| format!("Eintrag {} ist kein Voice-Track", i + 1))?;
        Ok(std::mem::replace(&mut v.params, p.sanitized()))
    }

    /// Legt einen Voice-Track für die Sprachdatei `path` bei `at` in die Playlist (an den
    /// Rest angehängt, wenn `at` hinter dem Ende liegt) und bindet ihn an die jetzigen
    /// Nachbarn. `seconds` ist die Länge der Datei, `lufs`/`peak_db` ihre Messung.
    /// Gibt den Index zurück.
    pub fn insert_voice(&mut self, at: usize, mut entry: PlaylistEntry) -> usize {
        let at = at.min(self.playlist.len());
        let seconds = entry.seconds.unwrap_or(0.0);
        let mut v = VoiceTrack::new(VoiceParams::default());
        // Vorschlag: bis zur Ramp-Marke des folgenden Titels, falls schon einer da ist.
        let b = self.playlist[at..].iter().find(|e| e.voice.is_none());
        let ramp_rel = b.and_then(|b| b.cues.ramp.map(|r| r - b.cues.start()));
        v.params.offset = suggest_offset(seconds, ramp_rel);
        entry.voice = Some(v);
        self.playlist.insert(at, entry);
        self.relink_voices();
        at
    }

    /// Eine Vorhör-Sendung aus A, Voice-Track `i` und B: nur das Stück um den Übergang, mit
    /// denselben Einstellungen wie die Sendung. Sie läuft durch denselben Mixer wie live
    /// ([`crate::control::render`]), nur offline.
    pub fn voice_preview(&self, i: usize) -> Result<RadioShow, String> {
        let ve = self
            .playlist
            .get(i)
            .filter(|e| e.voice.is_some())
            .ok_or_else(|| format!("Eintrag {} ist kein Voice-Track", i + 1))?;
        let v = ve.voice.as_ref().map(|v| v.params).unwrap_or_default();
        let a = self
            .prev_music(i)
            .map(|k| &self.playlist[k])
            .ok_or("Vor dem Voice-Track fehlt ein Titel")?;
        let b = self
            .next_music(i)
            .map(|k| &self.playlist[k])
            .ok_or("Nach dem Voice-Track fehlt ein Titel")?;
        let (Some(sa), Some(sb), Some(sv)) = (a.seconds, b.seconds, ve.seconds) else {
            return Err("Titel noch nicht gemessen, kurz warten".into());
        };
        let xf = self.crossfade_entries(a, b);
        let a_end = a.cues.end(sa);
        // Wo in A die Überblendung beginnt.
        let a_x = (a_end - xf).max(a.cues.start());
        let before = (-v.offset).max(0.0) + v.attack + 4.0;
        let a_from = (a_x - before).max(a.cues.start()).min(a_end - 0.05);
        let mut ca = a.cues;
        ca.cue_in = Some(a_from);
        ca.ramp = None;
        ca.hook = None;
        ca.fade_out = ca.fade_out.filter(|f| *f > a_from);
        let b_start = b.cues.start();
        let b_end = b.cues.end(sb);
        let after = (v.offset + sv + v.release + 6.0).max(6.0);
        let b_to = (b_start + after).min(b_end).max(b_start + 0.05);
        let mut cb = b.cues;
        cb.cue_out = Some(b_to);
        cb.hook = None;
        cb.fade_out = cb.fade_out.filter(|f| *f < b_to);
        let mut ea = a.clone();
        ea.cues = ca;
        let mut eb = b.clone();
        eb.cues = cb;
        let mut ev = ve.clone();
        if let Some(x) = ev.voice.as_mut() {
            x.before = None;
            x.after = None;
            x.check = false;
        }
        let mut show = self.clone();
        show.playlist = vec![ea, ev, eb];
        show.carts = vec![None; crate::CARTS];
        show.relink_voices();
        Ok(show)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duck_envelope_has_exact_depth_and_smooth_edges() {
        let (len, att, rel, depth) = (1000, 100, 200, 0.25f32);
        assert_eq!(duck_gain(-101, len, att, rel, depth), 1.0);
        assert!((duck_gain(-100, len, att, rel, depth) - 1.0).abs() < 1e-6);
        assert!((duck_gain(0, len, att, rel, depth) - depth).abs() < 1e-6);
        assert!((duck_gain(999, len, att, rel, depth) - depth).abs() < 1e-6);
        assert_eq!(duck_gain(1000 + 200, len, att, rel, depth), 1.0);
        // Monoton auf den Rampen, nie unter der Tiefe, nie über 1.
        let mut last = 1.0;
        for p in -100..0 {
            let g = duck_gain(p, len, att, rel, depth);
            assert!(g <= last + 1e-6 && (depth..=1.0).contains(&g), "{p}: {g}");
            last = g;
        }
        let mut last = depth;
        for p in 1000..1200 {
            let g = duck_gain(p, len, att, rel, depth);
            assert!(g >= last - 1e-6 && (depth..=1.0).contains(&g), "{p}: {g}");
            last = g;
        }
        // Ohne Rampen: Sprung ohne Teilung durch null.
        assert_eq!(duck_gain(0, 10, 0, 0, 0.5), 0.5);
        assert_eq!(duck_gain(10, 10, 0, 0, 0.5), 1.0);
        assert_eq!(duck_gain(-1, 10, 0, 0, 0.5), 1.0);
    }

    #[test]
    fn edges_fade_the_voice() {
        assert!(edge_gain(0, 1000, 10) < 0.2);
        assert_eq!(edge_gain(500, 1000, 10), 1.0);
        assert!(edge_gain(999, 1000, 10) < 0.2);
        assert_eq!(edge_gain(0, 1000, 0), 1.0);
    }

    #[test]
    fn params_are_clamped_and_depth_matches_db() {
        let p = VoiceParams {
            offset: 1e9,
            gain_db: f64::NAN,
            duck_db: 5.0,
            attack: -1.0,
            release: 99.0,
        }
        .sanitized();
        assert_eq!(p.offset, MAX_OFFSET);
        assert_eq!(p.gain_db, 0.0);
        assert_eq!(p.duck_db, 0.0);
        assert_eq!(p.attack, 0.0);
        assert_eq!(p.release, MAX_RAMP);
        let q = VoiceParams {
            duck_db: -12.0,
            ..VoiceParams::default()
        };
        assert!((q.depth() - 0.2512).abs() < 1e-3);
        let f = q.frames(1000, 3000);
        assert_eq!((f.len, f.attack, f.release), (3000, 400, 800));
    }

    #[test]
    fn curve_matches_the_envelope() {
        let p = VoiceParams {
            offset: -2.0,
            duck_db: -20.0,
            attack: 0.5,
            release: 1.0,
            gain_db: 0.0,
        };
        let c = duck_curve(&p, 4.0, -4.0, 6.0, 0.01);
        let at = |t: f64| {
            c.iter()
                .min_by_key(|(x, _)| ((x - t).abs() * 1e4) as i64)
                .unwrap()
                .1
        };
        assert_eq!(at(-3.0), 1.0);
        assert!((at(-1.0) - 0.1).abs() < 1e-3, "{}", at(-1.0));
        assert!((at(1.9) - 0.1).abs() < 1e-3);
        assert!((at(3.5) - 1.0).abs() < 0.2 && at(3.5) > 0.1);
        assert_eq!(at(5.5), 1.0);
    }

    #[test]
    fn suggestion_ends_at_the_ramp() {
        assert!((suggest_offset(6.0, Some(8.0)) - 2.0).abs() < 1e-9);
        assert!((suggest_offset(10.0, Some(8.0)) + 2.0).abs() < 1e-9);
        assert!((suggest_offset(6.0, None) + 4.0).abs() < 1e-9);
        assert_eq!(suggest_offset(500.0, None), -MAX_OFFSET);
    }

    fn music(path: &str, secs: f64) -> PlaylistEntry {
        let mut e = PlaylistEntry::new(path);
        e.seconds = Some(secs);
        e
    }

    fn voice(path: &str, secs: f64) -> PlaylistEntry {
        let mut e = music(path, secs);
        e.voice = Some(VoiceTrack::new(VoiceParams::default()));
        e
    }

    fn show3() -> RadioShow {
        let mut s = RadioShow {
            playlist: vec![
                music("a.wav", 100.0),
                voice("v.wav", 5.0),
                music("b.wav", 120.0),
            ],
            ..RadioShow::default()
        };
        s.relink_voices();
        s
    }

    #[test]
    fn swapping_a_neighbour_sets_check_and_confirm_clears_it() {
        let mut s = show3();
        let v = s.playlist[1].voice.as_ref().unwrap();
        assert!(!v.check);
        assert_eq!(v.before.as_ref().unwrap().path, PathBuf::from("a.wav"));
        assert_eq!(v.after.as_ref().unwrap().path, PathBuf::from("b.wav"));
        assert!(s.relink_voices().is_empty(), "nichts geändert");

        s.playlist[2] = music("c.wav", 90.0);
        assert_eq!(s.relink_voices(), vec![1]);
        assert!(s.playlist[1].voice.as_ref().unwrap().check);
        assert!(s.relink_voices().is_empty(), "nur einmal gemeldet");
        s.confirm_voice(1).unwrap();
        let v = s.playlist[1].voice.as_ref().unwrap();
        assert!(!v.check);
        assert_eq!(v.after.as_ref().unwrap().path, PathBuf::from("c.wav"));
        assert!(s.confirm_voice(0).is_err());
    }

    #[test]
    fn changed_length_cues_or_removed_neighbour_set_check() {
        let mut s = show3();
        s.playlist[0].seconds = Some(101.0);
        assert_eq!(s.relink_voices(), vec![1]);

        let mut s = show3();
        s.playlist[0].cues.cue_out = Some(95.0);
        assert_eq!(s.relink_voices(), vec![1]);

        let mut s = show3();
        s.playlist.remove(2);
        assert_eq!(s.relink_voices(), vec![1]);
    }

    #[test]
    fn first_measurement_and_new_neighbours_do_not_flag() {
        let mut s = RadioShow::default();
        let mut a = PlaylistEntry::new("a.wav");
        a.seconds = None;
        s.playlist = vec![a, voice("v.wav", 5.0)];
        s.relink_voices();
        // Der Voice-Track hat noch keinen Nachfolger; er kommt später.
        s.playlist.push(music("b.wav", 100.0));
        assert!(s.relink_voices().is_empty());
        // A wird gemessen (mit Auto-Cues): kein Alarm.
        s.playlist[0].seconds = Some(200.0);
        s.playlist[0].cues.cue_out = Some(190.0);
        assert!(s.relink_voices().is_empty());
        assert_eq!(
            s.playlist[1]
                .voice
                .as_ref()
                .unwrap()
                .before
                .as_ref()
                .unwrap()
                .ms,
            Some(200_000)
        );
        // Danach zählt jede Änderung.
        s.playlist[0].cues.cue_out = Some(150.0);
        assert_eq!(s.relink_voices(), vec![1]);
    }

    #[test]
    fn music_navigation_skips_voice_entries() {
        let s = show3();
        assert_eq!(s.prev_music(1), Some(0));
        assert_eq!(s.next_music(1), Some(2));
        assert_eq!(s.prev_music(0), None);
        assert_eq!(s.first_music_from(1), Some(2));
        assert_eq!(s.first_music_from(3), None);
        // Gesamtlänge: Voice-Tracks zählen nicht, sie liegen über der Musik.
        let mut s = s;
        s.crossfade_seconds = 3.0;
        assert!((s.total_seconds_from(0) - (100.0 + 120.0 - 3.0)).abs() < 1e-9);
        assert!((s.total_seconds_from(1) - 120.0).abs() < 1e-9);
    }

    #[test]
    fn insert_voice_binds_neighbours_and_suggests_an_offset() {
        let mut s = RadioShow {
            playlist: vec![music("a.wav", 100.0), {
                let mut b = music("b.wav", 100.0);
                b.cues.ramp = Some(9.0);
                b
            }],
            ..RadioShow::default()
        };
        let i = s.insert_voice(1, voice("v.wav", 6.0));
        assert_eq!(i, 1);
        let v = s.playlist[1].voice.as_ref().unwrap();
        assert!(
            (v.params.offset - 3.0).abs() < 1e-9,
            "endet an der Ramp-Marke"
        );
        assert!(v.before.is_some() && v.after.is_some() && !v.check);
    }

    #[test]
    fn preview_keeps_only_the_transition() {
        let mut s = show3();
        s.playlist[1].voice.as_mut().unwrap().params.offset = -3.0;
        let p = s.voice_preview(1).unwrap();
        assert_eq!(p.playlist.len(), 3);
        let a = &p.playlist[0];
        assert!(
            a.cues.cue_in.unwrap() > 80.0,
            "nur das Ende von A: {:?}",
            a.cues
        );
        assert!(
            p.playlist[2].cues.cue_out.unwrap() < 30.0,
            "nur der Anfang von B"
        );
        assert!(s.voice_preview(0).is_err());
        let mut u = s.clone();
        u.playlist[0].seconds = None;
        assert!(u.voice_preview(1).is_err());
    }

    #[test]
    fn json_roundtrip_keeps_voice_and_old_files_still_load() {
        let mut s = show3();
        s.playlist[1].voice.as_mut().unwrap().params.offset = -1.5;
        let text = serde_json::to_string(&s).unwrap();
        assert_eq!(RadioShow::from_json(&text).unwrap(), s);
        // Dateien aus 0.24: kein Voice-Feld.
        let old = r#"{"version":1,"playlist":[{"path":"x.wav"}]}"#;
        assert!(
            RadioShow::from_json(old).unwrap().playlist[0]
                .voice
                .is_none()
        );
        // Feindliche Werte werden begrenzt.
        let evil = r#"{"version":1,"playlist":[{"path":"v.wav","voice":{"offset":1e300,"duck_db":-9999}}]}"#;
        let t = RadioShow::from_json(evil).unwrap();
        let p = t.playlist[0].voice.as_ref().unwrap().params;
        assert_eq!((p.offset, p.duck_db), (MAX_OFFSET, -40.0));
    }
}
