//! Normalised parameter access for Channel EQ, Echo, Erosion and Reverb
//! (ids: `ceqp`, `ecp`, `erp`, `rvp`; switches have ids ≥ 40 and no norm).

use crate::chaneq::*;
use crate::echo::*;
use crate::erosion::*;
use crate::params::{idx_norm, lin_norm, lin_val, log_norm, log_val, norm_idx};
use crate::reverb::*;

// ---- Channel EQ ----

pub mod ceqp {
    pub const LOW: u8 = 0;
    pub const MID: u8 = 1;
    pub const FREQ: u8 = 2;
    pub const HIGH: u8 = 3;
    pub const OUTPUT: u8 = 4;
    pub const HP: u8 = 40;
}

pub(crate) fn ceq_norm(c: &ChannelEqParams, k: u8) -> Option<f32> {
    Some(match k {
        ceqp::LOW => lin_norm(c.low, -CEQ_SHELF_DB, CEQ_SHELF_DB),
        ceqp::MID => lin_norm(c.mid, -CEQ_MID_DB, CEQ_MID_DB),
        ceqp::FREQ => log_norm(c.mid_freq, CEQ_MID_HZ.0, CEQ_MID_HZ.1),
        ceqp::HIGH => lin_norm(c.high, -CEQ_SHELF_DB, CEQ_SHELF_DB),
        ceqp::OUTPUT => lin_norm(c.output, -CEQ_OUT_DB, CEQ_OUT_DB),
        _ => return None,
    })
}

pub(crate) fn ceq_set(c: &mut ChannelEqParams, k: u8, v: f32) {
    match k {
        ceqp::LOW => c.low = lin_val(v, -CEQ_SHELF_DB, CEQ_SHELF_DB),
        ceqp::MID => c.mid = lin_val(v, -CEQ_MID_DB, CEQ_MID_DB),
        ceqp::FREQ => c.mid_freq = log_val(v, CEQ_MID_HZ.0, CEQ_MID_HZ.1),
        ceqp::HIGH => c.high = lin_val(v, -CEQ_SHELF_DB, CEQ_SHELF_DB),
        ceqp::OUTPUT => c.output = lin_val(v, -CEQ_OUT_DB, CEQ_OUT_DB),
        _ => {}
    }
}

// ---- Erosion ----

pub mod erp {
    pub const FREQ: u8 = 0;
    pub const AMOUNT: u8 = 1;
    pub const BLEND: u8 = 2;
    pub const WIDTH: u8 = 3;
    pub const STEREO: u8 = 4;
}

pub(crate) fn ero_norm(e: &ErosionParams, k: u8) -> Option<f32> {
    Some(match k {
        erp::FREQ => log_norm(e.freq, ERO_FREQ.0, ERO_FREQ.1),
        erp::AMOUNT => e.amount.clamp(0.0, 1.0),
        erp::BLEND => e.blend.clamp(0.0, 1.0),
        erp::WIDTH => lin_norm(e.width, ERO_WIDTH.0, ERO_WIDTH.1),
        erp::STEREO => e.stereo.clamp(0.0, 1.0),
        _ => return None,
    })
}

pub(crate) fn ero_set(e: &mut ErosionParams, k: u8, v: f32) {
    match k {
        erp::FREQ => e.freq = log_val(v, ERO_FREQ.0, ERO_FREQ.1),
        erp::AMOUNT => e.amount = v,
        erp::BLEND => e.blend = v,
        erp::WIDTH => e.width = lin_val(v, ERO_WIDTH.0, ERO_WIDTH.1),
        erp::STEREO => e.stereo = v,
        _ => {}
    }
}

// ---- Reverb ----

pub mod rvp {
    pub const IN_FREQ: u8 = 0;
    pub const IN_WIDTH: u8 = 1;
    pub const SPIN_AMT: u8 = 2;
    pub const SPIN_RATE: u8 = 3;
    pub const SHAPE: u8 = 4;
    pub const HI_FREQ: u8 = 5;
    pub const HI_GAIN: u8 = 6;
    pub const LO_FREQ: u8 = 7;
    pub const LO_GAIN: u8 = 8;
    pub const DIFFUSION: u8 = 9;
    pub const SCALE: u8 = 10;
    pub const CHO_AMT: u8 = 11;
    pub const CHO_RATE: u8 = 12;
    pub const PREDELAY: u8 = 13;
    pub const SIZE: u8 = 14;
    pub const DECAY: u8 = 15;
    pub const STEREO: u8 = 16;
    pub const REFLECT: u8 = 17;
    pub const DIFFUSE: u8 = 18;
    pub const DRY_WET: u8 = 19;
    pub const LO_CUT: u8 = 40;
    pub const HI_CUT: u8 = 41;
    pub const SPIN: u8 = 42;
    pub const HI_ON: u8 = 43;
    pub const HI_LP: u8 = 44;
    pub const LO_ON: u8 = 45;
    pub const CHORUS: u8 = 46;
    pub const SMOOTH: u8 = 47;
    pub const FREEZE: u8 = 48;
    pub const FLAT: u8 = 49;
    pub const CUT: u8 = 50;
    pub const DENSITY: u8 = 51;
}

pub(crate) fn rv_norm(r: &ReverbParams, k: u8) -> Option<f32> {
    Some(match k {
        rvp::IN_FREQ => log_norm(r.in_freq, RV_IN_FREQ.0, RV_IN_FREQ.1),
        rvp::IN_WIDTH => lin_norm(r.in_width, RV_IN_WIDTH.0, RV_IN_WIDTH.1),
        rvp::SPIN_AMT => lin_norm(r.spin_amount, RV_SPIN_AMT.0, RV_SPIN_AMT.1),
        rvp::SPIN_RATE => log_norm(r.spin_rate, RV_SPIN_RATE.0, RV_SPIN_RATE.1),
        rvp::SHAPE => r.shape.clamp(0.0, 1.0),
        rvp::HI_FREQ => log_norm(r.hi_freq, RV_HI_FREQ.0, RV_HI_FREQ.1),
        rvp::HI_GAIN => lin_norm(r.hi_gain, RV_SHELF_GAIN.0, RV_SHELF_GAIN.1),
        rvp::LO_FREQ => log_norm(r.lo_freq, RV_LO_FREQ.0, RV_LO_FREQ.1),
        rvp::LO_GAIN => lin_norm(r.lo_gain, RV_SHELF_GAIN.0, RV_SHELF_GAIN.1),
        rvp::DIFFUSION => r.diffusion.clamp(0.0, 1.0),
        rvp::SCALE => lin_norm(r.scale, RV_SCALE.0, RV_SCALE.1),
        rvp::CHO_AMT => log_norm(r.cho_amount, RV_CHO_AMT.0, RV_CHO_AMT.1),
        rvp::CHO_RATE => log_norm(r.cho_rate, RV_CHO_RATE.0, RV_CHO_RATE.1),
        rvp::PREDELAY => log_norm(r.predelay, RV_PREDELAY.0, RV_PREDELAY.1),
        rvp::SIZE => log_norm(r.size, RV_SIZE.0, RV_SIZE.1),
        rvp::DECAY => log_norm(r.decay, RV_DECAY.0, RV_DECAY.1),
        rvp::STEREO => lin_norm(r.stereo, 0.0, RV_STEREO_MAX),
        rvp::REFLECT => lin_norm(r.reflect, RV_LEVEL.0, RV_LEVEL.1),
        rvp::DIFFUSE => lin_norm(r.diffuse, RV_LEVEL.0, RV_LEVEL.1),
        rvp::DRY_WET => r.dry_wet.clamp(0.0, 1.0),
        _ => return None,
    })
}

pub(crate) fn rv_set(r: &mut ReverbParams, k: u8, v: f32) {
    match k {
        rvp::IN_FREQ => r.in_freq = log_val(v, RV_IN_FREQ.0, RV_IN_FREQ.1),
        rvp::IN_WIDTH => r.in_width = lin_val(v, RV_IN_WIDTH.0, RV_IN_WIDTH.1),
        rvp::SPIN_AMT => r.spin_amount = lin_val(v, RV_SPIN_AMT.0, RV_SPIN_AMT.1),
        rvp::SPIN_RATE => r.spin_rate = log_val(v, RV_SPIN_RATE.0, RV_SPIN_RATE.1),
        rvp::SHAPE => r.shape = v,
        rvp::HI_FREQ => r.hi_freq = log_val(v, RV_HI_FREQ.0, RV_HI_FREQ.1),
        rvp::HI_GAIN => r.hi_gain = lin_val(v, RV_SHELF_GAIN.0, RV_SHELF_GAIN.1),
        rvp::LO_FREQ => r.lo_freq = log_val(v, RV_LO_FREQ.0, RV_LO_FREQ.1),
        rvp::LO_GAIN => r.lo_gain = lin_val(v, RV_SHELF_GAIN.0, RV_SHELF_GAIN.1),
        rvp::DIFFUSION => r.diffusion = v,
        rvp::SCALE => r.scale = lin_val(v, RV_SCALE.0, RV_SCALE.1),
        rvp::CHO_AMT => r.cho_amount = log_val(v, RV_CHO_AMT.0, RV_CHO_AMT.1),
        rvp::CHO_RATE => r.cho_rate = log_val(v, RV_CHO_RATE.0, RV_CHO_RATE.1),
        rvp::PREDELAY => r.predelay = log_val(v, RV_PREDELAY.0, RV_PREDELAY.1),
        rvp::SIZE => r.size = log_val(v, RV_SIZE.0, RV_SIZE.1),
        rvp::DECAY => r.decay = log_val(v, RV_DECAY.0, RV_DECAY.1),
        rvp::STEREO => r.stereo = lin_val(v, 0.0, RV_STEREO_MAX),
        rvp::REFLECT => r.reflect = lin_val(v, RV_LEVEL.0, RV_LEVEL.1),
        rvp::DIFFUSE => r.diffuse = lin_val(v, RV_LEVEL.0, RV_LEVEL.1),
        rvp::DRY_WET => r.dry_wet = v,
        _ => {}
    }
}

// ---- Echo ----

pub mod ecp {
    pub const TIME: [u8; 2] = [0, 1];
    pub const OFFSET: [u8; 2] = [2, 3];
    pub const INPUT: u8 = 4;
    pub const FEEDBACK: u8 = 5;
    pub const HP: u8 = 6;
    pub const HP_RES: u8 = 7;
    pub const LP: u8 = 8;
    pub const LP_RES: u8 = 9;
    pub const MOD_RATE: u8 = 10;
    pub const PHASE: u8 = 11;
    pub const MOD_DELAY: u8 = 12;
    pub const MOD_FILTER: u8 = 13;
    pub const ENV_MIX: u8 = 14;
    pub const GATE_THR: u8 = 15;
    pub const GATE_REL: u8 = 16;
    pub const DUCK_THR: u8 = 17;
    pub const DUCK_REL: u8 = 18;
    pub const NOISE_AMT: u8 = 19;
    pub const NOISE_MORPH: u8 = 20;
    pub const WOB_AMT: u8 = 21;
    pub const WOB_MORPH: u8 = 22;
    pub const SMOOTH: u8 = 23;
    pub const REVERB: u8 = 24;
    pub const VERB_DECAY: u8 = 25;
    pub const STEREO: u8 = 26;
    pub const OUTPUT: u8 = 27;
    pub const DRY_WET: u8 = 28;
    pub const MODE: u8 = 40;
    pub const SYNC: [u8; 2] = [41, 42];
    pub const SYNC_MODE: [u8; 2] = [43, 44];
    pub const LINK: u8 = 45;
    pub const CLIP: u8 = 46;
    pub const INVERT: u8 = 47;
    pub const TAB: u8 = 48;
    pub const FILTER: u8 = 49;
    pub const FILTER_SHOWN: u8 = 50;
    pub const WAVE: u8 = 51;
    pub const MOD_SYNC: u8 = 52;
    pub const X4: u8 = 53;
    pub const GATE: u8 = 54;
    pub const DUCK: u8 = 55;
    pub const NOISE: u8 = 56;
    pub const WOBBLE: u8 = 57;
    pub const REPITCH: u8 = 58;
    pub const VERB_LOC: u8 = 59;
    pub const EQ_LOUD: u8 = 60;
}

/// Echo Input / Output knob: 0 dB at 12 o'clock (Live); amplitude^¼ below,
/// linear dB above to +12.
pub fn echo_gain_norm(db: f32) -> f32 {
    if db <= ECHO_GAIN_DB.0 {
        0.0
    } else if db <= 0.0 {
        0.5 * crate::dynfx::db_lin(db).powf(0.25)
    } else {
        0.5 + 0.5 * (db / ECHO_GAIN_DB.1).min(1.0)
    }
}

pub fn echo_norm_gain(v: f32) -> f32 {
    if v <= 1e-4 {
        ECHO_GAIN_DB.0
    } else if v <= 0.5 {
        crate::dynfx::lin_db((v * 2.0).powf(4.0)).clamp(ECHO_GAIN_DB.0, 0.0)
    } else {
        ((v.min(1.0) - 0.5) * 2.0 * ECHO_GAIN_DB.1).clamp(0.0, ECHO_GAIN_DB.1)
    }
}

fn time_norm(e: &EchoParams, s: usize) -> f32 {
    let s = if e.link { 0 } else { s };
    if !e.sync[s] {
        log_norm(e.ms[s], ECHO_MS.0, ECHO_MS.1)
    } else if e.sync_mode[s] == 3 {
        idx_norm(e.sixteenths[s].clamp(1, 16) - 1, 16)
    } else {
        idx_norm(e.div[s], ECHO_NOTES.len())
    }
}

fn time_set(e: &mut EchoParams, s: usize, v: f32) {
    let sides: &[usize] = if e.link {
        &[0, 1]
    } else if s == 0 {
        &[0]
    } else {
        &[1]
    };
    let src = if e.link { 0 } else { s };
    let (sync, mode) = (e.sync[src], e.sync_mode[src]);
    for &k in sides {
        e.sync[k] = sync;
        e.sync_mode[k] = mode;
        if !sync {
            e.ms[k] = log_val(v, ECHO_MS.0, ECHO_MS.1);
        } else if mode == 3 {
            e.sixteenths[k] = norm_idx(v, 16) + 1;
        } else {
            e.div[k] = norm_idx(v, ECHO_NOTES.len());
        }
    }
}

pub(crate) fn echo_norm(e: &EchoParams, k: u8) -> Option<f32> {
    let off = |x: f32| lin_norm(x, -ECHO_OFFSET, ECHO_OFFSET);
    Some(match k {
        0 | 1 => time_norm(e, k as usize),
        2 | 3 => off(e.offset[k as usize - 2]),
        ecp::INPUT => echo_gain_norm(e.input),
        ecp::FEEDBACK => lin_norm(e.feedback, 0.0, ECHO_FB_MAX),
        ecp::HP => log_norm(e.hp, ECHO_FILT_HZ.0, ECHO_FILT_HZ.1),
        ecp::HP_RES => e.hp_res.clamp(0.0, 1.0),
        ecp::LP => log_norm(e.lp, ECHO_FILT_HZ.0, ECHO_FILT_HZ.1),
        ecp::LP_RES => e.lp_res.clamp(0.0, 1.0),
        ecp::MOD_RATE if e.mod_sync => idx_norm(e.mod_div, ECHO_MOD_DIVS.len()),
        ecp::MOD_RATE => log_norm(e.mod_hz, ECHO_MOD_HZ.0, ECHO_MOD_HZ.1),
        ecp::PHASE => lin_norm(e.phase, 0.0, 360.0),
        ecp::MOD_DELAY => e.mod_delay.clamp(0.0, 1.0),
        ecp::MOD_FILTER => e.mod_filter.clamp(0.0, 1.0),
        ecp::ENV_MIX => e.env_mix.clamp(0.0, 1.0),
        ecp::GATE_THR => lin_norm(e.gate_thr, ECHO_THR_DB.0, ECHO_THR_DB.1),
        ecp::GATE_REL => log_norm(e.gate_rel, ECHO_REL_MS.0, ECHO_REL_MS.1),
        ecp::DUCK_THR => lin_norm(e.duck_thr, ECHO_THR_DB.0, ECHO_THR_DB.1),
        ecp::DUCK_REL => log_norm(e.duck_rel, ECHO_REL_MS.0, ECHO_REL_MS.1),
        ecp::NOISE_AMT => e.noise_amt.clamp(0.0, 1.0),
        ecp::NOISE_MORPH => e.noise_morph.clamp(0.0, 1.0),
        ecp::WOB_AMT => e.wobble_amt.clamp(0.0, 1.0),
        ecp::WOB_MORPH => e.wobble_morph.clamp(0.0, 1.0),
        ecp::SMOOTH => log_norm(e.smooth_ms, ECHO_SMOOTH_MS.0, ECHO_SMOOTH_MS.1),
        ecp::REVERB => e.reverb.clamp(0.0, 1.0),
        ecp::VERB_DECAY => e.verb_decay.clamp(0.0, 1.0),
        ecp::STEREO => lin_norm(e.stereo, 0.0, ECHO_STEREO_MAX),
        ecp::OUTPUT => echo_gain_norm(e.output),
        ecp::DRY_WET => e.dry_wet.clamp(0.0, 1.0),
        _ => return None,
    })
}

pub(crate) fn echo_set(e: &mut EchoParams, k: u8, v: f32) {
    let off = |v: f32| lin_val(v, -ECHO_OFFSET, ECHO_OFFSET);
    match k {
        0 | 1 => time_set(e, k as usize, v),
        2 | 3 => e.offset[k as usize - 2] = off(v),
        ecp::INPUT => e.input = echo_norm_gain(v),
        ecp::FEEDBACK => e.feedback = lin_val(v, 0.0, ECHO_FB_MAX),
        ecp::HP => e.hp = log_val(v, ECHO_FILT_HZ.0, ECHO_FILT_HZ.1),
        ecp::HP_RES => e.hp_res = v,
        ecp::LP => e.lp = log_val(v, ECHO_FILT_HZ.0, ECHO_FILT_HZ.1),
        ecp::LP_RES => e.lp_res = v,
        ecp::MOD_RATE if e.mod_sync => e.mod_div = norm_idx(v, ECHO_MOD_DIVS.len()),
        ecp::MOD_RATE => e.mod_hz = log_val(v, ECHO_MOD_HZ.0, ECHO_MOD_HZ.1),
        ecp::PHASE => e.phase = lin_val(v, 0.0, 360.0),
        ecp::MOD_DELAY => e.mod_delay = v,
        ecp::MOD_FILTER => e.mod_filter = v,
        ecp::ENV_MIX => e.env_mix = v,
        ecp::GATE_THR => e.gate_thr = lin_val(v, ECHO_THR_DB.0, ECHO_THR_DB.1),
        ecp::GATE_REL => e.gate_rel = log_val(v, ECHO_REL_MS.0, ECHO_REL_MS.1),
        ecp::DUCK_THR => e.duck_thr = lin_val(v, ECHO_THR_DB.0, ECHO_THR_DB.1),
        ecp::DUCK_REL => e.duck_rel = log_val(v, ECHO_REL_MS.0, ECHO_REL_MS.1),
        ecp::NOISE_AMT => e.noise_amt = v,
        ecp::NOISE_MORPH => e.noise_morph = v,
        ecp::WOB_AMT => e.wobble_amt = v,
        ecp::WOB_MORPH => e.wobble_morph = v,
        ecp::SMOOTH => e.smooth_ms = log_val(v, ECHO_SMOOTH_MS.0, ECHO_SMOOTH_MS.1),
        ecp::REVERB => e.reverb = v,
        ecp::VERB_DECAY => e.verb_decay = v,
        ecp::STEREO => e.stereo = lin_val(v, 0.0, ECHO_STEREO_MAX),
        ecp::OUTPUT => e.output = echo_norm_gain(v),
        ecp::DRY_WET => e.dry_wet = v,
        _ => {}
    }
}
