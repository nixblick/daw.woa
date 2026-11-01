//! Ausgänge je Gerät, offline und ohne Ton: Die Cart-Wall geht auf ein eigenes Kanalpaar
//! oder über einen Ring auf ein zweites Gerät, die Playlist bleibt auf dem Hauptausgang.
//! Jeder Block läuft unter `assert_no_alloc`.

use std::path::Path;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_engine::pairs::{PairBuf, link};
use woa_radio::routing::{Router, Routes};
use woa_radio::{Controller, Mixer, PlaylistEntry, RadioShow};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const SR: u32 = 48_000;
const BLOCK: usize = 256;

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

/// Musik 5 s mit 0,2, Jingle 1 s mit 0,3 auf Cart 1 (Index 0).
fn setup(dir: &Path) -> (Controller, Mixer) {
    let music = dir.join("musik.wav");
    let jingle = dir.join("jingle.wav");
    write_dc(&music, 5.0, 0.2);
    write_dc(&jingle, 1.0, 0.3);
    let show = RadioShow {
        bus_processing: false,
        mic_processing: false,
        // Hier zählt die Leitung, nicht das Ducking (#149).
        cart_duck: false,
        playlist: vec![PlaylistEntry::new(&music)],
        ..RadioShow::default()
    };
    let (mut c, m) = Controller::new(show, SR);
    c.assign_cart(0, Some(jingle)).unwrap();
    (c, m)
}

/// Rechnet Blöcke; vor jedem Block ein Control-Takt. Der Jingle startet im Block `cart_at`.
fn drive(c: &mut Controller, blocks: usize, cart_at: usize, mut one: impl FnMut(usize)) {
    c.start();
    for b in 0..blocks {
        if b == cart_at {
            c.trigger_cart(0);
        }
        c.pump();
        one(b);
    }
}

#[test]
fn unrouted_router_equals_the_plain_mixer() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let (mut ca, mut ma) = setup(&a);
    let (mut cb, mb) = setup(&b);
    let mut router = Router::new(mb, 1, vec![(0, 0)], vec![]);
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let mut plain = Vec::new();
    drive(&mut ca, 220, 10, |_| {
        assert_no_alloc(|| ma.process(&mut l, &mut r));
        plain.extend_from_slice(&l);
    });
    let mut pairs = vec![PairBuf::new(4096)];
    let mut routed = Vec::new();
    drive(&mut cb, 220, 10, |_| {
        assert_no_alloc(|| router.render(&mut pairs, BLOCK));
        routed.extend_from_slice(&pairs[0].l[..BLOCK]);
    });
    assert_eq!(plain, routed);
}

#[test]
fn carts_on_their_own_pair_are_absent_from_the_main_pair() {
    let tmp = tempfile::tempdir().unwrap();
    let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    // Referenz: alles auf einem Ausgang.
    let (mut ca, mut ma) = setup(&a);
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let mut mixed = Vec::new();
    drive(&mut ca, 220, 10, |_| {
        assert_no_alloc(|| ma.process(&mut l, &mut r));
        mixed.extend_from_slice(&l);
    });
    // Cart-Wall auf Paar 1 desselben Geräts.
    let (mut cb, mb) = setup(&b);
    cb.set_routes(Routes {
        dest: [0, 1, 0, 0, 0, 0],
    });
    let mut router = Router::new(mb, 2, vec![(0, 0), (1, 1)], vec![]);
    let mut pairs = vec![PairBuf::new(4096), PairBuf::new(4096)];
    let (mut main, mut cart) = (Vec::new(), Vec::new());
    drive(&mut cb, 220, 10, |_| {
        assert_no_alloc(|| router.render(&mut pairs, BLOCK));
        main.extend_from_slice(&pairs[0].l[..BLOCK]);
        cart.extend_from_slice(&pairs[1].l[..BLOCK]);
    });
    let (start, end) = (10 * BLOCK, 10 * BLOCK + SR as usize);
    // Das Cart liegt nur auf Paar 1 und nur, solange es spielt.
    assert!(cart[..start - 1].iter().all(|v| *v == 0.0));
    assert!(
        (cart[start + 100] - 0.3).abs() < 1e-4,
        "{}",
        cart[start + 100]
    );
    assert!(cart[end + BLOCK..].iter().all(|v| *v == 0.0));
    // Die Playlist bleibt auf Paar 0, mit und ohne Cart gleich laut.
    let music = main[start - 100];
    assert!(music > 0.1);
    assert!(
        (main[start + 100] - music).abs() < 1e-5,
        "Cart im Hauptausgang"
    );
    assert!((main[end - 100] - music).abs() < 1e-5);
    // Zusammen ergibt es den bisherigen Mix.
    for i in 0..mixed.len() {
        assert!(
            (main[i] + cart[i] - mixed[i]).abs() < 1e-4,
            "Frame {i}: {} + {} != {}",
            main[i],
            cart[i],
            mixed[i]
        );
    }
}

#[test]
fn carts_on_a_second_device_arrive_through_the_ring_and_not_on_the_main_pair() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, m) = setup(tmp.path());
    c.set_routes(Routes {
        dest: [0, 1, 0, 0, 0, 0],
    });
    // Zweites Gerät mit 44,1 kHz: eigener Takt, der Ring gleicht aus.
    let (tx, mut rx, stats) = link(SR, 44_100);
    let mut router = Router::new(m, 2, vec![(0, 0)], vec![(1, tx)]);
    let mut pairs = vec![PairBuf::new(4096)];
    let (mut second_l, mut second_r) = (vec![0.0f32; 235], vec![0.0f32; 235]);
    let (mut main, mut second) = (Vec::new(), Vec::new());
    drive(&mut c, 100, 0, |_| {
        assert_no_alloc(|| {
            router.render(&mut pairs, BLOCK);
            // Das zweite Gerät holt in seinem eigenen Takt (44,1 kHz, andere Blockgröße).
            rx.pull(&mut second_l, &mut second_r);
        });
        main.extend_from_slice(&pairs[0].l[..BLOCK]);
        second.extend_from_slice(&second_l);
    });
    // Nach dem Einschwingen kommt das Cart (0,3) auf dem zweiten Gerät an …
    let loud = second.iter().filter(|v| (**v - 0.3).abs() < 1e-3).count();
    assert!(loud > 20_000, "{loud}");
    // … und der Hauptausgang trägt nur die Musik (0,2), nie das Cart.
    let music = main[2 * BLOCK];
    assert!(music > 0.1);
    assert!(
        main[2 * BLOCK..].iter().all(|v| (*v - music).abs() < 1e-4),
        "Cart im Hauptausgang"
    );
    assert_eq!(stats.faults(), 0);
}

#[test]
fn a_route_to_a_missing_destination_keeps_the_cart_in_the_main_output() {
    // Zeigt das Ziel ins Leere (mehr Ziel-Nummern als Puffer), bleibt das Cart im Hauptausgang.
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, m) = setup(tmp.path());
    c.set_routes(Routes {
        dest: [0, 7, 0, 0, 0, 0],
    });
    let mut router = Router::new(m, 1, vec![(0, 0)], vec![]);
    let mut pairs = vec![PairBuf::new(4096)];
    let mut main = Vec::new();
    drive(&mut c, 20, 2, |_| {
        assert_no_alloc(|| router.render(&mut pairs, BLOCK));
        main.extend_from_slice(&pairs[0].l[..BLOCK]);
    });
    assert!(
        main[3 * BLOCK] - main[BLOCK] > 0.25,
        "{} {}",
        main[3 * BLOCK],
        main[BLOCK]
    );
}
