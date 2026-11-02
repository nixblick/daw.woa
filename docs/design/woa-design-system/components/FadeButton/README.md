# FadeButton

Fade-Taste, **Dauer fest 3 s**, Symbol `fadeout`.

- **Sende-Deck:** „Fade“ (44 px) nach „Stopp“ – blendet alles Laufende aus (Playlist, Decks, Carts) und stoppt.
- **Decks A–C:** quadratische Taste neben Start/Stopp. **Cart-Wall:** Symbol oben rechts beim Hover/laufend, auch Rechtsklick. **Playlist:** Symbol in der laufenden Zeile.
- **Blendet aus** (`is-fading`): Rahmen `cue`, Füllung `cue` 28 % wächst mit `--p` von links, Restzeit „1,1 s“ in Mono. Erneuter Klick = sofort Stopp.
- **Nichts läuft:** gesperrt.
- Tooltip „Fade (3 s)“, Hinweis „Blendet in 3 Sekunden aus und stoppt“. MIDI-lernbar je Deck.
