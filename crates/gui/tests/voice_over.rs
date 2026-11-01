//! Paket J5 (#285): „Stimme über Bett“ in der App geht denselben Weg wie `voice_over` über MCP.

use std::time::Instant;

use woa_gui::AppState;
use woa_model::EffectKind;

#[test]
fn voice_over_puts_the_chain_on_the_voice_and_ducks_the_bed_in_one_step() {
    let tmp = tempfile::tempdir().expect("tmp");
    let mut st = AppState::new(tmp.path().join("rec"), Instant::now());
    let bed = st.add_track().expect("bed");
    let voice = st.add_track().expect("voice");
    assert!(st.voice_over(voice));
    let v = st.project().track(voice).expect("voice").inserts.clone();
    assert_eq!(v.len(), woa_model::VOICE_CHAIN.len());
    let b = st.project().track(bed).expect("bed").inserts.clone();
    assert_eq!(b.len(), 1);
    assert_eq!(b[0].kind, EffectKind::Compressor);
    assert_eq!(b[0].sidechain, Some(voice));
    st.undo();
    assert!(st.project().tracks.iter().all(|t| t.inserts.is_empty()));
}
