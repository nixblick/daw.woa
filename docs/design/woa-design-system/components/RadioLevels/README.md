# RadioLevels

Pegelgruppe im Radio (Fassung 8): **ein Kanalbau für alle**.

- Jeder Kanal (`woa-ch8`, 52 px, Master 64 px) hat drei feste Zeilen:
  1. **Oben 24 px:** Stumm-Taste `M` (`woa-key--rec`, 40 px breit) bei Ausgängen, sonst leerer Platz gleicher Höhe.
  2. **Mitte `--h`:** Fader + Stereo-LED-Kette, beide genau gleich hoch. Master ohne Fader, Kette mittig.
  3. **Unten 22 px:** Namensschild mit Kante in Spurfarbe, alle auf einer Grundlinie.
- Gruppen **Quellen · Summe · Ausgänge** mit kleinem Label oben, getrennt durch 1-px-Linien. Eine gemeinsame dB-Skala links (0, −6, −12, −24, −48).
- Quellen: Playlist · Carts · Decks · Mikro · Gäste. Ausgänge: Notebook · Stream.
- LED-Farben nur grün → gelb (ab −12) → rot (ab −3). **Kein Blau** im Pegel (Blau = Cue/MIDI).
- Stumm: Taste rot gefüllt, Kette 35 %, Name durchgestrichen.
- Die Karte füllt ihre Höhe (`--h` wächst mit), kein Leerraum unten. Werte nur im Tooltip/beim Ziehen.
- Analoge VU als Alternative über das Symbol im Kartenkopf.
- Die Werte sind dieselben wie in den Einstellungen und per MIDI-Learn steuerbar; keine Drehregler.
