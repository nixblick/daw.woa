//! Abhörlautstärke (#358): ein Faktor nur für den Lautsprecher, nach dem Mix. Die Oberfläche
//! setzt ihn über ein Atom; der Audio-Thread liest es je Block und gleitet je Sample hin, damit
//! ein schneller Zug am Regler nicht knackt. Keine Allokation, kein Lock.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// Gleitzeit in Samples bis ~63 % des Ziels (bei 48 kHz rund 5 ms).
const GLIDE: f32 = 240.0;

/// Geteilter Sollwert, 0..=1 linear.
#[derive(Clone, Debug)]
pub struct MonitorVolume(Arc<AtomicU32>);

impl Default for MonitorVolume {
    fn default() -> Self {
        Self(Arc::new(AtomicU32::new(1.0f32.to_bits())))
    }
}

impl MonitorVolume {
    pub fn set(&self, v: f32) {
        let v = if v.is_finite() {
            v.clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.0.store(v.to_bits(), Ordering::Relaxed);
    }

    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    /// Der Teil für den Audio-Thread.
    pub fn stage(&self) -> MonitorStage {
        MonitorStage {
            target: self.clone(),
            now: self.get(),
        }
    }
}

/// Wendet die Abhörlautstärke im Audio-Thread an.
pub struct MonitorStage {
    target: MonitorVolume,
    now: f32,
}

impl MonitorStage {
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let want = self.target.get();
        if (self.now - want).abs() < 1e-6 {
            self.now = want;
            if (want - 1.0).abs() < 1e-6 {
                return;
            }
            for (a, b) in l.iter_mut().zip(r.iter_mut()) {
                *a *= want;
                *b *= want;
            }
            return;
        }
        let k = 1.0 / GLIDE;
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            self.now += (want - self.now) * k;
            *a *= self.now;
            *b *= self.now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_no_alloc::assert_no_alloc;

    #[test]
    fn full_volume_leaves_the_signal_alone() {
        let v = MonitorVolume::default();
        let mut s = v.stage();
        let (mut l, mut r) = (vec![0.5f32; 64], vec![-0.5f32; 64]);
        s.process(&mut l, &mut r);
        assert!(l.iter().all(|x| *x == 0.5) && r.iter().all(|x| *x == -0.5));
    }

    #[test]
    fn turning_down_glides_without_a_jump_and_without_allocation() {
        let v = MonitorVolume::default();
        let mut s = v.stage();
        v.set(0.25);
        let (mut l, mut r) = (vec![1.0f32; 48_000], vec![1.0f32; 48_000]);
        assert_no_alloc(|| s.process(&mut l, &mut r));
        // Kein Sprung von Sample zu Sample.
        assert!(l.windows(2).all(|w| (w[0] - w[1]).abs() < 0.01));
        assert!(l[0] > 0.99, "beginnt bei voll");
        assert!((l[47_999] - 0.25).abs() < 1e-3, "kommt an: {}", l[47_999]);
        v.set(f32::NAN);
        assert_eq!(v.get(), 1.0);
        v.set(3.0);
        assert_eq!(v.get(), 1.0);
    }
}
