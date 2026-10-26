//! The real-time song: everything the audio thread needs to play, built on the
//! GUI thread from the project and swapped in atomically (old one is sent
//! back for deallocation, so the audio thread never frees memory).

use crate::instruments::Instrument;
use crate::sample::SampleBuffer;
use std::sync::Arc;

pub const MAX_TRACKS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Note {
    /// Start in beats from clip start.
    pub start: f64,
    pub length: f64,
    pub pitch: u8,
    pub velocity: u8,
    /// Deactivated (key 0): kept in the clip but never played.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub muted: bool,
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

/// Live-style monitoring (In / Auto / Off).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Monitor {
    /// Always monitor the input; clip output is suppressed.
    In,
    /// Monitor while armed (audio: only while no clip plays on the track).
    #[default]
    Auto,
    Off,
}

/// Where a track taps another track's audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Tap {
    PreFx,
    /// No FX yet: same signal as `PreFx`.
    #[default]
    PostFx,
    /// After volume/pan/mute/solo.
    PostMixer,
    /// Individual drum pad output (removed from the source's own mix).
    Pad(u8),
}

/// MIDI port index meaning "every port incl. the computer keyboard".
pub const MIDI_ALL_INS: u8 = 254;
/// MIDI port index of the computer keyboard.
pub const MIDI_KEYBOARD: u8 = 255;
/// Port that matches nothing (configured port not present).
pub const MIDI_NO_PORT: u8 = 253;
/// Device channels addressable by routing.
pub const MAX_IO_CHANNELS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrackInput {
    #[default]
    None,
    /// Device input channel `ch` (0-based); stereo uses `ch` and `ch + 1`.
    /// Mono is duplicated to both sides.
    Ext { ch: u16, stereo: bool },
    /// The main output (delayed by one internal block).
    Resampling,
    /// Another track's audio.
    Track { rt: u16, tap: Tap },
    /// Hardware MIDI: `port` index (or `MIDI_ALL_INS` / `MIDI_KEYBOARD`);
    /// `channel` 0 = all, 1..=16.
    Midi { port: u8, channel: u8 },
    /// MIDI played by another track (clips + its monitored input).
    MidiTrack { rt: u16 },
}

impl TrackInput {
    pub fn is_audio(&self) -> bool {
        matches!(self, Self::Ext { .. } | Self::Resampling | Self::Track { .. })
    }
    fn source(&self) -> Option<u16> {
        match *self {
            Self::Track { rt, .. } | Self::MidiTrack { rt } => Some(rt),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrackOutput {
    #[default]
    Master,
    /// Into another track (group bus, or an audio track after its input stage).
    Track(u16),
    /// Device output channel(s); mono = (L + R) / 2.
    Ext {
        ch: u16,
        stereo: bool,
    },
    None,
    /// Notes to a hardware MIDI output (`channel` 1..=16).
    MidiPort {
        port: u8,
        channel: u8,
    },
    /// Notes to another track: its instrument directly, or its input ("Track In").
    MidiTrack {
        rt: u16,
        instrument: bool,
    },
}

impl TrackOutput {
    fn dest(&self) -> Option<u16> {
        match *self {
            Self::Track(rt) | Self::MidiTrack { rt, .. } => Some(rt),
            _ => None,
        }
    }
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
    pub arm: bool,
    pub monitor: Monitor,
    pub input: TrackInput,
    pub output: TrackOutput,
    pub instrument: Instrument,
    /// Session view slots, index = scene.
    pub slots: Vec<Option<Clip>>,
    /// Must be sorted by `start`.
    pub arrangement: Vec<ArrangementClip>,
    /// Filled by [`Song::prepare`]: drum pads tapped by other tracks (bit = pad).
    pub pad_outs: u32,
    /// Filled by [`Song::prepare`]: runtime tracks whose input is this track's MIDI.
    pub midi_listeners: u64,
}

impl Default for SongTrack {
    fn default() -> Self {
        Self {
            rt: 0,
            gain: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            arm: false,
            monitor: Monitor::Off,
            input: TrackInput::None,
            output: TrackOutput::Master,
            instrument: Instrument::None,
            slots: Vec::new(),
            arrangement: Vec::new(),
            pad_outs: 0,
            midi_listeners: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Song {
    pub tracks: Vec<SongTrack>,
    /// Main output: first device channel and stereo / mono.
    pub master_out: (u16, bool),
    /// Filled by [`Song::prepare`]: processing order (indices into `tracks`);
    /// every track comes after the tracks it reads from.
    pub order: Vec<u16>,
}

impl Default for Song {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl Song {
    pub fn new(tracks: Vec<SongTrack>) -> Self {
        Self { tracks, master_out: (0, true), order: Vec::new() }
    }

    /// Normalise before sending to the audio thread: sort notes and
    /// arrangement clips, drop invalid data and routing cycles, compute the
    /// processing order.
    pub fn prepare(&mut self) {
        self.tracks.retain(|t| (t.rt as usize) < MAX_TRACKS);
        let mut seen = [false; MAX_TRACKS];
        self.tracks.retain(|t| !std::mem::replace(&mut seen[t.rt as usize], true));
        if self.master_out.0 as usize >= MAX_IO_CHANNELS {
            self.master_out = (0, true);
        }
        for t in &mut self.tracks {
            t.gain = if t.gain.is_finite() { t.gain.clamp(0.0, 4.0) } else { 0.0 };
            t.pan = if t.pan.is_finite() { t.pan.clamp(-1.0, 1.0) } else { 0.0 };
            for clip in t.slots.iter_mut().flatten().chain(t.arrangement.iter_mut().map(|a| &mut a.clip)) {
                prepare_clip(clip);
            }
            t.arrangement.retain(|a| a.start.is_finite() && a.start >= 0.0);
            t.arrangement.sort_by(|a, b| a.start.total_cmp(&b.start));
            // Dangling / self references and out-of-range channels.
            if t.output.dest().is_some_and(|d| d == t.rt || !seen[d as usize]) {
                t.output = TrackOutput::Master;
            }
            if t.input.source().is_some_and(|s| s == t.rt || !seen[s as usize]) {
                t.input = TrackInput::None;
            }
            if let TrackInput::Ext { ch, .. } = t.input
                && ch as usize >= MAX_IO_CHANNELS
            {
                t.input = TrackInput::None;
            }
            if let TrackOutput::Ext { ch, .. } = t.output
                && ch as usize >= MAX_IO_CHANNELS
            {
                t.output = TrackOutput::Master;
            }
            if let TrackInput::Midi { channel, .. } = &mut t.input {
                *channel = (*channel).min(16);
            }
            if let TrackOutput::MidiPort { channel, .. } = &mut t.output {
                *channel = (*channel).clamp(1, 16);
            }
        }
        self.sort_topologically();
        let mut index = [usize::MAX; MAX_TRACKS];
        for (i, t) in self.tracks.iter().enumerate() {
            index[t.rt as usize] = i;
        }
        for t in &mut self.tracks {
            t.pad_outs = 0;
            t.midi_listeners = 0;
        }
        for i in 0..self.tracks.len() {
            let (rt, input) = (self.tracks[i].rt, self.tracks[i].input);
            match input {
                TrackInput::Track { rt: src, tap: Tap::Pad(p) } if (p as usize) < crate::DRUM_PADS => {
                    self.tracks[index[src as usize]].pad_outs |= 1 << p;
                }
                TrackInput::MidiTrack { rt: src } => self.tracks[index[src as usize]].midi_listeners |= 1 << rt,
                _ => {}
            }
        }
    }

    /// Kahn's algorithm in list order; a cycle is broken by dropping the
    /// routing that feeds the earliest remaining track.
    fn sort_topologically(&mut self) {
        let n = self.tracks.len();
        let mut index = [usize::MAX; MAX_TRACKS];
        for (i, t) in self.tracks.iter().enumerate() {
            index[t.rt as usize] = i;
        }
        let mut done = vec![false; n];
        self.order.clear();
        // Track `j` must wait for `i` when it reads from `i`.
        let deps = |tracks: &[SongTrack], j: usize, done: &[bool]| -> bool {
            let me = tracks[j].rt;
            tracks[j].input.source().is_some_and(|s| !done[index[s as usize]])
                || tracks.iter().enumerate().any(|(i, t)| !done[i] && t.output.dest() == Some(me))
        };
        while self.order.len() < n {
            if let Some(j) = (0..n).find(|&j| !done[j] && !deps(&self.tracks, j, &done)) {
                done[j] = true;
                self.order.push(j as u16);
                continue;
            }
            // Cycle: cut every pending edge into the first remaining track.
            let j = (0..n).find(|&j| !done[j]).expect("remaining track");
            let me = self.tracks[j].rt;
            if self.tracks[j].input.source().is_some_and(|s| !done[index[s as usize]]) {
                self.tracks[j].input = TrackInput::None;
            }
            for (t, _) in self.tracks.iter_mut().zip(&done).filter(|(_, d)| !**d) {
                if t.output.dest() == Some(me) {
                    t.output = TrackOutput::Master;
                }
            }
        }
    }

    /// End of the last arrangement clip, in beats.
    pub fn arrangement_end(&self) -> f64 {
        self.tracks.iter().flat_map(|t| t.arrangement.iter().map(|a| a.start + a.clip.length)).fold(0.0, f64::max)
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
