//! Erosion (Live 12.4, manual 29.17): a short delay whose time is modulated
//! by a blend of a sine wave and band-pass filtered noise (centre = Freq,
//! bandwidth = Filter Width), Amount = modulation depth, Stereo Width =
//! mono → independent modulation per channel. The input spectrum feeds the
//! display.

use crate::eq8::{Coefs, Rbj, SpecTap, Tdf2, rbj, spectrum_tap};
use serde::{Deserialize, Serialize};
use std::f32::consts::{FRAC_PI_2, TAU};
use std::sync::Arc;

pub const ERO_FREQ: (f32, f32) = (30.0, 18_000.0);
pub const ERO_WIDTH: (f32, f32) = (0.1, 2.5);
/// Delay-time deviation at 100 % Amount (seconds).
const DEPTH_S: f32 = 0.000_2;
const BUF: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ErosionParams {
    pub freq: f32,
    /// 0..1.
    pub amount: f32,
    /// 0 = sine … 1 = noise.
    pub blend: f32,
    /// Noise filter bandwidth (octaves-like, 0.1..2.5).
    pub width: f32,
    /// 0 = mono … 1 = stereo modulation.
    pub stereo: f32,
}

impl Default for ErosionParams {
    fn default() -> Self {
        Self { freq: 1000.0, amount: 0.3, blend: 0.5, width: 0.8, stereo: 0.5 }
    }
}

#[derive(Debug, Clone)]
pub struct ErosionState {
    buf: Box<[[f32; BUF]; 2]>,
    pos: usize,
    phase: f32,
    rng: u32,
    bp: [Tdf2; 4],
    co: Coefs,
    norm: f32,
    key: Option<(u32, u32, u32)>,
    tap: Option<Arc<SpecTap>>,
}

impl ErosionState {
    pub fn new(id: Option<u64>) -> Self {
        Self {
            buf: Box::new([[0.0; BUF]; 2]),
            pos: 0,
            phase: 0.0,
            rng: 0x1234_5678,
            bp: Default::default(),
            co: Coefs::default(),
            norm: 1.0,
            key: None,
            tap: id.map(spectrum_tap),
        }
    }

    pub fn reset(&mut self) {
        *self.buf = [[0.0; BUF]; 2];
        self.bp = Default::default();
        self.phase = 0.0;
    }

    #[inline]
    fn noise(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    pub fn process(&mut self, p: &ErosionParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let n = l.len().min(r.len());
        if let Some(t) = &self.tap {
            t.write(l, r, n, sr);
        }
        let f = p.freq.clamp(ERO_FREQ.0, ERO_FREQ.1).min(0.45 * sr);
        let w = p.width.clamp(ERO_WIDTH.0, ERO_WIDTH.1);
        let key = (f.to_bits(), w.to_bits(), sr.to_bits());
        if self.key != Some(key) {
            // Width ≈ bandwidth in octaves → Q; two cascaded band-passes.
            let q = 1.0 / (2.0 * (w * 0.5 * std::f32::consts::LN_2).sinh()).max(1e-3);
            self.co = rbj(Rbj::Bp, f as f64, sr as f64, q as f64, 0.0);
            // Unit-RMS output for uniform white noise (σ² = 1/3) through the
            // pair (ENBW ≈ π f / (2 Q) per section, ~0.6 for the pair).
            let enbw = 0.6 * std::f32::consts::PI * f / (2.0 * q);
            self.norm = (1.0 / (enbw / (0.5 * sr) / 3.0).max(1e-9)).sqrt() * 0.5;
            self.key = Some(key);
        }
        let blend = p.blend.clamp(0.0, 1.0);
        let st = p.stereo.clamp(0.0, 1.0);
        let depth = p.amount.clamp(0.0, 1.0) * DEPTH_S * sr;
        let dph = f / sr;
        let mix = |a: f32, b: f32| (1.0 - st) * a + st * b;
        for i in 0..n {
            // Modulators: sine (R phase-shifted by Stereo × 90°) and noise
            // (R crossfaded towards an independent noise).
            let s_l = (TAU * self.phase).sin();
            let s_r = (TAU * self.phase + st * FRAC_PI_2).sin();
            self.phase = (self.phase + dph).fract();
            let (a, b) = (self.noise(), self.noise());
            let mut nl = self.bp[0].tick(&self.co, a as f64);
            nl = self.bp[1].tick(&self.co, nl);
            let mut nr = self.bp[2].tick(&self.co, b as f64);
            nr = self.bp[3].tick(&self.co, nr);
            let (nl, nr) = (nl as f32 * self.norm, nr as f32 * self.norm);
            let m = [(1.0 - blend) * s_l + blend * nl.clamp(-1.5, 1.5), (1.0 - blend) * s_r + blend * mix(nl, nr).clamp(-1.5, 1.5)];
            self.pos = (self.pos + 1) & (BUF - 1);
            for (c, x) in [&mut l[i], &mut r[i]].into_iter().enumerate() {
                let b = &mut self.buf[c];
                b[self.pos] = *x;
                // 1 sample + depth·(1 + m) behind (≥ 1: Hermite needs n+1).
                let d = (1.0 + depth * (1.0 + m[c])).clamp(1.0, (BUF - 4) as f32);
                let di = d as usize;
                let fr = d - di as f32;
                let at = |k: usize| b[(self.pos + BUF - k) & (BUF - 1)];
                let (xm1, x0, x1, x2) = (at(di - 1), at(di), at(di + 1), at(di + 2));
                // Hermite between x0 (d = di) and x1 (d = di + 1).
                let c1 = 0.5 * (x1 - xm1);
                let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
                let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
                *x = ((c3 * fr + c2) * fr + c1) * fr + x0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(p: &ErosionParams, f: f32) -> (Vec<f32>, Vec<f32>) {
        let sr = 48_000.0;
        let mut s = ErosionState::new(None);
        let mut l: Vec<f32> = (0..9600).map(|i| (TAU * f * i as f32 / sr).sin() * 0.5).collect();
        let mut r = l.clone();
        for (a, b) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
            s.process(p, a, b, sr);
        }
        (l, r)
    }

    #[test]
    #[allow(clippy::needless_range_loop)]
    fn zero_amount_is_one_sample_delay() {
        let p = ErosionParams { amount: 0.0, ..Default::default() };
        let (l, _) = run(&p, 440.0);
        let sr = 48_000.0;
        for i in 200..400 {
            let want = (TAU * 440.0 * (i - 1) as f32 / sr).sin() * 0.5;
            assert!((l[i] - want).abs() < 1e-4, "{i}");
        }
    }

    #[test]
    fn modulation_adds_sidebands_and_stereo() {
        let p = ErosionParams { amount: 1.0, blend: 0.0, freq: 3000.0, stereo: 1.0, ..Default::default() };
        let (l, r) = run(&p, 5000.0);
        let dry: Vec<f32> = (0..9600).map(|i| (TAU * 5000.0 * (i as f32 - 1.0) / 48_000.0).sin() * 0.5).collect();
        let err: f32 = l[1000..].iter().zip(&dry[1000..]).map(|(a, b)| (a - b).powi(2)).sum::<f32>();
        assert!(err > 1.0, "{err}");
        let diff: f32 = l[1000..].iter().zip(&r[1000..]).map(|(a, b)| (a - b).abs()).sum();
        assert!(diff > 1.0);
        assert!(l.iter().all(|x| x.is_finite() && x.abs() < 1.0));
        // Noise only: stays bounded.
        let p = ErosionParams { amount: 1.0, blend: 1.0, width: 0.1, ..Default::default() };
        let (l, _) = run(&p, 1000.0);
        assert!(l.iter().all(|x| x.is_finite() && x.abs() < 1.0));
    }
}
