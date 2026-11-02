//! Saturator (Live 12, manual 29.29): Drive → one of eight shaping curves
//! (Waveshaper with Drive/Curve/Depth/Linear/Damp/Period) → Post Clip →
//! Output, with the optional Color EQ (low shelf Amt Lo + bell Amt Hi at
//! Frequency/Width before the shaper, inverted after it), Pre-DC filter and
//! 2× oversampled Hi-Quality mode.

use crate::eq8::{Coefs, Rbj, Tdf2, rbj};
use serde::{Deserialize, Serialize};

pub const SAT_CURVES: [&str; 8] =
    ["Analog Clip", "Soft Sine", "Bass Shaper", "Medium Curve", "Hard Curve", "Sinoid Fold", "Digital Clip", "Waveshaper"];
pub const SAT_CLIPS: [&str; 3] = ["No Clip", "Soft Clip", "Hard Clip"];
pub const SAT_DRIVE: (f32, f32) = (-36.0, 36.0);
pub const SAT_OUT: (f32, f32) = (-36.0, 0.0);
pub const SAT_BASS: (f32, f32) = (-50.0, 0.0);
pub const SAT_FREQ: (f32, f32) = (30.0, 18_000.0);
/// Colour gain at ±100 % (dB).
pub const SAT_COLOR_DB: f32 = 15.0;
/// The Amt Lo shelf sits this factor below Frequency (display handle ①).
pub const SAT_LO_RATIO: f32 = 26.7;
/// Analog-style knee level.
const KNEE: f32 = 0.66;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SaturatorParams {
    pub curve: u8,
    pub drive: f32,
    pub output: f32,
    pub dry_wet: f32,
    pub clip: u8,
    pub color: bool,
    /// −1..1.
    pub amt_lo: f32,
    /// −1..1.
    pub amt_hi: f32,
    pub freq: f32,
    /// 0..1.
    pub width: f32,
    /// Bass Shaper threshold (dB).
    pub bass: f32,
    pub ws_drive: f32,
    pub ws_curve: f32,
    pub ws_depth: f32,
    pub ws_lin: f32,
    pub ws_damp: f32,
    pub ws_period: f32,
    pub hq: bool,
    pub pre_dc: bool,
    /// Extended view (Color Curve + Waveshaper panel).
    pub expanded: bool,
}

impl Default for SaturatorParams {
    fn default() -> Self {
        Self {
            curve: 0,
            drive: 0.0,
            output: 0.0,
            dry_wet: 1.0,
            clip: 0,
            color: true,
            amt_lo: 0.0,
            amt_hi: 0.0,
            freq: 1000.0,
            width: 0.3,
            bass: -50.0,
            ws_drive: 1.0,
            ws_curve: 0.05,
            ws_depth: 0.0,
            ws_lin: 0.5,
            ws_damp: 0.0,
            ws_period: 0.0,
            hq: false,
            pre_dc: false,
            expanded: false,
        }
    }
}

#[inline]
fn knee(x: f32, n: f32) -> f32 {
    x / (1.0 + (x.abs() / KNEE).powf(n)).powf(1.0 / n)
}

impl SaturatorParams {
    /// Shaping curve (input already scaled by Drive), before Post Clip.
    pub fn curve_fn(&self, x: f32) -> f32 {
        match self.curve {
            0 => knee(x, 6.0),
            1 => KNEE * (x / KNEE).clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2).sin(),
            2 => {
                let t = crate::dynfx::db_lin(self.bass.clamp(SAT_BASS.0, SAT_BASS.1)).max(0.003);
                // Lower thresholds push quieter signals into the curve.
                KNEE * (x / (2.0 * t * KNEE)).tanh()
            }
            3 => KNEE * (x / KNEE).tanh(),
            4 => knee(x, 12.0),
            5 => KNEE * (x / KNEE).sin(),
            6 => x.clamp(-1.0, 1.0),
            _ => {
                let d = self.ws_drive.clamp(0.0, 1.0);
                let lin = self.ws_lin.clamp(0.0, 1.0);
                let shaped = x + d * (self.ws_curve.clamp(0.0, 1.0) * x * x * x - (1.0 - lin) * (x - x.tanh()));
                let damp = self.ws_damp.clamp(0.0, 1.0);
                let gate = if damp > 1e-4 { 1.0 - (-x.abs() / (damp * 0.3)).exp() } else { 1.0 };
                let osc = self.ws_depth.clamp(0.0, 1.0)
                    * d
                    * (std::f32::consts::PI * x * (1.0 + 15.0 * self.ws_period.clamp(0.0, 1.0))).sin()
                    * gate;
                (shaped + osc).clamp(-4.0, 4.0)
            }
        }
    }

    #[inline]
    pub fn post_clip(&self, y: f32) -> f32 {
        match self.clip {
            1 => y.tanh(),
            2 => y.clamp(-1.0, 1.0),
            _ => y,
        }
    }

    /// Transfer curve shown in the display (input −1..1).
    pub fn display(&self, x: f32) -> f32 {
        self.post_clip(self.curve_fn(x * crate::dynfx::db_lin(self.drive)))
    }

    fn color_coefs(&self, sr: f32, sign: f32) -> [Coefs; 2] {
        let f = self.freq.clamp(SAT_FREQ.0, SAT_FREQ.1).min(0.45 * sr) as f64;
        let q = 0.4 + 4.6 * (1.0 - self.width.clamp(0.0, 1.0)) as f64;
        [
            rbj(Rbj::LowShelf, f / SAT_LO_RATIO as f64, sr as f64, 0.707, (sign * self.amt_lo * SAT_COLOR_DB) as f64),
            rbj(Rbj::Peak, f, sr as f64, q, (sign * self.amt_hi * SAT_COLOR_DB) as f64),
        ]
    }

    /// Pre-EQ colour response (dB) at `f` — the Color Curve display.
    pub fn color_db(&self, f: f32, sr: f32) -> f32 {
        let w = std::f64::consts::TAU * (f / sr).min(0.4999) as f64;
        let m: f64 = self.color_coefs(sr, 1.0).iter().map(|c| c.mag2(w)).product();
        (10.0 * m.max(1e-30).log10()) as f32
    }
}

#[derive(Debug, Clone, Default)]
pub struct SaturatorState {
    pre: [[Tdf2; 2]; 2],
    post: [[Tdf2; 2]; 2],
    co: [[Coefs; 2]; 2],
    key: Option<(u32, u32, u32, u32, u32)>,
    dc: [(f32, f32); 2],
    last: [f32; 2],
}

impl SaturatorState {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn process(&mut self, p: &SaturatorParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let n = l.len().min(r.len());
        if p.color {
            let key = (p.freq.to_bits(), p.width.to_bits(), p.amt_lo.to_bits(), p.amt_hi.to_bits(), sr.to_bits());
            if self.key != Some(key) {
                self.co = [p.color_coefs(sr, 1.0), p.color_coefs(sr, -1.0)];
                self.key = Some(key);
            }
        }
        let drive = crate::dynfx::db_lin(p.drive.clamp(SAT_DRIVE.0, SAT_DRIVE.1));
        let out = crate::dynfx::db_lin(p.output.clamp(SAT_OUT.0, SAT_OUT.1));
        let wet = p.dry_wet.clamp(0.0, 1.0);
        let dc_a = (-std::f32::consts::TAU * 10.0 / sr).exp();
        for i in 0..n {
            for (c, buf) in [&mut *l, &mut *r].into_iter().enumerate() {
                let dry = buf[i];
                let mut x = dry;
                if p.pre_dc {
                    // One-pole DC blocker (10 Hz).
                    let (px, py) = self.dc[c];
                    let y = x - px + dc_a * py;
                    self.dc[c] = (x, y);
                    x = y;
                }
                if p.color {
                    for k in 0..2 {
                        x = self.pre[c][k].tick(&self.co[0][k], x as f64) as f32;
                    }
                }
                x *= drive;
                let mut y = if p.hq {
                    // 2× oversampling: linear interpolation up, average down.
                    let mid = 0.5 * (self.last[c] + x);
                    self.last[c] = x;
                    0.5 * (p.post_clip(p.curve_fn(mid)) + p.post_clip(p.curve_fn(x)))
                } else {
                    p.post_clip(p.curve_fn(x))
                };
                if p.color {
                    for k in 0..2 {
                        y = self.post[c][k].tick(&self.co[1][k], y as f64) as f32;
                    }
                }
                if !y.is_finite() {
                    y = 0.0;
                    self.reset();
                }
                buf[i] = dry + (y * out - dry) * wet;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_are_odd_and_finite() {
        for c in 0..8u8 {
            let p = SaturatorParams { curve: c, ..Default::default() };
            for i in -20..=20 {
                let x = i as f32 / 5.0;
                let (a, b) = (p.curve_fn(x), p.curve_fn(-x));
                assert!(a.is_finite() && (a + b).abs() < 1e-4, "{c} {x}");
            }
        }
        // Analog Clip display matches the reference (≈0.51 at 0.5, ≈0.65 at 1).
        let p = SaturatorParams::default();
        assert!((p.display(0.5) - 0.49).abs() < 0.04);
        assert!((p.display(1.0) - 0.65).abs() < 0.03);
    }

    #[test]
    fn color_inverse_is_flat_at_low_level() {
        let p = SaturatorParams { amt_lo: 0.5, amt_hi: -0.7, curve: 6, ..Default::default() };
        let mut s = SaturatorState::default();
        let mut l: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.03).sin() * 0.01).collect();
        let mut r = l.clone();
        let src = l.clone();
        s.process(&p, &mut l, &mut r, 48_000.0);
        let err: f32 = l[2000..].iter().zip(&src[2000..]).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(err < 1e-3, "{err}");
        assert!(p.color_db(1000.0, 48_000.0) < -9.0);
    }
}
