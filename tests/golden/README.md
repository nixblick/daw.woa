# tests/golden

Referenz-Renders für Golden-File-Tests (ab M1).

Mischpult mit Bus, Send und Effekten: `crates/engine/tests/mixer.rs` (`mixer_bus_send_05s.wav`, 48 kHz, Stereo, Toleranz 1e-5).

Neu schreiben: `WOA_BLESS=1 cargo test`. Instrumente: `crates/dsp/tests/instruments.rs` (48 kHz, Mono, f32, Toleranz 1e-4).
