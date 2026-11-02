# DjDeck

DJ-Deck in der Party-Ansicht „DJ“: alles, was zum Mixen von Hand nötig ist, in einer Karte je Deck.

**Kopf:**

- Deck-Buchstabe in Deckfarbe und Status: LÄUFT, BEREIT · CUE, VORGELADEN.
- Titel und Interpret.
- Tonart als Camelot-Chip; grün, wenn sie zum anderen Deck passt.
- BPM-LCD und Restzeit-LCD (MM:SS mit Geister-Segmenten).

**Übersicht:** ganzer Titel als Wellenform, gespielter Teil in Deckfarbe. Hot-Cue-Marken mit Nummer und Farbe, aktive Loop als Lime-Bereich. Klick springt.

**Bedienung:**

- **Jogwheel:** Drehen am Rand schiebt das Tempo an, oben auf der Scheibe wird gescratcht.
- **Hot Cues 1–4:** Pads mit Farbbalken. Klick springt, leer = setzen, Umschalt+Klick löscht.
- **Auto-Loop:** 1 · 2 · 4 · 8 · 16 Schläge als Segmente, „Loop“ schaltet. Eingeschaltet = Lime.
- **Cue** (`cue`, Cyan): Steht das Deck, setzt Cue den Cue-Punkt. Halten spielt ab Cue, Loslassen springt zurück. Leuchtet, wenn das Deck am Cue-Punkt steht.
- **Play/Pause:** gefüllt in `accent`, pulsiert im Beat.
- **Sync** (Lime, wenn an): gleicht Tempo und Takt ans andere Deck an.
- **Keylock:** Symbol-Schalter, hält die Tonhöhe beim Tempoändern.
- **Tempo-Fader:** ±8 %, Wert darunter, Anschieben langsamer/schneller als Symbol-Tasten. Doppelklick = 0 %.

**Spiegelung:** Deck A hat den Plattenteller links und das Tempo rechts, Deck B gespiegelt – wie an einem DJ-Controller.

**Deckfarben:** A = `accent` (Magenta), B = `track-1` (Blau). Gleiche Farbe in Wellen, Mixer-Kanal und Kartenkante.


## Fassung 7

Kopf nach `DjHead`: Chips einheitlich 28 px, BPM und Restzeit als LCD in derselben Höhe.
