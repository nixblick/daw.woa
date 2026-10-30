# Lcd

Tiefschwarzes Display für Restzeit, Titel, Messwerte – mit 7-Segment-Ziffern samt Geister-Segmenten.

Markup: `.woa-lcd`; Überschriften `.woa-lcd__cap`; große Zahlen `<span class="woa-seg" data-ghost="88:88"><span>03:42</span></span>` (Schrift DSEG7, `lcd-clock`). Leuchtfarbe `lcd-ink` = Modusfarbe. Fortschritt `.woa-progress` mit `--p`.

- Unter 10 s Restzeit: `is-alert` (on-air) + `is-blink`; unter 30 s optional `warn`.
- LCD bleibt in allen Themen dunkel (`lcd-bg`), Text darauf `lcd-text`.
- Titelnamen im LCD in `font-ui`, Zahlen in `font-seg` oder `font-mono`.
