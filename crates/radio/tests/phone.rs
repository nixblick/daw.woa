//! Telefon offline und ohne Ton: Der Anrufer liegt nur auf Sendung, wenn der Moderator es will,
//! und der Rückweg (Mix-Minus) trägt die Sendung ohne seine Stimme. Jeder Block läuft unter
//! `assert_no_alloc`.

use std::path::Path;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_engine::pairs::{LinkTx, PairBuf, link};
use woa_radio::routing::{Router, Routes, SOURCES, Source};
use woa_radio::{Controller, Cues, RadioShow};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const SR: u32 = 48_000;
const BLOCK: usize = 256;
const PROGRAM: f32 = 0.25;
const CALLER: f32 = 0.5;
/// Pegel des Programms (Deck A) am Ausgang: Gleichwert mit seinem Auto-Gain (#297).
static PROGRAM_OUT: std::sync::LazyLock<f32> = std::sync::LazyLock::new(|| {
    let tmp = tempfile::tempdir().unwrap();
    let f = tmp.path().join("programm.wav");
    write_dc(&f, 30.0, PROGRAM);
    let m = woa_radio::audio::measure_file(&f, SR).unwrap();
    let g = woa_radio::level::auto_gain_db(m.lufs, m.peak_db, woa_radio::level::TARGET_LUFS);
    PROGRAM * woa_radio::level::db_to_gain(g)
});

fn write_dc(path: &Path, seconds: f64, amp: f32) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SR,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for _ in 0..(seconds * f64::from(SR)) as usize * 2 {
        w.write_sample(amp).unwrap();
    }
    w.finalize().unwrap();
}

/// Eine Sendung (Deck A läuft mit 0,25), Rückweg auf Ziel 1 (oder keiner) und Router mit zwei
/// Paaren. Der Anrufer ist ein Gleichwert, der als Taktverbindung ankommt (`None` = kein Telefon).
struct Rig {
    c: Controller,
    router: Router,
    pairs: Vec<PairBuf>,
    caller: Option<LinkTx>,
    level: f32,
    _dir: tempfile::TempDir,
}

/// Letzte Werte von Hauptausgang und Rückweg nach einem Lauf.
struct Heard {
    main: f32,
    ret: f32,
    ret_energy: f32,
}

impl Rig {
    fn new(return_on: bool, caller: Option<f32>, caller_rate: u32) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("programm.wav");
        write_dc(&file, 30.0, PROGRAM);
        let show = RadioShow {
            bus_processing: false,
            mic_processing: false,
            ..RadioShow::default()
        };
        let (mut c, m) = Controller::new(show, SR);
        let mut dest = [0u8; SOURCES];
        if return_on {
            dest[Source::PhoneReturn.index()] = 1;
        }
        c.set_routes(Routes { dest });
        let mut tx = None;
        if caller.is_some() {
            let (t, rx, _stats) = link(caller_rate, SR);
            c.set_phone_input(Some(rx));
            tx = Some(t);
        }
        c.deck_load(0, file, Cues::default()).unwrap();
        c.deck_play(0);
        let router = Router::new(m, 2, vec![(0, 0), (1, 1)], vec![]);
        Self {
            c,
            router,
            pairs: vec![PairBuf::new(4096), PairBuf::new(4096)],
            caller: tx,
            level: caller.unwrap_or(0.0),
            _dir: dir,
        }
    }

    /// Rechnet `blocks` Blöcke. Der Anrufer liefert, was in einem Block verbraucht wird
    /// (bei anderer Eingangsrate entsprechend mehr oder weniger), und ein Polster vorab.
    fn run(&mut self, blocks: usize, caller_rate: u32) -> Heard {
        let per_block = (BLOCK as u64 * u64::from(caller_rate) / u64::from(SR)) as usize;
        let mut heard = Heard {
            main: 0.0,
            ret: 0.0,
            ret_energy: 0.0,
        };
        if let Some(tx) = self.caller.as_mut() {
            let v = vec![self.level; caller_rate as usize / 10];
            tx.push(&v, &v);
        }
        let chunk = vec![self.level; per_block];
        for _ in 0..blocks {
            if let Some(tx) = self.caller.as_mut() {
                tx.push(&chunk, &chunk);
            }
            self.c.pump();
            assert_no_alloc(|| self.router.render(&mut self.pairs, BLOCK));
            heard.main = self.pairs[0].l[BLOCK - 1];
            heard.ret = self.pairs[1].l[BLOCK - 1];
            heard.ret_energy += self.pairs[1].l[..BLOCK].iter().map(|v| v * v).sum::<f32>();
        }
        heard
    }

    fn phone_peak(&self) -> f32 {
        self.c.status().take_phone_peak()
    }
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn return_carries_the_program_but_never_the_caller() {
    let mut rig = Rig::new(true, Some(CALLER), SR);
    rig.c.set_phone_on(true);
    let h = rig.run(200, SR);
    // Auf Sendung liegen Programm und Anrufer; der Rückweg trägt nur das Programm.
    assert!(
        near(h.main, *PROGRAM_OUT + CALLER),
        "Hauptausgang {}",
        h.main
    );
    assert!(near(h.ret, *PROGRAM_OUT), "Rückweg {}", h.ret);
}

#[test]
fn without_a_caller_signal_the_return_is_exactly_the_program() {
    // Telefon angeschlossen, aber der Anrufer sagt nichts (Stille).
    let mut rig = Rig::new(true, Some(0.0), SR);
    rig.c.set_phone_on(true);
    let h = rig.run(200, SR);
    assert!(
        near(h.main, *PROGRAM_OUT) && near(h.ret, *PROGRAM_OUT),
        "{} {}",
        h.main,
        h.ret
    );
    // Kein Telefon eingesteckt: derselbe Rückweg, derselbe Hauptausgang.
    let mut rig = Rig::new(true, None, SR);
    rig.c.set_phone_on(true);
    let h = rig.run(200, SR);
    assert!(near(h.main, *PROGRAM_OUT) && near(h.ret, *PROGRAM_OUT));
    assert!(h.ret_energy > 0.0);
}

#[test]
fn caller_off_air_is_metered_but_neither_on_air_nor_in_the_return() {
    let mut rig = Rig::new(true, Some(CALLER), SR);
    let h = rig.run(200, SR);
    assert!(near(h.main, *PROGRAM_OUT), "Hauptausgang {}", h.main);
    assert!(near(h.ret, *PROGRAM_OUT), "Rückweg {}", h.ret);
    assert!(near(rig.phone_peak(), CALLER), "Pegel {}", rig.phone_peak());
    // Dann auf Sendung: ab jetzt im Hauptausgang, der Rückweg bleibt rein.
    rig.c.set_phone_on(true);
    let h = rig.run(200, SR);
    assert!(near(h.main, *PROGRAM_OUT + CALLER));
    assert!(near(h.ret, *PROGRAM_OUT));
    rig.c.set_phone_on(false);
    let h = rig.run(200, SR);
    assert!(near(h.main, *PROGRAM_OUT) && near(h.ret, *PROGRAM_OUT));
}

#[test]
fn caller_from_another_card_with_another_rate_arrives_and_stays_out_of_the_return() {
    // Karte mit 44,1 kHz gegen 48 kHz: die Taktverbindung rechnet um.
    let mut rig = Rig::new(true, Some(CALLER), 44_100);
    rig.c.set_phone_on(true);
    let h = rig.run(400, 44_100);
    assert!(
        near(h.main, *PROGRAM_OUT + CALLER),
        "Hauptausgang {}",
        h.main
    );
    assert!(near(h.ret, *PROGRAM_OUT), "Rückweg {}", h.ret);
}

#[test]
fn without_a_return_route_nothing_reaches_the_return_pair() {
    let mut rig = Rig::new(false, Some(CALLER), SR);
    rig.c.set_phone_on(true);
    let h = rig.run(100, SR);
    assert!(near(h.main, *PROGRAM_OUT + CALLER));
    assert_eq!(h.ret_energy, 0.0, "kein Rückweg gewählt");
}

#[test]
fn phone_on_air_ducks_the_music_like_the_microphone() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("musik.wav");
    write_dc(&music, 20.0, 0.4);
    let show = RadioShow {
        bus_processing: false,
        mic_processing: false,
        playlist: vec![woa_radio::PlaylistEntry::new(&music)],
        ..RadioShow::default()
    };
    let (mut c, mut m) = Controller::new(show, SR);
    let (mut tx, rx, _s) = link(SR, SR);
    c.set_phone_input(Some(rx));
    c.start();
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let silence = vec![0.0f32; BLOCK];
    tx.push(&vec![0.0f32; 8000], &vec![0.0f32; 8000]);
    let mut last = 0.0;
    for b in 0..400 {
        if b == 100 {
            c.set_phone_on(true);
        }
        tx.push(&silence, &silence);
        c.pump();
        assert_no_alloc(|| m.process(&mut l, &mut r));
        if b == 99 {
            last = l[BLOCK - 1];
        }
    }
    assert!(last > 0.1, "vorher {last}");
    assert!(
        l[BLOCK - 1] < last * 0.5,
        "abgesenkt {} gegen {last}",
        l[BLOCK - 1]
    );
}
