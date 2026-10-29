# Abnahme in Paketen

André nimmt fertige Pakete ab, nicht einzelne Funktionen (29.10.2026). Claude baut ein Paket am
Stück, prüft selbst (Tests, Offline-Render, Probelauf am Gerät) und merged. Hier steht je Paket
eine kurze Klickliste. Ein „passt“ von André schließt das Paket; was hakt, wird ein Issue.

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

Selbst geprüft von Claude: Quellenliste gegen PipeWire (Eingang, Systemklang, Programme). Ein Testton mit Lautstärke 0,05 kam über Systemklang und über „Programm“ jeweils mit Spitze 0,050 an (`woa-headless sources` und `capture`). Workspace-Tests grün.

Noch nicht drin: Windows (WASAPI-Loopback), Quellenwahl im Projekt speichern.

## Abgenommen

- Paket 4 Radio-tauglich (0.1.9): 29.10.2026, André „funktioniert alles“.

- Paket 3 Arrangieren (0.1.8): 29.10.2026, André „passt“.

- Paket 2 Bearbeiten und Export (0.1.7): 29.10.2026, André „passt“.

- Paket 1 Aufnehmen und Abspielen (0.1.6): 29.10.2026, André „passt“.

- M1 Engine: 29.10.2026, 60 s Testton sauber, `underruns=0` (#2).
