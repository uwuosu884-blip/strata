/// Metronome sound (Live's metronome menu).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum MetroSound {
    #[default]
    Classic,
    Click,
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

/// Live's metronome samples (16-bit WAV, embedded). `UP` = the accented
/// downbeat; the plain sample plays on every other click.
const SAMPLES: [(&[u8], &[u8]); 3] = [
    (include_bytes!("../assets/metronome/Classic.wav"), include_bytes!("../assets/metronome/ClassicUP.wav")),
    (include_bytes!("../assets/metronome/Click.wav"), include_bytes!("../assets/metronome/ClickUP.wav")),
    (include_bytes!("../assets/metronome/Wood.wav"), include_bytes!("../assets/metronome/WoodUP.wav")),
];

/// The samples are quiet (peaks 0.14 – 0.37); bring them near the old level.
const GAIN: f32 = 1.5;

/// Decode a PCM WAV (8/16/24/32-bit int or 32-bit float) to mono f32 at `sr`
/// (linear resampling — fine for clicks). Malformed data gives silence.
fn decode_wav(bytes: &[u8], sr: f32) -> Vec<f32> {
    let u16le = |i: usize| bytes.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
    let u32le = |i: usize| bytes.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Vec::new();
    }
    let (mut fmt, mut ch, mut rate, mut bits, mut data) = (0, 0, 0, 0, None);
    let mut i = 12;
    while let (Some(id), Some(len)) = (bytes.get(i..i + 4), u32le(i + 4)) {
        let body = i + 8;
        if id == b"fmt " {
            fmt = u16le(body).unwrap_or(0);
            ch = u16le(body + 2).unwrap_or(0);
            rate = u32le(body + 4).unwrap_or(0);
            bits = u16le(body + 14).unwrap_or(0);
        } else if id == b"data" {
            data = bytes.get(body..(body + len).min(bytes.len()));
        }
        i = body + len + (len & 1);
    }
    let (Some(data), true) = (data, ch > 0 && rate > 0 && matches!(bits, 8 | 16 | 24 | 32)) else {
        return Vec::new();
    };
    let bps = bits / 8;
    let sample = |o: usize| -> f32 {
        let b = &data[o..o + bps];
        match (fmt, bits) {
            (3, 32) => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            (_, 8) => (b[0] as f32 - 128.0) / 128.0,
            (_, 16) => i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0,
            (_, 24) => (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8_388_608.0,
            _ => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0,
        }
    };
    let frames = data.len() / (bps * ch);
    let mono: Vec<f32> = (0..frames).map(|f| (0..ch).map(|c| sample((f * ch + c) * bps)).sum::<f32>() / ch as f32 * GAIN).collect();
    let ratio = rate as f32 / sr.max(1.0);
    if (ratio - 1.0).abs() < 1.0e-6 {
        return mono;
    }
    let n = (frames as f32 / ratio).floor() as usize;
    (0..n)
        .map(|k| {
            let x = k as f32 * ratio;
            let j = x as usize;
            let t = x - j as f32;
            let a = mono.get(j).copied().unwrap_or(0.0);
            let b = mono.get(j + 1).copied().unwrap_or(0.0);
            a + (b - a) * t
        })
        .collect()
}

/// Sample-accurate click player.
///
/// A click starts on the first sample whose beat position is at or past a
/// grid line (every `interval` beats), so it lines up exactly with the
/// musical grid at any tempo. Downbeats (beat 1 of each bar) play the `UP`
/// sample, every other click the plain one. All buffers are decoded at
/// construction, so the audio thread never allocates.
pub struct Metronome {
    pub enabled: bool,
    pub settings: MetronomeSettings,
    /// `[sound][0 = beat, 1 = downbeat]`, at the engine sample rate.
    samples: [[Vec<f32>; 2]; 3],
    /// Currently sounding buffer and read position.
    cur: (usize, usize),
    pos: usize,
}

impl Metronome {
    pub fn new(sample_rate: f64) -> Self {
        let sr = sample_rate as f32;
        let samples = SAMPLES.map(|(beat, up)| [decode_wav(beat, sr), decode_wav(up, sr)]);
        Self { enabled: true, settings: MetronomeSettings::default(), samples, cur: (0, 0), pos: usize::MAX }
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
            // `pos >= 4`: rounding can put two consecutive samples on one grid
            // line; never restart a click that has just started.
            if k - k.floor() < beats_per_sample / iv && self.pos >= 4 {
                let tick = k.floor() * iv;
                let bpb = beats_per_bar.max(1) as f64;
                let in_bar = tick.rem_euclid(bpb);
                let down = in_bar < 1.0e-6 || in_bar > bpb - 1.0e-6;
                self.trigger(down);
            }
        }
        self.tail_sample()
    }

    fn trigger(&mut self, downbeat: bool) {
        let sound = match self.settings.sound {
            MetroSound::Classic => 0,
            MetroSound::Click => 1,
            MetroSound::Wood => 2,
        };
        self.cur = (sound, downbeat as usize);
        self.pos = 0;
    }

    /// Next sample of whatever is still ringing (no new clicks).
    #[inline]
    pub fn tail_sample(&mut self) -> f32 {
        let buf = &self.samples[self.cur.0][self.cur.1];
        match buf.get(self.pos) {
            Some(&s) => {
                self.pos += 1;
                s
            }
            None => 0.0,
        }
    }

    /// True while a click is still sounding (tests).
    #[cfg(test)]
    fn sounding(&self) -> bool {
        self.pos < self.samples[self.cur.0][self.cur.1].len()
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
            m.next_sample(from + i as f64 * bps, bps, 4);
            if m.pos == 1 {
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

    #[test]
    fn samples_decode_and_downbeat_uses_up() {
        let m = Metronome::new(48_000.0);
        for (i, pair) in m.samples.iter().enumerate() {
            for b in pair {
                // 44.1 kHz files of 5513 – 6935 frames, resampled to 48 kHz.
                assert!(b.len() > 5_900 && b.len() < 7_600, "{i} {}", b.len());
                assert!(b.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
            }
            assert_ne!(pair[0], pair[1]);
        }
        let mut m = Metronome::new(44_100.0);
        assert_eq!(m.samples[0][0].len(), 5513, "same rate: no resampling");
        m.next_sample(0.0, 0.001, 4);
        assert_eq!(m.cur, (0, 1), "beat 1 = UP sample");
        assert!(m.sounding());
        (0..10).for_each(|_| _ = m.tail_sample());
        m.next_sample(1.0, 0.001, 4);
        assert_eq!(m.cur, (0, 0));
        m.settings.sound = MetroSound::Wood;
        (0..10).for_each(|_| _ = m.tail_sample());
        m.next_sample(4.0, 0.001, 4);
        assert_eq!(m.cur, (2, 1));
        assert!(decode_wav(b"junk", 48_000.0).is_empty());
    }
}
