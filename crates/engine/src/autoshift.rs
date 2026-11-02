//! Auto Shift (Live 12 manual 29.4): real-time pitch correction and
//! shifting. YIN pitch detection on a decimated mono sum; pitch-synchronous
//! overlap-add (PSOLA) synthesis with formant scaling. The target pitch
//! comes from the Quantizer (scale, strength, smoothing, scale-degree
//! shift) or from MIDI notes (mono with glide, or poly voices), plus
//! Pitch / Fine, the LFO, MIDI mod routing and Vibrato.

use crate::devtap::{DevTap, dev_tap};
use crate::dynfx::Sidechain;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

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

/// Auto Shift's scale list: Custom, Chromatic, then [`SCALES`] in order.
pub fn as_scale_names() -> impl Iterator<Item = &'static str> {
    ["Custom", "Chromatic"].into_iter().chain(SCALES[..SCALES.len() - 1].iter().map(|s| s.0))
}
pub const AS_SCALE_CUSTOM: u8 = 0;
pub const AS_SCALE_CHROMATIC: u8 = 1;
pub const AS_SCALE_COUNT: u8 = SCALES.len() as u8 + 1;

/// Pitch-class mask (bit n = pitch class n, C = 0) of `root` + `steps`.
pub fn scale_mask(root: u8, steps: &[u8]) -> u16 {
    steps.iter().fold(0u16, |m, &s| m | 1 << ((root as u16 + s as u16) % 12))
}

// ---- Current scale (song level; follows the selected clip's scale) ----

static CURRENT: AtomicU32 = AtomicU32::new(0);

/// Set by the app when the current scale changes: (root, pitch-class mask).
pub fn set_current_scale(s: Option<(u8, u16)>) {
    let v = s.map_or(0, |(root, mask)| 1 << 31 | (root as u32 % 12) << 12 | (mask as u32 & 0xFFF));
    CURRENT.store(v, Ordering::Relaxed);
}

pub fn current_scale() -> Option<(u8, u16)> {
    let v = CURRENT.load(Ordering::Relaxed);
    (v >> 31 != 0).then_some((((v >> 12) & 0xF) as u8, (v & 0xFFF) as u16))
}

// ---- Parameters ----

pub const AS_RANGES: [&str; 3] = ["High", "Mid", "Bass"];
/// Latency per range (ms; Mid = Live's 34.8 ms). Live Mode: 0 reported.
pub const AS_LAT_MS: [f32; 3] = [17.4, 34.8, 69.6];
/// Physical latency in Live Mode (shorter grains, lower quality).
const LIVE_LAT_MS: f32 = 6.0;
/// Detection range (Hz) per input range.
const AS_FREQ: [(f32, f32); 3] = [(140.0, 1400.0), (65.0, 900.0), (32.0, 450.0)];
pub const AS_WAVES: [&str; 9] = ["Sine", "Tri", "Tri 8", "Tri 16", "Saw Up", "Saw Down", "Rect", "Random", "Random S&H"];
pub const AS_MOD_SRC: [&str; 6] = ["Velocity", "Pressure", "Mod Wheel", "Pitch Bend", "Note PB", "Slide"];
/// Modulation targets (LFO and MIDI mod routing).
pub const AS_TARGETS: [&str; 4] = ["Pitch", "Formant", "Volume", "Pan"];
pub const AS_VOICES: [u8; 3] = [2, 4, 8];
pub const AS_GAIN_DB: f32 = 24.0;
pub const AS_SMOOTH_MS: f32 = 200.0;
pub const AS_SHIFT: i8 = 24;
pub const AS_GLIDE_MS: f32 = 5000.0;
pub const AS_ATTACK_MS: f32 = 1000.0;
pub const AS_RELEASE_MS: f32 = 5000.0;
pub const AS_PB_MAX: u8 = 48;
pub const AS_LFO_DELAY_MS: f32 = 1500.0;
pub const AS_LFO_FADE_MS: f32 = 2000.0;
pub const AS_LFO_HZ: (f32, f32) = (0.01, 20.0);
/// Pitch modulation depth (± semitones) of the LFO / mod routing.
pub const AS_MOD_ST: f32 = 12.0;
pub const AS_PITCH_ST: f32 = 24.0;
pub const AS_FINE_CT: f32 = 50.0;
pub const AS_VIB_CT: f32 = 200.0;
pub const AS_VIB_HZ: (f32, f32) = (2.0, 15.0);
pub const AS_VIB_FADE_MS: f32 = 5000.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoShiftParams {
    /// Shown tab: 0 = Quantizer (MIDI when MIDI is on), 1 = LFO (view).
    pub tab: u8,
    /// MIDI In panel unfolded (view).
    pub midi_open: bool,
    pub live: bool,
    /// Input range (High / Mid / Bass).
    pub range: u8,
    /// dB.
    pub in_gain: f32,
    /// MIDI In: `on` = MIDI On, `source` / `tap` = External Source.
    pub midi: Sidechain,
    pub poly: bool,
    /// Index into [`AS_VOICES`].
    pub voices: u8,
    /// Mono glide (ms).
    pub glide: f32,
    /// Quantizer active (tab switch).
    pub quant: bool,
    pub strength: f32,
    pub smooth: bool,
    pub smooth_ms: f32,
    pub root: u8,
    /// 0 = Custom, 1 = Chromatic, 2 + i = `SCALES[i]`.
    pub scale: u8,
    /// Included notes of the Custom scale (pitch-class mask).
    pub custom: u16,
    /// Scale degrees, applied after the correction.
    pub shift: i8,
    pub use_current: bool,
    // MIDI tab.
    pub attack: f32,
    pub release: f32,
    pub latch: bool,
    pub pb_range: u8,
    /// Mod routing source (index into [`AS_MOD_SRC`]) and depth (−1..1) per target.
    pub mod_src: [u8; 4],
    pub mod_amt: [f32; 4],
    // LFO tab.
    pub lfo_on: bool,
    /// Restart at each onset (MIDI note when MIDI is on).
    pub lfo_reset: bool,
    pub lfo_delay: f32,
    pub lfo_fade: f32,
    pub lfo_wave: u8,
    pub lfo_rate: f32,
    pub lfo_sync: bool,
    /// Index into `modfx::SYNC_DIVS`.
    pub lfo_div: u8,
    /// Depth (−1..1) per target.
    pub lfo_amt: [f32; 4],
    // Pitch.
    pub pitch: f32,
    pub fine: f32,
    /// −1..1 (±1 octave).
    pub formant: f32,
    /// 0..1: share of the pitch shift the formants follow.
    pub f_follow: f32,
    // Vibrato.
    pub vib_on: bool,
    pub vib_amt: f32,
    pub vib_rate: f32,
    pub vib_fade: f32,
    pub natural: bool,
    pub dry_wet: f32,
}

impl Default for AutoShiftParams {
    fn default() -> Self {
        Self {
            tab: 0,
            midi_open: false,
            live: false,
            range: 1,
            in_gain: 0.0,
            midi: Sidechain::default(),
            poly: false,
            voices: 1,
            glide: 0.0,
            quant: true,
            strength: 1.0,
            smooth: true,
            smooth_ms: 50.0,
            root: 0,
            scale: AS_SCALE_CHROMATIC,
            custom: 0xFFF,
            shift: 0,
            use_current: false,
            attack: 1.0,
            release: 100.0,
            latch: false,
            pb_range: 2,
            mod_src: [4, 1, 0, 5],
            mod_amt: [0.0; 4],
            lfo_on: false,
            lfo_reset: true,
            lfo_delay: 0.0,
            lfo_fade: 0.0,
            lfo_wave: 1,
            lfo_rate: 0.5,
            lfo_sync: false,
            lfo_div: 4,
            lfo_amt: [0.0; 4],
            pitch: 0.0,
            fine: 0.0,
            formant: 0.0,
            f_follow: 0.0,
            vib_on: false,
            vib_amt: 0.0,
            vib_rate: 6.0,
            vib_fade: 0.0,
            natural: false,
            dry_wet: 1.0,
        }
    }
}

impl AutoShiftParams {
    /// Included notes (pitch-class mask) and root, following the current
    /// scale when Use Current Scale is on.
    pub fn included(&self) -> (u8, u16) {
        if self.use_current
            && let Some(c) = current_scale()
        {
            return c;
        }
        let mask = match self.scale {
            AS_SCALE_CUSTOM => self.custom & 0xFFF,
            AS_SCALE_CHROMATIC => 0xFFF,
            s => scale_mask(self.root, SCALES.get(s as usize - 2).map_or(SCALES[0].1, |x| x.1)),
        };
        (self.root % 12, mask)
    }
    pub fn scale_name(&self) -> &'static str {
        as_scale_names().nth(self.scale as usize).unwrap_or("Custom")
    }
    pub fn midi_mode(&self) -> bool {
        self.midi.on && self.midi.source != 0
    }
    /// Reported latency (samples): none in Live Mode.
    pub fn latency(&self, sr: f32) -> usize {
        if self.live { 0 } else { (AS_LAT_MS[(self.range as usize).min(2)] * 0.001 * sr).round() as usize }
    }
}

/// Nearest included pitch to `m` (fractional MIDI note); None if no notes.
pub fn nearest_in(mask: u16, m: f32) -> Option<f32> {
    if mask & 0xFFF == 0 {
        return None;
    }
    let c = m.round() as i32;
    (0..=12)
        .flat_map(|d| [c - d, c + d])
        .filter(|p| mask >> p.rem_euclid(12) & 1 != 0)
        .min_by(|a, b| (*a as f32 - m).abs().total_cmp(&(*b as f32 - m).abs()))
        .map(|p| p as f32)
}

/// Move the included pitch `p` by `deg` included notes.
pub fn shift_degrees(mask: u16, p: f32, deg: i32) -> f32 {
    if mask & 0xFFF == 0 || deg == 0 {
        return p;
    }
    let mut q = p.round() as i32;
    let dir = deg.signum();
    for _ in 0..deg.unsigned_abs() {
        q += dir;
        while mask >> q.rem_euclid(12) & 1 == 0 {
            q += dir;
        }
    }
    q as f32
}

fn lfo_wave(w: u8, ph: f64, rnd: (f32, f32)) -> f32 {
    let ph = ph.rem_euclid(1.0) as f32;
    let tri = |p: f32| 1.0 - 4.0 * p.min(1.0 - p);
    match w {
        0 => (std::f32::consts::TAU * ph).sin(),
        1 => tri(ph),
        2 => tri((ph * 8.0).floor() / 8.0),
        3 => tri((ph * 16.0).floor() / 16.0),
        4 => 2.0 * ph - 1.0,
        5 => 1.0 - 2.0 * ph,
        6 => {
            if ph < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        7 => rnd.0 + (rnd.1 - rnd.0) * (0.5 - 0.5 * (std::f32::consts::PI * ph).cos()),
        _ => rnd.1,
    }
}

/// LFO wave value for the display (`ph` in cycles; random shapes seeded).
pub fn as_lfo_shape(w: u8, ph: f64) -> f32 {
    let r = |i: i64| ((i.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as u32 as f32 / u32::MAX as f32) * 2.0 - 1.0;
    let i = ph.floor() as i64;
    lfo_wave(w, ph, (r(i), r(i + 1)))
}

// ---- DSP ----

const RING: usize = 1 << 15;
const RM: usize = RING - 1;
const DRING: usize = 1 << 12;
const DM: usize = DRING - 1;
const MAXV: usize = 8;
const CTRL: u32 = 32;
const MAX_NOTES: usize = 64;
const MAX_HELD: usize = 16;

/// Device tap values for the UI.
pub mod tapv {
    /// Detected input pitch (fractional MIDI note, −1 = none).
    pub const INPUT: usize = 0;
    /// Target note of the Quantizer (−1 = none).
    pub const TARGET: usize = 1;
    /// Applied correction (cents).
    pub const CENTS: usize = 2;
    /// Onset seen (raised; the UI takes it).
    pub const ONSET: usize = 3;
    /// LFO phase (cycles) and envelope for the display.
    pub const LFO_PH: usize = 4;
    pub const LFO_ENV: usize = 5;
}

#[derive(Debug, Clone, Copy, Default)]
struct Voice {
    /// Key held (or latched).
    on: bool,
    note: f32,
    target: f32,
    vel: f32,
    env: f32,
    /// Output time of the next grain; analysis mark (input time).
    next: f64,
    mark: f64,
    ratio: f32,
    fmt: f32,
    gl: f32,
    gr: f32,
    age: u64,
}

/// A note for the MIDI input: (offset, pitch, velocity, length; 0 velocity = off).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct AsNote {
    pub offset: u32,
    pub pitch: u8,
    pub vel: u8,
    /// Samples until the note-off (`u32::MAX` = until an explicit off).
    pub len: u32,
}

#[derive(Debug, Clone)]
pub struct AutoShiftState {
    tap: Option<Arc<DevTap>>,
    in_l: Box<[f32]>,
    in_r: Box<[f32]>,
    out_l: Box<[f32]>,
    out_r: Box<[f32]>,
    t: u64,
    // Detection.
    dec: Box<[f32]>,
    yin: Box<[f32]>,
    xs: Box<[f32]>,
    dt: usize,
    dphase: u32,
    decim: u32,
    lp: [f32; 2],
    lp_a: f32,
    hop: u32,
    hop_n: u32,
    tmin: usize,
    tmax: usize,
    /// Input period (samples), 0 = unvoiced.
    period: f32,
    det: f32,
    target: f32,
    corr: f32,
    // Voices / MIDI.
    voices: [Voice; MAXV],
    notes: [AsNote; MAX_NOTES],
    n_notes: usize,
    /// Keys down: (pitch, samples left).
    held: [(u8, u32); MAX_HELD],
    n_held: usize,
    // LFO / vibrato / onsets.
    lfo_ph: f64,
    lfo_t: f32,
    rnd: (f32, f32),
    seed: u32,
    vib_ph: f64,
    vib_t: f32,
    nat: (f32, f32),
    env_f: f32,
    env_s: f32,
    cool: u32,
    voiced_was: bool,
    // Config.
    sr: f32,
    lat: usize,
    range: u8,
    live: bool,
    midi_was: bool,
}

impl Default for AutoShiftState {
    fn default() -> Self {
        Self::new(None)
    }
}

impl AutoShiftState {
    pub fn new(id: Option<u64>) -> Self {
        Self {
            tap: id.map(dev_tap),
            in_l: vec![0.0; RING].into_boxed_slice(),
            in_r: vec![0.0; RING].into_boxed_slice(),
            out_l: vec![0.0; RING].into_boxed_slice(),
            out_r: vec![0.0; RING].into_boxed_slice(),
            t: 0,
            dec: vec![0.0; DRING].into_boxed_slice(),
            yin: vec![0.0; DRING].into_boxed_slice(),
            xs: vec![0.0; DRING].into_boxed_slice(),
            dt: 0,
            dphase: 0,
            decim: 1,
            lp: [0.0; 2],
            lp_a: 1.0,
            hop: 64,
            hop_n: 0,
            tmin: 2,
            tmax: 64,
            period: 0.0,
            det: -1.0,
            target: -1.0,
            corr: 0.0,
            voices: [Voice::default(); MAXV],
            notes: [AsNote::default(); MAX_NOTES],
            n_notes: 0,
            held: [(0, 0); MAX_HELD],
            n_held: 0,
            lfo_ph: 0.0,
            lfo_t: 0.0,
            rnd: (0.0, 0.0),
            seed: 0x1234_5678,
            vib_ph: 0.0,
            vib_t: 0.0,
            nat: (1.0, 1.0),
            env_f: 0.0,
            env_s: 0.0,
            cool: 0,
            voiced_was: false,
            sr: 0.0,
            lat: 0,
            range: u8::MAX,
            live: false,
            midi_was: false,
        }
    }

    pub fn reset(&mut self) {
        self.in_l.fill(0.0);
        self.in_r.fill(0.0);
        self.out_l.fill(0.0);
        self.out_r.fill(0.0);
        self.dec.fill(0.0);
        self.period = 0.0;
        self.det = -1.0;
        self.target = -1.0;
        self.corr = 0.0;
        self.voices = [Voice::default(); MAXV];
        self.n_held = 0;
        self.n_notes = 0;
        self.lfo_t = 0.0;
        self.vib_t = 0.0;
        self.range = u8::MAX;
    }

    fn configure(&mut self, p: &AutoShiftParams, sr: f32) {
        let range = (p.range as usize).min(2);
        self.sr = sr;
        self.range = p.range;
        self.live = p.live;
        let up = (sr / 48_000.0).round().max(1.0) as u32;
        self.decim = [1, 2, 4][range] * up;
        let dsr = sr / self.decim as f32;
        let (f0, f1) = AS_FREQ[range];
        self.tmin = ((dsr / f1).floor() as usize).max(2);
        self.tmax = ((dsr / f0).ceil() as usize).min(DRING / 2 - 2);
        // Hop ≈ 2.7 ms.
        self.hop = ((dsr * 0.0027).round() as u32).max(8);
        self.lp_a = 1.0 - (-std::f32::consts::TAU * (dsr * 0.3) / sr).exp();
        let ms = if p.live { LIVE_LAT_MS } else { AS_LAT_MS[range] };
        self.lat = ((ms * 0.001 * sr).round() as usize).clamp(32, RING / 2);
        self.out_l.fill(0.0);
        self.out_r.fill(0.0);
        for v in &mut self.voices {
            v.next = self.t as f64;
            v.mark = 0.0;
        }
    }

    fn rand(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as f32 / u32::MAX as f32 * 2.0 - 1.0
    }

    /// MIDI input of this block (sorted by offset), from the source track.
    pub(crate) fn set_notes(&mut self, it: &mut dyn Iterator<Item = AsNote>) {
        self.n_notes = 0;
        for e in it {
            if self.n_notes < MAX_NOTES {
                self.notes[self.n_notes] = e;
                self.n_notes += 1;
            }
        }
        self.notes[..self.n_notes].sort_unstable_by_key(|e| e.offset);
    }

    fn restart_mod(&mut self, p: &AutoShiftParams) {
        if p.lfo_reset {
            self.lfo_t = 0.0;
            self.lfo_ph = 0.0;
        }
        self.vib_t = 0.0;
    }

    fn note_on(&mut self, p: &AutoShiftParams, pitch: u8, vel: u8, len: u32) {
        let any_held = self.n_held > 0;
        if let Some(i) = self.held[..self.n_held].iter().position(|h| h.0 == pitch) {
            self.held[i].1 = len;
        } else if self.n_held < MAX_HELD {
            self.held[self.n_held] = (pitch, len);
            self.n_held += 1;
        }
        let t = self.t;
        let vel = vel as f32 / 127.0;
        if !p.poly {
            let v = &mut self.voices[0];
            v.target = pitch as f32;
            if !(v.on || v.env > 0.0) || p.glide <= 0.0 {
                v.note = pitch as f32;
            }
            if !v.on {
                v.next = t as f64;
            }
            v.on = true;
            v.vel = vel;
        } else {
            let n = AS_VOICES[(p.voices as usize).min(2)] as usize;
            // Latch: a new chord (no key down) releases the latched notes.
            if p.latch && !any_held {
                for v in &mut self.voices[..n] {
                    v.on = false;
                }
            }
            let i = (0..n)
                .find(|&i| self.voices[i].on && self.voices[i].target == pitch as f32)
                .or_else(|| (0..n).find(|&i| !self.voices[i].on && self.voices[i].env <= 0.0))
                .or_else(|| (0..n).filter(|&i| !self.voices[i].on).min_by_key(|&i| self.voices[i].age))
                .unwrap_or_else(|| (0..n).min_by_key(|&i| self.voices[i].age).unwrap_or(0));
            let v = &mut self.voices[i];
            if !(v.on || v.env > 0.0) {
                v.next = t as f64;
            }
            *v = Voice { on: true, note: pitch as f32, target: pitch as f32, vel, age: t, ..*v };
        }
        self.restart_mod(p);
    }

    fn note_off(&mut self, p: &AutoShiftParams, pitch: u8) {
        if let Some(i) = self.held[..self.n_held].iter().position(|h| h.0 == pitch) {
            self.held.copy_within(i + 1..self.n_held, i);
            self.n_held -= 1;
        }
        if p.latch {
            return;
        }
        if !p.poly {
            let v = &mut self.voices[0];
            match self.held[..self.n_held].last() {
                // Last-note priority: glide back to the newest held key.
                Some(&(q, _)) => v.target = q as f32,
                None => v.on = false,
            }
        } else {
            for v in &mut self.voices {
                if v.on && v.target == pitch as f32 {
                    v.on = false;
                }
            }
        }
    }

    /// YIN on the newest decimated samples.
    fn detect(&mut self) {
        let (tmin, tmax) = (self.tmin, self.tmax);
        let w = tmax;
        let len = w + tmax + 1;
        let base = self.dt.wrapping_sub(len);
        for j in 0..len {
            self.xs[j] = self.dec[base.wrapping_add(j) & DM];
        }
        let x = &self.xs[..len];
        let energy = x[..w].iter().map(|v| v * v).sum::<f32>() / w as f32;
        if energy < 1e-7 {
            self.period = 0.0;
            return;
        }
        let d = &mut self.yin;
        d[0] = 1.0;
        let mut run = 0.0f32;
        for tau in 1..=tmax {
            let mut s = 0.0f32;
            for j in 0..w {
                let e = x[j] - x[j + tau];
                s += e * e;
            }
            run += s;
            d[tau] = if run > 0.0 { s * tau as f32 / run } else { 1.0 };
        }
        let mut best = None;
        let mut tau = tmin;
        while tau < tmax {
            if d[tau] < 0.15 {
                while tau + 1 < tmax && d[tau + 1] < d[tau] {
                    tau += 1;
                }
                best = Some(tau);
                break;
            }
            tau += 1;
        }
        if best.is_none() {
            let (i, v) = (tmin..tmax).map(|i| (i, d[i])).min_by(|a, b| a.1.total_cmp(&b.1)).unwrap_or((0, 1.0));
            if v < 0.3 {
                best = Some(i);
            }
        }
        self.period = match best {
            Some(t) if t > 0 && t < tmax => {
                let (a, b, c) = (d[t - 1], d[t], d[t + 1]);
                let den = a - 2.0 * b + c;
                let off = if den.abs() > 1e-9 { (0.5 * (a - c) / den).clamp(-0.5, 0.5) } else { 0.0 };
                (t as f32 + off) * self.decim as f32
            }
            _ => 0.0,
        };
    }

    /// Per-control-step values: correction, LFO, vibrato, voice ratios.
    fn control(&mut self, p: &AutoShiftParams, midi: bool) {
        let dt = CTRL as f32 / self.sr;
        let sr = self.sr;
        self.det = if self.period > 0.0 { 69.0 + 12.0 * (sr / self.period / 440.0).log2() } else { -1.0 };
        // Quantizer.
        let mut corr = 0.0;
        self.target = -1.0;
        if !midi && p.quant && self.det >= 0.0 {
            let (_, mask) = p.included();
            if let Some(mut tg) = nearest_in(mask, self.det) {
                // Hysteresis: keep the previous target near the boundary.
                let prev = self.voices[0].target;
                if prev >= 0.0 && mask >> (prev.round() as i32).rem_euclid(12) & 1 != 0 && (self.det - prev).abs() < 0.6 {
                    tg = prev;
                }
                self.voices[0].target = tg;
                self.target = shift_degrees(mask, tg, p.shift as i32);
                corr = (self.target - self.det) * p.strength;
            }
        }
        let k = if p.smooth && p.smooth_ms > 0.0 { 1.0 - (-dt / (p.smooth_ms * 0.001)).exp() } else { 1.0 };
        self.corr += (corr - self.corr) * k;
        // LFO.
        let hz = if p.lfo_sync {
            let (tempo, _) = crate::modfx::clock();
            let b = crate::modfx::SYNC_DIVS[(p.lfo_div as usize).min(crate::modfx::SYNC_DIVS.len() - 1)].0;
            (tempo / 60.0 / b) as f32
        } else {
            p.lfo_rate
        };
        let ph0 = self.lfo_ph;
        self.lfo_ph += (hz * dt) as f64;
        if self.lfo_ph.floor() != ph0.floor() {
            self.rnd = (self.rnd.1, self.rand());
        }
        self.lfo_t += dt;
        let lenv = if self.lfo_t < p.lfo_delay * 0.001 {
            0.0
        } else if p.lfo_fade > 0.0 {
            ((self.lfo_t - p.lfo_delay * 0.001) / (p.lfo_fade * 0.001)).min(1.0)
        } else {
            1.0
        };
        let lfo = if p.lfo_on { lfo_wave(p.lfo_wave, self.lfo_ph, self.rnd) * lenv } else { 0.0 };
        // Vibrato (Natural: slowly wandering rate and depth).
        self.vib_t += dt;
        if p.natural && self.vib_ph.floor() != (self.vib_ph + (p.vib_rate * dt) as f64).floor() {
            self.nat = (1.0 + 0.12 * self.rand(), 1.0 + 0.25 * self.rand());
        }
        let nat = if p.natural { self.nat } else { (1.0, 1.0) };
        self.vib_ph += (p.vib_rate * nat.0 * dt) as f64;
        let vfade = if p.vib_fade > 0.0 { (self.vib_t / (p.vib_fade * 0.001)).min(1.0) } else { 1.0 };
        let vib = if p.vib_on { p.vib_amt * nat.1 * vfade * (std::f32::consts::TAU * self.vib_ph as f32).sin() / 100.0 } else { 0.0 };
        if let Some(tap) = &self.tap {
            tap.set(tapv::INPUT, self.det);
            tap.set(tapv::TARGET, self.target);
            tap.set(tapv::CENTS, self.corr * 100.0);
            tap.set(tapv::LFO_PH, self.lfo_ph.rem_euclid(1.0) as f32);
            tap.set(tapv::LFO_ENV, if p.lfo_on { lenv } else { 0.0 });
        }
        let manual = p.pitch + p.fine / 100.0 + vib + lfo * p.lfo_amt[0] * AS_MOD_ST;
        let gk = if p.glide > 0.0 { 1.0 - (-dt / (p.glide * 0.001)).exp() } else { 1.0 };
        let att = dt / (p.attack.max(0.5) * 0.001);
        let rel = dt / (p.release.max(1.0) * 0.001);
        let n = if !midi {
            1
        } else if p.poly {
            AS_VOICES[(p.voices as usize).min(2)] as usize
        } else {
            1
        };
        for (i, v) in self.voices.iter_mut().enumerate() {
            if i >= n {
                v.env = 0.0;
                v.on = false;
                continue;
            }
            let modv = |k: usize| if midi && p.mod_src[k] == 0 { v.vel * p.mod_amt[k] } else { 0.0 };
            let shift = if midi {
                v.note += (v.target - v.note) * gk;
                v.env = if v.on { (v.env + att).min(1.0) } else { (v.env - rel).max(0.0) };
                if self.det >= 0.0 { v.note - self.det } else { 0.0 }
            } else {
                v.env = 1.0;
                self.corr
            } + manual
                + modv(0) * AS_MOD_ST;
            v.ratio = 2f32.powf(shift / 12.0).clamp(0.25, 4.0);
            let f = p.formant + lfo * p.lfo_amt[1] + modv(1);
            v.fmt = (2f32.powf(f) * v.ratio.powf(p.f_follow)).clamp(0.5, 2.0);
            let vol = (1.0 - p.lfo_amt[2].abs() * (0.5 - 0.5 * lfo * p.lfo_amt[2].signum())) * (1.0 + modv(2)).clamp(0.0, 2.0);
            let pan = (lfo * p.lfo_amt[3] + modv(3)).clamp(-1.0, 1.0);
            let a = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
            let g = v.env * vol;
            v.gl = g * a.cos() * std::f32::consts::SQRT_2;
            v.gr = g * a.sin() * std::f32::consts::SQRT_2;
        }
    }

    /// One PSOLA grain of voice `vi` starting at output time `t`.
    fn grain(&mut self, vi: usize) {
        let t = self.t;
        let v = self.voices[vi];
        let voiced = self.period > 0.0;
        let tp = if voiced { self.period } else { self.sr * 0.01 };
        let ratio = if voiced { v.ratio } else { 1.0 };
        let lat = self.lat as f32;
        let h = tp.max(tp / ratio).min((lat - 2.0) / (1.0 + v.fmt)).max(8.0);
        let target = t as f64 + h as f64 - lat as f64;
        let mut mark = v.mark;
        if voiced {
            if (mark - target).abs() > 2.0 * tp as f64 {
                mark = target;
            }
            while mark + 0.5 * tp as f64 <= target {
                mark += tp as f64;
            }
        } else {
            mark = target;
        }
        // Never read input that has not arrived yet.
        let a = mark.min(t as f64 - 1.0 - (h * v.fmt) as f64);
        let spacing = (tp / ratio).max(1.0);
        let amp = 1.0 / (h / spacing).max(1.0);
        let n = (2.0 * h) as usize;
        if v.gl.abs() + v.gr.abs() > 1e-6 {
            let inv = 1.0 / n as f32;
            for j in 0..n {
                let w = 0.5 - 0.5 * (std::f32::consts::TAU * j as f32 * inv).cos();
                let pos = a + ((j as f32 - h) * v.fmt) as f64;
                let i0 = pos.floor();
                let fr = (pos - i0) as f32;
                let i0 = (i0 as i64 as usize) & RM;
                let i1 = (i0 + 1) & RM;
                let sl = self.in_l[i0] + (self.in_l[i1] - self.in_l[i0]) * fr;
                let sr = self.in_r[i0] + (self.in_r[i1] - self.in_r[i0]) * fr;
                let o = (t as usize + j) & RM;
                let g = w * amp;
                self.out_l[o] += sl * g * v.gl;
                self.out_r[o] += sr * g * v.gr;
            }
        }
        let v = &mut self.voices[vi];
        v.mark = mark;
        v.next += spacing as f64;
        if v.next < t as f64 {
            v.next = t as f64 + 1.0;
        }
    }

    pub fn process(&mut self, p: &AutoShiftParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        if sr != self.sr || p.range != self.range || p.live != self.live {
            self.configure(p, sr);
        }
        let midi = p.midi_mode();
        if midi != self.midi_was {
            self.midi_was = midi;
            self.voices = [Voice { next: self.t as f64, ..Voice::default() }; MAXV];
            self.n_held = 0;
        }
        let g_in = 10f32.powf(p.in_gain / 20.0);
        let w = p.dry_wet.clamp(0.0, 1.0);
        let n = l.len().min(r.len());
        let fa = 1.0 - (-1.0 / (0.005 * sr)).exp();
        let sa = 1.0 - (-1.0 / (0.1 * sr)).exp();
        let mut ei = 0;
        for k in 0..n {
            while ei < self.n_notes && self.notes[ei].offset as usize <= k {
                let e = self.notes[ei];
                ei += 1;
                if !midi {
                    continue;
                }
                if e.vel > 0 {
                    self.note_on(p, e.pitch, e.vel, e.len);
                } else {
                    self.note_off(p, e.pitch);
                }
            }
            let xl = l[k] * g_in;
            let xr = r[k] * g_in;
            let i = self.t as usize & RM;
            self.in_l[i] = xl;
            self.in_r[i] = xr;
            // Decimate (2-pole low-pass) into the detection ring.
            let m = 0.5 * (xl + xr);
            self.lp[0] += (m - self.lp[0]) * self.lp_a;
            self.lp[1] += (self.lp[0] - self.lp[1]) * self.lp_a;
            self.dphase += 1;
            if self.dphase >= self.decim {
                self.dphase = 0;
                self.dec[self.dt & DM] = self.lp[1];
                self.dt = self.dt.wrapping_add(1);
                self.hop_n += 1;
                if self.hop_n >= self.hop {
                    self.hop_n = 0;
                    self.detect();
                }
            }
            // Onsets (level jump or a new voiced segment).
            let a = m.abs();
            self.env_f += (a - self.env_f) * fa;
            self.env_s += (a - self.env_s) * sa;
            self.cool = self.cool.saturating_sub(1);
            let voiced = self.period > 0.0;
            if self.cool == 0 && ((self.env_f > 2.0 * self.env_s && self.env_f > 0.01) || (voiced && !self.voiced_was)) {
                self.cool = (0.1 * sr) as u32;
                if let Some(tap) = &self.tap {
                    tap.raise(tapv::ONSET, 1.0);
                }
                if !midi {
                    self.restart_mod(p);
                }
            }
            self.voiced_was = voiced;
            if self.t.is_multiple_of(CTRL as u64) {
                self.control(p, midi);
            }
            for vi in 0..MAXV {
                let v = &self.voices[vi];
                if (v.env > 0.0 || v.on || !midi && vi == 0) && self.t as f64 >= v.next {
                    self.grain(vi);
                } else if v.env <= 0.0 && !v.on {
                    self.voices[vi].next = self.t as f64;
                }
            }
            let wl = std::mem::take(&mut self.out_l[i]);
            let wr = std::mem::take(&mut self.out_r[i]);
            let d = (self.t as usize).wrapping_sub(self.lat) & RM;
            l[k] = self.in_l[d] * (1.0 - w) + wl * w;
            r[k] = self.in_r[d] * (1.0 - w) + wr * w;
            self.t += 1;
        }
        // Clip notes end after their length.
        let mut k = 0;
        while k < self.n_held {
            let (pitch, left) = self.held[k];
            if left != u32::MAX && left <= n as u32 {
                self.note_off(p, pitch);
            } else {
                if left != u32::MAX {
                    self.held[k].1 = left - n as u32;
                }
                k += 1;
            }
        }
        self.n_notes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// Harmonic-rich test tone (like a voice) at `hz`.
    fn tone(hz: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let ph = std::f32::consts::TAU * hz * i as f32 / SR;
                0.3 * (ph.sin() + 0.5 * (2.0 * ph).sin() + 0.25 * (3.0 * ph).sin())
            })
            .collect()
    }

    /// Run `x` through the device; returns the output.
    fn run(p: &AutoShiftParams, st: &mut AutoShiftState, x: &[f32]) -> Vec<f32> {
        let mut out = Vec::with_capacity(x.len());
        for c in x.chunks(256) {
            let mut l = c.to_vec();
            let mut r = c.to_vec();
            st.process(p, &mut l, &mut r, SR);
            out.extend_from_slice(&l);
        }
        out
    }

    /// Fundamental of `y` via the device's own detector (fresh state).
    fn pitch_of(y: &[f32]) -> f32 {
        let mut st = AutoShiftState::new(None);
        let p = AutoShiftParams { quant: false, ..Default::default() };
        run(&p, &mut st, y);
        st.det
    }

    #[test]
    fn detects_low_saw() {
        for (hz, range, want) in [(65.41, 1u8, 36.0), (65.41, 2, 36.0), (98.0, 1, 43.0), (196.0, 0, 55.0)] {
            let x: Vec<f32> = (0..24_000).map(|i| 0.4 * (2.0 * ((i as f32 * hz / SR).fract()) - 1.0)).collect();
            let mut st = AutoShiftState::new(None);
            let p = AutoShiftParams { quant: false, range, ..Default::default() };
            run(&p, &mut st, &x);
            assert!((st.det - want).abs() < 0.1, "{hz} Hz range {range}: {}", st.det);
        }
    }

    #[test]
    fn sub_octave_needs_bass_range() {
        let mut lp = 0.0f32;
        let x: Vec<f32> = (0..48_000)
            .map(|i| {
                let t = i as f32 / SR;
                let saw = 2.0 * (t * 65.41).fract() - 1.0;
                let sq = if (t * 32.70).fract() < 0.5 { 1.0 } else { -1.0 };
                lp += 0.05 * (0.5 * saw + 0.5 * sq - lp);
                0.4 * lp
            })
            .collect();
        // Saw at C1 + square an octave down (32.7 Hz): below the Mid range,
        // found in the Bass range.
        for (range, want) in [(1u8, -1.0), (2, 24.0)] {
            let mut st = AutoShiftState::new(None);
            let p = AutoShiftParams { quant: false, range, ..Default::default() };
            run(&p, &mut st, &x);
            assert!((st.det - want).abs() < 0.1, "range {range}: {}", st.det);
        }
    }

    #[test]
    fn detects_at_44k_large_blocks() {
        let sr = 44_100.0;
        let x: Vec<f32> = (0..44_100).map(|i| 0.4 * (2.0 * ((i as f32 * 65.41 / sr).fract()) - 1.0)).collect();
        let mut st = AutoShiftState::new(None);
        let p = AutoShiftParams { quant: false, ..Default::default() };
        for c in x.chunks(1024) {
            let (mut l, mut r) = (c.to_vec(), c.to_vec());
            st.process(&p, &mut l, &mut r, sr);
        }
        assert!((st.det - 36.0).abs() < 0.1, "{}", st.det);
    }

    #[test]
    fn detects_pitch() {
        let mut st = AutoShiftState::new(None);
        let p = AutoShiftParams { quant: false, ..Default::default() };
        run(&p, &mut st, &tone(220.0, 24_000));
        assert!((st.det - 57.0).abs() < 0.05, "A3: {}", st.det);
        run(&p, &mut st, &tone(330.0, 24_000));
        assert!((st.det - 64.02).abs() < 0.08, "E4: {}", st.det);
    }

    #[test]
    fn quantizer_corrects_to_nearest_note() {
        // 30 cents sharp of A3 → corrected to A3 (Chromatic, 100 %, no smoothing).
        let hz = 220.0 * 2f32.powf(0.3 / 12.0);
        let p = AutoShiftParams { smooth: false, ..Default::default() };
        let mut st = AutoShiftState::new(None);
        let y = run(&p, &mut st, &tone(hz, 48_000));
        let got = pitch_of(&y[24_000..]);
        assert!((got - 57.0).abs() < 0.08, "corrected: {got}");
        assert!((st.target - 57.0).abs() < 1e-6);
        // Major scale on C: G#3 (56.3) goes to A3 (57) not G#.
        let p = AutoShiftParams { scale: 2, smooth: false, ..Default::default() };
        assert_eq!(p.included().1, scale_mask(0, &[0, 2, 4, 5, 7, 9, 11]));
        let mut st = AutoShiftState::new(None);
        let y = run(&p, &mut st, &tone(440.0 * 2f32.powf((56.4 - 69.0) / 12.0), 48_000));
        let got = pitch_of(&y[24_000..]);
        assert!((got - 57.0).abs() < 0.1, "major: {got}");
        // Shift +1 degree: A3 → B3.
        assert_eq!(shift_degrees(p.included().1, 57.0, 1), 59.0);
        assert_eq!(shift_degrees(p.included().1, 57.0, -2), 53.0);
    }

    #[test]
    fn pitch_knob_shifts_and_latency_aligns() {
        let p = AutoShiftParams { quant: false, pitch: 7.0, ..Default::default() };
        let mut st = AutoShiftState::new(None);
        let y = run(&p, &mut st, &tone(220.0, 48_000));
        let got = pitch_of(&y[24_000..]);
        assert!((got - 64.0).abs() < 0.1, "+7 st: {got}");
        assert_eq!(p.latency(SR), 1670);
        assert_eq!(AutoShiftParams { live: true, ..p }.latency(SR), 0);
        // Dry only: the input delayed by the latency.
        let p = AutoShiftParams { dry_wet: 0.0, ..Default::default() };
        let mut st = AutoShiftState::new(None);
        let mut x = vec![0.0; 4096];
        x[100] = 1.0;
        let y = run(&p, &mut st, &x);
        assert_eq!(y.iter().position(|v| *v == 1.0), Some(100 + 1670));
    }

    #[test]
    fn midi_mono_plays_notes_only_while_held() {
        let mut p = AutoShiftParams { quant: false, ..Default::default() };
        p.midi.on = true;
        p.midi.source = 7;
        let mut st = AutoShiftState::new(None);
        let x = tone(220.0, 256);
        let mut out = Vec::new();
        for i in 0..200 {
            if i == 10 {
                st.set_notes(&mut [AsNote { offset: 0, pitch: 64, vel: 100, len: 150 * 256 }].into_iter());
            }
            let mut l = x.clone();
            let mut r = x.clone();
            // Keep the phase continuous.
            let off = i * 256;
            for (k, v) in l.iter_mut().chain(r.iter_mut()).enumerate() {
                let j = off + k % 256;
                let ph = std::f32::consts::TAU * 220.0 * j as f32 / SR;
                *v = 0.3 * (ph.sin() + 0.5 * (2.0 * ph).sin() + 0.25 * (3.0 * ph).sin());
            }
            st.process(&p, &mut l, &mut r, SR);
            out.extend_from_slice(&l);
        }
        let silent = out[..2000].iter().all(|v| v.abs() < 1e-6);
        assert!(silent, "no note → no output");
        let got = pitch_of(&out[100 * 256..150 * 256]);
        assert!((got - 64.0).abs() < 0.1, "E4: {got}");
        let tail = &out[190 * 256..];
        assert!(tail.iter().all(|v| v.abs() < 1e-3), "released");
    }

    #[test]
    fn scale_list_and_current_scale() {
        let names: Vec<_> = as_scale_names().collect();
        assert_eq!(names.len(), AS_SCALE_COUNT as usize);
        assert_eq!(&names[..4], &["Custom", "Chromatic", "Major", "Minor"]);
        assert_eq!(*names.last().unwrap(), "Messiaen 7");
        assert_eq!(nearest_in(0b1, 62.6), Some(60.0));
        assert_eq!(nearest_in(0, 60.0), None);
        let p = AutoShiftParams { use_current: true, scale: 0, custom: 1, ..Default::default() };
        set_current_scale(Some((2, scale_mask(2, &[0, 2, 4, 5, 7, 9, 11]))));
        assert_eq!(p.included(), (2, scale_mask(2, &[0, 2, 4, 5, 7, 9, 11])));
        set_current_scale(None);
        assert_eq!(p.included(), (0, 1));
    }
}
