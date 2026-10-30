//! Selbstprüfung für Paket 34: Sperrliste, Neu-Scan, Logs je Plugin und ein langer Offline-Render
//! ohne Speicherzuwachs. Alles rechnet in Puffer; es gibt weder ein Audiogerät noch Ton.

use std::time::{Duration, Instant};

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_plugins::testplugin::{self, CRASH};
use woa_plugins::{HostConfig, PluginLogs, RemoteInstance};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

const RATE: f32 = 48_000.0;
const N: usize = 512;

fn config(extra: &[&str], offline: bool, logs: Option<PluginLogs>) -> HostConfig {
    HostConfig {
        exe: env!("CARGO_BIN_EXE_woa-plugin-host").into(),
        prefix: Vec::new(),
        extra: extra.iter().map(|s| (*s).to_string()).collect(),
        offline,
        wait_ms: if offline { 0 } else { 100 },
        logs,
    }
}

fn remote(cfg: &HostConfig) -> RemoteInstance {
    let cat = testplugin::catalog();
    let info = cat.find(testplugin::ID).expect("test plugin").clone();
    RemoteInstance::new(cfg, &info, RATE).expect("remote load")
}

fn wait_until(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < limit {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[cfg(unix)]
mod scan {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use super::*;
    use woa_plugins::{Blocklist, Catalog};

    fn host(exe: &str, logs: Option<PluginLogs>) -> HostConfig {
        HostConfig {
            exe: exe.into(),
            prefix: Vec::new(),
            extra: Vec::new(),
            offline: false,
            wait_ms: 0,
            logs,
        }
    }

    fn fake_plugin(dir: &Path, name: &str) -> std::path::PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, b"kein Plugin").expect("write");
        p
    }

    #[test]
    fn a_file_that_dies_is_blocked_and_not_loaded_again() {
        let d = tempfile::tempdir().expect("tempdir");
        let plugins = d.path().join("plugins");
        std::fs::create_dir_all(&plugins).expect("mkdir");
        let bad = fake_plugin(&plugins, "kaputt.clap");
        let list_file = Blocklist::default_path(d.path());
        let logs = PluginLogs::new(d.path().join("logs"));

        // Erster Lauf: der Suchprozess endet mit Fehler (wie ein Absturz) -> Problem und Sperre.
        let mut bl = Blocklist::load(&list_file);
        let cat = Catalog::scan_guarded(
            std::slice::from_ref(&plugins),
            &host("/bin/false", Some(logs.clone())),
            &mut bl,
        );
        assert!(cat.plugins().is_empty());
        assert_eq!(cat.problems().len(), 1);
        assert!(cat.problems()[0].reason.ends_with("gesperrt"));
        assert!(bl.is_blocked(&bad));
        assert!(
            Blocklist::load(&list_file).is_blocked(&bad),
            "the block survives a restart"
        );

        // Zweiter Lauf mit einem Programm, das es nicht gibt: Die Datei wird gar nicht erst
        // angefasst (sonst gäbe es ein Problem „Suchprozess startet nicht“).
        let mut bl = Blocklist::load(&list_file);
        let cat = Catalog::scan_guarded(
            std::slice::from_ref(&plugins),
            &host("/gibt/es/nicht", Some(logs.clone())),
            &mut bl,
        );
        assert!(cat.problems().is_empty(), "{:?}", cat.problems());

        // Freigeben: der nächste Lauf versucht es wieder.
        assert!(bl.unblock(&bad));
        let cat = Catalog::scan_guarded(&[plugins], &host("/gibt/es/nicht", None), &mut bl);
        assert_eq!(cat.problems().len(), 1);
        assert!(
            !bl.is_blocked(&bad),
            "a start failure of the scanner itself never blocks a file"
        );

        let log = std::fs::read_to_string(logs.scan_path()).expect("scan log");
        assert!(log.contains("kaputt.clap"), "{log}");
        assert!(log.contains("gesperrt, nicht geladen"), "{log}");
    }

    #[test]
    fn a_file_that_hangs_is_killed_and_blocked() {
        let d = tempfile::tempdir().expect("tempdir");
        let plugins = d.path().join("plugins");
        std::fs::create_dir_all(&plugins).expect("mkdir");
        let bad = fake_plugin(&plugins, "haengt.clap");
        let script = d.path().join("suche.sh");
        std::fs::write(&script, "#!/bin/sh\nexec sleep 30\n").expect("script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");

        let mut bl = Blocklist::in_memory();
        let t0 = Instant::now();
        let cat = Catalog::scan_guarded_with(
            &[plugins],
            &host(script.to_str().expect("utf8"), None),
            &mut bl,
            Duration::from_millis(300),
        );
        assert!(t0.elapsed() < Duration::from_secs(10), "no long wait");
        assert_eq!(cat.problems().len(), 1);
        assert!(cat.problems()[0].reason.contains("hängt"));
        assert!(bl.is_blocked(&bad));
        assert!(bl.reason(&bad).is_some_and(|r| r.contains("hängt")));
    }
}

#[test]
fn each_plugin_has_its_own_log_with_start_failure_and_reload() {
    let _g = testplugin::serial();
    let d = tempfile::tempdir().expect("tempdir");
    let logs = PluginLogs::new(d.path().join("logs"));
    let mut inst = remote(&config(&["--test-faults"], false, Some(logs.clone())));
    let mut p = inst.processor();
    let silence = vec![0.0f32; N];
    let (mut ol, mut or) = (vec![0.0f32; N], vec![0.0f32; N]);
    p.set_param(CRASH, 1.0);
    assert_no_alloc(|| p.process(&[], &silence, &silence, &mut ol, &mut or));
    assert!(
        wait_until(Duration::from_secs(5), || inst.supervise() || inst.failed()),
        "the crash is noticed"
    );
    inst.reload(None).expect("reload");
    drop(p);
    drop(inst);

    let text = std::fs::read_to_string(logs.path_for(testplugin::ID)).expect("plugin log");
    for needle in ["Start: Prozess", "Ausfall:", "Neu laden", "Ende"] {
        assert!(text.contains(needle), "missing {needle:?} in\n{text}");
    }
    // Ein Ausfall steht mit Zeitstempel und in der Reihenfolge des Geschehens.
    let first = text.find("Start:").expect("start");
    let fail = text.find("Ausfall:").expect("failure");
    let reload = text.find("Neu laden").expect("reload");
    assert!(first < fail && fail < reload, "{text}");
    assert!(text.lines().all(|l| l.contains('T') && l.contains('Z')));
}

/// Residenter Speicher eines Prozesses in KiB (Linux).
#[cfg(target_os = "linux")]
fn rss_kib(pid: &str) -> Option<u64> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = s.lines().find(|l| l.starts_with("VmRSS:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

/// Ein langer Offline-Render mit zwei Plugins im eigenen Prozess: Nach einer Einlaufzeit darf
/// weder die App (dieser Test) noch ein Plugin-Prozess nennenswert wachsen, und kein Block
/// kommt zu spät. Die Länge steht in `WOA_LONGRUN_MINUTES` (Vorgabe 10 Minuten Musik).
#[cfg(target_os = "linux")]
#[test]
fn ten_minute_render_with_two_plugins_does_not_grow() {
    use woa_dsp::instrument::NoteEvent;
    let _g = testplugin::serial();
    let minutes: f64 = std::env::var("WOA_LONGRUN_MINUTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10.0);
    let blocks = (minutes * 60.0 * f64::from(RATE) / N as f64) as usize;
    let warm = blocks / 10;

    let cfg = config(&[], true, None);
    let synth = remote(&cfg);
    let effect = remote(&cfg);
    let (mut ps, mut pe) = (synth.processor(), effect.processor());
    let pids = [synth.pid().expect("pid"), effect.pid().expect("pid")];

    let silence = vec![0.0f32; N];
    let (mut sl, mut sr) = (vec![0.0f32; N], vec![0.0f32; N]);
    let (mut el, mut er) = (vec![0.0f32; N], vec![0.0f32; N]);
    let mut energy = 0.0f64;
    let mut at_warm: Option<(u64, [u64; 2])> = None;

    for b in 0..blocks {
        // Alle 40 Blöcke (etwa 0,43 s) eine Note, damit das Plugin wirklich rechnet.
        let on = [NoteEvent::note_on(48 + (b / 40 % 24) as u8, 100)];
        let off = [NoteEvent::note_off(
            48 + ((b / 40).wrapping_sub(1) % 24) as u8,
        )];
        let events: &[NoteEvent] = match b % 40 {
            0 => &on,
            20 => &off,
            _ => &[],
        };
        assert_no_alloc(|| {
            ps.process(events, &silence, &silence, &mut sl, &mut sr);
            pe.process(&[], &sl, &sr, &mut el, &mut er);
        });
        if b % 97 == 0 {
            energy += el.iter().map(|v| f64::from(*v * *v)).sum::<f64>();
            assert!(el.iter().chain(&er).all(|v| v.is_finite()));
        }
        if b == warm {
            let app = rss_kib("self").expect("rss");
            let kids = [
                rss_kib(&pids[0].to_string()).expect("rss child"),
                rss_kib(&pids[1].to_string()).expect("rss child"),
            ];
            at_warm = Some((app, kids));
        }
    }
    assert!(energy > 0.0, "the plugins really produced sound");
    assert_eq!(synth.late_blocks(), 0);
    assert_eq!(effect.late_blocks(), 0);

    let (app0, kids0) = at_warm.expect("warm-up sample");
    let app1 = rss_kib("self").expect("rss");
    let kids1 = [
        rss_kib(&pids[0].to_string()).expect("rss child"),
        rss_kib(&pids[1].to_string()).expect("rss child"),
    ];
    // Spielraum 2 MiB je Prozess: der Zuteiler gibt Seiten nicht immer sofort zurück.
    let limit = 2048;
    eprintln!("rss app {app0} -> {app1} KiB, plugins {kids0:?} -> {kids1:?} KiB ({blocks} blocks)");
    assert!(app1 <= app0 + limit, "app grew from {app0} to {app1} KiB");
    for i in 0..2 {
        assert!(
            kids1[i] <= kids0[i] + limit,
            "plugin {i} grew from {} to {} KiB",
            kids0[i],
            kids1[i]
        );
    }
}
