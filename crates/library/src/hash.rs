//! Inhaltskennung einer Datei (128 Bit) für die Doppelerkennung beim Import.
//!
//! Kein kryptographischer Hash: Er soll nur gleiche Dateien erkennen, nicht Angriffe abwehren.
//! Zwei unabhängige 64-Bit-Bahnen über 8-Byte-Wörter, am Ende durchmischt; eine zufällige
//! Kollision zweier verschiedener Titel ist bei 128 Bit nicht zu erwarten. Eigener Code, damit
//! für ein paar Zeilen keine Abhängigkeit hinzukommt.

use std::io::Read;
use std::path::Path;

const P1: u64 = 0x9E37_79B1_85EB_CA87;
const P2: u64 = 0xC2B2_AE3D_27D4_EB4F;

/// Zustand der Berechnung; füttere Bytes mit [`Hasher::update`], hole das Ergebnis mit
/// [`Hasher::finish`].
pub struct Hasher {
    a: u64,
    b: u64,
    len: u64,
    tail: [u8; 8],
    tail_len: usize,
}

impl Default for Hasher {
    fn default() -> Self {
        Self::new()
    }
}

impl Hasher {
    pub fn new() -> Self {
        Self {
            a: 0x243F_6A88_85A3_08D3,
            b: 0x1319_8A2E_0370_7344,
            len: 0,
            tail: [0; 8],
            tail_len: 0,
        }
    }

    fn word(&mut self, w: u64) {
        self.a = (self.a ^ w.wrapping_mul(P2))
            .rotate_left(31)
            .wrapping_mul(P1);
        self.b = (self.b.rotate_left(23) ^ w)
            .wrapping_mul(P2)
            .wrapping_add(P1);
    }

    pub fn update(&mut self, data: &[u8]) {
        self.len += data.len() as u64;
        let mut rest = data;
        if self.tail_len > 0 {
            let take = (8 - self.tail_len).min(rest.len());
            self.tail[self.tail_len..self.tail_len + take].copy_from_slice(&rest[..take]);
            self.tail_len += take;
            rest = &rest[take..];
            if self.tail_len < 8 {
                return;
            }
            self.word(u64::from_le_bytes(self.tail));
            self.tail_len = 0;
        }
        while rest.len() >= 8 {
            let (head, r) = rest.split_at(8);
            let mut w = [0u8; 8];
            w.copy_from_slice(head);
            self.word(u64::from_le_bytes(w));
            rest = r;
        }
        self.tail[..rest.len()].copy_from_slice(rest);
        self.tail_len = rest.len();
    }

    /// 32 Hex-Zeichen.
    pub fn finish(mut self) -> String {
        if self.tail_len > 0 {
            let mut w = [0u8; 8];
            w[..self.tail_len].copy_from_slice(&self.tail[..self.tail_len]);
            self.word(u64::from_le_bytes(w));
        }
        let len = self.len;
        self.word(len);
        let a = mix(self.a ^ self.b.rotate_left(17));
        let b = mix(self.b ^ self.a.rotate_left(41) ^ len);
        format!("{a:016x}{b:016x}")
    }
}

fn mix(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Kennung des Dateiinhalts, in Blöcken gelesen.
pub fn file_hash(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Hasher::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(data: &[u8]) -> String {
        let mut h = Hasher::new();
        h.update(data);
        h.finish()
    }

    #[test]
    fn equal_content_equal_hash_regardless_of_chunking() {
        let data: Vec<u8> = (0..1000u32).map(|i| (i * 7 % 251) as u8).collect();
        let mut h = Hasher::new();
        for c in data.chunks(13) {
            h.update(c);
        }
        assert_eq!(h.finish(), of(&data));
    }

    #[test]
    fn small_changes_change_the_hash() {
        let a = of(b"hello world");
        assert_ne!(a, of(b"hello worle"));
        assert_ne!(a, of(b"hello world\0"));
        assert_ne!(of(b""), of(b"\0"));
        assert_eq!(a.len(), 32);
    }
}
