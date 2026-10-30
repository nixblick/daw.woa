# Herkunft: was von anderen Projekten kommt

Stand 30.10.2026 (Paket 37). Gesichtet wurden die öffentlichen Repositories von Mixxx und
Liquidsoap. Übernommen sind Ideen, Randfälle und Testfälle, kein Quelltext.

## Mixxx (`mixxxdj/mixxx`)

- **Gesichtet:** Branch `main`, Commit `bc49961fd3e5a46cd46588090c652a0d7ec44e18`.
- **Dateien:** `src/library/autodj/autodjprocessor.cpp` (`AutoDJProcessor::calculateTransition`,
  `useFixedFadeTime`) und `src/test/autodjprocessor_test.cpp` (Randfälle).
- **Lizenz, Wortlaut aus `LICENSE`:** „Mixxx is free software; you can redistribute it and/or
  modify it under the terms of the GNU General Public License as published by the Free Software
  Foundation; either version 2 of the License, or (at your option) any later version.“
  Das ist GPL-2.0-or-later.
- **Wo bei uns:** `crates/radio/src/transition.rs` (Überblendzeit aus Outro und Intro, Randfälle
  wie sehr kurze Titel, fehlende Marken, Titel kürzer als die Überblendung) und seine Tests.
  Die Logik ist nachgebaut und mit eigenen Tests belegt.
- **Lizenzfrage:** WOA steht unter GPL-3.0-only. GPL-2.0-or-later darf in GPL-3.0 aufgehen;
  trotzdem entscheidet André, ob der Nachbau als Übernahme gilt und wie der Hinweis im Quelltext
  lauten soll (offene Frage im PR).

## Liquidsoap (`savonet/liquidsoap`)

- **Gesichtet:** Branch `main`, Commit `cb8ba5d3276fe6e3c772699c00d885d025384aa4` (29.10.2026).
- **Datei:** `src/libs/cron.liq` (Tabelle von Einträgen, jeder mit eigenem Test, regelmäßig
  abgefragt).
- **Lizenz, Wortlaut aus `README.md`:** „This program is free software; you can redistribute it
  and/or modify it under the terms of the GNU General Public License as published by the Free
  Software Foundation; either version 2 of the License, or (at your option) any later version.“
- **Was wir daraus haben:** nur die Idee: eine Tabelle von Zeit-Einträgen, die ein Takt
  (`thread.when`, etwa alle 20 Sekunden) je Eintrag prüft. Bei uns prüft der Steuer-Thread alle
  10 ms und löst über `EventClock::tick` aus. **Kein Code übernommen.**
  Wo bei uns: `crates/radio/src/events.rs`.

- **Datei:** `src/libs/extra/audio.liq`, Funktion `smooth_add` (Commit `cb8ba5d3276fe6e3c772699c00d885d025384aa4`, Lizenz wie oben: GPL-2.0-or-later).
- **Was wir daraus haben:** nur die Idee: eine Sprachspur über Musik legen und die Musik währenddessen sanft absenken (Anstieg vor der Stimme, Halten, Rückkehr danach). Bei uns ist die Absenkung eine Kosinus-Rampe, die Mixer, Editor und Tests aus einer einzigen reinen Funktion (`voicetrack::duck_gain`) bekommen; die Stimme liegt auf einer festen Zeitachse zum Übergang zweier Titel. **Kein Code übernommen.** Mixxx kennt kein Voice-Tracking.
  Wo bei uns: `crates/radio/src/voicetrack.rs`.

## Plugin-Fenster (Paket 32)

- **Gelesen:** die Schnittstelle der Erweiterungen `gui`, `timer`, `posix-fd` und `params` in
  `clack-extensions` 0.2.0 (Abhängigkeit, kein kopierter Code).
- **Idee, nicht gelesen:** Ardour fasst Reglerbewegungen aus einer Plugin-Oberfläche als „Anfassen
  und Loslassen“ zusammen (start_touch/stop_touch). Der Quelltext von Ardour wurde dafür nicht
  gelesen; die Zusammenfassung in `crates/plugins/src/feedback.rs` ist eigener Entwurf aus der
  CLAP-Beschreibung der Gesten. **Kein Code übernommen.**

## Plugin-Prozess (Paket 33)

- **LMMS** (`LMMS/lmms`, GPL-2.0-or-later), `include/RemotePluginBase.h` und
  `src/core/RemotePlugin.cpp`, Commit bed447b. **Nur die Idee gelesen:** Das Plugin läuft in einem
  eigenen Prozess, Audio geht über gemeinsamen Speicher, Befehle über einen Nachrichtenweg, der
  Hauptprozess merkt, wenn der Partner stirbt. Die Umsetzung in
  `crates/plugins/src/{shm,remote,child}.rs` ist ein eigener Entwurf (Rust, Atome und Briefkasten
  statt Semaphoren und Pipes, andere Einteilung). **Kein Code übernommen.**

## Sperrliste und Plugin-Logs (Paket 34)

- **Ardour** (`Ardour/ardour`, GPL-2.0-or-later laut Kopfzeile der Datei), `libs/ardour/plugin_manager.cc`
  und `libs/ardour/ardour/plugin_manager.h`, Commit a12b4e108ea6. **Nur die Idee gelesen:** Der
  Plugin-Verwalter führt je Plugin einen Eintrag mit Ergebnis der Suche (neu, in Ordnung, Fehler,
  Zeitüberschreitung, gesperrt) samt Meldungen; ein Plugin, das beim Suchen scheitert, wird gesperrt
  und beim nächsten Suchen übersprungen, die Sperre lässt sich löschen. Bei uns: `crates/plugins/src/safety.rs`
  (Sperrliste als Textdatei, Log je Plugin) und `Catalog::scan_guarded` in `remote.rs`. Die Suche
  im eigenen Prozess stammt schon aus Paket 33 (LMMS-Idee). Eigener Entwurf in Rust, andere
  Einteilung (Datei statt Plugin-Kennung als Sperrschlüssel). **Kein Code übernommen.**
- **Nicht gelesen:** `tracktion_engine` (Plugin-Graph); Paket 34 baut keinen Graph.

## Party-Layout (Paket 35)

- **Mixxx** (`mixxxdj/mixxx`, GPL-2.0-or-later, Commit `bc49961fd3e5`), `src/library/autodj/autodjprocessor.cpp`
  (`AutoDJProcessor::skipNext`, `fadeNow`). **Nur die Idee gelesen:** Mixxx trennt „nächsten Titel aus
  der Warteschlange überspringen“ (`skip_next`) von „jetzt überblenden“ (`fade_now`), beide als benannte
  Steuerungen, die Oberfläche, Controller und Skripte gleich ansprechen. Bei uns: „Weiter“ ist das
  Überblenden jetzt (`Controller::start_or_next`), und alle Tasten sind `PartyCommand`s mit Protokoll
  (`crates/radio/src/party.rs`), damit Fernbedienung und MIDI später dieselben Befehle schicken.
  Eigener Entwurf in Rust. **Kein Code übernommen.**

## Web-Fernbedienung (Paket 36)

- **Ardour** (`Ardour/ardour`, GPL-2.0-or-later laut Kopfzeile von `libs/surfaces/osc/osc.cc`, Commit
  `a12b4e108ea6`), `libs/surfaces/osc/osc.h` und `osc.cc`. **Nur die Idee gelesen:** Jede Oberfläche
  (Surface) bekommt beim Anmelden den Vollstand und danach nur noch Rückmeldungen, welche Gruppen sie
  abonniert hat (Bitmaske `feedback`); Befehle haben feste OSC-Pfade. Bei uns: `crates/remote/src/snapshot.rs`
  (Vollstand beim Verbinden, danach `delta` mit nur den geänderten Feldern) und `crates/remote/src/osc.rs`
  (feste Pfade `/woa/party/...`, ein Pfad je Befehl). Eigener Entwurf in Rust. **Kein Code übernommen.**
- **Mixxx** (`mixxxdj/mixxx`, GPL-2.0-or-later, Commit `bc49961fd3e5`), `src/controllers/controller.h`.
  **Nur die Idee gelesen:** Controller sind eine Basisklasse, die benannte Steuerungen anspricht, statt
  einen Sonderweg je Gerät zu haben. Bei uns: Handy, OSC und große Tasten schicken dieselben
  `PartyCommand`s (`crates/remote/src/command.rs` übersetzt, `woa_radio::party::PartyState::apply` führt aus).
  **Kein Code übernommen.**
- **Liquidsoap** (`savonet/liquidsoap`, GPL-2.0-or-later, Commit `cb8ba5d3276f`), `src/core/sources/harbor_input.ml`.
  **Nur die Idee gelesen:** Ein Eingang hat einen Vorlauf (`buffer`, die Wiedergabe beginnt erst, wenn er
  gefüllt ist) und eine Obergrenze (`max`, mehr wird nicht gepuffert; `buffer` muss kleiner als `max` sein).
  Bei uns: `crates/guests/src/jitter.rs` (60 ms Vorlauf, 400 ms Obergrenze, Aussetzer und Überläufe gezählt
  und geloggt). Eigener Entwurf in Rust mit `rtrb`. **Kein Code übernommen.**
- **Nicht gelesen:** `tracktion_engine`; für Paket 38 nicht gebraucht.

## Browser-Gast (Paket 38)

- **str0m** (`algesten/str0m`, MIT OR Apache-2.0, Version 0.24.1), `examples/chat.rs`. **Nur das Muster gelesen und nachgebaut:** Eine Schleife mit `poll_output` (Timeout, Transmit, Event) und `handle_input` (Datagramm oder Zeitablauf), Angebot und Antwort über `sdp_api`, Medien über `writer(mid)`. Eigener Entwurf in `crates/guests/src/net.rs` (ein Thread für alle Gäste, Rückweg über `rtrb`). **Kein Code übernommen.** Als Bibliothek eingebunden mit der Lizenz MIT OR Apache-2.0; Opus (`opus` 0.4, MIT/Apache-2.0, Codec BSD-3-Clause) und `rustls`/`rcgen` ebenso.
- **Liquidsoap** (`savonet/liquidsoap`, GPL-2.0-or-later), Harbor-Eingang: die Idee eines Eingangs mit Vorlauf und Obergrenze, schon in Paket 37 genutzt; der Gast-Eingang aus dem Netz schreibt in denselben Jitterpuffer. Kein Code übernommen.
- **Nicht gelesen:** Mixxx, Ardour, Tracktion Engine (für dieses Paket nicht nötig).

## Linux-Pakete (Paket 39)

- **LMMS** (`LMMS/lmms`, GPL-2.0-or-later, Commit `bed447baab18`), `cmake/linux/LinuxDeploy.cmake`. **Nur den Ablauf gelesen:** AppDir mit `usr/`, Startmenü-Eintrag um `X-AppImage-Version` ergänzen, harte Kopie des 256-Pixel-Symbols als `.DirIcon`, `linuxdeploy` sammelt Bibliotheken, `appimagetool` schreibt das Paket, Werkzeuge laufen entpackt ohne FUSE. Bei uns: `packaging/linux/build-appimage.sh` als Shell-Skript, Werkzeuge mit fester Version statt „continuous“. **Kein Code übernommen.**
- **Mixxx** (`mixxxdj/mixxx`, GPL-2.0-or-later, Commit `bc49961fd3e5`), `packaging/flatpak/org.mixxx.Mixxx.yaml`. **Nur die Zugriffsrechte gelesen** (Wayland, X11-Rückfall, Pulse, alle Geräte, Netz, IPC, PipeWire-Socket, Musik und Wechseldatenträger). Bei uns: `packaging/linux/flatpak/de.wironair.woa.yaml`. **Kein Code übernommen.**
- **flatpak-builder-tools** (`flatpak/flatpak-builder-tools`, MIT, Commit `9d86d336a39b`), `cargo/flatpak-cargo-generator.py`. Wird im CI-Lauf heruntergeladen und ausgeführt, nicht ins Repo kopiert.
- **Nicht gelesen:** Audacity (Windows-Teil, MSI folgt in Paket 40).

## Release-Workflow (Paket 40)

- **Mixxx** (`mixxxdj/mixxx`, GPL-2.0-or-later, Commit `bc49961fd3e5`), `.github/workflows/release.yml`. **Nur den Aufbau gelesen:** ein Release-Workflow löst bei einem Tag aus und setzt die Bauten (Plattformen, AppImage, Flatpak) aus eigenen, wiederverwendbaren Workflows (`workflow_call`) zusammen; Prüfsummen liegen als Datei neben dem Paket. Bei uns: `release.yml` ruft `windows-build.yml` und `linux-build.yml` auf, dazu eine gemeinsame `SHA256SUMS`. **Kein Code übernommen.**
- **Nicht gelesen:** Audacity und LMMS für diesen Teil (Windows-Signierung und Mixxx' Hochladen auf eigene Server sind bewusst nicht nachgebaut: Zertifikat und Veröffentlichung sind Andrés Entscheidung).

## Schriften des Design Systems

Liegen als woff2 in `docs/design/woa-design-system/fonts/`; für egui später als TTF. Alle unter der
**SIL Open Font License 1.1** (frei nutzbar, einbettbar und weitergebbar, nicht einzeln verkaufen;
Lizenztext gehört zu jeder Weitergabe).

- **Barlow** (Jeremy Tribby, `jpt/barlow`, OFL 1.1): Oberfläche und Listen (`font-ui`).
- **Barlow Condensed** (Jeremy Tribby, `jpt/barlow`, OFL 1.1): Pult-Beschriftungen, Titel, Party-Anzeigen (`font-label`).
- **JetBrains Mono** (JetBrains, `JetBrains/JetBrainsMono`, OFL 1.1): Zahlen, Zeiten, Messwerte (`font-mono`).
- **DSEG7 Classic** (Keshikan, `keshikan/DSEG`, OFL 1.1): Leuchtziffern in LCDs und Uhren (`font-seg`).

## Regel

Quellen werden nur mit Repository, Datei und Commit genannt, nie mit Pfaden einer Maschine.
Wird Code (nicht nur eine Idee) aus einem fremden Projekt übernommen, steht hier vorher Lizenz
und Wortlaut, und André entscheidet.
