# TopBar

Gemeinsame Kopfleiste aller Modi: Marke, Menüs, MIDI-Pille, Mitschnitt, Modus-Umschalter, Feedback.

`.woa-topbar` mit `.woa-topbar__brand` (Bildmarke per `data-woa-mark`), `.woa-menu`, `.woa-spacer`, Pillen und Tasten.

- Kein Element bekommt hier Glow: leuchtende Tasten zeigen ihren Zustand nur über Füllung und Kontur, damit Schrift und Licht nicht über die Leiste laufen.
- Mitschnitt gedrückt: Fläche `on-air-fill`, weiße Schrift.
- Elemente schrumpfen nicht; wird es eng, kürzt sich zuerst das Menü.
- Mitschnitt-Taste immer sichtbar, Feedback-Taste immer ganz rechts.
