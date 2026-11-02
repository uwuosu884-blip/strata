//! Redux (Live 12, manual 29.27): sample-rate reduction with clock Jitter,
//! optional anti-alias Pre filter and reconstruction Post filter (offset in
//! octaves from half the Rate), bit reduction with a companding Shape and a
//! mid-rise (DC Shift) quantiser, Dry/Wet.

use crate::dsp::{BUTTER_K, Rng, Svf, svf_g};
use serde::{Deserialize, Serialize};

pub const RDX_RATE: (f32, f32) = (200.0, 40_000.0);
pub const RDX_OCT: f32 = 4.0;
pub const RDX_BITS: (f32, f32) = (1.0, 16.0);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReduxParams {
    /// Sample rate (Hz).
    pub rate: f32,
    /// 0..1 clock randomisation.
    pub jitter: f32,
    pub pre: bool,
    pub post: bool,
    /// Post filter cutoff offset in octaves relative to Rate/2.
    pub post_oct: f32,
    /// Bit depth (continuous, 16 = off).
    pub bits: f32,
    /// 0..1 quantiser companding.
    pub shape: f32,
    pub dc: bool,
    pub dry_wet: f32,
}

impl Default for ReduxParams {
    fn default() -> Self {
        Self { rate: 40_000.0, jitter: 0.0, pre: false, post: false, post_oct: 0.0, bits: 16.0, shape: 0.0, dc: false, dry_wet: 1.0 }
    }
}

impl ReduxParams {
    pub fn rate_label(&self) -> String {
        let r = self.rate;
        if r >= 10_000.0 {
            format!("{:.1} kHz", r / 1000.0)
        } else if r >= 1000.0 {
            format!("{:.2} kHz", r / 1000.0)
        } else {
            format!("{} Hz", r.round())
        }
    }

    pub fn bits_label(&self) -> String {
        let b = self.bits;
        if (b - b.round()).abs() < 0.005 { format!("{}", b.round()) } else { format!("{b:.2}") }
    }
}

/// Quantise `x` (−1..1 nominal) to `bits` with companding `shape` and
/// optional half-step DC shift (mid-rise).
#[inline]
pub fn quantise(x: f32, bits: f32, shape: f32, dc: bool) -> f32 {
    let levels = 2f32.powf(bits.clamp(RDX_BITS.0, RDX_BITS.1) - 1.0);
    let e = 1.0 + 3.0 * shape.clamp(0.0, 1.0);
    let c = x.signum() * x.abs().powf(1.0 / e);
    let q = if dc { ((c * levels).floor() + 0.5) / levels } else { (c * levels).round() / levels };
    q.signum() * q.abs().powf(e)
}

#[derive(Debug, Clone)]
pub struct ReduxState {
    hold: [f32; 2],
    phase: f32,
    pre: [[Svf; 2]; 2],
    post: [[Svf; 2]; 2],
    rng: Rng,
}

impl Default for ReduxState {
    fn default() -> Self {
        Self { hold: [0.0; 2], phase: 1.0, pre: Default::default(), post: Default::default(), rng: Rng(0x9e37_79b9) }
    }
}

impl ReduxState {
    pub fn reset(&mut self) {
        *self = Self { rng: self.rng, ..Default::default() };
    }

    #[allow(clippy::needless_range_loop)]
    pub fn process(&mut self, p: &ReduxParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let n = l.len().min(r.len());
        let rate = p.rate.clamp(RDX_RATE.0, RDX_RATE.1);
        let step = rate / sr;
        let g_pre = svf_g(0.5 * rate.min(sr), sr);
        let g_post = svf_g((0.5 * rate * 2f32.powf(p.post_oct.clamp(-RDX_OCT, RDX_OCT))).clamp(20.0, 0.49 * sr), sr);
        let jit = p.jitter.clamp(0.0, 1.0) * 0.9;
        let wet = p.dry_wet.clamp(0.0, 1.0);
        let bypass_q = p.bits >= RDX_BITS.1 - 1e-3 && p.shape <= 1e-4 && !p.dc;
        for i in 0..n {
            let mut x = [l[i], r[i]];
            if p.pre {
                for (c, v) in x.iter_mut().enumerate() {
                    let (a, _, _) = self.pre[c][0].tick(*v, g_pre, BUTTER_K);
                    *v = self.pre[c][1].tick(a, g_pre, BUTTER_K).0;
                }
            }
            self.phase += step;
            if self.phase >= 1.0 {
                // Next clock tick arrives late by up to `jit` of a period.
                self.phase = (self.phase - 1.0 - jit * self.rng.uni()).max(-1.0);
                for c in 0..2 {
                    self.hold[c] = if bypass_q { x[c] } else { quantise(x[c], p.bits, p.shape, p.dc) };
                }
            }
            let mut y = self.hold;
            if p.post {
                for (c, v) in y.iter_mut().enumerate() {
                    let (a, _, _) = self.post[c][0].tick(*v, g_post, BUTTER_K);
                    *v = self.post[c][1].tick(a, g_post, BUTTER_K).0;
                }
            }
            l[i] += (y[0] - l[i]) * wet;
            r[i] += (y[1] - r[i]) * wet;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_at_defaults() {
        let mut s = ReduxState::default();
        let mut l: Vec<f32> = (0..256).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let mut r = l.clone();
        let src = l.clone();
        s.process(&ReduxParams::default(), &mut l, &mut r, 40_000.0);
        for (a, b) in l.iter().zip(&src) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn reduces() {
        assert_eq!(quantise(0.3, 1.0, 0.0, false), 0.0);
        assert_eq!(quantise(0.3, 1.0, 0.0, true), 0.5);
        assert!((quantise(0.26, 3.0, 0.0, false) - 0.25).abs() < 1e-6);
        let p = ReduxParams { rate: 1000.0, ..Default::default() };
        let mut s = ReduxState::default();
        let mut l: Vec<f32> = (0..480).map(|i| i as f32 / 480.0).collect();
        let mut r = l.clone();
        s.process(&p, &mut l, &mut r, 48_000.0);
        // Held for 48 samples.
        assert_eq!(l[100], l[120]);
        assert!(l.iter().all(|x| x.is_finite()));
    }
}
