//! # daw-engine
//!
//! Real-time half of the DAW.
//! * [`Engine`] runs on the audio thread. [`Engine::process`] is RT-safe:
//!   no locks, no allocation/deallocation, no I/O.
//! * [`EngineHandle`] runs on the GUI thread: sends [`Command`]s and new
//!   [`Song`]s through lock-free queues, reads status through atomics.
//!
//! Songs are swapped as whole immutable snapshots; the old snapshot is sent
//! back to the GUI thread to be freed there.

mod buffer;
mod command;
pub mod instruments;
mod metronome;
pub mod pv;
mod sample;
mod smoother;
mod song;
mod status;
mod track;
mod transport;
pub mod warp;

pub use buffer::StereoBuffer;
pub use command::{Command, MAX_REC_LANES, MidiEvent, MidiInput, MidiOutput, RecordEvent};
pub use instruments::Instrument;
pub use instruments::adsr::AdsrParams;
pub use instruments::sampler::{DRUM_BASE_NOTE, DRUM_PADS, DrumPad, SamplerParams};
pub use instruments::synth::{SynthParams, Wave};
pub use sample::{PEAK_BLOCK, SampleBuffer};
pub use song::{
    ArrangementClip, AudioClip, Clip, ClipContent, ClipExtra, Follow, Launch, LaunchMode, MAX_IO_CHANNELS, MAX_TRACKS, MIDI_ALL_INS,
    MIDI_KEYBOARD, MIDI_NO_PORT, Monitor, Note, NoteMpe, Song, SongTrack, Tap, TrackInput, TrackOutput, WarpSetup, fade_gain,
};
pub use status::{PendingLaunch, RecordState, StatusSnapshot, TrackSnapshot};
pub use transport::{MusicalPosition, Transport};
pub use warp::{LoopMode, StretchMode, StretchParams, WarpMap};

use metronome::Metronome;
use rtrb::{Consumer, Producer, RingBuffer};
use smoother::Smoothed;
use status::{NO_PENDING, PENDING_STOP, SharedStatus};
use std::sync::Arc;
use track::{Action, EPS, MAX_OUT_NOTES, NoteEvent, Pending, SegCtx, TrackRt};

const COMMAND_QUEUE: usize = 1024;
const SONG_QUEUE: usize = 16;
const GARBAGE_QUEUE: usize = 64;
const MIDI_QUEUE: usize = 1024;
const RECORD_EVENT_QUEUE: usize = 8192;
const MIDI_OUT_QUEUE: usize = 2048;
const PREVIEW_QUEUE: usize = 8;
const PREVIEW_GC_QUEUE: usize = 32;
/// Depth limit for live MIDI forwarded between tracks.
const MAX_MIDI_HOPS: u8 = 8;
const NONE: u16 = u16::MAX;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EngineConfig {
    pub sample_rate: f64,
    /// Largest internal block; bigger callbacks are split, so any device
    /// buffer size works without allocating.
    pub max_block_frames: usize,
    /// Interleaved channels per frame pushed to [`EngineHandle::take_input`].
    pub input_channels: usize,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self { sample_rate: 48_000.0, max_block_frames: 512, input_channels: 2 }
    }
}

#[derive(Debug, Clone, Copy)]
struct Recorder {
    state: RecordState,
    /// Beat at which a pending start/stop happens.
    beat: f64,
}

/// Create a connected engine (audio thread) and handle (control thread).
pub fn create(config: EngineConfig) -> (Engine, EngineHandle) {
    assert!(config.sample_rate > 0.0 && config.max_block_frames > 0);
    let config = EngineConfig { input_channels: config.input_channels.clamp(1, MAX_IO_CHANNELS), ..config };
    let sr = config.sample_rate;
    let ich = config.input_channels;
    let (cmd_tx, cmd_rx) = RingBuffer::new(COMMAND_QUEUE);
    let (song_tx, song_rx) = RingBuffer::new(SONG_QUEUE);
    let (gc_tx, gc_rx) = RingBuffer::new(GARBAGE_QUEUE);
    let (midi_tx, midi_rx) = RingBuffer::new(MIDI_QUEUE);
    let (mout_tx, mout_rx) = RingBuffer::new(MIDI_OUT_QUEUE);
    let (pv_tx, pv_rx) = RingBuffer::new(PREVIEW_QUEUE);
    let (pvgc_tx, pvgc_rx) = RingBuffer::new(PREVIEW_GC_QUEUE);
    // ~0.7 s of input buffering, ~10 s of (stereo) recording buffering.
    let (in_tx, in_rx) = RingBuffer::new((sr as usize).max(8192) * ich * 7 / 10);
    let (rec_ev_tx, rec_ev_rx) = RingBuffer::new(RECORD_EVENT_QUEUE);
    let (rec_au_tx, rec_au_rx) = RingBuffer::new(sr as usize * 2 * 10);
    let status = Arc::new(SharedStatus::default());
    let transport = Transport::default();
    status.publish_transport(&transport);
    let mb = config.max_block_frames;
    let plans = pv::Plans::new();
    let bufs = |k: usize| (0..k).map(|_| vec![0.0f32; mb]).collect::<Vec<_>>();

    let engine = Engine {
        config,
        sr: sr as f32,
        commands: cmd_rx,
        songs: song_rx,
        garbage: gc_tx,
        midi: midi_rx,
        midi_out: mout_tx,
        input: in_rx,
        rec_events: rec_ev_tx,
        rec_audio: rec_au_tx,
        status: Arc::clone(&status),
        transport,
        metronome: Metronome::new(sr),
        master_gain: Smoothed::new(1.0, sr, 0.02),
        cue_gain: Smoothed::new(1.0, sr, 0.02),
        monitor_gain: Smoothed::new(1.0, sr, 0.02),
        cue: StereoBuffer::new(mb),
        previews: pv_rx,
        preview_gc: pvgc_tx,
        preview: None,
        preview_pos: 0.0,
        master_pan: [Smoothed::new(1.0, sr, 0.02), Smoothed::new(1.0, sr, 0.02)],
        master: StereoBuffer::new(mb),
        in_ch: bufs(MAX_IO_CHANNELS),
        in_peaks: [0.0; MAX_IO_CHANNELS],
        out_ch: bufs(MAX_IO_CHANNELS),
        out_used: 0,
        res_l: vec![0.0; 2 * mb],
        res_r: vec![0.0; 2 * mb],
        res_pos: 0,
        res_x: StereoBuffer::new(mb),
        song: Box::default(),
        tracks: (0..MAX_TRACKS).map(|_| TrackRt::new(mb, sr, &plans)).collect(),
        rt_to_song: [NONE; MAX_TRACKS],
        quantum: 4.0,
        follow_tempo: true,
        rng: 0x2545_F491_4F6C_DD1D,
        auto_arm: NONE,
        rec: Recorder { state: RecordState::Idle, beat: 0.0 },
        rec_lanes: [NONE; MAX_REC_LANES],
        n_lanes: 0,
        dropped_record_samples: 0,
        rec_pos: 0,
    };
    let handle = EngineHandle {
        commands: cmd_tx,
        songs: song_tx,
        garbage: gc_rx,
        status,
        config,
        input: Some(in_tx),
        midi: Some(midi_tx),
        midi_out: Some(mout_rx),
        recorder: Some(RecorderPorts { events: rec_ev_rx, audio: rec_au_rx }),
        previews: pv_tx,
        preview_gc: pvgc_rx,
        preview_keep: None,
    };
    (engine, handle)
}

/// The audio-thread side. Move it into the audio callback.
pub struct Engine {
    config: EngineConfig,
    sr: f32,
    commands: Consumer<Command>,
    songs: Consumer<Box<Song>>,
    garbage: Producer<Box<Song>>,
    midi: Consumer<MidiInput>,
    midi_out: Producer<MidiOutput>,
    input: Consumer<f32>,
    rec_events: Producer<RecordEvent>,
    rec_audio: Producer<f32>,
    status: Arc<SharedStatus>,
    transport: Transport,
    metronome: Metronome,
    master_gain: Smoothed,
    /// Cue (Live's Preview/Cue volume): metronome + browser preview, mixed
    /// after the master gain so it never reaches Resampling.
    cue_gain: Smoothed,
    /// Monitoring level of the device output only (renders ignore it).
    monitor_gain: Smoothed,
    cue: StereoBuffer,
    previews: Consumer<Arc<SampleBuffer>>,
    /// Finished previews go back so the audio thread never frees them.
    preview_gc: Producer<Arc<SampleBuffer>>,
    preview: Option<Arc<SampleBuffer>>,
    preview_pos: f64,
    /// Left/right pan gains of the master bus.
    master_pan: [Smoothed; 2],
    master: StereoBuffer,
    /// Device input channels of the current callback chunk.
    in_ch: Vec<Vec<f32>>,
    in_peaks: [f32; MAX_IO_CHANNELS],
    /// Device output channels fed by tracks routed to "Ext. Out".
    out_ch: Vec<Vec<f32>>,
    out_used: u64,
    /// Main output delay line for Resampling (read one block behind).
    res_l: Vec<f32>,
    res_r: Vec<f32>,
    res_pos: usize,
    /// Resampling tracks' own output to the main bus this segment: heard,
    /// but kept out of what Resampling records (no feedback).
    res_x: StereoBuffer,
    song: Box<Song>,
    tracks: Vec<TrackRt>,
    rt_to_song: [u16; MAX_TRACKS],
    quantum: f64,
    /// Follow the song's tempo map (leader clips); off = "Unfollow Tempo Automation".
    follow_tempo: bool,
    /// Follow-action dice (xorshift).
    rng: u64,
    auto_arm: u16,
    rec: Recorder,
    /// Runtime tracks whose input the current take records.
    rec_lanes: [u16; MAX_REC_LANES],
    n_lanes: usize,
    dropped_record_samples: u64,
    /// Samples pushed to the recorded-audio ring so far.
    rec_pos: u64,
}

/// Mutable references to two different tracks.
fn two_mut(v: &mut [TrackRt], a: usize, b: usize) -> (&mut TrackRt, &mut TrackRt) {
    debug_assert_ne!(a, b);
    if a < b {
        let (x, y) = v.split_at_mut(b);
        (&mut x[a], &mut y[0])
    } else {
        let (x, y) = v.split_at_mut(a);
        (&mut y[0], &mut x[b])
    }
}

impl Engine {
    pub fn config(&self) -> EngineConfig {
        self.config
    }

    pub fn transport(&self) -> &Transport {
        &self.transport
    }

    /// Fill an interleaved output buffer. Real-time safe; any length.
    pub fn process(&mut self, out: &mut [f32], channels: usize) {
        if channels == 0 {
            return;
        }
        self.receive_songs();
        self.receive_previews();
        self.drain_commands();
        self.drain_midi();

        let mut block_peak = [0.0f32; 2];
        for chunk in out.chunks_mut(self.config.max_block_frames * channels) {
            let frames = chunk.len() / channels;
            self.read_input(frames);
            for c in 0..MAX_IO_CHANNELS {
                if self.out_used & (1 << c) != 0 {
                    self.out_ch[c][..frames].fill(0.0);
                }
            }
            self.out_used = 0;
            self.render(frames);
            let p = self.master.peak(frames);
            block_peak = [block_peak[0].max(p[0]), block_peak[1].max(p[1])];
            self.apply_monitor_gain(frames);
            self.write_output(chunk, channels, frames);
        }
        self.publish(block_peak);
    }

    /// Monitoring level on everything that goes to the device (meters and
    /// offline renders stay at the mix level).
    fn apply_monitor_gain(&mut self, frames: usize) {
        if !self.monitor_gain.is_settled_at(1.0) {
            let used = self.out_used;
            let (l, r) = self.master.channels_mut(frames);
            for k in 0..frames {
                let g = self.monitor_gain.next();
                l[k] *= g;
                r[k] *= g;
                let mut u = used;
                while u != 0 {
                    let c = u.trailing_zeros() as usize;
                    u &= u - 1;
                    self.out_ch[c][k] *= g;
                }
            }
        }
    }

    /// Next browser preview (replacing the current one).
    fn receive_previews(&mut self) {
        while let Ok(buf) = self.previews.pop() {
            if let Some(old) = self.preview.replace(buf) {
                self.recycle_preview(old);
            }
            self.preview_pos = 0.0;
        }
    }

    fn recycle_preview(&mut self, old: Arc<SampleBuffer>) {
        if let Err(rtrb::PushError::Full(old)) = self.preview_gc.push(old) {
            std::mem::forget(old); // never free on the audio thread
        }
    }

    /// Main bus into its output channel(s), plus tracks routed to Ext. Out.
    fn write_output(&self, chunk: &mut [f32], channels: usize, frames: usize) {
        let (ch, stereo) = self.song.master_out;
        let ch = ch as usize;
        if channels == 1 || (!stereo && ch >= channels) {
            self.master.write_interleaved(chunk, channels, frames);
        } else if stereo {
            // Fall back to 1/2 if the configured pair isn't there.
            let c0 = if ch + 1 < channels { ch } else { 0 };
            chunk.fill(0.0);
            let (l, r) = (&self.master.left()[..frames], &self.master.right()[..frames]);
            for (f, frame) in chunk.chunks_exact_mut(channels).enumerate() {
                frame[c0] = l[f];
                frame[c0 + 1] = r[f];
            }
        } else {
            chunk.fill(0.0);
            let (l, r) = (&self.master.left()[..frames], &self.master.right()[..frames]);
            for (f, frame) in chunk.chunks_exact_mut(channels).enumerate() {
                frame[ch] = 0.5 * (l[f] + r[f]);
            }
        }
        let mut used = self.out_used;
        while used != 0 {
            let c = used.trailing_zeros() as usize;
            used &= used - 1;
            if c < channels {
                let src = &self.out_ch[c][..frames];
                for (f, frame) in chunk.chunks_exact_mut(channels).enumerate() {
                    frame[c] += src[f];
                }
            }
        }
    }

    pub fn report_cpu_load(&self, load: f32) {
        self.status.publish_cpu_load(load);
    }

    fn publish(&mut self, peak: [f32; 2]) {
        self.status.publish_transport(&self.transport);
        self.status.publish_peaks(peak);
        self.status.publish_input_peaks(&self.in_peaks);
        self.in_peaks = [0.0; MAX_IO_CHANNELS];
        self.status.publish_record_state(self.rec.state);
        let beat = self.transport.beat_position;
        for st in &self.song.tracks {
            let t = &mut self.tracks[st.rt as usize];
            let pending = match t.pending {
                None => NO_PENDING,
                Some(Pending { action: Action::Stop, .. }) => PENDING_STOP,
                Some(Pending { action: Action::Launch(s), .. }) => s as i32,
            };
            let pos = t.clip_pos(st, beat).unwrap_or(0.0);
            self.status.publish_track(
                st.rt as usize,
                t.playing.map(|p| p.0),
                pending,
                pos,
                t.peak,
                t.voices.active_voices(),
                t.session_override,
            );
            t.peak = [0.0; 2];
        }
    }

    fn receive_songs(&mut self) {
        while let Ok(new) = self.songs.pop() {
            // Hanging notes on a MIDI port the track no longer sends to.
            for ot in &self.song.tracks {
                if let TrackOutput::MidiPort { port, channel } = ot.output {
                    let still = new.tracks.iter().any(|t| t.rt == ot.rt && t.output == ot.output);
                    if !still {
                        Self::flush_out_notes(&mut self.tracks[ot.rt as usize], &mut self.midi_out, port, channel);
                    }
                }
            }
            let old = std::mem::replace(&mut self.song, new);
            if let Err(rtrb::PushError::Full(old)) = self.garbage.push(old) {
                // Never free on the audio thread: leak instead (GUI not draining).
                std::mem::forget(old);
            }
            self.rt_to_song = [NONE; MAX_TRACKS];
            for (i, t) in self.song.tracks.iter().enumerate() {
                self.rt_to_song[t.rt as usize] = i as u16;
            }
            for rt in 0..MAX_TRACKS {
                let in_song = self.rt_to_song[rt] != NONE;
                if self.tracks[rt].in_song && !in_song {
                    self.tracks[rt].reset();
                }
                self.tracks[rt].in_song = in_song;
            }
            for t in &self.song.tracks {
                self.tracks[t.rt as usize].apply_mixer(t);
            }
        }
    }

    fn song_track(&self, rt: u16) -> Option<&SongTrack> {
        let i = *self.rt_to_song.get(rt as usize)?;
        self.song.tracks.get(i as usize)
    }

    /// Next launch boundary at or after the current position.
    fn quantized(&self, beat: f64) -> f64 {
        self.quantized_by(beat, self.quantum)
    }

    fn quantized_by(&self, beat: f64, q: f64) -> f64 {
        if !self.transport.playing || q <= 0.0 { beat } else { ((beat / q) - 1.0e-9).ceil() * q }
    }

    fn dice(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        (x >> 40) as f32 / (1u64 << 24) as f32
    }

    fn slot_clip(&self, rt: u16, scene: u32) -> Option<&Clip> {
        self.song_track(rt)?.slots.get(scene as usize)?.as_ref()
    }

    fn launch(&mut self, rt: u16, action: Action) {
        if rt as usize >= MAX_TRACKS || self.rt_to_song[rt as usize] == NONE {
            return;
        }
        if !self.transport.playing {
            self.transport.playing = true;
        }
        // Clip launch quantization (None = global).
        let q = match action {
            Action::Launch(s) => self.slot_clip(rt, s).and_then(|c| c.extra.launch.quantize).unwrap_or(self.quantum),
            Action::Stop => self.quantum,
        };
        let beat = self.quantized_by(self.transport.beat_position, q);
        let t = &mut self.tracks[rt as usize];
        if action == Action::Stop && t.playing.is_none() {
            t.pending = None;
            return;
        }
        t.pending = Some(Pending { beat, action });
    }

    /// Launch button pressed on a slot (launch modes: Toggle stops a playing
    /// clip; Gate / Repeat remember the held button).
    fn press_clip(&mut self, rt: u16, scene: u32) {
        let Some(mode) = self.slot_clip(rt, scene).map(|c| c.extra.launch.mode) else {
            return self.launch(rt, Action::Stop);
        };
        let Some(t) = self.tracks.get_mut(rt as usize) else { return };
        let playing_this = t.playing.is_some_and(|(s, _)| s == scene);
        if mode == LaunchMode::Toggle && playing_this {
            return self.launch(rt, Action::Stop);
        }
        t.held = matches!(mode, LaunchMode::Gate | LaunchMode::Repeat).then_some(scene);
        self.launch(rt, Action::Launch(scene));
    }

    /// Launch button released: Gate stops the clip; Repeat stops retriggering.
    fn release_clip(&mut self, rt: u16, scene: u32) {
        let Some(t) = self.tracks.get_mut(rt as usize) else { return };
        if t.held != Some(scene) {
            return;
        }
        t.held = None;
        let gate = self.slot_clip(rt, scene).is_some_and(|c| c.extra.launch.mode == LaunchMode::Gate);
        let t = &mut self.tracks[rt as usize];
        if gate {
            if t.pending.is_some_and(|p| p.action == Action::Launch(scene)) {
                t.pending = None;
            }
            if t.playing.is_some_and(|(s, _)| s == scene) {
                t.voices.release_clip_notes();
                t.playing = None;
                t.follow_at = None;
            }
        } else if t.pending.is_some_and(|p| p.action == Action::Launch(scene)) && t.playing.is_some_and(|(s, _)| s == scene) {
            t.pending = None; // queued repeat
        }
    }

    /// Scene picked by a follow action for track `rt` (None = stop / nothing).
    fn follow_target(&mut self, rt: u16, cur: u32, f: Follow) -> Option<Option<u32>> {
        let r = self.dice();
        let st = self.song_track(rt)?;
        let n = st.slots.len() as u32;
        let has = |s: u32| st.slots.get(s as usize).is_some_and(Option::is_some);
        let count = (0..n).filter(|&s| has(s)).count() as u32;
        let nth = |k: u32| (0..n).filter(|&s| has(s)).nth(k as usize);
        let next = |from: u32, dir: i64| -> Option<u32> {
            (1..=n as i64).map(|d| (from as i64 + d * dir).rem_euclid(n as i64) as u32).find(|&s| has(s))
        };
        Some(match f {
            Follow::None => return None,
            Follow::Stop => None,
            Follow::Again => Some(cur),
            Follow::Prev => next(cur, -1),
            Follow::Next => next(cur, 1),
            Follow::First => nth(0),
            Follow::Last => count.checked_sub(1).and_then(nth),
            Follow::Any => nth(((r * count as f32) as u32).min(count.saturating_sub(1))),
            Follow::Other => {
                if count <= 1 {
                    Some(cur)
                } else {
                    let k = ((r * (count - 1) as f32) as u32).min(count - 2);
                    (0..n).filter(|&s| has(s) && s != cur).nth(k as usize)
                }
            }
            Follow::Jump(s) => has(s as u32).then_some(s as u32),
        })
    }

    fn drain_commands(&mut self) {
        while let Ok(cmd) = self.commands.pop() {
            match cmd {
                Command::Play => self.transport.playing = true,
                Command::Stop => self.stop(),
                Command::Rewind => self.set_position(0.0),
                Command::SetPosition(b) => {
                    if b.is_finite() {
                        self.set_position(b.max(0.0))
                    }
                }
                Command::SetTempo(bpm) => self.transport.set_tempo(bpm),
                Command::SetBeatsPerBar(n) => self.transport.beats_per_bar = n.clamp(1, 32),
                Command::SetMetronome(on) => self.metronome.enabled = on,
                Command::SetMasterGain(g) => self.master_gain.set_target(clamp_gain(g)),
                Command::SetCueGain(g) => self.cue_gain.set_target(clamp_gain(g)),
                Command::SetMonitorGain(g) => self.monitor_gain.set_target(clamp_gain(g)),
                Command::StopPreview => {
                    if let Some(old) = self.preview.take() {
                        self.recycle_preview(old);
                    }
                }
                Command::SetMasterPan(pan) => {
                    let a = (if pan.is_finite() { pan.clamp(-1.0, 1.0) } else { 0.0 } + 1.0) * std::f32::consts::FRAC_PI_4;
                    self.master_pan[0].set_target(a.cos() * std::f32::consts::SQRT_2);
                    self.master_pan[1].set_target(a.sin() * std::f32::consts::SQRT_2);
                }
                Command::SetLaunchQuantize(q) => self.quantum = if q.is_finite() { q.clamp(0.0, 64.0) } else { 0.0 },
                Command::SetLoop { enabled, start, end } => {
                    if start.is_finite() && end.is_finite() {
                        self.transport.loop_enabled = enabled;
                        self.transport.loop_start = start.max(0.0);
                        self.transport.loop_end = end.max(start + 1.0 / 16.0);
                    }
                }
                Command::SetTrackGain { track, gain } => {
                    if let Some(t) = self.tracks.get_mut(track as usize) {
                        t.gain = clamp_gain(gain);
                    }
                }
                Command::SetTrackPan { track, pan } => {
                    if let Some(t) = self.tracks.get_mut(track as usize) {
                        t.pan = if pan.is_finite() { pan.clamp(-1.0, 1.0) } else { 0.0 };
                    }
                }
                Command::SetTrackMute { track, mute } => {
                    if let Some(t) = self.tracks.get_mut(track as usize) {
                        t.mute = mute;
                    }
                }
                Command::SetTrackSolo { track, solo } => {
                    if let Some(t) = self.tracks.get_mut(track as usize) {
                        t.solo = solo;
                    }
                }
                Command::LaunchClip { track, scene } => self.press_clip(track, scene),
                Command::ReleaseClip { track, scene } => self.release_clip(track, scene),
                Command::SetTempoFollow(on) => self.follow_tempo = on,
                Command::Nudge(k) => self.transport.nudge = if k.is_finite() { k.clamp(0.5, 2.0) } else { 1.0 },
                Command::StopTrack { track } => self.launch(track, Action::Stop),
                Command::LaunchScene { scene } => {
                    for i in 0..self.song.tracks.len() {
                        let st = &self.song.tracks[i];
                        let has_clip = st.slots.get(scene as usize).is_some_and(Option::is_some);
                        let rt = st.rt;
                        self.launch(rt, if has_clip { Action::Launch(scene) } else { Action::Stop });
                    }
                }
                Command::StopAllClips => {
                    for i in 0..self.song.tracks.len() {
                        let rt = self.song.tracks[i].rt;
                        self.launch(rt, Action::Stop);
                    }
                }
                Command::BackToArrangement => {
                    for t in &mut self.tracks {
                        if t.playing.is_some() || t.session_override {
                            t.voices.release_clip_notes();
                        }
                        t.playing = None;
                        t.pending = None;
                        t.session_override = false;
                    }
                }
                Command::NoteOn { track, pitch, velocity } => self.direct_note(track, pitch, velocity),
                Command::NoteOff { track, pitch } => self.direct_note(track, pitch, 0),
                Command::AllNotesOff { track } => {
                    if let Some(t) = self.tracks.get_mut(track as usize) {
                        for p in 0..128 {
                            t.voices.note_off(p);
                        }
                        t.voices.release_clip_notes();
                    }
                }
                Command::SetAutoArm(rt) => self.auto_arm = rt,
                Command::KeyboardNote { pitch, velocity } => {
                    let ev = if velocity > 0 { MidiEvent::NoteOn { pitch, velocity } } else { MidiEvent::NoteOff { pitch } };
                    self.dispatch_midi(MidiInput { port: MIDI_KEYBOARD, channel: 0, event: ev });
                }
                Command::SetRecording(on) => self.set_recording(on),
            }
        }
    }

    fn drain_midi(&mut self) {
        while let Ok(ev) = self.midi.pop() {
            self.dispatch_midi(ev);
        }
    }

    /// Hardware / keyboard MIDI to every track whose input matches port + channel.
    fn dispatch_midi(&mut self, m: MidiInput) {
        for i in 0..self.song.tracks.len() {
            let st = &self.song.tracks[i];
            let TrackInput::Midi { port, channel } = st.input else { continue };
            let port_ok = port == MIDI_ALL_INS || port == m.port;
            let chan_ok = channel == 0 || channel == m.channel + 1;
            if !(port_ok && chan_ok) {
                continue;
            }
            let rt = st.rt;
            match m.event {
                MidiEvent::NoteOn { pitch, velocity } => self.live_input(rt, pitch, velocity, 0),
                MidiEvent::NoteOff { pitch } => self.live_input(rt, pitch, 0, 0),
                MidiEvent::AllNotesOff => {
                    for p in 0..128 {
                        self.tracks[rt as usize].voices.note_off(p);
                    }
                }
            }
        }
    }

    fn song_index(&self, rt: u16) -> Option<usize> {
        self.rt_to_song.get(rt as usize).copied().filter(|&i| i != NONE).map(|i| i as usize)
    }

    /// Straight to the track's instrument (auditioning): no routing, not recorded.
    fn direct_note(&mut self, rt: u16, pitch: u8, velocity: u8) {
        let Some(i) = self.song_index(rt) else { return };
        let t = &mut self.tracks[rt as usize];
        if velocity > 0 {
            t.voices.note_on(&self.song.tracks[i].instrument, pitch.min(127), velocity.min(127), u32::MAX, false, self.sr);
        } else {
            t.voices.note_off(pitch.min(127));
        }
    }

    /// A live note arriving at a track's input (velocity 0 = note off):
    /// recorded when armed, played when monitored.
    fn live_input(&mut self, rt: u16, pitch: u8, velocity: u8, hops: u8) {
        let Some(i) = self.song_index(rt) else { return };
        let st = &self.song.tracks[i];
        let armed = st.arm || self.auto_arm == rt;
        let monitored = match st.monitor {
            Monitor::In => true,
            Monitor::Auto => armed,
            Monitor::Off => false,
        };
        let pitch = pitch.min(127);
        let velocity = velocity.min(127);
        if armed && matches!(self.rec.state, RecordState::Recording | RecordState::StopPending) {
            let beat = self.transport.beat_position;
            let clip_pos = self.tracks[rt as usize].clip_pos(st, beat);
            let ev = if velocity > 0 {
                RecordEvent::NoteOn { track: rt, pitch, velocity, beat, clip_pos }
            } else {
                RecordEvent::NoteOff { track: rt, pitch, beat, clip_pos }
            };
            let _ = self.rec_events.push(ev);
        }
        if monitored {
            self.play_live(rt, pitch, velocity, hops);
        }
    }

    /// A live note played by a track: its instrument, MIDI listeners and MIDI output.
    fn play_live(&mut self, rt: u16, pitch: u8, velocity: u8, hops: u8) {
        let Some(i) = self.song_index(rt) else { return };
        if hops > MAX_MIDI_HOPS {
            return;
        }
        let st = &self.song.tracks[i];
        let t = &mut self.tracks[rt as usize];
        if velocity > 0 {
            t.voices.note_on(&st.instrument, pitch, velocity, u32::MAX, false, self.sr);
        } else {
            t.voices.note_off(pitch);
        }
        let (listeners, output) = (st.midi_listeners, st.output);
        let mut m = listeners;
        while m != 0 {
            let l = m.trailing_zeros() as u16;
            m &= m - 1;
            self.live_input(l, pitch, velocity, hops + 1);
        }
        match output {
            TrackOutput::MidiPort { port, channel } => {
                let s = if velocity > 0 { 0x90 } else { 0x80 } | (channel.saturating_sub(1) & 0x0F);
                let _ = self.midi_out.push(MidiOutput { port, bytes: [s, pitch, velocity] });
            }
            TrackOutput::MidiTrack { rt: d, instrument: true } => self.play_live(d, pitch, velocity, hops + 1),
            TrackOutput::MidiTrack { rt: d, instrument: false } => self.live_input(d, pitch, velocity, hops + 1),
            _ => {}
        }
    }

    /// Note-offs for every clip note still sounding on a MIDI output.
    fn flush_out_notes(t: &mut TrackRt, out: &mut Producer<MidiOutput>, port: u8, channel: u8) {
        for &(pitch, _) in &t.out_notes[..t.n_out_notes] {
            let _ = out.push(MidiOutput { port, bytes: [0x80 | (channel.saturating_sub(1) & 0x0F), pitch, 0] });
        }
        t.n_out_notes = 0;
    }

    fn set_recording(&mut self, on: bool) {
        let beat = self.transport.beat_position;
        match (on, self.rec.state) {
            (true, RecordState::Idle) => {
                if !self.transport.playing {
                    self.transport.playing = true;
                    self.rec = Recorder { state: RecordState::StartPending, beat };
                } else {
                    self.rec = Recorder { state: RecordState::StartPending, beat: self.quantized(beat) };
                }
            }
            (true, RecordState::StopPending) => self.rec.state = RecordState::Recording, // cancel stop
            (false, RecordState::StartPending) => self.rec.state = RecordState::Idle,
            (false, RecordState::Recording) => {
                self.rec = Recorder { state: RecordState::StopPending, beat: self.quantized(beat) };
            }
            _ => {}
        }
    }

    fn stop(&mut self) {
        self.transport.playing = false;
        if matches!(self.rec.state, RecordState::Recording | RecordState::StopPending) {
            let _ = self.rec_events.push(RecordEvent::Stopped { beat: self.transport.beat_position, pos: self.rec_pos });
        }
        self.rec.state = RecordState::Idle;
        for st in &self.song.tracks {
            if let TrackOutput::MidiPort { port, channel } = st.output {
                Self::flush_out_notes(&mut self.tracks[st.rt as usize], &mut self.midi_out, port, channel);
            }
        }
        for t in &mut self.tracks {
            t.playing = None;
            t.pending = None;
            t.follow_at = None;
            t.held = None;
            t.voices.release_clip_notes();
        }
    }

    fn set_position(&mut self, beat: f64) {
        let delta = beat - self.transport.beat_position;
        self.transport.beat_position = beat;
        for t in &mut self.tracks {
            // Session clips keep their phase; arrangement notes are cut.
            if let Some((_, origin)) = &mut t.playing {
                *origin += delta;
            }
            if let Some(f) = &mut t.follow_at {
                *f += delta;
            }
            if let Some(p) = &mut t.pending {
                p.beat = (p.beat + delta).max(beat);
            }
            if t.playing.is_none() {
                t.voices.release_clip_notes();
            }
        }
    }

    /// Deinterleave the next `frames` input frames into `in_ch` (silence on underrun).
    fn read_input(&mut self, frames: usize) {
        let ch = self.config.input_channels;
        let need = frames * ch;
        let avail = self.input.slots();
        // Keep input latency bounded if the input clock runs ahead.
        let max_backlog = need + self.config.max_block_frames * 8 * ch;
        if avail > max_backlog {
            let drop = (avail - need) / ch * ch;
            if let Ok(c) = self.input.read_chunk(drop) {
                c.commit_all();
            }
        }
        let take = need.min(self.input.slots()) / ch * ch;
        for c in &mut self.in_ch[..ch] {
            c[..frames].fill(0.0);
        }
        if take > 0
            && let Ok(chunk) = self.input.read_chunk(take)
        {
            let (a, b) = chunk.as_slices();
            for (k, s) in a.iter().chain(b.iter()).enumerate() {
                let (f, c) = (k / ch, k % ch);
                self.in_ch[c][f] = *s;
                self.in_peaks[c] = self.in_peaks[c].max(if s.is_finite() { s.abs() } else { 0.0 });
            }
            chunk.commit_all();
        }
    }

    /// Apply launches / record transitions that are due at `beat`.
    fn apply_due(&mut self, beat: f64) {
        let limit = beat + EPS;
        for i in 0..self.song.tracks.len() {
            let rt = self.song.tracks[i].rt;
            // Follow action due: pick A or B and launch right away.
            if let Some(fa) = self.tracks[rt as usize].follow_at.filter(|&f| f <= limit) {
                self.tracks[rt as usize].follow_at = None;
                if let Some((cur, _)) = self.tracks[rt as usize].playing
                    && let Some((a, b, chance, _)) = self.slot_clip(rt, cur).and_then(|c| c.extra.launch.follow)
                {
                    let f = if self.dice() < chance { a } else { b };
                    match self.follow_target(rt, cur, f) {
                        Some(Some(s)) => self.tracks[rt as usize].pending = Some(Pending { beat: fa, action: Action::Launch(s) }),
                        Some(None) => self.tracks[rt as usize].pending = Some(Pending { beat: fa, action: Action::Stop }),
                        None => {}
                    }
                }
            }
            let Some(p) = self.tracks[rt as usize].pending.filter(|p| p.beat <= limit) else { continue };
            let st = &self.song.tracks[i];
            let t = &mut self.tracks[rt as usize];
            t.pending = None;
            match p.action {
                Action::Launch(scene) => {
                    let Some(clip) = st.slots.get(scene as usize).and_then(Option::as_ref) else { continue };
                    let l = clip.extra.launch;
                    // Legato: take over the previous clip's play position.
                    let origin = match t.playing {
                        Some((_, o)) if l.legato => o,
                        _ => p.beat,
                    };
                    let same_pos = l.legato && t.playing.is_some();
                    if !same_pos {
                        t.voices.release_clip_notes();
                    }
                    t.playing = Some((scene, origin));
                    t.session_override = true;
                    t.follow_at = l.follow.and_then(|(_, _, _, time)| (time > 0.0).then_some(p.beat + time));
                    // Repeat: retrigger every quantize step while held.
                    if l.mode == LaunchMode::Repeat && t.held == Some(scene) {
                        let q = l.quantize.unwrap_or(self.quantum);
                        if q > 0.0 {
                            t.pending = Some(Pending { beat: p.beat + q, action: Action::Launch(scene) });
                        }
                    }
                    // Program change on launch (hardware MIDI out).
                    if let TrackOutput::MidiPort { port, channel } = st.output {
                        let c = channel.saturating_sub(1) & 0x0F;
                        let [msb, lsb, pgm] = clip.extra.program;
                        if let Some(v) = msb {
                            let _ = self.midi_out.push(MidiOutput { port, bytes: [0xB0 | c, 0, v & 0x7F] });
                        }
                        if let Some(v) = lsb {
                            let _ = self.midi_out.push(MidiOutput { port, bytes: [0xB0 | c, 32, v & 0x7F] });
                        }
                        if let Some(v) = pgm {
                            let _ = self.midi_out.push(MidiOutput { port, bytes: [0xC0 | c, v & 0x7F, 0] });
                        }
                    }
                }
                Action::Stop => {
                    t.voices.release_clip_notes();
                    t.playing = None;
                    t.follow_at = None;
                }
            }
        }
        match self.rec.state {
            RecordState::StartPending if self.rec.beat <= limit => {
                self.rec.state = RecordState::Recording;
                // Every armed track with an audio input records its own lane.
                let (mut lanes, mut n, mut mono) = ([NONE; MAX_REC_LANES], 0usize, 0u16);
                for st in self.song.tracks.iter().filter(|t| t.arm && t.input.is_audio()).take(MAX_REC_LANES) {
                    lanes[n] = st.rt;
                    if matches!(st.input, TrackInput::Ext { stereo: false, .. }) {
                        mono |= 1 << n;
                    }
                    n += 1;
                }
                self.rec_lanes = lanes;
                self.n_lanes = n;
                let ev = RecordEvent::Started { beat: self.rec.beat, pos: self.rec_pos, lanes, n_lanes: n as u8, mono };
                let _ = self.rec_events.push(ev);
            }
            RecordState::StopPending if self.rec.beat <= limit => {
                self.rec.state = RecordState::Idle;
                let _ = self.rec_events.push(RecordEvent::Stopped { beat: self.rec.beat, pos: self.rec_pos });
            }
            _ => {}
        }
    }

    /// Earliest scheduled event strictly after `beat`.
    fn next_event(&self, beat: f64) -> Option<f64> {
        let mut next = f64::INFINITY;
        for st in &self.song.tracks {
            let t = &self.tracks[st.rt as usize];
            if let Some(p) = t.pending {
                next = next.min(p.beat);
            }
            if let Some(f) = t.follow_at {
                next = next.min(f);
            }
        }
        if matches!(self.rec.state, RecordState::StartPending | RecordState::StopPending) {
            next = next.min(self.rec.beat);
        }
        if let Some((_, end)) = self.transport.active_loop()
            && beat < end - EPS
        {
            next = next.min(end);
        }
        // Tempo map steps.
        if self.follow_tempo {
            let m = &self.song.tempo_map;
            let i = m.partition_point(|p| p.0 <= beat + EPS);
            if let Some(p) = m.get(i) {
                next = next.min(p.0);
            }
        }
        (next.is_finite() && next > beat + EPS).then_some(next)
    }

    fn render(&mut self, frames: usize) {
        self.master.clear(frames);
        self.cue.clear(frames);
        let mut done = 0;
        while done < frames {
            let mut n = frames - done;
            let playing = self.transport.playing;
            let beat0 = self.transport.beat_position;
            // Leader clips: tempo automation (step map).
            if self.follow_tempo && !self.song.tempo_map.is_empty() {
                let m = &self.song.tempo_map;
                let i = m.partition_point(|p| p.0 <= beat0 + EPS);
                let bpm = if i == 0 { self.song.base_tempo } else { m[i - 1].1 };
                if bpm > 0.0 {
                    self.transport.set_tempo(bpm);
                }
            }
            let bps = self.transport.beats_per_sample(self.config.sample_rate);
            if playing {
                self.apply_due(beat0);
                if let Some(next) = self.next_event(beat0) {
                    n = n.min((((next - beat0 - EPS) / bps).ceil() as usize).max(1));
                }
            }
            self.render_segment(done, n, bps);
            if playing {
                let mut beat = beat0 + n as f64 * bps;
                if let Some((start, end)) = self.transport.active_loop()
                    && beat0 < end - EPS
                    && beat >= end - EPS
                {
                    let jump = end - start;
                    beat = start + (beat - end).max(0.0);
                    for t in &mut self.tracks {
                        if let Some((_, origin)) = &mut t.playing {
                            *origin -= jump;
                        }
                        if let Some(f) = &mut t.follow_at {
                            *f -= jump;
                        }
                        if let Some(p) = &mut t.pending {
                            p.beat -= jump;
                        }
                    }
                    if matches!(self.rec.state, RecordState::StartPending | RecordState::StopPending) {
                        self.rec.beat -= jump;
                    }
                }
                self.transport.beat_position = beat;
            }
            done += n;
        }
    }

    fn render_segment(&mut self, offset: usize, n: usize, bps: f64) {
        let ctx = SegCtx {
            beat0: self.transport.beat_position,
            bps,
            n,
            playing: self.transport.playing,
            sr: self.sr,
            tempo: self.transport.tempo(),
        };
        let Self {
            master,
            cue,
            preview,
            preview_pos,
            metronome,
            tracks,
            song,
            transport,
            in_ch,
            out_ch,
            out_used,
            res_l,
            res_r,
            res_pos,
            res_x,
            midi_out,
            auto_arm,
            ..
        } = self;
        let (ml, mr) = master.range_mut(offset, n);
        let (xl, xr) = res_x.range_mut(0, n);
        xl.fill(0.0);
        xr.fill(0.0);
        let (cl, cr) = cue.range_mut(offset, n);

        if ctx.playing {
            for i in 0..n {
                let s = metronome.next_sample(ctx.beat0 + i as f64 * bps, bps, transport.beats_per_bar);
                cl[i] += s;
                cr[i] += s;
            }
        } else {
            for i in 0..n {
                let s = metronome.tail_sample();
                cl[i] += s;
                cr[i] += s;
            }
        }
        // Browser preview (linear interpolation to the engine rate).
        let mut preview_done = false;
        if let Some(buf) = preview.as_deref() {
            let step = buf.sample_rate as f64 / ctx.sr as f64;
            let len = buf.len();
            for i in 0..n {
                let k = *preview_pos as usize;
                if k + 1 >= len {
                    preview_done = true;
                    break;
                }
                let f = (*preview_pos - k as f64) as f32;
                cl[i] += buf.left[k] + (buf.left[k + 1] - buf.left[k]) * f;
                cr[i] += buf.right[k] + (buf.right[k + 1] - buf.right[k]) * f;
                *preview_pos += step;
            }
        }

        // Solo through the routing graph: a track is audible if nothing is
        // soloed, it is soloed, it feeds a soloed track (upstream) or a
        // soloed track feeds it (downstream). `order` is topological.
        let mut up = [false; MAX_TRACKS];
        let mut down = [false; MAX_TRACKS];
        let mut any_solo = false;
        for &i in &song.order {
            let st = &song.tracks[i as usize];
            let rt = st.rt as usize;
            any_solo |= tracks[rt].solo;
            down[rt] |= tracks[rt].solo;
            if let TrackOutput::Track(d) = st.output {
                down[d as usize] |= down[rt];
            }
        }
        for &i in song.order.iter().rev() {
            let st = &song.tracks[i as usize];
            let rt = st.rt as usize;
            up[rt] = tracks[rt].solo || matches!(st.output, TrackOutput::Track(d) if up[d as usize]);
        }

        // Resampling reads the main output one internal block back.
        let rlen = res_l.len();
        let delay = rlen / 2;

        for &i in &song.order {
            let st = &song.tracks[i as usize];
            let rt = st.rt as usize;
            let armed = st.arm || *auto_arm == st.rt;
            let midi_mon = match st.monitor {
                Monitor::In => true,
                Monitor::Auto => armed,
                Monitor::Off => false,
            };
            let covered = tracks[rt].render(st, &ctx, st.monitor != Monitor::In, midi_mon);

            // Clip notes → MIDI port / other tracks.
            {
                let t = &mut tracks[rt];
                if let TrackOutput::MidiPort { port, channel } = st.output {
                    let c = channel.saturating_sub(1) & 0x0F;
                    // Timed note-offs of notes started earlier.
                    let mut k = 0;
                    while k < t.n_out_notes {
                        let (pitch, left) = t.out_notes[k];
                        if left as usize <= n {
                            let _ = midi_out.push(MidiOutput { port, bytes: [0x80 | c, pitch, 0] });
                            t.n_out_notes -= 1;
                            t.out_notes[k] = t.out_notes[t.n_out_notes];
                        } else {
                            t.out_notes[k].1 = left - n as u32;
                            k += 1;
                        }
                    }
                    for e in &t.events[..t.n_events] {
                        let _ = midi_out.push(MidiOutput { port, bytes: [0x90 | c, e.pitch, e.vel] });
                        if t.n_out_notes < MAX_OUT_NOTES {
                            t.out_notes[t.n_out_notes] = (e.pitch, e.offset.saturating_add(e.len));
                            t.n_out_notes += 1;
                        }
                    }
                }
            }
            let mut fwd_mask = st.midi_listeners;
            let direct = match st.output {
                TrackOutput::MidiTrack { rt: d, instrument } => {
                    fwd_mask |= 1 << d;
                    instrument.then_some(d)
                }
                _ => None,
            };
            while fwd_mask != 0 {
                let d = fwd_mask.trailing_zeros() as usize;
                fwd_mask &= fwd_mask - 1;
                if d == rt {
                    continue;
                }
                let (src, dst) = two_mut(tracks, rt, d);
                let is_direct = direct == Some(d as u16);
                for e in &src.events[..src.n_events] {
                    let e: NoteEvent = *e;
                    dst.push_fwd(e, is_direct);
                }
            }

            // Audio input stage.
            if st.input.is_audio() {
                match st.input {
                    TrackInput::Ext { ch, stereo } => {
                        let t = &mut tracks[rt];
                        let c = ch as usize;
                        let c2 = if stereo { (c + 1).min(MAX_IO_CHANNELS - 1) } else { c };
                        t.in_l[..n].copy_from_slice(&in_ch[c][offset..offset + n]);
                        t.in_r[..n].copy_from_slice(&in_ch[c2][offset..offset + n]);
                    }
                    TrackInput::Resampling => {
                        let t = &mut tracks[rt];
                        for k in 0..n {
                            let p = (*res_pos + rlen - delay + k) % rlen;
                            t.in_l[k] = res_l[p];
                            t.in_r[k] = res_r[p];
                        }
                    }
                    TrackInput::Track { rt: src, tap } => {
                        let (t, s) = two_mut(tracks, rt, src as usize);
                        let (sl, sr) = match tap {
                            Tap::PreFx | Tap::PostFx => (&s.l, &s.r),
                            Tap::PostMixer => (&s.post_l, &s.post_r),
                            Tap::Pad(p) => match s.pads.get(p as usize) {
                                Some(pb) => (&pb.l, &pb.r),
                                None => (&s.l, &s.r),
                            },
                        };
                        t.in_l[..n].copy_from_slice(&sl[..n]);
                        t.in_r[..n].copy_from_slice(&sr[..n]);
                    }
                    _ => {}
                }
                let pass = match st.monitor {
                    Monitor::In => true,
                    Monitor::Auto => armed && !covered,
                    Monitor::Off => false,
                };
                if pass {
                    tracks[rt].add_input(n);
                }
            }
            tracks[rt].add_bus(n);

            let audible = !any_solo || up[rt] || down[rt];
            if !tracks[rt].mix(n, audible) {
                continue;
            }
            match st.output {
                TrackOutput::Master => {
                    let t = &tracks[rt];
                    for k in 0..n {
                        ml[k] += t.post_l[k];
                        mr[k] += t.post_r[k];
                    }
                    if st.input == TrackInput::Resampling {
                        for k in 0..n {
                            xl[k] += t.post_l[k];
                            xr[k] += t.post_r[k];
                        }
                    }
                }
                TrackOutput::Track(d) => {
                    let (t, g) = two_mut(tracks, rt, d as usize);
                    for k in 0..n {
                        g.bus_l[k] += t.post_l[k];
                        g.bus_r[k] += t.post_r[k];
                    }
                    g.bus_active = true;
                }
                TrackOutput::Ext { ch, stereo } => {
                    let t = &tracks[rt];
                    let c = ch as usize;
                    if stereo && c + 1 < MAX_IO_CHANNELS {
                        for k in 0..n {
                            out_ch[c][offset + k] += t.post_l[k];
                            out_ch[c + 1][offset + k] += t.post_r[k];
                        }
                        *out_used |= 3 << c;
                    } else {
                        // Mono out: (L + R) / 2 (−6 dB).
                        for k in 0..n {
                            out_ch[c][offset + k] += 0.5 * (t.post_l[k] + t.post_r[k]);
                        }
                        *out_used |= 1 << c;
                    }
                }
                _ => {}
            }
        }

        let (cl, cr) = self.cue.range_mut(offset, n);
        for i in 0..n {
            let g = self.master_gain.next();
            let (gl, gr) = (g * self.master_pan[0].next(), g * self.master_pan[1].next());
            res_l[*res_pos] = (ml[i] - xl[i]) * gl;
            res_r[*res_pos] = (mr[i] - xr[i]) * gr;
            *res_pos = (*res_pos + 1) % rlen;
            let c = self.cue_gain.next();
            ml[i] = ml[i] * gl + cl[i] * c;
            mr[i] = mr[i] * gr + cr[i] * c;
        }
        if preview_done && let Some(old) = self.preview.take() {
            self.recycle_preview(old);
        }

        if matches!(self.rec.state, RecordState::Recording | RecordState::StopPending) && self.n_lanes > 0 {
            let samples = n * self.n_lanes * 2;
            let lanes = &self.rec_lanes[..self.n_lanes];
            match self.rec_audio.write_chunk_uninit(samples) {
                Ok(chunk) => {
                    let tracks = &self.tracks;
                    chunk.fill_from_iter(
                        (0..n).flat_map(|f| lanes.iter().flat_map(move |&rt| [tracks[rt as usize].in_l[f], tracks[rt as usize].in_r[f]])),
                    );
                    self.rec_pos += samples as u64;
                }
                Err(_) => self.dropped_record_samples += samples as u64,
            }
        }
    }
}

fn clamp_gain(g: f32) -> f32 {
    if g.is_finite() { g.clamp(0.0, 4.0) } else { 0.0 }
}

/// Consumer ends of the recording queues (for the recorder thread).
pub struct RecorderPorts {
    pub events: Consumer<RecordEvent>,
    /// Recorded input: per frame, interleaved (L, R) of each lane of the take.
    pub audio: Consumer<f32>,
}

/// The control-thread side (GUI, renderer, tests).
pub struct EngineHandle {
    commands: Producer<Command>,
    songs: Producer<Box<Song>>,
    garbage: Consumer<Box<Song>>,
    status: Arc<SharedStatus>,
    config: EngineConfig,
    input: Option<Producer<f32>>,
    midi: Option<Producer<MidiInput>>,
    midi_out: Option<Consumer<MidiOutput>>,
    recorder: Option<RecorderPorts>,
    previews: Producer<Arc<SampleBuffer>>,
    preview_gc: Consumer<Arc<SampleBuffer>>,
    /// Keeps the playing preview alive here too (the audio thread never drops the last ref).
    preview_keep: Option<Arc<SampleBuffer>>,
}

impl EngineHandle {
    /// Play a sample on the Cue bus (browser preview), replacing any current one.
    pub fn preview(&mut self, buf: Arc<SampleBuffer>) -> bool {
        self.collect_garbage();
        self.preview_keep = Some(Arc::clone(&buf));
        self.previews.push(buf).is_ok()
    }

    /// Queue a command. Returns it back if the queue is full.
    pub fn send(&mut self, cmd: Command) -> Result<(), Command> {
        self.commands.push(cmd).map_err(|rtrb::PushError::Full(c)| c)
    }

    /// Replace the song. Prepared (sorted/validated) here, off the audio thread.
    pub fn set_song(&mut self, mut song: Song) -> Result<(), Song> {
        self.collect_garbage();
        song.prepare();
        self.songs.push(Box::new(song)).map_err(|rtrb::PushError::Full(s)| *s)
    }

    /// Free songs the engine has finished with. Call regularly (each GUI frame).
    pub fn collect_garbage(&mut self) -> usize {
        let mut n = 0;
        while let Ok(old) = self.garbage.pop() {
            drop(old);
            n += 1;
        }
        while let Ok(old) = self.preview_gc.pop() {
            drop(old);
            n += 1;
        }
        n
    }

    /// Latest engine state. Reading resets the master peak meters.
    pub fn snapshot(&self) -> StatusSnapshot {
        self.status.snapshot()
    }

    /// Status of one track slot. Reading resets its peak meters.
    pub fn track(&self, rt: u16) -> TrackSnapshot {
        self.status.track(rt as usize)
    }

    pub fn config(&self) -> EngineConfig {
        self.config
    }

    /// Peak per device input channel since the last call (input meters).
    pub fn input_peaks(&self) -> [f32; MAX_IO_CHANNELS] {
        self.status.input_peaks()
    }

    /// Producer for interleaved input samples, `config.input_channels` per frame.
    pub fn take_input(&mut self) -> Option<Producer<f32>> {
        self.input.take()
    }

    /// Producer for hardware MIDI events (MIDI input thread).
    pub fn take_midi_input(&mut self) -> Option<Producer<MidiInput>> {
        self.midi.take()
    }

    /// Consumer of messages for hardware MIDI outputs (MIDI output thread).
    pub fn take_midi_output(&mut self) -> Option<Consumer<MidiOutput>> {
        self.midi_out.take()
    }

    pub fn take_recorder(&mut self) -> Option<RecorderPorts> {
        self.recorder.take()
    }
}
