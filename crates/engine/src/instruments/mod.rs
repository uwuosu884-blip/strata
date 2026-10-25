pub mod adsr;
pub mod sampler;
pub mod synth;

use crate::sample::SampleBuffer;
use sampler::{DRUM_VOICES, DrumPad, SAMPLER_VOICES, SampleVoices, SamplerParams};
use std::sync::Arc;
use synth::{Synth, SynthParams};

/// Instrument definition (part of the song; immutable on the audio thread).
#[derive(Debug, Clone, Default)]
pub enum Instrument {
    #[default]
    None,
    Synth(SynthParams),
    Sampler {
        sample: Option<Arc<SampleBuffer>>,
        params: SamplerParams,
    },
    Drums(Vec<DrumPad>),
}

/// Per-track voice state for every instrument type, pre-allocated so that
/// switching instruments never allocates on the audio thread.
#[derive(Default)]
pub struct Voices {
    synth: Synth,
    sampler: SampleVoices<SAMPLER_VOICES>,
    drums: SampleVoices<DRUM_VOICES>,
}

impl Voices {
    /// `remaining` = samples until auto note-off (u32::MAX = held).
    pub fn note_on(&mut self, def: &Instrument, pitch: u8, vel: u8, remaining: u32, from_clip: bool, sr: f32) {
        if vel == 0 {
            return self.note_off(pitch);
        }
        match def {
            Instrument::None => {}
            Instrument::Synth(p) => self.synth.note_on(p, pitch, vel, remaining, from_clip, sr),
            Instrument::Sampler { sample: Some(s), params } => self.sampler.sampler_on(s, params, pitch, vel, remaining, from_clip, sr),
            Instrument::Sampler { sample: None, .. } => {}
            Instrument::Drums(pads) => self.drums.drum_on(pads, pitch, vel, from_clip, sr),
        }
    }

    pub fn note_off(&mut self, pitch: u8) {
        self.synth.note_off(pitch);
        self.sampler.note_off(pitch);
        self.drums.note_off(pitch);
    }

    pub fn release_clip_notes(&mut self) {
        self.synth.release_clip_notes();
        self.sampler.release_clip_notes();
        self.drums.release_clip_notes();
    }

    pub fn kill(&mut self) {
        self.synth.kill();
        self.sampler.kill();
        self.drums.kill();
    }

    pub fn is_active(&self) -> bool {
        self.synth.is_active() || self.sampler.is_active() || self.drums.is_active()
    }

    pub fn active_voices(&self) -> usize {
        self.synth.active_voices() + self.sampler.active_voices() + self.drums.active_voices()
    }

    pub fn render(&mut self, def: &Instrument, l: &mut [f32], r: &mut [f32], sr: f32) {
        match def {
            Instrument::None => {}
            Instrument::Synth(p) => {
                if self.synth.is_active() {
                    self.synth.render(p, l, r, sr)
                }
            }
            Instrument::Sampler { sample, .. } => {
                if self.sampler.is_active() {
                    let s = sample.as_deref();
                    self.sampler.render(l, r, |_| s);
                }
            }
            Instrument::Drums(pads) => {
                if self.drums.is_active() {
                    self.drums.render(l, r, |pad| pads.get(pad as usize).and_then(|p| p.sample.as_deref()));
                }
            }
        }
    }
}
