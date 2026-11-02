//! Normalised parameter access for Resonators, Drum Buss, Spectral Resonator
//! and Spectral Time (ids: `rsp`, `dbp`, `srp`, `stp`; switches have ids ≥ 40
//! and no norm).

use crate::drumbuss::*;
use crate::params::{idx_norm, lin_norm, lin_val, log_norm, log_val, norm_idx, pow_norm, pow_val};
use crate::resonators::*;
use crate::specres::*;
use crate::spectime::*;

// ---- Resonators ----

pub mod rsp {
    pub const FREQ: u8 = 0;
    pub const DECAY: u8 = 1;
    pub const COLOR: u8 = 2;
    pub const NOTE: u8 = 3;
    /// Resonators II–V (index 1..5; [0] = NOTE).
    pub const PITCH: [u8; 5] = [3, 4, 5, 6, 7];
    pub const FINE: [u8; 5] = [8, 9, 10, 11, 12];
    pub const GAIN: [u8; 5] = [13, 14, 15, 16, 17];
    pub const WIDTH: u8 = 18;
    pub const OUTPUT: u8 = 19;
    pub const DRY_WET: u8 = 20;
    pub const FILTER_ON: u8 = 40;
    pub const FILTER: u8 = 41;
    pub const MODE: u8 = 42;
    pub const CONST: u8 = 43;
    pub const ON: [u8; 5] = [44, 45, 46, 47, 48];
    pub const SCALE: u8 = 49;
}

pub(crate) fn res_norm(p: &ResonatorsParams, k: u8) -> Option<f32> {
    Some(match k {
        rsp::FREQ => log_norm(p.freq, RES_FREQ.0, RES_FREQ.1),
        rsp::DECAY => p.decay.clamp(0.0, 1.0),
        rsp::COLOR => p.color.clamp(0.0, 1.0),
        rsp::NOTE => lin_norm(p.note, RES_NOTE.0, RES_NOTE.1),
        4..=7 => lin_norm(p.pitch[(k - 3) as usize], -RES_PITCH, RES_PITCH),
        8..=12 => lin_norm(p.fine[(k - 8) as usize], -RES_FINE, RES_FINE),
        13..=17 => lin_norm(p.gain[(k - 13) as usize], RES_GAIN.0, RES_GAIN.1),
        rsp::WIDTH => p.width.clamp(0.0, 1.0),
        rsp::OUTPUT => lin_norm(p.output, -RES_OUT, RES_OUT),
        rsp::DRY_WET => p.dry_wet.clamp(0.0, 1.0),
        _ => return None,
    })
}

pub(crate) fn res_set(p: &mut ResonatorsParams, k: u8, v: f32) {
    match k {
        rsp::FREQ => p.freq = log_val(v, RES_FREQ.0, RES_FREQ.1),
        rsp::DECAY => p.decay = v.clamp(0.0, 1.0),
        rsp::COLOR => p.color = v.clamp(0.0, 1.0),
        rsp::NOTE => p.note = lin_val(v, RES_NOTE.0, RES_NOTE.1).round(),
        4..=7 => p.pitch[(k - 3) as usize] = lin_val(v, -RES_PITCH, RES_PITCH).round(),
        8..=12 => p.fine[(k - 8) as usize] = lin_val(v, -RES_FINE, RES_FINE).round(),
        13..=17 => p.gain[(k - 13) as usize] = lin_val(v, RES_GAIN.0, RES_GAIN.1),
        rsp::WIDTH => p.width = v.clamp(0.0, 1.0),
        rsp::OUTPUT => p.output = lin_val(v, -RES_OUT, RES_OUT),
        rsp::DRY_WET => p.dry_wet = v.clamp(0.0, 1.0),
        _ => {}
    }
}

// ---- Drum Buss ----

pub mod dbp {
    pub const TRIM: u8 = 0;
    pub const DRIVE: u8 = 1;
    pub const CRUNCH: u8 = 2;
    pub const DAMP: u8 = 3;
    pub const TRANSIENTS: u8 = 4;
    pub const BOOM: u8 = 5;
    pub const FREQ: u8 = 6;
    pub const DECAY: u8 = 7;
    pub const OUTPUT: u8 = 8;
    pub const DRY_WET: u8 = 9;
    pub const COMP: u8 = 40;
    pub const TYPE: u8 = 41;
    pub const FORCE: u8 = 42;
    pub const AUDITION: u8 = 43;
}

pub(crate) fn db_norm(p: &DrumBussParams, k: u8) -> Option<f32> {
    Some(match k {
        dbp::TRIM => lin_norm(p.trim, DB_TRIM.0, DB_TRIM.1),
        dbp::DRIVE => p.drive.clamp(0.0, 1.0),
        dbp::CRUNCH => p.crunch.clamp(0.0, 1.0),
        dbp::DAMP => log_norm(p.damp, DB_DAMP.0, DB_DAMP.1),
        dbp::TRANSIENTS => lin_norm(p.transients, -1.0, 1.0),
        dbp::BOOM => p.boom.clamp(0.0, 1.0),
        dbp::FREQ => log_norm(p.freq, DB_FREQ.0, DB_FREQ.1),
        dbp::DECAY => p.decay.clamp(0.0, 1.0),
        dbp::OUTPUT => lin_norm(p.output, DB_OUT.0, DB_OUT.1),
        dbp::DRY_WET => p.dry_wet.clamp(0.0, 1.0),
        _ => return None,
    })
}

pub(crate) fn db_set(p: &mut DrumBussParams, k: u8, v: f32) {
    match k {
        dbp::TRIM => p.trim = lin_val(v, DB_TRIM.0, DB_TRIM.1),
        dbp::DRIVE => p.drive = v.clamp(0.0, 1.0),
        dbp::CRUNCH => p.crunch = v.clamp(0.0, 1.0),
        dbp::DAMP => p.damp = log_val(v, DB_DAMP.0, DB_DAMP.1),
        dbp::TRANSIENTS => p.transients = lin_val(v, -1.0, 1.0),
        dbp::BOOM => p.boom = v.clamp(0.0, 1.0),
        dbp::FREQ => p.freq = log_val(v, DB_FREQ.0, DB_FREQ.1),
        dbp::DECAY => p.decay = v.clamp(0.0, 1.0),
        dbp::OUTPUT => p.output = lin_val(v, DB_OUT.0, DB_OUT.1),
        dbp::DRY_WET => p.dry_wet = v.clamp(0.0, 1.0),
        _ => {}
    }
}

// ---- Spectral Resonator ----

pub mod srp {
    pub const FREQ: u8 = 0;
    pub const TRANSP: u8 = 1;
    pub const STRETCH: u8 = 2;
    pub const SHIFT: u8 = 3;
    pub const DECAY: u8 = 4;
    pub const HI_DAMP: u8 = 5;
    pub const LO_DAMP: u8 = 6;
    pub const MOD_RATE: u8 = 7;
    pub const PCH_MOD: u8 = 8;
    pub const HARMONICS: u8 = 9;
    pub const SEND: u8 = 10;
    pub const UNISON: u8 = 11;
    pub const UNI_AMT: u8 = 12;
    pub const DRY_WET: u8 = 13;
    pub const GLIDE: u8 = 14;
    pub const PB: u8 = 15;
    pub const VOICES: u8 = 16;
    pub const MODE: u8 = 40;
    pub const NOTE_MODE: u8 = 41;
    pub const POLY: u8 = 42;
    pub const GATE: u8 = 43;
    pub const MOD: u8 = 44;
    pub const QUANTIZE: u8 = 45;
    pub const SCALE: u8 = 46;
    pub const DISPLAY: u8 = 47;
    pub const SOURCE: u8 = 48;
    pub const TAP: u8 = 49;
}

pub(crate) fn sr_norm(p: &SpecResParams, k: u8) -> Option<f32> {
    Some(match k {
        srp::FREQ => lin_norm(hz_midi(p.freq), SR_MIDI.0, SR_MIDI.1),
        srp::TRANSP => lin_norm(p.transp, -SR_TRANSP, SR_TRANSP),
        srp::STRETCH => lin_norm(p.stretch, -1.0, 1.0),
        srp::SHIFT => lin_norm(p.shift, -SR_SHIFT, SR_SHIFT),
        srp::DECAY => pow_norm(p.decay, SR_DECAY.0, SR_DECAY.1, 3.0),
        srp::HI_DAMP => p.hi_damp.clamp(0.0, 1.0),
        srp::LO_DAMP => p.lo_damp.clamp(0.0, 1.0),
        srp::MOD_RATE => log_norm(p.mod_rate, SR_RATE.0, SR_RATE.1),
        srp::PCH_MOD => lin_norm(p.pch_mod, 0.0, SR_PCH),
        srp::HARMONICS => lin_norm(p.harmonics, SR_HARM.0, SR_HARM.1),
        srp::SEND => lin_norm(p.send, -SR_SEND, SR_SEND),
        srp::UNISON => lin_norm(p.unison, SR_UNISON.0, SR_UNISON.1),
        srp::UNI_AMT => p.uni_amt.clamp(0.0, 1.0),
        srp::DRY_WET => p.dry_wet.clamp(0.0, 1.0),
        srp::GLIDE => pow_norm(p.glide, SR_GLIDE.0, SR_GLIDE.1, 3.0),
        srp::PB => lin_norm(p.pb, 0.0, SR_PB),
        srp::VOICES => idx_norm(p.voices, SR_VOICES.len()),
        _ => return None,
    })
}

pub(crate) fn sr_set(p: &mut SpecResParams, k: u8, v: f32) {
    match k {
        srp::FREQ => {
            let m = lin_val(v, SR_MIDI.0, SR_MIDI.1);
            p.freq = midi_hz(if p.note_mode { m.round() } else { m });
        }
        srp::TRANSP => p.transp = lin_val(v, -SR_TRANSP, SR_TRANSP).round(),
        srp::STRETCH => p.stretch = lin_val(v, -1.0, 1.0),
        srp::SHIFT => p.shift = lin_val(v, -SR_SHIFT, SR_SHIFT),
        srp::DECAY => p.decay = pow_val(v, SR_DECAY.0, SR_DECAY.1, 3.0),
        srp::HI_DAMP => p.hi_damp = v.clamp(0.0, 1.0),
        srp::LO_DAMP => p.lo_damp = v.clamp(0.0, 1.0),
        srp::MOD_RATE => p.mod_rate = log_val(v, SR_RATE.0, SR_RATE.1),
        srp::PCH_MOD => p.pch_mod = lin_val(v, 0.0, SR_PCH),
        srp::HARMONICS => p.harmonics = lin_val(v, SR_HARM.0, SR_HARM.1).round(),
        srp::SEND => p.send = lin_val(v, -SR_SEND, SR_SEND),
        srp::UNISON => p.unison = lin_val(v, SR_UNISON.0, SR_UNISON.1).round(),
        srp::UNI_AMT => p.uni_amt = v.clamp(0.0, 1.0),
        srp::DRY_WET => p.dry_wet = v.clamp(0.0, 1.0),
        srp::GLIDE => p.glide = pow_val(v, SR_GLIDE.0, SR_GLIDE.1, 3.0),
        srp::PB => p.pb = lin_val(v, 0.0, SR_PB).round(),
        srp::VOICES => p.voices = norm_idx(v, SR_VOICES.len()),
        _ => {}
    }
}

// ---- Spectral Time ----

pub mod stp {
    pub const FADE_IN: u8 = 0;
    pub const FADE_OUT: u8 = 1;
    pub const XFADE: u8 = 2;
    pub const SENS: u8 = 3;
    pub const INTERVAL: u8 = 4;
    pub const TIME: u8 = 5;
    pub const FEEDBACK: u8 = 6;
    pub const SHIFT: u8 = 7;
    pub const STEREO: u8 = 8;
    pub const MIX: u8 = 9;
    pub const TILT: u8 = 10;
    pub const SPRAY: u8 = 11;
    pub const MASK: u8 = 12;
    pub const SEND: u8 = 13;
    pub const DRY_WET: u8 = 14;
    pub const FRZ_ON: u8 = 40;
    pub const RETRIG: u8 = 41;
    pub const SYNC: u8 = 42;
    pub const ENVELOPE: u8 = 43;
    pub const FREEZE: u8 = 44;
    pub const DLY_ON: u8 = 45;
    pub const MODE: u8 = 46;
    pub const RES: u8 = 47;
    pub const ORDER: u8 = 48;
    pub const DISPLAY: u8 = 49;
}

pub(crate) fn st_norm(p: &SpecTimeParams, k: u8) -> Option<f32> {
    Some(match k {
        stp::FADE_IN => pow_norm(p.fade_in, ST_FADE.0, ST_FADE.1, 2.0),
        stp::FADE_OUT => pow_norm(p.fade_out, ST_FADE.0, ST_FADE.1, 2.0),
        stp::XFADE => p.xfade.clamp(0.0, 1.0),
        stp::SENS => p.sensitivity.clamp(0.0, 1.0),
        stp::INTERVAL => idx_norm(p.interval, ST_DIVS.len()),
        stp::TIME => match p.time_mode {
            0 => log_norm(p.time, ST_TIME.0, ST_TIME.1),
            1 => idx_norm(p.div, ST_DIVS.len()),
            _ => lin_norm(p.steps, ST_STEPS.0, ST_STEPS.1),
        },
        stp::FEEDBACK => p.feedback.clamp(0.0, 1.0),
        stp::SHIFT => lin_norm(p.shift, -ST_SHIFT, ST_SHIFT),
        stp::STEREO => p.stereo.clamp(0.0, 1.0),
        stp::MIX => p.mix.clamp(0.0, 1.0),
        stp::TILT => lin_norm(p.tilt, -ST_TILT, ST_TILT),
        stp::SPRAY => lin_norm(p.spray, 0.0, ST_SPRAY),
        stp::MASK => lin_norm(p.mask, -1.0, 1.0),
        stp::SEND => lin_norm(p.send, ST_SEND.0, ST_SEND.1),
        stp::DRY_WET => p.dry_wet.clamp(0.0, 1.0),
        _ => return None,
    })
}

pub(crate) fn st_set(p: &mut SpecTimeParams, k: u8, v: f32) {
    match k {
        stp::FADE_IN => p.fade_in = pow_val(v, ST_FADE.0, ST_FADE.1, 2.0),
        stp::FADE_OUT => p.fade_out = pow_val(v, ST_FADE.0, ST_FADE.1, 2.0),
        stp::XFADE => p.xfade = v.clamp(0.0, 1.0),
        stp::SENS => p.sensitivity = v.clamp(0.0, 1.0),
        stp::INTERVAL => p.interval = norm_idx(v, ST_DIVS.len()),
        stp::TIME => match p.time_mode {
            0 => p.time = log_val(v, ST_TIME.0, ST_TIME.1),
            1 => p.div = norm_idx(v, ST_DIVS.len()),
            _ => p.steps = lin_val(v, ST_STEPS.0, ST_STEPS.1).round(),
        },
        stp::FEEDBACK => p.feedback = v.clamp(0.0, 1.0),
        stp::SHIFT => p.shift = lin_val(v, -ST_SHIFT, ST_SHIFT),
        stp::STEREO => p.stereo = v.clamp(0.0, 1.0),
        stp::MIX => p.mix = v.clamp(0.0, 1.0),
        stp::TILT => p.tilt = lin_val(v, -ST_TILT, ST_TILT),
        stp::SPRAY => p.spray = lin_val(v, 0.0, ST_SPRAY),
        stp::MASK => p.mask = lin_val(v, -1.0, 1.0),
        stp::SEND => p.send = lin_val(v, ST_SEND.0, ST_SEND.1),
        stp::DRY_WET => p.dry_wet = v.clamp(0.0, 1.0),
        _ => {}
    }
}
