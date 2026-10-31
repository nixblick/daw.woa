# NumberField

Zahlenfeld für Gain, Pan, Zeiten, Tonhöhe – und Auswahlfeld für Eingang, Ausgang, Raster.

- **Ziehen** hoch/runter ändert den Wert, Umschalt = fein. Doppelklick = Standardwert (0 dB, C, 0 st). Klick oder Enter = eintippen; Esc bricht ab. Pfeiltasten in Schritten, mit Umschalt ×10.
- **Breiten:** S 52 px (Pan, st), Standard 64 px (dB, %), M 84 px (ms, mit Symbol), L 104 px (Positionen mit Präfix).
- **Einheit** kurz und gedimmt hinter der Zahl: dB, %, ms, st. Pan ohne Einheit: C, L1–L100, R1–R100.
- **Zahlen** in `font-mono`, rechtsbündig, echtes Minus.
- **Zustände:**
  - Hover: Rahmen `text-dim`.
  - Ziehen: Rahmen und Zahl `accent`.
  - Eintippen: Fokusring `focus`.
  - Gemischt: „—“.
- **Auswahlfeld:** gleiche Höhe und Fläche, optionales Symbol links, Pfeil rechts, Text kürzt mit „…“.
