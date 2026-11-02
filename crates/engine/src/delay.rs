//! Delay (Live 12, manual 29.9): two delay lines (synced to 16th-note
//! multiples or free 1 ms–5 s, each with an offset), Stereo Link, Feedback,
//! Freeze, a band-pass filter in the feedback path (Frequency/Width), an LFO
//! modulating delay time and filter frequency (seven waves + Morph, six rate
//! modes), Repitch/Fade/Jump smoothing, Ping Pong and Dry/Wet with the
//! optional Equal-Loudness law.

pub use crate::dsp::lfo_wave;
use crate::dsp::{LFO_WAVES, Rng, read};
use crate::echo::ECHO_MOD_DIVS;
use crate::eq8::{Coefs, Rbj, Tdf2, rbj};
use serde::{Deserialize, Serialize};

pub const DLY_SIXTEENTHS: [u8; 8] = [1, 2, 3, 4, 5, 6, 8, 16];
pub const DLY_MS: (f32, f32) = (1.0, 5000.0);
pub const DLY_OFFSET: f32 = 0.333;
pub const DLY_FREQ: (f32, f32) = (50.0, 18_000.0);
pub const DLY_WIDTH: (f32, f32) = (0.5, 9.0);
pub const DLY_LFO_HZ: (f32, f32) = (0.01, 40.0);
pub const DLY_LFO_MS: (f32, f32) = (10.0, 10_000.0);
pub const DLY_LFO_MODES: [&str; 6] = ["Rate", "Time", "Synced", "Triplet", "Dotted", "Sixteenth"];
pub const DLY_SMOOTH: [&str; 3] = ["Repitch", "Fade", "Jump"];
pub const DLY_WAVES: [&str; 7] = LFO_WAVES;
/// Band-edge factor: corners at f·2^(±k·Width).
const EDGE_K: f32 = 0.48;
/// Delay-time deviation at 100 % LFO Delay.
const LFO_TIME_DEPTH: f32 = 0.25;
/// Filter deviation (octaves) at 100 % LFO Filter.
const LFO_FILTER_OCT: f32 = 4.0;
const BUF: usize = 1 << 19;
const FADE_S: f32 = 0.05;
const REPITCH_S: f32 = 0.12;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DelayParams {
    pub sync: [bool; 2],
    /// Index into `DLY_SIXTEENTHS`.
    pub six: [u8; 2],
    pub ms: [f32; 2],
    /// −0.333..0.333.
    pub offset: [f32; 2],
    pub link: bool,
    pub feedback: f32,
    pub freeze: bool,
    pub filter: bool,
    pub freq: f32,
    pub width: f32,
    pub lfo_shown: bool,
    pub lfo_delay: f32,
    pub lfo_filter: f32,
    pub lfo_mode: u8,
    pub lfo_hz: f32,
    pub lfo_ms: f32,
    /// Index into `ECHO_MOD_DIVS` (Synced/Triplet/Dotted).
    pub lfo_div: u8,
    /// 1..16 (Sixteenth mode).
    pub lfo_six: u8,
    pub wave: u8,
    pub morph: f32,
    pub smooth: u8,
    pub ping_pong: bool,
    pub dry_wet: f32,
    pub hq: bool,
    pub eq_loud: bool,
}

impl Default for DelayParams {
    fn default() -> Self {
        Self {
            sync: [true; 2],
            six: [2; 2],
            ms: [100.0; 2],
            offset: [0.0; 2],
            link: true,
            feedback: 0.5,
            freeze: false,
            filter: true,
            freq: 1000.0,
            width: 8.0,
            lfo_shown: false,
            lfo_delay: 0.0,
            lfo_filter: 0.0,
            lfo_mode: 0,
            lfo_hz: 0.5,
            lfo_ms: 1000.0,
            lfo_div: 4,
            lfo_six: 4,
            wave: 1,
            morph: 0.0,
            smooth: 0,
            ping_pong: false,
            dry_wet: 0.5,
            hq: true,
            eq_loud: false,
        }
    }
}

impl DelayParams {
    /// Side whose time settings apply to `s` (Link → left).
    #[inline]
    pub fn side(&self, s: usize) -> usize {
        if self.link { 0 } else { s.min(1) }
    }

    pub fn base_secs(&self, s: usize, tempo: f64) -> f32 {
        let k = self.side(s);
        if self.sync[k] {
            let n = DLY_SIXTEENTHS[(self.six[k] as usize).min(7)] as f32;
            n * 15.0 / tempo.max(1.0) as f32
        } else {
            self.ms[k].clamp(DLY_MS.0, DLY_MS.1) * 0.001
        }
    }

    pub fn secs(&self, s: usize, tempo: f64) -> f32 {
        let k = self.side(s);
        let off = if self.sync[k] { self.offset[k].clamp(-DLY_OFFSET, DLY_OFFSET) } else { 0.0 };
        (self.base_secs(s, tempo) * (1.0 + off)).max(0.0005)
    }

    pub fn ms_label(ms: f32) -> String {
        if ms >= 1000.0 {
            format!("{:.2} s", ms / 1000.0)
        } else if ms >= 100.0 {
            format!("{} ms", ms.round())
        } else {
            format!("{ms:.1} ms")
        }
    }

    pub fn hz_label(f: f32) -> String {
        if f >= 1000.0 { format!("{:.2} kHz", f / 1000.0) } else { format!("{} Hz", f.round()) }
    }

    pub fn lfo_rate_hz(&self, tempo: f64) -> f32 {
        let beat = tempo.max(1.0) as f32 / 60.0;
        let div = |m: f32| {
            let (a, b) = ECHO_MOD_DIVS[(self.lfo_div as usize).min(ECHO_MOD_DIVS.len() - 1)];
            beat / (4.0 * a as f32 / b as f32 * m)
        };
        match self.lfo_mode {
            1 => 1000.0 / self.lfo_ms.clamp(DLY_LFO_MS.0, DLY_LFO_MS.1),
            2 => div(1.0),
            3 => div(2.0 / 3.0),
            4 => div(1.5),
            5 => beat / (self.lfo_six.clamp(1, 16) as f32 / 4.0),
            _ => self.lfo_hz.clamp(DLY_LFO_HZ.0, DLY_LFO_HZ.1),
        }
    }

    pub fn lfo_rate_label(&self) -> String {
        let note = || crate::echo::EchoParams::mod_div_label(self.lfo_div as usize);
        match self.lfo_mode {
            1 => Self::ms_label(self.lfo_ms),
            2 => note(),
            3 => note() + "T",
            4 => note() + "D",
            5 => format!("{}/16", self.lfo_six.clamp(1, 16)),
            _ => format!("{:.2} Hz", self.lfo_hz),
        }
    }

    /// (high-pass, low-pass) corners of the band-pass at `freq`.
    pub fn band(&self, freq: f32) -> (f32, f32) {
        let k = 2f32.powf(EDGE_K * self.width.clamp(DLY_WIDTH.0, DLY_WIDTH.1));
        (freq / k, freq * k)
    }

    fn coefs(&self, freq: f32, sr: f32) -> [Coefs; 2] {
        let (h, l) = self.band(freq);
        [
            rbj(Rbj::Hp, h.clamp(10.0, 0.45 * sr) as f64, sr as f64, 0.707, 0.0),
            rbj(Rbj::Lp, l.clamp(20.0, 0.49 * sr) as f64, sr as f64, 0.707, 0.0),
        ]
    }

    /// Filter magnitude (dB) at `f` (display).
    pub fn filter_db(&self, f: f32, sr: f32) -> f32 {
        let w = std::f64::consts::TAU * (f / sr).min(0.4999) as f64;
        let m: f64 = self.coefs(self.freq.clamp(DLY_FREQ.0, DLY_FREQ.1), sr).iter().map(|c| c.mag2(w)).product();
        (10.0 * m.max(1e-30).log10()) as f32
    }
}

#[derive(Debug, Clone)]
pub struct DelayState {
    buf: Box<[Vec<f32>; 2]>,
    w: usize,
    /// Current (smoothed / faded-from) and target tap per side (samples).
    cur: [f32; 2],
    next: [f32; 2],
    fade: [f32; 2],
    filt: [[Tdf2; 2]; 2],
    co: [Coefs; 2],
    co_key: Option<(u32, u32, u32)>,
    ph: f32,
    held: f32,
    wander: f32,
    rng: Rng,
    started: bool,
}

impl Default for DelayState {
    fn default() -> Self {
        Self {
            buf: Box::new([vec![0.0; BUF], vec![0.0; BUF]]),
            w: 0,
            cur: [0.0; 2],
            next: [0.0; 2],
            fade: [1.0; 2],
            filt: Default::default(),
            co: [Coefs::default(); 2],
            co_key: None,
            ph: 0.0,
            held: 0.0,
            wander: 0.0,
            rng: Rng(0x2545_f491),
            started: false,
        }
    }
}

impl DelayState {
    pub fn reset(&mut self) {
        for b in self.buf.iter_mut() {
            b.fill(0.0);
        }
        self.filt = Default::default();
        self.fade = [1.0; 2];
        self.started = false;
        self.ph = 0.0;
    }

    #[allow(clippy::needless_range_loop)]
    pub fn process(&mut self, p: &DelayParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let n = l.len().min(r.len());
        let (tempo, _) = crate::modfx::clock();
        let base = [p.secs(0, tempo) * sr, p.secs(1, tempo) * sr];
        let lim = (BUF - 8) as f32;
        if !self.started {
            self.cur = [base[0].min(lim), base[1].min(lim)];
            self.next = self.cur;
            self.started = true;
        }
        let dph = p.lfo_rate_hz(tempo) / sr;
        let fb = if p.freeze { 1.0 } else { p.feedback.clamp(0.0, 1.0) };
        let w = p.dry_wet.clamp(0.0, 1.0);
        let (dry_g, wet_g) = if p.eq_loud {
            let a = w * std::f32::consts::FRAC_PI_2;
            (a.cos(), a.sin())
        } else {
            (1.0 - w, w)
        };
        let slew = 1.0 - (-1.0 / (REPITCH_S * sr)).exp();
        let fade_step = 1.0 / (FADE_S * sr);
        let wander_a = (dph * 4.0).min(1.0);
        let freq = p.freq.clamp(DLY_FREQ.0, DLY_FREQ.1);
        for i in 0..n {
            // LFO.
            let prev = self.ph;
            self.ph = (self.ph + dph).fract();
            if self.ph < prev {
                self.held = self.rng.bi();
            }
            self.wander += (self.held - self.wander) * wander_a;
            let held = if p.wave == 6 { self.wander } else { self.held };
            let lfo = lfo_wave(p.wave, self.ph, p.morph, held);
            // Filter coefficients every 32 samples.
            if p.filter && i % 32 == 0 {
                let f = (freq * 2f32.powf(LFO_FILTER_OCT * p.lfo_filter.clamp(0.0, 1.0) * lfo)).clamp(20.0, 0.45 * sr);
                let key = (f.to_bits(), p.width.to_bits(), sr.to_bits());
                if self.co_key != Some(key) {
                    self.co = p.coefs(f, sr);
                    self.co_key = Some(key);
                }
            }
            let mut y = [0.0f32; 2];
            for s in 0..2 {
                let target = (base[s] * (1.0 + LFO_TIME_DEPTH * p.lfo_delay.clamp(0.0, 1.0) * lfo)).clamp(1.0, lim);
                let buf = &self.buf[s];
                y[s] = match p.smooth {
                    1 => {
                        if self.fade[s] >= 1.0 && (target - self.next[s]).abs() > 0.5 {
                            self.cur[s] = self.next[s];
                            self.next[s] = target;
                            self.fade[s] = 0.0;
                        }
                        let a = read(buf, self.w, self.cur[s], p.hq);
                        let b = read(buf, self.w, self.next[s], p.hq);
                        let t = self.fade[s];
                        self.fade[s] = (t + fade_step).min(1.0);
                        a + (b - a) * t
                    }
                    2 => {
                        self.cur[s] = target;
                        self.next[s] = target;
                        read(buf, self.w, target, p.hq)
                    }
                    _ => {
                        self.cur[s] += (target - self.cur[s]) * slew;
                        self.next[s] = self.cur[s];
                        read(buf, self.w, self.cur[s], true)
                    }
                };
            }
            let mut wet = y;
            if p.filter {
                for s in 0..2 {
                    let mut v = wet[s] as f64;
                    for k in 0..2 {
                        v = self.filt[s][k].tick(&self.co[k], v);
                    }
                    wet[s] = v as f32;
                }
            }
            let (il, ir) = (l[i], r[i]);
            let loop_in = if p.freeze { y } else { wet };
            let (wl, wr) = if p.freeze {
                (loop_in[0], loop_in[1])
            } else if p.ping_pong {
                (0.5 * (il + ir) + fb * loop_in[1], fb * loop_in[0])
            } else {
                (il + fb * loop_in[0], ir + fb * loop_in[1])
            };
            // Gentle safety limit inside the loop.
            self.buf[0][self.w] = if wl.is_finite() { wl.clamp(-8.0, 8.0) } else { 0.0 };
            self.buf[1][self.w] = if wr.is_finite() { wr.clamp(-8.0, 8.0) } else { 0.0 };
            self.w = (self.w + 1) % BUF;
            l[i] = il * dry_g + wet[0] * wet_g;
            r[i] = ir * dry_g + wet[1] * wet_g;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impulse_arrives_at_delay_time() {
        let p = DelayParams { sync: [false; 2], ms: [10.0; 2], filter: false, feedback: 0.0, dry_wet: 1.0, ..Default::default() };
        let mut s = DelayState::default();
        let mut l = vec![0.0f32; 1024];
        l[0] = 1.0;
        let mut r = l.clone();
        s.process(&p, &mut l, &mut r, 48_000.0);
        let pk = l.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap().0;
        assert!((pk as i32 - 480).abs() <= 1, "{pk}");
    }

    #[test]
    fn filter_shape_and_labels() {
        let p = DelayParams::default();
        assert!(p.filter_db(1000.0, 48_000.0).abs() < 0.5);
        assert!(p.filter_db(40.0, 48_000.0) < -6.0);
        assert_eq!(p.lfo_rate_label(), "0.50 Hz");
        assert!((p.secs(0, 120.0) - 0.375).abs() < 1e-6);
    }

    #[test]
    fn ping_pong_and_freeze_stay_finite() {
        for (pp, fr) in [(true, false), (false, true)] {
            let p =
                DelayParams { ping_pong: pp, freeze: fr, feedback: 1.0, lfo_delay: 1.0, lfo_filter: 1.0, smooth: 1, ..Default::default() };
            let mut s = DelayState::default();
            for _ in 0..20 {
                let mut l: Vec<f32> = (0..512).map(|i| (i as f32 * 0.1).sin()).collect();
                let mut r = l.clone();
                s.process(&p, &mut l, &mut r, 48_000.0);
                assert!(l.iter().chain(&r).all(|x| x.is_finite()));
            }
        }
    }
}
