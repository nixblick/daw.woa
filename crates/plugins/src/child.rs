//! Der Plugin-Prozess (Paket 33): lädt genau ein Plugin und rechnet Blöcke, die die Oberfläche
//! über den gemeinsamen Speicher schickt ([`crate::remote`], [`crate::shm`]).
//!
//! Aufbau: Der Hauptthread bedient die Steuerseite (Briefkasten, Reglerwerte, Zeitgeber und
//! Fenster des Plugins, Rückmeldung, Spiegel der Reglerwerte, Lebenszeichen der Oberfläche); ein
//! eigener Audio-Thread rechnet die Blöcke. Stürzt das Plugin ab, endet nur dieser Prozess.
//!
//! Aufruf: `woa-plugin-host` oder die App selbst mit [`crate::remote::HOST_FLAG`]
//! ([`dispatch`]). Argumente: `--shm DATEI --file CLAP --id KENNUNG --rate HZ` zum Spielen,
//! `--scan CLAP --out DATEI` zum Suchen (Ergebnis als Textzeilen).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use woa_dsp::instrument::NoteEvent;
use woa_dsp::plugin::StereoProcessor;

use crate::feedback::ParamEdit;
use crate::host::{ClapInstance, MAX_FRAMES, ParamDesc};
use crate::remote::{HOST_FLAG, decode_event, info_reply, tag, write_reply};
use crate::scan::Catalog;
use crate::shm::{
    EV_CAP, MAILBOX_CAP, MIRROR_CAP, Mapping, RawEdit, SET_CAP, STATE_ERROR, STATE_READY,
};

/// So lange darf die Oberfläche still sein, bevor sich der Plugin-Prozess beendet.
const PARENT_SILENCE: Duration = Duration::from_secs(10);

/// Ruft das Programm den Plugin-Prozess (`--woa-plugin-host` als erstes Argument), läuft er hier
/// und liefert den Rückgabewert; sonst `None`. Eine App ruft es als Erstes in `main`.
pub fn dispatch() -> Option<i32> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some(HOST_FLAG) {
        return None;
    }
    Some(run(&args[2..]))
}

/// Einstieg des eigenständigen Programms `woa-plugin-host`.
pub fn main_from_args() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    run(&args)
}

struct Args {
    shm: Option<PathBuf>,
    file: Option<PathBuf>,
    id: Option<String>,
    rate: u32,
    scan: Option<PathBuf>,
    out: Option<PathBuf>,
    #[cfg_attr(not(feature = "test-plugin"), allow(dead_code))]
    test_plugin: bool,
    #[cfg_attr(not(feature = "test-plugin"), allow(dead_code))]
    faults: bool,
    #[cfg_attr(not(feature = "test-plugin"), allow(dead_code))]
    latency: u32,
}

fn parse(args: &[String]) -> Args {
    let mut a = Args {
        shm: None,
        file: None,
        id: None,
        rate: 48_000,
        scan: None,
        out: None,
        test_plugin: false,
        faults: false,
        latency: 0,
    };
    let mut it = args.iter();
    while let Some(k) = it.next() {
        match k.as_str() {
            "--shm" => a.shm = it.next().map(PathBuf::from),
            "--file" => a.file = it.next().map(PathBuf::from),
            "--id" => a.id = it.next().cloned(),
            "--rate" => a.rate = it.next().and_then(|v| v.parse().ok()).unwrap_or(48_000),
            "--scan" => a.scan = it.next().map(PathBuf::from),
            "--out" => a.out = it.next().map(PathBuf::from),
            "--test-plugin" => a.test_plugin = true,
            "--test-faults" => a.faults = true,
            "--test-latency" => {
                a.latency = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
            }
            _ => {}
        }
    }
    a
}

fn run(args: &[String]) -> i32 {
    let a = parse(args);
    if let (Some(file), Some(out)) = (&a.scan, &a.out) {
        return scan(file, out);
    }
    let (Some(shm), Some(id)) = (&a.shm, &a.id) else {
        eprintln!("woa-plugin-host: --shm und --id fehlen (das Programm wird von WOA gestartet)");
        return 2;
    };
    match Mapping::open(shm) {
        Ok(map) => serve(&map, &a, id),
        Err(e) => {
            eprintln!("woa-plugin-host: gemeinsamer Speicher: {e}");
            3
        }
    }
}

/// Sucht in einer Datei und schreibt je Plugin eine Zeile
/// `I<TAB>Kennung<TAB>Name<TAB>Hersteller<TAB>Merkmale`.
fn scan(file: &std::path::Path, out: &std::path::Path) -> i32 {
    let cat = Catalog::scan_files(&[file.to_path_buf()]);
    let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
    let mut text = String::new();
    for p in cat.plugins() {
        text.push_str(&format!(
            "I\t{}\t{}\t{}\t{}\n",
            clean(&p.clap_id),
            clean(&p.name),
            clean(&p.vendor),
            clean(&p.features.join(","))
        ));
    }
    match std::fs::write(out, text) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("woa-plugin-host: {e}");
            4
        }
    }
}

fn fail(map: &Mapping, msg: &str) -> i32 {
    let sh = map.shared();
    let n = msg.len().min(MAILBOX_CAP);
    // SAFETY: Vor `state` gehört der Briefkasten diesem Prozess.
    unsafe { std::ptr::copy_nonoverlapping(msg.as_ptr(), map.mailbox_ptr(), n) };
    sh.mb_len.store(n as u32, Ordering::Relaxed);
    sh.state.store(STATE_ERROR, Ordering::Release);
    1
}

#[cfg_attr(not(feature = "test-plugin"), allow(unused_variables))]
fn load_catalog(a: &Args) -> Catalog {
    #[cfg(feature = "test-plugin")]
    if a.test_plugin {
        crate::testplugin::allow_faults(a.faults);
        crate::testplugin::set_latency(a.latency);
        return crate::testplugin::catalog();
    }
    match &a.file {
        Some(f) => Catalog::scan_files(std::slice::from_ref(f)),
        None => Catalog::empty(),
    }
}

/// Zeiger auf das Mapping für den Audio-Thread.
struct SendMap(*const Mapping);
// SAFETY: Das Mapping wird nur über Atomics und das Block-Protokoll benutzt; der Zeiger gilt, bis
// der Thread beendet ist (`serve` wartet darauf).
unsafe impl Send for SendMap {}

fn serve(map: &Mapping, a: &Args, id: &str) -> i32 {
    let sh = map.shared();
    sh.child_pid.store(std::process::id(), Ordering::Relaxed);
    let cat = load_catalog(a);
    let mut inst = match ClapInstance::new(&cat, id, a.rate as f32) {
        Ok(i) => i,
        Err(e) => return fail(map, &e.to_string()),
    };
    let params: Vec<ParamDesc> = inst.params().to_vec();
    let has_gui = inst.has_gui();
    sh.has_gui.store(u32::from(has_gui), Ordering::Relaxed);
    sh.latency.store(inst.latency(), Ordering::Relaxed);
    refresh_mirror(map, &mut inst);

    let stop = Arc::new(AtomicBool::new(false));
    let audio = {
        let proc = inst.processor();
        let stop = Arc::clone(&stop);
        let handle = SendMap(std::ptr::from_ref(map));
        std::thread::Builder::new()
            .name("woa-plugin-audio".into())
            .spawn(move || {
                let handle = handle;
                // SAFETY: siehe `SendMap`.
                let map = unsafe { &*handle.0 };
                audio_loop(map, proc, &stop);
            })
    };
    let audio = match audio {
        Ok(h) => h,
        Err(e) => return fail(map, &format!("Audio-Thread: {e}")),
    };
    sh.state.store(STATE_READY, Ordering::Release);

    let mut last_beat = sh.beat.load(Ordering::Acquire);
    let mut beat_at = Instant::now();
    let mut mirror_at = Instant::now();
    let mut edits: Vec<ParamEdit> = Vec::new();
    let code = loop {
        if sh.shutdown.load(Ordering::Acquire) != 0 {
            break 0;
        }
        let beat = sh.beat.load(Ordering::Acquire);
        if beat != last_beat {
            last_beat = beat;
            beat_at = Instant::now();
        } else if beat_at.elapsed() > PARENT_SILENCE {
            eprintln!("woa-plugin-host: Oberfläche meldet sich nicht mehr, Ende");
            break 5;
        }

        // Briefkasten.
        let req = sh.mb_req.load(Ordering::Acquire);
        if req != sh.mb_rsp.load(Ordering::Relaxed) {
            handle_mail(map, &mut inst, &params, has_gui, req);
        }
        // Reglerwerte von der Oberfläche.
        let mut n = 0;
        while n < SET_CAP
            && let Some(s) = sh.set_ring.pop()
        {
            inst.push_param(s.id, s.value);
            n += 1;
        }
        inst.pump();
        edits.clear();
        inst.poll_feedback(&mut edits);
        for e in &edits {
            let raw = match *e {
                ParamEdit::Param { id, old, new } => RawEdit {
                    kind: 0,
                    id,
                    old,
                    new,
                },
                ParamEdit::WholeState => RawEdit {
                    kind: 1,
                    id: 0,
                    old: 0.0,
                    new: 0.0,
                },
            };
            let _ = sh.edit_ring.push(raw);
        }
        sh.gui_open
            .store(u32::from(inst.gui_open()), Ordering::Relaxed);
        if inst.failed() {
            sh.failed.store(1, Ordering::Release);
        }
        if mirror_at.elapsed() > Duration::from_millis(100) {
            refresh_mirror(map, &mut inst);
            mirror_at = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    stop.store(true, Ordering::Relaxed);
    let _ = audio.join();
    drop(inst);
    code
}

fn refresh_mirror(map: &Mapping, inst: &mut ClapInstance) {
    let sh = map.shared();
    let ids: Vec<u32> = inst.params().iter().map(|p| p.id).collect();
    for (i, id) in ids.into_iter().enumerate().take(MIRROR_CAP) {
        let v = inst.param_value(id).unwrap_or(f64::NAN);
        sh.mirror[i].store(v.to_bits(), Ordering::Relaxed);
    }
}

fn handle_mail(
    map: &Mapping,
    inst: &mut ClapInstance,
    params: &[ParamDesc],
    has_gui: bool,
    req: u64,
) {
    let sh = map.shared();
    let tag_v = sh.mb_tag.load(Ordering::Relaxed);
    let n = (sh.mb_len.load(Ordering::Relaxed) as usize).min(MAILBOX_CAP);
    // SAFETY: Die Oberfläche hat die Anfrage vor `mb_req` geschrieben und fasst den Briefkasten
    // bis zur Antwort nicht an.
    let payload = unsafe { std::slice::from_raw_parts(map.mailbox_ptr(), n) }.to_vec();
    let (ok, body): (bool, Vec<u8>) = match tag_v {
        tag::INFO => (true, info_reply(has_gui, inst.latency(), params)),
        tag::SAVE => match inst.save_state() {
            Ok(b) => (true, b),
            Err(e) => (false, e.to_string().into_bytes()),
        },
        tag::LOAD => {
            let r = inst.load_state(&payload);
            refresh_mirror(map, inst);
            match r {
                Ok(()) => (true, Vec::new()),
                Err(e) => (false, e.to_string().into_bytes()),
            }
        }
        tag::OPEN_GUI => {
            let title = String::from_utf8_lossy(&payload).into_owned();
            match inst.open_gui(&title) {
                Ok(()) => (true, Vec::new()),
                Err(e) => (false, e.to_string().into_bytes()),
            }
        }
        tag::CLOSE_GUI => {
            inst.close_gui();
            (true, Vec::new())
        }
        _ => (false, b"unbekannte Anfrage".to_vec()),
    };
    write_reply(map, req, ok, &body);
}

/// Der Audio-Thread des Plugin-Prozesses: wartet auf den nächsten Block der Oberfläche, rechnet
/// ihn und meldet ihn fertig.
fn audio_loop(map: &Mapping, mut proc: Box<dyn StereoProcessor>, stop: &AtomicBool) {
    let sh = map.shared();
    let cap = MAX_FRAMES as usize;
    let mut events: Vec<NoteEvent> = Vec::with_capacity(EV_CAP);
    let mut in_l = vec![0.0f32; cap];
    let mut in_r = vec![0.0f32; cap];
    let mut out_l = vec![0.0f32; cap];
    let mut out_r = vec![0.0f32; cap];
    let mut idle = Instant::now();
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let done = sh.seq_done.load(Ordering::Relaxed);
        if sh.seq_sub.load(Ordering::Acquire) == done {
            let quiet = idle.elapsed();
            if quiet < Duration::from_millis(1) {
                std::hint::spin_loop();
            } else if quiet < Duration::from_millis(50) {
                std::thread::sleep(Duration::from_micros(200));
            } else {
                std::thread::sleep(Duration::from_millis(2));
            }
            continue;
        }
        let n = (sh.frames.load(Ordering::Relaxed) as usize).min(cap);
        let count = (sh.n_events.load(Ordering::Relaxed) as usize).min(EV_CAP);
        let sets = (sh.n_sets.load(Ordering::Relaxed) as usize).min(SET_CAP);
        // SAFETY: `seq_sub != seq_done` (Acquire): der Block gehört diesem Prozess; die Oberfläche
        // schreibt erst wieder nach `seq_done`. Die Längen sind oben begrenzt.
        unsafe {
            std::ptr::copy_nonoverlapping(sh.in_l_ptr(), in_l.as_mut_ptr(), n);
            std::ptr::copy_nonoverlapping(sh.in_r_ptr(), in_r.as_mut_ptr(), n);
            events.clear();
            for i in 0..count {
                if let Some(e) = decode_event(&sh.events_ptr().add(i).read()) {
                    events.push(e);
                }
            }
            for i in 0..sets {
                let s = sh.sets_ptr().add(i).read();
                proc.set_param(s.id, s.value);
            }
        }
        if sh.flush.load(Ordering::Relaxed) != 0 {
            proc.reset();
        }
        proc.process(
            &events,
            &in_l[..n],
            &in_r[..n],
            &mut out_l[..n],
            &mut out_r[..n],
        );
        // SAFETY: wie oben; die Ausgangspuffer sind `MAX_FRAMES` lang.
        unsafe {
            std::ptr::copy_nonoverlapping(out_l.as_ptr(), sh.out_l_ptr(), n);
            std::ptr::copy_nonoverlapping(out_r.as_ptr(), sh.out_r_ptr(), n);
        }
        sh.seq_done.store(done + 1, Ordering::Release);
        idle = Instant::now();
    }
}
