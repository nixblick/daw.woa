// WOA Hörer-Seite: fragt /api/jetzt alle 15 s ab und spielt /stream erst auf Klick.
// Alle Texte aus dem Netz gehen nur über textContent ins Dokument. Keine Cookies, kein Speicher.
"use strict";
(function () {
  var STREAM = "/stream";
  var TAKT = 15000;
  var $ = function (id) { return document.getElementById(id); };
  var audio = $("audio"), play = $("play"), playText = $("play-text"), hinweis = $("hinweis");
  var STATUS = { on_air: "ON AIR", notfall: "Notfall-Schleife", aus: "Sendepause" };

  function uhr(t) {
    return new Date(t * 1000).toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit" });
  }

  function zeige(d) {
    var st = STATUS[d.status] ? d.status : "aus";
    $("status").className = "status status-" + st;
    $("status-text").textContent = STATUS[st];
    $("titel").textContent = d.titel || (st === "aus" ? "Gerade keine Sendung" : "–");
    $("seit").textContent = d.sendung_seit ? "Sendung seit " + uhr(d.sendung_seit) + " Uhr"
      : (st === "notfall" ? "Die Musik läuft weiter, bis die Sendung wieder beginnt." : "");
    $("hoerer").textContent = d.hoerer == null ? "–" : String(d.hoerer);
    $("spitze").textContent = d.spitze == null ? "–" : String(d.spitze);
    var ol = $("zuletzt");
    ol.textContent = "";
    (d.zuletzt || []).forEach(function (z) {
      var li = document.createElement("li");
      var zeit = document.createElement("time");
      if (z.t) { zeit.dateTime = new Date(z.t * 1000).toISOString(); zeit.textContent = uhr(z.t); }
      var name = document.createElement("span");
      name.textContent = z.titel;
      li.appendChild(zeit); li.appendChild(name); ol.appendChild(li);
    });
    $("zuletzt-leer").hidden = ol.children.length > 0;
  }

  function lade() {
    fetch("/api/jetzt", { cache: "no-store", credentials: "omit" })
      .then(function (r) { if (!r.ok) throw new Error(r.status); return r.json(); })
      .then(zeige)
      .catch(function () { $("status-text").textContent = "Keine Daten"; });
  }

  function naechste() {
    fetch("naechste.json", { cache: "no-cache", credentials: "omit" })
      .then(function (r) { return r.ok ? r.json() : { sendungen: [] }; })
      .then(function (k) {
        var jetzt = Date.now();
        var s = (k.sendungen || []).filter(function (x) {
          var a = Date.parse(x.start);
          return a && a + (x.dauer_min || 60) * 60000 > jetzt && typeof x.titel === "string";
        }).sort(function (a, b) { return Date.parse(a.start) - Date.parse(b.start); })[0];
        if (!s) return;
        var a = new Date(s.start);
        $("naechste").textContent = a.toLocaleDateString("de-DE", { weekday: "long", day: "numeric", month: "long" })
          + ", " + a.toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit" }) + " Uhr: " + s.titel;
        $("naechste-box").hidden = false;
      })
      .catch(function () {});
  }

  function stopp() {
    audio.pause();
    audio.removeAttribute("src");
    audio.load(); // Verbindung zum Server wirklich schließen
    play.setAttribute("aria-pressed", "false");
    playText.textContent = "Hören";
  }

  function start() {
    audio.src = STREAM + "?t=" + Date.now(); // immer live, nie ein alter Puffer
    play.setAttribute("aria-pressed", "true");
    playText.textContent = "Stopp";
    hinweis.textContent = "Verbinde mit dem Stream …";
    var p = audio.play();
    if (p && p.catch) {
      p.catch(function () {
        stopp();
        hinweis.textContent = "Der Browser hat das Abspielen verhindert. Bitte noch einmal tippen.";
      });
    }
  }

  play.addEventListener("click", function () {
    if (play.getAttribute("aria-pressed") === "true") { stopp(); hinweis.textContent = ""; } else { start(); }
  });
  audio.addEventListener("playing", function () { hinweis.textContent = "Läuft."; });
  audio.addEventListener("error", function () {
    if (!audio.getAttribute("src")) return;
    stopp();
    hinweis.textContent = "Der Stream ist gerade nicht erreichbar. Bitte gleich noch einmal versuchen.";
  });
  $("laut").addEventListener("input", function (e) { audio.volume = Number(e.target.value); });

  lade();
  naechste();
  setInterval(lade, TAKT);
})();
