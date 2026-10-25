use crate::transport::{MusicalPosition, Transport};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};

/// Engine → GUI state, shared through plain atomics (lock-free, wait-free).
///
/// Floats are stored as their bit patterns. For non-negative floats the bit
/// pattern orders the same way as the value, so `fetch_max` on the bits works
/// as a "max since last read" for peak meters.
#[derive(Default)]
pub(crate) struct SharedStatus {
    playing: AtomicBool,
    beat_position: AtomicU64,
    tempo: AtomicU64,
    beats_per_bar: AtomicU32,
    peak_left: AtomicU32,
    peak_right: AtomicU32,
    cpu_load: AtomicU32,
}

impl SharedStatus {
    pub fn publish_transport(&self, t: &Transport) {
        self.playing.store(t.playing, Relaxed);
        self.beat_position.store(t.beat_position.to_bits(), Relaxed);
        self.tempo.store(t.tempo().to_bits(), Relaxed);
        self.beats_per_bar.store(t.beats_per_bar, Relaxed);
    }

    pub fn publish_peaks(&self, peaks: [f32; 2]) {
        // NaN guard: a NaN would poison fetch_max forever.
        let clean = |p: f32| if p.is_finite() { p.abs() } else { 0.0 };
        self.peak_left.fetch_max(clean(peaks[0]).to_bits(), Relaxed);
        self.peak_right.fetch_max(clean(peaks[1]).to_bits(), Relaxed);
    }

    pub fn publish_cpu_load(&self, load: f32) {
        self.cpu_load.store(load.max(0.0).to_bits(), Relaxed);
    }

    pub fn snapshot(&self) -> StatusSnapshot {
        let beats_per_bar = self.beats_per_bar.load(Relaxed);
        let beat_position = f64::from_bits(self.beat_position.load(Relaxed));
        StatusSnapshot {
            playing: self.playing.load(Relaxed),
            beat_position,
            position: MusicalPosition::from_beats(beat_position, beats_per_bar),
            tempo: f64::from_bits(self.tempo.load(Relaxed)),
            beats_per_bar,
            peak: [
                f32::from_bits(self.peak_left.swap(0, Relaxed)),
                f32::from_bits(self.peak_right.swap(0, Relaxed)),
            ],
            cpu_load: f32::from_bits(self.cpu_load.load(Relaxed)),
        }
    }
}

/// A copy of the engine state, for display.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatusSnapshot {
    pub playing: bool,
    pub beat_position: f64,
    pub position: MusicalPosition,
    pub tempo: f64,
    pub beats_per_bar: u32,
    /// Peak absolute sample per channel since the previous snapshot.
    pub peak: [f32; 2],
    /// Fraction of the buffer deadline used by the last callback.
    pub cpu_load: f32,
}
