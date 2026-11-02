//! Dynamics: Live's Gate, Glue Compressor and Limiter, plus the shared
//! sidechain section (external source, gain, mix, sidechain EQ, listen).
//!
//! Parameters are `Copy` user-unit data (project format). States own fixed
//! buffers allocated on the control thread; processing never allocates.

use crate::devtap::{DevTap, HOP, dev_tap};
use crate::eq8::{Coefs, Fir, OS_TAPS, Rbj, Tdf2, halfband, rbj};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Longest lookahead (10 ms) at 384 kHz, rounded up to a power of two.
const LA_CAP: usize = 1 << 12;
/// Engine block cap for the sidechain buffers.
pub const SC_MAX: usize = 4096;

pub fn db_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

pub fn lin_db(x: f32) -> f32 {
    20.0 * x.max(1e-12).log10()
}

// ----------------------------------------------------------------------------
// Sidechain.
// ----------------------------------------------------------------------------

/// Where the external sidechain taps its source track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ScTap {
    PreFx,
    #[default]
    PostFx,
    PostMixer,
}

impl ScTap {
    pub const ALL: [ScTap; 3] = [ScTap::PreFx, ScTap::PostFx, ScTap::PostMixer];
    pub fn label(self) -> &'static str {
        match self {
            ScTap::PreFx => "Pre FX",
            ScTap::PostFx => "Post FX",
            ScTap::PostMixer => "Post Mixer",
        }
    }
}

/// Sidechain EQ filter types (Live's six buttons, row by row).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ScEq {
    LowShelf,
    Peak,
    HighShelf,
    LowPass,
    BandPass,
    #[default]
    HighPass,
}

impl ScEq {
    pub const ALL: [ScEq; 6] = [ScEq::LowShelf, ScEq::Peak, ScEq::HighShelf, ScEq::LowPass, ScEq::BandPass, ScEq::HighPass];
    pub fn label(self) -> &'static str {
        match self {
            ScEq::LowShelf => "Low Shelf",
            ScEq::Peak => "Peak",
            ScEq::HighShelf => "High Shelf",
            ScEq::LowPass => "Low Pass",
            ScEq::BandPass => "Band Pass",
            ScEq::HighPass => "High Pass",
        }
    }
    /// Shelves and peak show Gain instead of Q.
    pub fn has_gain(self) -> bool {
        matches!(self, ScEq::LowShelf | ScEq::Peak | ScEq::HighShelf)
    }
}

pub const SC_GAIN_MIN: f32 = -70.0;
pub const SC_GAIN_MAX: f32 = 24.0;
pub const SC_FREQ_MIN: f32 = 30.0;
pub const SC_FREQ_MAX: f32 = 15_000.0;
pub const SC_Q_MIN: f32 = 0.1;
pub const SC_Q_MAX: f32 = 18.0;
pub const SC_EQ_GAIN_MAX: f32 = 15.0;

/// Sidechain section of Gate / Glue Compressor (unfolded with the strip).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sidechain {
    /// Section unfolded (view).
    pub shown: bool,
    /// External sidechain on.
    pub on: bool,
    /// Project track id of the source (0 = none).
    pub source: u64,
    pub tap: ScTap,
    /// dB (≤ [`SC_GAIN_MIN`] = −inf).
    pub gain: f32,
    /// 0..1: share of the external signal in the detector.
    pub mix: f32,
    pub eq_on: bool,
    pub eq: ScEq,
    pub freq: f32,
    pub q: f32,
    pub eq_gain: f32,
    /// Headphones: hear the detector signal.
    pub listen: bool,
}

impl Default for Sidechain {
    fn default() -> Self {
        Self {
            shown: false,
            on: false,
            source: 0,
            tap: ScTap::PostFx,
            gain: 0.0,
            mix: 1.0,
            eq_on: false,
            eq: ScEq::HighPass,
            freq: 200.0,
            q: 0.71,
            eq_gain: 0.0,
            listen: false,
        }
    }
}

impl Sidechain {
    fn coefs(&self, sr: f32) -> Coefs {
        let (f, q, g, sr) =
            (self.freq.clamp(SC_FREQ_MIN, SC_FREQ_MAX) as f64, self.q.clamp(SC_Q_MIN, SC_Q_MAX) as f64, self.eq_gain as f64, sr as f64);
        match self.eq {
            ScEq::LowShelf => rbj(Rbj::LowShelf, f, sr, 0.71, g),
            ScEq::Peak => rbj(Rbj::Peak, f, sr, q, g),
            ScEq::HighShelf => rbj(Rbj::HighShelf, f, sr, 0.71, g),
            ScEq::LowPass => rbj(Rbj::Lp, f, sr, q, 0.0),
            ScEq::BandPass => rbj(Rbj::Bp, f, sr, q, 0.0),
            ScEq::HighPass => rbj(Rbj::Hp, f, sr, q, 0.0),
        }
    }
}

/// Detector input: own signal mixed with the external sidechain (filled by
/// the engine before the chain runs), then the sidechain EQ.
#[derive(Debug, Clone)]
pub struct ScState {
    /// External source block (engine-filled), valid for `len` frames.
    pub ext_l: Box<[f32]>,
    pub ext_r: Box<[f32]>,
    pub len: usize,
    eq: [Tdf2; 2],
    key: Option<(ScEq, u32, u32, u32, u32)>,
    coefs: Coefs,
}

impl Default for ScState {
    fn default() -> Self {
        Self {
            ext_l: vec![0.0; SC_MAX].into_boxed_slice(),
            ext_r: vec![0.0; SC_MAX].into_boxed_slice(),
            len: 0,
            eq: Default::default(),
            key: None,
            coefs: Coefs::default(),
        }
    }
}

impl ScState {
    pub(crate) fn reset(&mut self) {
        self.eq = Default::default();
    }

    /// Detector sample `i` (stereo) for own input (l, r).
    #[inline]
    pub(crate) fn detect(&mut self, sc: &Sidechain, i: usize, l: f32, r: f32, g_ext: f32) -> (f32, f32) {
        let (mut a, mut b) = (l, r);
        if sc.on && sc.source != 0 && i < self.len {
            let m = sc.mix.clamp(0.0, 1.0);
            a = a * (1.0 - m) + self.ext_l[i] * g_ext * m;
            b = b * (1.0 - m) + self.ext_r[i] * g_ext * m;
        }
        if sc.eq_on {
            a = self.eq[0].tick(&self.coefs, a as f64) as f32;
            b = self.eq[1].tick(&self.coefs, b as f64) as f32;
        }
        (a, b)
    }

    pub(crate) fn prepare(&mut self, sc: &Sidechain, sr: f32) -> f32 {
        if sc.eq_on {
            let key = (sc.eq, sc.freq.to_bits(), sc.q.to_bits(), sc.eq_gain.to_bits(), sr.to_bits());
            if self.key != Some(key) {
                self.key = Some(key);
                self.coefs = sc.coefs(sr);
            }
        }
        if sc.gain <= SC_GAIN_MIN { 0.0 } else { db_lin(sc.gain.min(SC_GAIN_MAX)) }
    }
}

/// Coefficient of a one-pole follower reaching 1 − 1/e in `ms`.
fn coef(ms: f32, sr: f32) -> f32 {
    if ms <= 0.0 { 0.0 } else { (-1.0 / (ms * 0.001 * sr)).exp() }
}

/// Fixed-size delay line (lookahead), stereo.
#[derive(Debug, Clone)]
struct Delay {
    buf: Box<[[f32; 2]]>,
    pos: usize,
}

impl Delay {
    fn new() -> Self {
        Self { buf: vec![[0.0; 2]; LA_CAP].into_boxed_slice(), pos: 0 }
    }
    fn reset(&mut self) {
        self.buf.fill([0.0; 2]);
        self.pos = 0;
    }
    #[inline]
    fn tick(&mut self, x: [f32; 2], d: usize) -> [f32; 2] {
        self.buf[self.pos] = x;
        let y = self.buf[(self.pos + LA_CAP - d.min(LA_CAP - 1)) & (LA_CAP - 1)];
        self.pos = (self.pos + 1) & (LA_CAP - 1);
        y
    }
}

/// Display history writer: a frame every `HOP` samples (at 48 kHz).
#[derive(Debug, Clone, Default)]
struct Hist {
    n: usize,
    acc: [f32; 4],
}

impl Hist {
    #[inline]
    fn add(&mut self, tap: &DevTap, sr: f32, f: [f32; 4]) {
        for (a, v) in self.acc.iter_mut().zip(f) {
            *a = a.max(v);
        }
        self.n += 1;
        let hop = ((HOP as f32 * sr / 48_000.0) as usize).max(1);
        if self.n >= hop {
            tap.set_rate(sr / hop as f32);
            tap.push(self.acc);
            self.n = 0;
            self.acc = [0.0; 4];
        }
    }
}

// ----------------------------------------------------------------------------
// Gate.
// ----------------------------------------------------------------------------

pub const GATE_THRESH_MIN: f32 = -70.0;
pub const GATE_THRESH_MAX: f32 = 6.0;
pub const GATE_RETURN_MAX: f32 = 24.0;
pub const GATE_ATTACK: (f32, f32) = (0.02, 150.0);
pub const GATE_HOLD: (f32, f32) = (0.0, 500.0);
pub const GATE_RELEASE: (f32, f32) = (0.01, 1000.0);
/// Floor at or below this is −inf.
pub const GATE_FLOOR_MIN: f32 = -70.0;
/// Lookahead choices (ms).
pub const GATE_LOOKAHEAD: [f32; 3] = [0.0, 1.5, 10.0];

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GateParams {
    pub threshold: f32,
    /// Hysteresis (dB below the threshold where the gate closes again).
    pub ret: f32,
    pub flip: bool,
    /// Index into [`GATE_LOOKAHEAD`].
    pub lookahead: u8,
    pub attack: f32,
    pub hold: f32,
    pub release: f32,
    pub floor: f32,
    pub sc: Sidechain,
}

impl Default for GateParams {
    fn default() -> Self {
        Self {
            threshold: -12.0,
            ret: 8.91,
            flip: false,
            lookahead: 1,
            attack: 3.5,
            hold: 10.0,
            release: 15.0,
            floor: -40.0,
            sc: Sidechain::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GateState {
    pub(crate) sc: ScState,
    delay: Delay,
    open: bool,
    hold_left: usize,
    env: f32,
    det: f32,
    hist: Hist,
    tap: Option<Arc<DevTap>>,
}

impl GateState {
    pub fn new(id: Option<u64>) -> Self {
        Self {
            sc: ScState::default(),
            delay: Delay::new(),
            open: false,
            hold_left: 0,
            env: 0.0,
            det: 0.0,
            hist: Hist::default(),
            tap: id.map(dev_tap),
        }
    }

    pub fn reset(&mut self) {
        self.sc.reset();
        self.delay.reset();
        self.open = false;
        self.hold_left = 0;
        self.env = 0.0;
        self.det = 0.0;
    }

    pub fn process(&mut self, p: &GateParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let la = (GATE_LOOKAHEAD[(p.lookahead as usize).min(2)] * 0.001 * sr).round() as usize;
        let g_ext = self.sc.prepare(&p.sc, sr);
        let open_at = db_lin(p.threshold);
        let close_at = db_lin(p.threshold - p.ret.clamp(0.0, GATE_RETURN_MAX));
        let floor = if p.floor <= GATE_FLOOR_MIN { 0.0 } else { db_lin(p.floor.min(0.0)) };
        let atk = 1.0 / (p.attack.clamp(GATE_ATTACK.0, GATE_ATTACK.1) * 0.001 * sr).max(1.0);
        let rel = 1.0 / (p.release.clamp(GATE_RELEASE.0, GATE_RELEASE.1) * 0.001 * sr).max(1.0);
        let hold = (p.hold.clamp(GATE_HOLD.0, GATE_HOLD.1) * 0.001 * sr) as usize;
        // Peak detector: instant rise, ~10 ms fall (keeps the gate from chattering on waves).
        let det_fall = coef(10.0, sr);
        for i in 0..l.len() {
            let (dl, dr) = self.sc.detect(&p.sc, i, l[i], r[i], g_ext);
            let x = dl.abs().max(dr.abs());
            self.det = if x > self.det { x } else { x + (self.det - x) * det_fall };
            // Hysteresis: opens above the threshold, closes below threshold − return.
            let above = if self.open { self.det >= close_at } else { self.det >= open_at };
            let want = above != p.flip;
            if want {
                self.open = true;
                self.hold_left = hold;
            } else if self.hold_left > 0 {
                self.hold_left -= 1;
            } else {
                self.open = false;
            }
            // Linear-in-time ramps over Attack / Release.
            self.env = if self.open { (self.env + atk).min(1.0) } else { (self.env - rel).max(0.0) };
            let g = floor + (1.0 - floor) * self.env;
            let [a, b] = self.delay.tick([l[i], r[i]], la);
            let (inp, sc_lvl) = (a.abs().max(b.abs()), x);
            if p.sc.listen {
                l[i] = dl;
                r[i] = dr;
            } else {
                l[i] = a * g;
                r[i] = b * g;
            }
            if let Some(t) = &self.tap {
                self.hist.add(t, sr, [inp, (a * g).abs().max((b * g).abs()), sc_lvl, 1.0 - g]);
            }
        }
        if let Some(t) = &self.tap {
            // Attenuation now (dB ≥ 0) for the meter.
            t.set(0, -lin_db(floor + (1.0 - floor) * self.env));
        }
    }
}

// ----------------------------------------------------------------------------
// Glue Compressor (SSL-style bus compressor).
// ----------------------------------------------------------------------------

pub const GLUE_THRESH_MIN: f32 = -40.0;
pub const GLUE_RATIOS: [f32; 3] = [2.0, 4.0, 10.0];
pub const GLUE_ATTACKS: [f32; 7] = [0.01, 0.1, 0.3, 1.0, 3.0, 10.0, 30.0];
/// Seconds; the last entry is Auto.
pub const GLUE_RELEASES: [f32; 7] = [0.1, 0.2, 0.4, 0.6, 0.8, 1.2, 0.0];
pub const GLUE_MAKEUP_MAX: f32 = 20.0;
pub const GLUE_RANGE_MAX: f32 = 70.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GlueParams {
    pub threshold: f32,
    /// Index into [`GLUE_RATIOS`].
    pub ratio: u8,
    /// Index into [`GLUE_ATTACKS`].
    pub attack: u8,
    /// Index into [`GLUE_RELEASES`] (6 = Auto).
    pub release: u8,
    pub makeup: f32,
    pub range: f32,
    /// 0..1.
    pub dry_wet: f32,
    pub soft_clip: bool,
    pub oversample: bool,
    pub sc: Sidechain,
}

impl Default for GlueParams {
    fn default() -> Self {
        Self {
            threshold: 0.0,
            ratio: 1,
            attack: 3,
            release: 3,
            makeup: 0.0,
            range: 70.0,
            dry_wet: 1.0,
            soft_clip: false,
            oversample: false,
            sc: Sidechain::default(),
        }
    }
}

/// Soft clipper ceiling (−0.5 dB).
const CLIP_C: f32 = 0.944_060_9;

#[inline]
fn soft_clip(x: f32) -> f32 {
    CLIP_C * (x / CLIP_C).tanh()
}

#[derive(Debug, Clone)]
pub struct GlueState {
    pub(crate) sc: ScState,
    /// Gain reduction (dB, ≥ 0): smoothed, and the slow auto-release part.
    gr: f32,
    gr_slow: f32,
    hb: Box<[f64; OS_TAPS]>,
    up: [Fir; 2],
    down: [Fir; 2],
    tap: Option<Arc<DevTap>>,
}

impl GlueState {
    pub fn new(id: Option<u64>) -> Self {
        Self {
            sc: ScState::default(),
            gr: 0.0,
            gr_slow: 0.0,
            hb: Box::new(halfband()),
            up: Default::default(),
            down: Default::default(),
            tap: id.map(dev_tap),
        }
    }

    pub fn reset(&mut self) {
        self.sc.reset();
        self.gr = 0.0;
        self.gr_slow = 0.0;
        self.up = Default::default();
        self.down = Default::default();
    }

    /// Static curve: gain reduction (dB) for detector level `x` dB. The knee
    /// narrows as the ratio rises (Live: no knee control).
    pub fn curve(p: &GlueParams, x: f32) -> f32 {
        let ratio = GLUE_RATIOS[(p.ratio as usize).min(2)];
        let knee = match p.ratio {
            0 => 10.0,
            1 => 6.0,
            _ => 3.0,
        };
        let t = p.threshold.clamp(GLUE_THRESH_MIN, 0.0);
        let s = 1.0 - 1.0 / ratio;
        let d = x - t;
        let gr = if d <= -knee / 2.0 {
            0.0
        } else if d >= knee / 2.0 {
            d * s
        } else {
            s * (d + knee / 2.0).powi(2) / (2.0 * knee)
        };
        gr.min(p.range.clamp(0.0, GLUE_RANGE_MAX))
    }

    pub fn process(&mut self, p: &GlueParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let g_ext = self.sc.prepare(&p.sc, sr);
        let atk = coef(GLUE_ATTACKS[(p.attack as usize).min(6)], sr);
        let rel_s = GLUE_RELEASES[(p.release as usize).min(6)];
        let auto = rel_s <= 0.0;
        let (rel_fast, rel_slow) = if auto { (coef(100.0, sr), coef(1200.0, sr)) } else { (coef(rel_s * 1000.0, sr), 0.0) };
        let slow_atk = coef(300.0, sr);
        let makeup = db_lin(p.makeup.clamp(0.0, GLUE_MAKEUP_MAX));
        let wet = p.dry_wet.clamp(0.0, 1.0);
        let mut clip = 0.0f32;
        let mut gr_max = 0.0f32;
        for i in 0..l.len() {
            let (dl, dr) = self.sc.detect(&p.sc, i, l[i], r[i], g_ext);
            let x = lin_db(dl.abs().max(dr.abs()));
            let target = Self::curve(p, x);
            // Attack toward more reduction, release back (dual release when Auto).
            if target > self.gr {
                self.gr = target + (self.gr - target) * atk;
            } else if auto {
                self.gr_slow = if target > self.gr_slow {
                    target + (self.gr_slow - target) * slow_atk
                } else {
                    target + (self.gr_slow - target) * rel_slow
                };
                let fast = target + (self.gr - target) * rel_fast;
                self.gr = fast.max(self.gr_slow.min(self.gr));
            } else {
                self.gr = target + (self.gr - target) * rel_fast;
            }
            gr_max = gr_max.max(self.gr);
            let g = db_lin(-self.gr) * makeup;
            let (a0, b0) = (l[i], r[i]);
            let (mut a, mut b) = (a0 * g, b0 * g);
            a = a0 * (1.0 - wet) + a * wet;
            b = b0 * (1.0 - wet) + b * wet;
            if p.sc.listen {
                l[i] = dl;
                r[i] = dr;
                continue;
            }
            let pk = a.abs().max(b.abs());
            if p.soft_clip {
                if pk > CLIP_C * 0.9 {
                    clip = clip.max(2.0);
                }
                if p.oversample {
                    a = self.os_clip(0, a);
                    b = self.os_clip(1, b);
                } else {
                    a = soft_clip(a);
                    b = soft_clip(b);
                }
            } else if pk > 1.0 {
                clip = clip.max(1.0);
            }
            l[i] = a;
            r[i] = b;
        }
        if let Some(t) = &self.tap {
            t.set(0, self.gr);
            t.raise(1, gr_max);
            t.raise(2, clip);
        }
    }

    /// Soft clip at twice the rate (halfband up / down).
    fn os_clip(&mut self, c: usize, x: f32) -> f32 {
        let h = &*self.hb;
        let mut y = 0.0;
        for k in 0..2 {
            self.up[c].push(if k == 0 { 2.0 * x as f64 } else { 0.0 });
            let u = self.up[c].out(h);
            self.down[c].push(soft_clip(u as f32) as f64);
            if k == 1 {
                y = self.down[c].out(h);
            }
        }
        y as f32
    }
}

// ----------------------------------------------------------------------------
// Limiter.
// ----------------------------------------------------------------------------

pub const LIM_GAIN: (f32, f32) = (-24.0, 24.0);
pub const LIM_CEIL_MIN: f32 = -24.0;
pub const LIM_RELEASE: (f32, f32) = (0.01, 3000.0);
pub const LIM_LOOKAHEAD: [f32; 3] = [1.5, 3.0, 6.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LimMode {
    #[default]
    Standard,
    SoftClip,
    TruePeak,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LimiterParams {
    pub input: f32,
    /// Maximize: Ceiling → Threshold, Input Gain → Output.
    pub maximize: bool,
    pub output: f32,
    pub threshold: f32,
    pub ceiling: f32,
    pub release: f32,
    pub auto_release: bool,
    /// Index into [`LIM_LOOKAHEAD`].
    pub lookahead: u8,
    pub mode: LimMode,
    pub mid_side: bool,
    /// 0..1.
    pub link: f32,
}

impl Default for LimiterParams {
    fn default() -> Self {
        Self {
            input: 0.0,
            maximize: false,
            output: 0.0,
            threshold: 0.0,
            ceiling: -0.3,
            release: 100.0,
            auto_release: false,
            lookahead: 0,
            mode: LimMode::Standard,
            mid_side: false,
            link: 1.0,
        }
    }
}

impl LimiterParams {
    /// (input gain, ceiling) as linear factors after the Maximize mapping.
    pub fn stage(&self) -> (f32, f32) {
        if self.maximize {
            let t = self.threshold.clamp(LIM_CEIL_MIN, 0.0);
            (db_lin(-t), db_lin(self.output.clamp(LIM_CEIL_MIN, 0.0)))
        } else {
            (db_lin(self.input.clamp(LIM_GAIN.0, LIM_GAIN.1)), db_lin(self.ceiling.clamp(LIM_CEIL_MIN, 0.0)))
        }
    }
}

/// Sliding-window minimum over the last `w` samples (monotonic deque in a
/// fixed ring).
#[derive(Debug, Clone)]
struct MinWin {
    v: Box<[(usize, f32)]>,
    head: usize,
    len: usize,
    t: usize,
}

impl MinWin {
    fn new() -> Self {
        Self { v: vec![(0, 1.0); LA_CAP].into_boxed_slice(), head: 0, len: 0, t: 0 }
    }
    fn reset(&mut self) {
        self.head = 0;
        self.len = 0;
    }
    #[inline]
    fn push(&mut self, x: f32, w: usize) -> f32 {
        let m = LA_CAP - 1;
        while self.len > 0 && self.v[(self.head + self.len - 1) & m].1 >= x {
            self.len -= 1;
        }
        self.v[(self.head + self.len) & m] = (self.t, x);
        self.len += 1;
        while self.t - self.v[self.head].0 >= w.max(1) {
            self.head = (self.head + 1) & m;
            self.len -= 1;
        }
        self.t += 1;
        self.v[self.head].1
    }
}

/// Moving average over `w` samples (fixed ring).
#[derive(Debug, Clone)]
struct BoxAvg {
    buf: Box<[f32]>,
    pos: usize,
    sum: f64,
    w: usize,
}

impl BoxAvg {
    fn new() -> Self {
        Self { buf: vec![1.0; LA_CAP].into_boxed_slice(), pos: 0, sum: 0.0, w: 0 }
    }
    fn reset(&mut self) {
        self.buf.fill(1.0);
        self.w = 0;
    }
    #[inline]
    fn push(&mut self, x: f32, w: usize) -> f32 {
        let w = w.clamp(1, LA_CAP - 1);
        if w != self.w {
            self.w = w;
            self.sum = (0..w).map(|k| self.buf[(self.pos + LA_CAP - k) & (LA_CAP - 1)] as f64).sum();
        }
        self.pos = (self.pos + 1) & (LA_CAP - 1);
        let old = self.buf[(self.pos + LA_CAP - w) & (LA_CAP - 1)];
        self.buf[self.pos] = x;
        self.sum += x as f64 - old as f64;
        (self.sum / w as f64) as f32
    }
}

#[derive(Debug, Clone)]
pub struct LimiterState {
    delay: Delay,
    win: [MinWin; 2],
    avg: [BoxAvg; 2],
    /// Gain after release smoothing (per channel), and the slow auto part.
    env: [f32; 2],
    slow: [f32; 2],
    /// Last 3 samples per channel (true-peak interpolation).
    prev: [[f32; 3]; 2],
    tap: Option<Arc<DevTap>>,
}

impl LimiterState {
    pub fn new(id: Option<u64>) -> Self {
        Self {
            delay: Delay::new(),
            win: [MinWin::new(), MinWin::new()],
            avg: [BoxAvg::new(), BoxAvg::new()],
            env: [1.0; 2],
            slow: [1.0; 2],
            prev: [[0.0; 3]; 2],
            tap: id.map(dev_tap),
        }
    }

    pub fn reset(&mut self) {
        self.delay.reset();
        self.win.iter_mut().for_each(MinWin::reset);
        self.avg.iter_mut().for_each(BoxAvg::reset);
        self.env = [1.0; 2];
        self.slow = [1.0; 2];
        self.prev = [[0.0; 3]; 2];
    }

    /// Lookahead latency in samples.
    pub fn latency(p: &LimiterParams, sr: f32) -> usize {
        ((LIM_LOOKAHEAD[(p.lookahead as usize).min(2)] * 0.001 * sr).round() as usize).clamp(1, LA_CAP - 2)
    }

    pub fn process(&mut self, p: &LimiterParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let (gin, ceil) = p.stage();
        let la = Self::latency(p, sr);
        let rel = coef(p.release.clamp(LIM_RELEASE.0, LIM_RELEASE.1), sr);
        let (rel_fast, rel_slow) = (coef(30.0, sr), coef(600.0, sr));
        let link = p.link.clamp(0.0, 1.0);
        let ms = p.mid_side;
        // M/S: each of mid and side gets half the ceiling, so L = M + S stays below it.
        let c = if ms { ceil * 0.5 } else { ceil };
        let soft = p.mode == LimMode::SoftClip;
        let tp = p.mode == LimMode::TruePeak;
        let (mut pk_in, mut gr, mut clipped) = ([0.0f32; 2], [0.0f32; 2], false);
        for i in 0..l.len() {
            let (mut a, mut b) = (l[i] * gin, r[i] * gin);
            pk_in = [pk_in[0].max(a.abs()), pk_in[1].max(b.abs())];
            if ms {
                (a, b) = (0.5 * (a + b), 0.5 * (a - b));
            }
            if soft {
                // Soft clip toward the ceiling; the limiter catches the rest.
                let k = 1.12 * c;
                for v in [&mut a, &mut b] {
                    if v.abs() > 0.7 * c {
                        clipped = true;
                    }
                    *v = k * (*v / k).tanh();
                }
            }
            // Required gain per channel (true peak: cubic interpolation between samples).
            let mut need = [0.0f32; 2];
            for (ch, x) in [a, b].into_iter().enumerate() {
                let mut pk = x.abs();
                if tp {
                    let [p0, p1, p2] = self.prev[ch];
                    for t in [0.25f32, 0.5, 0.75] {
                        // Catmull-Rom between p1 and p2 (p0, p1, p2, x).
                        let y = 0.5
                            * ((2.0 * p1)
                                + (-p0 + p2) * t
                                + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - x) * t * t
                                + (-p0 + 3.0 * p1 - 3.0 * p2 + x) * t * t * t);
                        pk = pk.max(y.abs());
                    }
                    self.prev[ch] = [p1, p2, x];
                }
                need[ch] = if pk > c { c / pk } else { 1.0 };
            }
            let linked = need[0].min(need[1]);
            let mut g = [0.0f32; 2];
            for ch in 0..2 {
                // Link in dB: 0 % independent, 100 % the common minimum.
                let n = db_lin(lin_db(need[ch]) * (1.0 - link) + lin_db(linked) * link);
                let m = self.win[ch].push(n, la + 1);
                // Instant attack, release toward unity (dual when Auto).
                let e = if m < self.env[ch] {
                    m
                } else if p.auto_release {
                    self.slow[ch] = if m < self.slow[ch] { m + (self.slow[ch] - m) * rel_fast } else { m + (self.slow[ch] - m) * rel_slow };
                    let f = m + (self.env[ch] - m) * rel_fast;
                    f.min(self.slow[ch].max(self.env[ch]))
                } else {
                    m + (self.env[ch] - m) * rel
                };
                self.env[ch] = e;
                // The box filter spreads the drop over the lookahead: the gain
                // is down before the peak leaves the delay line.
                g[ch] = self.avg[ch].push(e, la).min(1.0);
                gr[ch] = gr[ch].max(-lin_db(g[ch]));
            }
            let [da, db] = self.delay.tick([a, b], la);
            let (mut x, mut y) = (da * g[0], db * g[1]);
            // Safety: never above the ceiling.
            x = x.clamp(-c, c);
            y = y.clamp(-c, c);
            if ms {
                (x, y) = (x + y, x - y);
            }
            l[i] = x;
            r[i] = y;
        }
        if let Some(t) = &self.tap {
            t.raise(0, pk_in[0]);
            t.raise(1, pk_in[1]);
            t.raise(2, gr[0]);
            t.raise(3, gr[1]);
            if clipped {
                t.set(4, 1.0);
            }
        }
    }
}

// ----------------------------------------------------------------------------
// Compressor (Live 12): Peak / RMS / Expand, knee, lookahead, Lin / Log
// envelope, auto release, makeup, output, dry/wet, full sidechain.
// ----------------------------------------------------------------------------

pub const COMP_THRESH: (f32, f32) = (-70.0, 6.0);
pub const COMP_ATTACK: (f32, f32) = (0.01, 1000.0);
pub const COMP_RELEASE: (f32, f32) = (1.0, 3000.0);
pub const COMP_KNEE_MAX: f32 = 18.0;
pub const COMP_OUT_MAX: f32 = 36.0;
/// Lookahead choices (ms).
pub const COMP_LOOKAHEAD: [f32; 3] = [0.0, 1.0, 10.0];
/// Ratios at or above this are ∞ : 1.
pub const COMP_RATIO_INF: f32 = 100.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CompMode {
    Peak,
    #[default]
    Rms,
    Expand,
}

/// Display: collapsed (meters only), transfer curve, activity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CompView {
    Collapsed,
    Curve,
    #[default]
    Activity,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CompressorParams {
    pub threshold: f32,
    /// Compression ratio (1 ..= [`COMP_RATIO_INF`]); Expand derives its
    /// own ratio from it ([`CompressorParams::expand_ratio`]).
    pub ratio: f32,
    pub attack: f32,
    pub release: f32,
    pub auto_release: bool,
    pub knee: f32,
    /// Index into [`COMP_LOOKAHEAD`].
    pub lookahead: u8,
    /// Envelope: logarithmic (true) or linear.
    pub log_env: bool,
    pub mode: CompMode,
    pub makeup: bool,
    pub output: f32,
    pub dry_wet: f32,
    pub view: CompView,
    /// Activity view shows the output level instead of the gain reduction.
    pub show_output: bool,
    pub sc: Sidechain,
}

impl Default for CompressorParams {
    fn default() -> Self {
        Self {
            threshold: 0.0,
            ratio: 4.0,
            attack: 1.0,
            release: 30.0,
            auto_release: false,
            knee: 6.0,
            lookahead: 0,
            log_env: true,
            mode: CompMode::Rms,
            makeup: false,
            output: 0.0,
            dry_wet: 1.0,
            view: CompView::Activity,
            show_output: false,
            sc: Sidechain { freq: 80.0, eq_on: true, ..Sidechain::default() },
        }
    }
}

impl CompressorParams {
    /// Expansion ratio 1 : x shown in Expand mode (Live: 3.38 → 1.15).
    pub fn expand_ratio(&self) -> f32 {
        1.0 + 0.2136 * (1.0 - 1.0 / self.ratio.clamp(1.0, COMP_RATIO_INF))
    }

    fn slope(&self) -> f32 {
        if self.ratio >= COMP_RATIO_INF { 1.0 } else { 1.0 - 1.0 / self.ratio.max(1.0) }
    }

    /// Static curve: gain change (dB, positive = reduction) at detector
    /// level `x` dB. Soft knee from threshold − knee to threshold + knee
    /// (Live: 10 dB knee at −20 dB starts at −30, full at −10).
    pub fn curve(&self, x: f32) -> f32 {
        let w = 2.0 * self.knee.clamp(0.0, COMP_KNEE_MAX);
        let (s, d) = match self.mode {
            CompMode::Expand => (self.expand_ratio() - 1.0, self.threshold - x),
            _ => (self.slope(), x - self.threshold),
        };
        if 2.0 * d <= -w {
            0.0
        } else if 2.0 * d >= w {
            s * d
        } else {
            s * (d + w / 2.0).powi(2) / (2.0 * w)
        }
    }

    /// Output level (dB) of the static curve for input `x` dB (no makeup).
    pub fn transfer(&self, x: f32) -> f32 {
        x - self.curve(x)
    }

    /// Auto makeup (dB): undoes the reduction of a 0 dBFS signal. Not with
    /// an external sidechain or in Expand mode.
    pub fn makeup_db(&self) -> f32 {
        if !self.makeup || self.mode == CompMode::Expand || (self.sc.on && self.sc.source != 0) {
            0.0
        } else {
            self.curve(0.0)
        }
    }

    pub fn latency(&self, sr: f32) -> usize {
        (COMP_LOOKAHEAD[(self.lookahead as usize).min(2)] * 0.001 * sr).round() as usize
    }
}

#[derive(Debug, Clone)]
pub struct CompressorState {
    pub(crate) sc: ScState,
    delay: Delay,
    /// RMS mean square, smoothed reduction (dB) and its slow auto part,
    /// linear-envelope gain.
    ms: f32,
    gr: f32,
    gr_slow: f32,
    g_lin: f32,
    hist: Hist,
    tap: Option<Arc<DevTap>>,
}

/// Tap slots: 0 GR now (dB), 1/2 input / output peak (held), 3 detector
/// level now (dB), 4 GR peak (held), 5/6 input / output RMS.
impl CompressorState {
    pub fn new(id: Option<u64>) -> Self {
        Self {
            sc: ScState::default(),
            delay: Delay::new(),
            ms: 0.0,
            gr: 0.0,
            gr_slow: 0.0,
            g_lin: 1.0,
            hist: Hist::default(),
            tap: id.map(dev_tap),
        }
    }

    pub fn reset(&mut self) {
        self.sc.reset();
        self.delay.reset();
        self.ms = 0.0;
        self.gr = 0.0;
        self.gr_slow = 0.0;
        self.g_lin = 1.0;
    }

    pub fn process(&mut self, p: &CompressorParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let la = p.latency(sr);
        let g_ext = self.sc.prepare(&p.sc, sr);
        let atk = coef(p.attack.clamp(COMP_ATTACK.0, COMP_ATTACK.1), sr);
        let rel_ms = p.release.clamp(COMP_RELEASE.0, COMP_RELEASE.1);
        // Auto release: a fast stage for transients over a slow one for
        // sustained reduction (program dependent).
        let (rel, rel_slow) = if p.auto_release { (coef(40.0, sr), coef(1500.0, sr)) } else { (coef(rel_ms, sr), 0.0) };
        let slow_atk = coef(400.0, sr);
        let rms_c = coef(10.0, sr);
        let post = db_lin(p.makeup_db() + p.output.clamp(-COMP_OUT_MAX, COMP_OUT_MAX));
        let wet = p.dry_wet.clamp(0.0, 1.0);
        let (mut in_pk, mut out_pk, mut gr_pk, mut in_ms, mut out_ms) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let mut det_db = -120.0;
        for i in 0..l.len() {
            let (dl, dr) = self.sc.detect(&p.sc, i, l[i], r[i], g_ext);
            let x = match p.mode {
                CompMode::Peak => dl.abs().max(dr.abs()),
                _ => {
                    let sq = 0.5 * (dl * dl + dr * dr);
                    self.ms = sq + (self.ms - sq) * rms_c;
                    // √2: a full-scale sine reads 0 dB like the peak detector.
                    (2.0 * self.ms).sqrt()
                }
            };
            det_db = lin_db(x);
            let target = p.curve(det_db);
            let g = if p.log_env {
                // Logarithmic: smoothing in dB (deep reduction recovers fast).
                if target > self.gr {
                    self.gr = target + (self.gr - target) * atk;
                } else if p.auto_release {
                    self.gr_slow = if target > self.gr_slow {
                        target + (self.gr_slow - target) * slow_atk
                    } else {
                        target + (self.gr_slow - target) * rel_slow
                    };
                    let fast = target + (self.gr - target) * rel;
                    self.gr = fast.max(self.gr_slow.min(self.gr));
                } else {
                    self.gr = target + (self.gr - target) * rel;
                }
                db_lin(-self.gr)
            } else {
                // Linear: smoothing of the gain factor itself.
                let gt = db_lin(-target);
                let c = if gt < self.g_lin {
                    atk
                } else if p.auto_release {
                    if self.g_lin < 0.7 { rel_slow.min(coef(400.0, sr)) } else { rel }
                } else {
                    rel
                };
                self.g_lin = gt + (self.g_lin - gt) * c;
                self.gr = -lin_db(self.g_lin);
                self.g_lin
            };
            let [a0, b0] = self.delay.tick([l[i], r[i]], la);
            let (a, b) = (a0 * g * post, b0 * g * post);
            let (a, b) = (a0 * (1.0 - wet) + a * wet, b0 * (1.0 - wet) + b * wet);
            let ip = a0.abs().max(b0.abs());
            in_pk = in_pk.max(ip);
            in_ms += 0.5 * (a0 * a0 + b0 * b0);
            gr_pk = gr_pk.max(self.gr);
            if p.sc.listen {
                l[i] = dl;
                r[i] = dr;
            } else {
                l[i] = a;
                r[i] = b;
            }
            let op = a.abs().max(b.abs());
            out_pk = out_pk.max(op);
            out_ms += 0.5 * (a * a + b * b);
            if let Some(t) = &self.tap {
                // History: input, output, GR (dB / 100), detector.
                self.hist.add(t, sr, [ip, op, self.gr.max(0.0) / 100.0, x]);
            }
        }
        if let Some(t) = &self.tap {
            let n = l.len().max(1) as f32;
            t.set(0, self.gr);
            t.raise(1, in_pk);
            t.raise(2, out_pk);
            t.set(3, det_db);
            t.raise(4, gr_pk);
            t.raise(5, (in_ms / n).sqrt());
            t.raise(6, (out_ms / n).sqrt());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(n: usize, hz: f32, amp: f32) -> Vec<f32> {
        (0..n).map(|i| amp * (2.0 * std::f32::consts::PI * hz * i as f32 / 48_000.0).sin()).collect()
    }

    #[test]
    fn gate_opens_above_threshold_and_floors_below() {
        let p = GateParams {
            threshold: -20.0,
            ret: 3.0,
            floor: -40.0,
            lookahead: 0,
            attack: 0.02,
            release: 1.0,
            hold: 0.0,
            ..Default::default()
        };
        let mut st = GateState::new(None);
        // Loud: passes.
        let (mut l, mut r) = (sine(4800, 440.0, 0.5), sine(4800, 440.0, 0.5));
        st.process(&p, &mut l, &mut r, 48_000.0);
        let pk = l[2400..].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(pk > 0.49, "{pk}");
        // Quiet (−40 dBFS): floored by 40 dB.
        let (mut l, mut r) = (sine(9600, 440.0, 0.01), sine(9600, 440.0, 0.01));
        st.process(&p, &mut l, &mut r, 48_000.0);
        let pk = l[4800..].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(pk < 0.0002, "{pk}");
        // Flip: the quiet signal passes.
        let pf = GateParams { flip: true, ..p };
        let mut st = GateState::new(None);
        let (mut l, mut r) = (sine(9600, 440.0, 0.01), sine(9600, 440.0, 0.01));
        st.process(&pf, &mut l, &mut r, 48_000.0);
        assert!(l[4800..].iter().fold(0.0f32, |m, x| m.max(x.abs())) > 0.0099);
    }

    #[test]
    fn gate_external_sidechain_triggers() {
        let mut p = GateParams { threshold: -20.0, lookahead: 0, attack: 0.02, hold: 0.0, release: 1.0, ..Default::default() };
        p.sc.on = true;
        p.sc.source = 7;
        let mut st = GateState::new(None);
        // Own signal quiet, the sidechain loud → opens.
        let ext = sine(4096, 100.0, 0.8);
        st.sc.ext_l[..4096].copy_from_slice(&ext);
        st.sc.ext_r[..4096].copy_from_slice(&ext);
        st.sc.len = 4096;
        let (mut l, mut r) = (sine(4096, 440.0, 0.01), sine(4096, 440.0, 0.01));
        st.process(&p, &mut l, &mut r, 48_000.0);
        assert!(l[2400..].iter().fold(0.0f32, |m, x| m.max(x.abs())) > 0.0099);
    }

    #[test]
    fn compressor_curve_ratio_knee_expand() {
        let p = CompressorParams { threshold: -20.0, ratio: 4.0, knee: 0.0, ..Default::default() };
        assert_eq!(p.curve(-30.0), 0.0);
        assert!((p.curve(0.0) - 15.0).abs() < 1e-4, "20 dB over at 4:1 → 15 dB GR");
        let k = CompressorParams { knee: 10.0, ..p };
        assert_eq!(k.curve(-30.0), 0.0);
        assert!(k.curve(-25.0) > 0.0);
        assert!(k.curve(-20.0) > 0.0 && k.curve(-20.0) < 2.0);
        assert!((k.curve(-10.0) - 7.5).abs() < 1e-4);
        let e = CompressorParams { mode: CompMode::Expand, ratio: 3.38, ..p };
        assert!((e.expand_ratio() - 1.15).abs() < 0.005);
        assert_eq!(e.curve(-10.0), 0.0);
        assert!((e.curve(-40.0) - 20.0 * (e.expand_ratio() - 1.0)).abs() < 1e-3);
        let m = CompressorParams { makeup: true, ..p };
        assert!((m.makeup_db() - 15.0).abs() < 1e-4);
    }

    #[test]
    fn compressor_reduces_loud_signal_and_lookahead_delays() {
        let mut st = CompressorState::new(None);
        // DC test signal: the sidechain filter (on by default) would remove it.
        let mut p = CompressorParams { threshold: -20.0, ratio: 100.0, knee: 0.0, mode: CompMode::Peak, attack: 0.01, ..Default::default() };
        p.sc.eq_on = false;
        let (mut l, mut r) = (vec![0.5f32; 4800], vec![0.5f32; 4800]);
        st.process(&p, &mut l, &mut r, 48_000.0);
        let out = lin_db(l[4000]);
        assert!((out + 20.0).abs() < 0.5, "limited near the threshold: {out}");
        let q = CompressorParams { lookahead: 2, threshold: 6.0, ..p };
        assert_eq!(q.latency(48_000.0), 480);
        let mut st = CompressorState::new(None);
        let mut l = vec![0.0f32; 1000];
        l[0] = 1.0;
        let mut r = l.clone();
        st.process(&q, &mut l, &mut r, 48_000.0);
        assert_eq!(l.iter().position(|x| *x > 0.5), Some(480));
    }

    #[test]
    fn glue_curve_and_compression() {
        let p = GlueParams { threshold: -20.0, ratio: 2, ..Default::default() };
        assert_eq!(GlueState::curve(&p, -40.0), 0.0);
        assert!((GlueState::curve(&p, 0.0) - 18.0).abs() < 1e-4, "ratio 10: 20 dB over → 18 dB GR");
        let pr = GlueParams { range: 5.0, ..p };
        assert_eq!(GlueState::curve(&pr, 0.0), 5.0);
        let mut st = GlueState::new(None);
        let p = GlueParams { threshold: -20.0, ratio: 2, soft_clip: false, attack: 0, ..Default::default() };
        let (mut l, mut r) = (sine(48_000, 200.0, 1.0), sine(48_000, 200.0, 1.0));
        st.process(&p, &mut l, &mut r, 48_000.0);
        let pk = l[24_000..].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(pk < 0.3, "{pk}");
    }

    #[test]
    fn glue_soft_clip_caps_output() {
        let mut st = GlueState::new(None);
        let p = GlueParams { makeup: 20.0, soft_clip: true, ..Default::default() };
        let (mut l, mut r) = (sine(4800, 200.0, 1.0), sine(4800, 200.0, 1.0));
        st.process(&p, &mut l, &mut r, 48_000.0);
        assert!(l.iter().all(|x| x.abs() <= CLIP_C + 1e-6));
    }

    #[test]
    fn limiter_holds_the_ceiling() {
        for mode in [LimMode::Standard, LimMode::SoftClip, LimMode::TruePeak] {
            for ms in [false, true] {
                let p = LimiterParams { input: 12.0, ceiling: -1.0, mode, mid_side: ms, ..Default::default() };
                let mut st = LimiterState::new(None);
                let (mut l, mut r) = (sine(48_000, 100.0, 0.9), sine(48_000, 150.0, 0.7));
                st.process(&p, &mut l, &mut r, 48_000.0);
                let c = db_lin(-1.0) + 1e-5;
                let pk = l.iter().chain(&r).fold(0.0f32, |m, x| m.max(x.abs()));
                assert!(pk <= c, "{mode:?} {ms} {pk}");
                assert!(pk > if ms { 0.4 } else { 0.6 }, "still loud {pk}");
            }
        }
    }

    #[test]
    fn limiter_transparent_below_ceiling_after_latency() {
        let p = LimiterParams::default();
        let mut st = LimiterState::new(None);
        let src = sine(4800, 300.0, 0.25);
        let (mut l, mut r) = (src.clone(), src.clone());
        st.process(&p, &mut l, &mut r, 48_000.0);
        let la = LimiterState::latency(&p, 48_000.0);
        for i in la..4800 {
            assert!((l[i] - src[i - la]).abs() < 1e-5);
        }
    }

    #[test]
    fn limiter_maximize_raises_by_threshold() {
        let p = LimiterParams { maximize: true, threshold: -6.0, output: 0.0, ..Default::default() };
        let (g, c) = p.stage();
        assert!((lin_db(g) - 6.0).abs() < 1e-4 && (c - 1.0).abs() < 1e-6);
    }
}
