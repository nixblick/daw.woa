# Umsetzung Design-Runde 3 – Prüfliste

Stand 0.35.24 (02.11.2026). Maßstab: `docs/auftrag-runde-3.md`, `docs/auftrag-nachbesserung.md`,
`docs/auftrag-design.md`, Design System Fassung 4 (`docs/design/woa-design-system/`).

Stand-Spalte: **✓** mit Version und PR · **entschieden von André** (02.11.) · **offen: Frage an André**.
Jeder ✓ ist im Code nachgesehen (Datei in der Spalte Ort), nicht nur aus dem PR-Text übernommen.

PRs der Runde: #314 Party (0.35.17) · #317 Studio (0.35.18) · #313 Radio (0.35.19) ·
#316 Werkzeuge (0.35.20) · #318 Nacharbeit (0.35.21) · #319 Zusatz (0.35.22) · #326 Transport/Dock (0.35.23).

## Zählung

| ✓ gebaut | entschieden von André, noch nicht gebaut oder bewusst so | offen: Frage an André |
|---|---|---|
| 64 (davon 4 auf Andrés Entscheidung vom 02.11.) | 6 | 4 |

## §1 Korrekturen zu den Gründen

| Baustein | Zustand | Ort | Stand |
|---|---|---|---|
| Beats-Reiter | alle `DRUM_PRESETS` mit Mini-Raster, darunter die Kits; Klick = Kit-Spur + Clip, ein Undo | `gui/src/beats.rs` | ✓ 0.35.18 #317 |
| BPM-LCD am Party-Deck | analysiert: LCD; ohne Analyse: kein LCD | `gui/src/party.rs` (`bpm`) | ✓ 0.35.17 #314 |
| Play-Puls | im Beat / ohne BPM gleichmäßig | `gui/src/party.rs` | ✓ 0.35.17 #314 |
| Wellenform-Daten | im Hintergrund dekodiert, ~1000 Spitzen, Cache in der Bibliothek (Tabelle `waveforms`, Schema 5) | `library/src/waveform.rs` | ✓ 0.35.17 #314 |
| Wellenform am Deck | 52 px, gespielt `accent`, Rest gedimmt | `gui/src/party.rs` | ✓ 0.35.17 #314 |
| Probeton | Note an/aus auf die Spur, ohne Ton testbar: Instrument-Kachel, Browser-Vorhören, Piano-Roll, Drum-Zeilen, Klaviatur | `gui/src/probeton.rs` | ✓ 0.35.20 #316 (siehe Abweichungen) |
| Tonart beim Erzeugen | 12 Tasten im Akkorde-Reiter | `gui/src/akkorde.rs` | ✓ 0.35.18 #317 |
| Tonart-Erkennung aus Noten | für die Akkordspur | `gui/src/widgets/piano_roll.rs` | ✓ 0.35.20 #316 |
| Deck-Tasten Radio | Start/Stopp mit Text, Cue/Vorhören/Rückweg als Symbol mit Tooltip | `gui/src/radio.rs` | ✓ 0.35.19 #313 |
| Größerer QR-Code, 48 px bei 1366 × 768 | Party | `gui/src/party.rs` | ✓ 0.35.17 #314 |

## §2 Lime (`accent-2`)

| Baustein | Zustand | Ort | Stand |
|---|---|---|---|
| Token `accent-2` | dunkel `#c6ff3a` / Text `#0e1400`; hell `#3f6600` / Text weiß | `gui/src/theme.rs` | ✓ 0.35.17 #314 |
| Schalter Auto-Übergang, Zufall, Wdh., Stumm | an = Lime gefüllt | Party-Pult | ✓ 0.35.17 #314 |
| Wunsch-Taste | Lime | Wunschliste | ✓ 0.35.17 #314 |
| meistgewählte Vote-Pille | Lime | Wunschliste | ✓ 0.35.17 #314 |
| Kartentitel, „als Nächstes“, VORGELADEN, Beatgrid | Lime | Party | ✓ 0.35.17 #314 |
| Nie für „läuft“/„verbunden“, nie Weiß auf Lime dunkel | Test in `party.rs` | Party | ✓ 0.35.17 #314 |

## §3 Radio

| Baustein | Zustand | Ort | Stand |
|---|---|---|---|
| Fehlerbanner | eine Zeile mit Symbol, Rest im Tooltip und „Details“ | `gui/src/radio.rs` | ✓ 0.35.19 #313 |
| Sende-Tastenreihe | „Deck A/B/C“ raus (Übernahme im Deck-Menü), −10/+10 als Symbol | `gui/src/radio.rs` | ✓ 0.35.19 #313 |
| Tasten A–C starten/stoppen Deck A–C | nicht bei Textfokus oder Cart-Lernen | `gui/src/radio.rs` (`DECK_KEYS`) | ✓ 0.35.22 #319 (entschieden von André: ja) |
| Auf Sendung (Ruhe), Auto-Weiter, + Titel | neutral, Auto-Weiter mit LED | `gui/src/radio.rs` | ✓ 0.35.19 #313 |
| Sätze 1–5 | Segmentschalter | `gui/src/radio.rs` | ✓ 0.35.19 #313 |
| ON-AIR-Lampe | links in der Stream-Zeile, rot nur beim Senden | `gui/src/radio.rs` | ✓ 0.35.19 #313 |
| Restzeit-LCD | 132 px, 7-Segment, Geister „88:88“, Fortschritt darunter | `gui/src/radio.rs` (`DECK_CLOCK_WIDTH`) | ✓ 0.35.19 #313 |
| Pegel | Kanalzug mit LED-Kette, Schild unten, keine 9-px-Werte | `gui/src/radio.rs` | ✓ 0.35.19 #313 |
| Ausgangs-Pegel Notebook/Stream | Spitze hinter Regler/Stumm, lock-frei | `radio/src/mixer.rs`, `radio/src/station.rs` | ✓ 0.35.22 #319 (entschieden von André: ja) |
| Processing | Zeilen „Bezeichnung · Zahlenfeld/Schalter“: Crossfade-Blende, Musik-Ducking, Ducking bei Carts, Stopp blendet aus, Sendebus, Mikro-Kette | `gui/src/radio.rs` | ✓ 0.35.19 #313 |
| Messzeile | LCD in `lcd-ink` | `gui/src/radio.rs` | ✓ 0.35.19 #313 |
| Platz 1920 × 1030 | alles ohne Scrollen inkl. Wunschtitel-Zeile | Test in `radio.rs` | ✓ 0.35.19 #313, Bild `ist-radio-dunkel-1920x1030-0.35.23.png` |
| Gäste-Karte | – | Radio rechts | entschieden von André: Vorlage kommt als `GuestCard` im Designsystem (Fassung 6), Bau danach |

## §4 Party

| Baustein | Zustand | Ort | Stand |
|---|---|---|---|
| Decks: Wellenform, BPM | siehe §1 | `gui/src/party.rs` | ✓ 0.35.17 #314 |
| 7-Segment | immer MM:SS, Geister-Segmente | `gui/src/party.rs` (`ghost_of`) | ✓ 0.35.17 #314 |
| Suche, Wunsch-Eingabe | `woa-input` 44 px, `radius-party`, Symbol links | `gui/src/party.rs` (`INPUT_H`) | ✓ 0.35.17 #314 |
| „Wdh.“ | Wort ohne Zustand, Zustand über Füllung | `gui/src/party.rs` | ✓ 0.35.17 #314 |
| Fernbedienung | QR, Adresse, PIN, Handyzahl sichtbar, sobald „An“ leuchtet | `gui/src/party.rs` | ✓ 0.35.17 #314 |
| Deck-Tasten Cue / Sync | – | Deck | entschieden von André: ja, als DJ-Paket nach Designsystem Fassung 5 (geplant, läuft als nächstes) |
| Fernbedienung bei 1366 × 768 | Wunsch-Eingabe musste gescrollt werden | rechte Spalte | entschieden von André: RemoteCard kompakt unter 900 px Höhe, kommt mit dem DJ-Paket (geplant) |

## §5 Produzent

| Baustein | Zustand | Ort | Stand |
|---|---|---|---|
| Spurkopf MIDI-Eingang | MIDI-Gerät mit Klaviatur-Symbol statt „Mikrofon“ | `gui/src/app.rs` (`midi_input_combo`, `PIANO_KEYS`) | ✓ 0.35.18 #317 |
| „MIDI“-Badge | entfällt, kein Überlappen | `gui/src/app.rs` | ✓ 0.35.18 #317 |
| Transportleiste | Gruppen Transport · Loop/Punch/Stopp am Ende · Zeit-LCD + Tempo · Pegel · Latenz · Takte/Zeit, Raster, Zoom · Export | `gui/src/app.rs` | ✓ 0.35.18 #317, 0.35.21 #318 |
| Transport-Tasten | alle als `Key`, kein `egui::Button` | `gui/src/app.rs`, `widgets/key.rs` | ✓ 0.35.23 #326 |
| Clip-Inspektor Felder | Einheit ms/dB/st, Symbol; Fades in ms auch bei Mehrfachauswahl | `gui/src/app.rs` | ✓ 0.35.18 #317, 0.35.21 #318 |
| Clip-Inspektor Werkzeuge | `IconButton` mit Tooltip, Löschen rechts mit Abstand | `widgets/icon_button.rs` | ✓ 0.35.21 #318 |
| Clip-Optionen Rückwärts/Schleife/Stumm | Modell kennt sie nicht | – | entschieden von André: ja, eigenes Paket (geplant, noch nicht gebaut) |
| Studio-Dock | Reiter als Segmente mit Symbol (`widgets::Segment`), Lösen/Einklappen nur Symbol | `widgets/segment.rs` | ✓ 0.35.18 #317, 0.35.21 #318, 0.35.23 #326 |
| Rack im Mixer-Reiter | rechts, 340 px; „Ausgang / Pan“ bei 1920 × 1030 ganz sichtbar, sonst rollt nur das Rack | `gui/src/app.rs` | ✓ 0.35.18 #317, ✓ 0.36.1 #329 |
| Analyse | bleibt eigenes Fenster; oben die Pegelansicht (Analog/Modern/Aus, analoges VU nur hier) | `gui/src/app.rs` (`level_view`) | entschieden von André, Pegelansicht ✓ 0.36.1 #329 |
| Piano-Roll Leiste | Clip-Kennung, D/V/E, Raster + Triolen „3“, Notenlänge, Quantisieren, Velocity, Tonart + Magnet, „Nur benutzte Töne“, Probeton, Zoom | `widgets/piano_roll.rs` | ✓ 0.35.20 #316 |
| Klaviatur, Tonart-Zeilen 8 %, Clip-Bereich im Lineal, außen schraffiert | | `widgets/piano_roll.rs` (`hatch`) | ✓ 0.35.20 #316 |
| Noten | Spurfarbe, Velocity-Helligkeit, Notennamen; Velocity-Spur mit Stiel und Kopf | `widgets/piano_roll.rs` | ✓ 0.35.20 #316 |
| Anleitungszeile, Überschrift „Drum-Raster“ | raus (Test prüft das Fehlen) | `widgets/piano_roll.rs` | ✓ 0.35.20 #316 |
| Spuren 36 px bei offener Piano-Roll | | `gui/src/app.rs` (`row_h`) | ✓ 0.35.20 #316 |
| Drum-Raster | nur Kit-Spuren, 16 Schritte, Viertelgruppen, laut/leise, Lauflicht, Muster aus `DRUM_PRESETS` | `widgets/drum_grid.rs` | ✓ 0.35.20 #316 |
| Drum-Klangbänke nachladen | – | – | entschieden von André: ja, eigenes Paket (geplant) |
| Jingle-Assistent | Schrittleiste (6 Schritte), Wahlkarten mit Symbol, Zusammenfassung unten | `widgets/wizard.rs` | ✓ 0.35.20 #316, Fotomodus `WOA_JINGLE` 0.35.24 |
| Widgets nach `widgets/` | piano_roll, drum_grid, wizard | `gui/src/widgets/` | ✓ 0.35.21 #318 |
| Akkord-Vorhören | flüchtige Vorhör-Stimme mit dem Klang der Ausgabe (Akkorde = Streicher, Bass = E-Bass, Arpeggio = Lead), unabhängig von der gewählten Spur; Klick auf eine Ausgabe-Zeile spielt sie vor | `gui/src/akkorde.rs`, `gui/src/probeton.rs` (`voice_tap`, `VOICE_TRACK`) | ✓ 0.36.1 #329 (entschieden von André) |
| Lautsprecher-Knopf an der Instrument-Kachel | Probeton beim Wählen der Kachel, kein eigener Knopf | Rack, Instrument | nicht bauen, entschieden von André (Klick auf die Kachel klingt) |

## §6 Trim

| Baustein | Zustand | Ort | Stand |
|---|---|---|---|
| Eingangs-Trim | `Command::SetTrim` (−24…+24 dB), Undo, Engine-Knoten vor den Inserts, getrennt vom Fader | `model/src/command.rs`, `engine` | ✓ 0.35.18 #317 |
| MCP-Tool `set_trim` | | `agent/src/lib.rs` | ✓ 0.35.18 #317 |
| Rack-Feld „Trim … dB“ | | `gui/src/app.rs` | ✓ 0.35.18 #317 |
| Trim bei Aufnahme | Monitor und Pegel nach Trim, Datei roh | `gui/src` (`input_trim`) | ✓ 0.35.22 #319 (entschieden von André: ja) |

## §7 Abnahme und Weiteres

| Baustein | Zustand | Ort | Stand |
|---|---|---|---|
| Prüfliste | diese Datei | `docs/design/umsetzung.md` | ✓ 0.35.24 |
| Screenshots aller Modi | dunkel und hell, 1920 × 1030, dazu 1366 × 768 für Radio und Produzent | `docs/design/ist/` | ✓ 0.35.24 |
| Klickliste | „Design-Runde 3 gesamt“ | `docs/abnahme.md` | ✓ 0.35.24 |
| Vollbild beim Start | `WOA_WINDOW` gibt feste Größe, F11 schaltet | `apps/woa/src/main.rs` | ✓ 0.35.21 #318 (entschieden von André) |
| Ausgangspegel per MCP für Agenten | Werkzeug `output_levels` (nur lesend, nur Live-Modus): Spitze je Ausgang Notebook, Stream, Vorhören in dBFS; lesender Kanal App → Agent über Atomics, ohne Leihe der Sitzung | `agent/src/live.rs` (`OutputLevels`), `radio/src/pfl.rs` (Spitze), `gui/src/mcp_live.rs` | ✓ 0.36.1 #329 (entschieden von André: jetzt mitbauen) |
| Decks per MIDI lernbar | Start/Stopp, Cue, Vorhören je Deck A–C: Rechtsklick „MIDI lernen“ an jeder Deck-Taste und Zeile „Decks:“ in der Zuordnungsliste; alte Dateien laden weiter | `io/src/controller.rs` (`Target::Deck` + `DeckAction`, erweiterbar), `gui/src/radio.rs`, `gui/src/controls.rs` | ✓ 0.36.1 #329 (entschieden von André) |

## Punkte aus `auftrag-nachbesserung.md`

| Punkt | Stand |
|---|---|
| §0.1 Branch `design2-radio` mergen | ✓ 0.35.13 (Design-Runde 2) |
| §0.2 `umsetzung.md`, `ist/`, `soll/` | ✓ 0.35.24 (diese Datei), `ist/` ergänzt; `soll/` liegt seit 0.35.16 |
| §0.3 Trim | ✓ siehe §6 |
| §1 Radio: Fehlerbanner, Sende-Deck, Stream-Zeile, Rot nur live, Decks, Pegel, Processing, Wunschtitel-Platz | ✓ siehe §3 (0.35.19 #313) |
| §2 Party: Wellenform, MM:SS, Eingaben, „Wdh.“, Fernbedienung | ✓ siehe §4 (0.35.17 #314) |
| §3 Produzent: Transport, Browser statt Knopfreihe, Spurkopf, Inspektor, Akkordspur, Instrument im Rack, Piano-Roll, Drum-Raster, Jingle-Assistent, Studio-Dock | ✓ siehe §5 (Browser, Akkordspur, Instrument-Abschnitt seit Runde 2, 0.35.14/0.35.15) |
| §4 Screenshots, Prüfliste, Klickliste | ✓ 0.35.24 |

## DJ-Paket (0.36.0)

Auftrag André 02.11.2026, Design-System Fassung 5 (DjWaves, DjDeck, DjMixer, RemoteCard, ScreenPartyDJ).

| Baustein | Stand | Ort |
|---|---|---|
| Party-Umschalter Einfach / DJ, Auto-Übergang in DJ aus | ✓ | `crates/gui/src/party/dj.rs` (`view_bar`), `crates/radio/src/control/duo.rs` |
| Einfach-Deck nur Play/Pause | ✓ | `crates/gui/src/party.rs` |
| DjWaves: parallele Wellen mit Phase, Mausrad zoomt | ✓ | `party/dj.rs` (`dj_waves`) |
| DjDeck: Camelot-Chip, BPM, Restzeit, Übersicht mit Hot Cues und Loop | ✓ | `party/dj.rs` (`dj_deck`, `overview`) |
| Hot Cues 1–4, Auto-Loop 1/2/4/8/16, Cue halten, Play, Sync, Keylock | ✓ | `crates/radio/src/control/dj.rs` (`DjCommand`) |
| Tempo ±8 % mit Anschieben, Jogwheel (anschieben, scratchen) | ✓ | `crates/dsp/src/dj.rs` (Leser, WSOLA), `party/dj.rs` (`jog`, `tempo_column`) |
| DjMixer: Trim, EQ mit Kill, Filter, Cue, Kanalfader mit LED, Master, Kopfhörer Mix/Pegel, Crossfader weich/hart | ✓ | `crates/dsp/src/dj.rs`, `party/dj.rs` (`dj_mixer`) |
| Kopfhörer-Cue nur über Vorhör-Ausgang, ohne Ausgang gesperrt | ✓ | `crates/radio/src/mixer.rs`, `party/dj.rs` |
| Beatgrid-Lage und Camelot aus der Analyse | ✓ | `crates/dsp/src/dj.rs`, `crates/library/src/analysis.rs` |
| Hot Cues und Schleife je Titel gespeichert (Schema 6, Migration mit Test) | ✓ | `crates/library/src/dj.rs` |
| Playlist mit BPM, Tonart, „A/B laden“ | ✓ | `party/dj.rs` (`dj_playlist`) |
| RemoteCard kompakt unter 900 px, QR 300 px (Q, Klick, Esc) | ✓ | `crates/gui/src/remote.rs` |
| MCP `dj_state`, `dj_deck`, `dj_mixer` | ✓ | `crates/agent/src/dj.rs`, `docs/agent.md` |
| Golden Files EQ, Filter, Crossfader-Kurve, Time-Stretch | ✓ | `tests/golden/dj_*.wav` |
| MIDI-Lernen für DJ-Ziele | offen: Frage an André | – |

Abweichungen:

- **Fernbedienung in der DJ-Ansicht immer kompakt**, auch bei 1920 × 1030 (unten ist neben der Playlist nur eine Zeile Platz; der Auftrag nennt „Fernbedienung kompakt“).
- **Keylock als laufendes WSOLA** im Deck-Leser, nach dem Muster des vorhandenen `woa_dsp::stretch` (offline): keine neue Abhängigkeit, feste Puffer, keine Allokation im Audio-Thread. Klanglich einfacher als ein Phasen-Vocoder; reicht für ±8 %.
- **Tonart-Erkennung** liefert Dur/Moll aus der Bibliotheksanalyse; Camelot wird daraus abgeleitet.
- **MIDI-Lernen** nicht gebaut: `woa_io::controller::Target` kennt nur Spuren, Pegel, Carts und Layout. Für die DJ-Ziele bräuchte es rund zwanzig neue Ziele samt Seite, Beschriftung, Weiterleitung und Lern-Oberfläche – nicht billig, daher Frage.

## Abweichungen vom Entwurf

- **Probeton ist kein `woa_model::Command`:** Er ändert das Projekt nicht und gehört daher weder in die Undo-Kette noch in die Projektdatei. Er ist ein eigener `Probe`-Befehl (Note an/aus), den Tests ohne Ton lesen.
- **Probeton beim Wählen einer Instrument-Kachel** statt eigenem Knopf (siehe offene Frage).
- **Tonart in der Piano-Roll** ist nur Ansicht (Hervorhebung, Magnet), sie ändert keine Noten.
- **Piano-Roll:** Zeilenhöhe per Alt+Mausrad; Löschen über Werkzeug E oder Entf statt eigenem Knopf.
- **Pegel-LCD im Transport** weicht unter ~1000 px freier Breite (1366er Fenster).
- **Analyse** bleibt eigenes Fenster (entschieden von André), der Dock-Reiter öffnet es.
- **Party-Beatphase** zählt ab Titelanfang (die Analyse kennt keinen Beat-Offset); ein leeres Live-Deck zeigt die Wellenform des nächsten Titels.
- **Radio-Crate** behält intern „Wdh. aus“, nur die Oberfläche zeigt „Wdh.“.
- **Bibliothek Schema 5** (Tabelle `waveforms`): Headless und GUI gemeinsam ausliefern.
- **Deck-Übernahme im Radio** (Blende/hart) liegt im Deck-Menü statt in der Sendereihe.
- **Radio bei 1366 × 768:** Pegel, Processing und Wunschtitel liegen unter dem Bildrand (rechte Spalte scrollt); verlangt ist „ohne Scrollen“ nur für 1920 × 1030. Bild `ist-radio-*-1366x768-0.35.23.png`.
