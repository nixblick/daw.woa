# ModeSwitch

Segmentierter Umschalter Produzent · Radio · Party oben rechts in der Kopfleiste.

Drei `.woa-mode`-Knöpfe mit `data-m` in `.woa-modes`. Jede Taste trägt ihre feste Kennfarbe (`mode-studio`, `mode-radio`, `mode-party`) als LED, auch wenn ein anderer Modus aktiv ist. Aktiv = `is-active` (Kappe `surface`, Kontur und LED in Kennfarbe).

- Umschalten wechselt Layout und Presets, Audio läuft weiter.
- Läuft eine Sendung, bleibt die Kopfleiste im Radio-Rot markiert (Mitschnitt-Taste `is-on`).
- Tastatur: Strg+1/2/3; Agent: `set_mode`.
