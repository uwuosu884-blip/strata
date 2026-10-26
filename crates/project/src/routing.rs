//! Per-track routing (Live's In/Out section). Tracks are referenced by id and
//! MIDI ports by name, so files survive reordering and device changes.

pub use daw_engine::{Monitor, Tap};
use serde::{Deserialize, Serialize};

/// Audio track input ("Audio From").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioIn {
    None,
    /// Device input channel `ch` (0-based); stereo = `ch`/`ch + 1`.
    Ext {
        ch: u16,
        stereo: bool,
    },
    /// The main output.
    Resampling,
    Track {
        id: u64,
        tap: Tap,
    },
}

impl Default for AudioIn {
    fn default() -> Self {
        Self::Ext { ch: 0, stereo: true }
    }
}

/// MIDI track input ("MIDI From").
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MidiIn {
    None,
    /// Every enabled port plus the computer keyboard.
    #[default]
    AllIns,
    Keyboard,
    Port(String),
    Track(u64),
}

/// Audio output ("Audio To").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AudioOut {
    /// The parent group, or the main output.
    #[default]
    Default,
    /// The main output even inside a group.
    Master,
    Ext {
        ch: u16,
        stereo: bool,
    },
    Track(u64),
}

/// MIDI output of a MIDI track without an instrument ("MIDI To").
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MidiOut {
    #[default]
    None,
    /// `channel` 1..=16.
    Port { name: String, channel: u8 },
    /// Another MIDI track: its instrument directly, or its input ("Track In").
    Track { id: u64, instrument: bool },
}
