//! Brücke zur Party-Schicht in `woa-radio`: aus dem Layout und dem Schnappschuss der Sendung
//! wird, was das Handy sieht. Ohne Fenster testbar.

use std::path::PathBuf;

use woa_radio::party::{PartyLayout, PartyState};
use woa_radio::station::View;

use crate::snapshot::{CatalogItem, NowSnap, PartySnapshot, WishSnap};

/// Der Zustand für die Handys.
pub fn snapshot(layout: &PartyLayout, state: &PartyState) -> PartySnapshot {
    PartySnapshot {
        now: layout.now.as_ref().map(|n| NowSnap {
            title: n.title.clone(),
            artist: n.artist.clone(),
            remaining: n.remaining.clone(),
            progress: n.progress,
            playing: n.playing,
            paused: n.paused,
        }),
        next: layout.next.clone(),
        wishes: state
            .wishes
            .items()
            .iter()
            .map(|w| WishSnap {
                title: w.title.clone(),
                votes: w.votes,
                path: w.path.clone(),
            })
            .collect(),
        level_db: layout.level_db,
        repeat: layout.repeat,
        shuffle: layout.shuffle,
        muted: layout.muted,
    }
}

/// Die Titel, die Gäste wünschen dürfen: die Musiktitel der Playlist (keine Sprachaufnahmen).
pub fn catalog(view: &View) -> Vec<CatalogItem> {
    view.playlist
        .iter()
        .filter(|e| e.voice.is_none())
        .map(|e| CatalogItem {
            title: e.title.clone(),
            path: e.path.clone(),
        })
        .collect()
}

/// Fingerabdruck der Playlist: Länge, erster und letzter Pfad.
pub type CatalogSig = (usize, Option<PathBuf>, Option<PathBuf>);

/// Ein günstiger Fingerabdruck der Playlist, damit der Katalog nur bei Änderungen neu gebaut wird.
pub fn catalog_signature(view: &View) -> CatalogSig {
    (
        view.playlist.len(),
        view.playlist.first().map(|e| e.path.clone()),
        view.playlist.last().map(|e| e.path.clone()),
    )
}
