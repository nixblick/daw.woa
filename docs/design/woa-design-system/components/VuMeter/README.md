# VuMeter

Pegelansicht des Produzenten-Modus mit drei umschaltbaren Ansichten: Analog (VU-Meter mit Zeiger), Modern (Balken) oder Aus.

Markup: `.woa-levels` mit `data-view="analog|modern|aus"`, Umschalter `.woa-seg3` (Tasten mit `data-view`), darin `.woa-levels__analog` (zwei `.woa-vu`), `.woa-levels__modern` (`.woa-bars` mit `data-ch="L,R"`) und `.woa-levels__off`. `WOA.init()` zeichnet beides und verdrahtet den Umschalter.

- **Analog:** Skalenblatt `vu-face`, Skala `vu-ink`, roter Bereich ab 0 VU in `on-air`, Hinterleuchtung `accent`. Zeiger träge (≈300 ms).
- **Modern:** −40…+3 dBFS; RMS-Balken in `accent`, Peak-Strich `lcd-text`, Zahl in `lcd-ink` (über −1 dB in `on-air`), Bereich über 0 dB rot hinterlegt. Auf `lcd-bg`.
- **Aus:** blendet die Ansicht aus; die LED-Kette im Transport bleibt.
- Die Wahl ist eine Einstellung pro Nutzer und wird mit dem Layout-Preset gespeichert.
