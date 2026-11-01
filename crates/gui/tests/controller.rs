//! Paket 14 ohne Ton: ein virtueller ALSA-Port spielt den Controller gegen den echten
//! Sendemixer. Gelernte Pads lösen `trigger_cart` aus, der Fader setzt den Musik-Pegel,
//! der Mixer rechnet jeden Block unter `assert_no_alloc`. Gezählt wird in der Oberfläche
//! (Zähler je Aktion) und im Mixer (Cart-Bit, Pegel am Ausgang). Die Zuordnung übersteht
//! einen Neustart (config.json) und ein Wieder-Einstecken mit neuer ALSA-Nummer.
//! Ohne ALSA-Sequencer (z. B. CI-Container) wird der Port-Teil übersprungen.

use assert_no_alloc::AllocDisabler;
use woa_gui::GuiConfig;
use woa_gui::controls::ControlDesk;
use woa_io::controller::{Bus, Mapping, Source, SourceKind, Target};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

#[test]
fn mappings_survive_restart_and_new_alsa_number() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.json");
    let mut config = GuiConfig {
        controller_input: Some("nanoKONTROL2:nanoKONTROL2 MIDI 1 24:0".into()),
        ..GuiConfig::default()
    };
    config.controller_maps.insert(
        "nanoKONTROL2:nanoKONTROL2 MIDI 1".into(),
        vec![Mapping {
            source: Source {
                channel: 0,
                kind: SourceKind::Cc(0),
            },
            target: Target::Level(Bus::Music),
        }],
    );
    config.save(&path).unwrap();
    let mut loaded = GuiConfig::load(&path);
    assert_eq!(loaded.controller_maps, config.controller_maps);
    // Wieder eingesteckt mit anderer Client-Nummer: dieselbe Zuordnung.
    loaded.controller_input = Some("nanoKONTROL2:nanoKONTROL2 MIDI 1 28:0".into());
    let desk = ControlDesk::new(&loaded);
    assert_eq!(desk.mapper.mappings().len(), 1);
    assert_eq!(desk.mapper.mappings()[0].target, Target::Level(Bus::Music));
}

#[cfg(target_os = "linux")]
mod port {
    use std::path::Path;
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    use assert_no_alloc::assert_no_alloc;
    use woa_gui::controls::{Surface, level_from_norm, level_to_norm};
    use woa_radio::mixer::LevelBus;
    use woa_radio::{Controller, Mixer, PlaylistEntry, RadioShow};

    use super::*;

    const SR: u32 = 48_000;
    const BLOCK: usize = 256;

    /// 16-Bit-WAV, Stereo, Gleichwert `amp` (kein Hören nötig: der Pegel wird gemessen).
    fn write_dc(path: &Path, seconds: f64, amp: f32) {
        let frames = (seconds * f64::from(SR)) as u32;
        let data = frames * 4;
        let mut b = Vec::with_capacity(44 + data as usize);
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&SR.to_le_bytes());
        b.extend_from_slice(&(SR * 4).to_le_bytes());
        b.extend_from_slice(&4u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data.to_le_bytes());
        let v = (amp * 32767.0) as i16;
        for _ in 0..frames * 2 {
            b.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(path, b).unwrap();
    }

    /// Die Oberfläche im Test: reicht an den echten Sende-Controller weiter und zählt.
    struct Desk {
        c: Controller,
        level: f64,
        carts: u32,
        starts: u32,
        sets: u32,
    }

    impl Surface for Desk {
        fn current(&self, t: &Target) -> Option<f32> {
            matches!(t, Target::Level(Bus::Music)).then(|| level_to_norm(self.level))
        }
        fn set(&mut self, t: Target, v: f32) {
            if t == Target::Level(Bus::Music) {
                self.sets += 1;
                self.level = level_from_norm(v);
                self.c.set_level(LevelBus::Music, self.level);
            }
        }
        fn press(&mut self, t: Target) {
            match t {
                Target::Cart(slot) => {
                    self.carts += 1;
                    self.c.trigger_cart(slot);
                }
                Target::StartNext => {
                    self.starts += 1;
                    self.c.start_or_next();
                }
                _ => {}
            }
        }
        fn lit(&self, t: &Target) -> bool {
            match t {
                Target::Cart(s) => self.c.status().carts.load(Ordering::Acquire) & (1 << s) != 0,
                _ => false,
            }
        }
    }

    fn block(d: &mut Desk, m: &mut Mixer, l: &mut [f32], r: &mut [f32]) {
        d.c.pump();
        assert_no_alloc(|| m.process(l, r));
        d.c.pump();
    }

    #[test]
    fn virtual_controller_drives_mixer_and_carts() {
        use midir::MidiOutput;
        use midir::os::unix::VirtualOutput;
        const PORT: &str = "test-controller-gui-p14";

        let Some(mut dev) = MidiOutput::new(PORT)
            .ok()
            .and_then(|o| o.create_virtual(PORT).ok())
        else {
            eprintln!("kein ALSA-Sequencer, Test übersprungen");
            return;
        };
        let tmp = tempfile::tempdir().unwrap();
        let music = tmp.path().join("musik.wav");
        let jingle = tmp.path().join("jingle.wav");
        write_dc(&music, 5.0, 0.4);
        write_dc(&jingle, 3.0, 0.2);

        let show = RadioShow {
            bus_processing: false,
            mic_processing: false,
            playlist: vec![PlaylistEntry::new(&music)],
            ..RadioShow::default()
        };
        let (mut c, mut m) = Controller::new(show, SR);
        c.assign_cart(1, Some(jingle)).unwrap();
        let mut d = Desk {
            c,
            level: 0.0,
            carts: 0,
            starts: 0,
            sets: 0,
        };

        // Gerät wählen wie im Fenster, dann lernen.
        let mut config = GuiConfig::default();
        let mut desk = ControlDesk::new(&config);
        desk.poll(Instant::now());
        let name = desk
            .ports()
            .iter()
            .find(|p| p.starts_with(PORT))
            .cloned()
            .expect("virtueller Port gelistet");
        desk.select(Some(name), &mut config);

        let wait = |desk: &mut ControlDesk,
                    d: &mut Desk,
                    config: &mut GuiConfig,
                    f: &dyn Fn(&Desk, &ControlDesk) -> bool| {
            let end = Instant::now() + Duration::from_secs(3);
            while Instant::now() < end {
                desk.frame(d, config);
                if f(d, desk) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            false
        };

        for (target, bytes) in [
            (Target::Cart(1), [0x99u8, 37, 100]),
            (Target::StartNext, [0x99, 40, 100]),
            (Target::Level(Bus::Music), [0xB0, 0, 99]),
        ] {
            desk.mapper.learn(target);
            dev.send(&bytes).unwrap();
            assert!(wait(&mut desk, &mut d, &mut config, &|_, k| k
                .mapper
                .learning()
                .is_none()));
        }
        assert_eq!(desk.mapper.mappings().len(), 3);
        assert_eq!(
            config.controller_maps.values().next().map(Vec::len),
            Some(3),
            "gelernt = in config.json"
        );

        // Pad „Start“: Musik läuft.
        dev.send(&[0x99, 40, 100]).unwrap();
        assert!(wait(&mut desk, &mut d, &mut config, &|d, _| d.starts == 1));
        let mut l = vec![0.0f32; BLOCK];
        let mut r = vec![0.0f32; BLOCK];
        for _ in 0..20 {
            block(&mut d, &mut m, &mut l, &mut r);
        }
        let full = l[BLOCK - 1];
        assert!(full > 0.05, "Musik läuft: {full}");

        // Fader steht nach dem Lernen auf 99, der Pegel auf 0 dB (= 0,909): Takeover. Erst
        // unter 0 dB, dann bis −6 dB herunter (Wert 0,818 = 103,9 → 104 ist ~ -5,9 dB).
        for v in [100u8, 110, 116, 104] {
            dev.send(&[0xB0, 0, v]).unwrap();
        }
        assert!(wait(&mut desk, &mut d, &mut config, &|d, _| (d.level
            - level_from_norm(104.0 / 127.0))
        .abs()
            < 1e-6));
        assert!(d.sets >= 1);
        for _ in 0..40 {
            block(&mut d, &mut m, &mut l, &mut r);
        }
        let want = full * 10f32.powf(level_from_norm(104.0 / 127.0) as f32 / 20.0);
        assert!(
            (l[BLOCK - 1] - want).abs() < 2e-3,
            "{} vs {want}",
            l[BLOCK - 1]
        );

        // Pad „Cart 2“: trigger_cart, der Mixer spielt ihn (Bit gesetzt, Pegel steigt).
        dev.send(&[0x99, 37, 100]).unwrap();
        assert!(wait(&mut desk, &mut d, &mut config, &|d, _| d.carts == 1));
        // Ein Cart senkt die Musik ab (#149, Standard −12 dB, Zeitkonstante 80 ms): erst
        // ausschwingen lassen (64 Blöcke ≈ 340 ms), dann gegen die abgesenkte Musik messen.
        for _ in 0..64 {
            block(&mut d, &mut m, &mut l, &mut r);
        }
        assert_ne!(
            d.c.status().carts.load(Ordering::Acquire) & 0b10,
            0,
            "Cart 2 läuft"
        );
        let ducked = want * 10f32.powf(RadioShow::default().duck_db as f32 / 20.0);
        let with_cart = l[BLOCK - 1];
        assert!(
            with_cart > ducked + 0.15,
            "Cart hörbar im Pegel: {with_cart} (Musik abgesenkt {ducked})"
        );
        assert!(
            with_cart < want + 0.15,
            "Musik unter dem Cart abgesenkt: {with_cart} (Musik {want})"
        );
        // Zweites Drücken hält ihn an (wie ein Klick auf die Cart-Taste).
        dev.send(&[0x89, 37, 0]).unwrap();
        dev.send(&[0x99, 37, 100]).unwrap();
        assert!(wait(&mut desk, &mut d, &mut config, &|d, _| d.carts == 2));
        for _ in 0..4 {
            block(&mut d, &mut m, &mut l, &mut r);
        }
        assert_eq!(
            d.c.status().carts.load(Ordering::Acquire) & 0b10,
            0,
            "Cart 2 steht"
        );
        eprintln!(
            "Zähler: {} Cart, {} Start, {} Fader-Werte; Ausgang {:.3} (voll {full:.3})",
            d.carts,
            d.starts,
            d.sets,
            l[BLOCK - 1]
        );
    }
}
