//! Live's LFO (Max for Live modulator, manual 33.2.3): a low-frequency
//! oscillator that modulates up to eight mapped device parameters. Audio
//! passes through unchanged; the engine applies the modulation to the target
//! slots each block (`fx::modulate`), the stored (base) values stay untouched.

use crate::devtap::{DevTap, HOP, dev_tap};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const LFO_MAPS: usize = 8;
pub const LFO_RATE: (f32, f32) = (0.01, 40.0);
pub const LFO_STEPS_MAX: u8 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LfoWave {
    #[default]
    Sine,
    Up,
    Down,
    Triangle,
    Square,
    Random,
    Bin,
    Stray,
    Glider,
}

impl LfoWave {
    pub const ALL: [LfoWave; 9] = [
        LfoWave::Sine,
        LfoWave::Up,
        LfoWave::Down,
        LfoWave::Triangle,
        LfoWave::Square,
        LfoWave::Random,
        LfoWave::Bin,
        LfoWave::Stray,
        LfoWave::Glider,
    ];
    pub fn label(self) -> &'static str {
        match self {
            LfoWave::Sine => "Sine",
            LfoWave::Up => "Up",
            LfoWave::Down => "Down",
            LfoWave::Triangle => "Triangle",
            LfoWave::Square => "Square",
            LfoWave::Random => "Random",
            LfoWave::Bin => "Bin",
            LfoWave::Stray => "Stray",
            LfoWave::Glider => "Glider",
        }
    }
    /// Shape is greyed for the random waveforms.
    pub fn shapeable(self) -> bool {
        !matches!(self, LfoWave::Random | LfoWave::Bin | LfoWave::Stray | LfoWave::Glider)
    }
    /// Steps is greyed for Random, Bin and Square.
    pub fn steppable(self) -> bool {
        !matches!(self, LfoWave::Random | LfoWave::Bin | LfoWave::Square)
    }
}

/// Tempo-synced rates (beats per cycle) and their labels.
pub const LFO_SYNC: [(f64, &str); 18] = [
    (1.0 / 16.0, "1/64"),
    (1.0 / 12.0, "1/32T"),
    (1.0 / 8.0, "1/32"),
    (1.0 / 6.0, "1/16T"),
    (0.25, "1/16"),
    (1.0 / 3.0, "1/8T"),
    (0.5, "1/8"),
    (2.0 / 3.0, "1/4T"),
    (1.0, "1/4"),
    (4.0 / 3.0, "1/2T"),
    (2.0, "1/2"),
    (8.0 / 3.0, "1/1T"),
    (4.0, "1"),
    (8.0, "2"),
    (12.0, "3"),
    (16.0, "4"),
    (32.0, "8"),
    (64.0, "16"),
];

/// One mapping: device `device`'s parameter `param`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LfoMap {
    /// Target device id (0 = unmapped).
    pub device: u64,
    pub param: u8,
    /// Remote Control (Min/Max) instead of Modulation.
    pub remote: bool,
    /// Modulation polarity: unipolar adds in one direction.
    pub unipolar: bool,
    /// Modulation amount 0..1 (of the parameter range).
    pub amount: f32,
    /// Remote Control range (normalised).
    pub min: f32,
    pub max: f32,
}

impl Default for LfoMap {
    fn default() -> Self {
        Self { device: 0, param: 0, remote: false, unipolar: false, amount: 0.5, min: 0.0, max: 1.0 }
    }
}

impl LfoMap {
    /// Normalised value for base `base` and LFO output `out` (−1..1).
    pub fn apply(&self, base: f32, out: f32) -> f32 {
        let u = (out + 1.0) * 0.5;
        let v = if self.remote {
            self.min + (self.max - self.min) * u
        } else if self.unipolar {
            base + self.amount * u
        } else {
            base + self.amount * out * 0.5
        };
        v.clamp(0.0, 1.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LfoParams {
    pub wave: LfoWave,
    /// −1..1 (−100 %..100 %).
    pub shape: f32,
    /// 0 = Off, else 2..24.
    pub steps: u8,
    /// 0..1.
    pub jitter: f32,
    pub smooth: f32,
    /// Hz (Time Mode Hz).
    pub rate: f32,
    /// Index into [`LFO_SYNC`] (Time Mode sync).
    pub sync_rate: u8,
    pub sync: bool,
    pub x10: bool,
    /// 0..1.
    pub depth: f32,
    /// −1..1.
    pub offset: f32,
    /// 0..1 of a cycle.
    pub phase: f32,
    pub hold: bool,
    /// Bumped by the R button (the engine restarts at `phase`).
    pub retrig: u32,
    pub maps: [LfoMap; LFO_MAPS],
    /// Multimap view.
    pub multimap: bool,
    /// Map button armed (UI only): the next clicked parameter is mapped.
    #[serde(skip)]
    pub arm: Option<u8>,
}

impl Default for LfoParams {
    fn default() -> Self {
        Self {
            wave: LfoWave::Sine,
            shape: 0.0,
            steps: 0,
            jitter: 0.0,
            smooth: 0.0,
            rate: 1.0,
            sync_rate: 8,
            sync: false,
            x10: false,
            depth: 1.0,
            offset: 0.0,
            phase: 0.0,
            hold: false,
            retrig: 0,
            maps: [LfoMap::default(); LFO_MAPS],
            multimap: false,
            arm: None,
        }
    }
}

impl LfoParams {
    /// Cycles per second at `tempo` BPM.
    pub fn hz(&self, tempo: f64) -> f64 {
        if self.sync {
            let beats = LFO_SYNC[(self.sync_rate as usize).min(LFO_SYNC.len() - 1)].0;
            tempo / 60.0 / beats
        } else {
            (self.rate.clamp(LFO_RATE.0, LFO_RATE.1) * if self.x10 { 10.0 } else { 1.0 }) as f64
        }
    }

    /// Deterministic waveform value (−1..1) at cycle phase `ph` (0..1),
    /// before Steps; random waveforms return 0 (the state supplies them).
    pub fn shape_at(&self, ph: f64) -> f32 {
        let ph = ph.rem_euclid(1.0);
        // Shape skews the cycle: the first half takes `w` of it.
        let w = 0.5 + 0.49 * self.shape.clamp(-1.0, 1.0) as f64;
        let p = if !self.wave.shapeable() || self.wave == LfoWave::Square {
            ph
        } else if ph < w {
            0.5 * ph / w
        } else {
            0.5 + 0.5 * (ph - w) / (1.0 - w)
        };
        let v = match self.wave {
            LfoWave::Sine => (std::f64::consts::TAU * p).sin(),
            LfoWave::Up => 2.0 * p - 1.0,
            LfoWave::Down => 1.0 - 2.0 * p,
            LfoWave::Triangle => {
                if p < 0.25 {
                    4.0 * p
                } else if p < 0.75 {
                    2.0 - 4.0 * p
                } else {
                    4.0 * p - 4.0
                }
            }
            // Shape = pulse width.
            LfoWave::Square => {
                if ph < w {
                    1.0
                } else {
                    -1.0
                }
            }
            _ => 0.0,
        };
        v as f32
    }
}

#[derive(Debug, Clone)]
pub struct LfoState {
    /// Cycle position (unwrapped cycles).
    pos: f64,
    retrig: Option<u32>,
    rng: u64,
    /// Random waveforms: value of the current / next cycle.
    cur: f32,
    next: f32,
    cycle: i64,
    stray: f32,
    jit: f32,
    smooth: f32,
    held: Option<f32>,
    /// Last output (−1..1).
    pub out: f32,
    hop: usize,
    tap: Option<Arc<DevTap>>,
}

impl LfoState {
    pub fn new(id: Option<u64>) -> Self {
        Self {
            pos: 0.0,
            retrig: None,
            rng: 0x9e37_79b9_7f4a_7c15 ^ id.unwrap_or(1),
            cur: 0.0,
            next: 0.0,
            cycle: i64::MIN,
            stray: 0.0,
            jit: 0.0,
            smooth: 0.0,
            held: None,
            out: 0.0,
            hop: 0,
            tap: id.map(dev_tap),
        }
    }

    fn rand(&mut self) -> f32 {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        let x = self.rng.wrapping_mul(0x2545_f491_4f6c_dd1d);
        ((x >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
    }

    /// Advance by `n` samples and return the output (−1..1). `beat` = song
    /// position while playing (synced rates lock to it).
    pub fn tick(&mut self, p: &LfoParams, n: usize, sr: f32, tempo: f64, beat: Option<f64>) -> f32 {
        if self.retrig != Some(p.retrig) {
            if self.retrig.is_some() {
                self.pos = 0.0;
            }
            self.retrig = Some(p.retrig);
        }
        let hz = p.hz(tempo);
        let dt = n as f64 / sr as f64;
        if p.hold {
            if self.held.is_none() {
                self.held = Some(self.out);
            }
        } else {
            self.held = None;
            match (p.sync, beat) {
                (true, Some(b)) => {
                    let beats = LFO_SYNC[(p.sync_rate as usize).min(LFO_SYNC.len() - 1)].0;
                    self.pos = b / beats;
                }
                _ => self.pos += hz * dt,
            }
        }
        if let Some(h) = self.held {
            self.publish(h, n, sr);
            return h;
        }
        let ph = self.pos + p.phase.clamp(0.0, 1.0) as f64;
        let cyc = ph.floor() as i64;
        let frac = ph - ph.floor();
        if cyc != self.cycle {
            // New cycle: random targets (one step per cycle).
            if self.cycle == i64::MIN {
                self.cur = self.rand();
            } else {
                self.cur = self.next;
            }
            self.next = self.rand();
            self.cycle = cyc;
        }
        let steps = if p.wave.steppable() && p.steps >= 2 { p.steps.min(LFO_STEPS_MAX) as f64 } else { 0.0 };
        let q = if steps > 0.0 { (frac * steps).floor() / steps } else { frac };
        let mut v = match p.wave {
            LfoWave::Random => self.cur,
            LfoWave::Bin => self.cur.signum(),
            LfoWave::Glider => {
                let t = q as f32;
                self.cur + (self.next - self.cur) * t
            }
            LfoWave::Stray => {
                // Random walk, pulled back toward the centre.
                let step = (hz * dt).min(1.0) as f32;
                let r = self.rand();
                self.stray = (self.stray + r * 1.5 * step.sqrt() - self.stray * step * 0.5).clamp(-1.0, 1.0);
                self.stray
            }
            _ => p.shape_at(q),
        };
        if p.jitter > 0.0 {
            // A new random offset per block, scaled by Jitter.
            let r = self.rand();
            self.jit = r * p.jitter.clamp(0.0, 1.0);
            v = (v + self.jit).clamp(-1.0, 1.0);
        }
        // Smooth: one-pole, up to a quarter cycle (≥ 1 ms).
        let s = p.smooth.clamp(0.0, 1.0);
        if s > 0.0 {
            let tau = (s as f64 * s as f64 * 0.25 / hz.max(1e-3)).max(0.001);
            let k = (-dt / tau).exp() as f32;
            self.smooth = v + (self.smooth - v) * k;
            v = self.smooth;
        } else {
            self.smooth = v;
        }
        let out = (v * p.depth.clamp(0.0, 1.0) + p.offset.clamp(-1.0, 1.0)).clamp(-1.0, 1.0);
        self.publish(out, n, sr);
        out
    }

    fn publish(&mut self, out: f32, n: usize, sr: f32) {
        let prev = self.out;
        self.out = out;
        if let Some(t) = &self.tap {
            t.set(0, out);
            let hop = ((HOP as f32 * sr / 48_000.0) as usize).max(1);
            let start = self.hop;
            self.hop += n;
            if self.hop >= hop {
                t.set_rate(sr / hop as f32);
                // One value per block: the frames inside the block are
                // interpolated from the previous value (no staircase scope).
                let mut k = hop - start.min(hop);
                while self.hop >= hop {
                    let f = (k as f32 / n.max(1) as f32).min(1.0);
                    t.push([prev + (out - prev) * f, 0.0, 0.0, 0.0]);
                    self.hop -= hop;
                    k += hop;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waveforms_span_and_shape() {
        let p = LfoParams::default();
        assert!(p.shape_at(0.25) > 0.999 && p.shape_at(0.75) < -0.999);
        let up = LfoParams { wave: LfoWave::Up, ..p };
        assert!((up.shape_at(0.0) + 1.0).abs() < 1e-6 && up.shape_at(0.999) > 0.99);
        let sq = LfoParams { wave: LfoWave::Square, shape: 0.5, ..p };
        // Pulse width ~74 %.
        assert_eq!(sq.shape_at(0.7), 1.0);
        assert_eq!(sq.shape_at(0.8), -1.0);
        let tri = LfoParams { wave: LfoWave::Triangle, ..p };
        assert!((tri.shape_at(0.25) - 1.0).abs() < 1e-6 && (tri.shape_at(0.5)).abs() < 1e-6);
    }

    #[test]
    fn rate_steps_depth_offset_hold() {
        let p = LfoParams { rate: 1.0, ..Default::default() };
        let mut st = LfoState::new(None);
        // A quarter second at 1 Hz → sine peak.
        let v = st.tick(&p, 12_000, 48_000.0, 120.0, None);
        assert!(v > 0.999, "{v}");
        let p2 = LfoParams { depth: 0.5, offset: 0.25, ..p };
        let mut st = LfoState::new(None);
        let v = st.tick(&p2, 12_000, 48_000.0, 120.0, None);
        assert!((v - 0.75).abs() < 1e-3);
        // Steps: staircase.
        let ps = LfoParams { wave: LfoWave::Up, steps: 4, ..p };
        let mut st = LfoState::new(None);
        let v = st.tick(&ps, 14_000, 48_000.0, 120.0, None); // phase 0.29 → step 0.25
        assert!((v - (-0.5)).abs() < 1e-3, "{v}");
        // Hold freezes.
        let ph = LfoParams { hold: true, ..ps };
        let a = st.tick(&ph, 4000, 48_000.0, 120.0, None);
        let b = st.tick(&ph, 4000, 48_000.0, 120.0, None);
        assert_eq!(a, b);
        // Synced to the song position: 1/4 at beat 0.25 → quarter cycle.
        let pq = LfoParams { sync: true, sync_rate: 8, ..Default::default() };
        let mut st = LfoState::new(None);
        assert!(st.tick(&pq, 64, 48_000.0, 120.0, Some(0.25)) > 0.999);
    }

    #[test]
    fn random_waves_stay_in_range() {
        for w in [LfoWave::Random, LfoWave::Bin, LfoWave::Stray, LfoWave::Glider] {
            let p = LfoParams { wave: w, rate: 7.0, jitter: 0.3, smooth: 0.2, ..Default::default() };
            let mut st = LfoState::new(Some(3));
            for _ in 0..2000 {
                let v = st.tick(&p, 64, 48_000.0, 120.0, None);
                assert!((-1.0..=1.0).contains(&v));
            }
        }
    }

    #[test]
    fn map_modes() {
        let m = LfoMap::default();
        assert!((m.apply(0.5, 1.0) - 0.75).abs() < 1e-6, "bipolar ±amount/2");
        let u = LfoMap { unipolar: true, ..m };
        assert!((u.apply(0.2, 1.0) - 0.7).abs() < 1e-6 && (u.apply(0.2, -1.0) - 0.2).abs() < 1e-6);
        let r = LfoMap { remote: true, min: 0.2, max: 0.6, ..m };
        assert!((r.apply(0.9, 0.0) - 0.4).abs() < 1e-6);
    }
}
