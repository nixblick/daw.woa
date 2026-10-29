//! Peak-Sammlung für die Live-Wellenform (#43): nach dem Anlegen keine Allokation.

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_engine::clip::PEAK_BLOCK;
use woa_engine::live_peaks::live_peaks;

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

#[test]
fn collecting_peaks_does_not_allocate() {
    let (mut c, mut live) = live_peaks(2);
    let block = vec![0.5f32; 2 * 480];
    // 20 000 × 480 Frames ≈ 37 500 Blöcke, mehr als der Ring fasst.
    for _ in 0..20_000 {
        assert_no_alloc(|| c.push_slice(&block));
    }
    // Auch der volle Ring (verlorene Blöcke zählen) allokiert nicht.
    let mut out = Vec::new();
    live.drain_into(&mut out);
    assert_eq!(out.len(), 20_000 * 480 / PEAK_BLOCK);
}
