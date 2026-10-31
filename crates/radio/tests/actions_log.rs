//! Bedienaktionen erzeugen je eine Info-Zeile; Dateinamen stehen nur unter `LOCAL_TARGET`.

use std::path::Path;
use std::sync::OnceLock;

use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use woa_radio::{Controller, Cues, RadioShow};
use woa_telemetry::{Journal, JournalLayer, LOCAL_TARGET, journal_filter};

const SR: u32 = 48_000;

fn write_wav(path: &Path) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SR,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for _ in 0..SR as usize {
        w.write_sample(0.1f32).unwrap();
    }
    w.finalize().unwrap();
}

#[test]
fn actions_log_one_line_without_file_names_in_the_ticket() {
    static J: OnceLock<Journal> = OnceLock::new();
    let j = J.get_or_init(Journal::default);
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("geheim-titel.wav");
    write_wav(&file);
    let (mut c, _m) = Controller::new(RadioShow::default(), SR);
    c.deck_load(0, file.clone(), Cues::default()).unwrap();
    let (h, _pfl) = woa_radio::pfl::channel(SR);
    c.set_pfl(Some(h));
    let sub =
        tracing_subscriber::registry().with(JournalLayer::new(j).with_filter(journal_filter()));
    tracing::subscriber::with_default(sub, || {
        c.deck_play(0);
        c.pfl_file(&file).unwrap();
    });
    let all = j.snapshot();
    let ticket: Vec<_> = all.iter().filter(|e| e.ziel != LOCAL_TARGET).collect();
    let deck: Vec<_> = ticket
        .iter()
        .filter(|e| e.text.starts_with("deck start"))
        .collect();
    assert_eq!(deck.len(), 1, "{all:?}");
    assert!(deck[0].text.contains("deck=A"), "{all:?}");
    assert!(
        ticket.iter().any(|e| e.text.starts_with("pfl start")),
        "{all:?}"
    );
    assert!(
        ticket
            .iter()
            .all(|e| !e.text.contains("geheim") && e.datei.is_none()),
        "{all:?}"
    );
    assert!(
        all.iter()
            .any(|e| e.ziel == LOCAL_TARGET
                && e.datei.as_deref().is_some_and(|d| d.contains("geheim"))),
        "{all:?}"
    );
}
