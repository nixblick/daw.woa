"""Tests für den Hörer-Sammler, nur mit Attrappen (kein echter Server).
Aufruf: python3 -m unittest discover -s server/hoerer"""

import json
import threading
import unittest
import urllib.error
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

    def test_zeitraum_geprueft_und_begrenzt(self):
        self.assertEqual(hoerer.zeitraum({}, 1000000), (1000000 - 30 * 86400, 1000000))
        von, bis = hoerer.zeitraum({"von": ["0"], "bis": ["100000000"]}, 0)
        self.assertEqual(bis - von, hoerer.MAX_TAGE * 86400)
        with self.assertRaises(ValueError):
            hoerer.zeitraum({"von": ["10"], "bis": ["5"]}, 0)
        with self.assertRaises(ValueError):
            hoerer.zeitraum({"von": ["x"]}, 0)

    def test_sendungen_begrenzt(self):
        db = hoerer.oeffne(":memory:")
        for i in range(hoerer.MAX_SENDUNGEN + 20):
            hoerer.speichere(db, i * 1000, {"aktuell": 1, "quelle": True, "spitze": 1, "eindeutig": 1})
        d = hoerer.auswertung(db, 0, 10**9, 10**9)
        self.assertEqual(len(d["sendungen"]), hoerer.MAX_SENDUNGEN)

    def test_endpunkt_liefert_nur_zahlen(self):
        import tempfile
        import time

        tmp = tempfile.TemporaryDirectory()
        pfad = tmp.name + "/h.sqlite"
        db = hoerer.oeffne(pfad)
        jetzt = int(time.time())
        for i, n in enumerate([1, 5, 3]):
            hoerer.speichere(db, jetzt - 60 + i * 30,
                             {"aktuell": n, "quelle": True, "spitze": 7, "eindeutig": 4})
        srv, url = starte(hoerer.handler_fuer(pfad))
        try:
            with urllib.request.urlopen(url + "/api/hoerer") as r:
                d = json.load(r)
            self.assertEqual(d["jetzt"]["aktuell"], 3)
            self.assertEqual(d["jetzt"]["spitze_server"], 7)
            self.assertEqual(d["sendungen"][0]["spitze"], 5)
            self.assertEqual(d["sendungen"][0]["durchschnitt"], 3.0)
            self.assertEqual([p[1] for p in d["verlauf"]], [1, 5, 3])
            self.assertNotIn("127.0.0.1", json.dumps(d))
            with urllib.request.urlopen(url + f"/api/hoerer?von={jetzt + 10}&bis={jetzt + 20}") as r:
                self.assertEqual(json.load(r)["sendungen"], [])
            for bad in ["?von=abc", f"?von={jetzt}&bis={jetzt - 1}"]:
                with self.assertRaises(urllib.error.HTTPError) as e:
                    urllib.request.urlopen(url + "/api/hoerer" + bad)
                self.assertEqual(e.exception.code, 400)
        finally:
            srv.shutdown()
            srv.server_close()
            db.close()
            tmp.cleanup()

class FakeAdmin(BaseHTTPRequestHandler):
    pfade = []
    erwartet = "adm&in"

    def log_message(self, *a):
        pass

    def do_GET(self):
        from urllib.parse import parse_qs, urlparse

        FakeAdmin.pfade.append(self.path)
        u = urlparse(self.path)
        q = parse_qs(u.query)
        ok = (
            u.path == "/admin.cgi"
            and q.get("sid") == ["1"]
            and q.get("mode") == ["updinfo"]
            and q.get("pass") == [FakeAdmin.erwartet]
        )
        self.send_response(200 if ok else 403)
        self.end_headers()


def lied(pfad):
    from urllib.parse import parse_qs, urlparse

    return parse_qs(urlparse(pfad).query)["song"][0]


class TitelTests(unittest.TestCase):
    def setUp(self):
        import tempfile

        self.tmp = tempfile.TemporaryDirectory()
        d = self.tmp.name
        for name, wert in [("q", "quelle-kw\n"), ("a", FakeAdmin.erwartet + "\n")]:
            with open(f"{d}/{name}", "w") as f:
                f.write(wert)
        self.admin, admin_url = starte(FakeAdmin)
        FakeAdmin.pfade = []
        self.titel = {"quelle": admin_url, "sid": 1, "quell_kw": f"{d}/q", "admin_kw": f"{d}/a"}
        hoerer.oeffne(f"{d}/h.sqlite").close()
        self.srv, self.url = starte(hoerer.handler_fuer(f"{d}/h.sqlite", self.titel))

    def tearDown(self):
        self.srv.shutdown()
        self.admin.shutdown()
        self.tmp.cleanup()

    def post(self, body, kw="quelle-kw", url=None):
        daten = body if isinstance(body, bytes) else json.dumps(body).encode()
        req = urllib.request.Request(url or self.url + "/api/titel", data=daten, method="POST")
        if kw is not None:
            req.add_header("Authorization", f"Bearer {kw}")
        try:
            with urllib.request.urlopen(req) as r:
                return r.status, r.read()
        except urllib.error.HTTPError as e:
            return e.code, e.read()

    def test_titel_geht_mit_admin_kennwort_an_shoutcast(self):
        code, _ = self.post({"titel": "Probe – Äh & Co\r\n"})
        self.assertEqual(code, 204)
        self.assertEqual(len(FakeAdmin.pfade), 1)
        self.assertEqual(lied(FakeAdmin.pfade[0]), "Probe – Äh & Co")

    def test_falsches_oder_fehlendes_kennwort(self):
        for kw in ["falsch", None, FakeAdmin.erwartet, ""]:
            code, body = self.post({"titel": "x"}, kw=kw)
            self.assertEqual(code, 401, kw)
            self.assertNotIn(b"quelle-kw", body)
            self.assertNotIn(b"adm", body)
        self.assertEqual(FakeAdmin.pfade, [])

    def test_titel_begrenzt_und_geprueft(self):
        self.assertEqual(self.post({"titel": "y" * 500})[0], 204)
        self.assertEqual(lied(FakeAdmin.pfade[0]), "y" * hoerer.TITEL_MAX)
        self.assertEqual(self.post({"titel": " \x00 "})[0], 400)
        self.assertEqual(self.post({"anders": 1})[0], 400)
        self.assertEqual(self.post(b"kein json")[0], 400)
        self.assertEqual(self.post(b"[1]")[0], 400)
        self.assertEqual(self.post(b"x" * (hoerer.TITEL_RUMPF_MAX + 1))[0], 413)
        self.assertEqual(self.post({"titel": "x"}, url=self.url + "/api/hoerer")[0], 404)

    def test_ohne_kennwortdateien_aus(self):
        self.titel["admin_kw"] = self.tmp.name + "/fehlt"
        self.assertEqual(self.post({"titel": "x"})[0], 503)
        self.assertEqual(FakeAdmin.pfade, [])

    def test_shoutcast_lehnt_ab(self):
        with open(self.titel["admin_kw"], "w") as f:
            f.write("anders")
        self.assertEqual(self.post({"titel": "x"})[0], 502)


class FakeDnas(BaseHTTPRequestHandler):
    """Attrappe für /currentsong und /played; antwortet nur Browser-Kennungen mit Seiten."""
    aktuell = "Band &amp; Co \u2013 Lied\x07"
    played = json.dumps([
        {"playedat": 300, "title": "Band &amp; Co \u2013 Lied"},
        {"playedat": 200, "title": "<script>alert(1)</script>"},
        {"playedat": "kaputt", "title": "Ohne Zeit"},
        "Unsinn",
        {"playedat": 100},
    ])
    code = 200
    aufrufe = 0

    def log_message(self, *a):
        pass

    def do_GET(self):
        FakeDnas.aufrufe += 1
        if "Mozilla" not in (self.headers.get("User-Agent") or ""):
            body, code = b"\xff\xfb audio", 200
        elif self.path.startswith("/currentsong?sid=1"):
            body, code = FakeDnas.aktuell.encode(), FakeDnas.code
        elif self.path.startswith("/played?sid=1&type=json"):
            body, code = FakeDnas.played.encode(), FakeDnas.code
        else:
            body, code = b"", 404
        self.send_response(code)
        self.end_headers()
        self.wfile.write(body)


class JetztTests(unittest.TestCase):
    def setUp(self):
        import tempfile
        import time

        FakeDnas.code, FakeDnas.aufrufe = 200, 0
        FakeDnas.played = JetztTests.played_vorgabe
        self.tmp = tempfile.TemporaryDirectory()
        self.pfad = self.tmp.name + "/h.sqlite"
        self.db = hoerer.oeffne(self.pfad)
        self.jetzt = int(time.time())
        self.dnas, self.dnas_url = starte(FakeDnas)

    played_vorgabe = FakeDnas.played

    def tearDown(self):
        self.dnas.shutdown()
        self.dnas.server_close()
        self.db.close()
        self.tmp.cleanup()

    def proben(self, werte, quelle=True):
        for i, n in enumerate(werte):
            hoerer.speichere(self.db, self.jetzt - 30 * (len(werte) - 1 - i),
                             {"aktuell": n, "quelle": quelle, "spitze": 40, "eindeutig": 4})

    def antwort(self):
        return hoerer.jetzt_antwort(self.db, self.dnas_url, 1, self.jetzt)

    def test_on_air_mit_titel_und_verlauf(self):
        self.proben([2, 9, 4])
        a = self.antwort()
        self.assertEqual(a["status"], "on_air")
        self.assertEqual((a["hoerer"], a["spitze"]), (4, 9))
        self.assertEqual(a["titel"], "Band & Co \u2013 Lied")
        # laufender Titel nicht doppelt, Kaputtes fällt weg, HTML bleibt reiner Text
        self.assertEqual([z["titel"] for z in a["zuletzt"]], ["<script>alert(1)</script>", "Ohne Zeit"])
        self.assertEqual([z["t"] for z in a["zuletzt"]], [200, None])
        self.assertNotIn("127.0.0.1", json.dumps(a))

    def test_notfall_ohne_quelle_und_aus_ohne_probe(self):
        self.assertEqual(self.antwort()["status"], "aus")
        self.assertEqual(FakeDnas.aufrufe, 0)
        self.proben([3], quelle=False)
        a = self.antwort()
        self.assertEqual((a["status"], a["hoerer"], a["spitze"], a["sendung_seit"]), ("notfall", 3, 40, None))

    def test_shoutcast_gesperrt_oder_weg_still(self):
        self.proben([1])
        FakeDnas.code = 403
        a = self.antwort()
        self.assertEqual((a["status"], a["titel"], a["zuletzt"]), ("on_air", None, []))
        a = hoerer.jetzt_antwort(self.db, "http://127.0.0.1:9", 1, self.jetzt)
        self.assertEqual((a["titel"], a["zuletzt"], a["hoerer"]), (None, [], 1))

    def test_played_kaputt_und_lang(self):
        self.assertEqual(hoerer.parse_played("kein json"), [])
        self.assertEqual(hoerer.parse_played('{"x": 1}'), [])
        self.assertEqual(hoerer.parse_played('{"played": [{"title": "A"}]}'), [{"t": None, "titel": "A"}])
        viele = json.dumps([{"playedat": i, "title": "t" * 500} for i in range(50)])
        z = hoerer.parse_played(viele)
        self.assertEqual(len(z), hoerer.JETZT_ZULETZT)
        self.assertEqual(len(z[0]["titel"]), hoerer.TITEL_MAX)

    def test_endpunkt_cache_und_kopf(self):
        self.proben([5])
        srv, url = starte(hoerer.handler_fuer(self.pfad, {"quelle": self.dnas_url, "sid": 1}))
        try:
            for _ in range(3):
                with urllib.request.urlopen(url + "/api/jetzt") as r:
                    d = json.load(r)
                    kopf = r.headers.get("Cache-Control")
            self.assertEqual((d["status"], d["hoerer"]), ("on_air", 5))
            self.assertIn("max-age=", kopf)
            self.assertEqual(FakeDnas.aufrufe, 2)  # einmal currentsong + played, dann Cache
            req = urllib.request.Request(url + "/api/jetzt", data=b"x", method="POST")
            with self.assertRaises(urllib.error.HTTPError) as e:
                urllib.request.urlopen(req)
            self.assertEqual(e.exception.code, 404)
        finally:
            srv.shutdown()
            srv.server_close()

    def test_cors_nur_fuer_wir_on_air(self):
        self.proben([5])
        srv, url = starte(hoerer.handler_fuer(self.pfad, {"quelle": self.dnas_url, "sid": 1}))

        def hole(pfad, herkunft=None, methode="GET"):
            req = urllib.request.Request(url + pfad, method=methode)
            if herkunft:
                req.add_header("Origin", herkunft)
            with urllib.request.urlopen(req) as r:
                return r.status, r.headers

        try:
            for o in sorted(hoerer.CORS_URSPRUENGE):
                for p in ("/api/jetzt", "/api/hoerer"):
                    _, k = hole(p, o)
                    self.assertEqual(k.get("Access-Control-Allow-Origin"), o)
                    self.assertEqual(k.get("Vary"), "Origin")
                    self.assertIsNone(k.get("Access-Control-Allow-Credentials"))
            for o in (None, "https://evil.example", "http://www.wir-on-air.de",
                      "https://www.wir-on-air.de.evil.example", "null"):
                _, k = hole("/api/jetzt", o)
                self.assertIsNone(k.get("Access-Control-Allow-Origin"))
                self.assertEqual(k.get("Vary"), "Origin")
            _, k = hole("/api/hoerer/healthz", "https://wironair.de")
            self.assertIsNone(k.get("Access-Control-Allow-Origin"))
            code, k = hole("/api/jetzt", "https://wironair.de", "OPTIONS")
            self.assertEqual(code, 204)
            self.assertEqual(k.get("Access-Control-Allow-Methods"), "GET")
            code, k = hole("/api/jetzt", "https://evil.example", "OPTIONS")
            self.assertIsNone(k.get("Access-Control-Allow-Origin"))
            self.assertIsNone(k.get("Access-Control-Allow-Methods"))
            with self.assertRaises(urllib.error.HTTPError) as e:
                hole("/api/titel", "https://wironair.de", "OPTIONS")
            self.assertEqual(e.exception.code, 404)
        finally:
            srv.shutdown()
            srv.server_close()


if __name__ == "__main__":
    unittest.main()
