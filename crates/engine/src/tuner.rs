//! Tuner (Live 12, manual 29.39): a measurement tool — the signal passes
//! unchanged. Monophonic pitch detection (YIN on a 4× decimated mono sum)
//! publishes the frequency to the device tap (Classic view) and a pitch
//! history ring (Histogram view).

use crate::devtap::DevTap;
use crate::dsp::Svf;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const TU_REF: (f32, f32) = (410.0, 480.0);
/// Note spellings (display context menu).
pub const TU_SPELL: [&str; 3] = ["Sharps (C#)", "Flats (D♭)", "Sharps and Flats (C#/D♭)"];
/// Tap slots: detected frequency (0 = none), input level.
pub const TAP_FREQ: usize = 0;
pub const TAP_LEVEL: usize = 1;
/// History frame: [frequency (0 = none), level, 0, 0] every `HOP` input samples.
pub const HOP: usize = 1024;
const DEC: usize = 4;
const WIN: usize = 512;
const TAU_MAX: usize = 500;
const BUF: usize = WIN + TAU_MAX;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TunerParams {
    /// Histogram (else Classic) view.
    pub histogram: bool,
    /// Classic view: Strobe (else Target) mode.
    pub strobe: bool,
    /// Show Hz (else cents).
    pub hz: bool,
    /// Histogram: keep the pitch centred.
    pub auto: bool,
    /// Reference pitch of A4 (Hz).
    pub reference: f32,
    /// Index into [`TU_SPELL`].
    pub spelling: u8,
    /// Histogram: MIDI note at the display centre (manual scroll).
    pub center: f32,
    /// Histogram: semitones visible (zoom).
    pub span: f32,
}

impl Default for TunerParams {
    fn default() -> Self {
        Self { histogram: false, strobe: false, hz: false, auto: true, reference: 440.0, spelling: 0, center: 69.0, span: 3.0 }
    }
}

/// Frequency → (nearest MIDI note, cents off) for reference `a4`.
pub fn note_of(f: f32, a4: f32) -> (i32, f32) {
    let m = 69.0 + 12.0 * (f / a4.max(1.0)).log2();
    let n = m.round();
    (n as i32, (m - n) * 100.0)
}

#[derive(Debug, Clone)]
pub struct TunerState {
    tap: Option<Arc<DevTap>>,
    lp: [Svf; 2],
    acc: f32,
    k: usize,
    buf: Box<[f32]>,
    w: usize,
    filled: usize,
    since: usize,
    d: Box<[f32]>,
    level: f32,
    freq: f32,
}

impl Default for TunerState {
    fn default() -> Self {
        Self::new(None)
    }
}

impl TunerState {
    pub fn new(id: Option<u64>) -> Self {
        Self::build(id.map(crate::devtap::dev_tap), vec![0.0; BUF].into_boxed_slice(), vec![0.0; TAU_MAX + 1].into_boxed_slice())
    }

    fn build(tap: Option<Arc<DevTap>>, buf: Box<[f32]>, d: Box<[f32]>) -> Self {
        Self { tap, lp: [Svf::default(); 2], acc: 0.0, k: 0, buf, w: 0, filled: 0, since: 0, d, level: 0.0, freq: 0.0 }
    }

    pub fn set_id(&mut self, id: u64) {
        self.tap = Some(crate::devtap::dev_tap(id));
    }

    pub fn reset(&mut self) {
        let mut buf = std::mem::take(&mut self.buf);
        let mut d = std::mem::take(&mut self.d);
        buf.fill(0.0);
        d.fill(0.0);
        *self = Self::build(self.tap.take(), buf, d);
    }

    /// YIN over the last `BUF` decimated samples → frequency (0 = unvoiced).
    fn detect(&mut self, sr_d: f32) -> f32 {
        let n = self.buf.len();
        let at = |i: usize| self.buf[(self.w + i) % n];
        // Difference function.
        let mut run = 0.0;
        self.d[0] = 1.0;
        let mut best = None;
        for tau in 1..=TAU_MAX {
            let mut s = 0.0;
            for j in 0..WIN {
                let e = at(j) - at(j + tau);
                s += e * e;
            }
            run += s;
            self.d[tau] = if run > 0.0 { s * tau as f32 / run } else { 1.0 };
        }
        // First dip below the threshold (min 2 samples: ≤ 3 kHz).
        let mut tau = 3;
        while tau < TAU_MAX {
            if self.d[tau] < 0.15 {
                while tau + 1 < TAU_MAX && self.d[tau + 1] < self.d[tau] {
                    tau += 1;
                }
                best = Some(tau);
                break;
            }
            tau += 1;
        }
        let Some(t) = best else { return 0.0 };
        // Parabolic interpolation.
        let (a, b, c) = (self.d[t - 1], self.d[t], self.d[(t + 1).min(TAU_MAX)]);
        let den = a + c - 2.0 * b;
        let off = if den.abs() > 1e-9 { 0.5 * (a - c) / den } else { 0.0 };
        sr_d / (t as f32 + off.clamp(-1.0, 1.0))
    }

    pub fn process(&mut self, _p: &TunerParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let n = l.len().min(r.len());
        let g = crate::dsp::svf_g(sr / (DEC as f32 * 2.6), sr);
        let fall = (-1.0 / (0.3 * sr)).exp();
        for i in 0..n {
            let x = 0.5 * (l[i] + r[i]);
            self.level = x.abs().max(self.level * fall);
            let y = self.lp[0].tick(x, g, crate::dsp::BUTTER_K).0;
            let y = self.lp[1].tick(y, g, crate::dsp::BUTTER_K).0;
            self.acc = y;
            self.k += 1;
            if self.k == DEC {
                self.k = 0;
                let len = self.buf.len();
                self.buf[self.w] = self.acc;
                self.w = (self.w + 1) % len;
                self.filled = (self.filled + 1).min(len);
            }
            self.since += 1;
            if self.since >= HOP {
                self.since = 0;
                // Gate at −60 dB.
                self.freq = if self.filled == self.buf.len() && self.level > 1e-3 { self.detect(sr / DEC as f32) } else { 0.0 };
                if let Some(t) = &self.tap {
                    t.set_rate(sr / HOP as f32);
                    t.push([self.freq, self.level, 0.0, 0.0]);
                }
            }
        }
        if let Some(t) = &self.tap {
            t.set(TAP_FREQ, self.freq);
            t.set(TAP_LEVEL, self.level);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    #[test]
    fn detects_pitch_and_passes_audio() {
        let sr = 48_000.0;
        for f in [82.4f32, 196.0, 440.0, 1046.5] {
            let mut s = TunerState::new(None);
            let src: Vec<f32> = (0..48_000).map(|i| (i as f32 * TAU * f / sr).sin() * 0.5 + (i as f32 * TAU * 2.0 * f / sr).sin() * 0.2).collect();
            let mut l = src.clone();
            let mut r = src.clone();
            for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
                s.process(&TunerParams::default(), a, b, sr);
            }
            assert_eq!(l, src);
            assert!((s.freq - f).abs() / f < 0.01, "{f}: {}", s.freq);
        }
        let (n, c) = note_of(445.0, 440.0);
        assert_eq!(n, 69);
        assert!((c - 19.56).abs() < 0.1);
    }
}
