//! # daw-engine
//!
//! The real-time half of the DAW. It is split in two:
//!
//! * [`Engine`] lives on the **audio thread**. [`Engine::process`] is called by the
//!   audio backend for every buffer and must be *real-time safe*: no locks, no
//!   memory allocation, no I/O, no unbounded loops.
//! * [`EngineHandle`] lives on the **GUI / control thread**. It sends
//!   [`Command`]s through a lock-free ring buffer and reads [`StatusSnapshot`]s
//!   that the engine publishes through atomics.
//!
//! ```text
//!   GUI thread                          audio thread
//!  ┌──────────────┐  Command (rtrb)   ┌──────────────┐
//!  │ EngineHandle │ ────────────────▶ │    Engine    │──▶ speakers
//!  │              │ ◀──────────────── │              │
//!  └──────────────┘  atomics (status) └──────────────┘
//! ```

mod buffer;
mod command;
mod metronome;
mod smoother;
mod status;
mod transport;

pub use buffer::StereoBuffer;
pub use command::Command;
pub use status::StatusSnapshot;
pub use transport::{MusicalPosition, Transport};

use metronome::Metronome;
use smoother::Smoothed;
use status::SharedStatus;
use std::sync::Arc;

/// Capacity of the GUI → engine command queue.
const COMMAND_QUEUE_CAPACITY: usize = 1024;

/// Static configuration chosen when the audio device is opened.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EngineConfig {
    pub sample_rate: f64,
    /// Largest number of frames processed in one internal block. Callbacks
    /// larger than this are split into several blocks, so any device buffer
    /// size works without allocating.
    pub max_block_frames: usize,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self { sample_rate: 48_000.0, max_block_frames: 512 }
    }
}

/// Create a connected engine (for the audio thread) and handle (for the GUI).
pub fn create(config: EngineConfig) -> (Engine, EngineHandle) {
    assert!(config.sample_rate > 0.0, "sample rate must be positive");
    assert!(config.max_block_frames > 0, "max_block_frames must be > 0");

    let (producer, consumer) = rtrb::RingBuffer::new(COMMAND_QUEUE_CAPACITY);
    let status = Arc::new(SharedStatus::default());
    let transport = Transport::default();
    status.publish_transport(&transport);

    let engine = Engine {
        config,
        commands: consumer,
        status: Arc::clone(&status),
        transport,
        metronome: Metronome::new(config.sample_rate),
        master_gain: Smoothed::new(1.0, config.sample_rate, 0.02),
        scratch: StereoBuffer::new(config.max_block_frames),
    };
    let handle = EngineHandle { commands: producer, status, config };
    (engine, handle)
}

/// The audio-thread side. Move this into your audio callback.
pub struct Engine {
    config: EngineConfig,
    commands: rtrb::Consumer<Command>,
    status: Arc<SharedStatus>,
    transport: Transport,
    metronome: Metronome,
    master_gain: Smoothed,
    /// Pre-allocated planar work buffer (one Vec per channel).
    scratch: StereoBuffer,
}

impl Engine {
    pub fn config(&self) -> EngineConfig {
        self.config
    }

    pub fn transport(&self) -> &Transport {
        &self.transport
    }

    /// Fill an **interleaved** output buffer with `channels` channels.
    ///
    /// Real-time safe. Any number of frames is accepted: work is done in
    /// blocks of at most `max_block_frames`.
    pub fn process(&mut self, out: &mut [f32], channels: usize) {
        if channels == 0 {
            return;
        }
        debug_assert_eq!(out.len() % channels, 0, "buffer is not a whole number of frames");

        self.drain_commands();

        let mut block_peak = [0.0f32; 2];
        for chunk in out.chunks_mut(self.config.max_block_frames * channels) {
            let frames = chunk.len() / channels;
            self.render_block(frames);
            let peak = self.scratch.peak(frames);
            block_peak[0] = block_peak[0].max(peak[0]);
            block_peak[1] = block_peak[1].max(peak[1]);
            self.scratch.write_interleaved(chunk, channels, frames);
        }

        self.status.publish_transport(&self.transport);
        self.status.publish_peaks(block_peak);
    }

    /// Report how much of the buffer deadline the last callback used (0.0–1.0+).
    /// Called by the audio backend, which owns the timing measurement.
    pub fn report_cpu_load(&self, load: f32) {
        self.status.publish_cpu_load(load);
    }

    /// Apply every pending command. Bounded by the queue capacity.
    fn drain_commands(&mut self) {
        while let Ok(cmd) = self.commands.pop() {
            match cmd {
                Command::Play => {
                    self.transport.playing = true;
                }
                Command::Stop => self.transport.playing = false,
                Command::Rewind => self.transport.beat_position = 0.0,
                Command::SetTempo(bpm) => self.transport.set_tempo(bpm),
                Command::SetBeatsPerBar(n) => self.transport.beats_per_bar = n.clamp(1, 32),
                Command::SetMetronome(on) => self.metronome.enabled = on,
                Command::SetMasterGain(gain) => {
                    self.master_gain.set_target(gain.clamp(0.0, 4.0));
                }
            }
        }
    }

    /// Render one block (≤ max_block_frames) into `self.scratch`.
    fn render_block(&mut self, frames: usize) {
        self.scratch.clear(frames);

        if self.transport.playing {
            let beats_per_sample = self.transport.beats_per_sample(self.config.sample_rate);
            let (left, right) = self.scratch.channels_mut(frames);
            for i in 0..frames {
                let beat = self.transport.beat_position;
                let s = self.metronome.next_sample(beat, beats_per_sample, self.transport.beats_per_bar);
                left[i] += s;
                right[i] += s;
                self.transport.beat_position = beat + beats_per_sample;
            }
        } else {
            // Let a ringing click finish naturally instead of cutting it off.
            let (left, right) = self.scratch.channels_mut(frames);
            for i in 0..frames {
                let s = self.metronome.tail_sample();
                left[i] += s;
                right[i] += s;
            }
        }

        // Master gain, smoothed per sample so fader moves never click.
        let (left, right) = self.scratch.channels_mut(frames);
        for i in 0..frames {
            let g = self.master_gain.next();
            left[i] *= g;
            right[i] *= g;
        }
    }
}

/// The control-thread side (GUI, scripting, tests).
pub struct EngineHandle {
    commands: rtrb::Producer<Command>,
    status: Arc<SharedStatus>,
    config: EngineConfig,
}

impl EngineHandle {
    /// Queue a command. Returns it back if the queue is full (engine stalled).
    pub fn send(&mut self, cmd: Command) -> Result<(), Command> {
        self.commands.push(cmd).map_err(|rtrb::PushError::Full(c)| c)
    }

    /// Latest state published by the engine. Reading the peak meters resets
    /// them, so call this once per GUI frame.
    pub fn snapshot(&self) -> StatusSnapshot {
        self.status.snapshot()
    }

    pub fn config(&self) -> EngineConfig {
        self.config
    }
}
