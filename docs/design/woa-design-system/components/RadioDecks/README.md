# RadioDecks

Decks A · B · C im Radio-Modus: je eine Zeile, gleich hoch, gleich aufgebaut.

Aufbau von links nach rechts:

- Buchstabe im LCD-Feld (leuchtet in `lcd-ink`, solange das Deck spielt).
- Restzeit als 7-Segment mit Fortschrittsbalken.
- Titel und Interpret.
- Tasten:
  - Start/Stopp (`ok`): eine Taste, Text und Symbol wechseln; Kürzel A, B, C.
  - Cue: Symbol, zurück an den Anfang.
  - Vorhören (PFL): `cue`.
  - Danach zurück zur Playlist: Symbol-Schalter.

Zustände:

- **Läuft:** Rahmen und 10-%-Fläche in `accent`.
- **Geladen:** neutral. PFL leuchtet, wenn vorgehört wird.
- **Leer:** gestrichelt, Tasten gesperrt, Hinweis „Titel hierher ziehen · Rechtsklick: Datei laden“.
- **Ablegen:** Beim Ziehen eines Titels wird jedes Deck zur Ablage mit `accent-2`-Rahmen und „Hier ablegen“.
- **Restzeit < 10 s:** Ziffern rot und blinkend, wie im Sende-LCD.

Rechtsklick: Datei laden, Start und Ende schneiden, Leeren (rot, zuletzt).
