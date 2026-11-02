# Nachbesserung Design (Prüfung vom 01.11.2026, Stand `main` 465fe1f + Branch `design2-radio` 17b5a6b)

Maßstab bleibt `docs/auftrag-design.md` bzw. das Design System unter `docs/design/woa-design-system/` (Fassung 4, liegt bisher nur auf `design2-radio`).

## 0. Ordnung zuerst

1. **Branch `design2-radio` als PR öffnen**, CI grün, mergen. Er enthält Kopfleiste, Radio und das Design System Fassung 4; heute ist er nur gepusht, nicht in `main`.
2. **Fehlende Ablagen:**
   - `docs/design/umsetzung.md` (Prüfliste Baustein × Zustand × Ort × Stand) fehlt ganz.
   - `docs/design/ist/` und `docs/design/soll/` (aus `ist-soll.zip`) fehlen.
3. **Entscheidung Trim:**
   - Der Auftrag verlangt einen echten Eingangs-Trim (Model-Command mit Undo, MCP `set_trim`).
   - Es gibt ihn nicht, `luecken.md` steht unverändert.
   - Umsetzen oder in `umsetzung.md` als offene Frage an André.

## 1. Radio (Branch `design2-radio`) – was noch nicht stimmt

- **Fehlerbanner:** drei Zeilen über die volle Breite. Soll: eine Zeile mit Warnsymbol, Rest per Tooltip/„Details“.
- **Sende-Deck:**
  - Die Text-Tasten „Deck A / Deck B / Deck C“ stehen noch in der Symbolreihe. Sie entfallen; das machen die Decks selbst (Taste A–C).
  - „−10 / +10“ als Symbol-Tasten (`rewind`/`ffwd`) mit Tooltip.
- **Stream-Zeile:**
  - „Auf Sendung“ ist im Ruhezustand rot gefüllt. Soll: neutrale `Key` mit Antennen-Symbol.
  - Links davon die ON-AIR-Lampe (`woa-onair--sm`); rot nur, wenn wirklich gesendet wird.
- **Rot nur für live:** „Auto-Weiter“ und „+ Titel“ im Playlist-Kopf sowie „Satz 1“ der Cart-Wall sind rot gefüllt. Soll:
  - Auto-Weiter = `Key` mit LED (`is-lit`).
  - + Titel = neutrale Taste.
  - Sätze 1–5 als Segmentschalter (`woa-segs`).
- **Decks A · B · C:**
  - Rechts stehen weiter rohe Text-Knöpfe „Start · Cue · Vorhören · ↩ Playlist“. Soll (`RadioDecks`):
    - Start/Stopp als eine `Key` (`ok`) mit Symbol.
    - Cue und Vorhören als quadratische Symbol-Tasten.
    - „Danach zurück zur Playlist“ als Symbol-Schalter (`queue`).
  - Das Restzeit-LCD ist zu schmal und zu klein. Soll: 132 px breit, 7-Segment 26 px mit Geister-Segmenten „88:88“ und Fortschrittsbalken darunter.
- **Pegel:**
  - Die Namen stehen als Farbchips oben, Skalen und Werte in 9 px.
  - Soll (`RadioLevels`): Kanalzug wie im Produzenten-Mixer, LED-Kette neben dem Fader (gut sichtbar, 22 Segmente), Scribble-Strip unten (Name, Kante in Spurfarbe).
  - Werte nur im Tooltip/bei Ziehen.
- **Processing:**
  - Noch Slider + Checkboxen. Soll: Zeilen „Bezeichnung · Zahlenfeld/Schalter“:
    - Crossfade-Blende (s)
    - Musik-Ducking (dB)
    - Ducking auch bei Carts
    - Stopp blendet aus + Dauer
    - Sendebus
    - Mikro-Kette
  - Die Messzeile (LUFS · Regelung · Begrenzer) als LCD in `lcd-ink`, nicht rote Schrift.
- **Wunschtitel-Zeile** ist bei 1920 × 1030 unten abgeschnitten. Die rechte Spalte muss ohne Scrollen passen: Decks ≈ 240, Cart-Wall ≈ 320, Pegel/Processing ≈ 300, Wunschtitel 50 px.

## 2. Party (`main`, #309) – fast gut, Rest

- **Decks:**
  - Statt der Wellenform steht nur ein Fortschrittsstrich. Soll: Wellenform (52 px, gespielter Teil in `accent`, Rest gedimmt) wie im Entwurf.
  - Das 7-Segment-LCD ohne Geister-Segmente; Format immer „MM:SS“ (09:22, nicht 9:22).
- **Eingabefelder:** Bibliothek-Suche und Wunsch-Eingabe sind rohe einzeilige egui-Felder. Soll: `woa-input` (44 px, Radius `radius-party`, Symbol links).
- **Text als Zustand:** „Wdh. aus“ trägt den Zustand im Text. Soll: Wort „Wdh.“, Zustand nur über Füllung `accent-2`.
- **Fernbedienung:** QR, Adresse, PIN und Handyzahl prüfen; sie müssen sichtbar sein, sobald „An“ leuchtet. „An“ als `Key` mit LED.

## 3. Produzent – nicht begonnen (Pakete 4 und 5 des Auftrags)

Bis auf die Kopfleiste ist der Produzenten-Modus unverändert. Alles aus `docs/auftrag-design.md`, Abschnitte 2 und 2a, steht noch aus:

- **Transportleiste** in Gruppen. Heute noch: VU-Meter mit Zeiger als Platzhalter, „Takte“-Text, „Zusatz +0.0 ms“-Knopf.
- **Knopfreihe unter den Spuren** („+ Audio-Spur … Jingle bauen …“) → „+“ im Spurenkopf + **Browser** links mit den Reitern Klänge · Beats · Akkorde · FX · Samples · Jingles. Die Fenster „Akkordfolge“ und „Jingle-FX“ entfallen.
- **Spurkopf:**
  - MIDI-Spuren zeigen weiter „Mikrofon“ (`sources.rs` `short_label("") → "Mikrofon"`); richtig ist das MIDI-Gerät mit Klaviatur-Symbol.
  - Das „MIDI“-Badge überlappt den Namen.
- **Clip-Inspektor:** Felder ohne Einheit („0.00“). Soll: `NumberField` (ms, dB, st) mit Symbolen; Werkzeuge als `IconButton`.
- **Akkordspur und Akkord-Clips:** Akkordnamen je Takt, Klang im Clip-Kopf.
- **Instrument-Abschnitt im Rack** statt Klang-Auswahlfeld zwischen den Inserts.
- **Piano-Roll komplett neu** nach `PianoRoll`/`ScreenPianoRoll`:
  - Werkzeuge, Triolen, Tonart/Magnet, echte Klaviatur.
  - Noten in Spurfarbe, Velocity-Spur.
  - Ohne Anleitungszeile und ohne die falsche Überschrift „Drum-Raster“.
- **Drum-Raster** nur für Kit-Spuren, nach `DrumGrid`.
- **Jingle-Assistent** nach `JingleWizard`.
- **Studio-Dock:** Reiter als Segmentschalter mit Symbol; im Mixer-Reiter rechts das Rack.

## 4. Abnahme

- Für Radio, Party und Produzent je einen Screenshot bei 1920 × 1030 nach `docs/design/ist/` legen, neben `soll/`.
- `umsetzung.md` vollständig ✓, dann Klickliste in `docs/abnahme.md`.
- Merge nur mit grüner CI, über PR.
