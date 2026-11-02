//! Resonators (Live 12, manual 29.28): input filter (LP / HP / BP / Notch),
//! five tuned comb resonators (I: L+R, II / IV: left, III / V: right) with a
//! damping filter (Color) in the loop, Mode A (full-period comb, all
//! harmonics) or B (half-period inverted comb, odd harmonics), Decay with
//! optional constant decay time across pitch, Width, Output and Dry/Wet.
//! Scale awareness snaps Note to the clip scale and makes Pitch scale degrees.

use crate::dsp::{Svf, svf_g};
use serde::{Deserialize, Serialize};

pub const RES_FREQ: (f32, f32) = (50.0, 10_000.0);
pub const RES_NOTE: (f32, f32) = (12.0, 84.0);
pub const RES_PITCH: f32 = 24.0;
pub const RES_FINE: f32 = 50.0;
pub const RES_GAIN: (f32, f32) = (-36.0, 6.0);
pub const RES_OUT: f32 = 15.0;
pub const RES_FILTERS: [&str; 4] = ["LP", "HP", "BP", "Notch"];
/// Delay line per resonator (≥ one period of 4 Hz at 192 kHz).
const LINE: usize = 1 << 16;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResonatorsParams {
    pub filter_on: bool,
    /// Index into [`RES_FILTERS`].
    pub filter: u8,
    pub freq: f32,
    pub mode_b: bool,
    /// 0..1.
    pub decay: f32,
    /// Constant decay time across pitch.
    pub constant: bool,
    /// 0..1 (brightness of the loop filter).
    pub color: f32,
    pub on: [bool; 5],
    /// Resonator I note (MIDI, integer).
    pub note: f32,
    /// Resonators II–V transposition (semitones / scale degrees; [0] unused).
    pub pitch: [f32; 5],
    /// Cents.
    pub fine: [f32; 5],
    /// dB.
    pub gain: [f32; 5],
    /// 0..1 stereo width of II–V.
    pub width: f32,
    /// dB.
    pub output: f32,
    pub dry_wet: f32,
    pub scale_aware: bool,
}

impl Default for ResonatorsParams {
    fn default() -> Self {
        Self {
            filter_on: true,
            filter: 2,
            freq: 500.0,
            mode_b: false,
            decay: 0.5,
            constant: true,
            color: 0.9,
            on: [true; 5],
            note: 48.0,
            pitch: [0.0; 5],
            fine: [0.0; 5],
            gain: [0.0; 5],
            width: 1.0,
            output: 0.0,
            dry_wet: 0.5,
            scale_aware: false,
        }
    }
}

/// Live note name (C3 = 60).
pub fn note_name(n: i32) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", NAMES[n.rem_euclid(12) as usize], n.div_euclid(12) - 2)
}

/// `n` snapped to the nearest pitch of the scale (pitch-class `mask`, C = bit 0).
pub fn snap_to_scale(n: i32, mask: u16) -> i32 {
    if mask & 0xFFF == 0 {
        return n;
    }
    let inside = |p: i32| mask >> p.rem_euclid(12) & 1 != 0;
    for d in 0..12 {
        if inside(n - d) {
            return n - d;
        }
        if inside(n + d) {
            return n + d;
        }
    }
    n
}

/// `n` (in the scale) moved by `steps` scale degrees.
pub fn degree_shift(n: i32, steps: i32, mask: u16) -> i32 {
    if mask & 0xFFF == 0 {
        return n + steps;
    }
    let inside = |p: i32| mask >> p.rem_euclid(12) & 1 != 0;
    let mut p = n;
    let dir = steps.signum();
    for _ in 0..steps.abs() {
        p += dir;
        while !inside(p) {
            p += dir;
        }
    }
    p
}

impl ResonatorsParams {
    /// Resonator `i` pitch (MIDI, fractional) with the current scale.
    pub fn pitch_of(&self, i: usize, scale: Option<(u8, u16)>) -> f32 {
        let base = self.note.round() as i32;
        let st = self.pitch[i].round() as i32;
        let n = match scale.filter(|_| self.scale_aware) {
            Some((_, mask)) => {
                let b = snap_to_scale(base, mask);
                if i == 0 { b } else { degree_shift(b, st, mask) }
            }
            None => base + if i == 0 { 0 } else { st },
        };
        n as f32 + self.fine[i] / 100.0
    }
}

#[derive(Debug, Clone)]
struct Line {
    buf: Box<[f32]>,
    lp: f32,
}

impl Default for Line {
    fn default() -> Self {
        Self { buf: vec![0.0; LINE].into_boxed_slice(), lp: 0.0 }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ResonatorsState {
    lines: [Line; 5],
    w: usize,
    filt: [Svf; 2],
}

impl ResonatorsState {
    pub fn reset(&mut self) {
        for l in self.lines.iter_mut() {
            l.buf.fill(0.0);
            l.lp = 0.0;
        }
        self.filt = Default::default();
    }

    #[allow(clippy::needless_range_loop)]
    pub fn process(&mut self, p: &ResonatorsParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let n = l.len().min(r.len());
        let scale = crate::autoshift::current_scale();
        // Decay time (s) of a 100 Hz tone: 5 ms .. 10 s.
        let t60 = 0.005 * 2000f32.powf(p.decay.clamp(0.0, 1.0));
        let damp = 0.08 + 0.92 * p.color.clamp(0.0, 1.0);
        let mut cfg = [(0.0f32, 0.0f32, 0.0f32); 5];
        for i in 0..5 {
            if !p.on[i] {
                continue;
            }
            let f = 440.0 * 2f32.powf((p.pitch_of(i, scale) - 69.0) / 12.0);
            let period = (sr / f.max(1.0)) * if p.mode_b { 0.5 } else { 1.0 };
            let d = period.clamp(2.0, (LINE - 4) as f32);
            let loop_s = if p.constant { d / sr } else { 0.01 };
            let g = 10f32.powf(-3.0 * loop_s / t60).min(0.9995);
            let g = if p.mode_b { -g } else { g };
            let amp = 10f32.powf(p.gain[i] / 20.0) * (1.0 - g * g).max(1e-4).sqrt();
            cfg[i] = (d, g, amp);
        }
        let gf = svf_g(p.freq.clamp(RES_FREQ.0, RES_FREQ.1), sr);
        let out = 10f32.powf(p.output / 20.0);
        let wet = p.dry_wet.clamp(0.0, 1.0);
        let width = p.width.clamp(0.0, 1.0);
        for s in 0..n {
            let mut x = [l[s], r[s]];
            if p.filter_on {
                for c in 0..2 {
                    let (lp, bp, hp) = self.filt[c].tick(x[c], gf, 1.0);
                    x[c] = match p.filter {
                        0 => lp,
                        1 => hp,
                        2 => bp,
                        _ => lp + hp,
                    };
                }
            }
            let ins = [(x[0] + x[1]) * 0.5, x[0], x[1], x[0], x[1]];
            let mut y = [0.0f32; 5];
            let w = self.w;
            for i in 0..5 {
                let (d, g, amp) = cfg[i];
                if d == 0.0 {
                    continue;
                }
                let line = &mut self.lines[i];
                let fb = crate::dsp::read(&line.buf, w, d, false);
                line.lp += damp * (fb - line.lp);
                let v = ins[i] * amp + g * line.lp;
                let v = if v.is_finite() { v.clamp(-8.0, 8.0) } else { 0.0 };
                line.buf[w] = v;
                y[i] = v;
            }
            self.w = (w + 1) & (LINE - 1);
            let (sl, sr2) = (y[1] + y[3], y[2] + y[4]);
            let (m, sd) = ((sl + sr2) * 0.5, (sl - sr2) * 0.5 * width);
            let ol = (y[0] + m + sd) * out;
            let or = (y[0] + m - sd) * out;
            l[s] += (ol - l[s]) * wet;
            r[s] += (or - r[s]) * wet;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rings_at_the_note() {
        let p =
            ResonatorsParams { on: [true, false, false, false, false], filter_on: false, dry_wet: 1.0, decay: 0.8, ..Default::default() };
        let mut s = ResonatorsState::default();
        let sr = 48_000.0;
        let mut l = vec![0.0f32; 48_000];
        l[0] = 1.0;
        let mut r = l.clone();
        s.process(&p, &mut l, &mut r, sr);
        // C2 (MIDI 48) = 130.8 Hz: count zero crossings over the second half.
        let tail = &l[24_000..];
        let zc = tail.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        assert!((62..=68).contains(&zc), "{zc}");
        assert!(l.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn scale_helpers() {
        // C major.
        let maj = 0b1010_1011_0101u16;
        assert_eq!(snap_to_scale(61, maj), 60);
        assert_eq!(degree_shift(60, 2, maj), 64);
        assert_eq!(degree_shift(60, -1, maj), 59);
        assert_eq!(note_name(48), "C2");
        assert_eq!(note_name(31), "G0");
    }
}
