# AcceptanceWindow

Fenster „Abnahme“. Aufbau wie `QuestionsWindow`.

- Werkzeugzeile: Suche · Zähler „x von n geprüft“ mit Balken (grün = geht, gelb = geht nicht) · Schalter „Erledigte zeigen“.
- Prüfpunkt: Ticket-Chip, Titel, Chip „ab 0.38.0“; darunter zwei Kästen **Tun** und **Soll** nebeneinander.
- Tasten **Geht** (`thumbs-up`, Taste G) und **Geht nicht** (`thumbs-down`, Taste N), 40 px. Gedrückt: Fläche `ok` bzw. `warn`. Ein Klick sendet sofort.
- **Anmerkung** jederzeit nachschiebbar (auch nach „Geht“), eigener Senden-Knopf.
- Ergebnis: Kante in `ok`/`warn`, Meldung „Gesendet · Ticket #n · Uhrzeit“. Fehler wie bei den Fragen.
- Maximiert zweispaltig (siehe `ScreenAcceptance`).
