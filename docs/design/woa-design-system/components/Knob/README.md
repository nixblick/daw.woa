# Knob

Drehregler mit LED-Kranz in Modusfarbe für Gain, EQ, Pan, Sends und Effektparameter.

Markup: `.woa-knob` mit `--v` (0…1), darin `.woa-knob__dial > .woa-knob__cap`, optional `.woa-knob__val` und `.woa-knob__name`. `woa-knob--center` zeichnet den Kranz von der Mitte (EQ, Pan). `data-fmt`: `db`, `eq`, `pan`, `pct`. `woa-knob--sm` = 32 px.

- Bedienung: vertikal ziehen, Pfeiltasten (Umschalt = grob), Doppelklick = `data-default`.
- Kranz 270°, Start unten links; Zeiger `text` auf `surface`.
- egui: `Sense::drag`, Winkel `v*270°-135°`, Bogen mit `accent`, Rest `led-off`.
