# Window

Lösbares Fenster mit eigener Kopfzeile (Standard für Piano-Roll, Fragen und To-dos, Abnahme, Feedback, Einstellungen).

- **Kopf 44 px** (`woa-win__head`): Symbol 20 px in `accent` · Titel (Barlow Condensed 16/800, Versalien) · Stand-Zeile rechts (Mono 11, `text-dim`, kürzt mit …) · Trennstrich · Maximieren/Wiederherstellen (`corners-out`/`corners-in`, 32 px) · Schließen (`x`, Hover `on-air-fill`).
- Ziehen am Kopf verschiebt, Doppelklick maximiert/stellt wieder her, Esc schließt.
- **Werkzeugzeile** (`woa-win__tool`, optional): Suchfeld 40/48 px + Filter. **Fuß** 36 px (`woa-win__foot`): Hinweis links, Zähler rechts.
- **Normal:** Radius `radius-lg`, Rahmen `border-strong`, Schatten 0 24 64 −12 rgba(0,0,0,.55). **Maximiert** (`is-max`): füllt das Hauptfenster unter der Kopfleiste, ohne Radius und Schatten.
- Stand-Zeile darf Fehler zeigen (`warn`). Kein zweiter Titel im Inhalt.
- egui: `Window::new(..).title_bar(false)` + eigener Kopf; Größe/Position je Fenster in der Konfiguration merken.
