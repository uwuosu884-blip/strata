use std::f32::consts::TAU;

const DOWNBEAT_HZ: f32 = 1760.0;
const BEAT_HZ: f32 = 880.0;
const LEVEL: f32 = 0.5;
/// Time for the click to decay by 1/e.
const DECAY_SECONDS: f32 = 0.012;
const SILENCE: f32 = 1.0e-4;

/// Sample-accurate click generator.
///
/// A click starts on the first sample whose beat position is at or past a
/// whole beat, so it lines up exactly with the musical grid at any tempo.
/// Downbeats (beat 1 of each bar) use a higher pitch.
pub struct Metronome {
    pub enabled: bool,
    sample_rate: f32,
    decay_per_sample: f32,
    phase: f32,
    phase_inc: f32,
    envelope: f32,
}

impl Metronome {
    pub fn new(sample_rate: f64) -> Self {
        let sr = sample_rate as f32;
        Self {
            enabled: true,
            sample_rate: sr,
            decay_per_sample: (-1.0 / (DECAY_SECONDS * sr)).exp(),
            phase: 0.0,
            phase_inc: 0.0,
            envelope: 0.0,
        }
    }

    /// Next sample while the transport is playing.
    #[inline]
    pub fn next_sample(&mut self, beat: f64, beats_per_sample: f64, beats_per_bar: u32) -> f32 {
        // Consecutive samples are exactly `beats_per_sample` apart, so exactly one
        // sample per beat satisfies this — including position 0.0 at start, while
        // starting mid-beat correctly waits for the next beat.
        let on_beat = beat - beat.floor() < beats_per_sample;

        if on_beat && self.enabled {
            let beat_in_bar = (beat.floor() as u64) % beats_per_bar.max(1) as u64;
            let hz = if beat_in_bar == 0 { DOWNBEAT_HZ } else { BEAT_HZ };
            self.phase = 0.0; // sine starts at zero → no discontinuity
            self.phase_inc = TAU * hz / self.sample_rate;
            self.envelope = LEVEL;
        }
        self.tail_sample()
    }

    /// Next sample of whatever is still ringing (no new clicks).
    #[inline]
    pub fn tail_sample(&mut self) -> f32 {
        if self.envelope < SILENCE {
            self.envelope = 0.0;
            return 0.0;
        }
        let out = self.phase.sin() * self.envelope;
        self.phase += self.phase_inc;
        if self.phase >= TAU {
            self.phase -= TAU;
        }
        self.envelope *= self.decay_per_sample;
        out
    }
}
