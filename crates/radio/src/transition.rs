//! Übergänge nach der Outro/Intro-Logik des AutoDJ von Mixxx (Paket 29).
//!
//! Herkunft: `AutoDJProcessor::calculateTransition` und `useFixedFadeTime` in
//! `src/library/autodj/autodjprocessor.cpp` von Mixxx (GPL-2.0-or-later), eigene Umsetzung in
//! Rust, Randfälle aus `src/test/autodjprocessor_test.cpp`. Siehe `docs/herkunft.md`.
//!
//! Alles hier ist reine Rechnung in Sekunden (Control-Thread, nie im Audio-Thread). Der Mixer
//! bekommt am Ende nur die Länge der Überblendung in Frames.
//!
//! Begriffe wie bei Mixxx: Das **Outro** des laufenden Titels ist der Bereich, in dem er
//! ausklingt, das **Intro** des nächsten der Bereich, in dem noch kein Gesang einsetzt. Die
//! Überblendung legt Outro über Intro.
//!
//! Zuordnung zu den Cue-Punkten von WOA ([`crate::cues::Cues`]): Intro-Anfang = Cue-In, Intro-Ende
//! = Ramp, Outro-Anfang = Fade-Out-Marker, Outro-Ende = Cue-Out. Erster und letzter Klang sind
//! Cue-In und Cue-Out (dahinter ist Stille, die schon abgeschnitten ist).

use crate::cues::Cues;

/// Kürzeste Titellänge, mit der gerechnet wird (Mixxx `kMinimumTrackDurationSec`).
pub const MIN_TRACK_SEC: f64 = 0.2;

/// Übergangsart (Mixxx `TransitionMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Kürzeres von Outro und Intro, beide laufen voll; endet am Outro-Ende. Vorgabe in WOA.
    FullIntroOutro,
    /// Beginnt am Outro-Anfang, dauert so lang wie das kürzere von Outro und Intro; ein
    /// längeres Outro wird abgeschnitten. Der Mixer von WOA blendet immer bis zum Ende des
    /// alten Titels, deshalb ist diese Art bisher nur als Rechnung da.
    FadeAtOutroStart,
    /// Feste Zeit am Ende des ganzen Titels.
    FixedFullTrack,
    /// Feste Zeit am Ende des letzten Klangs.
    FixedSkipSilence,
}

/// Marken eines Titels in Sekunden ab Dateianfang. `None` heißt nicht gesetzt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Marks {
    pub duration: f64,
    pub first_sound: f64,
    pub last_sound: f64,
    pub intro_start: Option<f64>,
    pub intro_end: Option<f64>,
    pub outro_start: Option<f64>,
    pub outro_end: Option<f64>,
}

impl Marks {
    /// Titel ohne jede Marke.
    pub fn plain(duration: f64) -> Self {
        Self {
            duration,
            first_sound: 0.0,
            last_sound: duration,
            intro_start: None,
            intro_end: None,
            outro_start: None,
            outro_end: None,
        }
    }

    /// Aus den Cue-Punkten von WOA (siehe Modulkopf).
    pub fn from_cues(cues: &Cues, duration: f64) -> Self {
        let first = cues.start().min(duration);
        let last = cues.end(duration);
        Self {
            duration,
            first_sound: first,
            last_sound: last,
            intro_start: Some(first),
            intro_end: cues.ramp,
            outro_start: cues.fade_out,
            // Mit Fade-Out-Marker endet das Outro am Cue-Out (oder am Titelende).
            outro_end: if cues.fade_out.is_some() {
                Some(last)
            } else {
                cues.cue_out
            },
        }
    }

    fn valid(&self, v: Option<f64>) -> Option<f64> {
        v.filter(|x| x.is_finite() && *x >= 0.0 && *x <= self.duration)
    }

    fn intro_start_sec(&self, tt: f64) -> f64 {
        match self.valid(self.intro_start) {
            Some(s) => s,
            None => match self.valid(self.intro_end) {
                None => self.first_sound,
                Some(e) if tt >= 0.0 => e - tt,
                Some(e) => e,
            },
        }
    }

    fn intro_end_sec(&self, tt: f64) -> f64 {
        // Ohne Intro-Ende ist das Intro null lang.
        self.valid(self.intro_end)
            .unwrap_or_else(|| self.intro_start_sec(tt))
    }

    fn outro_start_sec(&self, tt: f64) -> f64 {
        // Ohne Outro-Anfang ist das Outro null lang.
        self.valid(self.outro_start)
            .unwrap_or_else(|| self.outro_end_sec(tt))
    }

    fn outro_end_sec(&self, tt: f64) -> f64 {
        match self.valid(self.outro_end) {
            Some(e) => e,
            None => match self.valid(self.outro_start) {
                None => self.last_sound,
                Some(s) => {
                    // Outro-Anfang gesetzt, Ende nicht: Anfang plus Übergangszeit, höchstens
                    // der letzte Klang.
                    if tt >= 0.0 && self.last_sound > s {
                        let from_time = s + tt;
                        if from_time < self.last_sound {
                            from_time
                        } else {
                            self.last_sound
                        }
                    } else {
                        s
                    }
                }
            },
        }
    }
}

/// Ergebnis der Übergangsrechnung, Zeiten in Sekunden ab Dateianfang des jeweiligen Titels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plan {
    /// Hier beginnt der laufende Titel auszublenden.
    pub from_fade_begin: f64,
    /// Hier ist er ausgeblendet.
    pub from_fade_end: f64,
    /// Hier beginnt der nächste Titel.
    pub to_start: f64,
    /// Beginn und Ende des Übergangs, der auf diesen folgt (zur Prüfung, ob der nächste Titel
    /// lang genug ist).
    pub to_fade_begin: f64,
    pub to_fade_end: f64,
    /// Die Länge kam aus der festen Übergangszeit (kein Outro und kein Intro gesetzt).
    pub fixed: bool,
}

impl Plan {
    /// Länge der Überblendung in Sekunden.
    pub fn overlap(&self) -> f64 {
        (self.from_fade_end - self.from_fade_begin).max(0.0)
    }
}

/// Was bei der Rechnung herauskommt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Outcome {
    Plan(Plan),
    /// Der laufende Titel ist zu kurz: der nächste beginnt sofort, harter Schnitt.
    JumpCut,
    /// Der nächste Titel hat keine Länge (Auswurf): nichts zu tun.
    Ignore,
    /// Der nächste Titel ist zu kurz: überspringen und den folgenden nehmen.
    SkipNext,
}

/// Rechnet den Übergang von `from` (an Position `from_pos`) nach `to` (an Position `to_pos`).
///
/// - `tt`: feste Übergangszeit (Mixxx `m_transitionTime`), im Radio die Überblendzeit.
/// - `seek_to_start`: den nächsten Titel an seinen Startpunkt setzen (Mixxx `seekToStartPoint`).
pub fn calculate(
    mode: Mode,
    tt: f64,
    from: &Marks,
    from_pos: f64,
    to: &Marks,
    to_pos: f64,
    seek_to_start: bool,
) -> Outcome {
    let from_dur = from.duration;
    let to_dur = to.duration;
    if from_dur.is_nan() || from_dur < MIN_TRACK_SEC {
        return Outcome::JumpCut;
    }
    if to_dur == 0.0 {
        return Outcome::Ignore;
    }
    if to_dur.is_nan() || to_dur < MIN_TRACK_SEC {
        return Outcome::SkipNext;
    }

    let mut outro_end = from.outro_end_sec(tt).min(from_dur);
    let mut outro_start = from.outro_start_sec(tt);
    if from_pos > outro_start {
        // Schon hinter dem Outro-Anfang, etwa weil der AutoDJ eben erst eingeschaltet wurde.
        outro_start = from_pos;
        if from_pos > outro_end {
            outro_end = (outro_start + tt.abs()).min(from_dur);
        }
    }
    let outro_len = outro_end - outro_start;

    // Der Übergang nach dem nächsten: Ende und Anfang des Outros des nächsten Titels.
    let mut to_fade_end = to.outro_end_sec(tt);
    let mut to_outro_start = to.outro_start_sec(tt);
    if to_fade_end == to_outro_start {
        // Kein Outro gesetzt: die Übergangszeit.
        to_outro_start -= tt;
    }
    let mut to_fade_begin = to_outro_start;

    let mut to_start = to_pos;
    let intro_start = to.intro_start_sec(tt);
    let intro_end = to.intro_end_sec(tt);
    if seek_to_start || to_pos >= to_fade_begin {
        // Der Titel steht schon hinter seinem eigenen Übergang (Nutzer hat gesprungen):
        // neu an den Intro-Anfang setzen.
        to_start = intro_start;
    }

    let mut intro_len = 0.0;
    if to_start < intro_end && intro_start < intro_end {
        intro_len = intro_end - to_start;
        // Ein viel zu großer Rücksprung wird ignoriert.
        if intro_len > (intro_end - intro_start) * 2.0
            && intro_len > (intro_end - intro_start) + tt
            && intro_len > outro_len
        {
            intro_len = 0.0;
        }
    }

    let mut plan = Plan {
        from_fade_begin: 0.0,
        from_fade_end: 0.0,
        to_start,
        to_fade_begin,
        to_fade_end,
        fixed: false,
    };

    match mode {
        Mode::FullIntroOutro => {
            let mut len = intro_len;
            if outro_len > 0.0 && (len <= 0.0 || len > outro_len) {
                len = outro_len;
            }
            if len > 0.0 {
                if to_start + len > to_fade_begin {
                    // Intro endet, bevor das nächste Outro beginnt.
                    len = to_fade_begin - to_start;
                    if len <= 0.0 {
                        len = 1.0;
                    }
                }
                plan.from_fade_begin = outro_end - len;
                plan.from_fade_end = outro_end;
                plan.to_start = to_start;
            } else {
                fixed_fade(
                    &mut plan,
                    tt,
                    to,
                    from_pos,
                    outro_end,
                    to_start,
                    &mut to_fade_begin,
                    to_fade_end,
                );
            }
        }
        Mode::FadeAtOutroStart => {
            let mut len = outro_len;
            if len > 0.0 {
                if intro_len > 0.0 && outro_len > intro_len {
                    len = intro_len;
                }
                if to_start + len > to_fade_begin {
                    len = to_fade_begin - to_start;
                    if len <= 0.0 {
                        len = 1.0;
                    }
                }
                plan.from_fade_begin = outro_start;
                plan.from_fade_end = outro_start + len;
                plan.to_start = to_start;
            } else if intro_len > 0.0 {
                len = intro_len;
                plan.from_fade_begin = outro_end - len;
                plan.from_fade_end = outro_end;
                plan.to_start = to_start;
            } else {
                fixed_fade(
                    &mut plan,
                    tt,
                    to,
                    from_pos,
                    outro_end,
                    to_start,
                    &mut to_fade_begin,
                    to_fade_end,
                );
            }
        }
        Mode::FixedSkipSilence => {
            to_fade_begin = to.last_sound;
            let start = if seek_to_start || to_pos >= to_fade_begin {
                to.first_sound
            } else {
                to_pos
            };
            fixed_fade(
                &mut plan,
                tt,
                to,
                from_pos,
                from.last_sound,
                start,
                &mut to_fade_begin,
                to_fade_end,
            );
        }
        Mode::FixedFullTrack => {
            to_fade_begin = to_dur;
            let start = if seek_to_start || to_pos >= to_fade_begin {
                0.0
            } else {
                to_pos
            };
            fixed_fade(
                &mut plan,
                tt,
                to,
                from_pos,
                from_dur,
                start,
                &mut to_fade_begin,
                to_fade_end,
            );
        }
    }
    // `to_fade_end` kann in `fixed_fade` unverändert bleiben.
    to_fade_end = to_fade_end.max(0.0);
    plan.to_fade_begin = to_fade_begin;
    plan.to_fade_end = to_fade_end;
    if plan.from_fade_begin > from_dur {
        plan.from_fade_begin = from_dur;
    }
    Outcome::Plan(plan)
}

/// Mixxx `useFixedFadeTime`: feste Zeit, die vor dem eigenen Übergang des nächsten Titels
/// endet.
#[allow(clippy::too_many_arguments)]
fn fixed_fade(
    plan: &mut Plan,
    tt: f64,
    to: &Marks,
    from_second: f64,
    fade_end: f64,
    to_start_in: f64,
    to_fade_begin: &mut f64,
    to_fade_end: f64,
) {
    plan.fixed = true;
    let mut to_start = to_start_in;
    if tt > 0.0 {
        // Der Übergang muss enden, bevor der nächste beginnt.
        let mut to_outro_start = *to_fade_begin;
        if *to_fade_begin >= to_fade_end {
            // Kein Outro: der nächste Titel nimmt ebenfalls die Übergangszeit.
            to_outro_start -= tt;
        }
        if to_outro_start <= to_start + MIN_TRACK_SEC {
            let mut end = to.outro_end_sec(tt);
            if end <= to_start + MIN_TRACK_SEC {
                end = to.duration;
                if end <= to_start + MIN_TRACK_SEC {
                    // Letzter Ausweg: den Startpunkt zurückschieben.
                    to_start = end - MIN_TRACK_SEC;
                }
            }
            // Die verbleibende Zeit wird geteilt.
            to_outro_start = (end - to_start) / 2.0 + to_start;
        }
        let mut transition = (to_outro_start - to_start).min(tt);
        if transition < MIN_TRACK_SEC / 2.0 {
            transition = MIN_TRACK_SEC / 2.0;
        }
        plan.from_fade_begin = (fade_end - transition).max(from_second);
        plan.from_fade_end = fade_end;
        plan.to_start = to_start;
    } else {
        plan.from_fade_begin = fade_end;
        plan.from_fade_end = fade_end;
        plan.to_start = to_start + tt;
    }
}

/// Länge der Überblendung zwischen zwei Titeln der Playlist in Sekunden.
///
/// `global` ist die eingestellte Überblendzeit. Der Rückgabewert ist nie länger als der alte
/// Titel und höchstens die halbe Länge des neuen, damit der neue Titel nie schon beim Einblenden
/// endet (wie bisher im Mixer).
pub fn overlap_seconds(
    global: f64,
    from: &Cues,
    from_seconds: f64,
    to: &Cues,
    to_seconds: f64,
) -> f64 {
    let (la, lb) = (from.play_seconds(from_seconds), to.play_seconds(to_seconds));
    if la <= 0.0 || lb <= 0.0 {
        return 0.0;
    }
    let (mut fm, mut tm) = (
        Marks::from_cues(from, from_seconds),
        Marks::from_cues(to, to_seconds),
    );
    // Ohne gesetztes Cue-In/-Out beginnt der erste Klang am Dateianfang.
    fm.first_sound = from.start();
    tm.first_sound = to.start();
    match calculate(
        Mode::FullIntroOutro,
        global.max(0.0),
        &fm,
        0.0,
        &tm,
        0.0,
        true,
    ) {
        Outcome::Plan(p) => {
            let o = p.overlap();
            let o = if p.fixed { o.min(la / 2.0) } else { o };
            o.min(la).min(lb / 2.0).max(0.0)
        }
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(dur: f64, intro: Option<(f64, f64)>, outro: Option<(f64, f64)>) -> Marks {
        let mut m = Marks::plain(dur);
        if let Some((a, b)) = intro {
            m.intro_start = Some(a);
            m.intro_end = Some(b);
        }
        if let Some((a, b)) = outro {
            m.outro_start = Some(a);
            m.outro_end = Some(b);
        }
        m
    }

    fn plan(mode: Mode, tt: f64, from: &Marks, to: &Marks) -> Plan {
        match calculate(mode, tt, from, 0.0, to, 0.0, true) {
            Outcome::Plan(p) => p,
            o => panic!("kein Plan: {o:?}"),
        }
    }

    // Die vier Fälle aus `autodjprocessor_test.cpp` (Titel je 100 s lang).

    #[test]
    fn mixxx_full_intro_outro_longer_intro() {
        // Outro 60-70, Intro 10-40: das kürzere (Outro, 10 s) gilt.
        let from = track(100.0, None, Some((60.0, 70.0)));
        let to = track(100.0, Some((10.0, 40.0)), None);
        let p = plan(Mode::FullIntroOutro, 3.0, &from, &to);
        assert_eq!(
            (p.from_fade_begin, p.from_fade_end, p.to_start),
            (60.0, 70.0, 10.0)
        );
    }

    #[test]
    fn mixxx_full_intro_outro_longer_outro() {
        // Outro 70-90, Intro 10-20: das kürzere (Intro, 10 s) gilt, endet am Outro-Ende.
        let from = track(100.0, None, Some((70.0, 90.0)));
        let to = track(100.0, Some((10.0, 20.0)), None);
        let p = plan(Mode::FullIntroOutro, 3.0, &from, &to);
        assert_eq!(
            (p.from_fade_begin, p.from_fade_end, p.to_start),
            (80.0, 90.0, 10.0)
        );
    }

    #[test]
    fn mixxx_fade_at_outro_start_longer_intro() {
        // Outro 80-90, Intro 10-30: Beginn am Outro-Anfang, 10 s.
        let from = track(100.0, None, Some((80.0, 90.0)));
        let to = track(100.0, Some((10.0, 30.0)), None);
        let p = plan(Mode::FadeAtOutroStart, 3.0, &from, &to);
        assert_eq!(
            (p.from_fade_begin, p.from_fade_end, p.to_start),
            (80.0, 90.0, 10.0)
        );
    }

    #[test]
    fn mixxx_fade_at_outro_start_longer_outro() {
        // Outro 60-80, Intro 10-20: das Outro wird nach 10 s abgeschnitten.
        let from = track(100.0, None, Some((60.0, 80.0)));
        let to = track(100.0, Some((10.0, 20.0)), None);
        let p = plan(Mode::FadeAtOutroStart, 3.0, &from, &to);
        assert_eq!(
            (p.from_fade_begin, p.from_fade_end, p.to_start),
            (60.0, 70.0, 10.0)
        );
    }

    #[test]
    fn only_one_side_marked_uses_that_side() {
        // Nur Outro 70-85: 15 s.
        let from = track(100.0, None, Some((70.0, 85.0)));
        let to = Marks::plain(100.0);
        let p = plan(Mode::FullIntroOutro, 3.0, &from, &to);
        assert_eq!((p.from_fade_begin, p.from_fade_end), (70.0, 85.0));
        // Nur Intro 0-6: 6 s, endet am Ende des alten Titels.
        let from = Marks::plain(100.0);
        let to = track(100.0, Some((0.0, 6.0)), None);
        let p = plan(Mode::FullIntroOutro, 3.0, &from, &to);
        assert_eq!(
            (p.from_fade_begin, p.from_fade_end, p.to_start),
            (94.0, 100.0, 0.0)
        );
    }

    #[test]
    fn nothing_marked_falls_back_to_fixed_time() {
        let from = Marks::plain(100.0);
        let to = Marks::plain(100.0);
        for mode in [
            Mode::FullIntroOutro,
            Mode::FixedFullTrack,
            Mode::FixedSkipSilence,
        ] {
            let p = plan(mode, 3.0, &from, &to);
            assert!(p.fixed, "{mode:?}");
            assert_eq!(
                (p.from_fade_begin, p.from_fade_end, p.to_start),
                (97.0, 100.0, 0.0)
            );
        }
    }

    #[test]
    fn zero_transition_time_is_a_hard_cut() {
        let from = Marks::plain(100.0);
        let to = Marks::plain(100.0);
        let p = plan(Mode::FixedFullTrack, 0.0, &from, &to);
        assert_eq!((p.from_fade_begin, p.from_fade_end), (100.0, 100.0));
        assert_eq!(p.overlap(), 0.0);
    }

    #[test]
    fn intro_is_cut_when_next_outro_starts_soon() {
        // Das Intro (0-40) würde über den eigenen Übergang des nächsten (Outro ab 20) hinaus
        // dauern: gekürzt auf 20 s.
        let from = track(100.0, None, Some((50.0, 100.0)));
        let to = track(100.0, Some((0.0, 40.0)), Some((20.0, 30.0)));
        let p = plan(Mode::FullIntroOutro, 3.0, &from, &to);
        assert_eq!(p.overlap(), 20.0);
    }

    #[test]
    fn next_track_shorter_than_transition_keeps_half_for_itself() {
        // Nächster Titel 4 s lang, Übergangszeit 10 s: höchstens bis zur Hälfte.
        let from = Marks::plain(100.0);
        let to = Marks::plain(4.0);
        let p = plan(Mode::FixedFullTrack, 10.0, &from, &to);
        assert_eq!(p.overlap(), 2.0);
    }

    #[test]
    fn too_short_tracks() {
        let ok = Marks::plain(100.0);
        assert_eq!(
            calculate(
                Mode::FullIntroOutro,
                3.0,
                &Marks::plain(0.1),
                0.0,
                &ok,
                0.0,
                true
            ),
            Outcome::JumpCut
        );
        assert_eq!(
            calculate(
                Mode::FullIntroOutro,
                3.0,
                &ok,
                0.0,
                &Marks::plain(0.0),
                0.0,
                true
            ),
            Outcome::Ignore
        );
        assert_eq!(
            calculate(
                Mode::FullIntroOutro,
                3.0,
                &ok,
                0.0,
                &Marks::plain(0.1),
                0.0,
                true
            ),
            Outcome::SkipNext
        );
    }

    #[test]
    fn already_past_outro_start_fades_from_here() {
        // AutoDJ eben eingeschaltet, Titel steht bei 75 s, Outro 60-70: Überblendung ab hier,
        // so lang wie die Übergangszeit, höchstens bis zum Ende.
        let from = track(100.0, None, Some((60.0, 70.0)));
        let to = Marks::plain(100.0);
        let Outcome::Plan(p) = calculate(Mode::FullIntroOutro, 3.0, &from, 75.0, &to, 0.0, true)
        else {
            panic!("Plan erwartet");
        };
        assert_eq!(p.from_fade_end, 78.0);
        assert_eq!(p.from_fade_begin, 75.0);
    }

    #[test]
    fn outro_end_without_outro_start_uses_last_sound() {
        let mut m = Marks::plain(100.0);
        m.last_sound = 90.0;
        assert_eq!(m.outro_end_sec(3.0), 90.0);
        assert_eq!(m.outro_start_sec(3.0), 90.0);
        // Nur Outro-Anfang: plus Übergangszeit, aber nicht hinter den letzten Klang.
        m.outro_start = Some(80.0);
        assert_eq!(m.outro_end_sec(3.0), 83.0);
        m.outro_start = Some(89.0);
        assert_eq!(m.outro_end_sec(3.0), 90.0);
    }

    #[test]
    fn marks_beyond_the_track_are_ignored() {
        let mut m = Marks::plain(100.0);
        m.outro_start = Some(150.0);
        m.outro_end = Some(160.0);
        assert_eq!(m.outro_end_sec(3.0), 100.0);
    }

    #[test]
    fn seek_back_too_far_ignores_the_intro() {
        // Nutzer hat den nächsten Titel weit hinter den Intro-Anfang gesetzt: Intro-Länge
        // wäre mehr als doppelt so groß wie das Intro selbst, also ignorieren.
        let from = Marks::plain(100.0);
        let to = track(200.0, Some((100.0, 105.0)), None);
        let Outcome::Plan(p) = calculate(Mode::FullIntroOutro, 3.0, &from, 0.0, &to, 20.0, false)
        else {
            panic!("Plan erwartet");
        };
        // Position 20 liegt vor dem Intro-Anfang (100): Länge 85 s > 2 * 5 s: ignoriert,
        // feste Zeit.
        assert!(p.fixed);
        assert_eq!(p.to_start, 20.0);
    }

    #[test]
    fn radio_cues_map_to_overlap() {
        // Fade-Out-Marker 6 s vor dem Ende ohne Ramp: wie bisher die Länge des Markers.
        let a = Cues {
            fade_out: Some(94.0),
            ..Cues::default()
        };
        let b = Cues::default();
        assert_eq!(overlap_seconds(3.0, &a, 100.0, &b, 100.0), 6.0);
        // Mit kurzem Intro des nächsten (Ramp 2 s) gewinnt das Intro.
        let b = Cues {
            ramp: Some(2.0),
            ..Cues::default()
        };
        assert_eq!(overlap_seconds(3.0, &a, 100.0, &b, 100.0), 2.0);
        // Ohne Marken die eingestellte Zeit, nie mehr als die halbe Länge.
        let n = Cues::default();
        assert_eq!(overlap_seconds(3.0, &n, 100.0, &n, 100.0), 3.0);
        assert_eq!(overlap_seconds(3.0, &n, 4.0, &n, 100.0), 2.0);
        // Ein 4-s-Titel behält 1 s für seinen eigenen Übergang (Mixxx: Ende minus Übergangszeit).
        assert_eq!(overlap_seconds(3.0, &n, 100.0, &n, 4.0), 1.0);
        assert_eq!(overlap_seconds(0.0, &n, 100.0, &n, 100.0), 0.0);
    }

    #[test]
    fn cue_in_and_cue_out_bound_the_overlap() {
        // Der Titel spielt nur 10-50: Überblendung 3 s vor Cue-Out, nicht vor dem Dateiende.
        let a = Cues {
            cue_in: Some(10.0),
            cue_out: Some(50.0),
            ..Cues::default()
        };
        assert_eq!(overlap_seconds(3.0, &a, 100.0, &a, 100.0), 3.0);
    }
}
