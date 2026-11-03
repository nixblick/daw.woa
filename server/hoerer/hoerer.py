#!/usr/bin/env python3
"""Hörer-Sammler für WOA: liest auf dem Sendeserver alle 30 s die öffentliche
Shoutcast-Kurzstatistik (/7.html, ohne Kennwort), legt sie in SQLite ab und
liefert unter /api/hoerer aggregierte Zahlen als JSON. Keine IPs, keine
User-Agents, nur Zählerstände. Nur Standardbibliothek (Python 3.9+).

Umgebung (alle mit Vorgabe):
  HOERER_QUELLE  Basis-URL des Shoutcast-Servers   http://127.0.0.1:8000
  HOERER_SID     Stream-ID                         1
  HOERER_DB      SQLite-Datei                      ./hoerer.sqlite
  HOERER_PORT    Port des JSON-Endpunkts           8082
  HOERER_TAKT    Sekunden zwischen zwei Abfragen   30
  HOERER_BIND    Adresse des Endpunkts             127.0.0.1 (hinter dem Reverse-Proxy)

Titel (POST /api/titel, nur wenn beide Dateien lesbar sind, sonst 503):
  HOERER_TITEL_QUELL_KW   Datei mit dem Shoutcast-Quellkennwort (Anmeldung der App)
  HOERER_TITEL_ADMIN_KW   Datei mit dem Shoutcast-Admin-Kennwort (für admin.cgi, intern)
Jetzt (GET /api/jetzt, für die Hörer-Seite server/live/): Titel aus /currentsong, die
zuletzt gespielten aus /played?type=json (beide im Docker-Netz, brauchen hidestats leer),
Hörer jetzt und Spitze der laufenden Sendung aus SQLite, Status on_air/notfall/aus. Keine IPs,
Titel als reiner Text (Steuerzeichen raus, Entities aufgelöst, gekürzt); die Seite setzt sie
nur per textContent. JETZT_CACHE Sekunden Zwischenspeicher, damit viele Hörer Shoutcast nicht
abfragen. Fällt Shoutcast aus, kommt trotzdem 200 mit leeren Titeln.

Die App schickt `Authorization: Bearer <Quellkennwort>` und `{"titel": "..."}`. Der Dienst
vergleicht konstantzeitig und ruft admin.cgi?mode=updinfo im Docker-Netz auf; das
Admin-Kennwort verlässt den Server nie. Kennwörter stehen in keinem Log und keiner Antwort.
"""

import html
import hmac
import json
import os
import sqlite3
import threading
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, quote, urlparse

# Shoutcast liefert /7.html nur an Browser-Kennungen als Text aus.
USER_AGENT = "Mozilla/5.0 (WOA hoerer)"
# Längere Lücke zwischen zwei Proben mit Quelle = neue Sendung.
LUECKE = 120
# Jünger als das gilt eine Probe als „jetzt“.
FRISCH = 120
# Höchstens so viele Punkte im Verlauf.
VERLAUF_MAX = 360
# Vorgabe-Zeitraum der Sendungsliste.
VORGABE_TAGE = 30
# Höchstens so viele Tage je Abfrage und so viele Sendungen in der Antwort.
MAX_TAGE = 90
MAX_SENDUNGEN = 200
# Titel: höchstens so viele Zeichen, Rumpf höchstens so viele Bytes.
TITEL_MAX = 200
TITEL_RUMPF_MAX = 2048
# /api/jetzt: so lange zwischengespeichert, so viele zuletzt gespielte Titel.
JETZT_CACHE = 10
JETZT_ZULETZT = 10


def zeitraum(q, jetzt):
    """`von`/`bis` aus der Anfrage prüfen. ValueError bei Nicht-Zahlen oder bis < von;
    ein zu langer Zeitraum wird auf MAX_TAGE vor `bis` gekürzt."""
    bis = int(q.get("bis", [jetzt])[0])
    von = int(q.get("von", [bis - VORGABE_TAGE * 86400])[0])
    if bis < von:
        raise ValueError("bis vor von")
    return max(von, bis - MAX_TAGE * 86400), bis


def parse_seven(text):
    """`aktuell,status,spitze,max,eindeutig,bitrate,titel` → dict oder None."""
    body = text.split("<body>", 1)[-1].split("</body>", 1)[0]
    f = [x.strip() for x in body.split(",")]

    def num(i):
        try:
            return int(f[i])
        except (IndexError, ValueError):
            return None

    aktuell = num(0)
    if aktuell is None:
        return None
    return {
        "aktuell": aktuell,
        "quelle": num(1) == 1,
        "spitze": num(2),
        "eindeutig": num(4),
    }


def lese(quelle, sid, timeout=5):
    """Fragt den Server einmal ab; None bei jedem Fehler (still)."""
    url = f"{quelle.rstrip('/')}/7.html?sid={int(sid)}"
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            if r.status != 200:
                return None
            return parse_seven(r.read(4096).decode("utf-8", "replace"))
    except Exception:  # noqa: BLE001 - Netz, HTTP, Zeitüberschreitung: alles still
        return None


def lies_kennwort(pfad):
    """Erste Zeile der Datei, ohne Zeilenende; None, wenn leer oder nicht lesbar."""
    if not pfad:
        return None
    try:
        with open(pfad, encoding="utf-8") as f:
            kw = f.readline().strip()
    except OSError:
        return None
    return kw or None


def titel_sauber(roh):
    """Steuerzeichen raus, getrimmt, auf TITEL_MAX gekürzt; None, wenn nichts bleibt."""
    if not isinstance(roh, str):
        return None
    t = "".join(c for c in roh if c.isprintable()).strip()[:TITEL_MAX]
    return t or None


def kennwort_passt(kopf, erwartet):
    """`Bearer <kw>` konstantzeitig gegen das erwartete Kennwort."""
    if not kopf or not erwartet or not kopf.startswith("Bearer "):
        return False
    return hmac.compare_digest(kopf[7:].encode(), erwartet.encode())


def setze_titel(quelle, sid, admin_kw, titel, timeout=5):
    """admin.cgi?mode=updinfo intern aufrufen. True bei HTTP 200. Die URL enthält das
    Admin-Kennwort und wird deshalb nie geloggt oder weitergegeben."""
    url = (
        f"{quelle.rstrip('/')}/admin.cgi?sid={int(sid)}&pass={quote(admin_kw, safe='')}"
        f"&mode=updinfo&song={quote(titel, safe='')}"
    )
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status == 200
    except Exception:  # noqa: BLE001 - die Ausnahme könnte die URL tragen: still
        return False


def hole_text(url, timeout=3, limit=65536):
    """GET mit Browser-Kennung (sonst liefert der DNAS den Stream); None bei jedem Fehler."""
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            if r.status != 200:
                return None
            return r.read(limit).decode("utf-8", "replace")
    except Exception:  # noqa: BLE001 - Netz, HTTP, Zeitüberschreitung: alles still
        return None


def titel_text(roh):
    """Shoutcast-Titel als reiner Text: Entities auflösen, dann titel_sauber."""
    if not isinstance(roh, str):
        return None
    return titel_sauber(html.unescape(roh))


def parse_played(text, n=JETZT_ZULETZT):
    """/played?type=json → [{"t": unix|None, "titel": str}], neueste zuerst; Unbrauchbares fällt weg."""
    try:
        daten = json.loads(text)
    except (TypeError, ValueError):
        return []
    if isinstance(daten, dict):  # manche DNAS-Fassungen packen die Liste ein
        daten = daten.get("played") or daten.get("history") or []
    if not isinstance(daten, list):
        return []
    aus = []
    for e in daten:
        if not isinstance(e, dict):
            continue
        t = titel_text(e.get("title"))
        if t is None:
            continue
        try:
            zeit = int(e.get("playedat"))
        except (TypeError, ValueError):
            zeit = None
        aus.append({"t": zeit, "titel": t})
        if len(aus) >= n:
            break
    return aus


def titel_daten(quelle, sid):
    """Aktueller Titel und Verlauf direkt vom DNAS (ohne Kennwort)."""
    basis = quelle.rstrip("/")
    jetzt = hole_text(f"{basis}/currentsong?sid={int(sid)}", limit=4096)
    played = hole_text(f"{basis}/played?sid={int(sid)}&type=json")
    titel = titel_text(jetzt) if jetzt is not None else None
    zuletzt = parse_played(played) if played is not None else []
    # /played enthält meist den laufenden Titel als ersten Eintrag: nicht doppelt zeigen.
    if titel and zuletzt and zuletzt[0]["titel"] == titel:
        zuletzt = zuletzt[1:]
    return titel, zuletzt


def jetzt_antwort(db, quelle, sid, jetzt):
    """Alles, was die Hörer-Seite braucht, in einer Antwort."""
    a = auswertung(db, jetzt - 86400, jetzt, jetzt)
    j = a["jetzt"]
    laufend = a["sendungen"][0] if a["sendungen"] and a["sendungen"][0]["laeuft"] else None
    if j is None:
        status = "aus"
    elif j["quelle"]:
        status = "on_air"
    else:
        status = "notfall"
    titel, zuletzt = titel_daten(quelle, sid) if quelle and status != "aus" else (None, [])
    return {
        "t": jetzt,
        "status": status,
        "titel": titel,
        "hoerer": j["aktuell"] if j else None,
        "spitze": laufend["spitze"] if laufend else (j["spitze_server"] if j else None),
        "sendung_seit": laufend["start"] if laufend else None,
        "zuletzt": zuletzt,
    }


class JetztCache:
    """Hält eine Antwort JETZT_CACHE Sekunden; nur ein Thread fragt Shoutcast gleichzeitig."""

    def __init__(self, dauer=JETZT_CACHE):
        self.dauer = dauer
        self.lock = threading.Lock()
        self.wert = None
        self.zeit = 0.0

    def hole(self, bauen, jetzt):
        with self.lock:
            if self.wert is None or jetzt - self.zeit >= self.dauer:
                self.wert = bauen()
                self.zeit = jetzt
            return self.wert


def oeffne(pfad):
    db = sqlite3.connect(pfad, check_same_thread=False)
    db.execute("PRAGMA journal_mode=WAL")
    db.execute(
        "CREATE TABLE IF NOT EXISTS proben ("
        "t INTEGER PRIMARY KEY, aktuell INTEGER NOT NULL, spitze INTEGER, "
        "eindeutig INTEGER, quelle INTEGER NOT NULL)"
    )
    db.commit()
    return db


def speichere(db, t, probe):
    db.execute(
        "INSERT OR REPLACE INTO proben VALUES (?,?,?,?,?)",
        (int(t), probe["aktuell"], probe["spitze"], probe["eindeutig"], int(probe["quelle"])),
    )
    db.commit()


def sendungen(zeilen, jetzt):
    """Fasst Proben mit Quelle zu Sendungen zusammen (Lücke > LUECKE trennt).
    `zeilen`: (t, aktuell, quelle) aufsteigend. Neueste Sendung zuerst."""
    aus, lauf = [], None
    for t, aktuell, quelle in zeilen:
        if not quelle:
            continue
        if lauf and t - lauf["ende"] <= LUECKE:
            lauf["ende"] = t
            lauf["werte"].append((t, aktuell))
        else:
            lauf = {"start": t, "ende": t, "werte": [(t, aktuell)]}
            aus.append(lauf)
    ergebnis = []
    for s in aus:
        werte = [a for _, a in s["werte"]]
        ergebnis.append(
            {
                "start": s["start"],
                "ende": s["ende"],
                "dauer_s": s["ende"] - s["start"],
                "spitze": max(werte),
                "durchschnitt": round(sum(werte) / len(werte), 1),
                "laeuft": jetzt - s["ende"] <= FRISCH,
                "_werte": s["werte"],
            }
        )
    ergebnis.reverse()
    return ergebnis


def ausduennen(werte, n=VERLAUF_MAX):
    if len(werte) <= n:
        return [list(w) for w in werte]
    schritt = len(werte) / n
    return [list(werte[int(i * schritt)]) for i in range(n)]


def auswertung(db, von, bis, jetzt):
    zeilen = db.execute(
        "SELECT t, aktuell, quelle FROM proben WHERE t BETWEEN ? AND ? ORDER BY t",
        (von, bis),
    ).fetchall()
    letzte = db.execute(
        "SELECT t, aktuell, spitze, eindeutig, quelle FROM proben ORDER BY t DESC LIMIT 1"
    ).fetchone()
    jetzt_d = None
    if letzte and jetzt - letzte[0] <= FRISCH:
        jetzt_d = {
            "t": letzte[0],
            "aktuell": letzte[1],
            "spitze_server": letzte[2],
            "eindeutig": letzte[3],
            "quelle": bool(letzte[4]),
        }
    liste = sendungen(zeilen, jetzt)[:MAX_SENDUNGEN]
    verlauf = ausduennen(liste[0]["_werte"]) if liste else []
    for s in liste:
        del s["_werte"]
    return {"jetzt": jetzt_d, "verlauf": verlauf, "sendungen": liste}


def sammler(db, quelle, sid, takt, stop):
    """Schreibt mit eigener Verbindung; Leser blockieren ihn nicht (WAL)."""
    while not stop.is_set():
        probe = lese(quelle, sid)
        if probe is not None:
            speichere(db, time.time(), probe)
        stop.wait(takt)


def handler_fuer(db_pfad, titel=None):
    """Jede Anfrage liest über eine eigene, nur lesende Verbindung. `titel`: dict mit
    quelle, sid, quell_kw, admin_kw (Dateipfade) für POST /api/titel und GET /api/jetzt
    (dort nur quelle und sid), sonst None."""
    cache = JetztCache()

    class H(BaseHTTPRequestHandler):
        def log_message(self, *a):  # keine Zugriffszeilen mit IPs
            pass

        def _json(self, code, obj):
            body = json.dumps(obj, ensure_ascii=False).encode()
            self.send_response(code)
            self.send_header("Content-Type", "application/json; charset=utf-8")
            self.send_header("Cache-Control", "no-store")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            u = urlparse(self.path)
            if u.path == "/api/hoerer/healthz":
                return self._json(200, {"ok": True})
            if u.path == "/api/jetzt":
                return self._jetzt()
            if u.path != "/api/hoerer":
                return self._json(404, {"fehler": "unbekannt"})
            q = parse_qs(u.query)
            jetzt = int(time.time())
            try:
                von, bis = zeitraum(q, jetzt)
            except ValueError:
                return self._json(400, {"fehler": "von/bis: Unix-Sekunden, bis nicht vor von"})
            db = sqlite3.connect(f"file:{db_pfad}?mode=ro", uri=True)
            try:
                return self._json(200, auswertung(db, von, bis, jetzt))
            finally:
                db.close()

        def _jetzt(self):
            quelle = (titel or {}).get("quelle")
            sid = (titel or {}).get("sid", 1)

            def bauen():
                jetzt = int(time.time())
                db = sqlite3.connect(f"file:{db_pfad}?mode=ro", uri=True)
                try:
                    return jetzt_antwort(db, quelle, sid, jetzt)
                finally:
                    db.close()

            body = json.dumps(cache.hole(bauen, time.time()), ensure_ascii=False).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json; charset=utf-8")
            self.send_header("Cache-Control", f"public, max-age={JETZT_CACHE}")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self):
            if urlparse(self.path).path != "/api/titel":
                return self._json(404, {"fehler": "unbekannt"})
            quell_kw = lies_kennwort(titel and titel.get("quell_kw"))
            admin_kw = lies_kennwort(titel and titel.get("admin_kw"))
            if not quell_kw or not admin_kw:
                return self._json(503, {"fehler": "Titel hier nicht eingerichtet"})
            if not kennwort_passt(self.headers.get("Authorization"), quell_kw):
                return self._json(401, {"fehler": "nicht berechtigt"})
            try:
                laenge = int(self.headers.get("Content-Length", "0"))
            except ValueError:
                laenge = -1
            if laenge < 0 or laenge > TITEL_RUMPF_MAX:
                return self._json(413, {"fehler": "zu groß"})
            try:
                t = titel_sauber(json.loads(self.rfile.read(laenge) or b"null").get("titel"))
            except (ValueError, AttributeError):
                t = None
            if t is None:
                return self._json(400, {"fehler": "titel fehlt"})
            if not setze_titel(titel["quelle"], titel["sid"], admin_kw, t):
                return self._json(502, {"fehler": "Shoutcast nahm den Titel nicht an"})
            self.send_response(204)
            self.send_header("Cache-Control", "no-store")
            self.end_headers()

    return H


def main():
    quelle = os.environ.get("HOERER_QUELLE", "http://127.0.0.1:8000")
    sid = int(os.environ.get("HOERER_SID", "1"))
    pfad = os.environ.get("HOERER_DB", "hoerer.sqlite")
    db = oeffne(pfad)
    bind = os.environ.get("HOERER_BIND", "127.0.0.1")
    port = int(os.environ.get("HOERER_PORT", "8082"))
    takt = float(os.environ.get("HOERER_TAKT", "30"))
    stop = threading.Event()
    threading.Thread(target=sammler, args=(db, quelle, sid, takt, stop), daemon=True).start()
    titel = {
        "quelle": quelle,
        "sid": sid,
        "quell_kw": os.environ.get("HOERER_TITEL_QUELL_KW", ""),
        "admin_kw": os.environ.get("HOERER_TITEL_ADMIN_KW", ""),
    }
    ThreadingHTTPServer((bind, port), handler_fuer(pfad, titel)).serve_forever()


if __name__ == "__main__":
    main()
