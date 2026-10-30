//! Übersetzt die Nachricht vom Handy in einen [`PartyCommand`], die Befehle der Party-Schicht
//! in `woa-radio`. GUI, MIDI und Fernbedienung sprechen so dieselbe Befehlsschicht (Idee aus
//! Mixxx: Controller sprechen benannte Controls, keine Sonderwege je Gerät).
//!
//! Hier steht auch die Rechtetabelle: Gäste dürfen nur wünschen, der DJ darf alles außer
//! Vollbild (das gehört dem Rechner, an dem die Party läuft).

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::Value;
use woa_radio::party::PartyCommand;

use crate::auth::Role;
use crate::snapshot::{CatalogItem, WishSnap, short_id};

/// Längster Wunschtitel (Zeichen).
pub const MAX_TITLE_CHARS: usize = 80;

/// Warum ein Befehl nicht ausgeführt wird.
#[derive(Debug, Clone, PartialEq)]
pub enum CmdError {
    /// Die Rolle darf das nicht (403).
    Forbidden,
    /// Nachricht kaputt oder unbekannter Befehl (400).
    Bad(&'static str),
    /// Titel oder Wunsch gibt es nicht (mehr) (404).
    NotFound,
}

/// Ein Befehl, der bei der Oberfläche ankommt.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteCommand {
    pub role: Role,
    pub command: PartyCommand,
}

/// Säubert einen Wunschtitel: Steuerzeichen raus, Leerraum zusammenziehen, Länge begrenzen.
pub fn clean_title(raw: &str) -> Option<String> {
    let t: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let n = t.chars().count();
    (1..=MAX_TITLE_CHARS).contains(&n).then_some(t)
}

/// Befehle, die nur der DJ schicken darf.
fn is_dj_only(cmd: &str) -> bool {
    matches!(
        cmd,
        "play_pause" | "next" | "volume_up" | "volume_down" | "play_wish" | "drop_wish"
    )
}

/// Wertet eine Nachricht `{"cmd": …}` aus.
///
/// `catalog`: Titel, die Gäste wünschen dürfen, nach Kennung. `wishes`: aktuelle Wünsche.
pub fn resolve(
    role: Role,
    body: &str,
    catalog: &HashMap<String, CatalogItem>,
    wishes: &[WishSnap],
) -> Result<PartyCommand, CmdError> {
    let v: Value = serde_json::from_str(body).map_err(|_| CmdError::Bad("kein JSON"))?;
    let cmd = v
        .get("cmd")
        .and_then(Value::as_str)
        .ok_or(CmdError::Bad("cmd fehlt"))?;
    let known = is_dj_only(cmd) || cmd == "wish";
    if !known {
        return Err(CmdError::Bad("unbekannter Befehl"));
    }
    if is_dj_only(cmd) && role != Role::Dj {
        return Err(CmdError::Forbidden);
    }
    let id = v.get("id").and_then(Value::as_str);
    let wish_path = |id: &str| -> Result<PathBuf, CmdError> {
        wishes
            .iter()
            .find(|w| short_id(&w.path) == id)
            .map(|w| w.path.clone())
            .ok_or(CmdError::NotFound)
    };
    match cmd {
        "play_pause" => Ok(PartyCommand::PlayPause),
        "next" => Ok(PartyCommand::Next),
        "volume_up" => Ok(PartyCommand::VolumeUp),
        "volume_down" => Ok(PartyCommand::VolumeDown),
        "play_wish" => Ok(PartyCommand::PlayWish(wish_path(
            id.ok_or(CmdError::Bad("id fehlt"))?,
        )?)),
        "drop_wish" => Ok(PartyCommand::DropWish(wish_path(
            id.ok_or(CmdError::Bad("id fehlt"))?,
        )?)),
        _ => {
            // wish: aus der Bibliothek (Kennung), eine Stimme für einen vorhandenen Wunsch
            // oder freier Text.
            if let Some(id) = id {
                if let Some(c) = catalog.get(id) {
                    return Ok(PartyCommand::Wish {
                        path: c.path.clone(),
                        title: c.title.clone(),
                    });
                }
                if let Some(w) = wishes.iter().find(|w| short_id(&w.path) == id) {
                    return Ok(PartyCommand::Wish {
                        path: w.path.clone(),
                        title: w.title.clone(),
                    });
                }
                return Err(CmdError::NotFound);
            }
            let title = v
                .get("title")
                .and_then(Value::as_str)
                .and_then(clean_title)
                .ok_or(CmdError::Bad("Titel fehlt oder ist zu lang"))?;
            // Freier Text hat keine Datei; der Platzhalter-Pfad ist nur Schlüssel in der Liste.
            Ok(PartyCommand::Wish {
                path: PathBuf::from(format!("wunsch:{}", title.to_lowercase())),
                title,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> HashMap<String, CatalogItem> {
        let item = CatalogItem {
            title: "Abba - Waterloo".into(),
            path: "/m/abba.mp3".into(),
        };
        HashMap::from([(short_id(&item.path), item)])
    }

    #[test]
    fn guest_may_only_wish() {
        let c = catalog();
        for cmd in ["play_pause", "next", "volume_up", "volume_down"] {
            let body = format!(r#"{{"cmd":"{cmd}"}}"#);
            assert_eq!(
                resolve(Role::Guest, &body, &c, &[]),
                Err(CmdError::Forbidden),
                "{cmd}"
            );
            assert!(resolve(Role::Dj, &body, &c, &[]).is_ok());
        }
        let body = format!(
            r#"{{"cmd":"wish","id":"{}"}}"#,
            short_id("/m/abba.mp3".as_ref())
        );
        assert_eq!(
            resolve(Role::Guest, &body, &c, &[]),
            Ok(PartyCommand::Wish {
                path: "/m/abba.mp3".into(),
                title: "Abba - Waterloo".into()
            })
        );
    }

    #[test]
    fn free_text_is_cleaned_and_limited() {
        let c = catalog();
        let r = resolve(
            Role::Guest,
            r#"{"cmd":"wish","title":"  Queen \n  Radio\u0007 "}"#,
            &c,
            &[],
        );
        match r {
            Ok(PartyCommand::Wish { title, .. }) => assert_eq!(title, "Queen Radio"),
            other => panic!("{other:?}"),
        }
        let long = format!(r#"{{"cmd":"wish","title":"{}"}}"#, "x".repeat(81));
        assert!(matches!(
            resolve(Role::Guest, &long, &c, &[]),
            Err(CmdError::Bad(_))
        ));
        assert!(matches!(
            resolve(Role::Guest, r#"{"cmd":"wish","title":"   "}"#, &c, &[]),
            Err(CmdError::Bad(_))
        ));
    }

    #[test]
    fn unknown_things_are_rejected() {
        let c = catalog();
        assert!(matches!(
            resolve(Role::Dj, "kein json", &c, &[]),
            Err(CmdError::Bad(_))
        ));
        assert!(matches!(
            resolve(Role::Dj, r#"{"cmd":"fullscreen"}"#, &c, &[]),
            Err(CmdError::Bad(_))
        ));
        assert!(matches!(
            resolve(Role::Dj, r#"{"cmd":"format_c"}"#, &c, &[]),
            Err(CmdError::Bad(_))
        ));
        assert_eq!(
            resolve(Role::Dj, r#"{"cmd":"play_wish","id":"0000"}"#, &c, &[]),
            Err(CmdError::NotFound)
        );
        assert_eq!(
            resolve(Role::Guest, r#"{"cmd":"wish","id":"0000"}"#, &c, &[]),
            Err(CmdError::NotFound)
        );
    }

    #[test]
    fn dj_can_play_and_drop_wishes_by_id() {
        let w = WishSnap {
            title: "X".into(),
            votes: 1,
            path: "/m/x.wav".into(),
        };
        let id = short_id(&w.path);
        let c = HashMap::new();
        assert_eq!(
            resolve(
                Role::Dj,
                &format!(r#"{{"cmd":"play_wish","id":"{id}"}}"#),
                &c,
                std::slice::from_ref(&w)
            ),
            Ok(PartyCommand::PlayWish("/m/x.wav".into()))
        );
        assert_eq!(
            resolve(
                Role::Dj,
                &format!(r#"{{"cmd":"drop_wish","id":"{id}"}}"#),
                &c,
                std::slice::from_ref(&w)
            ),
            Ok(PartyCommand::DropWish("/m/x.wav".into()))
        );
        // Gast kann für einen vorhandenen Wunsch stimmen.
        assert!(matches!(
            resolve(
                Role::Guest,
                &format!(r#"{{"cmd":"wish","id":"{id}"}}"#),
                &c,
                &[w]
            ),
            Ok(PartyCommand::Wish { .. })
        ));
    }
}
