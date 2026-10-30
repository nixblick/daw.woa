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
    mods: egui::Modifiers,
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
            mods: egui::Modifiers::NONE,
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
            modifiers: self.mods,
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
        let mods = self.mods;
        let btn = |pressed, pos| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: mods,
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

// ---- Paket 21: Schnitt und Fades ----------------------------------------------------------

#[test]
fn split_curves_gain_normalize_with_undo() {
    use woa_model::FadeCurve;
    let (mut st, t, c, _tmp) = state_with_clip();
    // Teilen bei Beat 8 (der Clip liegt 4..12): zwei Teile.
    let right = st.split_clip(c, 8.0).expect("split");
    assert_eq!(st.project().track(t).expect("t").clips.len(), 2);
    assert_eq!(clip(&st, right).offset_beats, 4.0);
    // Mitte (hier: den rechten Teil) löschen.
    assert!(st.remove_clip(right));
    assert_eq!(st.project().track(t).expect("t").clips.len(), 1);
    st.undo();
    st.undo();
    assert_eq!(st.project().track(t).expect("t").clips.len(), 1);
    assert_eq!(
        clip(&st, c).length_beats,
        8.0,
        "undo restores the whole clip"
    );

    // Kurven und Gain: je ein Undo-Schritt.
    assert!(st.set_fade_curves(c, FadeCurve::SCurve, FadeCurve::EqualPower));
    assert!(
        !st.set_fade_curves(c, FadeCurve::SCurve, FadeCurve::EqualPower),
        "no change"
    );
    assert!(st.set_clip_gain(c, -4.5));
    let k = clip(&st, c);
    assert_eq!(
        (k.fade_in_curve, k.fade_out_curve, k.gain_db),
        (FadeCurve::SCurve, FadeCurve::EqualPower, -4.5)
    );
    st.undo();
    assert_eq!(clip(&st, c).gain_db, 0.0);
    st.undo();
    assert_eq!(clip(&st, c).fade_in_curve, FadeCurve::Linear);

    // Normalisieren: Spitze 0,5 auf -1 dBFS sind +5,02 dB; Stille wird abgelehnt.
    assert!(st.normalize_clip(c, 0.5, -1.0));
    assert!((clip(&st, c).gain_db - 5.0206).abs() < 1e-3);
    assert!(
        !st.normalize_clip(c, 0.0, -1.0),
        "silence cannot be normalized"
    );
}

#[test]
fn fade_handle_drag_sets_the_fade_with_one_undo_step() {
    let mut rig = Rig::new();
    let t = rig.app.state_mut().add_track().expect("track");
    rig.step(Vec::new(), Vec::new());
    let a = wav(&rig.dir, "eins.wav", 4.0);
    let row = rig.row(t);
    rig.drop_files(&[&a], egui::pos2(rig.x_of(t, 4.0) + 1.0, row.center().y));
    let (_, c) = first_clip(&rig.app);
    assert_eq!(c.fade_in_beats, 0.0);
    let steps = rig.app.state().session().history().undo_entries().len();

    // Der Griff des Fade-in sitzt 10 Pixel rechts der linken Kante, an der Oberkante der Wellenform.
    let y = rig.row(t).top() + 3.0 + 16.0;
    let from = egui::pos2(rig.x_of(t, 4.0) + 10.0, y);
    let to = egui::pos2(from.x + 2.0 * rig.app.zoom(), y);
    rig.drag(from, to);
    let k = rig.app.state().project().clip(c.id).expect("c").1.clone();
    assert!(
        (k.fade_in_beats - 2.0).abs() < 0.3,
        "dragged by about 2 beats, got {}",
        k.fade_in_beats
    );
    assert_eq!(k.start_beats, 4.0, "the clip did not move");
    assert_eq!(k.fade_out_beats, 0.0);
    assert_eq!(
        rig.app.state().session().history().undo_entries().len(),
        steps + 1,
        "one undo step"
    );
    rig.key(egui::Key::Z, egui::Modifiers::COMMAND);
    assert_eq!(
        rig.app
            .state()
            .project()
            .clip(c.id)
            .expect("c")
            .1
            .fade_in_beats,
        0.0
    );
}

#[test]
fn curves_and_gain_survive_save_and_reopen() {
    use woa_model::FadeCurve;
    let (mut st, _t, c, tmp) = state_with_clip();
    st.set_fades(c, 1.5, 0.5);
    st.set_fade_curves(c, FadeCurve::EqualPower, FadeCurve::SCurve);
    st.set_clip_gain(c, 3.0);
    let proj = tmp.path().join("shape.woa");
    assert!(st.save_as(&proj));
    let mut again = AppState::new(tmp.path().join("rec3"), Instant::now());
    assert!(again.open(&proj));
    assert_eq!(again.project(), st.project());
    let k = clip(&again, c);
    assert_eq!(
        (k.fade_in_curve, k.fade_out_curve, k.gain_db),
        (FadeCurve::EqualPower, FadeCurve::SCurve, 3.0)
    );
}

// ---- Paket 23: Time-Stretch und Tonhöhe -----------------------------------------------------

#[test]
fn stretch_pitch_follow_as_state_with_undo_and_save() {
    let (mut st, _t, c, tmp) = state_with_clip();
    let steps = st.session().history().undo_entries().len();
    // Zehn Prozent länger: Länge 8 -> 8,8, Faktor 1,1, ein Undo-Schritt.
    assert!(st.stretch_clip(c, 8.8, false));
    let k = clip(&st, c);
    assert!((k.length_beats - 8.8).abs() < 1e-9 && (k.stretch - 1.1).abs() < 1e-9);
    assert_eq!(k.start_beats, 4.0);
    assert_eq!(st.session().history().undo_entries().len(), steps + 1);
    // Am linken Rand gedehnt: das Ende bleibt, der Anfang rückt.
    assert!(st.stretch_clip(c, 11.0, true));
    let k = clip(&st, c);
    assert!((k.start_beats - 1.8).abs() < 1e-9 && (k.end_beats() - 12.8).abs() < 1e-9);
    assert!((k.stretch - 1.375).abs() < 1e-9);
    st.undo();
    st.undo();
    let k = clip(&st, c);
    assert_eq!((k.start_beats, k.length_beats, k.stretch), (4.0, 8.0, 1.0));
    // Außerhalb des Bereichs: abgelehnt, nichts ändert sich.
    assert!(!st.stretch_clip(c, 80.0, false));
    assert!(!st.stretch_clip(c, 1.0, false));
    assert_eq!(clip(&st, c).stretch, 1.0);

    // Tonhöhe und Tempo-Folgen, dann Schneiden: der Versatz des rechten Teils folgt der Dehnung.
    assert!(st.set_clip_pitch(c, 2.0));
    assert!(st.set_clip_follow_tempo(c, true));
    assert!(st.set_clip_stretch(c, 2.0));
    assert_eq!(clip(&st, c).length_beats, 16.0);
    let right = st.split_clip(c, 12.0).expect("split");
    let (l, r) = (clip(&st, c), clip(&st, right));
    assert_eq!((l.length_beats, r.length_beats), (8.0, 8.0));
    assert!(
        (r.offset_beats - 4.0).abs() < 1e-9,
        "8 Beats Zeitachse = 4 Beats Ton, offset {}",
        r.offset_beats
    );
    assert_eq!((r.stretch, r.pitch_semitones), (2.0, 2.0));
    assert_eq!(r.follow_tempo_bpm, Some(120.0));

    // Speichern, neu öffnen: alles steht wie zuvor.
    let proj = tmp.path().join("song.woa");
    assert!(st.save_as(&proj));
    let mut again = AppState::new(tmp.path().join("rec2"), Instant::now());
    assert!(again.open(&proj));
    assert_eq!(again.project(), st.project());
}

#[test]
fn alt_drag_on_the_edge_stretches_and_plain_drag_still_trims() {
    let mut rig = Rig::new();
    let t = rig.app.state_mut().add_track().expect("track");
    rig.step(Vec::new(), Vec::new());
    let a = wav(&rig.dir, "eins.wav", 4.0);
    let row = rig.row(t);
    rig.drop_files(&[&a], egui::pos2(rig.x_of(t, 4.0) + 1.0, row.center().y));
    let (_, c) = first_clip(&rig.app);
    assert_eq!((c.start_beats, c.length_beats, c.stretch), (4.0, 8.0, 1.0));
    let steps = rig.app.state().session().history().undo_entries().len();

    // Mit Alt den rechten Rand 2 Beats nach rechts: Länge 8 -> 10, Faktor 1,25, Ton bleibt.
    let y = rig.row(t).center().y + 8.0;
    let end = 12.0;
    let from = egui::pos2(rig.x_of(t, end) - 3.0, y);
    let to = egui::pos2(from.x + 2.0 * rig.app.zoom(), y);
    rig.mods = egui::Modifiers::ALT;
    rig.drag(from, to);
    rig.mods = egui::Modifiers::NONE;
    let k = rig.app.state().project().clip(c.id).expect("c").1.clone();
    assert!(
        (k.length_beats - 10.0).abs() < 0.3,
        "stretched by about 2 beats, got {}",
        k.length_beats
    );
    assert!((k.stretch - k.length_beats / 8.0).abs() < 1e-9);
    assert_eq!(k.start_beats, 4.0, "the start stays");
    assert_eq!(k.offset_beats, 0.0, "the audio is not cut");
    assert_eq!(
        rig.app.state().session().history().undo_entries().len(),
        steps + 1,
        "one undo step"
    );
    rig.key(egui::Key::Z, egui::Modifiers::COMMAND);
    let k = rig.app.state().project().clip(c.id).expect("c").1.clone();
    assert_eq!((k.length_beats, k.stretch), (8.0, 1.0));

    // Ohne Alt kürzt derselbe Zug nur: Faktor bleibt 1.
    let end = 12.0;
    let from = egui::pos2(rig.x_of(t, end) - 3.0, y);
    let to = egui::pos2(from.x - 2.0 * rig.app.zoom(), y);
    rig.drag(from, to);
    let k = rig.app.state().project().clip(c.id).expect("c").1.clone();
    assert_eq!((k.length_beats, k.stretch), (6.0, 1.0));
}
