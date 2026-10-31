//! Eine Plugin-Datei, die nicht lädt: im Ticket steht kein Pfad, nur lokal.

use std::sync::OnceLock;

use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use woa_plugins::Catalog;
use woa_telemetry::{Journal, JournalLayer, LOCAL_TARGET, journal_filter};

#[test]
fn failed_clap_load_keeps_the_path_local() {
    static J: OnceLock<Journal> = OnceLock::new();
    let j = J.get_or_init(Journal::default);
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("geheim-plugin.clap");
    std::fs::write(&file, b"kein plugin").unwrap();
    let sub =
        tracing_subscriber::registry().with(JournalLayer::new(j).with_filter(journal_filter()));
    tracing::subscriber::with_default(sub, || {
        let _ = Catalog::scan_files(std::slice::from_ref(&file));
    });
    let all = j.snapshot();
    let ticket: Vec<_> = all.iter().filter(|e| e.ziel != LOCAL_TARGET).collect();
    assert!(
        ticket
            .iter()
            .any(|e| e.text.starts_with("clap load failed")),
        "{all:?}"
    );
    assert!(
        ticket
            .iter()
            .all(|e| !e.text.contains("geheim") && e.datei.is_none()),
        "{all:?}"
    );
    assert!(all.iter().any(|e| e.ziel == LOCAL_TARGET), "{all:?}");
}
