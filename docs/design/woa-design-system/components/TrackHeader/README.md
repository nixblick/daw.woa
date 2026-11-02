# TrackHeader

Spurkopf links neben jeder Spur im Arrangement; seine Höhe ist die Spurhöhe (64 px, zwei Zeilen).

- **Zeile 1:** Aufnahme-scharf (rund, rot wenn an, Umschalt+R) · Name (Doppelklick = umbenennen) · M · S · Papierkorb.
- **Zeile 2:** Eingang (Auswahlfeld mit Symbol Mikro bzw. Klaviatur) · Pegel in dB (= derselbe Wert wie der Fader) · Pan.
- **Rechts:** schmale Stereo-LED-Kette.
- **Spurfarbe:** 5-px-Kante links, immer.

Zustände:

- **Normal:** Fläche `panel`, Farbe nur in der Kante.
- **Gewählt:** Fläche 14 % Spurfarbe, 1-px-Rahmen in Spurfarbe, die Spur rechts davon 6 %. Der Papierkorb ist sichtbar.
- **Scharf:** nur der Aufnahmeknopf leuchtet rot (`on-air-fill`). Rot bleibt Status, nie Auswahl.
- **Hover:** Der Papierkorb erscheint, sein Platz ist immer reserviert.

Spur-Klick wählt die Spur in allen Ansichten.

## Fassung 7

- Spuren immer 64 px, auch bei offener Piano-Roll (die Zeitleiste scrollt, das Dock ist in der Höhe ziehbar).
- MIDI-Spur Zeile 2: links **Instrument** (`woa-inst`, Symbol der Kategorie in Spurfarbe, Name, Pfeil) → öffnet die Klangauswahl im Rack; rechts **MIDI-Eingang** (`woa-midiin`, 104 px, Symbol leuchtet `cue` bei Noten, gestrichelt ohne Gerät).
