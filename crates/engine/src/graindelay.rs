//! Grain Delay (Live 12, manual 29.14): the delayed signal is re-read as
//! overlapping Hann grains (Frequency = grains per second), each grain
//! pitch-shifted (Pitch + Random Pitch) and started at a random extra delay
//! (Spray). Delay time synced to 16th multiples (+ offset) or free; Feedback,
//! Dry/Wet; any two parameters can be played from the X-Y field.

use crate::dsp::{Rng, read};
use serde::{Deserialize, Serialize};

pub const GD_SIXTEENTHS: [u8; 8] = [1, 2, 3, 4, 5, 6, 8, 16];
pub const GD_MS: (f32, f32) = (1.0, 128.0);
pub const GD_OFFSET: f32 = 0.333;
pub const GD_SPRAY: (f32, f32) = (0.0, 500.0);
pub const GD_FREQ: (f32, f32) = (1.0, 150.0);
pub const GD_PITCH: (f32, f32) = (-36.0, 12.0);
pub const GD_RAND: (f32, f32) = (0.0, 161.0);
pub const GD_FB_MAX: f32 = 0.95;
/// X-axis choices (bottom row).
pub const GD_X: [&str; 6] = ["Delay Time", "Spray", "Frequency", "Pitch", "Rand Pitch", "Feedback"];
/// Y-axis choices (left column).
pub const GD_Y: [&str; 6] = ["Spray", "Frequency", "Pitch", "Rand Pitch", "Feedback", "DryWet"];
/// Random pitch range (semitones) at Rand Pitch = 161.
const RAND_ST: f32 = 12.0;
const BUF: usize = 1 << 19;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GrainDelayParams {
    pub sync: bool,
    pub six: u8,
    pub ms: f32,
    pub offset: f32,
    pub spray: f32,
    pub freq: f32,
    pub pitch: f32,
    pub rand: f32,
    pub feedback: f32,
    pub dry_wet: f32,
    pub x_axis: u8,
    pub y_axis: u8,
}

impl Default for GrainDelayParams {
    fn default() -> Self {
        Self {
            sync: true,
            six: 1,
            ms: 100.0,
            offset: 0.0,
            spray: 0.0,
            freq: 60.0,
            pitch: 0.0,
            rand: 0.0,
            feedback: 0.0,
            dry_wet: 1.0,
            x_axis: 3,
            y_axis: 1,
        }
    }
}

impl GrainDelayParams {
    pub fn secs(&self, tempo: f64) -> f32 {
        if self.sync {
            let n = GD_SIXTEENTHS[(self.six as usize).min(7)] as f32;
            n * 15.0 / tempo.max(1.0) as f32 * (1.0 + self.offset.clamp(-GD_OFFSET, GD_OFFSET))
        } else {
            self.ms.clamp(GD_MS.0, GD_MS.1) * 0.001
        }
    }

    pub fn ms_label(ms: f32) -> String {
        if ms >= 100.0 {
            format!("{} ms", ms.round())
        } else if ms >= 10.0 {
            format!("{ms:.1} ms")
        } else {
            format!("{ms:.2} ms")
        }
    }

    pub fn freq_label(&self) -> String {
        if self.freq >= 100.0 { format!("{} Hz", self.freq.round()) } else { format!("{:.1} Hz", self.freq) }
    }

    pub fn pct_label(v: f32) -> String {
        let p = v * 100.0;
        if p >= 99.95 { format!("{} %", p.round()) } else { format!("{p:.1} %") }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Grain {
    on: bool,
    age: f32,
    len: f32,
    d0: f32,
    rate: f32,
}

#[derive(Debug, Clone)]
pub struct GrainDelayState {
    buf: Box<[Vec<f32>; 2]>,
    w: usize,
    grains: [Grain; 2],
    next: usize,
    until: f32,
    rng: Rng,
}

impl Default for GrainDelayState {
    fn default() -> Self {
        Self {
            buf: Box::new([vec![0.0; BUF], vec![0.0; BUF]]),
            w: 0,
            grains: [Grain::default(); 2],
            next: 0,
            until: 0.0,
            rng: Rng(0x6d2b_79f5),
        }
    }
}

impl GrainDelayState {
    pub fn reset(&mut self) {
        for b in self.buf.iter_mut() {
            b.fill(0.0);
        }
        self.grains = [Grain::default(); 2];
        self.until = 0.0;
    }

    pub fn process(&mut self, p: &GrainDelayParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let n = l.len().min(r.len());
        let (tempo, _) = crate::modfx::clock();
        let lim = (BUF - 8) as f32;
        let d = (p.secs(tempo) * sr).clamp(1.0, lim * 0.5);
        let period = sr / p.freq.clamp(GD_FREQ.0, GD_FREQ.1);
        let fb = p.feedback.clamp(0.0, GD_FB_MAX);
        let wet_g = p.dry_wet.clamp(0.0, 1.0);
        let spray = p.spray.clamp(GD_SPRAY.0, GD_SPRAY.1) * 0.001 * sr;
        let rand_st = p.rand.clamp(GD_RAND.0, GD_RAND.1) / GD_RAND.1 * RAND_ST;
        let pitch = p.pitch.clamp(GD_PITCH.0, GD_PITCH.1);
        for i in 0..n {
            self.until -= 1.0;
            if self.until <= 0.0 {
                self.until += period;
                let len = 2.0 * period;
                let rate = 2f32.powf((pitch + rand_st * self.rng.bi()) / 12.0);
                let d0 = d + spray * self.rng.uni() + ((rate - 1.0) * len).max(0.0) + 1.0;
                self.grains[self.next] = Grain { on: true, age: 0.0, len, d0: d0.min(lim - ((1.0 - rate) * len).max(0.0)), rate };
                self.next ^= 1;
            }
            let mut y = [0.0f32; 2];
            for g in self.grains.iter_mut().filter(|g| g.on) {
                let dd = (g.d0 - g.age * (g.rate - 1.0)).clamp(1.0, lim);
                let win = (std::f32::consts::PI * g.age / g.len).sin().powi(2);
                for (c, v) in y.iter_mut().enumerate() {
                    *v += read(&self.buf[c], self.w, dd, true) * win;
                }
                g.age += 1.0;
                if g.age >= g.len {
                    g.on = false;
                }
            }
            let (il, ir) = (l[i], r[i]);
            let wl = il + fb * y[0];
            let wr = ir + fb * y[1];
            self.buf[0][self.w] = if wl.is_finite() { wl.clamp(-8.0, 8.0) } else { 0.0 };
            self.buf[1][self.w] = if wr.is_finite() { wr.clamp(-8.0, 8.0) } else { 0.0 };
            self.w = (self.w + 1) % BUF;
            l[i] = il + (y[0] - il) * wet_g;
            r[i] = ir + (y[1] - ir) * wet_g;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpitched_grains_reconstruct_a_delay() {
        let p = GrainDelayParams { sync: false, ms: 20.0, freq: 20.0, ..Default::default() };
        let mut s = GrainDelayState::default();
        let src: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.02).sin() * 0.5).collect();
        let mut l = src.clone();
        let mut r = src.clone();
        s.process(&p, &mut l, &mut r, 48_000.0);
        // Overlapping sin² grains sum to 1 → output ≈ input delayed by 21 samples+.
        let d = 961;
        let err: f32 = (10_000..20_000).map(|i| (l[i] - src[i - d]).abs()).fold(0.0, f32::max);
        assert!(err < 0.02, "{err}");
    }

    #[test]
    fn pitched_feedback_stays_finite() {
        let p = GrainDelayParams { pitch: 12.0, rand: 161.0, spray: 500.0, feedback: 1.0, freq: 1.0, ..Default::default() };
        let mut s = GrainDelayState::default();
        for _ in 0..50 {
            let mut l: Vec<f32> = (0..512).map(|i| (i as f32 * 0.1).sin()).collect();
            let mut r = l.clone();
            s.process(&p, &mut l, &mut r, 48_000.0);
            assert!(l.iter().all(|x| x.is_finite()));
        }
    }
}
