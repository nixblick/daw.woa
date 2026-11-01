//! Cart-Wall-Sätze 1–5 (#292) und Umbenennen von Carts und Playlist-Titeln.
//!
//! Wie bei den Playlisten liegt der aktive Satz in [`RadioShow::carts`]; Mixer, Tasten,
//! Schnitt und Agent sehen nur ihn. [`RadioShow::cart_sets`] hält die Namen aller Sätze und
//! die Belegung der nicht aktiven. Alte Dateien ohne Sätze: die Belegung wird Satz 1.
//! Diese Funktionen nutzen App und MCP gleichermaßen.

use serde::{Deserialize, Serialize};

use crate::CARTS;
use crate::show::{CartEntry, RadioShow, title_of};

/// Zahl der Cart-Wall-Sätze.
pub const CART_SETS: usize = 5;

/// Längster Name für Satz, Cart oder Titel (Zeichen).
pub const MAX_NAME: usize = 80;

/// Ein benannter Satz der Cart-Wall.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CartSet {
    pub name: String,
    /// Belegung; leer für den aktiven Satz, dessen Carts in [`RadioShow::carts`] stehen.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub carts: Vec<Option<CartEntry>>,
}

fn default_name(i: usize) -> String {
    format!("Satz {}", i + 1)
}

/// Prüft einen Namen: getrimmt, nicht zu lang, ohne Steuerzeichen.
fn clean(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.chars().count() > MAX_NAME {
        return Err(format!("Der Name ist länger als {MAX_NAME} Zeichen"));
    }
    if name.chars().any(char::is_control) {
        return Err("Der Name enthält Steuerzeichen".into());
    }
    Ok(name.to_string())
}

/// Fünf leere Sätze „Satz 1“ … „Satz 5“.
pub fn default_sets() -> Vec<CartSet> {
    (0..CART_SETS)
        .map(|i| CartSet {
            name: default_name(i),
            carts: Vec::new(),
        })
        .collect()
}

impl RadioShow {
    /// Genau [`CART_SETS`] Sätze, gültiger aktiver Index, Namen nie leer.
    pub(crate) fn sanitize_cart_sets(&mut self) {
        self.cart_sets.truncate(CART_SETS);
        while self.cart_sets.len() < CART_SETS {
            self.cart_sets.push(CartSet {
                name: default_name(self.cart_sets.len()),
                carts: Vec::new(),
            });
        }
        if self.cart_set >= CART_SETS {
            self.cart_set = 0;
        }
        for (i, s) in self.cart_sets.iter_mut().enumerate() {
            if s.name.trim().is_empty() {
                s.name = default_name(i);
            }
            if !s.carts.is_empty() {
                s.carts.resize(CARTS, None);
            }
        }
        let stray = std::mem::take(&mut self.cart_sets[self.cart_set].carts);
        if self.carts.iter().all(Option::is_none) && !stray.is_empty() {
            self.carts = stray;
            self.carts.resize(CARTS, None);
        }
    }

    /// Namen der Sätze in ihrer Reihenfolge.
    pub fn cart_set_names(&self) -> Vec<String> {
        self.cart_sets.iter().map(|s| s.name.clone()).collect()
    }

    /// Macht Satz `i` aktiv. Die bisherige Belegung wandert in ihren Satz zurück.
    pub fn switch_cart_set(&mut self, i: usize) -> Result<(), String> {
        if i >= self.cart_sets.len() {
            return Err(format!("Satz {} gibt es nicht", i + 1));
        }
        if i == self.cart_set {
            return Ok(());
        }
        let old = std::mem::replace(&mut self.carts, vec![None; CARTS]);
        self.cart_sets[self.cart_set].carts = if old.iter().all(Option::is_none) {
            Vec::new()
        } else {
            old
        };
        let mut new = std::mem::take(&mut self.cart_sets[i].carts);
        new.resize(CARTS, None);
        self.carts = new;
        self.cart_set = i;
        Ok(())
    }

    /// Benennt Satz `i` um; leer setzt „Satz n“.
    pub fn rename_cart_set(&mut self, i: usize, name: &str) -> Result<(), String> {
        let name = clean(name)?;
        let set = self
            .cart_sets
            .get_mut(i)
            .ok_or_else(|| format!("Satz {} gibt es nicht", i + 1))?;
        set.name = if name.is_empty() {
            default_name(i)
        } else {
            name
        };
        Ok(())
    }

    /// Benennt Cart `slot` im aktiven Satz um; leer setzt den Dateinamen.
    pub fn rename_cart(&mut self, slot: usize, name: &str) -> Result<(), String> {
        let name = clean(name)?;
        match self.carts.get_mut(slot) {
            Some(Some(e)) => {
                e.label = if name.is_empty() {
                    title_of(&e.path)
                } else {
                    name
                };
                Ok(())
            }
            Some(None) => Err(format!("Cart {} ist frei", slot + 1)),
            None => Err(format!("Cart {} gibt es nicht", slot + 1)),
        }
    }

    /// Benennt Titel `i` der aktiven Playlist um (nur die Anzeige, die Datei bleibt);
    /// leer setzt den Dateinamen.
    pub fn rename_title(&mut self, i: usize, name: &str) -> Result<(), String> {
        let name = clean(name)?;
        let e = self
            .playlist
            .get_mut(i)
            .ok_or_else(|| format!("Titel {} gibt es nicht", i + 1))?;
        e.name = (!name.is_empty() && name != title_of(&e.path)).then_some(name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlaylistEntry;

    fn cart(p: &str) -> Option<CartEntry> {
        Some(CartEntry::new(p))
    }

    #[test]
    fn old_file_becomes_set_one() {
        let mut old = RadioShow::default();
        old.carts[0] = cart("jingles/id.wav");
        let mut v = serde_json::to_value(&old).unwrap();
        let o = v.as_object_mut().unwrap();
        o.remove("cart_sets");
        o.remove("cart_set");
        let show = RadioShow::from_json(&v.to_string()).unwrap();
        assert_eq!(show.cart_sets.len(), CART_SETS);
        assert_eq!(show.cart_set, 0);
        assert_eq!(show.cart_set_names()[0], "Satz 1");
        assert_eq!(show.carts[0], cart("jingles/id.wav"));
    }

    #[test]
    fn switch_keeps_each_set_and_survives_save() {
        let mut show = RadioShow::default();
        show.carts[0] = cart("morgen.wav");
        show.rename_cart_set(0, " Morgen ").unwrap();
        show.switch_cart_set(1).unwrap();
        assert!(show.carts.iter().all(Option::is_none));
        assert_eq!(show.carts.len(), CARTS);
        show.carts[3] = cart("mittag.wav");
        show.rename_cart_set(1, "Mittag").unwrap();

        let text = serde_json::to_string(&show).unwrap();
        let mut back = RadioShow::from_json(&text).unwrap();
        assert_eq!(back.cart_set, 1);
        assert_eq!(back.carts[3], cart("mittag.wav"));
        assert_eq!(&back.cart_set_names()[..2], ["Morgen", "Mittag"]);
        back.switch_cart_set(0).unwrap();
        assert_eq!(back.carts[0], cart("morgen.wav"));
        assert!(back.carts[3].is_none());
        back.switch_cart_set(1).unwrap();
        assert_eq!(back.carts[3], cart("mittag.wav"));
        assert!(back.switch_cart_set(CART_SETS).is_err());
    }

    #[test]
    fn rename_cart_set_and_title() {
        let mut show = RadioShow::default();
        assert!(show.rename_cart(0, "x").is_err(), "freies Cart");
        show.carts[2] = cart("a/station-id.wav");
        show.rename_cart(2, "  Station ID morgens ").unwrap();
        assert_eq!(show.carts[2].as_ref().unwrap().label, "Station ID morgens");
        show.rename_cart(2, "").unwrap();
        assert_eq!(show.carts[2].as_ref().unwrap().label, "station-id");
        assert!(show.rename_cart(2, &"x".repeat(MAX_NAME + 1)).is_err());
        assert!(show.rename_cart(2, "a\nb").is_err());

        show.rename_cart_set(4, "Sondersendung").unwrap();
        show.rename_cart_set(4, " ").unwrap();
        assert_eq!(show.cart_set_names()[4], "Satz 5");

        show.playlist
            .push(PlaylistEntry::new("musik/song_final_v3.mp3"));
        show.rename_title(0, "Song").unwrap();
        assert_eq!(show.playlist[0].title(), "Song");
        let back = RadioShow::from_json(&serde_json::to_string(&show).unwrap()).unwrap();
        assert_eq!(back.playlist[0].title(), "Song");
        show.rename_title(0, "").unwrap();
        assert_eq!(show.playlist[0].title(), "song_final_v3");
        assert!(show.playlist[0].name.is_none());
        assert!(show.rename_title(5, "x").is_err());
    }
}
