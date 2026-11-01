//! Paket J5 (#285): Stimme über das Bett. Die Stimmspur bekommt die Sprachkette, jede andere
//! Audio- und MIDI-Spur einen Kompressor, dessen Sidechain die Stimme ist. Busse können keine
//! Sidechain haben, darum sitzt der Ducker auf den Bett-Spuren selbst. Ein Weg für App und MCP.

use crate::command::Command;
use crate::effect::{EffectKind, Insert, InsertId, VOICE_CHAIN};
use crate::error::{ModelError, Result};
use crate::project::{Track, TrackId, TrackKind};
use crate::session::Session;

/// Der Ducker auf dem Bett: tief ansetzen, kräftig drücken, langsam zurück, damit das Bett
/// zwischen zwei Wörtern nicht pumpt.
pub const DUCK_COMPRESSOR: [(&str, f64); 5] = [
    ("threshold_db", -40.0),
    ("ratio", 8.0),
    ("attack_ms", 5.0),
    ("release_ms", 300.0),
    ("makeup_db", 0.0),
];

/// Was [`Session::voice_over`] angelegt hat (leer, wenn schon alles da war).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VoiceOver {
    /// Die Sprachkette auf der Stimme, in Reihenfolge.
    pub chain: Vec<InsertId>,
    /// Je Bett-Spur der neue Ducker.
    pub ducks: Vec<(TrackId, InsertId)>,
}

/// Eine Spur mit De-Esser trägt schon eine Sprachkette.
fn has_voice_chain(t: &Track) -> bool {
    t.inserts.iter().any(|i| i.kind == EffectKind::DeEsser)
}

fn ducked_by(t: &Track, voice: TrackId) -> bool {
    t.inserts
        .iter()
        .any(|i| i.kind == EffectKind::Compressor && i.sidechain == Some(voice))
}

impl Session {
    /// Legt die Sprachkette auf `voice` (falls dort noch keine ist) und duckt jede andere
    /// Audio- und MIDI-Spur ohne eigene Sprachkette per Sidechain unter die Stimme.
    /// Ein Undo-Schritt; ein zweiter Aufruf legt nichts doppelt an.
    pub fn voice_over(&mut self, voice: TrackId) -> Result<VoiceOver> {
        let v = self
            .project()
            .track(voice)
            .ok_or(ModelError::TrackNotFound(voice))?;
        if v.kind == TrackKind::Bus {
            return Err(ModelError::InvalidArgument(format!(
                "Spur {} ist ein Bus, die Stimme muss eine Audio- oder MIDI-Spur sein",
                voice.0
            )));
        }
        let need_chain = !has_voice_chain(v);
        let bed: Vec<TrackId> = self
            .project()
            .tracks
            .iter()
            .filter(|t| t.id != voice && t.kind != TrackKind::Bus)
            .filter(|t| !has_voice_chain(t) && !ducked_by(t, voice))
            .map(|t| t.id)
            .collect();
        self.group("Stimme über Bett", |s| {
            let mut out = VoiceOver::default();
            if need_chain {
                for (kind, values) in VOICE_CHAIN {
                    let id = s.new_insert_id();
                    s.apply(Command::AddInsert {
                        track: voice,
                        insert: Insert::with_values(id, kind, values)?,
                        index: None,
                    })?;
                    out.chain.push(id);
                }
            }
            for track in bed {
                let id = s.new_insert_id();
                s.apply(Command::AddInsert {
                    track,
                    insert: Insert::with_values(id, EffectKind::Compressor, &DUCK_COMPRESSOR)?,
                    index: None,
                })?;
                s.apply(Command::SetInsertSidechain {
                    track,
                    insert: id,
                    source: Some(voice),
                })?;
                out.ducks.push((track, id));
            }
            Ok(out)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(s: &mut Session, name: &str, kind: TrackKind) -> TrackId {
        let id = s.new_track_id();
        s.apply(Command::add_track(id, name, kind)).unwrap();
        id
    }

    #[test]
    fn voice_gets_the_chain_and_every_bed_track_a_ducker() {
        let mut s = Session::new("j5");
        let drums = track(&mut s, "Drums", TrackKind::Midi);
        let loop_ = track(&mut s, "Loop", TrackKind::Audio);
        let bus = track(&mut s, "Bett", TrackKind::Bus);
        let voice = track(&mut s, "Stimme", TrackKind::Audio);
        let r = s.voice_over(voice).unwrap();
        assert_eq!(r.chain.len(), VOICE_CHAIN.len());
        assert_eq!(
            r.ducks.iter().map(|d| d.0).collect::<Vec<_>>(),
            vec![drums, loop_]
        );
        for t in [drums, loop_] {
            let ins = &s.project().track(t).unwrap().inserts;
            assert_eq!(ins.len(), 1);
            assert_eq!(ins[0].kind, EffectKind::Compressor);
            assert_eq!(ins[0].sidechain, Some(voice));
        }
        assert!(s.project().track(bus).unwrap().inserts.is_empty());
        // Ein zweiter Aufruf ändert nichts und hinterlässt keinen Undo-Schritt.
        assert_eq!(s.voice_over(voice).unwrap(), VoiceOver::default());
        s.undo().unwrap();
        assert!(s.project().tracks.iter().all(|t| t.inserts.is_empty()));
    }

    #[test]
    fn a_second_voice_is_not_ducked_and_buses_are_rejected() {
        let mut s = Session::new("j5");
        let bed = track(&mut s, "Bett", TrackKind::Audio);
        let a = track(&mut s, "Stimme A", TrackKind::Audio);
        let b = track(&mut s, "Stimme B", TrackKind::Audio);
        s.voice_over(a).unwrap();
        let r = s.voice_over(b).unwrap();
        // Bett wird auch unter B geduckt, Stimme A (mit Sprachkette) nicht.
        assert_eq!(r.ducks.iter().map(|d| d.0).collect::<Vec<_>>(), vec![bed]);
        let bus = track(&mut s, "Bus", TrackKind::Bus);
        assert!(s.voice_over(bus).is_err());
        assert!(s.voice_over(TrackId(9999)).is_err());
    }
}
