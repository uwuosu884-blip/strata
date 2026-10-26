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

/// Individual drum pad output buffer (one per pad, per track).
pub struct PadBuf {
    pub l: Vec<f32>,
    pub r: Vec<f32>,
}

/// Breakpoints in samples since note-on (fixed size: copied into voices).
#[derive(Debug, Clone, Copy)]
pub struct Curve {
    pub n: u8,
    pub t: [f32; Curve::MAX],
    pub v: [f32; Curve::MAX],
}

impl Default for Curve {
    fn default() -> Self {
        Self { n: 0, t: [0.0; Self::MAX], v: [0.0; Self::MAX] }
    }
}

impl Curve {
    pub const MAX: usize = 32;
    /// From `(beats, value)` points; `spb` = samples per beat. Long curves are thinned.
    pub fn from_points(pts: &[(f64, f32)], spb: f64) -> Self {
        let mut c = Self::default();
        let step = pts.len().div_ceil(Self::MAX).max(1);
        for (i, p) in pts.iter().step_by(step).take(Self::MAX).enumerate() {
            c.t[i] = (p.0 * spb) as f32;
            c.v[i] = p.1;
            c.n = i as u8 + 1;
        }
        c
    }
    #[inline]
    pub fn eval(&self, s: f32, none: f32) -> f32 {
        let n = self.n as usize;
        if n == 0 {
            return none;
        }
        if s <= self.t[0] {
            return self.v[0];
        }
        for i in 1..n {
            if s < self.t[i] {
                let k = (s - self.t[i - 1]) / (self.t[i] - self.t[i - 1]).max(1e-6);
                return self.v[i - 1] + (self.v[i] - self.v[i - 1]) * k;
            }
        }
        self.v[n - 1]
    }
}

/// Per-note expression (MPE): pitch (semitones), slide, pressure (0..1).
#[derive(Debug, Clone, Copy, Default)]
pub struct Expr {
    pub pitch: Curve,
    pub slide: Curve,
    pub press: Curve,
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
        self.note_on_expr(def, pitch, vel, remaining, from_clip, sr, None);
    }

    /// Note-on with optional per-note expression (synth only).
    #[allow(clippy::too_many_arguments)]
    pub fn note_on_expr(&mut self, def: &Instrument, pitch: u8, vel: u8, remaining: u32, from_clip: bool, sr: f32, expr: Option<&Expr>) {
        if vel == 0 {
            return self.note_off(pitch);
        }
        match def {
            Instrument::None => {}
            Instrument::Synth(p) => self.synth.note_on(p, pitch, vel, remaining, from_clip, sr, expr),
            Instrument::Sampler { sample: Some(s), params } => self.sampler.sampler_on(s, params, pitch, vel, remaining, from_clip, sr),
            Instrument::Sampler { sample: None, .. } => {}
            Instrument::Drums(pads) => self.drums.drum_on(pads, pitch, vel, from_clip, sr),
        }
    }

    /// Track-wide controllers from clip envelopes: pitch bend (semitones), mod wheel (0..1).
    pub fn set_controls(&mut self, bend: f32, modw: f32) {
        self.synth.bend = bend;
        self.synth.modw = modw;
        self.sampler.bend = bend;
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

    /// Render, with drum pads in `pad_outs` going to their own buffers
    /// (`pads[k]` = (left, right) for pad k, same range as `l`/`r`).
    #[allow(clippy::too_many_arguments)]
    pub fn render_split(
        &mut self,
        def: &Instrument,
        l: &mut [f32],
        r: &mut [f32],
        sr: f32,
        pad_outs: u32,
        pads: &mut [PadBuf],
        range: (usize, usize),
    ) {
        if let (Instrument::Drums(defs), true) = (def, pad_outs != 0) {
            if !self.drums.is_active() {
                return;
            }
            let s = |pad: u8| defs.get(pad as usize).and_then(|p| p.sample.as_deref());
            self.drums.render_pads(l, r, s, |p| pad_outs & (1 << p) == 0);
            let mut m = pad_outs;
            while m != 0 {
                let k = m.trailing_zeros() as u8;
                m &= m - 1;
                if let Some(pb) = pads.get_mut(k as usize) {
                    let (pl, pr) = (&mut pb.l[range.0..range.1], &mut pb.r[range.0..range.1]);
                    self.drums.render_pads(pl, pr, s, |p| p == k);
                }
            }
            return;
        }
        self.render(def, l, r, sr);
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
