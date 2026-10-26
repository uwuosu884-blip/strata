//! Streaming phase vocoder (Complex / Complex Pro warp modes).
//!
//! Synthesis frames sit every `HOP` output samples; each analyses the source
//! at the position the warp map gives for its centre (so tempo changes,
//! loops and seeks need no lookahead buffer). Instantaneous frequency comes
//! from two analysis frames `HOP` source samples apart; pitch shifting scales
//! bins; Complex Pro divides out / restores a cepstral spectral envelope
//! (formant preservation). All buffers are preallocated: RT-safe.

use crate::sample::SampleBuffer;
use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};
use std::f32::consts::{PI, TAU};
use std::sync::Arc;

pub const N: usize = 2048;
pub const HOP: usize = N / 4;
const BINS: usize = N / 2 + 1;
const RING: usize = 16384;

/// Shared FFT plans (planning allocates: do it once, off the audio thread).
#[derive(Clone)]
pub struct Plans {
    fwd: Arc<dyn RealToComplex<f32>>,
    inv: Arc<dyn ComplexToReal<f32>>,
}

impl Plans {
    pub fn new() -> Self {
        let mut p = RealFftPlanner::<f32>::new();
        Self { fwd: p.plan_fft_forward(N), inv: p.plan_fft_inverse(N) }
    }
}

impl Default for Plans {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Plans {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Plans")
    }
}

pub struct Pv {
    plans: Plans,
    win: Vec<f32>,
    time: Vec<f32>,
    sa: Vec<Complex<f32>>,
    sb: Vec<Complex<f32>>,
    so: Vec<Complex<f32>>,
    scratch_f: Vec<Complex<f32>>,
    scratch_i: Vec<Complex<f32>>,
    mag: Vec<f32>,
    pha: Vec<f32>,
    peaks: Vec<u16>,
    newph: Vec<f32>,
    omega: Vec<f32>,
    omag: Vec<f32>,
    env: Vec<f32>,
    cep: Vec<f32>,
    cspec: Vec<Complex<f32>>,
    phase: [Vec<f32>; 2],
    ola: [Vec<f32>; 2],
    /// Absolute output index of the next sample to emit.
    t: i64,
    next_c: i64,
    last_p: f64,
    primed: bool,
    /// Caller's continuity stamp (sample counter + clip key) expected next.
    expect: (u64, u64),
}

impl std::fmt::Debug for Pv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Pv")
    }
}

/// Formant options for Complex Pro.
#[derive(Debug, Clone, Copy)]
pub struct Formants {
    /// 0..1 how much of the original envelope is kept when transposing.
    pub amount: f32,
    /// Cepstral lifter length (Live's "Envelope", 8..256).
    pub lifter: usize,
}

impl Pv {
    pub fn new(plans: Plans) -> Self {
        let win: Vec<f32> = (0..N).map(|i| 0.5 - 0.5 * (TAU * i as f32 / N as f32).cos()).collect();
        let sf = plans.fwd.get_scratch_len();
        let si = plans.inv.get_scratch_len();
        Self {
            plans,
            win,
            time: vec![0.0; N],
            sa: vec![Complex::default(); BINS],
            sb: vec![Complex::default(); BINS],
            so: vec![Complex::default(); BINS],
            scratch_f: vec![Complex::default(); sf],
            scratch_i: vec![Complex::default(); si],
            mag: vec![0.0; BINS],
            pha: vec![0.0; BINS],
            peaks: vec![0; BINS],
            newph: vec![0.0; BINS],
            omega: vec![0.0; BINS],
            omag: vec![0.0; BINS],
            env: vec![1.0; BINS],
            cep: vec![0.0; N],
            cspec: vec![Complex::default(); BINS],
            phase: [vec![0.0; BINS], vec![0.0; BINS]],
            ola: [vec![0.0; RING], vec![0.0; RING]],
            t: 0,
            next_c: 0,
            last_p: 0.0,
            primed: false,
            expect: (u64::MAX, u64::MAX),
        }
    }

    pub fn reset(&mut self) {
        for o in &mut self.ola {
            o.fill(0.0);
        }
        self.t = 0;
        self.next_c = -(N as i64 / 2) + HOP as i64;
        self.primed = false;
    }

    /// Render `out.len()` samples (added into `out_l/out_r`). `pos_at(off)`
    /// = source frame for the output sample `off` samples after this block's
    /// start (any sign), None outside the clip. `rate` = pitch ratio.
    /// `stamp` = (running sample counter at block start, clip key): a
    /// mismatch with the previous call's end resets the vocoder.
    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        src: &SampleBuffer,
        stamp: (u64, u64),
        pos_at: impl Fn(i64) -> Option<f64>,
        rate: f32,
        formants: Option<Formants>,
        out_l: &mut [f32],
        out_r: &mut [f32],
    ) {
        let n = out_l.len().min(out_r.len()).min(RING - 2 * N);
        if stamp != self.expect {
            self.reset();
        }
        self.expect = (stamp.0 + n as u64, stamp.1);
        let last = self.t + n as i64 - 1;
        while self.next_c - (N as i64 / 2) <= last {
            let c = self.next_c;
            self.next_c += HOP as i64;
            let Some(p) = pos_at(c - self.t) else {
                self.primed = false;
                continue;
            };
            if self.primed && (p - self.last_p).abs() > N as f64 {
                self.primed = false; // loop wrap / jump: restart phases
            }
            self.frame(src, c, p, rate, formants);
            self.last_p = p;
            self.primed = true;
        }
        for k in 0..n {
            let i = ((self.t + k as i64).rem_euclid(RING as i64)) as usize;
            out_l[k] += self.ola[0][i];
            out_r[k] += self.ola[1][i];
            self.ola[0][i] = 0.0;
            self.ola[1][i] = 0.0;
        }
        self.t += n as i64;
    }

    fn analyse(&mut self, src: &SampleBuffer, ch: usize, centre: f64, into_b: bool) {
        let data = if ch == 0 { &src.left } else { &src.right };
        let len = data.len();
        let start = centre - (N / 2) as f64;
        let i0 = start.floor();
        let fr = (start - i0) as f32;
        let i0 = i0 as i64;
        for (j, t) in self.time.iter_mut().enumerate() {
            let k = i0 + j as i64;
            let a = if k >= 0 && (k as usize) < len { data[k as usize] } else { 0.0 };
            let b = if k + 1 >= 0 && ((k + 1) as usize) < len { data[(k + 1) as usize] } else { 0.0 };
            *t = (a + (b - a) * fr) * self.win[j];
        }
        let out = if into_b { &mut self.sb } else { &mut self.sa };
        let _ = self.plans.fwd.process_with_scratch(&mut self.time, out, &mut self.scratch_f);
    }

    /// Cepstral spectral envelope of `self.mag` into `self.env`.
    fn envelope(&mut self, lifter: usize) {
        for (c, m) in self.cspec.iter_mut().zip(&self.mag) {
            *c = Complex::new((m + 1e-9).ln(), 0.0);
        }
        self.cspec[0].im = 0.0;
        self.cspec[BINS - 1].im = 0.0;
        let _ = self.plans.inv.process_with_scratch(&mut self.cspec, &mut self.cep, &mut self.scratch_i);
        let l = lifter.clamp(4, N / 2 - 1);
        let norm = 1.0 / N as f32;
        for (i, v) in self.cep.iter_mut().enumerate() {
            let keep = i < l || i > N - l;
            *v = if keep { *v * norm } else { 0.0 };
        }
        let _ = self.plans.fwd.process_with_scratch(&mut self.cep, &mut self.cspec, &mut self.scratch_f);
        for (e, c) in self.env.iter_mut().zip(&self.cspec) {
            *e = c.re.exp().max(1e-9);
        }
    }

    /// One synthesis frame: analysis at `p`, identity phase locking
    /// (Laroche–Dolson): only spectral peaks accumulate phase, every other
    /// bin keeps its analysed offset to its peak — no phasiness.
    fn frame(&mut self, src: &SampleBuffer, c: i64, p: f64, rate: f32, formants: Option<Formants>) {
        let hop = HOP as f32;
        let bin_w = TAU / N as f32;
        for ch in 0..2 {
            self.analyse(src, ch, p, false);
            self.analyse(src, ch, p - HOP as f64, true);
            for k in 0..BINS {
                let (a, b) = (self.sa[k], self.sb[k]);
                self.mag[k] = a.norm();
                self.pha[k] = a.arg();
                let expected = bin_w * k as f32 * hop;
                let mut d = self.pha[k] - b.arg() - expected;
                d -= TAU * (d / TAU).round();
                self.omega[k] = bin_w * k as f32 + d / hop; // rad / sample
            }
            if let Some(f) = formants {
                self.envelope(f.lifter);
            }
            // Peaks: local maxima over ±2 bins.
            let mut np = 0;
            for k in 2..BINS - 2 {
                let m = self.mag[k];
                if m > 1e-7 && m >= self.mag[k - 1] && m > self.mag[k + 1] && m >= self.mag[k - 2] && m > self.mag[k + 2] {
                    self.peaks[np] = k as u16;
                    np += 1;
                }
            }
            self.omag.fill(0.0);
            self.newph.fill(0.0);
            let ph = &mut self.phase[ch];
            let primed = self.primed;
            // Each peak owns the bins up to the midpoint to its neighbours.
            let mut lo = 0usize;
            for q in 0..np {
                let pk = self.peaks[q] as usize;
                let hi = if q + 1 < np { (pk + self.peaks[q + 1] as usize).div_ceil(2) } else { BINS };
                let jp = (pk as f32 * rate).round() as usize;
                if jp >= BINS {
                    break;
                }
                let psi = if primed { ph[jp] + self.omega[pk] * rate * hop } else { self.pha[pk] };
                for k in lo..hi {
                    let j = (k as f32 * rate).round() as usize;
                    if j >= BINS {
                        break;
                    }
                    let mut m = self.mag[k];
                    if let Some(f) = formants {
                        let (e_src, e_dst) = (self.env[k], self.env[j]);
                        m = m / e_src * (f.amount * e_dst + (1.0 - f.amount) * e_src);
                    }
                    if m > self.omag[j] {
                        self.omag[j] = m;
                        self.newph[j] = psi + (self.pha[k] - self.pha[pk]);
                    }
                }
                lo = hi;
            }
            for (j, p) in ph.iter_mut().enumerate().take(BINS) {
                let v = self.newph[j];
                *p = if v.abs() > 64.0 * PI { v.rem_euclid(TAU) } else { v };
                self.so[j] = Complex::from_polar(self.omag[j], *p);
            }
            self.so[0].im = 0.0;
            self.so[BINS - 1].im = 0.0;
            let _ = self.plans.inv.process_with_scratch(&mut self.so, &mut self.time, &mut self.scratch_i);
            // Hann² at 75 % overlap sums to 1.5; inverse FFT scales by N.
            let g = 1.0 / (N as f32 * 1.5);
            let base = c - (N / 2) as i64;
            let o = &mut self.ola[ch];
            for (j, v) in self.time.iter().enumerate() {
                let i = ((base + j as i64).rem_euclid(RING as i64)) as usize;
                o[i] += v * self.win[j] * g;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(f: f64, secs: f64) -> SampleBuffer {
        let sr = 48_000.0;
        SampleBuffer::mono(
            "s",
            48_000,
            (0..(sr * secs) as usize).map(|i| (i as f64 / sr * f * std::f64::consts::TAU).sin() as f32 * 0.5).collect(),
        )
    }

    fn crossings(x: &[f32]) -> usize {
        x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count()
    }

    fn run(pv: &mut Pv, s: &SampleBuffer, stretch: f64, rate: f32, fm: Option<Formants>, total: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; total];
        let mut r = vec![0.0f32; total];
        let block = 480;
        let mut t = 0usize;
        while t < total {
            let n = block.min(total - t);
            let t0 = t as f64;
            pv.process(s, (t as u64, 1), |off| Some((t0 + off as f64) / stretch), rate, fm, &mut out[t..t + n], &mut r[t..t + n]);
            t += n;
        }
        out
    }

    #[test]
    fn stretch_keeps_pitch_and_level() {
        let s = sine(440.0, 2.0);
        let mut pv = Pv::new(Plans::new());
        let out = run(&mut pv, &s, 2.0, 1.0, None, 96_000);
        let body = &out[9_600..86_400]; // 1.6 s
        let zc = crossings(body);
        assert!((690..=720).contains(&zc), "pitch kept: {zc} crossings");
        let rms = (body.iter().map(|v| v * v).sum::<f32>() / body.len() as f32).sqrt();
        assert!((rms - 0.3535).abs() < 0.06, "level kept: {rms}");
    }

    #[test]
    fn transpose_octave_up_and_formant_mode_runs() {
        let s = sine(300.0, 2.0);
        let mut pv = Pv::new(Plans::new());
        let out = run(&mut pv, &s, 1.0, 2.0, None, 48_000);
        let zc = crossings(&out[4_800..43_200]); // 0.8 s
        assert!((460..=500).contains(&zc), "octave up: {zc}");
        let mut pv = Pv::new(Plans::new());
        let out = run(&mut pv, &s, 1.0, 2.0, Some(Formants { amount: 1.0, lifter: 128 }), 48_000);
        assert!(out.iter().all(|v| v.is_finite()) && out.iter().any(|v| v.abs() > 0.01));
    }
}
