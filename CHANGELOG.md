# Changelog

## Unreleased

- MCP-Gerüst (Vorarbeit M2, #3): `woa-agent` mit rmcp 3.8 über stdio, Unterbefehl `woa-headless mcp <pfad>`. Tools `project_info`, `add_track`, `rename_track`, `set_gain`, `set_pan`, `set_mute`, `set_tempo`, `add_clip`, `undo`, `redo`, `save`, alle über `Session::apply`, je Aufruf ein Undo-Schritt „Agent“; Schemas aus serde/schemars. Tests mit rmcp-Client und rohem JSON-RPC über Pipes. Anleitung `docs/agent.md`.

- Härtung `woa-model` (Sicherheits-Review): IDs über 2^53 werden beim Laden und in Befehlen abgelehnt (`IdOutOfRange`), damit keine präparierte Datei die ID-Vergabe in eine Endlosschleife treibt. Atomares Schreiben über `tempfile` (exklusiv angelegt, zufälliger Name), folgt keinem untergeschobenen Symlink mehr.

- Review M0 (#1): `.gitignore` hält jetzt auch `.claude/worktrees/`, `.claude/settings.local.json` und `*.tmp` (atomares Schreiben) aus dem Repo.
- `woa-headless` (M0, Abnahmewerkzeug für #1): `new`, `info [--json]`, `add-track [--kind]`, `undo-demo`, `roundtrip` (Exit 1 bei Abweichung), `log-test` (`--url`/`WOA_TELEMETRY_URL`), `feedback-test` (`--url`/`WOA_FEEDBACK_URL`). Integrationstests mit assert_cmd, tempfile und lokalem Mock-Server. Pfad-Abhängigkeiten tragen eine Version, sonst wertet cargo-deny sie als Wildcard.

- egui-Prototyp (M0, Branch 5, `m0/gui`): `woa-gui` mit fensterunabhängigem `AppState` (Spur hinzufügen/umbenennen, Gain, Mute, Undo/Redo, Neu/Öffnen/Speichern, Autosave alle 10 s, ungespeichert in `recovery.woa`) und Feedback-Dialog (Text + Screenshot über `woa-telemetry`, leere `feedback_url` = „Server noch nicht eingerichtet“). `apps/woa` startet das Fenster (eframe 0.35, rfd). Bewertung in `docs/entscheidungen.md`. `deny.toml`: Lizenz-Ausnahme für die egui-Standardschriften (OFL-1.1, Ubuntu Font Licence), vier „unmaintained“-Hinweise transitiver Crates (ttf-parser, zbus-Teilcrates) bewusst ignoriert.
- Telemetrie (M0, `woa-telemetry`): `tracing` mit JSON-Logdatei je Tag (30 Tage), Panic-Hook, Opt-in-Upload-Senke (gebündelt, nur Code/Stufe/Zahl, Scrubber, eigener Thread, `ureq`) im Format der bretteins-`/ingest`-Route, Feedback-Client `submit_feedback` mit Screenshot. Tests gegen lokalen Mock-Server, Vertrag in `docs/telemetrie.md`. `deny.toml`: Ausnahme CDLA-Permissive-2.0 nur für `webpki-roots`.
- Projektmodell `woa-model` (M0, Branch 2): `Project`/`Track`/`Clip` mit `u64`-IDs, `Command`-Enum (Spuren, Gain, Pan, Mute, Solo, Tempo, Clips) mit exakten Inversen, `Session::apply` mit Undo/Redo und Gruppen, Ordnerformat `<name>.woa/project.json` mit `format_version`, atomares Schreiben, Autosave-Hook `Session::autosave` ohne Zeitgeber. Tests: Roundtrip, insta-Snapshot, proptest apply+undo.
- Feedback-Server vorbereitet (M0, Branch 4): `server/README.md` mit Anleitung für eine zweite Instanz der bretteins-Rolle `feedback` (feedback.wironair.de, Port 8081, `GITHUB_REPO=nixblick/daw.woa`), Prüfung der Mehrinstanzfähigkeit samt Patch-Vorschlag, Vorlagen unter `server/feedback/`, Runbook `docs/andre-handgriffe.md`. Nichts ausgerollt.
- Engine-Spike (Vorarbeit M1, #2): `woa-dsp` mit `Node`-Trait (allokationsfreies `process`), linearer Parameter-Glättung, Sinus-Oszillator und Gain. `woa-engine` mit Graph-Kompilierung im Control-Thread (Topologie, Puffer vorab, Zyklusprüfung), Kommandos über `rtrb`, Graph-Tausch im Audio-Thread und Freigabe über `basedrop`, Graph-Beschreibung per `arc-swap`, Offline-Render und WAV (hound). cpal-Ausgabe nur hinter Feature `device`. Tests: `assert_no_alloc` um jeden Process-Aufruf, Golden-Render `tests/golden/sine_440hz_gain05_1s.wav`, Glättung ohne Sprung.

## 0.1.0 — 2026-10-28

- CI-Gate (M0, #1): fmt, clippy -D warnings, test auf Linux und Windows, cargo-deny mit GPL-kompatibler Lizenzliste. `.gitignore` hält lokale Projekte und Logs fern.

## 0.0.1 — 2026-10-28

- Repo angelegt: Konzept `docs/konzept.md` (aus Drive), Cargo-Workspace mit 14 Crates und 3 Apps als leere Gerüste, `CLAUDE.md` mit den Echtzeit- und Stabilitätsregeln, LICENSE GPLv3, Issues M0–M11.
