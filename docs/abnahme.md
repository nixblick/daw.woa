# Abnahme in Paketen

André nimmt fertige Pakete ab, nicht einzelne Funktionen (29.10.2026). Claude baut ein Paket am
Stück, prüft selbst (Tests, Offline-Render, Probelauf am Gerät) und merged. Hier steht je Paket
eine kurze Klickliste. Ein „passt“ von André schließt das Paket; was hakt, wird ein Issue.

## Paket 3: Arrangieren (0.1.8, offen)

Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. Zwei Spuren mit je einer Aufnahme. Einen Clip auf die andere Spur ziehen: er liegt dort, Strg+Z holt ihn zurück.
2. Rechten Rand eines Clips nach links ziehen: Clip wird kürzer, spielt nur noch bis dort.
3. Linken Rand nach rechts ziehen: der Anfang fällt weg, die Wellenform bleibt an ihrer Stelle in der Zeit.
4. Strg+Mausrad oder − und + oben: die Zeitleiste zoomt.
5. Strg+E dreimal: als .wav, .flac und .mp3 speichern, alle drei im Player anhören.

Selbst geprüft von Claude: Workspace-Tests grün (u. a. Trimmen, Spurwechsel mit Undo, Export in allen drei
Formaten), Probe-Projekt als WAV, FLAC und MP3 gerendert und mit `ffprobe` gelesen (48 kHz, stereo, 4,0 s;
MP3 320 kbit/s).

Noch nicht drin: Latenzausgleich, Lautheit (LUFS) beim Export, mehrere Clips auf einmal markieren.

## Abgenommen

- Paket 2 Bearbeiten und Export (0.1.7): 29.10.2026, André „passt“.

- Paket 1 Aufnehmen und Abspielen (0.1.6): 29.10.2026, André „passt“.

- M1 Engine: 29.10.2026, 60 s Testton sauber, `underruns=0` (#2).
