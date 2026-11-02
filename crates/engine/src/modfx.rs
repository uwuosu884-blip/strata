//! Live 12 modulation / filter effects: EQ Three, Chorus-Ensemble, Auto
//! Filter and Auto Pan-Tremolo (Live manual 29.2, 29.3, 29.8, 29.16).

use crate::devtap::{DevTap, dev_tap};
use crate::dynfx::{ScState, Sidechain, db_lin};
use serde::{Deserialize, Serialize};
use std::f32::consts::{PI, TAU};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

// ----------------------------------------------------------------------------
// Song clock (tempo-synced rates): set once per block by the engine.
// ----------------------------------------------------------------------------

static TEMPO: AtomicU64 = AtomicU64::new(0x405E_0000_0000_0000); // 120.0
static BEAT: AtomicU64 = AtomicU64::new(u64::MAX); // NaN = stopped

pub(crate) fn set_clock(tempo: f64, beat: Option<f64>) {
    TEMPO.store(tempo.to_bits(), Ordering::Relaxed);
    BEAT.store(beat.unwrap_or(f64::NAN).to_bits(), Ordering::Relaxed);
}

fn clock() -> (f64, Option<f64>) {
    let t = f64::from_bits(TEMPO.load(Ordering::Relaxed));
    let b = f64::from_bits(BEAT.load(Ordering::Relaxed));
    (if t.is_finite() && t > 0.0 { t } else { 120.0 }, b.is_finite().then_some(b))
}

// ----------------------------------------------------------------------------
// Rate modes shared by Auto Filter and Auto Pan-Tremolo.
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TimeMode {
    #[default]
    Rate,
    Time,
    Synced,
    Dotted,
    Triplet,
    Sixteenth,
}

impl TimeMode {
    pub const ALL: [TimeMode; 6] =
        [TimeMode::Rate, TimeMode::Time, TimeMode::Synced, TimeMode::Dotted, TimeMode::Triplet, TimeMode::Sixteenth];
    pub fn label(self) -> &'static str {
        match self {
            TimeMode::Rate => "Rate",
            TimeMode::Time => "Time",
            TimeMode::Synced => "Synced",
            TimeMode::Dotted => "Dotted",
            TimeMode::Triplet => "Triplet",
            TimeMode::Sixteenth => "16th",
        }
    }
    pub fn synced(self) -> bool {
        !matches!(self, TimeMode::Rate | TimeMode::Time)
    }
}

/// Synced divisions (beats per cycle at 4/4) and labels.
pub const SYNC_DIVS: [(f64, &str); 11] = [
    (1.0 / 16.0, "1/64"),
    (1.0 / 8.0, "1/32"),
    (0.25, "1/16"),
    (0.5, "1/8"),
    (1.0, "1/4"),
    (2.0, "1/2"),
    (4.0, "1"),
    (8.0, "2"),
    (16.0, "4"),
    (32.0, "8"),
    (64.0, "16"),
];
pub const SIXTEENTH_MAX: u8 = 64;
pub const LFO_HZ: (f32, f32) = (0.01, 40.0);
pub const LFO_MS: (f32, f32) = (25.0, 100_000.0);

/// Rate of a modulator in any [`TimeMode`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rate {
    pub mode: TimeMode,
    pub hz: f32,
    pub ms: f32,
    pub div: u8,
    pub sixteenth: u8,
}

impl Default for Rate {
    fn default() -> Self {
        Self { mode: TimeMode::Rate, hz: 1.0, ms: 1000.0, div: 4, sixteenth: 4 }
    }
}

impl Rate {
    /// Beats per cycle (synced modes).
    pub fn beats(&self) -> f64 {
        let b = SYNC_DIVS[(self.div as usize).min(SYNC_DIVS.len() - 1)].0;
        match self.mode {
            TimeMode::Dotted => b * 1.5,
            TimeMode::Triplet => b * 2.0 / 3.0,
            TimeMode::Sixteenth => self.sixteenth.clamp(1, SIXTEENTH_MAX) as f64 * 0.25,
            _ => b,
        }
    }
    pub fn hz(&self, tempo: f64) -> f64 {
        match self.mode {
            TimeMode::Rate => self.hz.clamp(LFO_HZ.0, LFO_HZ.1) as f64,
            TimeMode::Time => 1000.0 / self.ms.clamp(LFO_MS.0, LFO_MS.1) as f64,
            _ => tempo / 60.0 / self.beats(),
        }
    }
    /// Knob label (Live: Freq / Time / Rate).
    pub fn label(&self) -> &'static str {
        match self.mode {
            TimeMode::Rate => "Freq",
            TimeMode::Time => "Time",
            _ => "Rate",
        }
    }
    pub fn text(&self) -> String {
        match self.mode {
            TimeMode::Rate => {
                let h = self.hz;
                if h < 9.995 { format!("{h:.2} Hz") } else { format!("{h:.1} Hz") }
            }
            TimeMode::Time => {
                let m = self.ms;
                if m >= 1000.0 { format!("{:.2} s", m / 1000.0) } else { format!("{m:.0} ms") }
            }
            TimeMode::Sixteenth => format!("{} /16", self.sixteenth.clamp(1, SIXTEENTH_MAX)),
            m => {
                let s = SYNC_DIVS[(self.div as usize).min(SYNC_DIVS.len() - 1)].1;
                match m {
                    TimeMode::Dotted => format!("{s} D"),
                    TimeMode::Triplet => format!("{s} T"),
                    _ => s.to_string(),
                }
            }
        }
    }
}

/// Free-running / song-locked modulator phase (cycles).
#[derive(Debug, Clone, Copy, Default)]
struct Phasor {
    pos: f64,
}

impl Phasor {
    /// Phase at the block start; locks to the song position while playing.
    fn start(&mut self, r: &Rate, beat: Option<f64>) -> f64 {
        if r.mode.synced()
            && let Some(b) = beat
        {
            self.pos = b / r.beats();
        }
        self.pos
    }
    fn advance(&mut self, cycles: f64) {
        self.pos += cycles;
        if self.pos > 1.0e6 {
            self.pos = self.pos.fract();
        }
    }
}

fn xorshift(s: &mut u64) -> f32 {
    *s ^= *s >> 12;
    *s ^= *s << 25;
    *s ^= *s >> 27;
    let x = s.wrapping_mul(0x2545_f491_4f6c_dd1d);
    ((x >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
}

#[inline]
fn soft(x: f32) -> f32 {
    // Cheap tanh (Padé), exact enough for saturation.
    let x = x.clamp(-3.0, 3.0);
    x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
}

// ----------------------------------------------------------------------------
// Biquads (RBJ, TDF2, f64 state).
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Bq {
    b: [f64; 3],
    a: [f64; 2],
}

impl Default for Bq {
    fn default() -> Self {
        Self { b: [1.0, 0.0, 0.0], a: [0.0, 0.0] }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BqKind {
    Lp,
    Hp,
    Ap,
}

impl Bq {
    fn new(kind: BqKind, f: f32, q: f32, sr: f32) -> Self {
        let w = std::f64::consts::TAU * (f as f64 / sr as f64).clamp(1e-5, 0.49);
        let (s, c) = w.sin_cos();
        let al = s / (2.0 * q as f64);
        let a0 = 1.0 + al;
        let (b0, b1, b2) = match kind {
            BqKind::Lp => ((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0),
            BqKind::Hp => ((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0),
            BqKind::Ap => (1.0 - al, -2.0 * c, 1.0 + al),
        };
        Self { b: [b0 / a0, b1 / a0, b2 / a0], a: [-2.0 * c / a0, (1.0 - al) / a0] }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct BqSt {
    s: [f64; 2],
}

impl BqSt {
    #[inline]
    fn tick(&mut self, c: &Bq, x: f64) -> f64 {
        let y = c.b[0] * x + self.s[0];
        self.s[0] = c.b[1] * x - c.a[0] * y + self.s[1];
        self.s[1] = c.b[2] * x - c.a[1] * y;
        y
    }
}

/// Butterworth section Qs: 2nd order (LR4 halves) and 4th order (LR8 halves).
const BW2: [f32; 1] = [std::f32::consts::FRAC_1_SQRT_2];
const BW4: [f32; 2] = [0.541_196_1, 1.306_563];

// ----------------------------------------------------------------------------
// EQ Three.
// ----------------------------------------------------------------------------

/// Gain at the bottom of the knob (shown as −inf dB).
pub const EQ3_GAIN_MIN: f32 = -70.0;
pub const EQ3_GAIN_MAX: f32 = 6.0;
pub const EQ3_LO: (f32, f32) = (50.0, 5000.0);
pub const EQ3_HI: (f32, f32) = (200.0, 18_000.0);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EqThreeParams {
    /// dB (≤ [`EQ3_GAIN_MIN`] = −inf).
    pub gain: [f32; 3],
    /// L / M / H band buttons.
    pub on: [bool; 3],
    pub freq_lo: f32,
    pub freq_hi: f32,
    /// 48 dB/oct crossovers (else 24).
    pub slope48: bool,
}

impl Default for EqThreeParams {
    fn default() -> Self {
        Self { gain: [0.0; 3], on: [true; 3], freq_lo: 250.0, freq_hi: 2500.0, slope48: true }
    }
}

impl EqThreeParams {
    pub fn band_gain(&self, b: usize) -> f32 {
        if !self.on[b] || self.gain[b] <= EQ3_GAIN_MIN { 0.0 } else { db_lin(self.gain[b].min(EQ3_GAIN_MAX)) }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Eq3Ch {
    lo: [BqSt; 6],
    rest: [BqSt; 4],
    mid: [BqSt; 4],
    hi: [BqSt; 4],
}

#[derive(Debug, Clone)]
pub struct EqThreeState {
    ch: [Eq3Ch; 2],
    key: Option<(u32, u32, bool, u32)>,
    /// Sections: lp_lo, ap_hi, hp_lo, lp_hi, hp_hi (up to 4 / 2 each).
    lp_lo: [Bq; 4],
    ap_hi: [Bq; 2],
    hp_lo: [Bq; 4],
    lp_hi: [Bq; 4],
    hp_hi: [Bq; 4],
    n: usize,
    g: [f32; 3],
    init: bool,
    tap: Option<Arc<DevTap>>,
}

impl EqThreeState {
    pub fn new(id: Option<u64>) -> Self {
        Self {
            ch: Default::default(),
            key: None,
            lp_lo: Default::default(),
            ap_hi: Default::default(),
            hp_lo: Default::default(),
            lp_hi: Default::default(),
            hp_hi: Default::default(),
            n: 2,
            g: [1.0; 3],
            init: false,
            tap: id.map(dev_tap),
        }
    }

    pub fn reset(&mut self) {
        self.ch = Default::default();
        self.init = false;
    }

    fn design(&mut self, p: &EqThreeParams, sr: f32) {
        let fl = p.freq_lo.clamp(EQ3_LO.0, EQ3_LO.1);
        let fh = p.freq_hi.clamp(EQ3_HI.0, EQ3_HI.1).max(fl);
        let key = (fl.to_bits(), fh.to_bits(), p.slope48, sr.to_bits());
        if self.key == Some(key) {
            return;
        }
        self.key = Some(key);
        // Linkwitz-Riley: two cascaded Butterworths per side; the low band
        // passes the high crossover's allpass so the bands sum flat.
        let qs: &[f32] = if p.slope48 { &BW4 } else { &BW2 };
        let m = qs.len();
        self.n = 2 * m;
        for (i, &q) in qs.iter().chain(qs.iter()).enumerate() {
            self.lp_lo[i] = Bq::new(BqKind::Lp, fl, q, sr);
            self.hp_lo[i] = Bq::new(BqKind::Hp, fl, q, sr);
            self.lp_hi[i] = Bq::new(BqKind::Lp, fh, q, sr);
            self.hp_hi[i] = Bq::new(BqKind::Hp, fh, q, sr);
        }
        for (i, &q) in qs.iter().enumerate() {
            self.ap_hi[i] = Bq::new(BqKind::Ap, fh, q, sr);
        }
    }

    pub fn process(&mut self, p: &EqThreeParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        self.design(p, sr);
        let n = l.len().min(r.len());
        let tg = [p.band_gain(0), p.band_gain(1), p.band_gain(2)];
        if !self.init {
            self.g = tg;
            self.init = true;
        }
        let g0 = self.g;
        let inv = 1.0 / n.max(1) as f32;
        let ns = self.n;
        let na = ns / 2;
        let mut pk = [0.0f32; 3];
        for (c, buf) in [&mut *l, &mut *r].into_iter().enumerate() {
            let st = &mut self.ch[c];
            for (i, x) in buf[..n].iter_mut().enumerate() {
                let t = (i + 1) as f32 * inv;
                let g = [g0[0] + (tg[0] - g0[0]) * t, g0[1] + (tg[1] - g0[1]) * t, g0[2] + (tg[2] - g0[2]) * t];
                let v = *x as f64;
                let mut lo = v;
                for k in 0..ns {
                    lo = st.lo[k].tick(&self.lp_lo[k], lo);
                }
                for k in 0..na {
                    lo = st.lo[4 + k].tick(&self.ap_hi[k], lo);
                }
                let mut rest = v;
                for k in 0..ns {
                    rest = st.rest[k].tick(&self.hp_lo[k], rest);
                }
                let (mut mid, mut hi) = (rest, rest);
                for k in 0..ns {
                    mid = st.mid[k].tick(&self.lp_hi[k], mid);
                    hi = st.hi[k].tick(&self.hp_hi[k], hi);
                }
                // LR4 / LR8 sum in phase (allpass).
                let (a, b, cc) = (lo as f32 * g[0], mid as f32 * g[1], hi as f32 * g[2]);
                pk[0] = pk[0].max(a.abs());
                pk[1] = pk[1].max(b.abs());
                pk[2] = pk[2].max(cc.abs());
                *x = a + b + cc;
            }
        }
        self.g = tg;
        if let Some(t) = &self.tap {
            for (k, v) in pk.into_iter().enumerate() {
                t.raise(k, v);
            }
        }
    }
}

// ----------------------------------------------------------------------------
// Chorus-Ensemble.
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ChorusMode {
    #[default]
    Chorus,
    Ensemble,
    Vibrato,
}

impl ChorusMode {
    pub const ALL: [ChorusMode; 3] = [ChorusMode::Chorus, ChorusMode::Ensemble, ChorusMode::Vibrato];
    pub fn label(self) -> &'static str {
        match self {
            ChorusMode::Chorus => "Chorus",
            ChorusMode::Ensemble => "Ensemble",
            ChorusMode::Vibrato => "Vibrato",
        }
    }
}

/// Chorus Delay Time choices (ms; 0 = Auto).
pub const CHO_TIMES: [f32; 9] = [0.0, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 30.0, 40.0];
pub const CHO_RATE: (f32, f32) = (0.01, 20.0);
pub const CHO_HP: (f32, f32) = (20.0, 2000.0);
pub const CHO_OUT: (f32, f32) = (-24.0, 10.0);
pub const CHO_FB_MAX: f32 = 0.95;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChorusParams {
    pub mode: ChorusMode,
    pub hp_on: bool,
    pub hp_freq: f32,
    /// 0..2 (0..200 %).
    pub width: f32,
    /// Index into [`CHO_TIMES`].
    pub time: u8,
    /// 1 or 2 delay taps (Chorus).
    pub taps: u8,
    /// Vibrato: right-channel phase offset (degrees) and shape (sine → triangle).
    pub offset: f32,
    pub shape: f32,
    pub rate: f32,
    pub amount: f32,
    pub feedback: f32,
    pub fb_inv: bool,
    pub output: f32,
    pub warmth: f32,
    pub dry_wet: f32,
}

impl Default for ChorusParams {
    fn default() -> Self {
        Self {
            mode: ChorusMode::Chorus,
            hp_on: false,
            hp_freq: 50.0,
            width: 1.0,
            time: 6,
            taps: 2,
            offset: 0.0,
            shape: 0.0,
            rate: 0.9,
            amount: 0.5,
            feedback: 0.0,
            fb_inv: false,
            output: 0.0,
            warmth: 0.0,
            dry_wet: 0.5,
        }
    }
}

impl ChorusParams {
    /// Base delay (ms) of the current mode.
    pub fn base_ms(&self) -> f32 {
        match self.mode {
            ChorusMode::Chorus => {
                let t = CHO_TIMES[(self.time as usize).min(CHO_TIMES.len() - 1)];
                if t <= 0.0 { 7.0 } else { t }
            }
            ChorusMode::Ensemble => 9.0,
            ChorusMode::Vibrato => 4.0,
        }
    }
    /// Modulators (delay lines) and their phase offsets (cycles).
    pub fn voices(&self) -> &'static [f32] {
        match self.mode {
            ChorusMode::Chorus if self.taps >= 2 => &[0.0, 0.5],
            ChorusMode::Chorus => &[0.0],
            ChorusMode::Ensemble => &[0.0, 1.0 / 3.0, 2.0 / 3.0],
            ChorusMode::Vibrato => &[0.0],
        }
    }
    /// Modulator value (−1..1) at phase `ph` (Vibrato Shape: sine → triangle).
    pub fn lfo(&self, ph: f32) -> f32 {
        let ph = ph.rem_euclid(1.0);
        let s = (TAU * ph).sin();
        if self.mode != ChorusMode::Vibrato || self.shape <= 0.0 {
            return s;
        }
        let tri = if ph < 0.25 {
            4.0 * ph
        } else if ph < 0.75 {
            2.0 - 4.0 * ph
        } else {
            4.0 * ph - 4.0
        };
        s + (tri - s) * self.shape.clamp(0.0, 1.0)
    }
}

const CHO_LEN: usize = 1 << 14;

#[derive(Debug, Clone)]
pub struct ChorusState {
    buf: [Box<[f32]>; 2],
    w: usize,
    ph: f64,
    hp: [f32; 2],
    warm: [f32; 2],
    fb: [f32; 2],
    tap: Option<Arc<DevTap>>,
}

impl ChorusState {
    pub fn new(id: Option<u64>) -> Self {
        Self {
            buf: [vec![0.0; CHO_LEN].into_boxed_slice(), vec![0.0; CHO_LEN].into_boxed_slice()],
            w: 0,
            ph: 0.0,
            hp: [0.0; 2],
            warm: [0.0; 2],
            fb: [0.0; 2],
            tap: id.map(dev_tap),
        }
    }

    pub fn reset(&mut self) {
        for b in self.buf.iter_mut() {
            b.fill(0.0);
        }
        self.hp = [0.0; 2];
        self.warm = [0.0; 2];
        self.fb = [0.0; 2];
    }

    #[inline]
    fn read(&self, c: usize, d: f32) -> f32 {
        let d = d.clamp(1.0, (CHO_LEN - 4) as f32);
        let pos = self.w as f32 - d;
        let i = pos.floor();
        let f = pos - i;
        let m = CHO_LEN - 1;
        let i = (i as isize).rem_euclid(CHO_LEN as isize) as usize;
        // Cubic Hermite.
        let b = &self.buf[c];
        let (y0, y1, y2, y3) = (b[(i + m) & m], b[i], b[(i + 1) & m], b[(i + 2) & m]);
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
        ((c3 * f + c2) * f + c1) * f + y1
    }

    pub fn process(&mut self, p: &ChorusParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let n = l.len().min(r.len());
        let vib = p.mode == ChorusMode::Vibrato;
        let rate = p.rate.clamp(CHO_RATE.0, CHO_RATE.1) as f64;
        let dph = rate / sr as f64;
        let base = p.base_ms() * 0.001 * sr;
        let depth = match p.mode {
            ChorusMode::Chorus => 0.6 + p.base_ms().min(10.0) * 0.25,
            ChorusMode::Ensemble => 3.0,
            ChorusMode::Vibrato => 3.5,
        } * 0.001
            * sr
            * p.amount.clamp(0.0, 1.0);
        let base = base.max(depth + 2.0);
        let voices = p.voices();
        let nv = voices.len() as f32;
        let fb = if vib { 0.0 } else { p.feedback.clamp(0.0, CHO_FB_MAX) * if p.fb_inv { -1.0 } else { 1.0 } };
        let hp_k = if p.hp_on { 1.0 - (-TAU * p.hp_freq.clamp(CHO_HP.0, CHO_HP.1) / sr).exp() } else { 0.0 };
        let warm = p.warmth.clamp(0.0, 1.0);
        let lp_k = 1.0 - (-TAU * (18_000.0 - 14_000.0 * warm).min(0.45 * sr) / sr).exp();
        let width = p.width.clamp(0.0, 2.0);
        let wet = if vib { 1.0 } else { p.dry_wet.clamp(0.0, 1.0) };
        let out = db_lin(p.output.clamp(CHO_OUT.0, CHO_OUT.1));
        // Right channel: opposite phase (Chorus / Ensemble) or Offset (Vibrato).
        let r_off = if vib { p.offset.clamp(0.0, 180.0) / 360.0 } else { 0.25 };
        let m = CHO_LEN - 1;
        for i in 0..n {
            let ph = self.ph as f32;
            let x = [l[i], r[i]];
            let mut y = [0.0f32; 2];
            for c in 0..2 {
                // HP: the lows bypass the modulation.
                let (low, high) = if p.hp_on {
                    self.hp[c] += hp_k * (x[c] - self.hp[c]);
                    (self.hp[c], x[c] - self.hp[c])
                } else {
                    (0.0, x[c])
                };
                self.buf[c][self.w] = high + self.fb[c] * fb;
                let off = if c == 1 { r_off } else { 0.0 };
                let mut acc = 0.0;
                for &v in voices {
                    let d = base + depth * p.lfo(ph + v + off);
                    acc += self.read(c, d);
                }
                acc /= nv.sqrt().max(1.0);
                self.fb[c] = acc;
                let mut e = acc;
                if warm > 0.0 {
                    e = soft(e * (1.0 + 3.0 * warm)) / (1.0 + 1.5 * warm);
                    self.warm[c] += lp_k * (e - self.warm[c]);
                    e = self.warm[c];
                }
                y[c] = e + low;
            }
            // Width: mid / side balance of the wet signal.
            let (mid, side) = (0.5 * (y[0] + y[1]), 0.5 * (y[0] - y[1]) * width);
            let (wl, wr) = if vib { (y[0], y[1]) } else { (mid + side, mid - side) };
            l[i] = (x[0] * (1.0 - wet) + wl * wet) * out;
            r[i] = (x[1] * (1.0 - wet) + wr * wet) * out;
            self.w = (self.w + 1) & m;
            self.ph += dph;
        }
        self.ph = self.ph.fract();
        if let Some(t) = &self.tap {
            t.set(0, self.ph as f32);
        }
    }
}

// ----------------------------------------------------------------------------
// Auto Pan-Tremolo.
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PanWave {
    #[default]
    Sine,
    Triangle,
    SharkTooth,
    SawUp,
    SawDown,
    Square,
    Random,
    Wander,
    SampleHold,
}

impl PanWave {
    pub const ALL: [PanWave; 9] = [
        PanWave::Sine,
        PanWave::Triangle,
        PanWave::SharkTooth,
        PanWave::SawUp,
        PanWave::SawDown,
        PanWave::Square,
        PanWave::Random,
        PanWave::Wander,
        PanWave::SampleHold,
    ];
    pub fn label(self) -> &'static str {
        match self {
            PanWave::Sine => "Sine",
            PanWave::Triangle => "Triangle",
            PanWave::SharkTooth => "Shark Tooth",
            PanWave::SawUp => "Saw Up",
            PanWave::SawDown => "Saw Down",
            PanWave::Square => "Square",
            PanWave::Random => "Random",
            PanWave::Wander => "Wander",
            PanWave::SampleHold => "S&H",
        }
    }
    pub fn random(self) -> bool {
        matches!(self, PanWave::Random | PanWave::Wander | PanWave::SampleHold)
    }
}

pub const PAN_ATTACK_MAX: f32 = 10_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoPanParams {
    /// Tremolo mode (else Panning).
    pub tremolo: bool,
    pub wave: PanWave,
    pub rate: Rate,
    /// Stereo mode Spin (else Phase).
    pub spin_mode: bool,
    /// Degrees 0..360.
    pub phase: f32,
    /// 0..1 (right LFO rate + spin).
    pub spin: f32,
    /// Synced modes: degrees.
    pub offset: f32,
    pub invert: bool,
    pub harmonic: bool,
    pub vintage: bool,
    pub amount: f32,
    /// Panning 0..1; Tremolo −1..1.
    pub shape_pan: f32,
    pub shape_trem: f32,
    /// Att (ms after a transient) and Mod (dynamic frequency modulation −1..1).
    pub attack: f32,
    pub dyn_mod: f32,
}

impl Default for AutoPanParams {
    fn default() -> Self {
        Self {
            tremolo: false,
            wave: PanWave::Sine,
            rate: Rate::default(),
            spin_mode: false,
            phase: 180.0,
            spin: 0.0,
            offset: 0.0,
            invert: false,
            harmonic: false,
            vintage: false,
            amount: 0.5,
            shape_pan: 0.0,
            shape_trem: 0.0,
            attack: 0.0,
            dyn_mod: 0.0,
        }
    }
}

impl AutoPanParams {
    /// Deterministic waveform (−1..1) at phase `ph`; random waves use `rnd`
    /// (current, next value of the cycle).
    pub fn wave_at(&self, ph: f32, rnd: (f32, f32)) -> f32 {
        let p = ph.rem_euclid(1.0);
        let tri = |p: f32| {
            if p < 0.25 {
                4.0 * p
            } else if p < 0.75 {
                2.0 - 4.0 * p
            } else {
                4.0 * p - 4.0
            }
        };
        let mut v = if self.tremolo {
            // Tremolo: a triangle bent by Shape (+ rounder, − sharper, the
            // extremes skew toward ramps).
            let s = self.shape_trem.clamp(-1.0, 1.0);
            let w = 0.5 + 0.45 * (s.abs() - 0.5).max(0.0) * 2.0 * s.signum();
            let q = if p < w { 0.5 * p / w } else { 0.5 + 0.5 * (p - w) / (1.0 - w) };
            let t = tri(q);
            let e = if s >= 0.0 { 1.0 / (1.0 + 3.0 * s) } else { 1.0 + 3.0 * -s };
            t.signum() * t.abs().powf(e)
        } else {
            let v = match self.wave {
                PanWave::Sine => (TAU * p).sin(),
                PanWave::Triangle => tri(p),
                PanWave::SharkTooth => {
                    // Slow rise, fast fall (rounded).
                    let q = if p < 0.8 { p / 0.8 } else { 1.0 - (p - 0.8) / 0.2 };
                    (PI * q - PI / 2.0).sin()
                }
                PanWave::SawUp => 2.0 * p - 1.0,
                PanWave::SawDown => 1.0 - 2.0 * p,
                PanWave::Square => {
                    if p < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                PanWave::Random => rnd.0 + (rnd.1 - rnd.0) * (0.5 - 0.5 * (PI * p).cos()),
                PanWave::Wander | PanWave::SampleHold => rnd.0,
            };
            // Shape pushes the wave toward its limits.
            let s = self.shape_pan.clamp(0.0, 1.0);
            if s > 0.0 { v.signum() * v.abs().powf(1.0 / (1.0 + 6.0 * s)) } else { v }
        };
        if self.invert {
            v = -v;
        }
        v
    }
}

#[derive(Debug, Clone)]
pub struct AutoPanState {
    ph: [Phasor; 2],
    rng: u64,
    rnd: [(f32, f32, i64); 2],
    wander: [f32; 2],
    env: f32,
    slow: f32,
    att: f32,
    xo: [f32; 2],
    g: [f32; 2],
    tap: Option<Arc<DevTap>>,
}

impl AutoPanState {
    pub fn new(id: Option<u64>) -> Self {
        Self {
            ph: Default::default(),
            rng: 0x2545_f491_4f6c_dd1d ^ id.unwrap_or(7),
            rnd: [(0.0, 0.0, i64::MIN); 2],
            wander: [0.0; 2],
            env: 0.0,
            slow: 0.0,
            att: 1.0,
            xo: [0.0; 2],
            g: [1.0; 2],
            tap: id.map(dev_tap),
        }
    }

    pub fn reset(&mut self) {
        self.env = 0.0;
        self.slow = 0.0;
        self.att = 1.0;
        self.xo = [0.0; 2];
    }

    fn value(&mut self, p: &AutoPanParams, c: usize, ph: f64, step: f32) -> f32 {
        let cyc = ph.floor() as i64;
        let (_, next, seen) = self.rnd[c];
        if seen != cyc {
            let a = if seen == i64::MIN { xorshift(&mut self.rng) } else { next };
            self.rnd[c] = (a, xorshift(&mut self.rng), cyc);
        }
        if p.wave == PanWave::Wander && !p.tremolo {
            let r = xorshift(&mut self.rng);
            let w = &mut self.wander[c];
            *w = (*w + r * 1.4 * step.sqrt() - *w * step * 0.6).clamp(-1.0, 1.0);
            return if p.invert { -*w } else { *w };
        }
        let (a, b, _) = self.rnd[c];
        p.wave_at(ph.fract() as f32, (a, b))
    }

    pub fn process(&mut self, p: &AutoPanParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let n = l.len().min(r.len());
        let (tempo, beat) = clock();
        let hz = p.rate.hz(tempo);
        let off = if p.rate.mode.synced() || p.wave.random() { p.offset as f64 / 360.0 } else { 0.0 };
        let base = self.ph[0].start(&p.rate, beat);
        if p.rate.mode.synced() && beat.is_some() {
            self.ph[1].pos = base;
        }
        // Input level drives Att and Mod.
        let mut pk = 0.0f32;
        for i in 0..n {
            pk = pk.max(l[i].abs()).max(r[i].abs());
        }
        let blk = n as f32 / sr;
        let a_env = 1.0 - (-blk / 0.005).exp();
        let r_env = 1.0 - (-blk / 0.15).exp();
        self.env += (pk - self.env) * if pk > self.env { a_env } else { r_env };
        self.slow += (pk - self.slow) * (1.0 - (-blk / 0.3).exp());
        if p.attack > 0.0 && pk > 0.02 && pk > self.slow * 2.0 {
            self.att = 0.0;
        }
        let att_step = if p.attack > 0.0 { blk / (p.attack.min(PAN_ATTACK_MAX) * 0.001) } else { 1.0 };
        let hz = hz * 2f64.powf((p.dyn_mod.clamp(-1.0, 1.0) * 2.0 * self.env.min(1.0)) as f64);
        let cycles = hz * n as f64 / sr as f64;
        let spin = !p.tremolo && p.spin_mode && !p.wave.random();
        let ph_r = if spin { 0.0 } else { p.phase as f64 / 360.0 };
        let cyc_r = if spin { cycles * (1.0 + p.spin.clamp(0.0, 1.0) as f64) } else { cycles };
        let step = (cycles as f32).min(1.0);
        let amt0 = p.amount.clamp(0.0, 1.0);
        self.att = (self.att + att_step).min(1.0);
        let amt = amt0 * self.att;
        let pos_l = base + off;
        let pos_r = if spin { self.ph[1].pos + off } else { base + off + ph_r };
        let vl = self.value(p, 0, pos_l, step);
        let vr = self.value(p, 1, pos_r, step);
        let gain = |v: f32| {
            let mut g = 1.0 - amt * (0.5 + 0.5 * v);
            if p.tremolo && p.vintage {
                // Vintage: more time near full level.
                g = 1.0 - (1.0 - g) * (1.0 - g);
            }
            g
        };
        let (tl, tr) = if p.tremolo { (gain(vl), gain(vl)) } else { (gain(vl), gain(vr)) };
        let thi = if p.tremolo && p.harmonic { gain(-vl) } else { tl };
        let (g0l, g0r) = (self.g[0], self.g[1]);
        let inv = 1.0 / n.max(1) as f32;
        let xk = 1.0 - (-TAU * 600.0 / sr).exp();
        if p.tremolo && p.harmonic {
            // Harmonic: below / above 600 Hz move in opposite directions.
            let h0 = self.g[1];
            for i in 0..n {
                let t = (i + 1) as f32 * inv;
                let glo = g0l + (tl - g0l) * t;
                let ghi = h0 + (thi - h0) * t;
                for (c, s) in [&mut l[i], &mut r[i]].into_iter().enumerate() {
                    self.xo[c] += xk * (*s - self.xo[c]);
                    let lo = self.xo[c];
                    *s = lo * glo + (*s - lo) * ghi;
                }
            }
            self.g = [tl, thi];
        } else {
            for i in 0..n {
                let t = (i + 1) as f32 * inv;
                l[i] *= g0l + (tl - g0l) * t;
                r[i] *= g0r + (tr - g0r) * t;
            }
            self.g = [tl, tr];
        }
        self.ph[0].advance(cycles);
        self.ph[1].advance(cyc_r);
        if let Some(t) = &self.tap {
            t.set(0, (pos_l.fract()) as f32);
            t.set(1, vl);
            t.set(2, vr);
            t.set(3, tl);
            t.set(4, tr);
            t.set(5, (pos_r - pos_l).rem_euclid(1.0) as f32);
        }
    }
}

// ----------------------------------------------------------------------------
// Auto Filter.
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FilterType {
    #[default]
    LowPass,
    HighPass,
    BandPass,
    Notch,
    Morph,
    Dj,
    Comb,
    Resampling,
    NotchLp,
    Vowel,
}

impl FilterType {
    pub const ALL: [FilterType; 10] = [
        FilterType::LowPass,
        FilterType::HighPass,
        FilterType::BandPass,
        FilterType::Notch,
        FilterType::Morph,
        FilterType::Dj,
        FilterType::Comb,
        FilterType::Resampling,
        FilterType::NotchLp,
        FilterType::Vowel,
    ];
    pub fn label(self) -> &'static str {
        match self {
            FilterType::LowPass => "Lowpass",
            FilterType::HighPass => "Highpass",
            FilterType::BandPass => "Bandpass",
            FilterType::Notch => "Notch",
            FilterType::Morph => "Morph",
            FilterType::Dj => "DJ",
            FilterType::Comb => "Comb",
            FilterType::Resampling => "Resampling",
            FilterType::NotchLp => "Notch + LP",
            FilterType::Vowel => "Vowel",
        }
    }
    /// Circuit chooser applies (LP / HP / BP / Notch / Morph).
    pub fn has_circuit(self) -> bool {
        matches!(self, FilterType::LowPass | FilterType::HighPass | FilterType::BandPass | FilterType::Notch | FilterType::Morph)
    }
    /// Morph slider shown.
    pub fn has_morph(self) -> bool {
        matches!(self, FilterType::Morph | FilterType::Comb | FilterType::NotchLp | FilterType::Vowel)
    }
    /// Slope buttons: 12/24 (or 6/12/24/48 for Morph); none for the others.
    pub fn slopes(self) -> &'static [u8] {
        match self {
            FilterType::LowPass | FilterType::HighPass | FilterType::BandPass | FilterType::Notch | FilterType::NotchLp => &[12, 24],
            FilterType::Morph => &[6, 12, 24, 48],
            _ => &[],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Circuit {
    #[default]
    Svf,
    Dfm,
    Ms2,
    Prd,
}

impl Circuit {
    pub const ALL: [Circuit; 4] = [Circuit::Svf, Circuit::Dfm, Circuit::Ms2, Circuit::Prd];
    pub fn label(self) -> &'static str {
        match self {
            Circuit::Svf => "SVF",
            Circuit::Dfm => "DFM",
            Circuit::Ms2 => "MS2",
            Circuit::Prd => "PRD",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AfWave {
    #[default]
    Sine,
    Triangle,
    Saw,
    Square,
    RampUp,
    RampDown,
    Wander,
    SampleHold,
}

impl AfWave {
    pub const ALL: [AfWave; 8] =
        [AfWave::Sine, AfWave::Triangle, AfWave::Saw, AfWave::Square, AfWave::RampUp, AfWave::RampDown, AfWave::Wander, AfWave::SampleHold];
    pub fn label(self) -> &'static str {
        match self {
            AfWave::Sine => "Sine",
            AfWave::Triangle => "Triangle",
            AfWave::Saw => "Saw",
            AfWave::Square => "Square",
            AfWave::RampUp => "Ramp Up",
            AfWave::RampDown => "Ramp Down",
            AfWave::Wander => "Wander",
            AfWave::SampleHold => "S&H",
        }
    }
    pub fn random(self) -> bool {
        matches!(self, AfWave::Wander | AfWave::SampleHold)
    }
    /// Value (−1..1) at phase `ph` with Morph `m` (−1..1, skew).
    pub fn at(self, ph: f32, m: f32) -> f32 {
        let p = ph.rem_euclid(1.0);
        let w = 0.5 + 0.49 * m.clamp(-1.0, 1.0);
        let q = if p < w { 0.5 * p / w } else { 0.5 + 0.5 * (p - w) / (1.0 - w) };
        match self {
            AfWave::Sine => (TAU * q).sin(),
            AfWave::Triangle => {
                if q < 0.25 {
                    4.0 * q
                } else if q < 0.75 {
                    2.0 - 4.0 * q
                } else {
                    4.0 * q - 4.0
                }
            }
            AfWave::Saw => 1.0 - 2.0 * q,
            AfWave::Square => {
                if p < w {
                    1.0
                } else {
                    -1.0
                }
            }
            AfWave::RampUp => 2.0 * q * q - 1.0,
            AfWave::RampDown => 2.0 * (1.0 - q) * (1.0 - q) - 1.0,
            _ => 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Quantize {
    #[default]
    None,
    Steps,
    SampleHold,
}

impl Quantize {
    pub const ALL: [Quantize; 3] = [Quantize::None, Quantize::Steps, Quantize::SampleHold];
    pub fn label(self) -> &'static str {
        match self {
            Quantize::None => "None",
            Quantize::Steps => "Steps",
            Quantize::SampleHold => "S&H",
        }
    }
}

pub const AF_FREQ: (f32, f32) = (20.0, 20_000.0);
pub const AF_PITCH: (f32, f32) = (-24.0, 24.0);
pub const AF_OUT: (f32, f32) = (-24.0, 24.0);
pub const AF_ATTACK: (f32, f32) = (0.1, 100.0);
pub const AF_RELEASE: (f32, f32) = (1.0, 3000.0);
pub const AF_STEPS: (u8, u8) = (2, 32);
/// LFO / envelope modulation ranges (octaves at 100 %).
pub const AF_LFO_OCT: f32 = 5.0;
pub const AF_ENV_OCT: f32 = 7.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoFilterParams {
    pub kind: FilterType,
    /// dB/oct (12 / 24, Morph 6 / 12 / 24 / 48).
    pub slope: u8,
    pub circuit: Circuit,
    pub freq: f32,
    /// DJ Control −1..1.
    pub dj: f32,
    /// Vowel Pitch (semitones).
    pub pitch: f32,
    /// 0..1 (Vowel: Formant).
    pub res: f32,
    pub morph: f32,
    pub drive: f32,
    pub clip: bool,
    pub output: f32,
    pub dry_wet: f32,
    // LFO.
    pub lfo_amt: f32,
    pub rate: Rate,
    pub wave: AfWave,
    /// −1..1 (S&H: Smooth 0..1).
    pub lfo_morph: f32,
    pub spin_mode: bool,
    pub phase: f32,
    pub spin: f32,
    pub offset: f32,
    pub quantize: Quantize,
    pub steps: u8,
    /// Quantize S&H rate (index into [`SYNC_DIVS`]).
    pub q_div: u8,
    // Envelope.
    pub env_amt: f32,
    pub attack: f32,
    pub hold: bool,
    pub release: f32,
    pub env_sh: bool,
    pub env_div: u8,
    /// Envelope section selected (else LFO): the display row shows it.
    pub env_view: bool,
    pub sc: Sidechain,
    pub sc_mono: bool,
}

impl Default for AutoFilterParams {
    fn default() -> Self {
        Self {
            kind: FilterType::LowPass,
            slope: 24,
            circuit: Circuit::Svf,
            freq: 10_000.0,
            dj: 0.0,
            pitch: 0.0,
            res: 0.0,
            morph: 0.0,
            drive: 0.0,
            clip: false,
            output: 0.0,
            dry_wet: 1.0,
            lfo_amt: 0.0,
            rate: Rate::default(),
            wave: AfWave::Sine,
            lfo_morph: 0.0,
            spin_mode: false,
            phase: 0.0,
            spin: 0.0,
            offset: 0.0,
            quantize: Quantize::None,
            steps: 8,
            q_div: 2,
            env_amt: 0.0,
            attack: 6.0,
            hold: false,
            release: 200.0,
            env_sh: false,
            env_div: 2,
            env_view: false,
            sc: Sidechain::default(),
            sc_mono: true,
        }
    }
}

impl AutoFilterParams {
    pub fn slope_ok(&self) -> u8 {
        let s = self.kind.slopes();
        if s.is_empty() || s.contains(&self.slope) { self.slope } else { s[s.len() - 1].min(24) }
    }
    /// Cutoff (Hz) before modulation for every type (DJ / Vowel included).
    pub fn cutoff(&self) -> f32 {
        match self.kind {
            FilterType::Dj => {
                let c = self.dj.clamp(-1.0, 1.0);
                if c < 0.0 { 20_000.0 * 2f32.powf(c * 8.0) } else { 20.0 * 2f32.powf(c * 9.5) }
            }
            FilterType::Vowel => 1000.0 * 2f32.powf(self.pitch.clamp(AF_PITCH.0, AF_PITCH.1) / 12.0),
            _ => self.freq.clamp(AF_FREQ.0, AF_FREQ.1),
        }
    }
    /// Filter Q from Res (0.5..~25).
    pub fn q(&self) -> f32 {
        let r = self.res.clamp(0.0, 1.0);
        match self.kind {
            FilterType::Dj => 0.707 + r * self.dj.abs() * 5.0,
            _ => 0.707 * 30f32.powf(r * r * 0.5 + r * 0.5),
        }
    }
    /// Magnitude (dB) at `f` for cutoff `fc` (display; analog prototype).
    pub fn response_db(&self, fc: f32, f: f32) -> f32 {
        let h = self.response(fc, f);
        20.0 * h.max(1e-6).log10()
    }

    fn response(&self, fc: f32, f: f32) -> f32 {
        let x = (f / fc.max(1.0)) as f64;
        let q = self.q() as f64;
        // s = j x.
        let svf = |q: f64| {
            let d = C::new(1.0 - x * x, x / q);
            let lp = C::new(1.0, 0.0).div(d);
            let bp = C::new(0.0, x / q).div(d);
            let hp = C::new(-x * x, 0.0).div(d);
            (lp, bp, hp)
        };
        let stages = |n: u32| -> f64 { n as f64 };
        let slope = self.slope_ok();
        let n24 = if slope >= 24 { 2 } else { 1 };
        let mag = match self.kind {
            FilterType::LowPass | FilterType::HighPass | FilterType::BandPass | FilterType::Notch => {
                let (lp, bp, hp) = svf(q);
                let v = match self.kind {
                    FilterType::LowPass => lp,
                    FilterType::HighPass => hp,
                    FilterType::BandPass => bp.scale(1.0 / q),
                    _ => lp.add(hp),
                };
                if self.circuit == Circuit::Prd && self.kind == FilterType::LowPass && n24 == 2 {
                    // Ladder: G⁴ / (1 + k G⁴).
                    let k = 4.0 * (1.0 - 1.0 / (q / 0.707)).clamp(0.0, 0.97);
                    let g = C::new(1.0, 0.0).div(C::new(1.0, x));
                    let g4 = g.mul(g).mul(g).mul(g);
                    g4.div(C::new(1.0, 0.0).add(g4.scale(k))).scale(1.0 + k).abs()
                } else {
                    let m = v.abs();
                    if n24 == 2 {
                        let (l2, b2, h2) = svf(0.707);
                        let m2 = match self.kind {
                            FilterType::LowPass => l2,
                            FilterType::HighPass => h2,
                            FilterType::BandPass => b2.scale(1.0 / 0.707),
                            _ => l2.add(h2),
                        };
                        m * m2.abs()
                    } else {
                        m
                    }
                }
            }
            FilterType::Morph => {
                let m = self.morph.clamp(0.0, 1.0) as f64;
                let one = |q: f64| {
                    let (lp, bp, hp) = svf(q);
                    if m < 0.5 {
                        lp.scale(1.0 - 2.0 * m).add(bp.scale(2.0 * m / q))
                    } else {
                        bp.scale((2.0 - 2.0 * m) / q).add(hp.scale(2.0 * m - 1.0))
                    }
                };
                match slope {
                    6 => {
                        let lp = C::new(1.0, 0.0).div(C::new(1.0, x));
                        let hp = C::new(0.0, x).div(C::new(1.0, x));
                        lp.scale(1.0 - m).add(hp.scale(m)).abs()
                    }
                    12 => one(q).abs(),
                    24 => one(q).abs() * one(0.707).abs(),
                    _ => one(q).abs() * one(0.707).abs().powf(stages(3)),
                }
            }
            FilterType::Dj => {
                let (lp, _, hp) = svf(q);
                let c = self.dj.clamp(-1.0, 1.0);
                if c.abs() < 0.01 {
                    1.0
                } else if c < 0.0 {
                    lp.abs()
                } else {
                    hp.abs()
                }
            }
            FilterType::Comb => {
                let g = (self.res.clamp(0.0, 1.0) * 0.95) as f64;
                let w = std::f64::consts::TAU * x;
                let e = C::new(w.cos(), -w.sin());
                let peaks = C::new(1.0 - g, 0.0).div(C::new(1.0, 0.0).sub(e.scale(g))).abs();
                let notch = C::new(1.0, 0.0).sub(e.scale(0.5 + 0.5 * g)).abs() / (1.5 + 0.5 * g) * 1.5;
                let m = self.morph.clamp(0.0, 1.0) as f64;
                peaks * (1.0 - m) + notch * m
            }
            FilterType::Resampling => 1.0,
            FilterType::NotchLp => {
                let (lp, _, _) = svf(q);
                let fnot = notch_ratio(self.morph) as f64;
                let y = x / fnot;
                let d = C::new(1.0 - y * y, y / 2.0);
                let notch = C::new(1.0 - y * y, 0.0).div(d);
                let lp2 = if n24 == 2 { svf(0.707).0.abs() } else { 1.0 };
                lp.abs() * lp2 * notch.abs()
            }
            FilterType::Vowel => {
                let fm = vowel_formants(self.res, self.morph);
                let mut acc = C::new(0.0, 0.0);
                for (ff, g, bq) in fm {
                    let y = (f as f64) / (ff as f64 * fc as f64 / 1000.0);
                    let d = C::new(1.0 - y * y, y / bq as f64);
                    acc = acc.add(C::new(0.0, y / bq as f64).div(d).scale(g as f64));
                }
                acc.abs()
            }
        };
        mag as f32
    }
}

/// Notch + LP: notch frequency relative to the cutoff (Morph 1 = at it).
fn notch_ratio(morph: f32) -> f32 {
    2f32.powf(-(1.0 - morph.clamp(0.0, 1.0)) * 4.0)
}

/// Vowel formants (relative to 1 kHz pitch reference): (freq, gain, Q).
/// `formant` 0..1 morphs a → e → i → o → u; Morph normalizes the gains.
fn vowel_formants(formant: f32, morph: f32) -> [(f32, f32, f32); 3] {
    const V: [[f32; 3]; 5] =
        [[800.0, 1150.0, 2900.0], [400.0, 1600.0, 2700.0], [270.0, 2140.0, 2950.0], [450.0, 800.0, 2830.0], [325.0, 700.0, 2530.0]];
    let x = formant.clamp(0.0, 1.0) * 4.0;
    let i = (x.floor() as usize).min(3);
    let t = x - i as f32;
    let g = [1.0, 0.5, 0.25];
    let m = morph.clamp(0.0, 1.0);
    std::array::from_fn(|k| (V[i][k] + (V[i + 1][k] - V[i][k]) * t, g[k] + (1.0 - g[k]) * m, 8.0 + 4.0 * k as f32))
}

#[derive(Debug, Clone, Copy)]
struct C {
    re: f64,
    im: f64,
}

impl C {
    fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }
    fn add(self, o: C) -> C {
        C::new(self.re + o.re, self.im + o.im)
    }
    fn sub(self, o: C) -> C {
        C::new(self.re - o.re, self.im - o.im)
    }
    fn mul(self, o: C) -> C {
        C::new(self.re * o.re - self.im * o.im, self.re * o.im + self.im * o.re)
    }
    fn div(self, o: C) -> C {
        let d = o.re * o.re + o.im * o.im;
        C::new((self.re * o.re + self.im * o.im) / d, (self.im * o.re - self.re * o.im) / d)
    }
    fn scale(self, k: f64) -> C {
        C::new(self.re * k, self.im * k)
    }
    fn abs(self) -> f64 {
        (self.re * self.re + self.im * self.im).sqrt()
    }
}

/// Simper TPT state-variable filter (one channel, one stage).
#[derive(Debug, Clone, Copy, Default)]
struct Svf {
    ic1: f32,
    ic2: f32,
}

#[derive(Debug, Clone, Copy, Default)]
struct SvfK {
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
}

impl SvfK {
    fn new(fc: f32, q: f32, sr: f32) -> Self {
        let g = (PI * (fc / sr).clamp(1e-5, 0.49)).tan();
        let k = 1.0 / q.max(0.05);
        let a1 = 1.0 / (1.0 + g * (g + k));
        Self { k, a1, a2: g * a1, a3: g * g * a1 }
    }
}

impl Svf {
    /// (lp, bp, hp); `sat` = circuit nonlinearity.
    #[inline]
    fn tick(&mut self, c: &SvfK, x: f32, circuit: Circuit) -> (f32, f32, f32) {
        let v3 = x - self.ic2;
        let mut v1 = c.a1 * self.ic1 + c.a2 * v3;
        let v2 = self.ic2 + c.a2 * self.ic1 + c.a3 * v3;
        match circuit {
            Circuit::Dfm => v1 = soft(v1 * 1.2) / 1.2,
            Circuit::Ms2 => v1 = soft(v1),
            _ => {}
        }
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        if !(self.ic1.is_finite() && self.ic2.is_finite()) {
            *self = Svf::default();
        }
        (v2, v1, x - c.k * v1 - v2)
    }
}

/// ZDF 4-pole ladder (PRD): no resonance limiting.
#[derive(Debug, Clone, Copy, Default)]
struct Ladder {
    s: [f32; 4],
}

impl Ladder {
    /// Returns the 4 pole outputs (y1..y4) and input after feedback (y0).
    #[inline]
    fn tick(&mut self, g: f32, k: f32, x: f32) -> [f32; 5] {
        let gg = g / (1.0 + g);
        let gs = [gg * gg * gg / (1.0 + g), gg * gg / (1.0 + g), gg / (1.0 + g), 1.0 / (1.0 + g)];
        let sigma = gs[0] * self.s[0] + gs[1] * self.s[1] + gs[2] * self.s[2] + gs[3] * self.s[3];
        let gg4 = gg * gg * gg * gg;
        let u = (x - k * sigma) / (1.0 + k * gg4);
        let u = soft(u * 0.8) / 0.8;
        let mut y = [u, 0.0, 0.0, 0.0, 0.0];
        let mut inp = u;
        for i in 0..4 {
            let v = (inp - self.s[i]) * gg;
            let o = v + self.s[i];
            self.s[i] = o + v;
            y[i + 1] = o;
            inp = o;
        }
        if !self.s.iter().all(|v| v.is_finite()) {
            *self = Ladder::default();
        }
        y
    }
}

const COMB_LEN: usize = 1 << 13;
const AF_SUB: usize = 16;

#[derive(Debug, Clone)]
pub struct AutoFilterState {
    svf: [[Svf; 4]; 2],
    notch: [Svf; 2],
    vowel: [[Svf; 3]; 2],
    ladder: [Ladder; 2],
    comb: [Box<[f32]>; 2],
    comb_w: usize,
    rs: [(f32, f32); 2],
    ph: [Phasor; 2],
    rng: u64,
    rnd: [(f32, f32, i64); 2],
    wander: [f32; 2],
    qhold: [(f32, i64); 2],
    env: [f32; 2],
    env_hold: [usize; 2],
    env_sh: ([f32; 2], i64),
    /// Smoothed log2 cutoff per channel.
    cur: [f32; 2],
    init: bool,
    pub sc: ScState,
    tap: Option<Arc<DevTap>>,
    spec: Option<Arc<crate::eq8::SpecTap>>,
}

impl AutoFilterState {
    pub fn new(id: Option<u64>) -> Self {
        Self {
            svf: Default::default(),
            notch: Default::default(),
            vowel: Default::default(),
            ladder: Default::default(),
            comb: [vec![0.0; COMB_LEN].into_boxed_slice(), vec![0.0; COMB_LEN].into_boxed_slice()],
            comb_w: 0,
            rs: [(0.0, 0.0); 2],
            ph: Default::default(),
            rng: 0x9e37_79b9_7f4a_7c15 ^ id.unwrap_or(5),
            rnd: [(0.0, 0.0, i64::MIN); 2],
            wander: [0.0; 2],
            qhold: [(0.0, i64::MIN); 2],
            env: [0.0; 2],
            env_hold: [0; 2],
            env_sh: ([0.0; 2], i64::MIN),
            cur: [0.0; 2],
            init: false,
            sc: ScState::default(),
            tap: id.map(dev_tap),
            spec: id.map(crate::eq8::spectrum_tap),
        }
    }

    pub fn reset(&mut self) {
        self.svf = Default::default();
        self.notch = Default::default();
        self.vowel = Default::default();
        self.ladder = Default::default();
        for c in self.comb.iter_mut() {
            c.fill(0.0);
        }
        self.env = [0.0; 2];
        self.init = false;
        self.sc.reset();
    }

    fn lfo(&mut self, p: &AutoFilterParams, c: usize, ph: f64, step: f32) -> f32 {
        let cyc = ph.floor() as i64;
        if self.rnd[c].2 != cyc {
            let a = if self.rnd[c].2 == i64::MIN { xorshift(&mut self.rng) } else { self.rnd[c].1 };
            self.rnd[c] = (a, xorshift(&mut self.rng), cyc);
        }
        let fr = ph.fract() as f32;
        let v = match p.wave {
            AfWave::Wander => {
                let r = xorshift(&mut self.rng);
                let w = &mut self.wander[c];
                *w = (*w + r * 1.4 * step.sqrt() - *w * step * 0.6).clamp(-1.0, 1.0);
                *w
            }
            AfWave::SampleHold => {
                // Smooth (Morph): glide into the next value.
                let s = p.lfo_morph.clamp(0.0, 1.0);
                let (a, b, _) = self.rnd[c];
                if s > 0.0 && fr > 1.0 - s { a + (b - a) * ((fr - (1.0 - s)) / s) } else { a }
            }
            w => w.at(fr, p.lfo_morph),
        };
        match p.quantize {
            Quantize::None => v,
            Quantize::Steps => {
                let n = p.steps.clamp(AF_STEPS.0, AF_STEPS.1) as f32;
                ((v * 0.5 + 0.5) * (n - 1.0)).round() / (n - 1.0) * 2.0 - 1.0
            }
            Quantize::SampleHold => v,
        }
    }

    pub fn process(&mut self, p: &AutoFilterParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let n = l.len().min(r.len());
        let (tempo, beat) = clock();
        // --- LFO (per block) ---
        let hz = p.rate.hz(tempo);
        let base = self.ph[0].start(&p.rate, beat);
        let spin = p.spin_mode && !p.wave.random();
        if p.rate.mode.synced() && beat.is_some() {
            self.ph[1].pos = base;
        }
        let off = if p.rate.mode.synced() { p.offset as f64 / 360.0 } else { 0.0 };
        let cycles = hz * n as f64 / sr as f64;
        let step = (cycles as f32).min(1.0);
        let pl = base + off;
        let pr = if spin { self.ph[1].pos + off } else { pl + p.phase as f64 / 360.0 };
        let mut lv = [self.lfo(p, 0, pl, step), self.lfo(p, 1, pr, step)];
        if p.quantize == Quantize::SampleHold {
            // Hold the LFO for one beat division.
            let div = SYNC_DIVS[(p.q_div as usize).min(SYNC_DIVS.len() - 1)].0;
            let b = beat.unwrap_or(0.0);
            let cyc = (b / div).floor() as i64 + if beat.is_none() { (pl * 8.0).floor() as i64 } else { 0 };
            for (c, v) in lv.iter_mut().enumerate() {
                if self.qhold[c].1 != cyc {
                    self.qhold[c] = (*v, cyc);
                }
                *v = self.qhold[c].0;
            }
        }
        self.ph[0].advance(cycles);
        self.ph[1].advance(if spin { cycles * (1.0 + p.spin.clamp(0.0, 1.0) as f64) } else { cycles });

        // --- Sidechain / envelope source ---
        let g_ext = self.sc.prepare(&p.sc, sr);
        let a_c = 1.0 - (-1.0 / (p.attack.clamp(AF_ATTACK.0, AF_ATTACK.1) * 0.001 * sr)).exp();
        let r_c = 1.0 - (-1.0 / (p.release.clamp(AF_RELEASE.0, AF_RELEASE.1) * 0.001 * sr)).exp();
        let hold_n = (p.attack.clamp(AF_ATTACK.0, AF_ATTACK.1) * 0.001 * sr) as usize;
        let env_on = p.env_amt.abs() > 1e-4 || p.sc.listen;
        let fc0 = p.cutoff().log2();
        let q = p.q();
        let slope = p.slope_ok();
        let drive = db_lin(p.drive.clamp(0.0, 1.0) * 24.0);
        let out_g = db_lin(p.output.clamp(AF_OUT.0, AF_OUT.1));
        let wet = p.dry_wet.clamp(0.0, 1.0);
        let lfo_oct = p.lfo_amt.clamp(0.0, 1.0) * AF_LFO_OCT;
        let env_oct = p.env_amt.clamp(-1.0, 1.0) * AF_ENV_OCT;
        if !self.init {
            self.cur = [fc0 + lv[0] * lfo_oct, fc0 + lv[1] * lfo_oct];
            self.init = true;
        }
        let mut clip_pk = 0.0f32;
        let m = COMB_LEN - 1;
        let fv = vowel_formants(p.res, p.morph);
        let mut i0 = 0;
        let mut envs = [0.0f32; 2];
        while i0 < n {
            let i1 = (i0 + AF_SUB).min(n);
            // Envelope over the sub-block.
            if env_on {
                for i in i0..i1 {
                    let (a, b) = self.sc.detect(&p.sc, i, l[i], r[i], g_ext);
                    let lv2 = if p.sc_mono { [a.abs().max(b.abs()); 2] } else { [a.abs(), b.abs()] };
                    for (c, &x) in lv2.iter().enumerate() {
                        let e = &mut self.env[c];
                        if x > *e {
                            *e += (x - *e) * a_c;
                            if p.hold {
                                self.env_hold[c] = hold_n;
                            }
                        } else if self.env_hold[c] > 0 {
                            self.env_hold[c] -= 1;
                        } else {
                            *e += (x - *e) * r_c;
                        }
                    }
                    if p.sc.listen {
                        l[i] = a;
                        r[i] = b;
                    }
                }
            }
            if p.sc.listen {
                i0 = i1;
                continue;
            }
            envs = self.env;
            if p.env_sh {
                let div = SYNC_DIVS[(p.env_div as usize).min(SYNC_DIVS.len() - 1)].0;
                let pos = beat.unwrap_or(self.ph[0].pos * 4.0) + (i0 as f64 / sr as f64) * tempo / 60.0;
                let cyc = (pos / div).floor() as i64;
                if self.env_sh.1 != cyc {
                    self.env_sh = (self.env, cyc);
                }
                envs = self.env_sh.0;
            }
            // Cutoff per channel (smoothed in log2).
            let mut ks = [SvfK::default(); 2];
            let mut fcs = [0.0f32; 2];
            for c in 0..2 {
                let target = fc0 + lv[c] * lfo_oct + envs[c].min(1.0) * env_oct;
                self.cur[c] += (target - self.cur[c]) * 0.35;
                let fc = 2f32.powf(self.cur[c]).clamp(AF_FREQ.0, (0.45 * sr).min(22_000.0));
                fcs[c] = fc;
                ks[c] = SvfK::new(fc, q, sr);
            }
            let k2: [SvfK; 2] = std::array::from_fn(|c| SvfK::new(fcs[c], 0.707, sr));
            let mut kv = [[SvfK::default(); 3]; 2];
            if p.kind == FilterType::Vowel {
                for c in 0..2 {
                    for (j, &(ff, _, bq)) in fv.iter().enumerate() {
                        kv[c][j] = SvfK::new((ff * fcs[c] / 1000.0).clamp(AF_FREQ.0, 0.45 * sr), bq, sr);
                    }
                }
            }
            for i in i0..i1 {
                for (c, buf) in [&mut *l, &mut *r].into_iter().enumerate() {
                    let dry = buf[i];
                    let x = dry * drive;
                    let k = &ks[c];
                    let ci = p.circuit;
                    let st = &mut self.svf[c];
                    let y = match p.kind {
                        FilterType::LowPass | FilterType::HighPass | FilterType::BandPass | FilterType::Notch if ci == Circuit::Prd => {
                            let g = (PI * (fcs[c] / sr).clamp(1e-5, 0.49)).tan();
                            let kk = 4.0 * (1.0 - 0.707 / q).clamp(0.0, 1.05);
                            let y = self.ladder[c].tick(g, kk, x);
                            let (lp, hp, bp) = if slope >= 24 {
                                (y[4], y[0] - 4.0 * y[1] + 6.0 * y[2] - 4.0 * y[3] + y[4], 4.0 * (y[2] - 2.0 * y[3] + y[4]))
                            } else {
                                (y[2], y[0] - 2.0 * y[1] + y[2], 2.0 * (y[1] - y[2]))
                            };
                            match p.kind {
                                FilterType::LowPass => lp,
                                FilterType::HighPass => hp,
                                FilterType::BandPass => bp,
                                _ => lp + hp,
                            }
                        }
                        FilterType::LowPass | FilterType::HighPass | FilterType::BandPass | FilterType::Notch => {
                            let pick = |o: (f32, f32, f32), q: f32| match p.kind {
                                FilterType::LowPass => o.0,
                                FilterType::HighPass => o.2,
                                FilterType::BandPass => o.1 / q,
                                _ => o.0 + o.2,
                            };
                            let a = pick(st[0].tick(k, x, ci), q);
                            if slope >= 24 { pick(st[1].tick(&k2[c], a, ci), 0.707) } else { a }
                        }
                        FilterType::Morph => {
                            let mo = p.morph.clamp(0.0, 1.0);
                            let mix = |o: (f32, f32, f32), q: f32| {
                                if mo < 0.5 {
                                    o.0 * (1.0 - 2.0 * mo) + o.1 / q * 2.0 * mo
                                } else {
                                    o.1 / q * (2.0 - 2.0 * mo) + o.2 * (2.0 * mo - 1.0)
                                }
                            };
                            match slope {
                                6 => {
                                    // One-pole morph LP → HP.
                                    let o = st[0].tick(&k2[c], x, Circuit::Svf);
                                    let lp1 = o.0 + 0.5 * o.1 * 1.414;
                                    lp1 * (1.0 - mo) + (x - lp1) * mo
                                }
                                12 => mix(st[0].tick(k, x, ci), q),
                                24 => {
                                    let a = mix(st[0].tick(k, x, ci), q);
                                    mix(st[1].tick(&k2[c], a, ci), 0.707)
                                }
                                _ => {
                                    let mut a = mix(st[0].tick(k, x, ci), q);
                                    for s in st[1..4].iter_mut() {
                                        a = mix(s.tick(&k2[c], a, ci), 0.707);
                                    }
                                    a
                                }
                            }
                        }
                        FilterType::Dj => {
                            let o = st[0].tick(k, x, Circuit::Svf);
                            let o2 = st[1].tick(&k2[c], if p.dj < 0.0 { o.0 } else { o.2 }, Circuit::Svf);
                            let y = if p.dj < 0.0 { o2.0 } else { o2.2 };
                            let t = (p.dj.abs() * 20.0).min(1.0);
                            x + (y - x) * t
                        }
                        FilterType::Comb => {
                            let d = (sr / fcs[c]).clamp(1.0, (COMB_LEN - 2) as f32);
                            let pos = self.comb_w as f32 - d;
                            let ii = pos.floor();
                            let f = pos - ii;
                            let ii = (ii as isize).rem_euclid(COMB_LEN as isize) as usize;
                            let b = &self.comb[c];
                            let del = b[ii] + (b[(ii + 1) & m] - b[ii]) * f;
                            let g = p.res.clamp(0.0, 1.0) * 0.95;
                            let mo = p.morph.clamp(0.0, 1.0);
                            // Peaks: feedback comb; notches: feed-forward.
                            let fbk = x + g * del;
                            let ff = x - (0.5 + 0.5 * g) * del;
                            let ybuf = fbk * (1.0 - mo) + x * mo;
                            self.comb[c][self.comb_w] = ybuf;
                            (fbk * (1.0 - g)) * (1.0 - mo) + ff / (1.0 + 0.5 * g) * mo
                        }
                        FilterType::Resampling => {
                            let (ref mut acc, ref mut held) = self.rs[c];
                            *acc += fcs[c] * 2.0 / sr;
                            if *acc >= 1.0 {
                                *acc -= acc.floor();
                                *held = x;
                            }
                            *held
                        }
                        FilterType::NotchLp => {
                            let o = st[0].tick(k, x, ci);
                            let mut y = o.0;
                            if slope >= 24 {
                                y = st[1].tick(&k2[c], y, ci).0;
                            }
                            let kn = SvfK::new((fcs[c] * notch_ratio(p.morph)).max(AF_FREQ.0), 2.0, sr);
                            let o = self.notch[c].tick(&kn, y, Circuit::Svf);
                            o.0 + o.2
                        }
                        FilterType::Vowel => {
                            let mut acc = 0.0;
                            for (j, &(_, g, bq)) in fv.iter().enumerate() {
                                acc += self.vowel[c][j].tick(&kv[c][j], x, Circuit::Svf).1 / bq * g;
                            }
                            acc * 1.5
                        }
                    };
                    let mut y = y;
                    if p.clip {
                        clip_pk = clip_pk.max(y.abs());
                        y = soft(y);
                    }
                    buf[i] = (dry * (1.0 - wet) + y * wet) * out_g;
                }
                self.comb_w = (self.comb_w + 1) & m;
            }
            i0 = i1;
        }
        if let Some(t) = &self.tap {
            t.set(0, 2f32.powf(self.cur[0]));
            t.set(1, 2f32.powf(self.cur[1]));
            t.raise(2, clip_pk);
            t.set(3, envs[0]);
            t.set(4, lv[0]);
            t.set(5, (pl.fract()) as f32);
        }
        if let Some(s) = &self.spec {
            s.write(l, r, n, sr);
        }
    }
}

// ----------------------------------------------------------------------------
// Tests.
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(f: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| (TAU * f * i as f32 / 48_000.0).sin() * 0.5).collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    fn run_eq3(p: &EqThreeParams, f: f32) -> f32 {
        let mut st = EqThreeState::new(None);
        let mut l = sine(f, 48_000);
        let mut r = l.clone();
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            st.process(p, a, b, 48_000.0);
        }
        rms(&l[24_000..]) / rms(&sine(f, 48_000)[24_000..])
    }

    #[test]
    fn eq_three_flat_and_kills() {
        for slope48 in [false, true] {
            let p = EqThreeParams { slope48, ..Default::default() };
            for f in [60.0, 250.0, 1000.0, 2500.0, 8000.0] {
                let g = run_eq3(&p, f);
                assert!((g - 1.0).abs() < 0.03, "flat {slope48} {f}: {g}");
            }
            let kill = EqThreeParams { on: [false, true, true], ..p };
            assert!(run_eq3(&kill, 60.0) < 0.1);
            let kill = EqThreeParams { gain: [0.0, 0.0, EQ3_GAIN_MIN], ..p };
            assert!(run_eq3(&kill, 12_000.0) < 0.1);
        }
    }

    #[test]
    fn chorus_modes_run_and_vibrato_is_wet() {
        for mode in ChorusMode::ALL {
            let p = ChorusParams { mode, feedback: 0.5, warmth: 0.5, hp_on: true, ..Default::default() };
            let mut st = ChorusState::new(None);
            let mut l = sine(440.0, 24_000);
            let mut r = l.clone();
            for (a, b) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
                st.process(&p, a, b, 48_000.0);
            }
            assert!(l.iter().chain(r.iter()).all(|v| v.is_finite() && v.abs() < 4.0), "{mode:?}");
            assert!(rms(&l[12_000..]) > 0.1, "{mode:?}");
        }
    }

    #[test]
    fn autopan_pans_and_tremolo_modulates() {
        let p = AutoPanParams { amount: 1.0, ..Default::default() };
        let mut st = AutoPanState::new(None);
        // At 1 Hz, a quarter second in: L LFO at +1 → left silent, right full.
        let mut l = vec![0.5f32; 12_000];
        let mut r = l.clone();
        for (a, b) in l.chunks_mut(64).zip(r.chunks_mut(64)) {
            st.process(&p, a, b, 48_000.0);
        }
        assert!(l[11_990].abs() < 0.02 && (r[11_990] - 0.5).abs() < 0.02, "{} {}", l[11_990], r[11_990]);
        let t = AutoPanParams { tremolo: true, amount: 1.0, harmonic: true, vintage: true, ..Default::default() };
        let mut st = AutoPanState::new(None);
        let mut l = sine(1000.0, 48_000);
        let mut r = l.clone();
        for (a, b) in l.chunks_mut(64).zip(r.chunks_mut(64)) {
            st.process(&t, a, b, 48_000.0);
        }
        assert!(l.iter().all(|v| v.is_finite()));
        for w in PanWave::ALL {
            let p = AutoPanParams { wave: w, ..Default::default() };
            for i in 0..50 {
                let v = p.wave_at(i as f32 / 50.0, (0.3, -0.2));
                assert!((-1.0..=1.0).contains(&v), "{w:?}");
            }
        }
    }

    #[test]
    fn rate_modes() {
        let mut r = Rate { mode: TimeMode::Synced, div: 4, ..Default::default() };
        assert!((r.hz(120.0) - 2.0).abs() < 1e-9);
        r.mode = TimeMode::Dotted;
        assert!((r.hz(120.0) - 2.0 / 1.5).abs() < 1e-9);
        r.mode = TimeMode::Sixteenth;
        r.sixteenth = 4;
        assert!((r.hz(120.0) - 2.0).abs() < 1e-9);
        r.mode = TimeMode::Time;
        r.ms = 500.0;
        assert!((r.hz(120.0) - 2.0).abs() < 1e-9);
    }

    fn af_gain(p: &AutoFilterParams, f: f32) -> f32 {
        let mut st = AutoFilterState::new(None);
        let mut l = sine(f, 24_000);
        let mut r = l.clone();
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            st.process(p, a, b, 48_000.0);
        }
        assert!(l.iter().all(|v| v.is_finite()), "{:?}", p.kind);
        rms(&l[12_000..]) / rms(&sine(f, 24_000)[12_000..])
    }

    #[test]
    fn auto_filter_types() {
        let lp = AutoFilterParams { freq: 1000.0, ..Default::default() };
        assert!((af_gain(&lp, 100.0) - 1.0).abs() < 0.05);
        assert!(af_gain(&lp, 8000.0) < 0.02);
        let hp = AutoFilterParams { kind: FilterType::HighPass, ..lp };
        assert!(af_gain(&hp, 100.0) < 0.02 && (af_gain(&hp, 8000.0) - 1.0).abs() < 0.05);
        for c in Circuit::ALL {
            for kind in FilterType::ALL {
                for slope in [6, 12, 24, 48] {
                    let p = AutoFilterParams { kind, circuit: c, slope, res: 0.9, morph: 0.4, dj: -0.5, drive: 0.5, clip: true, ..lp };
                    let g = af_gain(&p, 440.0);
                    assert!(g < 8.0, "{kind:?} {c:?} {slope}: {g}");
                }
            }
        }
        // Response matches the DSP for the plain LP.
        assert!(lp.response_db(1000.0, 100.0).abs() < 0.5);
        assert!(lp.response_db(1000.0, 8000.0) < -60.0);
        // LFO moves the cutoff.
        let m = AutoFilterParams { lfo_amt: 1.0, ..lp };
        assert!((af_gain(&m, 440.0) - af_gain(&lp, 440.0)).abs() > 1e-3);
    }
}
