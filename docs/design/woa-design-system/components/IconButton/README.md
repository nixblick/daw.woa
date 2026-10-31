# IconButton

Knopf nur mit Symbol – für Werkzeuge in Clip-Inspektor, Spurkopf, Rack und Slots. Jeder hat einen Tooltip mit Name und Tastenkürzel.

- **Größen:** 24 px (`--sm`, Symbol 14), 28 px (Standard, 16), 32 px (`--lg`, 18). Symbol Phosphor Regular.
- **Ruhe:** keine Fläche, Symbol `text-dim`.
- **Hover:** Fläche `surface`, Rahmen `border`, Symbol `text`.
- **An** (`aria-pressed=true`): Fläche `accent-soft`, Rahmen und Symbol `accent` – für Schalter wie Loop, Tempo folgen, Fenster offen.
- **Gefahr** (`--danger`): wird erst bei Hover rot (`on-air`); steht immer zuletzt in seiner Gruppe, mit Abstand.
- **Fest** (`--solid`): mit Tastenkappe, nur wo der Knopf allein steht.
- Nicht verfügbar: 35 % Deckkraft, kein Tooltip-Text ändern.
- Ein Symbol nur, wenn es eindeutig ist (Schere, Papierkorb, Loop). Sonst Text-Taste (`Key`).
