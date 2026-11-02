//! Vinyl Distortion (Live 12, manual 29.41): Tracing Model (even harmonics
//! of a band at Freq / bandwidth B), Pinch Effect (odd harmonics, 180° out
//! of phase between the channels in Stereo), global Drive, Soft (dub plate)
//! / Hard (standard record) mode, and a Crackle generator (Density, Volume).

use crate::dsp::{Rng, Svf, svf_g};
use serde::{Deserialize, Serialize};

pub const VD_FREQ: (f32, f32) = (50.0, 18_000.0);
/// Bandwidth B (octaves, log).
pub const VD_B: (f32, f32) = (0.1, 3.0);
/// Crackle density (events / s scale) and volume.
pub const VD_DENSITY: (f32, f32) = (0.0, 50.0);
pub const VD_VOLUME: (f32, f32) = (0.0, 3.0);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VinylParams {
    pub tr_on: bool,
    /// 0..1.
    pub tr_drive: f32,
    /// Hz.
    pub tr_freq: f32,
    pub tr_b: f32,
    pub pi_on: bool,
    pub pi_drive: f32,
    pub pi_freq: f32,
    pub pi_b: f32,
    /// Soft (else Hard).
    pub soft: bool,
    /// Stereo pinch (else Mono).
    pub stereo: bool,
    pub volume: f32,
    pub density: f32,
    /// Global drive 0..1.
    pub drive: f32,
}

impl Default for VinylParams {
    fn default() -> Self {
        Self {
            tr_on: true,
            tr_drive: 0.0,
            tr_freq: 684.0,
            tr_b: 0.32,
            pi_on: true,
            pi_drive: 0.0,
            pi_freq: 7500.0,
            pi_b: 3.0,
            soft: true,
            stereo: true,
            volume: 0.0,
            density: 10.0,
            drive: 1.0,
        }
    }
}

/// SVF damping (1/Q) for bandwidth `b` (fitted to Live's response display:
/// B 0.32 → 0.21, B 3.0 → 2.84).
pub fn b_k(b: f32) -> f32 {
    1.32 * (0.5 * b.clamp(VD_B.0, VD_B.1)).sinh()
}

/// Display response (0..1) of a band at `f` for centre `fc` / bandwidth `b`
/// (the bilinear 2-pole at 48 kHz, as Live draws it: it falls towards
/// Nyquist).
pub fn band_mag(f: f32, fc: f32, b: f32) -> f32 {
    let k = b_k(b);
    let wt = |x: f32| (std::f32::consts::PI * x.clamp(1.0, 23_900.0) / 48_000.0).tan();
    let w = wt(f) / wt(fc);
    k * w / ((1.0 - w * w).powi(2) + (k * w).powi(2)).sqrt()
}

#[derive(Debug, Clone)]
pub struct VinylState {
    tr: [Svf; 2],
    pi: [Svf; 2],
    dc: [(f32, f32); 2],
    rng: Rng,
    crackle: [f32; 2],
    ck_lp: [Svf; 2],
}

impl Default for VinylState {
    fn default() -> Self {
        Self {
            tr: Default::default(),
            pi: Default::default(),
            dc: Default::default(),
            rng: Rng(0x5eed_1234),
            crackle: [0.0; 2],
            ck_lp: Default::default(),
        }
    }
}

impl VinylState {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    #[allow(clippy::needless_range_loop)]
    pub fn process(&mut self, p: &VinylParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let n = l.len().min(r.len());
        let gd = p.drive.clamp(0.0, 1.0);
        let (gt, kt) = (svf_g(p.tr_freq.clamp(VD_FREQ.0, VD_FREQ.1), sr), b_k(p.tr_b));
        let (gp, kp) = (svf_g(p.pi_freq.clamp(VD_FREQ.0, VD_FREQ.1), sr), b_k(p.pi_b));
        let dt = if p.tr_on { p.tr_drive.clamp(0.0, 1.0) * gd * 6.0 } else { 0.0 };
        let dp = if p.pi_on { p.pi_drive.clamp(0.0, 1.0) * gd * 6.0 } else { 0.0 };
        let hard = !p.soft;
        let r_dc = 1.0 - std::f32::consts::TAU * 8.0 / sr;
        // Crackle: Poisson clicks, density ~ clicks per second × 20.
        let rate = p.density.clamp(VD_DENSITY.0, VD_DENSITY.1) * 20.0 / sr;
        let vol = p.volume.clamp(VD_VOLUME.0, VD_VOLUME.1) * 0.1;
        let g_ck = svf_g(3500.0, sr);
        for i in 0..n {
            let x = [l[i], r[i]];
            let mut y = x;
            // Pinch in Mono: one shared distortion term on the mid signal.
            let mid = 0.5 * (x[0] + x[1]);
            for c in 0..2 {
                if dt > 0.0 {
                    let (_, bp, _) = self.tr[c].tick(x[c], gt, kt);
                    let e = bp * dt;
                    // Even harmonics: squared band (Hard → sharper, rectified).
                    let h = if hard { e.abs() * e.abs().min(1.0) } else { e * e / (1.0 + e * e) };
                    y[c] += h * 0.5;
                }
                if dp > 0.0 {
                    let src = if p.stereo { x[c] } else { mid };
                    let (_, bp, _) = self.pi[c].tick(src, gp, kp);
                    let e = bp * dp;
                    // Odd harmonics; the right channel 180° out of phase.
                    let h = if hard { e - (e * 1.5).clamp(-1.0, 1.0) / 1.5 } else { e - (e * 0.8).tanh() / 0.8 };
                    let sgn = if p.stereo && c == 1 { -1.0 } else { 1.0 };
                    y[c] += sgn * h * 0.7;
                }
                let (x1, y1) = self.dc[c];
                let v = y[c] - x1 + r_dc * y1;
                self.dc[c] = (y[c], if v.is_finite() { v } else { 0.0 });
                y[c] = v;
            }
            if vol > 0.0 && rate > 0.0 {
                for c in 0..2 {
                    if self.rng.uni() < rate {
                        self.crackle[c] = self.rng.bi() * (0.3 + 0.7 * self.rng.uni());
                    }
                    let (_, bp, _) = self.ck_lp[c].tick(self.crackle[c], g_ck, 0.9);
                    self.crackle[c] *= 0.6;
                    y[c] += bp * vol * 4.0;
                }
            }
            l[i] = y[0].clamp(-8.0, 8.0);
            r[i] = y[1].clamp(-8.0, 8.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distorts_and_crackles() {
        let sr = 48_000.0;
        let sine = |f: f32| -> Vec<f32> { (0..4800).map(|i| (i as f32 * std::f32::consts::TAU * f / sr).sin() * 0.5).collect() };
        // Defaults (drives 0, volume 0): transparent apart from the DC blocker.
        let mut s = VinylState::default();
        let (mut l, mut r) = (sine(700.0), sine(700.0));
        s.process(&VinylParams::default(), &mut l, &mut r, sr);
        let src = sine(700.0);
        let diff = l[2400..].iter().zip(&src[2400..]).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(diff < 0.02, "{diff}");
        // Pinch in stereo: the channels differ.
        let p = VinylParams { pi_drive: 1.0, tr_drive: 1.0, ..Default::default() };
        let mut s = VinylState::default();
        let (mut l, mut r) = (sine(7000.0), sine(7000.0));
        s.process(&p, &mut l, &mut r, sr);
        assert!(l.iter().zip(&r).any(|(a, b)| (a - b).abs() > 0.01));
        // Crackle on silence.
        let p = VinylParams { volume: 2.0, density: 30.0, ..Default::default() };
        let mut s = VinylState::default();
        let (mut l, mut r) = (vec![0.0; 4800], vec![0.0; 4800]);
        s.process(&p, &mut l, &mut r, sr);
        assert!(l.iter().any(|v| v.abs() > 1e-3) && l.iter().all(|v| v.is_finite()));
        assert!((band_mag(684.0, 684.0, 0.32) - 1.0).abs() < 1e-4);
    }
}
