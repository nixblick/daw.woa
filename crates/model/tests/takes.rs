//! Paket 22: Take-Ordner und Comping im Modell: Befehl mit Undo und Redo, Prüfung,
//! Projektformat.

use woa_model::{Command, Session, TakeEntry, TrackId, TrackKind, store};

fn takes(n: usize) -> Vec<TakeEntry> {
    (0..n)
        .map(|k| TakeEntry {
            name: format!("Take {}", k + 1),
            audio: format!("aufnahmen/aufnahme-{:03}.wav", k + 1),
            offset_beats: 0.25 + 4.0 * k as f64,
        })
        .collect()
}

fn session() -> (Session, TrackId) {
    let mut s = Session::new("takes");
    let t = s.new_track_id();
    s.apply(Command::add_track(t, "Gesang", TrackKind::Audio))
        .expect("track");
    (s, t)
}

/// Der Zähler für neue Ids wächst nur, auch bei Undo; verglichen wird der Rest.
fn same(p: &woa_model::Project) -> woa_model::Project {
    let mut p = p.clone();
    p.next_id = 0;
    p
}

#[test]
fn folder_comp_undo_redo_roundtrip() {
    let (mut s, t) = session();
    let empty = s.project().clone();
    let id = s.add_take_folder(t, "Loop", 4.0, 12.0, takes(3)).unwrap();
    let with_folder = s.project().clone();
    let tr = s.project().track(t).unwrap();
    assert_eq!(tr.take_folders.len(), 1);
    assert_eq!(tr.clips.len(), 1, "newest take over the whole folder");
    assert_eq!(
        tr.clips[0].audio.as_deref(),
        Some("aufnahmen/aufnahme-003.wav")
    );
    assert_eq!(tr.clips[0].offset_beats, 8.25);

    // Beat 8 bis 12 aus Take 1, Beat 12 bis 16 aus Take 2.
    let mut f = tr.take_folders[0].clone();
    f.comp = f.comp_with(8.0, 12.0, 0);
    f.comp = f.comp_with(12.0, 16.0, 1);
    s.set_comp(t, id, f.comp).unwrap();
    let tr = s.project().track(t).unwrap();
    assert_eq!(tr.clips.len(), 3);
    let starts: Vec<f64> = tr.clips.iter().map(|c| c.start_beats).collect();
    assert_eq!(starts, vec![4.0, 8.0, 12.0]);
    assert_eq!(
        tr.clips[1].audio.as_deref(),
        Some("aufnahmen/aufnahme-001.wav")
    );
    assert_eq!(tr.clips[1].offset_beats, 0.25 + 4.0);
    assert_eq!(
        tr.clips[2].audio.as_deref(),
        Some("aufnahmen/aufnahme-002.wav")
    );
    let comped = s.project().clone();

    s.undo().unwrap();
    assert_eq!(same(s.project()), same(&with_folder));
    s.undo().unwrap();
    assert_eq!(same(s.project()), same(&empty));
    s.redo().unwrap();
    s.redo().unwrap();
    assert_eq!(same(s.project()), same(&comped));
}

#[test]
fn bad_input_changes_nothing() {
    let (mut s, t) = session();
    let id = s.add_take_folder(t, "Loop", 4.0, 12.0, takes(2)).unwrap();
    let m = s.new_track_id();
    s.apply(Command::add_track(m, "Keys", TrackKind::Midi))
        .unwrap();
    let before = s.project().clone();
    let f = s.project().track(t).unwrap().take_folders[0].clone();
    let mut bad = f.comp.clone();
    bad[0].take = 9;
    assert!(s.set_comp(t, id, bad).is_err());
    let mut bad = f.comp.clone();
    bad[0].end_beats = 99.0;
    assert!(s.set_comp(t, id, bad).is_err());
    assert!(s.set_comp(t, id + 1000, f.comp.clone()).is_err());
    let mut evil = takes(1);
    evil[0].audio = "../x.wav".into();
    assert!(s.add_take_folder(t, "x", 0.0, 4.0, evil).is_err());
    assert!(s.add_take_folder(t, "x", 0.0, 4.0, vec![]).is_err());
    assert!(s.add_take_folder(m, "x", 0.0, 4.0, takes(1)).is_err());
    assert_eq!(same(s.project()), same(&before));
}

#[test]
fn format_three_only_with_folders_and_roundtrip() {
    let (mut s, t) = session();
    assert_eq!(store::required_version(s.project()), 2);
    s.add_take_folder(t, "Loop", 4.0, 12.0, takes(3)).unwrap();
    assert_eq!(store::required_version(s.project()), 3);
    let json = store::to_json(s.project()).unwrap();
    let back = store::from_json(&json, std::path::Path::new("project.json")).unwrap();
    assert_eq!(&back, s.project());
    // Ein Ordner mit unsicherem Pfad lässt sich nicht laden.
    let evil = json.replace("aufnahmen/aufnahme-001.wav", "../aufnahme-001.wav");
    assert!(store::from_json(&evil, std::path::Path::new("project.json")).is_err());
}

#[test]
fn removing_the_folder_removes_its_clips() {
    let (mut s, t) = session();
    let id = s.add_take_folder(t, "Loop", 4.0, 12.0, takes(2)).unwrap();
    s.apply(Command::SetTakeFolder {
        track: t,
        folder_id: id,
        folder: None,
        clips: vec![],
    })
    .unwrap();
    let tr = s.project().track(t).unwrap();
    assert!(tr.take_folders.is_empty() && tr.clips.is_empty());
    s.undo().unwrap();
    let tr = s.project().track(t).unwrap();
    assert_eq!((tr.take_folders.len(), tr.clips.len()), (1, 1));
}
