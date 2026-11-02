//! Reverb (Live manual 29.32): input low/high cut (centre + width), predelay,
//! early reflections (multi-tap, Spin modulation, Shape), a diffusion
//! network (allpass diffusers into a feedback delay network with in-loop
//! high / low shelving decay), Chorus on the network, Size (smoothed per the
//! Smooth chooser), Decay (RT60), Freeze / Flat / Cut, Stereo 0–120°,
//! Density (network size), Reflect / Diffuse levels, Dry/Wet.

use crate::dynfx::db_lin;
use crate::eq8::{Coefs, Tdf2};
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;

pub const RV_IN_FREQ: (f32, f32) = (50.0, 18_000.0);
pub const RV_IN_WIDTH: (f32, f32) = (0.5, 9.0);
pub const RV_SPIN_AMT: (f32, f32) = (0.0, 55.0);
pub const RV_SPIN_RATE: (f32, f32) = (0.07, 1.3);
pub const RV_HI_FREQ: (f32, f32) = (20.0, 16_000.0);
pub const RV_LO_FREQ: (f32, f32) = (20.0, 15_000.0);
pub const RV_SHELF_GAIN: (f32, f32) = (0.2, 1.0);
pub const RV_SCALE: (f32, f32) = (0.05, 1.0);
pub const RV_CHO_AMT: (f32, f32) = (0.01, 4.0);
pub const RV_CHO_RATE: (f32, f32) = (0.01, 8.0);
pub const RV_PREDELAY: (f32, f32) = (0.5, 250.0);
pub const RV_SIZE: (f32, f32) = (0.22, 500.0);
pub const RV_DECAY: (f32, f32) = (0.2, 60.0);
pub const RV_STEREO_MAX: f32 = 120.0;
pub const RV_LEVEL: (f32, f32) = (-30.0, 6.0);
pub const RV_SMOOTH: [&str; 3] = ["None", "Slow", "Fast"];
pub const RV_DENSITY: [&str; 4] = ["Sparse", "Low", "Mid", "High"];

/// Highest sample rate the buffers are sized for.
const SR_CAP: f32 = 192_000.0;
/// Network delay times at Size 100 (ms), spread so no two share factors.
const FDN_MS: [f32; 16] = [31.3, 37.9, 41.1, 47.3, 53.9, 59.1, 61.7, 67.3, 71.9, 73.7, 79.1, 83.3, 89.7, 92.3, 96.1, 97.9];
const ER_MS: [[f32; 8]; 2] = [[4.3, 9.7, 14.9, 21.1, 27.6, 33.9, 41.2, 48.7], [5.1, 11.3, 16.7, 19.9, 29.3, 36.1, 43.9, 47.1]];
const AP_MS: [[f32; 4]; 2] = [[4.77, 3.59, 12.73, 9.30], [5.21, 3.17, 11.89, 8.63]];
/// Size → time factor: (size / 100)^0.6.
const SIZE_EXP: f32 = 0.6;
const SIZE_MAX_F: f32 = 2.64;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReverbParams {
    pub lo_cut: bool,
    pub hi_cut: bool,
    pub in_freq: f32,
    pub in_width: f32,
    pub spin: bool,
    pub spin_amount: f32,
    pub spin_rate: f32,
    pub shape: f32,
    pub hi_on: bool,
    /// High filter as a low-pass instead of a shelf.
    pub hi_lp: bool,
    pub hi_freq: f32,
    pub hi_gain: f32,
    pub lo_on: bool,
    pub lo_freq: f32,
    pub lo_gain: f32,
    pub diffusion: f32,
    pub scale: f32,
    pub chorus: bool,
    pub cho_amount: f32,
    pub cho_rate: f32,
    /// ms.
    pub predelay: f32,
    /// [`RV_SMOOTH`] index.
    pub smooth: u8,
    pub size: f32,
    /// Seconds (RT60).
    pub decay: f32,
    pub freeze: bool,
    pub flat: bool,
    pub cut: bool,
    /// Degrees 0..120.
    pub stereo: f32,
    /// [`RV_DENSITY`] index.
    pub density: u8,
    /// dB.
    pub reflect: f32,
    pub diffuse: f32,
    pub dry_wet: f32,
}

impl Default for ReverbParams {
    fn default() -> Self {
        Self {
            lo_cut: false,
            hi_cut: true,
            in_freq: 830.0,
            in_width: 5.85,
            spin: true,
            spin_amount: 17.5,
            spin_rate: 0.3,
            shape: 0.5,
            hi_on: true,
            hi_lp: false,
            hi_freq: 4500.0,
            hi_gain: 0.7,
            lo_on: false,
            lo_freq: 90.0,
            lo_gain: 0.75,
            diffusion: 0.6,
            scale: 0.4,
            chorus: true,
            cho_amount: 0.02,
            cho_rate: 0.02,
            predelay: 2.5,
            smooth: 2,
            size: 100.0,
            decay: 1.2,
            freeze: false,
            flat: true,
            cut: true,
            stereo: 100.0,
            density: 1,
            reflect: 0.0,
            diffuse: 0.0,
            dry_wet: 0.55,
        }
    }
}

impl ReverbParams {
    /// Input band edges (low cut, high cut) in Hz: one filter alone has its
    /// corner 1.8× beyond Freq (as Live draws it); both span Width octaves around it.
    pub fn band(&self) -> (f32, f32) {
        let f = self.in_freq.clamp(RV_IN_FREQ.0, RV_IN_FREQ.1);
        let h = if self.lo_cut && self.hi_cut { 2f32.powf(self.in_width.clamp(RV_IN_WIDTH.0, RV_IN_WIDTH.1) * 0.5) } else { 1.8 };
        ((f / h).max(10.0), (f * h).min(22_000.0))
    }

    /// Input filter response (dB) at `f` (display).
    pub fn input_db(&self, f: f32, sr: f32) -> f32 {
        let w = std::f64::consts::TAU * (f as f64 / sr as f64).min(0.4999);
        let mut m = 1.0;
        for c in self.input_coefs(sr).iter().flatten() {
            m *= c.mag2(w);
        }
        (10.0 * f64::max(m, 1e-30).log10()) as f32
    }

    /// First-order (6 dB/oct) bilinear low / high cuts.
    fn input_coefs(&self, sr: f32) -> [Option<Coefs>; 2] {
        let (lo, hi) = self.band();
        let ny = 0.45 * sr;
        let g = |f: f32| (std::f64::consts::PI * (f.min(ny) / sr) as f64).tan();
        let hp = |g: f64| Coefs { b: [1.0 / (1.0 + g), -1.0 / (1.0 + g), 0.0], a: [(g - 1.0) / (1.0 + g), 0.0] };
        let lp = |g: f64| Coefs { b: [g / (1.0 + g), g / (1.0 + g), 0.0], a: [(g - 1.0) / (1.0 + g), 0.0] };
        [self.lo_cut.then(|| hp(g(lo))), (self.hi_cut && hi < ny).then(|| lp(g(hi)))]
    }

    /// Diffusion-network decay filter response in dB per RT60 pass (display:
    /// relative decay rate, 0 dB = full Decay).
    pub fn network_db(&self, f: f32) -> f32 {
        let mut db = 0.0;
        if self.hi_on {
            let fc = self.hi_freq.clamp(RV_HI_FREQ.0, RV_HI_FREQ.1);
            let lp = 1.0 / (1.0 + (f / fc).powi(2)).sqrt();
            let g = if self.hi_lp { lp } else { lp + self.hi_gain * (1.0 - lp) };
            db += 20.0 * g.max(1e-4).log10();
        }
        if self.lo_on {
            let fc = self.lo_freq.clamp(RV_LO_FREQ.0, RV_LO_FREQ.1);
            let lp = 1.0 / (1.0 + (f / fc).powi(2)).sqrt();
            db += 20.0 * (self.lo_gain * lp + (1.0 - lp)).max(1e-4).log10();
        }
        db
    }

    fn lines(&self) -> usize {
        [4, 8, 12, 16][(self.density as usize).min(3)]
    }

    fn size_f(&self) -> f32 {
        (self.size.clamp(RV_SIZE.0, RV_SIZE.1) / 100.0).powf(SIZE_EXP)
    }
}

/// Power-of-two circular buffer.
#[derive(Debug, Clone)]
struct Line {
    buf: Vec<f32>,
    mask: usize,
    pos: usize,
}

impl Line {
    fn new(min: usize) -> Self {
        let n = min.max(4).next_power_of_two();
        Self { buf: vec![0.0; n], mask: n - 1, pos: 0 }
    }
    #[inline]
    fn push(&mut self, x: f32) {
        self.pos = (self.pos + 1) & self.mask;
        self.buf[self.pos] = x;
    }
    /// Sample written `d` pushes ago (fractional, linear).
    #[inline]
    fn read(&self, d: f32) -> f32 {
        let d = d.clamp(0.0, (self.mask - 1) as f32);
        let i = d as usize;
        let fr = d - i as f32;
        let a = self.buf[(self.pos + self.buf.len() - i) & self.mask];
        let b = self.buf[(self.pos + self.buf.len() - i - 1) & self.mask];
        a + (b - a) * fr
    }
    #[inline]
    fn at(&self, d: usize) -> f32 {
        self.buf[(self.pos + self.buf.len() - d.min(self.mask)) & self.mask]
    }
    fn clear(&mut self) {
        self.buf.fill(0.0);
    }
}

/// Schroeder allpass with an integer delay.
#[derive(Debug, Clone)]
struct Allpass {
    line: Line,
}

impl Allpass {
    #[inline]
    fn tick(&mut self, x: f32, d: usize, g: f32) -> f32 {
        let z = self.line.at(d.saturating_sub(1));
        let v = x + g * z;
        self.line.push(v);
        z - g * v
    }
}

#[derive(Debug, Clone)]
pub struct ReverbState {
    pre: [Line; 2],
    ap: [[Allpass; 4]; 2],
    fdn: Vec<Line>,
    hi_lp: [f32; 16],
    lo_lp: [f32; 16],
    inf: [[Tdf2; 2]; 2],
    in_co: [Option<Coefs>; 2],
    in_key: Option<(u32, u32, bool, bool, u32)>,
    /// Smoothed size factor.
    size_f: f32,
    spin_ph: f32,
    cho_ph: f32,
    init: bool,
}

impl Default for ReverbState {
    fn default() -> Self {
        Self::new()
    }
}

impl ReverbState {
    /// Allocates every buffer for [`SR_CAP`] (control thread).
    pub fn new() -> Self {
        let s = |ms: f32| (ms * 0.001 * SR_CAP) as usize + 8;
        let er_max = ER_MS.iter().flatten().fold(0.0f32, |a, &b| a.max(b));
        let pre = s(RV_PREDELAY.1 + 2.0 * er_max * SIZE_MAX_F + 2.0);
        let ap_max = AP_MS.iter().flatten().fold(0.0f32, |a, &b| a.max(b));
        let ap = s(ap_max * 2.5 * 1.5);
        let mk_ap = || std::array::from_fn(|_| Allpass { line: Line::new(ap) });
        Self {
            pre: [Line::new(pre), Line::new(pre)],
            ap: [mk_ap(), mk_ap()],
            fdn: FDN_MS.iter().map(|&ms| Line::new(s(ms * SIZE_MAX_F + RV_CHO_AMT.1 + 1.0))).collect(),
            hi_lp: [0.0; 16],
            lo_lp: [0.0; 16],
            inf: Default::default(),
            in_co: [None; 2],
            in_key: None,
            size_f: 1.0,
            spin_ph: 0.0,
            cho_ph: 0.0,
            init: false,
        }
    }

    pub fn reset(&mut self) {
        for l in self.pre.iter_mut().chain(self.fdn.iter_mut()) {
            l.clear();
        }
        for a in self.ap.iter_mut().flatten() {
            a.line.clear();
        }
        self.hi_lp = [0.0; 16];
        self.lo_lp = [0.0; 16];
        self.inf = Default::default();
        self.init = false;
    }

    pub fn process(&mut self, p: &ReverbParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let n = l.len().min(r.len());
        let sr = sr.min(SR_CAP);
        let key = (p.in_freq.to_bits(), p.in_width.to_bits(), p.lo_cut, p.hi_cut, sr.to_bits());
        if self.in_key != Some(key) {
            self.in_co = p.input_coefs(sr);
            self.in_key = Some(key);
        }
        let target = p.size_f();
        if !self.init || p.smooth == 0 {
            self.size_f = target;
            self.init = true;
        }
        let tau = if p.smooth == 1 { 1.0 } else { 0.15 };
        let k_sz = 1.0 - (-1.0 / (tau * sr)).exp();
        let ms = sr * 0.001;
        let nl = p.lines();
        let freeze = p.freeze;
        let shelves = !(freeze && p.flat);
        let feed_in = !(freeze && p.cut);
        let rt = p.decay.clamp(RV_DECAY.0, RV_DECAY.1);
        // One-pole coefficients.
        let pole = |f: f32| (-TAU * f / sr).exp();
        let a_hi = pole(p.hi_freq.clamp(RV_HI_FREQ.0, RV_HI_FREQ.1).min(0.45 * sr));
        let a_lo = pole(p.lo_freq.clamp(RV_LO_FREQ.0, RV_LO_FREQ.1).min(0.45 * sr));
        let hg = p.hi_gain.clamp(RV_SHELF_GAIN.0, RV_SHELF_GAIN.1);
        let lg = p.lo_gain.clamp(RV_SHELF_GAIN.0, RV_SHELF_GAIN.1);
        let ap_g = 0.75 * p.diffusion.clamp(0.0, 1.0);
        let ap_scale = 2.5 * p.scale.clamp(RV_SCALE.0, RV_SCALE.1);
        let spin_d = if p.spin { p.spin_amount.clamp(RV_SPIN_AMT.0, RV_SPIN_AMT.1) * 0.02 * ms } else { 0.0 };
        let spin_inc = p.spin_rate.clamp(RV_SPIN_RATE.0, RV_SPIN_RATE.1) / sr;
        let cho_d = if p.chorus { p.cho_amount.clamp(RV_CHO_AMT.0, RV_CHO_AMT.1) * ms } else { 0.0 };
        let cho_inc = p.cho_rate.clamp(RV_CHO_RATE.0, RV_CHO_RATE.1) / sr;
        let shape = p.shape.clamp(0.0, 1.0);
        let er_g: [f32; 8] = std::array::from_fn(|i| (-(0.5 + 4.0 * shape) * i as f32 / 8.0).exp() * 0.35);
        let pre_d = p.predelay.clamp(RV_PREDELAY.0, RV_PREDELAY.1) * ms;
        let (g_ref, g_dif) = (db_lin(p.reflect.clamp(RV_LEVEL.0, RV_LEVEL.1)), db_lin(p.diffuse.clamp(RV_LEVEL.0, RV_LEVEL.1)));
        let w = p.stereo.clamp(0.0, RV_STEREO_MAX) / RV_STEREO_MAX;
        let wet = p.dry_wet.clamp(0.0, 1.0);
        let out_norm = 1.0 / (nl as f32).sqrt();
        let mut outs = [0.0f32; 16];
        // Per-line loop gains for this block (RT60 and shelf decay ratios).
        let (mut gl, mut ghs, mut gls) = ([1.0f32; 16], [1.0f32; 16], [1.0f32; 16]);
        for j in 0..nl {
            let len = FDN_MS[j * 16 / nl] * self.size_f * ms;
            // Frozen without Flat: the shelves keep their normal decay ratio.
            let g = 10f32.powf(-3.0 * len / (sr * rt));
            gl[j] = if freeze { 1.0 } else { g };
            ghs[j] = g.powf(1.0 / hg - 1.0);
            gls[j] = g.powf(1.0 / lg - 1.0);
        }
        for i in 0..n {
            self.size_f += (target - self.size_f) * k_sz;
            let sf = self.size_f;
            let (dl, dr) = (l[i], r[i]);
            // Input filter → predelay.
            let mut x = [dl, dr];
            for (c, v) in x.iter_mut().enumerate() {
                let mut y = *v as f64;
                for (s, co) in self.inf[c].iter_mut().zip(self.in_co.iter()) {
                    if let Some(co) = co {
                        y = s.tick(co, y);
                    }
                }
                *v = y as f32;
                self.pre[c].push(*v);
            }
            // Early reflections (Spin: each tap's time wobbles).
            self.spin_ph = (self.spin_ph + spin_inc).fract();
            let mut er = [0.0f32; 2];
            for (c, e) in er.iter_mut().enumerate() {
                for (t, &tm) in ER_MS[c].iter().enumerate() {
                    let m = (TAU * (self.spin_ph + 0.125 * t as f32 + 0.37 * c as f32)).sin();
                    *e += er_g[t] * self.pre[c].read(pre_d + tm * sf * ms + spin_d * (1.0 + m));
                }
            }
            // Diffuse onset follows Shape (later with more shape).
            let onset = pre_d + shape * ER_MS[0][7] * sf * ms;
            let mut din = [0.0f32; 2];
            for (c, d) in din.iter_mut().enumerate() {
                let mut v = if feed_in { self.pre[c].read(onset) } else { 0.0 };
                for (k, a) in self.ap[c].iter_mut().enumerate() {
                    let len = (AP_MS[c][k] * ap_scale * sf.clamp(0.1, 1.5) * ms).max(1.0) as usize;
                    v = a.tick(v, len, ap_g);
                }
                *d = v;
            }
            // Feedback delay network (Householder mixing).
            self.cho_ph = (self.cho_ph + cho_inc).fract();
            let mut sum = 0.0;
            for (j, o) in outs[..nl].iter_mut().enumerate() {
                let m = (TAU * (self.cho_ph + j as f32 / nl as f32)).sin();
                let q = j * 16 / nl;
                let len = FDN_MS[q] * sf * ms;
                let mut y = self.fdn[q].read(len + cho_d * (1.0 + m));
                if shelves {
                    if p.hi_on {
                        self.hi_lp[q] = y + a_hi * (self.hi_lp[q] - y);
                        let lp = self.hi_lp[q];
                        y = if p.hi_lp { lp } else { lp + ghs[j] * (y - lp) };
                    }
                    if p.lo_on {
                        self.lo_lp[q] = y + a_lo * (self.lo_lp[q] - y);
                        let lp = self.lo_lp[q];
                        y = gls[j] * lp + (y - lp);
                    }
                    y *= gl[j];
                }
                *o = y;
                sum += y;
            }
            let hh = 2.0 / nl as f32 * sum;
            let (mut ol, mut or) = (0.0, 0.0);
            for (j, &o) in outs[..nl].iter().enumerate() {
                let inj = din[j & 1];
                self.fdn[j * 16 / nl].push(o - hh + inj);
                if j % 4 < 2 {
                    ol += o;
                } else {
                    or += o;
                }
                if j % 3 == 0 {
                    or -= 0.5 * o;
                } else if j % 3 == 1 {
                    ol -= 0.5 * o;
                }
            }
            let dif = [ol * out_norm * g_dif, or * out_norm * g_dif];
            let wl = er[0] * g_ref + dif[0];
            let wr = er[1] * g_ref + dif[1];
            let (m, s) = (0.5 * (wl + wr), 0.5 * (wl - wr) * w);
            l[i] = dl * (1.0 - wet) + (m + s) * wet;
            r[i] = dr * (1.0 - wet) + (m - s) * wet;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn impulse(p: &ReverbParams, secs: f32) -> (Vec<f32>, Vec<f32>) {
        let sr = 48_000.0;
        let n = (secs * sr) as usize;
        let mut st = ReverbState::new();
        let mut l = vec![0.0f32; n];
        l[0] = 1.0;
        let mut r = l.clone();
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            st.process(p, a, b, sr);
        }
        (l, r)
    }

    fn energy(x: &[f32]) -> f32 {
        x.iter().map(|v| v * v).sum::<f32>()
    }

    #[test]
    fn decays_by_rt60() {
        let p = ReverbParams { dry_wet: 1.0, decay: 1.0, hi_on: false, lo_cut: false, hi_cut: false, ..Default::default() };
        let (l, r) = impulse(&p, 2.0);
        assert!(l.iter().chain(&r).all(|v| v.is_finite()));
        // Energy 0.2–0.4 s vs 0.8–1.0 s: 0.6 s at RT60 1 s ≈ −36 dB.
        let a = energy(&l[9600..19200]);
        let b = energy(&l[38400..48000]);
        let db = 10.0 * (b / a).log10();
        assert!((-46.0..-26.0).contains(&db), "{db}");
        // Stereo: channels differ.
        let d: f32 = l.iter().zip(&r).map(|(a, b)| (a - b).abs()).sum();
        assert!(d > 0.1);
    }

    #[test]
    fn input_filter_shapes() {
        let p = ReverbParams::default(); // Hi Cut only
        assert!(p.input_db(100.0, 48_000.0).abs() < 0.5);
        assert!(p.input_db(10_000.0, 48_000.0) < -12.0);
        let p = ReverbParams { lo_cut: true, hi_cut: false, ..Default::default() };
        assert!(p.input_db(10_000.0, 48_000.0).abs() < 0.5);
        assert!(p.input_db(40.0, 48_000.0) < -12.0);
        let p = ReverbParams { lo_cut: false, hi_cut: false, ..Default::default() };
        assert!(p.input_db(30.0, 48_000.0).abs() < 1e-3);
    }

    #[test]
    fn freeze_sustains_and_dry_passes() {
        let p = ReverbParams { dry_wet: 1.0, freeze: true, cut: false, chorus: false, ..Default::default() };
        let (l, _) = impulse(&p, 2.0);
        let a = energy(&l[24000..48000]);
        let b = energy(&l[72000..96000]);
        assert!(a > 1e-4 && b / a > 0.5, "{a} {b}");
        // Cut: input no longer feeds the frozen network.
        let p = ReverbParams { cut: true, ..p };
        let (l, _) = impulse(&p, 1.0);
        assert!(energy(&l[24000..48000]) < 1e-6);
        let p = ReverbParams { dry_wet: 0.0, ..Default::default() };
        let (l, _) = impulse(&p, 0.1);
        assert!((l[0] - 1.0).abs() < 1e-6 && l[1..].iter().all(|v| v.abs() < 1e-9));
        for d in 0..4 {
            let p = ReverbParams { density: d, size: 500.0, decay: 60.0, dry_wet: 1.0, ..Default::default() };
            let (l, _) = impulse(&p, 1.0);
            assert!(l.iter().all(|v| v.is_finite() && v.abs() < 2.0));
        }
    }
}
