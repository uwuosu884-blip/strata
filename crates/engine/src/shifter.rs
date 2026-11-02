//! Shifter (Live 12, manual 29.35): Pitch (windowed two-tap delay-line
//! shifter), Freq (single-sideband shift via a Hilbert pair) and Ring
//! (ring modulation, optional Drive) modes; Coarse / Fine, Wide + Spread,
//! Window, a feedback Delay (ms or synced) with Tone in its feedback path,
//! an LFO (10 waveforms, Duty Cycle, Phase / Spin / Width, Offset) and an
//! envelope follower modulating the shift, Output and Dry/Wet. The pitch or
//! frequency can follow an external MIDI track (Glide, PB).

use crate::autoshift::AsNote;
use crate::devtap::DevTap;
use crate::dsp::Rng;
use crate::dynfx::Sidechain;
use crate::modfx::SYNC_DIVS;
use serde::{Deserialize, Serialize};
use std::f32::consts::{PI, TAU};
use std::sync::Arc;

pub const SH_COARSE: f32 = 48.0;
/// Freq / Ring coarse (Hz, cubic around 0).
pub const SH_HZ: f32 = 5000.0;
/// Fine / Spread: cents (Pitch) or Hz (Freq / Ring).
pub const SH_FINE: f32 = 50.0;
pub const SH_WINDOW: (f32, f32) = (10.0, 300.0);
pub const SH_TONE: (f32, f32) = (50.0, 22_000.0);
pub const SH_DELAY_MS: (f32, f32) = (1.0, 5000.0);
pub const SH_RATE: (f32, f32) = (0.01, 40.0);
/// LFO / envelope amount ranges: semitones (Pitch) / Hz (Freq, Ring).
pub const SH_AMT_ST: f32 = 24.0;
pub const SH_AMT_HZ: f32 = 2000.0;
pub const SH_ENV_T: (f32, f32) = (0.1, 3000.0);
pub const SH_GLIDE: (f32, f32) = (0.0, 5000.0);
pub const SH_OUT: (f32, f32) = (-24.0, 24.0);
pub const SH_MODES: [&str; 3] = ["Pitch", "Freq", "Ring"];
pub const SH_WAVES: [&str; 10] = ["Sine", "Tri", "Tri Analog", "Tri 8", "Tri 16", "Saw Up", "Saw Down", "Rect", "Random", "Random S&H"];
/// Tap slots: LFO value L / R (display), envelope follower.
pub const TAP_LFO: [usize; 2] = [0, 1];
pub const TAP_ENV: usize = 2;
const DLEN: usize = 1 << 15;
const FBLEN: usize = 1 << 18;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShifterParams {
    /// Index into [`SH_MODES`].
    pub mode: u8,
    /// Semitones (Pitch).
    pub coarse: f32,
    /// Hz (Freq / Ring).
    pub coarse_hz: f32,
    pub fine: f32,
    pub spread: f32,
    pub wide: bool,
    /// ms.
    pub window: f32,
    /// Hz (delay feedback low-pass).
    pub tone: f32,
    pub delay_on: bool,
    pub delay_sync: bool,
    pub delay_div: u8,
    pub delay_ms: f32,
    pub feedback: f32,
    /// Index into [`SH_WAVES`].
    pub wave: u8,
    pub lfo_sync: bool,
    pub rate: f32,
    pub lfo_div: u8,
    /// 0..1 of [`SH_AMT_ST`] / [`SH_AMT_HZ`].
    pub lfo_amt: f32,
    /// −1..1.
    pub duty: f32,
    /// Spin (else Phase) for periodic waves.
    pub spin_mode: bool,
    /// Phase 0..1 (×360°), Spin 0..1, Width 0..1 (random waves).
    pub phase: f32,
    /// Synced start offset 0..1 (×360°).
    pub offset: f32,
    pub env_on: bool,
    /// −1..1 of the amount range.
    pub env_amt: f32,
    pub env_attack: f32,
    pub env_release: f32,
    /// Drive (Ring only).
    pub drive_on: bool,
    /// dB: drive level (Ring + Drive) / output gain.
    pub output: f32,
    pub dry_wet: f32,
    /// MIDI pitch source (else Internal).
    pub midi_mode: bool,
    pub midi: Sidechain,
    pub glide: f32,
    pub pb: f32,
    /// Sidechain column unfolded.
    pub midi_open: bool,
}

impl Default for ShifterParams {
    fn default() -> Self {
        Self {
            mode: 0,
            coarse: 0.0,
            coarse_hz: 0.0,
            fine: 0.0,
            spread: 0.0,
            wide: false,
            window: 80.0,
            tone: 22_000.0,
            delay_on: false,
            delay_sync: true,
            delay_div: 4,
            delay_ms: 100.0,
            feedback: 0.0,
            wave: 1,
            lfo_sync: false,
            rate: 0.5,
            lfo_div: 4,
            lfo_amt: 0.0,
            duty: 0.0,
            spin_mode: true,
            phase: 0.0,
            offset: 0.0,
            env_on: false,
            env_amt: 0.0,
            env_attack: 6.0,
            env_release: 200.0,
            drive_on: false,
            output: 0.0,
            dry_wet: 1.0,
            midi_mode: false,
            midi: Sidechain::default(),
            glide: 0.0,
            pb: 2.0,
            midi_open: false,
        }
    }
}

impl ShifterParams {
    /// Random waves use Width / Phase instead of Spin.
    pub fn is_random(&self) -> bool {
        self.wave >= 8
    }
}

/// LFO waveform `w` at phase `ph` (0..1) with duty `d` (−1..1) → −1..1.
/// `rnd` = (previous, next) random values for the random waves.
pub fn lfo_wave(w: u8, ph: f32, d: f32, rnd: (f32, f32)) -> f32 {
    // Duty: the first half of the cycle takes (1 + d) / 2 of the period.
    let h = ((1.0 + d.clamp(-0.98, 0.98)) * 0.5).clamp(0.01, 0.99);
    let ph = ph.rem_euclid(1.0);
    let q = if ph < h { ph / h * 0.5 } else { 0.5 + (ph - h) / (1.0 - h) * 0.5 };
    let tri = |q: f32| 1.0 - 4.0 * (q.rem_euclid(1.0) - 0.5).abs();
    match w {
        0 => (TAU * q).sin(),
        1 => tri(q),
        2 => {
            let t = tri(q);
            // Analog: rounded peaks.
            (t * 1.4).tanh() / 1.4f32.tanh()
        }
        3 => (tri(q) * 4.0).round() / 4.0,
        4 => (tri(q) * 8.0).round() / 8.0,
        5 => 2.0 * q - 1.0,
        6 => 1.0 - 2.0 * q,
        7 => {
            if q < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        8 => {
            // Smooth random: cosine between the two values.
            let u = 0.5 - 0.5 * (PI * ph).cos();
            rnd.0 + (rnd.1 - rnd.0) * u
        }
        _ => rnd.0,
    }
}

/// Second-order (z⁻²) allpass section of the Hilbert pair.
#[derive(Debug, Clone, Copy, Default)]
struct Ap2 {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Ap2 {
    #[inline]
    fn tick(&mut self, a2: f32, x: f32) -> f32 {
        let y = a2 * (x + self.y2) - self.x2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = if y.is_finite() { y } else { 0.0 };
        self.y1
    }
}

const HA: [f32; 4] = [0.692_387_8, 0.936_065_4, 0.988_229_5, 0.998_748_8];
const HB: [f32; 4] = [0.402_192_1, 0.856_171_1, 0.972_291, 0.995_288_5];

#[derive(Debug, Clone, Default)]
struct Hilbert {
    a: [Ap2; 4],
    b: [Ap2; 4],
    d: f32,
}

impl Hilbert {
    /// (in-phase, quadrature) of `x`.
    #[inline]
    fn tick(&mut self, x: f32) -> (f32, f32) {
        let mut i = x;
        for (s, c) in self.a.iter_mut().zip(HA) {
            i = s.tick(c * c, i);
        }
        let mut q = x;
        for (s, c) in self.b.iter_mut().zip(HB) {
            q = s.tick(c * c, q);
        }
        let i_d = self.d;
        self.d = i;
        (i_d, q)
    }
}

#[derive(Debug, Clone)]
pub struct ShifterState {
    tap: Option<Arc<DevTap>>,
    buf: [Box<[f32]>; 2],
    w: usize,
    grain: [f32; 2],
    hil: [Hilbert; 2],
    osc: [f32; 2],
    dl: [Box<[f32]>; 2],
    dw: usize,
    tone_lp: [f32; 2],
    lfo: [f32; 2],
    rnd: [(f32, f32); 2],
    rng: Rng,
    env: f32,
    notes: [AsNote; 32],
    n_notes: usize,
    held: [(u8, u32); 16],
    target: f32,
    note: f32,
    have_note: bool,
}

impl Default for ShifterState {
    fn default() -> Self {
        Self::new(None)
    }
}

impl ShifterState {
    pub fn new(id: Option<u64>) -> Self {
        let z = |n: usize| vec![0.0f32; n].into_boxed_slice();
        Self::build(id.map(crate::devtap::dev_tap), [z(DLEN), z(DLEN)], [z(FBLEN), z(FBLEN)])
    }

    /// Fresh state around existing (zeroed) buffers: `reset` must not allocate.
    fn build(tap: Option<Arc<DevTap>>, buf: [Box<[f32]>; 2], dl: [Box<[f32]>; 2]) -> Self {
        Self {
            tap,
            buf,
            w: 0,
            grain: [0.0; 2],
            hil: Default::default(),
            osc: [0.0; 2],
            dl,
            dw: 0,
            tone_lp: [0.0; 2],
            lfo: [0.0; 2],
            rnd: [(0.0, 0.0); 2],
            rng: Rng(0x0bad_cafe),
            env: 0.0,
            notes: [AsNote::default(); 32],
            n_notes: 0,
            held: [(0, 0); 16],
            target: 60.0,
            note: 60.0,
            have_note: false,
        }
    }

    pub fn set_id(&mut self, id: u64) {
        self.tap = Some(crate::devtap::dev_tap(id));
    }

    pub fn reset(&mut self) {
        let mut buf = std::mem::take(&mut self.buf);
        let mut dl = std::mem::take(&mut self.dl);
        for b in buf.iter_mut().chain(dl.iter_mut()) {
            b.fill(0.0);
        }
        *self = Self::build(self.tap.take(), buf, dl);
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

    /// Latest held note of the MIDI source (mono, last-note priority).
    fn midi_block(&mut self, p: &ShifterParams, n: usize, sr: f32) {
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
            let held = if e.len == u32::MAX { u32::MAX } else { e.len.saturating_add(e.offset) };
            let slot = self.held.iter().position(|h| h.0 == e.pitch && h.1 > 0).or_else(|| self.held.iter().position(|h| h.1 == 0));
            if let Some(i) = slot {
                self.held[i] = (e.pitch, held);
                live[i] = e.len == u32::MAX;
            }
            self.target = e.pitch as f32;
            self.have_note = true;
        }
        // Live keys not reported this block were released.
        for (i, h) in self.held.iter_mut().enumerate() {
            if h.1 == u32::MAX && !live[i] {
                h.1 = 0;
            }
        }
        self.n_notes = 0;
        let gl = if p.glide > 0.0 { 1.0 - (-(n as f32) / (p.glide * 0.001 * sr)).exp() } else { 1.0 };
        self.note += (self.target - self.note) * gl;
    }

    #[allow(clippy::needless_range_loop)]
    pub fn process(&mut self, p: &ShifterParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let n = l.len().min(r.len());
        if p.midi_mode {
            self.midi_block(p, n, sr);
        }
        let (tempo, _) = crate::modfx::clock();
        let beat_hz = tempo.max(1.0) as f32 / 60.0;
        let mode = p.mode.min(2);
        let pitch = mode == 0;
        // Base shift (st in Pitch, Hz otherwise) and the spread per channel.
        let base = if p.midi_mode {
            if pitch { self.note - 60.0 } else { crate::specres::midi_hz(self.note) }
        } else if pitch {
            p.coarse.clamp(-SH_COARSE, SH_COARSE) + p.fine.clamp(-SH_FINE, SH_FINE) / 100.0
        } else {
            p.coarse_hz.clamp(-SH_HZ, SH_HZ) + p.fine.clamp(-SH_FINE, SH_FINE)
        };
        let unit = |x: f32| if pitch { x / 100.0 } else { x };
        let spread = unit(p.spread.clamp(-SH_FINE, SH_FINE));
        let sp = [spread, if p.wide { -spread } else { spread }];
        let amt_range = if pitch { SH_AMT_ST } else { SH_AMT_HZ };
        let lfo_amt = p.lfo_amt.clamp(0.0, 1.0) * amt_range;
        let env_amt = if p.env_on { p.env_amt.clamp(-1.0, 1.0) * amt_range } else { 0.0 };
        let rate = if p.lfo_sync {
            beat_hz / SYNC_DIVS[(p.lfo_div as usize).min(SYNC_DIVS.len() - 1)].0 as f32
        } else {
            p.rate.clamp(SH_RATE.0, SH_RATE.1)
        };
        let rnd = p.is_random();
        let rates = if !rnd && p.spin_mode { [rate, rate * (1.0 + p.phase.clamp(0.0, 1.0))] } else { [rate, rate] };
        let ph_off = if !rnd && !p.spin_mode { p.phase.clamp(0.0, 1.0) } else { 0.0 };
        let width = if rnd { p.phase.clamp(0.0, 1.0) } else { 0.0 };
        let ea = (-1.0 / (p.env_attack.clamp(SH_ENV_T.0, SH_ENV_T.1) * 0.001 * sr)).exp();
        let er = (-1.0 / (p.env_release.clamp(SH_ENV_T.0, SH_ENV_T.1) * 0.001 * sr)).exp();
        let win = (p.window.clamp(SH_WINDOW.0, SH_WINDOW.1) * 0.001 * sr).clamp(64.0, (DLEN - 8) as f32);
        let k_tone = 1.0 - (-TAU * p.tone.clamp(SH_TONE.0, SH_TONE.1) / sr).exp();
        let dsamp = if p.delay_sync {
            SYNC_DIVS[(p.delay_div as usize).min(SYNC_DIVS.len() - 1)].0 as f32 / beat_hz * sr
        } else {
            p.delay_ms.clamp(SH_DELAY_MS.0, SH_DELAY_MS.1) * 0.001 * sr
        }
        .clamp(1.0, (FBLEN - 8) as f32);
        let fb = if p.delay_on { p.feedback.clamp(0.0, 0.95) } else { 0.0 };
        let g_out = 10f32.powf(p.output.clamp(SH_OUT.0, SH_OUT.1) / 20.0);
        let drive = mode == 2 && p.drive_on;
        let wet = p.dry_wet.clamp(0.0, 1.0);
        let mut ratio = [1.0f32; 2];
        let mut shift = [0.0f32; 2];
        for i in 0..n {
            let dry = [l[i], r[i]];
            // Modulators.
            let lvl = dry[0].abs().max(dry[1].abs());
            let c = if lvl > self.env { ea } else { er };
            self.env = lvl + c * (self.env - lvl);
            let mut mv = [0.0f32; 2];
            for ch in 0..2 {
                let prev = self.lfo[ch];
                self.lfo[ch] = (self.lfo[ch] + rates[ch] / sr).rem_euclid(1.0);
                if self.lfo[ch] < prev && rnd {
                    self.rnd[ch] = (self.rnd[ch].1, self.rng.bi());
                }
            }
            let v0 = lfo_wave(p.wave, self.lfo[0] + p.offset, p.duty, self.rnd[0]);
            let v1 = if rnd { v0 * (1.0 - 2.0 * width) } else { lfo_wave(p.wave, self.lfo[1] + ph_off + p.offset, p.duty, self.rnd[1]) };
            mv[0] = v0 * lfo_amt;
            mv[1] = v1 * lfo_amt;
            if i & 15 == 0 {
                for ch in 0..2 {
                    let s = base + sp[ch] + mv[ch] + env_amt * self.env.min(1.0);
                    if pitch {
                        ratio[ch] = 2f32.powf(s / 12.0);
                    } else {
                        shift[ch] = s;
                    }
                }
            }
            let mut out = [0.0f32; 2];
            for ch in 0..2 {
                let fbk = if p.delay_on { crate::dsp::read(&self.dl[ch], self.dw, dsamp, false) } else { 0.0 };
                self.tone_lp[ch] += k_tone * (fbk - self.tone_lp[ch]);
                let x = dry[ch] + fb * self.tone_lp[ch];
                let y = match mode {
                    0 => {
                        self.buf[ch][self.w] = x;
                        // Two taps half a window apart, raised-cosine crossfade.
                        let g = &mut self.grain[ch];
                        *g = (*g + (1.0 - ratio[ch]) / win).rem_euclid(1.0);
                        let g2 = (*g + 0.5).rem_euclid(1.0);
                        let a = crate::dsp::read(&self.buf[ch], self.w, 1.0 + *g * win, false);
                        let b = crate::dsp::read(&self.buf[ch], self.w, 1.0 + g2 * win, false);
                        let wa = (PI * *g).sin().powi(2);
                        a * wa + b * (1.0 - wa)
                    }
                    1 => {
                        let (re, im) = self.hil[ch].tick(x);
                        self.osc[ch] = (self.osc[ch] + shift[ch] / sr).rem_euclid(1.0);
                        let ph = TAU * self.osc[ch];
                        re * ph.cos() + im * ph.sin()
                    }
                    _ => {
                        self.osc[ch] = (self.osc[ch] + shift[ch] / sr).rem_euclid(1.0);
                        x * (TAU * self.osc[ch]).cos()
                    }
                };
                let y = if drive { (y * g_out).tanh() } else { y * g_out };
                let y = if y.is_finite() { y } else { 0.0 };
                self.dl[ch][self.dw] = y;
                let w = if p.delay_on { fbk } else { y };
                out[ch] = (dry[ch] + (w - dry[ch]) * wet).clamp(-8.0, 8.0);
            }
            self.w = (self.w + 1) & (DLEN - 1);
            self.dw = (self.dw + 1) & (FBLEN - 1);
            l[i] = out[0];
            r[i] = out[1];
        }
        if let Some(t) = &self.tap {
            t.set(TAP_LFO[0], self.lfo[0]);
            t.set(TAP_LFO[1], self.lfo[1]);
            t.set(TAP_ENV, self.env);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn freq_of(x: &[f32], sr: f32) -> f32 {
        let zc = x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        zc as f32 * sr / x.len() as f32
    }

    fn run(p: &ShifterParams, f: f32) -> Vec<f32> {
        let sr = 48_000.0;
        let mut s = ShifterState::new(None);
        let mut l: Vec<f32> = (0..48_000).map(|i| (i as f32 * TAU * f / sr).sin() * 0.5).collect();
        let mut r = l.clone();
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            s.process(p, a, b, sr);
        }
        l
    }

    #[test]
    fn pitch_freq_ring() {
        let sr = 48_000.0;
        let up = run(&ShifterParams { coarse: 12.0, ..Default::default() }, 440.0);
        let f = freq_of(&up[24_000..], sr);
        assert!((f - 880.0).abs() < 40.0, "pitch {f}");
        let fs = run(&ShifterParams { mode: 1, coarse_hz: 200.0, ..Default::default() }, 1000.0);
        let f = freq_of(&fs[24_000..], sr);
        assert!((f - 1200.0).abs() < 30.0, "freq {f}");
        let rg = run(&ShifterParams { mode: 2, coarse_hz: 100.0, ..Default::default() }, 1000.0);
        assert!(rg.iter().all(|v| v.is_finite()));
        let dl =
            run(&ShifterParams { delay_on: true, delay_sync: false, feedback: 0.5, wave: 9, lfo_amt: 0.2, ..Default::default() }, 300.0);
        assert!(dl.iter().all(|v| v.is_finite() && v.abs() <= 8.0));
        for w in 0..10u8 {
            for k in 0..50 {
                let v = lfo_wave(w, k as f32 / 50.0, 0.3, (0.2, -0.4));
                assert!((-1.001..=1.001).contains(&v), "wave {w}");
            }
        }
    }

    #[test]
    fn midi_sets_pitch() {
        let p = ShifterParams { midi_mode: true, ..Default::default() };
        let mut s = ShifterState::new(None);
        s.set_notes(&mut [AsNote { offset: 0, pitch: 72, vel: 100, len: 48_000 }].into_iter());
        let sr = 48_000.0;
        let mut l: Vec<f32> = (0..24_000).map(|i| (i as f32 * TAU * 440.0 / sr).sin() * 0.5).collect();
        let mut r = l.clone();
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            s.process(&p, a, b, sr);
        }
        let f = freq_of(&l[12_000..], sr);
        assert!((f - 880.0).abs() < 40.0, "{f}");
    }
}
