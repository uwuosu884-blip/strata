//! Sample-playback voices, shared by the Sampler (one sample, pitched across
//! the keyboard) and the Drum Rack (one sample per pad, one-shot).

use super::adsr::{Adsr, AdsrParams};
use crate::sample::SampleBuffer;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const SAMPLER_VOICES: usize = 16;
pub const DRUM_VOICES: usize = 32;
pub const DRUM_PADS: usize = 16;
/// MIDI note of pad 0 (C1, like Live's Drum Rack).
pub const DRUM_BASE_NOTE: u8 = 36;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SamplerParams {
    pub root_note: u8,
    pub tune_semitones: f32,
    pub gain: f32,
    pub amp: AdsrParams,
}

impl Default for SamplerParams {
    fn default() -> Self {
        Self { root_note: 60, tune_semitones: 0.0, gain: 0.8, amp: AdsrParams { attack: 0.001, decay: 0.1, sustain: 1.0, release: 0.15 } }
    }
}

#[derive(Debug, Clone)]
pub struct DrumPad {
    pub name: String,
    pub sample: Option<Arc<SampleBuffer>>,
    pub gain: f32,
    pub tune_semitones: f32,
    /// Pads in the same non-zero group cut each other off (open/closed hi-hat).
    pub choke: u8,
}

#[derive(Debug, Clone, Copy, Default)]
struct Voice {
    active: bool,
    pitch: u8,
    pad: u8,
    choke: u8,
    from_clip: bool,
    one_shot: bool,
    age: u32,
    remaining: u32,
    pos: f64,
    rate: f64,
    gain: f32,
    env: Adsr,
}

pub struct SampleVoices<const N: usize> {
    voices: [Voice; N],
    clock: u32,
}

impl<const N: usize> Default for SampleVoices<N> {
    fn default() -> Self {
        Self { voices: [Voice::default(); N], clock: 0 }
    }
}

const DRUM_ENV: AdsrParams = AdsrParams { attack: 0.0005, decay: 1.0, sustain: 1.0, release: 0.05 };

impl<const N: usize> SampleVoices<N> {
    fn alloc(&mut self) -> usize {
        self.clock = self.clock.wrapping_add(1);
        self.voices.iter().position(|v| !v.active).unwrap_or_else(|| (0..N).min_by_key(|&i| self.voices[i].age).unwrap_or(0))
    }

    /// Sampler note: pitch relative to root.
    #[allow(clippy::too_many_arguments)]
    pub fn sampler_on(&mut self, sample: &SampleBuffer, p: &SamplerParams, pitch: u8, vel: u8, remaining: u32, from_clip: bool, sr: f32) {
        let i = self.alloc();
        let semis = pitch as f64 - p.root_note as f64 + p.tune_semitones as f64;
        let rate = (semis / 12.0).exp2() * sample.sample_rate as f64 / sr as f64;
        let env = self.voices[i].env;
        self.voices[i] = Voice {
            active: true,
            pitch,
            from_clip,
            age: self.clock,
            remaining,
            rate,
            gain: p.gain * vel as f32 / 127.0,
            env,
            ..Voice::default()
        };
        self.voices[i].env.trigger(&p.amp, sr);
    }

    /// Drum pad hit (one-shot: plays to the end of the sample).
    pub fn drum_on(&mut self, pads: &[DrumPad], pitch: u8, vel: u8, from_clip: bool, sr: f32) {
        let Some(pad_idx) = pitch.checked_sub(DRUM_BASE_NOTE).map(usize::from) else { return };
        let Some(pad) = pads.get(pad_idx) else { return };
        let Some(sample) = pad.sample.as_deref() else { return };
        if pad.choke != 0 {
            for v in self.voices.iter_mut().filter(|v| v.active && v.choke == pad.choke) {
                v.env.fast_release(sr);
            }
        }
        let i = self.alloc();
        self.voices[i] = Voice {
            active: true,
            pitch,
            pad: pad_idx as u8,
            choke: pad.choke,
            from_clip,
            one_shot: true,
            age: self.clock,
            remaining: u32::MAX,
            rate: (pad.tune_semitones as f64 / 12.0).exp2() * sample.sample_rate as f64 / sr as f64,
            gain: pad.gain * vel as f32 / 127.0,
            ..Voice::default()
        };
        self.voices[i].env.trigger(&DRUM_ENV, sr);
    }

    pub fn note_off(&mut self, pitch: u8) {
        for v in self.voices.iter_mut().filter(|v| v.active && !v.one_shot && !v.from_clip && v.pitch == pitch) {
            if v.remaining == u32::MAX {
                v.env.release();
            }
        }
    }

    pub fn release_clip_notes(&mut self) {
        for v in self.voices.iter_mut().filter(|v| v.active && v.from_clip && !v.one_shot) {
            v.env.release();
        }
    }

    pub fn kill(&mut self) {
        for v in &mut self.voices {
            v.active = false;
            v.env = Adsr::default();
        }
    }

    pub fn is_active(&self) -> bool {
        self.voices.iter().any(|v| v.active)
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    /// Render. `sample_for(voice_pad)` resolves the buffer for each voice.
    pub fn render<'a>(&mut self, l: &mut [f32], r: &mut [f32], sample_for: impl Fn(u8) -> Option<&'a SampleBuffer>) {
        for v in self.voices.iter_mut().filter(|v| v.active) {
            let Some(s) = sample_for(v.pad) else {
                v.active = false;
                continue;
            };
            let end = s.len() as f64;
            for (lo, ro) in l.iter_mut().zip(r.iter_mut()) {
                if v.remaining != u32::MAX {
                    if v.remaining == 0 {
                        v.env.release();
                        v.remaining = u32::MAX;
                    } else {
                        v.remaining -= 1;
                    }
                }
                let (a, b) = s.read(v.pos);
                let g = v.env.tick() * v.gain;
                *lo += a * g;
                *ro += b * g;
                v.pos += v.rate;
                if v.pos >= end || v.env.is_idle() {
                    v.active = false;
                    break;
                }
            }
        }
    }
}
