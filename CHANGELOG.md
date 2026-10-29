# Changelog

## Unreleased

## 0.1.11 — 2026-10-29

- **Paket 5 Quellen.** Jede Spur hat eine Quellenwahl: Standard-Mikrofon, alle Eingänge, „Systemklang“ (alles, was abgespielt wird) oder ein einzelnes Programm. Unter Linux wird direkt mit PipeWire/Pulse gesprochen (`engine/src/capture.rs`, Crate `pulseaudio`, reines Rust, MIT). Mehrere Spuren lassen sich scharf schalten (roter Punkt) und nehmen gleichzeitig ab demselben Punkt auf. Mit „hören“ ist die Quelle beim Aufnehmen hörbar.
- **Stopp am Ende** neben ▶ (Vorgabe an): Die Wiedergabe hält am Ende des letzten Clips an. Erneutes ▶ startet von vorn. Beim Aufnehmen läuft sie weiter.
- `woa-headless sources` listet die Quellen, `woa-headless capture <quelle> <datei>` nimmt eine davon auf.

## 0.1.10 — 2026-10-29

- **Design-Runde 1.** Dunkles Studio-Thema mit einer Akzentfarbe (Orange) und Rot für Aufnahme, sechs Spurfarben, abgerundete Knöpfe, größere Abstände (`gui/src/theme.rs`). Transportleiste mit großen Knöpfen und Zeitanzeige (Minuten:Sekunden, Takt, BPM), Aufnahme-Knopf in Rot, Zoom und Export-Lautheit rechts. Spurköpfe mit Farbstreifen, Aufnahmeziel-Punkt, Name, M, Pegel und Gain (Doppelklick: 0 dB) in fester Breite; Lineal mit Taktnummern und Kopf-Marke (Klick oder Ziehen setzt den Kopf). Zeitleiste mit Taktraster, Clips in Spurfarbe mit Kopfzeile und Wellenform. Statusleiste mit Version.
- `woa <projektordner>` öffnet ein Projekt beim Start. Fotomodus `WOA_SCREENSHOT=datei.png`: speichert ein Bild des Fensters und beendet (für Design-Prüfungen ohne Bildschirm-Werkzeug, auch unter Wayland).

## 0.1.9 — 2026-10-29

- **Paket 4 „Radio-tauglich“.** Export-Lautheit wählbar (−16 LUFS Web/Podcast, Vorgabe; −23 LUFS EBU R128; unverändert), Spitzen bleiben unter −1 dBFS; Messung nach ITU-R BS.1770 (`woa_engine::loudness`), mit ffmpeg `ebur128` gegengeprüft. `woa-headless render --lufs -16`.
- Latenzausgleich bei Aufnahmen: Eingangs- und Ausgangslatenz des Treibers werden gemessen und die Aufnahme um diese Zeit nach vorn gelegt (Audio-Versatz), dazu ein Wert von Hand in ms. `woa-headless record` meldet die Eingangslatenz.
- Mehrere Clips markieren (Strg/Umschalt+Klick), zusammen verschieben und löschen, je ein Undo-Schritt.

## 0.1.8 — 2026-10-29

- **Paket 3 „Arrangieren“.** Clips auf eine andere Spur ziehen, Clip-Ränder ziehen zum Kürzen (links verschiebt den Audio-Versatz mit, nie über Anfang oder Ende des Audios, Raster 1/4 Beat, ein Undo-Schritt), Zoom der Zeitleiste (Knöpfe − und +, Strg+Mausrad).
- Export als WAV, FLAC (24 bit, `flacenc`, Apache-2.0) oder MP3 (320 kbit/s, LAME über `mp3lame-encoder`, LGPL-3.0) nach Dateiendung, im Fenster (Strg+E) und in `woa-headless render`. Neu: `woa_engine::export`. `deny.toml`: Ausnahme für die veraltete Kennung „LGPL-3.0“ der zwei LAME-Crates.

## 0.1.7 — 2026-10-29

- **Paket 2 „Bearbeiten und Export“.** Clips per Ziehen verschieben (Raster 1/4 Beat), am roten Kopf schneiden (S oder Knopf, ein Undo-Schritt), Fade-in und Fade-out je Clip (Zeile über den Spuren, als Schatten im Clip sichtbar), Wellenform in den Clips, „Datei → Exportieren als WAV“ (Strg+E) bis zum Ende des letzten Clips.
- Modell: `Clip.offset_beats`, `fade_in_beats`, `fade_out_beats` (im JSON nur wenn nicht 0), Befehl `EditClip` mit Prüfung (Fades passen in den Clip) und Undo, `Clip::new`, `Clip::end_beats`.
- Engine: Fades in `ClipPlayer`, `clip::place_clip`, `clip_graph`, `render_clips`, `write_wav_stereo`, `peaks`; Fenster und `woa-headless` nutzen denselben Weg. `render` ohne `--seconds` rendert bis zum letzten Clip.

## 0.1.6 — 2026-10-29

- Pegelanzeige am Anfang jeder Spur (André): Spitzenpegel der Wiedergabe nach Gain, Mute und Solo, dB-Skala -60..0 dB, grün, ab -12 dB gelb, ab -3 dB rot, mit Abfall. Engine: `ClipPlayer::with_meter`, `clip::take_peak`, lock-frei per Atomic.

## 0.1.5 — 2026-10-29

- **Paket „Aufnehmen und Abspielen“** (erstes Abnahme-Paket, `docs/abnahme.md`). Fenster: Transportleiste (⏮, ▶/⏸, Zeit; Leertaste, Pos1), Zeitleiste je Spur mit Clips und rotem Abspielkopf, Klick setzt den Kopf, Clip anklicken und Entf löscht (mit Undo). Aufnahme startet am Abspielkopf auf der gewählten Spur (sonst neue Spur), die anderen Spuren laufen mit (Overdub), danach liegt der Take als Clip auf der Spur, Aufnahme samt neuer Spur ist ein Undo-Schritt. „Speichern unter“ kopiert die Takes in den Projektordner.
- Modell: `Clip.audio` (Pfad relativ zum Projekt, im JSON nur wenn gesetzt, alte Projekte laden unverändert).
- Engine: `clip::ClipPlayer` liest die Transportposition lock-frei und spielt Clips sample-genau, `clip::load_wav_mono` (Mono-Mix, lineares Resampling).
- `woa-headless render <projekt> <out.wav>` rendert offline, `play --project <projekt>` spielt ein Projekt am Gerät.
- Sicherheit: Clip-Pfade nur relativ im Projektordner (`is_safe_audio_path`, kein `..`, keine Symlinks), WAV-Laden nur 8–384 kHz und höchstens 6 h.
- Arbeitsweise: Abnahme in Paketen statt je Funktion (CLAUDE.md Regel 7, Konzept).

## 0.1.4 — 2026-10-29

- Aufnahme vorgezogen (Teil von M6): `woa-engine::record` nimmt den Eingang lock-frei über `rtrb` auf, ein Schreib-Thread schreibt 32-bit-Float-WAV und aktualisiert den Kopf alle 0,5 s. `woa-headless record <datei.wav> --seconds N` mit Pegelbalken, `devices --input`. Im Fenster: Knopf „● Aufnahme“ (Strg+R) mit Pegelanzeige, Takes landen als `aufnahme-NNN.wav` im Projektordner unter `aufnahmen/`, bei ungespeicherten Projekten unter `~/.local/share/woa/aufnahmen/`. CI installiert unter Linux `libasound2-dev`.

## 0.1.3 — 2026-10-29

- `woa-headless play`: Standard-Lautstärke 0.3 statt 0.1; mit 25 % Systemlautstärke war der Testton nicht zu hören (Abnahme M1 durch André).

## 0.1.2 — 2026-10-29

- M1-Abnahme am Gerät (#2): `woa-headless devices` listet die Ausgabegeräte, `woa-headless play` spielt einen Testton (Vorgabe 440 Hz, leise, Puffer 128, 10 s) über die Engine und meldet `underruns=N`, Exit 1 bei mindestens einem Underrun. Nur mit Feature `device` (Linux: `alsa-lib-devel`). `woa-engine`: `device::output_sample_rate`, Gerätewahl in einer Funktion.

## 0.1.1 — 2026-10-29

- Entscheidungen 29.10. (André) in `docs/entscheidungen.md`: CDLA-Permissive-2.0 nur für `webpki-roots`, Pan Constant-Power, SoundFont GeneralUser GS 2.0.3 (Lizenz in `docs/sounds/`), Feedback-Rolle mehrinstanzfähig per bretteins.infra#145.

- Runbook `docs/runbook-morgen.md` (Merge-Reihenfolge, Server, Hardware-Test) samt Handgriffen für Merge-Erlaubnis und Worktree-Regel.
- M1 (#2) Instrumente (`woa-dsp`): polyphoner Synth (Säge/Rechteck, ADSR, Voice-Stealing), Sampler (Loop, Tonhöhe per Rate, lineare Interpolation) und SoundFont-Player auf Basis von `rustysynth` (MIT); Note-Events sample-genau über `rtrb`-Queue (`InstrumentNode`). Tests unter `assert_no_alloc`, Golden-Renders für Synth, Sampler und SoundFont. Review: versehentlich geleerte CHANGELOG wiederhergestellt.
- M1 (#2) Transport, Geräteauswahl, Projekt-Render (`woa-engine`): Transport als Uhr (Play/Stop/Seek, Position in Samples als Atomic, Loop-Bereich samplegenau auch bei krummen Blöcken, Tempo aus `woa-model`, Beats↔Samples), Stereo-Ausgang im Graphen (`set_stereo_output`, `process_stereo`), `build_project_graph` baut aus einem Projekt im Control-Thread Quelle → Spur-Gain (dB, Mute/Solo) → Constant-Power-Pan → Summe L/R, Änderungen geglättet per `ProjectGraph::update`; `render_project` und Stereo-WAV. Feature `device`: Ausgabegeräte auflisten, Gerät per Id wählen, Puffergröße 128 als Vorgabe mit sauberem Fehler bei unpassendem Bereich, Stereo-Callback. Underrun-Zähler (Atomic, zählt `Xrun` im Fehlerpfad). Tests: Golden `tests/golden/project_mix_2tracks_05s.wav`, Pan, Solo, Transport, Loop, alles unter `assert_no_alloc`.
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
