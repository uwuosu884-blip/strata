//! Warping: clip beat ↔ sample frame maps and the time-stretch modes.
//!
//! Re-Pitch, Beats, Tones and Texture are evaluated statelessly per output
//! sample from the warp map (RT-safe, seek/loop/tempo-change proof).
//! Complex / Complex Pro use the streaming phase vocoder in [`crate::pv`].

use crate::sample::SampleBuffer;

/// Piecewise-linear map content beat → sample frame (monotonic).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WarpMap {
    /// (beat, frame), sorted, ≥ 1 knot.
    pub knots: Vec<(f64, f64)>,
    /// Frames per beat before the first / after the last knot.
    pub slope_in: f64,
    pub slope_out: f64,
}

impl WarpMap {
    /// Straight line: `frame = offset + beat · fpb`.
    pub fn linear(offset: f64, fpb: f64) -> Self {
        Self { knots: vec![(0.0, offset)], slope_in: fpb, slope_out: fpb }
    }

    /// From markers `(beat, frame)`; ends extend with the neighbouring
    /// segment's tempo (or `fpb` with a single marker).
    pub fn from_markers(markers: &[(f64, f64)], offset: f64, fpb: f64) -> Self {
        let mut k: Vec<(f64, f64)> = markers.to_vec();
        k.sort_by(|a, b| a.0.total_cmp(&b.0));
        k.dedup_by(|b, a| (b.0 - a.0).abs() < 1e-9 || b.1 <= a.1);
        match k.len() {
            0 => Self::linear(offset, fpb),
            1 => Self { knots: k, slope_in: fpb, slope_out: fpb },
            n => {
                let s_in = (k[1].1 - k[0].1) / (k[1].0 - k[0].0);
                let s_out = (k[n - 1].1 - k[n - 2].1) / (k[n - 1].0 - k[n - 2].0);
                Self { knots: k, slope_in: s_in, slope_out: s_out }
            }
        }
    }

    #[inline]
    pub fn frame_at(&self, beat: f64) -> f64 {
        let k = &self.knots;
        if k.is_empty() {
            return beat * self.slope_out;
        }
        let i = k.partition_point(|p| p.0 <= beat);
        if i == 0 {
            k[0].1 + (beat - k[0].0) * self.slope_in
        } else if i >= k.len() {
            let l = k[k.len() - 1];
            l.1 + (beat - l.0) * self.slope_out
        } else {
            let (a, b) = (k[i - 1], k[i]);
            a.1 + (beat - a.0) * (b.1 - a.1) / (b.0 - a.0)
        }
    }

    /// Inverse of [`frame_at`].
    pub fn beat_at(&self, frame: f64) -> f64 {
        let k = &self.knots;
        if k.is_empty() {
            return frame / self.slope_out.max(1e-9);
        }
        let i = k.partition_point(|p| p.1 <= frame);
        if i == 0 {
            k[0].0 + (frame - k[0].1) / self.slope_in.max(1e-9)
        } else if i >= k.len() {
            let l = k[k.len() - 1];
            l.0 + (frame - l.1) / self.slope_out.max(1e-9)
        } else {
            let (a, b) = (k[i - 1], k[i]);
            a.0 + (frame - a.1) * (b.0 - a.0) / (b.1 - a.1)
        }
    }

    /// Local frames per beat at `beat`.
    pub fn slope_at(&self, beat: f64) -> f64 {
        let k = &self.knots;
        let i = k.partition_point(|p| p.0 <= beat);
        if i == 0 {
            self.slope_in
        } else if i >= k.len() {
            self.slope_out
        } else {
            (k[i].1 - k[i - 1].1) / (k[i].0 - k[i - 1].0)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StretchMode {
    #[default]
    RePitch,
    Beats,
    Tones,
    Texture,
    Complex,
    ComplexPro,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoopMode {
    Off,
    #[default]
    Forward,
    BackAndForth,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StretchParams {
    /// Beats: 0..1 segment fade depth (0 = none, Live's envelope 100).
    pub fade: f32,
    pub loop_mode: LoopMode,
    /// Grain length in seconds (Tones / Texture).
    pub grain: f64,
    /// Texture fluctuation 0..1.
    pub flux: f32,
    /// Complex Pro: formants 0..1, envelope lifter (cepstral bins).
    pub formants: f32,
    pub envelope: f32,
}

impl Default for StretchParams {
    fn default() -> Self {
        Self { fade: 0.0, loop_mode: LoopMode::Forward, grain: 0.06, flux: 0.0, formants: 1.0, envelope: 128.0 }
    }
}

/// Everything a stretch read needs for one clip.
pub struct StretchCtx<'a> {
    pub sample: &'a SampleBuffer,
    pub map: &'a WarpMap,
    pub params: StretchParams,
    /// Beats segment starts (content beats, sorted).
    pub segs: &'a [f64],
    /// Pitch ratio (2^(semitones/12)).
    pub rate: f64,
    /// Seconds per beat at the current tempo.
    pub spb: f64,
    /// Linear (false) or cubic (true) interpolation.
    pub hiq: bool,
}

#[inline]
fn read(s: &SampleBuffer, pos: f64, hiq: bool) -> (f32, f32) {
    if hiq {
        return s.read(pos);
    }
    let len = s.len();
    if !(pos >= 0.0 && pos < len as f64 - 1.0) {
        return s.read(pos);
    }
    let i = pos as usize;
    let t = (pos - i as f64) as f32;
    (s.left[i] + (s.left[i + 1] - s.left[i]) * t, s.right[i] + (s.right[i + 1] - s.right[i]) * t)
}

/// Deterministic hash → 0..1 (grain jitter).
#[inline]
fn hash01(k: i64, salt: u64) -> f64 {
    let mut x = (k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ salt;
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    (x >> 11) as f64 / (1u64 << 53) as f64
}

impl StretchCtx<'_> {
    /// Re-Pitch: the map alone sets the read position (speed changes pitch).
    #[inline]
    pub fn repitch(&self, content: f64) -> (f32, f32) {
        read(self.sample, self.map.frame_at(content), self.hiq)
    }

    /// Beats: every segment starts at its own sample position and plays at
    /// the original speed (× pitch); gaps loop / ping-pong / stay silent.
    /// `u` = unwrapped play position (for crossfades) — content is enough here.
    pub fn beats(&self, content: f64) -> (f32, f32) {
        let i = self.segs.partition_point(|&b| b <= content);
        if i == 0 {
            return self.repitch(content);
        }
        let (x, g) = self.seg_read(i - 1, content);
        // 3 ms crossfade from the previous segment's continuation.
        let xf = 0.003 / self.spb;
        let since = content - self.segs[i - 1];
        if since < xf && i >= 2 {
            let w = (since / xf) as f32;
            let (y, gy) = self.seg_read(i - 2, content);
            return ((x.0 * g) * w + y.0 * gy * (1.0 - w), (x.1 * g) * w + y.1 * gy * (1.0 - w));
        }
        (x.0 * g, x.1 * g)
    }

    fn seg_read(&self, j: usize, content: f64) -> ((f32, f32), f32) {
        let t0 = self.segs[j];
        let t1 = self.segs.get(j + 1).copied().unwrap_or(f64::INFINITY);
        let f0 = self.map.frame_at(t0);
        let f1 = if t1.is_finite() { self.map.frame_at(t1) } else { self.sample.len() as f64 };
        let seg_len = (f1 - f0).max(1.0);
        let elapsed = (content - t0) * self.spb * self.sample.sample_rate as f64 * self.rate;
        let mut g = 1.0f32;
        if self.params.fade > 0.0 && t1.is_finite() {
            let k = ((content - t0) / (t1 - t0)).clamp(0.0, 1.0) as f32;
            g = 1.0 - self.params.fade * k;
        }
        let pos = if elapsed < seg_len {
            f0 + elapsed
        } else {
            let mid = f0 + seg_len * 0.5;
            let lp = (f1 - mid).max(1.0);
            let over = elapsed - seg_len;
            match self.params.loop_mode {
                LoopMode::Off => return ((0.0, 0.0), 0.0),
                LoopMode::Forward => mid + over.rem_euclid(lp),
                LoopMode::BackAndForth => {
                    let ph = over.rem_euclid(2.0 * lp);
                    if ph < lp { f1 - ph } else { mid + (ph - lp) }
                }
            }
        };
        // Short fade at the segment end so loops / cuts don't click.
        let tail = (f1 - pos).min(pos - f0).max(0.0);
        let edge = (tail / 64.0).min(1.0) as f32;
        (read(self.sample, pos, self.hiq), g * if elapsed < seg_len { 1.0 } else { edge })
    }

    /// Tones / Texture: 50 % overlapping Hann grains anchored on the map,
    /// each read at the pitch rate. `u` = unwrapped play position in beats
    /// (continuous time), `wrap` maps it to content beats.
    pub fn granular(&self, u: f64, wrap: impl Fn(f64) -> f64) -> (f32, f32) {
        let sr = self.sample.sample_rate as f64;
        let grain_b = (self.params.grain / self.spb).max(1e-4); // grain length in beats
        let hop = grain_b * 0.5;
        let k = (u / hop).floor() as i64;
        let flux = self.params.flux as f64;
        let mut l = 0.0;
        let mut r = 0.0;
        for g in [k - 1, k] {
            let start = g as f64 * hop;
            let ph = (u - start) / grain_b;
            if !(0.0..1.0).contains(&ph) {
                continue;
            }
            let w = (0.5 - 0.5 * (std::f64::consts::TAU * ph).cos()) as f32;
            let jitter = if flux > 0.0 { (hash01(g, 0xA5) - 0.5) * flux * self.params.grain * sr } else { 0.0 };
            let anchor = self.map.frame_at(wrap(start)) + jitter;
            let pos = anchor + (u - start) * self.spb * sr * self.rate;
            let (x, y) = read(self.sample, pos, self.hiq);
            l += x * w;
            r += y * w;
        }
        (l, r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_round_trips_and_extrapolates() {
        let m = WarpMap::from_markers(&[(0.0, 0.0), (4.0, 96_000.0), (8.0, 144_000.0)], 0.0, 24_000.0);
        assert_eq!(m.frame_at(2.0), 48_000.0);
        assert_eq!(m.frame_at(6.0), 120_000.0);
        assert_eq!(m.frame_at(10.0), 168_000.0); // last segment's slope
        assert_eq!(m.frame_at(-1.0), -24_000.0);
        for b in [-1.0, 0.5, 3.9, 4.0, 7.25, 12.0] {
            assert!((m.beat_at(m.frame_at(b)) - b).abs() < 1e-9);
        }
        let l = WarpMap::linear(100.0, 10.0);
        assert_eq!(l.frame_at(3.0), 130.0);
        assert_eq!(l.slope_at(5.0), 10.0);
    }

    fn ramp(n: usize) -> SampleBuffer {
        SampleBuffer::mono("r", 48_000, (0..n).map(|i| i as f32).collect())
    }

    #[test]
    fn beats_segments_restart_at_their_transient() {
        let s = ramp(96_000);
        // 1 beat = 24000 frames in the sample; play at half tempo (spb = 1 s → 48000 frames/beat).
        let map = WarpMap::linear(0.0, 24_000.0);
        let segs = [0.0, 1.0, 2.0];
        let c = StretchCtx {
            sample: &s,
            map: &map,
            params: StretchParams { loop_mode: LoopMode::Off, ..Default::default() },
            segs: &segs,
            rate: 1.0,
            spb: 1.0,
            hiq: true,
        };
        // 0.25 beats into segment 1: 12000 frames of original-speed playback from 24000.
        assert!((c.beats(1.25).0 - 36_000.0).abs() < 1.0);
        // Past the segment's material (0.75 beats = 36000 > 24000): silent with loop Off.
        assert_eq!(c.beats(1.75).0, 0.0);
        let c2 = StretchCtx { params: StretchParams { loop_mode: LoopMode::Forward, ..Default::default() }, ..c };
        let v = c2.beats(1.75).0;
        assert!((36_000.0..48_000.0).contains(&v), "forward loop stays in the segment's second half: {v}");
    }

    #[test]
    fn granular_keeps_pitch_and_follows_map() {
        // 440 Hz sine stretched 2×: grains should keep ~440 Hz.
        let sr = 48_000.0;
        let s =
            SampleBuffer::mono("s", 48_000, (0..96_000).map(|i| (i as f64 / sr * 440.0 * std::f64::consts::TAU).sin() as f32).collect());
        let map = WarpMap::linear(0.0, 24_000.0); // sample tempo 120
        let c = StretchCtx {
            sample: &s,
            map: &map,
            params: StretchParams { grain: 0.05, ..Default::default() },
            segs: &[],
            rate: 1.0,
            spb: 1.0, // 60 BPM: twice as slow
            hiq: true,
        };
        let out: Vec<f32> = (0..48_000).map(|i| c.granular(i as f64 / sr, |u| u).0).collect();
        let zc = out.windows(2).skip(4800).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        // ≈ 0.9 s × 440 Hz rising zero crossings.
        assert!((380..=420).contains(&zc), "zero crossings {zc}");
    }
}
