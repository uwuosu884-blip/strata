//! Spectral Time (Live 12, manual 29.32): a spectral Freezer (Manual or
//! Retrigger on Onsets / Sync, Crossfade or Envelope fades, stacked layers)
//! and a spectral Delay (Time in ms / notes / 16ths, Feedback, frequency
//! Shift per repeat, Stereo spread, Tilt and Spray of per-bin delay times,
//! Mask, Mix), in either order; Resolution (FFT size), Input Send, Dry/Wet.

use crate::dsp::Rng;
use crate::stft::{MAX_BINS, SG_ROWS, SpecGram, Stft};
use realfft::num_complex::Complex;
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;
use std::sync::Arc;

pub const ST_TIME: (f32, f32) = (10.0, 10_000.0);
pub const ST_FADE: (f32, f32) = (2.0, 4000.0);
pub const ST_SHIFT: f32 = 1000.0;
pub const ST_TILT: f32 = 500.0;
pub const ST_SPRAY: f32 = 500.0;
pub const ST_SEND: (f32, f32) = (-48.0, 0.0);
pub const ST_STEPS: (f32, f32) = (1.0, 16.0);
pub const ST_MODES: [&str; 5] = ["Time", "Notes", "16th", "16th T", "16th D"];
pub const ST_RES: [&str; 3] = ["Low", "Mid", "High"];
/// Note divisions (labels, beats) for Notes mode and the Sync interval.
pub const ST_DIVS: [(&str, f32); 10] = [
    ("1/32", 0.125),
    ("1/16", 0.25),
    ("1/8", 0.5),
    ("1/4", 1.0),
    ("1/2", 2.0),
    ("1", 4.0),
    ("2", 8.0),
    ("4", 16.0),
    ("8", 32.0),
    ("16", 64.0),
];
/// History (complex values per channel): 10 s of frames at 48 kHz.
const HIST: usize = 2 * 48_000 * 10 + 4 * MAX_BINS;
const LAYERS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpecTimeParams {
    pub frz_on: bool,
    pub retrigger: bool,
    /// Retrigger source: Sync (else Onsets).
    pub sync: bool,
    pub sensitivity: f32,
    /// Index into [`ST_DIVS`].
    pub interval: u8,
    /// Fade type Envelope (else Crossfade).
    pub envelope: bool,
    /// 0..1.
    pub xfade: f32,
    /// ms.
    pub fade_in: f32,
    pub fade_out: f32,
    pub freeze: bool,
    pub dly_on: bool,
    /// ms.
    pub time: f32,
    /// Index into [`ST_MODES`].
    pub time_mode: u8,
    /// Index into [`ST_DIVS`] (Notes).
    pub div: u8,
    /// 16ths (16th modes).
    pub steps: f32,
    pub feedback: f32,
    /// Hz per repeat.
    pub shift: f32,
    pub stereo: f32,
    pub mix: f32,
    /// ms (high bins later when positive).
    pub tilt: f32,
    /// ms.
    pub spray: f32,
    /// −1..1 (positive: delay only the highs).
    pub mask: f32,
    /// Index into [`ST_RES`].
    pub resolution: u8,
    /// dB.
    pub send: f32,
    /// Delay before the Freezer.
    pub dly_first: bool,
    pub dry_wet: f32,
    pub display: bool,
}

impl Default for SpecTimeParams {
    fn default() -> Self {
        Self {
            frz_on: true,
            retrigger: false,
            sync: false,
            sensitivity: 0.5,
            interval: 3,
            envelope: false,
            xfade: 0.5,
            fade_in: 2.0,
            fade_out: 40.0,
            freeze: false,
            dly_on: true,
            time: 250.0,
            time_mode: 0,
            div: 3,
            steps: 3.0,
            feedback: 0.0,
            shift: 0.0,
            stereo: 0.0,
            mix: 0.5,
            tilt: 0.0,
            spray: 0.0,
            mask: 0.0,
            resolution: 1,
            send: 0.0,
            dly_first: false,
            dry_wet: 0.5,
            display: true,
        }
    }
}

impl SpecTimeParams {
    /// Delay time (s) at `tempo`.
    pub fn secs(&self, tempo: f64) -> f32 {
        let beat = 60.0 / tempo.max(1.0) as f32;
        match self.time_mode {
            0 => self.time.clamp(ST_TIME.0, ST_TIME.1) * 0.001,
            1 => ST_DIVS[(self.div as usize).min(ST_DIVS.len() - 1)].1 * beat,
            m => {
                let k = [1.0, 2.0 / 3.0, 1.5][(m as usize - 2).min(2)];
                self.steps.round().clamp(ST_STEPS.0, ST_STEPS.1) * 0.25 * beat * k
            }
        }
    }

    pub fn time_label(&self) -> String {
        match self.time_mode {
            0 => {
                let t = self.time;
                if t >= 1000.0 { format!("{:.2} s", t / 1000.0) } else { format!("{t:.0} ms") }
            }
            1 => ST_DIVS[(self.div as usize).min(ST_DIVS.len() - 1)].0.to_string(),
            _ => format!("{}", self.steps.round() as i32),
        }
    }

    pub fn latency(&self) -> usize {
        crate::stft::SIZES[(self.resolution as usize).min(2)]
    }
}

#[derive(Debug, Clone)]
struct Layer {
    mag: [Box<[f32]>; 2],
    dphi: [Box<[f32]>; 2],
    phase: [Box<[f32]>; 2],
    env: f32,
    /// Fading in (else out / idle).
    rising: bool,
}

impl Default for Layer {
    fn default() -> Self {
        let z = || vec![0.0f32; MAX_BINS].into_boxed_slice();
        Self { mag: [z(), z()], dphi: [z(), z()], phase: [z(), z()], env: 0.0, rising: false }
    }
}

#[derive(Debug, Clone)]
pub struct SpecTimeState {
    stft: Stft,
    gram: Option<Arc<SpecGram>>,
    layers: [Layer; LAYERS],
    cur: usize,
    prev_phase: [Box<[f32]>; 2],
    prev_mag: Box<[f32]>,
    hist: [Box<[Complex<f32>]>; 2],
    hw: usize,
    tmp: [Box<[Complex<f32>]>; 2],
    was_frozen: bool,
    since: f32,
    beat_was: f64,
    rng: Rng,
    size: u8,
}

impl Default for SpecTimeState {
    fn default() -> Self {
        Self::new(None)
    }
}

impl SpecTimeState {
    pub fn new(id: Option<u64>) -> Self {
        let z = || vec![0.0f32; MAX_BINS].into_boxed_slice();
        let zc = |n: usize| vec![Complex::new(0.0f32, 0.0); n].into_boxed_slice();
        Self {
            stft: Stft::new(),
            gram: id.map(crate::stft::spec_gram),
            layers: Default::default(),
            cur: 0,
            prev_phase: [z(), z()],
            prev_mag: z(),
            hist: [zc(HIST), zc(HIST)],
            hw: 0,
            tmp: [zc(MAX_BINS), zc(MAX_BINS)],
            was_frozen: false,
            since: 0.0,
            beat_was: 0.0,
            rng: Rng(0x1234_5679),
            size: 1,
        }
    }

    /// Attach the spectrogram of device `id` (control thread).
    pub fn set_id(&mut self, id: u64) {
        self.gram = Some(crate::stft::spec_gram(id));
    }

    pub fn reset(&mut self) {
        self.stft.reset();
        for l in self.layers.iter_mut() {
            l.env = 0.0;
            l.rising = false;
        }
        for h in self.hist.iter_mut() {
            h.fill(Complex::new(0.0, 0.0));
        }
        self.hw = 0;
        self.was_frozen = false;
        self.since = 0.0;
    }

    #[allow(clippy::needless_range_loop)]
    pub fn process(&mut self, p: &SpecTimeParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let res = p.resolution.min(2);
        if res != self.size {
            self.size = res;
            self.stft.set_size(res as usize);
            self.reset();
        }
        let n = l.len().min(r.len());
        let (tempo, beat) = crate::modfx::clock();
        // Free-running beat while the transport is stopped.
        let beat = beat.unwrap_or(self.beat_was + n as f64 / sr as f64 * tempo / 60.0);
        let frozen = p.frz_on && p.freeze;
        // Sync retrigger: crossing an interval boundary in this block.
        let iv = ST_DIVS[(p.interval as usize).min(ST_DIVS.len() - 1)].1 as f64;
        let mut sync_hit = (beat / iv).floor() != (self.beat_was / iv).floor() && beat > self.beat_was;
        self.beat_was = beat;
        let send = if p.send <= ST_SEND.0 { 0.0 } else { 10f32.powf(p.send / 20.0) };
        let wet = p.dry_wet.clamp(0.0, 1.0);
        let dly_s = p.secs(tempo);
        let gram = if p.display { self.gram.clone() } else { None };
        let was = self.was_frozen;
        let mut start = frozen && !was;
        let stop = !frozen && was;
        self.was_frozen = frozen;
        let (layers, cur, prev_phase, prev_mag, hist, hw, tmp, since, rng) = (
            &mut self.layers,
            &mut self.cur,
            &mut self.prev_phase,
            &mut self.prev_mag,
            &mut self.hist,
            &mut self.hw,
            &mut self.tmp,
            &mut self.since,
            &mut self.rng,
        );
        self.stft.process(l, r, &mut |a, b, bins| {
            let nfft = (bins - 1) * 2;
            let hop = nfft / 4;
            let hz = sr / nfft as f32;
            let dt = hop as f32 / sr;
            let mut rows_d = [0.0f32; SG_ROWS];
            if gram.is_some() {
                crate::stft::gram_rows(a, b, nfft, sr, &mut rows_d);
            }
            // Spectral flux for onsets; phase increments for freezing.
            let mut flux = 0.0f32;
            let mut total = 1e-9f32;
            for k in 0..bins {
                let m = (a[k].norm() + b[k].norm()) * 0.5;
                flux += (m - prev_mag[k]).max(0.0);
                total += m;
                prev_mag[k] = m;
            }
            let onset = flux / total > 0.6 - 0.55 * p.sensitivity.clamp(0.0, 1.0) && total > nfft as f32 * 1e-4;
            *since += dt;
            let mut trig = false;
            if frozen && p.retrigger {
                let hit = if p.sync { std::mem::take(&mut sync_hit) } else { onset && *since > 0.05 };
                trig = hit;
            }
            if start || trig {
                start = false;
                *since = 0.0;
                // Capture into a new layer; the others fade out.
                *cur = (*cur + 1) % LAYERS;
                for (i, ly) in layers.iter_mut().enumerate() {
                    ly.rising = i == *cur;
                }
                let ly = &mut layers[*cur];
                ly.env = 0.0;
                for (c, s) in [&*a, &*b].into_iter().enumerate() {
                    for k in 0..bins {
                        let ph = s[k].arg();
                        let expect = TAU * k as f32 * hop as f32 / nfft as f32;
                        let d = (ph - prev_phase[c][k] - expect + std::f32::consts::PI).rem_euclid(TAU) - std::f32::consts::PI;
                        ly.mag[c][k] = s[k].norm();
                        ly.dphi[c][k] = expect + d;
                        ly.phase[c][k] = ph;
                    }
                }
            }
            for (c, s) in [&*a, &*b].into_iter().enumerate() {
                for k in 0..bins {
                    prev_phase[c][k] = s[k].arg();
                }
            }
            if stop {
                for ly in layers.iter_mut() {
                    ly.rising = false;
                }
            }
            // Fade rates per frame.
            let (fin, fout) = if p.retrigger && !p.envelope {
                let span = (p.xfade.clamp(0.01, 1.0) * if p.sync { 0.5 } else { 0.25 }).max(dt);
                (dt / span, dt / span)
            } else {
                (dt / (p.fade_in.max(ST_FADE.0) * 0.001), dt / (p.fade_out.max(ST_FADE.0) * 0.001))
            };
            // Stage input (Send).
            for k in 0..bins {
                tmp[0][k] = a[k] * send;
                tmp[1][k] = b[k] * send;
            }
            let freezer = |tmp: &mut [Box<[Complex<f32>]>; 2], layers: &mut [Layer; LAYERS]| {
                if !p.frz_on {
                    return;
                }
                let mut sum_env = 0.0;
                for ly in layers.iter_mut() {
                    ly.env = if ly.rising { (ly.env + fin).min(1.0) } else { (ly.env - fout).max(0.0) };
                    sum_env += ly.env;
                }
                if sum_env <= 0.0 && !frozen {
                    return;
                }
                // Live input fades against the frozen layers.
                let live = (1.0 - sum_env).clamp(0.0, 1.0) * if frozen { 0.0 } else { 1.0 };
                for c in 0..2 {
                    for k in 0..bins {
                        let mut v = tmp[c][k] * live;
                        for ly in layers.iter_mut() {
                            if ly.env <= 0.0 {
                                continue;
                            }
                            ly.phase[c][k] = (ly.phase[c][k] + ly.dphi[c][k]).rem_euclid(TAU);
                            v += Complex::from_polar(ly.mag[c][k] * ly.env * send, ly.phase[c][k]);
                        }
                        tmp[c][k] = v;
                    }
                }
            };
            let delay = |tmp: &mut [Box<[Complex<f32>]>; 2], hist: &mut [Box<[Complex<f32>]>; 2], hw: &mut usize, rng: &mut Rng| {
                if !p.dly_on {
                    return;
                }
                let frames = (HIST / bins).max(2);
                let mix = p.mix.clamp(0.0, 1.0);
                let fb = p.feedback.clamp(0.0, 1.0);
                let shift = (p.shift.clamp(-ST_SHIFT, ST_SHIFT) / hz).round() as isize;
                let lo = if p.mask > 0.0 { (20.0 * 1000f32.powf(p.mask)) / hz } else { 0.0 };
                let hi = if p.mask < 0.0 { (20_000.0 * 1000f32.powf(p.mask)) / hz } else { f32::MAX };
                let w = *hw % frames;
                for c in 0..2 {
                    let st = 1.0 + p.stereo.clamp(0.0, 1.0) * if c == 0 { -0.25 } else { 0.25 };
                    for k in 0..bins {
                        let kn = k as f32 / bins as f32;
                        let spray = if p.spray > 0.0 { rng.uni() * p.spray.min(ST_SPRAY) * 0.001 } else { 0.0 };
                        let d = ((dly_s * st + p.tilt.clamp(-ST_TILT, ST_TILT) * 0.001 * (kn * 2.0 - 1.0) + spray) / dt)
                            .round()
                            .clamp(1.0, (frames - 1) as f32) as usize;
                        let rd = (w + frames - d) % frames;
                        let y = hist[c][rd * bins + k];
                        let masked = (k as f32) < lo || (k as f32) > hi;
                        // Repeats shifted by `shift` bins.
                        let src = k as isize - shift;
                        let fbv =
                            if (0..bins as isize).contains(&src) { hist[c][rd * bins + src as usize] } else { Complex::new(0.0, 0.0) };
                        let x = tmp[c][k];
                        let inp = if masked { Complex::new(0.0, 0.0) } else { x };
                        let wv = inp + fbv * fb;
                        hist[c][w * bins + k] = if wv.re.is_finite() && wv.im.is_finite() { wv } else { Complex::new(0.0, 0.0) };
                        let yv = if masked { Complex::new(0.0, 0.0) } else { y };
                        tmp[c][k] = x * (1.0 - mix) + yv * mix;
                    }
                }
                *hw = (w + 1) % frames;
            };
            if p.dly_first {
                delay(tmp, hist, hw, rng);
                freezer(tmp, layers);
            } else {
                freezer(tmp, layers);
                delay(tmp, hist, hw, rng);
            }
            if let Some(gm) = &gram {
                let mut rows_w = [0.0f32; SG_ROWS];
                crate::stft::gram_rows(&tmp[0][..bins], &tmp[1][..bins], nfft, sr, &mut rows_w);
                gm.push(&rows_d, &rows_w);
            }
            for k in 0..bins {
                a[k] = a[k] * (1.0 - wet) + tmp[0][k] * wet;
                b[k] = b[k] * (1.0 - wet) + tmp[1][k] * wet;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_and_freezes() {
        let sr = 48_000.0;
        let p = SpecTimeParams { frz_on: false, dry_wet: 1.0, mix: 1.0, time: 250.0, ..Default::default() };
        let mut s = SpecTimeState::new(None);
        let mut l = vec![0.0f32; 48_000];
        for x in l[..480].iter_mut() {
            *x = 0.5;
        }
        let mut r = l.clone();
        s.process(&p, &mut l, &mut r, sr);
        // Energy arrives ≈ 250 ms + 2048 (latency) later.
        let e = |a: usize, b: usize| l[a..b].iter().map(|x| x * x).sum::<f32>();
        let at = 12_000 + 2048;
        assert!(e(at - 1000, at + 1500) > 10.0 * e(4000, 9000), "{} {}", e(at - 1000, at + 1500), e(4000, 9000));
        // Freeze holds a tone after the input stops.
        let p = SpecTimeParams { dly_on: false, freeze: false, dry_wet: 1.0, ..Default::default() };
        let mut s = SpecTimeState::new(None);
        let mut l: Vec<f32> = (0..9600).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let mut r = l.clone();
        s.process(&p, &mut l, &mut r, sr);
        let p2 = SpecTimeParams { freeze: true, ..p };
        let mut l = vec![0.0f32; 24_000];
        let mut r = l.clone();
        // First block still carries the tone inside the STFT.
        let mut l0: Vec<f32> = (0..512).map(|i| ((i + 9600) as f32 * 0.05).sin() * 0.5).collect();
        let mut r0 = l0.clone();
        s.process(&p2, &mut l0, &mut r0, sr);
        s.process(&p2, &mut l, &mut r, sr);
        let tail = &l[12_000..];
        assert!(tail.iter().map(|x| x.abs()).fold(0.0, f32::max) > 0.05);
        assert!(l.iter().all(|x| x.is_finite()));
    }
}
