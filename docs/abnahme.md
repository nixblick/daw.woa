# Abnahme in Paketen

André nimmt fertige Pakete ab, nicht einzelne Funktionen (29.10.2026). Claude baut ein Paket am
Stück, prüft selbst (Tests, Offline-Render, Probelauf am Gerät) und merged. Hier steht je Paket
eine kurze Klickliste. Ein „passt“ von André schließt das Paket; was hakt, wird ein Issue.

## Paket 8: Sendeprozess (0.3.0, offen)

Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo build -q --release -p woa -p woa-streamd && cargo run -q --release -p woa`

Erst nach dem Ausrollen des Server-PRs in bretteins.infra (#150).

1. Kennwort in den Vault: den Wert aus dem Server-PR als eine Zeile nach `~/.secrets/daw.woa/shoutcast-source.pass`, danach `chmod 600`.
2. Optional eine Notfall-Playlist eintragen: in `~/.config/woa/config.json` unter `"stream": {"emergency": ["/pfad/zu/ordner"]}`.
3. Radio-Modus, Playlist starten, „Auf Sendung“ drücken. Die Zeile daneben zeigt „verbunden · Sendemix“.
4. Im Browser <https://feedback.wironair.de/stream> öffnen: Du hörst deine Sendung (wenige Sekunden verzögert).
5. WOA hart beenden (`pkill -9 -x woa`). Nach etwa 2 s spielt im Browser die Notfall-Playlist, ohne Abriss.
6. WOA neu starten und Playlist starten: Die Zeile zeigt wieder „verbunden“, nach 1 s Überblendung läuft die Sendung.
7. „Sendung beenden“: Der Browser-Stream endet.

Gegenprobe ohne Oberfläche: `cargo run -q -p woa-headless -- stream-test --seconds 30` sendet 30 s Testton (Exit 3 = Kennwort abgelehnt).

Selbst geprüft von Claude, nur gegen einen lokalen Fake-Server und nicht gegen lfs001 (das ist deine Gegenprobe):

- Anmeldung, MP3 kommt an, Titel per admin.cgi, Hörerzahl, Wiederverbinden nach Abriss, falsches Kennwort als klarer Fehler ohne Kennwort im Text (`crates/stream/tests/shoutcast.rs`).
- Oberfläche weg führt nach höchstens 2,5 s zur Notfall-Playlist, und zurück (`crates/stream/tests/failover.rs`).
- Die lokale Verbindung weist falsche Token, übergroße Rahmen und stumme Gegenüber ab. Auch bei 12 stummen Verbindungen kommt die Oberfläche herein.
- Der Socket-Ordner hat 0700.
- Der Mixer schreibt in den Sende-Ring ohne Allokation (`assert_no_alloc`, `crates/radio/tests/rt.rs`).
- `woa-headless stream-test` gegen einen Fake: Exit 0 und Exit 3 (`apps/woa-headless/tests/cli.rs`).
- Die Oberfläche ist gebaut. Den Knopf habe ich nicht am Gerät geklickt, und es lief kein echter Ton über lfs001.

Noch nicht drin:

- Titel: Shoutcast 1 trägt sie nicht in der Quellverbindung, und `admin.cgi` hätte das Kennwort im Klartext in der URL. Deshalb gibt es sie nur bei lokalem Server.
- Hörerzahl: Der Admin-Port 8000 ist von außen gesperrt.
- Die Quellanmeldung läuft im Klartext (bekanntes Risiko).
- Icecast.
- Uhrdrift zwischen Soundkarte und woa-streamd: Der Live-Puffer wird auf 0,5 s gekappt.
- Unter Windows ist die lokale Verbindung TCP mit Token. Ein lokaler Prozess kann dort das Verbinden stören, aber nicht senden.

## Paket 7: Sendeablauf (0.2.0, offen)

Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. Oben rechts „Radio“ anklicken: Links stehen Uhr, Start/Stopp und Playlist, rechts die Cart-Wall. Beim nächsten Start ist WOA wieder im Radio-Modus.
2. Zwei oder drei Audiodateien (WAV, FLAC oder MP3) in die Playlist ziehen (oder „+ Titel …“). Nach ein paar Sekunden stehen Länge und „bis Playlist-Ende“; beim Überfahren eines Titels mit der Maus siehst du LUFS und Auto-Gain.
3. „▶ Start“ (oder Leertaste): Die große Uhr zählt die Restzeit des Titels. Am Ende blendet er in den nächsten über, beide gleich laut. „▶ Weiter“ blendet sofort über, „■ Stopp“ blendet aus.
4. Mit „^ v x“ die Reihenfolge ändern oder einen Titel entfernen, auch während der Sendung.
5. Rechtsklick auf ein Cart-Feld, „Belegen …“, Jingle wählen. Mit Klick oder Taste 1 spielt er über die Musik.
6. Kopfhörer auf, „Mikro (M)“ oder Taste M: Die Musik geht weich um 12 dB herunter, dein Mikro ist im Mitschnitt. Noch einmal M, und die Musik kommt zurück.
7. „Mitschnitt“ an, etwas senden, wieder aus. Unter `~/.local/share/woa/mitschnitte/` liegt `sendung-<Datum>.wav` (Zeit in UTC).
8. Agent: `woa-headless mcp <projekt>` mit `build_playlist` füllt `~/.local/share/woa/radio.json`; die offene App übernimmt die neue Playlist nach höchstens zwei Sekunden.

Selbst geprüft von Claude:

- **Tests (workspace grün).** Jeder Process-Aufruf des Sendemixers lief unter `assert_no_alloc` (`crates/radio/tests/rt.rs`), nämlich:
  - Crossfade mit konstanter Leistung, in der Mitte genau √2 des Einzelpegels
  - Ducking mit −12 dB ± 0,2, ohne Sprung
  - Cart über Musik
  - Stopp
  - Mitschnitt als 16-Bit-WAV
- **Offline-Render.** `woa-headless radio-render` mit drei Titeln von sehr unterschiedlichem Pegel: Länge = Summe − 2 × 3 s, Lautheit −16 ± 1 LUFS (`apps/woa-headless/tests/radio.rs`).
- **Agent-Werkzeuge.** `build_playlist`/`run_order` (`crates/agent/tests/radio.rs`) schreiben nur in die Vorgabe oder relativ im Projektordner. `../`, absolute Pfade und Symlink-Ziele lehnen sie ab.
- **Probe am Gerät (PipeWire, 48 kHz).** `woa-headless radio-play` mit 22 s, Cart nach 4 s und Mikro nach 14 s:
  - 0 Gerätefehler
  - Überblendungen nach 9 s und 16 s wie berechnet
  - Mitschnitt 22,06 s, −15,6 LUFS (ffmpeg ebur128)
  - Musik mit offenem Mikro 11,4 dB leiser (RMS, zwei verschiedene Titel)
- **Bildschirmfoto.** Mit `WOA_SCREENSHOT` des Radio-Layouts aufgenommen, mit Temp-Konfiguration.
- **Nicht von Hand geklickt.** Drag-and-drop, Dateidialoge und Mikro über echten Eingang. Der Mikro-Weg ist nur in Tests mit Ring-Eingang geprüft. Hat der Eingang eine andere Abtastrate als die Ausgabe, meldet WOA das und öffnet das Mikro nicht.

Nachtrag 0.2.1: **MP3 und FLAC gehen jetzt** überall, wo WOA Audio liest: Playlist, Cart-Wall, LUFS-Messung, `radio-render`, Clips im Projekt, Wellenform und Agent-Import. Dialoge und Drag-and-drop nehmen `.mp3` und `.flac` an. Selbst geprüft: Testdateien per `flacenc` und LAME erzeugt, gelesen, Länge und Pegel mit dem Original verglichen (`crates/engine/src/decode.rs`). Nebenbei repariert: WOAs eigene FLAC-Exporte hatten eine falsche Mindest-Blockgröße und waren für strenge Leser unlesbar. Bildschirmfoto bei 900 px: Die Regler „Crossfade“ und „Ducking“ sind ganz lesbar.

Nachtrag 0.3.1: **Alle gängigen Formate.** Playlist, Cart-Wall und Agent-Import lesen jetzt auch OGG/Vorbis, M4A (AAC und ALAC), AAC, AIFF, CAF und MP2, Groß- und Kleinschreibung egal. Der Dialog heißt „Audio (MP3, FLAC, WAV, OGG, M4A, AAC, AIFF, CAF, MP2)“. MP3 mit großem Cover-Bild (#50, „150 Jahre Feuerwehr Frankfurt“) geht wieder. Zum Abnehmen: die Feuerwehr-MP3 und je eine OGG- und M4A-Datei in die Playlist ziehen, alle drei bekommen eine Länge und spielen. Selbst geprüft: jedes Format per ffmpeg erzeugt und auf Länge und Pegel geprüft, Andrés MP3 gelesen. Opus geht nicht (kein Decoder in symphonia).

Nachtrag 0.3.2: **Cover-Bild.** Die Feuerwehr-MP3 in die Playlist ziehen und doppelklicken (oder starten): Links neben der großen Uhr erscheint ihr Cover, in der Playlist ein kleines Vorschaubild. Ein Titel ohne Bild zeigt eine Note. Beim Wechsel zum nächsten Titel blendet das Bild weich über. Selbst geprüft: Bildschirmfoto mit Wegwerf-Konfiguration (Feuerwehr-MP3, M4A mit Testbild, OGG ohne Bild), alle drei Fälle richtig; Tests für das Auslesen aus MP3, FLAC, M4A und OGG. Nicht selbst geprüft: die Überblendung beim echten Titelwechsel (nur im Code, kein Video).

Noch nicht drin:

- Mikro im Kopfhörer (Monitor ist aus, gegen Rückkopplung)
- Stream ins Netz
- Notfall-Playlist
- 8-Stunden-Dauertest

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

Selbst geprüft von Claude: Quellenliste gegen PipeWire (Eingang, Systemklang, Programme). Ein Testton mit Lautstärke 0,05 kam über Systemklang und über „Programm“ jeweils mit Spitze 0,050 an (`woa-headless sources` und `capture`). Workspace-Tests grün.

Noch nicht drin: Windows (WASAPI-Loopback), Quellenwahl im Projekt speichern.

## Abgenommen

- Paket 4 Radio-tauglich (0.1.9): 29.10.2026, André „funktioniert alles“.

- Paket 3 Arrangieren (0.1.8): 29.10.2026, André „passt“.

- Paket 2 Bearbeiten und Export (0.1.7): 29.10.2026, André „passt“.

- Paket 1 Aufnehmen und Abspielen (0.1.6): 29.10.2026, André „passt“.

- M1 Engine: 29.10.2026, 60 s Testton sauber, `underruns=0` (#2).
