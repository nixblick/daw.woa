# Senden: woa-streamd und Shoutcast

WOA sendet über einen eigenen Prozess, `woa-streamd`. Die Oberfläche liefert nur den Sendemix.
Stürzt sie ab, sendet woa-streamd weiter: nach 2 s ohne Ton die Notfall-Playlist, und wenn die
Oberfläche wieder da ist, blendet er über 1 s zurück.

```
Mixer (Audio-Thread) ──rtrb──▶ Feeder-Thread ──lokal──▶ woa-streamd ──MP3/TCP──▶ Shoutcast DNAS
                                                         │ Notfall-Playlist
                                                         └ streamd-status.json (alle 250 ms)
```

## Server

Der Server ist der eigene Shoutcast DNAS v2 auf lfs001 (bretteins.infra#150).

- Quelle: `stream.wironair.de:8001`, Stream-ID 1
- Hören: <https://stream.wironair.de/stream>. Ohne Quelle spielt der Server eine eigene Notfall-Schleife; das ist erwartet.

## Protokoll: Shoutcast 1 (Legacy)

DNAS v2 nimmt das alte Quellprotokoll an:

1. Kennwort und CRLF senden. Ab Stream-ID 2 lautet die Zeile `kennwort:#sid`.
2. Der Server antwortet mit `OK2`.
3. `icy-*`-Kopfzeilen senden (`content-type: audio/mpeg`, `icy-name`, `icy-br`, `icy-pub:0` …), dann eine Leerzeile.
4. Danach folgt rohes MP3.

Warum nicht Uvox2 (das v2-Protokoll):

- Uvox2 braucht Binär-Rahmen und eine XTEA-verschlüsselte Anmeldung, also viel mehr Code für dasselbe Ergebnis.
- Auf lfs001 ist nur der v1-Quellport von außen offen.

**Titel:** Das v1-Protokoll kann keine Titel in der Quellverbindung tragen; die `icy-*`-Kopfzeilen
gelten nur einmal beim Anmelden. Titel gehen nur per HTTP an `admin.cgi?mode=updinfo`, und dabei
steht das Quell-Kennwort im Klartext in der URL. WOA setzt Titel deshalb **nur bei lokalem Server**
(Host Loopback) und eingetragenem `admin_port`. Gegen `stream.wironair.de` werden **vorerst keine
Titel gesendet**; der Admin-Port 8000 ist dort von außen ohnehin gesperrt. Die Hörerzahl (`/7.html`,
ohne Kennwort) kommt, sobald ein `admin_port` erreichbar ist.

**Bekanntes Risiko:** Die Quellanmeldung auf 8001 läuft im Klartext (Shoutcast 1 kennt kein TLS).
Wer die Strecke mitlesen kann, sieht das Kennwort. Abhilfe wäre ein Tunnel oder TLS-Proxy vor dem
DNAS; das gehört zum Server (bretteins.infra).

**Wiederverbinden:**

- Backoff von 1 s bis 30 s.
- Nach abgelehntem Kennwort wird erst nach 60 s wieder versucht.
- In der Zwischenzeit werden höchstens 5 s MP3 gepuffert, Älteres fällt weg.

**Kodierung:** MP3 CBR 128 kbit/s mit LAME, 44,1 kHz (oder 48 kHz).

## Einrichten

1. Kennwort-Datei anlegen, eine Zeile, Modus 600: `~/.secrets/daw.woa/shoutcast-source.pass`.
   Den Wert gibt der Server-PR in bretteins.infra vor. Er steht nie im Repo und nie in der
   Konfiguration. WOA liest nur die Datei und schreibt das Kennwort in kein Log.
2. Optional in `~/.config/woa/config.json` den Schlüssel `stream` setzen. Alle Felder haben Vorgaben:

```json
"stream": {
  "host": "stream.wironair.de",
  "port": 8001,
  "admin_port": null,
  "stream_id": 1,
  "bitrate_kbps": 128,
  "sample_rate": 44100,
  "password_file": "/home/<du>/.secrets/daw.woa/shoutcast-source.pass",
  "name": "WOA – Wir on Air",
  "emergency": ["/home/<du>/Musik/notfall"],
  "silence_seconds": 2.0
}
```

`emergency` nimmt Dateien oder Ordner (WAV, MP3, FLAC …). Sie laufen der Reihe nach und endlos. Ist
die Liste leer, sendet woa-streamd bei Ausfall Stille.

## Bedienen

Im Radio-Modus gibt es den Knopf **„Auf Sendung“**:

- Er startet woa-streamd abgelöst, in einer eigenen Prozessgruppe, und hängt den Sendemix an.
- Daneben zeigt eine Zeile den Zustand: verbunden, verbinde neu oder Notfall-Playlist, dazu Hörer, Bitrate und Dauer.
- **„Sendung beenden“** meldet ab und beendet woa-streamd.
- Schließt man nur das Fenster oder stürzt WOA ab, läuft die Sendung weiter. Beim nächsten Start hängt sich WOA wieder an.

Weitere Wege an die Sendung:

- **Agent:** Das MCP-Tool `stream_status` liefert den Inhalt von `streamd-status.json` mit `running` und `summary`.
- **Probe ohne Oberfläche:**

```
cargo run -q -p woa-headless -- stream-test --host stream.wironair.de --port 8001 --sid 1 \
  --pass-file ~/.secrets/daw.woa/shoutcast-source.pass --seconds 30
```

Exit-Codes:

| Code | Bedeutung |
|---|---|
| 0 | gesendet |
| 3 | Kennwort abgelehnt |
| 4 | Server nicht erreichbar |
| 1 | sonst |

## Lokale Verbindung Oberfläche → woa-streamd

**Unix:** Unix-Socket `link.sock` in `<datenordner>/streamd/`.

- Der Ordner hat Modus 0700, der Socket 0600. Fremde Benutzer kommen nicht an den Daemon.
- Ein Symlink als Ordner wird abgelehnt.

**Windows:** TCP auf 127.0.0.1.

**Für beide gilt:**

- Ein Token mit 64 Hex-Zeichen steht in `streamd.link` (Unix 0600) und wird in konstanter Zeit verglichen.
- Die Anmeldung hat feste Größe und eine Frist von 200 ms.
- Höchstens 4 Verbindungen warten gleichzeitig auf die Anmeldung. Kommt eine weitere, fliegt die älteste wartende.
- Nach erfolgreicher Anmeldung wird die Verbindung unter derselben Sperre aus der Warteliste genommen und kann nicht mehr verdrängt werden.
- Rahmen sind auf 1 MiB begrenzt, und die Größe wird vor jeder Allokation geprüft.

**Bekannter Rest (Windows):** Ein lokaler Prozess desselben Rechners kann durch viele Verbindungen
das Verbinden der Oberfläche stören. Senden kann er ohne Token nicht. Eine Named Pipe mit ACL nur für
den Benutzer wäre besser, ist aber mit `std` allein nicht sauber zu bauen und deshalb nicht gebaut.

## Fehlerbilder

| Anzeige / Log | Ursache | Abhilfe |
|---|---|---|
| „Kennwort abgelehnt“ | falsches Kennwort oder falsche Stream-ID | Kennwort-Datei prüfen; nächster Versuch nach 60 s |
| „verbinde neu“ mit Connect-Fehler | Server weg, Port zu, kein Netz | wartet selbst, Backoff bis 30 s |
| „Kennwortdatei … nicht lesbar“ | Datei fehlt | Datei anlegen (siehe oben) |
| „woa-streamd läuft schon“ | zweiter Start | nichts tun, WOA hängt sich an den laufenden an |
| Notfall-Playlist, obwohl WOA läuft | kein Ton im Sendemix (Stille unter −80 dBFS) oder Feeder getrennt | Wiedergabe prüfen; Status `ui_connected` |
| keine Hörerzahl, kein Titel | `admin_port` leer (Vorgabe); Titel nur bei lokalem Server | gewollt (Kennwort sonst in der URL) |

Logs: `<datenordner>/logs/streamd/`. Status: `<datenordner>/streamd-status.json`.
