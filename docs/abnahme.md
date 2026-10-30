# Abnahme in Paketen

André nimmt fertige Pakete ab, nicht einzelne Funktionen (29.10.2026). Claude baut ein Paket am
Stück, prüft selbst (Tests, Offline-Render, Probelauf am Gerät) und merged. Hier steht je Paket
eine kurze Klickliste. Ein „passt“ von André schließt das Paket; was hakt, wird ein Issue.

## Paket 22: Aufnahme mit Latenzausgleich, Punch-in, Loop-Takes, Comping (0.17.0, offen)

Aus M6. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. **Spur scharf schalten:** Auf der Audio-Spur den roten Punkt links anklicken, Eingang wählen. Mit Aufnahme-Taste starten und stoppen: der neue Clip liegt an der Stelle des Abspielkopfes beim Start, nicht dahinter. Ein Klatschen oder Klick sitzt auf dem Raster, wo du ihn gespielt hast (die Latenz des Geräts ist abgezogen).
2. **Punch-in:** „Punch“ einschalten, Takt von und bis eintragen (Standard 2 bis 4). Aufnehmen: vom Take bleibt nur der Teil zwischen den beiden Takten, der Rest fehlt.
3. **3 Loop-Takes:** Schleife über vier Takte setzen und einschalten, aufnehmen und drei Runden lang spielen, dann stoppen. Auf der Spur erscheint ein Take-Ordner „Loop 1 · 3 Takes“, eine Runde je Take; eine angefangene vierte Runde wird verworfen.
4. **Aufklappen:** Auf den Pfeil im Ordner klicken: eine Zeile je Take mit Wellenform. Der Streifen darüber zeigt, welcher Take wo spielt (zuerst überall der neueste).
5. **Abschnitte wählen:** In der Zeile eines Takes einen Bereich ziehen (rastet am Raster ein) oder einmal klicken (wählt den Takt): dieser Take spielt dort. Der Streifen und die Spur ändern sich sofort. Strg+Z nimmt jeden Schritt einzeln zurück.
6. **Abspielen:** Leertaste: der Comp spielt die gewählten Teile hintereinander, ohne Lücke an den Grenzen. Papierkorb am Ordner löscht Ordner samt Comp (Strg+Z holt ihn zurück).
7. **Speichern, neu öffnen:** Strg+S, schließen, öffnen: Ordner, Takes und Auswahl stehen wie zuvor.

Selbst geprüft von Claude (ohne Ton und ohne Mikrofon): Fake-Eingang mit künstlicher Latenz (20 ms): der Impuls liegt nach dem Ausgleich auf dem Sample, wo er gespielt wurde, ohne Ausgleich genau die Latenz zu spät; Punch behält nur den Impuls im Bereich, auf dem Sample genau; Runden am Null-Ausgang gezählt (Schleifenüberläufe) gleich Zahl der Takes; Comp-Render (Teile aus Take 1 und 3, Mitte aus Take 2, Lücke bleibt still, Undo stellt den Render zurück); Modell-Befehl `SetTakeFolder` mit Undo und Redo, Prüfung schlechter Eingaben (Take außerhalb, Pfad mit `..`, Bereich außerhalb) ändert nichts; Projektformat 3 nur mit Take-Ordner, Rundlauf; Zustandstests der Oberfläche (Aufnahme mit Latenz und Punch, Ordner, Comping, Undo); alles unter `assert_no_alloc`, wo die Engine läuft.

Nicht selbst geprüft: echte Aufnahme am Gerät (kein Mikrofon auf dem Notebook); die Latenz des Geräts selbst (die vom Treiber gemeldete Zeit plus der einstellbare Zuschlag, nicht am Kabel gemessen); Aussehen und Griffigkeit der Take-Zeilen im echten Fenster.

Noch nicht drin: Messung der Gerätelatenz per Kabelschleife (braucht Andrés Hardware am Schreibtisch), Ausschnitt-Ziehen über mehrere Spuren, Überblendung an den Comp-Grenzen (sie liegen stumpf aneinander).

## Paket 21: Schnitt und Fades (0.16.0, offen)

Aus M6. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. **Teilen:** Clip anklicken, Abspielkopf ins Lineal setzen, in der Clip-Leiste „Schneiden (S)“ (oder Taste S). Der Clip wird an der Stelle zu zwei Clips; beim Abspielen hört man keine Naht. Strg+Z macht es rückgängig.
2. **Mitte löschen:** Zweimal schneiden, den mittleren Teil anklicken und Entf drücken. Die Lücke bleibt, die Ränder sind unverändert.
3. **Fade ziehen:** Den Griff am oberen Clip-Rand nach innen ziehen: die Ein- oder Ausblendung wächst und wird in der Wellenform gezeichnet. In der Clip-Leiste Kurve wählen (Linear, Gleiche Leistung, S-Kurve), die Zeichnung ändert sich mit.
4. **Überlappen:** Einen Clip ein Stück über den Nachbarn auf derselben Spur schieben. Die Überlappung zeigt die automatische Überblendung (Gleiche Leistung), beim Abspielen gibt es keinen Pegeleinbruch und keinen Klick.
5. **Normalisieren und Clip-Lautstärke:** Clip wählen, „Normalisieren“: die Welle wächst, bis die Spitze bei −1 dBFS liegt. Mit dem dB-Regler lässt sich der Wert von Hand ändern; Strg+Z stellt zurück.
6. **Export prüfen:** Projekt exportieren (Strg+E, WAV), Datei in einem Editor oder Player öffnen: Schnitte und Überblendungen klingen wie in der Vorschau, kein Knacken an den Schnittstellen.
7. **Speichern, neu öffnen:** Strg+S, schließen, wieder öffnen: Kurven, Blenden und Clip-Lautstärken stehen wie zuvor.

Selbst geprüft von Claude: Nulltest (geteilter und wieder zusammengesetzter Clip ist bitgleich mit dem Original bei Positionen auf ganzen Samples; bei krummen Tempi höchstens ein Sample Versatz, mit Sprungerkennung geprüft); Tabellentests der drei Kurven (Werte, Monotonie, Summe bzw. Leistung 1); RMS bei Überblendung (Gleiche Leistung hält den Pegel, linear sinkt er in der Mitte); Klick-Erkennung an Blenden und Überblendung; Clip-Gain gegen die dB-Rechnung; Echtzeit-Test mit `assert_no_alloc`; Modell-Befehle mit Undo und Redo, Prüfung der Grenzen, Projektformat 2 bleibt ohne neue Felder, Rundlauf Speichern und Laden; Agent-Werkzeuge über MCP; Oberflächentest ohne Fenster und ohne Ton (Fade-Griff ziehen, Strg+Z); Schneiden, Mitte löschen, Kurven, Clip-Lautstärke, Normalisieren und Rundlauf Speichern und Laden als Zustandstests (`AppState`, ohne Zeiger- und Tastenereignisse). Alle Graphen (Wiedergabe, Analyse, Export) entstehen über `clip_graph_inner`, dort greifen Kurven, Clip-Lautstärke und Überblendung.

Nicht selbst geprüft: Klang (kein Ton auf dem Notebook, nur Messung im Offline-Render), Aussehen und Griffigkeit der Fade-Griffe im echten Fenster (mit simulierten Zeigerereignissen getestet, nicht von Hand).

Noch nicht drin (Paket 23): Time-Stretch.

## Paket 20: Timeline-Editor (0.15.0, offen)

Aus M6. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. **Zwei WAVs reinziehen:** Im Produzent-Modus zwei Audiodateien aus dem Dateimanager auf eine Spur ziehen. Die erste landet an der Zeigerstelle auf dieser Spur (eingerastet), die zweite auf einer neuen Spur an derselben Stelle. Wellenformen erscheinen im Clip.
2. **Raster:** Oben rechts „Raster“ auf „Takt“ stellen, einen Clip verschieben: er springt in ganzen Takten. Auf „Aus“ geht es stufenlos.
3. **Verschieben und kürzen:** Clip an der Mitte ziehen (auch auf eine andere Spur); an den Rändern ziehen kürzt, der Ton bleibt dabei an seiner Stelle.
4. **Kopieren:** Clip anklicken, Strg+C, Abspielkopf im Lineal setzen, Strg+V. Strg+D verdoppelt direkt hinter dem Clip. Strg+X schneidet aus.
5. **Lineal:** Schalter „Takte/Sekunden“ neben dem Raster; Strg+Mausrad zoomt, die Beschriftung wird dichter oder lichter.
6. **Strg+Z / Strg+Y:** jeder Schritt oben lässt sich zurücknehmen und wiederholen.
7. **Speichern, neu öffnen:** Strg+S, Anwendung schließen, Projekt wieder öffnen: alle Clips liegen wie zuvor.

Selbst geprüft von Claude: Modelltests der Befehle (Verschieben, Trimmen am Anfang und Ende, Spurwechsel, Kopieren, Einfügen, Verdoppeln, Spurart wird beachtet) mit Undo und Redo; Rundlauf Speichern und Laden (Projekt gleich, Lage gleich); Oberflächentest ohne Fenster und ohne Ton, der mit echten Zeiger- und Tastenereignissen durch `WoaApp` läuft (zwei Dateien ablegen, Clip ziehen, Rand ziehen, Strg+Z, Strg+Y, Strg+D, Speichern, neu öffnen); Offline-Render mit Impuls in der Datei: Verschieben um 4 Beats versetzt den Impuls um genau 96 000 Samples, Kürzen am Anfang lässt ihn auf dem Sample stehen, Undo stellt ihn wieder her; Lineal-Teilung und Raster als Tabellentests.

Nicht selbst geprüft: das Aussehen und die Griffigkeit im echten Fenster (Ziehen mit der Maus, Ablegen aus dem Dateimanager des Desktops, Strg+C/V über die Zwischenablage des Systems sind mit simulierten Ereignissen getestet, nicht von Hand), Klang (kein Ton auf dem Notebook).

Noch nicht drin (Paket 21 bis 23): Teilen mit Kurvenwahl, Crossfades, Normalisieren, Aufnahme mit Latenzausgleich und Comping, Time-Stretch.

## Paket 19: Analyse-Fenster (0.14.0, offen)

Aus M5. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. **Analyse öffnen:** Im unteren Dock auf „📊 Analyse“. Ein Fenster mit Lautheitsanzeige, Spektrum, Spektrogramm, Scope und Korrelationsleiste erscheint. Noch ohne Wiedergabe stehen die Werte auf `--.-`.
2. **Abspielen:** Leertaste mit einem Clip. Momentan, Kurzzeit und Integriert laufen, das Spektrum tanzt, das Spektrogramm wandert nach rechts, True Peak wird rot über −1 dBTP.
3. **Quelle wechseln:** Oben „Quelle“ von Master auf eine Spur; die Messung beginnt neu. „Zurücksetzen“ leert die integrierte Lautheit und die Halte-Marken.
4. **Scope bei breitem Pan:** Zwei Spuren, eine hart links, eine hart rechts: der Scope breitet sich in die Breite, die Korrelation fällt Richtung 0. Eine Mono-Spur zeigt eine senkrechte Linie und +1.
5. **Fenster schließen:** Das Kreuz oben rechts; danach ist die Messung aus.
6. **Agent:** `analyze` (jetzt mit True Peak, Spektrum, Korrelation, `track`) und `spectrogram_png`.

Selbst geprüft von Claude: Gemessen statt gehört. EBU-Referenzsignale: 1 kHz bei −23 und −33 dBFS ergibt −23 und −33 LUFS (momentan, kurzzeitig, integriert, Maximum, je ±0,1), ein Kanal allein ist 3 LU leiser, absolutes und relatives Gate wirken, auch bei 44,1 kHz und in krummen Stücken. True Peak: Sinus mit 45° Phase bei fs/4 liegt nach Überabtastung innerhalb 0,25 dB bei 0 dBTP, 997 Hz mit 0,5 gibt −6,02 dBTP. Spektrum: Sinus auf den Bins 10, 93, 512 und 1500 trifft den Bin mit 0 dB (±0,1), halbe Amplitude −6,02 dB, Stille und kurze Eingabe sind sicher; Oktavband und Log-Bänder zeigen den Ton an der richtigen Stelle. Korrelation +1 bei Mono, −1 bei Gegenphase, etwa 0 bei Rauschen. Spektrogramm als Text-Snapshot (`insta`) für drei Töne und als PNG über den Agenten (hellste Zeile stimmt auf die Logachse). Abgriffe lassen das Signal bitgleich, folgen dem Pan jeder Spur, bleiben leer wenn aus, und jeder Process-Aufruf läuft unter `assert_no_alloc`. Die Rechenlogik des Fensters (Quelle, Zurücksetzen, Ratenwechsel) ist als Test abgedeckt. Agent über MCP einschließlich Fehlerfällen.

Nicht selbst geprüft: echter Klang und echte Musik (kein Ton auf dem Notebook), die Bedienung im echten Fenster (Layout, Lesbarkeit, Flüssigkeit der Anzeigen sind gebaut und kompiliert, nicht angeklickt), die Belastung bei sehr vielen Spuren.

Noch nicht drin: Loudness-Range (LRA), Anzeige im Radio-Modus, Fenstergröße und Quelle über den Neustart merken, Spektrogramm als Datei aus der Oberfläche speichern (nur über den Agenten), Referenzkurve zum Vergleichen.

## Paket 18: Automation und Sidechain (0.13.0, offen)

Aus M5. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. **Lautstärke-Kurve zeichnen:** Studio-Dock → Reiter „〰 Automation“. Eine Spur anklicken, oben „Lautstärke“ ist vorgewählt. In die Fläche klicken setzt Punkte (zum Beispiel Beat 0 bei 0 dB, Beat 8 bei −18 dB, Beat 16 bei 0 dB). Punkt ziehen verschiebt ihn (rastet auf Viertel-Beats), Rechtsklick auf einen Punkt löscht ihn. Strg+Z nimmt jede Änderung als einen Schritt zurück.
2. **Abspielen:** Leertaste. Der Lautstärke-Regler der Spur wandert mit der Kurve; „am Kopf: … dB“ im Editor zeigt den Wert. Im Editor läuft der Kopf als blaue Linie mit.
3. **Andere Ziele:** In der Auswahl oben Pan, einen Send oder einen Effekt-Regler wählen (zum Beispiel „Kompressor · Schwelle“) und ebenso zeichnen. Auswahl-Regler (zum Beispiel „Takt“ beim Echo) halten ihren Wert bis zum nächsten Punkt.
4. **Sidechain (Ducking):** Zwei Spuren: Musik und Sprache. Bei der Musik „+ Effekt“ → Kompressor, darunter „Sidechain“ → „Sprache“. Spielt die Sprache, sinkt die Musik; der Balken „Absenkung“ schlägt aus. Auch Gate hat „Sidechain“. Auf einem Bus geht es nicht („nur Spuren“).
5. **Aufnehmen mit dem Controller:** Fader an „Pegel der Spur“ per Rechtsklick → „MIDI lernen“ binden (wie in Paket 14). Im Editor „Schreiben“ wählen, Wiedergabe starten, Fader fahren, Stopp: Die Fahrt steht als Kurve da, die Betriebsart springt auf „Lesen“, Strg+Z nimmt alles in einem Schritt zurück. „Latch“ wartet auf die erste Berührung und läuft bis zum Stopp.
6. **Speichern, neu öffnen:** Kurven und Sidechains sind noch da. Projekte ohne Automation bleiben im alten Format und öffnen auch in 0.12.
7. **Agent:** `add_automation`, `remove_automation`, `set_sidechain`.

Selbst geprüft von Claude: Lautstärke-Automation mit drei Punkten: Rampe Sample für Sample gegen die dB-Rechnung, identisch bei Blockgröße 1, 128 und 1000; abspielen im Offline-Render, ohne Kurve klingt das Projekt bitgleich wie vorher (alte Referenzdateien unverändert). Pan-Fahrt, Send-Kurve, Effekt-Regler-Kurve (+12 dB), Loop-Sprung, Wert am Cursor bei Stopp. Kompressor mit Sidechain Sprache: Die Musik wird gemessen mehr als 6 dB leiser, solange die Sprache spricht, und kommt danach zurück; der Absenkungsmesser (Gain-Reduction) schlägt aus; ohne Sidechain bleibt die Musik laut. Alles unter `assert_no_alloc`. Modell: Undo und Redo, Kreiserkennung über Sidechains, Spur löschen trennt Sidechains, Format 3 nur bei Bedarf (Insta-Schnappschuss, alte Dateien laden). Controller-Aufnahme über einen virtuellen ALSA-Port: gelernter Fader, Fahrt wird zur Kurve, Offline-Render folgt der Fahrt, ein Undo-Schritt. Während der Aufnahme setzt die Fahrt weder den festen Reglerstand noch einen Undo-Schritt (Test gegen die Controller-Oberfläche der App). Aufnehmer als reine Logik getestet (Schreiben, Latch, Ersetzen nur der aufgenommenen Strecke, gehaltener Wert bleibt flach, Punktgrenze 4096, Loop-Rücksprung). Agent über MCP einschließlich Fehlerfällen. Es wurde nichts abgespielt, kein Gerät und kein Mikrofon benutzt.

Nicht selbst geprüft: echter Klang (kein Ton auf dem Notebook, nur Messwerte), Aussehen und Bedienung des Kurven-Editors und der Sidechain-Auswahl im echten Fenster (gebaut und kompiliert, nicht angeklickt), ein echter Controller, das Gefühl beim Aufnehmen mit dem Fader.

Noch nicht drin: Sidechain für Effekte auf Bussen; der Schlüssel wird nach dem Fader und vor dem Pan der Quelle abgegriffen (keine Wahl vor Fader oder nach Effekten, kein Sidechain-Filter und kein „Schlüssel hören“). Aufnehmen mit dem Controller gibt es für Lautstärke und Pan; Sends und Effekt-Regler lassen sich zeichnen oder per Agent setzen, aber nicht live einfahren, und Kurven im Modus Schreiben oder Latch spielen nicht ab, bis die Aufnahme beendet ist. Der Fader zeigt die Kurve beim Abspielen, folgt ihr im Pan-Regler noch nicht. Kein Kopieren und Verschieben mehrerer Punkte, keine Kurvenform zwischen Punkten außer linear (dB-linear bei Lautstärke und Send). Effekt-Kurven laufen in Schritten zu 64 Samples. Jede Änderung einer Kurve baut den Graphen neu (hörbar nur als kurzer Wechsel, nicht als Knacken).

## Paket 17: Alle eingebauten Effekte (0.12.0, offen)

Aus M5. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. **Vorlage Sprachkette:** Im Mixer bei einer Spur „+ Effekt“ → ganz unten „Vorlage Sprachkette“. Es erscheinen fünf Effekte: EQ (Hochpass 80 Hz), Gate, EQ, Kompressor, De-Esser. Strg+Z nimmt alle fünf in einem Schritt zurück.
2. **Jeden Effekt an/aus:** Im Menü „+ Effekt“ stehen jetzt zehn Arten (EQ, Kompressor, Limiter, Gate, De-Esser, Hall, Echo, Chorus, Sättigung, Rauschunterdrückung). Jeden einmal einfügen, den Namensknopf klicken: ein/aus ohne Knacken.
3. **Regler bewegen:** „Regler“ aufklappen und ziehen (Doppelklick: Vorgabe). Beim Echo wählt „Takt“ einen Notenwert passend zum Projekttempo; „Ping-Pong“ und „Kennlinie“ der Sättigung sind Auswahllisten.
4. **Preset wechseln:** In „Regler“ oben „Preset ▾“; ein Klick setzt alle Regler des Effekts, Strg+Z stellt sie in einem Schritt zurück.
5. **Rauschunterdrückung:** wirkt nur bei 48 kHz (sonst läuft das Signal unverändert durch) und verzögert um 30 ms; der trockene Anteil wird um dasselbe verzögert.
6. **Speichern, neu öffnen:** Effekte, Regler und Bypass sind noch da; alte Projekte mit EQ, Kompressor und Limiter klingen unverändert (der EQ hat neue Bänder, alle flach).
7. **Agent:** `list_effects`, `insert_effect` (mit `preset`), `insert_voice_chain`.

Selbst geprüft von Claude: Gemessen statt gehört, mit erzeugten Testsignalen. EQ: Tiefen-/Höhenregal und zweite Glocke erreichen ihre Verstärkung (Goertzel), flacher EQ ändert nichts. Gate: offen unverändert, geschlossen um die eingestellte Absenkung leiser, Haltezeit wirkt. De-Esser: 7-kHz-Zischen fällt, 500-Hz-Stimme bleibt, die größte Absenkung wird eingehalten. Hall: Nachhallzeit (Schroeder-Integral) liegt für 0,8, 1,5 und 3 s innerhalb 25 %, Dämpfung verkürzt, Vorverzögerung stimmt auf das Sample, Mix 0 ist unverändert, Pegel für weißes Rauschen etwa gleich. Echo: Zeit frei und zum Tempo (1/4, 1/8 punktiert, 1/16, auch bei 90 BPM) auf das Sample genau, Rückkopplung 50 % halbiert jedes Echo, Ping-Pong wechselt die Seite, Zeitänderung ohne Sprung (die Tonhöhe zieht dabei kurz wie beim Bandecho). Chorus: ohne Tiefe ein reines Delay, mit Tiefe hörbar moduliert bei gleichem Pegel, beide Kanäle verschieden. Sättigung: dritter Oberton wächst mit der Verstärkung, weiche Kennlinie ohne, Röhre mit zweitem Oberton, weich und hart bleiben unter Vollaussteuerung, die Röhre ohne Gleichanteil (Gleichstromfilter nur dort) und mit höchstens 10 % Überschwingen bei extremer Verstärkung. Rauschunterdrückung: Rauschen neben einem Ton fällt bei 6, 10 und 15 kHz um mehr als 12 dB, der Ton bleibt (reines weißes Rauschen fällt nur um etwa 3 dB; das ist Verhalten von RNNoise selbst, gegen die Bibliothek ohne unseren Code gegengemessen); Nass- und Trockenanteil liegen auf demselben Sample (Gesamtverzögerung 1440 Samples = 30 ms, an den Kanten eines Tonstoßes gemessen); Live-Betrieb (Arbeits-Thread) und Export rechnen dasselbe Ergebnis, ohne Aussetzer. Für alle zehn Effekte: Bypass ist bitgenau, jeder Process-Aufruf (auch beim Reglerbewegen und Umschalten) läuft unter `assert_no_alloc`, im Graphen mit jedem Preset ebenso. Golden-File `tests/golden/effects_p17_chain.wav` (Abweichung unter 1e-3). Sprachkette im Graphen: Brummen (30 Hz) fällt auf unter ein Viertel, 1 kHz bleibt. Synchrones Echo folgt dem Projekttempo (120 BPM: 24000 Samples, 60 BPM: 48000). Agent über MCP einschließlich Fehlerfällen; Bedienlogik der Oberfläche (Preset, Sprachkette, je ein Undo-Schritt) als Test. Beim Messen aufgefallen und behoben: Die Allpässe des Halls hatten Verstärkung über 1 (Hall rund 11 dB zu laut), die Sättigung wurde bei der Röhrenkennlinie falsch normiert und konnte über Vollaussteuerung gehen.

Nicht selbst geprüft: echter Klang (kein Ton auf dem Notebook, nur Messwerte), ob die Rauschunterdrückung bei echter Sprache gut klingt (gemessen mit Sinus und Rauschen, nicht mit einer Stimme), die Bedienung im echten Fenster (Preset-Menü und Auswahllisten sind gebaut und kompiliert, nicht angeklickt).

Noch nicht drin: Latenzausgleich zwischen Spuren (die Rauschunterdrückung verzögert um 30 ms, `latency_samples` ist da, der Ausgleich im Graphen kommt später), Sidechain, eigene Preset-Namen speichern, Effekte per Ziehen umsortieren.

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
