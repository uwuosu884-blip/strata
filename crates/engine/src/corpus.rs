//! Corpus (Live 12, manual 29.10): two physically modelled resonators (one
//! per stereo side). Beam, Marimba, String, Membrane and Plate are modal
//! banks (mode ratios of the object, damping varying with Material, Bright
//! tilt, Inharm stretch, Hit / Pos. L / Pos. R excitation and pick-up
//! weights, Ratio for the 2-D objects); Pipe and Tube are waveguides
//! (Opening blends a closed and an open end, Radius sets loss and end
//! correction). Spread detunes the sides, an LFO (7 shapes, Phase / Spin)
//! modulates the tuning, a band-pass Filter, Bleed, Gain with a limiter
//! (LED), Dry/Wet feeding the resonators, and MIDI Frequency (Last / Low,
//! Transpose, Fine, PB) / Off Decay from a MIDI track.

use crate::autoshift::AsNote;
use crate::devtap::DevTap;
use crate::dsp::{Rng, Svf};
use crate::dynfx::Sidechain;
use crate::modfx::SYNC_DIVS;
use serde::{Deserialize, Serialize};
use std::f32::consts::{PI, TAU};
use std::sync::Arc;

pub const CO_TYPES: [&str; 7] = ["Beam", "Marimba", "String", "Membrane", "Plate", "Pipe", "Tube"];
pub const CO_QUALITY: [&str; 4] = ["Eco", "Low", "Med", "High"];
const MODES_Q: [usize; 4] = [8, 16, 32, 48];
pub const CO_LFO_WAVES: [&str; 7] = ["Sine", "Square", "Triangle", "Saw Up", "Saw Down", "Noise Stepped", "Noise Smooth"];
pub const CO_TUNE: (f32, f32) = (20.0, 5000.0);
pub const CO_DECAY: (f32, f32) = (0.01, 20.0);
pub const CO_TRANSPOSE: f32 = 48.0;
pub const CO_RATE: (f32, f32) = (0.01, 40.0);
pub const CO_FREQ: (f32, f32) = (50.0, 18_000.0);
pub const CO_BW: (f32, f32) = (0.1, 8.0);
pub const CO_GAIN: (f32, f32) = (-24.0, 24.0);
/// LFO depth at 100 % Amount (semitones).
pub const CO_LFO_ST: f32 = 12.0;
/// Spread range at ±100 % (cents).
pub const CO_SPREAD_CT: f32 = 50.0;
/// Tap slots: limiter active, LFO phase, sounding frequency (Hz).
pub const TAP_LIMIT: usize = 0;
pub const TAP_LFO: usize = 1;
pub const TAP_FREQ: usize = 2;
const MAXM: usize = 48;
const WG: usize = 1 << 13;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CorpusParams {
    /// Index into [`CO_TYPES`].
    pub kind: u8,
    /// Index into [`CO_QUALITY`].
    pub quality: u8,
    /// Seconds.
    pub decay: f32,
    /// −1..1.
    pub material: f32,
    /// 0..1 (Pipe / Tube).
    pub radius: f32,
    /// −1..1.
    pub bright: f32,
    /// −1..1.
    pub inharm: f32,
    /// 0..1 (Pipe).
    pub opening: f32,
    /// 0..1 (Membrane / Plate).
    pub ratio: f32,
    /// 0..1.
    pub hit: f32,
    pub width: f32,
    pub pos_l: f32,
    pub pos_r: f32,
    /// Hz.
    pub tune: f32,
    /// Semitones (MIDI Frequency on).
    pub transpose: f32,
    /// Cents (MIDI Frequency on).
    pub fine: f32,
    /// −1..1.
    pub spread: f32,
    pub lfo_on: bool,
    pub lfo_amt: f32,
    /// Index into [`CO_LFO_WAVES`].
    pub lfo_wave: u8,
    pub lfo_sync: bool,
    pub rate: f32,
    pub div: u8,
    /// Spin (else Phase) stereo mode.
    pub spin_mode: bool,
    pub phase: f32,
    pub spin: f32,
    pub offset: f32,
    pub filter_on: bool,
    pub freq: f32,
    /// Octaves.
    pub bw: f32,
    pub bleed: f32,
    /// dB.
    pub gain: f32,
    pub dry_wet: f32,
    /// MIDI sidechain.
    pub midi: Sidechain,
    pub midi_freq: bool,
    /// Lowest (else last) note priority.
    pub low: bool,
    pub pb: f32,
    pub off_decay: bool,
    pub off_amt: f32,
    /// MIDI Input column unfolded.
    pub open: bool,
}

impl Default for CorpusParams {
    fn default() -> Self {
        Self {
            kind: 0,
            quality: 2,
            decay: 1.26,
            material: 0.0,
            radius: 0.5,
            bright: 0.0,
            inharm: 0.0,
            opening: 1.0,
            ratio: 0.5,
            hit: 0.5,
            width: 1.0,
            pos_l: 0.1,
            pos_r: 0.9,
            tune: 261.63,
            transpose: 0.0,
            fine: 0.0,
            spread: 0.0,
            lfo_on: false,
            lfo_amt: 0.0,
            lfo_wave: 0,
            lfo_sync: false,
            rate: 0.32,
            div: 4,
            spin_mode: false,
            phase: 0.5,
            spin: 0.0,
            offset: 0.0,
            filter_on: false,
            freq: 1000.0,
            bw: 4.0,
            bleed: 0.0,
            gain: 0.0,
            dry_wet: 0.5,
            midi: Sidechain::default(),
            midi_freq: false,
            low: false,
            pb: 2.0,
            off_decay: false,
            off_amt: 1.0,
            open: false,
        }
    }
}

impl CorpusParams {
    pub fn is_tube(&self) -> bool {
        self.kind >= 5
    }

    pub fn has_ratio(&self) -> bool {
        self.kind == 3 || self.kind == 4
    }

    pub fn midi_active(&self) -> bool {
        self.midi_freq || self.off_decay
    }
}

/// Corpus LFO shapes (−1..1); `rnd` = (held, next) values of the noises.
pub fn co_wave(w: u8, ph: f32, rnd: (f32, f32)) -> f32 {
    let ph = ph.rem_euclid(1.0);
    match w {
        0 => (TAU * ph).sin(),
        1 => {
            if ph < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        2 => 1.0 - 4.0 * (ph - 0.5).abs(),
        3 => 2.0 * ph - 1.0,
        4 => 1.0 - 2.0 * ph,
        5 => rnd.0,
        _ => rnd.0 + (rnd.1 - rnd.0) * (0.5 - 0.5 * (PI * ph).cos()),
    }
}

/// Mode `k` of object `kind`: (frequency ratio, excitation / pick-up weight
/// at normalised position `x`, 0 = centre .. 1 = edge).
fn mode(kind: u8, k: usize, ratio: f32, x: f32) -> (f32, f32) {
    // Positions measured from the edge of the object (0.5 = centre).
    let u = 0.5 - 0.47 * x.clamp(0.0, 1.0);
    match kind {
        0 | 1 => {
            // Free-free beam: (β_n / β_1)², β_n ≈ (2n+3)π/2 for n ≥ 2.
            const B: [f32; 4] = [4.730_04, 7.853_2, 10.995_6, 14.137_2];
            let b = if k < 4 { B[k] } else { (2 * k + 3) as f32 * PI * 0.5 };
            let mut r = (b / B[0]).powi(2);
            if kind == 1 {
                // Arch-cut marimba bar: overtones pulled to 4:1, 10:1, …
                const M: [f32; 4] = [1.0, 3.99, 9.95, 17.6];
                r = if k < 4 { M[k] } else { r * 0.62 };
            }
            let w = (b * u).cos() + (b * u).cosh() * 0.02;
            (r, w.abs().min(1.5))
        }
        2 => {
            let n = (k + 1) as f32;
            (n, (PI * n * u).sin().abs())
        }
        _ => {
            // Rectangular membrane / plate: (m, n) pairs in order.
            let side = 7;
            let (m, n) = ((k % side + 1) as f32, (k / side + 1) as f32);
            let ry = 0.5 + 1.5 * ratio.clamp(0.0, 1.0);
            let w = (PI * m * u).sin() * (PI * n * (1.0 - u) * 0.9).sin();
            if kind == 3 {
                ((m * m + (n / ry).powi(2)).sqrt() / (1.0 + 1.0 / (ry * ry)).sqrt(), w.abs())
            } else {
                ((m * m + (n / ry).powi(2)) / (1.0 + 1.0 / (ry * ry)), w.abs())
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Bank {
    c1: [f32; MAXM],
    c2: [f32; MAXM],
    g: [f32; MAXM],
    y1: [f32; MAXM],
    y2: [f32; MAXM],
    n: usize,
}

impl Default for Bank {
    fn default() -> Self {
        let z = [0.0; MAXM];
        Self { c1: z, c2: z, g: z, y1: z, y2: z, n: 0 }
    }
}

#[derive(Debug, Clone)]
pub struct CorpusState {
    tap: Option<Arc<DevTap>>,
    bank: [Bank; 2],
    wg: [[Box<[f32]>; 2]; 2],
    wl: [[f32; 2]; 2],
    /// Tube output DC blocker (x1, y1).
    dc: [(f32, f32); 2],
    w: usize,
    lfo: [f32; 2],
    rnd: [(f32, f32); 2],
    rng: Rng,
    bp: [Svf; 2],
    lim: f32,
    notes: [AsNote; 32],
    n_notes: usize,
    held: [(u8, u32, u32); 16],
    order: u32,
    note: f32,
    damp: f32,
}

impl Default for CorpusState {
    fn default() -> Self {
        Self::new(None)
    }
}

impl CorpusState {
    pub fn new(id: Option<u64>) -> Self {
        let z = || vec![0.0f32; WG].into_boxed_slice();
        Self::build(id.map(crate::devtap::dev_tap), [[z(), z()], [z(), z()]])
    }

    fn build(tap: Option<Arc<DevTap>>, wg: [[Box<[f32]>; 2]; 2]) -> Self {
        Self {
            tap,
            bank: [Bank::default(); 2],
            wg,
            wl: [[0.0; 2]; 2],
            dc: [(0.0, 0.0); 2],
            w: 0,
            lfo: [0.0; 2],
            rnd: [(0.0, 0.0); 2],
            rng: Rng(0xc0_4905),
            bp: [Svf::default(); 2],
            lim: 1.0,
            notes: [AsNote::default(); 32],
            n_notes: 0,
            held: [(0, 0, 0); 16],
            order: 0,
            note: 60.0,
            damp: 1.0,
        }
    }

    pub fn set_id(&mut self, id: u64) {
        self.tap = Some(crate::devtap::dev_tap(id));
    }

    pub fn reset(&mut self) {
        let mut wg = std::mem::take(&mut self.wg);
        for b in wg.iter_mut().flatten() {
            b.fill(0.0);
        }
        *self = Self::build(self.tap.take(), wg);
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

    /// Held notes of the MIDI source → note (Last / Low) and Note Off damping.
    fn midi_block(&mut self, p: &CorpusParams, n: usize) {
        for h in self.held.iter_mut() {
            if h.1 != u32::MAX {
                h.1 = h.1.saturating_sub(n as u32);
            }
        }
        let mut live = [false; 16];
        for k in 0..self.n_notes {
            let e = self.notes[k];
            if e.vel == 0 {
                continue;
            }
            let len = if e.len == u32::MAX { u32::MAX } else { e.len.saturating_add(e.offset) };
            self.order += 1;
            let slot = self.held.iter().position(|h| h.0 == e.pitch && h.1 > 0).or_else(|| self.held.iter().position(|h| h.1 == 0));
            if let Some(i) = slot {
                self.held[i] = (e.pitch, len, self.order);
                live[i] = e.len == u32::MAX;
            }
        }
        for (i, h) in self.held.iter_mut().enumerate() {
            if h.1 == u32::MAX && !live[i] && self.n_notes > 0 {
                h.1 = 0;
            }
        }
        self.n_notes = 0;
        let on = self.held.iter().filter(|h| h.1 > 0);
        let pick = if p.low { on.min_by_key(|h| h.0) } else { on.max_by_key(|h| h.2) };
        match pick {
            Some(h) => {
                self.note = h.0 as f32;
                self.damp = 1.0;
            }
            None if p.off_decay => self.damp = 1.0 - p.off_amt.clamp(0.0, 1.0),
            None => self.damp = 1.0,
        }
    }

    /// Base frequency (Hz) of the resonance.
    pub fn base_hz(p: &CorpusParams, note: f32) -> f32 {
        if p.midi_freq { crate::specres::midi_hz(note + p.transpose + p.fine / 100.0) } else { p.tune.clamp(CO_TUNE.0, CO_TUNE.1) }
    }

    fn design(&mut self, p: &CorpusParams, ch: usize, f0: f32, sr: f32) {
        let nmax = MODES_Q[(p.quality as usize).min(3)];
        let t60 = p.decay.clamp(CO_DECAY.0, CO_DECAY.1) * self.damp.max(0.002);
        let pos = if ch == 0 { p.pos_l } else { p.pos_r };
        let b = &mut self.bank[ch];
        let mut k = 0;
        let mut m = 0;
        while k < nmax && m < 120 {
            let (mut r, we) = mode(p.kind, m, p.ratio, p.hit);
            let (_, wp) = mode(p.kind, m, p.ratio, pos);
            m += 1;
            if m > 1 {
                r = r.powf(1.0 + 0.25 * p.inharm.clamp(-1.0, 1.0));
            }
            let f = f0 * r;
            if f > 0.45 * sr {
                break;
            }
            let w = we * wp;
            if w < 1e-3 && m > 1 {
                continue;
            }
            // Material: low values damp the highs (wood), high values the lows.
            let t = (t60 * r.powf(0.9 * p.material.clamp(-1.0, 1.0) - 0.35)).clamp(0.003, 40.0);
            let rad = 10f32.powf(-3.0 / (t * sr));
            let th = TAU * f / sr;
            b.c1[k] = 2.0 * rad * th.cos();
            b.c2[k] = -rad * rad;
            let tilt = r.powf(0.6 * p.bright.clamp(-1.0, 1.0) - 0.3);
            b.g[k] = w * tilt * (1.0 - rad) * 2.0;
            k += 1;
        }
        for j in k..b.n {
            b.y1[j] = 0.0;
            b.y2[j] = 0.0;
        }
        b.n = k;
    }

    #[allow(clippy::needless_range_loop)]
    pub fn process(&mut self, p: &CorpusParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let n = l.len().min(r.len());
        if p.midi_active() {
            self.midi_block(p, n);
        } else {
            self.damp = 1.0;
        }
        let (tempo, _) = crate::modfx::clock();
        let beat_hz = tempo.max(1.0) as f32 / 60.0;
        let rate = if p.lfo_sync {
            beat_hz / SYNC_DIVS[(p.div as usize).min(SYNC_DIVS.len() - 1)].0 as f32
        } else {
            p.rate.clamp(CO_RATE.0, CO_RATE.1)
        };
        let spin = !p.lfo_sync && p.spin_mode;
        let rates = if spin { [rate, rate * (1.0 + p.spin.clamp(0.0, 1.0))] } else { [rate, rate] };
        let ph_off = if spin { 0.0 } else { p.phase.clamp(0.0, 1.0) };
        let off = if p.lfo_sync { p.offset.clamp(0.0, 1.0) } else { 0.0 };
        let base = Self::base_hz(p, self.note);
        let sp = p.spread.clamp(-1.0, 1.0) * CO_SPREAD_CT / 1200.0;
        let lfo_st = if p.lfo_on { p.lfo_amt.clamp(0.0, 1.0) * CO_LFO_ST } else { 0.0 };
        let tube = p.is_tube();
        let wet = p.dry_wet.clamp(0.0, 1.0);
        let width = p.width.clamp(0.0, 1.0);
        let bleed = if tube { 0.0 } else { p.bleed.clamp(0.0, 1.0) };
        let g = 10f32.powf(p.gain.clamp(CO_GAIN.0, CO_GAIN.1) / 20.0);
        let fg = crate::dsp::svf_g(p.freq.clamp(CO_FREQ.0, CO_FREQ.1), sr);
        // Bandwidth in octaves → damping k = 1/Q.
        let bw = p.bw.clamp(CO_BW.0, CO_BW.1);
        let q = 2f32.powf(bw * 0.5) / (2f32.powf(bw) - 1.0);
        let fk = 1.0 / q.max(0.05);
        let rel = (-1.0 / (0.2 * sr)).exp();
        // Tube: radius → loss low-pass and end correction.
        let rad = p.radius.clamp(0.0, 1.0);
        let lp = 0.15 + 0.8 * rad;
        let t60 = p.decay.clamp(CO_DECAY.0, CO_DECAY.1) * self.damp.max(0.002) * (0.5 + rad);
        let open = p.opening.clamp(0.0, 1.0);
        let mut f_side = [base; 2];
        let mut limited = false;
        for i in 0..n {
            for ch in 0..2 {
                let prev = self.lfo[ch];
                self.lfo[ch] = (self.lfo[ch] + rates[ch] / sr).rem_euclid(1.0);
                if self.lfo[ch] < prev {
                    self.rnd[ch] = (self.rnd[ch].1, self.rng.bi());
                }
            }
            if i & 31 == 0 {
                for ch in 0..2 {
                    let ph = self.lfo[ch] + off + if ch == 1 { ph_off } else { 0.0 };
                    let v = co_wave(p.lfo_wave, ph, if p.lfo_wave >= 5 { self.rnd[0] } else { self.rnd[ch] });
                    let s = if ch == 0 { sp } else { -sp };
                    f_side[ch] = base * 2f32.powf(s + v * lfo_st / 12.0);
                    if !tube {
                        self.design(p, ch, f_side[ch], sr);
                    }
                }
            }
            let dry = [l[i], r[i]];
            let mut res = [0.0f32; 2];
            for ch in 0..2 {
                let x = dry[ch] * wet;
                if tube {
                    let f = f_side[ch] / (1.0 + 0.35 * rad * rad);
                    let gl = 10f32.powf(-3.0 / (t60 * f));
                    let mut acc = 0.0;
                    // [0]: closed end (odd harmonics), [1]: open (all, octave up for Pipe).
                    for (e, len, sign) in [(0usize, sr / (4.0 * f), -1.0f32), (1, sr / (2.0 * f), 1.0)] {
                        let d = crate::dsp::read(&self.wg[ch][e], self.w, (2.0 * len).clamp(2.0, (WG - 8) as f32), true);
                        self.wl[ch][e] += lp * (d - self.wl[ch][e]);
                        let v = x + sign * gl * self.wl[ch][e];
                        self.wg[ch][e][self.w] = if v.is_finite() { v.clamp(-8.0, 8.0) } else { 0.0 };
                        let mix = match (p.kind, e) {
                            (6, 0) => 0.0,
                            (6, _) => 1.0,
                            (_, 0) => 1.0 - open,
                            _ => open,
                        };
                        acc += d * mix;
                    }
                    let v = acc * 0.5 * (1.0 - lp * 0.5);
                    // Radiation: remove the pulse train's DC.
                    let (x1, y1) = self.dc[ch];
                    let o = v - x1 + 0.995 * y1;
                    self.dc[ch] = (v, if o.is_finite() { o } else { 0.0 });
                    res[ch] = self.dc[ch].1 * 2.0;
                } else {
                    let b = &mut self.bank[ch];
                    let mut acc = 0.0;
                    for k in 0..b.n {
                        let y = b.g[k] * x + b.c1[k] * b.y1[k] + b.c2[k] * b.y2[k];
                        b.y2[k] = b.y1[k];
                        b.y1[k] = y;
                        acc += y;
                    }
                    if !acc.is_finite() {
                        *b = Bank::default();
                        acc = 0.0;
                    }
                    res[ch] = acc;
                }
            }
            // Width: 0 = both resonators on both sides.
            let (a, c) = (0.5 + 0.5 * width, 0.5 - 0.5 * width);
            let mut out = [a * res[0] + c * res[1], c * res[0] + a * res[1]];
            for ch in 0..2 {
                let mut y = out[ch];
                if p.filter_on {
                    y = self.bp[ch].tick(y, fg, fk).1 * fk;
                }
                y = (y + bleed * dry[ch] * wet) * g;
                out[ch] = y;
            }
            // Limiter: instant attack, 200 ms release.
            let pk = out[0].abs().max(out[1].abs());
            let need = if pk > 1.0 { 1.0 / pk } else { 1.0 };
            self.lim = if need < self.lim { need } else { 1.0 - rel * (1.0 - self.lim) }.min(1.0);
            if self.lim < 0.999 {
                limited = true;
            }
            for ch in 0..2 {
                out[ch] = (dry[ch] * (1.0 - wet) + out[ch] * self.lim).clamp(-8.0, 8.0);
            }
            self.w = (self.w + 1) & (WG - 1);
            l[i] = out[0];
            r[i] = out[1];
        }
        if let Some(t) = &self.tap {
            t.set(TAP_LIMIT, if limited { 1.0 } else { 0.0 });
            t.set(TAP_LFO, self.lfo[0]);
            t.set(TAP_FREQ, base);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn impulse(p: &CorpusParams) -> Vec<f32> {
        let mut s = CorpusState::new(None);
        let mut l = vec![0.0f32; 48_000];
        l[0] = 1.0;
        let mut r = l.clone();
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            s.process(p, a, b, 48_000.0);
        }
        l
    }

    fn freq_of(x: &[f32]) -> f32 {
        let zc = x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        zc as f32 * 48_000.0 / x.len() as f32
    }

    #[test]
    fn rings_at_tune() {
        // String, low brightness: the fundamental dominates.
        let p = CorpusParams { kind: 2, tune: 220.0, bright: -1.0, dry_wet: 1.0, hit: 0.0, pos_l: 0.0, pos_r: 0.0, ..Default::default() };
        let y = impulse(&p);
        let f = freq_of(&y[2000..20_000]);
        assert!((f - 220.0).abs() < 15.0, "{f}");
        // Tube rings at Tune too.
        let p = CorpusParams { kind: 6, tune: 220.0, radius: 0.0, dry_wet: 1.0, ..Default::default() };
        let y = impulse(&p);
        let f = freq_of(&y[2000..20_000]);
        assert!((f - 220.0).abs() < 15.0, "tube {f}");
        for kind in 0..7 {
            for q in 0..4 {
                let p = CorpusParams {
                    kind,
                    quality: q,
                    lfo_on: true,
                    lfo_amt: 1.0,
                    lfo_wave: kind % 7,
                    filter_on: true,
                    gain: 24.0,
                    ..Default::default()
                };
                let y = impulse(&p);
                assert!(y.iter().all(|v| v.is_finite() && v.abs() <= 8.0), "{kind}");
            }
        }
    }

    #[test]
    fn midi_frequency() {
        let p =
            CorpusParams { kind: 2, midi_freq: true, bright: -1.0, dry_wet: 1.0, hit: 0.0, pos_l: 0.0, pos_r: 0.0, ..Default::default() };
        let mut s = CorpusState::new(None);
        s.set_notes(&mut [AsNote { offset: 0, pitch: 57, vel: 100, len: 48_000 }].into_iter());
        let mut l = vec![0.0f32; 24_000];
        l[0] = 1.0;
        let mut r = l.clone();
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            s.process(&p, a, b, 48_000.0);
        }
        let f = freq_of(&l[2000..20_000]);
        assert!((f - 220.0).abs() < 15.0, "{f}");
    }
}
