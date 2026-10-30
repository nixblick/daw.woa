/* WOA – Wir on Air · Referenz-Verhalten der Hardware-Komponenten (vanilla, kein Framework).
   window.WOA: icon, mark, theme, meter, knob, fader, clock, wave, vu, bars, levels, init. */
(function () {
  'use strict';
  var P = {
    play: '<path d="M7 4.5v15l12-7.5z" class="f"/>',
    pause: '<path d="M7 4h3.5v16H7zM13.5 4H17v16h-3.5z" class="f"/>',
    stop: '<rect x="5.5" y="5.5" width="13" height="13" rx="1.5" class="f"/>',
    rec: '<circle cx="12" cy="12" r="7" class="f"/>',
    prev: '<path d="M6 5v14M19 5L9 12l10 7z"/>',
    next: '<path d="M18 5v14M5 5l10 7-10 7z"/>',
    loop: '<path d="M4 11V9a3 3 0 0 1 3-3h12l-3-3M20 13v2a3 3 0 0 1-3 3H5l3 3"/>',
    mic: '<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21M8.5 21h7"/>',
    phones: '<path d="M4 15v-3a8 8 0 0 1 16 0v3"/><rect x="3" y="14" width="4.5" height="7" rx="1.5"/><rect x="16.5" y="14" width="4.5" height="7" rx="1.5"/>',
    antenna: '<path d="M12 10v11M8 21h8"/><circle cx="12" cy="8" r="2"/><path d="M7.8 3.8a6 6 0 0 0 0 8.4M16.2 3.8a6 6 0 0 1 0 8.4M5 1.5a10 10 0 0 0 0 13M19 1.5a10 10 0 0 1 0 13"/>',
    sliders: '<path d="M6 3v18M12 3v18M18 3v18"/><rect x="3.5" y="13" width="5" height="4" rx="1" class="f"/><rect x="9.5" y="6" width="5" height="4" rx="1" class="f"/><rect x="15.5" y="10" width="5" height="4" rx="1" class="f"/>',
    disc: '<circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="2.5"/><path d="M12 5.5a6.5 6.5 0 0 1 6.5 6.5"/>',
    heart: '<path d="M12 20s-7.5-4.6-7.5-10A4.3 4.3 0 0 1 12 7.4 4.3 4.3 0 0 1 19.5 10C19.5 15.4 12 20 12 20z"/>',
    plus: '<path d="M12 5v14M5 12h14"/>',
    search: '<circle cx="11" cy="11" r="6.5"/><path d="M20 20l-4.3-4.3"/>',
    x: '<path d="M6 6l12 12M18 6L6 18"/>',
    up: '<path d="M6 15l6-6 6 6"/>',
    down: '<path d="M6 9l6 6 6-6"/>',
    cut: '<circle cx="6" cy="18" r="3"/><circle cx="18" cy="18" r="3"/><path d="M8 16L19 4M16 16L5 4"/>',
    note: '<path d="M9 18V5l11-2v13"/><circle cx="6.5" cy="18" r="2.5"/><circle cx="17.5" cy="16" r="2.5"/>',
    feedback: '<path d="M4 5h16v11H9l-5 4z"/><path d="M8 9.5h8M8 12.5h5"/>',
    fade: '<path d="M3 19L21 5M3 5h4M17 19h4"/>',
    wifi: '<path d="M2.5 9a14 14 0 0 1 19 0M5.5 12.5a9.5 9.5 0 0 1 13 0M8.7 16a5 5 0 0 1 6.6 0"/><circle cx="12" cy="19.5" r="1" class="f"/>',
    bolt: '<path d="M13 2L4 14h7l-1 8 9-12h-7z"/>'
  };
  function icon(name, cls) {
    var p = (P[name] || '').replace(/class="f"/g, 'fill="currentColor" stroke="none"');
    return '<svg class="woa-i ' + (cls || '') + '" viewBox="0 0 24 24" aria-hidden="true">' + p + '</svg>';
  }
  /* Bildmarke: fünf LED-Säulen in W-Form + ON-AIR-Lampe. Säulen in Modusfarbe, Lampe immer on-air. */
  function mark(size) {
    var H = [144, 72, 112, 72, 144], s = '';
    for (var i = 0; i < 5; i++) {
      var x = 44 + i * 36, h = H[i], n = Math.floor((h + 6) / 20), y = 204;
      for (var k = 0; k < n; k++) {
        y -= 14;
        s += '<rect x="' + x + '" y="' + y + '" width="24" height="14" rx="3" style="fill:var(' + (k === n - 1 ? '--accent-strong' : '--accent') + ')"/>';
        y -= 6;
      }
    }
    s += '<circle cx="128" cy="54" r="22" style="fill:var(--on-air)" opacity=".22"/><circle cx="128" cy="54" r="15" style="fill:var(--on-air)"/>';
    return '<svg viewBox="36 28 184 184" width="' + (size || 26) + '" height="' + (size || 26) + '" role="img" aria-label="WOA">' + s + '</svg>';
  }
  /* Modus-Thema auf ein Element: folgt hell/dunkel der Seite. */
  function theme(el, mode) {
    function apply() {
      var t = document.documentElement.getAttribute('data-theme') || '';
      el.setAttribute('data-theme', /-hell$/.test(t) ? mode + '-hell' : mode);
    }
    apply();
    new MutationObserver(apply).observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
  }
  var reduce = window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  var meters = [];
  function zone(f) { return f > 0.95 ? 'r' : f > 0.8 ? 'y' : 'g'; } /* -60..0 dB: >-3 rot, >-12 gelb */
  function meter(el, o) {
    o = o || {};
    var ch = +(el.dataset.ch || o.ch || 2), n = +(el.dataset.segs || o.segs || 30), html = '';
    for (var c = 0; c < ch; c++) {
      html += '<div class="woa-meter__ch">';
      for (var i = 0; i < n; i++) html += '<div class="woa-meter__seg" data-z="' + zone((i + 1) / n) + '"></div>';
      html += '</div>';
    }
    el.innerHTML = html;
    var m = { el: el, n: n, ch: [].slice.call(el.children), lv: [], tgt: +(el.dataset.level || 0.7), peak: [], live: el.dataset.static === undefined };
    m.ch.forEach(function (_, i) { m.lv[i] = m.tgt; m.peak[i] = m.tgt; });
    meters.push(m); paint(m); return m;
  }
  function paint(m) {
    m.ch.forEach(function (c, i) {
      var lit = Math.round(m.lv[i] * m.n), pk = Math.min(m.n - 1, Math.round(m.peak[i] * m.n) - 1);
      for (var k = 0; k < m.n; k++) c.children[k].classList.toggle('on', k < lit || k === pk);
    });
  }
  var t0 = 0;
  function tick(t) {
    if (t - t0 > 55) {
      t0 = t;
      meters.forEach(function (m) {
        if (!m.live) return;
        m.ch.forEach(function (_, i) {
          var target = m.tgt * (0.72 + Math.random() * 0.34);
          m.lv[i] += (target - m.lv[i]) * (target > m.lv[i] ? 0.7 : 0.25);
          m.lv[i] = Math.max(0, Math.min(1, m.lv[i]));
          m.peak[i] = m.lv[i] > m.peak[i] ? m.lv[i] : m.peak[i] - 0.006;
        });
        paint(m);
      });
    }
    requestAnimationFrame(tick);
  }
  if (!reduce) requestAnimationFrame(tick);
  function drag(el, target, horizontal, fmt, out) {
    var v = parseFloat(getComputedStyle(el).getPropertyValue('--v')) || 0;
    function set(nv) { v = Math.max(0, Math.min(1, nv)); el.style.setProperty('--v', v.toFixed(3)); if (out) out.textContent = fmt(v); }
    set(v);
    target.addEventListener('pointerdown', function (e) {
      target.setPointerCapture(e.pointerId);
      var sx = e.clientX, sy = e.clientY, sv = v, len = horizontal ? target.clientWidth : (target.clientHeight || 150);
      function mv(ev) { set(sv + (horizontal ? (ev.clientX - sx) : (sy - ev.clientY)) / len); }
      function up() { target.removeEventListener('pointermove', mv); target.removeEventListener('pointerup', up); }
      target.addEventListener('pointermove', mv); target.addEventListener('pointerup', up);
    });
    target.addEventListener('dblclick', function () { set(+(el.dataset.default || 0.5)); });
    target.tabIndex = 0;
    target.addEventListener('keydown', function (e) {
      var d = { ArrowUp: .02, ArrowRight: .02, ArrowDown: -.02, ArrowLeft: -.02 }[e.key];
      if (d) { e.preventDefault(); set(v + d * (e.shiftKey ? 5 : 1)); }
    });
  }
  var FMT = {
    db: function (v) { var d = v <= 0 ? -Infinity : 20 * Math.log10(Math.pow(v / 0.75, 2.5)); return d === -Infinity ? '−∞' : (d > 0 ? '+' : '') + d.toFixed(1); },
    pan: function (v) { var p = Math.round((v - .5) * 200); return p === 0 ? 'C' : (p < 0 ? 'L' + -p : 'R' + p); },
    eq: function (v) { var d = (v - .5) * 30; return (d > 0 ? '+' : '') + d.toFixed(1); },
    pct: function (v) { return Math.round(v * 100) + '%'; }
  };
  function knob(el) { drag(el, el.querySelector('.woa-knob__dial'), false, FMT[el.dataset.fmt || 'pct'], el.querySelector('.woa-knob__val')); }
  function fader(el) { drag(el, el.querySelector('.woa-fader__track'), el.classList.contains('woa-fader--h'), FMT[el.dataset.fmt || 'db'], document.getElementById(el.dataset.out || '')); }
  function clock(el) {
    var html = '', R = 0.39, Rh = 0.465;
    for (var i = 0; i < 60; i++) {
      var a = i / 60 * 2 * Math.PI;
      html += '<i class="woa-clock__dot" style="transform:translate(' + (Math.sin(a) * R * 100).toFixed(2) + 'cqw,' + (-Math.cos(a) * R * 100).toFixed(2) + 'cqw)"></i>';
    }
    for (var h = 0; h < 12; h++) {
      var b = h / 12 * 2 * Math.PI;
      html += '<i class="woa-clock__dot h" style="transform:translate(' + (Math.sin(b) * Rh * 100).toFixed(2) + 'cqw,' + (-Math.cos(b) * Rh * 100).toFixed(2) + 'cqw)"></i>';
    }
    html += '<div class="woa-clock__mid"><span class="woa-seg woa-clock__time" data-ghost="88:88"><span></span></span><span class="woa-seg woa-clock__sec" data-ghost="88"><span></span></span></div>';
    el.style.containerType = 'inline-size';
    el.innerHTML = html;
    var dots = el.querySelectorAll('.woa-clock__dot:not(.h)'), hd = el.querySelectorAll('.woa-clock__dot.h');
    var tm = el.querySelector('.woa-clock__time span'), sc = el.querySelector('.woa-clock__sec span');
    var fixed = el.dataset.time;
    function up() {
      var d = new Date(), H = d.getHours(), M = d.getMinutes(), S = d.getSeconds();
      if (fixed) { var p = fixed.split(':'); H = +p[0]; M = +p[1]; S = +p[2]; }
      for (var i = 0; i < 60; i++) dots[i].classList.toggle('on', i <= S);
      for (var j = 0; j < 12; j++) hd[j].classList.toggle('on', true);
      tm.textContent = ('0' + H).slice(-2) + ':' + ('0' + M).slice(-2);
      sc.textContent = ('0' + S).slice(-2);
    }
    up(); if (!fixed) setInterval(up, 1000);
  }
  /* Analoges VU-Meter: Zeiger −20…+3 VU, Skala per SVG. */
  var vus = [];
  function vu(el) {
    var ticks = [[-20, '20'], [-10, '10'], [-7, '7'], [-5, '5'], [-3, '3'], [-2, ''], [-1, ''], [0, '0'], [1, ''], [2, ''], [3, '+3']], s = '';
    function ang(db) { var lin = Math.pow(10, db / 20), f = (lin - 0.1) / (1.413 - 0.1); return -48 + f * 96; }
    function pt(a, r) { var t = (a - 90) * Math.PI / 180; return [100 + r * Math.cos(t), 128 + r * Math.sin(t)]; }
    function arc(a0, a1, r) { var p0 = pt(a0, r), p1 = pt(a1, r); return 'M' + p0[0].toFixed(1) + ' ' + p0[1].toFixed(1) + 'A' + r + ' ' + r + ' 0 0 1 ' + p1[0].toFixed(1) + ' ' + p1[1].toFixed(1); }
    s += '<path d="' + arc(-48, ang(0), 92) + '" fill="none" style="stroke:var(--vu-ink)" stroke-width="1.5"/>';
    s += '<path d="' + arc(ang(0), 48, 92) + '" fill="none" style="stroke:var(--on-air)" stroke-width="5"/>';
    ticks.forEach(function (t) {
      var a = ang(t[0]), p0 = pt(a, 92), p1 = pt(a, t[1] ? 102 : 98), col = t[0] > 0 ? 'var(--on-air)' : 'var(--vu-ink)';
      s += '<line x1="' + p0[0].toFixed(1) + '" y1="' + p0[1].toFixed(1) + '" x2="' + p1[0].toFixed(1) + '" y2="' + p1[1].toFixed(1) + '" style="stroke:' + col + '" stroke-width="1.5"/>';
      if (t[1]) { var q = pt(a, 110); s += '<text x="' + q[0].toFixed(1) + '" y="' + (q[1] + 3).toFixed(1) + '" text-anchor="middle" style="fill:' + col + ';font:600 9px var(--font-label)">' + t[1] + '</text>'; }
    });
    s += '<text x="100" y="92" text-anchor="middle" style="fill:var(--vu-ink);font:800 15px var(--font-label);letter-spacing:.1em">VU</text>';
    s += '<text x="100" y="106" text-anchor="middle" style="fill:var(--vu-ink);font:600 7px var(--font-label);letter-spacing:.18em;opacity:.7">' + (el.dataset.label || 'WOA STUDIO') + '</text>';
    s += '<g class="woa-vu__needle"><line x1="100" y1="128" x2="100" y2="30" style="stroke:var(--vu-ink)" stroke-width="1.6"/></g><circle cx="100" cy="128" r="9" style="fill:var(--surface)"/>';
    el.innerHTML = '<svg viewBox="0 0 200 118" aria-hidden="true">' + s + '</svg>';
    var n = el.querySelector('.woa-vu__needle'), v = { n: n, ang: ang, lv: +(el.dataset.level || -4), cur: -20 };
    function set(db) { n.style.transform = 'rotate(' + ang(Math.max(-20, Math.min(3, db))).toFixed(1) + 'deg)'; }
    v.set = set; set(v.lv); vus.push(v); return v;
  }
  setInterval(function () { if (reduce) return; vus.forEach(function (v) { if (v.n.isConnected) v.set(v.lv + (Math.random() - 0.6) * 5); }); }, 140);
  /* Moderne Pegelansicht: −40…+3 dBFS, RMS-Balken + Peak-Strich + Zahl. */
  var bars = [];
  function barsView(el) {
    var chs = (el.dataset.ch || 'L,R').split(','), lv = +(el.dataset.level || -8), html = '';
    function pos(db) { return Math.max(0, Math.min(100, (db + 40) / 43 * 100)); }
    chs.forEach(function (c) { html += '<span class="woa-bars__ch">' + c + '</span><div class="woa-bars__track" style="--zero:' + pos(0).toFixed(2) + '%"><i class="woa-bars__rms"></i><i class="woa-bars__pk"></i></div><span class="woa-bars__val">−∞</span>'; });
    html += '<div class="woa-bars__scale">' + [-40, -30, -20, -12, -6, -3, 0, 3].map(function (d) { return '<span style="left:' + pos(d).toFixed(1) + '%">' + (d > 0 ? '+' : d < 0 ? '−' : '') + Math.abs(d) + '</span>'; }).join('') + '</div>';
    el.innerHTML = html;
    var tr = el.querySelectorAll('.woa-bars__track'), vals = el.querySelectorAll('.woa-bars__val'), st = [];
    chs.forEach(function () { st.push({ rms: lv, pk: lv + 4 }); });
    function draw() {
      st.forEach(function (s, i) {
        tr[i].style.setProperty('--rms', pos(s.rms).toFixed(1) + '%');
        tr[i].style.setProperty('--pk', pos(s.pk).toFixed(1) + '%');
        vals[i].textContent = (s.pk < 0 ? '−' : '+') + Math.abs(s.pk).toFixed(1);
        vals[i].classList.toggle('is-hot', s.pk > -1);
      });
    }
    var b = { el: el, st: st, lv: lv, draw: draw }; bars.push(b); draw(); return b;
  }
  setInterval(function () {
    if (reduce) return;
    bars.forEach(function (b) {
      b.st.forEach(function (s) {
        var t = b.lv + (Math.random() - 0.6) * 6;
        s.rms += (t - s.rms) * 0.4; var p = s.rms + 3 + Math.random() * 3;
        s.pk = p > s.pk ? p : s.pk - 0.6;
      });
      b.draw();
    });
  }, 120);
  /* Umschalter Analog / Modern / Aus für .woa-levels */
  function levels(el) {
    var btns = el.querySelectorAll('.woa-seg3 button');
    function set(v) { el.setAttribute('data-view', v); btns.forEach(function (b) { b.setAttribute('aria-pressed', b.dataset.view === v); }); }
    btns.forEach(function (b) { b.addEventListener('click', function () { set(b.dataset.view); }); });
    set(el.dataset.view || 'analog');
  }
  function rnd(seed) { return function () { seed = (seed * 16807) % 2147483647; return (seed - 1) / 2147483646; }; }
  function wave(cv) {
    var css = getComputedStyle(cv), dpr = window.devicePixelRatio || 1;
    var w = cv.clientWidth || 300, h = cv.clientHeight || 60; cv.width = w * dpr; cv.height = h * dpr;
    var g = cv.getContext('2d'); g.scale(dpr, dpr);
    var r = rnd(+(cv.dataset.seed || 7)), played = +(cv.dataset.played || 0), col = css.getPropertyValue(cv.dataset.color || '--accent').trim();
    var dim = css.getPropertyValue('--lcd-text').trim(), mid = h / 2, step = +(cv.dataset.step || 3), amp = 0.4;
    for (var x = 0; x < w; x += step) {
      amp += (r() - 0.5) * 0.18; amp = Math.max(0.15, Math.min(0.95, amp));
      var a = amp * (0.55 + r() * 0.45) * (h / 2 - 3);
      g.fillStyle = x / w < played ? col : dim; g.globalAlpha = x / w < played ? 1 : 0.38;
      g.fillRect(x, mid - a, Math.max(1, step - 1), a * 2);
    }
    if (played > 0 && played < 1) { g.globalAlpha = 1; g.fillStyle = css.getPropertyValue('--lcd-text').trim(); g.fillRect(Math.round(played * w), 0, 2, h); }
    if (cv.dataset.beats) { g.globalAlpha = .5; g.fillStyle = css.getPropertyValue('--accent-2').trim(); for (var b = 0; b < w; b += +cv.dataset.beats) g.fillRect(b, 0, 1, 5); }
  }
  function init(root) {
    root = root || document;
    root.querySelectorAll('[data-woa-icon]').forEach(function (e) { e.innerHTML = icon(e.dataset.woaIcon) + e.innerHTML; });
    root.querySelectorAll('[data-woa-mark]').forEach(function (e) { e.innerHTML = mark(+e.dataset.woaMark || 26); });
    root.querySelectorAll('[data-woa-mode]').forEach(function (e) { theme(e, e.dataset.woaMode); });
    root.querySelectorAll('.woa-meter').forEach(function (e) { meter(e); });
    root.querySelectorAll('.woa-knob').forEach(knob);
    root.querySelectorAll('.woa-fader').forEach(fader);
    root.querySelectorAll('.woa-clock').forEach(clock);
    root.querySelectorAll('.woa-vu').forEach(vu);
    root.querySelectorAll('.woa-bars').forEach(barsView);
    root.querySelectorAll('.woa-levels').forEach(levels);
    var draw = function () { root.querySelectorAll('canvas[data-woa-wave]').forEach(wave); };
    draw(); new MutationObserver(draw).observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
    if (document.fonts) document.fonts.ready.then(draw);
  }
  window.WOA = { icon: icon, mark: mark, theme: theme, meter: meter, knob: knob, fader: fader, clock: clock, wave: wave, vu: vu, bars: barsView, levels: levels, init: init, fmt: FMT };
})();
