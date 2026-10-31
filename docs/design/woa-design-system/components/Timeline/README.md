# Timeline

Arrangement-Zeitleiste mit Spurköpfen (`TrackHeader`), Audio- und MIDI-Clips, Takt-Lineal und Playhead.

`.woa-arr` (Spalten 260 px + Zeitleiste). Spurkopf `.woa-tk`, Spur `.woa-lane` (gewählt: `is-sel-track`), Clip `.woa-clip` (`--t`, Kopf `__h`, Wellenform-`canvas data-woa-wave`), Clip-Auswahl `is-sel`, Playhead `.woa-playhead`. „+“ im Lineal-Kopf legt eine Spur an (Strg+T).

- Clip-Fläche = Spurfarbe 22 % auf `well`, Kopf voll; gewählter Clip mit 2-px-Rahmen in `text`.
- Playhead immer `accent` des Modus.
