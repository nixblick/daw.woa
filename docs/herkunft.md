# Herkunft: was von anderen Projekten kommt

Stand 30.10.2026 (Paket 29). Gesichtet wurden die öffentlichen Repositories von Mixxx und
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

## Regel

Quellen werden nur mit Repository, Datei und Commit genannt, nie mit Pfaden einer Maschine.
Wird Code (nicht nur eine Idee) aus einem fremden Projekt übernommen, steht hier vorher Lizenz
und Wortlaut, und André entscheidet.
