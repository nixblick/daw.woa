# Lücken im Design System

Was die App bedient, die Vorlage unter `woa-design-system/` aber bis 31.10.2026 nicht zeigte. Bis es eine
Vorlage gibt, baut `crates/gui` diese Stellen aus den vorhandenen Tokens und Bausteinen. Gesammelt
aus den Design-Nachbesserungen 0.34.19 und 0.34.23.

| Funktion | Wo in der App | Was eine Vorlage zeigen müsste | Stand |
|---|---|---|---|
| Auswahlzustand einer Spur | Spurkopf (Arrangement), Kanalzug (Mixer), Insert-Rack | Gewählt gegen nicht gewählt gegen scharf geschaltet, in Spurfarbe; wie stark Fläche und Rand gefärbt sind; dass Spurkopf, Kanalzug und Rack dieselbe Spur zeigen | **geschlossen** (Paket 31.10.2026, `SelectionSync`) |
| Spurkopf | Arrangement links | Ganzer Baustein fehlt: Farbkante, Aufnahmepunkt, Name, M/S, Papierkorb, Gain, Quelle/Pan-Zeile, Höhe passend zur Zeitleiste | **geschlossen** (Paket 31.10.2026, `TrackHeader`) |
| Spur löschen | Spurkopf (Papierkorb), Rechtsklick auf Spurkopf und Kanalzug, Entf | Ort und Gestalt des einen Papierkorbs, Bestätigung oder Rückgängig-Hinweis | **geschlossen** (Paket 31.10.2026, `DeleteTrack`) |
| Kontextmenü | Spurkopf, Kanalzug, Clips | Menü-Baustein: Abstände, Symbol + Text, Trenner, Gefahr-Eintrag (Löschen) in Rot | **geschlossen** (Paket 31.10.2026, `ContextMenu`) |
| Insert-Rack | Studio-Dock rechts | Kopf (Farbpunkt, Name, Art), Leer-Zustand ohne Spuren, Reihenfolge der Gruppen | **geschlossen** (Paket 31.10.2026, `InsertRack`) |
| Effekt-Slots | Insert-Rack | Slot mit Bypass, Umsortieren, Entfernen, „+ Effekt“-Liste, Effektzahl am Kanalzug („fx n“) | **geschlossen** (Paket 31.10.2026, `EffectSlot, MixerBus`) |
| Plugins | Insert-Rack | Plugin-Slot, Laden, Fenster öffnen/schließen, Bypass, Entfernen, Fehlerzustand | **geschlossen** (Paket 31.10.2026, `EffectSlot`) |
| Routing und Sends | Insert-Rack | Ausgang-Auswahl, Send-Zeile mit Pegel, Ziel-Bus | **geschlossen** (Paket 31.10.2026, `InsertRack`) |
| Gain- und Pan-Felder | Insert-Rack, Spurkopf, Clip-Inspektor | Zahlenfeld (Ziehen, Doppelklick auf Standard), Einheit kurz (dB, %, st), Breite | **geschlossen** (Paket 31.10.2026, `NumberField`) |
| „+ Bus“ | Mixer rechts | Knopf und Platz für neue Busse neben dem Master | **geschlossen** (Paket 31.10.2026, `MixerBus`) |
| Clip-Inspektor | Leiste über der Zeitleiste | Ganzer Baustein fehlt: Gruppen Fade, Gain, Zeit, Bearbeiten; Symbol-Knöpfe mit Tooltip; Leer-Zustand; Mehrfachauswahl („n Clips“) | **geschlossen** (Paket 31.10.2026, `ClipInspector`) |
| Symbol-Knopf | Clip-Inspektor, Spurkopf | Knopf nur mit Symbol: Größe, Fläche, Hover, Tooltip-Stil, Gefahr-Farbe für Löschen | **geschlossen** (Paket 31.10.2026, `IconButton`) |
| Tooltip | überall | Stil, Verzögerung, Tastenkürzel-Schreibweise (z. B. „Löschen (Entf)“) | **geschlossen** (Paket 31.10.2026, `Tooltip`) |

Alle Lücken sind mit dem Paket vom 31.10.2026 geschlossen (Abschnitt „Bearbeiten und Auswahl“ im
README des Design Systems). Was davon in der App steht und was fehlt, steht in `docs/abnahme.md`.

Offene Entscheidungen von André (31.10.2026); bis dahin gilt das Design:

- **Pan doppelt:** Pan steht im Spurkopf und im Rack-Ausgang. Die Spurkopf-Anzeige hängt an der
  Konstante `HEADER_PAN` in `crates/gui/src/app.rs`; `false` nimmt sie heraus.
- **Gain/Trim:** Das Design nennt das Feld im Rack-Eingang „Trim“. Das Modell kennt keinen eigenen
  Eingangs-Trim, nur den Spur-Gain (derselbe Wert wie Fader und Spurkopf).
