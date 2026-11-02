//! Device display taps: the audio thread publishes levels / modulation values
//! of a device (Gate, Glue Compressor, Limiter, LFO) through atomics, the GUI
//! reads them. Each tap has a few "latest" values and a ring of frames written
//! every [`HOP`] samples (display history).

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

/// Values per history frame.
pub const FRAME: usize = 4;
/// History frames kept (power of two).
pub const RING: usize = 1 << 14;
/// Latest-value slots.
pub const VALS: usize = 48;
/// Samples between history frames at 48 kHz (scaled with the rate).
pub const HOP: usize = 64;

pub struct DevTap {
    vals: [AtomicU32; VALS],
    ring: Box<[AtomicU32]>,
    pos: AtomicUsize,
    /// History frames per second.
    rate: AtomicU32,
}

impl std::fmt::Debug for DevTap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DevTap")
    }
}

impl DevTap {
    fn new() -> Self {
        Self {
            vals: std::array::from_fn(|_| AtomicU32::new(0)),
            ring: (0..RING * FRAME).map(|_| AtomicU32::new(0)).collect(),
            pos: AtomicUsize::new(0),
            rate: AtomicU32::new((48_000 / HOP) as u32),
        }
    }

    pub fn set(&self, k: usize, v: f32) {
        self.vals[k].store(v.to_bits(), Ordering::Relaxed);
    }

    pub fn get(&self, k: usize) -> f32 {
        f32::from_bits(self.vals[k].load(Ordering::Relaxed))
    }

    /// `set(k, max(get(k), v))` (peaks held until the GUI takes them).
    pub fn raise(&self, k: usize, v: f32) {
        let a = &self.vals[k];
        let mut cur = a.load(Ordering::Relaxed);
        while v > f32::from_bits(cur) {
            match a.compare_exchange_weak(cur, v.to_bits(), Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(x) => cur = x,
            }
        }
    }

    /// Read and reset a held peak.
    pub fn take(&self, k: usize, reset: f32) -> f32 {
        f32::from_bits(self.vals[k].swap(reset.to_bits(), Ordering::Relaxed))
    }

    pub fn push(&self, f: [f32; FRAME]) {
        let p = self.pos.load(Ordering::Relaxed);
        let i = (p & (RING - 1)) * FRAME;
        for (k, v) in f.iter().enumerate() {
            self.ring[i + k].store(v.to_bits(), Ordering::Relaxed);
        }
        self.pos.store(p + 1, Ordering::Release);
    }

    pub fn set_rate(&self, frames_per_sec: f32) {
        self.rate.store(frames_per_sec.to_bits(), Ordering::Relaxed);
    }

    /// History frames per second.
    pub fn rate(&self) -> f32 {
        let r = f32::from_bits(self.rate.load(Ordering::Relaxed));
        if r.is_finite() && r > 0.0 { r } else { 750.0 }
    }

    /// Frames pushed so far.
    pub fn written(&self) -> usize {
        self.pos.load(Ordering::Acquire)
    }

    /// The latest `n` frames, oldest first (≤ [`RING`]).
    pub fn history(&self, n: usize) -> Vec<[f32; FRAME]> {
        self.history_to(self.written(), n)
    }

    /// The `n` frames before frame `end` (absolute index), oldest first.
    /// Displays bucket frames from a fixed boundary so columns don't shimmer.
    pub fn history_to(&self, end: usize, n: usize) -> Vec<[f32; FRAME]> {
        let end = end.min(self.written());
        let n = n.min(RING).min(end);
        (0..n)
            .map(|j| {
                let i = (end - n + j) & (RING - 1);
                std::array::from_fn(|k| f32::from_bits(self.ring[i * FRAME + k].load(Ordering::Relaxed)))
            })
            .collect()
    }
}

static TAPS: Mutex<Vec<(u64, Weak<DevTap>)>> = Mutex::new(Vec::new());

/// The display tap of device `id` (shared by the engine and the GUI).
pub fn dev_tap(id: u64) -> Arc<DevTap> {
    let mut taps = TAPS.lock().unwrap_or_else(|e| e.into_inner());
    taps.retain(|(_, w)| w.strong_count() > 0);
    if let Some(t) = taps.iter().find(|(i, _)| *i == id).and_then(|(_, w)| w.upgrade()) {
        return t;
    }
    let t = Arc::new(DevTap::new());
    taps.push((id, Arc::downgrade(&t)));
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_and_peaks() {
        let t = dev_tap(0xdead_beef);
        for i in 0..10 {
            t.push([i as f32, 0.0, 0.0, 0.0]);
        }
        let h = t.history(3);
        assert_eq!(h.iter().map(|f| f[0]).collect::<Vec<_>>(), [7.0, 8.0, 9.0]);
        t.raise(1, 0.5);
        t.raise(1, 0.25);
        assert_eq!(t.take(1, 0.0), 0.5);
        assert_eq!(t.get(1), 0.0);
        assert!(Arc::ptr_eq(&t, &dev_tap(0xdead_beef)));
    }
}
