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
"""

import json
import os
import sqlite3
import threading
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

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


def oeffne(pfad):
    db = sqlite3.connect(pfad, check_same_thread=False)
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
    liste = sendungen(zeilen, jetzt)
    verlauf = ausduennen(liste[0]["_werte"]) if liste else []
    for s in liste:
        del s["_werte"]
    return {"jetzt": jetzt_d, "verlauf": verlauf, "sendungen": liste}


def sammler(db, quelle, sid, takt, stop, lock):
    while not stop.is_set():
        probe = lese(quelle, sid)
        if probe is not None:
            with lock:
                speichere(db, time.time(), probe)
        stop.wait(takt)


def handler_fuer(db, lock):
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
            if u.path != "/api/hoerer":
                return self._json(404, {"fehler": "unbekannt"})
            q = parse_qs(u.query)
            jetzt = int(time.time())
            try:
                bis = int(q.get("bis", [jetzt])[0])
                von = int(q.get("von", [bis - VORGABE_TAGE * 86400])[0])
            except ValueError:
                return self._json(400, {"fehler": "von/bis: Unix-Sekunden"})
            with lock:
                return self._json(200, auswertung(db, von, bis, jetzt))

    return H


def main():
    quelle = os.environ.get("HOERER_QUELLE", "http://127.0.0.1:8000")
    sid = int(os.environ.get("HOERER_SID", "1"))
    db = oeffne(os.environ.get("HOERER_DB", "hoerer.sqlite"))
    port = int(os.environ.get("HOERER_PORT", "8082"))
    takt = float(os.environ.get("HOERER_TAKT", "30"))
    stop, lock = threading.Event(), threading.Lock()
    threading.Thread(
        target=sammler, args=(db, quelle, sid, takt, stop, lock), daemon=True
    ).start()
    ThreadingHTTPServer(("0.0.0.0", port), handler_fuer(db, lock)).serve_forever()


if __name__ == "__main__":
    main()
