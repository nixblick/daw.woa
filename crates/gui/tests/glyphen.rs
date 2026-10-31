//! Zeichen ohne Glyphe erscheinen in egui als leeres Kästchen (#218: Hoch/Runter in „Verwalten“).
//! Dieser Test liest alle Quelltexte der Oberfläche und prüft jedes Nicht-ASCII-Zeichen außerhalb
//! von Kommentaren gegen die eingebetteten Schriften. Ohne Fenster, ohne Ton.

use std::path::Path;

use eframe::egui;
use woa_gui::theme;

fn sources(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).expect("src lesbar") {
        let p = e.expect("Eintrag").path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Code einer Zeile ohne Kommentar (grob: `//` außerhalb eines Strings beendet die Zeile).
fn code_part(line: &str) -> String {
    let mut in_str = false;
    let mut prev = ' ';
    let mut out = String::new();
    let mut it = line.chars().peekable();
    while let Some(c) = it.next() {
        if c == '"' && prev != '\\' {
            in_str = !in_str;
        }
        if !in_str && c == '/' && it.peek() == Some(&'/') {
            break;
        }
        out.push(c);
        prev = if prev == '\\' && c == '\\' { ' ' } else { c };
    }
    out
}

fn ctx_with_fonts() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.set_fonts(theme::font_definitions());
    let mut input = egui::RawInput::default();
    input.viewports.entry(egui::ViewportId::ROOT).or_default();
    let _ = ctx.run_ui(input, |_| {});
    ctx
}

#[test]
fn every_ui_character_has_a_glyph() {
    let ctx = ctx_with_fonts();
    let mut files = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    files.sort();
    let mut missing = Vec::new();
    let fonts = [
        egui::FontId::proportional(14.0),
        egui::FontId::monospace(14.0),
    ];
    for f in &files {
        let text = std::fs::read_to_string(f).expect("lesbar");
        // Testmodule prüfen Eingaben, keine Beschriftungen.
        let text = text.split("#[cfg(test)]").next().unwrap_or_default();
        for (n, line) in text.lines().enumerate() {
            for c in code_part(line).chars().filter(|c| !c.is_ascii()) {
                for font in &fonts {
                    if !ctx.fonts_mut(|fo| fo.has_glyph(font, c)) {
                        missing.push(format!(
                            "{}:{} '{c}' U+{:04X} {:?}",
                            f.file_name().unwrap().to_string_lossy(),
                            n + 1,
                            c as u32,
                            font.family
                        ));
                    }
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "Zeichen ohne Glyphe:\n{}",
        missing.join("\n")
    );
}

#[test]
fn icons_used_for_buttons_exist() {
    let ctx = ctx_with_fonts();
    let font = egui::FontId::proportional(14.0);
    for icon in [
        theme::ic::ARROW_UP,
        theme::ic::ARROW_DOWN,
        theme::ic::X,
        theme::ic::CARET_UP,
        theme::ic::CARET_DOWN,
        theme::ic::TRASH,
    ] {
        assert!(ctx.fonts_mut(|f| f.has_glyphs(&font, icon)), "{icon:?}");
    }
}

#[test]
fn fullwidth_title_characters_are_shown_narrow() {
    let ctx = ctx_with_fonts();
    let font = egui::FontId::proportional(14.0);
    let title = "Interpret ｜ Titel（Live）";
    assert!(!ctx.fonts_mut(|f| f.has_glyphs(&font, title)));
    let shown = theme::display_text(title);
    assert_eq!(shown, "Interpret | Titel(Live)");
    assert!(ctx.fonts_mut(|f| f.has_glyphs(&font, &shown)));
    assert!(matches!(
        theme::display_text("Ä normal"),
        std::borrow::Cow::Borrowed(_)
    ));
}
