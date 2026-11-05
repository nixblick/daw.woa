# DjMixerStyles

Regler-Stil im DJ-Mixer, wählbar (Fassung 8.1). Gilt für alle Stellen, an denen bisher Drehknöpfe sitzen (heute nur der DJ-Mixer).

| Stil | Bedienung | Standard |
|---|---|---|
| **Mini-Fader** | senkrecht ziehen, Klick springt; EQ als drei Fader, KILL darunter; Trim/Filter oben, Master und Kopfhörer in der Mitte | **ja** |
| Wertbalken | waagerecht, 22 px; Klick springt, Ziehen ändert; EQ/Filter ab der Mitte gefüllt, Wert rechts über dem Balken; KILL rechts daneben | |
| Zahlenfelder | `NumberField`: ziehen hoch/runter, Mausrad, Klick zum Tippen, Doppelklick = 0; KILL daneben | |
| Drehknöpfe | wie bisher, aber senkrecht ziehen (nicht kreisen), Mausrad, Doppelklick = Mitte, Umschalt = fein | |

- Für alle Stile gilt: Doppelklick = Standardwert, Umschalt + Ziehen = fein, Mausrad ändert, Tooltip zeigt den Wert. Alle Regler sind MIDI-lernbar; die Zuordnung hängt am Parameter, nicht am Stil.
- **KILL** (`woa-kill`, 22 px, aktiv `warn`) gibt es in jedem Stil, bei Drehknöpfen unter dem Knopf.
- **Einstellung:** Einstellungen › Party › DJ-Mixer › „Regler-Stil“ als Segmente. Dazu Menü **Ansicht › Regler-Stil** mit Häkchen; Tastenkürzel nur als Vorschlag (Strg+Alt+1–4).
- Der Wechsel wirkt sofort und verändert keine Werte. Er wird pro Nutzer gespeichert, nicht im Projekt.
