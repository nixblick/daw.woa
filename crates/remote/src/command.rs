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

/// Die Befehle ohne Argument (Transport, Pegel, Modi) mit ihrem Namen auf dem Draht. Sie
/// tragen dieselben Namen wie [`PartyCommand::name`], im Protokoll der Oberfläche und als
/// OSC-Pfad (`/woa/party/<name>`): eine Tabelle für alle Wege hinein.
pub fn simple_command(name: &str) -> Option<PartyCommand> {
    Some(match name {
        "play_pause" => PartyCommand::PlayPause,
        "play" => PartyCommand::Play,
        "stop" => PartyCommand::Stop,
        "next" => PartyCommand::Next,
        "previous" => PartyCommand::Previous,
        "restart" => PartyCommand::Restart,
        "seek_back" => PartyCommand::SeekBack,
        "seek_forward" => PartyCommand::SeekForward,
        "mute" => PartyCommand::Mute,
        "repeat" => PartyCommand::CycleRepeat,
        "shuffle" => PartyCommand::ToggleShuffle,
        "volume_up" => PartyCommand::VolumeUp,
        "volume_down" => PartyCommand::VolumeDown,
        _ => return None,
    })
}

/// Befehle, die nur der DJ schicken darf: alles außer dem Wünschen. Neue Befehle sind damit
/// standardmäßig DJ-only.
fn is_dj_only(cmd: &str) -> bool {
    simple_command(cmd).is_some() || matches!(cmd, "play_wish" | "drop_wish")
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
    if let Some(c) = simple_command(cmd) {
        return Ok(c);
    }
    match cmd {
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

    /// Jeder Befehl ohne Argument, den die Fernbedienung kennt, mit dem Befehl dahinter.
    const ALL_SIMPLE: [&str; 13] = [
        "play_pause",
        "play",
        "stop",
        "next",
        "previous",
        "restart",
        "seek_back",
        "seek_forward",
        "mute",
        "repeat",
        "shuffle",
        "volume_up",
        "volume_down",
    ];

    #[test]
    fn every_transport_command_resolves_and_matches_its_name() {
        let c = catalog();
        for cmd in ALL_SIMPLE {
            let body = format!(r#"{{"cmd":"{cmd}"}}"#);
            let got = resolve(Role::Dj, &body, &c, &[]).unwrap_or_else(|e| panic!("{cmd}: {e:?}"));
            // Der Name auf dem Draht ist der Name im Protokoll der Oberfläche.
            let logged = got.name();
            let expected = if cmd == "repeat" { "repeat" } else { cmd };
            assert_eq!(logged, expected);
        }
    }

    #[test]
    fn transport_commands_ignore_extra_fields_and_stay_dj_only() {
        let c = catalog();
        // Zusatzfelder ändern nichts; Gäste bleiben ausgesperrt, auch mit Tricks.
        for body in [
            r#"{"cmd":"previous","id":"x","title":"y"}"#,
            r#"{"cmd":"seek_forward","seconds":9999}"#,
            r#"{"cmd":"mute","role":"dj"}"#,
        ] {
            assert_eq!(
                resolve(Role::Guest, body, &c, &[]),
                Err(CmdError::Forbidden)
            );
            assert!(resolve(Role::Dj, body, &c, &[]).is_ok());
        }
        // Falsche Schreibweise und Nachbarn sind unbekannt, nicht „fast“ erlaubt.
        for cmd in [
            "Previous",
            "prev",
            "seek",
            "seek_forward ",
            "repeat_all",
            "PLAY",
        ] {
            let body = format!(r#"{{"cmd":"{cmd}"}}"#);
            assert!(
                matches!(resolve(Role::Dj, &body, &c, &[]), Err(CmdError::Bad(_))),
                "{cmd}"
            );
        }
    }

    #[test]
    fn guest_may_only_wish() {
        let c = catalog();
        for cmd in ALL_SIMPLE {
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
