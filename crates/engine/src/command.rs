/// Messages from the control thread to the audio thread.
///
/// Must stay `Copy` and small: commands are moved through a lock-free ring
/// buffer, and the audio thread must never free heap memory, so commands may
/// not own heap data (no `String`, `Vec`, `Box`…). Larger payloads will later
/// travel as pre-built objects that the engine hands back for deallocation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    Play,
    /// Pause; the playhead stays where it is.
    Stop,
    /// Jump the playhead back to bar 1.
    Rewind,
    /// Tempo in beats per minute (clamped to 20–999).
    SetTempo(f64),
    SetBeatsPerBar(u32),
    SetMetronome(bool),
    /// Linear gain (1.0 = 0 dB), clamped to 0.0–4.0.
    SetMasterGain(f32),
}
