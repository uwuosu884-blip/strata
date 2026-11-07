//! Phaser-Flanger (Live 12, manual 29.29): Phaser (modulated first-order
//! all-pass cascade with feedback; Notches / Center / Spread / Blend),
//! Flanger (modulated short delay with feedback, unipolar: modulation only
//! shortens Time) and Doubler (bipolar modulated delay). Main LFO (10
//! waveforms, Duty Cycle, Phase / Spin, synced Offset), a triangle LFO2
//! mixed in, an envelope follower, Safe Bass high-pass, Feedback (Ø
//! invert), Output, Warmth and Dry/Wet.

use crate::devtap::DevTap;
use crate::dsp::Rng;
use crate::modfx::SYNC_DIVS;
use crate::shifter::lfo_wave;
use serde::{Deserialize, Serialize};
use std::f32::consts::{PI, TAU};
use std::sync::Arc;

pub const PF_MODES: [&str; 3] = ["Phaser", "Flanger", "Doubler"];
pub use crate::shifter::SH_WAVES as PF_WAVES;
pub const PF_NOTCH_MAX: u8 = 42;
pub const PF_CENTER: (f32, f32) = (30.0, 18_000.0);
pub const PF_FL_TIME: (f32, f32) = (0.3, 20.0);
pub const PF_DB_TIME: (f32, f32) = (1.0, 100.0);
pub const PF_RATE: (f32, f32) = (0.01, 40.0);
pub const PF_OUT: (f32, f32) = (-24.0, 24.0);
pub const PF_ENV_T: (f32, f32) = (0.1, 3000.0);
pub const PF_SAFE: (f32, f32) = (5.0, 3000.0);
/// Octaves of centre / spread modulation at 100 % Amount.
pub const PF_OCT: f32 = 4.0;
/// Tap slots: LFO phase L / R, combined modulation L / R (−1..1).
pub const TAP_PH: [usize; 2] = [0, 1];
pub const TAP_MOD: [usize; 2] = [2, 3];
const MAX_AP: usize = 2 * PF_NOTCH_MAX as usize;
const DLEN: usize = 1 << 14;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhaserFlangerParams {
    /// Index into [`PF_MODES`].
    pub mode: u8,
    pub notches: u8,
    pub center: f32,
    /// 0..1.
    pub spread: f32,
    /// 0 = modulate Center .. 1 = modulate Spread.
    pub blend: f32,
    /// ms.
    pub fl_time: f32,
    pub db_time: f32,
    pub lfo_sync: bool,
    pub rate: f32,
    pub div: u8,
    /// Index into [`PF_WAVES`].
    pub wave: u8,
    /// Spin (else Phase) stereo mode.
    pub spin_mode: bool,
    /// Phase 0..1 (×360°).
    pub phase: f32,
    /// Spin 0..1.
    pub spin: f32,
    /// Synced start offset 0..1 (×360°).
    pub offset: f32,
    /// −1..1.
    pub duty: f32,
    pub lfo2_mix: f32,
    pub lfo2_sync: bool,
    pub lfo2_rate: f32,
    pub lfo2_div: u8,
    pub amount: f32,
    /// 0..1.
    pub feedback: f32,
    pub fb_invert: bool,
    pub env_on: bool,
    pub env_amt: f32,
    pub env_attack: f32,
    pub env_release: f32,
    pub safe_bass: f32,
    pub output: f32,
    pub warmth: f32,
    pub dry_wet: f32,
    /// Unfolded (LFO 2 / Env Fol / Safe Bass panel).
    pub open: bool,
}

impl Default for PhaserFlangerParams {
    fn default() -> Self {
        Self {
            mode: 0,
            notches: 2,
            center: 400.0,
            spread: 0.35,
            blend: 0.0,
            fl_time: 20.0,
            db_time: 80.0,
            lfo_sync: false,
            rate: 0.15,
            div: 4,
            wave: 1,
            spin_mode: false,
            phase: 0.0,
            spin: 0.0,
            offset: 0.0,
            duty: 0.0,
            lfo2_mix: 0.0,
            lfo2_sync: false,
            lfo2_rate: 0.5,
            lfo2_div: 4,
            amount: 0.75,
            feedback: 0.0,
            fb_invert: false,
            env_on: false,
            env_amt: 0.0,
            env_attack: 1.0,
            env_release: 120.0,
            safe_bass: 5.0,
            output: 0.0,
            warmth: 0.0,
            dry_wet: 1.0,
            open: false,
        }
    }
}

impl PhaserFlangerParams {
    /// Break frequencies of the all-pass stages for modulation `m` (−1..1).
    pub fn stage_freqs(&self, m: f32, out: &mut [f32; MAX_AP]) -> usize {
        let n = (2 * self.notches.clamp(1, PF_NOTCH_MAX) as usize).min(MAX_AP);
        let mc = m * self.amount.clamp(0.0, 1.0) * (1.0 - self.blend.clamp(0.0, 1.0));
        let ms = m * self.amount.clamp(0.0, 1.0) * self.blend.clamp(0.0, 1.0);
        let fc = self.center.clamp(PF_CENTER.0, PF_CENTER.1) * 2f32.powf(mc * PF_OCT * 0.5);
        // Spread: octave spacing between neighbouring stages.
        let sp = (self.spread.clamp(0.0, 1.0) * (1.0 + ms)).clamp(0.0, 2.0) * 0.6;
        for (i, o) in out.iter_mut().enumerate().take(n) {
            let k = i as f32 - (n as f32 - 1.0) * 0.5;
            *o = (fc * 2f32.powf(k * sp)).clamp(10.0, 21_000.0);
        }
        n
    }

    /// Notch frequencies (display) for modulation `m`: total phase −(2k+1)π.
    pub fn notch_freqs(&self, m: f32, out: &mut [f32; PF_NOTCH_MAX as usize]) -> usize {
        let mut f = [0.0; MAX_AP];
        let n = self.stage_freqs(m, &mut f);
        let phase = |x: f32| -> f32 { f[..n].iter().map(|fi| 2.0 * (x / fi).atan()).sum() };
        let notches = n / 2;
        for (k, o) in out.iter_mut().enumerate().take(notches) {
            let target = (2 * k + 1) as f32 * PI;
            let (mut lo, mut hi) = (1.0f32.ln(), 40_000.0f32.ln());
            for _ in 0..40 {
                let mid = 0.5 * (lo + hi);
                if phase(mid.exp()) < target {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            *o = (0.5 * (lo + hi)).exp();
        }
        notches
    }
}

/// Analog-style triangle: a low-passed rectangle (shape depends on rate).
fn tri_analog(state: &mut f32, ph: f32, duty: f32, rate: f32, sr: f32) -> f32 {
    let sq = if lfo_wave(7, ph, duty, (0.0, 0.0)) > 0.0 { 1.0 } else { -1.0 };
    let k = 1.0 - (-TAU * 2.5 / sr).exp();
    *state += k * (sq - *state);
    // Normalise roughly to the steady amplitude at this rate.
    let amp = (2.5 / rate.max(0.01)).clamp(0.05, 1.0);
    (*state / amp.max(0.2)).clamp(-1.0, 1.0)
}

#[derive(Debug, Clone)]
pub struct PhaserFlangerState {
    tap: Option<Arc<DevTap>>,
    ap: [[f32; MAX_AP]; 2],
    fb: [f32; 2],
    dl: [Box<[f32]>; 2],
    w: usize,
    lfo: [f32; 2],
    lfo2: f32,
    rnd: [(f32, f32); 2],
    rng: Rng,
    analog: [f32; 2],
    env: f32,
    bass: [f32; 2],
    warm: [f32; 2],
    smooth_d: [f32; 2],
}

impl Default for PhaserFlangerState {
    fn default() -> Self {
        Self::new(None)
    }
}

impl PhaserFlangerState {
    pub fn new(id: Option<u64>) -> Self {
        let z = || vec![0.0f32; DLEN].into_boxed_slice();
        Self::build(id.map(crate::devtap::dev_tap), [z(), z()])
    }

    fn build(tap: Option<Arc<DevTap>>, dl: [Box<[f32]>; 2]) -> Self {
        Self {
            tap,
            ap: [[0.0; MAX_AP]; 2],
            fb: [0.0; 2],
            dl,
            w: 0,
            lfo: [0.0; 2],
            lfo2: 0.0,
            rnd: [(0.0, 0.0); 2],
            rng: Rng(0x5eed_f1a6),
            analog: [0.0; 2],
            env: 0.0,
            bass: [0.0; 2],
            warm: [0.0; 2],
            smooth_d: [0.0; 2],
        }
    }

    pub fn set_id(&mut self, id: u64) {
        self.tap = Some(crate::devtap::dev_tap(id));
    }

    pub fn reset(&mut self) {
        let mut dl = std::mem::take(&mut self.dl);
        for b in dl.iter_mut() {
            b.fill(0.0);
        }
        *self = Self::build(self.tap.take(), dl);
    }

    #[allow(clippy::needless_range_loop)]
    pub fn process(&mut self, p: &PhaserFlangerParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let n = l.len().min(r.len());
        let (tempo, _) = crate::modfx::clock();
        let beat_hz = tempo.max(1.0) as f32 / 60.0;
        let div_hz = |d: u8| beat_hz / SYNC_DIVS[(d as usize).min(SYNC_DIVS.len() - 1)].0 as f32;
        let rate = if p.lfo_sync { div_hz(p.div) } else { p.rate.clamp(PF_RATE.0, PF_RATE.1) };
        let rate2 = if p.lfo2_sync { div_hz(p.lfo2_div) } else { p.lfo2_rate.clamp(PF_RATE.0, PF_RATE.1) };
        let rnd = p.wave >= 8;
        let spin = !p.lfo_sync && p.spin_mode;
        let rates = if spin && !rnd { [rate, rate * (1.0 + p.spin.clamp(0.0, 1.0))] } else { [rate, rate] };
        let ph_off = if !spin { p.phase.clamp(0.0, 1.0) } else { 0.0 };
        let off = if p.lfo_sync { p.offset.clamp(0.0, 1.0) } else { 0.0 };
        let mix2 = p.lfo2_mix.clamp(0.0, 1.0);
        let ea = (-1.0 / (p.env_attack.clamp(PF_ENV_T.0, PF_ENV_T.1) * 0.001 * sr)).exp();
        let er = (-1.0 / (p.env_release.clamp(PF_ENV_T.0, PF_ENV_T.1) * 0.001 * sr)).exp();
        let env_amt = if p.env_on { p.env_amt.clamp(-1.0, 1.0) } else { 0.0 };
        let fbg = p.feedback.clamp(0.0, 0.97) * if p.fb_invert { -1.0 } else { 1.0 };
        let kb = 1.0 - (-TAU * p.safe_bass.clamp(PF_SAFE.0, PF_SAFE.1) / sr).exp();
        let warmth = p.warmth.clamp(0.0, 1.0);
        let kw = 1.0 - (-TAU * (20_000.0 - 14_000.0 * warmth) / sr).exp();
        let g_out = 10f32.powf(p.output.clamp(PF_OUT.0, PF_OUT.1) / 20.0);
        let wet = p.dry_wet.clamp(0.0, 1.0);
        let mode = p.mode.min(2);
        let amt = p.amount.clamp(0.0, 1.0);
        let t_ms = if mode == 1 { p.fl_time.clamp(PF_FL_TIME.0, PF_FL_TIME.1) } else { p.db_time.clamp(PF_DB_TIME.0, PF_DB_TIME.1) };
        let ks = 1.0 - (-TAU * 30.0 / sr).exp();
        let mut coefs = [[0.0f32; MAX_AP]; 2];
        let mut nst = 0;
        let mut m = [0.0f32; 2];
        for i in 0..n {
            let dry = [l[i], r[i]];
            let lvl = dry[0].abs().max(dry[1].abs());
            let c = if lvl > self.env { ea } else { er };
            self.env = lvl + c * (self.env - lvl);
            for ch in 0..2 {
                let prev = self.lfo[ch];
                self.lfo[ch] = (self.lfo[ch] + rates[ch] / sr).rem_euclid(1.0);
                if rnd && self.lfo[ch] < prev {
                    self.rnd[ch] = (self.rnd[ch].1, self.rng.bi());
                }
            }
            self.lfo2 = (self.lfo2 + rate2 / sr).rem_euclid(1.0);
            if i & 15 == 0 {
                let l2 = lfo_wave(1, self.lfo2, 0.0, (0.0, 0.0));
                for ch in 0..2 {
                    let ph = self.lfo[ch] + off + if ch == 1 { ph_off } else { 0.0 };
                    let v = if p.wave == 2 {
                        tri_analog(&mut self.analog[ch], ph.rem_euclid(1.0), p.duty, rates[ch], sr / 16.0)
                    } else if rnd && ch == 1 && !spin {
                        // Random waves: Phase sets the stereo width.
                        self.rnd[0].0 + (self.rnd[1].0 - self.rnd[0].0) * p.phase
                    } else {
                        lfo_wave(p.wave, ph, p.duty, self.rnd[ch])
                    };
                    m[ch] = ((v * (1.0 - mix2) + l2 * mix2) + env_amt * self.env.min(1.0) * 2.0).clamp(-1.0, 1.0);
                }
                if mode == 0 {
                    for ch in 0..2 {
                        let mut f = [0.0; MAX_AP];
                        nst = p.stage_freqs(m[ch], &mut f);
                        for k in 0..nst {
                            let t = (PI * (f[k] / sr).min(0.49)).tan();
                            coefs[ch][k] = (t - 1.0) / (t + 1.0);
                        }
                    }
                }
            }
            let mut out = [0.0f32; 2];
            for ch in 0..2 {
                // Safe Bass: only the part above the corner is processed.
                self.bass[ch] += kb * (dry[ch] - self.bass[ch]);
                let lo = self.bass[ch];
                let x = dry[ch] - lo;
                let y = match mode {
                    0 => {
                        let mut s = x + fbg * self.fb[ch];
                        for k in 0..nst {
                            let a = coefs[ch][k];
                            let st = &mut self.ap[ch][k];
                            let y = a * s + *st;
                            *st = s - a * y;
                            s = y;
                        }
                        self.fb[ch] = if s.is_finite() { s } else { 0.0 };
                        0.5 * (x + s)
                    }
                    _ => {
                        let u = if mode == 1 { 1.0 - amt * 0.5 * (m[ch] + 1.0) * 0.98 } else { 1.0 + amt * m[ch] * 0.5 };
                        let target = (t_ms * u * 0.001 * sr).clamp(1.0, (DLEN - 8) as f32);
                        self.smooth_d[ch] += ks * (target - self.smooth_d[ch]);
                        let d = crate::dsp::read(&self.dl[ch], self.w, self.smooth_d[ch], true);
                        let v = x + fbg * d;
                        self.dl[ch][self.w] = if v.is_finite() { v.clamp(-4.0, 4.0) } else { 0.0 };
                        0.5 * (x + d)
                    }
                };
                // Warmth: gentle saturation and a falling low-pass.
                let y = if warmth > 0.0 { ((1.0 + 2.0 * warmth) * y).tanh() / (1.0 + 2.0 * warmth).tanh() } else { y };
                self.warm[ch] += kw * (y - self.warm[ch]);
                let y = (self.warm[ch] + lo) * g_out;
                out[ch] = (dry[ch] + (y - dry[ch]) * wet).clamp(-8.0, 8.0);
            }
            self.w = (self.w + 1) & (DLEN - 1);
            l[i] = out[0];
            r[i] = out[1];
        }
        if let Some(t) = &self.tap {
            t.set(TAP_PH[0], self.lfo[0]);
            t.set(TAP_PH[1], self.lfo[1]);
            t.set(TAP_MOD[0], m[0]);
            t.set(TAP_MOD[1], m[1]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(p: &PhaserFlangerParams, x: &[f32]) -> Vec<f32> {
        let mut s = PhaserFlangerState::new(None);
        let mut l = x.to_vec();
        let mut r = x.to_vec();
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            s.process(p, a, b, 48_000.0);
        }
        l
    }

    #[test]
    fn notches_and_modes() {
        let p = PhaserFlangerParams { notches: 3, amount: 0.0, ..Default::default() };
        let mut out = [0.0; PF_NOTCH_MAX as usize];
        let n = p.notch_freqs(0.0, &mut out);
        assert_eq!(n, 3);
        assert!(out[0] < out[1] && out[1] < out[2]);
        // A sine at a notch frequency is strongly attenuated (no modulation).
        let sr = 48_000.0;
        let f = out[1];
        let x: Vec<f32> = (0..48_000).map(|i| (i as f32 * TAU * f / sr).sin() * 0.5).collect();
        let y = run(&p, &x);
        let peak = y[24_000..].iter().fold(0.0f32, |a, v| a.max(v.abs()));
        assert!(peak < 0.05, "{peak}");
        for mode in 0..3 {
            for w in 0..10 {
                let p = PhaserFlangerParams { mode, wave: w, feedback: 0.9, fb_invert: w % 2 == 0, warmth: 0.5, ..Default::default() };
                let y = run(&p, &x[..8192]);
                assert!(y.iter().all(|v| v.is_finite() && v.abs() <= 8.0));
            }
        }
    }
}
