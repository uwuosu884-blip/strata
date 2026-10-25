/// Decoded audio held in memory (planar stereo). Shared via `Arc`, immutable.
#[derive(Debug, Clone, PartialEq)]
pub struct SampleBuffer {
    pub name: String,
    pub sample_rate: u32,
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    /// Min/max of the mono mix per `PEAK_BLOCK` frames, for waveform drawing.
    pub peaks: Vec<(f32, f32)>,
}

pub const PEAK_BLOCK: usize = 256;

impl SampleBuffer {
    pub fn new(name: impl Into<String>, sample_rate: u32, left: Vec<f32>, right: Vec<f32>) -> Self {
        assert_eq!(left.len(), right.len(), "channel length mismatch");
        let peaks = left
            .chunks(PEAK_BLOCK)
            .zip(right.chunks(PEAK_BLOCK))
            .map(|(l, r)| {
                l.iter().zip(r).fold((0.0f32, 0.0f32), |(lo, hi), (a, b)| {
                    let m = 0.5 * (a + b);
                    (lo.min(m), hi.max(m))
                })
            })
            .collect();
        Self { name: name.into(), sample_rate, left, right, peaks }
    }

    pub fn mono(name: impl Into<String>, sample_rate: u32, data: Vec<f32>) -> Self {
        Self::new(name, sample_rate, data.clone(), data)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.left.len()
    }

    pub fn is_empty(&self) -> bool {
        self.left.is_empty()
    }

    pub fn duration_secs(&self) -> f64 {
        self.len() as f64 / self.sample_rate as f64
    }

    /// 4-point cubic Hermite interpolation at fractional frame `pos`.
    /// Outside the buffer returns silence.
    #[inline]
    pub fn read(&self, pos: f64) -> (f32, f32) {
        let len = self.len();
        if !(pos > -1.0 && pos < len as f64) {
            return (0.0, 0.0);
        }
        let i = pos.floor() as isize;
        let t = (pos - i as f64) as f32;
        if i >= 1 && (i as usize) + 2 < len {
            let i = i as usize;
            (hermite(&self.left[i - 1..i + 3], t), hermite(&self.right[i - 1..i + 3], t))
        } else {
            let get = |ch: &[f32], k: isize| if k >= 0 && (k as usize) < len { ch[k as usize] } else { 0.0 };
            let l = [get(&self.left, i - 1), get(&self.left, i), get(&self.left, i + 1), get(&self.left, i + 2)];
            let r = [get(&self.right, i - 1), get(&self.right, i), get(&self.right, i + 1), get(&self.right, i + 2)];
            (hermite(&l, t), hermite(&r, t))
        }
    }
}

#[inline(always)]
fn hermite(y: &[f32], t: f32) -> f32 {
    let (ym1, y0, y1, y2) = (y[0], y[1], y[2], y[3]);
    let c1 = 0.5 * (y1 - ym1);
    let c2 = ym1 - 2.5 * y0 + 2.0 * y1 - 0.5 * y2;
    let c3 = 0.5 * (y2 - ym1) + 1.5 * (y0 - y1);
    ((c3 * t + c2) * t + c1) * t + y0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_positions_are_exact_and_outside_is_silent() {
        let s = SampleBuffer::mono("t", 48_000, (0..10).map(|i| i as f32).collect());
        for i in 0..10 {
            assert_eq!(s.read(i as f64).0, i as f32);
        }
        assert_eq!(s.read(-2.0), (0.0, 0.0));
        assert_eq!(s.read(10.0), (0.0, 0.0));
        assert!((s.read(4.5).0 - 4.5).abs() < 1e-6); // linear data stays linear
    }

    #[test]
    fn peaks_cover_buffer() {
        let s = SampleBuffer::mono("t", 48_000, vec![0.5; PEAK_BLOCK * 3 + 1]);
        assert_eq!(s.peaks.len(), 4);
        assert_eq!(s.peaks[0], (0.0, 0.5));
    }
}
