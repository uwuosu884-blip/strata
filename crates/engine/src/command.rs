/// Messages from the control thread to the audio thread. `Copy` + no heap
/// data, so the audio thread never allocates or frees while handling them.
/// `track` fields are engine track slots (`SongTrack::rt`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    Play,
    /// Stops the transport and all session clips.
    Stop,
    /// Jump the playhead back to bar 1.
    Rewind,
    /// Move the playhead (beats).
    SetPosition(f64),
    /// BPM, clamped to 20–999.
    SetTempo(f64),
    SetBeatsPerBar(u32),
    SetMetronome(bool),
    /// Linear gain, clamped to 0–4.
    SetMasterGain(f32),
    /// Master pan / balance (-1..1, equal power, unity at centre).
    SetMasterPan(f32),
    /// Clip launch quantization in beats (0 = immediate).
    SetLaunchQuantize(f64),
    /// Arrangement loop region (beats).
    SetLoop {
        enabled: bool,
        start: f64,
        end: f64,
    },

    SetTrackGain {
        track: u16,
        gain: f32,
    },
    SetTrackPan {
        track: u16,
        pan: f32,
    },
    SetTrackMute {
        track: u16,
        mute: bool,
    },
    SetTrackSolo {
        track: u16,
        solo: bool,
    },

    /// Launch the clip in `scene` (an empty slot stops the track).
    LaunchClip {
        track: u16,
        scene: u32,
    },
    StopTrack {
        track: u16,
    },
    LaunchScene {
        scene: u32,
    },
    StopAllClips,
    /// Return every track to arrangement playback.
    BackToArrangement,

    NoteOn {
        track: u16,
        pitch: u8,
        velocity: u8,
    },
    NoteOff {
        track: u16,
        pitch: u8,
    },
    AllNotesOff {
        track: u16,
    },
    /// Live's auto-arm: this runtime track monitors MIDI as if armed
    /// (`u16::MAX` = none). Not recorded into unless really armed.
    SetAutoArm(u16),
    /// Computer MIDI keyboard note (velocity 0 = off), routed like a port.
    KeyboardNote {
        pitch: u8,
        velocity: u8,
    },

    /// Start/stop recording at the next launch-quantize boundary.
    SetRecording(bool),
}

/// Events from hardware MIDI input (parsed on the MIDI thread).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiEvent {
    NoteOn { pitch: u8, velocity: u8 },
    NoteOff { pitch: u8 },
    AllNotesOff,
}

/// A hardware MIDI event with its source port index and channel (0..=15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiInput {
    pub port: u8,
    pub channel: u8,
    pub event: MidiEvent,
}

/// Raw MIDI bytes for a hardware output port (engine → MIDI output thread).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiOutput {
    pub port: u8,
    pub bytes: [u8; 3],
}

/// Most armed audio tracks recorded at once.
pub const MAX_REC_LANES: usize = 16;

impl MidiEvent {
    /// Channel (0..=15) of a channel message.
    pub fn channel_of(bytes: &[u8]) -> u8 {
        bytes.first().map_or(0, |s| s & 0x0F)
    }

    /// Parse a raw MIDI message (any channel). Returns None for unsupported messages.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let status = *bytes.first()?;
        match (status & 0xF0, bytes.get(1).copied(), bytes.get(2).copied()) {
            (0x90, Some(p), Some(0)) => Some(Self::NoteOff { pitch: p & 0x7F }),
            (0x90, Some(p), Some(v)) => Some(Self::NoteOn { pitch: p & 0x7F, velocity: v & 0x7F }),
            (0x80, Some(p), _) => Some(Self::NoteOff { pitch: p & 0x7F }),
            // CC 123 (all notes off) / 120 (all sound off)
            (0xB0, Some(123 | 120), _) => Some(Self::AllNotesOff),
            _ => None,
        }
    }
}

/// Recording events from the audio thread to the recorder thread.
/// Recorded audio travels separately: per frame, (L, R) of every lane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RecordEvent {
    /// Recording began at this song position. `pos` = sample index (in the
    /// recorded-audio stream) of the take's first sample. Lanes = runtime
    /// tracks whose input is recorded; bit k of `mono` = lane k is mono.
    Started {
        beat: f64,
        pos: u64,
        lanes: [u16; MAX_REC_LANES],
        n_lanes: u8,
        mono: u16,
    },
    /// Recording ended. `pos` = sample index one past the take's last sample.
    Stopped {
        beat: f64,
        pos: u64,
    },
    /// A live note played into `track` while recording. `clip_pos` is the
    /// position inside the session clip playing on that track (for overdub).
    NoteOn {
        track: u16,
        pitch: u8,
        velocity: u8,
        beat: f64,
        clip_pos: Option<f64>,
    },
    NoteOff {
        track: u16,
        pitch: u8,
        beat: f64,
        clip_pos: Option<f64>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midi_parsing() {
        assert_eq!(MidiEvent::parse(&[0x90, 60, 100]), Some(MidiEvent::NoteOn { pitch: 60, velocity: 100 }));
        assert_eq!(MidiEvent::parse(&[0x93, 60, 0]), Some(MidiEvent::NoteOff { pitch: 60 }));
        assert_eq!(MidiEvent::parse(&[0x81, 61, 40]), Some(MidiEvent::NoteOff { pitch: 61 }));
        assert_eq!(MidiEvent::parse(&[0xB0, 123, 0]), Some(MidiEvent::AllNotesOff));
        assert_eq!(MidiEvent::parse(&[0xE0, 0, 64]), None);
        assert_eq!(MidiEvent::parse(&[]), None);
        assert_eq!(MidiEvent::parse(&[0x90, 60]), None);
    }
}
