# Abnahme in Paketen

André nimmt fertige Pakete ab, nicht einzelne Funktionen (29.10.2026). Claude baut ein Paket am
Stück, prüft selbst (Tests, Offline-Render, Probelauf am Gerät) und merged. Hier steht je Paket
eine kurze Klickliste. Ein „passt“ von André schließt das Paket; was hakt, wird ein Issue.

## Paket 4: Radio-tauglich (0.1.9, offen)

Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. Zu einem vorhandenen Clip mitsprechen oder klatschen und dabei aufnehmen (Haken „Latenz ausgleichen“ an): die neue Aufnahme sitzt zeitlich auf dem Original. Liegt sie noch zu spät, den ms-Wert daneben erhöhen.
2. Strg+Klick auf zwei Clips: beide gelb. Einen ziehen: beide wandern mit. Entf: beide weg, Strg+Z: beide zurück.
3. Oben „Export-Lautheit: −16 LUFS“, Strg+E: die Statuszeile zeigt vorher/nachher, die Datei ist deutlich lauter als roh.
4. Dasselbe mit −23 LUFS: leiser als bei −16.

Selbst geprüft von Claude: Workspace-Tests grün (u. a. Lautheit eines Sinus, Angleichen mit Spitzengrenze, Latenz-Versatz,
Mehrfach verschieben/löschen mit Undo). Probe-Export gegengemessen mit ffmpeg: roh −42,3, Ziel −23 → −23,0 LUFS.
Ziel −16 erreicht die Probe nur bis −23 LUFS, weil sie kurze laute Spitzen hat (Grenze −1 dBFS). Eingangslatenz am Notebook 22,3 ms.

Noch nicht drin: Limiter (damit spitzes Material −16 LUFS erreicht), automatische Latenzmessung per Klick-Test.

## Abgenommen

- Paket 3 Arrangieren (0.1.8): 29.10.2026, André „passt“.

- Paket 2 Bearbeiten und Export (0.1.7): 29.10.2026, André „passt“.

- Paket 1 Aufnehmen und Abspielen (0.1.6): 29.10.2026, André „passt“.

- M1 Engine: 29.10.2026, 60 s Testton sauber, `underruns=0` (#2).
