# TopBar

Kopfleiste aller Modi: links Logo und Menüs, rechts ein einziger Block mit Modus, Status und Feedback.

- **Block** (`.woa-strip-bar`): eine eingelassene Leiste (`well`, `shadow-well`). Drei Gruppen, getrennt durch 1-px-Linien:
  1. Modus: Produzent · Radio · Party
  2. Status: MIDI · On Air · Rec · KI
  3. Feedback
- **Ein Stil für alle:** Jeder Eintrag ist ein `.woa-sb`, 28 px hoch, Versalien in `font-label`, LED-Punkt links. Nur die Farbe des Punkts und des Rahmens unterscheidet sie:
  - Modi: in ihrer Modusfarbe.
  - MIDI: `cue`.
  - On Air und Rec: `on-air`. Rec blinkt und zeigt die Laufzeit.
  - KI: `ok`.
  - Feedback: neutral, mit Symbol statt Punkt.
- **Aus:** LED gedimmt, Text `text-dim`, keine Fläche.
- **An:** Fläche `surface`, Rahmen und LED in der eigenen Farbe, Text `text`. Kein Glow, keine gefüllten Farbflächen – die Schrift läuft nie über Farbe.
- Status-Einträge sind zugleich Schalter: MIDI öffnet die Gerätewahl, On Air die Stream-Verbindung, Rec den Mitschnitt, KI die KI-Einstellungen. Es gibt keine zweiten Knöpfe dafür an anderer Stelle der Leiste.
- Die Bildmarke nimmt die Modusfarbe an.
