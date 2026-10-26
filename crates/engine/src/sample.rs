/// Decoded audio held in memory (planar stereo). Shared via `Arc`, immutable.
#[derive(Debug, Clone, PartialEq)]
pub struct SampleBuffer {
    pub name: String,
    pub sample_rate: u32,
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    /// Min/max of the mono mix per `PEAK_BLOCK` frames, for waveform drawing.
    pub peaks: Vec<(f32, f32)>,
    /// Detected transients (frames, ascending) — Beats-mode segments and
    /// warp-marker suggestions.
    pub transients: Vec<f64>,
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
        let transients = detect_transients(&left, &right, sample_rate);
        Self { name: name.into(), sample_rate, left, right, peaks, transients }
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

/// Onset detection: energy of the first difference (high-frequency emphasis)
/// per 256-frame hop; an onset is a local rise well above the recent average.
/// Positions are refined to the strongest sample jump within the hop.
pub fn detect_transients(left: &[f32], right: &[f32], sample_rate: u32) -> Vec<f64> {
    const HOP: usize = 256;
    let n = left.len().min(right.len());
    if n < HOP * 2 {
        return if n > 0 { vec![0.0] } else { Vec::new() };
    }
    let hops = n / HOP;
    let mut e = vec![0.0f32; hops];
    let mut prev = 0.0f32;
    for (h, v) in e.iter_mut().enumerate() {
        let mut acc = 0.0;
        for i in h * HOP..(h + 1) * HOP {
            let m = 0.5 * (left[i] + right[i]);
            acc += (m - prev) * (m - prev);
            prev = m;
        }
        *v = acc;
    }
    let peak = e.iter().copied().fold(0.0f32, f32::max);
    let floor = peak * 1e-3 + 1e-9;
    let min_gap = (sample_rate as usize / 20 / HOP).max(1); // 50 ms
    let mut out: Vec<f64> = Vec::new();
    let mut last: Option<usize> = None;
    for h in 0..hops {
        let lo = h.saturating_sub(8);
        let avg = if h > lo { e[lo..h].iter().sum::<f32>() / (h - lo) as f32 } else { 0.0 };
        let next = if h + 1 < hops { e[h + 1] } else { 0.0 };
        let rise = e[h] > floor && e[h] > 3.0 * avg + floor && e[h] >= next * 0.7;
        if rise && last.is_none_or(|l| h - l >= min_gap) {
            // Refine: first frame whose |jump| exceeds half the hop's maximum.
            let s = h.saturating_sub(1) * HOP;
            let mut best = 0.0f32;
            for i in s.max(1)..(h + 1) * HOP {
                best = best.max((left[i] + right[i] - left[i - 1] - right[i - 1]).abs());
            }
            let pos = (s.max(1)..(h + 1) * HOP)
                .find(|&i| (left[i] + right[i] - left[i - 1] - right[i - 1]).abs() >= best * 0.5)
                .unwrap_or(h * HOP);
            out.push(pos as f64);
            last = Some(h);
        }
    }
    if out.first().is_none_or(|&f| f > HOP as f64 * 4.0) {
        out.insert(0, 0.0);
    }
    out
}

#[cfg(test)]
mod transient_tests {
    use super::*;

    #[test]
    fn finds_clicks() {
        let mut d = vec![0.0f32; 48_000];
        for k in [0usize, 12_000, 24_000, 36_000] {
            for i in 0..2000 {
                d[k + i] = (i as f32 * 0.3).sin() * (1.0 - i as f32 / 2000.0);
            }
        }
        let t = detect_transients(&d, &d, 48_000);
        assert_eq!(t.len(), 4, "{t:?}");
        for (a, b) in t.iter().zip([0.0, 12_000.0, 24_000.0, 36_000.0]) {
            assert!((a - b).abs() < 300.0, "{t:?}");
        }
    }
}
