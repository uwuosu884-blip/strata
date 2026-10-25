pub const MIN_TEMPO: f64 = 20.0;
pub const MAX_TEMPO: f64 = 999.0;

/// Musical clock. Position is kept in **beats** (f64) rather than samples so
/// tempo changes never make the playhead jump.
#[derive(Debug, Clone, PartialEq)]
pub struct Transport {
    pub playing: bool,
    tempo_bpm: f64,
    pub beats_per_bar: u32,
    /// Playhead in beats from the start of the song (0.0 = bar 1, beat 1).
    pub beat_position: f64,
    pub loop_enabled: bool,
    pub loop_start: f64,
    pub loop_end: f64,
}

impl Default for Transport {
    fn default() -> Self {
        Self {
            playing: false,
            tempo_bpm: 120.0,
            beats_per_bar: 4,
            beat_position: 0.0,
            loop_enabled: false,
            loop_start: 0.0,
            loop_end: 16.0,
        }
    }
}

impl Transport {
    pub fn tempo(&self) -> f64 {
        self.tempo_bpm
    }

    pub fn set_tempo(&mut self, bpm: f64) {
        if bpm.is_finite() {
            self.tempo_bpm = bpm.clamp(MIN_TEMPO, MAX_TEMPO);
        }
    }

    /// How far the playhead advances per audio sample.
    #[inline]
    pub fn beats_per_sample(&self, sample_rate: f64) -> f64 {
        self.tempo_bpm / (60.0 * sample_rate)
    }

    /// Loop region if active and valid.
    pub fn active_loop(&self) -> Option<(f64, f64)> {
        (self.loop_enabled && self.loop_end - self.loop_start >= 1.0 / 16.0).then_some((self.loop_start, self.loop_end))
    }

    pub fn position(&self) -> MusicalPosition {
        MusicalPosition::from_beats(self.beat_position, self.beats_per_bar)
    }
}

/// Bars.Beats.Sixteenths, 1-based like Ableton's display ("1.1.1").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MusicalPosition {
    pub bar: u64,
    pub beat: u32,
    pub sixteenth: u32,
}

impl MusicalPosition {
    pub fn from_beats(beats: f64, beats_per_bar: u32) -> Self {
        let beats = beats.max(0.0);
        let bpb = beats_per_bar.max(1) as u64;
        let whole_beats = beats.floor() as u64;
        let sixteenth = ((beats.fract() * 4.0).floor() as u32).min(3);
        Self {
            bar: whole_beats / bpb + 1,
            beat: (whole_beats % bpb) as u32 + 1,
            sixteenth: sixteenth + 1,
        }
    }
}

impl std::fmt::Display for MusicalPosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.bar, self.beat, self.sixteenth)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_display_is_one_based() {
        assert_eq!(MusicalPosition::from_beats(0.0, 4).to_string(), "1.1.1");
        assert_eq!(MusicalPosition::from_beats(1.25, 4).to_string(), "1.2.2");
        assert_eq!(MusicalPosition::from_beats(4.0, 4).to_string(), "2.1.1");
        assert_eq!(MusicalPosition::from_beats(6.99, 3).to_string(), "3.1.4");
    }

    #[test]
    fn tempo_is_clamped_and_ignores_nan() {
        let mut t = Transport::default();
        t.set_tempo(5.0);
        assert_eq!(t.tempo(), MIN_TEMPO);
        t.set_tempo(5000.0);
        assert_eq!(t.tempo(), MAX_TEMPO);
        t.set_tempo(f64::NAN);
        assert_eq!(t.tempo(), MAX_TEMPO);
    }

    #[test]
    fn beats_per_sample_at_120_bpm() {
        let t = Transport::default();
        // 120 BPM = 2 beats/s → 1 beat every 24 000 samples at 48 kHz.
        assert!((1.0 / t.beats_per_sample(48_000.0) - 24_000.0).abs() < 1e-9);
    }
}
