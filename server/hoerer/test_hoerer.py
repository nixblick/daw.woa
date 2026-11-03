"""Tests für den Hörer-Sammler, nur mit Attrappen (kein echter Server).
Aufruf: python3 -m unittest discover -s server/hoerer"""

import json
import threading
import unittest
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import hoerer


class FakeShoutcast(BaseHTTPRequestHandler):
    antwort = "<html><body>12,1,18,100,9,128,Titel, mit Komma</body></html>"
    agent = None

    def log_message(self, *a):
        pass

    def do_GET(self):
        FakeShoutcast.agent = self.headers.get("User-Agent")
        body = FakeShoutcast.antwort.encode()
        self.send_response(200 if self.path.startswith("/7.html?sid=1") else 404)
        self.end_headers()
        self.wfile.write(body)


def starte(handler):
    srv = ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv, f"http://127.0.0.1:{srv.server_address[1]}"


class Tests(unittest.TestCase):
    def test_parse_seven(self):
        p = hoerer.parse_seven("<body>3,1,5,100,4,128,X</body>")
        self.assertEqual(p, {"aktuell": 3, "quelle": True, "spitze": 5, "eindeutig": 4})
        self.assertFalse(hoerer.parse_seven("<body>0,0,5</body>")["quelle"])
        self.assertIsNone(hoerer.parse_seven("<body>kaputt</body>"))

    def test_lese_attrappe_und_fehler_still(self):
        srv, url = starte(FakeShoutcast)
        try:
            p = hoerer.lese(url, 1)
            self.assertEqual((p["aktuell"], p["spitze"], p["eindeutig"]), (12, 18, 9))
            self.assertIn("Mozilla", FakeShoutcast.agent)
            self.assertIsNone(hoerer.lese(url, 2))  # 404
        finally:
            srv.shutdown()
        self.assertIsNone(hoerer.lese("http://127.0.0.1:9", 1, timeout=1))

    def test_sendungen_trennen_an_luecke_und_ohne_quelle(self):
        z = [(0, 2, 1), (30, 4, 1), (60, 6, 1), (90, 9, 0), (1000, 1, 1), (1030, 3, 1)]
        s = hoerer.sendungen(z, jetzt=1040)
        self.assertEqual(len(s), 2)
        self.assertEqual((s[0]["start"], s[0]["spitze"], s[0]["durchschnitt"]), (1000, 3, 2.0))
        self.assertTrue(s[0]["laeuft"])
        self.assertEqual((s[1]["dauer_s"], s[1]["spitze"], s[1]["durchschnitt"]), (60, 6, 4.0))
        self.assertFalse(s[1]["laeuft"])

    def test_ausduennen(self):
        w = [(i, i) for i in range(1000)]
        self.assertEqual(len(hoerer.ausduennen(w, 100)), 100)
        self.assertEqual(hoerer.ausduennen(w[:3]), [[0, 0], [1, 1], [2, 2]])

    def test_endpunkt_liefert_nur_zahlen(self):
        db = hoerer.oeffne(":memory:")
        lock = threading.Lock()
        import time

        jetzt = int(time.time())
        for i, n in enumerate([1, 5, 3]):
            hoerer.speichere(db, jetzt - 60 + i * 30,
                             {"aktuell": n, "quelle": True, "spitze": 7, "eindeutig": 4})
        srv, url = starte(hoerer.handler_fuer(db, lock))
        try:
            with urllib.request.urlopen(url + "/api/hoerer") as r:
                d = json.load(r)
            self.assertEqual(d["jetzt"]["aktuell"], 3)
            self.assertEqual(d["jetzt"]["spitze_server"], 7)
            self.assertEqual(d["sendungen"][0]["spitze"], 5)
            self.assertEqual(d["sendungen"][0]["durchschnitt"], 3.0)
            self.assertEqual([p[1] for p in d["verlauf"]], [1, 5, 3])
            text = json.dumps(d)
            self.assertNotIn("127.0.0.1", text)
            with urllib.request.urlopen(url + f"/api/hoerer?von={jetzt + 10}") as r:
                self.assertEqual(json.load(r)["sendungen"], [])
        finally:
            srv.shutdown()


if __name__ == "__main__":
    unittest.main()
