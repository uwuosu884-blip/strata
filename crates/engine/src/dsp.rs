//! Small shared DSP helpers for the v0.8.8 devices (Redux, Saturator, Delay,
//! Grain Delay).

/// Topology-preserving state-variable filter (Simper).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Svf {
    s1: f32,
    s2: f32,
}

impl Svf {
    /// Returns (low-pass, band-pass, high-pass).
    #[inline]
    pub(crate) fn tick(&mut self, x: f32, g: f32, k: f32) -> (f32, f32, f32) {
        let hp = (x - (k + g) * self.s1 - self.s2) / (1.0 + g * (k + g));
        let bp = g * hp + self.s1;
        self.s1 = g * hp + bp;
        let lp = g * bp + self.s2;
        self.s2 = g * bp + lp;
        if !self.s1.is_finite() || !self.s2.is_finite() {
            *self = Self::default();
        }
        (lp, bp, hp)
    }
}

#[inline]
pub(crate) fn svf_g(f: f32, sr: f32) -> f32 {
    (std::f32::consts::PI * (f / sr).clamp(1e-6, 0.49)).tan()
}

/// Butterworth damping for the SVF (k = 1/Q).
pub(crate) const BUTTER_K: f32 = std::f32::consts::SQRT_2;

/// xorshift32.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Rng(pub u32);

impl Rng {
    #[inline]
    pub(crate) fn next_u(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }

    /// Uniform 0..1.
    #[inline]
    pub(crate) fn uni(&mut self) -> f32 {
        (self.next_u() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Uniform −1..1.
    #[inline]
    pub(crate) fn bi(&mut self) -> f32 {
        self.uni() * 2.0 - 1.0
    }
}

/// Read a circular buffer `d` samples behind `w` (cubic Hermite when `hq`,
/// linear otherwise). `d` ≥ 1.
#[inline]
pub(crate) fn read(buf: &[f32], w: usize, d: f32, hq: bool) -> f32 {
    let n = buf.len();
    let d = d.clamp(1.0, (n - 4) as f32);
    let i = d.floor();
    let t = d - i;
    let at = |k: isize| buf[(w as isize - i as isize - k).rem_euclid(n as isize) as usize];
    // Positions w-i (t=0) … w-i-1 (t=1).
    let (x0, x1) = (at(0), at(1));
    if !hq {
        return x0 + (x1 - x0) * t;
    }
    let (xm, x2) = (at(-1), at(2));
    let c1 = 0.5 * (x1 - xm);
    let c2 = xm - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
    let c3 = 0.5 * (x2 - xm) + 1.5 * (x0 - x1);
    ((c3 * t + c2) * t + c1) * t + x0
}

/// Shared LFO shapes of Delay (manual 29.9): Sine, Triangle, Ramp Up,
/// Ramp Down, Square, S&H (`held`), Wander (`held` smoothed by the caller).
/// `morph` skews the cycle (sine/triangle/ramps) or sets the pulse width.
pub const LFO_WAVES: [&str; 7] = ["Sine", "Triangle", "Ramp Up", "Ramp Down", "Square", "S&H", "Wander"];

pub fn lfo_wave(w: u8, ph: f32, morph: f32, held: f32) -> f32 {
    use std::f32::consts::TAU;
    let m = morph.clamp(0.0, 1.0);
    // Skewed phase: the first half of the cycle takes 0.5 + 0.49·m.
    let k = 0.5 + 0.49 * m;
    let sk = if ph < k { 0.5 * ph / k } else { 0.5 + 0.5 * (ph - k) / (1.0 - k) };
    match w {
        0 => (TAU * sk).sin(),
        1 => 1.0 - 4.0 * (sk - 0.25).rem_euclid(1.0).min(1.0 - (sk - 0.25).rem_euclid(1.0)),
        2 => 2.0 * sk - 1.0,
        3 => 1.0 - 2.0 * sk,
        4 => {
            if ph < k {
                1.0
            } else {
                -1.0
            }
        }
        _ => held,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_integer_delay() {
        let mut b = vec![0.0; 64];
        for (i, x) in b.iter_mut().enumerate() {
            *x = i as f32;
        }
        assert_eq!(read(&b, 40, 3.0, false), 37.0);
        assert!((read(&b, 40, 3.5, true) - 36.5).abs() < 1e-4);
    }

    #[test]
    fn waves_bounded() {
        for w in 0..5 {
            for i in 0..100 {
                let v = lfo_wave(w, i as f32 / 100.0, (i % 7) as f32 / 6.0, 0.0);
                assert!((-1.001..=1.001).contains(&v), "{w} {v}");
            }
        }
        assert!((lfo_wave(1, 0.25, 0.0, 0.0) - 1.0).abs() < 1e-4);
    }
}
