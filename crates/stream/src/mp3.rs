//! MP3-Encoder (LAME, CBR) für den Stream: ein Encoder für die ganze Sendung.

use mp3lame_encoder::{Bitrate, Builder, DualPcm, Encoder, FlushNoGap, Quality};

/// kbit/s → LAME-Bitrate (nur die üblichen Stream-Raten).
pub fn bitrate(kbps: u32) -> Result<Bitrate, String> {
    Ok(match kbps {
        64 => Bitrate::Kbps64,
        96 => Bitrate::Kbps96,
        128 => Bitrate::Kbps128,
        160 => Bitrate::Kbps160,
        192 => Bitrate::Kbps192,
        256 => Bitrate::Kbps256,
        320 => Bitrate::Kbps320,
        _ => {
            return Err(format!(
                "Bitrate {kbps} kbit/s nicht möglich (64, 96, 128, 160, 192, 256, 320)"
            ));
        }
    })
}

pub struct Mp3Encoder {
    enc: Encoder,
    out: Vec<u8>,
}

impl Mp3Encoder {
    /// Stereo, `rate` Hz, CBR `kbps`.
    pub fn new(rate: u32, kbps: u32) -> Result<Self, String> {
        let e = |x: mp3lame_encoder::BuildError| format!("LAME: {x}");
        let mut b = Builder::new().ok_or("LAME nicht verfügbar")?;
        b.set_num_channels(2).map_err(e)?;
        b.set_sample_rate(rate).map_err(e)?;
        b.set_brate(bitrate(kbps)?).map_err(e)?;
        b.set_quality(Quality::Good).map_err(e)?;
        let enc = b.build().map_err(e)?;
        Ok(Self {
            enc,
            out: Vec::with_capacity(64 * 1024),
        })
    }

    /// Kodiert einen Block; die Bytes gelten bis zum nächsten Aufruf.
    pub fn encode(&mut self, left: &[f32], right: &[f32]) -> Result<&[u8], String> {
        self.out.clear();
        let n = left.len().min(right.len());
        self.out
            .reserve(mp3lame_encoder::max_required_buffer_size(n).max(7200));
        self.enc
            .encode_to_vec(
                DualPcm {
                    left: &left[..n],
                    right: &right[..n],
                },
                &mut self.out,
            )
            .map_err(|e| format!("LAME: {e}"))?;
        Ok(&self.out)
    }

    /// Rest ausgeben (am Ende der Sendung).
    pub fn flush(&mut self) -> Result<&[u8], String> {
        self.out.clear();
        self.out.reserve(7200);
        self.enc
            .flush_to_vec::<FlushNoGap>(&mut self.out)
            .map_err(|e| format!("LAME: {e}"))?;
        Ok(&self.out)
    }
}

/// Testton (Sinus, Stereo) für `stream-test` und Tests.
pub fn sine(rate: u32, freq: f32, start: usize, frames: usize, amp: f32) -> Vec<f32> {
    (start..start + frames)
        .map(|i| (i as f32 * freq * std::f32::consts::TAU / rate as f32).sin() * amp)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cbr_128_has_frame_sync_and_right_size() {
        let mut enc = Mp3Encoder::new(44_100, 128).unwrap();
        let mut bytes = Vec::new();
        for k in 0..10 {
            let s = sine(44_100, 440.0, k * 4410, 4410, 0.5);
            bytes.extend_from_slice(enc.encode(&s, &s).unwrap());
        }
        bytes.extend_from_slice(enc.flush().unwrap());
        assert!(
            bytes
                .windows(2)
                .any(|w| w[0] == 0xFF && w[1] & 0xE0 == 0xE0)
        );
        // 1 s at 128 kbit/s ≈ 16000 bytes.
        assert!((14_000..18_500).contains(&bytes.len()), "{}", bytes.len());
        assert!(Mp3Encoder::new(44_100, 100).is_err());
    }
}
