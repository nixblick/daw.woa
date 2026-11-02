# QuestionsWindow

Fenster „Fragen und To-dos“ (Entwickler-Modus).

- Werkzeugzeile: großes Suchfeld 48 px (Fokus: Rahmen `accent` + 3 px `accent-soft`), Filter `Offen | Gesendet | Alle`.
- Blöcke nach Ansicht (`woa-blk`: Label + Zähler + Linie).
- **Frage-Karte** (`woa-q`): linke Kante 4 px (Zustand), Kopf mit Ticket-Chip (`#320`, klickbar → GitHub) · Titel 16/700 · Zustandschip rechts. Erklärung 13/20 in `text-dim`, max. 86 Zeichen breit.
- **Vorschläge** (`woa-opt`, min. 44 px): Nummer im Kreis · Text · rechts „Dafür“ (`thumbs-up`) und „Dagegen“ (`thumbs-down`). Gewählt: Zeile und Nummer in `ok` bzw. `warn`. Mehrere Vorschläge dürfen bewertet werden.
- **Anders + Kommentar:** Feld unter den Vorschlägen („Anders? Eigenen Vorschlag …“), gefüllt = Rahmen `accent-2`. Senden (`ok`) ist erst aktiv, wenn etwas gewählt oder geschrieben ist; Strg+Enter.
- **Gesendet:** Kante + Chip `ok`, Meldung „Gesendet, danke · #n“ mit „Nachtrag“; 1,5 s Leuchtrahmen (`is-flash`), danach ruhig. Filter „Offen“ blendet sie beim nächsten Öffnen aus.
- **Fehler:** Kante + Chip `warn`, Meldung mit Grund und „Erneut senden“; Eingaben bleiben stehen.
- **Sendet:** Meldung mit Kreisel, Tasten gesperrt.
