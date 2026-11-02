# Auftrag: Design System (Fassung 4) vollständig umsetzen

Du arbeitest im Repo `daw.woa`. Lies zuerst `CLAUDE.md`, `docs/konzept.md` und danach komplett `docs/design/woa-design-system/README.md`.

## 0. Ablegen

1. Branch `design/fassung-4` von `origin/main`.
2. `woa-design-system.zip` (liegt bei mir unter `~/Downloads/`) nach `docs/design/woa-design-system.zip` kopieren (ersetzt die alte) und nach `docs/design/woa-design-system/` entpacken (alten Ordner vorher löschen).
3. `ist-soll.zip` nach `docs/design/` entpacken:
   - `ist/` sind Screenshots von 0.35.10/0.35.11 (auch Andrés eigene, 1920 × 1030).
   - `soll/` sind die gerenderten Entwürfe in derselben Größe mit denselben Inhalten.
   - Lege sie nebeneinander, bevor du anfängst.
4. `docs/herkunft.md`: Phosphor Icons 2.1.1 (MIT, `@phosphor-icons/core`) ergänzen.
5. `docs/design/luecken.md`: Pan/Trim-Entscheidung wie unten in Abschnitt 4 festhalten.

## 1. Ziel und Maßstab

Die Oberfläche soll aussehen und sich bedienen wie die Vorlagen. Nicht ungefähr, nicht in Teilen.

- **Maßstab** ist jede `components/<Baustein>/preview.html` (im Browser öffnen) samt ihrem `README.md`. Die `Screen*`-Entwürfe sind das Zielbild je Modus.
- **Tokens** (`tokens.json`) sind verbindlich: Farben je Thema (`studio`, `radio`, `party`, je `-hell`), Abstände, Radien, Schatten, Größen. Keine eigenen Farbwerte im Code.
- **Jeder Baustein mit jedem Zustand**, den sein README nennt, wird gebaut (z. B. RadioDecks: läuft, geladen, leer, Ablegen, < 10 s).
- **Kein „später“, kein „vorerst“:** Was die Vorlage zeigt, steht am Ende in der App, oder es steht begründet in `docs/design/umsetzung.md` als offene Frage an André.
- **Prüfliste:** Lege `docs/design/umsetzung.md` an, eine Tabelle Baustein × Zustand × Stelle in der App × Stand (✓ / offen). Fertig ist das Paket erst, wenn alles ✓ ist.

## 2. Was heute nicht passt (Ist, siehe `docs/design/ist/`)

### Produzent

Der Kanalzug im Mixer ist gut und bleibt so. Alles andere:

- **Knopfreihe unter den Spuren raus:** „+ Audio-Spur, + MIDI-Spur, Audio importieren, Produktionen, + MIDI-Clip, Akkordfolge …, Jingle-FX …, Jingle bauen …“ ist eine Text-Knopfreihe.
  - Neue Spuren über das „+“ im Lineal-Kopf (Menü Audio / MIDI / Bus).
  - Erzeugen über den **Browser** links (`Browser`).
- **MIDI-Spuren zeigen „Mikrofon“** als Eingang. Richtig ist das MIDI-Gerät mit Klaviatur-Symbol (`ChordTrack`, `TrackHeader`).
- **Name und „MIDI“-Badge überlappen** im Spurkopf („AkkordeMIDI“). Das Badge entfällt; die Art zeigt das Symbol im Eingangsfeld.
- **Akkord-Clips ohne Akkordnamen.** Soll:
  - Akkordspur unter dem Lineal.
  - Akkordnamen je Takt im Clip.
  - Klang im Clip-Kopf (`ChordTrack`).
- **Klang als Auswahlfeld zwischen den Inserts.** Soll: eigener Abschnitt „Instrument“ zwischen Eingang und Inserts mit Rollen Lead/Bass/Pad/Kit, Kacheln und Probeton (`SoundPicker`).
- **Piano-Roll:**
  - Noten sind blau statt in Spurfarbe.
  - Die Überschrift „Drum-Raster“ steht auch bei Akkorden.
  - Akkord-Chips im Lineal und Tonart-Hervorhebung fehlen.
  - Leiste nach `PianoRoll`.
  - Kit-Spuren öffnen `DrumGrid` (16 Schritte, laut/leise, Lauflicht).
- **Fenster „Akkordfolge“ und „Jingle-FX“** sind rohe egui-Fenster mit Radio-Buttons und Slider. Sie werden Browser-Reiter „Akkorde“ und „FX“:
  - Akkorde: Folgen mit Akkord-Chips, Tonart als 12 Tasten, Takte als Segmentschalter, drei Ausgaben, „Einfügen · Takt n“.
  - FX: Formkarten mit Kurve.
- **„Jingle bauen“:** Schrittleiste, Wahlkarten mit Symbol, Zusammenfassung unten (`JingleWizard`).
- **Neue Funktionen sichtbar machen:**
  - Freesound-Samples und Sound-Bänke: Browser-Reiter „Samples“ mit Lizenz-Chip und Ladezustand.
  - Drum-Muster: Reiter „Beats“ mit Mini-Raster.
- **Clip-Inspektor:** gibt es, aber die Werte stehen als „0.00“ ohne Einheit.
  - Felder nach `NumberField` (Einheit ms/dB/st, Symbol davor).
  - Werkzeuge als `IconButton` mit Tooltip.
  - Löschen rechts mit Abstand.

### Radio

- **Decks A · B · C:**
  - Heute: kleiner Buchstabe, `--:--` in normaler Schrift, rohe egui-Knöpfe „Start / Cue / Vorhören / ↩ Playlist“, Text „leer: Datei hierher ziehen“ abgeschnitten.
  - Soll `RadioDecks`: Buchstabe im LCD-Feld, 7-Segment-Restzeit mit Balken, Titel und Interpret, Hardware-Tasten (Start/Stopp als eine Taste, Cue und PFL als Symbol-Tasten, Playlist-Rückkehr als Symbol-Schalter).
  - Leere Decks gestrichelt, Ablege-Zustand beim Ziehen.
- **Pegelregler rechts daneben:** winzig, Beschriftung 9 px, keine Namen. Soll `RadioLevels`: Kanalzüge wie im Produzenten-Mixer (Fader, LED-Kette, Scribble-Strip in Spurfarbe), Gruppen Quellen · Master · Ausgänge.
- **Knopfzeile im Sende-Deck** (Play, Stopp, Deck A/B/C, ⏮ ⏭ ↻ −10 s +10 s …) ist eine Text-Knopfwüste:
  - „Deck A/B/C“ entfällt, das machen die Decks selbst.
  - Der Rest als `Key`-Hardware-Tasten nach `ScreenRadio`: Start, Blende, Mikro groß; Springen als Symbol-Tasten.
- **Fehlerbanner** (Audiogerät fehlt) nimmt drei Zeilen ein. Soll: eine Zeile mit Warnsymbol und „Details“ (Tooltip/Klick).

### Party

- **Deck-Karten:**
  - Heute: Text, kleiner Plattenteller rechts, grüner „Play“-Knopf.
  - Soll `Deck`/`ScreenParty`: Plattenteller links groß, Titel, BPM-LCD (wenn analysiert), Wellenform mit Fortschritt und Beatgrid, Tasten Play/Pause (`accent`, pulsiert im Beat), Cue, Sync.
  - Status-Badge LÄUFT / BEREIT / VORGELADEN.
- **Tastenfeld:**
  - Heute: 12 gleiche Knöpfe, „Play“ grün gefüllt.
  - Soll `PartyControls`: drei Gruppen (Abspielen, Springen, Lautstärke) mit Gruppentitel. Zustand nur über Füllung in `accent` (läuft) bzw. `accent-2` (Schalter). Kein Grün für Play.
  - Lautstärke als LCD-Anzeige zwischen Leiser und Lauter statt Text „Lautstärke +0 dB“.
- **Auto-Übergang und Zufall** als `accent-2`-Schalter links neben dem Crossfader. Crossfader nach `Fader` (horizontal, geriffelte Kappe).
- **Fernbedienung:**
  - Heute: grüner Vollbreiten-Balken „Fernbedienung an“.
  - Soll: Karte mit QR-Code, Adresse, PIN, Anzahl Handys, Schalter als `Key`.
- **Bibliothek-Suche und Wunsch-Eingabe** als `woa-input`-Stil (Höhe 44, Radius `radius-party`), Wunschliste mit Vote-Pillen.

### Kopfleiste (alle Modi)

- **Heute:** einzelne Kästchen MIDI · ON AIR · REC · KI und getrennt die Modus-Knöpfe, unterschiedliche Größen; Produzent/Radio/Party werden farbig gefüllt (Schrift über Farbe), Feedback grün gefüllt.
- **Soll `TopBar`:** Links Logo und Menüs, rechts **ein** eingelassener Block.
  - Gruppen: [Produzent | Radio | Party] ┊ [MIDI | On Air | Rec | KI] ┊ [Feedback].
  - Alle Einträge im selben Stil und in derselben Größe (28 px, Versalien, LED-Punkt). Nur die Farbe unterscheidet sie: Modi in ihrer Modusfarbe, MIDI `cue`, On Air und Rec `on-air`, KI `ok`, Feedback neutral mit Symbol.
  - Aktiv = Fläche `surface`, Rahmen und LED in der Farbe; nie gefüllte Farbflächen.

## 2a. Zielbild 1920 × 1030 (Fassung 4)

André arbeitet mit 1920 × 1030. Die Entwürfe `ScreenStudio`, `ScreenPianoRoll`, `ScreenRadio` und `ScreenParty` zeigen genau diese Größe mit seinen echten Inhalten (`docs/design/soll/*.png`). Jede Ansicht muss bei 1920 × 1030 ohne Scrollen so aussehen; kleiner darf sie umbrechen, größer wachsen die Listen.

**Harmonie-Regeln (aus `README.md`, Abschnitt „Arbeitsfläche 1920 × 1030“):**

- **Ein Raster:** 10 px Rand und Abstand, jede Fläche ist eine Karte mit Kopfzeile.
- **Drei Tastenhöhen:** 24 · 32 · 44 px; Party 48–64 px.
- **Keine grünen Vollflächen:** „Play“, „Fernbedienung an“, Feedback und die Cart-Pads sind heute grün gefüllt.
  - Gefüllt wird nur, was läuft (`accent`), eingeschaltet ist (`accent-soft`/`accent-2`) oder live ist (`on-air-fill`).
- **Kein Rot für Normales:** Titeldauern in der Playlist sind heute rot, „Playlist“ an den Decks ist rot gefüllt.
  - Dauern neutral in `font-mono`.
  - Rot nur für ON AIR, REC, Mikro offen und Restzeit < 10 s.
- **Knopfwüsten auflösen:**
  - Hauptaktion als Taste mit Wort (Start, Stopp, Blende, Mikro, Telefon).
  - Nebenaktionen als Symbol-Tasten mit Tooltip (⏮ ⏭ ↻ −10 +10 Wdh. Zufall).
  - Listenaktionen (▲ ▼ ✕) nur bei Hover.
- **Keine Einstellungsbereiche als Slider:** Processing als Zeilen „Bezeichnung · Zahlenfeld/Schalter“.

**Produzent:**

- **Transportleiste in Gruppen:** Transport · Loop/Punch/Stopp am Ende · Zeit-LCD mit Tempo-Feld · Pegel · Latenz (Symbol-Schalter + Feld) · Takte/Zeit, Raster, Zoom · Export.
- **Studio-Dock:** Reiter als Segmentschalter mit Symbol (Mixer, Piano-Roll, Automation, KI-Chat, Analyse), rechts „Lösen“ und „Einklappen“ als Symbole.
- **Rack:** im Mixer-Reiter rechts, 340 px, volle Dock-Höhe.

**Piano-Roll (André: „echt nicht gut“) – komplett nach `PianoRoll`/`ScreenPianoRoll` neu bauen:**

- **Leiste:**
  - Clip-Kennung (Spurfarbe, Name, Takte, Notenzahl, Auswahl).
  - Werkzeuge Zeichnen/Auswählen/Löschen (D/V/E) als Symbol-Segmente.
  - Raster und Triolen-Schalter „3“, Länge neuer Noten, Quantisieren.
  - Velocity-Feld.
  - Tonart-Auswahl, Magnet (auf Tonart einrasten), „Nur benutzte Töne“.
  - Probeton, Zoom.
- **Die Zeile mit Anleitungstext entfällt** („Klick: zeichnen · Ziehen …“), das steht in den Tooltips.
- **Die Überschrift „Drum-Raster“ entfällt;** Kit-Spuren öffnen das Drum-Raster, alle anderen die Noten.
- **Klaviatur** als echtes Klavier (schwarze Tasten kurz, C-Beschriftung je Oktave, gespielte Tonhöhen in Spurfarbe).
- **Zeilen:** Tonart-Zeilen 8 % Spurfarbe, schwarze Tasten 6 % dunkler. Taktlinien kräftig, Viertel mittel, Sechzehntel zart.
- **Lineal und Clip:**
  - Clip-Bereich als Balken im Lineal.
  - Außerhalb schraffiert.
  - Playhead in `accent`.
- **Noten** in Spurfarbe mit Velocity-Helligkeit, Notenname ab 40 px, Längen-Griff rechts, Auswahl mit hellem Rahmen. Heute sind die Noten blau, unabhängig von der Spur.
- **Velocity-Spur** 64 px mit Stiel und Kopf in Spurfarbe.
- **Wenn die Piano-Roll offen ist,** schrumpfen die Spuren im Arrangement auf 36 px, damit die Roll Platz hat.
- **Hell und dunkel:** Alle Flächen kommen aus Tokens, keine festen Schwarz-/Weißwerte außer der Klaviatur.

## 3. Umsetzung in egui (Regeln)

- **Theme mit Modus:** `theme.rs` bekommt einen `Mode`-Parameter (`Studio`/`Radio`/`Party` × dunkel/hell); alle Farben kommen aus den Tokens. `accent`, `accent-2`, `lcd-ink` und die Flächen wechseln mit dem Modus.
- **Ein Widget je Baustein** in `crates/gui/src/widgets/`. Bestehende (`key`, `fader`, `jog`, `lamp`, `pad`, `vu`, `clock`, `topbar`) an die Vorlagen angleichen. Neu:
  - `status_bar`, `radio_deck`, `level_group`, `party_controls`
  - `icon_button`, `number_field`, `select_field`, `segmented`, `tooltip`
  - `browser`, `chord_lane`, `sound_section`, `piano_roll`, `drum_grid`, `wizard`
- **Kein `egui::Button::new` für Hardware-Bedienung** (Decks, Party, Transport, Kopfleiste); nur die Widgets. Die rohe egui-Optik (graue Standardknöpfe, Radio-Buttons, Slider) darf in den drei Modi nicht mehr vorkommen. Ausnahme: Einstellungsdialoge.
- **7-Segment-Ziffern** mit DSEG7 Classic als eingebettete TTF, mit Geister-Segmenten „88:88“ in `led-off`.
- **Schriften:** Barlow, Barlow Condensed, JetBrains Mono, DSEG7 aus `fonts/` als TTF einbetten und per `FontDefinitions` registrieren.
- **Icons:** `egui-phosphor` mit denselben Namen wie in `bundle.js` (`WOA.icon`). Tooltip-Text nach `Tooltip`-README: „Löschen (Entf)“.
- **Eine Auswahl gilt überall** (`SelectionSync`): gewählte Spur = 14 % Spurfarbe + 1-px-Rahmen in Spurkopf, Kanalzug und Rack.
- **KISS:** jeder Befehl genau ein Knopf; Doppelungen entfernen. Beispiele: Deck A/B/C in der Sende-Knopfzeile, Lautstärke-Text unter dem Party-Pult.

## 4. Entscheidungen (gelten, bis André etwas anderes sagt)

- **Pan doppelt:** Pan bleibt im Spurkopf und im Rack-Ausgang. Es ist derselbe Wert an zwei Stellen, keine zwei Regler.
- **Trim:** Das Rack-Feld „Trim“ wird ein echter Eingangs-Trim. Dafür ein neuer Command im Model mit Undo und MCP-Tool `set_trim`, getrennt vom Spur-Gain (= Fader).

## 5. Pakete und Abnahme

Arbeite in diesen Paketen, je ein PR mit CHANGELOG, Version und Klickliste in `docs/abnahme.md`:

1. **Grundlagen:** Theme mit Modus und hellen Themen, Schriften, Icons, Widgets `key`, `icon_button`, `tooltip`, `number_field`, `select_field`, `segmented`, `status_bar` (Kopfleiste).
2. **Radio:** `radio_deck` × 3, `level_group`, Sende-Deck-Tasten, Fehlerzeile.
3. **Party:** Deck-Karten, Crossfader mit Schaltern, `party_controls`, Fernbedienungs-Karte, Wunschliste, Suche.
4. **Produzent – Bearbeiten:** Spurkopf ohne Badge-Überlappung und mit MIDI-Eingang, Clip-Inspektor mit Feldern und Symbolen, „+“ im Lineal statt Knopfreihe.
5. **Produzent – Komponieren:** `browser` (alle sechs Reiter), Akkordspur und Akkord-Clips, Instrument-Abschnitt im Rack, Piano-Roll nach Vorlage, Drum-Raster, Jingle-Assistent. Die Fenster Akkordfolge und Jingle-FX entfallen.
6. **Abgleich:**
   - Für jeden Modus in dunkel und hell einen Screenshot bei 1440×1000 neben den Entwurf legen (`docs/design/ist/`).
   - Kontrast prüfen.
   - `umsetzung.md` vollständig ✓.

Selbst prüfen:

- **Screenshots:** Die App mit `WOA_WINDOW=1440x1000` starten; ohne Audiogerät läuft sie unter Linux auch mit Xvfb + Mesa (lavapipe).
- **Test je Baustein:** `egui_kittest`-Test für die Zustände (wie `tests/design_system_2.rs`).

Merge nur bei grüner CI. Rückfragen nur, wenn eine Vorlage einer Funktion widerspricht. Dann in `umsetzung.md` als offene Frage notieren und mit dem Rest weitermachen.
