# Abnahme in Paketen

André nimmt fertige Pakete ab, nicht einzelne Funktionen (29.10.2026). Claude baut ein Paket am
Stück, prüft selbst (Tests, Offline-Render, Probelauf am Gerät) und merged. Hier steht je Paket
eine kurze Klickliste. Ein „passt“ von André schließt das Paket; was hakt, wird ein Issue.

## Paket 2: Bearbeiten und Export (0.1.7, offen)

Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. Etwas aufnehmen (wie Paket 1). Im blauen Clip ist die Wellenform zu sehen.
2. Clip mit der Maus nach rechts ziehen und loslassen: er bleibt dort, Strg+Z holt ihn zurück.
3. Roten Kopf in die Mitte des Clips klicken, Clip anklicken, S drücken: zwei Clips. Beide spielen lückenlos weiter.
4. Einen Clip anklicken, oben Fade-in und Fade-out ziehen: dunkler Schatten im Clip, beim Abspielen blendet es weich ein und aus.
5. Datei → Exportieren als WAV (Strg+E), Datei mit einem Player öffnen: klingt wie im Fenster, endet am letzten Clip.

Selbst geprüft von Claude: Workspace-Tests grün (u. a. Schneiden, Fades, Verschieben mit Undo, Render und Export),
echte Aufnahme in zwei Clips mit Versatz und Fades offline gerendert und gemessen (Fade-Anfang 0.004,
voll 0.047, Lücke 0.0, Fade-Ende 0.008), `play --project` am Gerät `underruns=0`.

Noch nicht drin: Clips auf andere Spur ziehen, Clip-Ränder ziehen (trimmen), Latenzausgleich, MP3/FLAC.

## Abgenommen

- Paket 1 Aufnehmen und Abspielen (0.1.6): 29.10.2026, André „passt“.

- M1 Engine: 29.10.2026, 60 s Testton sauber, `underruns=0` (#2).
