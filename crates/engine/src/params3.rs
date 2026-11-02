//! Normalised parameter access for Redux, Saturator, Delay and Grain Delay
//! (ids: `rdp`, `sap`, `dlp`, `gdp`; switches have ids ≥ 40 and no norm).

use crate::delay::*;
use crate::echo::ECHO_MOD_DIVS;
use crate::graindelay::*;
use crate::params::{idx_norm, lin_norm, lin_val, log_norm, log_val, norm_idx};
use crate::redux::*;
use crate::saturator::*;

// ---- Redux ----

pub mod rdp {
    pub const RATE: u8 = 0;
    pub const JITTER: u8 = 1;
    pub const POST_OCT: u8 = 2;
    pub const BITS: u8 = 3;
    pub const SHAPE: u8 = 4;
    pub const DRY_WET: u8 = 5;
    pub const PRE: u8 = 40;
    pub const POST: u8 = 41;
    pub const DC: u8 = 42;
}

pub(crate) fn rdx_norm(p: &ReduxParams, k: u8) -> Option<f32> {
    Some(match k {
        rdp::RATE => log_norm(p.rate, RDX_RATE.0, RDX_RATE.1),
        rdp::JITTER => p.jitter.clamp(0.0, 1.0),
        rdp::POST_OCT => lin_norm(p.post_oct, -RDX_OCT, RDX_OCT),
        rdp::BITS => lin_norm(p.bits, RDX_BITS.0, RDX_BITS.1),
        rdp::SHAPE => p.shape.clamp(0.0, 1.0),
        rdp::DRY_WET => p.dry_wet.clamp(0.0, 1.0),
        _ => return None,
    })
}

pub(crate) fn rdx_set(p: &mut ReduxParams, k: u8, v: f32) {
    match k {
        rdp::RATE => p.rate = log_val(v, RDX_RATE.0, RDX_RATE.1),
        rdp::JITTER => p.jitter = v,
        rdp::POST_OCT => p.post_oct = lin_val(v, -RDX_OCT, RDX_OCT),
        rdp::BITS => p.bits = lin_val(v, RDX_BITS.0, RDX_BITS.1),
        rdp::SHAPE => p.shape = v,
        rdp::DRY_WET => p.dry_wet = v,
        _ => {}
    }
}

// ---- Saturator ----

pub mod sap {
    pub const DRIVE: u8 = 0;
    pub const OUTPUT: u8 = 1;
    pub const DRY_WET: u8 = 2;
    pub const AMT_LO: u8 = 3;
    pub const AMT_HI: u8 = 4;
    pub const FREQ: u8 = 5;
    pub const WIDTH: u8 = 6;
    pub const BASS: u8 = 7;
    pub const WS_DRIVE: u8 = 8;
    pub const WS_CURVE: u8 = 9;
    pub const WS_DEPTH: u8 = 10;
    pub const WS_LIN: u8 = 11;
    pub const WS_DAMP: u8 = 12;
    pub const WS_PERIOD: u8 = 13;
    pub const CURVE: u8 = 40;
    pub const CLIP: u8 = 41;
    pub const COLOR: u8 = 42;
    pub const HQ: u8 = 43;
    pub const PRE_DC: u8 = 44;
    pub const EXPAND: u8 = 45;
}

pub(crate) fn sat_norm(p: &SaturatorParams, k: u8) -> Option<f32> {
    Some(match k {
        sap::DRIVE => lin_norm(p.drive, SAT_DRIVE.0, SAT_DRIVE.1),
        sap::OUTPUT => lin_norm(p.output, SAT_OUT.0, SAT_OUT.1),
        sap::DRY_WET => p.dry_wet.clamp(0.0, 1.0),
        sap::AMT_LO => lin_norm(p.amt_lo, -1.0, 1.0),
        sap::AMT_HI => lin_norm(p.amt_hi, -1.0, 1.0),
        sap::FREQ => log_norm(p.freq, SAT_FREQ.0, SAT_FREQ.1),
        sap::WIDTH => p.width.clamp(0.0, 1.0),
        sap::BASS => lin_norm(p.bass, SAT_BASS.0, SAT_BASS.1),
        sap::WS_DRIVE => p.ws_drive.clamp(0.0, 1.0),
        sap::WS_CURVE => p.ws_curve.clamp(0.0, 1.0),
        sap::WS_DEPTH => p.ws_depth.clamp(0.0, 1.0),
        sap::WS_LIN => p.ws_lin.clamp(0.0, 1.0),
        sap::WS_DAMP => p.ws_damp.clamp(0.0, 1.0),
        sap::WS_PERIOD => p.ws_period.clamp(0.0, 1.0),
        _ => return None,
    })
}

pub(crate) fn sat_set(p: &mut SaturatorParams, k: u8, v: f32) {
    match k {
        sap::DRIVE => p.drive = lin_val(v, SAT_DRIVE.0, SAT_DRIVE.1),
        sap::OUTPUT => p.output = lin_val(v, SAT_OUT.0, SAT_OUT.1),
        sap::DRY_WET => p.dry_wet = v,
        sap::AMT_LO => p.amt_lo = lin_val(v, -1.0, 1.0),
        sap::AMT_HI => p.amt_hi = lin_val(v, -1.0, 1.0),
        sap::FREQ => p.freq = log_val(v, SAT_FREQ.0, SAT_FREQ.1),
        sap::WIDTH => p.width = v,
        sap::BASS => p.bass = lin_val(v, SAT_BASS.0, SAT_BASS.1),
        sap::WS_DRIVE => p.ws_drive = v,
        sap::WS_CURVE => p.ws_curve = v,
        sap::WS_DEPTH => p.ws_depth = v,
        sap::WS_LIN => p.ws_lin = v,
        sap::WS_DAMP => p.ws_damp = v,
        sap::WS_PERIOD => p.ws_period = v,
        _ => {}
    }
}

// ---- Delay ----

pub mod dlp {
    pub const TIME: [u8; 2] = [0, 1];
    pub const OFFSET: [u8; 2] = [2, 3];
    pub const FEEDBACK: u8 = 4;
    pub const FREQ: u8 = 5;
    pub const WIDTH: u8 = 6;
    pub const LFO_DELAY: u8 = 7;
    pub const LFO_FILTER: u8 = 8;
    pub const LFO_RATE: u8 = 9;
    pub const MORPH: u8 = 10;
    pub const DRY_WET: u8 = 11;
    pub const SYNC: [u8; 2] = [40, 41];
    pub const LINK: u8 = 42;
    pub const FREEZE: u8 = 43;
    pub const FILTER: u8 = 44;
    pub const LFO_SHOWN: u8 = 45;
    pub const LFO_MODE: u8 = 46;
    pub const WAVE: u8 = 47;
    pub const SMOOTH: u8 = 48;
    pub const PING: u8 = 49;
    pub const HQ: u8 = 50;
    pub const EQ_LOUD: u8 = 51;
}

fn dly_time_norm(p: &DelayParams, s: usize) -> f32 {
    let k = p.side(s);
    if p.sync[k] { idx_norm(p.six[k].min(7), 8) } else { log_norm(p.ms[k], DLY_MS.0, DLY_MS.1) }
}

fn dly_time_set(p: &mut DelayParams, s: usize, v: f32) {
    let k = p.side(s);
    let sides: &[usize] = if p.link {
        &[0, 1]
    } else if k == 0 {
        &[0]
    } else {
        &[1]
    };
    let sync = p.sync[k];
    for &i in sides {
        p.sync[i] = sync;
        if sync {
            p.six[i] = norm_idx(v, 8);
        } else {
            p.ms[i] = log_val(v, DLY_MS.0, DLY_MS.1);
        }
    }
}

pub(crate) fn dly_norm(p: &DelayParams, k: u8) -> Option<f32> {
    Some(match k {
        0 | 1 => dly_time_norm(p, k as usize),
        2 | 3 => lin_norm(p.offset[p.side(k as usize - 2)], -DLY_OFFSET, DLY_OFFSET),
        dlp::FEEDBACK => p.feedback.clamp(0.0, 1.0),
        dlp::FREQ => log_norm(p.freq, DLY_FREQ.0, DLY_FREQ.1),
        dlp::WIDTH => lin_norm(p.width, DLY_WIDTH.0, DLY_WIDTH.1),
        dlp::LFO_DELAY => p.lfo_delay.clamp(0.0, 1.0),
        dlp::LFO_FILTER => p.lfo_filter.clamp(0.0, 1.0),
        dlp::LFO_RATE => match p.lfo_mode {
            1 => log_norm(p.lfo_ms, DLY_LFO_MS.0, DLY_LFO_MS.1),
            2..=4 => idx_norm(p.lfo_div, ECHO_MOD_DIVS.len()),
            5 => idx_norm(p.lfo_six.clamp(1, 16) - 1, 16),
            _ => log_norm(p.lfo_hz, DLY_LFO_HZ.0, DLY_LFO_HZ.1),
        },
        dlp::MORPH => p.morph.clamp(0.0, 1.0),
        dlp::DRY_WET => p.dry_wet.clamp(0.0, 1.0),
        _ => return None,
    })
}

pub(crate) fn dly_set(p: &mut DelayParams, k: u8, v: f32) {
    match k {
        0 | 1 => dly_time_set(p, k as usize, v),
        2 | 3 => {
            let o = lin_val(v, -DLY_OFFSET, DLY_OFFSET);
            if p.link {
                p.offset = [o; 2];
            } else {
                p.offset[k as usize - 2] = o;
            }
        }
        dlp::FEEDBACK => p.feedback = v,
        dlp::FREQ => p.freq = log_val(v, DLY_FREQ.0, DLY_FREQ.1),
        dlp::WIDTH => p.width = lin_val(v, DLY_WIDTH.0, DLY_WIDTH.1),
        dlp::LFO_DELAY => p.lfo_delay = v,
        dlp::LFO_FILTER => p.lfo_filter = v,
        dlp::LFO_RATE => match p.lfo_mode {
            1 => p.lfo_ms = log_val(v, DLY_LFO_MS.0, DLY_LFO_MS.1),
            2..=4 => p.lfo_div = norm_idx(v, ECHO_MOD_DIVS.len()),
            5 => p.lfo_six = norm_idx(v, 16) + 1,
            _ => p.lfo_hz = log_val(v, DLY_LFO_HZ.0, DLY_LFO_HZ.1),
        },
        dlp::MORPH => p.morph = v,
        dlp::DRY_WET => p.dry_wet = v,
        _ => {}
    }
}

// ---- Grain Delay ----

pub mod gdp {
    pub const TIME: u8 = 0;
    pub const SPRAY: u8 = 1;
    pub const FREQ: u8 = 2;
    pub const PITCH: u8 = 3;
    pub const RAND: u8 = 4;
    pub const FEEDBACK: u8 = 5;
    pub const DRY_WET: u8 = 6;
    pub const OFFSET: u8 = 7;
    pub const SYNC: u8 = 40;
    pub const X_AXIS: u8 = 41;
    pub const Y_AXIS: u8 = 42;
    /// Parameter ids of the X-axis choices (`GD_X`).
    pub const X_IDS: [u8; 6] = [TIME, SPRAY, FREQ, PITCH, RAND, FEEDBACK];
    /// Parameter ids of the Y-axis choices (`GD_Y`).
    pub const Y_IDS: [u8; 6] = [SPRAY, FREQ, PITCH, RAND, FEEDBACK, DRY_WET];
}

pub(crate) fn gd_norm(p: &GrainDelayParams, k: u8) -> Option<f32> {
    Some(match k {
        gdp::TIME if p.sync => idx_norm(p.six.min(7), 8),
        gdp::TIME => log_norm(p.ms, GD_MS.0, GD_MS.1),
        gdp::SPRAY => (p.spray.clamp(GD_SPRAY.0, GD_SPRAY.1) / GD_SPRAY.1).sqrt(),
        gdp::FREQ => log_norm(p.freq, GD_FREQ.0, GD_FREQ.1),
        gdp::PITCH => lin_norm(p.pitch, GD_PITCH.0, GD_PITCH.1),
        gdp::RAND => lin_norm(p.rand, GD_RAND.0, GD_RAND.1),
        gdp::FEEDBACK => lin_norm(p.feedback, 0.0, GD_FB_MAX),
        gdp::DRY_WET => p.dry_wet.clamp(0.0, 1.0),
        gdp::OFFSET => lin_norm(p.offset, -GD_OFFSET, GD_OFFSET),
        _ => return None,
    })
}

pub(crate) fn gd_set(p: &mut GrainDelayParams, k: u8, v: f32) {
    match k {
        gdp::TIME if p.sync => p.six = norm_idx(v, 8),
        gdp::TIME => p.ms = log_val(v, GD_MS.0, GD_MS.1),
        gdp::SPRAY => p.spray = v * v * GD_SPRAY.1,
        gdp::FREQ => p.freq = log_val(v, GD_FREQ.0, GD_FREQ.1),
        gdp::PITCH => p.pitch = lin_val(v, GD_PITCH.0, GD_PITCH.1),
        gdp::RAND => p.rand = lin_val(v, GD_RAND.0, GD_RAND.1),
        gdp::FEEDBACK => p.feedback = lin_val(v, 0.0, GD_FB_MAX),
        gdp::DRY_WET => p.dry_wet = v,
        gdp::OFFSET => p.offset = lin_val(v, -GD_OFFSET, GD_OFFSET),
        _ => {}
    }
}
