#![allow(dead_code)]
//! Shared helpers for engine integration tests.

use daw_engine::*;
use std::sync::Arc;

pub const SR: f64 = 48_000.0;
/// Samples per beat at 120 BPM / 48 kHz.
pub const SPB: usize = 24_000;

pub fn engine() -> (Engine, EngineHandle) {
    let (e, mut h) = create(EngineConfig { sample_rate: SR, max_block_frames: 512, input_channels: 2 });
    h.send(Command::SetMetronome(false)).unwrap();
    (e, h)
}

/// Render `frames` stereo frames with callbacks of `cb` frames; returns interleaved.
pub fn render(e: &mut Engine, frames: usize, cb: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; frames * 2];
    for chunk in out.chunks_mut(cb * 2) {
        e.process(chunk, 2);
    }
    out
}

pub fn left(v: &[f32]) -> Vec<f32> {
    v.iter().step_by(2).copied().collect()
}
pub fn right(v: &[f32]) -> Vec<f32> {
    v.iter().skip(1).step_by(2).copied().collect()
}

/// Indices of non-zero samples.
pub fn nonzero(v: &[f32]) -> Vec<usize> {
    v.iter().enumerate().filter(|(_, s)| s.abs() > 1e-9).map(|(i, _)| i).collect()
}

/// A one-sample impulse: a Sampler playing it marks each note-on with
/// exactly one non-zero output sample at the note's sample position.
pub fn impulse() -> Arc<SampleBuffer> {
    let mut d = vec![0.0; 64];
    d[0] = 1.0;
    Arc::new(SampleBuffer::mono("impulse", SR as u32, d))
}

pub fn impulse_sampler() -> Instrument {
    Instrument::Sampler {
        sample: Some(impulse()),
        params: SamplerParams { amp: AdsrParams { attack: 0.0, decay: 1.0, sustain: 1.0, release: 0.01 }, ..Default::default() },
    }
}

pub fn notes_at(beats: &[f64]) -> Vec<Note> {
    beats.iter().map(|&b| Note { start: b, length: 0.25, pitch: 60, velocity: 127, muted: false, mpe: 0, vel_dev: 0 }).collect()
}

pub fn midi_clip(length: f64, beats: &[f64]) -> Clip {
    Clip::new(length, true, ClipContent::Midi(notes_at(beats)))
}

pub fn track(rt: u16, slots: Vec<Option<Clip>>) -> SongTrack {
    SongTrack { rt, instrument: impulse_sampler(), slots, ..Default::default() }
}

/// MIDI track listening to every port and channel, monitor Auto.
pub fn midi_track(rt: u16, arm: bool) -> SongTrack {
    SongTrack { input: TrackInput::Midi { port: MIDI_ALL_INS, channel: 0 }, monitor: Monitor::Auto, arm, ..track(rt, vec![]) }
}

pub fn note_on(port: u8, channel: u8, pitch: u8) -> MidiInput {
    MidiInput { port, channel, event: MidiEvent::NoteOn { pitch, velocity: 127 } }
}

/// Process zero frames so queued commands/songs are applied.
pub fn apply(e: &mut Engine) {
    e.process(&mut [], 2);
}

pub fn send_all(h: &mut EngineHandle, cmds: &[Command]) {
    for c in cmds {
        h.send(*c).unwrap();
    }
}

/// Convert sample indices to beats (rounded to 1/1000).
pub fn beats(idx: &[usize]) -> Vec<f64> {
    idx.iter().map(|&i| (i as f64 / SPB as f64 * 1000.0).round() / 1000.0).collect()
}

/// Estimate frequency from rising zero crossings.
pub fn freq(v: &[f32], sr: f64) -> f64 {
    let ups: Vec<usize> = v.windows(2).enumerate().filter(|(_, w)| w[0] <= 0.0 && w[1] > 0.0).map(|(i, _)| i).collect();
    if ups.len() < 2 {
        return 0.0;
    }
    (ups.len() - 1) as f64 * sr / (ups[ups.len() - 1] - ups[0]) as f64
}

pub fn dc_clip(level: f32) -> Clip {
    let s = Arc::new(SampleBuffer::mono("dc", SR as u32, vec![level; SR as usize * 4]));
    Clip::new(
        4.0,
        true,
        ClipContent::Audio(AudioClip { sample: s, offset_frames: 0.0, warp_bpm: None, gain: 1.0, warp: Default::default() }),
    )
}

pub fn audio_track(rt: u16, clip: Clip) -> SongTrack {
    SongTrack { rt, slots: vec![Some(clip)], ..Default::default() }
}
