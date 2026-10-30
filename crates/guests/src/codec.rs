//! Opus für den Netz-Thread: Decoder für das Mikro des Gastes, Encoder für den Rückweg.
//!
//! Opus läuft mit 48 kHz Stereo, 20-ms-Blöcke. Die Engine-Rate wird im Netz-Thread umgerechnet
//! ([`crate::resample`]). Nichts hiervon läuft im Audio-Thread.

use opus::{Application, Bitrate, Channels, Decoder, Encoder};

use crate::resample::StreamResampler;

/// Opus-Rate.
pub const OPUS_RATE: u32 = 48_000;
/// Frames je 20-ms-Block bei 48 kHz.
pub const FRAME_48K: usize = 960;
/// Bitrate des Rückwegs (Stereo, Musik und Sprache).
const RETURN_BITRATE: i32 = 64_000;
/// Größtes Opus-Paket, das wir erzeugen.
const MAX_PACKET: usize = 1_275;
/// Größter Block, den ein Paket liefern kann (120 ms bei 48 kHz).
const MAX_DECODE_FRAMES: usize = 5_760;

/// Wandelt Opus-Pakete in Stereo-PCM in der Rate der Engine.
pub struct VoiceDecoder {
    dec: Decoder,
    pcm: Vec<f32>,
    rs: StreamResampler,
    /// Länge des letzten Pakets in Frames (für den Ersatz bei Verlust).
    last_frames: usize,
}

impl VoiceDecoder {
    pub fn new(engine_rate: u32) -> Result<Self, String> {
        let dec = Decoder::new(OPUS_RATE, Channels::Stereo).map_err(|e| e.to_string())?;
        Ok(Self {
            dec,
            pcm: vec![0.0; MAX_DECODE_FRAMES * 2],
            rs: StreamResampler::new(OPUS_RATE, engine_rate),
            last_frames: FRAME_48K,
        })
    }

    /// Dekodiert ein Paket und hängt das Ergebnis an `out` an. Gibt die Zahl der Frames (Engine-
    /// Rate) zurück, die angehängt wurden.
    pub fn decode(&mut self, packet: &[u8], out: &mut Vec<f32>) -> Result<usize, String> {
        let n = self
            .dec
            .decode_float(packet, &mut self.pcm, false)
            .map_err(|e| e.to_string())?;
        self.last_frames = n.max(1);
        let before = out.len();
        self.rs.process(&self.pcm[..n * 2], out);
        Ok((out.len() - before) / 2)
    }

    /// Ersatz für ein verlorenes Paket (Opus-Verschleierung, Packet Loss Concealment).
    pub fn conceal(&mut self, out: &mut Vec<f32>) -> Result<usize, String> {
        let frames = self.last_frames.min(MAX_DECODE_FRAMES);
        let n = self
            .dec
            .decode_float(&[], &mut self.pcm[..frames * 2], false)
            .map_err(|e| e.to_string())?;
        let before = out.len();
        self.rs.process(&self.pcm[..n * 2], out);
        Ok((out.len() - before) / 2)
    }
}

/// Wandelt Stereo-PCM der Engine in Opus-Pakete zu 20 ms.
pub struct ReturnEncoder {
    enc: Encoder,
    rs: StreamResampler,
    /// Gesammeltes PCM bei 48 kHz, bis ein Block voll ist.
    pending: Vec<f32>,
    scratch: Vec<f32>,
    packet: Vec<u8>,
}

impl ReturnEncoder {
    pub fn new(engine_rate: u32) -> Result<Self, String> {
        let mut enc = Encoder::new(OPUS_RATE, Channels::Stereo, Application::Audio)
            .map_err(|e| e.to_string())?;
        enc.set_bitrate(Bitrate::Bits(RETURN_BITRATE))
            .map_err(|e| e.to_string())?;
        // Verlust im WLAN ist normal: Opus bekommt Reserve für Vorwärtsfehlerkorrektur.
        let _ = enc.set_inband_fec(true);
        let _ = enc.set_packet_loss_perc(5);
        Ok(Self {
            enc,
            rs: StreamResampler::new(engine_rate, OPUS_RATE),
            pending: Vec::with_capacity(FRAME_48K * 4),
            scratch: Vec::with_capacity(FRAME_48K * 4),
            packet: vec![0; MAX_PACKET],
        })
    }

    /// Nimmt PCM der Engine-Rate entgegen.
    pub fn push(&mut self, engine_pcm: &[f32]) {
        self.scratch.clear();
        self.rs.process(engine_pcm, &mut self.scratch);
        self.pending.extend_from_slice(&self.scratch);
    }

    /// Frames, die noch auf einen vollen Block warten.
    pub fn pending_frames(&self) -> usize {
        self.pending.len() / 2
    }

    /// Holt das nächste Paket, wenn ein voller 20-ms-Block da ist.
    pub fn next_packet(&mut self) -> Option<Result<Vec<u8>, String>> {
        if self.pending.len() < FRAME_48K * 2 {
            return None;
        }
        let r = self
            .enc
            .encode_float(&self.pending[..FRAME_48K * 2], &mut self.packet)
            .map(|n| self.packet[..n].to_vec())
            .map_err(|e| e.to_string());
        self.pending.drain(..FRAME_48K * 2);
        Some(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_block(freq: f32, start: usize, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let v = 0.3
                    * (std::f32::consts::TAU * freq * (start + i) as f32 / OPUS_RATE as f32).sin();
                [v, v]
            })
            .collect()
    }

    #[test]
    fn roundtrip_keeps_the_tone() {
        let mut enc = ReturnEncoder::new(OPUS_RATE).unwrap();
        let mut dec = VoiceDecoder::new(OPUS_RATE).unwrap();
        let mut out = Vec::new();
        for b in 0..50 {
            enc.push(&sine_block(1000.0, b * FRAME_48K, FRAME_48K));
            while let Some(p) = enc.next_packet() {
                dec.decode(&p.unwrap(), &mut out).unwrap();
            }
        }
        assert!(out.len() >= 48 * FRAME_48K);
        // Hinter dem Anlauf: Spitze nahe 0,3.
        let peak = out[out.len() / 2..]
            .iter()
            .fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.3).abs() < 0.05, "{peak}");
    }

    #[test]
    fn concealment_fills_a_lost_packet() {
        let mut enc = ReturnEncoder::new(OPUS_RATE).unwrap();
        let mut dec = VoiceDecoder::new(OPUS_RATE).unwrap();
        let mut out = Vec::new();
        enc.push(&sine_block(500.0, 0, FRAME_48K));
        let p = enc.next_packet().unwrap().unwrap();
        dec.decode(&p, &mut out).unwrap();
        let n = dec.conceal(&mut out).unwrap();
        assert_eq!(n, FRAME_48K);
    }
}
