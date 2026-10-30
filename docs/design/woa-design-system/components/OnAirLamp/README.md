# OnAirLamp

ON-AIR-Lampe wie ein Leuchtkasten über der Studiotür: schwarzer Rahmen, Rotglas, klare weiße Schrift.

`<div class="woa-onair is-on" role="status">ON AIR</div>`, klein `woa-onair--sm` (auch für „REC“).

- Rahmen 5 px `lcd-bg` (klein 3 px) – in allen Themen schwarz, trennt die Lampe von jeder Fläche.
- An: Fläche `on-air-fill`, Schrift reines Weiß ohne Schatten oder Weichzeichner (5.9:1). Glow nur außen am Rahmen (`glow-on-air`).
- Aus: dunkles Rotglas, Schrift gedimmt rot – Lampe bleibt als Objekt sichtbar.
- Leuchtet genau dann, wenn der Stream verbunden ist UND der Sendebus Signal hat. Pro Bildschirm höchstens eine große Lampe.
