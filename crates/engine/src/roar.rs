//! Roar (Live 12, manual 29.33): Input (Drive, Tone Amount / Frequency tilt,
//! Color Compensation), seven routing modes (Single, Serial, Parallel, Multi
//! Band, Mid Side, Feedback, Delay) over up to three gain stages (12 shaper
//! curves with Amount / Bias / Level, 9 filter types with Pre), a feedback
//! loop (Time / Synced / Triplet / Dotted / Note, Invert, Gate, band-pass
//! Freq | Width) with the compressor inside it, Compress (SC HPF), Output
//! (hard clip), Dry/Wet, and a modulation matrix (LFO 1 / LFO 2 / Env /
//! Noise → every continuous parameter, Global Amount). External audio
//! sidechain drives the envelope; a MIDI track can set the feedback note.

use crate::autoshift::AsNote;
use crate::devtap::DevTap;
use crate::dsp::{Rng, Svf, svf_g};
use crate::dynfx::{ScState, Sidechain};
use crate::modfx::SYNC_DIVS;
use serde::{Deserialize, Serialize};
use std::f32::consts::{PI, TAU};
use std::sync::Arc;

pub const RO_ROUTINGS: [&str; 7] = ["Single", "Serial", "Parallel", "Multi Band", "Mid Side", "Feedback", "Delay"];
pub const RO_SHAPERS: [&str; 12] = [
    "Soft Sine",
    "Digital Clip",
    "Bit Crusher",
    "Diode Clipper",
    "Tube Preamp",
    "Half Wave Rectifier",
    "Full Wave Rectifier",
    "Polynomial",
    "Fractal",
    "Tri Fold",
    "Noise Injection",
    "Shards",
];
/// Short chooser labels.
pub const RO_SHAPERS_SHORT: [&str; 12] =
    ["Soft", "Digital", "Bitcrush", "Diode", "Tube", "Half Rect", "Full Rect", "Poly", "Fractal", "Tri Fold", "Noise", "Shards"];
pub const RO_FILTERS: [&str; 9] = ["LP", "BP", "HP", "Notch", "Peak", "Morph", "Comb", "Resampling", "Dispersion"];
pub const RO_FB_MODES: [&str; 5] = ["Time", "Synced", "Triplet", "Dotted", "Note"];
pub const RO_LFO_WAVES: [&str; 5] = ["Sine", "Triangle", "Square", "Up", "Down"];
pub const RO_RATE_MODES: [&str; 5] = ["Free", "Synced", "Triplet", "Dotted", "Sixteenth"];
pub const RO_NOISES: [&str; 4] = ["Simplex", "Wander", "S & H", "Brown"];
pub const RO_SOURCES: [&str; 4] = ["LFO 1", "LFO 2", "Env", "Noise"];
pub const RO_DRIVE: (f32, f32) = (-24.0, 24.0);
pub const RO_TONE_F: (f32, f32) = (50.0, 10_000.0);
pub const RO_XO: (f32, f32) = (40.0, 16_000.0);
pub const RO_FREQ: (f32, f32) = (20.0, 20_000.0);
pub const RO_LEVEL: (f32, f32) = (-24.0, 24.0);
pub const RO_PEAK: (f32, f32) = (-24.0, 24.0);
pub const RO_FB_MS: (f32, f32) = (1.0, 2000.0);
pub const RO_FB_W: (f32, f32) = (0.1, 10.0);
pub const RO_OUT: (f32, f32) = (-36.0, 12.0);
pub const RO_RATE: (f32, f32) = (0.01, 40.0);
pub const RO_ENV_T: (f32, f32) = (0.1, 3000.0);
pub const RO_THRESH: (f32, f32) = (-70.0, 0.0);
pub const RO_ENV_GAIN: (f32, f32) = (-24.0, 24.0);
pub const RO_SC_GAIN: (f32, f32) = (-24.0, 24.0);
/// Shaper drive at 100 % Amount (dB).
pub const RO_AMT_DB: f32 = 36.0;
/// Modulation targets = continuous parameter ids 0..MT.
pub const MT: usize = 36;
/// Sixteenth-mode counts.
pub const RO_SIXTEENTHS: usize = 64;
/// Tap slots: source values (LFO 1, LFO 2, Env, Noise), compressor GR (dB).
pub const TAP_SRC: [usize; 4] = [0, 1, 2, 3];
pub const TAP_GR: usize = 4;
const CLEN: usize = 4096;
const FBLEN: usize = 1 << 17;
const SUB: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RoarStage {
    pub on: bool,
    /// 0..1.
    pub amount: f32,
    /// −1..1.
    pub bias: f32,
    /// Index into [`RO_SHAPERS`].
    pub shaper: u8,
    pub shaper_on: bool,
    /// dB.
    pub level: f32,
    pub filter_on: bool,
    /// Index into [`RO_FILTERS`].
    pub filter: u8,
    pub freq: f32,
    /// 0..1.
    pub res: f32,
    pub peak: f32,
    pub morph: f32,
    pub pre: bool,
}

impl Default for RoarStage {
    fn default() -> Self {
        Self {
            on: true,
            amount: 0.0,
            bias: 0.0,
            shaper: 0,
            shaper_on: true,
            level: 0.0,
            filter_on: true,
            filter: 0,
            freq: 16_000.0,
            res: 0.1,
            peak: 0.0,
            morph: 0.0,
            pre: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RoarLfo {
    /// Index into [`RO_LFO_WAVES`].
    pub wave: u8,
    /// Index into [`RO_RATE_MODES`].
    pub mode: u8,
    pub rate: f32,
    /// SYNC_DIVS index (Synced / Triplet / Dotted) or sixteenths − 1.
    pub div: u8,
    pub morph: f32,
    pub smooth: f32,
}

impl Default for RoarLfo {
    fn default() -> Self {
        Self { wave: 1, mode: 1, rate: 1.0, div: 6, morph: 0.0, smooth: 0.05 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RoarParams {
    pub drive: f32,
    pub tone_amt: f32,
    pub tone_freq: f32,
    pub color: bool,
    /// Index into [`RO_ROUTINGS`].
    pub routing: u8,
    pub blend: f32,
    pub xo_lo: f32,
    pub xo_hi: f32,
    pub stages: [RoarStage; 3],
    /// Index into [`RO_FB_MODES`].
    pub fb_mode: u8,
    pub fb_ms: f32,
    pub fb_div: u8,
    /// MIDI note (Note mode).
    pub fb_note: f32,
    pub fb_amt: f32,
    pub fb_invert: bool,
    pub fb_gate: bool,
    pub fb_freq: f32,
    pub fb_width: f32,
    pub compress: f32,
    pub sc_hpf: bool,
    pub output: f32,
    pub dry_wet: f32,
    pub lfo: [RoarLfo; 2],
    pub env_attack: f32,
    pub env_release: f32,
    pub env_hold: bool,
    pub env_thresh: f32,
    pub env_gain: f32,
    pub env_freq: f32,
    pub env_width: f32,
    pub env_listen: bool,
    /// Index into [`RO_NOISES`].
    pub noise: u8,
    pub noise_mode: u8,
    pub noise_rate: f32,
    pub noise_div: u8,
    pub noise_smooth: f32,
    /// [source][target] −1..1.
    #[serde(with = "mat_serde")]
    pub matrix: [[f32; MT]; 4],
    pub mod_amt: f32,
    /// External audio sidechain (envelope follower).
    pub sc: Sidechain,
    /// MIDI > FB Note source.
    pub midi: Sidechain,
    /// Views.
    pub sc_open: bool,
    pub mod_open: bool,
    /// Matrix (else Mod Sources) tab.
    pub matrix_tab: bool,
    pub src_tab: u8,
    pub stage_tab: u8,
    /// Expanded matrix view.
    pub expanded: bool,
}

impl Default for RoarParams {
    fn default() -> Self {
        let stages = [RoarStage::default(); 3];
        Self {
            drive: 0.0,
            tone_amt: 0.0,
            tone_freq: 180.0,
            color: true,
            routing: 0,
            blend: 0.5,
            xo_lo: 200.0,
            xo_hi: 2000.0,
            stages,
            fb_mode: 0,
            fb_ms: 18.2,
            fb_div: 4,
            fb_note: 60.0,
            fb_amt: 0.0,
            fb_invert: false,
            fb_gate: true,
            fb_freq: 1000.0,
            fb_width: 8.0,
            compress: 0.25,
            sc_hpf: true,
            output: 0.0,
            dry_wet: 1.0,
            lfo: [RoarLfo::default(), RoarLfo { div: 4, ..Default::default() }],
            env_attack: 1.0,
            env_release: 100.0,
            env_hold: true,
            env_thresh: RO_THRESH.0,
            env_gain: 0.0,
            env_freq: 1000.0,
            env_width: 8.0,
            env_listen: false,
            noise: 0,
            noise_mode: 1,
            noise_rate: 1.0,
            noise_div: 4,
            noise_smooth: 0.05,
            matrix: [[0.0; MT]; 4],
            mod_amt: 1.0,
            sc: Sidechain::default(),
            midi: Sidechain::default(),
            sc_open: false,
            mod_open: false,
            matrix_tab: false,
            src_tab: 0,
            stage_tab: 0,
            expanded: false,
        }
    }
}

impl RoarParams {
    /// Gain stages in use by the routing mode.
    pub fn n_stages(&self) -> usize {
        match self.routing {
            0 => 1,
            3 => 3,
            _ => 2,
        }
    }

    /// Stage tab names of the routing mode.
    pub fn stage_names(&self) -> &'static [&'static str] {
        match self.routing {
            0 => &["Stage 1"],
            3 => &["Low", "Mid", "High"],
            4 => &["Mid", "Side"],
            5 => &["Main", "Feedback"],
            _ => &["Stage 1", "Stage 2"],
        }
    }

    pub fn has_blend(&self) -> bool {
        !matches!(self.routing, 0 | 3)
    }

    pub fn mod_active(&self) -> bool {
        self.matrix.iter().flatten().any(|v| *v != 0.0)
    }

    /// A source drives at least one target.
    pub fn source_used(&self, s: usize) -> bool {
        self.matrix[s.min(3)].iter().any(|v| *v != 0.0)
    }
}

/// Serde for the 4 × 36 matrix (serde derives arrays up to 32 only).
mod mat_serde {
    use super::MT;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(m: &[[f32; MT]; 4], s: S) -> Result<S::Ok, S::Error> {
        let v: Vec<&[f32]> = m.iter().map(|r| &r[..]).collect();
        v.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[[f32; MT]; 4], D::Error> {
        let v: Vec<Vec<f32>> = Vec::deserialize(d)?;
        let mut m = [[0.0; MT]; 4];
        for (row, src) in m.iter_mut().zip(&v) {
            for (a, b) in row.iter_mut().zip(src) {
                *a = if b.is_finite() { b.clamp(-1.0, 1.0) } else { 0.0 };
            }
        }
        Ok(m)
    }
}

/// Shaper curve `t` (no drive / bias).
pub fn shape(t: u8, u: f32, amount: f32, noise: f32, shard: f32) -> f32 {
    match t {
        0 => u.clamp(-PI * 0.5, PI * 0.5).sin(),
        1 => u.clamp(-1.0, 1.0),
        2 => {
            let bits = 16.0 - 14.0 * amount.clamp(0.0, 1.0);
            let mu = 1.0 + 255.0 * amount.clamp(0.0, 1.0);
            let x = u.clamp(-1.0, 1.0);
            let c = x.signum() * (1.0 + mu * x.abs()).ln() / (1.0 + mu).ln();
            let lv = 2f32.powf(bits - 1.0);
            let q = (c * lv).round() / lv;
            q.signum() * ((1.0 + mu).powf(q.abs()) - 1.0) / mu
        }
        3 => {
            // Diode pair: softer knee than tanh.
            u / (1.0 + u.abs().powf(2.5)).powf(0.4)
        }
        4 => {
            if u >= 0.0 {
                1.0 - (-u).exp()
            } else {
                -(1.0 - (1.5 * u).exp()) / 1.5
            }
        }
        5 => u.max(0.0).tanh(),
        6 => u.tanh().abs(),
        7 => {
            let x = u.clamp(-1.0, 1.0);
            let t5 = 16.0 * x.powi(5) - 20.0 * x.powi(3) + 5.0 * x;
            x + (t5 - x) * amount.clamp(0.0, 1.0)
        }
        8 => {
            let mut s = 0.0;
            let mut a = 1.0;
            let mut f = 1.0;
            for _ in 0..4 {
                s += a * (u * f).sin();
                a *= 0.5;
                f *= 3.0;
            }
            s / 1.875
        }
        9 => (2.0 / PI) * (u * PI * 0.5).sin().asin(),
        10 => ((u + noise * (0.02 + 0.3 * amount) * (0.2 + u.abs())) * 1.2).tanh(),
        _ => {
            // Shards: piecewise segments with moving slopes.
            let seg = (u * 3.0).floor();
            let slope = 1.0 + 0.8 * (seg * 2.3 + shard * TAU).sin();
            (u * slope).tanh()
        }
    }
}

/// A stage's transfer at input `x` (display and DSP share it).
/// LFO shape of a Roar LFO (−1..1) for display: wave index into [`RO_LFO_WAVES`].
pub fn lfo_shape(wave: u8, ph: f32, morph: f32) -> f32 {
    let w = [0u8, 1, 4, 2, 3][(wave as usize).min(4)];
    crate::dsp::lfo_wave(w, ph, morph, 0.0)
}

pub fn stage_curve(s: &RoarStage, x: f32) -> f32 {
    let g = 10f32.powf(s.amount.clamp(0.0, 1.0) * RO_AMT_DB / 20.0);
    let b = s.bias.clamp(-1.0, 1.0) * 2.0;
    let lvl = 10f32.powf(s.level.clamp(RO_LEVEL.0, RO_LEVEL.1) / 20.0);
    (shape(s.shaper, x * g + b, s.amount, 0.0, 0.0) - shape(s.shaper, b, s.amount, 0.0, 0.0)) * lvl
}

/// Magnitude response (linear) of a stage filter at `f` Hz (display).
pub fn filter_mag(s: &RoarStage, f: f32, sr: f32) -> f32 {
    let fc = s.freq.clamp(RO_FREQ.0, RO_FREQ.1);
    let k = res_k(s.res);
    // Analog prototype, s = j·w.
    let w = f / fc;
    let (re, im) = (1.0 - w * w, k * w);
    let den = (re * re + im * im).sqrt().max(1e-9);
    let (lp, bp, hp) = (1.0 / den, k * w / den, w * w / den);
    match s.filter {
        0 => lp,
        1 => bp,
        2 => hp,
        3 => (1.0 - w * w).abs() / den,
        4 => {
            let a = 10f32.powf(s.peak / 20.0);
            let (nr, ni) = (1.0 - w * w, a * k * w);
            (nr * nr + ni * ni).sqrt() / den
        }
        5 => {
            let m = s.morph.clamp(0.0, 1.0);
            if m < 0.5 { lp + (bp - lp) * m * 2.0 } else { bp + (hp - bp) * (m - 0.5) * 2.0 }
        }
        6 => {
            let fb = 0.95 * s.res.clamp(0.0, 1.0);
            let ph = TAU * f / fc;
            1.0 / ((1.0 - fb * ph.cos()).powi(2) + (fb * ph.sin()).powi(2)).sqrt() * (1.0 - fb)
        }
        7 => {
            if f < fc.min(sr * 0.5) * 0.5 {
                1.0
            } else {
                0.6
            }
        }
        _ => 1.0,
    }
}

fn res_k(res: f32) -> f32 {
    1.414 - 1.37 * res.clamp(0.0, 1.0)
}

/// RBJ biquad.
#[derive(Debug, Clone, Copy, Default)]
struct Bq {
    b: [f32; 3],
    a: [f32; 2],
    z: [[f32; 2]; 2],
}

impl Bq {
    fn shelf(&mut self, f: f32, db: f32, sr: f32) {
        let a = 10f32.powf(db / 40.0);
        let w = TAU * (f / sr).min(0.45);
        let (sn, cs) = w.sin_cos();
        let al = sn / 2.0 * std::f32::consts::SQRT_2;
        let sa = 2.0 * a.sqrt() * al;
        let a0 = (a + 1.0) + (a - 1.0) * cs + sa;
        self.b = [
            a * ((a + 1.0) - (a - 1.0) * cs + sa) / a0,
            2.0 * a * ((a - 1.0) - (a + 1.0) * cs) / a0,
            a * ((a + 1.0) - (a - 1.0) * cs - sa) / a0,
        ];
        self.a = [-2.0 * ((a - 1.0) + (a + 1.0) * cs) / a0, ((a + 1.0) + (a - 1.0) * cs - sa) / a0];
    }

    #[inline]
    fn tick(&mut self, ch: usize, x: f32) -> f32 {
        let z = &mut self.z[ch];
        let y = self.b[0] * x + z[0];
        z[0] = self.b[1] * x - self.a[0] * y + z[1];
        z[1] = self.b[2] * x - self.a[1] * y;
        if !y.is_finite() {
            *z = [0.0; 2];
            return 0.0;
        }
        y
    }
}

#[derive(Debug, Clone)]
struct StageDsp {
    svf: [Svf; 2],
    comb: [Box<[f32]>; 2],
    cw: usize,
    hold: [f32; 2],
    hph: [f32; 2],
    ap: [[f32; 6]; 2],
    dc: [(f32, f32); 2],
}

impl StageDsp {
    fn new() -> Self {
        let z = || vec![0.0f32; CLEN].into_boxed_slice();
        Self { svf: [Svf::default(); 2], comb: [z(), z()], cw: 0, hold: [0.0; 2], hph: [0.0; 2], ap: [[0.0; 6]; 2], dc: [(0.0, 0.0); 2] }
    }

    fn clear(&mut self) {
        for c in self.comb.iter_mut() {
            c.fill(0.0);
        }
        self.svf = [Svf::default(); 2];
        self.hold = [0.0; 2];
        self.hph = [0.0; 2];
        self.ap = [[0.0; 6]; 2];
        self.dc = [(0.0, 0.0); 2];
    }

    fn filter(&mut self, s: &RoarStage, ch: usize, x: f32, g: f32, sr: f32) -> f32 {
        let k = res_k(s.res);
        let fc = s.freq.clamp(RO_FREQ.0, RO_FREQ.1);
        match s.filter {
            0..=5 => {
                let (lp, bp, hp) = self.svf[ch].tick(x, g, k);
                match s.filter {
                    0 => lp,
                    1 => bp * k,
                    2 => hp,
                    3 => lp + hp,
                    4 => x + (10f32.powf(s.peak / 20.0) - 1.0) * k * bp,
                    _ => {
                        let m = s.morph.clamp(0.0, 1.0);
                        let bp = bp * k;
                        if m < 0.5 { lp + (bp - lp) * m * 2.0 } else { bp + (hp - bp) * (m - 0.5) * 2.0 }
                    }
                }
            }
            6 => {
                let d = (sr / fc).clamp(1.0, (CLEN - 8) as f32);
                let fb = 0.95 * s.res.clamp(0.0, 1.0);
                let y = crate::dsp::read(&self.comb[ch], self.cw, d, false);
                let v = x + fb * y;
                self.comb[ch][self.cw] = if v.is_finite() { v } else { 0.0 };
                0.5 * (x + y) * (1.0 - fb * 0.5)
            }
            7 => {
                // Sample-rate reduction to the cutoff.
                self.hph[ch] += fc.min(sr) / sr;
                if self.hph[ch] >= 1.0 {
                    self.hph[ch] -= 1.0;
                    self.hold[ch] = x;
                }
                self.hold[ch]
            }
            _ => {
                // Dispersion: first-order all-passes at the cutoff (Res: more stages).
                let t = (PI * (fc / sr).min(0.49)).tan();
                let a = (t - 1.0) / (t + 1.0);
                let n = 2 + (s.res.clamp(0.0, 1.0) * 4.0) as usize;
                let mut v = x;
                for st in self.ap[ch].iter_mut().take(n) {
                    let y = a * v + *st;
                    *st = v - a * y;
                    v = y;
                }
                v
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run(&mut self, s: &RoarStage, ch: usize, x: f32, g: f32, sr: f32, noise: f32, shard: f32) -> f32 {
        if !s.on {
            return x;
        }
        let mut y = x;
        if s.filter_on && s.pre {
            y = self.filter(s, ch, y, g, sr);
        }
        if s.shaper_on {
            let gd = 10f32.powf(s.amount.clamp(0.0, 1.0) * RO_AMT_DB / 20.0);
            let b = s.bias.clamp(-1.0, 1.0) * 2.0;
            let lvl = 10f32.powf(s.level.clamp(RO_LEVEL.0, RO_LEVEL.1) / 20.0);
            let v = shape(s.shaper, y * gd + b, s.amount, noise, shard) - shape(s.shaper, b, s.amount, 0.0, shard);
            // DC blocker (rectifiers, bias).
            let (x1, y1) = self.dc[ch];
            let o = v - x1 + 0.9995 * y1;
            self.dc[ch] = (v, if o.is_finite() { o } else { 0.0 });
            y = self.dc[ch].1 * lvl;
        }
        if s.filter_on && !s.pre {
            y = self.filter(s, ch, y, g, sr);
        }
        y
    }
}

#[derive(Debug, Clone)]
pub struct RoarState {
    tap: Option<Arc<DevTap>>,
    pub(crate) sc: ScState,
    st: [StageDsp; 3],
    tone: Bq,
    comp_bq: Bq,
    xo: [[Svf; 2]; 4],
    fb: [Box<[f32]>; 2],
    fw: usize,
    fb_bp: [Svf; 2],
    fb_d: f32,
    gate: f32,
    in_env: f32,
    comp_env: f32,
    comp_hp: [f32; 2],
    rng: Rng,
    // Modulation sources.
    lfo_ph: [f32; 2],
    lfo_out: [f32; 2],
    env_bp: Svf,
    env: f32,
    env_attack_done: bool,
    noise_ph: f32,
    noise_v: [f32; 4],
    noise_out: f32,
    brown: f32,
    src: [f32; 4],
    shard: f32,
    notes: [AsNote; 32],
    n_notes: usize,
    note: f32,
    have_note: bool,
}

impl Default for RoarState {
    fn default() -> Self {
        Self::new(None)
    }
}

impl RoarState {
    pub fn new(id: Option<u64>) -> Self {
        let z = || vec![0.0f32; FBLEN].into_boxed_slice();
        Self {
            tap: id.map(crate::devtap::dev_tap),
            sc: ScState::default(),
            st: [StageDsp::new(), StageDsp::new(), StageDsp::new()],
            tone: Bq::default(),
            comp_bq: Bq::default(),
            xo: [[Svf::default(); 2]; 4],
            fb: [z(), z()],
            fw: 0,
            fb_bp: [Svf::default(); 2],
            fb_d: 100.0,
            gate: 1.0,
            in_env: 0.0,
            comp_env: 0.0,
            comp_hp: [0.0; 2],
            rng: Rng(0x0a0a_7777),
            lfo_ph: [0.0; 2],
            lfo_out: [0.0; 2],
            env_bp: Svf::default(),
            env: 0.0,
            env_attack_done: false,
            noise_ph: 0.0,
            noise_v: [0.0; 4],
            noise_out: 0.0,
            brown: 0.0,
            src: [0.0; 4],
            shard: 0.0,
            notes: [AsNote::default(); 32],
            n_notes: 0,
            note: 60.0,
            have_note: false,
        }
    }

    pub fn set_id(&mut self, id: u64) {
        self.tap = Some(crate::devtap::dev_tap(id));
    }

    /// Clear without allocating (audio thread).
    pub fn reset(&mut self) {
        for s in self.st.iter_mut() {
            s.clear();
        }
        for b in self.fb.iter_mut() {
            b.fill(0.0);
        }
        self.sc.reset();
        self.tone = Bq::default();
        self.comp_bq = Bq::default();
        self.xo = [[Svf::default(); 2]; 4];
        self.fb_bp = [Svf::default(); 2];
        self.gate = 1.0;
        self.in_env = 0.0;
        self.comp_env = 0.0;
        self.comp_hp = [0.0; 2];
        self.lfo_ph = [0.0; 2];
        self.lfo_out = [0.0; 2];
        self.env_bp = Svf::default();
        self.env = 0.0;
        self.noise_ph = 0.0;
        self.noise_v = [0.0; 4];
        self.noise_out = 0.0;
        self.brown = 0.0;
        self.src = [0.0; 4];
        self.n_notes = 0;
        self.have_note = false;
    }

    pub(crate) fn set_notes(&mut self, it: &mut dyn Iterator<Item = AsNote>) {
        self.n_notes = 0;
        for e in it {
            if self.n_notes < self.notes.len() {
                self.notes[self.n_notes] = e;
                self.n_notes += 1;
            }
        }
    }

    /// Current modulation source values (−1..1; Env 0..1).
    pub fn sources(&self) -> [f32; 4] {
        self.src
    }

    fn rate_hz(mode: u8, rate: f32, div: u8, beat_hz: f32) -> f32 {
        let d = |i: u8| SYNC_DIVS[(i as usize).min(SYNC_DIVS.len() - 1)].0 as f32;
        match mode {
            0 => rate.clamp(RO_RATE.0, RO_RATE.1),
            1 => beat_hz / d(div),
            2 => beat_hz / (d(div) * 2.0 / 3.0),
            3 => beat_hz / (d(div) * 1.5),
            _ => beat_hz * 4.0 / (div as f32 + 1.0).min(RO_SIXTEENTHS as f32),
        }
    }

    /// Advance the modulation sources by `n` samples (`env_in`: peak level).
    fn mod_sources(&mut self, p: &RoarParams, n: usize, env_in: f32, beat_hz: f32, sr: f32) {
        let dt = n as f32 / sr;
        for i in 0..2 {
            let l = &p.lfo[i];
            let hz = Self::rate_hz(l.mode, l.rate, l.div, beat_hz);
            self.lfo_ph[i] = (self.lfo_ph[i] + hz * dt).rem_euclid(1.0);
            let w = [0u8, 1, 4, 2, 3][(l.wave as usize).min(4)];
            let v = crate::dsp::lfo_wave(w, self.lfo_ph[i], l.morph, 0.0);
            let k = if l.smooth > 0.0 { 1.0 - (-dt / (l.smooth.clamp(0.0, 1.0) * 0.25)).exp() } else { 1.0 };
            self.lfo_out[i] += k * (v - self.lfo_out[i]);
        }
        // Envelope follower (Hold: finish the attack before releasing).
        let a = (-dt / (p.env_attack.clamp(RO_ENV_T.0, RO_ENV_T.1) * 0.001)).exp();
        let r = (-dt / (p.env_release.clamp(RO_ENV_T.0, RO_ENV_T.1) * 0.001)).exp();
        let gain = 10f32.powf(p.env_gain.clamp(RO_ENV_GAIN.0, RO_ENV_GAIN.1) / 20.0);
        let thr = if p.env_thresh <= RO_THRESH.0 { 0.0 } else { 10f32.powf(p.env_thresh / 20.0) };
        let x = if env_in > thr { (env_in * gain).min(1.0) } else { 0.0 };
        if x > self.env || (p.env_hold && !self.env_attack_done && x > 0.0) {
            let tgt = x.max(self.env);
            self.env = tgt + a * (self.env - tgt);
            self.env_attack_done = self.env >= tgt * 0.95;
        } else {
            self.env = x + r * (self.env - x);
            if x <= 0.0 {
                self.env_attack_done = false;
            }
        }
        // Noise.
        let hz = Self::rate_hz(p.noise_mode, p.noise_rate, p.noise_div, beat_hz);
        let prev = self.noise_ph;
        self.noise_ph += hz * dt;
        if self.noise_ph >= 1.0 || prev == 0.0 {
            self.noise_ph = self.noise_ph.fract();
            self.noise_v = [self.noise_v[1], self.noise_v[2], self.noise_v[3], self.rng.bi()];
        }
        let t = self.noise_ph;
        let v = match p.noise {
            0 => {
                // Simplex-like: cubic (Catmull-Rom) through the random points.
                let [p0, p1, p2, p3] = self.noise_v;
                0.5 * ((2.0 * p1)
                    + (-p0 + p2) * t
                    + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t
                    + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t * t * t)
            }
            1 => {
                let u = 0.5 - 0.5 * (PI * t).cos();
                self.noise_v[2] + (self.noise_v[3] - self.noise_v[2]) * u
            }
            2 => self.noise_v[3],
            _ => {
                self.brown = (self.brown * 0.98 + self.rng.bi() * 0.2).clamp(-1.0, 1.0);
                self.brown
            }
        }
        .clamp(-1.0, 1.0);
        let k = if p.noise_smooth > 0.0 { 1.0 - (-dt / (p.noise_smooth.clamp(0.0, 1.0) * 0.25)).exp() } else { 1.0 };
        self.noise_out += k * (v - self.noise_out);
        self.src = [self.lfo_out[0], self.lfo_out[1], self.env, self.noise_out];
    }

    /// Parameters with the matrix applied.
    fn modulated(&self, p: &RoarParams) -> RoarParams {
        let mut q = *p;
        if !p.mod_active() {
            return q;
        }
        let g = p.mod_amt.clamp(0.0, 1.0);
        for t in 0..MT {
            let m: f32 = (0..4).map(|s| p.matrix[s][t] * self.src[s]).sum::<f32>() * g;
            if m != 0.0
                && let Some(v) = crate::params6::ro_norm(p, t as u8)
            {
                crate::params6::ro_set(&mut q, t as u8, v + m);
            }
        }
        q
    }

    fn midi_block(&mut self) {
        for k in 0..self.n_notes {
            let e = self.notes[k];
            if e.vel > 0 {
                self.note = e.pitch as f32;
                self.have_note = true;
            }
        }
        self.n_notes = 0;
    }

    #[allow(clippy::needless_range_loop)]
    pub fn process(&mut self, base: &RoarParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let n = l.len().min(r.len());
        let midi_fb = base.midi.on && base.midi.source != 0;
        if midi_fb {
            self.midi_block();
        }
        let (tempo, _) = crate::modfx::clock();
        let beat_hz = tempo.max(1.0) as f32 / 60.0;
        let g_ext = self.sc.prepare(&base.sc, sr);
        let mut start = 0;
        while start < n {
            let end = (start + SUB).min(n);
            // Envelope input over this sub-block (band-passed, internal/external mix).
            let eg = svf_g(base.env_freq.clamp(RO_FREQ.0, RO_FREQ.1), sr);
            let ek = (1.0 / base.env_width.clamp(RO_FB_W.0, RO_FB_W.1)).clamp(0.1, 10.0) * 2.0;
            let mut pk = 0.0f32;
            for i in start..end {
                let (el, er) = self.sc.detect(&base.sc, i, l[i], r[i], g_ext);
                let x = 0.5 * (el + er);
                pk = pk.max((self.env_bp.tick(x, eg, ek).1 * ek).abs());
            }
            self.mod_sources(base, end - start, pk, beat_hz, sr);
            let p = self.modulated(base);
            self.block(&p, &mut l[start..end], &mut r[start..end], sr, beat_hz, midi_fb);
            start = end;
        }
        if let Some(t) = &self.tap {
            for s in 0..4 {
                t.set(TAP_SRC[s], self.src[s]);
            }
            t.set_rate(sr / SUB as f32);
            t.push(self.src);
            t.set(TAP_GR, self.comp_env);
        }
    }

    #[allow(clippy::needless_range_loop)]
    fn block(&mut self, p: &RoarParams, l: &mut [f32], r: &mut [f32], sr: f32, beat_hz: f32, midi_fb: bool) {
        let n = l.len().min(r.len());
        let drive = 10f32.powf(p.drive.clamp(RO_DRIVE.0, RO_DRIVE.1) / 20.0);
        let ta = p.tone_amt.clamp(-1.0, 1.0);
        self.tone.shelf(p.tone_freq.clamp(RO_TONE_F.0, RO_TONE_F.1), -15.0 * ta, sr);
        self.comp_bq.shelf(p.tone_freq.clamp(RO_TONE_F.0, RO_TONE_F.1), 15.0 * ta, sr);
        let tone_g = 10f32.powf(7.5 * ta / 20.0);
        let gs: [f32; 3] = std::array::from_fn(|i| svf_g(p.stages[i].freq.clamp(RO_FREQ.0, RO_FREQ.1), sr));
        let g_lo = svf_g(p.xo_lo.clamp(RO_XO.0, RO_XO.1), sr);
        let g_hi = svf_g(p.xo_hi.clamp(RO_XO.0, RO_XO.1).max(p.xo_lo), sr);
        let k_b = crate::dsp::BUTTER_K;
        // Feedback time.
        let note = if midi_fb && self.have_note { self.note } else { p.fb_note };
        let d_beats = SYNC_DIVS[(p.fb_div as usize).min(SYNC_DIVS.len() - 1)].0 as f32;
        let fb_s = match p.fb_mode {
            0 => p.fb_ms.clamp(RO_FB_MS.0, RO_FB_MS.1) * 0.001 * sr,
            1 => d_beats / beat_hz * sr,
            2 => d_beats * 2.0 / 3.0 / beat_hz * sr,
            3 => d_beats * 1.5 / beat_hz * sr,
            _ => sr / crate::specres::midi_hz(note),
        }
        .clamp(1.0, (FBLEN - 8) as f32);
        let fb_amt = p.fb_amt.clamp(0.0, 1.0) * if p.fb_invert { -1.0 } else { 1.0 };
        let fg = svf_g(p.fb_freq.clamp(RO_FREQ.0, RO_FREQ.1), sr);
        let fk = (2.0 / p.fb_width.clamp(RO_FB_W.0, RO_FB_W.1)).clamp(0.05, 20.0);
        let ks = 1.0 - (-TAU * 8.0 / sr).exp();
        // Compressor: threshold falls with Compress, ratio 4.
        let thr = 10f32.powf(-30.0 * p.compress.clamp(0.0, 1.0) / 20.0);
        let ca = (-1.0 / (0.003 * sr)).exp();
        let cr = (-1.0 / (0.12 * sr)).exp();
        let khp = 1.0 - (-TAU * 120.0 / sr).exp();
        let g_out = 10f32.powf(p.output.clamp(RO_OUT.0, RO_OUT.1) / 20.0);
        let wet = p.dry_wet.clamp(0.0, 1.0);
        let blend = p.blend.clamp(0.0, 1.0);
        let gate_rel = (-1.0 / (0.2 * sr)).exp();
        let env_fall = (-1.0 / (0.05 * sr)).exp();
        let listen = p.env_listen || p.sc.listen;
        let routing = p.routing.min(6);
        for i in 0..n {
            let dry = [l[i], r[i]];
            self.in_env = dry[0].abs().max(dry[1].abs()).max(self.in_env * env_fall);
            let gate_t = if !p.fb_gate || self.in_env > 1e-3 { 1.0 } else { 0.0 };
            self.gate = gate_t + gate_rel * (self.gate - gate_t);
            self.fb_d += ks * (fb_s - self.fb_d);
            self.shard = (self.shard + 0.37 / sr).fract();
            let mut fbin = [0.0f32; 2];
            for ch in 0..2 {
                let d = crate::dsp::read(&self.fb[ch], self.fw, self.fb_d, false);
                fbin[ch] = self.fb_bp[ch].tick(d, fg, fk).1 * fk * fb_amt * self.gate;
            }
            let mut x = [0.0f32; 2];
            for ch in 0..2 {
                x[ch] = self.tone.tick(ch, dry[ch] * drive) * tone_g;
            }
            let nz = [self.rng.bi(), self.rng.bi()];
            let sh = self.shard;
            let mut y = [0.0f32; 2];
            let mut line = [0.0f32; 2];
            match routing {
                3 => {
                    for ch in 0..2 {
                        let v = x[ch] + fbin[ch];
                        let a = self.xo[0][ch].tick(v, g_lo, k_b).0;
                        let lo = self.xo[1][ch].tick(a, g_lo, k_b).0;
                        let rest = v - lo;
                        let b = self.xo[2][ch].tick(rest, g_hi, k_b).0;
                        let mid = self.xo[3][ch].tick(b, g_hi, k_b).0;
                        let hi = rest - mid;
                        y[ch] = self.st[0].run(&p.stages[0], ch, lo, gs[0], sr, nz[ch], sh)
                            + self.st[1].run(&p.stages[1], ch, mid, gs[1], sr, nz[ch], sh)
                            + self.st[2].run(&p.stages[2], ch, hi, gs[2], sr, nz[ch], sh);
                    }
                }
                4 => {
                    let m = 0.5 * (x[0] + x[1] + fbin[0] + fbin[1]);
                    let s = 0.5 * (x[0] - x[1] + fbin[0] - fbin[1]);
                    let m = self.st[0].run(&p.stages[0], 0, m, gs[0], sr, nz[0], sh) * (2.0 * (1.0 - blend)).min(1.0);
                    let s = self.st[1].run(&p.stages[1], 1, s, gs[1], sr, nz[1], sh) * (2.0 * blend).min(1.0);
                    y = [m + s, m - s];
                }
                _ => {
                    for ch in 0..2 {
                        let s1 = &p.stages[0];
                        let s2 = &p.stages[1];
                        y[ch] = match routing {
                            0 => self.st[0].run(s1, ch, x[ch] + fbin[ch], gs[0], sr, nz[ch], sh),
                            1 => {
                                let a = self.st[0].run(s1, ch, x[ch] + fbin[ch], gs[0], sr, nz[ch], sh);
                                let b = self.st[1].run(s2, ch, a, gs[1], sr, nz[ch], sh);
                                a + (b - a) * blend
                            }
                            2 => {
                                let v = x[ch] + fbin[ch];
                                let a = self.st[0].run(s1, ch, v, gs[0], sr, nz[ch], sh);
                                let b = self.st[1].run(s2, ch, v, gs[1], sr, nz[ch], sh);
                                a + (b - a) * blend
                            }
                            5 => {
                                let a = self.st[0].run(s1, ch, x[ch], gs[0], sr, nz[ch], sh);
                                let b = self.st[1].run(s2, ch, fbin[ch], gs[1], sr, nz[ch], sh);
                                a + b * blend
                            }
                            _ => {
                                // Delay: stage 2 processes the delayed stage 1 output.
                                let a = self.st[0].run(s1, ch, x[ch], gs[0], sr, nz[ch], sh);
                                let d = crate::dsp::read(&self.fb[ch], self.fw, self.fb_d, false);
                                let b = self.st[1].run(s2, ch, d, gs[1], sr, nz[ch], sh);
                                line[ch] = a + self.fb_bp[ch].tick(b, fg, fk).1 * fk * fb_amt * self.gate;
                                a + b * blend
                            }
                        };
                    }
                }
            }
            // Colour compensation, compressor (in the feedback loop).
            let mut det = 0.0f32;
            for ch in 0..2 {
                if p.color {
                    y[ch] = self.comp_bq.tick(ch, y[ch]) / tone_g;
                }
                let d = if p.sc_hpf {
                    self.comp_hp[ch] += khp * (y[ch] - self.comp_hp[ch]);
                    y[ch] - self.comp_hp[ch]
                } else {
                    y[ch]
                };
                det = det.max(d.abs());
            }
            let gr = if det > thr { (thr / det).powf(0.75) } else { 1.0 };
            let c = if gr < self.comp_env { ca } else { cr };
            self.comp_env = gr + c * (self.comp_env - gr);
            if !self.comp_env.is_finite() || self.comp_env <= 0.0 {
                self.comp_env = 1.0;
            }
            let makeup = 1.0 + p.compress.clamp(0.0, 1.0) * 1.5;
            let mut out = [0.0f32; 2];
            for ch in 0..2 {
                let v = (y[ch] * self.comp_env * makeup).clamp(-4.0, 4.0);
                self.fb[ch][self.fw] = if routing == 6 { line[ch].clamp(-4.0, 4.0) } else { v };
                let w = (v * g_out).clamp(-1.0, 1.0);
                out[ch] = if listen { x[ch] } else { dry[ch] + (w - dry[ch]) * wet };
            }
            self.fw = (self.fw + 1) & (FBLEN - 1);
            for st in self.st.iter_mut() {
                st.cw = (st.cw + 1) & (CLEN - 1);
            }
            l[i] = out[0];
            r[i] = out[1];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(p: &RoarParams, x: &[f32]) -> Vec<f32> {
        let mut s = RoarState::new(None);
        let mut l = x.to_vec();
        let mut r = x.to_vec();
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            s.process(p, a, b, 48_000.0);
        }
        l
    }

    #[test]
    fn transparent_at_default_and_bounded_everywhere() {
        let sr = 48_000.0;
        let x: Vec<f32> = (0..24_000).map(|i| (i as f32 * TAU * 220.0 / sr).sin() * 0.05).collect();
        let p = RoarParams { compress: 0.0, color: false, ..Default::default() };
        let y = run(&p, &x);
        let err = x[12_000..].iter().zip(&y[12_000..]).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(err < 0.01, "{err}");
        for routing in 0..7u8 {
            for sh in 0..12u8 {
                let mut p = RoarParams { routing, fb_amt: 0.9, fb_mode: (sh % 5).min(4), ..Default::default() };
                for (i, s) in p.stages.iter_mut().enumerate() {
                    *s = RoarStage {
                        on: true,
                        amount: 0.8,
                        bias: 0.3,
                        shaper: sh,
                        filter: (sh + i as u8) % 9,
                        res: 0.9,
                        ..Default::default()
                    };
                }
                p.matrix[0][6] = 1.0;
                p.matrix[3][8] = -0.5;
                let y = run(&p, &x[..6000]);
                assert!(y.iter().all(|v| v.is_finite() && v.abs() <= 1.0001), "routing {routing} shaper {sh}");
            }
        }
    }

    #[test]
    fn curves_and_responses() {
        for t in 0..12u8 {
            for i in 0..=20 {
                let u = i as f32 / 10.0 - 1.0;
                assert!(shape(t, u * 4.0, 0.5, 0.1, 0.3).is_finite());
            }
        }
        let s = RoarStage::default();
        assert!((filter_mag(&s, 100.0, 48_000.0) - 1.0).abs() < 0.01);
        assert!(filter_mag(&s, 20_000.0, 48_000.0) < 1.0);
        assert!(stage_curve(&s, 0.0).abs() < 1e-6);
    }
}
