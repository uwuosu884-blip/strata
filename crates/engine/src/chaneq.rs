//! Channel EQ (Live manual 29.7): HP 80 Hz switch, adaptive low shelf at
//! 100 Hz (±15 dB), sweepable mid peak 120 Hz–7.5 kHz (±12 dB), high shelf
//! (+15 dB) that adds a low-pass (20 → 8 kHz) when cutting, output gain.

use crate::dynfx::db_lin;
use crate::eq8::{Coefs, Rbj, SpecTap, Tdf2, rbj, spectrum_tap};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const CEQ_SHELF_DB: f32 = 15.0;
pub const CEQ_MID_DB: f32 = 12.0;
pub const CEQ_MID_HZ: (f32, f32) = (120.0, 7500.0);
pub const CEQ_OUT_DB: f32 = 12.0;
const HP_HZ: f64 = 80.0;
const LOW_HZ: f64 = 100.0;
const HIGH_HZ: f64 = 5000.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelEqParams {
    pub hp: bool,
    pub low: f32,
    pub mid: f32,
    pub mid_freq: f32,
    pub high: f32,
    pub output: f32,
}

impl Default for ChannelEqParams {
    fn default() -> Self {
        Self { hp: false, low: 0.0, mid: 0.0, mid_freq: 1500.0, high: 0.0, output: 0.0 }
    }
}

impl ChannelEqParams {
    /// The five sections (HP, low shelf, mid peak, high shelf, LP); `None`
    /// where a section is bypassed.
    pub fn sections(&self, sr: f32) -> [Option<Coefs>; 5] {
        let sr = sr as f64;
        let low = self.low.clamp(-CEQ_SHELF_DB, CEQ_SHELF_DB) as f64;
        let mid = self.mid.clamp(-CEQ_MID_DB, CEQ_MID_DB) as f64;
        let high = self.high.clamp(-CEQ_SHELF_DB, CEQ_SHELF_DB) as f64;
        let fm = self.mid_freq.clamp(CEQ_MID_HZ.0, CEQ_MID_HZ.1) as f64;
        // Adaptive curves: steeper / narrower with more gain (console style).
        let ql = 0.55 + 0.25 * low.abs() / 15.0;
        let qm = 0.6 + 0.5 * mid.abs() / 12.0;
        let qh = 0.5 + 0.2 * high.abs() / 15.0;
        [
            self.hp.then(|| rbj(Rbj::Hp, HP_HZ, sr, 0.6, 0.0)),
            (low != 0.0).then(|| rbj(Rbj::LowShelf, LOW_HZ, sr, ql, low)),
            (mid != 0.0).then(|| rbj(Rbj::Peak, fm, sr, qm, mid)),
            (high != 0.0).then(|| rbj(Rbj::HighShelf, HIGH_HZ, sr, qh, high)),
            (high < 0.0).then(|| rbj(Rbj::Lp, self.lp_hz() as f64, sr, std::f64::consts::FRAC_1_SQRT_2, 0.0)),
        ]
    }

    /// Low-pass cutoff when High is cut: 20 kHz at 0 dB → 8 kHz at −15 dB.
    pub fn lp_hz(&self) -> f32 {
        let u = (-self.high / CEQ_SHELF_DB).clamp(0.0, 1.0);
        20_000.0 * 0.4f32.powf(u)
    }

    /// Filter response (dB, without Output) at `f`.
    pub fn response_db(&self, f: f32, sr: f32) -> f32 {
        let w = std::f64::consts::TAU * (f as f64 / sr as f64).min(0.4999);
        let m: f64 = self.sections(sr).iter().flatten().map(|c| c.mag2(w)).product();
        (10.0 * m.max(1e-30).log10()) as f32
    }
}

#[derive(Debug, Clone)]
pub struct ChannelEqState {
    st: [[Tdf2; 5]; 2],
    co: [Option<Coefs>; 5],
    key: Option<(ChannelEqParams, u32)>,
    g: f32,
    tap: Option<Arc<SpecTap>>,
}

impl ChannelEqState {
    pub fn new(id: Option<u64>) -> Self {
        Self { st: Default::default(), co: [None; 5], key: None, g: f32::NAN, tap: id.map(spectrum_tap) }
    }

    pub fn reset(&mut self) {
        self.st = Default::default();
        self.g = f32::NAN;
    }

    pub fn process(&mut self, p: &ChannelEqParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let key = (*p, sr.to_bits());
        if self.key != Some(key) {
            let co = p.sections(sr);
            // A section switching in or out starts from a clean state.
            for (i, c) in co.iter().enumerate() {
                if c.is_some() != self.co[i].is_some() {
                    self.st[0][i] = Tdf2::default();
                    self.st[1][i] = Tdf2::default();
                }
            }
            self.co = co;
            self.key = Some(key);
        }
        let n = l.len().min(r.len());
        let tg = db_lin(p.output.clamp(-CEQ_OUT_DB, CEQ_OUT_DB));
        let g0 = if self.g.is_finite() { self.g } else { tg };
        let inv = 1.0 / n.max(1) as f32;
        for (c, buf) in [&mut *l, &mut *r].into_iter().enumerate() {
            let st = &mut self.st[c];
            for (i, x) in buf[..n].iter_mut().enumerate() {
                let mut v = *x as f64;
                for (s, co) in st.iter_mut().zip(self.co.iter()) {
                    if let Some(co) = co {
                        v = s.tick(co, v);
                    }
                }
                let g = g0 + (tg - g0) * (i + 1) as f32 * inv;
                *x = v as f32 * g;
            }
        }
        self.g = tg;
        if let Some(t) = &self.tap {
            t.write(l, r, n, sr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_shapes() {
        let sr = 48_000.0;
        let flat = ChannelEqParams::default();
        for f in [30.0, 100.0, 1000.0, 10_000.0] {
            assert!(flat.response_db(f, sr).abs() < 1e-3);
        }
        let p = ChannelEqParams { low: 15.0, ..flat };
        assert!(p.response_db(30.0, sr) > 12.0 && p.response_db(5000.0, sr).abs() < 0.5);
        let p = ChannelEqParams { mid: 12.0, mid_freq: 1000.0, ..flat };
        assert!((p.response_db(1000.0, sr) - 12.0).abs() < 0.1);
        let p = ChannelEqParams { high: -15.0, ..flat };
        assert!((p.lp_hz() - 8000.0).abs() < 1.0);
        assert!(p.response_db(16_000.0, sr) < -20.0 && p.response_db(200.0, sr).abs() < 0.5);
        let p = ChannelEqParams { hp: true, ..flat };
        assert!(p.response_db(20.0, sr) < -18.0 && p.response_db(1000.0, sr).abs() < 0.1);
    }

    #[test]
    fn processes_gain() {
        let p = ChannelEqParams { output: -6.0, ..Default::default() };
        let mut s = ChannelEqState::new(None);
        let (mut l, mut r) = (vec![1.0f32; 64], vec![1.0f32; 64]);
        s.process(&p, &mut l, &mut r, 48_000.0);
        assert!((l[63] - db_lin(-6.0)).abs() < 1e-4 && (r[10] - db_lin(-6.0)).abs() < 1e-4);
    }
}
