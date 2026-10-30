# Key

Beleuchtete Hardware-Taste für jede Schalt- und Auslöseaktion in allen drei Modi.

Markup: `<button class="woa-key">` mit optionaler `woa-key__led`. Zustände: ohne Klasse = bereit, `is-lit` = aktiv/eingerastet (Grundschimmer `accent-soft`, Kontur `accent`), `is-on` = voll gedrückt (Füllung `accent`, Text `on-accent`). Farben: `woa-key--rec` (on-air), `--ok`, `--cue`, `--warn`, `--alt` (`accent-2`). Größen: `--sm` (24 px), Standard (`key-h` 32 px), `--lg` (44 px), `--sq` quadratisch für Icon-Tasten.

- Eine Taste = ein Command (gleich für Maus, Tastatur, MIDI, Agent).
- Icon-Tasten brauchen `aria-label`.
- Rot nur für Aufnahme, Mikro, Sendung – nie für „Löschen“-Dialoge.
- egui: `theme::key(ui, label, Kind, state)` mit `SURFACE`, `BORDER_STRONG`, Schatten als 2-px-Unterkante zeichnen.
