# Lücken im Design System

Was die App bedient, die Vorlage unter `woa-design-system/` aber noch nicht zeigt. Bis es eine
Vorlage gibt, baut `crates/gui` diese Stellen aus den vorhandenen Tokens und Bausteinen. Gesammelt
aus den Design-Nachbesserungen 0.34.19 und 0.34.21.

| Funktion | Wo in der App | Was eine Vorlage zeigen müsste |
|---|---|---|
| Auswahlzustand einer Spur | Spurkopf (Arrangement), Kanalzug (Mixer), Insert-Rack | Gewählt gegen nicht gewählt gegen scharf geschaltet, in Spurfarbe; wie stark Fläche und Rand gefärbt sind; dass Spurkopf, Kanalzug und Rack dieselbe Spur zeigen |
| Spurkopf | Arrangement links | Ganzer Baustein fehlt: Farbkante, Aufnahmepunkt, Name, M/S, Papierkorb, Gain, Quelle/Pan-Zeile, Höhe passend zur Zeitleiste |
| Spur löschen | Spurkopf (Papierkorb), Rechtsklick auf Spurkopf und Kanalzug, Entf | Ort und Gestalt des einen Papierkorbs, Bestätigung oder Rückgängig-Hinweis |
| Kontextmenü | Spurkopf, Kanalzug, Clips | Menü-Baustein: Abstände, Symbol + Text, Trenner, Gefahr-Eintrag (Löschen) in Rot |
| Insert-Rack | Studio-Dock rechts | Kopf (Farbpunkt, Name, Art), Leer-Zustand ohne Spuren, Reihenfolge der Gruppen |
| Effekt-Slots | Insert-Rack | Slot mit Bypass, Umsortieren, Entfernen, „+ Effekt“-Liste, Effektzahl am Kanalzug („fx n“) |
| Plugins | Insert-Rack | Plugin-Slot, Laden, Fenster öffnen/schließen, Bypass, Entfernen, Fehlerzustand |
| Routing und Sends | Insert-Rack | Ausgang-Auswahl, Send-Zeile mit Pegel, Ziel-Bus |
| Gain- und Pan-Felder | Insert-Rack, Spurkopf, Clip-Inspektor | Zahlenfeld (Ziehen, Doppelklick auf Standard), Einheit kurz (dB, %, st), Breite |
| „+ Bus“ | Mixer rechts | Knopf und Platz für neue Busse neben dem Master |
| Clip-Inspektor | Leiste über der Zeitleiste | Ganzer Baustein fehlt: Gruppen Fade, Gain, Zeit, Bearbeiten; Symbol-Knöpfe mit Tooltip; Leer-Zustand; Mehrfachauswahl („n Clips“) |
| Symbol-Knopf | Clip-Inspektor, Spurkopf | Knopf nur mit Symbol: Größe, Fläche, Hover, Tooltip-Stil, Gefahr-Farbe für Löschen |
| Tooltip | überall | Stil, Verzögerung, Tastenkürzel-Schreibweise (z. B. „Löschen (Entf)“) |
