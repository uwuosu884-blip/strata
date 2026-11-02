//! Vocoder (Live 12, manual 29.42): band-pass filter banks on the modulator
//! (the device's input) and the carrier (Noise with downsampling / density,
//! External sidechain, the Modulator itself, or a Pitch Tracking
//! oscillator), Enhance, Unvoiced noise with Sens. and Fast / Slow, band
//! level attenuation, Bands, Range, BW, Precise / Retro, Gate, Level,
//! Depth, Attack / Release, Mono / Stereo / L/R, Formant and Dry/Wet.

use crate::devtap::DevTap;
use crate::dsp::{Rng, Svf, svf_g};
use crate::dynfx::{ScState, Sidechain};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const VC_MAXB: usize = 40;
pub const VC_BANDS: [u8; 10] = [4, 8, 12, 16, 20, 24, 28, 32, 36, 40];
pub const VC_CARRIERS: [&str; 4] = ["Noise", "External", "Modulator", "Pitch Tracking"];
pub const VC_WAVES: [&str; 4] = ["Saw", "Pulse 50", "Pulse 25", "Pulse 12"];
pub const VC_RANGE: (f32, f32) = (20.0, 20_000.0);
pub const VC_BW: (f32, f32) = (0.1, 2.0);
pub const VC_GATE: (f32, f32) = (-80.0, 0.0);
pub const VC_LEVEL: (f32, f32) = (-24.0, 24.0);
pub const VC_UNVOICED: (f32, f32) = (-70.0, 0.0);
pub const VC_ATTACK: (f32, f32) = (1.0, 500.0);
pub const VC_RELEASE: (f32, f32) = (1.0, 3000.0);
pub const VC_FORMANT: f32 = 1.0;
pub const VC_PITCH: f32 = 24.0;
pub const VC_TRACK: (f32, f32) = (20.0, 2000.0);
/// Tap slots 0..40: modulator band levels (display).
pub const TAP_BANDS: usize = 0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VocoderParams {
    /// Index into [`VC_CARRIERS`].
    pub carrier: u8,
    /// External carrier source (`on` follows `carrier == 1`).
    pub ext: Sidechain,
    pub enhance: bool,
    /// Noise XY: downsampling (1 = none) and density (1 = white).
    pub noise_rate: f32,
    pub noise_density: f32,
    /// Pitch tracking: Hz range, waveform, coarse st.
    pub track_lo: f32,
    pub track_hi: f32,
    pub wave: u8,
    pub pitch: f32,
    /// dB (≤ −70 = off).
    pub unvoiced: f32,
    pub sens: f32,
    pub fast: bool,
    /// Index into [`VC_BANDS`].
    pub bands: u8,
    /// Hz.
    pub lo: f32,
    pub hi: f32,
    pub bw: f32,
    pub retro: bool,
    /// dB (≤ −80 = −inf).
    pub gate: f32,
    /// dB.
    pub level: f32,
    /// 0..2.
    pub depth: f32,
    /// ms.
    pub attack: f32,
    pub release: f32,
    /// 0 Mono, 1 Stereo, 2 L/R.
    pub chan: u8,
    /// Octaves (−1..1).
    pub formant: f32,
    pub dry_wet: f32,
    /// Per-band attenuation (display edits), 0..1.
    #[serde(with = "band_serde")]
    pub band_lv: [f32; VC_MAXB],
}

impl Default for VocoderParams {
    fn default() -> Self {
        Self {
            carrier: 0,
            ext: Sidechain::default(),
            enhance: false,
            noise_rate: 1.0,
            noise_density: 1.0,
            track_lo: 50.0,
            track_hi: 1000.0,
            wave: 0,
            pitch: 0.0,
            unvoiced: VC_UNVOICED.0,
            sens: 0.5,
            fast: true,
            bands: 4,
            lo: 80.0,
            hi: 12_000.0,
            bw: 1.0,
            retro: false,
            gate: VC_GATE.0,
            level: 0.0,
            depth: 1.0,
            attack: 1.0,
            release: 150.0,
            chan: 1,
            formant: 0.0,
            dry_wet: 1.0,
            band_lv: [1.0; VC_MAXB],
        }
    }
}

/// serde for the 40-band array (serde's arrays stop at 32): a sequence.
mod band_serde {
    use super::VC_MAXB;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &[f32; VC_MAXB], s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(v.iter())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[f32; VC_MAXB], D::Error> {
        let v: Vec<f32> = Vec::deserialize(d)?;
        let mut out = [1.0f32; VC_MAXB];
        for (o, x) in out.iter_mut().zip(v) {
            *o = x;
        }
        Ok(out)
    }
}

impl VocoderParams {
    pub fn n_bands(&self) -> usize {
        VC_BANDS[(self.bands as usize).min(VC_BANDS.len() - 1)] as usize
    }

    /// Centre frequency of band `i` (log-spaced over the range).
    pub fn band_freq(&self, i: usize) -> f32 {
        let n = self.n_bands();
        let (lo, hi) = (self.lo.clamp(VC_RANGE.0, VC_RANGE.1), self.hi.clamp(VC_RANGE.0, VC_RANGE.1));
        let u = if n > 1 { i as f32 / (n - 1) as f32 } else { 0.5 };
        lo * (hi / lo).powf(u)
    }
}

#[derive(Debug, Clone)]
pub struct VocoderState {
    tap: Option<Arc<DevTap>>,
    pub(crate) sc: ScState,
    mf: [[Svf; 2]; VC_MAXB],
    cf: [[Svf; 2]; VC_MAXB],
    menv: [[f32; 2]; VC_MAXB],
    cenv: [[f32; 2]; VC_MAXB],
    peak: [f32; VC_MAXB],
    rng: Rng,
    /// Noise sample-and-hold (downsampling) phase and value.
    nph: f32,
    nval: f32,
    /// Unvoiced detector: high-band and full envelopes, smoothed flag.
    uv_hp: Svf,
    uv_hf: f32,
    uv_full: f32,
    uv: f32,
    /// Pitch tracker: decimated ring, oscillator.
    pt_buf: Box<[f32]>,
    pt_w: usize,
    pt_acc: f32,
    pt_cnt: usize,
    pt_hop: usize,
    pt_f: f32,
    osc: f32,
    dc: [(f32, f32); 2],
}

const PT_LEN: usize = 1024;
const PT_DEC: usize = 4;

impl Default for VocoderState {
    fn default() -> Self {
        Self::new(None)
    }
}

impl VocoderState {
    pub fn new(id: Option<u64>) -> Self {
        Self {
            tap: id.map(crate::devtap::dev_tap),
            sc: ScState::default(),
            mf: std::array::from_fn(|_| Default::default()),
            cf: std::array::from_fn(|_| Default::default()),
            menv: [[0.0; 2]; VC_MAXB],
            cenv: [[0.0; 2]; VC_MAXB],
            peak: [0.0; VC_MAXB],
            rng: Rng(0x1234_9876),
            nph: 0.0,
            nval: 0.0,
            uv_hp: Svf::default(),
            uv_hf: 0.0,
            uv_full: 0.0,
            uv: 0.0,
            pt_buf: vec![0.0; PT_LEN].into_boxed_slice(),
            pt_w: 0,
            pt_acc: 0.0,
            pt_cnt: 0,
            pt_hop: 0,
            pt_f: 110.0,
            osc: 0.0,
            dc: [(0.0, 0.0); 2],
        }
    }

    pub fn set_id(&mut self, id: u64) {
        self.tap = Some(crate::devtap::dev_tap(id));
    }

    pub fn reset(&mut self) {
        // In place: no allocation on the audio thread.
        self.mf = std::array::from_fn(|_| Default::default());
        self.cf = std::array::from_fn(|_| Default::default());
        self.menv = [[0.0; 2]; VC_MAXB];
        self.cenv = [[0.0; 2]; VC_MAXB];
        self.peak = [0.0; VC_MAXB];
        (self.nph, self.nval) = (0.0, 0.0);
        self.uv_hp = Svf::default();
        (self.uv_hf, self.uv_full, self.uv) = (0.0, 0.0, 0.0);
        self.pt_buf.fill(0.0);
        (self.pt_w, self.pt_acc, self.pt_cnt, self.pt_hop) = (0, 0.0, 0, 0);
        (self.pt_f, self.osc) = (110.0, 0.0);
        self.dc = [(0.0, 0.0); 2];
        self.sc.reset();
    }

    /// Autocorrelation pitch estimate over the decimated ring; keeps the last
    /// pitch unless the peak is clear.
    fn track(&mut self, p: &VocoderParams, sr: f32) {
        let dsr = sr / PT_DEC as f32;
        let (lo, hi) = (p.track_lo.clamp(VC_TRACK.0, VC_TRACK.1), p.track_hi.clamp(VC_TRACK.0, VC_TRACK.1));
        let (lo, hi) = (lo.min(hi), hi.max(lo));
        let lag_min = ((dsr / hi) as usize).max(2);
        let lag_max = ((dsr / lo) as usize).min(PT_LEN / 2 - 1);
        if lag_min >= lag_max {
            return;
        }
        let win = PT_LEN / 2;
        let at = |k: usize| self.pt_buf[(self.pt_w + PT_LEN - win - lag_max + k) % PT_LEN];
        let mut e0 = 0.0f32;
        for k in 0..win {
            e0 += at(lag_max + k).powi(2);
        }
        if e0 < 1e-6 {
            return;
        }
        let (mut best, mut best_lag) = (0.0f32, 0usize);
        for lag in lag_min..=lag_max {
            let (mut c, mut e1) = (0.0f32, 0.0f32);
            for k in 0..win {
                let a = at(lag_max + k);
                let b = at(lag_max + k - lag);
                c += a * b;
                e1 += b * b;
            }
            let v = c / (e0 * e1).sqrt().max(1e-9);
            if v > best {
                best = v;
                best_lag = lag;
            }
        }
        if best > 0.7 && best_lag > 0 {
            self.pt_f = dsr / best_lag as f32;
        }
    }

    #[allow(clippy::needless_range_loop)]
    pub fn process(&mut self, p: &VocoderParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let n = l.len().min(r.len());
        let nb = p.n_bands();
        let (lo, hi) = (p.lo.clamp(VC_RANGE.0, VC_RANGE.1), p.hi.clamp(VC_RANGE.0, VC_RANGE.1));
        let span = (hi / lo).abs().log2().max(0.1);
        let spacing = span / (nb.max(2) - 1) as f32;
        let k0 = crate::vinyl::b_k((spacing * p.bw.clamp(VC_BW.0, VC_BW.1)).clamp(0.02, 3.0));
        let fm = 2f32.powf(p.formant.clamp(-VC_FORMANT, VC_FORMANT));
        let mut gm = [0.0f32; VC_MAXB];
        let mut gc = [0.0f32; VC_MAXB];
        let mut kk = [0.0f32; VC_MAXB];
        let mut bg = [0.0f32; VC_MAXB];
        for i in 0..nb {
            let f = p.band_freq(i);
            let u = if nb > 1 { i as f32 / (nb - 1) as f32 } else { 0.0 };
            gm[i] = svf_g(f, sr);
            gc[i] = svf_g((f * fm).min(sr * 0.45), sr);
            let (k, g) = if p.retro { (k0 * (1.0 - 0.6 * u), 0.6 + 1.6 * u) } else { (k0, 1.0) };
            kk[i] = k;
            bg[i] = g * p.band_lv[i].clamp(0.0, 1.0);
        }
        let a = |ms: f32| (-1.0 / (ms * 0.001 * sr)).exp();
        let (ca, cr) = (a(p.attack.clamp(VC_ATTACK.0, VC_ATTACK.1)), a(p.release.clamp(VC_RELEASE.0, VC_RELEASE.1)));
        let ce = a(20.0);
        let depth = p.depth.clamp(0.0, 2.0);
        let gate = if p.gate <= VC_GATE.0 { 0.0 } else { 10f32.powf(p.gate / 20.0) };
        let level = 10f32.powf(p.level.clamp(VC_LEVEL.0, VC_LEVEL.1) / 20.0);
        let uvg = if p.unvoiced <= VC_UNVOICED.0 { 0.0 } else { 10f32.powf(p.unvoiced / 20.0) };
        let sens = p.sens.clamp(0.0, 1.0);
        let uv_c = a(if p.fast { 5.0 } else { 50.0 });
        let g_uv = svf_g(5000.0, sr);
        let wet = p.dry_wet.clamp(0.0, 1.0);
        let ext = p.carrier == 1 && p.ext.source != 0;
        let nrate = p.noise_rate.clamp(0.0, 1.0);
        let nstep = 0.005 + 0.995 * nrate * nrate;
        let ndens = p.noise_density.clamp(0.0, 1.0);
        let pt_ratio = 2f32.powf(p.pitch.clamp(-VC_PITCH, VC_PITCH) / 12.0);
        let duty = [0.5, 0.5, 0.25, 0.125][(p.wave as usize).min(3)];
        let stereo_mod = p.chan == 2;
        let stereo_car = p.chan >= 1;
        let r_dc = 1.0 - std::f32::consts::TAU * 10.0 / sr;
        let norm = 1.2 * (20.0 / nb as f32).sqrt();
        for i in 0..n {
            let dry = [l[i], r[i]];
            let mono = 0.5 * (dry[0] + dry[1]);
            let m_in = if stereo_mod { dry } else { [mono, mono] };
            // Unvoiced detector on the modulator.
            let (_, _, hp) = self.uv_hp.tick(mono, g_uv, 0.7);
            self.uv_hf = hp.abs() + ce * (self.uv_hf - hp.abs());
            self.uv_full = mono.abs() + ce * (self.uv_full - mono.abs());
            let ratio = self.uv_hf / (self.uv_full + 1e-6);
            let target = if sens >= 0.999 {
                1.0
            } else if sens <= 0.001 {
                0.0
            } else if ratio > 1.0 - sens * 0.8 {
                1.0
            } else {
                0.0
            };
            self.uv = target + uv_c * (self.uv - target);
            // Carrier.
            let mut car = match p.carrier {
                0 => {
                    self.nph += nstep;
                    if self.nph >= 1.0 {
                        self.nph -= 1.0;
                        self.nval = if self.rng.uni() <= ndens * ndens.max(0.02) + 0.001 { self.rng.bi() } else { 0.0 };
                        if ndens < 1.0 {
                            self.nval *= 1.0 + 2.0 * (1.0 - ndens);
                        }
                    }
                    [self.nval, self.nval]
                }
                1 => {
                    if ext && i < self.sc.len {
                        [self.sc.ext_l[i], self.sc.ext_r[i]]
                    } else {
                        [0.0, 0.0]
                    }
                }
                2 => dry,
                _ => {
                    // Decimate the modulator into the tracker ring.
                    self.pt_acc += mono;
                    self.pt_cnt += 1;
                    if self.pt_cnt == PT_DEC {
                        self.pt_buf[self.pt_w] = self.pt_acc / PT_DEC as f32;
                        self.pt_w = (self.pt_w + 1) % PT_LEN;
                        self.pt_acc = 0.0;
                        self.pt_cnt = 0;
                    }
                    self.pt_hop += 1;
                    if self.pt_hop >= 512 {
                        self.pt_hop = 0;
                        self.track(p, sr);
                    }
                    self.osc = (self.osc + self.pt_f * pt_ratio / sr).rem_euclid(1.0);
                    let v = if p.wave == 0 {
                        2.0 * self.osc - 1.0
                    } else if self.osc < duty {
                        1.0
                    } else {
                        -1.0
                    };
                    [v * 0.5, v * 0.5]
                }
            };
            if !stereo_car {
                let m = 0.5 * (car[0] + car[1]);
                car = [m, m];
            }
            if uvg > 0.0 && self.uv > 1e-3 {
                let nz = self.rng.bi() * uvg * self.uv;
                car[0] += nz;
                car[1] += nz;
            }
            let mut out = [0.0f32; 2];
            let chans = if stereo_mod || stereo_car { 2 } else { 1 };
            for b in 0..nb {
                for c in 0..chans {
                    let mc = if stereo_mod { c } else { 0 };
                    let e = if c == mc || stereo_mod {
                        let (_, bp, _) = self.mf[b][mc].tick(m_in[mc], gm[b], kk[b]);
                        let v = (bp * kk[b]).abs();
                        let co = if v > self.menv[b][mc] { ca } else { cr };
                        self.menv[b][mc] = v + co * (self.menv[b][mc] - v);
                        self.menv[b][mc]
                    } else {
                        self.menv[b][0]
                    };
                    let (_, cb, _) = self.cf[b][c].tick(car[c], gc[b], kk[b]);
                    let cb = cb * kk[b];
                    let mut cv = cb;
                    if p.enhance {
                        let v = cb.abs();
                        self.cenv[b][c] = v + ce * (self.cenv[b][c] - v);
                        cv = cb / (self.cenv[b][c] * 4.0 + 0.01);
                    }
                    let g = if e < gate { 0.0 } else { (e * 4.0).powf(depth) };
                    out[c] += cv * g * bg[b];
                    if c == 0 {
                        self.peak[b] = self.peak[b].max(e);
                    }
                }
            }
            if chans == 1 {
                out[1] = out[0];
            }
            for c in 0..2 {
                let v = out[c] * norm * level;
                let (x1, y1) = self.dc[c];
                let y = v - x1 + r_dc * y1;
                self.dc[c] = (v, if y.is_finite() { y } else { 0.0 });
                out[c] = (dry[c] + (y - dry[c]) * wet).clamp(-8.0, 8.0);
            }
            l[i] = out[0];
            r[i] = out[1];
        }
        if let Some(t) = &self.tap {
            for b in 0..nb {
                t.raise(TAP_BANDS + b, self.peak[b] * 4.0);
                self.peak[b] = 0.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(p: &VocoderParams, input: &[f32]) -> Vec<f32> {
        let mut s = VocoderState::new(None);
        let mut l = input.to_vec();
        let mut r = input.to_vec();
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            s.process(p, a, b, 48_000.0);
        }
        l
    }

    #[test]
    fn follows_the_modulator() {
        let sr = 48_000.0;
        // Modulator: 1 kHz burst then silence.
        let m: Vec<f32> =
            (0..48_000).map(|i| if i < 24_000 { (i as f32 * std::f32::consts::TAU * 1000.0 / sr).sin() * 0.5 } else { 0.0 }).collect();
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        for carrier in [0u8, 2, 3] {
            let p = VocoderParams { carrier, release: 50.0, ..Default::default() };
            let o = run(&p, &m);
            assert!(o.iter().all(|v| v.is_finite()));
            let (on, off) = (rms(&o[12_000..24_000]), rms(&o[40_000..]));
            eprintln!("carrier {carrier}: {on} {off} (in 0.35)");
            assert!(on > 0.01 && off < on * 0.05, "carrier {carrier}: {on} {off}");
        }
        // Retro, Enhance, L/R, all bands, unvoiced noise.
        let p =
            VocoderParams { retro: true, enhance: true, chan: 2, bands: 9, unvoiced: -6.0, sens: 1.0, formant: 0.5, ..Default::default() };
        assert!(run(&p, &m).iter().all(|v| v.is_finite() && v.abs() <= 8.0));
    }
}
