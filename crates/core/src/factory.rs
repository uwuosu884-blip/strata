//! Built-in drum sounds, synthesised on demand (no sample files to ship).
//! Deterministic: same name + rate → same buffer.

use daw_engine::SampleBuffer;
use std::f32::consts::TAU;

pub const NAMES: [&str; 10] = ["Kick", "Snare", "Clap", "Closed Hat", "Open Hat", "Low Tom", "Hi Tom", "Rim", "Cowbell", "Crash"];

/// Synthesise a factory sound. `None` for unknown names.
pub fn generate(name: &str, sr: u32) -> Option<SampleBuffer> {
    let s = sr as f32;
    let mut rng = Rng(0x9E37_79B9 ^ name.len() as u32 * 7919);
    let secs = |t: f32| (t * s) as usize;
    let data: Vec<f32> = match name {
        "Kick" => {
            let mut ph = 0.0f32;
            (0..secs(0.5))
                .map(|i| {
                    let t = i as f32 / s;
                    let f = 45.0 + 110.0 * (-t * 30.0).exp();
                    ph += TAU * f / s;
                    let click = if i < secs(0.003) { rng.noise() * 0.3 } else { 0.0 };
                    (ph.sin() * (-t * 7.0).exp() + click) * 0.95
                })
                .collect()
        }
        "Snare" => {
            let mut hp = OnePoleHp::new(1500.0, s);
            (0..secs(0.3))
                .map(|i| {
                    let t = i as f32 / s;
                    let tone = (TAU * 185.0 * t).sin() * (-t * 25.0).exp() * 0.5;
                    let noise = hp.run(rng.noise()) * (-t * 14.0).exp() * 0.7;
                    (tone + noise) * 0.9
                })
                .collect()
        }
        "Clap" => {
            let mut bp = Biquad::bandpass(1200.0, 1.2, s);
            (0..secs(0.35))
                .map(|i| {
                    let t = i as f32 / s;
                    // Three quick bursts, then a tail.
                    let burst = (0..3).map(|k| t - k as f32 * 0.011).filter(|&d| (0.0..0.011).contains(&d)).map(|d| (-d * 300.0).exp()).sum::<f32>();
                    let tail = if t > 0.022 { (-(t - 0.022) * 18.0).exp() } else { 0.0 };
                    bp.run(rng.noise()) * (burst + tail) * 1.8
                })
                .collect()
        }
        "Closed Hat" | "Open Hat" | "Crash" => {
            let (len, decay, cut) = match name {
                "Closed Hat" => (0.08, 60.0, 7000.0),
                "Open Hat" => (0.5, 7.0, 7000.0),
                _ => (2.0, 2.2, 4500.0),
            };
            let mut hp = Biquad::highpass(cut, 0.7, s);
            // Metallic: six detuned square oscillators + noise (TR-808 style).
            let freqs = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];
            (0..secs(len))
                .map(|i| {
                    let t = i as f32 / s;
                    let metal: f32 = freqs.iter().map(|f| if (t * f * 1.7).fract() < 0.5 { 1.0 } else { -1.0 }).sum::<f32>() / 6.0;
                    hp.run(metal * 0.6 + rng.noise() * 0.5) * (-t * decay).exp() * 0.8
                })
                .collect()
        }
        "Low Tom" | "Hi Tom" => {
            let (f0, f1) = if name == "Low Tom" { (130.0, 85.0) } else { (210.0, 150.0) };
            let mut ph = 0.0f32;
            (0..secs(0.45))
                .map(|i| {
                    let t = i as f32 / s;
                    ph += TAU * (f1 + (f0 - f1) * (-t * 12.0).exp()) / s;
                    ph.sin() * (-t * 8.0).exp() * 0.85
                })
                .collect()
        }
        "Rim" => (0..secs(0.05))
            .map(|i| {
                let t = i as f32 / s;
                ((TAU * 1700.0 * t).sin() * 0.6 + rng.noise() * 0.3) * (-t * 90.0).exp()
            })
            .collect(),
        "Cowbell" => {
            let mut bp = Biquad::bandpass(800.0, 2.0, s);
            (0..secs(0.35))
                .map(|i| {
                    let t = i as f32 / s;
                    let sq = |f: f32| if (t * f).fract() < 0.5 { 1.0 } else { -1.0 };
                    bp.run((sq(540.0) + sq(800.0)) * 0.5) * (-t * 10.0).exp() * 1.5
                })
                .collect()
        }
        _ => return None,
    };
    // Normalize to -1 dBFS so every pad sits at the same level.
    let mut data = data;
    let peak = data.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if peak > 0.0 {
        let g = 0.89 / peak;
        data.iter_mut().for_each(|v| *v *= g);
    }
    Some(SampleBuffer::mono(name, sr, data))
}

struct Rng(u32);
impl Rng {
    fn noise(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

struct OnePoleHp {
    a: f32,
    x1: f32,
    y1: f32,
}
impl OnePoleHp {
    fn new(cut: f32, sr: f32) -> Self {
        Self { a: (-TAU * cut / sr).exp(), x1: 0.0, y1: 0.0 }
    }
    fn run(&mut self, x: f32) -> f32 {
        let y = self.a * (self.y1 + x - self.x1);
        self.x1 = x;
        self.y1 = y;
        y
    }
}

/// RBJ biquad (only used offline here).
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [f32; 2],
}
impl Biquad {
    fn new(b: [f32; 3], a0: f32, a: [f32; 2]) -> Self {
        Self { b: b.map(|v| v / a0), a: a.map(|v| v / a0), z: [0.0; 2] }
    }
    fn bandpass(f: f32, q: f32, sr: f32) -> Self {
        let w = TAU * f / sr;
        let al = w.sin() / (2.0 * q);
        Self::new([al, 0.0, -al], 1.0 + al, [-2.0 * w.cos(), 1.0 - al])
    }
    fn highpass(f: f32, q: f32, sr: f32) -> Self {
        let w = TAU * f / sr;
        let (c, al) = (w.cos(), w.sin() / (2.0 * q));
        Self::new([(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0], 1.0 + al, [-2.0 * c, 1.0 - al])
    }
    fn run(&mut self, x: f32) -> f32 {
        // Transposed direct form II.
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_factory_sound_is_audible_and_bounded() {
        for name in NAMES {
            let b = generate(name, 48_000).unwrap();
            let peak = b.left.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak > 0.1 && peak <= 1.0, "{name}: peak {peak}");
            assert!(b.left.iter().all(|v| v.is_finite()));
            let tail = b.left[b.len() - 10..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(tail < 0.05, "{name} must decay, tail {tail}");
        }
        assert!(generate("Nope", 48_000).is_none());
        assert_eq!(generate("Kick", 44_100).unwrap().len(), 22_050);
    }
}
