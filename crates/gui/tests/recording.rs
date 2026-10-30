//! Paket 22 in der Oberflächenschicht, ohne Fenster, Mikrofon und Ton: Aufnahme mit
//! Latenzausgleich und Punch als Clip, Loop-Takes als Take-Ordner, Comping und Undo.

use std::time::Instant;

use woa_gui::AppState;
use woa_model::{RecordInput, RecordPlan, plan_recording};

const RATE: f64 = 48_000.0;

fn input(frames: u64, loop_beats: Option<(f64, f64)>, punch: Option<(f64, f64)>) -> RecordInput {
    RecordInput {
        start_beats: 4.0,
        tempo_bpm: 120.0,
        rate: RATE,
        frames,
        latency_frames: 960,
        loop_beats,
        punch_beats: punch,
    }
}

fn state() -> (AppState, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("tmp");
    let st = AppState::new(tmp.path().join("rec"), Instant::now());
    (st, tmp)
}

fn file(st: &AppState) -> std::path::PathBuf {
    let dir = st.takes_dir();
    std::fs::create_dir_all(&dir).expect("dir");
    let p = dir.join("aufnahme-001.wav");
    let s = vec![0.1f32; 48_000 * 10];
    woa_engine::write_wav_stereo(&p, &s, &s, 48_000).expect("wav");
    p
}

#[test]
fn single_recording_is_placed_with_latency_and_punch() {
    let (mut st, _tmp) = state();
    let f = file(&st);
    let plan = plan_recording(&input(48_000 * 5, None, None)).expect("plan");
    let id = st.add_recording(None, &f, &plan).expect("clip");
    let c = st.project().clip(id).expect("c").1.clone();
    assert_eq!(c.start_beats, 4.0);
    assert!(c.offset_beats > 0.0, "latency skipped at the file start");

    let plan = plan_recording(&input(48_000 * 5, None, Some((6.0, 8.0)))).expect("plan");
    let id = st.add_recording(None, &f, &plan).expect("clip");
    let c = st.project().clip(id).expect("c").1.clone();
    assert_eq!((c.start_beats, c.length_beats), (6.0, 2.0));
}

#[test]
fn loop_recording_makes_a_folder_and_comping_undoes() {
    let (mut st, _tmp) = state();
    let f = file(&st);
    // 3 Runden von je 4 Beats (2 s) plus Latenz.
    let plan = plan_recording(&input(48_000 * 6 + 960, Some((4.0, 8.0)), None)).expect("plan");
    let RecordPlan::Folder { offsets, .. } = &plan else {
        panic!("folder expected");
    };
    assert_eq!(offsets.len(), 3);
    st.add_recording(None, &f, &plan).expect("clip");
    let t = st.project().tracks[0].id;
    let fold = st.project().tracks[0].take_folders[0].clone();
    assert_eq!(fold.takes.len(), 3);
    assert_eq!(st.project().tracks[0].clips.len(), 1);

    let before = st.project().tracks[0].clips.len();
    assert!(st.comp_range(t, fold.id, 4.0, 6.0, 0));
    assert_eq!(st.project().tracks[0].clips.len(), before + 1);
    st.undo();
    assert_eq!(st.project().tracks[0].clips.len(), before);

    assert!(st.remove_take_folder(t, fold.id));
    assert!(st.project().tracks[0].take_folders.is_empty());
    assert!(st.project().tracks[0].clips.is_empty());
    st.undo();
    assert_eq!(st.project().tracks[0].take_folders.len(), 1);
}
