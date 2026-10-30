//! Optionaler OSC-Eingang (UDP) für TouchOSC & Co.
//!
//! OSC kennt keine Anmeldung. Darum ist er standardmäßig aus, und wer ihn einschaltet, gibt
//! jedem im Netz DJ-Rechte. Die Pfade sind fest (Muster aus Ardours OSC-Oberfläche: ein Pfad je
//! Befehl, Namen wie in [`crate::command::simple_command`]): `/woa/party/play_pause`, `/next`,
//! `/previous`, `/stop`, `/seek_back`, `/volume_up` und so weiter. Tasten schicken oft
//! 1.0 beim Drücken und 0.0 beim Loslassen; nur das Drücken zählt.

use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rosc::{OscPacket, OscType};
use woa_radio::party::PartyCommand;

use crate::auth::{Bucket, Role};
use crate::command::RemoteCommand;

/// Der Befehl zu einem OSC-Pfad.
pub fn command_for(addr: &str) -> Option<PartyCommand> {
    crate::command::simple_command(addr.strip_prefix("/woa/party/")?)
}

/// Zählt als Drücken: kein Argument, Zahl ungleich 0, wahr.
fn pressed(args: &[OscType]) -> bool {
    match args.first() {
        None => true,
        Some(OscType::Float(f)) => *f > 0.5,
        Some(OscType::Double(f)) => *f > 0.5,
        Some(OscType::Int(i)) => *i != 0,
        Some(OscType::Long(i)) => *i != 0,
        Some(OscType::Bool(b)) => *b,
        Some(_) => true,
    }
}

fn collect(p: &OscPacket, depth: u32, out: &mut Vec<PartyCommand>) {
    match p {
        OscPacket::Message(m) => {
            if pressed(&m.args)
                && let Some(c) = command_for(&m.addr)
            {
                out.push(c);
            }
        }
        OscPacket::Bundle(b) if depth < 4 => {
            for q in &b.content {
                collect(q, depth + 1, out);
            }
        }
        OscPacket::Bundle(_) => {}
    }
}

/// Startet den Eingang. Gibt Thread und die tatsächliche Adresse zurück.
pub(crate) fn spawn(
    addr: SocketAddr,
    tx: SyncSender<RemoteCommand>,
    stop: Arc<AtomicBool>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
    per_second: u32,
) -> io::Result<(JoinHandle<()>, SocketAddr)> {
    let sock = UdpSocket::bind(addr)?;
    sock.set_read_timeout(Some(Duration::from_millis(100)))?;
    let local = sock.local_addr()?;
    let h = std::thread::Builder::new()
        .name("woa-remote-osc".into())
        .spawn(move || {
            let mut buf = vec![0u8; 2048];
            let mut bucket = Bucket::new(Instant::now());
            while !stop.load(Ordering::Relaxed) {
                let (n, from) = match sock.recv_from(&mut buf) {
                    Ok(x) => x,
                    Err(e)
                        if matches!(
                            e.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                        ) =>
                    {
                        continue;
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "remote: osc receive failed");
                        std::thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                };
                let packet = match rosc::decoder::decode_udp(&buf[..n]) {
                    Ok((_, p)) => p,
                    Err(e) => {
                        tracing::debug!(%from, error = ?e, "remote: bad osc packet");
                        continue;
                    }
                };
                let mut cmds = Vec::new();
                collect(&packet, 0, &mut cmds);
                for command in cmds {
                    if !bucket.take(Instant::now(), per_second) {
                        tracing::warn!(%from, "remote: osc rate limit");
                        break;
                    }
                    tracing::info!(%from, cmd = %command.name(), "remote: osc command");
                    match tx.try_send(RemoteCommand {
                        role: Role::Dj,
                        command,
                    }) {
                        Ok(()) => {}
                        Err(TrySendError::Full(_)) => {
                            tracing::warn!(%from, "remote: osc queue full");
                            continue;
                        }
                        Err(TrySendError::Disconnected(_)) => return,
                    }
                    if let Some(w) = &wake {
                        w();
                    }
                }
            }
        })?;
    Ok((h, local))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rosc::{OscBundle, OscMessage, OscTime};

    fn msg(addr: &str, args: Vec<OscType>) -> OscPacket {
        OscPacket::Message(OscMessage {
            addr: addr.into(),
            args,
        })
    }

    fn bundle(content: Vec<OscPacket>) -> OscPacket {
        OscPacket::Bundle(OscBundle {
            timetag: OscTime {
                seconds: 0,
                fractional: 1,
            },
            content,
        })
    }

    #[test]
    fn only_presses_of_known_paths_count() {
        let mut out = Vec::new();
        collect(
            &msg("/woa/party/next", vec![OscType::Float(1.0)]),
            0,
            &mut out,
        );
        collect(
            &msg("/woa/party/next", vec![OscType::Float(0.0)]),
            0,
            &mut out,
        );
        collect(&msg("/woa/party/play_pause", vec![]), 0, &mut out);
        collect(&msg("/woa/party/unbekannt", vec![]), 0, &mut out);
        collect(&msg("/ardour/transport", vec![]), 0, &mut out);
        assert_eq!(out, vec![PartyCommand::Next, PartyCommand::PlayPause]);
    }

    #[test]
    fn transport_paths_map_to_commands() {
        for (path, cmd) in [
            ("previous", PartyCommand::Previous),
            ("stop", PartyCommand::Stop),
            ("play", PartyCommand::Play),
            ("restart", PartyCommand::Restart),
            ("seek_back", PartyCommand::SeekBack),
            ("seek_forward", PartyCommand::SeekForward),
            ("mute", PartyCommand::Mute),
            ("repeat", PartyCommand::CycleRepeat),
            ("shuffle", PartyCommand::ToggleShuffle),
        ] {
            assert_eq!(command_for(&format!("/woa/party/{path}")), Some(cmd));
        }
        assert_eq!(command_for("/woa/party/"), None);
        assert_eq!(command_for("/woa/other/previous"), None);
    }

    #[test]
    fn bundles_are_unpacked_with_depth_limit() {
        let mut p = bundle(vec![msg("/woa/party/volume_up", vec![])]);
        for _ in 0..6 {
            p = bundle(vec![p]);
        }
        let mut out = Vec::new();
        collect(&p, 0, &mut out);
        assert!(out.is_empty(), "zu tief verschachtelt");
        collect(
            &bundle(vec![msg("/woa/party/volume_up", vec![])]),
            0,
            &mut out,
        );
        assert_eq!(out, vec![PartyCommand::VolumeUp]);
    }
}
