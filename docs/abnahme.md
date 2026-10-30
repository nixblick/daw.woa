# Abnahme in Paketen

André nimmt fertige Pakete ab, nicht einzelne Funktionen (29.10.2026). Claude baut ein Paket am
Stück, prüft selbst (Tests, Offline-Render, Probelauf am Gerät) und merged. Hier steht je Paket
eine kurze Klickliste. Ein „passt“ von André schließt das Paket; was hakt, wird ein Issue.

## Paket 16: Mixer mit Bussen, Sends und Inserts (0.11.0, offen)

Aus M5. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. **Mixer öffnen:** Produzent-Layout, unten im Studio-Dock den Reiter „Mixer“. Jede Spur hat jetzt Ausgang, Sends und Effekte unter dem Pegelregler; rechts steht „NEUER BUS“.
2. **Bus anlegen:** Name „Hall“ eintippen, „+ Bus“. Es erscheint ein Zug „BUS“ mit eigenem Pegel, Pan und Pegelanzeige.
3. **Send hochziehen:** Bei Spur 1 „+ Send“ → Hall, dann den Wert ziehen (Doppelklick: 0 dB). „POST“/„PRE“ schaltet um, ob vor oder nach dem Fader abgegriffen wird; „✕“ entfernt den Send.
4. **EQ einfügen:** Bei Spur 1 „+ Effekt“ → „EQ“. Der Knopf schaltet den Effekt ein/aus (Bypass, ohne Knacken), „Regler“ klappt Hochpass und Glocke auf. Dasselbe geht mit Kompressor und Limiter, auch auf dem Bus.
5. **Strg+Z** nimmt den EQ zurück, „Wiederholen“ bringt ihn wieder; ebenso jeder Send, Ausgang und Reglerzug (je ein Schritt).
6. **Ausgang:** Bei Spur 1 „→ Master“ auf „→ Hall“ stellen. Ein Bus lässt sich nicht auf sich selbst oder im Kreis schalten (Meldung in der Statusleiste).
7. **Speichern, neu öffnen:** Bus, Sends, Effekte samt Reglern und Bypass sind noch da. Alte Projekte (Format 1) öffnen unverändert.
8. **Agent:** `insert_effect`, `set_param`, `set_insert_bypass`, `remove_insert`, `set_output`, `set_send`, `remove_send`; jeder Aufruf ist ein Undo-Schritt „Agent“.

Selbst geprüft von Claude: Modell mit Undo/Redo je Routing-Befehl, Kreiserkennung (Bus auf sich selbst, Bus im Kreis, Send im Kreis), Löschen eines angeschlossenen Busses samt Rückgängig, Projektdatei-Rundlauf, Insta-Schnappschuss des Formats 2 und Laden einer Datei im Format 1. Engine: Offline-Render mit Bus, Send (vor/nach Fader), Inserts auf Spur und Bus gegen die neue Referenzdatei `tests/golden/mixer_bus_send_05s.wav` (Abweichung unter 1e-5); der alte Referenz-Mix bleibt unverändert; Solo hält den gespeisten Bus hörbar; Live-Änderung von Send und Bypass ohne Sprung; jeder Block unter `assert_no_alloc`; Zyklus im Graphen wird abgelehnt. DSP: EQ, Kompressor, Limiter, Bypass-Überblendung und Stereo-Kopplung als Tests, EQ/Kompressor/Limiter von `radio` nach `dsp` verschoben (Radio-Tests unverändert grün). Agent-Werkzeuge über MCP einschließlich Fehlerfällen; Bedienlogik der Oberfläche als Test (Bus, Send, EQ, Rückgängig, Löschen). Nicht selbst geprüft: echter Klang (kein Ton auf dem Notebook, nur Offline-Render und Messwerte), die Bedienung im echten Fenster (Layout und Abstände der neuen Zugteile sind nur gebaut und kompiliert, nicht angeklickt), das Aussehen mit sehr vielen Spuren.

Noch nicht drin: Hall-Effekt (Paket 17), Sidechain, Reihenfolge der Effekte per Ziehen (neu eingefügte Effekte kommen ans Ende der Kette), Send-Pegel auf dem MIDI-Controller, Pegelanzeige hinter dem Fader für normale Spuren (die Anzeige zeigt wie bisher den Pegel vor dem Fader).

## Paket 15: Cue-Punkte und Vorhören (0.10.0, offen)

Aus M7 nach M4 vorgezogen (André, 30.10.2026: „ja, cue-punkte und pfl nach m4 vorziehen“).
Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. **Kopfhörer wählen:** Einstellungen → „Vorhören (PFL)“ → „Kanal 3/4 des Sendegeräts“ (Interface mit vier Ausgängen) oder ein zweites Gerät, etwa den USB-Kopfhörer. Hat das Gerät keine vier Kanäle, steht oben eine Meldung und das Vorhören bleibt aus.
2. **Cue-Punkte setzen:** Radio-Layout, Rechtsklick auf einen Titel → „Cue-Punkte …“. Oben den Marker wählen (Cue-In, Ramp, Hook, Fade-Out, Cue-Out), in die Wellenform klicken; Feinschliff in den Zahlenfeldern. „Übernehmen“. Die Dauer in der Playlist zeigt jetzt die gespielte Länge.
3. **Vorhören:** Im Editor „Vorhören ab …“ (ab dem gewählten Marker), oder Rechtsklick auf Titel/Cart → „Vorhören“ bzw. „Vorhören ab Hook“. Der Ton kommt nur im Kopfhörer; die Lautsprecher (Kanal 1/2) und der Stream bleiben unberührt, auch wenn gerade gesendet wird.
4. **Sendung:** Start. Der Titel beginnt bei Cue-In, oben läuft „RAMP n“ bis zum Intro-Ende herunter, die Restzeit zählt bis Cue-Out, ab dem Fade-Out-Marker blendet er in den nächsten Titel über.
5. **Rückgängig:** Im Editor „Rückgängig“ und „Wiederholen“; nach Neustart sind die Punkte noch da (stehen in `radio.json`).

Selbst geprüft von Claude: Prüfregeln der Cue-Punkte (Cue-In vor Cue-Out, alle anderen dazwischen, nichts hinter dem Titelende) als Tests; Offline-Render mit Cue-In, Cue-Out und Fade-Out-Marker ergibt genau die vorhergesagte Länge (6 s + 4 s − 1 s = 9,000 s), Vorhersage der Restzeit gleich; Undo/Redo der Cue-Bearbeitung; MCP-Werkzeug `set_cue_points` setzt, prüft, lehnt falsche Werte und Positionen ab, ohne `radio.json` zu ändern, und gibt die alten Punkte zurück. PFL gegen den echten Sendemixer: 20 Blöcke vorgehört, der Sendebus bleibt bitgenau still, der Kopfhörer hört den Titel; jeder Block von Mixer und PFL-Spieler unter `assert_no_alloc`. Kanalaufteilung als Test: PFL landet nur auf Kanal 3/4, bei einem Stereo-Gerät gar nicht. Screenshot per `WOA_SCREENSHOT` mit `WOA_CUE_EDITOR=1`: Editor mit Wellenform und fünf Markern. Nicht selbst geprüft: echter Ton im Kopfhörer und an einem Vier-Kanal-Interface (keine Wiedergabe auf dem Notebook), die Rechtsklick-Menüs im echten Fenster, die Ramp-Anzeige während einer laufenden Sendung.

Noch nicht drin: Cue-Punkte automatisch aus der Analyse (kommt mit der Bibliothek in M7), Vorhören-Pegel, Scrubbing in der Wellenform, Cue-Punkte für Carts. Ein Wechsel des PFL-Ausgangs während einer Sendung gilt erst ab dem nächsten Öffnen des Radio-Layouts. Ändert der Agent Cue-Punkte, nimmt er sie mit einem zweiten Aufruf zurück (die alten Werte stehen in der Antwort); in der Oberfläche landen sie nicht im Rückgängig-Verlauf.

## Paket 14: Controller und MIDI-Learn (0.9.0, offen)

Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. **Controller einstecken:** Einstellungen → „Controller und MIDI-Learn …“ → Gerät wählen. Die Zeile darunter sagt „Verbunden“ (mit LEDs, wenn das Gerät sie kann).
2. **Fader und zwei Pads lernen:** Radio-Layout. Rechtsklick auf „Pegel Musik“ → „MIDI lernen“, einen Fader bewegen. Rechtsklick auf eine belegte Cart-Taste (Jingle) → „MIDI lernen“, Pad drücken; dasselbe mit „▶ Start“ und einem zweiten Pad. Im Controller-Fenster stehen die drei Zeilen.
3. **Musik-Fader fahren:** Sendung mit dem Start-Pad starten, Fader fahren. Der Regler springt nicht, er greift, sobald er die Stellung auf dem Bildschirm erreicht; danach folgt die Musik weich.
4. **Jingle per Pad:** Pad drücken, der Jingle läuft über der Musik, die Pad-LED leuchtet (falls das Gerät LEDs hat). Nochmal drücken hält ihn an.
5. **Neustart:** WOA schließen und starten, Controller-Fenster öffnen: Die Zuordnung ist noch da, Fader und Pads gehen sofort.

Selbst geprüft von Claude: Zuordnung und Lernen als Tests (nächste Bewegung wird gebunden, Pad passt nicht an einen Fader, gleiche Quelle oder gleiches Ziel ersetzt die alte Zeile, Löschen, Kanal zählt mit), Taster nur auf der steigenden Flanke, Takeover von unten, von oben, durch Erreichen und nach Mausänderung neu, JSON-Roundtrip der Zuordnung. Virtueller ALSA-Port als Controller gegen den echten Sendemixer: drei Ziele gelernt und in der Konfiguration, Start-Pad startet die Musik, Fader setzt nach dem Takeover den Musik-Pegel (Ausgang −5,9 dB wie verlangt, 0,449 statt 0,891), Cart-Pad ruft `trigger_cart` (Zähler 1, Cart-Bit im Mixer gesetzt, Pegel steigt), zweites Drücken hält ihn an; jeder Mixer-Block unter `assert_no_alloc`. Pegelregler im Mixer: −6 dB landet bei der Hälfte, größter Sprung je Sample unter 0,002 (geglättet), Carts auf aus und wieder an. Lasttest über den virtuellen Port: 2000 CC in 2 s (1000 CC/s), alle angekommen, keiner verworfen, je Bild höchstens ein Fader-Wert, längstes Abholen 0,09 ms. LEDs über einen virtuellen Eingang: Note On 127 und 0 kommen an, ohne Änderung wird nichts gesendet. Neustart: `config.json` speichern und laden ergibt dieselbe Zuordnung, auch bei neuer ALSA-Nummer des Geräts. Screenshot per `WOA_SCREENSHOT` mit `WOA_CONTROLLER=1`: Controller-Fenster mit neun Zuordnungen. Nicht selbst geprüft: ein echtes Gerät, die echten LEDs, der Klang und das Gefühl beim Fahren (keine Wiedergabe auf dem Notebook), die Rechtsklick-Menüs im echten Fenster (der Fotomodus klickt nicht).

Noch nicht drin: Pegelregler werden nicht gespeichert (Start bei 0 dB), Spur-Fader wirkt wie die Maus erst nach dem Loslassen bzw. 300 ms Ruhe hörbar, Endlos-Drehregler (relative CC), Motorfader und Lernen von Pitchbend-Fadern (Mackie-Protokoll). Welcher Controller abgenommen wird, ist noch offen (Frage 2 in #5).

## Paket 13: Piano-Roll (0.8.0, offen)

Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. **Öffnen:** Eine aufgenommene Phrase (Paket 12) oder eine importierte `.mid` doppelklicken. Die Piano-Roll zeigt die Noten mit Klaviatur links und Velocity-Leiste unten.
2. **Korrigieren:** Eine falsche Note auf die richtige Tonhöhe ziehen, eine zu kurze am rechten Rand länger ziehen, eine überflüssige doppelklicken (weg). Ins Leere klicken: neue Note. Strg+Z nimmt jeden Schritt einzeln zurück.
3. **Quantisieren:** Raster 1/16 wählen, nichts auswählen, „Alle quantisieren“ (oder Q). Die Noten springen auf das Raster. Nur ein paar Noten: mit Rahmen wählen, dann Q.
4. **Velocity ändern:** Ein paar Noten wählen, in der Velocity-Leiste nach oben oder unten ziehen.
5. **Anhören:** Fenster schließen (Esc), Kopf an den Anfang, ▶: die Phrase klingt korrigiert und im Takt. Klick auf die Klaviatur hört einzelne Töne vor. Fühlt sich die Bedienung wie in einer DAW an?

Selbst geprüft von Claude: Bearbeitungsbefehle als Tests (Raster 1/4 bis 1/32 und Triolen, Quantisieren aller oder gewählter Noten, mit und ohne Enden; Verschieben als Ganzes, ohne vor 0 oder über Tonhöhe 127 zu rutschen; Auswahl folgt der neuen Reihenfolge; Länge, Löschen, Velocity). Sechs Bearbeitungen hintereinander, jede ein eigener Undo- und Redo-Schritt, Pedal bleibt unverändert. Offline-Render vor und nach dem Quantisieren: vier Noten liegen vorher 20 bis 60 ms neben dem Viertel, nachher höchstens 1 ms (48 Samples) daneben. Agent über MCP: `quantize_notes` (nur eine Note, dann alle), `edit_notes` (Velocity, Löschen), Fehlerfälle ändern nichts, Undo Schritt für Schritt. Screenshots per `WOA_SCREENSHOT` mit `WOA_PIANOROLL=1` an einem über MCP angelegten Testclip, dunkel und hell (`WOA_THEME=hell`): Raster, Klaviatur, zwei gewählte Noten in Orange, Velocity-Leiste. Nicht selbst geprüft: Maus-Bedienung im echten Fenster (der Fotomodus klickt nicht), Vorhören und der Klang (keine Wiedergabe auf dem Notebook).

Noch nicht drin: Noten als Vorschau im Clip auf der Zeitleiste, Pedal (CC 64) in der Piano-Roll bearbeiten, Kopieren/Einfügen von Noten, helles Thema für die ganze App.

## Paket 12: MIDI aufnehmen und als Noten speichern (0.7.0, offen)

Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. **Phrase einspielen:** Yamaha gewählt (Paket 11), MIDI-Spur scharf schalten, Aufnahme drücken, ein paar Takte mit Pedal spielen, Aufnahme stoppen. Auf der Spur liegt ein Clip „MIDI-Take“.
2. **Abspielen:** Kopf an den Anfang, ▶. Die Phrase klingt wie gespielt, im Takt, mit Pedal. Mitten drin springen oder stoppen: kein Ton hängt.
3. **Speichern:** Strg+S.
4. **Neu öffnen:** WOA schließen, wieder starten, Projekt öffnen, ▶: dieselbe Phrase. Strg+Z nimmt den Take zurück, Strg+Y bringt ihn wieder.
5. **WAV exportieren:** Datei → Exportieren als WAV. Die Datei in einem Player anhören: die Phrase ist drin.
6. **Fremde .mid importieren:** Datei → „MIDI importieren (.mid) …“, eine `.mid` aus dem Netz oder einem anderen Programm wählen. Clip erscheint am Kopf, ▶ spielt sie. Gegenrichtung: Clip auswählen, „MIDI-Clip exportieren (.mid) …“, Datei in einem anderen Programm öffnen.

Selbst geprüft von Claude: Modell-Roundtrip (JSON hin und zurück), `SetNotes` mit Undo/Redo, ungültige Noten werden abgelehnt. Aufnahme über einen virtuellen ALSA-Port mit festem Muster (vier Noten, Pedal an/aus) gegen eine Sample-Uhr wie vom Audiogerät: alle Events kommen an, Anschlag stimmt, größte Abweichung 0,2 ms (Grenze 1 ms). Wiedergabe unter `assert_no_alloc`: Einsatz höchstens 2 Samples daneben; Offline-Export als WAV mit Einsatzprüfung; Stopp und Springen ohne hängende Note. `.mid` schreiben und lesen gibt dieselben Noten; eine fremde Datei (Format 1, Tempospur, Running Status, Note On mit Velocity 0, SysEx) wird richtig gelesen. Diese Datei ist von Hand nach der SMF-Spezifikation geschrieben, nicht von einem anderen Programm erzeugt (keins auf dem Prüfrechner). Agent-Loop über MCP: Noten als MIDI-Clip schreiben, mit `read_notes` lesen, exportieren (Einsatz an der richtigen Stelle), Fehlerfälle, Undo. Nicht selbst geprüft: echter Ton, die Yamaha, die Dateidialoge und das Gefühl beim Einspielen (keine Wiedergabe auf dem Notebook).

Noch nicht drin: Noten in der Zeitleiste zeichnen und mit der Maus bearbeiten (der Clip zeigt nur seinen Namen), Quantisieren, Instrumentwahl je Spur.

## Paket 11: Keyboard spielt live (0.6.0, offen)

Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. Yamaha per USB einstecken. Einstellungen → MIDI-Eingang → die Yamaha wählen. Die Lampe „MIDI“ rechts oben wird grau, beim Spielen leuchtet sie grün.
2. „+ MIDI-Spur“ klicken und die Spur scharf schalten (oder nur auswählen). Tasten spielen: Ton kommt sofort? Latenz gefühlt ok?
3. Leise und fest anschlagen: Anschlagstärke hörbar?
4. Sustain-Pedal treten, Akkord anschlagen, loslassen: Ton hält, bis das Pedal los ist.
5. USB-Kabel ziehen: Lampe wird rot, kein Ton hängt. Wieder stecken: nach höchstens 2 s grau, Spielen geht wieder.

Selbst geprüft von Claude: Parser-Tests mit Bytefolgen (Running Status, Note On mit Velocity 0, Echtzeit-Bytes, SysEx, Pedal, Pitchbend, Program Change). Virtueller ALSA-Port (`midir` `create_virtual`) als Keyboard: Events kommen in der Queue an; Offline-Render der Instrument-Node unter `assert_no_alloc` ist nicht still, die laute Note ist mehr als dreimal so laut wie die leise, das Pedal hält und gibt frei; Port schließen → Status „wartet“, keine hängende Note; Port wieder öffnen → verbunden, Events kommen an. Live-Instrument im Projekt-Graphen klingt ohne ▶. Screenshot per `WOA_SCREENSHOT`: Kopfzeile mit MIDI-Lampe (rot, gewähltes Gerät fehlt) und Knopf „+ MIDI-Spur“. Nicht selbst geprüft: das geöffnete Menü MIDI-Eingang (der Fotomodus kann keine Menüs öffnen), echter Ton und Latenz (keine Wiedergabe auf dem Notebook), die Yamaha selbst.

Noch nicht drin: MIDI aufnehmen und speichern (Paket 12), Instrumentwahl je Spur (fest: Synthesizer), Haltepedal im Sampler.

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

## Abgenommen

- Paket 10 Dauertest (0.5.0): 30.10.2026, André „2 abgenommen“ (Sammelabnahme Pakete 5, 9, 10 und Nachtrag #43).

- Paket 9 Sendebus-Processing (0.4.0): 30.10.2026, Sammelabnahme.

- Nachtrag #43 Wellenform während der Aufnahme (0.3.5): 30.10.2026, Sammelabnahme.

- Paket 5 Quellen (0.1.11, mit #44 in 0.3.6): 30.10.2026, Sammelabnahme.

- Paket 8 Sendeprozess (0.3.0, mit 0.3.4): 29.10.2026, André „stream geht, abnahme paket 8 fertig!“

- Paket 7 Sendeablauf (0.2.0): 29.10.2026, André „abnahme paket 7 fertig“.

- Paket 4 Radio-tauglich (0.1.9): 29.10.2026, André „funktioniert alles“.

- Paket 3 Arrangieren (0.1.8): 29.10.2026, André „passt“.

- Paket 2 Bearbeiten und Export (0.1.7): 29.10.2026, André „passt“.

- Paket 1 Aufnehmen und Abspielen (0.1.6): 29.10.2026, André „passt“.

- M1 Engine: 29.10.2026, 60 s Testton sauber, `underruns=0` (#2).
