//! Drum Buss (Live 12, manual 29.10): Trim, fixed Comp, Drive with Soft
//! (waveshaper) / Medium (limiter) / Hard (clipper + bass boost) types,
//! Crunch (sine distortion on the mid-highs), Damp (low-pass), Transients
//! (attack / sustain shaping above 100 Hz), Boom (resonant low-frequency
//! filter at Freq with Decay, Bass meter, Audition), Output and Dry/Wet.

use crate::devtap::DevTap;
use crate::dsp::{BUTTER_K, Svf, svf_g};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const DB_TRIM: (f32, f32) = (-24.0, 0.0);
pub const DB_DAMP: (f32, f32) = (500.0, 20_000.0);
pub const DB_FREQ: (f32, f32) = (30.0, 90.0);
pub const DB_OUT: (f32, f32) = (-70.0, 0.0);
pub const DB_TYPES: [&str; 3] = ["Soft", "Medium", "Hard"];
/// Tap slots: output peaks L / R, Boom (bass) peak.
pub const TAP_OUT: [usize; 2] = [0, 1];
pub const TAP_BASS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DrumBussParams {
    /// dB.
    pub trim: f32,
    pub comp: bool,
    /// 0..1.
    pub drive: f32,
    /// Index into [`DB_TYPES`].
    pub dtype: u8,
    pub crunch: f32,
    /// Hz.
    pub damp: f32,
    /// −1..1.
    pub transients: f32,
    pub boom: f32,
    /// Hz.
    pub freq: f32,
    pub decay: f32,
    pub audition: bool,
    /// dB (≤ −70 = −inf).
    pub output: f32,
    pub dry_wet: f32,
}

impl Default for DrumBussParams {
    fn default() -> Self {
        Self {
            trim: 0.0,
            comp: false,
            drive: 0.2,
            dtype: 0,
            crunch: 0.0,
            damp: 9200.0,
            transients: 0.0,
            boom: 0.0,
            freq: 50.0,
            decay: 1.0,
            audition: false,
            output: 0.0,
            dry_wet: 1.0,
        }
    }
}

/// Nearest note to `f` (MIDI) and the offset sign label ("G0+", "A1-", "C2").
pub fn force_note(f: f32) -> (i32, String) {
    let m = 69.0 + 12.0 * (f.max(1.0) / 440.0).log2();
    let n = m.round();
    let sign = if m - n > 0.05 {
        "+"
    } else if n - m > 0.05 {
        "-"
    } else {
        ""
    };
    (n as i32, format!("{}{sign}", crate::resonators::note_name(n as i32)))
}

#[derive(Debug, Clone, Default)]
pub struct DrumBussState {
    tap: Option<Arc<DevTap>>,
    env: f32,
    bass: [f32; 2],
    crunch_lp: [f32; 2],
    damp: [[Svf; 2]; 2],
    tr_lp: [f32; 2],
    fast: f32,
    slow: f32,
    boom_lp: [f32; 2],
    boom: [Svf; 2],
}

impl DrumBussState {
    pub fn new(id: Option<u64>) -> Self {
        Self { tap: id.map(crate::devtap::dev_tap), ..Default::default() }
    }

    pub fn reset(&mut self) {
        *self = Self { tap: self.tap.take(), ..Default::default() };
    }

    #[allow(clippy::needless_range_loop)]
    pub fn process(&mut self, p: &DrumBussParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let n = l.len().min(r.len());
        let a = |ms: f32| (-1.0 / (ms * 0.001 * sr)).exp();
        let (c_att, c_rel) = (a(3.0), a(80.0));
        let (f_att, f_rel, s_att, s_rel) = (a(0.5), a(30.0), a(25.0), a(250.0));
        let trim = 10f32.powf(p.trim.clamp(DB_TRIM.0, DB_TRIM.1) / 20.0);
        let drive = p.drive.clamp(0.0, 1.0);
        let pre = 10f32.powf(drive * 30.0 / 20.0);
        let lp1 = |f: f32| 1.0 - (-std::f32::consts::TAU * f / sr).exp();
        let (k_bass, k_crunch, k_tr) = (lp1(100.0), lp1(600.0), lp1(100.0));
        let g_damp = svf_g(p.damp.clamp(DB_DAMP.0, DB_DAMP.1), sr);
        let freq = p.freq.clamp(DB_FREQ.0, DB_FREQ.1);
        let g_boom = svf_g(freq, sr);
        let k_boom = 0.012 + (1.0 - p.decay.clamp(0.0, 1.0)) * 0.6;
        let k_blp = lp1(freq * 2.0);
        let ck = 1.0 + 7.0 * p.crunch;
        let out = if p.output <= DB_OUT.0 { 0.0 } else { 10f32.powf(p.output / 20.0) };
        let wet = p.dry_wet.clamp(0.0, 1.0);
        let (mut pk_l, mut pk_r, mut pk_b) = (0.0f32, 0.0f32, 0.0f32);
        for i in 0..n {
            let dry = [l[i], r[i]];
            let mut x = [dry[0] * trim, dry[1] * trim];
            if p.comp {
                // Fixed: −18 dB threshold, 4:1, +6 dB makeup, stereo-linked.
                let lvl = x[0].abs().max(x[1].abs());
                let c = if lvl > self.env { c_att } else { c_rel };
                self.env = lvl + c * (self.env - lvl);
                let db = 20.0 * self.env.max(1e-6).log10();
                let gr = if db > -18.0 { (db + 18.0) * 0.75 } else { 0.0 };
                let g = 10f32.powf((6.0 - gr) / 20.0);
                x[0] *= g;
                x[1] *= g;
            }
            let mut y = [0.0f32; 2];
            for c in 0..2 {
                let mut v = x[c] * pre;
                v = match p.dtype {
                    0 => v.tanh(),
                    1 => v / (1.0 + v.abs().powf(2.5)).powf(0.4),
                    _ => {
                        self.bass[c] += k_bass * (v - self.bass[c]);
                        (v + self.bass[c]).clamp(-1.0, 1.0)
                    }
                };
                v /= 1.0 + drive;
                if p.crunch > 0.0 {
                    self.crunch_lp[c] += k_crunch * (v - self.crunch_lp[c]);
                    let hp = v - self.crunch_lp[c];
                    v += p.crunch * ((hp * ck * 1.5).sin() * 0.5 - hp * 0.5);
                }
                let (a1, _, _) = self.damp[c][0].tick(v, g_damp, BUTTER_K);
                y[c] = a1;
            }
            // Transients: band above 100 Hz scaled by the attack / sustain ratio.
            if p.transients.abs() > 1e-4 {
                let mut band = [0.0f32; 2];
                for c in 0..2 {
                    self.tr_lp[c] += k_tr * (y[c] - self.tr_lp[c]);
                    band[c] = y[c] - self.tr_lp[c];
                }
                let lvl = band[0].abs().max(band[1].abs());
                let cf = if lvl > self.fast { f_att } else { f_rel };
                self.fast = lvl + cf * (self.fast - lvl);
                let cs = if lvl > self.slow { s_att } else { s_rel };
                self.slow = lvl + cs * (self.slow - lvl);
                let ratio = ((self.fast + 1e-6) / (self.slow + 1e-6)).ln().clamp(-2.0, 2.0);
                let g = (p.transients * ratio * 1.5).exp().clamp(0.25, 4.0);
                for c in 0..2 {
                    y[c] = self.tr_lp[c] + band[c] * g;
                }
            }
            // Boom: resonant band-pass excited by the low end.
            let mut bm = [0.0f32; 2];
            if p.boom > 0.0 || p.audition {
                for c in 0..2 {
                    self.boom_lp[c] += k_blp * (x[c] - self.boom_lp[c]);
                    let (_, bp, _) = self.boom[c].tick(self.boom_lp[c], g_boom, k_boom);
                    bm[c] = (bp * k_boom * 4.0 * p.boom.max(0.05)).tanh();
                }
                pk_b = pk_b.max(bm[0].abs()).max(bm[1].abs());
            }
            let mut o = [0.0f32; 2];
            for c in 0..2 {
                let v = if p.audition { bm[c] } else { y[c] + bm[c] * p.boom };
                o[c] = (dry[c] + (v * out - dry[c]) * wet).clamp(-8.0, 8.0);
            }
            pk_l = pk_l.max(o[0].abs());
            pk_r = pk_r.max(o[1].abs());
            l[i] = o[0];
            r[i] = o[1];
        }
        if let Some(t) = &self.tap {
            t.raise(TAP_OUT[0], pk_l);
            t.raise(TAP_OUT[1], pk_r);
            t.raise(TAP_BASS, pk_b * p.boom.max(0.05).sqrt());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_and_notes() {
        let mut s = DrumBussState::new(None);
        let p = DrumBussParams { comp: true, crunch: 0.5, transients: 0.7, boom: 0.8, dtype: 2, ..Default::default() };
        let mut l: Vec<f32> = (0..4800).map(|i| if i % 1200 < 40 { 0.9 } else { 0.0 }).collect();
        let mut r = l.clone();
        s.process(&p, &mut l, &mut r, 48_000.0);
        assert!(l.iter().all(|x| x.is_finite() && x.abs() <= 8.0));
        assert_eq!(force_note(50.0).1, "G0+");
        assert_eq!(force_note(55.0).1, "A0");
    }
}
