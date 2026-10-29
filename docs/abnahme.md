# Abnahme in Paketen

André nimmt fertige Pakete ab, nicht einzelne Funktionen (29.10.2026). Claude baut ein Paket am
Stück, prüft selbst (Tests, Offline-Render, Probelauf am Gerät) und merged. Hier steht je Paket
eine kurze Klickliste. Ein „passt“ von André schließt das Paket; was hakt, wird ein Issue.

## Nachtrag #43: Wellenform während der Aufnahme (0.3.5, offen)

Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. Spur wählen, Quelle z. B. „Programm: firefox“ mit laufender Musik, „Aufnahme“ drücken: Ab dem Abspielkopf wächst ein roter Clip „Aufnahme läuft“, darin die Wellenform. Der Pegel oben läuft mit.
2. Laut aufnehmen, bis es übersteuert: Neben dem Pegel steht rot „ÜBERSTEUERT“.
3. Stopp: Der rote Clip wird zum normalen Clip mit derselben Wellenform an derselben Stelle.

Selbst geprüft von Claude: Workspace-Tests grün, darunter die Peak-Sammlung und `assert_no_alloc`. Kein Bildschirmfoto: Ohne Mikrofon oder hörbare Quelle lässt sich keine Aufnahme starten (Andrés Regel), eine Testquelle gibt es nicht.

## Paket 6: Agent (0.1.16, offen)

Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo build --release -p woa-headless -p woa`

1. Eintragen: `claude mcp add woa -- ~/GitHub/nixblick/daw.woa/target/release/woa-headless mcp ~/woa-test.woa`
2. Claude Code neu starten und sagen: „Bau mir einen 8-Takt-Loop mit -14 LUFS und exportiere ihn.“ Claude legt Spuren an, erzeugt Drums, Bass und Akkorde, misst mit `analyze` und exportiert.
3. Die Datei anhören, die Claude nennt (z. B. `~/woa-test.woa/loop.wav`): 16 Sekunden bei 120 BPM, Drums, Bass, Akkorde, nicht übersteuert.
4. In WOA öffnen: `cargo run -q --release -p woa -- ~/woa-test.woa`. Drei Spuren mit Clips und Wellenform, ▶ spielt den Loop. (Claudes Schritte lassen sich nur in der laufenden MCP-Sitzung zurücknehmen, „mach den letzten Schritt rückgängig“; die Undo-Liste wird nicht mit dem Projekt gespeichert.)

Selbst geprüft von Claude: Abnahmetest `crates/agent/tests/loop.rs` baut den Loop nur über MCP-Tools (Tempo 120, drei Spuren, ein Takt Drums ×8, zwei Takte Bass und Akkorde ×4, zwei Gain-Schritte), exportiert mit `target_lufs: -14` und misst die Datei neu: −14,00 LUFS, 16,00 s, Spitze −2,8 dBFS. ffmpeg `ebur128` bestätigt −14,0 LUFS und True Peak −2,8 dBFS. Jeder Fehlerfall der neuen Tools liefert `isError` und lässt Projekt und Undo-Liste unverändert. GUI per `WOA_SCREENSHOT` am gespeicherten Testprojekt: drei Spuren (Drums −5 dB, Bass 0 dB, Akkorde −6 dB) mit Clips und Wellenform über 8 Takte. Nicht selbst geprüft: Abspielen mit ▶ und der Weg über ein echtes Claude Code mit `claude mcp add` (dafür ist Schritt 1–2 da).

Noch nicht drin: MIDI-Clips mit Noten im Projekt (die Noten werden sofort zu Audio), True-Peak-Messung in `analyze`. (Import von FLAC/MP3 geht seit 0.2.1.)

## M0 Fundament (offen)

Abnahmekriterium (#1): Projekt-Roundtrip, Feedback erzeugt GitHub-Issue, Logs kommen auf dem Server an.

1. Roundtrip: `cargo run -q -p woa-headless -- new /tmp/m0 && cargo run -q -p woa-headless -- roundtrip /tmp/m0` meldet „roundtrip: gleich“.
2. Feedback: In WOA „Feedback …“, Text schreiben, Senden: „Danke! Gemeldet als #n“, Issue n steht in daw.woa (Gegenprobe #40).
3. Logs: WOA starten, im Dialog „Fehlercodes senden?“ „Ja, anonym senden“ wählen (sonst „Einstellungen“ → Haken). Unter `~/.config/woa/config.json` steht `"telemetry_enabled": true` und eine `install_id`.
4. Erst nach dem Ausrollen des Server-PRs (bretteins.infra, `--tags feedback_woa,edge`): `cargo run -q -p woa-headless -- log-test` meldet „Server hat angenommen“ (heute noch 404).

Selbst geprüft von Claude: Workspace-Tests grün, darunter Opt-in gegen lokalen Fake-Server (`crates/gui/tests/telemetry_optin.rs`: ohne Ja geht nichts raus, mit Ja kommt der Code an und der Puffer wird geleert) und die Roundtrip-Tests (`apps/woa-headless/tests/cli.rs`, `crates/model/tests/model.rs`).

## Paket 5: Quellen (0.1.11, offen)

Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. Im Browser oder Player Musik laufen lassen. Bei Spur 1 unten in der Liste „Programm: firefox“ (oder den Player) wählen. Roter Punkt an, Aufnahme, nach ein paar Sekunden stoppen: Die Musik liegt auf Spur 1.
2. Bei Spur 2 „Mikrofon“ lassen und den roten Punkt ebenfalls an: Bei der Aufnahme laufen beide Pegel, danach hat jede Spur ihren eigenen Clip ab derselben Stelle.
3. In der Liste „Systemklang“ wählen: Damit wird alles aufgenommen, was gerade zu hören ist.
4. Bei einem Eingang (Liste, „Eingang: …“) den Haken „hören“ setzen, mit Kopfhörer aufnehmen: Du hörst dich selbst.
5. „Stopp am Ende“ (neben ▶, orange = an): Die Wiedergabe hält am Ende des letzten Clips an. Noch einmal ▶ startet von vorn.
6. (#44, 0.3.6) Erst die Quellenliste einer Spur öffnen, dann in Chromium Musik starten und die Liste erneut öffnen: Dort steht „Programm: Chromium“ (bei sprechendem Titel mit „– Titel“), WOA selbst steht nicht drin. Musik stoppen, Liste erneut öffnen: Ohne Programm erscheint unten grau „Programm spielt gerade keinen Ton?“.

Selbst geprüft von Claude: Quellenliste gegen PipeWire (Eingang, Systemklang, Programme). Ein Testton mit Lautstärke 0,05 kam über Systemklang und über „Programm“ jeweils mit Spitze 0,050 an (`woa-headless sources` und `capture`). Workspace-Tests grün.

Noch nicht drin: Windows (WASAPI-Loopback), Quellenwahl im Projekt speichern.

## Abgenommen

- Paket 8 Sendeprozess (0.3.0, mit 0.3.4): 29.10.2026, André „stream geht, abnahme paket 8 fertig!“

- Paket 7 Sendeablauf (0.2.0): 29.10.2026, André „abnahme paket 7 fertig“.

- Paket 4 Radio-tauglich (0.1.9): 29.10.2026, André „funktioniert alles“.

- Paket 3 Arrangieren (0.1.8): 29.10.2026, André „passt“.

- Paket 2 Bearbeiten und Export (0.1.7): 29.10.2026, André „passt“.

- Paket 1 Aufnehmen und Abspielen (0.1.6): 29.10.2026, André „passt“.

- M1 Engine: 29.10.2026, 60 s Testton sauber, `underruns=0` (#2).
