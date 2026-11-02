# DjHead

Deck-Kopf im DJ-Modus. **Alle Chips 28 px hoch, eine Mittellinie**, Symbole 14 px.

- Zeile 1: Chip „DECK A“ (Rahmen und Schrift in Deckfarbe) · Zustand (LÄUFT gefüllt nur bei Lauf, BEREIT/VORGELADEN ohne Füllung) · Titel/Interpret.
- Zeile 2: LCD BPM (7-Segment 20 px, Geister „888.8“) · Tonart-Chip Camelot (`is-ok` grün, wenn zum anderen Deck passend: gleich, ±1, A↔B) · Analyse-Chip · rechts LCD „REST 06:41“.
- Analyse läuft: Chip `cue` mit Kreisel und Prozent, BPM „---.-“, kein Tonart-Chip. Fehlgeschlagen: Chip `warn` + Symbol-Taste „Neu analysieren“.
