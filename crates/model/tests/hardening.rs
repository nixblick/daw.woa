//! Hostile inputs: oversized ids and planted symlinks.

use woa_model::store;
use woa_model::*;

#[test]
fn add_track_with_huge_id_is_rejected() {
    let mut s = Session::new("x");
    let err = s
        .apply(Command::add_track(TrackId(u64::MAX), "T", TrackKind::Audio))
        .unwrap_err();
    assert!(matches!(err, ModelError::IdOutOfRange(u64::MAX)));
    // The counter is untouched, so new ids keep working.
    let id = s.new_track_id();
    assert!(id.0 <= MAX_ID);
}

#[test]
fn loading_huge_ids_is_rejected() {
    let mut s = Session::new("x");
    let id = s.new_track_id();
    s.apply(Command::add_track(id, "T", TrackKind::Audio))
        .unwrap();
    let json = store::to_json(s.project()).unwrap();
    let hostile = json.replacen(
        &format!("\"id\": {}", id.0),
        &format!("\"id\": {}", u64::MAX),
        1,
    );
    assert_ne!(json, hostile, "test must actually change the id");
    let err = store::from_json(&hostile, std::path::Path::new("x")).unwrap_err();
    assert!(matches!(err, ModelError::IdOutOfRange(_)));
}

#[cfg(unix)]
#[test]
fn save_does_not_follow_planted_tmp_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let project_dir = dir.path().join("p.woa");
    std::fs::create_dir_all(&project_dir).unwrap();
    let victim = dir.path().join("victim.txt");
    std::fs::write(&victim, "keep").unwrap();
    std::os::unix::fs::symlink(&victim, project_dir.join("project.json.tmp")).unwrap();

    store::save(Session::new("x").project(), &project_dir).unwrap();

    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
    assert!(store::load(&project_dir).is_ok());
}

/// #79: a long chain of buses loads in linear time, not one graph search per edge.
#[test]
fn loading_a_long_bus_chain_is_fast() {
    let mut s = Session::new("x");
    let mut prev: Option<TrackId> = None;
    for n in 0..2000 {
        let id = s.new_track_id();
        s.apply(Command::add_track(id, format!("B{n}"), TrackKind::Bus))
            .unwrap();
        if let Some(p) = prev {
            s.apply(Command::SetOutput {
                track: p,
                output: Some(id),
            })
            .unwrap();
        }
        prev = Some(id);
    }
    let json = store::to_json(s.project()).unwrap();
    let t0 = std::time::Instant::now();
    store::from_json(&json, std::path::Path::new("x")).unwrap();
    assert!(t0.elapsed() < std::time::Duration::from_secs(2));
}

/// #79: a cycle planted in the file is still found by the one-pass check.
#[test]
fn loading_a_planted_cycle_is_rejected() {
    let mut s = Session::new("x");
    let a = s.new_track_id();
    s.apply(Command::add_track(a, "A", TrackKind::Bus)).unwrap();
    let b = s.new_track_id();
    s.apply(Command::add_track(b, "B", TrackKind::Bus)).unwrap();
    s.apply(Command::SetOutput {
        track: a,
        output: Some(b),
    })
    .unwrap();
    let json = store::to_json(s.project()).unwrap();
    let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let tracks = v["project"]["tracks"].as_array_mut().unwrap();
    let tb = tracks.iter_mut().find(|t| t["id"] == b.0).unwrap();
    tb["output"] = serde_json::json!(a.0);
    let hostile = serde_json::to_string(&v).unwrap();
    let err = store::from_json(&hostile, std::path::Path::new("x")).unwrap_err();
    assert!(matches!(err, ModelError::RoutingCycle { .. }));
}
