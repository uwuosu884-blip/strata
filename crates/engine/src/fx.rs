//! Audio effects (Live's Utility, Align Delay and EQ Eight).
//!
//! Parameters are plain `Copy` data in user units (they are also the project
//! file format). Each effect slot owns a boxed DSP state that the control
//! thread allocates; when a new song arrives, the audio thread moves the state
//! of slots with the same id over, so edits never reset delay lines/filters.

use crate::dynfx::{CompressorState, GateState, GlueState, LimiterState, ScState};
pub use crate::eq8::EqEightParams;
use crate::eq8::EqState;
use crate::lfo::{LfoMap, LfoState};
use crate::modfx::{AutoFilterState, AutoPanState, ChorusState, EqThreeState};
use serde::{Deserialize, Serialize};
use std::f32::consts::{FRAC_PI_2, PI};

/// Utility Gain at or below this is −inf dB.
pub const UTIL_GAIN_MIN_DB: f32 = -70.0;
pub const UTIL_GAIN_MAX_DB: f32 = 35.0;
/// Width range (1.0 = 100 %).
pub const UTIL_WIDTH_MAX: f32 = 4.0;
pub const BASS_MONO_MIN_HZ: f32 = 50.0;
pub const BASS_MONO_MAX_HZ: f32 = 500.0;

/// Align Delay ranges.
pub const ALIGN_TIME_MAX_MS: f32 = 100.0;
pub const ALIGN_SAMPLES_MAX: f32 = 10_000.0;
pub const ALIGN_DIST_MAX_M: f32 = 100.0;
pub const ALIGN_TEMP_MIN_C: f32 = -50.0;
pub const ALIGN_TEMP_MAX_C: f32 = 50.0;
pub const FEET_PER_M: f32 = 3.280_84;
/// Delay line length (power of two): > 100 m at −50 °C (≈ 0.33 s) at 384 kHz.
const DELAY_CAP: usize = 1 << 17;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ChannelMode {
    Left,
    Right,
    #[default]
    Stereo,
    Swap,
}

impl ChannelMode {
    pub const ALL: [ChannelMode; 4] = [ChannelMode::Left, ChannelMode::Right, ChannelMode::Stereo, ChannelMode::Swap];
    pub fn label(self) -> &'static str {
        match self {
            ChannelMode::Left => "Left",
            ChannelMode::Right => "Right",
            ChannelMode::Stereo => "Stereo",
            ChannelMode::Swap => "Swap",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UtilityParams {
    pub phase_l: bool,
    pub phase_r: bool,
    pub channel: ChannelMode,
    /// The Width knob shows Mid/Side instead (context menu "Mid/Side Mode").
    pub ms_mode: bool,
    /// 0..4 (0 % .. 400 %).
    pub width: f32,
    /// −1 (100M) .. 0 .. 1 (100S).
    pub mid_side: f32,
    pub mono: bool,
    pub bass_mono: bool,
    pub bass_freq: f32,
    pub bass_audition: bool,
    /// dB; ≤ [`UTIL_GAIN_MIN_DB`] = −inf.
    pub gain_db: f32,
    /// −1 (50L) .. 1 (50R).
    pub balance: f32,
    pub mute: bool,
    pub dc: bool,
}

impl Default for UtilityParams {
    fn default() -> Self {
        Self {
            phase_l: false,
            phase_r: false,
            channel: ChannelMode::Stereo,
            ms_mode: false,
            width: 1.0,
            mid_side: 0.0,
            mono: false,
            bass_mono: false,
            bass_freq: 120.0,
            bass_audition: false,
            gain_db: 0.0,
            balance: 0.0,
            mute: false,
            dc: false,
        }
    }
}

impl UtilityParams {
    pub fn gain_lin(&self) -> f32 {
        if self.gain_db <= UTIL_GAIN_MIN_DB { 0.0 } else { 10f32.powf(self.gain_db.min(UTIL_GAIN_MAX_DB) / 20.0) }
    }

    /// (mid gain, side gain) of the stereo stage.
    pub fn ms_gains(&self) -> (f32, f32) {
        if self.mono {
            return (1.0, 0.0);
        }
        if matches!(self.channel, ChannelMode::Left | ChannelMode::Right) {
            return (1.0, 1.0); // already mono; Width / Mid-Side disabled
        }
        if self.ms_mode {
            let x = self.mid_side.clamp(-1.0, 1.0);
            ((1.0 - x).min(1.0), (1.0 + x).min(1.0))
        } else {
            (1.0, self.width.clamp(0.0, UTIL_WIDTH_MAX))
        }
    }

    /// Balance: unity at the centre; the opposite side falls with a sine law.
    pub fn balance_gains(&self) -> (f32, f32) {
        let b = self.balance.clamp(-1.0, 1.0);
        if b >= 0.0 { ((b * FRAC_PI_2).cos(), 1.0) } else { (1.0, (b * FRAC_PI_2).cos()) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DelayMode {
    #[default]
    Time,
    Samples,
    Distance,
}

impl DelayMode {
    pub const ALL: [DelayMode; 3] = [DelayMode::Time, DelayMode::Samples, DelayMode::Distance];
    pub fn label(self) -> &'static str {
        match self {
            DelayMode::Time => "Time",
            DelayMode::Samples => "Samples",
            DelayMode::Distance => "Distance",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AlignDelayParams {
    pub mode: DelayMode,
    /// Link L/R: the left settings drive both channels.
    pub link: bool,
    /// Milliseconds.
    pub time_l: f32,
    pub time_r: f32,
    pub samples_l: f32,
    pub samples_r: f32,
    /// Metres (shown in feet when `feet`).
    pub dist_l: f32,
    pub dist_r: f32,
    pub feet: bool,
    /// °C (shown in °F when `fahrenheit`).
    pub temp: f32,
    pub fahrenheit: bool,
}

impl Default for AlignDelayParams {
    fn default() -> Self {
        Self {
            mode: DelayMode::Time,
            link: false,
            time_l: 0.0,
            time_r: 0.0,
            samples_l: 0.0,
            samples_r: 0.0,
            dist_l: 0.0,
            dist_r: 0.0,
            feet: false,
            temp: 20.0,
            fahrenheit: false,
        }
    }
}

impl AlignDelayParams {
    /// Speed of sound (m/s) at `temp` °C.
    pub fn speed_of_sound(&self) -> f32 {
        331.4 + 0.607 * self.temp.clamp(ALIGN_TEMP_MIN_C, ALIGN_TEMP_MAX_C)
    }

    /// Delays in milliseconds (left, right) at sample rate `sr`.
    pub fn delays_ms(&self, sr: f32) -> (f32, f32) {
        let (l, r) = match self.mode {
            DelayMode::Time => (self.time_l, self.time_r),
            DelayMode::Samples => (self.samples_l.round() * 1000.0 / sr, self.samples_r.round() * 1000.0 / sr),
            DelayMode::Distance => {
                let c = self.speed_of_sound();
                (self.dist_l * 1000.0 / c, self.dist_r * 1000.0 / c)
            }
        };
        let (l, r) = (l.max(0.0), r.max(0.0));
        if self.link { (l, l) } else { (l, r) }
    }

    /// Whole-sample delays (left, right).
    pub fn delays_samples(&self, sr: f32) -> (usize, usize) {
        let to = |ms: f32| ((ms * sr / 1000.0).round() as usize).min(DELAY_CAP - 1);
        if self.mode == DelayMode::Samples {
            let l = (self.samples_l.max(0.0).round() as usize).min(DELAY_CAP - 1);
            let r = if self.link { l } else { (self.samples_r.max(0.0).round() as usize).min(DELAY_CAP - 1) };
            return (l, r);
        }
        let (l, r) = self.delays_ms(sr);
        (to(l), to(r))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)]
pub enum FxParams {
    Utility(UtilityParams),
    AlignDelay(AlignDelayParams),
    EqEight(EqEightParams),
    Rack(crate::rack::RackParams),
    Gate(crate::dynfx::GateParams),
    Glue(crate::dynfx::GlueParams),
    Limiter(crate::dynfx::LimiterParams),
    Lfo(crate::lfo::LfoParams),
    EqThree(crate::modfx::EqThreeParams),
    Chorus(crate::modfx::ChorusParams),
    AutoFilter(crate::modfx::AutoFilterParams),
    AutoPan(crate::modfx::AutoPanParams),
    Compressor(crate::dynfx::CompressorParams),
    Spectrum(crate::spectrum::SpectrumParams),
}

impl FxParams {
    pub fn name(&self) -> &'static str {
        match self {
            FxParams::Utility(_) => "Utility",
            FxParams::AlignDelay(_) => "Align Delay",
            FxParams::EqEight(_) => "EQ Eight",
            FxParams::Rack(_) => "Audio Effect Rack",
            FxParams::Gate(_) => "Gate",
            FxParams::Glue(_) => "Glue Compressor",
            FxParams::Limiter(_) => "Limiter",
            FxParams::Lfo(l) if l.shaper => "Shaper",
            FxParams::Lfo(_) => "LFO",
            FxParams::EqThree(_) => "EQ Three",
            FxParams::Chorus(_) => "Chorus-Ensemble",
            FxParams::AutoFilter(_) => "Auto Filter",
            FxParams::AutoPan(_) => "Auto Pan-Tremolo",
            FxParams::Compressor(_) => "Compressor",
            FxParams::Spectrum(_) => "Spectrum",
        }
    }

    /// Sidechain section (Gate, Glue Compressor).
    /// Lookahead latency (samples) of the device itself (racks: the
    /// slowest chain, see [`chain_latency`]).
    pub fn latency(&self, sr: f32) -> usize {
        match self {
            FxParams::Gate(p) => (crate::dynfx::GATE_LOOKAHEAD[(p.lookahead as usize).min(2)] * 0.001 * sr).round() as usize,
            FxParams::Limiter(p) => crate::dynfx::LimiterState::latency(p, sr),
            FxParams::Compressor(p) => p.latency(sr),
            _ => 0,
        }
    }

    pub fn sidechain(&self) -> Option<&crate::dynfx::Sidechain> {
        match self {
            FxParams::Gate(g) => Some(&g.sc),
            FxParams::Glue(g) => Some(&g.sc),
            FxParams::AutoFilter(a) => Some(&a.sc),
            FxParams::Compressor(c) => Some(&c.sc),
            _ => None,
        }
    }
}

/// One device in a chain (song side).
#[derive(Debug, Clone)]
pub struct FxSlot {
    /// Project device id (stable; used to carry the DSP state across songs).
    pub id: u64,
    pub on: bool,
    pub params: FxParams,
    pub state: Box<FxState>,
    /// Audio Effect Rack chains (`FxParams::Rack`).
    pub rack: Option<Box<crate::rack::RackDsp>>,
    /// Stored values while an LFO modulates `params`.
    pub base: FxParams,
    pub modded: bool,
    /// External sidechain source (engine track, tap), resolved by compile.
    pub sc_src: Option<(u16, crate::song::Tap)>,
}

impl FxSlot {
    /// Allocates the DSP state (control thread).
    pub fn new(id: u64, on: bool, params: FxParams) -> Self {
        let mut state = FxState::for_params(&params);
        match &mut state.kind {
            StateKind::Eq(e) => **e = EqState::new(Some(crate::eq8::spectrum_tap(id))),
            StateKind::Gate(g) => **g = GateState::new(Some(id)),
            StateKind::Glue(g) => **g = GlueState::new(Some(id)),
            StateKind::Limiter(l) => **l = LimiterState::new(Some(id)),
            StateKind::Lfo(l) => **l = LfoState::new(Some(id)),
            StateKind::EqThree(e) => **e = EqThreeState::new(Some(id)),
            StateKind::Chorus(c) => **c = ChorusState::new(Some(id)),
            StateKind::AutoFilter(a) => **a = AutoFilterState::new(Some(id)),
            StateKind::AutoPan(a) => **a = AutoPanState::new(Some(id)),
            StateKind::Compressor(c) => **c = CompressorState::new(Some(id)),
            StateKind::Spectrum(s) => **s = crate::spectrum::SpectrumState::new(Some(crate::eq8::spectrum_tap(id))),
            _ => {}
        }
        Self { id, on, params, state: Box::new(state), rack: None, base: params, modded: false, sc_src: None }
    }

    /// New stored values (SetFx); an active modulation is re-applied next block.
    pub fn set_params(&mut self, p: FxParams) {
        self.params = p;
        self.base = p;
    }

    /// Calls `f` on this slot and every slot inside its racks.
    pub fn visit(&mut self, f: &mut dyn FnMut(&mut FxSlot)) {
        f(self);
        if let Some(r) = self.rack.as_deref_mut() {
            for c in r.chains.iter_mut() {
                for s in c.devices.iter_mut() {
                    s.visit(f);
                }
            }
        }
    }

    pub fn visit_ref(&self, f: &mut dyn FnMut(&FxSlot)) {
        f(self);
        if let Some(r) = self.rack.as_deref() {
            for c in r.chains.iter() {
                for s in c.devices.iter() {
                    s.visit_ref(f);
                }
            }
        }
    }

    fn sc_state(&mut self) -> Option<&mut ScState> {
        match &mut self.state.kind {
            StateKind::Gate(g) => Some(&mut g.sc),
            StateKind::Glue(g) => Some(&mut g.sc),
            StateKind::AutoFilter(a) => Some(&mut a.sc),
            StateKind::Compressor(c) => Some(&mut c.sc),
            _ => None,
        }
    }
}

/// Copy the external sidechain sources into the Gate / Glue states of
/// `chain` (before it runs). `src(rt, tap)` returns the source block.
pub(crate) fn fill_sidechains<'a>(chain: &mut [FxSlot], n: usize, src: &dyn Fn(u16, crate::song::Tap) -> Option<(&'a [f32], &'a [f32])>) {
    for s in chain.iter_mut() {
        s.visit(&mut |s| {
            let Some((rt, tap)) = s.sc_src else { return };
            let on = s.params.sidechain().is_some_and(|c| c.on);
            if let Some(st) = s.sc_state() {
                st.len = 0;
                if on && let Some((l, r)) = src(rt, tap) {
                    let m = n.min(crate::dynfx::SC_MAX).min(l.len()).min(r.len());
                    st.ext_l[..m].copy_from_slice(&l[..m]);
                    st.ext_r[..m].copy_from_slice(&r[..m]);
                    st.len = m;
                }
            }
        });
    }
}

/// Maximum simultaneous LFO mappings applied per block.
const MAX_ROUTES: usize = 128;

/// LFO pass (start of every block): restore modulated slots to their stored
/// values, advance the LFOs and write their modulation into the targets.
pub(crate) fn modulate(song: &mut crate::song::Song, n: usize, sr: f32, tempo: f64, beat: Option<f64>) {
    if !song.lfo {
        return;
    }
    let crate::song::Song { tracks, master_fx, .. } = song;
    let mut each = |f: &mut dyn FnMut(&mut FxSlot)| {
        for s in tracks.iter_mut().flat_map(|t| t.fx.iter_mut()).chain(master_fx.iter_mut()) {
            s.visit(f);
        }
    };
    each(&mut |s| {
        if s.modded {
            s.params = s.base;
            s.modded = false;
        }
    });
    let mut routes = [(LfoMap::default(), 0.0f32); MAX_ROUTES];
    let mut nr = 0;
    each(&mut |s| {
        if let (FxParams::Lfo(p), StateKind::Lfo(st)) = (&s.params, &mut s.state.kind) {
            if !s.on {
                return;
            }
            let out = st.tick(p, n, sr, tempo, beat);
            for m in p.maps.iter().filter(|m| m.device != 0 && m.device != s.id) {
                if nr < MAX_ROUTES {
                    routes[nr] = (*m, out);
                    nr += 1;
                }
            }
        }
    });
    if nr == 0 {
        return;
    }
    each(&mut |t| {
        for &(m, out) in &routes[..nr] {
            if m.device == t.id && crate::params::mappable(&t.params, m.param) {
                // Mod: relative to the (possibly already modulated) value;
                // several LFOs on one parameter add up.
                let cur = crate::params::norm(&t.params, m.param).unwrap_or(0.0);
                crate::params::set_norm(&mut t.params, m.param, m.apply(cur, out));
                t.modded = true;
            }
        }
    });
}

/// Any LFO in these chains (the modulation pass is skipped without one).
pub(crate) fn has_lfo(chain: &[FxSlot]) -> bool {
    let mut any = false;
    for s in chain {
        s.visit_ref(&mut |s| any |= matches!(s.params, FxParams::Lfo(_)));
    }
    any
}

/// Move DSP states from `old` into `new` for slots with the same id, at any
/// rack depth (no allocation).
pub(crate) fn carry_states(new: &mut [FxSlot], old: &mut [FxSlot]) {
    for n in new.iter_mut() {
        if let Some(o) = find_slot(old, n.id)
            && o.state.kind == n.state.kind
        {
            std::mem::swap(&mut n.state, &mut o.state);
        }
        if let Some(r) = n.rack.as_deref_mut() {
            for c in r.chains.iter_mut() {
                carry_states(&mut c.devices, old);
            }
        }
    }
}

/// Device `id` anywhere in `chain` (racks searched recursively).
pub fn find_slot(chain: &mut [FxSlot], id: u64) -> Option<&mut FxSlot> {
    for s in chain.iter_mut() {
        if s.id == id {
            return Some(s);
        }
        if let Some(r) = s.rack.as_deref_mut() {
            for c in r.chains.iter_mut() {
                if let Some(x) = find_slot(&mut c.devices, id) {
                    return Some(x);
                }
            }
        }
    }
    None
}

/// Rack chain `id` anywhere in `chain`.
pub fn find_rack_chain(chain: &mut [FxSlot], id: u64) -> Option<&mut crate::rack::ChainDsp> {
    chain.iter_mut().filter_map(|s| s.rack.as_deref_mut()).find_map(|r| r.chain_mut(id))
}

#[derive(Debug, Clone)]
pub struct FxState {
    kind: StateKind,
    /// Device was processing last block (turning it on clears the history).
    was_on: bool,
}

#[derive(Debug, Clone)]
enum StateKind {
    Utility(Box<UtilityState>),
    Align(AlignState),
    Eq(Box<EqState>),
    Rack,
    Gate(Box<GateState>),
    Glue(Box<GlueState>),
    Limiter(Box<LimiterState>),
    Lfo(Box<LfoState>),
    EqThree(Box<EqThreeState>),
    Chorus(Box<ChorusState>),
    AutoFilter(Box<AutoFilterState>),
    AutoPan(Box<AutoPanState>),
    Compressor(Box<CompressorState>),
    Spectrum(Box<crate::spectrum::SpectrumState>),
}

impl PartialEq for StateKind {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

impl FxState {
    pub fn for_params(p: &FxParams) -> Self {
        let kind = match p {
            FxParams::Utility(_) => StateKind::Utility(Box::default()),
            FxParams::AlignDelay(_) => StateKind::Align(AlignState::new()),
            FxParams::EqEight(_) => StateKind::Eq(Box::new(EqState::new(None))),
            FxParams::Rack(_) => StateKind::Rack,
            FxParams::Gate(_) => StateKind::Gate(Box::new(GateState::new(None))),
            FxParams::Glue(_) => StateKind::Glue(Box::new(GlueState::new(None))),
            FxParams::Limiter(_) => StateKind::Limiter(Box::new(LimiterState::new(None))),
            FxParams::Lfo(_) => StateKind::Lfo(Box::new(LfoState::new(None))),
            FxParams::EqThree(_) => StateKind::EqThree(Box::new(EqThreeState::new(None))),
            FxParams::Chorus(_) => StateKind::Chorus(Box::new(ChorusState::new(None))),
            FxParams::AutoFilter(_) => StateKind::AutoFilter(Box::new(AutoFilterState::new(None))),
            FxParams::AutoPan(_) => StateKind::AutoPan(Box::new(AutoPanState::new(None))),
            FxParams::Compressor(_) => StateKind::Compressor(Box::new(CompressorState::new(None))),
            FxParams::Spectrum(_) => StateKind::Spectrum(Box::default()),
        };
        Self { kind, was_on: false }
    }

    fn reset(&mut self) {
        match &mut self.kind {
            StateKind::Utility(u) => u.reset(),
            StateKind::Align(a) => a.reset(),
            StateKind::Eq(e) => e.reset(),
            StateKind::Rack | StateKind::Lfo(_) | StateKind::Spectrum(_) => {}
            StateKind::Gate(g) => g.reset(),
            StateKind::Glue(g) => g.reset(),
            StateKind::Limiter(l) => l.reset(),
            StateKind::EqThree(e) => e.reset(),
            StateKind::Chorus(c) => c.reset(),
            StateKind::AutoFilter(a) => a.reset(),
            StateKind::AutoPan(a) => a.reset(),
            StateKind::Compressor(c) => c.reset(),
        }
    }
}

/// Process a chain in place. Returns the output peak (0 = silent).
/// Runs the chain in place; `meters[0]` takes the chain input and
/// `meters[i + 1]` the signal after device `i` (empty slice = no metering).
/// Returns the output peak.
/// Latency (samples) of a DSP chain: enabled devices in series; a rack
/// counts its slowest chain (the others are compensated inside).
pub fn chain_latency(chain: &[FxSlot], sr: f32) -> usize {
    chain
        .iter()
        .filter(|s| s.on)
        .map(|s| match (&s.params, s.rack.as_deref()) {
            (FxParams::Rack(_), Some(r)) => r.latency(sr),
            (p, _) => p.latency(sr),
        })
        .sum()
}

pub(crate) fn run_chain(chain: &mut [FxSlot], l: &mut [f32], r: &mut [f32], sr: f32, meters: &[crate::status::FxMeter]) -> f32 {
    if let Some(m) = meters.first() {
        m.add(l, r);
    }
    for (i, s) in chain.iter_mut().enumerate() {
        if !s.on {
            s.state.was_on = false;
            if let Some(m) = meters.get(i + 1) {
                m.add(l, r);
            }
            continue;
        }
        if !s.state.was_on {
            s.state.reset();
            s.state.was_on = true;
        }
        match (&s.params, &mut s.state.kind) {
            (FxParams::Utility(p), StateKind::Utility(st)) => st.process(p, l, r, sr),
            (FxParams::AlignDelay(p), StateKind::Align(st)) => st.process(p, l, r, sr),
            (FxParams::EqEight(p), StateKind::Eq(st)) => st.process(p, l, r, sr),
            (FxParams::Gate(p), StateKind::Gate(st)) => st.process(p, l, r, sr),
            (FxParams::Glue(p), StateKind::Glue(st)) => st.process(p, l, r, sr),
            (FxParams::Limiter(p), StateKind::Limiter(st)) => st.process(p, l, r, sr),
            (FxParams::EqThree(p), StateKind::EqThree(st)) => st.process(p, l, r, sr),
            (FxParams::Chorus(p), StateKind::Chorus(st)) => st.process(p, l, r, sr),
            (FxParams::AutoFilter(p), StateKind::AutoFilter(st)) => st.process(p, l, r, sr),
            (FxParams::AutoPan(p), StateKind::AutoPan(st)) => st.process(p, l, r, sr),
            (FxParams::Compressor(p), StateKind::Compressor(st)) => st.process(p, l, r, sr),
            (FxParams::Spectrum(p), StateKind::Spectrum(st)) => st.process(p, l, r, sr),
            (FxParams::Rack(p), StateKind::Rack) => {
                if let Some(rk) = s.rack.as_deref_mut() {
                    rk.process(p, l, r, sr);
                }
            }
            _ => {}
        }
        if let Some(m) = meters.get(i + 1) {
            m.add(l, r);
        }
    }
    let mut pk = 0.0f32;
    for (a, b) in l.iter().zip(r.iter()) {
        pk = pk.max(a.abs()).max(b.abs());
    }
    pk
}

/// Direct form I biquad.
#[derive(Debug, Clone, Copy, Default)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    x: [f32; 2],
    y: [f32; 2],
}

impl Biquad {
    /// RBJ low/high-pass with Q = 1/√2 (Butterworth).
    fn butter(&mut self, f: f32, sr: f32, high: bool) {
        let w = 2.0 * PI * (f / sr).clamp(1.0e-5, 0.49);
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        let (b0, b1) = if high { ((1.0 + c) / 2.0, -(1.0 + c)) } else { ((1.0 - c) / 2.0, 1.0 - c) };
        self.b = [b0 / a0, b1 / a0, b0 / a0];
        self.a = [-2.0 * c / a0, (1.0 - alpha) / a0];
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1] - self.a[0] * self.y[0] - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        let y = if y.abs() < 1.0e-20 { 0.0 } else { y };
        self.y = [y, self.y[0]];
        y
    }

    fn clear(&mut self) {
        self.x = [0.0; 2];
        self.y = [0.0; 2];
    }
}

/// 4th-order Linkwitz-Riley = two cascaded Butterworth sections.
#[derive(Debug, Clone, Copy, Default)]
struct Lr4([Biquad; 2]);

impl Lr4 {
    fn set(&mut self, f: f32, sr: f32, high: bool) {
        self.0[0].butter(f, sr, high);
        self.0[1].butter(f, sr, high);
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let y = self.0[0].tick(x);
        self.0[1].tick(y)
    }
    fn clear(&mut self) {
        self.0[0].clear();
        self.0[1].clear();
    }
}

/// Linear per-block ramp toward a target (click-free parameter changes).
#[derive(Debug, Clone, Copy)]
struct Ramp {
    cur: f32,
    init: bool,
}

impl Default for Ramp {
    fn default() -> Self {
        Self { cur: 0.0, init: false }
    }
}

impl Ramp {
    /// Returns (start, step) for an `n`-sample block ending at `target`.
    fn block(&mut self, target: f32, n: usize) -> (f32, f32) {
        if !self.init {
            self.cur = target;
            self.init = true;
        }
        let start = self.cur;
        self.cur = target;
        (start, (target - start) / n.max(1) as f32)
    }
}

#[derive(Debug, Clone, Default)]
struct UtilityState {
    lp_m: Lr4,
    /// Side low band (audition without Bass Mono keeps the lows stereo).
    lp_s: Lr4,
    hp_m: Lr4,
    hp_s: Lr4,
    coef: (f32, f32),
    mid: Ramp,
    side: Ramp,
    gl: Ramp,
    gr: Ramp,
    /// DC blocker: (x[n-1], y[n-1]) per channel.
    dc: [(f32, f32); 2],
}

impl UtilityState {
    fn reset(&mut self) {
        self.lp_m.clear();
        self.lp_s.clear();
        self.hp_m.clear();
        self.hp_s.clear();
        self.dc = [(0.0, 0.0); 2];
        self.mid.init = false;
        self.side.init = false;
        self.gl.init = false;
        self.gr.init = false;
    }

    fn process(&mut self, p: &UtilityParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let n = l.len().min(r.len());
        let (mg, sg) = p.ms_gains();
        let (bl, br) = p.balance_gains();
        let g = if p.mute { 0.0 } else { p.gain_lin() };
        let (m0, md) = self.mid.block(mg, n);
        let (s0, sd) = self.side.block(sg, n);
        let (l0, ld) = self.gl.block(g * bl, n);
        let (r0, rd) = self.gr.block(g * br, n);
        let bass = p.bass_mono && !p.mono;
        // Audition plays the band below the frequency: mono when Bass Mono is
        // on, otherwise the stereo lows.
        let audition = p.bass_audition;
        if bass || audition {
            let f = p.bass_freq.clamp(BASS_MONO_MIN_HZ, BASS_MONO_MAX_HZ);
            if self.coef != (f, sr) {
                self.coef = (f, sr);
                self.lp_m.set(f, sr, false);
                self.lp_s.set(f, sr, false);
                self.hp_m.set(f, sr, true);
                self.hp_s.set(f, sr, true);
            }
        }
        // DC blocker: one-pole high-pass at ~5 Hz.
        let dc_k = 1.0 - 2.0 * PI * 5.0 / sr;
        let (fl, fr) = (if p.phase_l { -1.0 } else { 1.0 }, if p.phase_r { -1.0 } else { 1.0 });
        for i in 0..n {
            let (mut a, mut b) = (l[i] * fl, r[i] * fr);
            match p.channel {
                ChannelMode::Left => b = a,
                ChannelMode::Right => a = b,
                ChannelMode::Stereo => {}
                ChannelMode::Swap => std::mem::swap(&mut a, &mut b),
            }
            let k = (i + 1) as f32;
            let m = 0.5 * (a + b) * (m0 + md * k);
            let s = 0.5 * (a - b) * (s0 + sd * k);
            let (mut x, mut y) = if audition {
                let low = self.lp_m.tick(m);
                if bass {
                    (low, low)
                } else {
                    let ls = self.lp_s.tick(s);
                    (low + ls, low - ls)
                }
            } else if bass {
                let low = self.lp_m.tick(m);
                let hm = self.hp_m.tick(m);
                let hs = self.hp_s.tick(s);
                (low + hm + hs, low + hm - hs)
            } else {
                (m + s, m - s)
            };
            x *= l0 + ld * k;
            y *= r0 + rd * k;
            if p.dc {
                for (c, v) in [&mut x, &mut y].into_iter().enumerate() {
                    let (px, py) = self.dc[c];
                    let out = *v - px + dc_k * py;
                    let out = if out.abs() < 1.0e-20 { 0.0 } else { out };
                    self.dc[c] = (*v, out);
                    *v = out;
                }
            }
            l[i] = x;
            r[i] = y;
        }
    }
}

#[derive(Debug, Clone)]
struct AlignState {
    buf: Vec<[f32; 2]>,
    pos: usize,
}

impl AlignState {
    fn new() -> Self {
        Self { buf: vec![[0.0; 2]; DELAY_CAP], pos: 0 }
    }

    fn reset(&mut self) {
        self.buf.fill([0.0; 2]);
        self.pos = 0;
    }

    fn process(&mut self, p: &AlignDelayParams, l: &mut [f32], r: &mut [f32], sr: f32) {
        let (dl, dr) = p.delays_samples(sr);
        if dl == 0 && dr == 0 {
            // Keep the history running so a later delay change has signal.
            for (a, b) in l.iter().zip(r.iter()) {
                self.buf[self.pos] = [*a, *b];
                self.pos = (self.pos + 1) & (DELAY_CAP - 1);
            }
            return;
        }
        let mask = DELAY_CAP - 1;
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            self.buf[self.pos] = [*a, *b];
            *a = self.buf[(self.pos + DELAY_CAP - dl) & mask][0];
            *b = self.buf[(self.pos + DELAY_CAP - dr) & mask][1];
            self.pos = (self.pos + 1) & mask;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(p: FxParams, l: &mut [f32], r: &mut [f32]) {
        let mut chain = vec![FxSlot::new(1, true, p)];
        run_chain(&mut chain, l, r, 48_000.0, &[]);
    }

    #[test]
    fn utility_defaults_are_transparent() {
        let mut l: Vec<f32> = (0..64).map(|i| (i as f32 * 0.1).sin()).collect();
        let mut r: Vec<f32> = (0..64).map(|i| (i as f32 * 0.13).cos()).collect();
        let (l0, r0) = (l.clone(), r.clone());
        run(FxParams::Utility(UtilityParams::default()), &mut l, &mut r);
        for i in 0..64 {
            assert!((l[i] - l0[i]).abs() < 1e-6 && (r[i] - r0[i]).abs() < 1e-6);
        }
    }

    #[test]
    fn utility_width_mono_phase_gain_balance() {
        let p = UtilityParams { width: 0.0, ..Default::default() };
        let (mut l, mut r) = (vec![1.0; 8], vec![0.0; 8]);
        run(FxParams::Utility(p), &mut l, &mut r);
        assert!((l[7] - 0.5).abs() < 1e-6 && (r[7] - 0.5).abs() < 1e-6);
        // 100S: side only, L/R opposite.
        let p = UtilityParams { ms_mode: true, mid_side: 1.0, ..Default::default() };
        let (mut l, mut r) = (vec![1.0; 8], vec![0.0; 8]);
        run(FxParams::Utility(p), &mut l, &mut r);
        assert!((l[7] - 0.5).abs() < 1e-6 && (r[7] + 0.5).abs() < 1e-6);
        // Phase L + left channel mode.
        let p = UtilityParams { phase_l: true, channel: ChannelMode::Left, ..Default::default() };
        let (mut l, mut r) = (vec![0.5; 8], vec![0.2; 8]);
        run(FxParams::Utility(p), &mut l, &mut r);
        assert!((l[7] + 0.5).abs() < 1e-6 && (r[7] + 0.5).abs() < 1e-6);
        // −inf gain / mute silence; full right balance kills the left.
        let p = UtilityParams { gain_db: UTIL_GAIN_MIN_DB, ..Default::default() };
        let (mut l, mut r) = (vec![1.0; 8], vec![1.0; 8]);
        run(FxParams::Utility(p), &mut l, &mut r);
        assert_eq!((l[7], r[7]), (0.0, 0.0));
        let p = UtilityParams { balance: 1.0, gain_db: 6.0, ..Default::default() };
        let (mut l, mut r) = (vec![1.0; 8], vec![1.0; 8]);
        run(FxParams::Utility(p), &mut l, &mut r);
        assert!(l[7].abs() < 1e-6 && (r[7] - 1.995).abs() < 1e-3);
    }

    #[test]
    fn bass_mono_removes_low_side_only() {
        let sr = 48_000.0;
        let p = UtilityParams { bass_mono: true, bass_freq: 120.0, ..Default::default() };
        // 40 Hz fully out of phase → side only → removed; 4 kHz side stays.
        let n = 48_000;
        for (hz, keep) in [(40.0f32, false), (4000.0, true)] {
            let mut l: Vec<f32> = (0..n).map(|i| (2.0 * PI * hz * i as f32 / sr).sin()).collect();
            let mut r: Vec<f32> = l.iter().map(|x| -x).collect();
            run(FxParams::Utility(p), &mut l, &mut r);
            let rms = (l[n / 2..].iter().map(|x| x * x).sum::<f32>() / (n / 2) as f32).sqrt();
            if keep { assert!(rms > 0.65, "{rms}") } else { assert!(rms < 0.02, "{rms}") }
        }
    }

    #[test]
    fn audition_plays_lows_mono_or_stereo() {
        let sr = 48_000.0;
        let n = 48_000;
        let rms = |v: &[f32]| (v[n / 2..].iter().map(|x| x * x).sum::<f32>() / (n / 2) as f32).sqrt();
        // (Bass Mono, frequency, audible)
        for (bass, hz, loud) in [(false, 40.0f32, true), (false, 4000.0, false), (true, 40.0, false)] {
            let p = UtilityParams { bass_mono: bass, bass_audition: true, bass_freq: 120.0, ..Default::default() };
            let mut l: Vec<f32> = (0..n).map(|i| (2.0 * PI * hz * i as f32 / sr).sin()).collect();
            let mut r: Vec<f32> = l.iter().map(|x| -x).collect();
            run(FxParams::Utility(p), &mut l, &mut r);
            // Out-of-phase lows: kept (stereo) without Bass Mono, cancelled with it.
            if loud {
                assert!(rms(&l) > 0.6 && rms(&r) > 0.6, "{hz} {}", rms(&l))
            } else {
                assert!(rms(&l) < 0.05, "{hz} {bass} {}", rms(&l))
            }
        }
    }

    #[test]
    fn align_delay_modes() {
        let p = AlignDelayParams { time_l: 1.0, time_r: 0.0, ..Default::default() };
        assert_eq!(p.delays_samples(48_000.0), (48, 0));
        let p = AlignDelayParams { mode: DelayMode::Samples, samples_l: 3.0, samples_r: 7.4, ..Default::default() };
        assert_eq!(p.delays_samples(44_100.0), (3, 7));
        let p = AlignDelayParams { mode: DelayMode::Distance, dist_r: 7.6, ..Default::default() };
        let (_, r) = p.delays_ms(44_100.0);
        assert_eq!(format!("{r:.2}"), "22.12");
        let p = AlignDelayParams { link: true, time_l: 2.0, time_r: 9.0, ..Default::default() };
        assert_eq!(p.delays_samples(48_000.0), (96, 96));
        // Impulse comes out 3 samples later on the left.
        let p = AlignDelayParams { mode: DelayMode::Samples, samples_l: 3.0, ..Default::default() };
        let (mut l, mut r) = (vec![0.0; 8], vec![0.0; 8]);
        l[0] = 1.0;
        r[0] = 1.0;
        run(FxParams::AlignDelay(p), &mut l, &mut r);
        assert_eq!(l, [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(r[0], 1.0);
    }

    #[test]
    fn meters_tap_input_and_each_device() {
        let m: Vec<crate::status::FxMeter> = (0..3).map(|_| Default::default()).collect();
        let mute = FxParams::Utility(UtilityParams { mute: true, ..Default::default() });
        let mut chain = vec![FxSlot::new(1, true, FxParams::Utility(UtilityParams::default())), FxSlot::new(2, true, mute)];
        let (mut l, mut r) = (vec![0.5f32; 64], vec![-0.25f32; 64]);
        run_chain(&mut chain, &mut l, &mut r, 48_000.0, &m);
        let read = |i: usize| m[i].peek();
        assert_eq!(read(0)[..2], [0.5, 0.25]);
        assert!((read(0)[2] - 0.25).abs() < 1e-6 && (read(0)[3] - 0.0625).abs() < 1e-6, "mean squares");
        assert!(read(1)[0] > 0.49, "unity Utility passes");
        assert_eq!(read(2), [0.0; 4], "muted");
    }

    #[test]
    fn state_survives_song_swaps() {
        let p = FxParams::AlignDelay(AlignDelayParams { mode: DelayMode::Samples, samples_l: 4.0, samples_r: 4.0, ..Default::default() });
        let mut old = vec![FxSlot::new(9, true, p)];
        let (mut l, mut r) = (vec![1.0, 0.0], vec![1.0, 0.0]);
        run_chain(&mut old, &mut l, &mut r, 48_000.0, &[]);
        let mut new = vec![FxSlot::new(9, true, p)];
        carry_states(&mut new, &mut old);
        let (mut l, mut r) = (vec![0.0; 4], vec![0.0; 4]);
        run_chain(&mut new, &mut l, &mut r, 48_000.0, &[]);
        assert_eq!(l, [0.0, 0.0, 1.0, 0.0]);
    }
}
