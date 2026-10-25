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
mod sample;
mod smoother;
mod song;
mod status;
mod track;
mod transport;

pub use buffer::StereoBuffer;
pub use command::{Command, MidiEvent, RecordEvent};
pub use instruments::Instrument;
pub use instruments::adsr::AdsrParams;
pub use instruments::sampler::{DRUM_BASE_NOTE, DRUM_PADS, DrumPad, SamplerParams};
pub use instruments::synth::{SynthParams, Wave};
pub use sample::{PEAK_BLOCK, SampleBuffer};
pub use song::{ArrangementClip, AudioClip, Clip, ClipContent, MAX_TRACKS, Note, Song, SongTrack};
pub use status::{PendingLaunch, RecordState, StatusSnapshot, TrackSnapshot};
pub use transport::{MusicalPosition, Transport};

use metronome::Metronome;
use rtrb::{Consumer, Producer, RingBuffer};
use smoother::Smoothed;
use status::{NO_PENDING, PENDING_STOP, SharedStatus};
use std::sync::Arc;
use track::{Action, EPS, Pending, SegCtx, TrackRt};

const COMMAND_QUEUE: usize = 1024;
const SONG_QUEUE: usize = 16;
const GARBAGE_QUEUE: usize = 64;
const MIDI_QUEUE: usize = 1024;
const RECORD_EVENT_QUEUE: usize = 8192;
const NONE: u16 = u16::MAX;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EngineConfig {
    pub sample_rate: f64,
    /// Largest internal block; bigger callbacks are split, so any device
    /// buffer size works without allocating.
    pub max_block_frames: usize,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self { sample_rate: 48_000.0, max_block_frames: 512 }
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
    let sr = config.sample_rate;
    let (cmd_tx, cmd_rx) = RingBuffer::new(COMMAND_QUEUE);
    let (song_tx, song_rx) = RingBuffer::new(SONG_QUEUE);
    let (gc_tx, gc_rx) = RingBuffer::new(GARBAGE_QUEUE);
    let (midi_tx, midi_rx) = RingBuffer::new(MIDI_QUEUE);
    // ~0.7 s of stereo input buffering, ~10 s of recording buffering.
    let (in_tx, in_rx) = RingBuffer::new((sr as usize).max(8192) * 2 * 7 / 10);
    let (rec_ev_tx, rec_ev_rx) = RingBuffer::new(RECORD_EVENT_QUEUE);
    let (rec_au_tx, rec_au_rx) = RingBuffer::new(sr as usize * 2 * 10);
    let status = Arc::new(SharedStatus::default());
    let transport = Transport::default();
    status.publish_transport(&transport);
    let mb = config.max_block_frames;

    let engine = Engine {
        config,
        sr: sr as f32,
        commands: cmd_rx,
        songs: song_rx,
        garbage: gc_tx,
        midi: midi_rx,
        input: in_rx,
        rec_events: rec_ev_tx,
        rec_audio: rec_au_tx,
        status: Arc::clone(&status),
        transport,
        metronome: Metronome::new(sr),
        master_gain: Smoothed::new(1.0, sr, 0.02),
        master: StereoBuffer::new(mb),
        input_l: vec![0.0; mb],
        input_r: vec![0.0; mb],
        song: Box::default(),
        tracks: (0..MAX_TRACKS).map(|_| TrackRt::new(mb, sr)).collect(),
        rt_to_song: [NONE; MAX_TRACKS],
        quantum: 4.0,
        midi_target: None,
        rec: Recorder { state: RecordState::Idle, beat: 0.0 },
        dropped_record_samples: 0,
        rec_frames: 0,
    };
    let handle = EngineHandle {
        commands: cmd_tx,
        songs: song_tx,
        garbage: gc_rx,
        status,
        config,
        input: Some(in_tx),
        midi: Some(midi_tx),
        recorder: Some(RecorderPorts { events: rec_ev_rx, audio: rec_au_rx }),
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
    midi: Consumer<MidiEvent>,
    input: Consumer<f32>,
    rec_events: Producer<RecordEvent>,
    rec_audio: Producer<f32>,
    status: Arc<SharedStatus>,
    transport: Transport,
    metronome: Metronome,
    master_gain: Smoothed,
    master: StereoBuffer,
    input_l: Vec<f32>,
    input_r: Vec<f32>,
    song: Box<Song>,
    tracks: Vec<TrackRt>,
    rt_to_song: [u16; MAX_TRACKS],
    quantum: f64,
    midi_target: Option<u16>,
    rec: Recorder,
    dropped_record_samples: u64,
    /// Frames pushed to the recorded-audio ring so far.
    rec_frames: u64,
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
        self.drain_commands();
        self.drain_midi();

        let mut block_peak = [0.0f32; 2];
        for chunk in out.chunks_mut(self.config.max_block_frames * channels) {
            let frames = chunk.len() / channels;
            self.read_input(frames);
            self.render(frames);
            let p = self.master.peak(frames);
            block_peak = [block_peak[0].max(p[0]), block_peak[1].max(p[1])];
            self.master.write_interleaved(chunk, channels, frames);
        }
        self.publish(block_peak);
    }

    pub fn report_cpu_load(&self, load: f32) {
        self.status.publish_cpu_load(load);
    }

    fn publish(&mut self, peak: [f32; 2]) {
        self.status.publish_transport(&self.transport);
        self.status.publish_peaks(peak);
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
        if !self.transport.playing || self.quantum <= 0.0 {
            beat
        } else {
            ((beat / self.quantum) - 1.0e-9).ceil() * self.quantum
        }
    }

    fn launch(&mut self, rt: u16, action: Action) {
        if rt as usize >= MAX_TRACKS || self.rt_to_song[rt as usize] == NONE {
            return;
        }
        if !self.transport.playing {
            self.transport.playing = true;
        }
        let beat = self.quantized(self.transport.beat_position);
        let t = &mut self.tracks[rt as usize];
        if action == Action::Stop && t.playing.is_none() {
            t.pending = None;
            return;
        }
        t.pending = Some(Pending { beat, action });
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
                Command::SetTrackMonitor { track, monitor } => {
                    if let Some(t) = self.tracks.get_mut(track as usize) {
                        t.monitor = monitor;
                    }
                }
                Command::LaunchClip { track, scene } => {
                    let has_clip = self
                        .song_track(track)
                        .is_some_and(|t| t.slots.get(scene as usize).is_some_and(Option::is_some));
                    self.launch(track, if has_clip { Action::Launch(scene) } else { Action::Stop });
                }
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
                Command::NoteOn { track, pitch, velocity } => self.live_note(track, pitch, velocity),
                Command::NoteOff { track, pitch } => self.live_note(track, pitch, 0),
                Command::AllNotesOff { track } => {
                    if let Some(t) = self.tracks.get_mut(track as usize) {
                        for p in 0..128 {
                            t.voices.note_off(p);
                        }
                        t.voices.release_clip_notes();
                    }
                }
                Command::SetMidiTarget(t) => self.midi_target = t.filter(|&t| (t as usize) < MAX_TRACKS),
                Command::SetRecording(on) => self.set_recording(on),
            }
        }
    }

    fn drain_midi(&mut self) {
        while let Ok(ev) = self.midi.pop() {
            let Some(track) = self.midi_target else { continue };
            match ev {
                MidiEvent::NoteOn { pitch, velocity } => self.live_note(track, pitch, velocity),
                MidiEvent::NoteOff { pitch } => self.live_note(track, pitch, 0),
                MidiEvent::AllNotesOff => {
                    for p in 0..128 {
                        self.tracks[track as usize].voices.note_off(p);
                    }
                }
            }
        }
    }

    /// Live note (velocity 0 = note off). Recorded while recording.
    fn live_note(&mut self, rt: u16, pitch: u8, velocity: u8) {
        let Some(i) = self.rt_to_song.get(rt as usize).copied().filter(|&i| i != NONE) else { return };
        let st = &self.song.tracks[i as usize];
        let t = &mut self.tracks[rt as usize];
        let pitch = pitch.min(127);
        if velocity > 0 {
            t.voices.note_on(&st.instrument, pitch, velocity.min(127), u32::MAX, false, self.sr);
        } else {
            t.voices.note_off(pitch);
        }
        if matches!(self.rec.state, RecordState::Recording | RecordState::StopPending) {
            let beat = self.transport.beat_position;
            let clip_pos = t.clip_pos(st, beat);
            let ev = if velocity > 0 {
                RecordEvent::NoteOn { track: rt, pitch, velocity, beat, clip_pos }
            } else {
                RecordEvent::NoteOff { track: rt, pitch, beat, clip_pos }
            };
            let _ = self.rec_events.push(ev);
        }
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
            let _ = self.rec_events.push(RecordEvent::Stopped { beat: self.transport.beat_position, frame: self.rec_frames });
        }
        self.rec.state = RecordState::Idle;
        for t in &mut self.tracks {
            t.playing = None;
            t.pending = None;
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
            if let Some(p) = &mut t.pending {
                p.beat = (p.beat + delta).max(beat);
            }
            if t.playing.is_none() {
                t.voices.release_clip_notes();
            }
        }
    }

    fn read_input(&mut self, frames: usize) {
        let need = frames * 2;
        let avail = self.input.slots();
        // Keep input latency bounded if the input clock runs ahead.
        let max_backlog = need + self.config.max_block_frames * 8 * 2; // samples (stereo)
        if avail > max_backlog
            && let Ok(c) = self.input.read_chunk((avail - need) & !1)
        {
            c.commit_all();
        }
        let take = need.min(self.input.slots()) & !1;
        let (il, ir) = (&mut self.input_l[..frames], &mut self.input_r[..frames]);
        il.fill(0.0);
        ir.fill(0.0);
        if take > 0
            && let Ok(chunk) = self.input.read_chunk(take)
        {
            let (a, b) = chunk.as_slices();
            for (k, s) in a.iter().chain(b.iter()).enumerate() {
                if k % 2 == 0 {
                    il[k / 2] = *s;
                } else {
                    ir[k / 2] = *s;
                }
            }
            chunk.commit_all();
        }
    }

    /// Apply launches / record transitions that are due at `beat`.
    fn apply_due(&mut self, beat: f64) {
        let limit = beat + EPS;
        for st in &self.song.tracks {
            let t = &mut self.tracks[st.rt as usize];
            if let Some(p) = t.pending.filter(|p| p.beat <= limit) {
                t.pending = None;
                t.voices.release_clip_notes();
                match p.action {
                    Action::Launch(scene) => {
                        t.playing = Some((scene, p.beat));
                        t.session_override = true;
                    }
                    Action::Stop => t.playing = None,
                }
            }
        }
        match self.rec.state {
            RecordState::StartPending if self.rec.beat <= limit => {
                self.rec.state = RecordState::Recording;
                let _ = self.rec_events.push(RecordEvent::Started { beat: self.rec.beat, frame: self.rec_frames });
            }
            RecordState::StopPending if self.rec.beat <= limit => {
                self.rec.state = RecordState::Idle;
                let _ = self.rec_events.push(RecordEvent::Stopped { beat: self.rec.beat, frame: self.rec_frames });
            }
            _ => {}
        }
    }

    /// Earliest scheduled event strictly after `beat`.
    fn next_event(&self, beat: f64) -> Option<f64> {
        let mut next = f64::INFINITY;
        for st in &self.song.tracks {
            if let Some(p) = self.tracks[st.rt as usize].pending {
                next = next.min(p.beat);
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
        (next.is_finite() && next > beat + EPS).then_some(next)
    }

    fn render(&mut self, frames: usize) {
        self.master.clear(frames);
        let mut done = 0;
        while done < frames {
            let mut n = frames - done;
            let playing = self.transport.playing;
            let bps = self.transport.beats_per_sample(self.config.sample_rate);
            let beat0 = self.transport.beat_position;
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
        let Self { master, metronome, tracks, song, transport, input_l, input_r, .. } = self;
        let (ml, mr) = master.range_mut(offset, n);

        if ctx.playing {
            for i in 0..n {
                let s = metronome.next_sample(ctx.beat0 + i as f64 * bps, bps, transport.beats_per_bar);
                ml[i] += s;
                mr[i] += s;
            }
        } else {
            for i in 0..n {
                let s = metronome.tail_sample();
                ml[i] += s;
                mr[i] += s;
            }
        }

        let input = (&input_l[offset..offset + n], &input_r[offset..offset + n]);
        let any_solo = song.tracks.iter().any(|t| tracks[t.rt as usize].solo);
        for st in &song.tracks {
            let t = &mut tracks[st.rt as usize];
            let monitor = t.monitor.then_some(input);
            t.render(st, &ctx, monitor);
            t.mix_into(ml, mr, any_solo);
        }

        for i in 0..n {
            let g = self.master_gain.next();
            ml[i] *= g;
            mr[i] *= g;
        }

        if matches!(self.rec.state, RecordState::Recording | RecordState::StopPending) {
            let samples = n * 2;
            match self.rec_audio.write_chunk_uninit(samples) {
                Ok(chunk) => {
                    let (il, ir) = input;
                    chunk.fill_from_iter(il.iter().zip(ir).flat_map(|(a, b)| [*a, *b]));
                    self.rec_frames += n as u64;
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
    /// Interleaved stereo input frames, captured while recording.
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
    midi: Option<Producer<MidiEvent>>,
    recorder: Option<RecorderPorts>,
}

impl EngineHandle {
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

    /// Producer for interleaved stereo input samples (audio input callback).
    pub fn take_input(&mut self) -> Option<Producer<f32>> {
        self.input.take()
    }

    /// Producer for hardware MIDI events (MIDI input thread).
    pub fn take_midi_input(&mut self) -> Option<Producer<MidiEvent>> {
        self.midi.take()
    }

    pub fn take_recorder(&mut self) -> Option<RecorderPorts> {
        self.recorder.take()
    }
}
