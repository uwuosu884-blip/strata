//! The real-time song: everything the audio thread needs to play, built on the
//! GUI thread from the project and swapped in atomically (old one is sent
//! back for deallocation, so the audio thread never frees memory).

use crate::instruments::Instrument;
use crate::sample::SampleBuffer;
use std::sync::Arc;

pub const MAX_TRACKS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    /// Start in beats from clip start.
    pub start: f64,
    pub length: f64,
    pub pitch: u8,
    pub velocity: u8,
}

#[derive(Debug, Clone)]
pub struct AudioClip {
    pub sample: Arc<SampleBuffer>,
    /// Where playback starts inside the sample, in sample frames.
    pub offset_frames: f64,
    /// Re-pitch warp: the sample's own tempo. Playback speed follows the song
    /// tempo (like Live's "Re-Pitch" mode). `None` = play at original speed.
    pub warp_bpm: Option<f64>,
    pub gain: f32,
}

#[derive(Debug, Clone)]
pub enum ClipContent {
    Audio(AudioClip),
    /// Must be sorted by `start` (see [`Song::prepare`]).
    Midi(Vec<Note>),
}

#[derive(Debug, Clone)]
pub struct Clip {
    pub length: f64,
    pub looping: bool,
    pub content: ClipContent,
}

#[derive(Debug, Clone)]
pub struct ArrangementClip {
    /// Song position (beats) of the clip start.
    pub start: f64,
    pub clip: Clip,
}

#[derive(Debug, Clone)]
pub struct SongTrack {
    /// Index into the engine's per-track state pool (stable across edits).
    pub rt: u16,
    pub gain: f32,
    /// -1 (left) .. 1 (right)
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    /// Pass live audio input through this track.
    pub monitor: bool,
    pub instrument: Instrument,
    /// Session view slots, index = scene.
    pub slots: Vec<Option<Clip>>,
    /// Must be sorted by `start`.
    pub arrangement: Vec<ArrangementClip>,
}

impl Default for SongTrack {
    fn default() -> Self {
        Self {
            rt: 0,
            gain: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            monitor: false,
            instrument: Instrument::None,
            slots: Vec::new(),
            arrangement: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Song {
    pub tracks: Vec<SongTrack>,
}

impl Song {
    /// Normalise before sending to the audio thread: sort notes and
    /// arrangement clips, drop invalid data, enforce track limits.
    pub fn prepare(&mut self) {
        self.tracks.retain(|t| (t.rt as usize) < MAX_TRACKS);
        let mut seen = [false; MAX_TRACKS];
        self.tracks.retain(|t| !std::mem::replace(&mut seen[t.rt as usize], true));
        for t in &mut self.tracks {
            t.gain = if t.gain.is_finite() { t.gain.clamp(0.0, 4.0) } else { 0.0 };
            t.pan = if t.pan.is_finite() { t.pan.clamp(-1.0, 1.0) } else { 0.0 };
            for clip in t.slots.iter_mut().flatten().chain(t.arrangement.iter_mut().map(|a| &mut a.clip)) {
                prepare_clip(clip);
            }
            t.arrangement.retain(|a| a.start.is_finite() && a.start >= 0.0);
            t.arrangement.sort_by(|a, b| a.start.total_cmp(&b.start));
        }
    }

    /// End of the last arrangement clip, in beats.
    pub fn arrangement_end(&self) -> f64 {
        self.tracks
            .iter()
            .flat_map(|t| t.arrangement.iter().map(|a| a.start + a.clip.length))
            .fold(0.0, f64::max)
    }
}

fn prepare_clip(clip: &mut Clip) {
    if !clip.length.is_finite() || clip.length < 1.0 / 64.0 {
        clip.length = 1.0 / 64.0;
    }
    if let ClipContent::Midi(notes) = &mut clip.content {
        notes.retain(|n| n.start.is_finite() && n.length.is_finite() && n.start >= 0.0 && n.pitch < 128);
        for n in notes.iter_mut() {
            n.length = n.length.max(1.0 / 256.0);
            n.velocity = n.velocity.clamp(1, 127);
        }
        notes.sort_by(|a, b| a.start.total_cmp(&b.start));
    }
}
