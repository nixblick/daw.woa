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
