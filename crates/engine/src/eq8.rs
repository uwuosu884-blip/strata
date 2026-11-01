//! EQ Eight (Live 12): eight filter bands per channel, Stereo / L/R / M/S
//! processing, Adaptive Q, Scale, Output gain, band audition, optional 2x
//! oversampling and an output tap for the spectrum display.
//!
//! Filters are RBJ biquads (48 dB/oct = four cascaded sections). The same
//! coefficient code drives the GUI curve, so the display matches the sound.

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

pub const EQ_FREQ_MIN: f32 = 10.0;
pub const EQ_FREQ_MAX: f32 = 22_000.0;
pub const EQ_GAIN_MAX: f32 = 15.0;
pub const EQ_Q_MIN: f32 = 0.1;
pub const EQ_Q_MAX: f32 = 18.0;
/// Scale ±200 % (1.0 = 100 %).
pub const EQ_SCALE_MAX: f32 = 2.0;
pub const EQ_OUT_MAX: f32 = 12.0;
/// Analyzer block sizes (FFT length).
pub const EQ_BLOCKS: [u32; 5] = [1024, 2048, 4096, 8192, 16384];
pub const EQ_REFRESH_MIN: f32 = 10.0;
pub const EQ_REFRESH_MAX: f32 = 400.0;
pub const EQ_AVG_MAX: f32 = 100.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EqFilter {
    LowCut48,
    LowCut24,
    LowCut12,
    LowShelf,
    #[default]
    Bell,
    Notch,
    HighShelf,
    HighCut12,
    HighCut24,
    HighCut48,
}

impl EqFilter {
    /// Live's chooser order (top to bottom).
    pub const ALL: [EqFilter; 10] = [
        EqFilter::LowCut48,
        EqFilter::LowCut24,
        EqFilter::LowCut12,
        EqFilter::LowShelf,
        EqFilter::Bell,
        EqFilter::Notch,
        EqFilter::HighShelf,
        EqFilter::HighCut12,
        EqFilter::HighCut24,
        EqFilter::HighCut48,
    ];

    pub fn label(self) -> &'static str {
        match self {
            EqFilter::LowCut48 => "Low Cut 48 dB/oct",
            EqFilter::LowCut24 => "Low Cut 24 dB/oct",
            EqFilter::LowCut12 => "Low Cut 12 dB/oct",
            EqFilter::LowShelf => "Low Shelf",
            EqFilter::Bell => "Bell",
            EqFilter::Notch => "Notch",
            EqFilter::HighShelf => "High Shelf",
            EqFilter::HighCut12 => "High Cut 12 dB/oct",
            EqFilter::HighCut24 => "High Cut 24 dB/oct",
            EqFilter::HighCut48 => "High Cut 48 dB/oct",
        }
    }

    /// Bell and shelves have a Gain (and follow Scale / Adaptive Q).
    pub fn has_gain(self) -> bool {
        matches!(self, EqFilter::LowShelf | EqFilter::Bell | EqFilter::HighShelf)
    }

    /// Low / high cut (vertical drag = Q; the dot shows the resonance).
    pub fn is_cut(self) -> bool {
        matches!(
            self,
            EqFilter::LowCut48 | EqFilter::LowCut24 | EqFilter::LowCut12 | EqFilter::HighCut12 | EqFilter::HighCut24 | EqFilter::HighCut48
        )
    }

    /// Biquad sections the band runs.
    pub fn sections(self) -> usize {
        match self {
            EqFilter::LowCut48 | EqFilter::HighCut48 => 4,
            EqFilter::LowCut24 | EqFilter::HighCut24 => 2,
            _ => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EqBand {
    pub on: bool,
    pub kind: EqFilter,
    /// Hz.
    pub freq: f32,
    /// dB.
    pub gain: f32,
    pub q: f32,
}

impl Default for EqBand {
    fn default() -> Self {
        Self { on: false, kind: EqFilter::Bell, freq: 1000.0, gain: 0.0, q: 0.71 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EqMode {
    #[default]
    Stereo,
    LR,
    MS,
}

impl EqMode {
    pub const ALL: [EqMode; 3] = [EqMode::Stereo, EqMode::LR, EqMode::MS];
    pub fn label(self) -> &'static str {
        match self {
            EqMode::Stereo => "Stereo",
            EqMode::LR => "L/R",
            EqMode::MS => "M/S",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EqEightParams {
    /// `[0]` = Stereo / L / M, `[1]` = R / S.
    pub bands: [[EqBand; 8]; 2],
    pub mode: EqMode,
    /// Edit switch on R / S (L/R and M/S modes).
    pub edit_b: bool,
    pub adaptive_q: bool,
    /// Gain scale of bells and shelves (1.0 = 100 %).
    pub scale: f32,
    /// Output gain, dB.
    pub output: f32,
    /// Headphone switch: holding a band dot plays only that filter.
    pub audition: bool,
    pub analyze: bool,
    /// Context menu "Oversampling" (2x).
    pub oversample: bool,
    /// Selected band (0..8) shown in the Freq / Gain / Q dials.
    pub sel: u8,
    /// Display unfolded into the main window (expanded device view).
    pub expanded: bool,
    /// Analyzer FFT size, refresh (ms) and averaging.
    pub block: u32,
    pub refresh: f32,
    pub avg: f32,
    /// Band being auditioned (while its dot is held; not saved).
    #[serde(skip)]
    pub solo: Option<u8>,
    /// Dots enclosed by a drag in the display (bit per band; not saved).
    #[serde(skip)]
    pub dots: u8,
}

impl Default for EqEightParams {
    fn default() -> Self {
        // Live's default preset.
        let kinds = [
            EqFilter::LowShelf,
            EqFilter::Bell,
            EqFilter::Bell,
            EqFilter::HighShelf,
            EqFilter::Bell,
            EqFilter::Bell,
            EqFilter::Bell,
            EqFilter::HighCut12,
        ];
        let freqs = [30.0, 200.0, 1000.0, 5000.0, 100.0, 10_000.0, 5000.0, 18_000.0];
        let band = |i: usize| EqBand { on: i < 4, kind: kinds[i], freq: freqs[i], gain: 0.0, q: 0.71 };
        let ch: [EqBand; 8] = std::array::from_fn(band);
        Self {
            bands: [ch, ch],
            mode: EqMode::Stereo,
            edit_b: false,
            adaptive_q: true,
            scale: 1.0,
            output: 0.0,
            audition: false,
            analyze: true,
            oversample: false,
            sel: 4,
            expanded: false,
            block: 8192,
            refresh: 60.0,
            avg: 1.0,
            solo: None,
            dots: 0,
        }
    }
}

/// Normalised biquad coefficients (a0 = 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coefs {
    pub b: [f64; 3],
    pub a: [f64; 2],
}

impl Default for Coefs {
    fn default() -> Self {
        Self { b: [1.0, 0.0, 0.0], a: [0.0, 0.0] }
    }
}

impl Coefs {
    fn norm(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        Self { b: [b0 / a0, b1 / a0, b2 / a0], a: [a1 / a0, a2 / a0] }
    }

    /// |H|² at `w` (radians / sample).
    pub fn mag2(&self, w: f64) -> f64 {
        let (c1, c2) = (w.cos(), (2.0 * w).cos());
        let [b0, b1, b2] = self.b;
        let [a1, a2] = self.a;
        let num = b0 * b0 + b1 * b1 + b2 * b2 + 2.0 * (b0 * b1 + b1 * b2) * c1 + 2.0 * b0 * b2 * c2;
        let den = 1.0 + a1 * a1 + a2 * a2 + 2.0 * (a1 + a1 * a2) * c1 + 2.0 * a2 * c2;
        (num / den.max(1e-30)).max(0.0)
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Rbj {
    Lp,
    Hp,
    Bp,
    Notch,
    Peak,
    LowShelf,
    HighShelf,
}

pub(crate) fn rbj(kind: Rbj, f: f64, sr: f64, q: f64, db: f64) -> Coefs {
    let w = 2.0 * PI * (f / sr).clamp(1e-6, 0.4999);
    let (s, c) = w.sin_cos();
    let alpha = s / (2.0 * q.max(1e-3));
    let a = 10f64.powf(db / 40.0);
    match kind {
        Rbj::Lp => Coefs::norm((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha),
        Rbj::Hp => Coefs::norm((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha),
        Rbj::Bp => Coefs::norm(alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * c, 1.0 - alpha),
        Rbj::Notch => Coefs::norm(1.0, -2.0 * c, 1.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha),
        Rbj::Peak => Coefs::norm(1.0 + alpha * a, -2.0 * c, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * c, 1.0 - alpha / a),
        Rbj::LowShelf => {
            let k = 2.0 * a.sqrt() * alpha;
            Coefs::norm(
                a * ((a + 1.0) - (a - 1.0) * c + k),
                2.0 * a * ((a - 1.0) - (a + 1.0) * c),
                a * ((a + 1.0) - (a - 1.0) * c - k),
                (a + 1.0) + (a - 1.0) * c + k,
                -2.0 * ((a - 1.0) + (a + 1.0) * c),
                (a + 1.0) + (a - 1.0) * c - k,
            )
        }
        Rbj::HighShelf => {
            let k = 2.0 * a.sqrt() * alpha;
            Coefs::norm(
                a * ((a + 1.0) + (a - 1.0) * c + k),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
                a * ((a + 1.0) + (a - 1.0) * c - k),
                (a + 1.0) - (a - 1.0) * c + k,
                2.0 * ((a - 1.0) - (a + 1.0) * c),
                (a + 1.0) - (a - 1.0) * c - k,
            )
        }
    }
}

/// Butterworth 8th-order section Qs (θ = (2k−1)π/16).
const BW8_Q: [f64; 4] = [0.509_795_6, 0.601_344_9, 0.899_976_2, 2.562_915_4];
/// 4th-order Butterworth section Qs.
const BW4_Q: [f64; 2] = [0.541_196_1, 1.306_563];

/// Up to four cascaded sections.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Sections {
    pub c: [Coefs; 4],
    pub n: usize,
}

impl Sections {
    fn one(c: Coefs) -> Self {
        Self { c: [c, Coefs::default(), Coefs::default(), Coefs::default()], n: 1 }
    }

    pub fn mag_db(&self, w: f64) -> f64 {
        let m: f64 = self.c[..self.n].iter().map(|c| c.mag2(w)).product();
        10.0 * m.max(1e-30).log10()
    }
}

impl EqEightParams {
    /// Channel set edited by the dials / dots (0 in Stereo mode).
    pub fn edit_ch(&self) -> usize {
        if self.mode == EqMode::Stereo { 0 } else { self.edit_b as usize }
    }

    /// Effective (gain dB, Q) after Scale and Adaptive Q.
    pub fn effective(&self, b: &EqBand) -> (f32, f32) {
        if !b.kind.has_gain() {
            return (0.0, b.q.clamp(EQ_Q_MIN, EQ_Q_MAX));
        }
        let g = (b.gain * self.scale).clamp(-2.0 * EQ_GAIN_MAX * EQ_SCALE_MAX, 2.0 * EQ_GAIN_MAX * EQ_SCALE_MAX);
        // Adaptive Q: Q rises with the amount of boost / cut (analog consoles).
        let q = if self.adaptive_q { b.q * 10f32.powf(g.abs() / 40.0) } else { b.q };
        (g, q.clamp(EQ_Q_MIN, EQ_Q_MAX * 4.0))
    }

    /// Filter sections of a band at sample rate `sr`.
    pub fn sections(&self, b: &EqBand, sr: f32) -> Sections {
        let (g, q) = self.effective(b);
        let (f, sr, q, g) = (b.freq.clamp(EQ_FREQ_MIN, EQ_FREQ_MAX) as f64, sr as f64, q as f64, g as f64);
        let cut = |k: Rbj, qs: &[f64]| {
            let mut s = Sections { n: qs.len(), ..Default::default() };
            for (i, bq) in qs.iter().enumerate() {
                // Q above 0.71 raises the resonance of the sharpest section.
                let qq = if i == qs.len() - 1 { bq * q / std::f64::consts::FRAC_1_SQRT_2 } else { *bq };
                s.c[i] = rbj(k, f, sr, qq, 0.0);
            }
            s
        };
        let cut48 = |k: Rbj| cut(k, &BW8_Q);
        match b.kind {
            EqFilter::LowCut48 => cut48(Rbj::Hp),
            EqFilter::LowCut24 => cut(Rbj::Hp, &BW4_Q),
            EqFilter::LowCut12 => Sections::one(rbj(Rbj::Hp, f, sr, q, 0.0)),
            EqFilter::LowShelf => Sections::one(rbj(Rbj::LowShelf, f, sr, q, g)),
            EqFilter::Bell => Sections::one(rbj(Rbj::Peak, f, sr, q, g)),
            EqFilter::Notch => Sections::one(rbj(Rbj::Notch, f, sr, q, 0.0)),
            EqFilter::HighShelf => Sections::one(rbj(Rbj::HighShelf, f, sr, q, g)),
            EqFilter::HighCut12 => Sections::one(rbj(Rbj::Lp, f, sr, q, 0.0)),
            EqFilter::HighCut24 => cut(Rbj::Lp, &BW4_Q),
            EqFilter::HighCut48 => cut48(Rbj::Lp),
        }
    }

    /// Audition filter of a band: what the band acts on.
    pub fn solo_sections(&self, b: &EqBand, sr: f32) -> Sections {
        let (_, q) = self.effective(b);
        let (f, srd) = (b.freq.clamp(EQ_FREQ_MIN, EQ_FREQ_MAX) as f64, sr as f64);
        match b.kind {
            EqFilter::Bell | EqFilter::Notch => Sections::one(rbj(Rbj::Bp, f, srd, q as f64, 0.0)),
            EqFilter::LowShelf => Sections::one(rbj(Rbj::Lp, f, srd, std::f64::consts::FRAC_1_SQRT_2, 0.0)),
            EqFilter::HighShelf => Sections::one(rbj(Rbj::Hp, f, srd, std::f64::consts::FRAC_1_SQRT_2, 0.0)),
            _ => self.sections(b, sr),
        }
    }

    /// Summed response (dB) of channel set `ch` at `f` Hz (Scale included,
    /// Output not).
    pub fn response_db(&self, ch: usize, f: f32, sr: f32) -> f32 {
        let w = 2.0 * PI * (f as f64 / sr as f64).min(0.4999);
        self.bands[ch.min(1)].iter().filter(|b| b.on).map(|b| self.sections(b, sr).mag_db(w)).sum::<f64>() as f32
    }
}

// ----------------------------------------------------------------------------
// Spectrum tap (audio thread writes atomics, the GUI reads).
// ----------------------------------------------------------------------------

pub const TAP_LEN: usize = 1 << 15;

pub struct SpecTap {
    buf: Box<[AtomicU32]>,
    pos: AtomicUsize,
    sr: AtomicU32,
}

impl std::fmt::Debug for SpecTap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SpecTap")
    }
}

impl SpecTap {
    fn new() -> Self {
        Self { buf: (0..TAP_LEN).map(|_| AtomicU32::new(0)).collect(), pos: AtomicUsize::new(0), sr: AtomicU32::new(48_000) }
    }

    /// Total samples written so far.
    pub fn written(&self) -> usize {
        self.pos.load(Ordering::Acquire)
    }

    pub fn sample_rate(&self) -> f32 {
        self.sr.load(Ordering::Relaxed) as f32
    }

    /// The latest `out.len()` samples (≤ [`TAP_LEN`]).
    pub fn read(&self, out: &mut [f32]) {
        let n = out.len().min(TAP_LEN);
        let end = self.written();
        for (i, o) in out[..n].iter_mut().enumerate() {
            let idx = end.wrapping_sub(n).wrapping_add(i) & (TAP_LEN - 1);
            *o = f32::from_bits(self.buf[idx].load(Ordering::Relaxed));
        }
    }
}

static TAPS: Mutex<Vec<(u64, Weak<SpecTap>)>> = Mutex::new(Vec::new());

/// The spectrum tap of device `id` (shared by the engine and the GUI).
pub fn spectrum_tap(id: u64) -> Arc<SpecTap> {
    let mut taps = TAPS.lock().unwrap_or_else(|e| e.into_inner());
    taps.retain(|(_, w)| w.strong_count() > 0);
    if let Some(t) = taps.iter().find(|(i, _)| *i == id).and_then(|(_, w)| w.upgrade()) {
        return t;
    }
    let t = Arc::new(SpecTap::new());
    taps.push((id, Arc::downgrade(&t)));
    t
}

// ----------------------------------------------------------------------------
// DSP.
// ----------------------------------------------------------------------------

/// Transposed direct form II section (f64 state: stable at 10 Hz).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Tdf2 {
    s: [f64; 2],
}

impl Tdf2 {
    #[inline]
    pub(crate) fn tick(&mut self, c: &Coefs, x: f64) -> f64 {
        let y = c.b[0] * x + self.s[0];
        self.s[0] = c.b[1] * x - c.a[0] * y + self.s[1];
        self.s[1] = c.b[2] * x - c.a[1] * y;
        y
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct BandDsp {
    st: [Tdf2; 4],
    coefs: Sections,
    /// Smoothed (log2 freq, gain, log2 q).
    cur: [f32; 3],
    kind: Option<EqFilter>,
    on: bool,
}

/// 2x halfband FIR (windowed sinc) for oversampling.
pub(crate) const OS_TAPS: usize = 47;

#[derive(Debug, Clone)]
pub(crate) struct Fir {
    hist: [f64; 64],
    pos: usize,
}

impl Default for Fir {
    fn default() -> Self {
        Self { hist: [0.0; 64], pos: 0 }
    }
}

impl Fir {
    #[inline]
    pub(crate) fn push(&mut self, x: f64) {
        self.pos = (self.pos + 1) & 63;
        self.hist[self.pos] = x;
    }
    #[inline]
    pub(crate) fn out(&self, h: &[f64; OS_TAPS]) -> f64 {
        let mut acc = 0.0;
        for (k, hk) in h.iter().enumerate() {
            acc += hk * self.hist[(self.pos.wrapping_sub(k)) & 63];
        }
        acc
    }
}

pub(crate) fn halfband() -> [f64; OS_TAPS] {
    let m = (OS_TAPS - 1) as f64 / 2.0;
    let mut h = [0.0; OS_TAPS];
    for (i, v) in h.iter_mut().enumerate() {
        let x = i as f64 - m;
        let sinc = if x == 0.0 { 1.0 } else { (PI * x / 2.0).sin() / (PI * x / 2.0) };
        let w = 0.42
            + 0.5 * (2.0 * PI * i as f64 / (OS_TAPS - 1) as f64 - PI).cos()
            + 0.08 * (4.0 * PI * i as f64 / (OS_TAPS - 1) as f64 - 2.0 * PI).cos();
        *v = 0.5 * sinc * w;
    }
    let s: f64 = h.iter().sum();
    h.iter_mut().for_each(|v| *v /= s);
    h
}

#[derive(Debug, Clone)]
pub(crate) struct EqState {
    bands: [[BandDsp; 8]; 2],
    solo: [BandDsp; 2],
    up: [Fir; 2],
    down: [Fir; 2],
    hb: [f64; OS_TAPS],
    out_cur: f32,
    init: bool,
    tap: Option<Arc<SpecTap>>,
    /// Oversampling / mode last block (a change clears the filters).
    last: (bool, EqMode),
}

impl EqState {
    pub(crate) fn new(tap: Option<Arc<SpecTap>>) -> Self {
        Self {
            bands: [[BandDsp::default(); 8]; 2],
            solo: [BandDsp::default(); 2],
            up: Default::default(),
            down: Default::default(),
            hb: halfband(),
            out_cur: 1.0,
            init: false,
            tap,
            last: (false, EqMode::Stereo),
        }
    }

    pub(crate) fn reset(&mut self) {
        let tap = self.tap.take();
        let hb = self.hb;
        *self = Self { hb, ..Self::new(tap) };
    }

    /// Smooth a band toward its target and refresh its coefficients.
    fn update_band(d: &mut BandDsp, p: &EqEightParams, b: &EqBand, sr: f32, k: f32, solo: bool) {
        let target = [b.freq.clamp(EQ_FREQ_MIN, EQ_FREQ_MAX).log2(), b.gain, b.q.clamp(EQ_Q_MIN, EQ_Q_MAX).log2()];
        if d.kind != Some(b.kind) || !d.on {
            if d.kind.is_some_and(|k0| k0.sections() != b.kind.sections()) {
                d.st = Default::default();
            }
            d.kind = Some(b.kind);
            d.cur = target;
            d.on = true;
            d.coefs = Sections::default();
        }
        let mut moved = false;
        for (c, t) in d.cur.iter_mut().zip(target) {
            let nv = *c + (t - *c) * k;
            let nv = if (t - nv).abs() < 1e-4 { t } else { nv };
            if nv != *c {
                moved = true;
            }
            *c = nv;
        }
        if moved || d.coefs.n == 0 {
            let sb = EqBand { freq: d.cur[0].exp2(), gain: d.cur[1], q: d.cur[2].exp2(), ..*b };
            d.coefs = if solo { p.solo_sections(&sb, sr) } else { p.sections(&sb, sr) };
        }
    }

    pub(crate) fn process(&mut self, p: &EqEightParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let os = p.oversample;
        if self.last != (os, p.mode) {
            let tap = self.tap.take();
            let hb = self.hb;
            *self = Self { hb, ..Self::new(tap) };
            self.last = (os, p.mode);
        }
        let fs = if os { sr * 2.0 } else { sr };
        let n = l.len().min(r.len());
        if p.mode == EqMode::MS {
            for i in 0..n {
                let (a, b) = (l[i], r[i]);
                l[i] = 0.5 * (a + b);
                r[i] = 0.5 * (a - b);
            }
        }
        // Parameter smoothing per 32-sample sub-block (≈ 20 ms).
        const SUB: usize = 32;
        let k = 1.0 - (-(SUB as f32) / (0.02 * sr)).exp();
        let solo = p.solo.filter(|_| p.audition).map(|b| b.min(7) as usize);
        let edit = p.edit_ch();
        let mut i0 = 0;
        while i0 < n {
            let m = SUB.min(n - i0);
            for c in 0..2 {
                let set = if p.mode == EqMode::Stereo { 0 } else { c };
                let x = if c == 0 { &mut l[i0..i0 + m] } else { &mut r[i0..i0 + m] };
                if let Some(sb) = solo {
                    if p.mode != EqMode::Stereo && c != edit {
                        x.fill(0.0);
                        continue;
                    }
                    let band = p.bands[set][sb];
                    Self::update_band(&mut self.solo[c], p, &band, fs, k, true);
                } else {
                    for (bi, d) in self.bands[c].iter_mut().enumerate() {
                        let band = p.bands[set][bi];
                        if band.on {
                            Self::update_band(d, p, &band, fs, k, false);
                        } else if d.on {
                            *d = BandDsp::default();
                        }
                    }
                }
                let bands = &mut self.bands[c];
                let sd = &mut self.solo[c];
                let mut run = |v: f64| -> f64 {
                    let mut v = v;
                    if solo.is_some() {
                        for (s, cf) in sd.st.iter_mut().zip(&sd.coefs.c[..sd.coefs.n]) {
                            v = s.tick(cf, v);
                        }
                    } else {
                        for d in bands.iter_mut().filter(|d| d.on) {
                            for (s, cf) in d.st.iter_mut().zip(&d.coefs.c[..d.coefs.n]) {
                                v = s.tick(cf, v);
                            }
                        }
                    }
                    if v.abs() < 1e-30 { 0.0 } else { v }
                };
                if os {
                    let (up, down, hb) = (&mut self.up[c], &mut self.down[c], &self.hb);
                    for s in x.iter_mut() {
                        // Zero-stuffed input (×2 gain), filtered at 2x.
                        up.push(2.0 * *s as f64);
                        let a = run(up.out(hb));
                        up.push(0.0);
                        let b = run(up.out(hb));
                        down.push(a);
                        down.push(b);
                        *s = down.out(hb) as f32;
                    }
                } else {
                    for s in x.iter_mut() {
                        *s = run(*s as f64) as f32;
                    }
                }
            }
            i0 += m;
        }
        if p.mode == EqMode::MS {
            for i in 0..n {
                let (m, s) = (l[i], r[i]);
                l[i] = m + s;
                r[i] = m - s;
            }
        }
        // Output gain (linear ramp per block).
        let g = 10f32.powf(p.output.clamp(-EQ_OUT_MAX, EQ_OUT_MAX) / 20.0);
        if !self.init {
            self.out_cur = g;
            self.init = true;
        }
        let (g0, step) = (self.out_cur, (g - self.out_cur) / n.max(1) as f32);
        for i in 0..n {
            let gi = g0 + step * (i + 1) as f32;
            l[i] *= gi;
            r[i] *= gi;
        }
        self.out_cur = g;
        if p.analyze
            && let Some(tap) = &self.tap
        {
            tap.sr.store(sr as u32, Ordering::Relaxed);
            let start = tap.pos.load(Ordering::Relaxed);
            for i in 0..n {
                let v = match (p.mode, p.edit_b) {
                    (EqMode::Stereo, _) => 0.5 * (l[i] + r[i]),
                    (EqMode::LR, false) => l[i],
                    (EqMode::LR, true) => r[i],
                    (EqMode::MS, false) => 0.5 * (l[i] + r[i]),
                    (EqMode::MS, true) => 0.5 * (l[i] - r[i]),
                };
                tap.buf[(start + i) & (TAP_LEN - 1)].store(v.to_bits(), Ordering::Relaxed);
            }
            tap.pos.store(start.wrapping_add(n), Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_gain(p: &EqEightParams, f: f32, sr: f32) -> f32 {
        let mut st = EqState::new(None);
        let n = 16384;
        let mut l: Vec<f32> = (0..n).map(|i| (2.0 * std::f32::consts::PI * f * i as f32 / sr).sin()).collect();
        let mut r = l.clone();
        st.process(p, &mut l, &mut r, sr);
        let tail = &l[n / 2..];
        let rms = (tail.iter().map(|x| x * x).sum::<f32>() / tail.len() as f32).sqrt();
        20.0 * (rms * std::f32::consts::SQRT_2).log10()
    }

    #[test]
    fn default_is_transparent() {
        let p = EqEightParams::default();
        for f in [50.0, 1000.0, 8000.0] {
            assert!(sine_gain(&p, f, 48_000.0).abs() < 0.05, "{f}");
            assert!(p.response_db(0, f, 48_000.0).abs() < 0.05);
        }
    }

    #[test]
    fn bell_boost_matches_curve() {
        let mut p = EqEightParams { adaptive_q: false, ..Default::default() };
        p.bands[0][2].gain = 9.0;
        let g = sine_gain(&p, 1000.0, 48_000.0);
        assert!((g - 9.0).abs() < 0.2, "{g}");
        assert!((p.response_db(0, 1000.0, 48_000.0) - 9.0).abs() < 0.05);
        // Scale 50 % halves it.
        p.scale = 0.5;
        assert!((sine_gain(&p, 1000.0, 48_000.0) - 4.5).abs() < 0.2);
    }

    #[test]
    fn cuts_and_oversampling() {
        let mut p = EqEightParams::default();
        p.bands[0][0] = EqBand { on: true, kind: EqFilter::LowCut48, freq: 1000.0, gain: 0.0, q: 0.71 };
        assert!(sine_gain(&p, 125.0, 48_000.0) < -60.0);
        p.oversample = true;
        assert!(sine_gain(&p, 125.0, 48_000.0) < -60.0);
        assert!(sine_gain(&p, 8000.0, 48_000.0).abs() < 0.3);
    }

    #[test]
    fn ms_edits_side_only() {
        let mut p = EqEightParams { mode: EqMode::MS, ..Default::default() };
        p.bands[1][0] = EqBand { on: true, kind: EqFilter::LowCut48, freq: 2000.0, gain: 0.0, q: 0.71 };
        // Mono (mid-only) signal passes, the side is cut at low frequencies.
        let mut st = EqState::new(None);
        let n = 8192;
        let mut l: Vec<f32> = (0..n).map(|i| (i as f32 * 0.01).sin()).collect();
        let mut r = l.clone();
        st.process(&p, &mut l, &mut r, 48_000.0);
        assert!((l[n - 1] - r[n - 1]).abs() < 1e-3);
        let mut l: Vec<f32> = (0..n).map(|i| (i as f32 * 0.01).sin()).collect();
        let mut r: Vec<f32> = l.iter().map(|x| -x).collect();
        st.reset();
        st.process(&p, &mut l, &mut r, 48_000.0);
        assert!(l[n / 2..].iter().all(|x| x.abs() < 0.01));
    }

    #[test]
    fn tap_records_output() {
        let tap = spectrum_tap(7);
        let mut st = EqState::new(Some(tap.clone()));
        let p = EqEightParams::default();
        let mut l = vec![0.5f32; 256];
        let mut r = vec![0.5f32; 256];
        st.process(&p, &mut l, &mut r, 48_000.0);
        let mut out = [0.0f32; 4];
        tap.read(&mut out);
        assert!(out.iter().all(|x| (x - 0.5).abs() < 1e-3));
        assert!(Arc::ptr_eq(&tap, &spectrum_tap(7)));
    }
}
