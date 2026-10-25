/// One-pole parameter smoother: moves toward a target exponentially so that
/// parameter changes (volume, pan, cutoff…) never produce zipper noise.
#[derive(Debug, Clone)]
pub struct Smoothed {
    current: f32,
    target: f32,
    coeff: f32,
}

impl Smoothed {
    /// `time_seconds` ≈ time to cover ~63 % of a jump.
    pub fn new(initial: f32, sample_rate: f64, time_seconds: f64) -> Self {
        let coeff = (-1.0 / (time_seconds * sample_rate)).exp() as f32;
        Self { current: initial, target: initial, coeff }
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        let next = self.target + (self.current - self.target) * self.coeff;
        // Snap when close enough, or when f32 rounding stalls progress (near the
        // target each step can be smaller than one ULP, so `next == current`).
        self.current = if next == self.current || (next - self.target).abs() < 1.0e-6 {
            self.target
        } else {
            next
        };
        self.current
    }
}

impl Smoothed {
    /// Advance `n` samples without producing output (exact for a one-pole).
    pub fn skip(&mut self, n: usize) {
        if self.current == self.target || n == 0 {
            return;
        }
        let next = self.target + (self.current - self.target) * self.coeff.powi(n.min(i32::MAX as usize) as i32);
        self.current = if (next - self.target).abs() < 1.0e-6 { self.target } else { next };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converges_to_target_without_overshoot() {
        let mut s = Smoothed::new(0.0, 48_000.0, 0.01);
        s.set_target(1.0);
        let mut prev = 0.0;
        for _ in 0..48_000 {
            let v = s.next();
            assert!(v >= prev && v <= 1.0);
            prev = v;
        }
        assert_eq!(prev, 1.0);
    }
}
