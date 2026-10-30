//! Paket 20: Timeline-Editor. Befehle am Modell (Verschieben, Trimmen, Kopieren, Undo/Redo),
//! Rundlauf Speichern/Laden und die Oberfläche ohne Fenster und ohne Ton: Dateien ablegen,
//! Clip ziehen, Rand ziehen, Tastenkürzel. Es läuft nie ein Audiogerät.

use std::path::{Path, PathBuf};
use std::time::Instant;

use eframe::egui;
use woa_gui::{AppState, GuiConfig, WoaApp};
use woa_model::{Clip, ClipId, TrackId};

fn wav(dir: &Path, name: &str, seconds: f32) -> PathBuf {
    let rate = 48_000u32;
    let n = (seconds * rate as f32) as usize;
    let s: Vec<f32> = (0..n).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
    let path = dir.join(name);
    woa_engine::write_wav_stereo(&path, &s, &s, rate).expect("write wav");
    path
}

fn state_with_clip() -> (AppState, TrackId, ClipId, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("tmp");
    let mut st = AppState::new(tmp.path().join("rec"), Instant::now());
    let t = st.add_track().expect("track");
    let f = wav(tmp.path(), "a.wav", 4.0); // 8 Beats bei 120 bpm
    let dir = st.takes_dir();
    std::fs::create_dir_all(&dir).expect("dir");
    let dst = dir.join("a.wav");
    std::fs::copy(&f, &dst).expect("copy");
    let c = st.add_take(Some(t), &dst, 4.0, 4.0, 0.0).expect("clip");
    (st, t, c, tmp)
}

fn clip(st: &AppState, id: ClipId) -> Clip {
    st.project().clip(id).expect("clip exists").1.clone()
}

#[test]
fn move_trim_undo_redo() {
    let (mut st, _t, c, _tmp) = state_with_clip();
    assert_eq!(clip(&st, c).start_beats, 4.0);
    assert!(st.move_clip(c, 10.0));
    assert_eq!(clip(&st, c).start_beats, 10.0);
    // Ende kürzen
    assert!(st.trim_clip(c, 10.0, 5.0, 0.0));
    assert_eq!(clip(&st, c).length_beats, 5.0);
    // Anfang kürzen: Start rückt, Versatz in der Datei rückt mit
    assert!(st.trim_clip(c, 11.0, 4.0, 1.0));
    let k = clip(&st, c);
    assert_eq!(
        (k.start_beats, k.length_beats, k.offset_beats),
        (11.0, 4.0, 1.0)
    );
    st.undo();
    let k = clip(&st, c);
    assert_eq!(
        (k.start_beats, k.length_beats, k.offset_beats),
        (10.0, 5.0, 0.0)
    );
    st.undo();
    assert_eq!(clip(&st, c).length_beats, 8.0);
    st.undo();
    assert_eq!(clip(&st, c).start_beats, 4.0);
    st.redo();
    st.redo();
    st.redo();
    let k = clip(&st, c);
    assert_eq!(
        (k.start_beats, k.length_beats, k.offset_beats),
        (11.0, 4.0, 1.0)
    );
}

#[test]
fn move_to_other_track_and_back() {
    let (mut st, t, c, _tmp) = state_with_clip();
    let t2 = st.add_track().expect("t2");
    assert!(st.move_clip_to(c, Some(t2), 2.0));
    assert_eq!(st.project().clip(c).map(|(tr, _)| tr.id), Some(t2));
    st.undo();
    let (tr, k) = st.project().clip(c).expect("back");
    assert_eq!((tr.id, k.start_beats), (t, 4.0));
}

#[test]
fn copy_paste_duplicate_with_undo() {
    let (mut st, t, c, _tmp) = state_with_clip();
    let items = st.copy_clips(&[c]);
    assert_eq!(items.len(), 1);
    // Einfügen bei Beat 20: neue Id, gleiche Datei, Länge und Versatz
    let new = st.paste_clips(&items, 20.0, Some(t));
    assert_eq!(new.len(), 1);
    assert_ne!(new[0], c);
    let k = clip(&st, new[0]);
    assert_eq!((k.start_beats, k.length_beats), (20.0, 8.0));
    assert_eq!(k.audio, clip(&st, c).audio);
    // Verdoppeln: direkt hinter dem Original (4 + 8 = 12)
    let d = st.duplicate_clips(&[c]);
    assert_eq!(clip(&st, d[0]).start_beats, 12.0);
    assert_eq!(st.project().track(t).expect("t").clips.len(), 3);
    // Ein Undo je Schritt
    st.undo();
    st.undo();
    assert_eq!(st.project().track(t).expect("t").clips.len(), 1);
    st.redo();
    assert_eq!(st.project().track(t).expect("t").clips.len(), 2);
}

#[test]
fn paste_respects_track_kind() {
    let (mut st, _t, c, _tmp) = state_with_clip();
    let midi = st.add_midi_track().expect("midi");
    let items = st.copy_clips(&[c]);
    // Audio-Clip auf MIDI-Spur: geht zurück auf die Ursprungsspur
    let new = st.paste_clips(&items, 0.0, Some(midi));
    assert_eq!(new.len(), 1);
    assert_ne!(st.project().clip(new[0]).map(|(t, _)| t.id), Some(midi));
    assert!(st.paste_clips(&[], 0.0, None).is_empty());
}

#[test]
fn save_and_reopen_keeps_the_layout() {
    let (mut st, t, c, tmp) = state_with_clip();
    st.move_clip(c, 6.5);
    st.trim_clip(c, 7.0, 3.0, 0.5);
    let dup = st.duplicate_clips(&[c]);
    let proj = tmp.path().join("song.woa");
    assert!(st.save_as(&proj));
    let mut again = AppState::new(tmp.path().join("rec2"), Instant::now());
    assert!(again.open(&proj));
    assert_eq!(again.project(), st.project());
    let k = clip(&again, c);
    assert_eq!(
        (k.start_beats, k.length_beats, k.offset_beats),
        (7.0, 3.0, 0.5)
    );
    assert_eq!(clip(&again, dup[0]).start_beats, 10.0);
    assert!(again.project().track(t).is_some());
}

// ---- Oberfläche ohne Fenster -------------------------------------------------------------

struct Rig {
    ctx: egui::Context,
    app: WoaApp,
    frame: eframe::Frame,
    n: u32,
    _tmp: tempfile::TempDir,
    dir: PathBuf,
}

impl Rig {
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tmp");
        let dir = tmp.path().to_path_buf();
        let app = WoaApp::without_audio(GuiConfig::default(), dir.join("rec"));
        let mut rig = Self {
            ctx: egui::Context::default(),
            app,
            frame: eframe::Frame::_new_kittest(),
            n: 0,
            _tmp: tmp,
            dir,
        };
        rig.step(Vec::new(), Vec::new());
        rig.step(Vec::new(), Vec::new());
        rig
    }

    fn step(&mut self, events: Vec<egui::Event>, dropped: Vec<egui::DroppedFile>) {
        self.n += 1;
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 1000.0),
            )),
            time: Some(f64::from(self.n) * 0.05),
            events,
            dropped_files: dropped,
            ..Default::default()
        };
        input.viewports.entry(egui::ViewportId::ROOT).or_default();
        let app = &mut self.app;
        let frame = &mut self.frame;
        let _ = self.ctx.run_ui(input, |ui| {
            eframe::App::logic(app, ui.ctx(), frame);
            eframe::App::ui(app, ui, frame);
        });
    }

    fn key(&mut self, key: egui::Key, mods: egui::Modifiers) {
        let ev = |pressed| egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers: mods,
        };
        self.step(vec![ev(true)], Vec::new());
        self.step(vec![ev(false)], Vec::new());
    }

    fn drop_files(&mut self, paths: &[&Path], at: egui::Pos2) {
        self.step(vec![egui::Event::PointerMoved(at)], Vec::new());
        let files = paths
            .iter()
            .map(|p| egui::DroppedFile {
                path: Some(p.to_path_buf()),
                ..Default::default()
            })
            .collect();
        self.step(Vec::new(), files);
        self.step(Vec::new(), Vec::new());
    }

    fn drag(&mut self, from: egui::Pos2, to: egui::Pos2) {
        let btn = |pressed, pos| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.step(vec![egui::Event::PointerMoved(from)], Vec::new());
        self.step(vec![btn(true, from)], Vec::new());
        for i in 1..=6 {
            let p = from + (to - from) * (i as f32 / 6.0);
            self.step(vec![egui::Event::PointerMoved(p)], Vec::new());
        }
        self.step(vec![btn(false, to)], Vec::new());
        self.step(Vec::new(), Vec::new());
    }

    fn row(&self, track: TrackId) -> egui::Rect {
        self.app
            .track_rects()
            .iter()
            .find(|(t, _)| *t == track)
            .map(|(_, r)| *r)
            .expect("track row drawn")
    }

    fn x_of(&self, track: TrackId, beat: f64) -> f32 {
        self.row(track).left() + beat as f32 * self.app.zoom()
    }
}

fn first_clip(app: &WoaApp) -> (TrackId, Clip) {
    let p = app.state().project();
    let t = p
        .tracks
        .iter()
        .find(|t| !t.clips.is_empty())
        .expect("a clip");
    (t.id, t.clips[0].clone())
}

#[test]
fn two_files_dropped_move_trim_undo_save_reopen() {
    let mut rig = Rig::new();
    let t = rig.app.state_mut().add_track().expect("track");
    rig.step(Vec::new(), Vec::new());
    let a = wav(&rig.dir, "eins.wav", 4.0);
    let b = wav(&rig.dir, "zwei.wav", 2.0);

    // Beide Dateien auf Beat 4 der Spur fallen lassen: die erste dorthin, die zweite auf
    // eine neue Spur an derselben Stelle.
    let row = rig.row(t);
    let at = egui::pos2(rig.x_of(t, 4.0) + 1.0, row.center().y);
    rig.drop_files(&[&a, &b], at);
    let (tr, c) = first_clip(&rig.app);
    assert_eq!(tr, t);
    assert_eq!(c.start_beats, 4.0, "dropped at the pointer, on the grid");
    assert_eq!(c.length_beats, 8.0);

    let p = rig.app.state().project().clone();
    assert_eq!(p.tracks.len(), 2, "second file got its own track");
    assert_eq!(p.tracks[1].clips[0].start_beats, 4.0);
    assert_eq!(p.tracks[1].clips[0].length_beats, 4.0);

    // Clip ziehen: 4 Beats nach rechts.
    let id = c.id;
    let y = rig.row(t).center().y + 8.0;
    let from = egui::pos2(rig.x_of(t, 8.0), y);
    let to = egui::pos2(rig.x_of(t, 12.0), y);
    rig.drag(from, to);
    assert_eq!(
        rig.app.state().project().clip(id).expect("c").1.start_beats,
        8.0
    );

    // Rechten Rand 2 Beats nach links ziehen: Länge 8 -> 6.
    let end = 8.0 + 8.0;
    let from = egui::pos2(rig.x_of(t, end) - 3.0, y);
    let to = egui::pos2(rig.x_of(t, end - 2.0) - 3.0, y);
    rig.drag(from, to);
    assert_eq!(
        rig.app
            .state()
            .project()
            .clip(id)
            .expect("c")
            .1
            .length_beats,
        6.0
    );

    // Strg+Z zweimal: zurück bei Start 4 und Länge 8; Strg+Y: wieder vor.
    rig.key(egui::Key::Z, egui::Modifiers::COMMAND);
    rig.key(egui::Key::Z, egui::Modifiers::COMMAND);
    let k = rig.app.state().project().clip(id).expect("c").1.clone();
    assert_eq!((k.start_beats, k.length_beats), (4.0, 8.0));
    rig.key(egui::Key::Y, egui::Modifiers::COMMAND);
    rig.key(egui::Key::Y, egui::Modifiers::COMMAND);
    let k = rig.app.state().project().clip(id).expect("c").1.clone();
    assert_eq!((k.start_beats, k.length_beats), (8.0, 6.0));

    // Strg+D verdoppelt hinter dem Clip (der Clip ist nach dem Ziehen markiert).
    rig.key(egui::Key::D, egui::Modifiers::COMMAND);
    assert_eq!(
        rig.app.state().project().track(t).expect("t").clips.len(),
        2
    );
    assert_eq!(
        rig.app.state().project().track(t).expect("t").clips[1].start_beats,
        14.0
    );

    // Speichern und neu öffnen: gleiche Lage.
    let proj = rig.dir.join("song.woa");
    assert!(rig.app.state_mut().save_as(&proj));
    let mut again = AppState::new(rig.dir.join("rec2"), Instant::now());
    assert!(again.open(&proj));
    assert_eq!(again.project(), rig.app.state().project());
}
