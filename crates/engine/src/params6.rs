//! Normalised parameter access for Tuner, Phaser-Flanger, Corpus and Roar
//! (ids: `tup`, `pfp`, `cop`, `rop`). Switches have no norm (set by the UI).

use crate::corpus::*;
use crate::modfx::SYNC_DIVS;
use crate::params::{idx_norm, lin_norm, lin_val, log_norm, log_val, norm_idx};
use crate::phaserflanger::*;
use crate::roar::*;
use crate::tuner::*;

// ---- Tuner ----

pub mod tup {
    pub const REFERENCE: u8 = 0;
    pub const VIEW: u8 = 40;
    pub const STROBE: u8 = 41;
    pub const HZ: u8 = 42;
    pub const AUTO: u8 = 43;
    pub const SPELL: u8 = 44;
}

pub(crate) fn tu_norm(p: &TunerParams, k: u8) -> Option<f32> {
    Some(match k {
        tup::REFERENCE => lin_norm(p.reference, TU_REF.0, TU_REF.1),
        _ => return None,
    })
}

pub(crate) fn tu_set(p: &mut TunerParams, k: u8, v: f32) {
    if k == tup::REFERENCE {
        p.reference = lin_val(v, TU_REF.0, TU_REF.1).round();
    }
}

// ---- Phaser-Flanger ----

pub mod pfp {
    pub const NOTCHES: u8 = 0;
    pub const CENTER: u8 = 1;
    pub const SPREAD: u8 = 2;
    pub const BLEND: u8 = 3;
    pub const FL_TIME: u8 = 4;
    pub const DB_TIME: u8 = 5;
    pub const RATE: u8 = 6;
    pub const DIV: u8 = 7;
    pub const PHASE: u8 = 8;
    pub const SPIN: u8 = 9;
    pub const OFFSET: u8 = 10;
    pub const DUTY: u8 = 11;
    pub const LFO2_MIX: u8 = 12;
    pub const LFO2_RATE: u8 = 13;
    pub const LFO2_DIV: u8 = 14;
    pub const AMOUNT: u8 = 15;
    pub const FEEDBACK: u8 = 16;
    pub const ENV_AMT: u8 = 17;
    pub const ENV_ATTACK: u8 = 18;
    pub const ENV_RELEASE: u8 = 19;
    pub const SAFE_BASS: u8 = 20;
    pub const OUTPUT: u8 = 21;
    pub const WARMTH: u8 = 22;
    pub const DRY_WET: u8 = 23;
    pub const MODE: u8 = 40;
    pub const LFO_SYNC: u8 = 41;
    pub const WAVE: u8 = 42;
    pub const SPIN_MODE: u8 = 43;
    pub const LFO2_SYNC: u8 = 44;
    pub const FB_INV: u8 = 45;
    pub const ENV_ON: u8 = 46;
    pub const OPEN: u8 = 47;
}

pub(crate) fn pf_norm(p: &PhaserFlangerParams, k: u8) -> Option<f32> {
    let nd = SYNC_DIVS.len();
    Some(match k {
        pfp::NOTCHES => lin_norm(p.notches as f32, 1.0, PF_NOTCH_MAX as f32),
        pfp::CENTER => log_norm(p.center, PF_CENTER.0, PF_CENTER.1),
        pfp::SPREAD => p.spread.clamp(0.0, 1.0),
        pfp::BLEND => p.blend.clamp(0.0, 1.0),
        pfp::FL_TIME => log_norm(p.fl_time, PF_FL_TIME.0, PF_FL_TIME.1),
        pfp::DB_TIME => log_norm(p.db_time, PF_DB_TIME.0, PF_DB_TIME.1),
        pfp::RATE => log_norm(p.rate, PF_RATE.0, PF_RATE.1),
        pfp::DIV => idx_norm(p.div, nd),
        pfp::PHASE => p.phase.clamp(0.0, 1.0),
        pfp::SPIN => p.spin.clamp(0.0, 1.0),
        pfp::OFFSET => p.offset.clamp(0.0, 1.0),
        pfp::DUTY => lin_norm(p.duty, -1.0, 1.0),
        pfp::LFO2_MIX => p.lfo2_mix.clamp(0.0, 1.0),
        pfp::LFO2_RATE => log_norm(p.lfo2_rate, PF_RATE.0, PF_RATE.1),
        pfp::LFO2_DIV => idx_norm(p.lfo2_div, nd),
        pfp::AMOUNT => p.amount.clamp(0.0, 1.0),
        pfp::FEEDBACK => p.feedback.clamp(0.0, 1.0),
        pfp::ENV_AMT => lin_norm(p.env_amt, -1.0, 1.0),
        pfp::ENV_ATTACK => log_norm(p.env_attack, PF_ENV_T.0, PF_ENV_T.1),
        pfp::ENV_RELEASE => log_norm(p.env_release, PF_ENV_T.0, PF_ENV_T.1),
        pfp::SAFE_BASS => log_norm(p.safe_bass, PF_SAFE.0, PF_SAFE.1),
        pfp::OUTPUT => lin_norm(p.output, PF_OUT.0, PF_OUT.1),
        pfp::WARMTH => p.warmth.clamp(0.0, 1.0),
        pfp::DRY_WET => p.dry_wet.clamp(0.0, 1.0),
        _ => return None,
    })
}

pub(crate) fn pf_set(p: &mut PhaserFlangerParams, k: u8, v: f32) {
    let nd = SYNC_DIVS.len();
    match k {
        pfp::NOTCHES => p.notches = lin_val(v, 1.0, PF_NOTCH_MAX as f32).round() as u8,
        pfp::CENTER => p.center = log_val(v, PF_CENTER.0, PF_CENTER.1),
        pfp::SPREAD => p.spread = v.clamp(0.0, 1.0),
        pfp::BLEND => p.blend = v.clamp(0.0, 1.0),
        pfp::FL_TIME => p.fl_time = log_val(v, PF_FL_TIME.0, PF_FL_TIME.1),
        pfp::DB_TIME => p.db_time = log_val(v, PF_DB_TIME.0, PF_DB_TIME.1),
        pfp::RATE => p.rate = log_val(v, PF_RATE.0, PF_RATE.1),
        pfp::DIV => p.div = norm_idx(v, nd),
        pfp::PHASE => p.phase = v.clamp(0.0, 1.0),
        pfp::SPIN => p.spin = v.clamp(0.0, 1.0),
        pfp::OFFSET => p.offset = v.clamp(0.0, 1.0),
        pfp::DUTY => p.duty = lin_val(v, -1.0, 1.0),
        pfp::LFO2_MIX => p.lfo2_mix = v.clamp(0.0, 1.0),
        pfp::LFO2_RATE => p.lfo2_rate = log_val(v, PF_RATE.0, PF_RATE.1),
        pfp::LFO2_DIV => p.lfo2_div = norm_idx(v, nd),
        pfp::AMOUNT => p.amount = v.clamp(0.0, 1.0),
        pfp::FEEDBACK => p.feedback = v.clamp(0.0, 1.0),
        pfp::ENV_AMT => p.env_amt = lin_val(v, -1.0, 1.0),
        pfp::ENV_ATTACK => p.env_attack = log_val(v, PF_ENV_T.0, PF_ENV_T.1),
        pfp::ENV_RELEASE => p.env_release = log_val(v, PF_ENV_T.0, PF_ENV_T.1),
        pfp::SAFE_BASS => p.safe_bass = log_val(v, PF_SAFE.0, PF_SAFE.1),
        pfp::OUTPUT => p.output = lin_val(v, PF_OUT.0, PF_OUT.1),
        pfp::WARMTH => p.warmth = v.clamp(0.0, 1.0),
        pfp::DRY_WET => p.dry_wet = v.clamp(0.0, 1.0),
        _ => {}
    }
}

// ---- Corpus ----

pub mod cop {
    pub const DECAY: u8 = 0;
    pub const MATERIAL: u8 = 1;
    pub const RADIUS: u8 = 2;
    pub const BRIGHT: u8 = 3;
    pub const INHARM: u8 = 4;
    pub const OPENING: u8 = 5;
    pub const RATIO: u8 = 6;
    pub const HIT: u8 = 7;
    pub const WIDTH: u8 = 8;
    pub const POS_L: u8 = 9;
    pub const POS_R: u8 = 10;
    pub const TUNE: u8 = 11;
    pub const TRANSPOSE: u8 = 12;
    pub const FINE: u8 = 13;
    pub const SPREAD: u8 = 14;
    pub const LFO_AMT: u8 = 15;
    pub const RATE: u8 = 16;
    pub const DIV: u8 = 17;
    pub const PHASE: u8 = 18;
    pub const SPIN: u8 = 19;
    pub const OFFSET: u8 = 20;
    pub const FREQ: u8 = 21;
    pub const BW: u8 = 22;
    pub const BLEED: u8 = 23;
    pub const GAIN: u8 = 24;
    pub const DRY_WET: u8 = 25;
    pub const PB: u8 = 26;
    pub const OFF_AMT: u8 = 27;
    pub const KIND: u8 = 40;
    pub const QUALITY: u8 = 41;
    pub const LFO_ON: u8 = 42;
    pub const WAVE: u8 = 43;
    pub const LFO_SYNC: u8 = 44;
    pub const SPIN_MODE: u8 = 45;
    pub const FILTER_ON: u8 = 46;
    pub const MIDI_FREQ: u8 = 47;
    pub const LOW: u8 = 48;
    pub const OFF_DECAY: u8 = 49;
    pub const SOURCE: u8 = 50;
    pub const TAP: u8 = 51;
    pub const OPEN: u8 = 52;
    pub const MIDI_ON: u8 = 53;
}

pub(crate) fn co_norm(p: &CorpusParams, k: u8) -> Option<f32> {
    let nd = SYNC_DIVS.len();
    Some(match k {
        cop::DECAY => log_norm(p.decay, CO_DECAY.0, CO_DECAY.1),
        cop::MATERIAL => lin_norm(p.material, -1.0, 1.0),
        cop::RADIUS => p.radius.clamp(0.0, 1.0),
        cop::BRIGHT => lin_norm(p.bright, -1.0, 1.0),
        cop::INHARM => lin_norm(p.inharm, -1.0, 1.0),
        cop::OPENING => p.opening.clamp(0.0, 1.0),
        cop::RATIO => p.ratio.clamp(0.0, 1.0),
        cop::HIT => p.hit.clamp(0.0, 1.0),
        cop::WIDTH => p.width.clamp(0.0, 1.0),
        cop::POS_L => p.pos_l.clamp(0.0, 1.0),
        cop::POS_R => p.pos_r.clamp(0.0, 1.0),
        cop::TUNE => log_norm(p.tune, CO_TUNE.0, CO_TUNE.1),
        cop::TRANSPOSE => lin_norm(p.transpose, -CO_TRANSPOSE, CO_TRANSPOSE),
        cop::FINE => lin_norm(p.fine, -50.0, 50.0),
        cop::SPREAD => lin_norm(p.spread, -1.0, 1.0),
        cop::LFO_AMT => p.lfo_amt.clamp(0.0, 1.0),
        cop::RATE => log_norm(p.rate, CO_RATE.0, CO_RATE.1),
        cop::DIV => idx_norm(p.div, nd),
        cop::PHASE => p.phase.clamp(0.0, 1.0),
        cop::SPIN => p.spin.clamp(0.0, 1.0),
        cop::OFFSET => p.offset.clamp(0.0, 1.0),
        cop::FREQ => log_norm(p.freq, CO_FREQ.0, CO_FREQ.1),
        cop::BW => log_norm(p.bw, CO_BW.0, CO_BW.1),
        cop::BLEED => p.bleed.clamp(0.0, 1.0),
        cop::GAIN => lin_norm(p.gain, CO_GAIN.0, CO_GAIN.1),
        cop::DRY_WET => p.dry_wet.clamp(0.0, 1.0),
        cop::PB => lin_norm(p.pb, 0.0, 24.0),
        cop::OFF_AMT => p.off_amt.clamp(0.0, 1.0),
        _ => return None,
    })
}

pub(crate) fn co_set(p: &mut CorpusParams, k: u8, v: f32) {
    let nd = SYNC_DIVS.len();
    match k {
        cop::DECAY => p.decay = log_val(v, CO_DECAY.0, CO_DECAY.1),
        cop::MATERIAL => p.material = lin_val(v, -1.0, 1.0),
        cop::RADIUS => p.radius = v.clamp(0.0, 1.0),
        cop::BRIGHT => p.bright = lin_val(v, -1.0, 1.0),
        cop::INHARM => p.inharm = lin_val(v, -1.0, 1.0),
        cop::OPENING => p.opening = v.clamp(0.0, 1.0),
        cop::RATIO => p.ratio = v.clamp(0.0, 1.0),
        cop::HIT => p.hit = v.clamp(0.0, 1.0),
        cop::WIDTH => p.width = v.clamp(0.0, 1.0),
        cop::POS_L => p.pos_l = v.clamp(0.0, 1.0),
        cop::POS_R => p.pos_r = v.clamp(0.0, 1.0),
        cop::TUNE => p.tune = log_val(v, CO_TUNE.0, CO_TUNE.1),
        cop::TRANSPOSE => p.transpose = lin_val(v, -CO_TRANSPOSE, CO_TRANSPOSE).round(),
        cop::FINE => p.fine = lin_val(v, -50.0, 50.0),
        cop::SPREAD => p.spread = lin_val(v, -1.0, 1.0),
        cop::LFO_AMT => p.lfo_amt = v.clamp(0.0, 1.0),
        cop::RATE => p.rate = log_val(v, CO_RATE.0, CO_RATE.1),
        cop::DIV => p.div = norm_idx(v, nd),
        cop::PHASE => p.phase = v.clamp(0.0, 1.0),
        cop::SPIN => p.spin = v.clamp(0.0, 1.0),
        cop::OFFSET => p.offset = v.clamp(0.0, 1.0),
        cop::FREQ => p.freq = log_val(v, CO_FREQ.0, CO_FREQ.1),
        cop::BW => p.bw = log_val(v, CO_BW.0, CO_BW.1),
        cop::BLEED => p.bleed = v.clamp(0.0, 1.0),
        cop::GAIN => p.gain = lin_val(v, CO_GAIN.0, CO_GAIN.1),
        cop::DRY_WET => p.dry_wet = v.clamp(0.0, 1.0),
        cop::PB => p.pb = lin_val(v, 0.0, 24.0).round(),
        cop::OFF_AMT => p.off_amt = v.clamp(0.0, 1.0),
        _ => {}
    }
}

// ---- Roar ----

pub mod rop {
    pub const DRIVE: u8 = 0;
    pub const TONE_AMT: u8 = 1;
    pub const TONE_FREQ: u8 = 2;
    pub const BLEND: u8 = 3;
    pub const XO_LO: u8 = 4;
    pub const XO_HI: u8 = 5;
    /// Stage `s` parameter `o` at `STAGE + 7·s + o`.
    pub const STAGE: u8 = 6;
    pub const S_AMOUNT: u8 = 0;
    pub const S_BIAS: u8 = 1;
    pub const S_FREQ: u8 = 2;
    pub const S_RES: u8 = 3;
    pub const S_LEVEL: u8 = 4;
    pub const S_PEAK: u8 = 5;
    pub const S_MORPH: u8 = 6;
    pub const FB_AMT: u8 = 27;
    pub const FB_TIME: u8 = 28;
    pub const FB_FREQ: u8 = 29;
    pub const COMPRESS: u8 = 30;
    pub const DRY_WET: u8 = 31;
    pub const FB_WIDTH: u8 = 32;
    pub const OUTPUT: u8 = 33;
    pub const L1_RATE: u8 = 34;
    pub const L2_RATE: u8 = 35;
    pub const FB_DIV: u8 = 36;
    pub const FB_NOTE: u8 = 37;
    pub const L1_DIV: u8 = 38;
    pub const L1_MORPH: u8 = 39;
    pub const L1_SMOOTH: u8 = 40;
    pub const L2_DIV: u8 = 41;
    pub const L2_MORPH: u8 = 42;
    pub const L2_SMOOTH: u8 = 43;
    pub const ENV_ATT: u8 = 44;
    pub const ENV_REL: u8 = 45;
    pub const ENV_THR: u8 = 46;
    pub const ENV_GAIN: u8 = 47;
    pub const ENV_FREQ: u8 = 48;
    pub const ENV_WIDTH: u8 = 49;
    pub const N_RATE: u8 = 50;
    pub const N_DIV: u8 = 51;
    pub const N_SMOOTH: u8 = 52;
    pub const SC_MIX: u8 = 53;
    pub const SC_GAIN: u8 = 54;
    pub const MOD_AMT: u8 = 55;
    pub const COLOR: u8 = 60;
    pub const ROUTING: u8 = 61;
    pub const STAGE_ON: u8 = 62;
    pub const SHAPER: u8 = 65;
    pub const SHAPER_ON: u8 = 68;
    pub const FILTER: u8 = 71;
    pub const FILTER_ON: u8 = 74;
    pub const PRE: u8 = 77;
    pub const FB_MODE: u8 = 80;
    pub const FB_INV: u8 = 81;
    pub const FB_GATE: u8 = 82;
    pub const SC_HPF: u8 = 83;
    pub const L1_WAVE: u8 = 84;
    pub const L1_MODE: u8 = 85;
    pub const L2_WAVE: u8 = 86;
    pub const L2_MODE: u8 = 87;
    pub const ENV_HOLD: u8 = 88;
    pub const ENV_LISTEN: u8 = 89;
    pub const N_KIND: u8 = 90;
    pub const N_MODE: u8 = 91;
    pub const SC_ON: u8 = 92;
    pub const SC_SRC: u8 = 93;
    pub const SC_TAP: u8 = 94;
    pub const MIDI_ON: u8 = 95;
    pub const MIDI_SRC: u8 = 96;
    pub const MIDI_TAP: u8 = 97;
    pub const SC_LISTEN: u8 = 98;
    pub const MOD_CLEAR: u8 = 99;
    /// Matrix cell (source s, target t) at `MATRIX + 36·s + t`.
    pub const MATRIX: u8 = 100;
    pub const SC_OPEN: u8 = 244;
    pub const MOD_OPEN: u8 = 245;
    pub const MOD_TAB: u8 = 246;
    pub const SRC_TAB: u8 = 247;
    pub const STAGE_TAB: u8 = 248;
    pub const EXPAND: u8 = 249;

    pub const fn stage(s: usize, o: u8) -> u8 {
        STAGE + 7 * s as u8 + o
    }

    pub const fn cell(s: usize, t: usize) -> u8 {
        MATRIX + (36 * s + t) as u8
    }
}

fn rate_count(mode: u8) -> usize {
    if mode == 4 { RO_SIXTEENTHS } else { SYNC_DIVS.len() }
}

pub(crate) fn ro_norm(p: &RoarParams, k: u8) -> Option<f32> {
    let nd = SYNC_DIVS.len();
    Some(match k {
        rop::DRIVE => lin_norm(p.drive, RO_DRIVE.0, RO_DRIVE.1),
        rop::TONE_AMT => lin_norm(p.tone_amt, -1.0, 1.0),
        rop::TONE_FREQ => log_norm(p.tone_freq, RO_TONE_F.0, RO_TONE_F.1),
        rop::BLEND => p.blend.clamp(0.0, 1.0),
        rop::XO_LO => log_norm(p.xo_lo, RO_XO.0, RO_XO.1),
        rop::XO_HI => log_norm(p.xo_hi, RO_XO.0, RO_XO.1),
        6..=26 => {
            let s = &p.stages[((k - 6) / 7) as usize];
            match (k - 6) % 7 {
                rop::S_AMOUNT => s.amount.clamp(0.0, 1.0),
                rop::S_BIAS => lin_norm(s.bias, -1.0, 1.0),
                rop::S_FREQ => log_norm(s.freq, RO_FREQ.0, RO_FREQ.1),
                rop::S_RES => s.res.clamp(0.0, 1.0),
                rop::S_LEVEL => lin_norm(s.level, RO_LEVEL.0, RO_LEVEL.1),
                rop::S_PEAK => lin_norm(s.peak, RO_PEAK.0, RO_PEAK.1),
                _ => s.morph.clamp(0.0, 1.0),
            }
        }
        rop::FB_AMT => p.fb_amt.clamp(0.0, 1.0),
        rop::FB_TIME => log_norm(p.fb_ms, RO_FB_MS.0, RO_FB_MS.1),
        rop::FB_FREQ => log_norm(p.fb_freq, RO_FREQ.0, RO_FREQ.1),
        rop::COMPRESS => p.compress.clamp(0.0, 1.0),
        rop::DRY_WET => p.dry_wet.clamp(0.0, 1.0),
        rop::FB_WIDTH => log_norm(p.fb_width, RO_FB_W.0, RO_FB_W.1),
        rop::OUTPUT => lin_norm(p.output, RO_OUT.0, RO_OUT.1),
        rop::L1_RATE => log_norm(p.lfo[0].rate, RO_RATE.0, RO_RATE.1),
        rop::L2_RATE => log_norm(p.lfo[1].rate, RO_RATE.0, RO_RATE.1),
        rop::FB_DIV => idx_norm(p.fb_div, nd),
        rop::FB_NOTE => lin_norm(p.fb_note, 0.0, 127.0),
        rop::L1_DIV => idx_norm(p.lfo[0].div, rate_count(p.lfo[0].mode)),
        rop::L1_MORPH => p.lfo[0].morph.clamp(0.0, 1.0),
        rop::L1_SMOOTH => p.lfo[0].smooth.clamp(0.0, 1.0),
        rop::L2_DIV => idx_norm(p.lfo[1].div, rate_count(p.lfo[1].mode)),
        rop::L2_MORPH => p.lfo[1].morph.clamp(0.0, 1.0),
        rop::L2_SMOOTH => p.lfo[1].smooth.clamp(0.0, 1.0),
        rop::ENV_ATT => log_norm(p.env_attack, RO_ENV_T.0, RO_ENV_T.1),
        rop::ENV_REL => log_norm(p.env_release, RO_ENV_T.0, RO_ENV_T.1),
        rop::ENV_THR => lin_norm(p.env_thresh, RO_THRESH.0, RO_THRESH.1),
        rop::ENV_GAIN => lin_norm(p.env_gain, RO_ENV_GAIN.0, RO_ENV_GAIN.1),
        rop::ENV_FREQ => log_norm(p.env_freq, RO_FREQ.0, RO_FREQ.1),
        rop::ENV_WIDTH => log_norm(p.env_width, RO_FB_W.0, RO_FB_W.1),
        rop::N_RATE => log_norm(p.noise_rate, RO_RATE.0, RO_RATE.1),
        rop::N_DIV => idx_norm(p.noise_div, rate_count(p.noise_mode)),
        rop::N_SMOOTH => p.noise_smooth.clamp(0.0, 1.0),
        rop::SC_MIX => p.sc.mix.clamp(0.0, 1.0),
        rop::SC_GAIN => lin_norm(p.sc.gain, RO_SC_GAIN.0, RO_SC_GAIN.1),
        rop::MOD_AMT => p.mod_amt.clamp(0.0, 1.0),
        100..=243 => {
            let c = (k - rop::MATRIX) as usize;
            lin_norm(p.matrix[c / MT][c % MT], -1.0, 1.0)
        }
        _ => return None,
    })
}

pub(crate) fn ro_set(p: &mut RoarParams, k: u8, v: f32) {
    let nd = SYNC_DIVS.len();
    match k {
        rop::DRIVE => p.drive = lin_val(v, RO_DRIVE.0, RO_DRIVE.1),
        rop::TONE_AMT => p.tone_amt = lin_val(v, -1.0, 1.0),
        rop::TONE_FREQ => p.tone_freq = log_val(v, RO_TONE_F.0, RO_TONE_F.1),
        rop::BLEND => p.blend = v.clamp(0.0, 1.0),
        rop::XO_LO => p.xo_lo = log_val(v, RO_XO.0, RO_XO.1).min(p.xo_hi),
        rop::XO_HI => p.xo_hi = log_val(v, RO_XO.0, RO_XO.1).max(p.xo_lo),
        6..=26 => {
            let s = &mut p.stages[((k - 6) / 7) as usize];
            match (k - 6) % 7 {
                rop::S_AMOUNT => s.amount = v.clamp(0.0, 1.0),
                rop::S_BIAS => s.bias = lin_val(v, -1.0, 1.0),
                rop::S_FREQ => s.freq = log_val(v, RO_FREQ.0, RO_FREQ.1),
                rop::S_RES => s.res = v.clamp(0.0, 1.0),
                rop::S_LEVEL => s.level = lin_val(v, RO_LEVEL.0, RO_LEVEL.1),
                rop::S_PEAK => s.peak = lin_val(v, RO_PEAK.0, RO_PEAK.1),
                _ => s.morph = v.clamp(0.0, 1.0),
            }
        }
        rop::FB_AMT => p.fb_amt = v.clamp(0.0, 1.0),
        rop::FB_TIME => p.fb_ms = log_val(v, RO_FB_MS.0, RO_FB_MS.1),
        rop::FB_FREQ => p.fb_freq = log_val(v, RO_FREQ.0, RO_FREQ.1),
        rop::COMPRESS => p.compress = v.clamp(0.0, 1.0),
        rop::DRY_WET => p.dry_wet = v.clamp(0.0, 1.0),
        rop::FB_WIDTH => p.fb_width = log_val(v, RO_FB_W.0, RO_FB_W.1),
        rop::OUTPUT => p.output = lin_val(v, RO_OUT.0, RO_OUT.1),
        rop::L1_RATE => p.lfo[0].rate = log_val(v, RO_RATE.0, RO_RATE.1),
        rop::L2_RATE => p.lfo[1].rate = log_val(v, RO_RATE.0, RO_RATE.1),
        rop::FB_DIV => p.fb_div = norm_idx(v, nd),
        rop::FB_NOTE => p.fb_note = lin_val(v, 0.0, 127.0).round(),
        rop::L1_DIV => p.lfo[0].div = norm_idx(v, rate_count(p.lfo[0].mode)),
        rop::L1_MORPH => p.lfo[0].morph = v.clamp(0.0, 1.0),
        rop::L1_SMOOTH => p.lfo[0].smooth = v.clamp(0.0, 1.0),
        rop::L2_DIV => p.lfo[1].div = norm_idx(v, rate_count(p.lfo[1].mode)),
        rop::L2_MORPH => p.lfo[1].morph = v.clamp(0.0, 1.0),
        rop::L2_SMOOTH => p.lfo[1].smooth = v.clamp(0.0, 1.0),
        rop::ENV_ATT => p.env_attack = log_val(v, RO_ENV_T.0, RO_ENV_T.1),
        rop::ENV_REL => p.env_release = log_val(v, RO_ENV_T.0, RO_ENV_T.1),
        rop::ENV_THR => p.env_thresh = lin_val(v, RO_THRESH.0, RO_THRESH.1),
        rop::ENV_GAIN => p.env_gain = lin_val(v, RO_ENV_GAIN.0, RO_ENV_GAIN.1),
        rop::ENV_FREQ => p.env_freq = log_val(v, RO_FREQ.0, RO_FREQ.1),
        rop::ENV_WIDTH => p.env_width = log_val(v, RO_FB_W.0, RO_FB_W.1),
        rop::N_RATE => p.noise_rate = log_val(v, RO_RATE.0, RO_RATE.1),
        rop::N_DIV => p.noise_div = norm_idx(v, rate_count(p.noise_mode)),
        rop::N_SMOOTH => p.noise_smooth = v.clamp(0.0, 1.0),
        rop::SC_MIX => p.sc.mix = v.clamp(0.0, 1.0),
        rop::SC_GAIN => p.sc.gain = lin_val(v, RO_SC_GAIN.0, RO_SC_GAIN.1),
        rop::MOD_AMT => p.mod_amt = v.clamp(0.0, 1.0),
        100..=243 => {
            let c = (k - rop::MATRIX) as usize;
            p.matrix[c / MT][c % MT] = lin_val(v, -1.0, 1.0);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips() {
        let mut r = RoarParams::default();
        for k in 0..250u8 {
            if let Some(v) = ro_norm(&r, k) {
                ro_set(&mut r, k, v);
                let w = ro_norm(&r, k).unwrap();
                assert!((v - w).abs() < 0.02, "roar {k}: {v} {w}");
            }
        }
        let mut p = PhaserFlangerParams::default();
        for k in 0..40u8 {
            if let Some(v) = pf_norm(&p, k) {
                pf_set(&mut p, k, v);
                assert!((v - pf_norm(&p, k).unwrap()).abs() < 0.02, "pf {k}");
            }
        }
        let mut c = CorpusParams::default();
        for k in 0..40u8 {
            if let Some(v) = co_norm(&c, k) {
                co_set(&mut c, k, v);
                assert!((v - co_norm(&c, k).unwrap()).abs() < 0.02, "corpus {k}");
            }
        }
        let mut t = TunerParams::default();
        let v = tu_norm(&t, tup::REFERENCE).unwrap();
        tu_set(&mut t, tup::REFERENCE, v);
        assert_eq!(t.reference, 440.0);
    }
}
