//! Offline-Probe ohne Audiogerät: zwei Sinus-Gäste und Musik, Rückwege prüfen, keine
//! Allokation im Audio-Weg, Puffer-Über- und Unterlauf.

use std::sync::atomic::Ordering;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_guests::source::SineGen;
use woa_guests::{GuestBank, GuestFeed, GuestHub, channel};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const RATE: u32 = 48_000;
const BLOCK: usize = 480;

/// Amplitude einer Frequenz im Signal (Goertzel), linear.
fn tone(x: &[f32], freq: f32) -> f32 {
    let w = std::f64::consts::TAU * f64::from(freq) / f64::from(RATE);
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, &v) in x.iter().enumerate() {
        re += f64::from(v) * (w * i as f64).cos();
        im += f64::from(v) * (w * i as f64).sin();
    }
    (2.0 * (re * re + im * im).sqrt() / x.len() as f64) as f32
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-12).log10()
}

struct Rig {
    hub: GuestHub,
    bank: GuestBank,
    feeds: Vec<GuestFeed>,
    gens: Vec<SineGen>,
    music: SineGen,
}

/// Zwei Gäste (440 und 660 Hz) und Musik (880 Hz) mit eigenem Takt.
fn rig(guest_freqs: &[f32]) -> Rig {
    let (mut hub, bank) = channel(RATE);
    let mut feeds = Vec::new();
    for _ in guest_freqs {
        let (_slot, f) = hub.add_external("Test").unwrap();
        feeds.push(f);
    }
    Rig {
        hub,
        bank,
        feeds,
        gens: guest_freqs
            .iter()
            .map(|&f| SineGen::new(f, RATE, 0.3))
            .collect(),
        music: SineGen::new(880.0, RATE, 0.3),
    }
}

/// Wie [`rig`], aber Browser-Gäste: sie kommen in den Warteraum.
fn rig_net(guest_freqs: &[f32]) -> Rig {
    let (mut hub, bank) = channel(RATE);
    let mut feeds = Vec::new();
    for (i, _) in guest_freqs.iter().enumerate() {
        let g = hub.add_network("Browser", &format!("G{i}")).unwrap();
        feeds.push(g.feed);
    }
    Rig {
        hub,
        bank,
        feeds,
        gens: guest_freqs
            .iter()
            .map(|&f| SineGen::new(f, RATE, 0.3))
            .collect(),
        music: SineGen::new(880.0, RATE, 0.3),
    }
}

impl Rig {
    /// Vorlauf: 70 ms, mehr als der Puffer zum Start braucht.
    fn prefill(&mut self) {
        for _ in 0..7 {
            self.feed_block();
        }
    }

    /// Schreibt einen Block in jeden Gast-Feed.
    fn feed_block(&mut self) {
        let mut buf = vec![0.0f32; BLOCK * 2];
        for (f, g) in self.feeds.iter_mut().zip(self.gens.iter_mut()) {
            g.fill(&mut buf);
            f.push(&buf);
        }
    }

    /// Rechnet `blocks` Blöcke; gibt (Rückweg je Gast links, Sendesumme links) zurück.
    fn run(&mut self, blocks: usize) -> (Vec<Vec<f32>>, Vec<f32>) {
        let n = self.feeds.len();
        let mut returns = vec![Vec::new(); n];
        let mut prog = Vec::new();
        let mut music = vec![0.0f32; BLOCK * 2];
        for _ in 0..blocks {
            self.feed_block();
            self.music.fill(&mut music);
            self.bank.begin_block();
            for i in 0..BLOCK {
                let base = (music[i * 2], music[i * 2 + 1]);
                let (gl, _gr) = assert_no_alloc(|| self.bank.frame());
                prog.push(base.0 + gl);
                for (slot, r) in returns.iter_mut().enumerate() {
                    let (l, _r) = assert_no_alloc(|| self.bank.return_bus(slot, base));
                    r.push(l);
                }
            }
            assert_no_alloc(|| self.bank.end_block());
        }
        (returns, prog)
    }
}

#[test]
fn return_of_a_contains_b_and_music_but_not_a() {
    let mut r = rig(&[440.0, 660.0]);
    // Vorlauf für beide Gäste, dann zwei Sekunden rechnen.
    r.prefill();
    let (ret, prog) = r.run(200);
    // Erste halbe Sekunde (Einblenden) auslassen, Länge auf ganze Perioden passend.
    let from = 24_000;
    let a = &ret[0][from..];
    let b = &ret[1][from..];
    let p = &prog[from..];

    let a_has = |f| db(tone(a, f));
    // Rückweg A: B und Musik mit ihrem Pegel (0,3 = -10,5 dB), A selbst unter -90 dB.
    assert!(
        a_has(660.0) > -12.0,
        "B fehlt im Rückweg A: {}",
        a_has(660.0)
    );
    assert!(a_has(880.0) > -12.0, "Musik fehlt im Rückweg A");
    assert!(
        a_has(440.0) < -90.0,
        "A hört sich selbst: {} dB",
        a_has(440.0)
    );
    // Rückweg B spiegelbildlich.
    assert!(db(tone(b, 440.0)) > -12.0 && db(tone(b, 880.0)) > -12.0);
    assert!(db(tone(b, 660.0)) < -90.0, "B hört sich selbst");
    // Die Sendung enthält alles.
    for f in [440.0, 660.0, 880.0] {
        assert!(db(tone(p, f)) > -12.0, "{f} Hz fehlt in der Sendung");
    }
}

#[test]
fn single_guest_return_is_exactly_the_music() {
    let mut r = rig(&[440.0]);
    r.prefill();
    let (ret, _prog) = r.run(100);
    let mut music = SineGen::new(880.0, RATE, 0.3);
    let mut m = vec![0.0f32; 100 * BLOCK * 2];
    // Die Musik des Rigs lief in Blöcken, hier am Stück: gleiche Phase, gleiche Werte.
    music.fill(&mut m);
    for (i, v) in ret[0].iter().enumerate() {
        assert_eq!(*v, m[i * 2], "Frame {i}");
    }
}

#[test]
fn fader_and_mute_shape_the_guest_in_the_mix() {
    let mut r = rig(&[440.0, 660.0]);
    r.prefill();
    r.hub.set_gain_db(0, -20.0);
    r.hub.set_mute(1, true);
    let (ret, prog) = r.run(200);
    let p = &prog[24_000..];
    // A um 20 dB leiser: 0,3 -> 0,03 (-30,5 dB); B stumm.
    let a = db(tone(p, 440.0));
    assert!((a + 30.5).abs() < 1.0, "A {a}");
    assert!(db(tone(p, 660.0)) < -80.0, "B stumm");
    // Stummer Gast fehlt auch im Rückweg von A.
    assert!(db(tone(&ret[0][24_000..], 660.0)) < -80.0);
}

#[test]
fn level_meter_follows_the_fader() {
    let mut r = rig(&[440.0]);
    r.prefill();
    r.run(100);
    let loud = r.hub.view()[0].peak;
    assert!((loud - 0.3).abs() < 0.02, "{loud}");
    r.hub.set_gain_db(0, -20.0);
    r.run(10);
    let _ = r.hub.view();
    r.run(100);
    let quiet = r.hub.view()[0].peak;
    assert!((quiet - 0.03).abs() < 0.005, "{quiet}");
}

#[test]
fn underrun_gives_silence_and_is_counted_and_logged() {
    let mut r = rig(&[440.0]);
    r.prefill();
    // Zwei Blöcke laufen, dann kommt nichts mehr nach: Puffer läuft leer.
    let mut music = vec![0.0f32; BLOCK * 2];
    let mut silent_tail = true;
    for b in 0..60 {
        if b < 2 {
            r.feed_block();
        }
        r.music.fill(&mut music);
        r.bank.begin_block();
        for _ in 0..BLOCK {
            let (gl, _) = assert_no_alloc(|| r.bank.frame());
            if b >= 50 && gl != 0.0 {
                silent_tail = false;
            }
        }
        r.bank.end_block();
    }
    assert!(silent_tail, "nach dem Aussetzer muss Stille kommen");
    let st = r.hub.stats(0).unwrap();
    assert_eq!(st.underruns.load(Ordering::Relaxed), 1);
    assert!(r.hub.poll().unwrap().contains("Aussetzer"));
    assert!(r.hub.poll().is_none(), "jeder Aussetzer nur einmal melden");
}

#[test]
fn overflow_is_capped_and_logged() {
    let mut r = rig(&[440.0]);
    // Das Netz liefert viel zu viel auf einmal (2 s).
    for _ in 0..200 {
        r.feed_block();
    }
    let st = r.hub.stats(0).unwrap();
    let cap = (woa_guests::jitter::MAX_SECONDS * RATE as f32) as usize;
    assert!(r.feeds[0].buffered_frames() <= cap);
    assert!(st.overflows.load(Ordering::Relaxed) > 0);
    assert!(r.hub.poll().unwrap().contains("übergelaufen"));
}

#[test]
fn removed_guest_frees_the_channel_and_the_monitor() {
    let (mut hub, mut bank) = channel(RATE);
    let (slot, feed) = hub.add_external("Test").unwrap();
    hub.set_monitor(Some(slot));
    assert_eq!(hub.monitor(), Some(slot));
    bank.begin_block();
    assert_eq!(bank.monitor(), Some(slot));
    assert!(bank.any_active());
    hub.remove(slot);
    bank.begin_block();
    assert_eq!(bank.monitor(), None);
    assert!(!bank.any_active());
    hub.return_feed(slot, feed);
    let (again, _f) = hub.add_external("Test").unwrap();
    assert_eq!(again, slot);
}

#[test]
fn eight_guests_at_most() {
    let (mut hub, _bank) = channel(RATE);
    let mut keep = Vec::new();
    for _ in 0..woa_guests::MAX_GUESTS {
        keep.push(hub.add_external("Test").unwrap());
    }
    assert!(hub.add_external("Test").is_err());
}

#[test]
fn test_source_thread_fills_the_buffer_in_real_time() {
    let (mut hub, mut bank) = channel(RATE);
    let slot = hub
        .add_test_guest(woa_guests::TestSignal::Sine { freq: 440.0 })
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(250));
    bank.begin_block();
    let mut any = false;
    for _ in 0..BLOCK {
        let (l, _) = bank.frame();
        any |= l != 0.0;
    }
    assert!(any, "Testgast liefert nach 250 ms Ton");
    assert_eq!(hub.view()[0].slot, slot);
    hub.remove(slot);
    // Kanal ist wieder frei.
    assert!(
        hub.add_test_guest(woa_guests::TestSignal::Sine { freq: 330.0 })
            .is_ok()
    );
}

#[test]
fn new_network_guest_starts_with_an_empty_return() {
    let (mut hub, mut bank) = channel(RATE);
    let a = hub.add_network("Test", "Anna").unwrap();
    let slot = a.slot;
    bank.begin_block();
    let music = (0.5f32, 0.5f32);
    for _ in 0..BLOCK {
        let _ = bank.frame();
        bank.send_returns(music, |l, r| (l, r));
    }
    bank.end_block();
    assert!(a.ret.frames() > 0, "Rückweg des ersten Gasts hat Audio");
    hub.remove(slot);
    hub.return_network(a);
    let b = hub.add_network("Test", "Ben").unwrap();
    assert_eq!(b.slot, slot);
    assert_eq!(b.ret.frames(), 0, "neuer Gast beginnt mit leerem Rückweg");
}

#[test]
fn network_guest_name_has_no_control_characters() {
    let (mut hub, _bank) = channel(RATE);
    let g = hub
        .add_network("Test", "  Mal\nory\u{1b}[31m\u{7}  ")
        .unwrap();
    let view = hub.view();
    let name = &view.iter().find(|v| v.slot == g.slot).unwrap().name;
    assert_eq!(name, "Malory[31m");
    assert!(!name.chars().any(char::is_control));
    let (_s, _f) = hub.add_external("x").unwrap();
    let h = hub.add_network("Test", "\n\u{1b}").unwrap();
    let view = hub.view();
    let n2 = &view.iter().find(|v| v.slot == h.slot).unwrap().name;
    assert!(n2.starts_with("Gast "));
}

#[test]
fn browser_guests_start_in_the_waiting_room_and_go_on_air() {
    let mut r = rig_net(&[440.0, 660.0]);
    let v = r.hub.view();
    assert!(v.iter().all(|g| !g.on_air), "neue Gäste warten");
    r.prefill();
    let (ret, prog) = r.run(100);
    let p = &prog[24_000..];
    // Im Warteraum: nicht in der Sendung, aber beide hören die Musik.
    assert!(db(tone(p, 440.0)) < -80.0 && db(tone(p, 660.0)) < -80.0);
    assert!(
        db(tone(&ret[0][24_000..], 880.0)) > -12.0,
        "A hört die Sendung"
    );
    assert!(
        db(tone(&ret[1][24_000..], 880.0)) > -12.0,
        "B hört die Sendung"
    );
    // Der Pegel vor dem Fader zeigt, dass der Gast spricht.
    assert!(r.hub.view()[0].input_peak > 0.2);

    r.hub.set_on_air(0, true);
    r.hub.set_on_air(1, true);
    let (ret, prog) = r.run(100);
    let p = &prog[24_000..];
    assert!(db(tone(p, 440.0)) > -12.0 && db(tone(p, 660.0)) > -12.0);
    // Mix-Minus mit zwei Gästen: A hört B, nicht sich selbst; B spiegelbildlich.
    let a = &ret[0][24_000..];
    let b = &ret[1][24_000..];
    assert!(db(tone(a, 660.0)) > -12.0 && db(tone(a, 440.0)) < -90.0);
    assert!(db(tone(b, 440.0)) > -12.0 && db(tone(b, 660.0)) < -90.0);
}

#[test]
fn muted_guest_still_hears_the_show_and_volume_resets_to_zero() {
    let mut r = rig_net(&[440.0, 660.0]);
    r.hub.set_on_air(0, true);
    r.hub.set_on_air(1, true);
    r.hub.set_mute(0, true);
    r.hub.set_gain_db(1, -20.0);
    r.prefill();
    let (ret, prog) = r.run(200);
    let p = &prog[24_000..];
    assert!(db(tone(p, 440.0)) < -80.0, "A stumm");
    let a = &ret[0][24_000..];
    assert!(db(tone(a, 880.0)) > -12.0, "stummer A hört die Musik");
    assert!((db(tone(a, 660.0)) + 30.5).abs() < 1.0, "A hört B leiser");
    // Doppelklick = 0 dB, Stumm aus: beide wieder voll.
    r.hub.set_gain_db(1, 0.0);
    r.hub.set_mute(0, false);
    let (_ret, prog) = r.run(100);
    let p = &prog[24_000..];
    assert!(db(tone(p, 440.0)) > -12.0 && db(tone(p, 660.0)) > -12.0);
    // Zurück in den Warteraum: wieder still.
    r.hub.set_on_air(0, false);
    let (_ret, prog) = r.run(100);
    assert!(db(tone(&prog[24_000..], 440.0)) < -80.0);
}

#[test]
fn hangup_removes_guest_from_show_and_returns() {
    let mut r = rig(&[440.0, 660.0]);
    r.prefill();
    r.hub.remove(1);
    assert_eq!(r.hub.count(), 1);
    let (ret, prog) = r.run(100);
    assert!(db(tone(&prog[24_000..], 660.0)) < -80.0);
    assert!(
        db(tone(&ret[0][24_000..], 660.0)) < -80.0,
        "A hört B nicht mehr"
    );
}

#[test]
fn four_guests_each_hear_all_others_but_not_themselves() {
    let freqs = [440.0, 550.0, 660.0, 770.0];
    let mut r = rig(&freqs);
    r.prefill();
    let (ret, _prog) = r.run(200);
    for (i, f) in freqs.iter().enumerate() {
        let x = &ret[i][24_000..];
        assert!(db(tone(x, *f)) < -90.0, "Gast {i} hört sich selbst");
        for (j, g) in freqs.iter().enumerate() {
            if j != i {
                assert!(db(tone(x, *g)) > -12.0, "Gast {i} hört {j} nicht");
            }
        }
    }
}
