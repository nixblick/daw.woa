# Abnahme in Paketen

André nimmt fertige Pakete ab, nicht einzelne Funktionen (29.10.2026). Claude baut ein Paket am
Stück, prüft selbst (Tests, Offline-Render, Probelauf am Gerät) und merged. Hier steht je Paket
eine kurze Klickliste. Ein „passt“ von André schließt das Paket; was hakt, wird ein Issue.

## Design D1–D6: Neues Aussehen (0.34.15, offen)

Start wie immer. Die sechs Design-Pakete (0.34.7, 0.34.10–0.34.14) werden hier zusammen abgenommen. Vorgabe ist `docs/design/woa-design-system/` (README und `tokens.json`).

1. **Thema und Modusfarbe:** Beim ersten Start ist das Fenster dunkel, im Produzent amber, in Radio rot, in Party magenta. Akzent, aktive Tasten und Leuchtziffern wechseln mit dem Modus.
2. **Hell:** Einstellungen, „Erscheinungsbild“, Hell wählen. Fenster, Menüs, Karten und Zeitleiste werden hell, Spurfarben dunkler, Clip-Namen lesbar. Uhr, Restzeit, LED-Pegel, LUFS-Anzeige, Lampe und VU bleiben dunkel mit leuchtenden Ziffern. Alle drei Modi durchklicken. Im Piano-Roll sind Raster, Noten und die weiße Auswahlkontur sichtbar.
3. **Merken:** App schließen und öffnen, Hell bleibt. Danach zurück auf Dunkel.
4. **Schriften:** Fließtext in Barlow (etwas größer als vorher), Zahlen in Analyse und Bibliothek in JetBrains Mono, Statuspillen und Überschriften in Barlow Condensed. In Radio stehen Uhr und Restzeit als 7-Segment-Ziffern; unter 10 s wird die Restzeit rot, das Minuszeichen sieht sauber aus. Beim Umschalten Hell/Dunkel ist nichts abgeschnitten.
5. **Icons:** In Radio (Tastenreihe unter den Decks), im Transport des Produzenten, im Spur-Rechtsklick, in den Dock-Kopfzeilen und in der Bibliothek stehen Phosphor-Icons statt Emojis. Play wechselt auf Pause. Alle Icons nehmen die Textfarbe an, auch in Hell. Gesperrte Knöpfe (Play ohne Audioausgabe, Vorhören) sehen blasser aus.
6. **Bausteine Produzent:** Beim Abspielen läuft in der Transportleiste das VU mit Zeiger. Erscheinungsbild, „Pegelansicht“: „Modern (Balken)“ und „Aus“ wirken und bleiben nach dem Neustart. Im Spurkopf rasten M, S und Aufnahme ein und leuchten; Fader ziehen, Doppelklick setzt 0 dB.
7. **Bausteine Radio und Party:** Die Studio-Uhr läuft mit Sekunden-Kette, ON AIR leuchtet bei Sendung oder offenem Mikro. Ein Cart-Pad leuchtet in der Modusfarbe und blinkt. Pegel-Fader ziehen, Doppelklick 0 dB, Rechtsklick öffnet weiter das MIDI-Lernen. In Party dreht sich das Jogwheel beim Spielen und steht bei Pause; die Großtasten leuchten bei aktiven Funktionen.
8. **Mixer:** Dock „Mixer“: jeder Kanalzug zeigt nur Namensstreifen, M/S/Aufnahme, LED-Kette und Fader. Klick auf den Namensstreifen färbt den Rand und öffnet rechts das Insert-Rack (Pegel, Pan, Ausgang, Sends, Effektkette, Plugins, „Spur löschen“). Rack und Fader folgen einander; Doppelklick auf Pan setzt Mitte. Effekt schalten und Send ändern verhalten sich wie vorher. Viele Spuren scrollen waagerecht, das Rack bleibt stehen.
9. **Logo:** Links oben in der Kopfleiste steht die Bildmarke (Säulen als W, rote Lampe). Die Säulen wechseln mit dem Modus, im hellen Thema sind sie dunkler. Fensterleiste, Alt-Tab und die AppImage im Dateimanager zeigen die neue Kachel. Unter Windows nach dem MSI-Installieren: Startmenü „WOA - Wir on Air“ und „Apps“ zeigen das Icon.
10. **Über WOA:** Einstellungen, „Über WOA …“ zeigt die Version, GPL-3.0-only, die OFL-Nennung der vier Schriften und Phosphor Icons (MIT).

Selbst geprüft von Claude (Code und Tests, ohne Fenster): Zuordnung der drei Modi zu den sechs Paletten; alte Einstellungen ohne Thema starten dunkel; alle zwölf Schriftschnitte aus `tokens.json` eingebunden, Absturz bei fehlenden Schriften im ersten Bild abgefangen; keine verlorenen Funktionen in Kanalzug, Rack, Cart-Menü und MIDI-Lernen; Logo-Koordinaten im Feld, `app_icon()` ohne Absturzpfad; Herkunft und Lizenzen in `docs/herkunft.md`; clippy und Tests von `woa-gui` grün, CI je Paket grün.

Nicht selbst geprüft: Die App wurde nie gestartet, es gibt keine Screenshots. Farben, Kontraste, Schriftgrößen und Layout sind nur nach Code und `tokens.json` geprüft. Ob gesperrte Icons blasser werden, ist eine Vermutung. Der MSI-Pfad zu `woa.ico` ist erst im nächsten regulären Windows-Build zu sehen.

Offen für André (noch nicht gebaut): Icon in der `woa.exe` selbst (Explorer); Drehregler (Knob) und Reglerkränze in Modusfarbe; Gast-Gain im Radio noch egui-Slider; Geister-Segmente „88:88“ hinter den Leuchtziffern; VU zeigt den höchsten Spurpegel statt Summe, ohne Peak-Hold; einzelne feste Farben in `radio.rs`, `app.rs` und `party.rs` noch ohne Token; feste Schriftgrößen (9–13 px) statt Skala; Emojis in der Handy-/Remote-Textausgabe (`crates/radio/src/party.rs`); kein Crossfader und keine PFL-Taste am Kanalzug; im dunklen Thema wirken inaktive Tasten durch `border_strong` etwas heller umrandet.

## Silvester Teil 2, Paket 2: drei Decks (0.34.4, offen, #134)

Start wie immer, Modus „Radio“. Die Decks A, B, C stehen rechts über der Cart-Wall.

1. **Laden:** Eine Musikdatei aus dem Explorer auf Deck A ziehen: Titel und Restzeit erscheinen, das Deck steht. Auf Deck B per Rechtsklick auf den Titel, „Datei laden …“. Auf Deck C per Rechtsklick auf einen Playlist-Titel, „Auf Deck laden“.
2. **Start und Stopp:** „Start“ auf Deck A: Die Restzeit läuft rückwärts, unter 10 s wird sie rot. „Stopp“: kurz ausgeblendet, die Stelle bleibt. „Start“ geht von dort weiter. „Cue“ stellt das Deck an den Anfang. Am Ende des Titels steht das Deck wieder am Anfang.
3. **Gleichzeitig:** Alle drei Decks starten, dazu Playlist und ein Cart: Alles ist zu hören.
4. **Schnitt:** Rechtsklick auf den Decktitel, „Start und Ende schneiden …“, Cue-In und Cue-Out setzen, „Übernehmen“. Das Deck springt an den neuen Anfang und spielt nur bis zum neuen Ende.
5. **Vorhören:** „Vorhören“ am Deck: nur im Kopfhörer, der Sendepegel bewegt sich nicht.
6. **Ausgänge:** Unter Einstellungen, „Ausgänge (Soundkarten) …“ Deck A auf Karte 2 Kanal 1/2 legen, Deck B auf „Wie Playlist“. Deck A kommt nur am Pult-Kanal von Karte 2, Deck B im normalen Programm.
7. **Neu starten:** Programm schließen und öffnen: Die Titel liegen noch auf den Decks, die Zuordnung ist noch da.

Die Decks laufen ohne Software-Pegel (der Pegel kommt vom Pult) und werden vom Mikro nicht abgesenkt. Sie laufen im Sendeweg mit, also in Aufnahme und Stream und im Rückkanal der Gäste.

Selbst geprüft von Claude (ohne Ton, offline durch den echten Mixer, ohne Allokation im Audio-Pfad): Restzeit zählt herunter, Start, Stopp mit gehaltener Stelle, Weiterspielen, Cue, Ende; Schnitt zwischen Cue-In und Cue-Out genau (0,75 s), Schnitt ändern bei stehendem Deck; drei Decks addieren sich im Hauptausgang; ein Deck auf eigenem Kanalpaar fehlt im Hauptausgang; Speichern und Laden der Decks, alte Dateien.

Nicht selbst geprüft: Aussehen der Decks, Ziehen von Dateien und Rechtsklick-Menüs (kein Bildschirm), echtes Gerät und Pult unter Windows.

## Silvester Teil 2, Paket 4: Wunschtitel-Ordner (0.34.9, offen, #134)

Start wie immer, Modus „Radio“. Rechts unter den Decks steht die Karte „WUNSCHTITEL“.

1. **Ordner:** Ohne Wahl legt WOA den Ordner `Wunschtitel` im Datenordner der Sendung an; sein Pfad steht in der Karte. „Ordner wählen …“ nimmt einen anderen (etwa einen Ordner, in den ein Helfer am zweiten Rechner kopiert). Die Wahl bleibt nach dem Neustart.
2. **Erscheinen:** Eine Musikdatei in den Ordner kopieren (Explorer, USB-Stick, Netzfreigabe): Sie steht innerhalb einer Sekunde ganz oben in der Liste, fett mit „neu“. Während sie noch kopiert wird, steht „wird kopiert …“ und die Knöpfe fehlen. Andere Dateien (Text, Bilder) und versteckte Dateien erscheinen nicht. Löschen aus dem Ordner nimmt sie aus der Liste.
3. **In die Playlist:** „In die Playlist“ reiht den Titel direkt nach dem laufenden Titel ein. Läuft nichts, kommt er hinter den Cue-Punkt.
4. **Aufs Deck:** „A“, „B“ oder „C“ lädt den Titel auf das Deck, es steht dann bereit zum Start.
5. **Ordner fehlt:** Ordner im Explorer umbenennen: rote Meldung mit Pfad, die Sendung läuft weiter. Wieder zurückbenennen: Die Liste füllt sich von selbst.

Es gibt keine Anbindung an Spotify oder einen anderen Dienst, nur Dateien. Die Datei bleibt im Wunschordner liegen; WOA verschiebt oder löscht nichts. Last: ein `read_dir` pro Sekunde in einem eigenen Faden und ein Neuzeichnen pro Sekunde, kaum messbar.

Selbst geprüft von Claude (offline, ohne Fenster): Die Liste zeigt nur lesbare Audiodateien, neueste zuerst, ohne versteckte Dateien und Ordner; eine frische oder leere Datei gilt als „wird kopiert“; eine neue Datei erscheint von selbst und ist neu, vorhandene nicht; fehlender Ordner wird angelegt, ein nicht lesbarer gibt eine klare Meldung; das Beenden des Beobachters hängt nicht. Das Einreihen und Laden benutzt dieselben Wege wie Ziehen auf die Playlist und Deck-Laden (Pakete 1 und 2).

Nicht selbst geprüft: Aussehen der Karte, Klicks, Ordnerwahl-Dialog, Netzfreigaben unter Windows (Verzögerung beim Erscheinen), sehr große Ordner.

## Silvester Teil 2, Paket 3: Telefon mit Rückweg (0.34.8, offen, #134)

Start wie immer, Modus „Radio“. Das Handy hängt über den Adapter am Pult; sein Signal kommt in einen Eingang einer Soundkarte, der Rückweg zum Handy geht aus Karte 1 zurück ins Pult.

1. **Eingang wählen:** Einstellungen, „Telefon (Eingang für den Anrufer) …“: Soundkarte und Kanalpaar des Handys wählen, „Übernehmen“. Fehlt die Karte, steht das in Klartext im Fenster, die Sendung läuft weiter.
2. **Rückweg wählen:** Einstellungen, „Ausgänge (Soundkarten) …“, Zeile „Telefon-Rückweg“: Karte 1 und ein Kanalpaar, das sonst nichts trägt (Pult-Kanal, der zum Handy zurückführt). „Übernehmen“. Auf dem Paar der Playlist oder einer anderen Quelle lehnt WOA ab und sagt es.
3. **Pegel:** Anrufen und sprechen. Der Pegelbalken neben dem Knopf „Telefon“ bewegt sich, auch wenn das Telefon nicht auf Sendung ist. Auf Sendung ist der Anrufer dann noch nicht zu hören.
4. **Auf Sendung:** „Telefon“ drücken: Der Knopf wird rot und heißt „Telefon auf Sendung“. Der Anrufer ist im Programm zu hören, die Musik geht runter wie beim Mikro. Nochmal drücken: Anrufer weg, Musik wieder oben.
5. **Rückweg:** Der Anrufer hört die Sendung (Musik, Moderator), aber nicht sich selbst und kein Echo. Das gilt mit und ohne „Telefon auf Sendung“.
6. **Fehler:** Stecker der Karte ziehen: rote Meldung mit Namen der Karte, die Sendung läuft weiter. Anstecken, im Telefon-Fenster „Übernehmen“.
7. **Neu starten:** Programm schließen und öffnen: Eingang und Rückweg sind noch eingestellt.

Die Laptop-Last bleibt klein: Der Eingang läuft über eine Taktverbindung mit leichtem Umrechner (Hermite), etwa 50 ms Polster; bei 44,1 gegen 48 kHz zwischen zwei Karten und beim Auseinanderlaufen der Takte wird so ohne Knackser ausgeglichen. Ein Aussetzer des Eingangs steht im Telefon-Fenster.

Selbst geprüft von Claude (ohne Ton, offline durch den echten Mixer und Router, ohne Allokation im Audio-Pfad): Rückweg trägt das Programm und nie den Anrufer; ohne Anrufer-Signal oder ohne Telefon ist der Rückweg genau das Programm; Anrufer nicht auf Sendung wird gemessen, liegt aber weder im Programm noch im Rückweg; Anrufer von einer Karte mit 44,1 kHz kommt an; ohne gewählten Rückweg erreicht nichts das Rückweg-Paar; „Telefon auf Sendung“ senkt die Musik ab; die Zuordnung lehnt ein Paar ab, das schon eine andere Quelle trägt.

Nicht selbst geprüft: Aussehen von Knopf, Pegel und Fenstern, echte Karten, das Handy am Pult (Pegelanpassung, ob das Pult den Rückweg ohne Echo führt), Latenz auf dem alten Laptop, Aussetzer bei langer Laufzeit.

## Transportknöpfe: Zurück, Spulen, Stumm, Wiederholen, Zufall (offen, #137)

Wunsch von André bei der Abnahme von Party-Modus und Fernbedienung (30.10.2026): normale Knöpfe in der App und am Handy, dazu Wiederholen und Zufall wie bei Apple Music. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`, Modus „Party“, Playlist mit mindestens drei Titeln laden (Knopf „Ordner laden …“), Fernbedienung an, Handy per QR-Code als DJ anmelden („DJ-PIN zeigen“, Adresse mit der DJ-PIN öffnen). Zum Prüfen leise stellen oder Kopfhörer nehmen.

Die Tasten stehen in drei Reihen, in der App und am Handy gleich: **Zurück · Play/Pause · Stopp · Weiter**, dann **Von vorn · −10 s · +10 s · Stumm**, dann **Leiser · Lauter · Zufall · Wiederholen**. Im Radio-Modus stehen neben „Stopp“ zusätzlich ⏮, ↺, −10 s, +10 s, Wiederholen und Zufall.

1. **Ein Knopf für Play und Pause, daneben Stopp:** Es gibt überall genau einen solchen Knopf: ▶ wenn nichts läuft oder pausiert ist, ⏸ solange gespielt wird, daneben ■ Stopp (am Handy ⏹). Das gilt im Party-Modus, im Radio-Layout (dort ist „Weiter“ als ⏭ in die Reihe daneben gewandert), im Player und am Handy; die Leertaste macht dasselbe. In der App ▶ drücken: Titel läuft, der Knopf zeigt jetzt ⏸, am Handy auch. „Pause“ (App oder Handy) blendet kurz aus und merkt sich die Stelle: oben steht „PAUSE“ statt „JETZT“, die Restzeit bleibt stehen, der Balken auch, in der App und am Handy. „Play“ macht an derselben Stelle weiter (nicht von vorn). „Stopp“ blendet aus und vergisst die Stelle: danach beginnt „Play“ den Titel von vorn. „Weiter“ während der Pause macht an der Stelle weiter, springt nicht zum nächsten Titel.
2. **Weiter und Zurück:** „Weiter“ blendet zum nächsten Titel über. „Zurück“ blendet zum vorherigen Titel. Beim ersten Titel beginnt „Zurück“ den Titel von vorn. Tasten in der App: P oder Pfeil links für Zurück.
3. **Von vorn, −10 s, +10 s:** Titel eine Weile laufen lassen, „+10 s“ springt zehn Sekunden vor (die Restzeit sinkt um 10, am Handy und in der App), „−10 s“ zurück. „Von vorn“ setzt den Titel auf den Anfang, ohne Überblendung. Am Ende des Titels hören die Sprünge an, sie springen nie über das Ende hinaus.
4. **Stumm:** „Stumm“ drücken: Pegel ganz unten, die Taste ist orange in der App und am Handy. Nochmal: alter Pegel ist wieder da. „Lauter“ bei Stumm holt den alten Pegel und geht einen Schritt höher. Taste M in der App.
5. **Wiederholen (ein Knopf, drei Zustände):** Jeder Klick schaltet weiter: aus → 🔁 Playlist → 🔂 Titel → aus. Ist es an, ist der Knopf orange, in der App und am Handy gleich, auch wenn du am Handy drückst. **Playlist:** Nach dem letzten Titel geht es mit dem ersten weiter. **Titel:** Der Titel beginnt am Ende von vorn; „Weiter“ springt trotzdem zum nächsten Titel (der wird dann wiederholt). Taste R in der App.
6. **Zufall:** Eigener Knopf, orange, wenn an. Die Titel kommen in zufälliger Reihenfolge, jeder einmal, danach ist die Sendung zu Ende. Mit Wiederholen „Playlist“ geht es mit einer neuen Runde weiter. Taste S in der App.
7. **Gast:** Mit der Gast-PIN am Handy erscheinen die Tasten nicht, ein Gast kann nur wünschen. Wiederholen, Zufall und Stumm gelten für die Sendung, nicht für den einzelnen Handy-Besucher. OSC: dieselben Namen unter `/woa/party/previous`, `/stop`, `/seek_back`, `/seek_forward`, `/restart`, `/mute`, `/repeat`, `/shuffle`, `/play`.

Selbst geprüft von Claude (ohne Ton, offline durch den echten Mixer): Pause mit Play an derselben Stelle (Restzeit vor und nach der Pause gleich, läuft danach weiter), Play während der Ausblendung, Stopp vergisst die Stelle, Zurück, Weiter, Von vorn, Spulen ±10 s (Restzeit gemessen, Grenzen am Anfang und am Ende), Stumm mit altem Pegel, Wiederholen aus/Playlist/Titel je mit eigenem Test (Titel wechselt nicht, Playlist beginnt von vorn, aus beendet die Sendung), Weiter bei „Titel wiederholen“, Zufall (jeder Titel genau einmal, mit Wiederholen endlos), die Befehle durch dieselbe Prüfung wie bisher (Gast bekommt 403, Unbekanntes 400, Zusatzfelder ohne Wirkung) und der Weg Handy → Party-Schicht für alle 13 Befehle.

Nicht selbst geprüft: das Aussehen der Tasten in der App und im Handy-Browser (gebaut und von Clippy geprüft, nicht angeklickt, kein Bildschirm), die Emoji-Symbole 🔁 🔂 🔀 in der App-Schrift (ein leeres Kästchen statt Symbol wäre ein Befund), und das Hören von Sprung, Überblendung und Stumm am echten Gerät. Wiederholen und Zufall sind bei jedem Programmstart aus.

## Paket 40: Release-Workflow für beide Plattformen (0.34.1, offen)

Aus M10. Ein Tag `v…` baut Windows und Linux und legt ein Release als **Entwurf** an. Den Probelauf gibt es ohne Tag (GitHub → Actions → Release → Run workflow); er legt kein Release an. Ablauf: `docs/release.md`.

1. **Release-Entwurf ansehen:** Nach dem Tag (oder dem ersten echten Release) auf GitHub unter Releases den Entwurf „WOA <version>“ öffnen. Die Neuerungen im Text sind der CHANGELOG-Abschnitt.
2. **Dateien plus SHA256SUMS da:** `woa-<v>-x64.msi`, `woa-<v>-windows-x64.zip`, `WOA-<v>-x86_64.AppImage`, `woa-<v>.flatpak`, `latest.json`, `SHA256SUMS`. Nach dem Herunterladen in einen Ordner: `sha256sum -c SHA256SUMS` meldet überall OK.
3. **Freigeben:** Entwurf veröffentlichen. Erst danach bietet die App das Update an.

Selbst geprüft von Claude (ohne Ton, ohne Audiogerät; lokal und im CI-Lauf):
- **Prüfskript:** `packaging/test-release-check.sh` mit 19 Fällen: Tag passt, Tag zu alt, Tag mit Zusatz, CHANGELOG hinkt hinterher oder ist leer, geänderter UpgradeCode, fehlende oder leere Release-Dateien, `SHA256SUMS` (schließt sich nicht selbst ein, Gegenprobe, geänderte Datei fällt auf). `shellcheck` ohne Befund.
- **Probelauf im PR:** Der Workflow läuft bei Änderungen an `release.yml` ohne Tag durch (Prüfen, Windows, Linux, Paket) und hängt den fertigen Ordner als Artefakt `release-probe` an; keine Veröffentlichung.
- **MSI-Upgrade im Windows-Runner:** alt (0.0.1) installieren, neu darüber, ein Eintrag, Dateien da, Downgrade abgelehnt, Deinstallation sauber (`test-upgrade.ps1`).

Nicht selbst geprüft: ein echter Tag-Lauf mit Entwurf (der Tag setzt ein echtes Release in Gang; das ist dein Handgriff); Update von einer schon installierten, früheren Version auf deinem Rechner (nur der Runner-Ersatz mit 0.0.1).

**Offen für André:**
- Windows-Codesigning-Zertifikat (kostenpflichtig) und Signatur der Linux-Pakete.
- Veröffentlichung: Download-Seite und Datenschutzerklärung auf wironair.de, Flathub.
- minisign-Schlüssel für Stufe B des Updaters (`docs/updater.md`).

## Paket 39: Linux-Pakete aus der CI (AppImage und Flatpak) (0.34.0, offen)

Aus M10. Die Pakete entstehen im Workflow „Linux-Bau“ (GitHub → Actions → Linux-Bau → Run workflow, oder bei diesem PR automatisch). Sie sind unsigniert.

1. **Artefakt laden:** Am grünen Lauf unten „woa-appimage“ und „woa-flatpak“ herunterladen und entpacken (GitHub packt jedes Artefakt in eine ZIP).
2. **AppImage starten:** `chmod +x WOA-0.34.0-x86_64.AppImage`, dann `./WOA-0.34.0-x86_64.AppImage --version` (zeigt `woa 0.34.0`) und ohne Zusatz starten: Das Fenster von WOA öffnet sich.
3. **Flatpak installieren:** `flatpak install --user woa-0.34.0.flatpak` (die Laufzeit freedesktop 25.08 wird bei Bedarf nachgeladen), dann „WOA - Wir on Air“ aus dem Startmenü öffnen. Beim Flatpak nach dem Ausgabegerät in den Einstellungen sehen: Es soll sich wie sonst verhalten.

Selbst geprüft von Claude (ohne Ton, ohne Audiogerät; im CI-Lauf und lokal):
- **Eintrag und Metainfo:** `desktop-file-validate` und `appstreamcli validate` ohne Fehler (lokal und im Lauf, an Metainfo und Eintrag im fertigen Paket).
- **AppImage im Lauf:** gebaut, `--version` stimmt mit der Version überein, Offline-Render eines neuen Projekts mit `woa-headless` aus dem Paket (2 Sekunden, Datei nicht leer).
- **Flatpak im Lauf:** ohne Netz gebaut (Crates als Quellen), installiert, `flatpak run de.wironair.woa --version` und derselbe Offline-Render.
- **Lokal:** `woa --version`/`--help` per Einheitstest; Shell-Skripte mit `shellcheck`; Manifest als YAML lesbar.

Nicht selbst geprüft: Start der Oberfläche aus dem Paket (die CI hat keinen Bildschirm; Fenster und Ton sind deine Klicks); Wayland/X11-Verhalten im Flatpak; MIDI-Geräte im Flatpak; ein frisches System ohne Entwicklerpakete.

**Offen für André:**
- Signatur der Pakete (Zertifikat, Kosten) und der Ort der Veröffentlichung (wironair.de, Flathub, GitHub-Releases).
- Die App-Kennung `de.wironair.woa` setzt voraus, dass die Domain wironair.de dir gehört.
- CLAP-Plugins des Rechners sieht ein Flatpak nicht; dafür bräuchte es eine Erweiterung oder das AppImage.

## Kopfhörer unter Windows (0.34.1, offen, #139)

Zweck: Stecken Sie Kopfhörer ein oder aus, schaltet Windows das Standard-Ausgabegerät um. Bisher blieb WOA stumm; jetzt verbindet es sich selbst neu. Windows-Notebook mit Kopfhörern (Klinke oder USB), Ausgabegerät in den Windows-Einstellungen auf „Standard“, WOA aus der neuen Version.

1. **Einstecken im Player:** Modus „Arrangement“, ein Projekt mit Ton starten. Kopfhörer einstecken. Nach ein bis drei Sekunden kommt der Ton auf den Kopfhörern weiter, ohne dass Sie etwas klicken. Wiedergabe und Playhead laufen ohne Sprung.
2. **Ausstecken:** Kopfhörer wieder abziehen. Der Ton kommt nach ein bis drei Sekunden aus den Lautsprechern zurück.
3. **Im Radio-Modus:** Modus „Radio“, Titel starten, Kopfhörer ein- und ausstecken wie oben. Titel und Carts laufen auf dem neuen Gerät weiter. Oben im Sendepult darf kurz eine Meldung zu einem ausgefallenen Ausgang stehen.
4. **Vorhören (PFL):** Einstellungen, Vorhören auf „Gerät: Kopfhörer“ stellen. Kopfhörer abziehen und wieder einstecken, dann ein Vorhören auslösen. Der Ton kommt wieder an.
5. **Rettungsknopf:** Einstellungen, „Ausgabe neu verbinden“. Der Ton setzt kurz aus und läuft danach auf dem aktuellen Standardgerät. Probieren Sie ihn, falls einmal nach dem Umstecken Stille bleibt.
6. **Ohne Neustart prüfen:** Das Ganze dreimal hintereinander. Es darf kein Neustart des Programms nötig sein.

Telemetrie: Für Gerätefehler gibt es Codes der Form `audio.output.device_invalidated`, `audio.output.device_not_available`, `audio.output.device_busy`, `audio.output.config`, `audio.output.backend`, `audio.output.other`; kein Freitext, kein Gerätename.

Grenze: Hat das neue Gerät eine andere Abtastrate als das alte (zum Beispiel 44,1 statt 48 kHz), kann WOA den laufenden Mix nicht darauf umstellen. Dann bleibt es still, und der Code `audio.output.config` erscheint. Dann bitte melden; WOA versucht es mit wachsenden Pausen weiter, bis zum Programmneustart.

Selbst geprüft von Claude (ohne Ton, ohne Windows): Fehlerklassen, Neuöffnen nur bei Gerätefehlern (nicht bei Aussetzern), Wartezeiten zwischen Versuchen, dass die Render-Funktion ein Neuöffnen überlebt und im Audio-Thread nie wartet. Quelle für die Windows-Fehler: der Quelltext von cpal 0.18.2 (WASAPI meldet beim Wechsel des Standardgeräts `StreamInvalidated` oder `DeviceNotAvailable` und baut den Strom nicht selbst neu).

Nicht selbst geprüft: das Verhalten mit echten Kopfhörern unter Windows (kein Windows, kein Gerät hier), Bluetooth-Kopfhörer, der Bauvorgang für Windows, ob die Fehlermeldungen von cpal in der Praxis genau so ankommen, wie der Quelltext es erwarten lässt.

## Silvester Teil 2, Paket 1: Ausgänge je Soundkarte (0.33.6, offen, #134)

Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`, Modus „Radio“. Möglichst beide Soundkarten und das Pult angeschlossen.

1. **Zuordnen:** Einstellungen, „Ausgänge (Soundkarten) …“. Playlist auf Karte 1 Kanal 1/2, Cart-Wall auf Karte 2 Kanal 1/2 (oder auf Karte 1 Kanal 3/4), „Übernehmen“. Programm neu starten, falls die Sendung lief.
2. **Hören am Pult:** Titel starten, Cart drücken. Die Musik kommt nur am Pult-Kanal der Playlist, das Cart nur am Pult-Kanal der Cart-Wall. Den Pegel am Pult einstellen.
3. **Neu starten:** Programm schließen und öffnen: Die Zuordnung ist noch da.
4. **Gerät fehlt:** Karte 2 abziehen, Programm starten. Es erscheint eine Meldung, das Cart läuft über die Playlist weiter (nie Stille ohne Hinweis). Karte 2 im Betrieb abziehen: Meldung „Ausgang … ist ausgefallen“, Playlist läuft weiter.
5. **Dauertest:** Eine Stunde Musik und Carts auf zwei Karten mit unterschiedlichem Takt. In den Ausgängen sollen „verworfen“, „übersprungen“ und „leer gelaufen“ bei 0 bleiben oder nur selten zählen. Knackser notieren.

Takt und Drift: Das Gerät der Playlist taktet den Mixer. Weitere Geräte laufen mit eigener Uhr; ein Ring und ein Resampler (Hermite) halten das Polster bei etwa 50 ms, auch bei 44,1 gegen 48 kHz. Empfehlung bleibt gleiche Abtastrate an allen Karten. Getrennte Ausgänge tragen das Signal vor der Sendeverarbeitung (Kompressor/Limiter); Aufnahme und Stream bekommen weiter die Summe.

Selbst geprüft von Claude (ohne Ton, offline): Kanalzuordnung je Paar, Cart-Wall nicht im Hauptausgang und Summe gleich dem alten Mix, zweites Gerät mit 44,1 kHz über den Ring, Drift bis 1000 ppm über 60 s ohne Lücken, ausgefallenes Gerät, Plan bei fehlendem Gerät, Speichern und Laden der Zuordnung, keine Speicheranforderung im Audio-Thread.

Nicht selbst geprüft: Öffnen echter Karten und des Pults unter Windows (kein Gerät, kein Windows hier), Aussehen des Fensters, Verhalten beim Abziehen im Betrieb.

## Silvester-Paket: Cart-Tasten, Schnitt, Uhrzeit, Jingle-Übergang (0.33.4, offen, #132)

Wünsche des Moderators der Silvestersendung. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`, Modus „Radio“. Kopfhörer-Ausgang für das Vorhören unter Einstellungen → Vorhören (PFL) einstellen, sonst bleibt „Vorhören“ grau.

1. **Cart-Nummer ist die Taste:** In der Cart-Wall zeigt jedes Pad groß seine Nummer 1 bis 16. Die Tasten 1 bis 9 und 0 (für Cart 10) starten das Cart, nochmal drücken hält es an. Zwei oder drei Carts nacheinander starten: Sie laufen gleichzeitig.
2. **Taste ändern:** Rechtsklick auf ein Pad, „Taste ändern …“, dann die neue Taste drücken (Esc bricht ab, Entf nimmt dem Cart die Taste). Die Taste steht jetzt klein in der Ecke des Pads. Carts 11 bis 16 haben anfangs keine Taste („–“ in der Ecke), dort selbst eine vergeben, etwa Q, W, E. Vergibst du eine schon benutzte Taste, verliert das andere Cart sie. M und Leertaste sind belegt und werden abgelehnt. Programm schließen und neu starten: Die Tasten sind noch da.
3. **Cart schneiden:** Rechtsklick auf ein belegtes Pad, „Start und Ende schneiden …“. In der Wellenform Cue-In und Cue-Out setzen, „Übernehmen“. Cart starten: Es spielt nur das Stück dazwischen. „Vorhören ab …“ spielt im Kopfhörer, nie auf Sendung.
4. **Vorhören:** Rechtsklick auf Cart oder Playlist-Titel, „Vorhören“: Es kommt nur im Kopfhörer an, der Sendepegel bewegt sich nicht.
5. **Uhrzeit:** Im Sende-Deck steht über dem Countdown die Uhrzeit in großen Ziffern, sie läuft sekundengenau. Stimmt sie nicht mit deiner Uhr überein, unter Stundenuhr den Zeitversatz prüfen.
6. **Jingle-Übergang:** Ein Jingle als Voice-Track zwischen zwei Titel legen (Rechtsklick auf den Titel davor, „Voice-Track danach einfügen …“, Jingle wählen), im Editor „Jingle-Übergang“ klicken, „Übernehmen“. Der Jingle beginnt genau mit dem nächsten Titel, der Titel startet leise darunter und wird nach dem Ende des Jingles in 1,5 Sekunden hochgefahren. Das Ende des Jingles und die Lautstärke darunter lassen sich im Editor ändern.

Selbst geprüft von Claude (ohne Ton, offline durch den echten Mixer): Cart mit Cue-Punkten spielt genau die Länge dazwischen (ohne Schnitt 2,0 s, mit Schnitt 0,75 s); Tasten Standard, ändern, eindeutig, Speichern und Laden, alte Dateien; Uhrzeit mit Zeitzone und Mitternacht; Jingle-Übergang gemessen (Jingle zeitgenau mit Titel B, B darunter −20 dB, nach dem Jingle wieder volle Lautstärke, dazwischen ohne Sprung).

Nicht selbst geprüft: das Aussehen von Pad-Ecke, Uhr und Schnitt-Fenster im Fenster (gebaut, nicht angeklickt, kein Bildschirm), das Drücken der Tasten am echten Gerät.

**Offen (wartet auf Antwort des Moderators, nicht Teil dieses Pakets):** mehrere Haupt-Player auf getrennten Soundkarten-Ausgängen, Mischpult-Steuerung, Telefon-Kanal, Wunschtitel-Fach statt Spotify.

## Paket 38: Browser-Gast per WebRTC (lokal) (0.33.0, offen)

Aus M9. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`
Modus „Radio“, Karte „Gäste & Mix-Minus“, darunter „BROWSER-GÄSTE“. Das Handy muss im selben WLAN sein wie der Rechner. Dabei Kopfhörer aufsetzen (am Handy und am Rechner), sonst hört sich der Gast über den Lautsprecher selbst.

1. **Gast einladen, Link ans Handy:** „Browser-Zugang einschalten“, dann „＋ Einladung“. QR-Code mit der Handy-Kamera scannen (oder „Link kopieren“ und per Nachrichten-App schicken). Das Handy zeigt eine Zertifikatswarnung („Erweitert“, „Weiter“): Das ist erwartet, das Zertifikat ist selbst ausgestellt. Namen eintragen, „Beitreten“, Mikrofon erlauben.
2. **In WOA annehmen:** Unter „BROWSER-GÄSTE“ steht „<Name> wartet“. „Annehmen“ drücken: Es erscheint ein Gast-Kanal mit Pegel, die Zeile zeigt „Browser · RTT … ms · Verlust … %“, das Handy zeigt „Du bist auf Sendung geschaltet“.
3. **Sprechen mit Kopfhörern:** Am Handy sprechen: Der Pegel des Gastes schlägt aus, die Stimme ist in der Sendung. Im Handy ist die Sendung (Musik, Moderator) zu hören, die eigene Stimme nicht.
4. **Auflegen:** „Auflegen“ am Handy (oder „Entfernen“ in WOA): Der Kanal verschwindet, das Handy zeigt „beendet“. Dieselbe Einladung lässt sich nicht noch einmal benutzen; „Browser-Zugang“ aus: Die Ports sind zu.

Selbst geprüft von Claude (ohne Ton, ohne Audiogerät, ohne echtes Mikrofon; Loopback und kopfloser Browser mit falschem Mikrofon):
- **Zwei Teilnehmer im selben Prozess:** Gast-Sinus 440 Hz kommt am Pult mit −14 bis −8 dB an (Frequenz und Pegel gemessen), der Rückweg-Sinus 880 Hz beim Gast ebenso, der eigene Ton des Gastes liegt im Rückweg unter −60 dB; über 100 Pakete in jede Richtung. Danach Auflegen, Kanal frei, wiederbenutzbar.
- **Ganzer Weg über HTTPS:** Einladung, Beitritt, Warteraum, Annehmen, Gespräch über fünf Sekunden mit Messung, Auflegen, Ablehnen, kaputtes Angebot, Einladung nur einmal benutzbar, Ausschalten schließt Port.
- **Echter Browser, falsches Mikrofon:** Kopfloser Chromium mit `--use-fake-device-for-media-stream` und `--mute-audio` gegen den Server auf der LAN-Adresse: Seite wartet im Warteraum, wird angenommen, verbindet (RTT 14 ms, 0 Promille Verlust), der Browser zählt eintreffende Rückweg-Pakete, WOA legt auf und die Seite zeigt „ende“.
- **Grenzen:** Namen bereinigt, Anfragen und Köpfe begrenzt, höchstens 16 Einladungen und 8 Wartende, Wartende ohne Lebenszeichen fliegen raus, hängende Verbindung gibt den Kanal nach 20 Sekunden frei.
- **Echtzeit:** `assert_no_alloc` um Gast-Summe und Rückweg-Schreiben.

Nicht selbst geprüft: ein echtes Handy, echte Kopfhörer und WLAN (nur Chromium am Notebook über die LAN-Adresse); das Aussehen der Karte im Fenster (gebaut, nicht angeklickt); Windows und die Windows-Firewall.

**Offen für André:**
- Gäste von außerhalb des Netzes brauchen einen Relay-Dienst (TURN). Ein eigener Server oder ein Fremddienst kostet laufend; das ist deine Entscheidung. Ohne TURN geht es nur im selben Netz.
- Ein echtes Zertifikat (und damit eine Domain) würde die Zertifikatswarnung am Handy beseitigen; bis dahin muss der Gast einmal „Weiter“ tippen.
- Firewall: UDP 47600 und TCP 47601 müssen am Rechner im Heimnetz erreichbar sein (Windows fragt beim ersten Start).

## Paket 37: Gast-Kanäle mit Mix-Minus (ohne Netz) (0.32.0, offen)

Aus M9. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`
Modus „Radio“, Karte „Gäste & Mix-Minus“ rechts unter dem Processing. Zum Hören der Rückwege ein Kopfhörer am normalen Ausgang (der Abhörausgang trägt den Rückweg, solange 🎧 an ist). Töne sind Testsignale, kein Mikrofon, kein Netz.

1. **Zweimal Testgast hinzufügen:** „＋ Testgast“ zweimal. Es erscheinen „Gast 1“ (tieferer Ton) und „Gast 2“, je mit Pegelanzeige, die ausschlägt, und der Pufferzeile („Puffer … ms“, kein Aussetzer).
2. **Fader bewegen, Pegel sehen:** Den Fader von Gast 1 nach unten ziehen: die Anzeige wird kleiner, der Ton in der Sendung leiser. „M“ macht ihn stumm.
3. **Rückweg Gast 1 im Kopfhörer:** Bei Gast 1 auf 🎧: im Kopfhörer ist nur Gast 2 zu hören (und Musik, falls sie läuft), Gast 1 nicht. Bei Gast 2 auf 🎧: umgekehrt. Nochmal 🎧: wieder der normale Abhörmix mit beiden.

Selbst geprüft von Claude (ohne Ton, ohne Audiogerät, ohne Mikrofon; Offline-Render):
- **Mix-Minus:** Zwei Sinus-Gäste (440 und 660 Hz) und Musik (880 Hz): Rückweg A enthält B und Musik mit vollem Pegel, A liegt unter −90 dB (Goertzel über 1,5 s), Rückweg B spiegelbildlich; die Sendung enthält alle drei. Mit nur einem Gast ist der Rückweg bitgleich die Musik. Dasselbe durch den echten Sendemixer: Abhörausgang auf Rückweg 1 gestellt zeigt 660 Hz, nicht 440 Hz; zurückgestellt wieder beide.
- **Fader, Stumm, Pegel:** −20 dB am Fader senkt den Gast in der Sendung um 20 dB, Stumm auf unter −80 dB, auch im Rückweg des anderen; die Pegelanzeige folgt dem Fader.
- **Jitterpuffer:** Stille bis 60 ms Vorlauf; Obergrenze 400 ms (Überschuss wird abgewiesen und als Überlauf gezählt; die Leseseite schneidet auf den Vorlauf zurück); leerer Puffer gibt Stille und genau einen Aussetzer, Wiederanlauf erst nach dem Vorlauf; Aussetzer und Überlauf stehen im Log und in der Anzeige.
- **Echtzeit:** `assert_no_alloc` um Gast-Summe, Rückweg, Spitzenmeldung und den ganzen Mixer-Block.
- **Kanäle:** Höchstens acht Gäste; ein entfernter Gast gibt den Kanal und den Rückweg-Abhörer frei; Testquelle liefert in Echtzeit.

Nicht selbst geprüft: Aussehen der Karte im Fenster (gebaut und übersetzt, nicht angeklickt), Hören im echten Kopfhörer, Windows.

**Offen für André:**
- WebRTC-Gäste (Paket 38) brauchen für Gäste hinter fremden Routern vermutlich einen Relay-Dienst (TURN). Ein eigener Server kostet laufend, ein Fremddienst auch; das ist deine Entscheidung. Im selben WLAN geht es ohne.
- Soll der Rückweg später auf einem zweiten Audiogerät (echter Kopfhörer-Ausgang) liegen statt auf dem Abhörausgang des Sendemixers?

## Paket 36: Web-Fernbedienung im WLAN (0.31.0, offen)

Aus M9. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`
Das Handy muss im selben WLAN sein wie der Rechner.

1. **Fernbedienung an:** Modus „Party“, rechts unter der Wunschliste „Fernbedienung an“. Es erscheinen ein QR-Code, die Adresse und die Gast-PIN. Mit der Handy-Kamera scannen: die Seite öffnet sich, oben steht „verbunden“ und die Anzeige des laufenden Titels.
2. **Als Gast Wunsch einreichen:** Am Handy einen Titel suchen (Ordner vorher laden) und „Wünschen“ tippen. Er erscheint in der Wunschliste am Rechner und am Handy. Ein zweiter Wunsch gleich danach wird mit einer Wartezeit abgelehnt. Die Tasten Play, Weiter, Lauter, Leiser gibt es als Gast nicht.
3. **Als DJ Weiter:** In WOA „DJ-PIN zeigen“, am zweiten Handy (oder im Browser, Seite neu laden) die DJ-PIN eingeben. „Weiter“ tippen: der nächste Titel wird überblendet. „Als Nächstes“ bei einem Wunsch reiht ihn ein.
4. **Aus:** „Aus“ drücken: am Handy steht „getrennt“, und die Seite lädt nicht mehr (Port ist zu).

Selbst geprüft von Claude (ohne Ton, ohne Audiogerät, ohne Mikrofon; Server und Clients gegen 127.0.0.1):
- **Anmeldung:** Falsche PIN gibt 401; ohne Token kein Zustand, keine Suche, kein Befehl, kein WebSocket. Fünf falsche PINs sperren die Adresse mit 429 und Wartezeit, nach Ablauf geht es wieder.
- **Rollen:** Gast bekommt bei Play, Weiter, Lauter, Leiser, Wunsch einreihen und verwerfen 403, per HTTP und per WebSocket; ausgelöst wird nichts. DJ-Befehle kommen als `PartyCommand` an und laufen durch dieselbe Schicht wie die Tasten.
- **Wünsche:** Wunsch aus der Suche und als Freitext landet in der Befehlsschicht und in der Wunschliste; zu langer oder leerer Titel wird abgelehnt; die Handys sehen nie einen Dateipfad.
- **Grenzen:** Wunsch je Adresse alle 10 s (zweiter Login hilft nicht), zehn Befehle je Sekunde, 64 Verbindungen, zu große Anfragen 413.
- **Vollstand und Änderungen:** Beim Verbinden der ganze Zustand, danach nur das geänderte Feld; unveränderter Stand sendet nichts.
- **Last:** 20 Handys gleichzeitig bekommen Vollstand und Änderung; beim Ausschalten bekommen alle Close, der Port ist danach zu (Verbindung wird verweigert) und lässt sich neu öffnen.
- **OSC:** Aus ohne Haken; mit Haken werden nur Drücken und bekannte Pfade zu Befehlen.

Nicht selbst geprüft: Aussehen und Bedienung der Seite im Handy-Browser, QR-Scan mit echter Kamera, Windows (Firewall-Abfrage), Fenster mit der neuen Karte (gebaut und übersetzt, nicht angeklickt).

**Offen für André:**
- Verschlüsselung: Die Seite läuft über http im WLAN, die PIN geht unverschlüsselt durchs Heimnetz. Für den Party-Zweck vertretbar; https bräuchte ein selbstsigniertes Zertifikat (Warnung am Handy) oder eine eigene Domain mit echtem Zertifikat (Kosten). Gebraucht wird es spätestens für Paket 38 (Mikrofon im Browser).
- OSC hat keine Anmeldung (Protokoll bedingt). Soll der Haken in der Oberfläche bleiben?
- Soll die DJ-PIN auch per QR-Code anzeigbar sein (bequemer, aber auf einem Party-Bildschirm für alle lesbar)?

## Paket 35: Party-Layout (0.30.0, offen)

Aus M9. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. **Modus Party:** Oben rechts „Party“ wählen. Große Anzeige, vier große Tasten, rechts die Wunschliste.
2. **Musikordner laden, Play:** „Ordner laden …“, einen Ordner mit Musik wählen, dann „Play“ (oder Leertaste). Titel, Interpret und große Restzeit laufen, darunter steht der nächste Titel.
3. **Weiter:** „Weiter“ drücken (oder N): Der nächste Titel wird überblendet, die Anzeige springt um.
4. **Vollbild an/aus:** „Vollbild an“ (oder F), dann wieder aus.
5. **Wunschliste (optional):** „+ Wunsch aus Datei“ zweimal mit derselben Datei: ein Eintrag mit „×2“. „Als Nächstes“ reiht ihn hinter dem laufenden Titel ein.

Selbst geprüft von Claude (ohne Ton, ohne Audiogerät, ohne Mikrofon; Controller und Mixer in Blöcken mit erzeugten Sinus-Dateien):
- **Layout ohne Fenster:** Textschnappschuss von Titel, Interpret, Restzeit, Fortschritt, nächstem Titel, Tasten, Pegel, Vollbild und Wünschen, auch vor dem Start (Taste heißt „Play“) und bei leerer Playlist.
- **Tasten:** Jeder Befehl löst den erwarteten Effekt aus und steht im Befehlsprotokoll; Lauter/Leiser stoppen an den Grenzen; das Protokoll ist auf 200 Zeilen begrenzt.
- **Wunschliste:** sortiert nach Stimmen, dann Eingang; Doppelte (anderer Pfad, andere Schreibweise) zählen als Stimme.
- **Restzeit:** Die angezeigte Restzeit weicht bei 1, 5 und 12,5 s nicht mehr als 1,5 s von „Länge des Offline-Renders minus gespielte Zeit“ ab, der Fortschritt nicht mehr als 8 Prozentpunkte.
- **Weiter und Wunsch:** „Weiter“ blendet in einer Sekunde zum Wunsch-Titel über, der hinter dem laufenden Titel eingereiht war; der nächste Titel danach stimmt.

Nicht selbst geprüft: Aussehen und Bedienung des Fensters (Tasten, Vollbild und Dateidialoge sind gebaut und übersetzt, nicht angeklickt), Windows, echte Musik und Tonqualität.

**Offen für André:**
- Die Lautheit gleicht der Sendemixer schon an (Ziel-Lautheit der Sendung); ein eigener Wert für Party ist nicht eingestellt. Soll Party anders zielen als Radio?
- Wünsche kommen erst mit der Handy-Seite (Paket 36) von den Gästen; bis dahin trägt nur man selbst per Datei ein.
- Eine Taste am Controller für den Party-Modus ist nicht gebaut.

## Paket 34: Abnahme M8: Surge XT und Vital stabil (0.29.0, offen)

Aus M8, schließt den Meilenstein. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`
(Für Schritt 4 zusätzlich `WOA_TESTMENU=1` davor setzen.)

1. **Projekt bauen:** Zwei MIDI-Spuren, auf die erste Surge XT, auf die zweite Vital (als CLAP). Je einen Clip mit Noten anlegen. Kommt Vital nicht in der Liste unter „+ Plugin“: siehe offene Frage unten; dann ein anderes Instrument nehmen.
2. **30 Minuten spielen:** Beide Spuren im Loop abspielen und zwischendurch live spielen, Presets wechseln, Fenster auf- und zumachen. Es soll nichts aussetzen, knacken oder hängen.
3. **Speicher:** Nach 30 Minuten im Systemmonitor schauen: weder `woa` noch die beiden Prozesse `woa-plugin-host` (bei `cargo run` heißen sie `woa --woa-plugin-host`) sollen sich auffällig vergrößert haben.
4. **Eine Instanz abschießen:** Beim Surge-Platz „Absturz simulieren“ (oder den Prozess im Terminal mit `kill -9` beenden). Vital und alles andere spielt weiter; beim Surge-Platz steht „Plugin abgestürzt – neu laden“. „Neu laden“ bringt Surge mit dem letzten Klang zurück.
5. **Log:** Beim Surge-Platz „Log“ aufklappen: Start, Ausfall, Neu laden stehen mit Uhrzeit drin.
6. **Sperrliste:** Beim abgestürzten Platz „Sperren“ drücken, dann unter „+ Plugin“ „Neu suchen“: Surge verschwindet aus der Liste und steht bei „Gesperrt“. „Freigeben“, dann „Neu suchen“: Surge ist wieder da.

Selbst geprüft von Claude (ohne Ton, ohne Audiogerät, ohne Mikrofon; Test-Plugin aus `clack-plugin`, Offline-Render, Hilfsprogramme als Suchprozess):
- **10 Minuten Musik** (56 250 Blöcke) offline durch zwei Plugin-Prozesse, unter `assert_no_alloc`: kein verspäteter Block, Klang kam heraus, der Speicher der App und beider Plugin-Prozesse ist nach der Einlaufzeit unverändert (App 4332 → 4336 KiB, Plugins ±0).
- Eine Datei, die beim Suchen abstürzt oder hängt, wird gesperrt, beim nächsten Suchen nicht mehr angefasst und lässt sich freigeben; die Sperre überlebt einen Neustart; ein Suchprozess, der gar nicht startet, sperrt nichts.
- Das Log je Plugin enthält Start, Ausfall, Neu laden und Ende in der richtigen Reihenfolge, mit Zeitstempel.

Nicht selbst geprüft: Surge XT und Vital selbst, die Oberfläche (Knöpfe „Sperren“, „Freigeben“, „Log“ sind gebaut und übersetzt, nicht angeklickt), Windows, Speicher über 30 Minuten mit echten Plugins.

**Offen für André:**
- Gibt es Vital als CLAP in der Version, die du hast? Sonst läuft die Abnahme mit einem freien Ersatz (Odin 2, Dexed). Ein Kauf wäre deine Entscheidung; gebaut ist nichts dafür.
- Eine Datei wird auch bei einer Zeitüberschreitung (20 s) gesperrt; auf einer sehr langsamen Platte könnte das ein gutes Plugin treffen. „Freigeben“ macht es rückgängig. Soll die Frist länger sein?
- Im Betrieb abgestürzte Plugins werden nicht automatisch gesperrt, nur von Hand.
- Die Suche läuft noch im Bild (kurzes Einfrieren bei vielen Plugins), nicht im Hintergrund.

## Paket 33: Absturzschutz per eigenem Prozess (0.28.0, offen)

Aus M8. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && WOA_TESTMENU=1 cargo run -q --release -p woa`
(Das Testmenü ist nur für diese Probe; ohne die Variable fehlt der Knopf „Absturz simulieren“.)

1. **Absturz simulieren:** MIDI-Spur mit einem Plugin (Surge XT oder ein anderes), eine zweite Spur mit einem eingebauten Klang. Beides abspielen. Beim Plugin-Platz „Absturz simulieren“ drücken.
2. **Hinweis:** Beim Platz steht „Plugin abgestürzt – neu laden“, die Statuszeile nennt es ebenfalls. Die App bleibt bedienbar.
3. **Rest spielt:** Die andere Spur spielt ohne Aussetzer weiter; nur das Plugin ist stumm.
4. **Neu laden:** „Neu laden“ drücken: Das Plugin spielt wieder, mit seinem letzten Klang und denselben Reglern. Mehrfach wiederholen.
5. **Latenz:** Ein Plugin mit Latenz (z. B. ein Limiter mit Vorausschau) neben einer Spur ohne: Beide bleiben zeitgleich (Schlag auf Schlag, kein Versatz).

Selbst geprüft von Claude (ohne Ton, ohne Audiogerät, ohne Mikrofon; Test-Plugin aus `clack-plugin`, das auf Befehl abstürzt oder hängt; Offline-Render und ein Echtzeit-Fenster ohne Gerät):
- Das Plugin im eigenen Prozess liefert dieselben Samples wie im Programm, und 0 verspätete Blöcke im Offline-Lauf.
- Absturz mitten im Block: dieser Block ist still, der Rest läuft, die Blöcke warten nie lange, der Ausfall wird erkannt, ein zweites Plugin spielt weiter; „Neu laden“ bringt das Plugin zurück.
- Hänger (Endlosschleife im Plugin): der Block wartet nicht darauf, der Prozess wird nach der Frist beendet.
- Latenz: Ein Impuls kommt exakt um die gemeldete Latenz später heraus; die Engine gleicht zwei Spuren aus; ohne Latenz wird nichts verzögert.
- Zustand und Regler wandern über die Prozessgrenze; ein Suchprozess, der stirbt, wird ein Suchproblem; keine Allokation im Audio-Thread (`assert_no_alloc`).

Nicht selbst geprüft: Surge XT und andere echte Plugins, die Oberfläche (Knöpfe und Hinweis sind nur gebaut und übersetzt), Windows, Dauerlast.

Bewusst so und **offen für André:**
- Ein Prozess je Plugin: sauber getrennt, kostet aber Arbeitsspeicher bei vielen Plugins. Ein gemeinsamer Prozess für alle wäre sparsamer, ein Absturz träfe dann alle.
- Neu laden nur von Hand, nicht automatisch (sonst flattert ein abstürzendes Plugin).
- Plugin-Fenster im Schutzprozess unter Windows ist nicht gebaut; eingebettete Fenster wären M11.
- Ändert ein Plugin seine Latenz nach dem Laden, wird das nicht nachgeführt.
- Reglerverläufe (Automation) würden um den Ausgleich verschoben, sobald es Automation für Plugins gibt.
- Das zweite Programm `woa-plugin-host` ist in AppImage, MSI und Flatpak noch nicht verpackt; die App ruft sich bis dahin selbst mit `--woa-plugin-host` auf.
- Ob Vital als CLAP gegenüber Odin 2 und Dexed der bessere Test-Klang ist, ist nicht entschieden.

## Paket 32: Plugin-Fenster (GUI) (0.27.0, offen)

Aus M8. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. **Surge-Fenster öffnen:** MIDI-Spur mit Surge XT (wie bei Paket 31). Unter „PLUGINS“ neben dem Namen die Taste „Fenster“ drücken. Das Fenster von Surge erscheint schwebend; die Taste leuchtet. Erscheint kein Fenster, steht in der Statuszeile unten der Grund (z. B. „nur eingebettet“) – bitte genau diesen Text melden.
2. **Preset wechseln:** Im Surge-Fenster ein anderes Preset wählen, die Klangänderung hören. Danach in der App „Rückgängig“ (Strg+Z): Das Preset ist wieder das alte.
3. **Regler drehen:** Einen Regler im Fenster ziehen, loslassen, Strg+Z, Strg+Umschalt+Z: Wert springt zurück und wieder vor.
4. **Schließen und öffnen:** Fenster mit dem Schließknopf des Fensters zumachen, dann „Fenster“ erneut drücken: Es kommt mit demselben Stand wieder. Taste noch einmal: schließt.
5. **Speichern:** Speichern, Projekt neu öffnen: Der Klang von Preset und Reglern ist wie vorher.

Selbst geprüft von Claude (ohne Ton, ohne Audiogerät, mit dem Test-Plugin aus `clack-plugin`, dessen Oberfläche nur protokolliert; **kein echtes Fenster, kein Xvfb, Surge nicht installiert**):
- Fensterablauf: anlegen, zeigen, verstecken, wieder zeigen, abbauen vor dem Entladen.
- Zeitgeber des Plugins laufen über `pump`.
- Ein gezogener Regler wird ein Schritt (alt/neu); ein noch laufender Zug meldet nichts; 400 Meldungen auf einmal werden ein „ganzer Zustand“ ohne Allokation im Audio-Thread; bei geschlossenem Fenster wird nichts gemeldet; Werte des Hosts kommen nicht zurück.
- Rückgängig und Wiederholen eines Fensterreglers und eines Preset-Wechsels stellen den Wert im laufenden Plugin wieder her.

Nicht selbst geprüft: ein echtes Plugin-Fenster (X11/Wayland), Surge XT, Fenster-Verhalten beim Schließen durch den Fenstermanager, Windows.

Bewusst so:
- Nur das schwebende Fenster; ein eingebettetes Fenster ist nicht gebaut. **Offen:** Ob Surge XT unter Linux ein schwebendes Fenster anbietet, ist nicht belegt (JUCE-Plugins sind oft nur eingebettet). Dann zeigt die App den Hinweis, und ein eingebettetes Fenster wäre ein eigenes Paket.
- **Keine Automation von Plugin-Reglern:** Das Modell hat dafür kein Ziel, das wäre eine Formatänderung. Fensterbewegungen gehen als Reglerwert in den Rückgängig-Verlauf, nicht als Kurve.
- Ardour wurde nicht gelesen; die Zusammenfassung zu Schritten ist ein eigener Entwurf (siehe `docs/herkunft.md`).
- Während die Wiedergabe steht, rechnet der Audio-Thread keine Blöcke; Reglermeldungen eines Plugins, das nur bei stehendem Transport sendet, kommen erst beim nächsten Rechnen an.

## Paket 31: CLAP-Plugins finden und laden (0.26.0, offen)

Aus M8. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. **Surge XT installieren:** Das Paket der Distribution oder von der Webseite des Projekts, als CLAP (`~/.clap` oder `/usr/lib/clap`; `CLAP_PATH` geht auch).
2. **Im Browser sehen:** Produzent-Modus, MIDI-Spur anlegen, in der Spur unter dem Kanalzug „+ Plugin“. Surge XT steht in der Liste (♪ als Instrument, fx als Effekt). Fehlt es, „Neu suchen“.
3. **Auf die Spur legen:** „♪ Surge XT“ wählen. Unter „PLUGINS“ steht der Platz mit „Regler“.
4. **Spielen:** Noten in einen Clip setzen oder die Spur scharf schalten und die Tastatur spielen; Abspielen. Einen Regler ändern, dann hört man es.
5. **Speichern und neu öffnen:** Speichern, Projekt schließen, wieder öffnen. Das Plugin ist geladen, Reglerwerte und Klangzustand stehen wie vorher. Ohne installiertes Surge XT steht „nicht installiert“, der Platz bleibt im Projekt.
6. **Effekt:** Auf einer Audio-Spur „fx“ eines Effekt-Plugins wählen; die Ein/Aus-Taste schaltet es um.

Selbst geprüft von Claude (ohne Ton, ohne Audiogerät, ohne Mikrofon; Offline-Render durch den echten Graphen mit einem Test-Plugin aus `clack-plugin`):
- Suche, Laden über `clack-host`, Instrument spielt den MIDI-Clip, Regler ändert den Pegel (Pegelvergleich), Effekt verdoppelt den Pegel bei Verstärkung 2 (auf 2 % genau), Bypass stellt das Signal wieder her.
- Fehlendes Plugin: Instrument schweigt, Effekt reicht durch, der Platz bleibt im Projekt.
- Zustand: Speichern des Plugin-Zustands ins Projekt, Laden in einer neuen Instanz gibt den Reglerwert zurück; Zustandstest der Oberfläche (Hinzufügen, Regler, Bypass, Entfernen, Rückgängig, Speichern, Öffnen).
- Der Graph rechnet 200 Blöcke unter `assert_no_alloc`.

Nicht selbst geprüft: Surge XT selbst (nicht installiert, Klang nur am Gerät), die Oberfläche im echten Bild, Wiedergabe über ein Audiogerät, Plugins mit mehr als einem Audio-Port.

Bewusst so:
- Kein Plugin-Fenster (kommt mit Paket 32); Regler stehen als Zahlenfelder (bis 64 je Plugin).
- Suche und Laden laufen im Programm selbst; ein abstürzendes Plugin kann die App mitreißen, bis Paket 33 es auslagert. Die Suche startet erst beim ersten Bedarf (Projekt mit Plugins oder „+ Plugin“).
- Spuren mit Plugins rechnen in Stereo; Pan-Automation wird dort ignoriert, der eingebaute Sidechain gilt nicht mit Instrument-Plugin.
- Nur der erste Audio-Port, höchstens zwei Kanäle. Keine Latenzkompensation (Paket 33), keine Transportinfo an Plugins.
- Ein fehlendes Instrument schweigt, ein fehlender Effekt reicht durch; der Platz bleibt im Projekt, im Projekt steht nur die CLAP-Kennung, nie ein Pfad.
- Beim Speichern, Autosave und Export wird der Plugin-Zustand erfasst; das kann einen Rückgängig-Schritt je Plugin erzeugen.
- Ardour und tracktion sind noch nicht gelesen (Pakete 32/33). Plugins in `woa-headless` und im Agenten sind nicht angebunden.
- Offen für André: Vital liegt nicht als CLAP vor; Odin 2 oder Dexed wären Alternativen zum Prüfen.

## Paket 30: Voice-Tracking-Editor (0.25.0, offen)

Aus M9. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. **Zwei Titel, dazwischen ein Voice-Track:** Radio-Modus, zwei Titel in die Playlist. Rechtsklick auf den ersten Titel, „Voice-Track danach einfügen …“, eine Sprachdatei wählen. Der Eintrag steht zwischen den Titeln, markiert „VOICE“.
2. **Sprachdatei importieren:** Rechtsklick auf den Voice-Track, „Voice-Track bearbeiten …“. Das Fenster zeigt Ausklang von A, Anfang von B, die Stimme und die Absenkungskurve. „Vorschlag“ setzt die Stimme so, dass sie am Ramp-Punkt von B endet.
3. **Stimme in den Ausklang schieben:** Stimme mit der Maus nach links ziehen, die rote Kurve zeigt die Absenkung der Musik. „Übernehmen“, dann **über Kopfhörer vorhören** („Vorhören“, braucht den Kopfhörer-Ausgang in den Einstellungen). Nie auf Sendung.
4. **Zweiten Titel tauschen:** Den Titel hinter dem Voice-Track entfernen und einen anderen einfügen. Der Voice-Track zeigt „prüfen“ und geht nicht auf Sendung; im Editor löscht „Prüfung bestätigen“ den Hinweis.

Selbst geprüft von Claude (ohne Ton, ohne Mikrofon, ohne Gerät, erzeugte Töne durch den echten Mixer offline):
- A, Stimme, B als Sinustöne mit verschiedener Frequenz (Goertzel-Messung je Ton): Die Stimme beginnt auf 0,06 s genau zum eingestellten Versatz, B beginnt zum Übergang, die Musik unter der Stimme ist um 12 dB abgesenkt (auf 3 % genau), die Rampe beginnt 0,4 s vor der Stimme, nach der Rückholzeit ist die Musik wieder unberührt, und über die ganze Strecke bleibt der Pegel hörbar (keine Lücke).
- Der Mixer läuft in diesen Tests in `assert_no_alloc`.
- Kommt der Übergang früher („Weiter“ nach 10 s), läuft die Stimme vollständig von vorn, nie angeschnitten.
- Tausch eines Nachbarn setzt „prüfen“, die Stimme wird nicht gesendet (kein Ton, Musik nicht abgesenkt); nach „Bestätigen“ läuft sie.
- Speichern und Laden als Schnappschuss (`insta`) samt Nachbarbindung; ein Titel, der sich nach dem Laden ändert, setzt „prüfen“.
- Editor als Zustandstest (Vorschlag, Verschieben mit Grenzen, Wellenformen), Fenster zeichnet ohne Display; Auswahl aus `moderation` wird Voice-Track.

Nicht selbst geprüft: das Fenster im echten Bild, der Klang der Absenkung bei echter Musik und Sprache, das Vorhören im Kopfhörer (nur der Offline-Render ist getestet, kein Gerät), das Ziehen mit der Maus und das Verhalten mit langen echten Dateien.

Bewusst so:
- Die Stimme läuft über den Cart-Regler; ein Voice-Track folgt damit dem Cart-Pegel.
- Eine Sprachdatei darf bis 300 s lang sein, die Stimme bis 60 s vor dem Übergang beginnen.
- Aufnahme der Stimme direkt im Editor und ein MCP-Werkzeug für Voice-Tracks sind nicht gebaut. Aufnahme braucht einen Mikrofontest am Gerät; das entscheidet André.

## Paket 29: Feste Zeit-Events und pünktliche volle Stunde (0.24.0, offen)

Aus M9. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. **Event anlegen:** Radio-Modus, Knopf „Zeit-Events“, „+ Event …“, eine kurze Audiodatei (Jingle) wählen. Es erscheint als Stundenjingle: Tage `*`, Stunden `*`, Minute 00, Sekunde 00, **hart**.
2. **Pünktlichkeit (Kopfhörer, nicht auf Sendung):** AutoDJ an, Sendung kurz vor der vollen Stunde laufen lassen. Unter der Uhr zählt „NÄCHSTES EVENT … in mm:ss (hart)“ herunter, in den letzten 30 s rot. Zur vollen Stunde blendet der laufende Titel schnell ab, das Jingle beginnt 0 bis 2 s nach der Zeit; danach steht „ZULETZT … +0,x s“.
3. **Weich:** Das Event auf „weich“ stellen und einen Zeitpunkt mitten in einem Titel wählen: das Event kommt erst nach dem laufenden Titel, der Titel wird nicht angeschnitten.
4. **Sommerzeit:** Bei Bedarf das Häkchen in „Stundenuhr“ prüfen; die Zeit-Events folgen derselben Einstellung.

Selbst geprüft von Claude (ohne Ton, ohne Gerät, alles mit Puffern und simulierter Uhr):
- Eine Stunde Sendung durch den echten Mixer mit 20-s-Titeln und Überblendung: hartes Event zur vollen Stunde nach 0,12 s gehört (erlaubt 0 bis 2 s), auch wenn es mitten in einer Überblendung liegt; weiches Event kommt nach dem Titel, vor ihm nur Musik; in der ganzen Stunde keine Lücke (kleinster Pegel über 50 ms bleibt hörbar); gemeldete Abweichung stimmt mit der Messung überein.
- Der Mixer läuft in diesen Tests in `assert_no_alloc`: kein Speicher im Audio-Thread beim Event-Start.
- Eine Woche Events mit simulierter Uhr (ohne Audio, `EventClock`): jedes Event genau einmal, über Mitternacht und über die Zeitumstellung im Oktober; Verspätungsgrenzen (hart 20 s, weich 30 min) führen zu „verpasst“, nicht zu Doppelauslösung. Die Woche läuft nicht durch den Mixer; dort gilt nur die eine Stunde oben.
- Mixxx-Randfälle als Einheitentests (`transition.rs`): kurzer Folgetitel (4 s gibt 1 s Überblendung statt der Hälfte), fehlende Marken, Titel kürzer als Mindestlänge, ungültige Werte.
- Rückwärts planender AutoDJ: vor einem harten Event wird die Füllung so gewählt, dass der Schnitt klein bleibt (schlechtester Schnitt höchstens 100 s und besser als ohne Planung).
- Oberfläche: Zustandstests des Editors (Anlegen, Muster, Speichern, Laden, kaputte Datei wird nicht überschrieben, Zone folgt der Stundenuhr); Fenster zeichnet ohne Display.

Nicht selbst geprüft: das Fenster im echten Bild, Hörbarkeit und Klang des schnellen Abblendens (250 ms) bei echter Musik, das Verhalten über Stunden mit echten Dateien und echtem Gerät, und die Zeitumstellung am echten Tag (nur simuliert).

Bewusst so:
- **Hart** heißt: das Event beginnt zur genauen Zeit, der alte Titel blendet in 250 ms ab; **weich**: direkt nach dem laufenden Titel (bis 30 min Verspätung, danach verpasst).
- **Sommerzeit:** Ein Event mit fester Uhrzeit in der doppelten Stunde im Oktober läuft nur einmal (im ersten Durchgang). Ein Event in der ausgefallenen Stunde im März läuft im Moment der Umstellung. Ein Event für jede Stunde (Stundenjingle) läuft jede echte Stunde, also im Oktober um 02:00 zweimal.
- Events spielen nur eine lokale Datei. Eine Nachrichtenquelle (Audio-Feed eines Anbieters) ist ein externer Dienst und nicht gebaut; das entscheidet André.
- Endet der vorherige Titel vor der Zeit, bleibt bis etwa 1 s Stille vor dem Event.
- Kurzer Folgetitel: Mixxx-Überblendung (siehe CHANGELOG 0.24.0).

## Paket 28: Stundenuhr und AutoDJ-Rotation (0.23.0, offen)

Aus M9. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. **Titel taggen:** „Bibliothek“ öffnen, einige Titel mit den Tags `hit`, `neu` und `jingle` versehen (mindestens je ein paar Stück; für den Notfall ein paar mit `notfall`). Interpret steht im Titel als „Interpret - Titel“, sonst gilt nur die Titel-Sperre.
2. **Stundenuhr anlegen und zuordnen:** Radio-Modus, Knopf „Stundenuhr“. Die Uhr „Standard“ (hit, hit, neu, jingle) anpassen oder eine neue anlegen, im Wochenraster die aktuelle Stunde anklicken (umrandet), oder „Gewählte Uhr für die ganze Woche“. Zeitverschiebung prüfen (Grundwert 60; Sommerzeit schaltet seit 0.24.0 selbst, Häkchen „Sommerzeit“).
3. **AutoDJ an (Kopfhörer):** „AutoDJ an“ klicken. Die Playlist füllt sich hinten auf drei Titel; beim Abspielen (über den Kopfhörer, nicht auf Sendung) folgt sie der Uhr: hit, hit, neu, jingle, und kein Titel oder Interpret kommt zu früh wieder. Die Liste „Letzte Auswahlen“ nennt zu jedem Titel Slot und Kategorie.
4. **Eigenen Titel einschieben:** Einen Titel von Hand in die Playlist ziehen oder einfügen: er bleibt stehen, der AutoDJ hängt nur hinten an. Eine Kategorie ohne Titel (z. B. `werbung`) fällt auf die Ersatzkategorie zurück.

Selbst geprüft von Claude (ohne Ton, ohne Gerät): 24 Stunden mit simulierter Uhr gegen 500 erzeugte Titel: keine Sperrfrist für Titel (180 min) oder Interpret (60 min) verletzt, in jeder der 24 Stunden folgen die Slots der Uhr der Reihe nach; leere Kategorie fällt auf die Ersatzkategorie, dann auf den Notfall-Tag zurück, ist auch der leer, kommt keine Wahl und kein Absturz; gleicher Startwert gibt dieselbe Folge, ein anderer eine andere; die Historie liegt in SQLite und überlebt das Schließen und Öffnen der Datenbank (Sperren und Position in der Stunde gelten danach weiter); von Hand gesetzte Titel werden nicht gewählt und belegen ihre Zeit; Dateien mit kaputtem oder feindlichem Inhalt werden gemeldet, nie überschrieben; Editor als Zustandstest (Uhren, Slots, Wochenraster, Umbenennen nimmt die Woche mit, Speichern und Laden); der AutoDJ-Takt gegen eine nachgebildete Sendung (Auffüllen bis zur Zahl im Voraus, Historie genau einmal je Titel); jede Wahl steht im Log.

Nicht selbst geprüft: das Fenster im echten Bild (Aussehen, Wochenraster, Klickverhalten), das Zusammenspiel mit dem echten Sendemixer über Stunden (nur der Takt gegen nachgebildete Sendedaten), und wie die Folge mit echter Musik klingt. Die Zeitzone ist eine Zahl in Minuten; die Sommerzeitumstellung ist seit 0.24.0 automatisch.

Noch nicht drin (gehört zu M9, nicht zu diesem Paket): feste Zeit-Events (Nachrichten, Werbung, Stundenjingle zur vollen Stunde) und der Voice-Tracking-Editor. Gespielte Titel werden nur eingetragen, solange die Sendung läuft und der Titel in der Bibliothek steht.

## Paket 27: DAWproject Import/Export (0.22.0, offen)

Aus M7. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. **Als DAWproject exportieren:** Ein Projekt mit mehreren Spuren (Audio, MIDI, ein Bus mit Effekt, ein Send) öffnen, Datei, „Als DAWproject exportieren …“, Ziel wählen. Die Statuszeile nennt Spuren, Clips, Audiodateien und eventuell Hinweise zu nicht Übertragenem.
2. **In Bitwig oder Studio One öffnen:** Die Datei dort öffnen (über den Öffnen- oder Import-Eintrag des Programms). Es sollten Spuren, Clips an ihrer Stelle, Lautstärke, Pan, Mute, Sends und die Lautstärke-Automation da sein. Unsere Effekte erscheinen dort nicht (sie sind WOA-eigen), das ist so gewollt. Cubase kann DAWproject laut Hersteller ebenfalls, die Version bitte selbst prüfen.
3. **Fremde Datei in WOA öffnen:** In Bitwig oder Studio One ein kleines Projekt als `.dawproject` speichern, in WOA Datei, „DAWproject öffnen …“. Es entsteht ein Ordner `<Name>.woa` neben der Datei und wird geöffnet. Fremde Plugins fehlen (siehe Log), alles andere sollte stimmen.
4. **Zurück nach WOA:** Die eigene Exportdatei aus Schritt 1 wieder öffnen: Tonhöhe, Clip-Verstärkung, Blendenform und „Tempo folgen“ der Clips sind wieder da.

Selbst geprüft von Claude (ohne Ton): Rundreise WOA nach DAWproject nach WOA mit Audio-, MIDI- und Bus-Spuren, Pegel, Pan, Mute, Ausgang, Send, Effekten mit allen Reglern, Clip-Werten (Position, Länge, Versatz, Blenden, Dehnung, Gain, Tonhöhe, Tempo folgen), MIDI-Noten und Lautstärke-Automation, jeweils mit Werten verglichen; `project.xml` besteht die Prüfung gegen das DAWproject-Schema 1.0 (`xmllint`); eine handgebaute Datei nach den Konventionen der Referenztests von Bitwig (Lautstärke linear, Pan normalisiert, Master als Track, Audio über Warps) wird richtig gelesen, das fremde Plugin steht im Bericht; kaputte oder feindliche Dateien (kein ZIP, ohne `project.xml`, zu tief verschachtelt, falsche Wurzel, `../` im Audiopfad) enden mit klarer Meldung und schreiben nichts außerhalb des Zielordners.

Nicht selbst geprüft: das Öffnen in Bitwig, Studio One oder Cubase (ich habe keine davon), also ob die Gegenseite alle Konventionen so liest, wie die Referenztests es zeigen. Offizielle Beispieldateien gibt es im DAWproject-Repository nicht (nicht gefunden); geprüft wurde gegen das Schema und gegen eine nach der Referenz gebaute Datei. Der Export läuft im Oberflächen-Thread: bei sehr großen Audiodateien kann die Oberfläche kurz stehen.

Bewusst so: DAWproject kennt keine Erweiterungen, deshalb stehen Tonhöhe, Gain, Blendenform und „Tempo folgen“ als Text im Kommentar des Clips (`woa: …`). Warp-Punkte außer Anfang und Ende, Marker, Szenen, Tempo-Automation und fremde Plugins werden nicht übernommen, aber im Log genannt. Automation von Lautstärke wird linear gespeichert; ob andere Programme zwischen den Punkten in dB oder linear interpolieren, ist offen.

Offene Frage an André: Hast du Bitwig, Studio One oder Cubase für den Gegentest? Ohne eine davon bleibt „öffnet dort richtig“ eine Annahme nach der Referenz.

## Paket 26: Alle Export-Formate und Stems (0.21.0, offen)

Aus M7. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. **Opus exportieren:** Ein Projekt mit Audio öffnen, Strg+E. Im Fenster „Exportieren“ das Format **Opus** wählen (ist es grau, fehlt ffmpeg: Hinweis beim Darüberfahren, dann ffmpeg installieren und das Fenster neu öffnen), „Datei wählen und exportieren …“. Die `.opus`-Datei in einem Player öffnen: klingt wie das Projekt, Länge stimmt. Dasselbe mit Ogg Vorbis und AAC, und mit WAV 16 Bit, 24 Bit und 32 Bit Float sowie FLAC 16 und 24 Bit.
2. **Stems exportieren:** Strg+E, Haken „Stems: eine Datei je Spur in einen Ordner“, „Busse dazu“ und „Mixdown dazu“ nach Wunsch, Ordner wählen. Es entsteht ein Unterordner „<Projekt> Stems“ mit einer Datei je Spur („01 Bass.wav“, „02 …“), je Bus eine und „00 Mixdown“. Stumme Spuren fehlen, die Statuszeile nennt sie.
3. **In einer anderen DAW anlegen:** Alle Dateien (ohne den Mixdown) in einer anderen DAW auf getrennte Spuren an Position 0 legen. Sie liegen deckungsgleich und klingen zusammen wie der Mixdown, solange keine Spur über einen Bus mit Effekt läuft. Läuft eine Spur in einen Bus, steckt sie schon im Bus-Stem: dann entweder „Busse dazu“ abwählen oder die Spur-Dateien und den Bus nicht beide anlegen.

Selbst geprüft von Claude (ohne Ton, im Offline-Render): Stems und Mixdown in einem Durchgang über die Abgriffe des Graphen; alle Stems gleich lang wie der Mixdown; **die Summe der obersten Stems (Spuren und Busse mit Ausgang Master) ist der Mixdown auf 1e-4 genau**, getestet mit einem Projekt aus direkter Spur, Spur über Bus, Send auf den Bus, gedrehtem Pan, Pegel und einer stummen Spur (die keine Datei bekommt); die Dateien werden zurückgelesen und die Summe der zurückgelesenen Dateien stimmt. Lautheitsangleich gilt mit demselben Gain für alle Dateien (Pegelverhältnis −6 dB gemessen). WAV 16/24/32f und FLAC 16/24 gelesen: Länge gleich, Fehler höchstens eine Quantisierungsstufe, Dateigrößen wachsen mit der Breite. FLAC mit Float wird abgelehnt und hinterlässt keine Datei. Opus, Ogg Vorbis und AAC über das echte ffmpeg: Dateikopf (OggS, ADTS), Rücklesen über ffmpeg, Länge auf 100 ms, Pegel auf 12 %, rechter Kanal bleibt still, wenn nur links Signal ist. **Fehlt ffmpeg, kommt der Text „ffmpeg nicht gefunden: …“ für jedes der drei Formate und es bleibt keine halbe Datei zurück**, auch bei Stems (keine Datei im Ordner). Dateinamen: laufende Nummer vorn, Sonderzeichen, `..`, reservierte Windows-Namen und Überlänge bereinigt; ein Symlink als Ziel wird abgelehnt. Export-Fenster als Zustandstest (ausgegraut ohne ffmpeg oder ohne Encoder, Wahl wird zurückgesetzt, Grenzen der Bitrate) und Zeichnen ohne Fenster. Der Agent-Export (`export`) kennt `.opus`, `.ogg`, `.aac` mit derselben Fehlermeldung.

Nicht selbst geprüft: der Klang der verlustbehafteten Formate (kein Ton auf dem Notebook), das Öffnen der Dateien in anderen Playern und DAWs, das Aussehen des Export-Fensters und der Dialoge im echten Fenster. Die Tests mit ffmpeg laufen in der CI nur, wo ffmpeg installiert ist (sonst übersprungen mit einer Logzeile); lokal wurden sie mit ffmpeg ausgeführt. Der Export läuft wie bisher im Oberflächen-Thread: bei langen Projekten mit Opus kann die Oberfläche einige Sekunden stehen.

Bewusst so: **FLAC kann keine Fließkommazahlen speichern** (nur ganze Zahlen, bis 24 Bit hier); „32 Bit Float“ gibt es deshalb nur als WAV. AAC wird als `.aac` (ADTS) geschrieben, nicht als `.m4a`, weil der MP4-Container ein suchbares Ziel braucht. Opus lesen kann WOA weiterhin nicht (symphonia hat keinen Opus-Decoder); Opus-Dateien lassen sich nur über ffmpeg zurücklesen (so prüfen es die Tests), in die Bibliothek importieren geht sie noch nicht.

Offene Frage an André (nicht gebaut, weil es eine Entscheidung über Lizenz und Größe ist): ffmpeg im Installer mitliefern (LGPL-Build, etwa 80 MB, bei AAC eine Patentfrage unter Windows) oder vom Nutzer installieren lassen? Bis dahin sucht WOA ffmpeg in `WOA_FFMPEG`, neben dem Programm und im `PATH`.

Noch nicht drin: Stems über den Agenten (MCP), Opus-Import in die Bibliothek, Export im Hintergrund-Thread mit Fortschritt.

## Paket 25: Analyse: BPM, Tonart, Lautheit, automatische Cue-Punkte (0.20.0, offen)

Aus M7. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. **Spalten ansehen:** „Bibliothek“ öffnen, einen Ordner mit Musik importieren (oder den schon importierten lassen). Unten läuft „analysiert: n von m“ im Hintergrund, die App bleibt bedienbar. In der Liste füllen sich die Spalten BPM, TONART und LUFS (bis dahin „…“, „–“ heißt: nicht bestimmbar).
2. **Bekannten Song prüfen:** Einen Song wählen, dessen Tempo und Tonart du kennst. Stimmt das Tempo (±1; bei halbem oder doppeltem Tempo sag es mir) und die Tonart (Dur/Moll)? Unter der Liste steht die ganze Analyse in einer Zeile, „Neu analysieren“ misst noch einmal.
3. **Laut und leise nacheinander:** Radio-Modus, einen sehr lauten und einen sehr leisen Titel nacheinander in die Playlist ziehen und abspielen (Probe über Kopfhörer, nicht auf Sendung): beide wirken gleich laut, die Lautheitsanzeige bleibt um −16 LUFS.
4. **Auto-Cues im Cue-Editor:** In der Playlist bei einem Titel mit Stille am Anfang oder Ende den Cue-Editor öffnen: blasse Linien „(Vorschlag)“ stehen hinter der Stille vorn, vor der Stille hinten und am Beginn des Ausklangs. „Vorschlag einsetzen“ übernimmt sie; schon von dir gesetzte Marker bleiben unverändert.

Selbst geprüft von Claude (ohne Ton): Klicktrack 120 BPM gibt 120 ±1, weitere Tempi und Abtastraten; Sinus ohne Einsätze, Stille und zu kurzes Material geben kein Tempo; Sinus-Akkorde geben ihre Tonart; Stille vorn und hinten wird gefunden, Cue-Vorschläge überspringen sie und finden den Ausklang; EBU Tech 3341 (1 kHz, −23 dBFS) gibt −23,0 LUFS, auch über den Dateiweg; 1 000 erzeugte Titel unterschiedlicher Lautstärke werden im Hintergrund analysiert (keine Fehler), mit dem berechneten Gain liegen alle auf dem Ziel; kaputte Datei wird markiert und nicht wiederholt, geänderter Inhalt macht den Titel wieder offen, Migration von Schema 2 behält die Titel; von Hand gesetzte Cues werden nie überschrieben; Bibliothek-Fenster und Cue-Editor als Zustandstests ohne Fenster. Alles im Hintergrund-Thread, nichts im Audio-Thread.

Nicht selbst geprüft: Treffsicherheit an echter Musik (nur erzeugte Signale; bei dichter Musik, Tempowechseln und Modulation sind Tempo und Tonart Näherungen, halbes oder doppeltes Tempo ist möglich), Geschwindigkeit mit echten MP3-Titeln, Aussehen und Spaltenbreiten im echten Fenster, der Klang des Lautheitsangleichs.

Noch nicht drin: Tempo und Tonart im Radio-Modus anzeigen, Sortieren nach Spalten, Analysewerte im Agenten (MCP).

## Paket 24: Bibliothek in SQLite mit Suche und Tags (0.19.0, offen)

Aus M7. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. **Bibliothek öffnen, Ordner wählen:** Oben in der Menüleiste „Bibliothek“ anklicken. Im Fenster „Ordner importieren …“, einen Ordner mit Musik wählen (Art vorher: Musik, Sample oder Jingle). Ein Fortschritt „n von m“ läuft, die Liste füllt sich; die App bleibt bedienbar. Danach steht oben „… neu“. Denselben Ordner nochmal importieren: „… schon vorhanden“, nichts doppelt.
2. **Suchen:** Im Suchfeld tippen, schon ab dem ersten Buchstaben (auch Teile von Wörtern, auch Ordnernamen): die Liste zeigt nur Treffer. Groß-/Kleinschreibung und Akzente sind egal.
3. **Tag vergeben und filtern:** Titel anklicken, unten „neuer Tag“ eintippen, Enter oder „+ Tag“. Oben im Filter „Alle Tags“ den Tag wählen: nur diese Titel bleiben. Tippt man den Tag ins Suchfeld, findet die Suche ihn auch. Das ✕ am Tag nimmt ihn wieder weg.
4. **Titel in die Playlist ziehen:** In den Radio-Modus wechseln (Bibliothek bleibt offen), einen Titel mit der Maus auf die Sende-Playlist ziehen und loslassen: er steht am Ende. Auf ein Cart-Pad ziehen belegt das Pad. Im Produzenten-Layout auf eine Spur ziehen legt den Titel als Clip dort ab, wo der Zeiger loslässt.
5. **Vorhören:** Titel anklicken, „▶ Vorhören“ (nur im Radio-Modus mit eingestelltem Kopfhörer-Ausgang unter Einstellungen → Vorhören): läuft im Kopfhörer, nie auf Sendung. „■“ hält an.
6. **Neu starten:** App schließen und öffnen, Bibliothek öffnen: die Titel und Tags sind noch da.

Selbst geprüft von Claude (ohne Ton): 1 000 erzeugte Sinus-Dateien in 10 Ordnern plus eine Textdatei und eine kaputte WAV werden im Hintergrund importiert (1 000 neu, 1 nicht lesbar, Zeit im Testlauf unter 2 s); Suche nach „titel 0042“ findet genau einen Treffer mit richtiger Dauer, „album_3“ genau die 100 Titel des Ordners; zweiter Import: 1 000 Doppelte, 0 neu, Zählung bleibt 1 000. Kopie unter anderem Namen wird am Inhalt erkannt, verschobene Datei zieht den Eintrag nach, geänderte aktualisiert ihn. Suche mit Präfixen, Akzenten, Groß-/Kleinschreibung und feindlichem Text (Anführungszeichen, `OR`, `NEAR(`, `'; DROP TABLE`), Tags (Normalform, Grenzen, Filter, Suchindex, Entfernen), Art, Rücksetzen einer Transaktion, Migration einer Schema-1-Datenbank, Ablehnen einer neueren, Symlink-Schleife, Abbruch, fehlender Ordner. Fenster-Zustand (Import, Suche, Tag, Filter, Fehlerfall) und Zeichnen mit Treffern ohne Fenster. Alles im Hintergrund-Thread bzw. Control-Thread, nichts im Audio-Thread; Vorhören nutzt den vorhandenen PFL-Weg unverändert.

Nicht selbst geprüft: das Ziehen mit der Maus im echten Fenster (Zeigerereignisse nicht simuliert; die Ablage-Logik auf Playlist und Cart-Pads ist nur durch Übersetzen und Lesen geprüft), das Vorhören im Kopfhörer (kein Ton auf dem Notebook), Aussehen und Griffigkeit des Fensters, Geschwindigkeit mit echten MP3-Dateien (die Tests nutzen kurze WAVs; echte Titel brauchen länger, weil jede Datei zur Dauerbestimmung dekodiert wird).

Noch nicht drin (folgende Pakete): Titel aus der Bibliothek entfernen; Vorhören im Produzenten-Layout ohne Radio-Modus.

## Paket 23: Time-Stretch, Tonhöhe, Abnahme ganzer Song (0.18.0, offen)

Aus M6, letztes Paket. Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. **Um 10 % dehnen:** Eine Audiodatei mit einem Ton oder einer Melodie in die Timeline ziehen. Am rechten Rand des Clips mit gedrückter **Alt**-Taste ziehen, bis die Länge 10 % größer ist (in der Clip-Leiste unter „ZEIT“ steht dann etwa „Dehnung 110 %“; man kann die Zahl auch eintippen). Abspielen: der Ton ist langsamer, aber **nicht tiefer**. Ohne Alt kürzt derselbe Zug nur, wie bisher. Strg+Z holt die alte Länge zurück.
2. **+2 Halbtöne:** In der Clip-Leiste „Tonhöhe“ auf +2,0 stellen. Der Ton wird höher, die Länge bleibt. Doppelklick auf das Feld setzt auf 0 zurück, dann klingt es wieder wie das Original.
3. **Tempo 120 auf 100:** Beim Clip den Haken „Tempo folgen“ setzen, dann oben das Tempo von 120 auf 100 stellen. Der Ton wird gedehnt und bleibt auf dem Raster, die Tonhöhe bleibt. Ohne den Haken bliebe der Ton in seiner Geschwindigkeit (wie bisher).
4. **Exportieren:** Strg+E, WAV. Die Datei in einem Player öffnen: Dehnung, Tonhöhe und das neue Tempo klingen wie in der Vorschau, keine Knackser an den Nahtstellen.
5. **Ganzer Song (M6-Abnahme):** Eine Spur einspielen oder aufnehmen, an zwei Stellen schneiden (S), einen Teil dehnen, einen Teil transponieren, zwei Clips überlappen lassen, exportieren, Projekt speichern und neu öffnen: alles steht wie zuvor.

Selbst geprüft von Claude: Gemessen im Offline-Render, nichts gehört. Sinus 440 Hz um 10 % gedehnt: Länge innerhalb 1 % von 4,4 Beats, Frequenz bleibt bei 440 Hz ±1 % (FFT-Spitze); weitere Verhältnisse 0,5, 0,9, 1,5 und 2 auf Länge (auf das Sample genau) und Frequenz; Pegel nach dem Dehnen innerhalb 3 %. Tonhöhe +2, −2, +7, ±12 Halbtöne: FFT-Spitze bei 440·2^(n/12) Hz ±1 %, Länge unverändert; Dehnen und Transponieren zusammen. Tempo 120 auf 100 mit Folgen: vier Beats ergeben 115 200 Samples (±1 %), Frequenz bleibt; gleiches Tempo und 0 Halbtöne lassen den Render bitgleich, Undo ebenso. Jeder Process-Aufruf läuft unter `assert_no_alloc` (das Rechnen geschieht vorher im Control-Thread). Modell: Befehle mit Undo und Redo, Grenzen (Faktor 0,25 bis 4, ±24 Halbtöne, Tempo 20 bis 400), Fades werden mit zurückgesetzt, Projektformat 2 bleibt ohne neue Felder, Rundlauf Speichern und Laden. Oberfläche ohne Fenster und ohne Ton: Alt-Ziehen am Rand dehnt (ein Undo-Schritt, Anfang bleibt, Ton nicht abgeschnitten), ohne Alt kürzt es; Schneiden eines gedehnten Clips setzt den Ton an der richtigen Stelle fort. Agent-Werkzeuge über MCP einschließlich Fehlerfällen. M6-Abnahme „ganzer Song“ als Test: geteilte, gedehnte, transponierte und überblendete Clips bei geändertem Tempo exportiert und wieder eingelesen (Länge, kein NaN, keine Knackser, Pegel, Frequenz der transponierten Strecke).

Nicht selbst geprüft: Klang (kein Ton auf dem Notebook). Die Dehnung ist ein eigenes WSOLA-Verfahren: bei Sinus und einfachen Tönen messbar sauber, bei dichter Musik, Schlagzeug und Stimmen können bei starker Dehnung (über etwa ±30 %) leichte Schwebungen oder verschmierte Anschläge hörbar sein. Das hört nur André. Aussehen und Griffigkeit des Alt-Ziehens im echten Fenster (mit simulierten Zeigerereignissen getestet, nicht von Hand). Bei Tonhöhe über +12 Halbtöne kann es oberhalb von etwa 18 kHz zu leichtem Aliasing kommen (Neuabtastung ohne Tiefpass).

Offene Frage an André (nicht gebaut, weil es eine Wahl ist): Wenn WSOLA bei Musik zu hörbar ist, kann stattdessen `signalsmith-stretch` eingebunden werden (MIT, bessere Qualität, aber C++-Bindung und ein Windows-CI-Check). Erst nach seinem Hörtest entscheiden.

Damit ist M6 (Arrangement und Aufnahme) vollständig gebaut; Latenz-Messung per Kabel-Rückschleife bleibt, bis André zu Hause ist (Paket 22).

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
2. **Fader und zwei Pads lernen:** Radio-Layout. Rechtsklick auf „Pegel Musik“ → „MIDI lernen“, einen Fader bewegen. Rechtsklick auf eine belegte Cart-Taste (Jingle) → „MIDI lernen“, Pad drücken; dasselbe mit „▶ Play“ und einem zweiten Pad. Im Controller-Fenster stehen die drei Zeilen.
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
