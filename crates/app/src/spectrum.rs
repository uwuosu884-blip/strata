//! EQ Eight analyzer: reads the engine's output tap of a device and runs a
//! Hann-windowed FFT every frame; the shown spectrum follows it with a time
//! constant set by Refresh (ms) × Avg, so it moves smoothly at any frame rate.

use daw_engine::{SpecTap, spectrum_tap};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

pub struct EqSpec {
    tap: Arc<SpecTap>,
    /// Smoothed power per bin (len = block / 2 + 1).
    pub pow: Vec<f32>,
    pub block: usize,
    /// Something above −100 dB is shown (the display still moves).
    pub live: bool,
    last: Option<Instant>,
    /// Tap position at the last analysis (no new audio → decay to silence).
    seen: usize,
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
            live: false,
            last: None,
            seen: 0,
            buf: Vec::new(),
            re: Vec::new(),
            im: Vec::new(),
            win: Vec::new(),
        })
    }

    /// Analyse the newest block (at most every 4 ms) and return the smoothed
    /// power spectrum. Rises follow in Refresh × 0.35, falls in Refresh × Avg.
    pub fn update(&mut self, id: u64, block: u32, refresh_ms: f32, avg: f32) -> &EqSpec {
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
        let dt = e.last.map_or(1.0, |t| now.duration_since(t).as_secs_f32());
        if dt >= 0.004 {
            e.last = Some(now);
            let written = e.tap.written();
            let fresh = written != e.seen;
            e.seen = written;
            if fresh {
                e.tap.read(&mut e.buf);
            } else {
                e.buf.iter_mut().for_each(|x| *x = 0.0);
            }
            for i in 0..n {
                e.re[i] = e.buf[i] * e.win[i];
                e.im[i] = 0.0;
            }
            fft(&mut e.re, &mut e.im);
            // Full-scale sine = 0 dB: amplitude 2|X| / Σw, Σw = n / 2.
            let k = 4.0 / n as f32;
            let tau = (refresh_ms.clamp(1.0, 2000.0) / 1000.0).max(0.001);
            let up = 1.0 - (-dt / (0.35 * tau)).exp();
            let down = 1.0 - (-dt / (tau * avg.max(1.0))).exp();
            for (b, p) in e.pow.iter_mut().enumerate() {
                let m = (e.re[b] * e.re[b] + e.im[b] * e.im[b]) * k * k;
                let a = if m > *p { up } else { down };
                *p += (m - *p) * a;
            }
            e.live = e.pow.iter().any(|&p| p > 1e-10);
        }
        self.map.get(&id).expect("entry")
    }

    /// Clear a device's analyzer (device off / analyzer off: empty display).
    pub fn clear(&mut self, id: u64) {
        if let Some(e) = self.map.get_mut(&id) {
            e.pow.iter_mut().for_each(|p| *p = 0.0);
            e.live = false;
        }
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

/// The analyzer curve for `xs.len()` pixel columns: `freq(x)` maps a column
/// edge to Hz. Each column takes the loudest bin it covers (interpolated
/// where bins are wider than a column), then neighbouring columns are mixed
/// (≈ 1/90 octave): a smooth curve on the log axis.
pub fn curve_db(spec: &EqSpec, sr: f32, cols: usize, freq: impl Fn(f32) -> f32) -> Vec<f32> {
    let raw: Vec<f32> = (0..cols)
        .map(|i| {
            let (f0, f1) = (freq(i as f32 - 0.5).max(1.0), freq(i as f32 + 0.5).max(1.0));
            10f32.powf(spec.level_db(f0, f1, sr) / 10.0)
        })
        .collect();
    // Smoothing width in columns ∝ display width (a constant octave fraction).
    let per_oct = cols as f32 / (freq(cols as f32) / freq(0.0)).max(2.0).log2();
    let sigma = (per_oct / 90.0).clamp(0.5, 3.0);
    let rad = (2.5 * sigma).ceil() as isize;
    let w: Vec<f32> = (-rad..=rad).map(|d| (-(d * d) as f32 / (2.0 * sigma * sigma)).exp()).collect();
    (0..cols as isize)
        .map(|i| {
            let (mut acc, mut ws) = (0.0f32, 0.0f32);
            for (j, wj) in (-rad..=rad).zip(&w) {
                let k = i + j;
                if k >= 0 && (k as usize) < cols {
                    acc += raw[k as usize] * wj;
                    ws += wj;
                }
            }
            10.0 * (acc / ws.max(1e-9)).max(1e-20).log10()
        })
        .collect()
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
            live: true,
            last: None,
            seen: 0,
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

    #[test]
    fn curve_is_smooth_and_keeps_a_tone_visible() {
        let mut pow = vec![1e-12f32; 513];
        pow[100] = 1.0;
        let s = EqSpec {
            tap: spectrum_tap(u64::MAX - 1),
            pow,
            block: 1024,
            live: true,
            last: None,
            seen: 0,
            buf: vec![],
            re: vec![],
            im: vec![],
            win: vec![],
        };
        let sr = 48_000.0;
        // 400 columns over 10 Hz..22 kHz (log).
        let f = |x: f32| 10.0 * (2200f32).powf(x / 400.0);
        let db = curve_db(&s, sr, 400, f);
        let peak = db.iter().cloned().fold(f32::MIN, f32::max);
        assert!(peak > -6.0, "{peak}");
        assert!(db.iter().all(|d| d.is_finite()));
    }
}
