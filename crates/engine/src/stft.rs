//! Streaming STFT shared by Spectral Resonator and Spectral Time: Hann
//! analysis / synthesis windows, 4× overlap, FFT size switchable at run time
//! (1024 / 2048 / 4096; latency = size). All buffers preallocated: RT-safe.
//! Also the spectrogram display tap (dry / wet columns).

use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

pub const SIZES: [usize; 3] = [1024, 2048, 4096];
pub const MAX_N: usize = 4096;
pub const MAX_BINS: usize = MAX_N / 2 + 1;

type Plan = (Arc<dyn RealToComplex<f32>>, Arc<dyn ComplexToReal<f32>>);

/// Per-frame callback: left / right spectra and the bin count.
pub type FrameFn<'a> = dyn FnMut(&mut [Complex<f32>], &mut [Complex<f32>], usize) + 'a;

pub struct Stft {
    plans: Vec<Plan>,
    size: usize,
    n: usize,
    win: Box<[f32]>,
    inp: [Box<[f32]>; 2],
    out: [Box<[f32]>; 2],
    w: usize,
    count: usize,
    time: Box<[f32]>,
    pub spec: [Box<[Complex<f32>]>; 2],
    scratch_f: Box<[Complex<f32>]>,
    scratch_i: Box<[Complex<f32>]>,
}

impl std::fmt::Debug for Stft {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Stft({})", self.n)
    }
}

impl Clone for Stft {
    fn clone(&self) -> Self {
        let mut s = Self::new();
        s.set_size(self.size);
        s
    }
}

impl Default for Stft {
    fn default() -> Self {
        Self::new()
    }
}

impl Stft {
    /// Plans every size (allocates: control thread).
    pub fn new() -> Self {
        let mut pl = RealFftPlanner::<f32>::new();
        let plans: Vec<Plan> = SIZES.iter().map(|&n| (pl.plan_fft_forward(n), pl.plan_fft_inverse(n))).collect();
        let sf = plans.iter().map(|p| p.0.get_scratch_len()).max().unwrap_or(0);
        let si = plans.iter().map(|p| p.1.get_scratch_len()).max().unwrap_or(0);
        let z = |n: usize| vec![0.0f32; n].into_boxed_slice();
        let zc = |n: usize| vec![Complex::new(0.0f32, 0.0); n].into_boxed_slice();
        let mut s = Self {
            plans,
            size: 1,
            n: SIZES[1],
            win: z(MAX_N),
            inp: [z(MAX_N), z(MAX_N)],
            out: [z(MAX_N), z(MAX_N)],
            w: 0,
            count: 0,
            time: z(MAX_N),
            spec: [zc(MAX_BINS), zc(MAX_BINS)],
            scratch_f: zc(sf.max(1)),
            scratch_i: zc(si.max(1)),
        };
        s.set_size(1);
        s
    }

    pub fn n(&self) -> usize {
        self.n
    }

    pub fn hop(&self) -> usize {
        self.n / 4
    }

    pub fn bins(&self) -> usize {
        self.n / 2 + 1
    }

    /// Switch the FFT size (index into [`SIZES`]); clears the buffers.
    pub fn set_size(&mut self, i: usize) {
        let i = i.min(SIZES.len() - 1);
        if i == self.size && self.win[1] != 0.0 {
            return;
        }
        self.size = i;
        self.n = SIZES[i];
        let n = self.n;
        for (k, w) in self.win[..n].iter_mut().enumerate() {
            *w = 0.5 - 0.5 * (std::f32::consts::TAU * k as f32 / n as f32).cos();
        }
        self.reset();
    }

    pub fn reset(&mut self) {
        for c in 0..2 {
            self.inp[c].fill(0.0);
            self.out[c].fill(0.0);
        }
        self.w = 0;
        self.count = 0;
    }

    /// Runs `l` / `r` through the STFT; `frame(spec_l, spec_r, bins)` edits
    /// each analysis frame in place (output delayed by `n`).
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], frame: &mut FrameFn<'_>) {
        let n = self.n;
        let mask = MAX_N - 1;
        let hop = n / 4;
        // Hann² at 4× overlap sums to 1.5; the inverse FFT scales by n.
        let norm = 1.0 / (1.5 * n as f32);
        for i in 0..l.len().min(r.len()) {
            let w = self.w;
            self.inp[0][w] = l[i];
            self.inp[1][w] = r[i];
            l[i] = self.out[0][w];
            r[i] = self.out[1][w];
            self.out[0][w] = 0.0;
            self.out[1][w] = 0.0;
            self.w = (w + 1) & mask;
            self.count += 1;
            if self.count < hop {
                continue;
            }
            self.count = 0;
            let bins = n / 2 + 1;
            let (fwd, inv) = (self.plans[self.size].0.clone(), self.plans[self.size].1.clone());
            for c in 0..2 {
                // Oldest sample of the frame sits at w + 1 - n.
                let start = (w + 1 + MAX_N - n) & mask;
                for k in 0..n {
                    self.time[k] = self.inp[c][(start + k) & mask] * self.win[k];
                }
                let _ = fwd.process_with_scratch(&mut self.time[..n], &mut self.spec[c][..bins], &mut self.scratch_f);
            }
            let [a, b] = &mut self.spec;
            frame(&mut a[..bins], &mut b[..bins], bins);
            for c in 0..2 {
                let s = &mut self.spec[c][..bins];
                s[0].im = 0.0;
                s[bins - 1].im = 0.0;
                let _ = inv.process_with_scratch(s, &mut self.time[..n], &mut self.scratch_i);
                for k in 0..n {
                    let j = (w + 1 + k) & mask;
                    self.out[c][j] += self.time[k] * self.win[k] * norm;
                }
            }
        }
    }
}

// ---- Spectrogram tap ----

pub const SG_ROWS: usize = 112;
pub const SG_COLS: usize = 256;
/// Display range (Hz) of the rows (log spaced, row 0 = lowest).
pub const SG_FREQ: (f32, f32) = (30.0, 20_000.0);

/// Dry / wet magnitude columns (0..1 on a 72 dB scale), written by the
/// audio thread, read by the GUI.
pub struct SpecGram {
    dry: Box<[AtomicU32]>,
    wet: Box<[AtomicU32]>,
    col: AtomicUsize,
}

impl SpecGram {
    fn new() -> Self {
        let z = || (0..SG_ROWS * SG_COLS).map(|_| AtomicU32::new(0)).collect::<Vec<_>>().into_boxed_slice();
        Self { dry: z(), wet: z(), col: AtomicUsize::new(0) }
    }

    pub fn push(&self, dry: &[f32; SG_ROWS], wet: &[f32; SG_ROWS]) {
        let c = self.col.load(Ordering::Relaxed) % SG_COLS;
        for k in 0..SG_ROWS {
            self.dry[c * SG_ROWS + k].store(dry[k].to_bits(), Ordering::Relaxed);
            self.wet[c * SG_ROWS + k].store(wet[k].to_bits(), Ordering::Relaxed);
        }
        self.col.fetch_add(1, Ordering::Release);
    }

    /// Columns written so far.
    pub fn written(&self) -> usize {
        self.col.load(Ordering::Acquire)
    }

    /// (dry, wet) of absolute column `c`, row `k`.
    pub fn get(&self, c: usize, k: usize) -> (f32, f32) {
        let i = (c % SG_COLS) * SG_ROWS + k.min(SG_ROWS - 1);
        (f32::from_bits(self.dry[i].load(Ordering::Relaxed)), f32::from_bits(self.wet[i].load(Ordering::Relaxed)))
    }
}

impl std::fmt::Debug for SpecGram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SpecGram")
    }
}

static GRAMS: Mutex<Vec<(u64, Weak<SpecGram>)>> = Mutex::new(Vec::new());

/// The spectrogram of device `id` (shared by the engine and the GUI).
pub fn spec_gram(id: u64) -> Arc<SpecGram> {
    let mut g = GRAMS.lock().unwrap_or_else(|e| e.into_inner());
    g.retain(|(_, w)| w.strong_count() > 0);
    if let Some(t) = g.iter().find(|(i, _)| *i == id).and_then(|(_, w)| w.upgrade()) {
        return t;
    }
    let t = Arc::new(SpecGram::new());
    g.push((id, Arc::downgrade(&t)));
    t
}

/// Spectrum rows for the display: max magnitude of both channels per row.
pub fn gram_rows(a: &[Complex<f32>], b: &[Complex<f32>], n: usize, sr: f32, out: &mut [f32; SG_ROWS]) {
    let bins = a.len().min(b.len());
    let hz = sr / n as f32;
    let scale = 4.0 / n as f32;
    let ratio = (SG_FREQ.1 / SG_FREQ.0).ln();
    for (k, o) in out.iter_mut().enumerate() {
        let f0 = SG_FREQ.0 * (ratio * k as f32 / SG_ROWS as f32).exp();
        let f1 = SG_FREQ.0 * (ratio * (k + 1) as f32 / SG_ROWS as f32).exp();
        let i0 = ((f0 / hz) as usize).min(bins - 1);
        let i1 = ((f1 / hz) as usize).clamp(i0, bins - 1);
        let mut m = 0.0f32;
        for i in i0..=i1 {
            m = m.max(a[i].norm_sqr()).max(b[i].norm_sqr());
        }
        let db = 10.0 * (m.max(1e-12)).log10() + 20.0 * scale.log10();
        *o = ((db + 72.0) / 72.0).clamp(0.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_with_latency() {
        for size in 0..3 {
            let mut s = Stft::new();
            s.set_size(size);
            let n = s.n();
            let len = n * 4;
            let src: Vec<f32> = (0..len).map(|i| (i as f32 * 0.031).sin() * 0.5).collect();
            let mut l = src.clone();
            let mut r = src.clone();
            s.process(&mut l, &mut r, &mut |_, _, _| {});
            for i in 2 * n..len {
                assert!((l[i] - src[i - n]).abs() < 1e-3, "{size} {i} {} {}", l[i], src[i - n]);
            }
        }
    }
}
