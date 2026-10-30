//! Messung: Lautheit (BS.1770 / EBU R128), True Peak, Spektrum, Spektrogramm und Stereo-Bild
//! (Paket 19).
//!
//! Alles hier läuft im Control-Thread (Oberfläche, Agent, Tests), nie im Audio-Thread: Der
//! Audio-Thread reicht Samples nur über eine lock-freie Queue weiter (`woa-engine::tap`).

pub mod loudness;
pub mod spectrogram;
pub mod spectrum;
pub mod stereo;

/// Pegel in dBFS; `None` bei Stille.
pub fn amp_to_db(amp: f64) -> Option<f64> {
    (amp > 0.0).then(|| 20.0 * amp.log10())
}
