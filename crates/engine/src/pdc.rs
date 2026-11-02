//! Plug-in delay compensation: per-track / per-chain delay lines that line
//! up paths with different lookahead latency (Live's PDC).

/// Ring length (power of two); the usable delay is half of it.
pub const PDC_LEN: usize = 1 << 14;
const MASK: usize = PDC_LEN - 1;
/// Longest compensated delay (samples).
pub const PDC_MAX: usize = PDC_LEN / 2;

/// Stereo delay line read at any delay up to [`PDC_MAX`].
#[derive(Debug, Clone)]
pub struct PdcLine {
    l: Box<[f32]>,
    r: Box<[f32]>,
    w: usize,
}

impl Default for PdcLine {
    fn default() -> Self {
        Self::new()
    }
}

impl PdcLine {
    /// Control thread (allocates).
    pub fn new() -> Self {
        Self { l: vec![0.0; PDC_LEN].into_boxed_slice(), r: vec![0.0; PDC_LEN].into_boxed_slice(), w: 0 }
    }

    pub fn reset(&mut self) {
        self.l.fill(0.0);
        self.r.fill(0.0);
        self.w = 0;
    }

    /// Append a block.
    pub fn write(&mut self, l: &[f32], r: &[f32]) {
        for (a, b) in l.iter().zip(r) {
            self.l[self.w] = *a;
            self.r[self.w] = *b;
            self.w = (self.w + 1) & MASK;
        }
    }

    /// Append `n` samples of silence.
    pub fn write_silence(&mut self, n: usize) {
        for _ in 0..n {
            self.l[self.w] = 0.0;
            self.r[self.w] = 0.0;
            self.w = (self.w + 1) & MASK;
        }
    }

    /// Sample `j` of the last written block of length `n`, delayed by `d`.
    #[inline]
    pub fn read(&self, n: usize, j: usize, d: usize) -> (f32, f32) {
        let d = d.min(PDC_MAX);
        let i = (self.w + PDC_LEN * 2 - n + j - d) & MASK;
        (self.l[i], self.r[i])
    }

    /// Delay a block in place by `d` samples.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], d: usize) {
        let n = l.len().min(r.len());
        self.write(&l[..n], &r[..n]);
        if d == 0 {
            return;
        }
        for j in 0..n {
            let (a, b) = self.read(n, j, d);
            l[j] = a;
            r[j] = b;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_by_exact_samples() {
        let mut p = PdcLine::new();
        let mut out = Vec::new();
        for b in 0..4 {
            let mut l: Vec<f32> = (0..64).map(|i| (b * 64 + i) as f32).collect();
            let mut r = l.clone();
            p.process(&mut l, &mut r, 100);
            out.extend(l);
        }
        assert_eq!(out[99], 0.0);
        assert_eq!(out[100], 0.0);
        assert_eq!(out[101], 1.0);
        assert_eq!(out[255], 155.0);
    }
}
