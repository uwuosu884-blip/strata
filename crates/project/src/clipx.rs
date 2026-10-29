//! Clip View properties (Live 12 ch. 8–9 and Launching Clips): markers, loop,
//! launch / follow actions, scale, warping, envelopes, program change.

use serde::{Deserialize, Serialize};

// ---- Warping -----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum WarpMode {
    #[default]
    Beats,
    Tones,
    Texture,
    RePitch,
    Complex,
    ComplexPro,
}

impl WarpMode {
    pub const ALL: [WarpMode; 6] = [Self::Beats, Self::Tones, Self::Texture, Self::RePitch, Self::Complex, Self::ComplexPro];
    pub fn label(self) -> &'static str {
        match self {
            Self::Beats => "Beats",
            Self::Tones => "Tones",
            Self::Texture => "Texture",
            Self::RePitch => "Re-Pitch",
            Self::Complex => "Complex",
            Self::ComplexPro => "Complex Pro",
        }
    }
}

/// Beats mode "Preserve": transients, or fixed grid divisions (beats).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum Preserve {
    #[default]
    Transients,
    Grid(f64),
}

impl Preserve {
    pub const ALL: [Preserve; 6] = [Self::Transients, Self::Grid(4.0), Self::Grid(2.0), Self::Grid(1.0), Self::Grid(0.5), Self::Grid(0.25)];
    pub fn label(self) -> String {
        match self {
            Self::Transients => "Transients".into(),
            Self::Grid(b) if b >= 4.0 => "1 Bar".into(),
            Self::Grid(b) => format!("1/{}", (4.0 / b).round() as u32),
        }
    }
}

/// Beats mode: what plays in the gap when a segment is slowed down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TransientLoop {
    Off,
    #[default]
    Forward,
    BackAndForth,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WarpParams {
    pub preserve: Preserve,
    pub loop_mode: TransientLoop,
    /// Beats: fade per segment, 0..100 (100 = none).
    pub envelope: f32,
    /// Tones / Texture grain size (Live 12..100).
    pub grain_tones: f32,
    pub grain_texture: f32,
    /// Texture fluctuation 0..100.
    pub flux: f32,
    /// Complex Pro: formant preservation 0..100 %.
    pub formants: f32,
    /// Complex Pro: spectral envelope resolution (8..256).
    pub cpro_envelope: f32,
}

impl Default for WarpParams {
    fn default() -> Self {
        Self {
            preserve: Preserve::Transients,
            loop_mode: TransientLoop::Forward,
            envelope: 100.0,
            grain_tones: 30.0,
            grain_texture: 65.0,
            flux: 25.0,
            formants: 100.0,
            cpro_envelope: 128.0,
        }
    }
}

/// Pins sample frame `frame` to clip beat `beat` (content beats, the same
/// space as MIDI note starts).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct WarpMarker {
    pub beat: f64,
    pub frame: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioProps {
    pub mode: WarpMode,
    pub params: WarpParams,
    /// Sorted by beat (and frame). Empty = straight line from `offset_frames`
    /// at the clip's BPM.
    pub markers: Vec<WarpMarker>,
    /// Pitch: semitones (-48..48) and cents (-50..50).
    pub transpose: i32,
    pub detune: f32,
    pub hiq: bool,
    /// Session clip edge fades (Live: 0–4 ms).
    pub fade: bool,
    pub ram: bool,
    /// Arrangement tempo leader.
    pub lead: bool,
    /// User-edited transients (sample frames); None = detected.
    pub transients: Option<Vec<f64>>,
    /// Arrangement fades (Live: drag the clip's top corners): lengths in beats
    /// from the clip start / end, curvature −1..1 (0 = Live's default S-curve).
    pub fade_in: f64,
    pub fade_out: f64,
    pub fade_in_curve: f32,
    pub fade_out_curve: f32,
}

impl Default for AudioProps {
    fn default() -> Self {
        Self {
            mode: WarpMode::Beats,
            params: WarpParams::default(),
            markers: Vec::new(),
            transpose: 0,
            detune: 0.0,
            hiq: true,
            fade: true,
            ram: false,
            lead: false,
            transients: None,
            fade_in: 0.0,
            fade_out: 0.0,
            fade_in_curve: 0.0,
            fade_out_curve: 0.0,
        }
    }
}

// ---- Launching ------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LaunchMode {
    #[default]
    Trigger,
    Gate,
    Toggle,
    Repeat,
}

impl LaunchMode {
    pub const ALL: [LaunchMode; 4] = [Self::Trigger, Self::Gate, Self::Toggle, Self::Repeat];
    pub fn label(self) -> &'static str {
        match self {
            Self::Trigger => "Trigger",
            Self::Gate => "Gate",
            Self::Toggle => "Toggle",
            Self::Repeat => "Repeat",
        }
    }
}

/// Launch quantization choices: None = Global; Some(0) = None; else beats
/// (triplets are 2/3 of the straight value).
pub const LAUNCH_QUANTS: [(Option<f64>, &str); 15] = [
    (None, "Global"),
    (Some(0.0), "None"),
    (Some(32.0), "8 Bars"),
    (Some(16.0), "4 Bars"),
    (Some(8.0), "2 Bars"),
    (Some(4.0), "1 Bar"),
    (Some(2.0), "1/2"),
    (Some(4.0 / 3.0), "1/2T"),
    (Some(1.0), "1/4"),
    (Some(2.0 / 3.0), "1/4T"),
    (Some(0.5), "1/8"),
    (Some(1.0 / 3.0), "1/8T"),
    (Some(0.25), "1/16"),
    (Some(1.0 / 6.0), "1/16T"),
    (Some(0.125), "1/32"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FollowAction {
    #[default]
    NoAction,
    Stop,
    PlayAgain,
    Previous,
    Next,
    First,
    Last,
    Any,
    Other,
    Jump,
}

impl FollowAction {
    pub const ALL: [FollowAction; 10] = [
        Self::NoAction,
        Self::Stop,
        Self::PlayAgain,
        Self::Previous,
        Self::Next,
        Self::First,
        Self::Last,
        Self::Any,
        Self::Other,
        Self::Jump,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::NoAction => "No Action",
            Self::Stop => "Stop",
            Self::PlayAgain => "Play Again",
            Self::Previous => "Previous",
            Self::Next => "Next",
            Self::First => "First",
            Self::Last => "Last",
            Self::Any => "Any",
            Self::Other => "Other",
            Self::Jump => "Jump",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FollowActions {
    pub enabled: bool,
    pub a: FollowAction,
    pub b: FollowAction,
    /// Chance of A in percent; B gets the rest.
    pub chance_a: u8,
    /// Follow Action Time in beats (used unless `linked`).
    pub time: f64,
    /// Time = clip loop length × `multiplier`.
    pub linked: bool,
    pub multiplier: u16,
    /// Jump targets (scene index, 0-based).
    pub jump_a: u16,
    pub jump_b: u16,
}

impl Default for FollowActions {
    fn default() -> Self {
        Self {
            enabled: false,
            a: FollowAction::Next,
            b: FollowAction::NoAction,
            chance_a: 100,
            time: 4.0,
            linked: true,
            multiplier: 1,
            jump_a: 0,
            jump_b: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LaunchProps {
    pub mode: LaunchMode,
    pub quantize: Option<f64>,
    pub legato: bool,
    /// Velocity sensitivity of the clip volume, 0..1.
    pub velocity: f32,
    pub follow: FollowActions,
}

impl Default for LaunchProps {
    fn default() -> Self {
        Self { mode: LaunchMode::Trigger, quantize: None, legato: false, velocity: 0.0, follow: FollowActions::default() }
    }
}

/// MIDI Bank / Sub-Bank / Program sent on launch (None = "—").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MidiProgram {
    pub bank: Option<u8>,
    pub sub: Option<u8>,
    pub program: Option<u8>,
}

// ---- Scale --------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipScale {
    pub on: bool,
    pub root: u8,
    /// Index into [`SCALES`].
    pub scale: u8,
}

/// Live 12's scale list (name, semitone steps from the root).
pub const SCALES: &[(&str, &[u8])] = &[
    ("Major", &[0, 2, 4, 5, 7, 9, 11]),
    ("Minor", &[0, 2, 3, 5, 7, 8, 10]),
    ("Dorian", &[0, 2, 3, 5, 7, 9, 10]),
    ("Mixolydian", &[0, 2, 4, 5, 7, 9, 10]),
    ("Lydian", &[0, 2, 4, 6, 7, 9, 11]),
    ("Phrygian", &[0, 1, 3, 5, 7, 8, 10]),
    ("Locrian", &[0, 1, 3, 5, 6, 8, 10]),
    ("Whole Tone", &[0, 2, 4, 6, 8, 10]),
    ("Half-whole Dim.", &[0, 1, 3, 4, 6, 7, 9, 10]),
    ("Whole-half Dim.", &[0, 2, 3, 5, 6, 8, 9, 11]),
    ("Minor Blues", &[0, 3, 5, 6, 7, 10]),
    ("Minor Pentatonic", &[0, 3, 5, 7, 10]),
    ("Major Pentatonic", &[0, 2, 4, 7, 9]),
    ("Harmonic Minor", &[0, 2, 3, 5, 7, 8, 11]),
    ("Harmonic Major", &[0, 2, 4, 5, 7, 8, 11]),
    ("Dorian #4", &[0, 2, 3, 6, 7, 9, 10]),
    ("Phrygian Dominant", &[0, 1, 4, 5, 7, 8, 10]),
    ("Melodic Minor", &[0, 2, 3, 5, 7, 9, 11]),
    ("Lydian Augmented", &[0, 2, 4, 6, 8, 9, 11]),
    ("Lydian Dominant", &[0, 2, 4, 6, 7, 9, 10]),
    ("Super Locrian", &[0, 1, 3, 4, 6, 8, 10]),
    ("8-Tone Spanish", &[0, 1, 3, 4, 5, 6, 8, 10]),
    ("Bhairav", &[0, 1, 4, 5, 7, 8, 11]),
    ("Hungarian Minor", &[0, 2, 3, 6, 7, 8, 11]),
    ("Hirajoshi", &[0, 2, 3, 7, 8]),
    ("In-Sen", &[0, 1, 5, 7, 10]),
    ("Iwato", &[0, 1, 5, 6, 10]),
    ("Kumoi", &[0, 2, 3, 7, 9]),
    ("Pelog Selisir", &[0, 1, 3, 7, 8]),
    ("Pelog Tembung", &[0, 1, 5, 7, 8]),
    ("Messiaen 3", &[0, 2, 3, 4, 6, 7, 8, 10, 11]),
    ("Messiaen 4", &[0, 1, 2, 5, 6, 7, 8, 11]),
    ("Messiaen 5", &[0, 1, 5, 6, 7, 11]),
    ("Messiaen 6", &[0, 2, 4, 5, 6, 8, 10, 11]),
    ("Messiaen 7", &[0, 1, 2, 3, 5, 6, 7, 8, 9, 11]),
    ("Chromatic", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]),
];

impl ClipScale {
    pub fn steps(&self) -> &'static [u8] {
        SCALES.get(self.scale as usize).map_or(SCALES[0].1, |s| s.1)
    }
    pub fn name(&self) -> &'static str {
        SCALES.get(self.scale as usize).map_or(SCALES[0].0, |s| s.0)
    }
    pub fn contains(&self, pitch: i32) -> bool {
        let pc = (pitch - self.root as i32).rem_euclid(12) as u8;
        self.steps().contains(&pc)
    }
    /// Scale degree of `pitch` (floor to the nearest scale note at or below).
    pub fn degree(&self, pitch: i32) -> i32 {
        let st = self.steps();
        let n = st.len() as i32;
        let rel = pitch - self.root as i32;
        let oct = rel.div_euclid(12);
        let pc = rel.rem_euclid(12) as u8;
        let idx = st.iter().rposition(|&s| s <= pc).unwrap_or(0) as i32;
        oct * n + idx
    }
    pub fn pitch_of(&self, degree: i32) -> i32 {
        let st = self.steps();
        let n = st.len() as i32;
        self.root as i32 + degree.div_euclid(n) * 12 + st[degree.rem_euclid(n) as usize] as i32
    }
    /// Move `pitch` by `deg` scale degrees (pitch is first snapped into the scale).
    pub fn shift(&self, pitch: i32, deg: i32) -> i32 {
        self.pitch_of(self.degree(pitch) + deg)
    }
    /// Nearest in-scale pitch (ties go down).
    pub fn snap(&self, pitch: i32) -> i32 {
        (0..12).flat_map(|d| [pitch - d, pitch + d]).find(|&p| self.contains(p)).unwrap_or(pitch)
    }
}

pub const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

// ---- Envelopes ----------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum EnvTarget {
    /// Mixer volume modulation (0..1 × fader).
    #[default]
    Volume,
    /// Mixer pan modulation (-1..1 added).
    Pan,
    /// Audio: clip gain (dB offset -inf..0).
    ClipGain,
    /// Audio: transpose (semitones ±48).
    Transpose,
    /// MIDI: pitch bend (±1 → ± bend range).
    PitchBend,
    /// MIDI: mod wheel (0..1).
    ModWheel,
}

impl EnvTarget {
    pub const AUDIO: [EnvTarget; 4] = [Self::ClipGain, Self::Transpose, Self::Volume, Self::Pan];
    pub const MIDI: [EnvTarget; 4] = [Self::Volume, Self::Pan, Self::PitchBend, Self::ModWheel];
    pub fn label(self) -> &'static str {
        match self {
            Self::Volume => "Mixer: Track Volume",
            Self::Pan => "Mixer: Track Panning",
            Self::ClipGain => "Clip: Gain",
            Self::Transpose => "Clip: Transposition",
            Self::PitchBend => "MIDI Ctrl: Pitch Bend",
            Self::ModWheel => "MIDI Ctrl: Mod Wheel",
        }
    }
    /// (min, max, neutral)
    pub fn range(self) -> (f32, f32, f32) {
        match self {
            Self::Volume | Self::ClipGain => (0.0, 1.0, 1.0),
            Self::Pan | Self::PitchBend => (-1.0, 1.0, 0.0),
            Self::Transpose => (-48.0, 48.0, 0.0),
            Self::ModWheel => (0.0, 1.0, 0.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct EnvPoint {
    pub beat: f64,
    pub value: f32,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipEnvelope {
    pub target: EnvTarget,
    /// Sorted by beat (content beats, or envelope beats when unlinked).
    pub points: Vec<EnvPoint>,
    /// Unlinked: own loop of `loop_len` beats from the clip's play start.
    pub unlinked: bool,
    pub loop_len: f64,
}

impl ClipEnvelope {
    /// Value at `beat` (linear between points, held at the ends).
    pub fn value_at(&self, beat: f64) -> f32 {
        let p = &self.points;
        match p.len() {
            0 => self.target.range().2,
            1 => p[0].value,
            _ => {
                let i = p.partition_point(|q| q.beat <= beat);
                if i == 0 {
                    p[0].value
                } else if i >= p.len() {
                    p[p.len() - 1].value
                } else {
                    let (a, b) = (p[i - 1], p[i]);
                    let t = if b.beat > a.beat { ((beat - a.beat) / (b.beat - a.beat)) as f32 } else { 1.0 };
                    a.value + (b.value - a.value) * t
                }
            }
        }
    }
    pub fn sort(&mut self) {
        self.points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    }
}

/// MIDI note expression (MPE): breakpoints in beats relative to the note start.
pub use daw_engine::NoteMpe;

// ---- Groove Pool ------------------------------------------------------------------

/// A groove hit: position within the groove (beats) and velocity 0..1.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct GrooveHit {
    pub pos: f64,
    pub vel: f32,
}

/// Groove Pool entry (Live: Base, Quantize, Timing, Random, Velocity).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Groove {
    pub id: u64,
    pub name: String,
    /// Grid (beats) notes are related to.
    pub base: f64,
    /// Straight quantization to `base`, 0..100 %.
    pub quantize: f32,
    /// Pull towards the groove's timing, 0..100 %.
    pub timing: f32,
    /// Random timing offsets, 0..100 %.
    pub random: f32,
    /// Groove velocity influence, -100..100 %.
    pub velocity: f32,
    /// Pattern length (beats); hits repeat every `length`.
    pub length: f64,
    pub hits: Vec<GrooveHit>,
}

impl Default for Groove {
    fn default() -> Self {
        Self {
            id: 0,
            name: "Groove".into(),
            base: 0.25,
            quantize: 0.0,
            timing: 100.0,
            random: 0.0,
            velocity: 0.0,
            length: 1.0,
            hits: Vec::new(),
        }
    }
}

/// Deterministic noise in -1..1 (per note / grid index).
fn hash_noise(k: u64) -> f64 {
    let mut x = k.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03;
    x ^= x >> 31;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 29;
    (x >> 11) as f64 / (1u64 << 52) as f64 - 1.0
}

impl Groove {
    /// Swing groove: every second `base` step delayed to `pct` % of the pair.
    pub fn swing(base: f64, pct: f64, name: &str) -> Self {
        let off = 2.0 * base * pct / 100.0;
        Self {
            name: name.into(),
            base,
            length: 2.0 * base,
            hits: vec![GrooveHit { pos: 0.0, vel: 1.0 }, GrooveHit { pos: off, vel: 0.8 }],
            ..Default::default()
        }
    }

    /// The built-in library (browser "Grooves").
    pub fn factory() -> Vec<Groove> {
        let mut v = Vec::new();
        for (b, tag) in [(0.5, "8"), (0.25, "16")] {
            for pct in [54.0, 58.0, 62.0, 66.0, 71.0] {
                v.push(Self::swing(b, pct, &format!("Swing {tag}-{pct}")));
            }
        }
        // MPC-style 16th swing with accented downbeats.
        let mut mpc = Self::swing(0.25, 60.0, "MPC 16 Swing-60");
        mpc.hits = vec![GrooveHit { pos: 0.0, vel: 1.0 }, GrooveHit { pos: 0.3, vel: 0.7 }];
        v.push(mpc);
        // Laid-back / pushed straight 16ths.
        let mut lazy = Self::swing(0.25, 50.0, "Straight 16 Late");
        lazy.length = 1.0;
        lazy.hits = (0..4).map(|i| GrooveHit { pos: i as f64 * 0.25 + 0.02, vel: [1.0, 0.75, 0.9, 0.75][i] }).collect();
        v.push(lazy);
        v
    }

    /// Extract from notes (Live: drag a clip to the Groove Pool).
    pub fn from_notes(name: &str, notes: &[crate::Note], length: f64) -> Self {
        let mut hits: Vec<GrooveHit> = notes
            .iter()
            .filter(|n| !n.muted && n.start >= 0.0 && n.start < length)
            .map(|n| GrooveHit { pos: n.start, vel: n.velocity as f32 / 127.0 })
            .collect();
        hits.sort_by(|a, b| a.pos.total_cmp(&b.pos));
        hits.dedup_by(|b, a| (b.pos - a.pos).abs() < 1e-3);
        let peak = hits.iter().map(|h| h.vel).fold(0.0f32, f32::max).max(1e-3);
        hits.iter_mut().for_each(|h| h.vel /= peak);
        Self { name: name.into(), base: 0.25, length: length.max(0.25), hits, ..Default::default() }
    }

    /// Nearest groove hit to grid beat `g` (absolute), as (offset, velocity).
    fn hit_near(&self, g: f64) -> Option<(f64, f32)> {
        if self.hits.is_empty() || self.length <= 0.0 {
            return None;
        }
        let cyc = (g / self.length).floor() * self.length;
        let mut best: Option<(f64, f32)> = None;
        for k in [-1.0, 0.0, 1.0] {
            for h in &self.hits {
                let d = cyc + k * self.length + h.pos - g;
                if best.is_none_or(|(b, _)| d.abs() < b.abs()) {
                    best = Some((d, h.vel));
                }
            }
        }
        // Only hits within half a grid step count.
        best.filter(|(d, _)| d.abs() <= self.base * 0.5 + 1e-9)
    }

    /// Timing offset for a grid point (beats), scaled by the global amount.
    pub fn shift_at(&self, g: f64, amount: f32) -> f64 {
        self.hit_near(g).map_or(0.0, |(d, _)| d * (self.timing * amount / 100.0) as f64)
    }

    /// Apply to a note (`k`: stable index for the random part).
    pub fn apply(&self, n: &crate::Note, amount: f32, k: u64) -> crate::Note {
        let mut out = *n;
        if self.base <= 0.0 {
            return out;
        }
        let g = (n.start / self.base).round() * self.base;
        let mut s = n.start + (g - n.start) * (self.quantize / 100.0) as f64;
        if let Some((d, v)) = self.hit_near(g) {
            s += d * (self.timing * amount / 100.0) as f64;
            let w = self.velocity * amount / 100.0;
            let nv = n.velocity as f32 * (1.0 + (v - 1.0) * w);
            out.velocity = nv.round().clamp(1.0, 127.0) as u8;
        }
        s += hash_noise(k ^ (g.to_bits())) * self.base * 0.25 * (self.random * amount / 100.0) as f64;
        out.start = s;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swing_delays_offbeats_only() {
        let g = Groove::swing(0.5, 66.0, "s");
        let n = |start| crate::Note { start, length: 0.1, pitch: 60, velocity: 100, muted: false, mpe: 0 };
        assert_eq!(g.apply(&n(0.0), 1.0, 0).start, 0.0);
        assert!((g.apply(&n(0.5), 1.0, 1).start - 0.66).abs() < 1e-9);
        assert!((g.apply(&n(0.5), 0.5, 1).start - 0.58).abs() < 1e-9);
        assert!((g.shift_at(1.5, 1.0) - 0.16).abs() < 1e-9);
        assert_eq!(g.apply(&n(0.5), 1.0, 1).velocity, 100);
    }

    #[test]
    fn scale_degrees_round_trip() {
        let s = ClipScale { on: true, root: 2, scale: 0 }; // D major
        assert!(s.contains(62) && s.contains(66) && !s.contains(65));
        assert_eq!(s.shift(62, 1), 64);
        assert_eq!(s.shift(62, 7), 74);
        assert_eq!(s.shift(62, -1), 61);
        assert_eq!(s.snap(65), 64);
        for p in 40..90 {
            if s.contains(p) {
                assert_eq!(s.pitch_of(s.degree(p)), p);
            }
        }
    }

    #[test]
    fn envelope_interpolates() {
        let e = ClipEnvelope {
            target: EnvTarget::Volume,
            points: vec![EnvPoint { beat: 0.0, value: 0.0 }, EnvPoint { beat: 2.0, value: 1.0 }],
            ..Default::default()
        };
        assert_eq!(e.value_at(-1.0), 0.0);
        assert!((e.value_at(1.0) - 0.5).abs() < 1e-6);
        assert_eq!(e.value_at(5.0), 1.0);
    }
}
