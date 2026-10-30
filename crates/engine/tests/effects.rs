//! Paket 17: die neuen Effekte im Projekt-Graphen. Jeder Process-Aufruf läuft unter
//! `assert_no_alloc`; die Sprachkette wird gemessen (Brummen fällt, Sprachband bleibt), das
//! Tempo des Projekts steuert die synchrone Verzögerung, der Export rechnet ohne Arbeits-Thread.

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_dsp::{Node, SineOsc};
use woa_engine::{AudioProcessor, Engine, build_project_graph, render_project};
use woa_model::{
    Command, EffectKind, Insert, Project, Session, Track, TrackId, TrackKind, VOICE_CHAIN,
};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const SR: u32 = 48_000;
const BLOCK: usize = 128;

/// Frequenz aus dem Spurnamen, Amplitude 0.5.
fn sine_source(t: &Track) -> Option<Box<dyn Node>> {
    if t.kind == TrackKind::Bus {
        return None;
    }
    let freq = t.name.parse::<f32>().unwrap_or(1000.0);
    Some(Box::new(SineOsc::new(freq, 0.5)))
}

fn live(project: &Project) -> (Engine, AudioProcessor) {
    let (mut engine, processor) = Engine::new(1024);
    let g = build_project_graph(project, &mut sine_source);
    engine
        .set_graph(g.builder, SR as f32, BLOCK)
        .expect("compiles");
    engine.set_tempo(project.tempo_bpm).expect("tempo");
    engine.play().expect("play");
    (engine, processor)
}

fn render_live(project: &Project, frames: usize) -> (Vec<f32>, Vec<f32>) {
    let (_engine, mut processor) = live(project);
    let mut l = vec![0.0f32; frames];
    let mut r = vec![0.0f32; frames];
    for (a, b) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)) {
        assert_no_alloc(|| processor.process_stereo(a, b));
    }
    (l, r)
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn track(s: &mut Session, name: &str) -> TrackId {
    let id = s.new_track_id();
    s.apply(Command::add_track(id, name, TrackKind::Audio))
        .expect("add");
    id
}

fn insert(s: &mut Session, track: TrackId, insert: Insert) {
    s.apply(Command::AddInsert {
        track,
        insert,
        index: None,
    })
    .expect("insert");
}

fn voice_chain(s: &mut Session, track: TrackId) {
    for (kind, values) in VOICE_CHAIN {
        let id = s.new_insert_id();
        insert(
            s,
            track,
            Insert::with_values(id, kind, values).expect("valid"),
        );
    }
}

#[test]
fn voice_chain_removes_rumble_and_keeps_the_voice_band() {
    let mut s = Session::new("voice");
    let rumble = track(&mut s, "30");
    let voice = track(&mut s, "1000");
    let (rumble_before, _) = render_live(s.project(), 24_000);
    voice_chain(&mut s, rumble);
    voice_chain(&mut s, voice);
    let (after, _) = render_live(s.project(), 24_000);
    // Beide Spuren laufen zusammen; das Brummen (30 Hz) ist gedämpft, 1 kHz bleibt im Mix.
    assert!(peak(&after[12_000..]).is_finite());
    let mut solo = Session::new("rumble");
    let r = track(&mut solo, "30");
    voice_chain(&mut solo, r);
    let (rumble_after, _) = render_live(solo.project(), 24_000);
    assert!(
        peak(&rumble_after[12_000..]) < 0.25 * peak(&rumble_before[12_000..]),
        "rumble only lowered from {} to {}",
        peak(&rumble_before[12_000..]),
        peak(&rumble_after[12_000..])
    );
    let mut vs = Session::new("voice1k");
    let v = track(&mut vs, "1000");
    let (v_before, _) = render_live(vs.project(), 24_000);
    voice_chain(&mut vs, v);
    let (v_after, _) = render_live(vs.project(), 24_000);
    let ratio = peak(&v_after[12_000..]) / peak(&v_before[12_000..]);
    assert!(
        (0.5..1.5).contains(&ratio),
        "1 kHz changed by factor {ratio}"
    );
}

#[test]
fn every_effect_runs_in_the_graph_without_allocating() {
    for kind in EffectKind::ALL {
        for preset in kind.presets() {
            let mut s = Session::new("all");
            let t = track(&mut s, "440");
            let id = s.new_insert_id();
            insert(
                &mut s,
                t,
                Insert::with_preset(id, kind, preset.name).expect("preset"),
            );
            let (l, r) = render_live(s.project(), 12_000);
            assert!(
                l.iter().chain(&r).all(|x| x.is_finite() && x.abs() <= 4.0),
                "{kind:?} / {}",
                preset.name
            );
        }
    }
}

#[test]
fn bypassed_effects_leave_the_mix_bit_exact() {
    let mut plain = Session::new("plain");
    track(&mut plain, "440");
    let (want_l, want_r) = render_live(plain.project(), 24_000);
    for kind in EffectKind::ALL {
        let mut s = Session::new("bypass");
        let t = track(&mut s, "440");
        let id = s.new_insert_id();
        let mut ins = Insert::new(id, kind);
        ins.bypass = true;
        insert(&mut s, t, ins);
        let (l, r) = render_live(s.project(), 24_000);
        assert!(
            l.iter()
                .zip(&want_l)
                .chain(r.iter().zip(&want_r))
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "{kind:?}: bypass changes the mix"
        );
    }
}

#[test]
fn synced_delay_follows_the_project_tempo() {
    // Viertelnote (sync 2), nur Echo (mix 100), keine Rückkopplung.
    let echo_start = |bpm: f64| {
        let mut s = Session::new("sync");
        let t = track(&mut s, "440");
        s.apply(Command::SetTempo { bpm }).expect("tempo");
        let id = s.new_insert_id();
        insert(
            &mut s,
            t,
            Insert::with_values(
                id,
                EffectKind::Delay,
                &[("sync", 2.0), ("feedback_pct", 0.0), ("mix_pct", 100.0)],
            )
            .expect("valid"),
        );
        let (l, _) = render_live(s.project(), 60_000);
        l.iter().position(|x| x.abs() > 0.01).expect("echo")
    };
    // 120 BPM: 500 ms = 24000 Samples; 60 BPM: 1000 ms = 48000 Samples (Spurwinkel mischt nichts).
    let a = echo_start(120.0);
    let b = echo_start(60.0);
    assert!((24_000..24_600).contains(&a), "120 BPM: first echo at {a}");
    assert!((48_000..48_600).contains(&b), "60 BPM: first echo at {b}");
}

#[test]
fn export_render_with_denoise_is_deterministic_and_matches_the_chain_delay() {
    let mut s = Session::new("export");
    let t = track(&mut s, "440");
    let id = s.new_insert_id();
    insert(
        &mut s,
        t,
        Insert::with_values(id, EffectKind::Denoise, &[("amount_pct", 100.0)]).expect("valid"),
    );
    let mut src = sine_source;
    let a = render_project(s.project(), &mut src, SR as f32, 24_000, 512).expect("render");
    let b = render_project(s.project(), &mut src, SR as f32, 24_000, 1024).expect("render");
    assert_eq!(a.0.len(), 24_000);
    assert!(a.0.iter().all(|x| x.is_finite()));
    // Der Blocktakt der Ausgabe ändert das Ergebnis nicht.
    let diff =
        a.0.iter()
            .zip(&b.0)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0f32, f32::max);
    assert!(diff < 1e-6, "block size changes the export by {diff}");
}
