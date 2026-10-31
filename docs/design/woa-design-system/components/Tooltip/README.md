# Tooltip

Tooltip für jeden Symbol-Knopf und jedes Feld: Name, Tastenkürzel, höchstens ein Satz Hinweis.

- **Stil:** dunkel in allen Themen (`lcd-bg`, Text `lcd-text`), Radius `radius-md`, 12 px, max. 260 px breit, 6 px unter dem Element (oben, wenn kein Platz).
- **Zeitverhalten:** erscheint nach 500 ms. Danach 300 ms „warm“, der nächste erscheint sofort. Verschwindet beim Verlassen und beim Klicken.
- **Tastenkürzel:** deutsche Tastennamen Strg, Umschalt, Alt, Entf, Leertaste, Enter, Esc; Kombinationen mit „+“, je Taste eine Kappe.
- **Text-Fallback** (egui ohne Kappen): „Löschen (Entf)“, „Duplizieren (Strg+D)“.
- Der Hinweis erklärt Bedienung, nie das Offensichtliche: „Ziehen · Doppelklick = Standard · Klick = eintippen“.
- Markup: `data-tip`, `data-kbd`, `data-hint`; `WOA.tips()` setzt auch `aria-label`.
