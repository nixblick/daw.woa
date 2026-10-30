# Changelog

## 0.28.0 — 2026-10-30

Paket 33 aus M8: Absturzschutz per eigenem Prozess (Abnahme: `docs/abnahme.md`).

- **Plugin-Prozess:** Neues Programm `woa-plugin-host` (und derselbe Code über `woa --woa-plugin-host`) lädt ein CLAP-Plugin abseits der Oberfläche. Stürzt das Plugin ab oder hängt es, trifft es nur diesen Prozess.
- **Austausch:** Gemeinsamer Speicher (eine Datei, die beide abbilden) mit Atomen, dazu ein Briefkasten für Aufrufe (Info, Zustand, Fenster) und lock-freie Ringe für Reglerwerte und Rückmeldungen. Die Idee folgt dem RemotePlugin von LMMS; Code ist nicht übernommen (`docs/herkunft.md`).
- **Takt:** Jeder Block wird im Plugin-Prozess gerechnet, ohne zusätzliche Latenz. Kommt die Antwort nicht rechtzeitig (Echtzeit: etwa zwei Drittel der Blockdauer, mindestens 1 ms, höchstens 50 ms), gibt der Block Stille aus und zählt einen verspäteten Block. Im Audio-Thread wartet nur eine begrenzte Schleife; sie ist im Quelltext benannt.
- **Absturz und Hänger:** Ein beendeter Prozess oder ein Block, der länger als 1,5 s nicht fertig wird (dann wird der Prozess beendet), schaltet nur dieses Plugin stumm. Der Platz zeigt „Plugin abgestürzt – neu laden“, die Statuszeile nennt es ebenfalls; alles andere spielt weiter. „Neu laden“ startet den Prozess neu und stellt den letzten Zustand und die Regler wieder her; der Graph bleibt derselbe. Kein automatisches Neuladen.
- **Suche:** Jede Plugin-Datei wird in einem eigenen Suchprozess gelesen (20 s Frist); Absturz oder Zeitüberschreitung wird ein Suchproblem statt eines Absturzes der App.
- **Latenzausgleich:** Meldet ein Plugin Latenz (CLAP `latency`), verzögert die Engine alle anderen Spuren um die Differenz, damit alles zusammen ankommt; ein abgeschaltetes Effekt-Plugin behält seine Latenz. `StereoProcessor::latency()` (Standard 0).
- **Wächter:** Der Plugin-Prozess beendet sich selbst, wenn die App 10 s lang kein Lebenszeichen gibt.
- **Testmenü:** Mit `WOA_TESTMENU=1` erscheint im Plugin-Platz „Absturz simulieren“. `WOA_PLUGINS_IN_PROCESS=1` schaltet den Schutz aus (Fehlersuche).
- **Tests:** Selbstprüfung mit dem Test-Plugin, das absichtlich abstürzt oder hängt (Offline-Render und Echtzeit-Fenster, ohne Audiogerät): gleiche Ausgabe wie im Programm, gemessene Latenz (Impuls an Stelle 64), Absturz stumm nur dort, Hänger wird beendet, Neuladen, Zustand und Regler über die Prozessgrenze, ein Suchprozess, der stirbt; Engine: Ausgleich zwischen Spuren.
- **Nicht gebaut:** Plugin-Fenster im Schutzprozess unter Windows; Nachführen einer Latenz, die sich nach dem Laden ändert; Verpacken des zweiten Programms (AppImage, MSI, Flatpak) — bis dahin ruft sich die App selbst mit `--woa-plugin-host` auf.
- **Abhängigkeit:** `memmap2` in `woa-plugins`; `latency` in `clack-extensions`.

## 0.27.1 — 2026-10-30

- **CI:** Neuer Workflow „Windows-Bau“ (`.github/workflows/windows-build.yml`), von Hand zu starten. Baut `woa.exe`, `woa-headless.exe` und `woa-streamd.exe` im Release-Profil und legt sie als MSI-Installer (WiX 5, `packaging/windows/woa.wxs`: Programme-Ordner, Startmenü-Eintrag, neuere Version ersetzt ältere) und als ZIP an den Lauf (30 Tage). Unsigniert. Produktname in der MSI mit einfachem Bindestrich (Codepage 1252).

## 0.27.0 — 2026-10-30

Paket 32 aus M8: Plugin-Fenster (Abnahme: `docs/abnahme.md`).

- **Fenster:** Im Kanalzug hat jedes geladene Plugin die Taste „Fenster“. Das Plugin öffnet sein eigenes, schwebendes Fenster (CLAP `gui` im schwebenden Modus). Schließen und wieder Öffnen baut nichts neu auf; beim Entfernen oder Beenden wird die Oberfläche vor dem Plugin abgebaut. Bietet ein Plugin kein schwebendes Fenster an, steht ein Hinweis in der Statuszeile.
- **Host:** Zeitgeber (`timer`), Dateikennungen (`posix-fd`, nur Unix), `request_callback` und Regler-Neuabgleich (`params`) werden bedient, jedes Bild der Oberfläche über `ClapInstance::pump`.
- **Reglerrückmeldung:** Was im Plugin-Fenster bewegt wird, kommt über eine lock-freie Queue (`rtrb`) aus dem Audio-Thread zurück und wird zu Rückgängig-Schritten: ein gezogener Regler ist ein Schritt, ein Preset-Wechsel (viele Regler auf einmal) ein Schritt mit dem ganzen Zustand. Werte, die der Host selbst setzt, werden nicht zurückgemeldet. Rückgängig und Wiederholen stellen die Regler im laufenden Plugin wieder her.
- **Tests:** Host- und Rack-Tests mit dem Test-Plugin (Fensterablauf, Zeitgeber, Regler ziehen, Preset-Flut ohne Allokation unter `assert_no_alloc`, Rückgängig/Wiederholen); kein echtes Fenster.
- **Nicht gebaut:** Automation von Plugin-Reglern, eingebettetes Fenster, Auslagern in einen eigenen Prozess (Paket 33).
- **Abhängigkeiten:** `rtrb` und `libc` (Unix) in `woa-plugins`; `gui`, `timer`, `posix-fd` in `clack-extensions`.

## 0.26.1 — 2026-10-30

Sicherheit (#101): Der DAWproject-Import begrenzt jede Audiodatei beim Lesen auf das Restbudget der Gesamtgrenze, nicht nur auf die Grenze je Datei. Eine präparierte Datei mit gefälschten ZIP-Größen kann die Platte nicht mehr vollschreiben. Test dazu.

## 0.26.0 — 2026-10-30

Paket 31 aus M8: CLAP-Plugins finden und laden (Abnahme: `docs/abnahme.md`).

- **Modell:** Plugin-Plätze auf Spuren (`PluginSlot`), fünf Befehle mit Rückgängig, Projektformat 4 nur bei Plugins.
- **Host (`woa-plugins`):** Suche in den Standardordnern und `CLAP_PATH`, Instanz über `clack-host`, Rechner ohne Sperre und ohne Speicheranforderung im Audio-Thread, Test-Plugin aus `clack-plugin` mit Selbstprüfung.
- **Engine:** Spuren mit Plugins laufen in Stereo. Ein Instrument-Plugin ersetzt den eingebauten Synth der MIDI-Spur (Clip-Noten im Takt des Transports, Live-Noten, Sprung und Stopp lassen nichts hängen), Effekt-Plugins folgen den eingebauten Effekten. Fehlt ein Instrument, schweigt die Spur; ein fehlender Effekt wird durchgereicht. Export und Stems rechnen mit Plugins (`render_clips_plugins`, `render_stems_plugins`).
- **Oberfläche:** Im Kanalzug „+ Plugin“ (Browser mit Instrumenten und Effekten, „Neu suchen“), Platz mit Ein/Aus und Entfernen, Regler als Zahlenfelder, „nicht installiert“ bei fehlendem Plugin. Die Suche startet erst beim ersten Bedarf. Der Plugin-Zustand wird beim Speichern, Autosave und Export ins Projekt übernommen; der Export rechnet mit eigenen Instanzen.
- **Tests:** Offline-Render mit dem Test-Plugin (Pegelvergleich, Bypass, fehlendes Plugin, Zustand speichern und laden, 200 Blöcke unter `assert_no_alloc`), Zustandstest der Oberfläche.
- **Herkunft:** Kein Code übernommen; `clack-host` und `clack-plugin` sind Abhängigkeiten. Ardour und tracktion sind noch nicht gelesen (Pakete 32/33).
- **Prüfung:** Kein Log mehr im Audio-Thread, wenn `start_processing` scheitert; der Platz meldet den Ausfall über seinen Status.
- **Abhängigkeiten:** `clack-host`, `clack-plugin` und `base64` (Lizenzen prüft `cargo deny` in der CI).
- **Nicht gebaut:** Plugin-Fenster (Paket 32), Auslagern in einen eigenen Prozess und Latenzausgleich (Paket 33), Anbindung in `woa-headless` und Agent.

## 0.25.0 — 2026-10-30

Paket 30 aus M9: Voice-Tracking-Editor.

- **Voice-Track (`woa_radio::voicetrack`):** Eine vorab aufgenommene oder importierte Moderation liegt als eigener Playlist-Eintrag (`voice`) zwischen zwei Titeln und verweist auf ihre Nachbarn. Zeitachse: Null ist der Beginn der Überblendung, die Stimme beginnt bei `offset` (negativ: im Ausklang von A). Werte: Beginn, Stimmenpegel, Absenkung der Musik (Vorgabe −12 dB), Rampe vor und nach der Stimme.
- **Ducking im Mixer:** Der Mixer startet die Stimme selbst, sample-genau relativ zum Übergang (keine Zeitsteuerung im Steuer-Thread). Die Absenkung ist eine Kosinus-Rampe aus einer einzigen reinen Funktion (`duck_gain`), die Mixer, Editor und Tests gemeinsam nutzen. Kommt der Übergang früher (Taste „Weiter“) oder später als geplant, läuft die Stimme immer vollständig von vorn, nie mittendrin. Die Stimme läuft über den Cart-Zweig; neuer Befehl `Cmd::VoiceOver`, kein Speicher und keine Sperre im Audio-Thread.
- **„Prüfen“:** Ändert sich ein Nachbar (anderer Titel, andere Länge, andere Marken), steht der Voice-Track auf „prüfen“ und geht nicht auf Sendung, bis er im Editor bestätigt wird. Nachträgliches Messen eines Nachbarn gilt nicht als Änderung. Die Meldung steht in der Sendungsmeldung und im Log.
- **Voice-Tracks sind keine Titel:** sie sind nie das laufende Deck, zählen nicht in Gesamtlänge und Restzeit, beginnen nie eine Sendung und werden bei harten Events nicht zwischen Stimme und Titel zerschnitten.
- **AutoDJ:** Auswahlen aus der Kategorie `moderation` werden als Voice-Track eingefügt, vor dem Titel, der dahinter kommt; sie zählen nicht zum Vorlauf.
- **Editor (Radio-Modus, Rechtsklick auf einen Titel):** „Voice-Track danach einfügen …“ wählt eine Sprachdatei (Messung, Vorschlag für den Beginn: die Stimme endet am Ramp-Punkt von B, sonst 2 s nach dem Beginn von B). „Voice-Track bearbeiten …“ zeigt die Wellenformen von A (Ausklang) und B (Anfang), die mit der Maus verschiebbare Stimme und darunter die Absenkungskurve. „Vorhören“ rendert A, Stimme, B offline durch den echten Mixer und spielt das Ergebnis nur im Kopfhörer (PFL), nie auf Sendung. In der Playlist trägt die Zeile „VOICE“ und „prüfen“.
- **Tests:** Offline-Render A, Stimme, B mit erzeugten Tönen durch den echten Mixer unter `assert_no_alloc` (Beginn der Stimme auf 0,06 s genau, Absenkung 12 dB auf 3 % genau, Rückkehr der Musik, keine Lücke), Stimme nach dem Übergang, „Weiter“ vorzeitig, Tausch eines Nachbarn setzt „prüfen“ und die Stimme wird nicht gesendet (nach Bestätigung schon), Speichern und Laden als Schnappschuss (`insta`), Einheitentests der Kurve, des Abgleichs und des Vorschlags, Editor als Zustandstest, Fenster zeichnet ohne Display.
- **Herkunft und Lizenz:** Idee von `smooth_add` in Liquidsoap, kein Code übernommen (`docs/herkunft.md`).
- **Nicht gebaut:** Aufnahme der Stimme im Editor (Mikrofon, braucht einen Gerätetest) und ein MCP-Werkzeug für Voice-Tracks.

## 0.24.0 — 2026-10-30

Paket 29 aus M9: Feste Zeit-Events und pünktliche volle Stunde.

- **Zeit-Events (`woa_radio::events`):** Nachrichten, Werbung oder Stundenjingle mit Uhrzeit oder Muster (Wochentage, Stunden, Minute, Sekunde), **hart** oder **weich**. Tabelle in `events.json` (atomar gespeichert, Grenzen geprüft, kaputte Datei wird gemeldet und nie überschrieben). Ein Prüftakt im Steuer-Thread löst aus; Vorbild ist die Eintragstabelle `cron.liq` in Liquidsoap (nur die Idee, kein Code).
- **Hart:** 30 s vorher wird die Datei vorgeladen, das Event wird in die Playlist eingefügt und beginnt zur genauen Zeit; der laufende Titel blendet in 250 ms ab, auch mitten in einer Überblendung (die wird beschleunigt beendet). **Weich:** das Event kommt direkt nach dem laufenden Titel. Zu spät (hart 20 s, weich 30 min) oder nicht auf Sendung: „verpasst“, mit Log, nie doppelt.
- **Sommerzeit automatisch:** Ortszeit mit EU-Regel ohne Zeitzonendatenbank (`woa_engine::localtime`), Häkchen „Sommerzeit“ in der Stundenuhr. Feste Uhrzeit in der doppelten Oktoberstunde: einmal; in der ausgefallenen Märzstunde: zum Umstellmoment; jede Stunde: jede echte Stunde. Die Stundenuhr nutzt dieselbe Zone.
- **AutoDJ plant rückwärts:** Vor einem harten Event wählt `rotation::plan_until` in den letzten 8 Minuten die Titel, die am besten in die Lücke passen, und zieht die Überblendung ab; Schnitt und Rest stehen im Log.
- **Übergänge nach Mixxx:** `woa_radio::transition` rechnet die Überblendung aus Outro- und Intro-Marken samt Randfällen. **Verhaltensänderung:** bei sehr kurzem Folgetitel (4 s) ist die Überblendung jetzt 1 s statt der Hälfte; `show::total_length` rechnet danach. `Cmd::Queue` trägt dafür `lead` und `hold`, neu sind `Cmd::StartAt` und `Cmd::ClearStartAt`.
- **Oberfläche:** Knopf „Zeit-Events“ (Liste, Muster, hart/weich, Datei, Entfernen); unter der Uhr „NÄCHSTES EVENT … in mm:ss“ und „ZULETZT … ±x,x s“.
- **Tests:** eine Stunde durch den echten Mixer (hart nach 0,12 s, mitten in Überblendung, weich nach dem Titel, keine Lücke, ohne Speicher im Audio-Thread), eine Woche mit simulierter Uhr über Mitternacht und Zeitumstellung, Mixxx-Randfälle, Planer, Editor, Fenster ohne Display.
- **Herkunft und Lizenz:** `docs/herkunft.md`.
- **CI:** Clippy-Hinweis (`explicit_counter_loop`) der neueren Toolchain in der Mustersuche behoben.
- **Nicht gebaut:** Nachrichtenquelle (externer Dienst); Events spielen eine lokale Datei.

## 0.23.0 — 2026-10-30

Paket 28 aus M9: Stundenuhr und AutoDJ-Rotation.

- **Stundenuhren (`woa_library::rotation`):** Eine Uhr ist eine Folge von Slots, jeder Slot hat eine Kategorie (ein Tag der Bibliothek, z. B. `hit`, `neu`, `jingle`) und optional eine Ersatzkategorie. Eine Wochentabelle (7 × 24 Stunden) ordnet jeder Stunde eine Uhr zu; innerhalb der Stunde laufen die Slots der Reihe nach und beginnen mit jeder neuen Stunde von vorn. Gespeichert wird in `stundenuhr.json` (atomar, Grenzen und Namen werden geprüft, kaputte Dateien werden gemeldet und nie überschrieben).
- **Auswahl:** Gewichtet nach der Zeit seit dem letzten Spiel (je länger her, desto wahrscheinlicher, höchstens eine Woche zählt), mit Sperrfristen für Titel (Vorgabe 180 min) und Interpret (60 min). Der Interpret kommt aus dem Titel nach dem Muster „Interpret - Titel“; ohne Trenner gilt nur die Titel-Sperre. Ist der Slot leer (keine Titel oder alle gesperrt), kommt die Ersatzkategorie, danach der Notfall-Tag (Vorgabe `notfall`; er ignoriert die Sperrfristen, bevor gar nichts spielt). Das Muster Slot, Ersatz, Notfall stammt von `fallback`/`rotate` in Liquidsoap, das Auffüllen einer Warteschlange vom AutoDJ in Mixxx; Code ist nicht übernommen.
- **Deterministisch:** Zufall ist SplitMix64 mit dem Startwert aus der Datei; gleicher Startwert, gleiche Bibliothek und Historie ergeben die gleiche Folge. Jede Wahl steht im Log (Uhr, Slot, gewünschte und gewählte Kategorie, Titel) und in der Liste „Letzte Auswahlen“.
- **Spielhistorie:** Neue Tabelle `plays` in der Bibliothek (Schema 4, Migration von 1 bis 3 getestet). Jeder Titel, der in der Sendung beginnt und in der Bibliothek steht, kommt mit Interpret, Kategorie und Zeit hinein; die Sperrfristen überleben damit einen Neustart, auch die Position in der Stundenuhr.
- **AutoDJ:** Schalter im Fenster. Er hält `Im Voraus` Titel hinter dem laufenden bereit (Vorgabe 3) und hängt nur hinten an: von Hand gesetzte Titel bleiben stehen, belegen aber ihre Zeit und gelten für die Sperrfristen (keine Doppel). Läuft im Oberflächen-Thread alle 3 Sekunden höchstens einmal, nie im Audio-Thread.
- **Oberfläche:** Neuer Knopf „Stundenuhr“ in der Menüleiste öffnet das Fenster „Stundenuhr und AutoDJ“: Uhren anlegen, umbenennen, löschen; Slots mit Kategorie und Ersatz (Auswahl aus den Tags der Bibliothek), verschieben, entfernen; Wochenraster mit Klick zum Zuordnen und Rechtsklick zum Leeren, die aktuelle Stunde ist umrandet; Sperrfristen, Zeitverschiebung gegenüber UTC, Notfall-Tag, AutoDJ-Schalter.
- **Zeitzone:** Die Ortszeit ist eine Einstellung in Minuten gegenüber UTC (Vorgabe 60); die Umstellung auf Sommerzeit muss von Hand erfolgen.
- **Nicht in diesem Paket:** feste Zeit-Events (Nachrichten, Werbung zur vollen Stunde) und der Voice-Tracking-Editor; beides gehört zu M9, steht aber nicht im Auftrag dieses Pakets.
- **Tests:** 24 Stunden mit simulierter Uhr gegen 500 erzeugte Titel (keine Sperrfrist verletzt, Slot-Folge je Stunde stimmt), Rückfall auf Ersatz und Notfall, nichts zu spielen ohne Absturz, gleicher und anderer Startwert, Historie nach Neustart der Datenbank (Sperren und Slot-Position), von Hand gesetzte Titel, Datei mit feindlichen Werten, Editor als Zustandstest (Anlegen, Umbenennen mit Wochenübernahme, Slots, Speichern und Laden), AutoDJ-Takt gegen eine nachgebildete Sendung, Fenster zeichnet ohne Display.

## 0.22.0 — 2026-10-30

Paket 27 aus M7: DAWproject-Import und -Export.

- **Neues Modul `woa_io::dawproject`:** Eine `.dawproject`-Datei (DAWproject 1.0, ZIP mit `project.xml`, `metadata.xml` und `audio/`) wird geschrieben und gelesen. Übertragen werden Spuren (Audio, MIDI, Bus), Clips mit Position, Länge, Versatz, Blenden und Dehnung, MIDI-Noten und Controller, Tempo, Lautstärke, Pan, Mute, Solo, Ausgänge, Sends, die eigenen Effekte (als `Device` mit der Kennung `woa:<art>` samt allen Reglern) und die Automation von Lautstärke, Pan, Sends und Effekt-Reglern.
- **Was das Format nicht kennt** (Tonhöhe, Clip-Verstärkung, Blendenform, „Tempo folgen“, die genaue Dehnung) steht als Notiz `woa: …` im Kommentar des Clips und kommt bei der Rückkehr nach WOA wieder heraus. Andere Programme zeigen die Notiz nur als Kommentar.
- **Was WOA nicht kennt** (fremde Plugins, Szenen, Marker, Tempo- und Taktart-Automation, Clip-Schleifen, Video, Warp-Punkte außer Anfang und Ende) wird nicht übernommen, aber im Bericht genannt und geloggt; die Statuszeile zeigt die Zahl der Hinweise.
- **Fremde Dateien sind Eingabe, nicht Vertrauen:** XML mit Tiefen- und Elementgrenze (Entitäten und DTDs werden nie aufgelöst), ZIP mit Grenzen für Einträge, Größe je Eintrag und gesamt (auch wenn die Größenangabe lügt), Audio landet nur unter selbst vergebenen Namen im Ordner `audio/` des neuen Projekts (nie ein Pfad aus der Datei), und das Ergebnis geht durch dieselbe Prüfung wie eine geladene `project.json`. Bei einem Fehler wird der angelegte Ordner wieder entfernt.
- **Konventionen nach den Referenztests von Bitwig:** Lautstärke linear, Pan normalisiert (0,5 = Mitte), Master als Track mit `Channel role="master"`, Audio über `Warps` mit Inhalt in Sekunden und Zeit in Beats, MIDI-Controller als `Points` mit `Target expression="channelController"`.
- **Oberfläche:** Menü Datei: „Als DAWproject exportieren …“ und „DAWproject öffnen …“ (legt `<Name>.woa` neben der Datei an und öffnet es). Rate und Länge der Audiodateien kommen aus dem WAV-Kopf, sonst vom Decoder.
- **Schema-Datei:** `crates/io/tests/schema/Project.xsd` stammt aus dem DAWproject-Repository (MIT, Copyright 2020 Bitwig), die Lizenz liegt daneben.
- **Abhängigkeiten:** `zip` 9.0 (nur Deflate) und `quick-xml` 0.42, beide MIT; `tempfile` jetzt auch für das atomare Schreiben. `zip` 8.6 fiel durch `cargo deny` (RUSTSEC-2026-0367 bis 0369, alle zu `extract()`, das wir nicht benutzen; behoben ab 9.0).
- **Tests:** Rundreise (Spuren, Mixer, Bus, Send, Effekte, Automation, MIDI, Clip-Werte), gedehnter Clip mit „Tempo folgen“, `project.xml` gegen das offizielle Schema geprüft (`xmllint`, übersprungen wo es fehlt), Schnappschuss des XML, handgebaute fremde Datei nach den Konventionen von Bitwig (mit fremdem Plugin im Bericht), feindliche Dateien (kein ZIP, ohne `project.xml`, zu tief, falsche Wurzel, Pfad-Trick im Audiopfad). Offizielle Beispieldateien: im DAWproject-Repository gibt es keine `.dawproject`-Dateien, nicht gefunden.

## 0.21.0 — 2026-10-30

Paket 26 aus M7: Alle Export-Formate und Stems.

- **Neue Formate:** Opus (`.opus`), Ogg Vorbis (`.ogg`) und AAC (`.aac`, ADTS) über ein externes `ffmpeg` (`woa_engine::ffmpeg`): als Prozess aufgerufen, nichts gelinkt, Argumente als Liste ohne Shell, Rohdaten über Pipes (Eingang in eigenem Thread, damit kein Pipe-Puffer verklemmt). Gesucht wird in `WOA_FFMPEG`, neben dem Programm und im `PATH`. Fehlt ffmpeg oder der Encoder, kommt eine klare Meldung und es bleibt keine halbe Datei zurück (erst kodieren, dann anlegen). Opus rechnet ffmpeg auf 48 kHz um.
- **Wortbreite:** WAV in 16 Bit, 24 Bit oder 32 Bit Float (Vorgabe wie bisher Float); FLAC in 16 oder 24 Bit (Vorgabe wie bisher 24). FLAC kennt kein Float, die Auswahl wird abgelehnt statt still etwas anderes zu bauen. `export_stereo` und `export_stereo_to` behalten ihr Verhalten; `export_with`/`export_with_to` nehmen einen `ExportSpec`.
- **Stems (`woa_engine::stems`):** Eine Datei je Spur und je Bus, alle gleich lang, dazu auf Wunsch der Mixdown (`00 Mixdown`). Gerechnet in einem Offline-Durchgang über die Abgriffe des Graphen, nach jedem Block geleert (ein Verlust wäre ein Fehler). Die Summe der obersten Stems (Ausgang Master) ist der Mixdown; Spuren in einem Bus stecken schon im Bus-Stem. Stumme Spuren bekommen keine Datei. Dateinamen mit laufender Nummer und bereinigt (Sonderzeichen, `..`, reservierte Windows-Namen); ein Symlink als Ziel wird abgelehnt; bei einem Fehler werden die neu angelegten Dateien entfernt. Der Lautheitsangleich gilt mit demselben Gain für alle Dateien.
- **Oberfläche:** Neues Export-Fenster (Strg+E, Menü Datei): Format (ausgegraut mit Hinweis, wenn ffmpeg oder der Encoder fehlt), Wortbreite oder Bitrate, Haken für Stems, Busse und Mixdown. Die Suche nach ffmpeg läuft einmal beim Öffnen, nie je Bild. Stems gehen in einen Unterordner „<Projekt> Stems“.
- **Kennung:** Das Lesen der ffmpeg-Ausgabe nutzt `as_chunks` (der neuere Clippy der CI verlangt es).
- **Agent:** `export` nimmt auch `.opus`, `.ogg`, `.aac`; die Fehlermeldung für eine unbekannte Endung nennt alle sechs.
- **Tests:** Summe der Stems gleich Mixdown (Projekt mit Bus, Send, Pan, Pegel, stummer Spur), Rücklesen aller Dateien, gleicher Gain, Wortbreiten, FLAC ohne Float, fehlendes ffmpeg für alle drei Formate, Opus/Ogg/AAC über das echte ffmpeg (übersprungen, wo es fehlt), Dateinamen, Symlink-Ziel, Export-Fenster als Zustandstest. Zwei bestehende Agent-Tests nutzten `.ogg` als „unbekannte Endung“ und nehmen jetzt `.xyz`.

## 0.20.1 — 2026-10-30

Öffentliches Repo aufgeräumt (André: „push nach nix hin was da nicht hin soll“).

- Interne Runbooks und die Mitbewerber-/UI-Analyse aus dem Repo genommen (`docs/andre-handgriffe.md`, `docs/runbook-morgen.md`, `docs/ui-analyse-und-ziele.md`); sie liegen lokal unter `docs/intern/`.
- Servername, Server-IP, Hosting-Anbieter, interne Token-Liste und Heimatpfade aus Code, Doku und Tests entfernt („eigener Server“).
- `.gitignore`: `docs/intern/`, `.refs/`, alles mit `mitbewerber` im Namen, `*.local.md`.
- `CLAUDE.md`: Regel „Das Repo ist öffentlich“.
- Prüfung: gitleaks über alle 136 Commits ohne Fund; kein Geheimnis war im Repo.## 0.20.0 — 2026-10-30

Paket 25 aus M7: Analyse mit Tempo, Tonart, Lautheit und automatischen Cue-Punkten.

- **Analyse (`woa_dsp::analysis::track`):** Tempo aus der Autokorrelation der Einsatzkurve (70 bis 180 BPM, `None` bei Sinus ohne Einsätze, Stille oder zu kurzem Material), Tonart aus einem Chroma-Vektor gegen die Krumhansl-Profile (Anzeige „C-Dur“, „a-Moll“), Stille vorn und hinten (Schwelle −50 dBFS), Lautheit und True Peak aus dem vorhandenen Code. Alles ohne neue Abhängigkeit, im Hintergrund-Thread, nie im Audio-Thread. Die Treffsicherheit an echter Musik ist eine Vermutung, bis André sie an bekannten Titeln gesehen hat.
- **Cue-Vorschläge:** Cue-In hinter der Stille am Anfang, Cue-Out vor der Stille am Ende, Fade-Out am Beginn des Ausklangs (höchstens 15 s). Kürzer als 2 s oder ohne Stille: kein Vorschlag. Im Radio-Modus setzt das Messen einer Playlist-Zeile die Vorschläge nur, wo noch keine Cue-Punkte stehen; von Hand gesetzte haben Vorrang. Im Cue-Editor zeigt eine blasse Linie den Vorschlag, „Vorschlag einsetzen“ übernimmt nur die fehlenden Marker.
- **Bibliothek:** Schema 3 (Spalten für Tempo, Tonart, Lautheit, True Peak, Stille und Cue-Vorschlag; `analyzed_hash` verknüpft die Werte mit dem Inhalt, eine geänderte Datei kommt wieder an die Reihe). Schema 2 wird beim Öffnen angehoben, die Titel bleiben. Die Analyse läuft nach dem Import im Hintergrund mit eigener Datenbankverbindung, Fortschritt und Abbruch über Atomics; eine nicht lesbare Datei wird einmal gezählt und nicht endlos wiederholt.
- **Oberfläche:** Neue Spalten BPM, TONART und LUFS in der Bibliothek („…“ bis der Titel dran war), die Detailzeile zeigt alle Messwerte und „Neu analysieren“. Fenster etwas breiter.
- **Lautheitsangleich:** Der Radio-Modus gleicht Titel schon auf −16 LUFS an (Spitze höchstens −1 dBFS); die Rechnung steht jetzt an einer Stelle (`gain_to_target_db`), Radio und Bibliothek nutzen dieselbe.
- **Tests:** Klicktrack 120 BPM (und andere Tempi und Abtastraten) gibt 120 ±1; Sinus-Akkorde geben ihre Tonart; Stille vorn und hinten; EBU-Tech-3341-Referenz (1 kHz bei −23 dBFS gibt −23,0 LUFS) auch über den Dateiweg; 1 000 erzeugte Titel offline analysiert, danach liegen alle mit dem Gain auf dem Ziel; Migration, kaputte Datei, geänderter Inhalt; Cue-Editor und Bibliothek-Fenster als Zustandstests.

## 0.19.0 — 2026-10-30

Paket 24 aus M7: Bibliothek in SQLite mit Suche und Tags.

- **`woa-library`:** Datenbank `bibliothek.sqlite3` im Datenordner. Schema 2 (Titel mit Pfad, Inhaltskennung, Dauer, Art Musik/Sample/Jingle; Tags; FTS5-Volltextindex über Titel, Tags und Pfad). Die Schemaversion steht in `PRAGMA user_version`; ältere Datenbanken werden beim Öffnen schrittweise angehoben (Test: Datenbank im Zustand von Schema 1 behält ihre Titel und bekommt den Suchindex), eine neuere wird abgelehnt statt verändert.
- **Suche:** Jedes Wort ist ein Präfix, alle Wörter müssen passen; Groß-/Kleinschreibung und Akzente sind egal („cafe“ findet „Café“). Der Suchtext wird zu einem sicheren FTS-Ausdruck umgeschrieben (Test mit Sonderzeichen und SQL-Fragmenten). Filter nach Art und Tag.
- **Tags:** getrimmt, klein geschrieben, höchstens 40 Zeichen und 50 je Titel; der Tag-Text steht auch im Suchindex.
- **Import im Hintergrund:** Ordner rekursiv (ohne Symlinks, höchstens 16 Ebenen und 200 000 Dateien), eine eigene Datenbankverbindung im Hintergrund-Thread, Fortschritt und Abbruch über Atomics, Eintragen in Stapeln von 50 je Transaktion. Doppelte werden am Inhalt erkannt (eigene 128-Bit-Kennung, kein kryptographischer Hash und keine neue Abhängigkeit dafür), eine verschobene Datei zieht ihren Eintrag nach, eine geänderte aktualisiert ihn. Kaputte Dateien werden gezählt und übersprungen.
- **Oberfläche:** Neuer Knopf „Bibliothek“ in der Menüleiste öffnet in beiden Layouts dasselbe Fenster: Suchfeld, Filter nach Art und Tag, „Ordner importieren …“ mit Fortschritt, Trefferliste, darunter Tags des gewählten Titels (anlegen und entfernen), „▶ Vorhören“ über den vorhandenen PFL-Weg (Radio-Modus mit Kopfhörer-Ausgang; nie auf Sendung). Titel lassen sich ziehen: in der Timeline auf die Spur unter dem Zeiger, im Radio auf die Playlist (hinten anhängen) oder auf ein Cart-Pad (belegen).
- **Kennung:** Die Inhaltskennung liest ihre 8-Byte-Wörter ohne `chunks_exact` (der neuere Clippy der CI verlangt dafür `as_chunks`).
- **Neue Abhängigkeit:** `rusqlite` 0.41 mit `bundled` (MIT; SQLite selbst ist gemeinfrei, wird mit eingebaut und braucht auf Linux und Windows einen C-Compiler im Bau). `cargo deny` läuft in der CI.
- **Tests:** 1 000 erzeugte Sinus-Dateien werden im Hintergrund importiert (Zeit im Testlauf unter 2 s im Debug-Bau), danach Suche nach Titel und Ordnername, zweiter Import erkennt alle 1 000 als Doppelte, Kopie unter anderem Namen wird erkannt, Abbruch, fehlender Ordner, Symlink-Schleife; Suche, Tags, Grenzen, Transaktion mit Rücksetzen, Migration; Fenster-Zustand (Import, Suche, Tag, Filter) und Zeichnen ohne Fenster.

## 0.18.1 — 2026-10-30

Feedback-Fenster (André: „mittig und in schön … nach 3 Sekunden automatisch schließt“).

- **Oberfläche:** Das Feedback-Fenster steht mittig über der abgedunkelten App, als Karte im Studio-Look mit Überschrift, breitem Textfeld und orangem Senden-Knopf.
- **Danke:** Nach erfolgreichem Senden zeigt es ein grünes Häkchen und „Danke!“ mit einem ablaufenden Balken und schließt sich nach 3 Sekunden von selbst. Fehlermeldungen bleiben stehen, bis man sie schließt. Test dazu.

## 0.18.0 — 2026-10-30

Paket 23 aus M6: Time-Stretch, Tonhöhe und die Abnahme „ganzer Song“.

- **Dehnung (`woa_dsp::stretch`):** Eigene Zeitdehnung nach WSOLA, rein in Rust und ohne neue Abhängigkeit (die Alternative `signalsmith-stretch` hätte eine C++-Bindung und einen Windows-CI-Check gebraucht, das bleibt eine spätere Option). Die Tonhöhe bleibt beim Dehnen erhalten; die Ausgabe hat genau `round(Länge × Faktor)` Samples. Die Tonhöhe ändert sich über Dehnen plus kubische Neuabtastung. Das läuft nur beim Bau des Graphen im Control-Thread, ein kleiner Zwischenspeicher verhindert, dass jeder Graphbau alles neu rechnet; der Audio-Thread spielt das fertige Ergebnis wie jeden anderen Clip.
- **Modell:** Jeder Clip hat `stretch` (Faktor, 0,25 bis 4), `pitch_semitones` (±24) und optional `follow_tempo_bpm`. Neue Befehle `SetClipStretch` (Länge, Faktor und Blenden zusammen), `SetClipPitch` und `SetClipFollowTempo`, je ein Undo-Schritt; ungültige Werte werden abgelehnt. Projektformat 3 wird nur geschrieben, wenn ein Clip eines dieser Felder nutzt; alle anderen Projekte bleiben Format 2. Der Versatz im Ton (`offset_beats`) zählt in Beats des Tons, die Länge in Beats der Zeitachse; Schneiden (Oberfläche und Agent) rechnet das um.
- **Tempo folgen:** Ein Clip mit „Tempo folgen“ merkt sich das Tempo, zu dem sein Ton gehört. Ändert sich das Projekttempo, wird der Ton gedehnt statt abgeschnitten oder verschoben: er bleibt auf dem Raster, die Tonhöhe bleibt. Ohne den Haken verhält sich der Clip wie bisher.
- **Oberfläche:** Alt gedrückt am linken oder rechten Clip-Rand ziehen dehnt die Zeit (ohne Alt kürzt der Rand wie bisher); die Wellenform zeigt die Dehnung schon beim Ziehen, das Raster rastet ein und der Faktor bleibt zwischen 0,25 und 4. In der Clip-Leiste eine neue Zeile „ZEIT“: Dehnung in Prozent, Tonhöhe in Halbtönen, Haken „Tempo folgen“; Doppelklick setzt zurück.
- **Agent:** neue Werkzeuge `stretch_clip`, `set_clip_pitch` und `set_clip_follow_tempo`, beschrieben in `docs/agent.md`.
- **Tests:** Sinus 440 Hz um 10 % gedehnt: Länge ±1 %, Frequenz bleibt (FFT-Spitze); Tonhöhe +2 Halbtöne: FFT-Spitze bei 493,9 Hz ±1 %, Länge gleich; Tempo 120 auf 100 mit Folgen: Länge und Frequenz stimmen; Undo bitgleich, 0 Halbtöne und gleiches Tempo lassen den Render bitgleich; Echtzeit unter `assert_no_alloc`. Modelltests (Undo, Redo, Fades, schlechte Werte, Format), Zustands- und Oberflächentest ohne Fenster und ohne Ton (Alt-Ziehen, ein Undo-Schritt, ohne Alt nur Kürzen), Agent über MCP. Dazu die M6-Abnahme als Test: ein ganzer Song mit geteilten, gedehnten, transponierten und überblendeten Clips und geändertem Tempo wird exportiert und wieder eingelesen (Länge, kein NaN, keine Knackser, Pegel, Frequenz).

## 0.17.2 — 2026-10-30

Sicherheit (#88): Take-Ordner aus einer präparierten Projektdatei können die Oberfläche nicht mehr lahmlegen.

- **Modell:** Ende und Versatz eines Take-Ordners sind auf `MAX_TAKE_BEATS` (1 000 000 Schläge, 24 h bei 400 BPM passen) begrenzt; größere Werte lehnt `check()` ab. Test dazu.
- **Oberfläche:** Die Wellenform im Comping-Bereich zeichnet nur noch die sichtbaren Spalten und hört am Ende der Peaks auf.

## 0.17.1 — 2026-10-30

Sicherheit: zwei Funde der automatischen Prüfung behoben.

- **Modell (#79):** Die Routing-Prüfung beim Laden erkennt Kreise jetzt in einem Durchgang über den ganzen Signalgraphen (O(V+E)) statt mit einer Suche je Verbindung; doppelte Sends und IDs prüfen Mengen statt Listen. Eine präparierte Projektdatei mit vielen Spuren und Sends legt das Laden nicht mehr lahm. Tests: 2000 Busse in Kette laden schnell, ein in die Datei geschmuggelter Kreis wird abgelehnt.
- **Agent (#83):** `write_png` schreibt wie der Export in eine neu angelegte Temp-Datei und setzt sie per `rename` an den Zielnamen; ein untergeschobener Symlink wird ersetzt, nicht beschrieben. Test dazu.

## 0.17.0 — 2026-10-30

Paket 22 aus M6: Aufnahme mit Latenzausgleich, Punch-in, Loop-Takes und Comping.

- **Modell:** Neuer Take-Ordner je Spur (`TakeFolder`): die Takes einer Strecke, dazu der Comp als Liste von Abschnitten. Der Comp wird als gewöhnliche Clips der Spur abgelegt, daher bleiben Engine, Export und Editoren unverändert. Ein Befehl `SetTakeFolder` mit Undo und Redo (ein Schritt je Auswahl); ungültige Takes, Bereiche und Pfade werden abgelehnt. Projektformat 3 wird auch geschrieben, wenn eine Spur einen Take-Ordner hat; alle anderen Projekte bleiben unverändert. `plan_recording` rechnet Latenz, Punch und Loop-Runden auf Samples genau.
- **Aufnahme:** Die Aufnahme wird ein Clip an der Abspielkopf-Position beim Start, die Gerätelatenz wird am Dateianfang übersprungen. Ein Punch-Bereich (in Takten) behält nur den Teil darin. Bei eingeschalteter Schleife entsteht je volle Runde ein Take (höchstens 64, eine angefangene Runde wird verworfen), alle als Ausschnitte einer einzigen Aufnahmedatei.
- **Oberfläche:** Schalter und Takt-Felder für Punch in der Transportleiste. Unter der Spur ein aufklappbarer Take-Ordner mit dem Comp-Streifen, aufgeklappt eine Zeile je Take mit Wellenform. Bereich ziehen oder Takt anklicken wählt den Take für diese Stelle; der Papierkorb löscht den Ordner (Strg+Z holt ihn zurück).
- **Tests:** Fake-Eingang mit künstlicher Latenz ohne Gerät und ohne Ton: Impuls liegt nach dem Ausgleich auf dem Sample, Punch, Runden am Null-Ausgang gleich Zahl der Takes, Comp-Render mit Undo; Modelltests (Undo, Redo, schlechte Eingaben, Format) und Zustandstests der Oberfläche.

## 0.16.0 — 2026-10-30

Paket 21 aus M6: Schnitt und Fades.

- **Modell:** Jeder Clip hat jetzt eine Kurve für Ein- und Ausblendung (Linear, Gleiche Leistung, S-Kurve) und eine Clip-Lautstärke in dB. Neue Befehle `SetClipFadeCurves` und `SetClipGain`, je ein Undo-Schritt; ungültige Werte werden abgelehnt. Projektformat 3 wird nur geschrieben, wenn ein Clip Gain oder eine andere Kurve als linear hat; alle anderen Projekte bleiben Format 2, ältere Dateien laden unverändert.
- **Engine:** Die Blenden folgen der gewählten Kurve, die Clip-Lautstärke wirkt vor der Spur. Überlappen sich zwei Clips auf einer Spur, blendet die Engine automatisch über (Gleiche Leistung über die Länge der Überlappung, wenn sie länger ist als die eigenen Blenden); der Offline-Render nutzt dieselbe Regel. Schneiden gibt bei Positionen auf ganzen Samples bitgleiches Audio; bei krummen Tempi kann ein Sample Versatz bleiben (getestet).
- **Oberfläche:** Der Griff am oberen Clip-Rand zieht die Ein- und Ausblendung (ein Undo-Schritt je Ziehen), die Blende wird mit ihrer Kurve gezeichnet. In der Clip-Leiste: Kurvenwahl je Seite, Clip-Lautstärke und „Normalisieren“ (Spitze des Clip-Teils auf −1 dBFS). Überlappungen zeigen die automatische Überblendung. Die Wellenform ist mit der Clip-Lautstärke skaliert.
- **Agent:** neue Werkzeuge `split_clip`, `set_fade_curves` und `set_clip_gain`, beschrieben in `docs/agent.md`.
- **Tests:** Nulltest nach dem Schneiden (zusammengesetzt gleich Original), Schneiden bei krummem Tempo, Mitte löschen; Kurventabellen und Monotonie; Überblendung gemessen (RMS bleibt bei Gleicher Leistung, bei linear sinkt sie); Klick-Erkennung; Clip-Gain; alles unter `assert_no_alloc`. Modell, Zustand, Agent über MCP und ein Oberflächentest ohne Fenster und ohne Ton (Blende ziehen, Strg+Z).

## 0.15.0 — 2026-10-30

Paket 20 aus M6: Timeline-Editor.

- **Oberfläche:** Die Arrangement-Ansicht hatte schon Lineal, Wellenformen, Zoom, Verschieben, Kürzen und Löschen; neu sind Raster, Kopieren und Einfügen, Verdoppeln, das Hineinziehen von Audiodateien und ein Lineal in Sekunden. Das Raster (Aus, Takt, 1/4, 1/8, 1/16) gilt für Verschieben, Kürzen und Einfügen. Strg+C, Strg+X und Strg+V kopieren, schneiden aus und fügen am Abspielkopf ein (auf die gewählte Spur, wenn der Spurtyp passt), Strg+D verdoppelt direkt hinter der Auswahl. Audiodateien lassen sich aufs Fenster ziehen: die erste landet auf der Spur unter dem Zeiger an der Zeigerposition, jede weitere auf einer neuen Spur an derselben Stelle. Der Schalter „Takte/Sekunden“ stellt die Lineal-Beschriftung um; die Teilung folgt dem Zoom.
- **Zustand:** `AppState::copy_clips`, `paste_clips` und `duplicate_clips`, jeweils ein Undo-Schritt, neue Ids, Spurart wird geprüft. `WoaApp::without_audio` baut die Oberfläche ohne Audiogerät für Tests.
- **Tests:** Modelltests für Verschieben, Trimmen, Spurwechsel, Kopieren, Einfügen und Verdoppeln mit Undo und Redo; Rundlauf Speichern und Laden; Oberflächentest ohne Fenster und ohne Ton (zwei Dateien ablegen, Clip ziehen, Rand ziehen, Strg+Z, Strg+Y, Strg+D, Speichern, neu öffnen); Offline-Render mit Impuls: Verschieben versetzt um genau die Beats in Samples, Kürzen am Anfang lässt das Audio an derselben Stelle, Undo stellt das Sample wieder her. Teilung des Lineals und Raster als Tabellentests.

## 0.14.0 — 2026-10-30

Paket 19 aus M5: Analyse-Fenster.

- **Oberfläche:** Der Knopf „📊 Analyse“ im unteren Dock öffnet ein schwebendes Fenster mit Lautheit (momentan, kurzzeitig, integriert), True Peak, Spektrum mit Halte-Marken, Spektrogramm, Stereo-Scope und Korrelationsanzeige. Die Quelle ist der Master oder eine einzelne Spur; „Zurücksetzen“ beginnt die integrierte Messung neu. Ist das Fenster zu, kostet es im Audio-Thread nur einen Atomic-Lesezugriff je Block.
- **Messung (`woa_dsp::analysis`):** Lautheit nach BS.1770/EBU R128 (K-Filter, Blöcke mit 75 % Überlappung, absolutes und relatives Gate), True Peak mit vierfacher Überabtastung, Spektrum mit Hann-Fenster und `realfft` (MIT/Apache), Oktavbänder, Spektrogramm, Korrelation, Balance und Seiten-zu-Mitte-Verhältnis. Der K-Filter der Engine liegt jetzt dort und wird von `woa_engine::loudness` nur noch weitergereicht.
- **Engine:** Neue Abgriffe (`woa_engine::tap`) am Master, an jedem Bus und an jeder Spur: lock-freier Ring (`rtrb`), Frames bleiben als Paar zusammen, ein voller Ring zählt Verluste statt zu warten. Das Signal bleibt bitgleich.
- **Agent:** `analyze` meldet zusätzlich True Peak, höchste momentane und kurzzeitige Lautheit, Spektrum-Spitze und Oktavbänder, Korrelation, Balance und Hinweise; mit `track` misst es eine Spur allein. Neues Werkzeug `spectrogram_png` schreibt ein Spektrogramm als PNG in den Projektordner.
- **Tests:** EBU-Referenzsignale (1 kHz bei −23 und −33 dBFS ergibt −23 und −33 LUFS), Sinus landet auf dem richtigen Bin mit richtigem Pegel, Korrelation +1 bei Mono und −1 bei Gegenphase, Snapshot des Spektrogramms (`insta`), Abgriffe unter `assert_no_alloc`, Agent über MCP einschließlich PNG-Prüfung.

## 0.13.0 — 2026-10-30

Paket 18 aus M5: Automation und Sidechain (Sidechain für Effekte auf Bussen und Live-Einfahren von Sends und Effekt-Reglern folgen später).

- **Modell:** Jede Spur kann je Ziel (Lautstärke, Pan, Send, Effekt-Regler) eine Automationskurve mit Betriebsart Lesen, Schreiben oder Latch haben (`Command::SetAutomation`). Kompressor und Gate nehmen ein Sidechain-Signal einer anderen Spur (`Command::SetInsertSidechain`); Kreise über Sidechains werden abgelehnt, Spur löschen trennt Sidechains im selben Undo-Schritt. Projektformat 3 wird nur geschrieben, wenn Automation oder Sidechain vorkommt; alle anderen Projekte bleiben Format 2, ältere Dateien laden unverändert.
- **Engine und DSP:** Kurven laufen im Audio-Thread sample-genau (Blockgröße ohne Einfluss, Loop-Sprung exakt, bei Stopp gilt der Wert am Cursor). Lautstärke, Pan (auch Balance am Bus) und Sends rechnen die Kurve pro Sample (dB-Interpolation), Effekt-Regler folgen in 64-Sample-Schritten. Nur wo eine Kurve spielt, tauscht der Graph die Knoten; Projekte ohne Automation klingen bit-gleich wie vorher. Kompressor und Gate einer Spur nehmen einen Sidechain-Schlüssel (Signal der Quelle nach Fader, vor Pan); jeder dynamische Effekt hat einen Absenkungsmesser (`ProjectGraph::reductions`). Kurven im Modus Schreiben oder Latch spielen nicht ab.
- **Agent:** neue Werkzeuge `add_automation` (Kurve für Lautstärke, Pan, Send oder Effekt-Regler, ein Undo-Schritt), `remove_automation` und `set_sidechain` (Kompressor oder Gate misst den Pegel einer anderen Spur; Schleifen werden abgelehnt). Beschrieben in `docs/agent.md`.
- **Oberfläche:** Neuer Reiter „Automation“ im Studio-Dock: Kurve einer Spur für Lautstärke, Pan, Sends und Effekt-Regler zeichnen (Klick setzt, Ziehen verschiebt, Rechtsklick löscht, ein Undo-Schritt je Änderung), Betriebsart Lesen, Schreiben oder Latch. Der Lautstärke-Regler folgt beim Abspielen der Kurve. Kompressor und Gate haben die Auswahl „Sidechain“ (nur Spuren) und eine Absenkungs-Anzeige. Controller-Fahrten (Fader und Pan, per MIDI-Lernen gebunden) werden in den Modi Schreiben und Latch aufgenommen und beim Stopp als eine Kurve eingetragen; währenddessen bleiben fester Reglerstand und Undo-Verlauf unberührt.
- **Tests:** Rampe mit drei Punkten gegen die dB-Rechnung Sample für Sample in Blöcken zu 1, 128 und 1000; Pan-Fahrt; Send-Kurve; Regler-Kurve (+12 dB); Loop-Sprung; Stopp am Cursor; Ducking per Sidechain gemessen (Musik mehr als 6 dB leiser, Absenkungsmesser schlägt aus); alles zusammen unter `assert_no_alloc`. Controller-Fahrt über einen virtuellen MIDI-Port wird zur Kurve, der Offline-Render folgt ihr.

## 0.12.0 — 2026-10-30

Paket 17 aus M5: Alle eingebauten Effekte.

- **Neue Effekte:** Gate, De-Esser, Hall, Echo (frei oder tempo-synchron, mit Ping-Pong), Chorus, Sättigung (weich, Röhre, hart) und Rauschunterdrückung (RNNoise über `nnnoiseless`, BSD-3, nur bei 48 kHz, 30 ms Verzögerung). Der EQ hat jetzt Tiefen- und Höhenregal und eine zweite Glocke; die neuen Bänder stehen flach, alte Projekte klingen unverändert.
- **Oberfläche:** „+ Effekt“ bietet alle zehn Arten und die Vorlage „Sprachkette“ (Hochpass-EQ, Gate, EQ, Kompressor, De-Esser) als einen Undo-Schritt. Jeder Effekt hat Voreinstellungen („Preset ▾“, ein Undo-Schritt) und Auswahllisten für Notenwert, Ping-Pong und Kennlinie.
- **Modell:** `EffectKind` mit zehn Arten, Auswahl-Regler (nur ganze Werte), Voreinstellungen je Effekt, Vorlage `VOICE_CHAIN`; `Insert::with_values` und `Insert::with_preset`. Projektformat bleibt 2, alte Dateien füllen neue Regler mit Vorgaben auf.
- **Engine:** Der Graph gibt Effekten das Projekttempo (synchrones Echo) und rechnet im Export (`render_project`, `render_clips`) die Rauschunterdrückung ohne Arbeits-Thread, mit demselben Ergebnis wie live. Live rechnet ein eigener Thread, damit der Audio-Thread nie allokiert; zählt Aussetzer statt zu warten.
- **Agent:** neue Werkzeuge `list_effects` und `insert_voice_chain`; `insert_effect` nimmt alle Arten und ein `preset`.
- **Fehler beim Messen behoben:** Die Allpässe des Halls hatten Verstärkung über 1 (Hall rund 11 dB zu laut); die Sättigung wurde bei der Röhrenkennlinie falsch normiert und konnte über Vollaussteuerung gehen.
- **Tests:** Messungen mit erzeugten Signalen für jeden Effekt, bitgenauer Bypass, `assert_no_alloc` um jeden Process-Aufruf, Golden-File `effects_p17_chain.wav`, Sprachkette im Graphen.

## 0.11.0 — 2026-10-30

Paket 16 aus M5: Mixer mit Bussen, Sends und Inserts.

- **Oberfläche:** Im Mixer-Dock hat jeder Zug Ausgangswahl, Sends (Pegel, vor/nach Fader) und eine Effektkette mit Bypass und Reglern; „+ Bus“ legt Gruppenspuren an, Bus-Züge haben Pegelanzeige. Der Player baut den Graphen neu, wenn sich Routing oder Effektkette ändern, und schickt Reglerwerte geglättet. Spuren löschen trennt Sends und Ausgänge im selben Undo-Schritt.

- **Modell:** Bus-Spuren sind jetzt verschaltbar. Jede Spur hat einen Ausgang (Master oder Bus), Sends (vor oder nach dem Fader) und eine Insert-Kette. Alles sind Commands mit genauem Gegenstück (Rückgängig/Wiederholen); ein Bus auf sich selbst oder im Kreis wird abgelehnt. Projektformat 2, Dateien der Version 1 laden unverändert.
- **Engine:** Der Graph kennt Stereo-Nodes (zwei Ein-, zwei Ausgänge). Daraus baut `build_project_graph` das Mischpult: Insert-Kette je Spur, Ausgang auf Master oder Bus, Sends vor oder nach dem Fader, Bus-Züge mit Inserts, Fader, Balance und Pegelmesser. Solo einer Spur hält die Busse hörbar, die sie speist. Änderungen an Reglern, Sends und Effekt-Parametern gehen geglättet ohne neuen Graphen. Der Golden-Render des alten Mixes bleibt bit-gleich innerhalb 1e-5, ein neuer Golden-Render (`mixer_bus_send_05s.wav`) hält Bus, Send und Effekte fest.
- **Agent:** Werkzeugliste in `docs/agent.md` ergänzt. Neue Werkzeuge `insert_effect`, `set_param`, `set_insert_bypass`, `remove_insert`, `set_output`, `set_send`, `remove_send`; `remove_track` trennt Sends und Ausgänge im selben Undo-Schritt.
- **DSP:** Neues Modul `fx` mit EQ (Hochpass und Glocke, Überblendung beim Umstellen), Kompressor und Limiter als Mono- und Stereo-Node (verkoppelt) samt geglättetem Bypass. Biquad-EQ, Kompressor, Limiter und Gate liegen jetzt in `woa-dsp` (vorher `woa-radio`); der Radio-Bus nutzt sie von dort.

## 0.10.0 — 2026-10-30

Paket 15: Cue-Punkte und Vorhören (PFL), aus M7 nach M4 vorgezogen (André, 30.10.2026).

- **Cue-Punkte je Titel:** Cue-In, Cue-Out, Ramp (Intro-Ende), Hook und Fade-Out-Marker, in Sekunden in `radio.json`. Der Mixer spielt von Cue-In bis Cue-Out; mit Fade-Out-Marker dauert die Überblendung von dort bis Cue-Out, sonst gilt die eingestellte Überblendung. Restzeit, Gesamtlänge und Offline-Render rechnen genauso.
- **Cue-Editor:** Rechtsklick auf einen Titel → „Cue-Punkte …“. Wellenform mit allen Markern, Klick setzt den gewählten, Zahlenfelder für den Feinschliff, Prüfung vor dem Übernehmen. „Übernehmen“ ist ein Rückgängig-Schritt (eigener Verlauf für Cue-Punkte, Rückgängig/Wiederholen im Editor).
- **Ramp-Countdown:** Im Sende-Deck läuft neben der Restzeit „RAMP n“ bis zum Intro-Ende herunter.
- **Vorhören (PFL):** Einstellungen → „Vorhören (PFL)“: aus, Kanal 3/4 des Sendegeräts oder ein zweites Gerät, gespeichert in `config.json`. Titel (ab Cue-In oder ab Hook), Carts und jeder Marker im Editor lassen sich vorhören. Der PFL-Spieler ist ein eigener Spieler mit eigener `rtrb`-Queue ohne Weg in Mixer, Mitschnitt oder Sende-Ring; im Audio-Thread ohne Allokation und Sperre, 5 ms Ein- und Ausblenden gegen Knacken.
- **Agent:** neues Werkzeug `set_cue_points` (prüft die Werte, gibt die alten zurück); `run_order` zeigt Cue-Punkte und gespielte Länge.
- Engine: `start_output_aux` schreibt einen zweiten Stereo-Callback auf Kanal 3/4 desselben Geräts.

## 0.9.1 — 2026-10-30

- Neues Studio- und Sende-Design (von Gemini gebaut, von André übernommen: „gefällt mir ganz gut“): Tiefenstaffelung und Hardware-Rahmen, Status-Pillen, 20-Segment-LED-Meter; Radio-Ansicht in Modulen (Sende-Deck mit Master-Uhr, Playlist mit LUFS, Cart-Wall im Drumpad-Stil, Processing-Rack).
- Studio: unteres Dock mit Mischpult (Kanalzüge mit M/S/Aufnahme/Löschen, LED-Meter, Gain, Pan) und angedockter Piano-Roll (lösbar als Fenster); Solo und Pan im Spurkopf, Kontextmenü (Umbenennen, leerer MIDI-Clip, Audio importieren, Spur löschen); Tempo im LCD editierbar, Loop mit Klammer im Lineal; Audio importieren, leere MIDI-Clips per Doppelklick.
- MIDI-Learn (Paket 14) bleibt an Start, Stopp, Mikro, Pegeln und Stumm erhalten.

## 0.9.0 — 2026-10-30

Paket 14 aus M4 (#5): Controller und MIDI-Learn.

- **Controller wählen:** Einstellungen → „Controller und MIDI-Learn …“. Ein zweiter MIDI-Eingang neben dem Keyboard, mit Hotplug wie dort (abziehen, wieder einstecken, verbindet nach höchstens 2 s neu).
- **MIDI-Learn per Rechtsklick:** Spur-Fader (dort auch Pan und Solo), Stumm-Knopf, Start/Weiter, Stopp, Mikro, die neuen Pegelregler und jede Cart-Taste haben „MIDI lernen“. Die nächste Bewegung oder das nächste Pad am Gerät wird gebunden; eine Quelle steuert genau ein Ziel. Das Layout (Produzent/Radio) lässt sich im Controller-Fenster auf einen Taster legen. Das Fenster zeigt die Liste (Quelle, Ziel) mit „Löschen“.
- **Gespeichert je Gerät** in `config.json` (nicht im Projekt), unter dem Gerätenamen ohne ALSA-Nummern: Nach Neustart oder Wieder-Einstecken an anderem USB-Port gilt dieselbe Zuordnung.
- **Takeover:** Ein Fader springt nicht. Er greift erst, wenn er den Wert auf dem Bildschirm erreicht oder überquert; stellt die Maus den Wert um, wird neu abgeholt. Spur-Fader und Pan gehen nach 300 ms Ruhe als ein Undo-Schritt ins Projekt, nicht je CC.
- **Pads lösen Carts aus** (zweites Drücken hält an, wie der Klick), Taster als CC lösen nur beim Drücken aus. **LEDs:** Hat das Gerät einen MIDI-Eingang, bekommen Pads Note-Out (127 an, 0 aus) für laufende Carts, offenes Mikro, laufende Sendung, stumme oder Solo-Spuren und das aktive Layout; gesendet wird nur bei Änderung.
- **Pegelregler im Radio-Layout:** Musik, Carts und Mikro von −60 dB (aus) bis +6 dB, im Mixer geglättet (kein Zipper beim Fader). Sie werden nicht gespeichert und starten bei 0 dB.
- **Echtzeit:** Die Controller-Meldungen gehen im MIDI-Thread in eine eigene `rtrb`-Queue (1024 Plätze), die Oberfläche leert sie je Bild und fasst mehrere Werte eines Faders zum letzten zusammen. Der MIDI-Thread weckt die Oberfläche sofort, ein Pad startet den Cart ohne auf das nächste Bild zu warten. Der Audio-Callback bekommt nur die vorhandenen Mixer-Befehle; neu ist dort `SetLevel`, ohne Sperre und Allokation.

## 0.8.0 — 2026-10-30

Paket 13 aus M4 (#5): Piano-Roll.

- **Editor für MIDI-Clips:** Doppelklick auf einen MIDI-Clip öffnet die Piano-Roll. Klick ins Leere zeichnet eine Note (einen Rasterschritt lang), Ziehen verschiebt in Zeit und Tonhöhe, der rechte Rand ändert die Länge, Doppelklick oder Entf löscht. Mehrfachauswahl mit Strg/Umschalt+Klick, Rahmen ziehen oder Strg+A. Alt beim Ziehen schaltet das Raster ab, Esc schließt.
- **Raster und Quantisieren:** 1/4 bis 1/32, auf Wunsch Triolen. „Quantisieren“ (oder Q) setzt die Einsätze der Auswahl, ohne Auswahl aller Noten, aufs Raster; die Längen bleiben, das Pedal bleibt, wo es war.
- **Velocity-Leiste** unter den Noten: senkrecht ziehen setzt die Velocity der Auswahl (sonst der nächsten Note). Die Helligkeit der Noten zeigt die Velocity.
- **Klaviatur** links mit Notennamen (C4 = 60). Klick hört die Taste vor, solange gedrückt, über das Live-Instrument der Spur; ohne aktiven Ausgang bleibt es still. Zoom waagerecht (Regler oder Strg+Mausrad), Zeilenhöhe, Umschalt+Mausrad blättert.
- **Alles über `SetNotes`:** Jede Bearbeitung ist ein Befehl und ein Undo-Schritt, Strg+Z/Strg+Y gehen sie einzeln zurück. Die Bearbeitungen selbst (`add_note`, `move_notes`, `resize_notes`, `delete_notes`, `set_velocity`, `quantize`, `grid_beats`) liegen in `woa_model::MidiData`, damit Oberfläche und Agent denselben Code nutzen.
- **Agent:** neue Werkzeuge `quantize_notes` (Raster, Triolen, Enden, nur bestimmte Noten) und `edit_notes` (Notenliste ersetzen: verschieben, Länge, Velocity, löschen; Pedal bleibt).
- Fotomodus: `WOA_PIANOROLL=1` öffnet den ersten MIDI-Clip, `WOA_THEME=hell` schaltet zur Probe auf das helle Thema (nur Fenster und Piano-Roll, die Zeitleiste bleibt dunkel).
- Tests: Bearbeitungsbefehle (Raster, Quantisieren mit und ohne Auswahl und Enden, Verschieben als Ganzes mit Grenzen, Auswahl folgt der neuen Reihenfolge, Länge, Löschen, Velocity), sechs Bearbeitungen als einzelne Undo-/Redo-Schritte, Offline-Render vor und nach dem Quantisieren (Einsätze vorher daneben, nachher höchstens 48 Samples am Raster), Agent-Loop über MCP mit Fehlerfällen und Undo.

## 0.7.2 — 2026-10-30

- Konzept: Lücken im Sendestudio aufgenommen – Cue-Punkte je Titel (Cue-In/-Out, Ramp, Hook, Fade-Out), Vorhören (PFL) auf eigenem Ausgang, Stundenuhr mit AutoDJ-Rotation und festen Zeit-Events, Voice-Tracking. Eingeplant in M7 (Bibliothek in SQLite, Cue-Punkte, PFL) und M9 (Automation, Voice-Tracking, Gäste).

## 0.7.1 — 2026-10-30

- MIDI-Import: Noten ohne „Aus“ zählen gegen die Obergrenze mit. Eine präparierte Datei nur aus Note-Ons konnte die Grenze umgehen und den Speicher füllen; das Auflösen ist jetzt FIFO mit `VecDeque` statt `remove(0)` (Fund der Sicherheitsprüfung).

## 0.7.0 — 2026-10-30

Paket 12 aus M4 (#5): MIDI aufnehmen und als Noten speichern.

- **MIDI-Clips im Projekt:** Ein Clip kann jetzt Noten (Beginn, Länge, Tonhöhe, Anschlag) und Controller (Haltepedal CC 64) tragen statt einer Audiodatei, gespeichert in `project.json` (Feld `midi`, alte Projekte laden unverändert). Neuer Befehl `SetNotes` ändert den Inhalt als ein Undo-Schritt; beim Laden und vor jedem Befehl werden die Noten geprüft (Tonhöhe, Anschlag, Länge, Obergrenze).
- **Aufnahme auf der scharfen MIDI-Spur:** Aufnahme-Knopf wie bei Audio. Jedes Event bekommt im MIDI-Thread die Zeit, eine Sample-Uhr des Audio-Threads (Seqlock, ohne Sperre und Allokation im Callback) rechnet sie auf das Sample genau in Transportposition um, unabhängig von der Blockgröße. Stopp legt einen Clip „MIDI-Take“ an (ein Undo-Schritt); ohne MIDI-Spur entsteht eine neue.
- **Abspielen und Mixdown:** Die MIDI-Spur spielt ihre Clips sampelgenau mit dem Spur-Synthesizer (16 Stimmen), zusammen mit dem Live-Keyboard. Springen und Stopp schicken „alle Noten aus“, nichts hängt. Der Export (WAV, FLAC, MP3) und der Agent-Export enthalten die MIDI-Spuren. Änderungen an Noten bauen den Graphen neu.
- **`.mid` importieren und exportieren:** Datei → „MIDI importieren (.mid) …“ legt den Inhalt als Clip am Kopf an, „MIDI-Clip exportieren (.mid) …“ schreibt den ausgewählten Clip. Eigener Leser und Schreiber (`woa_io::smf`) für Standard-MIDI-Dateien Format 0 und 1: Running Status, Note On mit Velocity 0, Tempo, Spurname, SysEx und Meta werden verstanden; geschrieben wird Format 0 mit 480 Ticks je Viertel. Kein `midly`, weil dessen Lizenz (Unlicense) in #5 noch offen ist.
- **Agent:** `generate_notes` mit `midi: true` legt auf einer MIDI-Spur einen bearbeitbaren MIDI-Clip an statt einer WAV-Datei. Neues Werkzeug `read_notes` liest Noten und Pedal eines MIDI-Clips zurück.
- `CHANGELOG.md` ohne die versehentlich mitgelieferten Konfliktmarken zwischen 0.6.2, 0.6.1, 0.6.0 und 0.5.2.
- Tests: Modell-Roundtrip, `SetNotes` mit Undo/Redo, ungültige Noten abgelehnt; Wiedergabe unter `assert_no_alloc` mit Einsatz höchstens 2 Samples daneben, Offline-Export mit Einsatzprüfung, Stopp und Springen ohne hängende Note; virtueller ALSA-Port spielt ein festes Muster mit Pedal, aufgenommen höchstens 1 ms daneben (gemessen: 0,2 ms); `.mid`-Roundtrip und eine von Hand nach der Spezifikation geschriebene Fremddatei (Format 1, Tempospur, Running Status); Agent-Loop über MCP (Noten schreiben, lesen, exportieren, Fehlerfälle, Undo).

## 0.6.2 — 2026-10-30

- CLAUDE.md: Lokal laufen nur clippy und die Tests der geänderten Crates, die volle Testsuite nur in der CI (Pflicht auf main). André: „ja, mach beides“.

## 0.6.1 — 2026-10-30

- Bauen ohne volle Debug-Infos: eigener Code mit `line-tables-only` (Backtraces mit Zeilen bleiben), Abhängigkeiten ganz ohne. Kleinerer Bauordner, schnelleres Linken, weniger RAM auf dem 7-GB-Notebook. André: „kann man das nicht effizienter???“

## 0.6.0 — 2026-10-30

Paket 11 aus M4 (#5): Keyboard spielt live.

- **MIDI-Eingang** (`woa_io::midi`, mit `midir` 0.11, MIT): Eingänge auflisten, verbinden, alle 2 s neu abfragen. Wird das Gerät abgezogen, trennt WOA die Verbindung, schickt „Pedal los“ und „alle Noten aus“ (nichts hängt) und verbindet von selbst wieder, sobald es zurück ist, auch mit neuer ALSA-Nummer. Log-Zeile je Auswahl, Verbindung, Trennung und Fehler (gleiche Fehler nur einmal).
- **Parser** für Note On/Off (Note On mit Velocity 0 = Note aus), Anschlagstärke, Haltepedal (CC 64), Pitchbend, Program Change und All Notes Off (CC 120/123); versteht Running Status, überspringt SysEx und Echtzeit-Bytes.
- **Scharfe MIDI-Spur spielt:** Neuer Knopf „+ MIDI-Spur“. Die erste scharf geschaltete MIDI-Spur (sonst die ausgewählte, wenn sie eine MIDI-Spur ist) bekommt ein Live-Instrument (Synthesizer, 16 Stimmen) und klingt durch Fader, Pan, Mute und Solo der Spur, auch ohne ▶. Die Events gehen über die `rtrb`-Queue in den Audio-Thread, dort ohne Sperre und ohne Allokation.
- **Synthesizer** versteht jetzt Haltepedal und Pitchbend (±2 Halbtöne), der SoundFont-Player reicht beides weiter. Der Sampler ignoriert beides noch.
- **Einstellungen → MIDI-Eingang:** Aus oder ein Gerät, gespeichert in `config.json` (`midi_input`). Ein gewähltes, nicht eingestecktes Gerät steht mit „(nicht verbunden)“ in der Liste.
- **MIDI-Lampe** rechts in der Kopfzeile: grau = aus oder verbunden in Ruhe, rot = gewählt, aber nicht da, grün bei jedem Event. Der Tooltip nennt Gerät und gespielte Spur.
- Tests: Parser mit Bytefolgen; virtueller ALSA-Port spielt eine leise und eine laute Note mit Pedal in die Instrument-Node (Offline-Render unter `assert_no_alloc`: nicht still, laut mehr als dreimal so laut wie leise, Pedal hält, Pedal los gibt frei), schließt den Port (Status „wartet“, keine hängende Note) und öffnet ihn wieder (verbunden, Events kommen an); Live-Instrument im Projekt-Graphen klingt ohne laufenden Transport.
## 0.5.2 — 2026-10-30

- **Dauertest: Bündel zu später Blöcke behoben.** Ursache: `broadcast-ui` rechnete den Mixer und dekodierte Titel in derselben Schleife. Läuft die Playlist im Kreis, wird der Titel beim Neustart neu dekodiert (rund 250 ms für 3 min MP3), und der Takt stand so lange still. Belegt mit Zeitstempeln: alle 25 späten Blöcke im Probelauf fielen auf t = 197,8 s, das Ende des 197,6-s-Titels, unmittelbar nach „decode_ms=255“; die Verspätung fiel von 296 ms in 11,6-ms-Schritten (ein Block) ab. Jetzt läuft der Mixer wie in der Oberfläche in einem eigenen Takt-Thread, der nur rechnet; Dekodieren, Titel und Bericht macht der Steuer-Thread. Jeder späte Block und jeder Neustart der Playlist steht als `ui-log` auf stderr, mit Unix-Zeit (Abgleich mit dem Journal), Prozess-ID (erste oder neu gestartete Oberfläche) und Sekunden seit Start; dazu ein Hinweis, wenn der Steuer-Thread über 100 ms still stand, und bei späten Blöcken Seitenfehler und Auslagerung des Prozesses. Probe nach dem Umbau: zwei Läufe à 10 min gegen lokalen Icecast, beide bestanden (0 zu spät, 0 verloren, 0 Puffer leer, 0 Neuverbindungen, Notfall nach 2009 und 2113 ms), obwohl der Steuer-Thread bei jedem Neustart der Playlist 315–382 ms stand.
- Nicht geklärt: Im ersten 10-min-Probelauf nach dem Umbau stand der ganze Rechner rund 5 s (Oberfläche und Prüfprozess gleichzeitig, Nutzer-Journal von 07:16:30 bis 07:18:32 leer, dann viele Timer auf einmal). Vermutung: Ruhezustand oder Speicherdruck des Notebooks, nicht der Mixer. Die neuen Logzeilen trennen das beim nächsten Mal.
- **CI-Test wackelt nicht mehr unter Windows (#66).** `broadcast_test_survives_ui_crash_against_icecast` prüft die Logik (Notfall übernimmt binnen 2,5 s, Oberfläche übernimmt wieder, keine verlorenen Samples, keine Neuverbindung, Bytes beim Server). Zu späte Blöcke und leere Sendepuffer auf dem geteilten Runner werden nur gemeldet; die Grenzen im Code und die strenge Zählung im echten Dauertest bleiben.

## 0.5.1 — 2026-10-30

- `cargo build -p woa-headless` ohne Feature `device` baut wieder. `RADIO_BUFFER_FRAMES` stand im Modul `device`, das ohne Feature fehlt; die Konstante liegt jetzt im Wurzelmodul von `woa-engine`, `device` reicht sie weiter. Gefunden beim Bau von Paket 10.

## 0.5.0 — 2026-10-29

Paket 10 aus M3 (#4): Dauertest der Sendung.

- **`woa-headless broadcast-test`**: Sendeprozess (`woa-streamd`-Kern) und Oberfläche als eigener Kindprozess (`broadcast-ui`). Die Oberfläche lässt den Sendemixer samt Sendebus-Processing im Takt der Uhr laufen, ohne Audiogerät; es wird nichts hörbar abgespielt. Die Playlist läuft im Kreis. Nach der Hälfte der Zeit (`--kill-ui-at`) wird die Oberfläche hart beendet. Der Test misst, wann die Notfall-Playlist übernimmt (Grenze 2,5 s), startet die Oberfläche neu und misst die Rückgabe. Gezählt werden zu spät gerechnete Blöcke, verlorene Bytes auf der lokalen Verbindung, leere Sendepuffer bei verbundener Oberfläche und neue Verbindungen zum Server. Bestanden nur, wenn alles null ist. Eine Fortschrittszeile je Minute, am Ende ein Bericht (`--report bericht.json`). Exit 0 = bestanden, 5 = Fehler gezählt, 3 = Kennwort abgelehnt. Vorgabe 8 Stunden.
- **Icecast als Server** (`protocol: "icecast"`, `mount: "/stream"` in `stream.json`): Quelle per HTTP `PUT` mit Basic-Auth (`source:<kennwort>`), das Kennwort steht nur im Anfragekopf. Titel und Hörerzahl gibt es bei Icecast noch nicht. Shoutcast bleibt Vorgabe.
- **`live_underruns`** in `streamd-status.json`: wie oft der Sendeprozess bei verbundener Oberfläche keinen Sendemix im Puffer hatte.
- **`tests/broadcast/icecast.sh start|stop`**: lokaler Icecast im Container (`libretime/icecast:2.4.4`, nur 127.0.0.1:8000) mit Wegwerf-Kennwörtern im Laufzeitordner.
- Tests: `broadcast-test` gegen einen Mini-Icecast im Test (14 s, Absturz der Oberfläche nach 5 s, Notfall unter 2,5 s, Rückgabe, keine Unterläufe, kein Neuverbinden) und mit falschem Kennwort; Base64 nach RFC 4648.

## 0.4.0 — 2026-10-29

Paket 9 aus M3 (#4): Sendebus-Processing.

- **Sendebus mit Lautheitsregelung, Kompressor und Limiter** (`woa_radio::dynamics::BusProcessor`). Die Regelung misst die Lautheit am Sendeausgang (K-Filter nach BS.1770, gleitend über etwa 3 s) und führt die Verstärkung langsam zum Ziel aus `radio.json` (`target_lufs`, Vorgabe −16 LUFS): höchstens 3 dB je Sekunde, Bereich ±12 dB. In Pausen unter −45 LUFS bleibt sie stehen und zieht kein Rauschen hoch. Danach folgt ein RMS-Kompressor (2,5 : 1, Schwelle 4 dB über dem Ziel, weiches Knie) und ein Limiter bei −1 dBFS mit sofortigem Angriff und harter Sicherung. Der Limiter hat keine Vorausschau und deshalb keine Latenz; Uhr, Mitschnitt und Sendung bleiben sample-genau. Sendung, Mitschnitt und Abhören bekommen denselben Faktor.
- **Mikrofon-Kette** (`MicChain`): Hochpass 80 Hz, Präsenz +3 dB bei 3 kHz, Gate ab −50 dBFS (schließt um 30 dB, 100 ms Haltezeit), Kompressor 4 : 1 ab −24 dBFS mit +6 dB Aufholverstärkung. Der De-Esser kommt in M5.
- **Schalter und Anzeige im Radio-Modus** unter den Reglern: „Sendebus“ und „Mikro-Kette“, beide an (gespeichert in `radio.json` als `bus_processing` und `mic_processing`). Daneben stehen die Kurzzeit-Lautheit am Ausgang, die Verstärkung der Regelung und die Absenkung durch Kompressor und Limiter, bei offenem Mikro „Gate offen/zu“. Das Agent-Werkzeug `build_playlist` meldet beide Schalter mit.
- Alles im Audio-Thread ohne Allokation (Tests unter `assert_no_alloc`). `radio-render` läuft mit Processing; die Spitze bleibt unter −1 dBFS.
- Tests: Limiter nie über der Decke bei +6 dBFS mit harten Sprüngen; leises und lautes Material landet bei −16 ± 1 LUFS; Regelung steht in Stille; Gate schließt bei Rauschen und öffnet bei Sprache; Hochpass nimmt Rumpeln, Kompressor verkleinert 20 dB Eingangsunterschied auf 3–12 dB; Sendemix mit lautem Titel, Cart und Mikro bleibt unter −1 dBFS und nach 10 s bei −16 ± 2 LUFS.

## 0.3.6 — 2026-10-29

- **Programme in der Quellenliste (Fixes #44).** Belegt am 29.10.: WOA listete seine eigenen Wiedergabe-Streams als „Programm: woa“, zweimal mit demselben Schlüssel. pipewire-alsa nennt sie „PipeWire ALSA [woa]“ und meldet keine Prozess-ID, deshalb griff der Filter über die eigene PID nicht. Chromium meldet sich laut WirePlumber als `application.name = Chromium` und wurde vom Namen her richtig erkannt. Warum es am 29.10. fehlte, ließ sich ohne laufenden Chromium-Stream nicht nachstellen (Wiedergabe auf dem Notebook ist tabu). Vermutung: Der Stream hatte keinen bekannten Ausgang (`sink_index`) und fiel stumm aus der Liste. Widerlegt sind: Die Liste wird nur beim Start gelesen (sie wurde jede Sekunde neu gelesen). Pausierte Streams werden ausgefiltert (es gab keinen Filter). Die Abfrage scheiterte (keine Warnung im Log).
- **Neu:** Ein Stream ohne bekannten Ausgang bleibt in der Liste und nimmt den ersten Ausgang. Eigene Streams erkennt WOA zusätzlich am Programmnamen. Jedes Programm steht einmal in der Liste, ein laufender Stream geht einem pausierten vor. Der Name kommt aus `application.name`, sonst aus dem Programmdatei-Namen, sonst aus dem Stream. Nichtssagende Medientitel („Playback“, „ALSA Playback“) werden weggelassen, echte Titel angehängt. Beim Öffnen fragt die Auswahl sofort neu ab. Ohne Programm steht unten „Programm spielt gerade keinen Ton?“. Die Zuordnung steckt jetzt in `capture::build_sources` und ist mit Beispieldaten aus `wpctl inspect` getestet.

## 0.3.5 — 2026-10-29

- **Man sieht, was man aufnimmt (Fixes #43).** Während der Aufnahme wächst auf jeder aufnehmenden Spur ab dem Startpunkt ein roter Clip „Aufnahme läuft“ mit, darin die laufende Wellenform. Nach dem Stopp tritt an seine Stelle der fertige Clip. Die Zeitleiste wird mitverlängert, wenn die Aufnahme über das bisherige Ende hinausläuft.
- **So kommen die Punkte an:** `woa_engine::live_peaks` sammelt im Schreib-Thread der Aufnahme (cpal) bzw. im Lese-Thread (PipeWire) je 256 Frames den lautesten Betrag über alle Kanäle, dieselbe Einheit wie die Wellenform fertiger Clips. Die Werte gehen über einen lock-freien Ring (`rtrb`) an die Oberfläche; der Audio-Callback bleibt unverändert. Läuft der Ring voll (etwa drei Minuten ohne Abholen), füllt die Oberfläche die fehlenden Stellen als Stille auf, damit die Zeitachse stimmt. Die Wellenform nutzt die Abtastrate der Aufnahme, nicht die der Wiedergabe.
- **Pegel:** Die Pegelanzeigen in der Aufnahmeleiste und an der Spur gab es schon. Neu ist die Übersteuerungslampe „ÜBERSTEUERT“ neben dem Pegel, sobald ein Sample 0 dBFS erreicht; sie bleibt bis zur nächsten Aufnahme an.
- **Keine Mehrlast:** Die Oberfläche zeichnet wie bisher nur während einer Aufnahme alle 40 ms neu, in Ruhe bleibt es bei 2 Bildern je Sekunde (#57). Gezeichnet werden nur sichtbare Spalten.
- Tests: Peak-Sammlung gegen die Wellenform der Mono-Datei (Stereo, zerstückelt angeliefert), schrittweises Abholen, voller Ring; `assert_no_alloc` um das Sammeln (`crates/engine/tests/live_peaks_rt.rs`).

## 0.3.4 — 2026-10-29

- **Oberfläche ohne Dauerlast (Fixes #56). Hauptursache ist die Ereignisschleife unter Wayland.** eframe 0.35 setzt bei jedem fälligen Repaint `ControlFlow::Poll`. Unter Wayland kommt das Neuzeichnen aber erst mit dem Frame-Callback des Compositors, und ein verdecktes Fenster (etwa hinter dem Browser) bekommt keinen. Dann dreht der UI-Thread leer in `epoll` und belegt einen ganzen Kern (egui#8326, behoben erst in eframe 0.37, das rustc 1.95 braucht; hier ist 1.92 installiert). Unter Linux nimmt WOA deshalb X11 (XWayland), solange `DISPLAY` gesetzt ist. `WOA_WAYLAND=1` erzwingt Wayland. Bei gebrochener Skalierung kann die Schrift unter XWayland etwas weicher aussehen. Gemessen am 29.10. im Radio-Modus, ohne Wiedergabe und ohne Sendung, je 20 s nach 8 s Anlauf: 0.3.2 86–102 %, 0.3.4 unter Wayland 24–91 % (je nach Fensterlage), 0.3.4 als Vorgabe (X11) 1 %. `WOA_REPAINT_DEBUG=1` schreibt alle 5 s Bilder je Sekunde und Repaint-Auslöser ins Log.
- **Cover-Überblendung ohne Dauer-Repaint.** Ein Titel ohne Cover-Bild startete die Überblendung des großen Covers in jedem Frame neu. Das löste `request_repaint()` ohne Pause aus, und der UI-Thread lief mit 100 % CPU, auch ohne Wiedergabe und ohne Sendung. Unter X11 gemessen, Radio-Modus, nichts abgespielt: 0.3.2 12 %, 0.3.4 1 %. Jetzt merkt sich die Überblendung das Ziel, auch wenn es kein Bild gibt. Sie fordert nur während der 0,4 s neue Bilder an, mit `request_repaint_after(16 ms)`. Der Radio-Modus zeichnet in Ruhe nur noch zweimal je Sekunde neu (vorher zehnmal), solange ein Titel läuft fünfmal.
- **Lokale Verbindung Oberfläche → woa-streamd bleibt stehen.** woa-streamd liest mit einer Frist von 500 ms. Lief sie mitten in einem Rahmen ab, ging dessen Rest verloren. Der nächste „Kopf“ bestand dann aus Audio-Bytes, und die Verbindung riss ab. Im Log stand alle 2–10 s „ui disconnected“, im Status `ui_connected:false`, und für diese Zeit lief die Notfall-Playlist. Neu ist `link::read_frame_patient`: Ein angefangener Rahmen wird zu Ende gelesen. Der Feeder versucht es mit Backoff von 1 s bis 10 s erneut statt fest jede Sekunde.
- **Sendeprozess versucht sichtbar weiter.** Ein Verbindungsversuch zum Server dauert höchstens 5 s für alle Adressen zusammen. Vorher waren es 5 s je Adresse, mit IPv6 und IPv4 also 10 s. Jeder fehlgeschlagene Versuch zählt in `reconnects`. Neu in `streamd-status.json` ist `retry_in_seconds`. Unter dem Sende-Knopf steht dann z. B. „Server nicht erreichbar (Port 8001 zu?), neuer Versuch in 30 s“. Je Versuch gibt es eine Logzeile, der Backoff geht bis 30 s. `connection:"beendet"` steht nur nach „Sendung beenden“; der Sendeprozess gab auch vorher nicht von selbst auf.
- **Stream-Server heißt jetzt `stream.wironair.de`.** Die Vorgabe für `host` ist `stream.wironair.de:8001` (Stream 1), gehört wird unter <https://stream.wironair.de/stream>. Ohne Quelle spielt der Server eine eigene Notfall-Schleife. Feedback und Telemetrie bleiben auf `feedback.wironair.de`. Angepasst: `docs/stream.md`, `docs/abnahme.md`.

## 0.3.3 — 2026-10-29

- **Aussetzer der Ausgabe protokollieren (#51).** Der Fehlerpfad des Stroms kann im Audio-Thread laufen. Er zählt Unterläufe jetzt nur noch, je Strom getrennt (Radio, Player), statt jede Meldung zu loggen; am 29.10. waren das 682 Zeilen. Im Log steht höchstens eine zusammengefasste Zeile je Minute (`output underruns (last minute)`). Sie nennt Anzahl, Gesamtzahl, Puffergröße, Samplerate und Gerätename. Beim Radio kommen dazu: Läuft der Mitschnitt? Läuft der Player-Strom gleichzeitig? Welcher Titel spielt, in welchem Format, und wie lange dauerte das letzte Dekodieren? Die Radio-Zeile kommt jede Minute, die Player-Zeile nur bei Aussetzern. Die Radio-Statuszeile zeigt dezent „Aussetzer: N“, sobald einer auftrat.
- **Zwei kleine Gegenmaßnahmen.** Im Radio-Modus ruht der Player-Strom (`pause_output`), damit nicht zwei Ströme auf demselben Gerät spielen. Beim Wechsel zurück läuft er weiter. Die Radio-Ausgabe nimmt die Puffergröße des Treibers statt fester 128 Frames. Einstellbar ist das in `config.json` mit `radio_buffer_frames` (fehlt oder 0 = Treiber) und bei `woa-headless radio-play --buffer`. `radio-play` meldet Aussetzer und verlorene Mitschnitt-Samples getrennt. Die eigentliche Ursache wird später behoben; die Vermutungen stehen in #51.

## 0.3.2 — 2026-10-29

- **Cover-Bild im Radio-Layout.** Neben der großen Uhr steht ein Quadrat (168 px) mit dem Cover des laufenden Titels (vor dem Start: des nächsten). Beim Titelwechsel blendet es in 0,4 s weich über. Ohne Bild steht dort eine dezente Note, nie ein leeres Loch. In der Playlist hat jede Zeile ein Vorschaubild (32 px).
- **Auslesen:** `woa_engine::cover::read_cover` holt das eingebettete Bild über die symphonia-Metadaten (ID3 APIC, MP4 `covr`, FLAC PICTURE, Vorbis `METADATA_BLOCK_PICTURE`), bevorzugt „Cover vorne“. Auslesen und Dekodieren laufen in einem eigenen Hintergrund-Thread (`woa-cover`), nie im Audio-Thread; die Oberfläche holt nur fertige Texturen ab.
- **Dekodieren:** über den `image`-Crate, der schon im Lockfile war, jetzt mit JPEG (`zune-jpeg`, ebenfalls schon im Lockfile) und PNG, ohne Standard-Features. Herunterskaliert auf 512 px (groß) und 64 px (Vorschau). Vorschaubilder bleiben im Cache, große Texturen nur für den laufenden und den vorigen Titel.
- **Speicherschutz:** Bilder über 10 MB oder über 8000 px Kante werden nicht angezeigt; symphonia liest größere Bilder gar nicht erst ein.
- Tests: Cover aus MP3 (selbst gebauter ID3-Tag), FLAC, M4A und OGG (per ffmpeg, ohne ffmpeg übersprungen); Datei ohne Bild, WAV und zu großes Bild ergeben kein Cover; PNG und JPEG werden dekodiert und skaliert, zu große und kaputte Bilder abgelehnt.

## 0.3.1 — 2026-10-29

- **MP3 mit großem Cover-Tag lesbar (Fixes #50).** Eine MP3 mit 1,2 MB großem ID3v2.3-Tag (APIC-Bild) scheiterte mit „no suitable format reader found“: symphonia lief ohne die Metadaten-Features und konnte den Tag nicht überspringen. Jetzt mit `id3v1`, `id3v2` und `ape`. Test mit selbst erzeugter MP3 und 1,2 MB APIC-Bild; Andrés Datei geht wieder (8 710 592 Frames, 44,1 kHz, Stereo).
- **Alle gängigen Formate lesen.** Neu über symphonia 0.6.1 (alle Teile MPL-2.0, `extended` MIT, ohne C-Abhängigkeit): OGG/Vorbis (`.ogg`, `.oga`), M4A/MP4 mit AAC oder ALAC (`.m4a`, `.mp4`), rohes AAC/ADTS (`.aac`), AIFF (`.aif`, `.aiff`, `.aifc`), CAF (`.caf`) und MP2 (`.mp2`). Opus geht weiter nicht: symphonia hat keinen Opus-Decoder.
- **Eine Liste für alle Endungen.** `woa_engine::decode::EXTENSIONS` (klein, Vergleich ohne Rücksicht auf die Schreibweise), `FILTER_NAME` und `dialog_extensions()` für die Dateidialoge der Radio-Playlist und der Cart-Wall; Drag-and-drop und Agent-Import prüfen über dieselbe Liste. Filtertext „Audio (MP3, FLAC, WAV, OGG, M4A, AAC, AIFF, CAF, MP2)“.
- Tests: Jedes neue Format wird mit ffmpeg aus dem Testton erzeugt, gelesen und auf Länge und Pegel je Kanal geprüft; 9 Kanäle in AIFF, CAF und M4A/ALAC werden abgelehnt (Speicherschutz aus #49). Ohne ffmpeg überspringen sich diese Tests mit Meldung.
- **Patent-Hinweis AAC:** Der AAC-Decoder ist reiner Rust-Code (MPL-2.0). AAC-Patente sind nach Kenntnis weitgehend ausgelaufen, das ist aber keine Rechtsauskunft; vor einem kommerziellen Vertrieb einmal prüfen.

## 0.3.0 — 2026-10-29

- **Sendeprozess `woa-streamd` (Paket 8).** Die Sendung geht an einen eigenen Shoutcast DNAS v2. `crates/stream` enthält:
  - den Quellclient im Shoutcast-1-Protokoll (Legacy): Kennwort-Zeile (ab Stream-ID 2 mit `:#sid`), `OK2`, `icy-*`-Kopfzeilen, dann MP3. Vorgabe ist der eigene Server `feedback.wironair.de:8001`, Stream 1 (bretteins.infra#150)
  - Titel per `admin.cgi?mode=updinfo` nur bei lokalem Server, weil dort das Kennwort im Klartext in der URL steht; gegen den eigenen Server vorerst keine Titel (v1 kann sie nicht in der Quellverbindung tragen). Hörerzahl aus `/7.html` nur mit `stream.admin_port`; der ist auf dem eigenen Server von außen gesperrt. Die Quellanmeldung auf 8001 läuft im Klartext (bekanntes Risiko, `docs/stream.md`)
  - Wiederverbinden mit Backoff (1 s bis 30 s, nach abgelehntem Kennwort 60 s); während des Neuaufbaus werden höchstens 5 s gepuffert
  - MP3 CBR 128 kbit/s über LAME, 44,1 oder 48 kHz
  - die Notfall-Playlist: Nach 2 s ohne Ton von der Oberfläche spielt woa-streamd selbst Dateien oder Ordner aus der Konfiguration (WAV, MP3, FLAC über `woa_engine::decode`), endlos. Kommt die Oberfläche zurück, blendet er über 1 s zurück.

  Die Oberfläche schickt den Sendemix über einen eigenen `rtrb`-Ring des Mixers (der Audio-Callback schreibt nur hinein, Test unter `assert_no_alloc`) und einen Feeder-Thread an woa-streamd. Unter Unix läuft das über einen Unix-Socket in `<datenordner>/streamd/` (Modus 0700, also nur der eigene Benutzer), unter Windows über TCP auf 127.0.0.1. Dazu kommt ein zufälliges Token in `streamd.link` (Unix: Modus 0600). Die Verbindung ist abgesichert:
  - Die Anmeldung hat feste Größe und eine Frist von 200 ms.
  - Das Token wird in konstanter Zeit verglichen.
  - Höchstens 4 Verbindungen dürfen gleichzeitig unangemeldet sein; kommt eine weitere, fliegt die älteste wartende, damit stumme Verbindungen die Oberfläche nicht aussperren.
  - Es gibt nur eine aktive Oberfläche.
  - Rahmen sind auf 1 MiB begrenzt, und die Größe wird vor jeder Allokation geprüft.

  In der Oberfläche (Radio-Modus) gibt es den Knopf „Auf Sendung“ / „Sendung beenden“ und eine Zeile mit dem Zustand (verbunden, verbinde neu, Notfall), den Hörern, der Bitrate und der Dauer. woa-streamd wird abgelöst gestartet und überlebt einen Absturz der Oberfläche; nach dem Neustart hängt sie sich wieder an.

  Neu für Agent und Probe: das MCP-Tool `stream_status` (Zustand aus `streamd-status.json`) und `woa-headless stream-test` (30 s Testton an den Server, Exit 3 = Kennwort abgelehnt, 4 = nicht erreichbar). Anleitung und Fehlerbilder in `docs/stream.md`, Klickliste in `docs/abnahme.md`.

  Das Kennwort steht nie in der Konfiguration. Es wird aus einer Datei gelesen (Vorgabe `~/.secrets/daw.woa/shoutcast-source.pass`) und erscheint weder im Log noch in `Debug`.

- Neuere Clippy-Regeln der CI (Rust 1.99) im Sendeprozess erfüllt (`as_chunks`, keine unnötigen Borrows).

## 0.2.1 — 2026-10-29

- **MP3 und FLAC lesen.** Neues Modul `woa_engine::decode`: WAV weiter über `hound`, FLAC und MP3 über `symphonia` 0.6.1 (MPL-2.0, nur Features `mp3` und `flac`). Gilt überall, wo Audio gelesen wird: Clips im Projekt und Wellenform (`load_wav_mono`), Agent-Import (`import_audio` behält jetzt die Endung der Quelle), Playlist, Cart-Wall, LUFS-Messung und `radio-render` (`woa_radio::audio::load_stereo`). Dekodiert wird nur im Control- oder Hintergrund-Thread; andere Raten rechnet WOA wie bisher bei WAV linear auf die Projektrate um. Dateidialoge und Drag-and-drop im Radio-Modus nehmen `.mp3` und `.flac` an.
- **Speicherschutz beim Lesen.** Rate (8–384 kHz) und Kanalzahl (1–8) werden vor dem Dekodieren geprüft und für jeden dekodierten Puffer wiederholt; wechseln sie mitten in der Datei, wird sie abgelehnt. Feste Obergrenze aller Samples (`MAX_SAMPLES`, 6 h Stereo bei 48 kHz), unabhängig von Angaben aus der Datei, geprüft vor jedem Anhängen; WAV prüft die angegebene Länge vor dem Lesen. Tests mit Rate 0 im FLAC-Kopf, 9 Kanälen und einem WAV-Kopf, der fast 4 GB behauptet.
- **FLAC-Export repariert.** `flacenc` schrieb die Länge des kürzeren letzten Blocks als Mindest-Blockgröße in STREAMINFO. Strenge Leser (symphonia) fanden dann keinen einzigen Frame. Der Export setzt sie jetzt auf die feste Blockgröße, wie es die FLAC-Spezifikation verlangt.
- **Radio-Layout:** Crossfade und Ducking stehen je in einer eigenen Zeile, die Einheit steht am Wert (s, dB). Bei 900 px Fensterbreite wird die Beschriftung nicht mehr abgeschnitten.
- Tests: FLAC- und MP3-Datei per `flacenc`/LAME erzeugen, lesen und mit dem Original vergleichen (Länge, Pegel je Kanal, Mono-Mix in 48 kHz); unbekannte Endung und kaputte MP3 ergeben Fehler.

## 0.2.0 — 2026-10-29

- **Radio-Modus (Paket 7: Sendeablauf).** Umschalter Produzent/Radio oben rechts, gespeichert in `config.json`. Das Radio-Layout zeigt:
  - eine große Uhr mit der Restzeit des Titels und bis zum Playlist-Ende
  - Start/Weiter (Leertaste) und Stopp
  - die Playlist, gefüllt per Dialog oder Drag-and-drop, sortierbar und mit Entfernen
  - Regler für Crossfade und Ducking
  - die Cart-Wall 4×4 (Tasten 1–9, 0)
  - Mikro (Taste M) und Mitschnitt

  Die Sendung läuft in einem eigenen Steuer-Thread weiter, auch bei minimiertem Fenster. Änderungen an `radio.json` von außen (Agent) übernimmt die App. Neu: `woa-headless radio-play` als Probe am Gerät, `woa_engine::device::start_stereo_output` und `woa_engine::record::start_live_input`.
- **Agent-Werkzeuge für den Sendeablauf.** `build_playlist` (Dateiliste ersetzen oder anhängen, jeder Titel wird gemessen) und `run_order` (Reihenfolge, Gesamtlänge mit Überblendungen, LUFS und Auto-Gain je Titel). Beide arbeiten auf `radio.json`, Pfad als Parameter oder Vorgabe im Datenordner. Geschrieben wird nur in die Vorgabe oder relativ im Projektordner (Regeln wie beim Export: kein `..`, kein absoluter Pfad, kein Symlink als Ziel).
- **Sendeablauf-Kern (`crates/radio`).** Playlist mit Crossfade konstanter Leistung (Vorgabe 3 s), Auto-Gain auf −16 LUFS (gemessen und in `radio.json` gecacht), Cart-Wall mit 16 Tasten, Mikrofon mit geglättetem Ducking (−12 dB), Mitschnitt als WAV in `<datenordner>/mitschnitte/`. Der Mixer allokiert nicht (Tests unter `assert_no_alloc`), Dekodieren läuft im Control-Thread. Neu: `woa-headless radio-render <radio.json> <out.wav>`.
- **Export schreibt in das geöffnete Handle.** `woa_engine::export::export_stereo_to` schreibt in ein schon offenes `Write + Seek`; der Agent-Export öffnet die Temp-Datei mit `create_new` und schreibt genau in dieses Handle, danach `rename`. Geht der Export schief, entfernt er die Unterordner, die er selbst angelegt hat, wieder (nur leere). Die Endung wird geprüft, bevor ein Ordner entsteht. Neuer Test: nach einem Fehler bleibt kein leerer Ordner zurück.

## 0.1.17 — 2026-10-29

- **Export dicht gegen Symlinks.** `export` prüft jetzt den tiefsten vorhandenen Ordner im Pfad kanonisch gegen den Projektordner, legt fehlende Unterordner einzeln an und prüft jeden sofort nach. Ein Symlink weiter oben im Pfad (etwa `link/neu/x.wav` mit `link` → `/tmp`) wird abgelehnt, ebenso ein Ziel, das selbst ein Symlink ist. Geschrieben wird zuerst in eine neu angelegte Temp-Datei im geprüften Ordner, die danach per `rename` an den Zielnamen kommt; ein zwischendurch untergeschobener Symlink wird so ersetzt, nicht beschrieben. Neue Tests: Export in neuen Unterordner, Symlink-Vorfahr und Symlink-Ziel (nur Unix).

## 0.1.16 — 2026-10-29

- **Paket 6 Agent (M2).** Der MCP-Server kann jetzt allein einen hörbaren Loop bauen: `generate_notes` (Noten mit dem eingebauten Synth) und `generate_drums` (Kick, Snare, Hi-Hat als 16tel-Muster, `woa_dsp::generate`) erzeugen WAV-Dateien in `aufnahmen/` und legen Audio-Clips an. Dazu `import_audio`, `move_clip`, `trim_clip`, `set_fades`, `repeat_clip`, `remove_clip`, `remove_track`, `set_solo`. `analyze` misst LUFS, Spitze, Länge und Spitze je Spur, `export` schreibt WAV/FLAC/MP3 und gleicht auf Wunsch auf eine Ziel-Lautheit an. Jedes Tool ist ein Undo-Schritt „Agent“, Fehler lassen das Projekt unverändert. `export` schreibt nur relativ in den Projektordner (absolute Pfade und `..` werden abgelehnt). Abnahmetest: 8-Takt-Loop nur über Tools, exportiert und nachgemessen −14,0 LUFS, 16 s (ffmpeg bestätigt).

## 0.1.15 — 2026-10-29

- **M0 Logs, Client-Seite.** Beim ersten Start fragt WOA, ob anonyme Fehlercodes gesendet werden dürfen (Opt-in, nur Code, Stufe, Zahl). Mit Ja gehen sie ab dem nächsten Start an `https://feedback.wironair.de/api/logs/ingest`; umschaltbar unter „Einstellungen“. `config.json` bekommt `install_id` (zufällige UUID) und `telemetry_enabled`. `woa-headless log-test` nutzt dieselbe Adresse als Standard. Server-Seite folgt in bretteins.infra.
- `docs/abnahme.md`: Klickliste M0 Fundament; `docs/telemetrie.md`: Opt-in und Datenschutz.

## 0.1.14 — 2026-10-29

- **Feedback geht an den eigenen Server.** Ohne `config.json` schickt der Feedback-Knopf an `https://feedback.wironair.de/api/feedback`, und daraus wird ein Issue in daw.woa (Gegenprobe #40). Mit `"feedback_url": ""` in der `config.json` ist er abgeschaltet.

## 0.1.13 — 2026-10-29

- `docs/andre-handgriffe.md`: Andrés erledigte Schritte (DNS, Token, Ablauf) entfernt; Ausrollen macht Claude.

## 0.1.12 — 2026-10-29

- Windows-Build wieder grün (Clippy `unused_mut` in der Quellenliste).

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
