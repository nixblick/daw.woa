WOA – Wir on Air ist eine DAW mit eingebautem Sendestudio und Party-Player. Ein Programm, drei Räume: **Produzent** ist das Tonstudio, **Radio** das Sendepult, **Party** das DJ-Pult. Alle drei teilen denselben Kern – dunkle Hardware-Flächen, LCDs, LED-Ketten, beleuchtete Tasten – und unterscheiden sich durch ihre Modusfarbe und ihre Leitbauteile.

## Die drei Modi

| Modus | Thema | Modusfarbe `accent` | Zweitfarbe `accent-2` | Leitbauteile |
| --- | --- | --- | --- | --- |
| Produzent | `studio` | Röhren-Amber | MIDI-Cyan | Pegelansicht (VU analog oder modern), Transport-LCD, Kanalzug, Insert-Rack, Zeitleiste |
| Radio | `radio` | Studio-Uhr-Rot | Tasten-Gelb | Studio-Uhr, ON-AIR-Lampe, Restzeit-LCD, Cart-Wall, Pult |
| Party | `party` | Club-Magenta | Beat-Lime | Jogwheel-Deck, Now-Playing, Großtasten, Wunschliste |

- Jeder Modus hat ein dunkles und ein helles Thema (`studio-hell`, `radio-hell`, `party-hell`). Dunkel ist Standard; Hell ist für Tageslicht und die Website.
- Wechsle nie Bauteile zwischen den Modi um, sondern nur die Gewichtung: Ein Kanalzug sieht überall gleich aus, nur der Fader-Strich und die Regler-Kränze nehmen `accent` an.
- Setze das Thema auf den Fenster-Container (`data-theme`); in egui bekommt `theme::apply` den Modus als Parameter.

## Farbe

- Flächen in vier Tiefen: `well` (Vertiefung) < `bg` (Fenster) < `panel` (Modul-Karte) < `surface` (Taste, Pad). Kopfzeilen von Modulen in `panel-header`. Kanten `border` sind dekorativ; bedienbare Elemente bekommen `border-strong`.
- LCDs, Uhren und LED-Ketten stehen immer auf `lcd-bg` – auch im hellen Thema, wie echte Hardware. Ziffern darauf in `lcd-ink` (= Modusfarbe), Text in `lcd-text`, unbeleuchtete Segmente `led-off`.
- `accent` markiert nur: Playhead, aktiven Modus, eingerastete Taste, Auswahl, laufendes Cart-Pad. Auf gefülltem `accent` steht `on-accent`.
- Statusfarben sind in allen Modi gleich und tragen immer ein Wort oder Symbol: `on-air` (Sendung, Aufnahme, offenes Mikro, Clip), `ok` (läuft, verbunden), `cue` (Vorhören, nächster Titel, MIDI), `warn` (Warnung).
- Im Radio-Modus ist `accent` ebenfalls Rot. Unterscheide dort: `accent` = Uhr/Restzeit/Anzeige, `on-air` = live. Live-Zustände tragen immer zusätzlich „ON AIR“, „REC“ oder „MIKRO OFFEN“.
- LED-Ketten: grün bis −12 dB (`led-green`), gelb bis −3 dB (`led-yellow`), rot darüber (`led-red`).
- Spurfarben `track-1` … `track-6` für Spurstreifen, Clips, Cart-Pad-Balken und Scribble-Strips – nie für Status.
- Kontrast: `text` und `text-dim` erreichen 4.5:1 auf `bg`, `panel` und `panel-header` in allen sechs Themen; `accent` als Text 4.5:1 auf `panel` und `bg`; Marken (Status, `accent-2`, `border-strong`, `focus`) 3:1. `border` liegt bewusst darunter und trägt nie allein eine Bedeutung.

## Schrift

- **Barlow** (`font-ui`) für Oberfläche und Listen: `body`, `body-strong`, `small`.
- **Barlow Condensed** (`font-label`) für alles, was auf ein Pult gedruckt wäre: `panel-label` und `key-cap` in Versalien, `title`, und für die Party-Anzeigen `display-l` und `display-party`.
- **JetBrains Mono** (`font-mono`) für Zahlen, Zeiten, Messwerte: `lcd-l`, `lcd-s`.
- **DSEG7 Classic** (`font-seg`) nur für Leuchtziffern in LCDs und Uhren (`lcd-clock`). Setze dahinter Geister-Segmente (`data-ghost="88:88"`), damit es wie ein echtes Display wirkt.
- Alle vier Schriften stehen unter der SIL Open Font License und liegen in `fonts/`. Für egui als TTF ins Repo legen und in `FontDefinitions` registrieren; der Lizenztext gehört nach `docs/`.

## Haptik und Form

- Tasten sind erhaben (`shadow-key`) und rasten gedrückt ein (`shadow-key-pressed`). Leuchtende Tasten zeigen den Zustand, nicht die Maus: `is-lit` = aktiv, `is-on` = voll an.
- Schächte (Fader, Meter, LCD) sind eingelassen (`shadow-well`).
- Radien: `radius-xs` LED-Segmente, `radius-sm` Meter und kleine Tasten, `radius-md` Widgets, `radius-lg` Module und Pads, `radius-xl` Fenster, `radius-party` Party-Tasten und Fernbedienung.
- Abstände: `space-3` in Zeilen und im Pad-Raster, `space-4` Kartenpolster, `space-5` zwischen Modulen, `space-7` Party-Ränder.
- Größen: Standard-Taste `key-h`, Party-Taste `key-party-h`, Regler `knob-d`, Faderweg `fader-h`, Cart-Pad `pad`.
- Kanalzüge haben keine Drehregler: nur Kanaltasten, Fader, LED-Kette und Name. Regler leben im Insert-Rack.
- Die Pegelansicht im Produzenten-Modus ist umschaltbar: Analog (VU), Modern (Balken) oder Aus.
- Rote Flächen mit weißer Schrift (ON-AIR-Lampe, gedrückte REC-Taste) nutzen `on-air-fill`; Schrift ohne Schatten, die Lampe sitzt in einem schwarzen Rahmen.
- Licht: Glows nur für Leuchtendes – `glow-accent` für aktive Tasten und Pads, `glow-on-air` für Lampe und REC. In der Kopfleiste kein Glow. Keine Verläufe als Dekoration.
- Bewegung hat einen Grund: LED-Ketten folgen dem Pegel, Sekundenkranz der Zeit, Jogwheel dem Abspielen, Party-Play-Taste dem Beat. Blinken nur für Aufnahme, Restzeit < 10 s und ein laufendes Pad. Bei `prefers-reduced-motion` stehen alle Anzeigen still.

## Inhalt und Sprache

- Deutsch, du-Form nur in Hilfetexten; Tasten sind Verben oder Zustände: „Start“, „Blende“, „Mikro“, „Wünschen“.
- Pult-Beschriftungen in Versalien (`panel-label`, `key-cap`): „SENDE-PLAYLIST“, „CART-WALL“, „DECK LÄUFT“.
- Zahlen mit Einheit und echtem Minuszeichen: „−14.1 LUFS“, „−0:04“, „+1.9 dB“.
- Keine Emojis in der Oberfläche (die bisherigen 🎚 📻 in `app.rs` ersetzen durch Icons).

## Ikonografie

- Linien-Icons 24×24, Strich 2 px, runde Enden; Transport-Symbole (Play, Stopp, Rec, Pause) gefüllt. `WOA.icon(name)` liefert sie für die Entwürfe.
- Für egui: `egui-phosphor` (MIT) im Stil „Regular“/„Fill“ verwenden; die Entwurfs-Icons sind diesem Stil nachgezeichnet, keine Kopie.
- Icons stehen in `text` auf `surface`, in leuchtenden Tasten in der Tastenfarbe.

## Logo

- Fünf LED-Säulen, deren Oberkanten ein W zeichnen, darüber die rote ON-AIR-Lampe; in der Wortmarke ist das O selbst die Lampe. „WIR ON AIR“ steht darunter und läuft exakt von der linken Kante des W bis zur rechten Kante des A.
- Auf dunklen Flächen `assets/Logos/*-dark.svg`, auf hellen `*-light.svg`; App-Icon `woa-icon-*`, Website-Kopf `woa-wordmark-*`, Tab `woa-favicon-*`.
- In der Kopfleiste nehmen die Säulen die Modusfarbe an (`WOA.mark()`), die Lampe bleibt Rot.

## Umsetzung in egui

- `crates/gui/src/theme.rs` bleibt die Quelle für die Rust-Konstanten; Studio-dunkel entspricht den heutigen Werten. Neu: ein `Mode`-Parameter für `accent`, `accent-2`, `lcd-ink` und die Flächen, plus `BORDER_STRONG`, `LCD_BG`, `LED_OFF`, `VU_FACE`, `VU_INK`.
- `card_frame`, `lcd_frame`, `status_pill`, `led_meter` gibt es schon; neu nach diesen Vorlagen: `key`, `knob`, `fader`, `vu_meter`, `studio_clock`, `onair_lamp`, `cart_pad`, `jog`.
- Die Entwürfe unter „Entwürfe“ sind Zielbilder, keine pixelgenaue Vorgabe; Abstände und Tokens sind verbindlich.

## Nicht übernommen

- Quelle: `nixblick/daw.woa` (main) – `crates/gui/src/theme.rs`, `docs/konzept.md`, `docs/ui-analyse-und-ziele.md` (Branch `feat/ui-studio-redesign-main`), App-Icon. Konkurrenz-Analysen über mAirList/Ableton hinaus lagen im Repo nicht vor.
- Komponenten sind HTML/CSS-Nachbauten als Vorlage für egui, keine Rust-Bausteine.
- Typo-Skala ist neu: `theme.rs` nutzt egui-Standardschriften mit Body/Button/Monospace 13.5 px, Small 11 px, Heading 16 px; hier `body` 14 px, `small` 12 px, `title` 22 px.


---

## Consuming this system (generated — do not edit)

Every path named below is under `project/` in this design system: read `project/api/tokens.md`, not `api/tokens.md`.

`components/bundle.js` defines `window.WOA` (20 components); `components/bundle.css` is its stylesheet; `tokens.css` is every token as a CSS variable plus `@font-face` for the fonts. `components/bundle.css` reads its variables from `tokens.css`. Build any UI by mounting these components; never hand-build a control or draw an icon the system provides.

- **Standalone page:** inline `tokens.css` and `components/bundle.css` in a `<style>`, then `components/bundle.js` as a classic script (a file containing `</style`, `</script` or `<!--` breaks an inline element: write the sequence `<\/style`, `<\/script` or `\x3C!--` in your copy, or load that file by URL).
- **Design canvas:** bring `tokens.css`, `components/bundle.css` and `components/bundle.js` onto the canvas in full, as the canvas type’s design-system components reference says (a server-side copy first where it offers one); in each artboard that uses this system, right after the `support.js` line, load `tokens.css` first, then the stylesheet, then the script; mount with `<x-import component-from-global-scope="WOA.<Comp>" …>`.
- **Slides deck, or any surface that cannot run the bundle:** tokens and fonts — the values are on `api/tokens.md`; the deck takes `tokens.json` by file path for its colour pickers, and fonts as “Fonts” below says.

Fonts: `tokens.css` loads each font from its file, by path.

- **Design canvas:** copy `fonts/Barlow-400.woff2`, `fonts/Barlow-500.woff2`, `fonts/Barlow-600.woff2`, `fonts/Barlow-700.woff2`, `fonts/BarlowCondensed-500.woff2`, `fonts/BarlowCondensed-600.woff2`, `fonts/BarlowCondensed-700.woff2`, `fonts/BarlowCondensed-800.woff2`, `fonts/JetBrainsMono-400.woff2`, `fonts/JetBrainsMono-500.woff2`, `fonts/JetBrainsMono-700.woff2` and `fonts/DSEG7Classic-Bold.woff2` to the same paths under the canvas’s `project/ds/<folder>/` (`<folder>` is the folder you install this system under; `fonts/Barlow-400.woff2` lands on `project/ds/<folder>/fonts/Barlow-400.woff2`), in the same call that installs this system; the copy of `tokens.css` that each artboard links loads them from there. If the install’s reply leaves a font out, bring that font in as for any other page.
- **Slides deck:** for each family you use in the deck (`faces` holds at most the number of typefaces Slides’ `deck-files.md` gives), install its file with the system and name the path it lands on as that family’s `src` in `project/deck.json` `faces`, under the key in the Slides column. A deck takes a font as a file only under the rule in Slides’ `fonts.md` (no space or leading underscore in its path, for two): any other font, or one kept by id, is uploaded as `fonts.md` says.
- **Any other page:** one file per family is enough (below: the upright face nearest regular weight; bold and italic synthesize). Fetch it as the fetch column says (Artifact tool `read`, that id or path as `path`), upload it as an asset (`publish`, `file_path`, `asset:true`) and declare it with an `@font-face { font-family: "<family>"; src: url(<uploaded>) }` rule.

| family | file | fetch | CSS | Slides `faces` key |
| --- | --- | --- | --- | --- |
| Barlow | `fonts/Barlow-400.woff2` (weight 400) | `read` the file with `project/` in front | `var(--font-ui)` | `barlow` |
| Barlow Condensed | `fonts/BarlowCondensed-500.woff2` (weight 500) | `read` the file with `project/` in front | `var(--font-label)` | `barlow-condensed` |
| JetBrains Mono | `fonts/JetBrainsMono-400.woff2` (weight 400) | `read` the file with `project/` in front | `var(--font-mono)` | `jetbrains-mono` |
| DSEG7 Classic | `fonts/DSEG7Classic-Bold.woff2` (weight 700) | `read` the file with `project/` in front | `var(--font-seg)` | `dseg7-classic` |

**Read, per thing:** a component’s props, parts and examples: `api/components/<Comp>.md`; token values: `api/tokens.md`; stored assets and their paths: `api/assets/<Group>.md`. After this README, fetch the cards and fonts you need in ONE message as parallel calls — none depends on another.

**Two rules.** Before you use a thing — a component, a token group, an icon, an asset — read its card from the index below; a value you did not read from a card is a guess. `tokens.json`, `manifest.json` and `design-system.json` are sources for tools: hand them over. `components/<Comp>/README.md` and `assets/<Group>/README.md` are the long-form second read a card links to; `SKILL.md` and `artifact-type/` beside them are authoring guidance, not needed to consume the system.

## Index (generated — do not edit)

**Tokens**

- `api/tokens.md` — Every token: surface, text, fill, border, palette, type, spacing, radius, shadow, size. (21.1k)

**Icons and assets**

- `api/assets/Logos.md` — 8 files, by asset id. (2.1k)

**Components** (`api/components/<Comp>.md`, 24; 4 of them showcase pages)

- **Party**: `BigKey` — Party-Großtaste (88 px, große Radien) für Bedienung per Touch und aus Distanz · `Deck` — DJ-Deck für den Party-Modus: Jogwheel, BPM-LCD, Wellenform mit Beatgrid und Großtasten
- **Radio**: `CartPad` — Jingle-/Cart-Wall-Pad mit Tastennummer, Status-LED und Spurfarben-Balken · `Playlist` — Sende-Playlist mit Zebra-Zeilen, Cover, Lautheit und farbigem 4-px-Streifen für ON AIR und CUE
- **Bedienelemente**: `ChannelKeys` — Kanaltasten Mute, Solo, PFL und Rec-Arm als feste Vierergruppe pro Kanal · `Fader` — Kanalfader (vertikal) und Crossfader (horizontal) mit geriffelter Kappe und Modusfarben-Strich · `Key` — Beleuchtete Hardware-Taste für jede Schalt- und Auslöseaktion in allen drei Modi · `Knob` — Drehregler mit LED-Kranz in Modusfarbe für Gain, EQ, Pan, Sends und Effektparameter · `ModeSwitch` — Segmentierter Umschalter Produzent · Radio · Party oben rechts in der Kopfleiste
- **Produzent**: `ChannelStrip` — Mischpult-Kanalzug: Gain, EQ, Pan, M/S/PFL, Fader mit Stereo-LED-Kette und Scribble-Strip · `Timeline` — Arrangement-Zeitleiste mit Spurköpfen, Audio- und MIDI-Clips, Takt-Lineal und Playhead · `Transport` — Transport-Konsole des Produzenten: Hardware-Tasten, Timecode-LCD, Eingangspegel, REC-Pille
- **Anzeigen**: `Lcd` — Tiefschwarzes Display für Restzeit, Titel, Messwerte – mit 7-Segment-Ziffern samt Geister-Segmenten · `LedMeter` — Mehrgliedrige LED-Pegelkette −60…0 dB, vertikal oder horizontal, mit Peak-Hold – das „Blinken nach oben“ aller Modi · `OnAirLamp` — ON-AIR-Lampe wie über der Studiotür: aus = dunkles Rotglas, an = leuchtend mit Glow · `StatusPill` — Beleuchtete LED-Pille für Zustände wie DECK LÄUFT, MIKRO OFFEN, REC, STREAM, MIDI · `StudioClock` — Radio-Studio-Uhr mit 60 Sekunden-LEDs, 12 Stundenpunkten und Digitalzeit in der Mitte · `VuMeter` — Analoges VU-Meter mit Zeiger und hinterleuchtetem Skalenblatt – Markenzeichen des Produzenten-Modus
- **Grundlagen**: `Logo` — WOA-Logo: fünf LED-Säulen in W-Form mit ON-AIR-Lampe, als Wortmarke, App-Icon und Favicon in Dunkel und Hell · `TopBar` — Gemeinsame Kopfleiste aller Modi: Marke, Menüs, MIDI-Pille, Mitschnitt, Modus-Umschalter, Feedback
- **Entwürfe**: `ScreenParty` (showcase page) — Entwurf Party-Modus: großer Now-Playing-Titel, zwei Decks, Wunschliste, Crossfader und Großtasten · `ScreenRadio` (showcase page) — Entwurf Radio-Modus: Sendepult mit Studio-Uhr, Restzeit-LCD, ON-AIR-Lampe, Playlist, Cart-Wall und Mikro-/Gäste-Pult · `ScreenRemote` (showcase page) — Entwurf der Party-Fernbedienung im Handy-Browser: Jetzt läuft, Wunsch-Suche, Voting, zwei Großtasten · `ScreenStudio` (showcase page) — Entwurf Produzenten-Modus: Tonstudio mit Transport, Arrangement, Analyse mit VU-Metern, Mixer und Insert-Rack
