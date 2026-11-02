# Runde 3 – am Stück, ohne neue Aufteilung

Fertig ist es erst, wenn alles unten erledigt oder in `docs/design/umsetzung.md` begründet als offene Frage an André steht. Maßstab: `docs/design/woa-design-system/` (Fassung 4), `auftrag-design.md`, `auftrag-nachbesserung.md`.

## 1. Korrekturen zu den Gründen

- **Beat-Katalog gibt es:** `woa_model::DRUM_PRESETS` (Pop, Four on the Floor, Rock, Hip-Hop, Nachrichten, Party, Halbes Tempo, Shuffle). Der Beats-Reiter zeigt die Muster mit Mini-Raster, darunter die Kits.
- **Tempo gibt es:** Die Bibliotheksanalyse speichert `bpm` je Titel (`crates/library/src/analysis.rs`). Party zeigt ein BPM-LCD am Deck und pulst im Beat. Ohne Analyse kein BPM-LCD und ein gleichmäßiger Puls.
- **Wellenform:** Für Party gibt es noch keine Daten, also bauen. Beim Laden im Hintergrund dekodieren, auf ~1000 Spitzenwerte verdichten und in der Bibliothek cachen. Dann Wellenform statt Balken: 52 px, gespielter Teil in `accent`, Rest gedimmt.
- **Probeton:** Note an/aus als Command auf die Spur, ohne Ton testbar. Gilt für Instrument-Kacheln, Browser „Vorhören“ und Piano-Roll „Probeton beim Zeichnen“.
- **Tonart:** Erkennung aus den Noten für die Akkordspur ist gut. Beim Erzeugen im Browser bleibt die Tonart wählbar (12 Tasten).
- **Deck-Tasten:** Start/Stopp mit Text ist in Ordnung. Cue, Vorhören und „zurück zur Playlist“ als Symbol-Tasten mit Tooltip.
- **Größerer QR-Code und 48 px bei 1366 × 768:** übernehmen.

## 2. Lime (Party, Token `accent-2`)

- **Werte:** Dunkel `#c6ff3a`, Text darauf `on-accent-2` `#0e1400`. Hell `#3f6600`, Text darauf `#ffffff`.
- **Bedeutung „eingeschaltet“:** Auto-Übergang, Zufall, Wiederholen, Stumm, Wunsch-Taste, meistgewählte Vote-Pille.
- **Außerdem:** Kartentitel, „als Nächstes“, VORGELADEN, Beatgrid-Striche.
- **Nie** für „läuft“ (`accent`, Magenta) oder „verbunden“ (`ok`, Grün). Nie weiße Schrift auf Lime im dunklen Thema.

## 3. Radio – prüfen und, wo noch offen, umsetzen

- **Fehlerbanner:** eine Zeile mit Symbol, Rest im Tooltip/„Details“.
- **Sende-Tastenreihe:** „Deck A/B/C“-Tasten raus. −10/+10 als Symbole.
- **Rot nur für live:**
  - Auf Sendung (Ruhe), Auto-Weiter, + Titel und Satz 1–5 neutral.
  - ON-AIR-Lampe links in der Stream-Zeile.
  - Sätze als Segmentschalter.
- **Decks:** Restzeit-LCD 132 px, 7-Segment mit Geister-Segmenten „88:88“, Fortschritt darunter.
- **Pegel** wie der Produzenten-Kanalzug: LED-Kette sichtbar, Namensschild unten, keine 9-px-Werte.
- **Processing** als Zeilen „Bezeichnung · Zahlenfeld/Schalter“. Die Messzeile als LCD in `lcd-ink`.
- **Platz:** Bei 1920 × 1030 alles ohne Scrollen, auch die Wunschtitel-Zeile.

## 4. Party – Rest

- **Decks:** Wellenform und BPM (siehe 1). 7-Segment immer MM:SS mit Geister-Segmenten.
- **Eingabefelder:** Suche und Wunsch-Eingabe als `woa-input` (44 px, `radius-party`, Symbol links).
- **„Wdh. aus“ → „Wdh.“:** Zustand nur über die Füllung.
- **Fernbedienung:** QR, Adresse, PIN und Handyzahl sichtbar, sobald „An“ leuchtet.

## 5. Produzent – Rest

- **Spurkopf:** MIDI-Spuren zeigen das MIDI-Gerät mit Klaviatur-Symbol statt „Mikrofon“ (`sources.rs` `short_label`). Kein „MIDI“-Badge, das den Namen überlappt.
- **Transportleiste in Gruppen** nach ScreenStudio: Transport · Loop/Punch/Stopp am Ende · Zeit-LCD mit Tempo-Feld · Pegel · Latenz · Takte/Zeit, Raster, Zoom · Export.
- **Clip-Inspektor:** Zahlenfelder mit Einheit (ms, dB, st) und Symbol, Werkzeuge als Symbol-Knöpfe mit Tooltip, Löschen rechts mit Abstand.
- **Studio-Dock:** Reiter als Segmentschalter mit Symbol, im Mixer-Reiter rechts das Rack (340 px).
- **Piano-Roll komplett** nach `PianoRoll`/`ScreenPianoRoll`:
  - Leiste: Clip-Kennung, Werkzeuge D/V/E, Raster + Triolen „3“, Notenlänge, Quantisieren, Velocity, Tonart + Magnet, „Nur benutzte Töne“, Probeton, Zoom.
  - Echte Klaviatur, Tonart-Zeilen 8 %, Clip-Bereich im Lineal, außerhalb schraffiert.
  - Noten in Spurfarbe mit Velocity-Helligkeit und Notennamen, Velocity-Spur mit Stiel und Kopf.
  - Anleitungszeile „Klick: zeichnen …“ und Überschrift „Drum-Raster“ raus, die Bedienung steht in den Tooltips.
  - Bei offener Piano-Roll schrumpfen die Spuren auf 36 px.
- **Drum-Raster** nur für Kit-Spuren nach `DrumGrid`: 16 Schritte, Viertelgruppen, laut/leise, Lauflicht, Muster aus `DRUM_PRESETS`.
- **Jingle-Assistent** nach `JingleWizard`: Schrittleiste, Wahlkarten mit Symbol, Zusammenfassung unten.

## 6. Trim

Echten Eingangs-Trim bauen: Model-Command mit Undo und MCP-Tool `set_trim`, getrennt vom Spur-Gain. Sonst in `umsetzung.md` als offene Frage an André und das Feld bis dahin „Gain“ nennen.

## 7. Abnahme

- **Prüfliste:** `docs/design/umsetzung.md` (Baustein × Zustand × Ort × Stand), alles ✓ oder begründet offen.
- **Screenshots:** `docs/design/ist/` und `soll/` füllen. Neue Screenshots aller Modi, dunkel und hell, bei 1920 × 1030 nach `ist/`.
- **Abschluss:** Klickliste in `docs/abnahme.md`, CHANGELOG, Version. PRs nur mit grüner CI.
