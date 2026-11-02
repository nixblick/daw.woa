# TopBar

Kopfleiste aller Modi: links Logo und Menüs, rechts ein einziger Block mit Modus, Status und Feedback.

- **Block** (`.woa-strip-bar`): eine eingelassene Leiste (`well`, `shadow-well`). Drei Gruppen, getrennt durch 1-px-Linien:
  1. Modus: Produzent · Radio · Party
  2. Status: MIDI · On Air · Rec · KI
  3. Feedback
- **Ein Stil für alle:** Jeder Eintrag ist ein `.woa-sb`, 28 px hoch, Versalien in `font-label`, LED-Punkt links. Nur die Farbe des Punkts und des Rahmens unterscheidet sie:
  - Modi: in ihrer Modusfarbe.
  - MIDI: `cue`.
  - Rec: `on-air`, blinkt und zeigt die Laufzeit.
  - On Air: keine Pille, sondern die ON-AIR-Lampe (Fassung 7, unten).
  - KI: `ok`.
  - Feedback: neutral, mit Symbol statt Punkt.
- **Aus:** LED gedimmt, Text `text-dim`, keine Fläche.
- **An:** Fläche `surface`, Rahmen und LED in der eigenen Farbe, Text `text`. Kein Glow, keine gefüllten Farbflächen – die Schrift läuft nie über Farbe. Ausnahme: die ON-AIR-Lampe.
- Status-Einträge sind zugleich Schalter: MIDI öffnet die Gerätewahl, On Air die Stream-Verbindung, Rec den Mitschnitt, KI die KI-Einstellungen. Es gibt keine zweiten Knöpfe dafür an anderer Stelle der Leiste.
- Die Bildmarke nimmt die Modusfarbe an.

## Fassung 7: ON AIR als Lampe

- „On Air“ ist keine Pille mehr, sondern die Lampe `woa-onair woa-onair--bar` (28 px, Rahmen 3 px `lcd-bg`, Barlow Condensed 14/800, Sperrung .12em).
- Aus: dunkles Rotglas. An (`is-on`): `on-air-fill`, weiß, `glow-on-air` – dieselbe Glut wie die Lampe im Sende-Deck. Gedimmt (`is-dim`, Pult-Modus): 72 % Füllung, kleiner Schein.
- Vollbild (F11): Kopfleiste bleibt; rechts zusätzlich „Vollbild verlassen“ (`corners-in`). Beim Wechsel 2 s Hinweis „Vollbild · F11 oder Esc beendet“.
