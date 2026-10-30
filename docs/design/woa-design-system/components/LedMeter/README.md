# LedMeter

Mehrgliedrige LED-Pegelkette −60…0 dB, vertikal oder horizontal, mit Peak-Hold – das „Blinken nach oben“ aller Modi.

Markup: leeres `.woa-meter` mit `data-ch` (Kanäle), `data-segs`, `data-level` (0…1); `WOA.init()` baut die Segmente. `woa-meter--h` horizontal, `woa-meter--accent` einfarbig für Gain-Reduction. `data-static` friert die Anzeige ein.

- Zonen fest: grün bis −12 dB (`led-green`), gelb bis −3 dB (`led-yellow`), rot darüber (`led-red`); unbeleuchtet sind sie zu 14 % angedeutet.
- Segment `led-seg` hoch, Abstand 2 px, Schacht `lcd-bg` – auch im hellen Thema.
- egui: `theme::led_meter` / `led_meter_vertical` (vorhanden), Peak-Hold ergänzen.
