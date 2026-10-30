//! Paket 20: Schnittbefehle gegen den Offline-Render. Ein Impuls in der Datei zeigt, ob ein
//! Clip nach Verschieben und Kürzen auf dem Sample genau dort liegt, wo das Modell ihn setzt.

use std::sync::Arc;

use woa_engine::clip::render_clips;
use woa_model::{Clip, ClipId, Command, Session, TrackKind};

const RATE: u32 = 48_000;
/// 120 bpm: ein Beat = 0,5 s = 24 000 Samples.
const BEAT: usize = 24_000;

/// Impuls 1 Beat nach Dateianfang; Datei 4 Beats lang.
fn file() -> Arc<[f32]> {
    let mut v = vec![0.0f32; 4 * BEAT];
    v[BEAT] = 1.0;
    v.into()
}

fn first_impulse(session: &Session) -> Option<usize> {
    let f = file();
    let (l, _) =
        render_clips(session.project(), RATE, 16.0, &mut |_| Some(Arc::clone(&f))).expect("render");
    l.iter().position(|s| s.abs() > 0.01)
}

fn setup() -> (Session, ClipId) {
    let mut s = Session::new("edit");
    let t = s.new_track_id();
    s.apply(Command::add_track(t, "A", TrackKind::Audio))
        .expect("track");
    let c = s.new_clip_id();
    s.apply(Command::AddClip {
        track: t,
        clip: Clip::new(c, "c", 2.0, 4.0, Some("a.wav".into())),
        index: None,
    })
    .expect("clip");
    (s, c)
}

#[test]
fn move_shifts_the_render_by_exactly_the_distance() {
    let (mut s, c) = setup();
    let track = s.project().tracks[0].id;
    let before = first_impulse(&s).expect("sound");
    assert_eq!(
        before,
        3 * BEAT,
        "clip at beat 2 + impulse 1 beat into the file"
    );
    s.apply(Command::MoveClip {
        clip: c,
        to_track: track,
        start_beats: 6.0,
        index: None,
    })
    .expect("move");
    let after = first_impulse(&s).expect("sound");
    assert_eq!(after - before, 4 * BEAT, "moved by 4 beats");
    s.undo().expect("undo");
    assert_eq!(
        first_impulse(&s),
        Some(before),
        "undo restores the exact sample"
    );
}

#[test]
fn trimming_the_start_keeps_the_audio_in_place() {
    let (mut s, c) = setup();
    let track = s.project().tracks[0].id;
    let before = first_impulse(&s).expect("sound");
    // Anfang um 0,5 Beat kürzen: Start rückt, Versatz rückt gleich weit.
    s.apply(Command::MoveClip {
        clip: c,
        to_track: track,
        start_beats: 2.5,
        index: None,
    })
    .expect("move");
    s.apply(Command::EditClip {
        clip: c,
        length_beats: 3.5,
        offset_beats: 0.5,
        fade_in_beats: 0.0,
        fade_out_beats: 0.0,
    })
    .expect("trim");
    assert_eq!(first_impulse(&s), Some(before), "impulse did not move");
    // Anfang hinter den Impuls ziehen: Stille.
    s.apply(Command::EditClip {
        clip: c,
        length_beats: 2.0,
        offset_beats: 1.5,
        fade_in_beats: 0.0,
        fade_out_beats: 0.0,
    })
    .expect("trim 2");
    assert_eq!(first_impulse(&s), None, "impulse trimmed away");
}
