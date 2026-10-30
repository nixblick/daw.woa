# Fader

Kanalfader (vertikal) und Crossfader (horizontal) mit geriffelter Kappe und Modusfarben-Strich.

Markup: `.woa-fader` mit `--v`, Skala `.woa-fader__scale`, Schacht `.woa-fader__track` mit `.woa-fader__cap`. Horizontal: `woa-fader--h`. `data-fmt="db"` liefert dB nach Studio-Kurve (0 dB bei 75 %).

- Schacht immer `lcd-bg` mit `shadow-well`, Kappe `surface` mit Mittelstrich `accent`.
- Faderweg `fader-h` (180 px); im Radio-Pult 130 px.
- Doppelklick setzt auf 0 dB bzw. Mitte.
