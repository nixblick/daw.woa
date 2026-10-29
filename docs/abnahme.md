# Abnahme in Paketen

André nimmt fertige Pakete ab, nicht einzelne Funktionen (29.10.2026). Claude baut ein Paket am
Stück, prüft selbst (Tests, Offline-Render, Probelauf am Gerät) und merged. Hier steht je Paket
eine kurze Klickliste. Ein „passt“ von André schließt das Paket; was hakt, wird ein Issue.

## Paket 1: Aufnehmen und Abspielen (0.1.6, offen)

Start: `cd ~/GitHub/nixblick/daw.woa && git pull --ff-only && cargo run -q --release -p woa`

1. „+ Spur“, Kreis vor der Spur anklicken (Ziel der Aufnahme).
2. „● Aufnahme“ oder Strg+R, ein paar Sekunden sprechen, nochmal klicken. Ein blauer Clip erscheint.
3. Pos1, dann Leertaste: die Aufnahme ist zu hören, der rote Kopf läuft mit.
4. Beim Abspielen zeigt der Balken vorne in der Spur den Pegel (grün, gelb, rot); M macht ihn leer.
5. In die Zeitleiste klicken: der Kopf springt dorthin. Gain und M (stumm) wirken beim Abspielen.
6. Zweite Spur wählen, Kopf auf den Anfang, aufnehmen: die erste Spur läuft dabei mit.
7. Clip anklicken, Entf: weg. Strg+Z: wieder da.
8. „Speichern unter …“, WOA schließen, neu starten, „Öffnen …“: Clips sind da und spielen.

Selbst geprüft von Claude: 40 Testläufe grün, Offline-Render eines Projekts mit Take (Stille vor
dem Clip, Ton im Clip, Stille danach), `play --project` am Gerät 5 s `underruns=0`.

Noch nicht drin (nächste Pakete): Clips verschieben/schneiden, Fades, Wellenform, Latenzausgleich,
Export.

## Abgenommen

- M1 Engine: 29.10.2026, 60 s Testton sauber, `underruns=0` (#2).
