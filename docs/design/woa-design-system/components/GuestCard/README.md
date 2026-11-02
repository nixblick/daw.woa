# GuestCard

Gäste-Karte im Radio-Modus: Gäste schalten sich per Link im Browser zu (WebRTC), kein Programm nötig. Ersetzt die Taste „Telefon“ im Sende-Deck.

**Ort:** linke Spalte von ScreenRadio, zwischen Sende-Deck und Playlist. Höhe wächst mit der Gästezahl (max. 4 Zeilen à 48 px), die Playlist gibt Platz ab.

## Aufbau

- **Kopf:** „Gäste“, Zähler `verbunden / max` (`woa-pill--ok`, sobald ≥ 1), rechts `Key` „An“ mit LED (`woa-key--alt`, `is-lit`). Erst „An“ öffnet die Leitung und erzeugt einen Link.
- **Linkzeile** (`woa-gst__link`): Einladungslink (Mono, Symbol `link`), PIN, Symbol-Tasten Kopieren (`copy`), QR groß (`qr`, Taste Q), Neuer Link (`reload`, alter Link wird ungültig).
- **Gastzeile** (`woa-gst__r`, 48 px): Kante in Spurfarbe · Avatar mit Initiale · Name + Statuszeile (Verbindungssymbol, Zustand, Latenz, Dauer) · LED-Kette 14 Segmente · Aktionen.
- **Fußzeile:** Hinweis Mix-Minus und Vorgespräch.

## Zustände

| Zustand | Zeile | Aktionen |
|---|---|---|
| Aus | nur Kopf, Satz „Gästeleitung zu“ | „An“ |
| An, leer | QR 72 px + Kurzanleitung | Kopieren, QR groß, Neuer Link |
| Wartet | `is-wait` (8 % accent-2), Avatar blinkt | `Key ok` „Annehmen“, Symbol „Ablehnen“ (`x`, danger) |
| Vorgespräch | neutral, Kopfhörer gedrückt (`woa-gst__pfl` in `cue`) | Kopfhörer, „On Air“ (`woa-key--rec`), Stumm, Trennen |
| On Air | `is-air` (8 % on-air), „On Air“ `is-on` | wie oben |
| Stumm | Stumm gedrückt (`woa-gst__mute` in `warn`) | wie oben |
| Schwach | Symbol `wifi-low` in `warn`, Text „schwach · 180 ms“ | wie oben |
| Getrennt | `is-lost` (70 %), `wifi-off` in `on-air` | Neu verbinden, Entfernen |

## Regeln

- **Annehmen landet nie direkt auf Sendung**, immer erst im Vorgespräch. Auf Sendung nur über „On Air“.
- Rot nur bei „On Air“ (Taste und Zeilenton). Kein zusätzliches ON-AIR-Badge, der Zustand steht in der Statuszeile.
- **Mix-Minus:** jeder Gast hört die Sendung ohne seine eigene Stimme. Im Vorgespräch hört der Gast nur den Moderator, die Sendung läuft unbeeinflusst.
- Gäste laufen in der Pegelgruppe als ein Kanal **„Gäste“** (Summe, nach Mikro). Einzelpegel nur in der Karte.
- Ducking wie beim Mikro, sobald ein Gast On Air ist.
- Tooltips mit Taste: „Vorgespräch (V)“, „QR groß zeigen (Q)“, „Entfernen (Entf)“.
- MCP/MIDI: `guest_accept`, `guest_on_air`, `guest_mute`, `guest_kick` je Gast-Slot 1–4; MIDI-lernbar.
- Kein Link, keine PIN in Logs oder Screenshots fürs Repo; Beispieladresse `sendung.example`.
