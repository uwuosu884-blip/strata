//! Spectral Resonator (Live 12, manual 29.31): the input spectrum (Shift
//! applied, scaled by Input Send with a limiter) excites a bank of partials
//! — a harmonic series on Freq (Internal) or on the notes of a MIDI source
//! (Mono / Poly, MIDI Gate, Glide) — spaced by Stretch, optionally quantised
//! to semitones / the clip scale, decaying with Decay and HF / LF damping.
//! Modulation: None / Chorus / Wander / Granular (Mod Rate, Pch. Mod);
//! Unison voices detuned by Uni. Amt; Dry/Wet. Partials are resynthesised
//! into the STFT frames (sub-bin accurate Hann kernels).

use crate::devtap::DevTap;
use crate::dsp::Rng;
use crate::dynfx::Sidechain;
use crate::stft::{SG_ROWS, SpecGram, Stft};
use realfft::num_complex::Complex;
use serde::{Deserialize, Serialize};
use std::f32::consts::{PI, TAU};
use std::sync::Arc;

pub const SR_MIDI: (f32, f32) = (0.0, 96.0);
pub const SR_TRANSP: f32 = 48.0;
pub const SR_SHIFT: f32 = 48.0;
pub const SR_DECAY: (f32, f32) = (1.0, 35_000.0);
pub const SR_RATE: (f32, f32) = (1.0, 100.0);
pub const SR_PCH: f32 = 12.0;
pub const SR_HARM: (f32, f32) = (1.0, 256.0);
pub const SR_SEND: f32 = 12.0;
pub const SR_UNISON: (f32, f32) = (1.0, 8.0);
pub const SR_GLIDE: (f32, f32) = (0.0, 5000.0);
pub const SR_PB: f32 = 24.0;
pub const SR_VOICES: [u8; 4] = [2, 4, 8, 16];
pub const SR_MODS: [&str; 4] = ["None", "Chorus", "Wander", "Granular"];
/// Tap slot: input limiter active (held peak).
pub const TAP_LIMIT: usize = 0;
const MAXP: usize = 2048;
const MAXV: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpecResParams {
    /// MIDI mode (else Internal).
    pub midi_mode: bool,
    /// MIDI source (`on` follows `midi_mode`).
    pub midi: Sidechain,
    pub poly: bool,
    /// Index into [`SR_VOICES`].
    pub voices: u8,
    pub midi_gate: bool,
    /// ms.
    pub glide: f32,
    /// Pitch bend range (semitones).
    pub pb: f32,
    /// Hz (Internal).
    pub freq: f32,
    /// Freq shown / dragged as notes.
    pub note_mode: bool,
    /// Semitones (MIDI mode).
    pub transp: f32,
    /// −1..1 (100 % = odd partials only).
    pub stretch: f32,
    /// Semitones applied to the input spectrum.
    pub shift: f32,
    /// ms.
    pub decay: f32,
    pub hi_damp: f32,
    pub lo_damp: f32,
    /// Index into [`SR_MODS`].
    pub mod_mode: u8,
    /// Percent (1..100).
    pub mod_rate: f32,
    /// Semitones.
    pub pch_mod: f32,
    pub harmonics: f32,
    pub quantize: bool,
    /// dB.
    pub send: f32,
    pub unison: f32,
    pub uni_amt: f32,
    pub dry_wet: f32,
    pub scale_aware: bool,
    /// Spectrogram running.
    pub display: bool,
}

impl Default for SpecResParams {
    fn default() -> Self {
        Self {
            midi_mode: false,
            midi: Sidechain::default(),
            poly: false,
            voices: 2,
            midi_gate: false,
            glide: 0.0,
            pb: 2.0,
            freq: 110.0,
            note_mode: false,
            transp: 0.0,
            stretch: 0.0,
            shift: 0.0,
            decay: 222.0,
            hi_damp: 0.0,
            lo_damp: 0.0,
            mod_mode: 0,
            mod_rate: 10.0,
            pch_mod: 0.0,
            harmonics: 256.0,
            quantize: false,
            send: 0.0,
            unison: 1.0,
            uni_amt: 0.1,
            dry_wet: 1.0,
            scale_aware: false,
            display: true,
        }
    }
}

pub fn midi_hz(m: f32) -> f32 {
    440.0 * 2f32.powf((m - 69.0) / 12.0)
}

pub fn hz_midi(f: f32) -> f32 {
    69.0 + 12.0 * (f.max(1e-3) / 440.0).log2()
}

#[derive(Debug, Clone, Copy, Default)]
struct Voice {
    pitch: u8,
    cur: f32,
    /// Samples until release (`u32::MAX` = live key).
    held: u32,
    seen: bool,
    active: bool,
    age: u64,
    /// Gate envelope (MIDI Gate release).
    env: f32,
}

#[derive(Debug, Clone)]
pub struct SpecResState {
    stft: Stft,
    tap: Option<Arc<DevTap>>,
    gram: Option<Arc<SpecGram>>,
    amp: [Box<[f32]>; 2],
    phase: Box<[f32]>,
    wander: Box<[f32]>,
    gate: Box<[f32]>,
    out: [Box<[Complex<f32>]>; 2],
    voices: [Voice; MAXV],
    notes: [crate::autoshift::AsNote; 64],
    n_notes: usize,
    clock: u64,
    lfo: f32,
    rng: Rng,
    lim: f32,
}

impl Default for SpecResState {
    fn default() -> Self {
        Self::new(None)
    }
}

impl SpecResState {
    pub fn new(id: Option<u64>) -> Self {
        let z = || vec![0.0f32; MAXP].into_boxed_slice();
        let zc = || vec![Complex::new(0.0f32, 0.0); crate::stft::MAX_BINS].into_boxed_slice();
        Self {
            stft: Stft::new(),
            tap: id.map(crate::devtap::dev_tap),
            gram: id.map(crate::stft::spec_gram),
            amp: [z(), z()],
            phase: z(),
            wander: z(),
            gate: vec![1.0f32; MAXP].into_boxed_slice(),
            out: [zc(), zc()],
            voices: [Voice::default(); MAXV],
            notes: [Default::default(); 64],
            n_notes: 0,
            clock: 0,
            lfo: 0.0,
            rng: Rng(0x2545_f491),
            lim: 1.0,
        }
    }

    /// Attach the display taps of device `id` (control thread).
    pub fn set_id(&mut self, id: u64) {
        self.tap = Some(crate::devtap::dev_tap(id));
        self.gram = Some(crate::stft::spec_gram(id));
    }

    pub fn reset(&mut self) {
        self.stft.reset();
        for a in self.amp.iter_mut() {
            a.fill(0.0);
        }
        self.wander.fill(0.0);
        self.gate.fill(1.0);
        self.voices = [Voice::default(); MAXV];
        self.n_notes = 0;
        self.lim = 1.0;
    }

    /// STFT latency (samples).
    pub const LATENCY: usize = crate::stft::SIZES[1];

    pub(crate) fn set_notes(&mut self, it: &mut dyn Iterator<Item = crate::autoshift::AsNote>) {
        self.n_notes = 0;
        for e in it {
            if self.n_notes < self.notes.len() {
                self.notes[self.n_notes] = e;
                self.n_notes += 1;
            }
        }
    }

    fn note_on(&mut self, p: &SpecResParams, pitch: u8, held: u32) {
        self.clock += 1;
        let clock = self.clock;
        let nv = if p.poly { SR_VOICES[(p.voices as usize).min(3)] as usize } else { 1 };
        if let Some(v) = self.voices[..nv].iter_mut().find(|v| v.active && v.pitch == pitch) {
            v.held = held;
            v.seen = true;
            v.env = 1.0;
            return;
        }
        let i = if nv == 1 {
            0
        } else {
            self.voices[..nv].iter().position(|v| !v.active).unwrap_or_else(|| {
                // Steal the oldest released voice, else the oldest.
                let mut best = 0;
                for (k, v) in self.voices[..nv].iter().enumerate() {
                    let older = v.age < self.voices[best].age;
                    let rel = v.held == 0 && self.voices[best].held != 0;
                    if rel || (older && (v.held == 0) == (self.voices[best].held == 0)) {
                        best = k;
                    }
                }
                best
            })
        };
        let v = &mut self.voices[i];
        let glide = nv == 1 && v.active && p.glide > 0.0;
        *v = Voice { pitch, cur: if glide { v.cur } else { pitch as f32 }, held, seen: true, active: true, age: clock, env: 1.0 };
    }

    /// Applies this block's notes (MIDI mode).
    fn midi_block(&mut self, p: &SpecResParams, n: usize, sr: f32) {
        for v in self.voices.iter_mut() {
            v.seen = false;
        }
        for k in 0..self.n_notes {
            let e = self.notes[k];
            if e.vel == 0 {
                continue;
            }
            let held = if e.len == u32::MAX { u32::MAX } else { e.len.saturating_add(e.offset) };
            self.note_on(p, e.pitch, held);
        }
        let gl = if p.glide > 0.0 { 1.0 - (-(n as f32) / (p.glide * 0.001 * sr)).exp() } else { 1.0 };
        let rel = (-(n as f32) / (0.02 * sr)).exp();
        for v in self.voices.iter_mut().filter(|v| v.active) {
            if v.held == u32::MAX {
                if !v.seen {
                    v.held = 0;
                }
            } else {
                v.held = v.held.saturating_sub(n as u32);
            }
            v.cur += (v.pitch as f32 - v.cur) * gl;
            if v.held == 0 && p.midi_gate {
                v.env *= rel;
                if v.env < 1e-4 {
                    v.active = false;
                }
            }
        }
        self.n_notes = 0;
    }

    #[allow(clippy::needless_range_loop)]
    pub fn process(&mut self, p: &SpecResParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let n = l.len().min(r.len());
        if p.midi_mode {
            self.midi_block(p, n, sr);
        }
        // Fundamentals (MIDI pitch, gate env) of the active voices.
        let mut f0s = [(0.0f32, 0.0f32); MAXV];
        let mut nf = 0;
        if p.midi_mode {
            for v in self.voices.iter().filter(|v| v.active) {
                f0s[nf] = (v.cur + p.transp, v.env);
                nf += 1;
            }
        } else {
            f0s[0] = (hz_midi(p.freq), 1.0);
            nf = 1;
        }
        let scale = crate::autoshift::current_scale().filter(|_| p.scale_aware);
        let send = 10f32.powf(p.send.clamp(-SR_SEND, SR_SEND) / 20.0);
        let wet = p.dry_wet.clamp(0.0, 1.0);
        let uni = (p.unison.round() as usize).clamp(1, 8);
        let harm = (p.harmonics.round() as usize).clamp(1, 256);
        let spacing = (1.0 + p.stretch.clamp(-1.0, 1.0)).max(0.02);
        let shift = 2f32.powf(p.shift.clamp(-SR_SHIFT, SR_SHIFT) / 12.0);
        let rate = p.mod_rate.clamp(SR_RATE.0, SR_RATE.1) / 100.0;
        let depth = if p.pch_mod > 0.0 { p.pch_mod.min(SR_PCH) } else { 0.15 };
        let decay_s = p.decay.clamp(SR_DECAY.0, SR_DECAY.1) * 0.001 / 6.9;
        let gram = if p.display { self.gram.clone() } else { None };
        let (amp, phase, wander, gate, out, rng, lfo, lim) =
            (&mut self.amp, &mut self.phase, &mut self.wander, &mut self.gate, &mut self.out, &mut self.rng, &mut self.lfo, &mut self.lim);
        let mut limited = false;
        self.stft.process(l, r, &mut |a, b, bins| {
            let nfft = (bins - 1) * 2;
            let hop = nfft / 4;
            let hz = sr / nfft as f32;
            let nyq = sr * 0.5 * 0.95;
            let mut rows_d = [0.0f32; SG_ROWS];
            if gram.is_some() {
                crate::stft::gram_rows(a, b, nfft, sr, &mut rows_d);
            }
            // Input level after Send (limiter).
            let mut peak = 0.0f32;
            for k in 0..bins {
                peak = peak.max(a[k].norm()).max(b[k].norm());
            }
            peak *= send;
            // Limiter: full-scale sine ≈ n/4 in a Hann frame.
            let ceil = nfft as f32 / 4.0;
            let target = if peak > ceil { ceil / peak } else { 1.0 };
            *lim = if target < *lim { target } else { *lim + (target - *lim) * 0.2 };
            if *lim < 0.99 {
                limited = true;
            }
            *lfo = (*lfo + hop as f32 / sr * rate * 10.0).fract();
            for o in out.iter_mut() {
                o[..bins].fill(Complex::new(0.0, 0.0));
            }
            let mut idx = 0usize;
            'voices: for &(m0, env) in f0s[..nf].iter() {
                for h in 0..harm {
                    let mut m = hz_midi(midi_hz(m0) * (1.0 + h as f32 * spacing));
                    if p.quantize {
                        m = m.round();
                        if let Some((_, mask)) = scale {
                            m = crate::resonators::snap_to_scale(m as i32, mask) as f32;
                        }
                    }
                    for u in 0..uni {
                        if idx >= MAXP {
                            break 'voices;
                        }
                        let det = if uni > 1 { (u as f32 / (uni - 1) as f32 * 2.0 - 1.0) * p.uni_amt * 0.5 } else { 0.0 };
                        let modst = match p.mod_mode {
                            1 => depth * (TAU * (*lfo + idx as f32 * 0.618_034)).sin(),
                            2 => {
                                wander[idx] += (rng.bi() * depth - wander[idx]) * rate * 0.3;
                                wander[idx]
                            }
                            _ => 0.0,
                        };
                        let f = midi_hz(m + det + modst);
                        if f >= nyq || f < 10.0 {
                            idx += 1;
                            continue;
                        }
                        let bin = f / hz;
                        let k0 = bin.round() as usize;
                        // Excitation: input energy around the partial (both channels).
                        let mut e = [0.0f32; 2];
                        for (c, s) in [&*a, &*b].into_iter().enumerate() {
                            let mut acc = 0.0f32;
                            for k in k0.saturating_sub(1)..=(k0 + 1).min(bins - 1) {
                                let src = (k as f32 / shift).round() as usize;
                                if src < bins {
                                    acc += s[src].norm_sqr();
                                }
                            }
                            e[c] = acc.sqrt() * send * *lim;
                        }
                        let wl = (f / 8000.0).min(1.0);
                        let wlo = (120.0 / f).min(1.0);
                        let tau = decay_s * (1.0 - 0.97 * p.hi_damp * wl) * (1.0 - 0.97 * p.lo_damp * wlo);
                        let df = (-(hop as f32) / (sr * tau.max(1e-4))).exp();
                        let g = if p.mod_mode == 3 {
                            if rng.uni() < 0.05 + 0.6 * rate {
                                gate[idx] = rng.uni();
                            }
                            gate[idx]
                        } else {
                            1.0
                        };
                        let pan = if uni > 1 { u as f32 / (uni - 1) as f32 } else { 0.5 };
                        phase[idx] = (phase[idx] + TAU * f * hop as f32 / sr).rem_euclid(TAU);
                        let ph = Complex::from_polar(1.0, phase[idx]);
                        let ex = env * if p.midi_mode && env < 1.0 { 0.0 } else { 1.0 };
                        for c in 0..2 {
                            let a1 = (amp[c][idx] * df).max(e[c] * ex);
                            amp[c][idx] = if a1.is_finite() { a1 } else { 0.0 };
                            let pg = if c == 0 { (1.0 - pan).sqrt() } else { pan.sqrt() } * std::f32::consts::SQRT_2;
                            let v = amp[c][idx] * g * env * pg / (uni as f32).sqrt();
                            if v < 1e-7 {
                                continue;
                            }
                            let kb = bin.floor() as isize;
                            for k in (kb - 1)..=(kb + 2) {
                                if k < 0 || k as usize >= bins {
                                    continue;
                                }
                                let d = k as f32 - bin;
                                let w = hann_kernel(d);
                                let sign = if k & 1 == 0 { 1.0 } else { -1.0 };
                                out[c][k as usize] += ph * (v * w * sign);
                            }
                        }
                        idx += 1;
                    }
                }
            }
            for k in idx..MAXP.min(idx + 64) {
                amp[0][k] = 0.0;
                amp[1][k] = 0.0;
            }
            if let Some(gm) = &gram {
                let mut rows_w = [0.0f32; SG_ROWS];
                crate::stft::gram_rows(&out[0][..bins], &out[1][..bins], nfft, sr, &mut rows_w);
                gm.push(&rows_d, &rows_w);
            }
            for k in 0..bins {
                a[k] = a[k] * (1.0 - wet) + out[0][k] * wet;
                b[k] = b[k] * (1.0 - wet) + out[1][k] * wet;
            }
        });
        if limited && let Some(t) = &self.tap {
            t.raise(TAP_LIMIT, 1.0);
        }
    }
}

/// Hann window spectrum normalised to 1 at the centre (`d` in bins).
#[inline]
fn hann_kernel(d: f32) -> f32 {
    let ad = d.abs();
    if ad < 1e-4 {
        return 1.0;
    }
    if (ad - 1.0).abs() < 1e-4 {
        return 0.5;
    }
    if ad >= 2.0 {
        return 0.0;
    }
    (PI * d).sin() / (PI * d) / (1.0 - d * d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resonates_on_freq() {
        let p = SpecResParams { decay: 2000.0, harmonics: 1.0, ..Default::default() };
        let mut s = SpecResState::new(None);
        let sr = 48_000.0;
        let mut rng = Rng(7);
        let mut l: Vec<f32> = (0..48_000).map(|i| if i < 4800 { rng.bi() * 0.3 } else { 0.0 }).collect();
        let mut r = l.clone();
        s.process(&p, &mut l, &mut r, sr);
        assert!(l.iter().all(|x| x.is_finite()));
        // Rings at 110 Hz after the noise stops.
        let tail = &l[20_000..44_000];
        let zc = tail.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count() as f32;
        let f = zc / (tail.len() as f32 / sr);
        assert!((f - 110.0).abs() < 6.0, "{f}");
        assert!(tail.iter().map(|x| x.abs()).fold(0.0, f32::max) > 1e-3);
    }

    #[test]
    fn midi_voices() {
        let p = SpecResParams { midi_mode: true, poly: true, ..Default::default() };
        let mut s = SpecResState::new(None);
        s.set_notes(&mut [crate::autoshift::AsNote { offset: 0, pitch: 60, vel: 100, len: 1000 }].into_iter());
        let mut l = vec![0.1f32; 512];
        let mut r = l.clone();
        s.process(&p, &mut l, &mut r, 48_000.0);
        assert!(s.voices[0].active && s.voices[0].pitch == 60);
    }
}
