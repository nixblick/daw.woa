//! Beschädigte und unlesbare Dateien: Zählung je Datei, Klartext im lokalen Protokoll.
//! Testdateien entstehen im Tempordner (kein Ton) und verschwinden danach.

use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use woa_engine::decode::{Damage, damage, decode_file};
use woa_engine::export::export_stereo;
use woa_telemetry::{FaultCounter, Journal, JournalLayer, LOCAL_TARGET, Stufe, journal_filter};

fn tone(rate: u32, secs: u32) -> Vec<f32> {
    (0..rate * secs)
        .map(|i| 0.4 * (std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin())
        .collect()
}

#[test]
fn damaged_mp3_counts_faults_and_unreadable_file_is_named() {
    tracing_log::LogTracer::init().unwrap();
    let j: &'static Journal = Box::leak(Box::default());
    let sub = tracing_subscriber::registry()
        .with(FaultCounter.with_filter(FaultCounter::filter()))
        .with(JournalLayer::new(j).with_filter(journal_filter()));
    let dir = tempfile::tempdir().unwrap();
    let ok = dir.path().join("heil.mp3");
    let bad = dir.path().join("kaputt.mp3");
    let junk = dir.path().join("muell.mp3");
    let t = tone(44_100, 4);
    export_stereo(&ok, &t, &t, 44_100).unwrap();
    // Bit-Reservoir verbiegen: In den ersten Frames zeigt `main_data_begin` (9 Bit nach dem
    // Kopf, ohne CRC) auf 511 Byte davor, die es nicht gibt, wie bei einer Datei, die mitten
    // im Strom beginnt. Der Dekoder warnt je Frame und spielt weiter.
    let mut b = std::fs::read(&ok).unwrap();
    let heads: Vec<usize> = (0..b.len() - 6)
        .filter(|&i| b[i] == 0xFF && b[i + 1] == 0xFB)
        .take(6)
        .collect();
    assert!(heads.len() >= 3);
    for p in heads {
        b[p + 4] = 0xFF;
        b[p + 5] |= 0x80;
    }
    std::fs::write(&bad, &b).unwrap();
    std::fs::write(&junk, vec![0x55u8; 4096]).unwrap();

    tracing::subscriber::with_default(sub, || {
        assert!(decode_file(&ok).is_ok());
        assert_eq!(damage(&ok), None);
        let r = decode_file(&bad);
        assert!(r.is_ok(), "beschädigt, spielt aber: {:?}", r.err());
        assert!(decode_file(&bad).is_ok());
        assert!(decode_file(&junk).is_err());
    });
    let Some(Damage::Damaged(n)) = damage(&bad) else {
        panic!("{:?}", damage(&bad));
    };
    assert!(n > 0);
    assert!(matches!(damage(&junk), Some(Damage::Unreadable(_))));

    let s = j.snapshot();
    let raw = format!("{s:?}");
    // Keine Rohwarnung je Frame, nur die Klartextzeilen (zweites Lesen ohne Dublette).
    assert!(!raw.contains("main_data_begin"), "{raw}");
    let local: Vec<_> = s.iter().filter(|e| e.ziel == LOCAL_TARGET).collect();
    assert_eq!(local.len(), 2, "{raw}");
    assert_eq!(local[0].stufe, Stufe::Warnung);
    assert_eq!(
        local[0].text,
        format!(
            "Datei „kaputt.mp3“ ist beschädigt ({n} fehlerhafte Stellen), spielt aber. Tipp: neu exportieren."
        )
    );
    assert!(local[0].datei.as_deref().unwrap().ends_with("kaputt.mp3"));
    assert!(
        local[1]
            .text
            .starts_with("Datei „muell.mp3“ ist nicht lesbar (")
    );
}
