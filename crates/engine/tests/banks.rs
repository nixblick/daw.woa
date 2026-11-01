//! Paket J7 (#271): Bank-Katalog, Prüfsummen-Test, Rückfall auf GeneralUser GS.
//! Ohne Netz: Installiert wird aus einer Datei, die Bank ist eine winzige, im Test erzeugte SF2.

use std::sync::Arc;

use sha2::{Digest, Sha256};
use woa_engine::banks::{self, BankEntry, BankError};
use woa_engine::soundfont::{SOUNDFONT_ENV, default_soundfont, soundfont_for};
use woa_model::{SoundBank, TrackSound};

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn tiny_entry(bank: SoundBank, sha256: String) -> BankEntry {
    let bytes = woa_dsp::soundfont::tiny_sf2();
    BankEntry {
        bank,
        file: banks::entry(bank).unwrap().file.clone(),
        bytes: bytes.len() as u64,
        sha256,
        url: "https://example.org/bank.sf2".into(),
        license_file: banks::entry(bank).unwrap().license_file.clone(),
    }
}

#[test]
fn catalog_has_every_bank_with_https_hash_and_license() {
    for bank in SoundBank::ALL {
        let e = banks::entry(bank).unwrap_or_else(|| panic!("{bank:?} fehlt im Katalog"));
        assert!(e.url.starts_with("https://"), "{}", e.url);
        assert_eq!(e.sha256.len(), 64);
        assert!(e.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(e.bytes > 1_000_000);
        assert!(
            e.file.ends_with(".sf2"),
            "SF3 lädt rustysynth nicht: {}",
            e.file
        );
        assert!(!banks::license_text(bank).is_empty());
    }
    assert_eq!(banks::catalog().len(), SoundBank::ALL.len());
    assert!(banks::entry(SoundBank::Unknown).is_none());
}

#[test]
fn only_https_is_downloaded() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = tiny_entry(SoundBank::MusescoreGeneral, "0".repeat(64));
    e.url = "http://127.0.0.1:9/bank.sf2".into();
    assert!(matches!(
        banks::download(&e, dir.path(), |_| {}),
        Err(BankError::NotHttps)
    ));
    assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
}

#[test]
fn install_checks_the_hash_and_keeps_nothing_on_mismatch() {
    let bytes = woa_dsp::soundfont::tiny_sf2();
    let dir = tempfile::tempdir().unwrap();

    let bad = tiny_entry(SoundBank::MusescoreGeneral, "0".repeat(64));
    let r = banks::install(&bad, bytes.as_slice(), dir.path(), |_| {});
    assert!(matches!(r, Err(BankError::Hash { .. })), "{r:?}");
    assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());

    let good = tiny_entry(SoundBank::MusescoreGeneral, hex(&bytes));
    let mut seen = 0;
    let path = banks::install(&good, bytes.as_slice(), dir.path(), |n| seen = n).unwrap();
    assert_eq!(seen, bytes.len() as u64);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(path, banks::bank_path(dir.path(), &good));
    let license = dir.path().join(&good.license_file);
    assert!(std::fs::read_to_string(license).unwrap().contains("MIT"));
    // Zu gross ist auch falsch, ohne bis zum Ende zu lesen.
    let mut long = bytes.clone();
    long.extend_from_slice(&[0; 16]);
    let other = tempfile::tempdir().unwrap();
    assert!(banks::install(&good, long.as_slice(), other.path(), |_| {}).is_err());
    assert!(std::fs::read_dir(other.path()).unwrap().next().is_none());
}

#[test]
fn a_track_plays_its_bank_and_falls_back_to_generaluser() {
    let bytes = woa_dsp::soundfont::tiny_sf2();
    let tmp = tempfile::tempdir().unwrap();
    let general = tmp.path().join("GeneralUser-GS.sf2");
    std::fs::write(&general, &bytes).unwrap();
    // SAFETY: einziger Test dieser Datei, der die Umgebung setzt.
    unsafe { std::env::set_var(SOUNDFONT_ENV, &general) };
    let dir = tmp.path().join("banks");
    banks::set_bank_dir(dir.clone());
    let default = default_soundfont().unwrap();
    let piano = TrackSound {
        program: 0,
        drums: false,
        bank: Some(SoundBank::MusescoreGeneral),
    };
    // Ohne Netz und ohne Datei: GeneralUser GS.
    assert_eq!(
        banks::state(SoundBank::MusescoreGeneral),
        banks::BankState::Missing
    );
    assert!(Arc::ptr_eq(&soundfont_for(piano).unwrap(), &default));

    let entry = tiny_entry(SoundBank::MusescoreGeneral, hex(&bytes));
    let before = banks::generation();
    banks::install(&entry, bytes.as_slice(), &dir, |_| {}).unwrap();
    banks::mark_ready(SoundBank::MusescoreGeneral);
    assert!(banks::generation() > before);
    assert_eq!(
        banks::state(SoundBank::MusescoreGeneral),
        banks::BankState::Ready
    );
    let font = soundfont_for(piano).unwrap();
    assert!(
        !Arc::ptr_eq(&font, &default),
        "die Bank, nicht GeneralUser GS"
    );
    assert!(
        Arc::ptr_eq(&soundfont_for(piano).unwrap(), &font),
        "einmal geladen"
    );

    // Unbekannte Bank und reine Drum-Bank für Melodie: GeneralUser GS.
    let unknown = TrackSound {
        bank: Some(SoundBank::Unknown),
        ..piano
    };
    assert!(Arc::ptr_eq(&soundfont_for(unknown).unwrap(), &default));
    let kit = tiny_entry(SoundBank::AvlRedZeppelin, hex(&bytes));
    banks::install(&kit, bytes.as_slice(), &dir, |_| {}).unwrap();
    let melodic = TrackSound {
        bank: Some(SoundBank::AvlRedZeppelin),
        ..piano
    };
    assert!(Arc::ptr_eq(&soundfont_for(melodic).unwrap(), &default));
    let drums = TrackSound {
        drums: true,
        ..melodic
    };
    assert!(!Arc::ptr_eq(&soundfont_for(drums).unwrap(), &default));
}

/// Probe mit den echten Bänken, nicht in der CI: `WOA_BANK_PROBE_DIR` zeigt auf einen Ordner
/// mit den Originaldateien (Namen wie im Katalog). Prüft die Katalog-Prüfsumme an der echten
/// Datei und rendert offline Klavier (MuseScore) und Kick/Snare/Hi-Hat (AVL, GM-Noten 36/38/42).
#[test]
#[ignore = "braucht die echten Bänke"]
fn real_banks_match_the_catalog_and_sound() {
    use woa_dsp::instrument::{Instrument, NoteEvent};
    use woa_dsp::soundfont::{SoundFontPlayer, load_soundfont};
    let src = std::path::PathBuf::from(std::env::var_os("WOA_BANK_PROBE_DIR").unwrap());
    let dir = tempfile::tempdir().unwrap();
    for (bank, channel, keys) in [
        (SoundBank::MusescoreGeneral, 0u8, vec![60u8]),
        (SoundBank::AvlRedZeppelin, 9, vec![36, 38, 42]),
    ] {
        let e = banks::entry(bank).unwrap();
        let file = std::fs::File::open(src.join(&e.file)).unwrap();
        let path = banks::install(e, std::io::BufReader::new(file), dir.path(), |_| {}).unwrap();
        let font = load_soundfont(&mut std::io::BufReader::new(
            std::fs::File::open(path).unwrap(),
        ))
        .unwrap();
        for key in keys {
            let mut p = SoundFontPlayer::new(font.clone(), 48_000, 256).unwrap();
            p.handle_event(&NoteEvent::note_on(key, 110).on_channel(channel));
            let mut out = vec![0.0f32; 24_000];
            p.render(&mut out);
            let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            println!("{bank:?} key {key}: peak {peak:.3}");
            assert!(peak > 0.01, "{bank:?} key {key} stumm");
        }
    }
}

/// Echter Download über https, nicht in der CI: `WOA_BANK_PROBE_NET=1`. Lädt die kleine
/// AVL-Bank (26 MB) und prüft die Prüfsumme.
#[test]
#[ignore = "braucht Netz"]
fn real_download_of_the_small_bank() {
    assert!(std::env::var_os("WOA_BANK_PROBE_NET").is_some());
    let dir = tempfile::tempdir().unwrap();
    let e = banks::entry(SoundBank::AvlRedZeppelin).unwrap();
    let path = banks::download(e, dir.path(), |_| {}).unwrap();
    assert_eq!(std::fs::metadata(path).unwrap().len(), e.bytes);
}
