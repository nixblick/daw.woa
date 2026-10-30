# WOA Design System

Gestaltungsvorgabe für die Oberfläche (`crates/gui`), erstellt mit Claude Design.
`woa-design-system.zip` ist das Original, `woa-design-system/` der entpackte Inhalt.

| Was | Wo |
|---|---|
| Brand-Book: Modi, Farbe, Schrift, Haptik, Sprache, Icons, Logo, Umsetzung in egui | `woa-design-system/README.md` |
| Tokens für 6 Themen: `studio`, `radio`, `party` und je `-hell` | `tokens.json` (Quelle für Werkzeuge), `tokens.css` (CSS-Variablen + `@font-face`) |
| Bausteine (Key, Fader, Knob, VuMeter, LedMeter, Lcd, StudioClock, OnAirLamp, CartPad, Deck, …), je `README.md` + `preview.html` | `components/<Baustein>/` |
| Gemeinsames CSS/JS der Vorschauen | `components/bundle.css`, `components/bundle.js` |
| Entwürfe (Zielbilder, nicht pixelgenau) | `components/ScreenStudio`, `ScreenRadio`, `ScreenParty`, `ScreenRemote` |
| Logos als SVG (Wortmarke, App-Icon, Bildmarke, Favicon, je dunkel/hell) | `assets/Logos/` |
| Schriften Barlow, Barlow Condensed, JetBrains Mono, DSEG7 Classic (woff2, SIL OFL 1.1) | `fonts/`, Herkunft in `docs/herkunft.md` |

Verbindlich sind Tokens und Abstände; die Screen-Entwürfe zeigen das Ziel. Die Vorschauen
öffnet man direkt im Browser (`preview.html`).
