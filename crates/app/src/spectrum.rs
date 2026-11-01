//! EQ Eight analyzer: reads the engine's output tap of a device, runs a
//! Hann-windowed FFT every Refresh ms and averages Avg blocks.

use daw_engine::{SpecTap, spectrum_tap};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct EqSpec {
    tap: Arc<SpecTap>,
    /// Smoothed power per bin (len = block / 2 + 1).
    pub pow: Vec<f32>,
    pub block: usize,
    last: Option<Instant>,
    buf: Vec<f32>,
    re: Vec<f32>,
    im: Vec<f32>,
    win: Vec<f32>,
}

#[derive(Default)]
pub struct Spectra {
    map: HashMap<u64, EqSpec>,
}

impl Spectra {
    /// Engine sample rate seen by a device's tap.
    pub fn sample_rate(&mut self, id: u64) -> f32 {
        self.entry(id).tap.sample_rate()
    }

    fn entry(&mut self, id: u64) -> &mut EqSpec {
        self.map.entry(id).or_insert_with(|| EqSpec {
            tap: spectrum_tap(id),
            pow: Vec::new(),
            block: 0,
            last: None,
            buf: Vec::new(),
            re: Vec::new(),
            im: Vec::new(),
            win: Vec::new(),
        })
    }

    /// Update (when due) and return the averaged power spectrum and the
    /// time until the next refresh.
    pub fn update(&mut self, id: u64, block: u32, refresh_ms: f32, avg: f32) -> (&EqSpec, Duration) {
        let refresh = Duration::from_secs_f32(refresh_ms.clamp(1.0, 2000.0) / 1000.0);
        let e = self.entry(id);
        let n = (block as usize).clamp(256, daw_engine::eq8::TAP_LEN).next_power_of_two();
        let now = Instant::now();
        if e.block != n {
            e.block = n;
            e.pow = vec![0.0; n / 2 + 1];
            e.buf = vec![0.0; n];
            e.re = vec![0.0; n];
            e.im = vec![0.0; n];
            e.win = (0..n).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos()).collect();
            e.last = None;
        }
        let due = e.last.is_none_or(|t| now.duration_since(t) >= refresh);
        if due {
            e.last = Some(now);
            e.tap.read(&mut e.buf);
            for i in 0..n {
                e.re[i] = e.buf[i] * e.win[i];
                e.im[i] = 0.0;
            }
            fft(&mut e.re, &mut e.im);
            // Full-scale sine = 0 dB: amplitude 2|X| / Σw, Σw = n / 2.
            let k = 4.0 / n as f32;
            let a = 1.0 / avg.max(1.0);
            for (b, p) in e.pow.iter_mut().enumerate() {
                let m = (e.re[b] * e.re[b] + e.im[b] * e.im[b]) * k * k;
                *p = if a >= 1.0 { m } else { *p + (m - *p) * a };
            }
        }
        let wait = e.last.map(|t| refresh.saturating_sub(now.duration_since(t))).unwrap_or(refresh);
        (self.map.get(&id).expect("entry"), wait)
    }

    /// Forget devices that no longer exist.
    pub fn retain(&mut self, alive: impl Fn(u64) -> bool) {
        if !self.map.is_empty() {
            self.map.retain(|id, _| alive(*id));
        }
    }
}

impl EqSpec {
    /// Level (dB) between `f0` and `f1` Hz: the loudest bin when the range
    /// spans bins, else a Catmull-Rom interpolation (smooth when zoomed in).
    pub fn level_db(&self, f0: f32, f1: f32, sr: f32) -> f32 {
        let n = self.block as f32;
        let last = self.pow.len().saturating_sub(1);
        if last == 0 {
            return -200.0;
        }
        let (k0, k1) = (f0 * n / sr, f1 * n / sr);
        let db = |i: isize| 10.0 * self.pow[i.clamp(0, last as isize) as usize].max(1e-20).log10();
        if k1 - k0 >= 1.0 {
            let (a, b) = (k0.ceil() as usize, (k1.floor() as usize).min(last));
            let m = self.pow[a.min(last)..=b.max(a.min(last))].iter().fold(0.0f32, |m, &x| m.max(x));
            return 10.0 * m.max(1e-20).log10();
        }
        let k = 0.5 * (k0 + k1);
        let i = k.floor() as isize;
        let t = k - i as f32;
        let (p0, p1, p2, p3) = (db(i - 1), db(i), db(i + 1), db(i + 2));
        let t2 = t * t;
        0.5 * (2.0 * p1 + (p2 - p0) * t + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2 + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t2 * t)
    }
}

/// In-place iterative radix-2 FFT (`re.len()` a power of two).
pub fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    debug_assert!(n.is_power_of_two() && im.len() == n);
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        let (ws, wc) = ang.sin_cos();
        for s in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f64, 0.0f64);
            for k in 0..len / 2 {
                let (a, b) = (s + k, s + k + len / 2);
                let (xr, xi) = (re[b] as f64 * cr - im[b] as f64 * ci, re[b] as f64 * ci + im[b] as f64 * cr);
                re[b] = (re[a] as f64 - xr) as f32;
                im[b] = (im[a] as f64 - xi) as f32;
                re[a] = (re[a] as f64 + xr) as f32;
                im[a] = (im[a] as f64 + xi) as f32;
                let t = cr * wc - ci * ws;
                ci = cr * ws + ci * wc;
                cr = t;
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_finds_a_sine_at_0_db() {
        let n = 1024;
        let mut re: Vec<f32> = (0..n).map(|i| (2.0 * std::f32::consts::PI * 64.0 * i as f32 / n as f32).sin()).collect();
        let win: Vec<f32> = (0..n).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos()).collect();
        re.iter_mut().zip(&win).for_each(|(x, w)| *x *= w);
        let mut im = vec![0.0; n];
        fft(&mut re, &mut im);
        let mag = |k: usize| (re[k] * re[k] + im[k] * im[k]).sqrt() * 4.0 / n as f32;
        assert!((mag(64) - 1.0).abs() < 1e-3, "{}", mag(64));
        assert!(mag(200) < 1e-3);
    }

    #[test]
    fn level_takes_peak_or_interpolates() {
        let s = EqSpec {
            tap: spectrum_tap(u64::MAX),
            pow: vec![1e-6, 1.0, 1e-6, 1e-6, 1e-6],
            block: 8,
            last: None,
            buf: vec![],
            re: vec![],
            im: vec![],
            win: vec![],
        };
        // Bin 1 = 1000 Hz at 8 kHz / 8.
        assert!(s.level_db(900.0, 2100.0, 8000.0).abs() < 1e-3);
        assert!(s.level_db(1000.0, 1000.0, 8000.0).abs() < 1e-3);
        assert!(s.level_db(3000.0, 3000.0, 8000.0) < -50.0);
    }
}
