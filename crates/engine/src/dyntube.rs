//! Dynamic Tube (Live 12, manual 29.13): tube saturation with three models
//! (A: clean below a Bias-dependent threshold, C: always distorting, B in
//! between), Drive into the tube, Tone (spectral tilt of the distortion
//! products), Bias pushed by an envelope follower (Envelope ±, Attack,
//! Release), Output and Dry/Wet. Hi-Quality (context menu) runs the tube
//! 2× oversampled.

use crate::devtap::DevTap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const DT_DRIVE: (f32, f32) = (-24.0, 24.0);
pub const DT_OUT: (f32, f32) = (-36.0, 36.0);
/// Envelope amount (−3..3 = ±300 %).
pub const DT_ENV: f32 = 3.0;
pub const DT_ATTACK: (f32, f32) = (1.0, 500.0);
pub const DT_RELEASE: (f32, f32) = (1.0, 2000.0);
pub const DT_TUBES: [&str; 3] = ["A", "B", "C"];
/// Tap slots: output peaks L / R.
pub const TAP_OUT: [usize; 2] = [0, 1];

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DynTubeParams {
    /// dB.
    pub drive: f32,
    /// −1..1.
    pub tone: f32,
    /// 0..1.
    pub bias: f32,
    /// Index into [`DT_TUBES`].
    pub tube: u8,
    /// −3..3.
    pub envelope: f32,
    /// ms.
    pub attack: f32,
    pub release: f32,
    /// dB.
    pub output: f32,
    pub dry_wet: f32,
    pub hq: bool,
}

impl Default for DynTubeParams {
    fn default() -> Self {
        Self { drive: 0.0, tone: 0.0, bias: 0.0, tube: 0, envelope: 0.0, attack: 5.0, release: 35.0, output: 0.0, dry_wet: 1.0, hq: false }
    }
}

/// Static tube transfer for model `tube` at bias `b` (0..1): odd-symmetric
/// core plus a bias offset (asymmetry → even harmonics). Exposed for the
/// display.
#[inline]
pub fn tube_fn(tube: u8, u: f32, b: f32) -> f32 {
    let b = b.clamp(0.0, 1.0);
    match tube {
        0 => {
            // A: linear below the threshold, soft-limited above it.
            let th = 1.0 - 0.85 * b;
            let o = 0.25 * b;
            let f = |v: f32| {
                let a = v.abs();
                if a <= th {
                    v
                } else {
                    let e = a - th;
                    v.signum() * (th + e / (1.0 + e / (1.15 - th)))
                }
            };
            f(u + o) - f(o)
        }
        1 => {
            let k = 1.0 + 2.5 * b;
            let o = 0.15 + 0.45 * b;
            ((u + o) * k).tanh() / k - (o * k).tanh() / k
        }
        _ => {
            // C: asymmetric exponential, distorts at every level.
            let o = 0.3 + 0.6 * b;
            let f = |v: f32| if v >= 0.0 { (1.0 - (-1.6 * v).exp()) / 1.6 } else { (v - 0.35 * v * v).max(-1.4) };
            f(u + o) - f(o)
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DynTubeState {
    tap: Option<Arc<DevTap>>,
    env: f32,
    /// Tone split (one-pole low part of the distortion products).
    lp: [f32; 2],
    /// DC blocker (x1, y1) per channel.
    dc: [(f32, f32); 2],
    /// Last input (2× oversampling interpolation) and half-band state.
    last: [f32; 2],
    down: [f32; 2],
}

impl DynTubeState {
    pub fn new(id: Option<u64>) -> Self {
        Self { tap: id.map(crate::devtap::dev_tap), ..Default::default() }
    }

    pub fn reset(&mut self) {
        *self = Self { tap: self.tap.take(), ..Default::default() };
    }

    #[allow(clippy::needless_range_loop)]
    pub fn process(&mut self, p: &DynTubeParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let n = l.len().min(r.len());
        let a = |ms: f32| (-1.0 / (ms * 0.001 * sr)).exp();
        let c_att = a(p.attack.clamp(DT_ATTACK.0, DT_ATTACK.1));
        let c_rel = a(p.release.clamp(DT_RELEASE.0, DT_RELEASE.1));
        let g = 10f32.powf(p.drive.clamp(DT_DRIVE.0, DT_DRIVE.1) / 20.0);
        let out = 10f32.powf(p.output.clamp(DT_OUT.0, DT_OUT.1) / 20.0);
        let wet = p.dry_wet.clamp(0.0, 1.0);
        let k_tone = 1.0 - (-std::f32::consts::TAU * 1200.0 / sr).exp();
        let tone = p.tone.clamp(-1.0, 1.0);
        let (w_lo, w_hi) = (1.0 - tone, 1.0 + tone);
        let r_dc = 1.0 - std::f32::consts::TAU * 10.0 / sr;
        let env_amt = p.envelope.clamp(-DT_ENV, DT_ENV);
        let (mut pk_l, mut pk_r) = (0.0f32, 0.0f32);
        for i in 0..n {
            let dry = [l[i], r[i]];
            if env_amt != 0.0 {
                let lvl = dry[0].abs().max(dry[1].abs());
                let c = if lvl > self.env { c_att } else { c_rel };
                self.env = lvl + c * (self.env - lvl);
            }
            let b = (p.bias + env_amt * self.env.min(2.0) * 0.5).clamp(0.0, 1.0);
            let mut o = [0.0f32; 2];
            for c in 0..2 {
                let u = dry[c] * g;
                let y = if p.hq {
                    // 2×: midpoint + sample, averaged (simple half-band).
                    let m = tube_fn(p.tube, 0.5 * (self.last[c] + u), b);
                    let s = tube_fn(p.tube, u, b);
                    self.last[c] = u;
                    let v = 0.25 * self.down[c] + 0.5 * m + 0.25 * s;
                    self.down[c] = s;
                    v
                } else {
                    tube_fn(p.tube, u, b)
                };
                // Tone: tilt the distortion products around 1.2 kHz.
                let d = y - u;
                self.lp[c] += k_tone * (d - self.lp[c]);
                let d = self.lp[c] * w_lo + (d - self.lp[c]) * w_hi;
                let v = u + d;
                // DC blocker (bias offset leaves DC on asymmetric curves).
                let (x1, y1) = self.dc[c];
                let yv = v - x1 + r_dc * y1;
                self.dc[c] = (v, if yv.is_finite() { yv } else { 0.0 });
                o[c] = (dry[c] + (yv * out - dry[c]) * wet).clamp(-8.0, 8.0);
            }
            pk_l = pk_l.max(o[0].abs());
            pk_r = pk_r.max(o[1].abs());
            l[i] = o[0];
            r[i] = o[1];
        }
        if let Some(t) = &self.tap {
            t.raise(TAP_OUT[0], pk_l);
            t.raise(TAP_OUT[1], pk_r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thd(p: &DynTubeParams, amp: f32) -> f32 {
        let mut s = DynTubeState::new(None);
        let sr = 48_000.0;
        let mut l: Vec<f32> = (0..9600).map(|i| (i as f32 * std::f32::consts::TAU * 1000.0 / sr).sin() * amp).collect();
        let mut r = l.clone();
        s.process(p, &mut l, &mut r, sr);
        // Residual after removing the fundamental (least squares on the tail).
        let t = &l[4800..];
        let (mut ss, mut cc) = (0.0f32, 0.0f32);
        for (i, v) in t.iter().enumerate() {
            let ph = (i + 4800) as f32 * std::f32::consts::TAU * 1000.0 / sr;
            ss += v * ph.sin();
            cc += v * ph.cos();
        }
        let (ss, cc) = (ss * 2.0 / t.len() as f32, cc * 2.0 / t.len() as f32);
        let res: f32 = t
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let ph = (i + 4800) as f32 * std::f32::consts::TAU * 1000.0 / sr;
                (v - ss * ph.sin() - cc * ph.cos()).powi(2)
            })
            .sum();
        (res / t.len() as f32).sqrt() / amp
    }

    #[test]
    fn models_and_bias() {
        // A is clean at low bias and level; C always distorts.
        let a = DynTubeParams::default();
        assert!(thd(&a, 0.3) < 1e-3);
        let c = DynTubeParams { tube: 2, ..Default::default() };
        assert!(thd(&c, 0.3) > 0.01);
        // More bias → more distortion on A.
        let a_b = DynTubeParams { bias: 0.9, ..Default::default() };
        assert!(thd(&a_b, 0.3) > thd(&a, 0.3) + 0.005);
        // Envelope pushes bias with the level.
        let a_e = DynTubeParams { envelope: 3.0, ..Default::default() };
        assert!(thd(&a_e, 0.5) > thd(&a, 0.5));
        let hq = DynTubeParams { tube: 1, bias: 0.5, drive: 12.0, hq: true, ..Default::default() };
        assert!(thd(&hq, 0.5).is_finite());
    }
}
