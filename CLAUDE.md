# daw.woa – WOA, Wir on Air

Radio-DAW in Rust für Linux und Windows, GPLv3. Drei Modi (Produzent, Radio, Party) auf einer Engine;
Mensch und KI-Agent (MCP) gleichberechtigt.

**Das vollständige Konzept steht in [`docs/konzept.md`](docs/konzept.md). Vor jeder Arbeit lesen.**
Was dort steht, gilt; diese Datei wiederholt nur die Regeln, die bei jeder Zeile Code gelten.

## Echtzeit- und Stabilitätsregeln

Diese Regeln stehen wörtlich in der `CLAUDE.md` und gelten für jede Zeile Code.

1. Im Audio-Thread: keine Allokation, kein Mutex, kein I/O, kein Logging.
2. Kommunikation nur über lock-free Queues (`rtrb`).
3. Struktur-Änderungen baut der Control-Thread als neuen Graphen, atomarer Tausch (`arc-swap`), Freigabe außerhalb des Audio-Threads (`basedrop`).
4. Buffer vorab allokieren; Parameter glätten gegen Zipper-Noise.
5. Tests mit `assert_no_alloc` um jeden Process-Call.
6. Kein `unwrap()` außerhalb von Tests; jeder Fehler wird geloggt und sauber behandelt.
7. Autosave alle paar Sekunden, Wiederherstellung nach Absturz.
8. Stream-Prozess getrennt von der Oberfläche; Notfall-Playlist bei Ausfall.
9. Vor jedem Release ein Dauertest von mindestens 8 Stunden Radio-Betrieb ohne Underrun.

## Architektur in einem Satz

GUI und Agent schreiben nie direkt in die Engine. Beide schicken Commands an eine gemeinsame Schicht
(`crates/model`); daraus entstehen Undo, Projektdatei und der Audio-Graph. Stream-Encoder und
Notfall-Playlist laufen als eigener Prozess (`apps/woa-streamd`).

## Arbeitsweise

1. Pro Session ein klar umrissenes Feature aus dem aktuellen Meilenstein (Issues mit Label `milestone`,
   M0–M11 der Reihe nach), mit Akzeptanzkriterium. **Plan-Mode zuerst**, Code erst nach Andrés OK.
2. Kleine Commits, ein Feature pro Branch, Merge nur bei grüner CI:
   `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo fmt --check`, `cargo deny check`.
3. Golden-File-Tests für den Klang (`tests/golden/`), `assert_no_alloc` für Echtzeit,
   Snapshot-Tests (`insta`) für das Projektformat.
4. Testen per Feedback-Button: Screenshot plus Hinweis wird Issue, Agent arbeitet Issues ab.
5. Regelmäßige Telemetrie-Auswertung durch Claude Code; Auffälligkeiten werden Issues.
6. Ab und zu Architektur-Review, damit die Codebasis verständlich bleibt.
7. **Abnahme in Paketen** (André, 29.10.2026: „ich will nicht jede funktion einzeln abnehmen … ich nehme pakete ab“).
   Claude baut ein ganzes Nutzer-Paket (z. B. Aufnehmen + Abspielen + Speichern) am Stück, prüft selbst
   (Tests, Probelauf am Gerät) und merged grüne PRs ohne Rückfrage. André nimmt nur das fertige Paket ab,
   mit einer kurzen Klickliste in `docs/abnahme.md`. Keine Einzel-Rückfragen je Funktion.
8. Nach jeder Änderung: `CHANGELOG.md` und Workspace-Version nachziehen, commit, push.
9. Crate-Versionen und Pflegestand vor dem Einbinden prüfen; die Liste im Konzept ist nicht frisch recherchiert.

## Geheimnisse

Nie im Repo. Vault: `~/.secrets/daw.woa/`. Die App enthält keine GitHub-Zugangsdaten; Feedback und
Telemetrie gehen an den eigenen Server (`server/`), der die Issues erzeugt.

## Struktur

`crates/` Bibliotheken (model, engine, dsp, io, library, plugins, radio, stream, guests, remote, agent,
telemetry, updater, gui) · `apps/` woa, woa-headless, woa-streamd · `server/` Feedback/Telemetrie ·
`packaging/` MSI, AppImage, Flatpak · `docs/` Konzept, Anleitungen, Sound-Lizenzen · `tests/golden/` Referenz-Renders.
