//! MIDI Tools (Live 12 ch. 11) and Clip View "Pitch and Time" utilities.
//! Pure functions over notes: the UI keeps the original notes and re-applies
//! on every parameter change (Auto Apply), so tools must be deterministic.
//! Each tool is described by a parameter table; the UI renders it generically.

use crate::Note;
use crate::clipx::{ClipScale, NoteMpe};

// ---- Parameter tables ---------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PKind {
    Num,
    Pct,
    Toggle,
    Grid,
    Choice(&'static [&'static str]),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamSpec {
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub kind: PKind,
}

const fn num(name: &'static str, min: f32, max: f32, default: f32) -> ParamSpec {
    ParamSpec { name, min, max, default, kind: PKind::Num }
}
const fn pct(name: &'static str, min: f32, max: f32, default: f32) -> ParamSpec {
    ParamSpec { name, min, max, default, kind: PKind::Pct }
}
const fn tog(name: &'static str, default: bool) -> ParamSpec {
    ParamSpec { name, min: 0.0, max: 1.0, default: if default { 1.0 } else { 0.0 }, kind: PKind::Toggle }
}
const fn grid(name: &'static str, default: f32) -> ParamSpec {
    ParamSpec { name, min: 0.0, max: (GRIDS.len() - 1) as f32, default, kind: PKind::Grid }
}
const fn choice(name: &'static str, list: &'static [&'static str], default: f32) -> ParamSpec {
    ParamSpec { name, min: 0.0, max: (list.len() - 1) as f32, default, kind: PKind::Choice(list) }
}

/// Grid choices: (label, beats).
pub const GRIDS: [(&str, f64); 12] = [
    ("1/1", 4.0),
    ("1/2", 2.0),
    ("1/2T", 4.0 / 3.0),
    ("1/4", 1.0),
    ("1/4T", 2.0 / 3.0),
    ("1/8", 0.5),
    ("1/8T", 1.0 / 3.0),
    ("1/16", 0.25),
    ("1/16T", 1.0 / 6.0),
    ("1/32", 0.125),
    ("1/32T", 1.0 / 12.0),
    ("1/64", 0.0625),
];

pub fn grid_beats(v: f32) -> f64 {
    GRIDS[(v.round().max(0.0) as usize).min(GRIDS.len() - 1)].1
}

pub const ARP_STYLES: &[&str] = &[
    "Up",
    "Down",
    "Up Down",
    "Down Up",
    "Up & Down",
    "Down & Up",
    "Converge",
    "Diverge",
    "Con & Diverge",
    "Pinky Up",
    "Pinky Up Down",
    "Thumb Up",
    "Thumb Up Down",
    "Play Order",
    "Chord Trigger",
    "Random",
    "Random Other",
    "Random Once",
];
const LFO_TARGETS: &[&str] = &["Pitch Bend", "Slide", "Pressure"];
const LFO_SHAPES: &[&str] = &["Sine", "Square", "Triangle", "Random"];
const GRACE_PITCH: &[&str] = &["High", "Low", "Same"];
const RECOMBINE_DIM: &[&str] = &["Position", "Pitch", "Duration", "Velocity"];
const RECOMBINE_MODE: &[&str] = &["Shuffle", "Mirror", "Rotate"];
const SPAN_MODE: &[&str] = &["Legato", "Tenuto", "Staccato"];
const SHAPES: &[&str] = &["Sine", "Triangle", "Saw Up", "Saw Down", "Square", "Random", "Bounce", "Steps"];
pub const CHORDS: &[(&str, &[i32])] = &[
    ("Maj", &[0, 4, 7]),
    ("min", &[0, 3, 7]),
    ("dim", &[0, 3, 6]),
    ("aug", &[0, 4, 8]),
    ("sus2", &[0, 2, 7]),
    ("sus4", &[0, 5, 7]),
    ("Maj7", &[0, 4, 7, 11]),
    ("min7", &[0, 3, 7, 10]),
    ("7", &[0, 4, 7, 10]),
    ("m7b5", &[0, 3, 6, 10]),
    ("6", &[0, 4, 7, 9]),
    ("add9", &[0, 4, 7, 14]),
];
const CHORD_NAMES: &[&str] = &["Maj", "min", "dim", "aug", "sus2", "sus4", "Maj7", "min7", "7", "m7b5", "6", "add9"];
pub const NOTE_NAMES12: &[&str] = &["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transform {
    #[default]
    Arpeggiate,
    Chop,
    Connect,
    Glissando,
    Lfo,
    Ornament,
    Quantize,
    Recombine,
    Span,
    Strum,
    TimeWarp,
    VelocityShaper,
}

impl Transform {
    pub const ALL: [Transform; 12] = [
        Self::Arpeggiate,
        Self::Chop,
        Self::Connect,
        Self::Glissando,
        Self::Lfo,
        Self::Ornament,
        Self::Quantize,
        Self::Recombine,
        Self::Span,
        Self::Strum,
        Self::TimeWarp,
        Self::VelocityShaper,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Arpeggiate => "Arpeggiate",
            Self::Chop => "Chop",
            Self::Connect => "Connect",
            Self::Glissando => "Glissando",
            Self::Lfo => "LFO",
            Self::Ornament => "Ornament",
            Self::Quantize => "Quantize",
            Self::Recombine => "Recombine",
            Self::Span => "Span",
            Self::Strum => "Strum",
            Self::TimeWarp => "Time Warp",
            Self::VelocityShaper => "Velocity Shaper",
        }
    }
    /// MPE tools write note expression.
    pub fn is_mpe(self) -> bool {
        matches!(self, Self::Glissando | Self::Lfo)
    }
    pub fn params(self) -> &'static [ParamSpec] {
        const ARP: &[ParamSpec] = &[
            choice("Style", ARP_STYLES, 0.0),
            num("Distance", -24.0, 24.0, 12.0),
            num("Steps", 0.0, 8.0, 0.0),
            grid("Rate", 7.0),
            pct("Gate", 1.0, 200.0, 100.0),
        ];
        const CHOP: &[ParamSpec] = &[
            num("Parts", 2.0, 64.0, 4.0),
            num("Gaps", -16.0, 16.0, 0.0),
            pct("Variation", 0.0, 100.0, 0.0),
            pct("Stretch", -100.0, 100.0, 0.0),
            num("Pattern", 0.0, 65535.0, 65535.0),
        ];
        const CONNECT: &[ParamSpec] =
            &[num("Spread", 0.0, 12.0, 0.0), pct("Density", 0.0, 100.0, 100.0), grid("Rate", 7.0), pct("Tie", 1.0, 100.0, 100.0)];
        const GLISS: &[ParamSpec] = &[pct("Start", 0.0, 100.0, 50.0), num("Curve", -1.0, 1.0, 0.0)];
        const LFO: &[ParamSpec] = &[
            choice("Target", LFO_TARGETS, 0.0),
            choice("Shape", LFO_SHAPES, 0.0),
            num("Rate", 0.25, 32.0, 4.0),
            pct("Time Shift", 0.0, 100.0, 0.0),
            num("Amount", -48.0, 48.0, 1.0),
            num("Base", -48.0, 48.0, 0.0),
            pct("Attack", 0.0, 100.0, 0.0),
            pct("Decay", 0.0, 100.0, 0.0),
            num("Seed", 0.0, 9999.0, 1.0),
        ];
        const ORN: &[ParamSpec] = &[
            tog("Flam", true),
            num("Flam Pos", -1.0, 1.0, -0.5),
            pct("Flam Vel", 0.0, 100.0, 60.0),
            tog("Grace", false),
            choice("Grace Pitch", GRACE_PITCH, 0.0),
            num("Grace Pos", -1.0, 1.0, -0.5),
            pct("Grace Vel", 0.0, 100.0, 70.0),
            pct("Chance", 0.0, 100.0, 100.0),
            num("Amount", 1.0, 4.0, 1.0),
        ];
        const QUANT: &[ParamSpec] = &[grid("Grid", 7.0), tog("Start", true), tog("End", false), pct("Amount", 0.0, 100.0, 100.0)];
        const RECOMB: &[ParamSpec] = &[
            choice("Dimension", RECOMBINE_DIM, 1.0),
            choice("Mode", RECOMBINE_MODE, 0.0),
            num("Rotate", -16.0, 16.0, 1.0),
            num("Seed", 0.0, 9999.0, 1.0),
            tog("On Grid", false),
        ];
        const SPAN: &[ParamSpec] = &[choice("Mode", SPAN_MODE, 0.0), pct("Offset", -100.0, 100.0, 0.0), pct("Variation", 0.0, 100.0, 0.0)];
        const STRUM: &[ParamSpec] = &[pct("Low", -100.0, 100.0, 0.0), pct("High", -100.0, 100.0, 50.0), num("Tension", -1.0, 1.0, 0.0)];
        const TW: &[ParamSpec] = &[
            num("Breakpoints", 1.0, 3.0, 1.0),
            pct("In 1", 0.0, 100.0, 50.0),
            pct("Out 1", 0.0, 100.0, 30.0),
            pct("In 2", 0.0, 100.0, 70.0),
            pct("Out 2", 0.0, 100.0, 70.0),
            pct("In 3", 0.0, 100.0, 90.0),
            pct("Out 3", 0.0, 100.0, 90.0),
            pct("Quantize", 0.0, 100.0, 0.0),
            tog("Preserve Range", true),
            tog("Include Note End", false),
        ];
        const VS: &[ParamSpec] = &[
            num("Min", 1.0, 127.0, 20.0),
            num("Max", 1.0, 127.0, 127.0),
            choice("Shape", SHAPES, 3.0),
            tog("Loop", false),
            grid("Division", 3.0),
            pct("Rotate", 0.0, 100.0, 0.0),
        ];
        match self {
            Self::Arpeggiate => ARP,
            Self::Chop => CHOP,
            Self::Connect => CONNECT,
            Self::Glissando => GLISS,
            Self::Lfo => LFO,
            Self::Ornament => ORN,
            Self::Quantize => QUANT,
            Self::Recombine => RECOMB,
            Self::Span => SPAN,
            Self::Strum => STRUM,
            Self::TimeWarp => TW,
            Self::VelocityShaper => VS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Generate {
    #[default]
    Rhythm,
    Seed,
    Shape,
    Stacks,
    Euclidean,
}

impl Generate {
    pub const ALL: [Generate; 5] = [Self::Rhythm, Self::Seed, Self::Shape, Self::Stacks, Self::Euclidean];
    pub fn label(self) -> &'static str {
        match self {
            Self::Rhythm => "Rhythm",
            Self::Seed => "Seed",
            Self::Shape => "Shape",
            Self::Stacks => "Stacks",
            Self::Euclidean => "Euclidean",
        }
    }
    pub fn params(self) -> &'static [ParamSpec] {
        const RHY: &[ParamSpec] = &[
            num("Pitch", 0.0, 127.0, 36.0),
            num("Steps", 1.0, 16.0, 16.0),
            num("Pattern", 0.0, 99.0, 0.0),
            pct("Density", 0.0, 100.0, 50.0),
            grid("Step Duration", 7.0),
            num("Split", 1.0, 4.0, 1.0),
            num("Shift", -16.0, 16.0, 0.0),
            num("Velocity", 1.0, 127.0, 100.0),
            pct("Accent", 0.0, 100.0, 0.0),
            num("Frequency", 1.0, 16.0, 4.0),
            num("Offset", 0.0, 15.0, 0.0),
        ];
        const SEED: &[ParamSpec] = &[
            num("Pitch Min", 0.0, 127.0, 48.0),
            num("Pitch Max", 0.0, 127.0, 72.0),
            grid("Dur Min", 7.0),
            grid("Dur Max", 5.0),
            num("Vel Min", 1.0, 127.0, 60.0),
            num("Vel Max", 1.0, 127.0, 120.0),
            num("Voices", 1.0, 8.0, 1.0),
            pct("Density", 0.0, 100.0, 50.0),
            num("Seed", 0.0, 9999.0, 1.0),
        ];
        const SHAPE: &[ParamSpec] = &[
            choice("Shape", SHAPES, 0.0),
            num("Min", 0.0, 127.0, 48.0),
            num("Max", 0.0, 127.0, 72.0),
            grid("Rate", 7.0),
            pct("Tie", 1.0, 100.0, 100.0),
            pct("Density", 0.0, 100.0, 100.0),
            pct("Jitter", 0.0, 100.0, 0.0),
            num("Seed", 0.0, 9999.0, 1.0),
        ];
        const STACKS: &[ParamSpec] = &[
            choice("Root", NOTE_NAMES12, 0.0),
            num("Octave", -1.0, 8.0, 3.0),
            choice("Chord", CHORD_NAMES, 0.0),
            num("Inversion", -3.0, 3.0, 0.0),
            num("Duration", 0.25, 16.0, 4.0),
            num("Offset", 0.0, 16.0, 0.0),
            num("Slot", 1.0, 8.0, 1.0),
        ];
        const EUC: &[ParamSpec] = &[
            num("Steps", 1.0, 32.0, 16.0),
            grid("Division", 7.0),
            tog("V1", true),
            num("V1 Pitch", 0.0, 127.0, 36.0),
            num("V1 Density", 0.0, 32.0, 4.0),
            num("V1 Rotate", 0.0, 31.0, 0.0),
            num("V1 Vel", 1.0, 127.0, 110.0),
            tog("V2", true),
            num("V2 Pitch", 0.0, 127.0, 38.0),
            num("V2 Density", 0.0, 32.0, 2.0),
            num("V2 Rotate", 0.0, 31.0, 4.0),
            num("V2 Vel", 1.0, 127.0, 100.0),
            tog("V3", true),
            num("V3 Pitch", 0.0, 127.0, 42.0),
            num("V3 Density", 0.0, 32.0, 7.0),
            num("V3 Rotate", 0.0, 31.0, 0.0),
            num("V3 Vel", 1.0, 127.0, 80.0),
            tog("V4", false),
            num("V4 Pitch", 0.0, 127.0, 46.0),
            num("V4 Density", 0.0, 32.0, 3.0),
            num("V4 Rotate", 0.0, 31.0, 2.0),
            num("V4 Vel", 1.0, 127.0, 90.0),
            pct("Random", 0.0, 100.0, 0.0),
        ];
        match self {
            Self::Rhythm => RHY,
            Self::Seed => SEED,
            Self::Shape => SHAPE,
            Self::Stacks => STACKS,
            Self::Euclidean => EUC,
        }
    }
}

pub fn defaults(specs: &[ParamSpec]) -> Vec<f32> {
    specs.iter().map(|s| s.default).collect()
}

// ---- Context / helpers ------------------------------------------------------------

/// Where tools act: time range (selection, time selection or loop), scale.
#[derive(Debug, Clone, Copy)]
pub struct Ctx {
    pub range: (f64, f64),
    pub scale: Option<ClipScale>,
}

struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    /// 0..1
    fn f(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.f()
    }
}

fn p(v: &[f32], i: usize) -> f32 {
    v.get(i).copied().unwrap_or(0.0)
}

fn clamp_pitch(x: i32) -> u8 {
    x.clamp(0, 127) as u8
}

fn snap_pitch(cx: &Ctx, pitch: i32) -> i32 {
    match cx.scale {
        Some(s) if s.on => s.snap(pitch),
        _ => pitch,
    }
}

/// Move `pitch` by `n` steps: scale degrees when a scale is on, else semitones.
fn step_pitch(cx: &Ctx, pitch: i32, n: i32) -> i32 {
    match cx.scale {
        Some(s) if s.on => s.shift(s.snap(pitch), n),
        _ => pitch + n,
    }
}

fn note(start: f64, length: f64, pitch: i32, velocity: f32) -> Note {
    Note {
        start,
        length: length.max(1.0 / 256.0),
        pitch: clamp_pitch(pitch),
        velocity: velocity.round().clamp(1.0, 127.0) as u8,
        muted: false,
        mpe: 0,
        vel_dev: 0,
    }
}

/// Groups of notes that overlap in time (chords), sorted by start.
fn chords(notes: &[Note]) -> Vec<Vec<Note>> {
    let mut v = notes.to_vec();
    v.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
    let mut out: Vec<Vec<Note>> = Vec::new();
    let mut end = f64::NEG_INFINITY;
    for n in v {
        if n.start < end - 1e-9
            && let Some(last) = out.last_mut()
        {
            end = end.max(n.start + n.length);
            last.push(n);
        } else {
            end = n.start + n.length;
            out.push(vec![n]);
        }
    }
    out
}

/// Shape value 0..1 at phase `t` (0..1).
fn shape(kind: usize, t: f64, rng: &mut Rng) -> f64 {
    let t = t.rem_euclid(1.0);
    match kind {
        0 => 0.5 - 0.5 * (t * std::f64::consts::TAU).cos(),
        1 => 1.0 - (2.0 * t - 1.0).abs(),
        2 => t,
        3 => 1.0 - t,
        4 => {
            if t < 0.5 {
                1.0
            } else {
                0.0
            }
        }
        5 => rng.f(),
        6 => (t * std::f64::consts::PI).sin().abs(),
        _ => (t * 4.0).floor() / 3.0,
    }
}

// ---- Transform tools ------------------------------------------------------------------

/// Apply a Transform tool to `input` (the targeted notes); returns the
/// replacement notes. MPE tools append expression entries to `mpe`.
pub fn transform(tool: Transform, v: &[f32], input: &[Note], mpe: &mut Vec<NoteMpe>, cx: &Ctx) -> Vec<Note> {
    if input.is_empty() {
        return Vec::new();
    }
    match tool {
        Transform::Arpeggiate => arpeggiate(v, input, cx),
        Transform::Chop => chop(v, input),
        Transform::Connect => connect(v, input, cx),
        Transform::Glissando => glissando(v, input, mpe),
        Transform::Lfo => lfo(v, input, mpe),
        Transform::Ornament => ornament(v, input, cx),
        Transform::Quantize => {
            let g = grid_beats(p(v, 0));
            let (qs, qe, amt) = (p(v, 1) > 0.5, p(v, 2) > 0.5, p(v, 3) as f64 / 100.0);
            input
                .iter()
                .map(|n| {
                    let mut n = *n;
                    let end = n.start + n.length;
                    if qs {
                        n.start += ((n.start / g).round() * g - n.start) * amt;
                    }
                    let e = if qe {
                        end + ((end / g).round() * g - end) * amt
                    } else if qs {
                        n.start + n.length
                    } else {
                        end
                    };
                    n.length = (e - n.start).max(g * 0.25 * amt.max(0.01)).max(1.0 / 256.0);
                    n
                })
                .collect()
        }
        Transform::Recombine => recombine(v, input),
        Transform::Span => span(v, input),
        Transform::Strum => strum(v, input),
        Transform::TimeWarp => time_warp(v, input, cx),
        Transform::VelocityShaper => velocity_shaper(v, input, cx),
    }
}

fn arpeggiate(v: &[f32], input: &[Note], cx: &Ctx) -> Vec<Note> {
    let style = p(v, 0) as usize;
    let distance = p(v, 1) as i32;
    let steps = p(v, 2) as i32;
    let rate = grid_beats(p(v, 3));
    let gate = p(v, 4) as f64 / 100.0;
    let mut out = Vec::new();
    let mut rng = Rng::new(17);
    for ch in chords(input) {
        let s = ch.iter().map(|n| n.start).fold(f64::INFINITY, f64::min);
        let e = ch.iter().map(|n| n.start + n.length).fold(f64::NEG_INFINITY, f64::max);
        let mut asc: Vec<&Note> = ch.iter().collect();
        asc.sort_by_key(|n| n.pitch);
        let order: Vec<&Note> = ch.iter().collect(); // play order (by start)
        // Base pattern over all transposition steps.
        let mut base: Vec<(i32, u8)> = Vec::new();
        for k in 0..=steps {
            for n in &asc {
                base.push((step_pitch(cx, n.pitch as i32, k * distance), n.velocity));
            }
        }
        let rev: Vec<(i32, u8)> = base.iter().rev().copied().collect();
        let m = base.len();
        let pattern: Vec<(i32, u8)> = match style {
            0 => base.clone(),
            1 => rev.clone(),
            2 => base.iter().chain(rev.iter().skip(1).take(m.saturating_sub(2))).copied().collect(),
            3 => rev.iter().chain(base.iter().skip(1).take(m.saturating_sub(2))).copied().collect(),
            4 => base.iter().chain(rev.iter()).copied().collect(),
            5 => rev.iter().chain(base.iter()).copied().collect(),
            6..=8 => {
                let mut c = Vec::new();
                let (mut i, mut j) = (0usize, m.saturating_sub(1));
                while i <= j && j < m {
                    c.push(base[i]);
                    if i != j {
                        c.push(base[j]);
                    }
                    i += 1;
                    if j == 0 {
                        break;
                    }
                    j -= 1;
                }
                match style {
                    6 => c,
                    7 => c.into_iter().rev().collect(),
                    _ => {
                        let mut d = c.clone();
                        d.extend(c.into_iter().rev().skip(1));
                        d
                    }
                }
            }
            9 | 10 => {
                // Pinky: alternate the top note with each other note.
                let top = *base.last().unwrap_or(&(60, 100));
                let mut c: Vec<(i32, u8)> = base[..m.saturating_sub(1)].iter().flat_map(|&x| [x, top]).collect();
                if style == 10 {
                    let back: Vec<(i32, u8)> = c.iter().rev().copied().collect();
                    c.extend(back);
                }
                c
            }
            11 | 12 => {
                let bot = base[0];
                let mut c: Vec<(i32, u8)> = base[1..].iter().flat_map(|&x| [bot, x]).collect();
                if style == 12 {
                    let back: Vec<(i32, u8)> = c.iter().rev().copied().collect();
                    c.extend(back);
                }
                c
            }
            13 => (0..=steps).flat_map(|k| order.iter().map(move |n| (step_pitch(cx, n.pitch as i32, k * distance), n.velocity))).collect(),
            _ => base.clone(),
        };
        if pattern.is_empty() {
            continue;
        }
        let count = ((e - s) / rate - 1e-9).ceil().max(1.0) as usize;
        let mut once: Vec<(i32, u8)> = pattern.clone();
        if style == 17 {
            // Random Once: one shuffled order, repeated.
            for i in (1..once.len()).rev() {
                once.swap(i, (rng.next() % (i as u64 + 1)) as usize);
            }
        }
        let mut last = usize::MAX;
        for i in 0..count {
            let t = s + i as f64 * rate;
            let len = (rate * gate).min(e - t + rate * (gate - 1.0).max(0.0));
            match style {
                14 => {
                    // Chord Trigger: the whole chord on every step.
                    for n in &asc {
                        out.push(note(t, len, n.pitch as i32, n.velocity as f32));
                    }
                }
                15 | 16 => {
                    let mut k = (rng.next() % pattern.len() as u64) as usize;
                    if style == 16 && pattern.len() > 1 && k == last {
                        k = (k + 1) % pattern.len();
                    }
                    last = k;
                    out.push(note(t, len, pattern[k].0, pattern[k].1 as f32));
                }
                17 => {
                    let x = once[i % once.len()];
                    out.push(note(t, len, x.0, x.1 as f32));
                }
                _ => {
                    let x = pattern[i % pattern.len()];
                    out.push(note(t, len, x.0, x.1 as f32));
                }
            }
        }
    }
    out
}

fn chop(v: &[f32], input: &[Note]) -> Vec<Note> {
    let parts = (p(v, 0) as usize).clamp(2, 64);
    let gaps = p(v, 1) as i32; // >0: every n-th part removed; <0: keep every n-th
    let variation = p(v, 2) as f64 / 100.0;
    let stretch = p(v, 3) as f64 / 100.0;
    let pattern = p(v, 4) as u32;
    let mut rng = Rng::new(5);
    let mut out = Vec::new();
    for n in input {
        // Part lengths: stretched towards the start (>0) or end (<0), plus variation.
        let w: Vec<f64> = (0..parts)
            .map(|i| {
                let t = i as f64 / (parts - 1) as f64;
                let s = 1.0 + stretch * (1.0 - 2.0 * t);
                (s * (1.0 + variation * (rng.f() - 0.5))).max(0.05)
            })
            .collect();
        let total: f64 = w.iter().sum();
        let mut t = n.start;
        for (i, wi) in w.iter().enumerate() {
            let len = n.length * wi / total;
            let on = pattern & (1 << (i % 16)) != 0;
            let gap_off = match gaps {
                g if g > 0 => (i + 1) % (g as usize + 1) == 0,
                g if g < 0 => i % ((-g) as usize + 1) != 0,
                _ => false,
            };
            if on && !gap_off {
                out.push(Note { start: t, length: len * 0.98, ..*n });
            }
            t += len;
        }
    }
    out
}

fn connect(v: &[f32], input: &[Note], cx: &Ctx) -> Vec<Note> {
    let spread = p(v, 0) as i32;
    let density = p(v, 1) as f64 / 100.0;
    let rate = grid_beats(p(v, 2));
    let tie = p(v, 3) as f64 / 100.0;
    let mut sorted = input.to_vec();
    sorted.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut out = sorted.clone();
    let mut rng = Rng::new(11);
    for w in sorted.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (gs, ge) = (a.start + a.length, b.start);
        if ge - gs < rate * 0.5 {
            continue;
        }
        let k = (((ge - gs) / rate) - 1e-9).floor() as usize;
        for i in 0..k {
            if rng.f() >= density {
                continue;
            }
            let t = gs + i as f64 * rate;
            let f = (t - a.start) / (b.start - a.start).max(1e-9);
            let mut pitch = a.pitch as f64 + (b.pitch as f64 - a.pitch as f64) * f;
            if spread > 0 {
                pitch += rng.range(-(spread as f64), spread as f64);
            }
            let vel = a.velocity as f64 + (b.velocity as f64 - a.velocity as f64) * f;
            out.push(note(t, (rate * tie).min(ge - t), snap_pitch(cx, pitch.round() as i32), vel as f32));
        }
    }
    out
}

fn push_mpe(mpe: &mut Vec<NoteMpe>, n: &mut Note, f: impl FnOnce(&mut NoteMpe)) {
    let mut e = if n.mpe > 0 { mpe.get(n.mpe as usize - 1).cloned().unwrap_or_default() } else { NoteMpe::default() };
    f(&mut e);
    mpe.push(e);
    n.mpe = mpe.len() as u32;
}

fn glissando(v: &[f32], input: &[Note], mpe: &mut Vec<NoteMpe>) -> Vec<Note> {
    let start = p(v, 0) as f64 / 100.0;
    let curve = p(v, 1) as f64;
    let mut sorted = input.to_vec();
    sorted.sort_by(|a, b| a.start.total_cmp(&b.start));
    let nexts: Vec<Option<u8>> =
        (0..sorted.len()).map(|i| sorted.iter().skip(i + 1).find(|m| m.start > sorted[i].start + 1e-9).map(|m| m.pitch)).collect();
    for (n, next) in sorted.iter_mut().zip(nexts) {
        let Some(target) = next else { continue };
        let d = target as f32 - n.pitch as f32;
        if d == 0.0 {
            continue;
        }
        let len = n.length;
        push_mpe(mpe, n, |e| {
            let t0 = len * start;
            e.pitch = vec![(0.0, 0.0), (t0, 0.0)];
            for k in 1..=16 {
                let u = k as f64 / 16.0;
                // Curve: >0 slow start (exp), <0 fast start (log).
                let y = if curve >= 0.0 { u.powf(1.0 + 3.0 * curve) } else { 1.0 - (1.0 - u).powf(1.0 - 3.0 * curve) };
                e.pitch.push((t0 + (len - t0) * u, d * y as f32));
            }
        });
    }
    sorted
}

fn lfo(v: &[f32], input: &[Note], mpe: &mut Vec<NoteMpe>) -> Vec<Note> {
    let target = p(v, 0) as usize;
    let shape_k = p(v, 1) as usize;
    let rate = p(v, 2) as f64; // cycles per beat
    let shift = p(v, 3) as f64 / 100.0;
    let amount = p(v, 4);
    let base = p(v, 5);
    let attack = p(v, 6) as f64 / 100.0;
    let decay = p(v, 7) as f64 / 100.0;
    let mut rng = Rng::new(p(v, 8) as u64 + 1);
    let mut out = input.to_vec();
    for n in &mut out {
        let len = n.length;
        let pts = ((len * rate * 16.0).ceil() as usize).clamp(8, 256);
        let mut curve = Vec::with_capacity(pts + 1);
        let mut held = 0.0;
        for k in 0..=pts {
            let t = len * k as f64 / pts as f64;
            let phase = t * rate + shift;
            let s = match shape_k {
                0 => (phase * std::f64::consts::TAU).sin(),
                1 => {
                    if phase.rem_euclid(1.0) < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                2 => 1.0 - 4.0 * (phase.rem_euclid(1.0) - 0.5).abs(),
                _ => {
                    // Sample & hold per cycle.
                    if k == 0 || (phase.floor() != ((len * (k - 1) as f64 / pts as f64) * rate + shift).floor()) {
                        held = rng.f() * 2.0 - 1.0;
                    }
                    held
                }
            };
            let u = t / len.max(1e-9);
            let env =
                (if attack > 0.0 { (u / attack).min(1.0) } else { 1.0 }) * (if decay > 0.0 { ((1.0 - u) / decay).min(1.0) } else { 1.0 });
            let mut y = base + amount * (s * env) as f32;
            if target > 0 {
                // Slide / pressure are 0..1: Amount/Base read as percent of 48.
                y = (0.5 + y / 96.0).clamp(0.0, 1.0);
            }
            curve.push((t, y));
        }
        push_mpe(mpe, n, |e| match target {
            0 => e.pitch = curve,
            1 => e.slide = curve,
            _ => e.pressure = curve,
        });
    }
    out
}

fn ornament(v: &[f32], input: &[Note], cx: &Ctx) -> Vec<Note> {
    let mut out = input.to_vec();
    let mut rng = Rng::new(3);
    let unit = 1.0 / 16.0; // position ±1 = ±1/16 beat
    for n in input {
        if p(v, 0) > 0.5 {
            let pos = p(v, 1) as f64 * unit;
            let vel = n.velocity as f32 * p(v, 2) / 100.0;
            out.push(note(n.start + pos, unit * 0.5, n.pitch as i32, vel));
        }
        if p(v, 3) > 0.5 && rng.f() * 100.0 < p(v, 7) as f64 {
            let dir = match p(v, 4) as usize {
                0 => 1,
                1 => -1,
                _ => 0,
            };
            let count = p(v, 8).round().clamp(1.0, 4.0) as i32;
            let pos = p(v, 5) as f64 * unit * 2.0;
            for k in 0..count {
                let pitch = step_pitch(cx, n.pitch as i32, dir * (count - k));
                let t = n.start + pos * (count - k) as f64 / count as f64;
                out.push(note(t, (pos.abs() / count as f64).max(1.0 / 64.0), pitch, n.velocity as f32 * p(v, 6) / 100.0));
            }
        }
    }
    out
}

fn recombine(v: &[f32], input: &[Note]) -> Vec<Note> {
    let dim = p(v, 0) as usize;
    let mode = p(v, 1) as usize;
    let rot = p(v, 2) as i64;
    let mut rng = Rng::new(p(v, 3) as u64 + 1);
    let on_grid = p(v, 4) > 0.5;
    let mut out = input.to_vec();
    out.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
    let n = out.len();
    let mut idx: Vec<usize> = (0..n).collect();
    match mode {
        0 => {
            for i in (1..n).rev() {
                idx.swap(i, (rng.next() % (i as u64 + 1)) as usize);
            }
        }
        1 => idx.reverse(),
        _ => idx.rotate_left(rot.rem_euclid(n.max(1) as i64) as usize),
    }
    let src = out.clone();
    for (i, k) in idx.into_iter().enumerate() {
        let s = &src[k];
        match dim {
            0 => {
                // Mirror positions: reflect around the range centre.
                out[i].start = if mode == 1 {
                    let (a, b) = (src[0].start, src[n - 1].start + src[n - 1].length);
                    a + b - (src[i].start + src[i].length)
                } else {
                    s.start
                };
                if on_grid {
                    out[i].start = (out[i].start * 4.0).round() / 4.0;
                }
            }
            1 => out[i].pitch = s.pitch,
            2 => out[i].length = s.length,
            _ => out[i].velocity = s.velocity,
        }
    }
    out
}

fn span(v: &[f32], input: &[Note]) -> Vec<Note> {
    let mode = p(v, 0) as usize;
    let offset = p(v, 1) as f64 / 100.0;
    let variation = p(v, 2) as f64 / 100.0;
    let mut rng = Rng::new(9);
    let mut out = input.to_vec();
    out.sort_by(|a, b| a.start.total_cmp(&b.start));
    let starts: Vec<f64> = out.iter().map(|n| n.start).collect();
    let last_end = out.iter().map(|n| n.start + n.length).fold(0.0, f64::max);
    for n in out.iter_mut() {
        let next = starts.iter().copied().find(|&s| s > n.start + 1e-9).unwrap_or(last_end.max(n.start + n.length));
        let gap = next - n.start;
        let base = match mode {
            0 => gap,        // legato: to the next note
            1 => gap * 0.9,  // tenuto: held, slight separation
            _ => gap * 0.35, // staccato
        };
        let var = 1.0 + variation * (rng.f() - 0.5);
        n.length = (base * (1.0 + offset) * var).max(1.0 / 128.0);
    }
    out
}

fn strum(v: &[f32], input: &[Note]) -> Vec<Note> {
    let low = p(v, 0) as f64 / 100.0;
    let high = p(v, 1) as f64 / 100.0;
    let tension = p(v, 2) as f64;
    const MAX: f64 = 0.25; // ±100 % = ±1/16 bar
    let mut out = Vec::new();
    for ch in chords(input) {
        let mut ch = ch;
        ch.sort_by_key(|n| n.pitch);
        let k = ch.len();
        for (i, n) in ch.iter().enumerate() {
            let t = if k > 1 { i as f64 / (k - 1) as f64 } else { 0.0 };
            let t = if tension >= 0.0 { t.powf(1.0 + 2.0 * tension) } else { 1.0 - (1.0 - t).powf(1.0 - 2.0 * tension) };
            let off = (low + (high - low) * t) * MAX;
            let end = n.start + n.length;
            let s = (n.start + off).min(end - 1.0 / 128.0);
            out.push(Note { start: s, length: end - s, ..*n });
        }
    }
    out
}

fn time_warp(v: &[f32], input: &[Note], cx: &Ctx) -> Vec<Note> {
    let k = (p(v, 0) as usize).clamp(1, 3);
    let (a, b) = if p(v, 8) > 0.5 {
        cx.range
    } else {
        let s = input.iter().map(|n| n.start).fold(f64::INFINITY, f64::min);
        (s, input.iter().map(|n| n.start + n.length).fold(s, f64::max))
    };
    let l = (b - a).max(1e-6);
    let mut pts: Vec<(f64, f64)> = vec![(0.0, 0.0)];
    for i in 0..k {
        pts.push(((p(v, 1 + 2 * i) as f64 / 100.0).clamp(0.0, 1.0), (p(v, 2 + 2 * i) as f64 / 100.0).clamp(0.0, 1.0)));
    }
    pts.push((1.0, 1.0));
    pts.sort_by(|x, y| x.0.total_cmp(&y.0));
    // Output must stay monotonic.
    for i in 1..pts.len() {
        pts[i].1 = pts[i].1.max(pts[i - 1].1);
    }
    let map = |t: f64| -> f64 {
        let u = ((t - a) / l).clamp(0.0, 1.0);
        let i = pts.partition_point(|q| q.0 <= u).clamp(1, pts.len() - 1);
        let (p0, p1) = (pts[i - 1], pts[i]);
        let f = if p1.0 > p0.0 { (u - p0.0) / (p1.0 - p0.0) } else { 1.0 };
        a + (p0.1 + (p1.1 - p0.1) * f) * l
    };
    let q = p(v, 7) as f64 / 100.0;
    let include_end = p(v, 9) > 0.5;
    input
        .iter()
        .map(|n| {
            let mut s = map(n.start);
            s += ((s * 4.0).round() / 4.0 - s) * q;
            let e = if include_end { map(n.start + n.length) } else { s + n.length };
            Note { start: s, length: (e - s).max(1.0 / 128.0), ..*n }
        })
        .collect()
}

fn velocity_shaper(v: &[f32], input: &[Note], cx: &Ctx) -> Vec<Note> {
    let (lo, hi) = (p(v, 0) as f64, p(v, 1) as f64);
    let kind = p(v, 2) as usize;
    let looped = p(v, 3) > 0.5;
    let div = grid_beats(p(v, 4));
    let rot = p(v, 5) as f64 / 100.0;
    let (a, b) = cx.range;
    let len = if looped { div } else { (b - a).max(1e-6) };
    let mut rng = Rng::new(21);
    input
        .iter()
        .map(|n| {
            let t = ((n.start - a) / len + rot).rem_euclid(1.0);
            let y = shape(kind, t, &mut rng);
            Note { velocity: (lo + (hi - lo) * y).round().clamp(1.0, 127.0) as u8, ..*n }
        })
        .collect()
}

// ---- Generate tools ---------------------------------------------------------------------

/// Bjorklund / Euclidean rhythm: `k` hits over `n` steps, rotated.
pub fn euclid(k: usize, n: usize, rot: usize) -> Vec<bool> {
    if n == 0 {
        return Vec::new();
    }
    let k = k.min(n);
    let mut v: Vec<bool> = (0..n).map(|i| (i * k) % n < k).collect();
    // The modular formula puts a hit on step 0; rotate as requested.
    v.rotate_right(rot % n);
    v
}

/// Run a Generate tool over `cx.range`; returns the new notes (the caller
/// replaces notes overlapping the range).
pub fn generate(tool: Generate, v: &[f32], cx: &Ctx) -> Vec<Note> {
    let (a, b) = cx.range;
    let mut out = Vec::new();
    if b <= a {
        return out;
    }
    match tool {
        Generate::Rhythm => {
            let pitch = p(v, 0) as i32;
            let steps = (p(v, 1) as usize).clamp(1, 16);
            let pattern = p(v, 2) as u64;
            let density = p(v, 3) as f64 / 100.0;
            let dur = grid_beats(p(v, 4));
            let split = (p(v, 5) as usize).clamp(1, 4);
            let shift = p(v, 6) as i64;
            let vel = p(v, 7);
            let accent = p(v, 8) / 100.0;
            let freq = (p(v, 9) as usize).max(1);
            let aoff = p(v, 10) as usize;
            // Pattern 0 = evenly spread (Euclidean); others = seeded variations.
            let hits = (steps as f64 * density).round() as usize;
            let mut pat = euclid(hits, steps, 0);
            if pattern > 0 {
                let mut rng = Rng::new(pattern);
                for i in (1..steps).rev() {
                    pat.swap(i, (rng.next() % (i as u64 + 1)) as usize);
                }
            }
            pat.rotate_right(shift.rem_euclid(steps as i64) as usize);
            let mut t = a;
            let mut i = 0usize;
            let mut hit_no = 0usize;
            while t < b - 1e-9 {
                if pat[i % steps] {
                    let acc = (hit_no + aoff).is_multiple_of(freq);
                    let v = vel * if acc { 1.0 + accent } else { 1.0 - accent * 0.3 };
                    let sub = dur / split as f64;
                    for s in 0..split {
                        out.push(note(t + s as f64 * sub, sub * 0.9, snap_pitch(cx, pitch), v));
                    }
                    hit_no += 1;
                }
                t += dur;
                i += 1;
            }
        }
        Generate::Seed => {
            let (pmin, pmax) = (p(v, 0).min(p(v, 1)) as f64, p(v, 0).max(p(v, 1)) as f64);
            let (dmin, dmax) = (grid_beats(p(v, 2)).min(grid_beats(p(v, 3))), grid_beats(p(v, 2)).max(grid_beats(p(v, 3))));
            let (vmin, vmax) = (p(v, 4).min(p(v, 5)) as f64, p(v, 4).max(p(v, 5)) as f64);
            let voices = (p(v, 6) as usize).clamp(1, 8);
            let density = p(v, 7) as f64 / 100.0;
            let mut rng = Rng::new(p(v, 8) as u64 + 1);
            for _ in 0..voices {
                let mut t = a;
                while t < b - 1e-9 {
                    let d = (rng.range(dmin, dmax) / dmin).round().max(1.0) * dmin;
                    if rng.f() < density {
                        let pitch = snap_pitch(cx, rng.range(pmin, pmax + 0.999).floor() as i32);
                        out.push(note(t, d.min(b - t), pitch, rng.range(vmin, vmax) as f32));
                    }
                    t += d;
                }
            }
        }
        Generate::Shape => {
            let kind = p(v, 0) as usize;
            let (lo, hi) = (p(v, 1).min(p(v, 2)) as f64, p(v, 1).max(p(v, 2)) as f64);
            let rate = grid_beats(p(v, 3));
            let tie = p(v, 4) as f64 / 100.0;
            let density = p(v, 5) as f64 / 100.0;
            let jitter = p(v, 6) as f64 / 100.0;
            let mut rng = Rng::new(p(v, 7) as u64 + 1);
            let mut t = a;
            while t < b - 1e-9 {
                let u = (t - a) / (b - a);
                let y = shape(kind, u, &mut rng) + jitter * (rng.f() - 0.5);
                if rng.f() < density {
                    let pitch = snap_pitch(cx, (lo + (hi - lo) * y.clamp(0.0, 1.0)).round() as i32);
                    out.push(note(t, (rate * tie).min(b - t), pitch, 100.0));
                }
                t += rate;
            }
        }
        Generate::Stacks => {
            let root = p(v, 0) as i32 + 12 * (p(v, 1) as i32 + 2);
            let chord = CHORDS[(p(v, 2) as usize).min(CHORDS.len() - 1)].1;
            let inv = p(v, 3) as i32;
            let dur = p(v, 4) as f64;
            let off = p(v, 5) as f64;
            let slot = (p(v, 6) as usize).max(1) - 1;
            let mut pitches: Vec<i32> = chord.iter().map(|i| root + i).collect();
            for _ in 0..inv.max(0) {
                let x = pitches.remove(0);
                pitches.push(x + 12);
            }
            for _ in 0..(-inv).max(0) {
                let x = pitches.pop().unwrap_or(root);
                pitches.insert(0, x - 12);
            }
            let s = a + off + slot as f64 * dur;
            if s < b {
                for pch in pitches {
                    out.push(note(s, dur.min(b - s), snap_pitch(cx, pch), 100.0));
                }
            }
        }
        Generate::Euclidean => {
            let steps = (p(v, 0) as usize).clamp(1, 32);
            let div = grid_beats(p(v, 1));
            let random = p(v, 22) as f64 / 100.0;
            let mut rng = Rng::new(7);
            for voice in 0..4 {
                let o = 2 + voice * 5;
                if p(v, o) < 0.5 {
                    continue;
                }
                let pat = euclid(p(v, o + 2) as usize, steps, p(v, o + 3) as usize);
                let mut t = a;
                let mut i = 0;
                while t < b - 1e-9 {
                    if pat[i % steps] && rng.f() >= random * 0.5 {
                        out.push(note(t, div * 0.9, p(v, o + 1) as i32, p(v, o + 4)));
                    }
                    t += div;
                    i += 1;
                }
            }
        }
    }
    out.retain(|n| n.start >= a - 1e-9 && n.start < b);
    out.sort_by(|x, y| x.start.total_cmp(&y.start).then(x.pitch.cmp(&y.pitch)));
    out
}

// ---- Pitch and Time utilities (Clip View) --------------------------------------------------

/// Transpose by `n` semitones, or scale degrees when the clip scale is on.
pub fn transpose(notes: &mut [Note], n: i32, scale: Option<ClipScale>) {
    let cx = Ctx { range: (0.0, 0.0), scale };
    for x in notes {
        x.pitch = clamp_pitch(step_pitch(&cx, x.pitch as i32, n));
    }
}

pub fn fit_to_scale(notes: &mut [Note], scale: ClipScale) {
    for x in notes {
        x.pitch = clamp_pitch(scale.snap(x.pitch as i32));
    }
}

/// Mirror pitches between the lowest and highest note.
pub fn invert(notes: &mut [Note], scale: Option<ClipScale>) {
    let (lo, hi) = notes.iter().fold((127i32, 0i32), |(l, h), n| (l.min(n.pitch as i32), h.max(n.pitch as i32)));
    let cx = Ctx { range: (0.0, 0.0), scale };
    for x in notes {
        x.pitch = clamp_pitch(snap_pitch(&cx, lo + hi - x.pitch as i32));
    }
}

/// Add a copy of every note `interval` steps away.
pub fn add_interval(notes: &mut Vec<Note>, sel: &[usize], interval: i32, scale: Option<ClipScale>) -> Vec<usize> {
    let cx = Ctx { range: (0.0, 0.0), scale };
    let base = notes.len();
    let add: Vec<Note> = sel
        .iter()
        .filter_map(|&i| notes.get(i))
        .map(|n| Note { pitch: clamp_pitch(step_pitch(&cx, n.pitch as i32, interval)), ..*n })
        .collect();
    notes.extend(add);
    (base..notes.len()).collect()
}

/// Stretch positions and lengths around `origin` by `f`.
pub fn stretch(notes: &mut [Note], origin: f64, f: f64) {
    for x in notes {
        x.start = origin + (x.start - origin) * f;
        x.length *= f;
    }
}

/// Fit notes into `[a, a + len)` keeping proportions (Set Length).
pub fn set_length(notes: &mut [Note], len: f64) {
    let a = notes.iter().map(|n| n.start).fold(f64::INFINITY, f64::min);
    let b = notes.iter().map(|n| n.start + n.length).fold(f64::NEG_INFINITY, f64::max);
    if b > a && len > 0.0 {
        stretch(notes, a, len / (b - a));
    }
}

/// Random timing / velocity variation (deterministic per `seed`).
pub fn humanize(notes: &mut [Note], amount: f64, seed: u64) {
    let mut rng = Rng::new(seed);
    for x in notes {
        x.start = (x.start + (rng.f() - 0.5) * 0.05 * amount).max(0.0);
        let v = x.velocity as f64 * (1.0 + (rng.f() - 0.5) * 0.3 * amount);
        x.velocity = v.round().clamp(1.0, 127.0) as u8;
    }
}

/// Velocity bar Randomize: every velocity moves by a random amount within
/// ±`amount`/2 (1..127).
pub fn randomize_velocity(notes: &mut [Note], amount: u8, seed: u64) {
    let mut rng = Rng::new(seed);
    let a = amount as f64 * 0.5;
    for x in notes {
        x.velocity = (x.velocity as f64 + rng.range(-a, a)).round().clamp(1.0, 127.0) as u8;
    }
}

/// Velocity bar Ramp: velocities go linearly from `a` (earliest note) to `b`
/// (latest note start).
pub fn ramp_velocity(notes: &mut [Note], a: u8, b: u8) {
    let t0 = notes.iter().map(|n| n.start).fold(f64::INFINITY, f64::min);
    let t1 = notes.iter().map(|n| n.start).fold(f64::NEG_INFINITY, f64::max);
    for x in notes {
        let f = if t1 > t0 { (x.start - t0) / (t1 - t0) } else { 0.0 };
        x.velocity = (a as f64 + (b as f64 - a as f64) * f).round().clamp(1.0, 127.0) as u8;
    }
}

/// Reverse in time within the notes' span.
pub fn reverse(notes: &mut [Note]) {
    let a = notes.iter().map(|n| n.start).fold(f64::INFINITY, f64::min);
    let b = notes.iter().map(|n| n.start + n.length).fold(f64::NEG_INFINITY, f64::max);
    for x in notes {
        x.start = a + b - (x.start + x.length);
    }
}

/// Extend every note to the next (later) note start.
pub fn legato(notes: &mut [Note]) {
    let mut starts: Vec<f64> = notes.iter().map(|n| n.start).collect();
    starts.sort_by(f64::total_cmp);
    starts.dedup();
    for x in notes {
        if let Some(&n) = starts.iter().find(|&&s| s > x.start + 1e-9) {
            x.length = n - x.start;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(start: f64, pitch: u8) -> Note {
        Note { start, length: 1.0, pitch, velocity: 100, muted: false, mpe: 0, vel_dev: 0 }
    }
    const CX: Ctx = Ctx { range: (0.0, 4.0), scale: None };

    #[test]
    fn every_tool_runs_with_defaults() {
        let input = vec![n(0.0, 60), n(0.0, 64), n(0.0, 67), n(2.0, 62)];
        for t in Transform::ALL {
            let mut mpe = Vec::new();
            let out = transform(t, &defaults(t.params()), &input, &mut mpe, &CX);
            assert!(!out.is_empty(), "{t:?}");
            assert!(out.iter().all(|x| x.length > 0.0 && x.start.is_finite()), "{t:?}");
            assert_eq!(t.is_mpe(), !mpe.is_empty(), "{t:?}");
            assert!(out.iter().all(|x| x.mpe as usize <= mpe.len()));
        }
        for g in Generate::ALL {
            let out = generate(g, &defaults(g.params()), &CX);
            assert!(!out.is_empty(), "{g:?}");
            assert!(out.iter().all(|x| x.start >= 0.0 && x.start < 4.0), "{g:?}");
        }
    }

    #[test]
    fn arpeggiate_up_and_quantize() {
        let chord = vec![n(0.0, 64), n(0.0, 60), n(0.0, 67)];
        let mut v = defaults(Transform::Arpeggiate.params());
        v[3] = 3.0; // 1/4
        let out = transform(Transform::Arpeggiate, &v, &chord, &mut Vec::new(), &CX);
        assert_eq!(out.iter().map(|x| x.pitch).collect::<Vec<_>>(), vec![60]); // 1 beat chord / 1/4 rate
        let long: Vec<Note> = chord.iter().map(|x| Note { length: 3.0, ..*x }).collect();
        let out = transform(Transform::Arpeggiate, &v, &long, &mut Vec::new(), &CX);
        assert_eq!(out.iter().map(|x| (x.start, x.pitch)).collect::<Vec<_>>(), vec![(0.0, 60), (1.0, 64), (2.0, 67)]);
        let q = transform(Transform::Quantize, &[3.0, 1.0, 0.0, 100.0], &[n(0.9, 60), n(2.2, 60)], &mut Vec::new(), &CX);
        assert_eq!(q.iter().map(|x| x.start).collect::<Vec<_>>(), vec![1.0, 2.0]);
    }

    #[test]
    fn euclid_and_scale_transpose() {
        assert_eq!(euclid(3, 8, 0).iter().filter(|&&x| x).count(), 3);
        assert!(euclid(4, 16, 0)[0] && euclid(4, 16, 0)[4]);
        let s = ClipScale { on: true, root: 0, scale: 0 };
        let mut v = vec![n(0.0, 60), n(1.0, 64)];
        transpose(&mut v, 2, Some(s));
        assert_eq!(v.iter().map(|x| x.pitch).collect::<Vec<_>>(), vec![64, 67]);
        legato(&mut v);
        assert_eq!(v[0].length, 1.0);
        reverse(&mut v);
        assert_eq!(v[0].start, 1.0);
    }
}
