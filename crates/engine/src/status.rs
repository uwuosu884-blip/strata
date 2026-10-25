use crate::song::MAX_TRACKS;
use crate::transport::{MusicalPosition, Transport};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicU32, AtomicU64, Ordering::Relaxed};

/// Per-track state the GUI displays.
#[derive(Default)]
pub(crate) struct TrackStatus {
    playing: AtomicI32,
    pending: AtomicI32,
    clip_pos: AtomicU64,
    peak_l: AtomicU32,
    peak_r: AtomicU32,
    voices: AtomicU32,
    arrangement_off: AtomicBool,
}

/// Engine → GUI state through plain atomics (wait-free both sides).
/// Non-negative floats are stored as bits; their bit order matches value order,
/// so `fetch_max` works as "peak since last read".
pub(crate) struct SharedStatus {
    playing: AtomicBool,
    beat_position: AtomicU64,
    tempo: AtomicU64,
    beats_per_bar: AtomicU32,
    peak_left: AtomicU32,
    peak_right: AtomicU32,
    cpu_load: AtomicU32,
    record_state: AtomicU8,
    tracks: Box<[TrackStatus; MAX_TRACKS]>,
}

impl Default for SharedStatus {
    fn default() -> Self {
        let s = Self {
            playing: AtomicBool::new(false),
            beat_position: AtomicU64::new(0),
            tempo: AtomicU64::new(120f64.to_bits()),
            beats_per_bar: AtomicU32::new(4),
            peak_left: AtomicU32::new(0),
            peak_right: AtomicU32::new(0),
            cpu_load: AtomicU32::new(0),
            record_state: AtomicU8::new(0),
            tracks: Box::new(std::array::from_fn(|_| TrackStatus::default())),
        };
        for t in s.tracks.iter() {
            t.playing.store(-1, Relaxed);
            t.pending.store(NO_PENDING, Relaxed);
        }
        s
    }
}

pub(crate) const NO_PENDING: i32 = -1;
pub(crate) const PENDING_STOP: i32 = -2;

#[inline]
fn clean(p: f32) -> u32 {
    if p.is_finite() { p.abs().to_bits() } else { 0 }
}

impl SharedStatus {
    pub fn publish_transport(&self, t: &Transport) {
        self.playing.store(t.playing, Relaxed);
        self.beat_position.store(t.beat_position.to_bits(), Relaxed);
        self.tempo.store(t.tempo().to_bits(), Relaxed);
        self.beats_per_bar.store(t.beats_per_bar, Relaxed);
    }

    pub fn publish_peaks(&self, peaks: [f32; 2]) {
        self.peak_left.fetch_max(clean(peaks[0]), Relaxed);
        self.peak_right.fetch_max(clean(peaks[1]), Relaxed);
    }

    pub fn publish_cpu_load(&self, load: f32) {
        self.cpu_load.store(load.max(0.0).to_bits(), Relaxed);
    }

    pub fn publish_record_state(&self, s: RecordState) {
        self.record_state.store(s as u8, Relaxed);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn publish_track(
        &self,
        rt: usize,
        playing: Option<u32>,
        pending: i32,
        clip_pos: f64,
        peak: [f32; 2],
        voices: usize,
        arrangement_off: bool,
    ) {
        let t = &self.tracks[rt];
        t.playing.store(playing.map_or(-1, |s| s as i32), Relaxed);
        t.pending.store(pending, Relaxed);
        t.clip_pos.store(clip_pos.to_bits(), Relaxed);
        t.peak_l.fetch_max(clean(peak[0]), Relaxed);
        t.peak_r.fetch_max(clean(peak[1]), Relaxed);
        t.voices.store(voices as u32, Relaxed);
        t.arrangement_off.store(arrangement_off, Relaxed);
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
            peak: [f32::from_bits(self.peak_left.swap(0, Relaxed)), f32::from_bits(self.peak_right.swap(0, Relaxed))],
            cpu_load: f32::from_bits(self.cpu_load.load(Relaxed)),
            record_state: RecordState::from_u8(self.record_state.load(Relaxed)),
        }
    }

    /// Read (and reset the peaks of) one track's status.
    pub fn track(&self, rt: usize) -> TrackSnapshot {
        let t = &self.tracks[rt.min(MAX_TRACKS - 1)];
        let pending = t.pending.load(Relaxed);
        TrackSnapshot {
            playing_scene: u32::try_from(t.playing.load(Relaxed)).ok(),
            pending: match pending {
                NO_PENDING => None,
                PENDING_STOP => Some(PendingLaunch::Stop),
                s => Some(PendingLaunch::Scene(s as u32)),
            },
            clip_pos: f64::from_bits(t.clip_pos.load(Relaxed)),
            peak: [f32::from_bits(t.peak_l.swap(0, Relaxed)), f32::from_bits(t.peak_r.swap(0, Relaxed))],
            voices: t.voices.load(Relaxed),
            arrangement_off: t.arrangement_off.load(Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum RecordState {
    #[default]
    Idle = 0,
    /// Armed, waiting for the next quantize boundary.
    StartPending = 1,
    Recording = 2,
    StopPending = 3,
}

impl RecordState {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::StartPending,
            2 => Self::Recording,
            3 => Self::StopPending,
            _ => Self::Idle,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatusSnapshot {
    pub playing: bool,
    pub beat_position: f64,
    pub position: MusicalPosition,
    pub tempo: f64,
    pub beats_per_bar: u32,
    /// Master peak per channel since the previous snapshot.
    pub peak: [f32; 2],
    /// Fraction of the buffer deadline used by the last callback.
    pub cpu_load: f32,
    pub record_state: RecordState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingLaunch {
    Scene(u32),
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TrackSnapshot {
    pub playing_scene: Option<u32>,
    pub pending: Option<PendingLaunch>,
    /// Position inside the playing session clip (beats).
    pub clip_pos: f64,
    pub peak: [f32; 2],
    pub voices: u32,
    /// True when a session clip overrides the arrangement on this track.
    pub arrangement_off: bool,
}
