//! Polyphonic subtractive synth: 2 band-limited oscillators → TPT state-variable
//! low-pass filter with envelope → amp envelope.

use super::Expr;
use super::adsr::{Adsr, AdsrParams};
use serde::{Deserialize, Serialize};
use std::f32::consts::PI;

pub const SYNTH_VOICES: usize = 16;
/// Filter coefficients are recomputed every N samples (control rate).
const CONTROL_RATE: u32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wave {
    Sine,
    Saw,
    Square,
    Triangle,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SynthParams {
    pub osc1: Wave,
    pub osc2: Wave,
    pub osc2_semitones: f32,
    pub osc2_detune_cents: f32,
    /// 0 = only osc1, 1 = only osc2.
    pub osc_mix: f32,
    pub cutoff_hz: f32,
    /// 0..1
    pub resonance: f32,
    /// Filter envelope depth in octaves.
    pub filter_env_octaves: f32,
    pub amp: AdsrParams,
    pub filter: AdsrParams,
    pub gain: f32,
}

impl Default for SynthParams {
    fn default() -> Self {
        Self {
            osc1: Wave::Saw,
            osc2: Wave::Saw,
            osc2_semitones: 0.0,
            osc2_detune_cents: 7.0,
            osc_mix: 0.5,
            cutoff_hz: 2500.0,
            resonance: 0.2,
            filter_env_octaves: 2.0,
            amp: AdsrParams::default(),
            filter: AdsrParams { attack: 0.002, decay: 0.3, sustain: 0.3, release: 0.3 },
            gain: 0.7,
        }
    }
}

impl SynthParams {
    pub const PRESETS: [&'static str; 5] = ["Init Saw", "Bass", "Pluck", "Pad", "Square Lead"];

    pub fn preset(name: &str) -> Self {
        let d = Self::default();
        match name {
            "Bass" => Self {
                osc1: Wave::Saw,
                osc2: Wave::Square,
                osc2_semitones: -12.0,
                osc2_detune_cents: 0.0,
                cutoff_hz: 400.0,
                resonance: 0.35,
                filter_env_octaves: 3.0,
                amp: AdsrParams { attack: 0.002, decay: 0.3, sustain: 0.7, release: 0.08 },
                filter: AdsrParams { attack: 0.001, decay: 0.18, sustain: 0.1, release: 0.1 },
                ..d
            },
            "Pluck" => Self {
                cutoff_hz: 600.0,
                filter_env_octaves: 4.0,
                amp: AdsrParams { attack: 0.001, decay: 0.35, sustain: 0.0, release: 0.2 },
                filter: AdsrParams { attack: 0.001, decay: 0.15, sustain: 0.0, release: 0.1 },
                ..d
            },
            "Pad" => Self {
                osc2_detune_cents: 12.0,
                cutoff_hz: 1200.0,
                resonance: 0.1,
                filter_env_octaves: 1.0,
                amp: AdsrParams { attack: 0.6, decay: 1.0, sustain: 0.8, release: 1.2 },
                filter: AdsrParams { attack: 0.8, decay: 1.5, sustain: 0.6, release: 1.0 },
                gain: 0.5,
                ..d
            },
            "Square Lead" => Self {
                osc1: Wave::Square,
                osc2: Wave::Square,
                osc2_semitones: 12.0,
                osc_mix: 0.3,
                cutoff_hz: 3000.0,
                amp: AdsrParams { attack: 0.005, decay: 0.1, sustain: 0.9, release: 0.15 },
                ..d
            },
            _ => d,
        }
    }
}

#[inline(always)]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

#[inline(always)]
fn osc(wave: Wave, phase: f32, dt: f32) -> f32 {
    match wave {
        Wave::Sine => (phase * 2.0 * PI).sin(),
        Wave::Saw => 2.0 * phase - 1.0 - poly_blep(phase, dt),
        Wave::Square => {
            let naive = if phase < 0.5 { 1.0 } else { -1.0 };
            let mut p2 = phase + 0.5;
            if p2 >= 1.0 {
                p2 -= 1.0;
            }
            naive + poly_blep(phase, dt) - poly_blep(p2, dt)
        }
        Wave::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Voice {
    active: bool,
    pitch: u8,
    from_clip: bool,
    age: u32,
    /// Samples until automatic note-off (u32::MAX = held until NoteOff).
    remaining: u32,
    vel: f32,
    phase1: f32,
    phase2: f32,
    inc1: f32,
    inc2: f32,
    amp: Adsr,
    fenv: Adsr,
    ic1: f32,
    ic2: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
    ctl: u32,
    /// Expression slot (index + 1 into `Synth::expr`), samples since note-on.
    expr: u8,
    t: u32,
    /// Base increments (before bend / MPE pitch), pressure gain.
    base1: f32,
    base2: f32,
    press: f32,
}

pub struct Synth {
    voices: [Voice; SYNTH_VOICES],
    expr: [Expr; SYNTH_VOICES],
    clock: u32,
    /// Track-wide pitch bend (semitones) and mod wheel (0..1, opens the filter).
    pub bend: f32,
    pub modw: f32,
}

impl Default for Synth {
    fn default() -> Self {
        Self { voices: [Voice::default(); SYNTH_VOICES], expr: [Expr::default(); SYNTH_VOICES], clock: 0, bend: 0.0, modw: 0.0 }
    }
}

fn midi_hz(note: f32) -> f32 {
    440.0 * ((note - 69.0) / 12.0).exp2()
}

impl Synth {
    #[allow(clippy::too_many_arguments)]
    pub fn note_on(&mut self, p: &SynthParams, pitch: u8, vel: u8, remaining: u32, from_clip: bool, sr: f32, expr: Option<&Expr>) {
        self.clock = self.clock.wrapping_add(1);
        // Free voice, else the oldest released one, else the oldest.
        let idx = self
            .voices
            .iter()
            .position(|v| !v.active)
            .or_else(|| (0..SYNTH_VOICES).filter(|&i| self.voices[i].amp.is_released()).min_by_key(|&i| self.voices[i].age))
            .unwrap_or_else(|| (0..SYNTH_VOICES).min_by_key(|&i| self.voices[i].age).unwrap_or(0));
        let v = &mut self.voices[idx];
        let f1 = midi_hz(pitch as f32);
        let f2 = midi_hz(pitch as f32 + p.osc2_semitones + p.osc2_detune_cents / 100.0);
        let was_active = v.active;
        *v = Voice {
            active: true,
            pitch,
            from_clip,
            age: self.clock,
            remaining,
            vel: vel as f32 / 127.0,
            inc1: (f1 / sr).min(0.49),
            inc2: (f2 / sr).min(0.49),
            base1: f1 / sr,
            base2: f2 / sr,
            press: 1.0,
            expr: if expr.is_some() { idx as u8 + 1 } else { 0 },
            t: 0,
            ctl: 0,
            // keep envelope level & filter state when stealing → no click
            amp: v.amp,
            fenv: v.fenv,
            ic1: if was_active { v.ic1 } else { 0.0 },
            ic2: if was_active { v.ic2 } else { 0.0 },
            ..Voice::default()
        };
        v.amp.trigger(&p.amp, sr);
        v.fenv.trigger(&p.filter, sr);
        if let Some(e) = expr {
            self.expr[idx] = *e;
        }
    }

    pub fn note_off(&mut self, pitch: u8) {
        for v in self.voices.iter_mut().filter(|v| v.active && !v.from_clip && v.pitch == pitch) {
            if v.remaining == u32::MAX {
                v.amp.release();
                v.fenv.release();
            }
        }
    }

    pub fn release_clip_notes(&mut self) {
        for v in self.voices.iter_mut().filter(|v| v.active && v.from_clip) {
            v.amp.release();
            v.fenv.release();
        }
    }

    pub fn kill(&mut self) {
        for v in &mut self.voices {
            v.active = false;
            v.amp = Adsr::default();
            v.fenv = Adsr::default();
        }
    }

    pub fn is_active(&self) -> bool {
        self.voices.iter().any(|v| v.active)
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    /// Add output into `l`/`r`.
    pub fn render(&mut self, p: &SynthParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let mix2 = p.osc_mix.clamp(0.0, 1.0);
        let mix1 = 1.0 - mix2;
        let gain = p.gain * 0.25;
        let nyq = 0.45 * sr;
        let k_base = 2.0 - 1.95 * p.resonance.clamp(0.0, 1.0);
        let (bend, modw) = (self.bend, self.modw);
        for (vi, v) in self.voices.iter_mut().enumerate().filter(|(_, v)| v.active) {
            let ex = (v.expr > 0).then(|| &self.expr[vi]);
            for (lo, ro) in l.iter_mut().zip(r.iter_mut()) {
                if v.remaining != u32::MAX {
                    if v.remaining == 0 {
                        v.amp.release();
                        v.fenv.release();
                        v.remaining = u32::MAX - 1; // released; don't re-trigger release
                    } else if v.remaining != u32::MAX - 1 {
                        v.remaining -= 1;
                    }
                }
                let fe = v.fenv.tick();
                if v.ctl == 0 {
                    // Control rate: bend + MPE pitch / slide (filter) / pressure (level).
                    let (mp, slide) = match ex {
                        Some(e) => {
                            let ts = v.t as f32;
                            v.press = e.press.eval(ts, 1.0);
                            (e.pitch.eval(ts, 0.0), e.slide.eval(ts, 0.0))
                        }
                        None => (0.0, 0.0),
                    };
                    let m = ((bend + mp) / 12.0).exp2();
                    v.inc1 = (v.base1 * m).min(0.49);
                    v.inc2 = (v.base2 * m).min(0.49);
                    let fc = (p.cutoff_hz * (p.filter_env_octaves * fe + 3.0 * (slide + modw)).exp2()).clamp(20.0, nyq);
                    let g = (PI * fc / sr).tan();
                    v.k = k_base;
                    v.a1 = 1.0 / (1.0 + g * (g + v.k));
                    v.a2 = g * v.a1;
                    v.a3 = g * v.a2;
                    v.ctl = CONTROL_RATE;
                }
                v.ctl -= 1;
                let s = mix1 * osc(p.osc1, v.phase1, v.inc1) + mix2 * osc(p.osc2, v.phase2, v.inc2);
                v.phase1 += v.inc1;
                if v.phase1 >= 1.0 {
                    v.phase1 -= 1.0;
                }
                v.phase2 += v.inc2;
                if v.phase2 >= 1.0 {
                    v.phase2 -= 1.0;
                }
                // TPT SVF low-pass
                let v3 = s - v.ic2;
                let v1 = v.a1 * v.ic1 + v.a2 * v3;
                let v2 = v.ic2 + v.a2 * v.ic1 + v.a3 * v3;
                v.ic1 = 2.0 * v1 - v.ic1;
                v.ic2 = 2.0 * v2 - v.ic2;
                v.t = v.t.saturating_add(1);
                let out = v2 * v.amp.tick() * v.vel * gain * v.press;
                *lo += out;
                *ro += out;
                if v.amp.is_idle() {
                    v.active = false;
                    break;
                }
            }
        }
    }
}
