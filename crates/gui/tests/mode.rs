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
