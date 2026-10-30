//! Der Modus (Produzent/Radio) steht in config.json; alte Dateien ohne Feld starten im
//! Produzent-Modus.

use woa_gui::{GuiConfig, Mode};

#[test]
fn mode_is_saved_and_defaults_to_producer() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("config.json");
    std::fs::write(&p, r#"{"install_id": "x"}"#).unwrap();
    assert_eq!(GuiConfig::load(&p).mode, Mode::Produzent);

    let c = GuiConfig {
        mode: Mode::Radio,
        ..GuiConfig::default()
    };
    c.save(&p).unwrap();
    assert!(
        std::fs::read_to_string(&p)
            .unwrap()
            .contains(r#""mode": "radio""#)
    );
    assert_eq!(GuiConfig::load(&p).mode, Mode::Radio);
}

#[test]
fn party_mode_is_saved_and_uses_the_broadcast_desk() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("config.json");
    let c = GuiConfig {
        mode: Mode::Party,
        ..GuiConfig::default()
    };
    c.save(&p).unwrap();
    assert!(
        std::fs::read_to_string(&p)
            .unwrap()
            .contains(r#""mode": "party""#)
    );
    assert_eq!(GuiConfig::load(&p).mode, Mode::Party);
    assert!(Mode::Party.uses_desk() && Mode::Radio.uses_desk() && !Mode::Produzent.uses_desk());
}
