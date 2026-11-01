//! Paket J8 (#283): Fenster „Jingle-FX“ legt denselben Clip an wie `generate_fx` über MCP.

use std::time::Instant;

use woa_agent::audio::AgentFxKind;
use woa_gui::AppState;

#[test]
fn fx_lands_as_audio_clip_in_tempo_on_a_new_track() {
    let tmp = tempfile::tempdir().expect("tmp");
    let mut st = AppState::new(tmp.path().join("rec"), Instant::now());
    let clip = st.add_fx(None, AgentFxKind::Riser, 8.0, 4.0).expect("clip");
    assert_eq!(st.project().tracks.len(), 1);
    let c = st.project().clip(clip).expect("clip in project").1.clone();
    assert_eq!(c.name, "Riser");
    assert!((c.start_beats - 4.0).abs() < 1e-9);
    assert!((c.length_beats - 8.0).abs() < 1e-6, "{}", c.length_beats);
    let file = st
        .resolve_audio(c.audio.as_deref().expect("audio"))
        .expect("file");
    let l = woa_engine::clip::load_wav_mono(&file, 48_000).expect("wav");
    assert!(l.iter().all(|s| s.abs() < 1.0), "clipping");
    // Zweiter Effekt: neue Datei, nichts überschrieben.
    let again = st
        .add_fx(None, AgentFxKind::Whoosh, 4.0, 0.0)
        .expect("clip");
    let (_, d) = st.project().clip(again).expect("clip");
    assert_ne!(c.audio, d.audio);
}
