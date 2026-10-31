# EffectSlot

Effekt- und Plugin-Slot im Insert-Rack: eine Zeile, Griff links, Bypass, Name, Aktionen rechts.

- **Griff** (sechs Punkte): Ziehen sortiert um. Der gezogene Slot hebt sich mit `accent`-Rahmen ab, die Einfügestelle ist ein 2-px-Strich in `accent`, der Platz darunter bleibt blass stehen.
- **Bypass** (Power-Symbol, B): an = `accent`; umgangen = gestrichelter Rahmen, Name gedimmt.
- **Eingebauter Effekt:** Klick auf den Namen klappt die Parameter als Zahlenfelder auf. Kein eigenes Fenster.
- **Plugin:** Formatkürzel (CLAP, VST3) neben dem Namen; Fenster öffnen/schließen (Symbol, eingedrückt = offen). Die Parameter liegen im Plugin-Fenster.
- **Laden:** Drehkreis statt Bypass, „lädt …“.
- **Fehler:** roter Rahmen, Warnsymbol, ein Satz Ursache. Abgestürzt → „Neu laden“, nicht gefunden → „Plugin suchen“. Die Spur läuft immer ohne das Plugin weiter.
- **Entfernen** (×, Entf): erscheint bei Hover und im Fehlerfall; rückgängig machbar, keine Rückfrage.
- Am Kanalzug zeigt „fx n“ die Anzahl, bei Fehler rot mit Warnsymbol.
