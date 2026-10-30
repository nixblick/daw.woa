# CartPad

Jingle-/Cart-Wall-Pad mit Tastennummer, Status-LED und Spurfarben-Balken.

`.woa-cartwall` (4 Spalten) mit `button.woa-cart` und `--t` (Spurfarbe `track-1…6`). Teile: `__no`, `__led`, `__title`, `__len`, `__bar > i`. Zustände `is-playing` (+ `--p` Fortschritt, Glow `accent`), `is-armed` (LED `accent-2`), `is-empty`.

- Tasten 1–9, 0 belegen die ersten zehn Pads; MIDI-Learn möglich.
- Laufend zeigt `__len` die Restzeit mit Minus.
- Leere Pads nehmen Dateien per Drag & Drop an.
