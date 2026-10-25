use serde::{Deserialize, Serialize};

/// Envelope times in seconds, sustain 0..1.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AdsrParams {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

impl Default for AdsrParams {
    fn default() -> Self {
        Self { attack: 0.003, decay: 0.2, sustain: 0.8, release: 0.2 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Linear attack, exponential decay/release (reach ~1 % in the given time).
#[derive(Debug, Clone, Copy)]
pub struct Adsr {
    stage: Stage,
    level: f32,
    attack_step: f32,
    decay_coef: f32,
    sustain: f32,
    release_coef: f32,
}

const LN_001: f32 = -4.605_17;
const OFF: f32 = 1.0e-4;

impl Default for Adsr {
    fn default() -> Self {
        Self { stage: Stage::Idle, level: 0.0, attack_step: 1.0, decay_coef: 0.0, sustain: 1.0, release_coef: 0.0 }
    }
}

impl Adsr {
    /// Start (or restart from the current level — no click on retrigger).
    pub fn trigger(&mut self, p: &AdsrParams, sr: f32) {
        self.attack_step = 1.0 / (p.attack.max(0.0005) * sr);
        self.decay_coef = (LN_001 / (p.decay.max(0.001) * sr)).exp();
        self.release_coef = (LN_001 / (p.release.max(0.001) * sr)).exp();
        self.sustain = p.sustain.clamp(0.0, 1.0);
        self.stage = Stage::Attack;
    }

    pub fn release(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    /// Very fast release (choke / voice steal), ~2 ms.
    pub fn fast_release(&mut self, sr: f32) {
        self.release_coef = (LN_001 / (0.002 * sr)).exp();
        self.release();
    }

    #[inline]
    pub fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }

    #[inline]
    pub fn is_released(&self) -> bool {
        matches!(self.stage, Stage::Release | Stage::Idle)
    }

    #[inline]
    pub fn tick(&mut self) -> f32 {
        match self.stage {
            Stage::Idle => return 0.0,
            Stage::Attack => {
                self.level += self.attack_step;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level = self.sustain + (self.level - self.sustain) * self.decay_coef;
                if (self.level - self.sustain).abs() < OFF {
                    self.level = self.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => {}
            Stage::Release => {
                self.level *= self.release_coef;
                if self.level < OFF {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape() {
        let sr = 1000.0;
        let p = AdsrParams { attack: 0.01, decay: 0.1, sustain: 0.5, release: 0.1 };
        let mut e = Adsr::default();
        e.trigger(&p, sr);
        let attack: Vec<f32> = (0..10).map(|_| e.tick()).collect();
        assert!((attack[9] - 1.0).abs() < 1e-5, "{attack:?}");
        for _ in 0..500 {
            e.tick();
        }
        assert_eq!(e.tick(), 0.5);
        e.release();
        for _ in 0..100 {
            e.tick();
        }
        assert!(e.tick() < 0.01);
        for _ in 0..200 {
            e.tick();
        }
        assert!(e.is_idle());
    }
}
