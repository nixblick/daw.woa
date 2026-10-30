# tests/golden

Referenz-Renders für Golden-File-Tests (ab M1).

Mischpult mit Bus, Send und Effekten: `crates/engine/tests/mixer.rs` (`mixer_bus_send_05s.wav`, 48 kHz, Stereo, Toleranz 1e-5).

Eingebaute Effekte (Paket 17): `crates/dsp/tests/effects.rs` (`effects_p17_chain.wav`, 48 kHz, Mono, f32, neun Effekte hintereinander auf demselben Testsignal, Toleranz 1e-3 wegen Gleitkomma-Unterschieden zwischen Linux und Windows). Die Rauschunterdrückung steht nicht darin; sie wird gemessen (Pegel, gleiches Ergebnis live und im Export).

Neu schreiben: `WOA_BLESS=1 cargo test`. Instrumente: `crates/dsp/tests/instruments.rs` (48 kHz, Mono, f32, Toleranz 1e-4).
