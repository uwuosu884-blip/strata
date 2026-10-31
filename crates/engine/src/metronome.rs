use std::f32::consts::TAU;

const LEVEL: f32 = 0.5;
const SILENCE: f32 = 1.0e-4;

/// Metronome sound (Live's metronome menu).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum MetroSound {
    /// Sine blip, 1760 Hz downbeat / 880 Hz beat.
    #[default]
    Classic,
    /// Short bright noise tick.
    Click,
    /// Woodblock: two inharmonic partials, fast decay.
    Wood,
}

/// Metronome options (saved in the project).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct MetronomeSettings {
    /// Bars counted in before recording starts from stop (0 = none).
    pub count_in_bars: u8,
    pub sound: MetroSound,
    /// Click interval in beats; 0 = Auto (every beat), -1 = one bar.
    pub interval: f64,
    /// Click only while recording (and during the count-in).
    pub rec_only: bool,
}

impl Default for MetronomeSettings {
    fn default() -> Self {
        Self { count_in_bars: 0, sound: MetroSound::Classic, interval: 0.0, rec_only: false }
    }
}

/// Sample-accurate click generator.
///
/// A click starts on the first sample whose beat position is at or past a
/// grid line (every `interval` beats), so it lines up exactly with the
/// musical grid at any tempo. Downbeats (beat 1 of each bar) are accented;
/// subdivisions between beats are softer.
pub struct Metronome {
    pub enabled: bool,
    pub settings: MetronomeSettings,
    sample_rate: f32,
    decay_per_sample: f32,
    phase: f32,
    phase_inc: f32,
    phase2: f32,
    phase2_inc: f32,
    envelope: f32,
    noise: u32,
}

impl Metronome {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            enabled: true,
            settings: MetronomeSettings::default(),
            sample_rate: sample_rate as f32,
            decay_per_sample: 0.0,
            phase: 0.0,
            phase_inc: 0.0,
            phase2: 0.0,
            phase2_inc: 0.0,
            envelope: 0.0,
            noise: 0x1234_5678,
        }
    }

    fn interval(&self, beats_per_bar: u32) -> f64 {
        let iv = self.settings.interval;
        if !iv.is_finite() {
            1.0
        } else if iv < 0.0 {
            beats_per_bar.max(1) as f64
        } else if iv > 1.0e-3 {
            iv
        } else {
            1.0
        }
    }

    /// Next sample while the transport is playing (or counting in: `beat` < 0).
    #[inline]
    pub fn next_sample(&mut self, beat: f64, beats_per_sample: f64, beats_per_bar: u32) -> f32 {
        if self.enabled {
            let iv = self.interval(beats_per_bar);
            let k = beat / iv;
            // Consecutive samples are exactly `beats_per_sample` apart, so exactly
            // one sample per grid line satisfies this — including position 0.0.
            if k - k.floor() < beats_per_sample / iv {
                let tick = k.floor() * iv;
                let bpb = beats_per_bar.max(1) as f64;
                let in_bar = tick.rem_euclid(bpb);
                let accent = if in_bar < 1.0e-6 || in_bar > bpb - 1.0e-6 {
                    2
                } else if (tick - tick.round()).abs() < 1.0e-6 {
                    1
                } else {
                    0
                };
                self.trigger(accent);
            }
        }
        self.tail_sample()
    }

    /// Start a click: accent 2 = downbeat, 1 = beat, 0 = subdivision.
    fn trigger(&mut self, accent: u8) {
        let sr = self.sample_rate;
        let level = LEVEL * [0.55, 1.0, 1.0][accent as usize];
        let (hz, ratio, decay) = match self.settings.sound {
            MetroSound::Classic => ([660.0, 880.0, 1760.0][accent as usize], 0.0, 0.012),
            MetroSound::Click => ([5000.0, 6000.0, 8000.0][accent as usize], 0.0, 0.0025),
            MetroSound::Wood => ([700.0, 900.0, 1250.0][accent as usize], 2.76, 0.022),
        };
        self.phase = 0.0; // sines start at zero → no discontinuity
        self.phase2 = 0.0;
        self.phase_inc = TAU * hz / sr;
        self.phase2_inc = TAU * hz * ratio / sr;
        self.decay_per_sample = (-1.0 / (decay * sr)).exp();
        self.envelope = level;
    }

    /// Next sample of whatever is still ringing (no new clicks).
    #[inline]
    pub fn tail_sample(&mut self) -> f32 {
        if self.envelope < SILENCE {
            self.envelope = 0.0;
            return 0.0;
        }
        let s = match self.settings.sound {
            MetroSound::Classic => self.phase.sin(),
            MetroSound::Click => {
                // White noise ringing a resonant tone: a bright, short tick.
                self.noise ^= self.noise << 13;
                self.noise ^= self.noise >> 17;
                self.noise ^= self.noise << 5;
                let n = (self.noise as f32 / u32::MAX as f32) * 2.0 - 1.0;
                0.6 * n + 0.6 * self.phase.sin()
            }
            MetroSound::Wood => 0.75 * self.phase.sin() + 0.35 * self.phase2.sin(),
        };
        let out = s * self.envelope;
        self.phase += self.phase_inc;
        if self.phase >= TAU {
            self.phase -= TAU;
        }
        self.phase2 += self.phase2_inc;
        if self.phase2 >= TAU {
            self.phase2 -= TAU;
        }
        self.envelope *= self.decay_per_sample;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clicks(m: &mut Metronome, from: f64, beats: f64) -> Vec<usize> {
        let bps = 1.0 / 1000.0;
        let n = (beats / bps) as usize;
        let mut out = Vec::new();
        for i in 0..n {
            let before = m.envelope;
            m.next_sample(from + i as f64 * bps, bps, 4);
            if m.envelope > before + 1.0e-3 || (i == 0 && m.envelope > 0.0 && before == 0.0) {
                out.push(i);
            }
        }
        out
    }

    #[test]
    fn interval_and_count_in_beats() {
        let mut m = Metronome::new(48_000.0);
        assert_eq!(clicks(&mut m, 0.0, 2.0).len(), 2);
        m.settings.interval = 0.5;
        assert_eq!(clicks(&mut m, 0.0, 2.0).len(), 4);
        m.settings.interval = -1.0; // one bar
        assert_eq!(clicks(&mut m, 0.0, 8.0).len(), 2);
        // Count-in runs at negative beats.
        m.settings.interval = 0.0;
        assert_eq!(clicks(&mut m, -4.0, 4.0).len(), 4);
    }

    #[test]
    fn sounds_are_finite() {
        for s in [MetroSound::Classic, MetroSound::Click, MetroSound::Wood] {
            let mut m = Metronome::new(44_100.0);
            m.settings.sound = s;
            let mut peak = 0.0f32;
            for i in 0..4000 {
                let v = m.next_sample(i as f64 / 4000.0, 1.0 / 4000.0, 4);
                assert!(v.is_finite());
                peak = peak.max(v.abs());
            }
            assert!(peak > 0.05 && peak <= 1.0, "{s:?} {peak}");
        }
    }
}
