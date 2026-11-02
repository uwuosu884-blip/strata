//! Echo (Live manual 29.14): two delay lines (Stereo / Ping Pong / Mid-Side),
//! synced or free times with offsets and Stereo Link, Input gain with dry
//! clipping, Feedback (to 150 %, inverted on request) through a resonant
//! HP / LP filter; an LFO + envelope modulation of time and filter; Gate,
//! Ducking, sampled Noise and Wobble (Ableton's sample set), Repitch (else
//! crossfades on time changes); a small reverb Pre / Post / in the feedback
//! loop; Stereo width, Output and Dry/Wet (optionally equal-loudness).

use crate::devtap::{DevTap, dev_tap};
use crate::dynfx::db_lin;
use serde::{Deserialize, Serialize};
use std::f32::consts::{FRAC_PI_2, TAU};
use std::sync::Arc;

pub const ECHO_MS: (f32, f32) = (1.0, 5000.0);
pub const ECHO_OFFSET: f32 = 0.333;
pub const ECHO_FB_MAX: f32 = 1.5;
/// Input / Output gain range (dB); the bottom is −inf.
pub const ECHO_GAIN_DB: (f32, f32) = (-70.0, 12.0);
pub const ECHO_FILT_HZ: (f32, f32) = (20.0, 20_000.0);
pub const ECHO_MOD_HZ: (f32, f32) = (0.01, 40.0);
pub const ECHO_THR_DB: (f32, f32) = (-60.0, 0.0);
pub const ECHO_REL_MS: (f32, f32) = (1.0, 1000.0);
pub const ECHO_SMOOTH_MS: (f32, f32) = (10.0, 4000.0);
pub const ECHO_STEREO_MAX: f32 = 2.0;
pub const ECHO_MODES: [&str; 3] = ["Stereo", "Ping Pong", "Mid/Side"];
pub const ECHO_SYNC_MODES: [&str; 4] = ["Synced", "Triplet", "Dotted", "16th"];
pub const ECHO_VERB_LOC: [&str; 3] = ["Pre", "Post", "Feedback"];
pub const ECHO_WAVES: [&str; 6] = ["Sine", "Triangle", "Saw Up", "Saw Down", "Square", "Noise"];
/// Note values (fractions of a whole note) for the Synced / Triplet / Dotted modes.
pub const ECHO_NOTES: [(u32, u32); 8] = [(1, 64), (1, 32), (1, 16), (1, 8), (1, 4), (1, 2), (1, 1), (2, 1)];
/// Modulation sync rates (whole notes per cycle).
pub const ECHO_MOD_DIVS: [(u32, u32); 11] = [(1, 64), (1, 32), (1, 16), (1, 8), (1, 4), (1, 2), (1, 1), (2, 1), (4, 1), (8, 1), (16, 1)];

/// Buffer length per channel (5.4 s at 96 kHz).
const BUF: usize = 1 << 19;
const XFADE_S: f32 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EchoParams {
    /// [`ECHO_MODES`] index.
    pub mode: u8,
    /// Per side (L/R or Mid/Side): synced, sync mode, note index / sixteenths, ms, offset.
    pub sync: [bool; 2],
    pub sync_mode: [u8; 2],
    /// [`ECHO_NOTES`] index (Synced / Triplet / Dotted) or 1..16 sixteenths.
    pub div: [u8; 2],
    pub sixteenths: [u8; 2],
    pub ms: [f32; 2],
    pub offset: [f32; 2],
    pub link: bool,
    pub input: f32,
    pub clip_dry: bool,
    pub feedback: f32,
    pub invert: bool,
    // Echo tab.
    pub tab: u8,
    pub filter_on: bool,
    pub filter_shown: bool,
    pub hp: f32,
    pub hp_res: f32,
    pub lp: f32,
    pub lp_res: f32,
    // Modulation.
    pub wave: u8,
    pub mod_sync: bool,
    pub mod_div: u8,
    pub mod_hz: f32,
    pub phase: f32,
    pub mod_delay: f32,
    pub mod_x4: bool,
    pub mod_filter: f32,
    pub env_mix: f32,
    // Character.
    pub gate: bool,
    pub gate_thr: f32,
    pub gate_rel: f32,
    pub duck: bool,
    pub duck_thr: f32,
    pub duck_rel: f32,
    pub noise: bool,
    pub noise_amt: f32,
    pub noise_morph: f32,
    pub wobble: bool,
    pub wobble_amt: f32,
    pub wobble_morph: f32,
    pub repitch: bool,
    pub smooth_ms: f32,
    // Global.
    pub reverb: f32,
    pub verb_loc: u8,
    pub verb_decay: f32,
    pub stereo: f32,
    pub output: f32,
    pub dry_wet: f32,
    pub equal_loud: bool,
}

impl Default for EchoParams {
    fn default() -> Self {
        Self {
            mode: 0,
            sync: [true; 2],
            sync_mode: [0; 2],
            div: [4; 2],
            sixteenths: [3; 2],
            ms: [375.0; 2],
            offset: [0.0, -0.01],
            link: true,
            input: 0.0,
            clip_dry: false,
            feedback: 0.5,
            invert: false,
            tab: 0,
            filter_on: true,
            filter_shown: false,
            hp: 100.0,
            hp_res: 0.0,
            lp: 10_000.0,
            lp_res: 0.0,
            wave: 0,
            mod_sync: false,
            mod_div: 4,
            mod_hz: 2.0,
            phase: 0.0,
            mod_delay: 0.0,
            mod_x4: false,
            mod_filter: 0.0,
            env_mix: 0.0,
            gate: false,
            gate_thr: 0.0,
            gate_rel: 50.0,
            duck: true,
            duck_thr: 0.0,
            duck_rel: 100.0,
            noise: false,
            noise_amt: 0.0,
            noise_morph: 0.71,
            wobble: true,
            wobble_amt: 0.023,
            wobble_morph: 0.0,
            repitch: false,
            smooth_ms: 400.0,
            reverb: 0.0,
            verb_loc: 1,
            verb_decay: 0.5,
            stereo: 1.0,
            output: 0.0,
            dry_wet: 0.3,
            equal_loud: false,
        }
    }
}

impl EchoParams {
    /// Side `s` as edited (Stereo Link: the left side drives both).
    fn side(&self, s: usize) -> usize {
        if self.link { 0 } else { s }
    }

    /// Delay time label of side `s` ("1/4", "3/16", "1/8T", "375 ms").
    pub fn time_label(&self, s: usize) -> String {
        let k = self.side(s);
        if !self.sync[k] {
            let ms = self.ms[k];
            return if ms >= 1000.0 { format!("{:.2} s", ms / 1000.0) } else { format!("{} ms", ms.round()) };
        }
        match self.sync_mode[k] {
            3 => format!("{}/16", self.sixteenths[k].clamp(1, 16)),
            m => {
                let (a, b) = ECHO_NOTES[(self.div[k] as usize).min(ECHO_NOTES.len() - 1)];
                let base = if b == 1 { format!("{a}") } else { format!("{a}/{b}") };
                match m {
                    1 => base + "T",
                    2 => base + "D",
                    _ => base,
                }
            }
        }
    }

    /// Base delay time of side `s` in seconds (before offset).
    pub fn base_secs(&self, s: usize, tempo: f64) -> f32 {
        let k = self.side(s);
        if !self.sync[k] {
            return self.ms[k].clamp(ECHO_MS.0, ECHO_MS.1) * 0.001;
        }
        let beat = 60.0 / tempo.max(1.0) as f32;
        let whole = match self.sync_mode[k] {
            3 => self.sixteenths[k].clamp(1, 16) as f32 / 16.0,
            m => {
                let (a, b) = ECHO_NOTES[(self.div[k] as usize).min(ECHO_NOTES.len() - 1)];
                let f = a as f32 / b as f32;
                match m {
                    1 => f * 2.0 / 3.0,
                    2 => f * 1.5,
                    _ => f,
                }
            }
        };
        whole * 4.0 * beat
    }

    /// Delay time of side `s` with its offset (seconds).
    pub fn secs(&self, s: usize, tempo: f64) -> f32 {
        (self.base_secs(s, tempo) * (1.0 + self.offset[s].clamp(-ECHO_OFFSET, ECHO_OFFSET))).max(0.0005)
    }

    /// Modulation rate (Hz).
    pub fn mod_rate(&self, tempo: f64) -> f32 {
        if self.mod_sync {
            let (a, b) = ECHO_MOD_DIVS[(self.mod_div as usize).min(ECHO_MOD_DIVS.len() - 1)];
            tempo as f32 / 60.0 / (4.0 * a as f32 / b as f32)
        } else {
            self.mod_hz.clamp(ECHO_MOD_HZ.0, ECHO_MOD_HZ.1)
        }
    }

    pub fn mod_div_label(i: usize) -> String {
        let (a, b) = ECHO_MOD_DIVS[i.min(ECHO_MOD_DIVS.len() - 1)];
        if b == 1 { format!("{a}") } else { format!("{a}/{b}") }
    }
}

/// LFO shape at phase `ph` (0..1); `rnd` is the held random value (Noise).
pub fn echo_wave(w: u8, ph: f32, rnd: f32) -> f32 {
    match w {
        0 => (TAU * ph).sin(),
        1 => 1.0 - 4.0 * (ph - 0.25 - (ph - 0.25).floor() - 0.5).abs(),
        2 => 2.0 * ph - 1.0,
        3 => 1.0 - 2.0 * ph,
        4 => {
            if ph < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        _ => rnd,
    }
}

// ---- embedded samples (Ableton's Echo noise / wobble set) ----

const NOISE: [&[u8]; 5] = [
    include_bytes!("../assets/echo/noise1.pcm"),
    include_bytes!("../assets/echo/noise2.pcm"),
    include_bytes!("../assets/echo/noise3.pcm"),
    include_bytes!("../assets/echo/noise4.pcm"),
    include_bytes!("../assets/echo/noise5.pcm"),
];
const WOBBLE: [&[u8]; 4] = [
    include_bytes!("../assets/echo/wobble1.pcm"),
    include_bytes!("../assets/echo/wobble2.pcm"),
    include_bytes!("../assets/echo/wobble3.pcm"),
    include_bytes!("../assets/echo/wobble4.pcm"),
];
const NOISE_SR: f32 = 44_100.0;
const WOBBLE_SR: f32 = 100.0;

#[inline]
fn pcm(b: &[u8], i: usize) -> f32 {
    let n = b.len() / 2;
    let i = (i % n) * 2;
    i16::from_le_bytes([b[i], b[i + 1]]) as f32 / 32768.0
}

/// Looped, linearly interpolated sample at position `pos` (in sample frames).
#[inline]
fn pcm_at(b: &[u8], pos: f32) -> f32 {
    let i = pos as usize;
    let f = pos - i as f32;
    let a = pcm(b, i);
    a + (pcm(b, i + 1) - a) * f
}

/// Morph `m` (0..1) across a sample set.
#[inline]
fn morph(set: &[&[u8]], m: f32, pos: f32) -> f32 {
    let x = m.clamp(0.0, 1.0) * (set.len() - 1) as f32;
    let i = (x as usize).min(set.len() - 2);
    let f = x - i as f32;
    pcm_at(set[i], pos) * (1.0 - f) + pcm_at(set[i + 1], pos) * f
}

// ---- building blocks ----

/// TPT state-variable filter (Zavalishin), low / high outputs.
#[derive(Debug, Clone, Copy, Default)]
struct Svf {
    s1: f32,
    s2: f32,
}

impl Svf {
    #[inline]
    fn tick(&mut self, x: f32, g: f32, k: f32) -> (f32, f32) {
        let hp = (x - (k + g) * self.s1 - self.s2) / (1.0 + g * (k + g));
        let bp = g * hp + self.s1;
        self.s1 = g * hp + bp;
        let lp = g * bp + self.s2;
        self.s2 = g * bp + lp;
        (lp, hp)
    }
}

#[inline]
fn svf_g(f: f32, sr: f32) -> f32 {
    (std::f32::consts::PI * (f / sr).min(0.49)).tan()
}

#[inline]
fn res_k(res: f32) -> f32 {
    2.0 - 1.9 * res.clamp(0.0, 1.0)
}

/// Bounded soft saturation (≈ x for small x).
#[inline]
fn soft(x: f32) -> f32 {
    x / (1.0 + x * x * 0.11).sqrt()
}

/// Small Schroeder reverb (4 combs + 2 allpasses per channel).
#[derive(Debug, Clone)]
struct MiniVerb {
    comb: [[Vec<f32>; 4]; 2],
    damp: [[f32; 4]; 2],
    ap: [[Vec<f32>; 2]; 2],
    pos: usize,
}

const COMB_N: [[usize; 4]; 2] = [[1557, 1617, 1491, 1422], [1580, 1640, 1514, 1445]];
const AP_N: [[usize; 2]; 2] = [[556, 441], [579, 464]];
/// The comb lengths are for 44.1 kHz; buffers hold up to 4x that.
const VERB_SCALE_MAX: usize = 5;

impl MiniVerb {
    fn new() -> Self {
        Self {
            comb: std::array::from_fn(|c| std::array::from_fn(|k| vec![0.0; COMB_N[c][k] * VERB_SCALE_MAX])),
            damp: [[0.0; 4]; 2],
            ap: std::array::from_fn(|c| std::array::from_fn(|k| vec![0.0; AP_N[c][k] * VERB_SCALE_MAX])),
            pos: 0,
        }
    }

    fn clear(&mut self) {
        for b in self.comb.iter_mut().flatten().chain(self.ap.iter_mut().flatten()) {
            b.fill(0.0);
        }
        self.damp = [[0.0; 4]; 2];
    }

    /// Wet output for `x` (stereo); `fb` = comb feedback, `sc` = length scale.
    #[inline]
    #[allow(clippy::needless_range_loop)]
    fn tick(&mut self, x: [f32; 2], fb: f32, sc: f32) -> [f32; 2] {
        self.pos = self.pos.wrapping_add(1);
        let mut out = [0.0; 2];
        for c in 0..2 {
            let inp = 0.5 * (x[0] + x[1]) * 0.3;
            let mut acc = 0.0;
            for k in 0..4 {
                let n = ((COMB_N[c][k] as f32 * sc) as usize).clamp(1, self.comb[c][k].len());
                let i = self.pos % n;
                let y = self.comb[c][k][i];
                self.damp[c][k] = y * 0.7 + self.damp[c][k] * 0.3;
                self.comb[c][k][i] = inp + self.damp[c][k] * fb;
                acc += y;
            }
            for k in 0..2 {
                let n = ((AP_N[c][k] as f32 * sc) as usize).clamp(1, self.ap[c][k].len());
                let i = self.pos % n;
                let b = self.ap[c][k][i];
                self.ap[c][k][i] = acc + b * 0.5;
                acc = b - acc;
            }
            out[c] = acc;
        }
        out
    }
}

/// Tap slots for the GUI.
pub mod etap {
    /// LFO phase (0..1, left).
    pub const LFO_PH: usize = 0;
    /// Current delay times (seconds, after modulation smoothing).
    pub const TIME_L: usize = 1;
    pub const TIME_R: usize = 2;
}

#[derive(Debug, Clone)]
pub struct EchoState {
    buf: Box<[Vec<f32>; 2]>,
    pos: usize,
    /// Read head times (samples): current, previous (crossfade), fade 0..1.
    cur: [f32; 2],
    old: [f32; 2],
    fade: [f32; 2],
    init: bool,
    out: [f32; 2],
    hp: [Svf; 2],
    lp: [Svf; 2],
    lfo: f32,
    rnd: [f32; 2],
    rnd_t: [f32; 2],
    ph_r_prev: f32,
    env: f32,
    gate_env: f32,
    gate_g: f32,
    duck_env: f32,
    noise_pos: f32,
    wob_pos: f32,
    rng: u32,
    verb: MiniVerb,
    tap: Option<Arc<DevTap>>,
}

impl Default for EchoState {
    fn default() -> Self {
        Self::new(None)
    }
}

impl EchoState {
    /// Allocates the delay buffers (control thread).
    pub fn new(id: Option<u64>) -> Self {
        Self {
            buf: Box::new([vec![0.0; BUF], vec![0.0; BUF]]),
            pos: 0,
            cur: [0.0; 2],
            old: [0.0; 2],
            fade: [1.0; 2],
            init: false,
            out: [0.0; 2],
            hp: Default::default(),
            lp: Default::default(),
            lfo: 0.0,
            rnd: [0.0; 2],
            rnd_t: [0.0; 2],
            ph_r_prev: 0.0,
            env: 0.0,
            gate_env: 0.0,
            gate_g: 1.0,
            duck_env: 0.0,
            noise_pos: 0.0,
            wob_pos: 0.0,
            rng: 0x9e37_79b9,
            verb: MiniVerb::new(),
            tap: id.map(dev_tap),
        }
    }

    /// Attaches the GUI tap without reallocating the buffers.
    pub fn set_tap(&mut self, id: u64) {
        self.tap = Some(dev_tap(id));
    }

    pub fn reset(&mut self) {
        for b in self.buf.iter_mut() {
            b.fill(0.0);
        }
        self.verb.clear();
        self.out = [0.0; 2];
        self.hp = Default::default();
        self.lp = Default::default();
        self.init = false;
        self.env = 0.0;
        self.gate_env = 0.0;
        self.gate_g = 1.0;
        self.duck_env = 0.0;
    }

    #[inline]
    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    /// Hermite read `d` samples behind the write head of channel `c`.
    #[inline]
    fn read(&self, c: usize, d: f32) -> f32 {
        let d = d.clamp(1.0, (BUF - 4) as f32);
        let di = d as usize;
        let fr = d - di as f32;
        let b = &self.buf[c];
        let at = |k: usize| b[(self.pos + BUF - k) & (BUF - 1)];
        let (xm1, x0, x1, x2) = (at(di - 1), at(di), at(di + 1), at(di + 2));
        let c1 = 0.5 * (x1 - xm1);
        let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
        ((c3 * fr + c2) * fr + c1) * fr + x0
    }

    pub fn process(&mut self, p: &EchoParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let n = l.len().min(r.len());
        let (tempo, _) = crate::modfx::clock();
        let g_in = if p.input <= ECHO_GAIN_DB.0 { 0.0 } else { db_lin(p.input.min(ECHO_GAIN_DB.1)) };
        let g_out = if p.output <= ECHO_GAIN_DB.0 { 0.0 } else { db_lin(p.output.min(ECHO_GAIN_DB.1)) };
        let fb = p.feedback.clamp(0.0, ECHO_FB_MAX) * if p.invert { -1.0 } else { 1.0 };
        let target = [p.secs(0, tempo) * sr, p.secs(1, tempo) * sr];
        let max_d = (BUF - 8) as f32;
        let target = [target[0].min(max_d), target[1].min(max_d)];
        if !self.init {
            self.cur = target;
            self.old = target;
            self.fade = [1.0; 2];
            self.init = true;
        }
        // Repitch glides the read head; otherwise a new time crossfades in.
        let k_glide = 1.0 - (-1.0 / (p.smooth_ms.clamp(ECHO_SMOOTH_MS.0, ECHO_SMOOTH_MS.1) * 0.001 * sr)).exp();
        let fade_inc = 1.0 / (XFADE_S * sr);
        let rate = p.mod_rate(tempo);
        let ph_off = p.phase.rem_euclid(360.0) / 360.0;
        let md = p.mod_delay.clamp(0.0, 1.0) * 0.05 * if p.mod_x4 { 4.0 } else { 1.0 };
        let mf = 2.0 * p.mod_filter.clamp(0.0, 1.0);
        let env_mix = p.env_mix.clamp(0.0, 1.0);
        let k_env = (-1.0 / (0.1 * sr)).exp();
        let filt = p.filter_on;
        let (hp_k, lp_k) = (res_k(p.hp_res), res_k(p.lp_res));
        let hp_f = p.hp.clamp(ECHO_FILT_HZ.0, ECHO_FILT_HZ.1);
        let lp_f = p.lp.clamp(ECHO_FILT_HZ.0, ECHO_FILT_HZ.1);
        let gate_thr = db_lin(p.gate_thr.clamp(ECHO_THR_DB.0, ECHO_THR_DB.1));
        let k_gate_rel = (-1.0 / (p.gate_rel.clamp(ECHO_REL_MS.0, ECHO_REL_MS.1) * 0.001 * sr)).exp();
        let k_att = (-1.0 / (0.001 * sr)).exp();
        let duck_thr = db_lin(p.duck_thr.clamp(ECHO_THR_DB.0, ECHO_THR_DB.1));
        let k_duck_rel = (-1.0 / (p.duck_rel.clamp(ECHO_REL_MS.0, ECHO_REL_MS.1) * 0.001 * sr)).exp();
        let noise_g = if p.noise { p.noise_amt.clamp(0.0, 1.0) * 0.3 } else { 0.0 };
        let wob_d = if p.wobble { p.wobble_amt.clamp(0.0, 1.0) * 0.003 * sr } else { 0.0 };
        let verb_amt = p.reverb.clamp(0.0, 1.0);
        let verb_fb = 0.7 + 0.28 * p.verb_decay.clamp(0.0, 1.0);
        let verb_sc = (sr / 44_100.0).min((VERB_SCALE_MAX - 1) as f32);
        let width = p.stereo.clamp(0.0, ECHO_STEREO_MAX);
        let wet = p.dry_wet.clamp(0.0, 1.0);
        let (gd, gw) = if p.equal_loud { ((wet * FRAC_PI_2).cos(), (wet * FRAC_PI_2).sin()) } else { (1.0 - wet, wet) };
        let mode = p.mode.min(2);
        for i in 0..n {
            let (xl, xr) = (l[i] * g_in, r[i] * g_in);
            let lvl = xl.abs().max(xr.abs());
            // Modulation: LFO (+ envelope).
            self.lfo += rate / sr;
            if self.lfo >= 1.0 {
                self.lfo -= 1.0;
                self.rnd_t[0] = self.rand();
            }
            let ph_r = (self.lfo + ph_off).fract();
            if p.wave == 5 {
                // Smoothed random steps.
                let k = (-rate * 8.0 / sr).exp();
                let t = self.rnd_t[0];
                self.rnd[0] = t + k * (self.rnd[0] - t);
                if ph_r < self.ph_r_prev {
                    self.rnd_t[1] = self.rand();
                }
                let t1 = self.rnd_t[1];
                self.rnd[1] = t1 + k * (self.rnd[1] - t1);
            }
            self.ph_r_prev = ph_r;
            self.env = lvl.max(self.env * k_env);
            let e = self.env.min(1.0);
            let m = [
                (1.0 - env_mix) * echo_wave(p.wave, self.lfo, self.rnd[0]) + env_mix * e,
                (1.0 - env_mix) * echo_wave(p.wave, ph_r, self.rnd[1]) + env_mix * e,
            ];
            // Gate on the delay input.
            self.gate_env = lvl.max(self.gate_env * k_gate_rel);
            if p.gate {
                let open = (self.gate_env > gate_thr) as u8 as f32;
                let k = if open > self.gate_g { k_att } else { k_gate_rel };
                self.gate_g = open + k * (self.gate_g - open);
            } else {
                self.gate_g = 1.0;
            }
            let mut win = [xl * self.gate_g, xr * self.gate_g];
            if verb_amt > 0.0 && p.verb_loc == 0 {
                let v = self.verb.tick(win, verb_fb, verb_sc);
                win = [win[0] + verb_amt * v[0], win[1] + verb_amt * v[1]];
            }
            // Wobble: sampled tape-speed curves.
            self.wob_pos += WOBBLE_SR / sr;
            if self.wob_pos >= 384.0 {
                self.wob_pos -= 384.0;
            }
            let wob = if wob_d > 0.0 { morph(&WOBBLE, p.wobble_morph, self.wob_pos) * wob_d } else { 0.0 };
            // Read both lines.
            let mut y = [0.0f32; 2];
            for c in 0..2 {
                if p.repitch {
                    self.cur[c] += (target[c] - self.cur[c]) * k_glide;
                    self.fade[c] = 1.0;
                } else if (target[c] - self.cur[c]).abs() > 0.5 && self.fade[c] >= 1.0 {
                    self.old[c] = self.cur[c];
                    self.cur[c] = target[c];
                    self.fade[c] = 0.0;
                }
                let modd = 1.0 + md * m[c];
                let d_new = self.cur[c] * modd + wob;
                let mut v = self.read(c, d_new);
                if self.fade[c] < 1.0 {
                    let d_old = self.old[c] * modd + wob;
                    let f = self.fade[c];
                    v = v * f + self.read(c, d_old) * (1.0 - f);
                    self.fade[c] = (f + fade_inc).min(1.0);
                }
                if filt {
                    let oct = 2f32.powf(mf * m[c]);
                    let (_, h) = self.hp[c].tick(v, svf_g((hp_f * oct).min(0.45 * sr), sr), hp_k);
                    let (lo, _) = self.lp[c].tick(h, svf_g((lp_f * oct).min(0.45 * sr), sr), lp_k);
                    v = lo;
                }
                y[c] = v;
            }
            // Feedback (bounded) and optional reverb inside the loop.
            let mut f = [y[0] * fb, y[1] * fb];
            if verb_amt > 0.0 && p.verb_loc == 2 {
                let v = self.verb.tick(f, verb_fb, verb_sc);
                f = [f[0] + verb_amt * v[0], f[1] + verb_amt * v[1]];
            }
            let f = [soft(f[0]), soft(f[1])];
            self.pos = (self.pos + 1) & (BUF - 1);
            let (wl, wr) = match mode {
                0 => {
                    self.buf[0][self.pos] = win[0] + f[0];
                    self.buf[1][self.pos] = win[1] + f[1];
                    (y[0], y[1])
                }
                1 => {
                    // Ping Pong: input → L → R → (feedback) → L.
                    self.buf[0][self.pos] = 0.5 * (win[0] + win[1]) + f[1];
                    self.buf[1][self.pos] = y[0];
                    (y[0], y[1])
                }
                _ => {
                    // Mid/Side: line 0 = mid, line 1 = side.
                    self.buf[0][self.pos] = 0.5 * (win[0] + win[1]) + f[0];
                    self.buf[1][self.pos] = 0.5 * (win[0] - win[1]) + f[1];
                    (y[0] + y[1], y[0] - y[1])
                }
            };
            let (mut wl, mut wr) = (wl, wr);
            // Ducking: the wet signal follows the input level.
            if p.duck {
                self.duck_env = lvl.max(self.duck_env * k_duck_rel);
                let over = (self.duck_env / duck_thr.max(1e-6)).max(1.0);
                let g = 1.0 / over;
                wl *= g;
                wr *= g;
            }
            if noise_g > 0.0 {
                self.noise_pos += NOISE_SR / sr;
                if self.noise_pos >= 88_200.0 {
                    self.noise_pos -= 88_200.0;
                }
                let nz = morph(&NOISE, p.noise_morph, self.noise_pos) * noise_g;
                let nz2 = morph(&NOISE, p.noise_morph, (self.noise_pos + 44_100.0) % 88_200.0) * noise_g;
                wl += nz;
                wr += nz2;
            }
            if verb_amt > 0.0 && p.verb_loc == 1 {
                let v = self.verb.tick([wl, wr], verb_fb, verb_sc);
                wl += verb_amt * v[0];
                wr += verb_amt * v[1];
            }
            let (mm, ss) = (0.5 * (wl + wr), 0.5 * (wl - wr) * width);
            wl = (mm + ss) * g_out;
            wr = (mm - ss) * g_out;
            let (dl, dr) = if p.clip_dry { (soft(xl * 1.5) / 1.5, soft(xr * 1.5) / 1.5) } else { (xl, xr) };
            l[i] = dl * gd + wl * gw;
            r[i] = dr * gd + wr * gw;
            self.out = [wl, wr];
        }
        if let Some(t) = &self.tap {
            t.set(etap::LFO_PH, self.lfo);
            t.set(etap::TIME_L, self.cur[0] / sr);
            t.set(etap::TIME_R, self.cur[1] / sr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(p: &EchoParams, secs: f32, imp: bool) -> (Vec<f32>, Vec<f32>) {
        let sr = 48_000.0;
        let n = (secs * sr) as usize;
        let mut st = EchoState::new(None);
        let mut l = vec![0.0f32; n];
        if imp {
            l[0] = 1.0;
        }
        let mut r = vec![0.0f32; n];
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            st.process(p, a, b, sr);
        }
        (l, r)
    }

    fn peak_near(x: &[f32], at: usize, w: usize) -> f32 {
        x[at.saturating_sub(w)..(at + w).min(x.len())].iter().fold(0.0f32, |a, v| a.max(v.abs()))
    }

    #[test]
    fn repeats_at_time_with_feedback() {
        let p = EchoParams {
            sync: [false; 2],
            ms: [100.0; 2],
            offset: [0.0; 2],
            filter_on: false,
            wobble: false,
            duck: false,
            dry_wet: 1.0,
            feedback: 0.5,
            ..Default::default()
        };
        let (l, r) = run(&p, 0.5, true);
        assert!(peak_near(&l, 4800, 3) > 0.8, "{}", peak_near(&l, 4800, 3));
        let second = peak_near(&l, 9600, 3);
        assert!((0.35..0.6).contains(&second), "{second}");
        assert!(peak_near(&l, 2400, 100) < 1e-3);
        assert!(peak_near(&r, 4800, 3) < 1e-3);
        // Ping Pong: first repeat left, second right.
        let p = EchoParams { mode: 1, ..p };
        let (l, r) = run(&p, 0.5, true);
        assert!(peak_near(&l, 4800, 3) > 0.4 && peak_near(&r, 9600, 3) > 0.4 && peak_near(&r, 4800, 3) < 1e-3);
    }

    #[test]
    fn synced_time_and_labels() {
        let p = EchoParams::default();
        assert_eq!(p.time_label(0), "1/4");
        assert!((p.base_secs(0, 120.0) - 0.5).abs() < 1e-6);
        let p = EchoParams { sync_mode: [1, 1], ..p };
        assert_eq!(p.time_label(1), "1/4T");
        assert!((p.base_secs(1, 120.0) - 1.0 / 3.0).abs() < 1e-6);
        let p = EchoParams { sync_mode: [3, 3], sixteenths: [3, 3], ..p };
        assert_eq!(p.time_label(0), "3/16");
        let p = EchoParams { sync: [false; 2], ..p };
        assert_eq!(p.time_label(0), "375 ms");
    }

    #[test]
    fn high_feedback_stays_bounded() {
        let p = EchoParams {
            feedback: 1.5,
            sync: [false; 2],
            ms: [20.0; 2],
            reverb: 1.0,
            verb_loc: 2,
            noise: true,
            noise_amt: 1.0,
            ..Default::default()
        };
        let (l, r) = run(&p, 1.0, true);
        assert!(l.iter().chain(&r).all(|v| v.is_finite() && v.abs() < 10.0));
        for w in 0..6 {
            for ph in [0.0, 0.3, 0.99] {
                let v = echo_wave(w, ph, 0.5);
                assert!((-1.0..=1.0).contains(&v));
            }
        }
    }
}
