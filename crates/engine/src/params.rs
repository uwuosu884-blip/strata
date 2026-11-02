//! Normalised (0..1) parameter access for every device: knobs, the LFO's
//! modulation (engine) and macro mappings (app) share these mappings.
//! Parameter ids are per device type (`up`, `ap`, `eqp`, `gp`, `cp`, `lp`,
//! `fp`; the sidechain ids `sc` are shared by Gate and Glue Compressor).

use crate::dynfx::*;
use crate::fx::*;
use crate::lfo::*;
use crate::modfx::*;

/// Rack: macros are 0..16, the chain selector 16.
pub const RACK_SELECTOR: u8 = 16;

pub const P18: f32 = 0.5 - 18.0 / 70.0;

/// Utility Gain knob: 0 dB at the top, 70 dB per knob turn above −18 dB,
/// a quadratic amplitude taper down to −inf below.
pub fn gain_to_norm(db: f32) -> f32 {
    if db <= UTIL_GAIN_MIN_DB {
        0.0
    } else if db >= -18.0 {
        (0.5 + db / 70.0).min(1.0)
    } else {
        P18 * 10f32.powf((db + 18.0) / 40.0)
    }
}

pub fn norm_to_gain(p: f32) -> f32 {
    let p = p.clamp(0.0, 1.0);
    let db = if p >= P18 {
        70.0 * (p - 0.5)
    } else if p <= 0.0 {
        UTIL_GAIN_MIN_DB
    } else {
        -18.0 + 40.0 * (p / P18).log10()
    };
    if db <= UTIL_GAIN_MIN_DB + 0.05 { UTIL_GAIN_MIN_DB } else { db.min(UTIL_GAIN_MAX_DB) }
}

/// Utility parameter ids.
pub mod up {
    pub const PHASE_L: u8 = 0;
    pub const PHASE_R: u8 = 1;
    pub const CHANNEL: u8 = 2;
    pub const WIDTH: u8 = 3;
    pub const MONO: u8 = 4;
    pub const BASS: u8 = 5;
    pub const FREQ: u8 = 6;
    pub const AUDITION: u8 = 7;
    pub const GAIN: u8 = 8;
    pub const BALANCE: u8 = 9;
    pub const MUTE: u8 = 10;
    pub const DC: u8 = 11;
}

/// Align Delay parameter ids.
pub mod ap {
    pub const MODE: u8 = 0;
    pub const LINK: u8 = 1;
    pub const LEFT: u8 = 2;
    pub const RIGHT: u8 = 3;
    pub const FEET: u8 = 4;
    pub const TEMP: u8 = 5;
    pub const FAHRENHEIT: u8 = 6;
}

/// Continuous parameters: normalised value (None for switches).
pub fn norm(p: &FxParams, k: u8) -> Option<f32> {
    match p {
        FxParams::Utility(u) => Some(match k {
            up::WIDTH if u.ms_mode => (u.mid_side + 1.0) / 2.0,
            up::WIDTH if u.width <= 1.0 => u.width / 2.0,
            up::WIDTH => 0.5 + (u.width - 1.0) / 6.0,
            up::FREQ => (u.bass_freq / BASS_MONO_MIN_HZ).ln() / 10f32.ln(),
            up::GAIN => gain_to_norm(u.gain_db),
            up::BALANCE => (u.balance + 1.0) / 2.0,
            _ => return None,
        }),
        FxParams::AlignDelay(a) => Some(match k {
            ap::LEFT | ap::RIGHT => (align_value(a, k == ap::RIGHT) / align_max(a)).max(0.0).sqrt(),
            ap::TEMP => (a.temp - ALIGN_TEMP_MIN_C) / (ALIGN_TEMP_MAX_C - ALIGN_TEMP_MIN_C),
            _ => return None,
        }),
        FxParams::EqEight(e) => eqp::norm(e, k),
        FxParams::Gate(g) => gate_norm(g, k),
        FxParams::Glue(g) => glue_norm(g, k),
        FxParams::Limiter(l) => lim_norm(l, k),
        FxParams::Lfo(l) => lfo_norm(l, k),
        FxParams::EqThree(e) => eq3_norm(e, k),
        FxParams::Chorus(c) => cho_norm(c, k),
        FxParams::AutoFilter(a) => af_norm(a, k),
        FxParams::AutoPan(a) => pan_norm(a, k),
        FxParams::Compressor(c) => comp_norm(c, k),
        FxParams::Spectrum(s) => spec_norm(s, k),
        FxParams::AutoShift(a) => as_norm(a, k),
        FxParams::ChannelEq(c) => crate::params2::ceq_norm(c, k),
        FxParams::Echo(e) => crate::params2::echo_norm(e, k),
        FxParams::Erosion(e) => crate::params2::ero_norm(e, k),
        FxParams::Reverb(r) => crate::params2::rv_norm(r, k),
        FxParams::Redux(r) => crate::params3::rdx_norm(r, k),
        FxParams::Saturator(s) => crate::params3::sat_norm(s, k),
        FxParams::Delay(d) => crate::params3::dly_norm(d, k),
        FxParams::GrainDelay(g) => crate::params3::gd_norm(g, k),
        FxParams::Resonators(x) => crate::params4::res_norm(x, k),
        FxParams::DrumBuss(x) => crate::params4::db_norm(x, k),
        FxParams::SpectralResonator(x) => crate::params4::sr_norm(x, k),
        FxParams::SpectralTime(x) => crate::params4::st_norm(x, k),
        FxParams::DynamicTube(x) => crate::params5::dt_norm(x, k),
        FxParams::Vocoder(x) => crate::params5::vc_norm(x, k),
        FxParams::VinylDistortion(x) => crate::params5::vd_norm(x, k),
        FxParams::Shifter(x) => crate::params5::sh_norm(x, k),
        FxParams::Rack(r) => match k {
            0..16 => Some(r.macros[k as usize] / 127.0),
            RACK_SELECTOR => Some(r.selector / 127.0),
            _ => None,
        },
    }
}

pub fn set_norm(p: &mut FxParams, k: u8, v: f32) {
    let v = v.clamp(0.0, 1.0);
    match p {
        FxParams::Utility(u) => match k {
            up::WIDTH if u.ms_mode => u.mid_side = v * 2.0 - 1.0,
            up::WIDTH => u.width = if v <= 0.5 { v * 2.0 } else { 1.0 + (v - 0.5) * 6.0 },
            up::FREQ => u.bass_freq = BASS_MONO_MIN_HZ * 10f32.powf(v),
            up::GAIN => u.gain_db = norm_to_gain(v),
            up::BALANCE => u.balance = v * 2.0 - 1.0,
            _ => {}
        },
        FxParams::AlignDelay(a) => match k {
            ap::LEFT | ap::RIGHT => {
                let x = v * v * align_max(a);
                set_align_value(a, k == ap::RIGHT, x);
            }
            ap::TEMP => a.temp = ALIGN_TEMP_MIN_C + v * (ALIGN_TEMP_MAX_C - ALIGN_TEMP_MIN_C),
            _ => {}
        },
        FxParams::EqEight(e) => eqp::set_norm(e, k, v),
        FxParams::Gate(g) => gate_set(g, k, v),
        FxParams::Glue(g) => glue_set(g, k, v),
        FxParams::Limiter(l) => lim_set(l, k, v),
        FxParams::Lfo(l) => lfo_set(l, k, v),
        FxParams::EqThree(e) => eq3_set(e, k, v),
        FxParams::Chorus(c) => cho_set(c, k, v),
        FxParams::AutoFilter(a) => af_set(a, k, v),
        FxParams::AutoPan(a) => pan_set(a, k, v),
        FxParams::Compressor(c) => comp_set(c, k, v),
        FxParams::Spectrum(s) => spec_set(s, k, v),
        FxParams::AutoShift(a) => as_set(a, k, v),
        FxParams::ChannelEq(c) => crate::params2::ceq_set(c, k, v),
        FxParams::Echo(e) => crate::params2::echo_set(e, k, v),
        FxParams::Erosion(e) => crate::params2::ero_set(e, k, v),
        FxParams::Reverb(r) => crate::params2::rv_set(r, k, v),
        FxParams::Redux(r) => crate::params3::rdx_set(r, k, v),
        FxParams::Saturator(s) => crate::params3::sat_set(s, k, v),
        FxParams::Delay(d) => crate::params3::dly_set(d, k, v),
        FxParams::GrainDelay(g) => crate::params3::gd_set(g, k, v),
        FxParams::Resonators(x) => crate::params4::res_set(x, k, v),
        FxParams::DrumBuss(x) => crate::params4::db_set(x, k, v),
        FxParams::SpectralResonator(x) => crate::params4::sr_set(x, k, v),
        FxParams::SpectralTime(x) => crate::params4::st_set(x, k, v),
        FxParams::DynamicTube(x) => crate::params5::dt_set(x, k, v),
        FxParams::Vocoder(x) => crate::params5::vc_set(x, k, v),
        FxParams::VinylDistortion(x) => crate::params5::vd_set(x, k, v),
        FxParams::Shifter(x) => crate::params5::sh_set(x, k, v),
        FxParams::Rack(r) => match k {
            0..16 => r.macros[k as usize] = (v * 127.0).round(),
            RACK_SELECTOR => r.selector = (v * 127.0).round(),
            _ => {}
        },
    }
}

pub fn align_max(a: &AlignDelayParams) -> f32 {
    match a.mode {
        DelayMode::Time => ALIGN_TIME_MAX_MS,
        DelayMode::Samples => ALIGN_SAMPLES_MAX,
        DelayMode::Distance => ALIGN_DIST_MAX_M,
    }
}

/// Current L/R value in the mode's unit (metres for Distance).
pub fn align_value(a: &AlignDelayParams, right: bool) -> f32 {
    let right = right && !a.link;
    match (a.mode, right) {
        (DelayMode::Time, false) => a.time_l,
        (DelayMode::Time, true) => a.time_r,
        (DelayMode::Samples, false) => a.samples_l,
        (DelayMode::Samples, true) => a.samples_r,
        (DelayMode::Distance, false) => a.dist_l,
        (DelayMode::Distance, true) => a.dist_r,
    }
}

pub fn set_align_value(a: &mut AlignDelayParams, right: bool, x: f32) {
    let x = x.clamp(0.0, align_max(a));
    match (a.mode, right) {
        (DelayMode::Time, false) => a.time_l = x,
        (DelayMode::Time, true) => a.time_r = x,
        (DelayMode::Samples, false) => a.samples_l = x.round(),
        (DelayMode::Samples, true) => a.samples_r = x.round(),
        (DelayMode::Distance, false) => a.dist_l = x,
        (DelayMode::Distance, true) => a.dist_r = x,
    }
}

/// EQ Eight parameter ids and mappings.
pub mod eqp {
    use crate::eq8::*;

    // Parameter ids: bands are `ch * 40 + band * 5 + field`.
    pub const FREQ: u8 = 0;
    pub const GAIN: u8 = 1;
    pub const QF: u8 = 2;
    pub const ON: u8 = 3;
    pub const TYPE: u8 = 4;
    pub const MODE: u8 = 100;
    pub const EDIT: u8 = 101;
    pub const ADAPT: u8 = 102;
    pub const SCALE: u8 = 103;
    pub const OUTPUT: u8 = 104;
    pub const AUDITION: u8 = 105;
    pub const ANALYZE: u8 = 106;
    pub const BLOCK: u8 = 107;
    pub const REFRESH: u8 = 108;
    pub const AVG: u8 = 109;
    /// The display's band dots (arrow keys move the selected dots).
    pub const DOTS: u8 = 110;

    pub fn band_id(ch: usize, b: usize, field: u8) -> u8 {
        (ch * 40 + b * 5) as u8 + field
    }

    pub fn split(k: u8) -> Option<(usize, usize, u8)> {
        (k < 80).then_some(((k / 40) as usize, ((k % 40) / 5) as usize, k % 5))
    }

    pub const LN_RANGE: f32 = 7.696_213; // ln(22000 / 10)

    pub fn freq_norm(f: f32) -> f32 {
        (f.clamp(EQ_FREQ_MIN, EQ_FREQ_MAX) / EQ_FREQ_MIN).ln() / LN_RANGE
    }

    pub fn norm_freq(v: f32) -> f32 {
        (EQ_FREQ_MIN * (v.clamp(0.0, 1.0) * LN_RANGE).exp()).clamp(EQ_FREQ_MIN, EQ_FREQ_MAX)
    }

    pub fn q_norm(q: f32) -> f32 {
        (q.clamp(EQ_Q_MIN, EQ_Q_MAX) / EQ_Q_MIN).ln() / (EQ_Q_MAX / EQ_Q_MIN).ln()
    }

    pub fn norm_q(v: f32) -> f32 {
        EQ_Q_MIN * (v.clamp(0.0, 1.0) * (EQ_Q_MAX / EQ_Q_MIN).ln()).exp()
    }

    pub fn norm(e: &EqEightParams, k: u8) -> Option<f32> {
        if let Some((ch, b, f)) = split(k) {
            let band = &e.bands[ch][b];
            return match f {
                FREQ => Some(freq_norm(band.freq)),
                GAIN => Some((band.gain + EQ_GAIN_MAX) / (2.0 * EQ_GAIN_MAX)),
                QF => Some(q_norm(band.q)),
                _ => None,
            };
        }
        Some(match k {
            SCALE => (e.scale + EQ_SCALE_MAX) / (2.0 * EQ_SCALE_MAX),
            OUTPUT => (e.output + EQ_OUT_MAX) / (2.0 * EQ_OUT_MAX),
            REFRESH => (e.refresh - EQ_REFRESH_MIN) / (EQ_REFRESH_MAX - EQ_REFRESH_MIN),
            AVG => (e.avg - 1.0) / (EQ_AVG_MAX - 1.0),
            _ => return None,
        })
    }

    pub fn set_norm(e: &mut EqEightParams, k: u8, v: f32) {
        let v = v.clamp(0.0, 1.0);
        if let Some((ch, b, f)) = split(k) {
            let band = &mut e.bands[ch][b];
            match f {
                FREQ => band.freq = norm_freq(v),
                GAIN => band.gain = v * 2.0 * EQ_GAIN_MAX - EQ_GAIN_MAX,
                QF => band.q = norm_q(v),
                _ => {}
            }
            return;
        }
        match k {
            SCALE => e.scale = v * 2.0 * EQ_SCALE_MAX - EQ_SCALE_MAX,
            OUTPUT => e.output = v * 2.0 * EQ_OUT_MAX - EQ_OUT_MAX,
            REFRESH => e.refresh = EQ_REFRESH_MIN + v * (EQ_REFRESH_MAX - EQ_REFRESH_MIN),
            AVG => e.avg = 1.0 + v * (EQ_AVG_MAX - 1.0),
            _ => {}
        }
    }
}

/// `n` (0..1) → `lo..hi` with power taper `k` (k > 1: finer at the bottom).
pub fn pow_val(n: f32, lo: f32, hi: f32, k: f32) -> f32 {
    lo + (hi - lo) * n.clamp(0.0, 1.0).powf(k)
}

pub fn pow_norm(v: f32, lo: f32, hi: f32, k: f32) -> f32 {
    ((v - lo) / (hi - lo)).clamp(0.0, 1.0).powf(1.0 / k)
}

pub fn log_val(n: f32, lo: f32, hi: f32) -> f32 {
    lo * (hi / lo).powf(n.clamp(0.0, 1.0))
}

pub fn log_norm(v: f32, lo: f32, hi: f32) -> f32 {
    ((v.clamp(lo, hi) / lo).ln() / (hi / lo).ln()).clamp(0.0, 1.0)
}

pub(crate) fn lin_val(n: f32, lo: f32, hi: f32) -> f32 {
    lo + (hi - lo) * n.clamp(0.0, 1.0)
}

pub(crate) fn lin_norm(v: f32, lo: f32, hi: f32) -> f32 {
    ((v - lo) / (hi - lo)).clamp(0.0, 1.0)
}

pub(crate) fn idx_norm(i: u8, n: usize) -> f32 {
    (i as f32 / (n - 1) as f32).clamp(0.0, 1.0)
}

pub(crate) fn norm_idx(v: f32, n: usize) -> u8 {
    (v.clamp(0.0, 1.0) * (n - 1) as f32).round() as u8
}

/// Gate threshold knob and display share one scale (Live's labels: +6, 0,
/// −6, −12, −18, −36, −70 dB at these heights).
pub const GATE_SCALE: [(f32, f32); 7] =
    [(-70.0, 0.0), (-36.0, 18.0 / 98.0), (-18.0, 39.0 / 98.0), (-12.0, 53.0 / 98.0), (-6.0, 68.0 / 98.0), (0.0, 82.0 / 98.0), (6.0, 1.0)];

/// Monotone cubic (Fritsch–Carlson) through `pts` (x ascending).
pub fn mono_cubic(pts: &[(f32, f32)], x: f32) -> f32 {
    let n = pts.len();
    let x = x.clamp(pts[0].0, pts[n - 1].0);
    let i = pts.windows(2).position(|w| x <= w[1].0).unwrap_or(n - 2);
    let d = |k: usize| (pts[k + 1].1 - pts[k].1) / (pts[k + 1].0 - pts[k].0);
    let m = |k: usize| {
        if k == 0 {
            d(0)
        } else if k == n - 1 {
            d(n - 2)
        } else {
            let (a, b) = (d(k - 1), d(k));
            if a * b <= 0.0 { 0.0 } else { 2.0 / (1.0 / a + 1.0 / b) }
        }
    };
    let (x0, y0, x1, y1) = (pts[i].0, pts[i].1, pts[i + 1].0, pts[i + 1].1);
    let h = x1 - x0;
    let t = (x - x0) / h;
    let (t2, t3) = (t * t, t * t * t);
    (2.0 * t3 - 3.0 * t2 + 1.0) * y0 + (t3 - 2.0 * t2 + t) * h * m(i) + (-2.0 * t3 + 3.0 * t2) * y1 + (t3 - t2) * h * m(i + 1)
}

/// Inverse of [`mono_cubic`] (monotone rising) by bisection.
pub fn mono_cubic_inv(pts: &[(f32, f32)], y: f32) -> f32 {
    let (mut lo, mut hi) = (pts[0].0, pts[pts.len() - 1].0);
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if mono_cubic(pts, mid) < y {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

pub fn gate_db_norm(db: f32) -> f32 {
    mono_cubic(&GATE_SCALE, db)
}

pub fn gate_norm_db(n: f32) -> f32 {
    mono_cubic_inv(&GATE_SCALE, n.clamp(0.0, 1.0))
}

/// Sidechain ids (Gate and Glue Compressor).
pub mod sc {
    pub const SHOWN: u8 = 40;
    pub const ON: u8 = 41;
    pub const SOURCE: u8 = 42;
    pub const TAP: u8 = 43;
    pub const GAIN: u8 = 44;
    pub const MIX: u8 = 45;
    pub const EQ_ON: u8 = 46;
    pub const EQ: u8 = 47;
    pub const FREQ: u8 = 48;
    pub const Q: u8 = 49;
    pub const EQ_GAIN: u8 = 50;
    pub const LISTEN: u8 = 51;
}

fn sc_norm(s: &Sidechain, k: u8) -> Option<f32> {
    Some(match k {
        sc::GAIN => lin_norm(s.gain, SC_GAIN_MIN, SC_GAIN_MAX),
        sc::MIX => s.mix,
        sc::FREQ => log_norm(s.freq, SC_FREQ_MIN, SC_FREQ_MAX),
        sc::Q => log_norm(s.q, SC_Q_MIN, SC_Q_MAX),
        sc::EQ_GAIN => lin_norm(s.eq_gain, -SC_EQ_GAIN_MAX, SC_EQ_GAIN_MAX),
        _ => return None,
    })
}

fn sc_set(s: &mut Sidechain, k: u8, v: f32) {
    match k {
        sc::GAIN => s.gain = lin_val(v, SC_GAIN_MIN, SC_GAIN_MAX),
        sc::MIX => s.mix = v,
        sc::FREQ => s.freq = log_val(v, SC_FREQ_MIN, SC_FREQ_MAX),
        sc::Q => s.q = log_val(v, SC_Q_MIN, SC_Q_MAX),
        sc::EQ_GAIN => s.eq_gain = lin_val(v, -SC_EQ_GAIN_MAX, SC_EQ_GAIN_MAX),
        _ => {}
    }
}

/// Gate ids.
pub mod gp {
    pub const THRESHOLD: u8 = 0;
    pub const RETURN: u8 = 1;
    pub const FLIP: u8 = 2;
    pub const LOOKAHEAD: u8 = 3;
    pub const ATTACK: u8 = 4;
    pub const HOLD: u8 = 5;
    pub const RELEASE: u8 = 6;
    pub const FLOOR: u8 = 7;
}

/// Gate tapers (fitted to Live's knob positions: Hold 10 ms and Release
/// 15 ms sit at ~0.3 and ~0.5 of the turn).
pub const GATE_HOLD_K: f32 = 3.0;
pub const GATE_RELEASE_K: f32 = 6.0;

fn gate_norm(g: &GateParams, k: u8) -> Option<f32> {
    Some(match k {
        gp::THRESHOLD => gate_db_norm(g.threshold),
        gp::RETURN => lin_norm(g.ret, 0.0, GATE_RETURN_MAX),
        gp::ATTACK => lin_norm(g.attack, GATE_ATTACK.0, GATE_ATTACK.1),
        gp::HOLD => pow_norm(g.hold, GATE_HOLD.0, GATE_HOLD.1, GATE_HOLD_K),
        gp::RELEASE => pow_norm(g.release, GATE_RELEASE.0, GATE_RELEASE.1, GATE_RELEASE_K),
        gp::FLOOR => lin_norm(g.floor, GATE_FLOOR_MIN, 0.0),
        _ => return sc_norm(&g.sc, k),
    })
}

fn gate_set(g: &mut GateParams, k: u8, v: f32) {
    match k {
        gp::THRESHOLD => g.threshold = gate_norm_db(v),
        gp::RETURN => g.ret = lin_val(v, 0.0, GATE_RETURN_MAX),
        gp::ATTACK => g.attack = lin_val(v, GATE_ATTACK.0, GATE_ATTACK.1),
        gp::HOLD => g.hold = pow_val(v, GATE_HOLD.0, GATE_HOLD.1, GATE_HOLD_K),
        gp::RELEASE => g.release = pow_val(v, GATE_RELEASE.0, GATE_RELEASE.1, GATE_RELEASE_K),
        gp::FLOOR => g.floor = lin_val(v, GATE_FLOOR_MIN, 0.0),
        _ => sc_set(&mut g.sc, k, v),
    }
}

/// Glue Compressor ids.
pub mod cp {
    pub const THRESHOLD: u8 = 0;
    pub const RATIO: u8 = 1;
    pub const ATTACK: u8 = 2;
    pub const RELEASE: u8 = 3;
    pub const MAKEUP: u8 = 4;
    pub const RANGE: u8 = 5;
    pub const DRY_WET: u8 = 6;
    pub const SOFT: u8 = 7;
    pub const OVERSAMPLE: u8 = 8;
}

fn glue_norm(g: &GlueParams, k: u8) -> Option<f32> {
    Some(match k {
        cp::THRESHOLD => lin_norm(g.threshold, GLUE_THRESH_MIN, 0.0),
        cp::RATIO => idx_norm(g.ratio, GLUE_RATIOS.len()),
        cp::ATTACK => idx_norm(g.attack, GLUE_ATTACKS.len()),
        cp::RELEASE => idx_norm(g.release, GLUE_RELEASES.len()),
        cp::MAKEUP => lin_norm(g.makeup, 0.0, GLUE_MAKEUP_MAX),
        cp::RANGE => lin_norm(g.range, 0.0, GLUE_RANGE_MAX),
        cp::DRY_WET => g.dry_wet,
        _ => return sc_norm(&g.sc, k),
    })
}

fn glue_set(g: &mut GlueParams, k: u8, v: f32) {
    match k {
        cp::THRESHOLD => g.threshold = lin_val(v, GLUE_THRESH_MIN, 0.0),
        cp::RATIO => g.ratio = norm_idx(v, GLUE_RATIOS.len()),
        cp::ATTACK => g.attack = norm_idx(v, GLUE_ATTACKS.len()),
        cp::RELEASE => g.release = norm_idx(v, GLUE_RELEASES.len()),
        cp::MAKEUP => g.makeup = lin_val(v, 0.0, GLUE_MAKEUP_MAX),
        cp::RANGE => g.range = lin_val(v, 0.0, GLUE_RANGE_MAX),
        cp::DRY_WET => g.dry_wet = v,
        _ => sc_set(&mut g.sc, k, v),
    }
}

/// Compressor ids (the sidechain uses [`sc`]).
pub mod cmp {
    pub const THRESHOLD: u8 = 0;
    pub const RATIO: u8 = 1;
    pub const ATTACK: u8 = 2;
    pub const RELEASE: u8 = 3;
    pub const AUTO: u8 = 4;
    pub const KNEE: u8 = 5;
    pub const LOOKAHEAD: u8 = 6;
    pub const ENV: u8 = 7;
    pub const MODE: u8 = 8;
    pub const MAKEUP: u8 = 9;
    pub const OUTPUT: u8 = 10;
    pub const DRY_WET: u8 = 11;
    pub const VIEW: u8 = 12;
    pub const SHOW_OUT: u8 = 13;
}

/// Ratio knob: 1 − 1/ratio (4 : 1 at 3/4 of the turn, ∞ at the end).
pub fn ratio_norm(r: f32) -> f32 {
    if r >= COMP_RATIO_INF { 1.0 } else { 1.0 - 1.0 / r.max(1.0) }
}

pub fn ratio_val(v: f32) -> f32 {
    if v >= 0.995 { COMP_RATIO_INF } else { (1.0 / (1.0 - v.clamp(0.0, 1.0))).min(COMP_RATIO_INF) }
}

fn comp_norm(c: &CompressorParams, k: u8) -> Option<f32> {
    Some(match k {
        cmp::THRESHOLD => lin_norm(c.threshold, COMP_THRESH.0, COMP_THRESH.1),
        cmp::RATIO => ratio_norm(c.ratio),
        cmp::ATTACK => log_norm(c.attack, COMP_ATTACK.0, COMP_ATTACK.1),
        cmp::RELEASE => log_norm(c.release, COMP_RELEASE.0, COMP_RELEASE.1),
        cmp::KNEE => lin_norm(c.knee, 0.0, COMP_KNEE_MAX),
        cmp::OUTPUT => lin_norm(c.output, -COMP_OUT_MAX, COMP_OUT_MAX),
        cmp::DRY_WET => c.dry_wet,
        _ => return sc_norm(&c.sc, k),
    })
}

fn comp_set(c: &mut CompressorParams, k: u8, v: f32) {
    match k {
        cmp::THRESHOLD => c.threshold = lin_val(v, COMP_THRESH.0, COMP_THRESH.1),
        cmp::RATIO => c.ratio = ratio_val(v),
        cmp::ATTACK => c.attack = log_val(v, COMP_ATTACK.0, COMP_ATTACK.1),
        cmp::RELEASE => c.release = log_val(v, COMP_RELEASE.0, COMP_RELEASE.1),
        cmp::KNEE => c.knee = lin_val(v, 0.0, COMP_KNEE_MAX),
        cmp::OUTPUT => c.output = lin_val(v, -COMP_OUT_MAX, COMP_OUT_MAX),
        cmp::DRY_WET => c.dry_wet = v,
        _ => sc_set(&mut c.sc, k, v),
    }
}

/// Spectrum ids.
/// Auto Shift: continuous parameters 0..27, switches 60..85 (clear of `sc`).
pub mod asp {
    pub const IN_GAIN: u8 = 0;
    pub const GLIDE: u8 = 1;
    pub const STRENGTH: u8 = 2;
    pub const SMOOTH_MS: u8 = 3;
    pub const SHIFT: u8 = 4;
    pub const ATTACK: u8 = 5;
    pub const RELEASE: u8 = 6;
    pub const PB_RANGE: u8 = 7;
    /// + target (Pitch, Formant, Volume, Pan).
    pub const MOD_AMT: u8 = 8;
    pub const LFO_DELAY: u8 = 12;
    pub const LFO_FADE: u8 = 13;
    pub const LFO_RATE: u8 = 14;
    pub const LFO_DIV: u8 = 15;
    /// + target.
    pub const LFO_AMT: u8 = 16;
    pub const PITCH: u8 = 20;
    pub const FINE: u8 = 21;
    pub const FORMANT: u8 = 22;
    pub const F_FOLLOW: u8 = 23;
    pub const VIB_AMT: u8 = 24;
    pub const VIB_RATE: u8 = 25;
    pub const VIB_FADE: u8 = 26;
    pub const DRY_WET: u8 = 27;
    pub const TAB: u8 = 60;
    pub const MIDI_OPEN: u8 = 61;
    pub const LIVE: u8 = 62;
    pub const RANGE: u8 = 63;
    pub const MIDI_ON: u8 = 64;
    pub const SOURCE: u8 = 65;
    pub const TAP: u8 = 66;
    pub const POLY: u8 = 67;
    pub const VOICES: u8 = 68;
    pub const QUANT: u8 = 69;
    pub const SMOOTH: u8 = 70;
    pub const ROOT: u8 = 71;
    pub const SCALE: u8 = 72;
    pub const KEY: u8 = 73;
    pub const USE_CUR: u8 = 74;
    pub const LATCH: u8 = 75;
    /// + target.
    pub const MOD_SRC: u8 = 76;
    pub const LFO_ON: u8 = 80;
    pub const LFO_RESET: u8 = 81;
    pub const LFO_WAVE: u8 = 82;
    pub const LFO_SYNC: u8 = 83;
    pub const VIB_ON: u8 = 84;
    pub const NATURAL: u8 = 85;
}

fn as_norm(a: &crate::autoshift::AutoShiftParams, k: u8) -> Option<f32> {
    use crate::autoshift::*;
    let sq = |v: f32, max: f32| (v / max).clamp(0.0, 1.0).sqrt();
    Some(
        match k {
            asp::IN_GAIN => (a.in_gain + AS_GAIN_DB) / (2.0 * AS_GAIN_DB),
            asp::GLIDE => sq(a.glide, AS_GLIDE_MS),
            asp::STRENGTH => a.strength,
            asp::SMOOTH_MS => a.smooth_ms / AS_SMOOTH_MS,
            asp::SHIFT => (a.shift as f32 + AS_SHIFT as f32) / (2.0 * AS_SHIFT as f32),
            asp::ATTACK => sq(a.attack, AS_ATTACK_MS),
            asp::RELEASE => sq(a.release, AS_RELEASE_MS),
            asp::PB_RANGE => a.pb_range as f32 / AS_PB_MAX as f32,
            8..=11 => (a.mod_amt[(k - asp::MOD_AMT) as usize] + 1.0) / 2.0,
            asp::LFO_DELAY => sq(a.lfo_delay, AS_LFO_DELAY_MS),
            asp::LFO_FADE => sq(a.lfo_fade, AS_LFO_FADE_MS),
            asp::LFO_RATE => (a.lfo_rate / AS_LFO_HZ.0).ln() / (AS_LFO_HZ.1 / AS_LFO_HZ.0).ln(),
            asp::LFO_DIV => a.lfo_div as f32 / (crate::modfx::SYNC_DIVS.len() - 1) as f32,
            16..=19 => (a.lfo_amt[(k - asp::LFO_AMT) as usize] + 1.0) / 2.0,
            asp::PITCH => (a.pitch + AS_PITCH_ST) / (2.0 * AS_PITCH_ST),
            asp::FINE => (a.fine + AS_FINE_CT) / (2.0 * AS_FINE_CT),
            asp::FORMANT => (a.formant + 1.0) / 2.0,
            asp::F_FOLLOW => a.f_follow,
            asp::VIB_AMT => a.vib_amt / AS_VIB_CT,
            asp::VIB_RATE => (a.vib_rate / AS_VIB_HZ.0).ln() / (AS_VIB_HZ.1 / AS_VIB_HZ.0).ln(),
            asp::VIB_FADE => sq(a.vib_fade, AS_VIB_FADE_MS),
            asp::DRY_WET => a.dry_wet,
            _ => return None,
        }
        .clamp(0.0, 1.0),
    )
}

fn as_set(a: &mut crate::autoshift::AutoShiftParams, k: u8, v: f32) {
    use crate::autoshift::*;
    match k {
        asp::IN_GAIN => a.in_gain = ((v * 2.0 - 1.0) * AS_GAIN_DB * 10.0).round() / 10.0,
        asp::GLIDE => a.glide = v * v * AS_GLIDE_MS,
        asp::STRENGTH => a.strength = v,
        asp::SMOOTH_MS => a.smooth_ms = v * AS_SMOOTH_MS,
        asp::SHIFT => a.shift = ((v * 2.0 - 1.0) * AS_SHIFT as f32).round() as i8,
        asp::ATTACK => a.attack = v * v * AS_ATTACK_MS,
        asp::RELEASE => a.release = v * v * AS_RELEASE_MS,
        asp::PB_RANGE => a.pb_range = (v * AS_PB_MAX as f32).round() as u8,
        8..=11 => a.mod_amt[(k - asp::MOD_AMT) as usize] = v * 2.0 - 1.0,
        asp::LFO_DELAY => a.lfo_delay = v * v * AS_LFO_DELAY_MS,
        asp::LFO_FADE => a.lfo_fade = v * v * AS_LFO_FADE_MS,
        asp::LFO_RATE => a.lfo_rate = AS_LFO_HZ.0 * (AS_LFO_HZ.1 / AS_LFO_HZ.0).powf(v),
        asp::LFO_DIV => a.lfo_div = (v * (crate::modfx::SYNC_DIVS.len() - 1) as f32).round() as u8,
        16..=19 => a.lfo_amt[(k - asp::LFO_AMT) as usize] = v * 2.0 - 1.0,
        asp::PITCH => a.pitch = ((v * 2.0 - 1.0) * AS_PITCH_ST).round(),
        asp::FINE => a.fine = (v * 2.0 - 1.0) * AS_FINE_CT,
        asp::FORMANT => a.formant = v * 2.0 - 1.0,
        asp::F_FOLLOW => a.f_follow = v,
        asp::VIB_AMT => a.vib_amt = v * AS_VIB_CT,
        asp::VIB_RATE => a.vib_rate = AS_VIB_HZ.0 * (AS_VIB_HZ.1 / AS_VIB_HZ.0).powf(v),
        asp::VIB_FADE => a.vib_fade = v * v * AS_VIB_FADE_MS,
        asp::DRY_WET => a.dry_wet = v,
        _ => {}
    }
}

pub mod spp {
    pub const BLOCK: u8 = 0;
    pub const CHANNEL: u8 = 1;
    pub const REFRESH: u8 = 2;
    pub const AVG: u8 = 3;
    pub const LINE: u8 = 4;
    pub const MAX: u8 = 5;
    pub const SCALE: u8 = 6;
    pub const AUTO: u8 = 7;
    pub const HI: u8 = 8;
    pub const LO: u8 = 9;
}

fn spec_norm(s: &crate::spectrum::SpectrumParams, k: u8) -> Option<f32> {
    use crate::spectrum::*;
    let bi = SPEC_BLOCKS.iter().position(|&b| b == s.block).unwrap_or(5) as u8;
    Some(match k {
        spp::BLOCK => idx_norm(bi, SPEC_BLOCKS.len()),
        spp::CHANNEL => idx_norm(s.channel as u8, 3),
        spp::REFRESH => lin_norm(s.refresh, SPEC_REFRESH.0, SPEC_REFRESH.1),
        spp::AVG => lin_norm(s.avg, 1.0, SPEC_AVG_MAX),
        spp::LINE => s.line as u8 as f32,
        spp::MAX => s.max as u8 as f32,
        spp::SCALE => idx_norm(s.scale as u8, 3),
        spp::AUTO => s.auto as u8 as f32,
        spp::HI => lin_norm(s.hi, SPEC_DB.0, SPEC_DB.1),
        spp::LO => lin_norm(s.lo, SPEC_DB.0, SPEC_DB.1),
        _ => return None,
    })
}

fn spec_set(s: &mut crate::spectrum::SpectrumParams, k: u8, v: f32) {
    use crate::spectrum::*;
    match k {
        spp::BLOCK => s.block = SPEC_BLOCKS[norm_idx(v, SPEC_BLOCKS.len()) as usize],
        spp::CHANNEL => s.channel = [SpecChannel::L, SpecChannel::R, SpecChannel::LR][norm_idx(v, 3) as usize],
        spp::REFRESH => s.refresh = lin_val(v, SPEC_REFRESH.0, SPEC_REFRESH.1),
        spp::AVG => s.avg = lin_val(v, 1.0, SPEC_AVG_MAX).round(),
        spp::LINE => s.line = v >= 0.5,
        spp::MAX => s.max = v >= 0.5,
        spp::SCALE => s.scale = [SpecScale::Lin, SpecScale::Log, SpecScale::St][norm_idx(v, 3) as usize],
        spp::AUTO => s.auto = v >= 0.5,
        spp::HI => s.hi = lin_val(v, SPEC_DB.0, SPEC_DB.1).max(s.lo + 1.0),
        spp::LO => s.lo = lin_val(v, SPEC_DB.0, SPEC_DB.1).min(s.hi - 1.0),
        _ => {}
    }
}

/// Limiter ids (Input/Output and Ceiling/Threshold swap with Maximize).
pub mod lp {
    pub const INPUT: u8 = 0;
    pub const MAXIMIZE: u8 = 1;
    pub const CEILING: u8 = 2;
    pub const RELEASE: u8 = 3;
    pub const AUTO: u8 = 4;
    pub const LOOKAHEAD: u8 = 5;
    pub const MODE: u8 = 6;
    pub const ROUTING: u8 = 7;
    pub const LINK: u8 = 8;
    pub const OUTPUT: u8 = 9;
    pub const THRESHOLD: u8 = 10;
    /// The GR display (click resets the held maximum).
    pub const GR: u8 = 11;
}

fn lim_norm(l: &LimiterParams, k: u8) -> Option<f32> {
    Some(match k {
        lp::INPUT => lin_norm(l.input, LIM_GAIN.0, LIM_GAIN.1),
        lp::OUTPUT => lin_norm(l.output, LIM_CEIL_MIN, 0.0),
        lp::CEILING => lin_norm(l.ceiling, LIM_CEIL_MIN, 0.0),
        lp::THRESHOLD => lin_norm(l.threshold, LIM_CEIL_MIN, 0.0),
        lp::RELEASE => log_norm(l.release, LIM_RELEASE.0, LIM_RELEASE.1),
        lp::LOOKAHEAD => idx_norm(l.lookahead, LIM_LOOKAHEAD.len()),
        lp::LINK => l.link,
        _ => return None,
    })
}

fn lim_set(l: &mut LimiterParams, k: u8, v: f32) {
    match k {
        lp::INPUT => l.input = lin_val(v, LIM_GAIN.0, LIM_GAIN.1),
        lp::OUTPUT => l.output = lin_val(v, LIM_CEIL_MIN, 0.0),
        lp::CEILING => l.ceiling = lin_val(v, LIM_CEIL_MIN, 0.0),
        lp::THRESHOLD => l.threshold = lin_val(v, LIM_CEIL_MIN, 0.0),
        lp::RELEASE => l.release = log_val(v, LIM_RELEASE.0, LIM_RELEASE.1),
        lp::LOOKAHEAD => l.lookahead = norm_idx(v, LIM_LOOKAHEAD.len()),
        lp::LINK => l.link = v,
        _ => {}
    }
}

/// LFO ids; map `m` fields are `MAP + m * 8 + field`.
pub mod fp {
    pub const WAVE: u8 = 0;
    pub const SHAPE: u8 = 1;
    pub const STEPS: u8 = 2;
    pub const JITTER: u8 = 3;
    pub const SMOOTH: u8 = 4;
    pub const RATE: u8 = 5;
    pub const SYNC_RATE: u8 = 6;
    pub const SYNC: u8 = 7;
    pub const X10: u8 = 8;
    pub const DEPTH: u8 = 9;
    pub const OFFSET: u8 = 10;
    pub const PHASE: u8 = 11;
    pub const HOLD: u8 = 12;
    pub const RETRIG: u8 = 13;
    pub const MULTIMAP: u8 = 14;
    /// Shaper.
    pub const GRID: u8 = 15;
    pub const SNAP: u8 = 16;
    pub const TRIG: u8 = 17;
    pub const POINTS: u8 = 18;
    pub const PRESET: u8 = 19;
    pub const MAP: u8 = 20;
    pub const M_AMOUNT: u8 = 0;
    pub const M_MIN: u8 = 1;
    pub const M_MAX: u8 = 2;
    pub const M_REMOTE: u8 = 3;
    pub const M_POLARITY: u8 = 4;
    pub const M_MAP: u8 = 5;
    pub const M_UNMAP: u8 = 6;
    pub fn map(m: usize, field: u8) -> u8 {
        MAP + (m * 8) as u8 + field
    }
    pub fn split(k: u8) -> Option<(usize, u8)> {
        (MAP..MAP + 64).contains(&k).then(|| (((k - MAP) / 8) as usize, (k - MAP) % 8))
    }
}

/// LFO rate taper (1 Hz at ~0.16 of the turn, as in Live).
pub const LFO_RATE_K: f32 = 2.0;

fn lfo_norm(l: &LfoParams, k: u8) -> Option<f32> {
    if let Some((m, f)) = fp::split(k) {
        let m = &l.maps[m];
        return match f {
            fp::M_AMOUNT => Some(m.amount),
            fp::M_MIN => Some(m.min),
            fp::M_MAX => Some(m.max),
            _ => None,
        };
    }
    Some(match k {
        fp::SHAPE => (l.shape + 1.0) / 2.0,
        fp::STEPS => l.steps as f32 / LFO_STEPS_MAX as f32,
        fp::JITTER => l.jitter,
        fp::SMOOTH => l.smooth,
        fp::RATE => pow_norm(l.rate, LFO_RATE.0, LFO_RATE.1, LFO_RATE_K),
        fp::SYNC_RATE => idx_norm(l.sync_rate, LFO_SYNC.len()),
        fp::DEPTH => l.depth,
        fp::OFFSET => (l.offset + 1.0) / 2.0,
        fp::PHASE => l.phase,
        fp::GRID if l.shaper => (l.grid.clamp(1, SH_GRID_MAX) - 1) as f32 / (SH_GRID_MAX - 1) as f32,
        _ => return None,
    })
}

fn lfo_set(l: &mut LfoParams, k: u8, v: f32) {
    if let Some((m, f)) = fp::split(k) {
        let m = &mut l.maps[m];
        match f {
            fp::M_AMOUNT => m.amount = v,
            fp::M_MIN => m.min = v,
            fp::M_MAX => m.max = v,
            _ => {}
        }
        return;
    }
    match k {
        fp::SHAPE => l.shape = v * 2.0 - 1.0,
        // 1 step is meaningless: Off below 2.
        fp::STEPS => {
            let s = (v * LFO_STEPS_MAX as f32).round() as u8;
            l.steps = if s < 2 { 0 } else { s };
        }
        fp::JITTER => l.jitter = v,
        fp::SMOOTH => l.smooth = v,
        fp::RATE => l.rate = pow_val(v, LFO_RATE.0, LFO_RATE.1, LFO_RATE_K),
        fp::SYNC_RATE => l.sync_rate = norm_idx(v, LFO_SYNC.len()),
        fp::DEPTH => l.depth = v,
        fp::OFFSET => l.offset = v * 2.0 - 1.0,
        fp::PHASE => l.phase = v,
        fp::GRID if l.shaper => l.grid = 1 + (v * (SH_GRID_MAX - 1) as f32).round() as u8,
        _ => {}
    }
}

// ----------------------------------------------------------------------------
// EQ Three, Chorus-Ensemble, Auto Filter, Auto Pan-Tremolo.
// ----------------------------------------------------------------------------

/// EQ Three ids.
pub mod e3p {
    pub const GAIN: [u8; 3] = [0, 1, 2];
    pub const ON: [u8; 3] = [3, 4, 5];
    pub const FREQ_LO: u8 = 6;
    pub const FREQ_HI: u8 = 7;
    pub const SLOPE: u8 = 8;
}

/// EQ Three gain knob: amplitude taper, 0 dB at 0.865 of the turn (Live).
pub const EQ3_TAPER: f32 = 0.209;

pub fn eq3_gain_norm(db: f32) -> f32 {
    if db <= EQ3_GAIN_MIN { 0.0 } else { (db_lin(db.min(EQ3_GAIN_MAX)) / db_lin(EQ3_GAIN_MAX)).powf(EQ3_TAPER) }
}

pub fn eq3_norm_gain(v: f32) -> f32 {
    if v <= 1e-4 {
        return EQ3_GAIN_MIN;
    }
    let a = v.min(1.0).powf(1.0 / EQ3_TAPER) * db_lin(EQ3_GAIN_MAX);
    lin_db(a).clamp(EQ3_GAIN_MIN, EQ3_GAIN_MAX)
}

fn eq3_norm(e: &EqThreeParams, k: u8) -> Option<f32> {
    Some(match k {
        0..=2 => eq3_gain_norm(e.gain[k as usize]),
        e3p::FREQ_LO => log_norm(e.freq_lo, EQ3_LO.0, EQ3_LO.1),
        e3p::FREQ_HI => log_norm(e.freq_hi, EQ3_HI.0, EQ3_HI.1),
        _ => return None,
    })
}

fn eq3_set(e: &mut EqThreeParams, k: u8, v: f32) {
    match k {
        0..=2 => e.gain[k as usize] = eq3_norm_gain(v),
        e3p::FREQ_LO => e.freq_lo = log_val(v, EQ3_LO.0, EQ3_LO.1),
        e3p::FREQ_HI => e.freq_hi = log_val(v, EQ3_HI.0, EQ3_HI.1),
        _ => {}
    }
}

/// Chorus-Ensemble ids.
pub mod chp {
    pub const MODE: u8 = 0;
    pub const HP_ON: u8 = 1;
    pub const HP_FREQ: u8 = 2;
    pub const WIDTH: u8 = 3;
    pub const TIME: u8 = 4;
    pub const TAPS: u8 = 5;
    pub const OFFSET: u8 = 6;
    pub const SHAPE: u8 = 7;
    pub const RATE: u8 = 8;
    pub const AMOUNT: u8 = 9;
    pub const FEEDBACK: u8 = 10;
    pub const FB_INV: u8 = 11;
    pub const OUTPUT: u8 = 12;
    pub const WARMTH: u8 = 13;
    pub const DRY_WET: u8 = 14;
}

/// Rate knob tapers fitted to Live (Chorus 0.90 Hz at 0.413, LFOs 1 Hz at 0.372).
pub const CHO_RATE_K: f32 = 3.52;
pub const LFO_HZ_K: f32 = 3.75;

fn cho_norm(c: &ChorusParams, k: u8) -> Option<f32> {
    Some(match k {
        chp::HP_FREQ => log_norm(c.hp_freq, CHO_HP.0, CHO_HP.1),
        chp::WIDTH => c.width / 2.0,
        chp::OFFSET => c.offset / 180.0,
        chp::SHAPE => c.shape,
        chp::RATE => pow_norm(c.rate, CHO_RATE.0, CHO_RATE.1, CHO_RATE_K),
        chp::AMOUNT => c.amount,
        chp::FEEDBACK => c.feedback / CHO_FB_MAX,
        chp::OUTPUT => lin_norm(c.output, CHO_OUT.0, CHO_OUT.1),
        chp::WARMTH => c.warmth,
        chp::DRY_WET => c.dry_wet,
        _ => return None,
    })
}

fn cho_set(c: &mut ChorusParams, k: u8, v: f32) {
    match k {
        chp::HP_FREQ => c.hp_freq = log_val(v, CHO_HP.0, CHO_HP.1),
        chp::WIDTH => c.width = v * 2.0,
        chp::OFFSET => c.offset = v * 180.0,
        chp::SHAPE => c.shape = v,
        chp::RATE => c.rate = pow_val(v, CHO_RATE.0, CHO_RATE.1, CHO_RATE_K),
        chp::AMOUNT => c.amount = v,
        chp::FEEDBACK => c.feedback = v * CHO_FB_MAX,
        chp::OUTPUT => c.output = lin_val(v, CHO_OUT.0, CHO_OUT.1),
        chp::WARMTH => c.warmth = v,
        chp::DRY_WET => c.dry_wet = v,
        _ => {}
    }
}

/// Rate knob of a [`Rate`] (mode-dependent; synced modes are stepped).
pub fn rate_norm(r: &Rate) -> f32 {
    match r.mode {
        TimeMode::Rate => pow_norm(r.hz, LFO_HZ.0, LFO_HZ.1, LFO_HZ_K),
        TimeMode::Time => log_norm(r.ms, LFO_MS.0, LFO_MS.1),
        TimeMode::Sixteenth => (r.sixteenth.clamp(1, SIXTEENTH_MAX) as f32 - 1.0) / (SIXTEENTH_MAX as f32 - 1.0),
        _ => idx_norm(r.div, SYNC_DIVS.len()),
    }
}

pub fn rate_set(r: &mut Rate, v: f32) {
    match r.mode {
        TimeMode::Rate => r.hz = pow_val(v, LFO_HZ.0, LFO_HZ.1, LFO_HZ_K),
        TimeMode::Time => r.ms = log_val(v, LFO_MS.0, LFO_MS.1),
        TimeMode::Sixteenth => r.sixteenth = 1 + (v.clamp(0.0, 1.0) * (SIXTEENTH_MAX as f32 - 1.0)).round() as u8,
        _ => r.div = norm_idx(v, SYNC_DIVS.len()),
    }
}

/// Auto Pan-Tremolo ids.
pub mod pnp {
    pub const MODE: u8 = 0;
    pub const WAVE: u8 = 1;
    pub const TIME_MODE: u8 = 2;
    pub const RATE: u8 = 3;
    pub const STEREO: u8 = 4;
    /// Phase (Phase mode) or Spin (Spin mode).
    pub const PHASE: u8 = 5;
    pub const OFFSET: u8 = 6;
    pub const INVERT: u8 = 7;
    pub const HARMONIC: u8 = 8;
    pub const VINTAGE: u8 = 9;
    pub const AMOUNT: u8 = 10;
    pub const SHAPE: u8 = 11;
    pub const ATTACK: u8 = 12;
    pub const DYN_MOD: u8 = 13;
}

fn pan_norm(a: &AutoPanParams, k: u8) -> Option<f32> {
    Some(match k {
        pnp::RATE => rate_norm(&a.rate),
        pnp::PHASE if a.spin_mode => a.spin,
        pnp::PHASE => a.phase / 360.0,
        pnp::OFFSET => a.offset / 360.0,
        pnp::AMOUNT => a.amount,
        pnp::SHAPE if a.tremolo => 0.5 + 0.5 * a.shape_trem,
        pnp::SHAPE => a.shape_pan,
        pnp::ATTACK => pow_norm(a.attack, 0.0, PAN_ATTACK_MAX, 4.0),
        pnp::DYN_MOD => 0.5 + 0.5 * a.dyn_mod,
        _ => return None,
    })
}

fn pan_set(a: &mut AutoPanParams, k: u8, v: f32) {
    match k {
        pnp::RATE => rate_set(&mut a.rate, v),
        pnp::PHASE if a.spin_mode => a.spin = v,
        pnp::PHASE => a.phase = v * 360.0,
        pnp::OFFSET => a.offset = v * 360.0,
        pnp::AMOUNT => a.amount = v,
        pnp::SHAPE if a.tremolo => a.shape_trem = v * 2.0 - 1.0,
        pnp::SHAPE => a.shape_pan = v,
        pnp::ATTACK => a.attack = pow_val(v, 0.0, PAN_ATTACK_MAX, 4.0),
        pnp::DYN_MOD => a.dyn_mod = v * 2.0 - 1.0,
        _ => {}
    }
}

/// Auto Filter ids (sidechain: the shared `sc` ids).
pub mod afp {
    pub const TYPE: u8 = 0;
    pub const SLOPE: u8 = 1;
    pub const CIRCUIT: u8 = 2;
    /// Freq (DJ: Control, Vowel: Pitch).
    pub const FREQ: u8 = 3;
    /// Res (Vowel: Formant).
    pub const RES: u8 = 4;
    pub const MORPH: u8 = 5;
    pub const DRIVE: u8 = 6;
    pub const CLIP: u8 = 7;
    pub const OUTPUT: u8 = 8;
    pub const DRY_WET: u8 = 9;
    pub const LFO_AMT: u8 = 10;
    pub const RATE: u8 = 11;
    pub const WAVE: u8 = 12;
    pub const LFO_MORPH: u8 = 13;
    pub const TIME_MODE: u8 = 14;
    pub const STEREO: u8 = 15;
    /// Phase or Spin.
    pub const PHASE: u8 = 16;
    pub const OFFSET: u8 = 17;
    pub const QUANT: u8 = 18;
    /// Steps count or S&H rate.
    pub const STEPS: u8 = 19;
    pub const ENV_AMT: u8 = 20;
    pub const ATTACK: u8 = 21;
    pub const HOLD: u8 = 22;
    pub const RELEASE: u8 = 23;
    pub const ENV_SH: u8 = 24;
    pub const ENV_DIV: u8 = 25;
    pub const SECTION: u8 = 26;
}

fn af_norm(a: &AutoFilterParams, k: u8) -> Option<f32> {
    Some(match k {
        afp::FREQ => match a.kind {
            FilterType::Dj => 0.5 + 0.5 * a.dj,
            FilterType::Vowel => lin_norm(a.pitch, AF_PITCH.0, AF_PITCH.1),
            _ => log_norm(a.freq, AF_FREQ.0, AF_FREQ.1),
        },
        afp::RES => a.res,
        afp::MORPH => a.morph,
        afp::DRIVE => a.drive,
        afp::OUTPUT => lin_norm(a.output, AF_OUT.0, AF_OUT.1),
        afp::DRY_WET => a.dry_wet,
        afp::LFO_AMT => a.lfo_amt,
        afp::RATE => rate_norm(&a.rate),
        afp::LFO_MORPH if a.wave == AfWave::SampleHold => a.lfo_morph.max(0.0),
        afp::LFO_MORPH => 0.5 + 0.5 * a.lfo_morph,
        afp::PHASE if a.spin_mode => a.spin,
        afp::PHASE => a.phase / 360.0,
        afp::OFFSET => a.offset / 360.0,
        afp::STEPS if a.quantize == Quantize::SampleHold => idx_norm(a.q_div, SYNC_DIVS.len()),
        afp::STEPS => (a.steps.clamp(AF_STEPS.0, AF_STEPS.1) - AF_STEPS.0) as f32 / (AF_STEPS.1 - AF_STEPS.0) as f32,
        afp::ENV_AMT => 0.5 + 0.5 * a.env_amt,
        afp::ATTACK => log_norm(a.attack, AF_ATTACK.0, AF_ATTACK.1),
        afp::RELEASE => log_norm(a.release, AF_RELEASE.0, AF_RELEASE.1),
        afp::ENV_DIV => idx_norm(a.env_div, SYNC_DIVS.len()),
        _ => return sc_norm(&a.sc, k),
    })
}

fn af_set(a: &mut AutoFilterParams, k: u8, v: f32) {
    match k {
        afp::FREQ => match a.kind {
            FilterType::Dj => a.dj = v * 2.0 - 1.0,
            FilterType::Vowel => a.pitch = lin_val(v, AF_PITCH.0, AF_PITCH.1),
            _ => a.freq = log_val(v, AF_FREQ.0, AF_FREQ.1),
        },
        afp::RES => a.res = v,
        afp::MORPH => a.morph = v,
        afp::DRIVE => a.drive = v,
        afp::OUTPUT => a.output = lin_val(v, AF_OUT.0, AF_OUT.1),
        afp::DRY_WET => a.dry_wet = v,
        afp::LFO_AMT => a.lfo_amt = v,
        afp::RATE => rate_set(&mut a.rate, v),
        afp::LFO_MORPH if a.wave == AfWave::SampleHold => a.lfo_morph = v,
        afp::LFO_MORPH => a.lfo_morph = v * 2.0 - 1.0,
        afp::PHASE if a.spin_mode => a.spin = v,
        afp::PHASE => a.phase = v * 360.0,
        afp::OFFSET => a.offset = v * 360.0,
        afp::STEPS if a.quantize == Quantize::SampleHold => a.q_div = norm_idx(v, SYNC_DIVS.len()),
        afp::STEPS => a.steps = AF_STEPS.0 + (v.clamp(0.0, 1.0) * (AF_STEPS.1 - AF_STEPS.0) as f32).round() as u8,
        afp::ENV_AMT => a.env_amt = v * 2.0 - 1.0,
        afp::ATTACK => a.attack = log_val(v, AF_ATTACK.0, AF_ATTACK.1),
        afp::RELEASE => a.release = log_val(v, AF_RELEASE.0, AF_RELEASE.1),
        afp::ENV_DIV => a.env_div = norm_idx(v, SYNC_DIVS.len()),
        _ => sc_set(&mut a.sc, k, v),
    }
}

/// Whether an LFO may map this parameter (continuous device parameters,
/// not rack macros/selector and not other LFOs).
pub fn mappable(p: &FxParams, k: u8) -> bool {
    !matches!(p, FxParams::Rack(_) | FxParams::Lfo(_)) && norm(p, k).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let all = [
            FxParams::Gate(GateParams::default()),
            FxParams::Glue(GlueParams::default()),
            FxParams::Limiter(LimiterParams::default()),
            FxParams::Lfo(LfoParams::default()),
            FxParams::Lfo(LfoParams::shaper()),
            FxParams::EqThree(EqThreeParams::default()),
            FxParams::Chorus(ChorusParams::default()),
            FxParams::AutoFilter(AutoFilterParams::default()),
            FxParams::AutoPan(AutoPanParams::default()),
            FxParams::Compressor(CompressorParams::default()),
            FxParams::Spectrum(crate::spectrum::SpectrumParams::default()),
            FxParams::AutoShift(crate::autoshift::AutoShiftParams::default()),
            FxParams::ChannelEq(crate::chaneq::ChannelEqParams::default()),
            FxParams::Echo(crate::echo::EchoParams::default()),
            FxParams::Echo(crate::echo::EchoParams { sync: [false; 2], link: false, mod_sync: true, ..Default::default() }),
            FxParams::Echo(crate::echo::EchoParams { sync_mode: [3, 3], link: false, ..Default::default() }),
            FxParams::Erosion(crate::erosion::ErosionParams::default()),
            FxParams::Reverb(crate::reverb::ReverbParams::default()),
            FxParams::Redux(crate::redux::ReduxParams::default()),
            FxParams::Saturator(crate::saturator::SaturatorParams::default()),
            FxParams::Delay(crate::delay::DelayParams::default()),
            FxParams::Delay(crate::delay::DelayParams { sync: [false; 2], link: false, lfo_mode: 3, ..Default::default() }),
            FxParams::Delay(crate::delay::DelayParams { lfo_mode: 5, link: false, ..Default::default() }),
            FxParams::GrainDelay(crate::graindelay::GrainDelayParams::default()),
            FxParams::GrainDelay(crate::graindelay::GrainDelayParams { sync: false, ..Default::default() }),
            FxParams::Resonators(crate::resonators::ResonatorsParams::default()),
            FxParams::DrumBuss(crate::drumbuss::DrumBussParams::default()),
            FxParams::SpectralResonator(crate::specres::SpecResParams::default()),
            FxParams::SpectralResonator(crate::specres::SpecResParams { note_mode: true, ..Default::default() }),
            FxParams::SpectralTime(crate::spectime::SpecTimeParams::default()),
            FxParams::SpectralTime(crate::spectime::SpecTimeParams { time_mode: 1, ..Default::default() }),
            FxParams::SpectralTime(crate::spectime::SpecTimeParams { time_mode: 3, ..Default::default() }),
            FxParams::DynamicTube(crate::dyntube::DynTubeParams::default()),
            FxParams::Vocoder(crate::vocoder::VocoderParams::default()),
            FxParams::VinylDistortion(crate::vinyl::VinylParams::default()),
            FxParams::Shifter(crate::shifter::ShifterParams::default()),
            FxParams::Shifter(crate::shifter::ShifterParams { coarse_hz: 120.0, mode: 1, ..Default::default() }),
            FxParams::AutoPan(AutoPanParams { tremolo: true, ..Default::default() }),
            FxParams::AutoFilter(AutoFilterParams { kind: FilterType::Dj, ..Default::default() }),
            FxParams::AutoFilter(AutoFilterParams { kind: FilterType::Vowel, ..Default::default() }),
        ];
        for p in all {
            for k in 0..=120u8 {
                if let Some(n) = norm(&p, k) {
                    let mut q = p;
                    set_norm(&mut q, k, n);
                    let n2 = norm(&q, k).unwrap();
                    assert!((n - n2).abs() < 1e-3, "{} {k}: {n} vs {n2}", p.name());
                }
            }
        }
        assert!((gate_db_norm(-12.0) - 53.0 / 98.0).abs() < 1e-6);
        assert!((gate_norm_db(gate_db_norm(-18.1)) + 18.1).abs() < 1e-4);
    }
}
