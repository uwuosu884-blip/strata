//! Normalised parameter access for Dynamic Tube, Vinyl Distortion, Shifter
//! and Vocoder (ids: `dtp`, `vdp`, `shp`, `vcp`; switches have ids ≥ 40 and
//! no norm, except the Vocoder band levels at 60..100).

use crate::dyntube::*;
use crate::modfx::SYNC_DIVS;
use crate::params::{idx_norm, lin_norm, lin_val, log_norm, log_val, norm_idx, pow_norm, pow_val};
use crate::shifter::*;
use crate::vinyl::*;
use crate::vocoder::*;

// ---- Dynamic Tube ----

pub mod dtp {
    pub const DRIVE: u8 = 0;
    pub const TONE: u8 = 1;
    pub const BIAS: u8 = 2;
    pub const ENVELOPE: u8 = 3;
    pub const ATTACK: u8 = 4;
    pub const RELEASE: u8 = 5;
    pub const OUTPUT: u8 = 6;
    pub const DRY_WET: u8 = 7;
    pub const TUBE: u8 = 40;
    pub const HQ: u8 = 41;
}

pub(crate) fn dt_norm(p: &DynTubeParams, k: u8) -> Option<f32> {
    Some(match k {
        dtp::DRIVE => lin_norm(p.drive, DT_DRIVE.0, DT_DRIVE.1),
        dtp::TONE => lin_norm(p.tone, -1.0, 1.0),
        dtp::BIAS => p.bias.clamp(0.0, 1.0),
        dtp::ENVELOPE => lin_norm(p.envelope, -DT_ENV, DT_ENV),
        dtp::ATTACK => log_norm(p.attack, DT_ATTACK.0, DT_ATTACK.1),
        dtp::RELEASE => log_norm(p.release, DT_RELEASE.0, DT_RELEASE.1),
        dtp::OUTPUT => lin_norm(p.output, DT_OUT.0, DT_OUT.1),
        dtp::DRY_WET => p.dry_wet.clamp(0.0, 1.0),
        _ => return None,
    })
}

pub(crate) fn dt_set(p: &mut DynTubeParams, k: u8, v: f32) {
    match k {
        dtp::DRIVE => p.drive = lin_val(v, DT_DRIVE.0, DT_DRIVE.1),
        dtp::TONE => p.tone = lin_val(v, -1.0, 1.0),
        dtp::BIAS => p.bias = v.clamp(0.0, 1.0),
        dtp::ENVELOPE => p.envelope = lin_val(v, -DT_ENV, DT_ENV),
        dtp::ATTACK => p.attack = log_val(v, DT_ATTACK.0, DT_ATTACK.1),
        dtp::RELEASE => p.release = log_val(v, DT_RELEASE.0, DT_RELEASE.1),
        dtp::OUTPUT => p.output = lin_val(v, DT_OUT.0, DT_OUT.1),
        dtp::DRY_WET => p.dry_wet = v.clamp(0.0, 1.0),
        _ => {}
    }
}

// ---- Vinyl Distortion ----

pub mod vdp {
    pub const TR_DRIVE: u8 = 0;
    pub const TR_FREQ: u8 = 1;
    pub const TR_B: u8 = 2;
    pub const PI_DRIVE: u8 = 3;
    pub const PI_FREQ: u8 = 4;
    pub const PI_B: u8 = 5;
    pub const VOLUME: u8 = 6;
    pub const DENSITY: u8 = 7;
    pub const DRIVE: u8 = 8;
    pub const TR_ON: u8 = 40;
    pub const PI_ON: u8 = 41;
    pub const SOFT: u8 = 42;
    pub const STEREO: u8 = 43;
}

pub(crate) fn vd_norm(p: &VinylParams, k: u8) -> Option<f32> {
    Some(match k {
        vdp::TR_DRIVE => p.tr_drive.clamp(0.0, 1.0),
        vdp::TR_FREQ => log_norm(p.tr_freq, VD_FREQ.0, VD_FREQ.1),
        vdp::TR_B => log_norm(p.tr_b, VD_B.0, VD_B.1),
        vdp::PI_DRIVE => p.pi_drive.clamp(0.0, 1.0),
        vdp::PI_FREQ => log_norm(p.pi_freq, VD_FREQ.0, VD_FREQ.1),
        vdp::PI_B => log_norm(p.pi_b, VD_B.0, VD_B.1),
        vdp::VOLUME => lin_norm(p.volume, VD_VOLUME.0, VD_VOLUME.1),
        vdp::DENSITY => lin_norm(p.density, VD_DENSITY.0, VD_DENSITY.1),
        vdp::DRIVE => p.drive.clamp(0.0, 1.0),
        _ => return None,
    })
}

pub(crate) fn vd_set(p: &mut VinylParams, k: u8, v: f32) {
    match k {
        vdp::TR_DRIVE => p.tr_drive = v.clamp(0.0, 1.0),
        vdp::TR_FREQ => p.tr_freq = log_val(v, VD_FREQ.0, VD_FREQ.1),
        vdp::TR_B => p.tr_b = log_val(v, VD_B.0, VD_B.1),
        vdp::PI_DRIVE => p.pi_drive = v.clamp(0.0, 1.0),
        vdp::PI_FREQ => p.pi_freq = log_val(v, VD_FREQ.0, VD_FREQ.1),
        vdp::PI_B => p.pi_b = log_val(v, VD_B.0, VD_B.1),
        vdp::VOLUME => p.volume = lin_val(v, VD_VOLUME.0, VD_VOLUME.1),
        vdp::DENSITY => p.density = lin_val(v, VD_DENSITY.0, VD_DENSITY.1),
        vdp::DRIVE => p.drive = v.clamp(0.0, 1.0),
        _ => {}
    }
}

// ---- Shifter ----

pub mod shp {
    pub const COARSE: u8 = 0;
    pub const COARSE_HZ: u8 = 1;
    pub const FINE: u8 = 2;
    pub const SPREAD: u8 = 3;
    pub const WINDOW: u8 = 4;
    pub const TONE: u8 = 5;
    pub const DELAY_MS: u8 = 6;
    pub const DELAY_DIV: u8 = 7;
    pub const FEEDBACK: u8 = 8;
    pub const RATE: u8 = 9;
    pub const LFO_DIV: u8 = 10;
    pub const LFO_AMT: u8 = 11;
    pub const DUTY: u8 = 12;
    pub const PHASE: u8 = 13;
    pub const OFFSET: u8 = 14;
    pub const ENV_AMT: u8 = 15;
    pub const ENV_ATTACK: u8 = 16;
    pub const ENV_RELEASE: u8 = 17;
    pub const OUTPUT: u8 = 18;
    pub const DRY_WET: u8 = 19;
    pub const GLIDE: u8 = 20;
    pub const PB: u8 = 21;
    pub const MODE: u8 = 40;
    pub const WIDE: u8 = 41;
    pub const DELAY_ON: u8 = 42;
    pub const DELAY_SYNC: u8 = 43;
    pub const WAVE: u8 = 44;
    pub const LFO_SYNC: u8 = 45;
    pub const SPIN: u8 = 46;
    pub const ENV_ON: u8 = 47;
    pub const DRIVE_ON: u8 = 48;
    pub const MIDI_MODE: u8 = 49;
    pub const SOURCE: u8 = 50;
    pub const TAP: u8 = 51;
    pub const OPEN: u8 = 52;
}

pub fn hz_norm(hz: f32) -> f32 {
    let x = (hz / SH_HZ).clamp(-1.0, 1.0);
    (x.signum() * x.abs().cbrt() + 1.0) * 0.5
}

pub fn norm_hz(v: f32) -> f32 {
    let x = v.clamp(0.0, 1.0) * 2.0 - 1.0;
    SH_HZ * x * x * x
}

pub(crate) fn sh_norm(p: &ShifterParams, k: u8) -> Option<f32> {
    let nd = SYNC_DIVS.len();
    Some(match k {
        shp::COARSE => lin_norm(p.coarse, -SH_COARSE, SH_COARSE),
        shp::COARSE_HZ => hz_norm(p.coarse_hz),
        shp::FINE => lin_norm(p.fine, -SH_FINE, SH_FINE),
        shp::SPREAD => lin_norm(p.spread, -SH_FINE, SH_FINE),
        shp::WINDOW => log_norm(p.window, SH_WINDOW.0, SH_WINDOW.1),
        shp::TONE => log_norm(p.tone, SH_TONE.0, SH_TONE.1),
        shp::DELAY_MS => log_norm(p.delay_ms, SH_DELAY_MS.0, SH_DELAY_MS.1),
        shp::DELAY_DIV => idx_norm(p.delay_div, nd),
        shp::FEEDBACK => p.feedback.clamp(0.0, 1.0),
        shp::RATE => log_norm(p.rate, SH_RATE.0, SH_RATE.1),
        shp::LFO_DIV => idx_norm(p.lfo_div, nd),
        shp::LFO_AMT => p.lfo_amt.clamp(0.0, 1.0),
        shp::DUTY => lin_norm(p.duty, -1.0, 1.0),
        shp::PHASE => p.phase.clamp(0.0, 1.0),
        shp::OFFSET => p.offset.clamp(0.0, 1.0),
        shp::ENV_AMT => lin_norm(p.env_amt, -1.0, 1.0),
        shp::ENV_ATTACK => log_norm(p.env_attack, SH_ENV_T.0, SH_ENV_T.1),
        shp::ENV_RELEASE => log_norm(p.env_release, SH_ENV_T.0, SH_ENV_T.1),
        shp::OUTPUT => lin_norm(p.output, SH_OUT.0, SH_OUT.1),
        shp::DRY_WET => p.dry_wet.clamp(0.0, 1.0),
        shp::GLIDE => pow_norm(p.glide, SH_GLIDE.0, SH_GLIDE.1, 3.0),
        shp::PB => lin_norm(p.pb, 0.0, 24.0),
        _ => return None,
    })
}

pub(crate) fn sh_set(p: &mut ShifterParams, k: u8, v: f32) {
    let nd = SYNC_DIVS.len();
    match k {
        shp::COARSE => p.coarse = lin_val(v, -SH_COARSE, SH_COARSE).round(),
        shp::COARSE_HZ => p.coarse_hz = norm_hz(v),
        shp::FINE => p.fine = lin_val(v, -SH_FINE, SH_FINE),
        shp::SPREAD => p.spread = lin_val(v, -SH_FINE, SH_FINE),
        shp::WINDOW => p.window = log_val(v, SH_WINDOW.0, SH_WINDOW.1),
        shp::TONE => p.tone = log_val(v, SH_TONE.0, SH_TONE.1),
        shp::DELAY_MS => p.delay_ms = log_val(v, SH_DELAY_MS.0, SH_DELAY_MS.1),
        shp::DELAY_DIV => p.delay_div = norm_idx(v, nd),
        shp::FEEDBACK => p.feedback = v.clamp(0.0, 1.0),
        shp::RATE => p.rate = log_val(v, SH_RATE.0, SH_RATE.1),
        shp::LFO_DIV => p.lfo_div = norm_idx(v, nd),
        shp::LFO_AMT => p.lfo_amt = v.clamp(0.0, 1.0),
        shp::DUTY => p.duty = lin_val(v, -1.0, 1.0),
        shp::PHASE => p.phase = v.clamp(0.0, 1.0),
        shp::OFFSET => p.offset = v.clamp(0.0, 1.0),
        shp::ENV_AMT => p.env_amt = lin_val(v, -1.0, 1.0),
        shp::ENV_ATTACK => p.env_attack = log_val(v, SH_ENV_T.0, SH_ENV_T.1),
        shp::ENV_RELEASE => p.env_release = log_val(v, SH_ENV_T.0, SH_ENV_T.1),
        shp::OUTPUT => p.output = lin_val(v, SH_OUT.0, SH_OUT.1),
        shp::DRY_WET => p.dry_wet = v.clamp(0.0, 1.0),
        shp::GLIDE => p.glide = pow_val(v, SH_GLIDE.0, SH_GLIDE.1, 3.0),
        shp::PB => p.pb = lin_val(v, 0.0, 24.0).round(),
        _ => {}
    }
}

// ---- Vocoder ----

pub mod vcp {
    pub const UNVOICED: u8 = 0;
    pub const SENS: u8 = 1;
    pub const NOISE_RATE: u8 = 2;
    pub const NOISE_DENS: u8 = 3;
    pub const TRACK_LO: u8 = 4;
    pub const TRACK_HI: u8 = 5;
    pub const PITCH: u8 = 6;
    pub const BANDS: u8 = 7;
    pub const LO: u8 = 8;
    pub const HI: u8 = 9;
    pub const BW: u8 = 10;
    pub const GATE: u8 = 11;
    pub const LEVEL: u8 = 12;
    pub const DEPTH: u8 = 13;
    pub const ATTACK: u8 = 14;
    pub const RELEASE: u8 = 15;
    pub const FORMANT: u8 = 16;
    pub const DRY_WET: u8 = 17;
    pub const CARRIER: u8 = 40;
    pub const ENHANCE: u8 = 41;
    pub const WAVE: u8 = 42;
    pub const FAST: u8 = 43;
    pub const RETRO: u8 = 44;
    pub const CHAN: u8 = 45;
    pub const SOURCE: u8 = 46;
    pub const TAP: u8 = 47;
    pub const XY: u8 = 48;
    /// Band levels 60..100.
    pub const BAND_LV: u8 = 60;
}

pub(crate) fn vc_norm(p: &VocoderParams, k: u8) -> Option<f32> {
    Some(match k {
        vcp::UNVOICED => lin_norm(p.unvoiced, VC_UNVOICED.0, VC_UNVOICED.1),
        vcp::SENS => p.sens.clamp(0.0, 1.0),
        vcp::NOISE_RATE => p.noise_rate.clamp(0.0, 1.0),
        vcp::NOISE_DENS => p.noise_density.clamp(0.0, 1.0),
        vcp::TRACK_LO => log_norm(p.track_lo, VC_TRACK.0, VC_TRACK.1),
        vcp::TRACK_HI => log_norm(p.track_hi, VC_TRACK.0, VC_TRACK.1),
        vcp::PITCH => lin_norm(p.pitch, -VC_PITCH, VC_PITCH),
        vcp::BANDS => idx_norm(p.bands, VC_BANDS.len()),
        vcp::LO => log_norm(p.lo, VC_RANGE.0, VC_RANGE.1),
        vcp::HI => log_norm(p.hi, VC_RANGE.0, VC_RANGE.1),
        vcp::BW => lin_norm(p.bw, VC_BW.0, VC_BW.1),
        vcp::GATE => lin_norm(p.gate, VC_GATE.0, VC_GATE.1),
        vcp::LEVEL => lin_norm(p.level, VC_LEVEL.0, VC_LEVEL.1),
        vcp::DEPTH => lin_norm(p.depth, 0.0, 2.0),
        vcp::ATTACK => log_norm(p.attack, VC_ATTACK.0, VC_ATTACK.1),
        vcp::RELEASE => pow_norm(p.release, VC_RELEASE.0, VC_RELEASE.1, 2.8),
        vcp::FORMANT => lin_norm(p.formant, -VC_FORMANT, VC_FORMANT),
        vcp::DRY_WET => p.dry_wet.clamp(0.0, 1.0),
        60..100 => p.band_lv[(k - 60) as usize].clamp(0.0, 1.0),
        _ => return None,
    })
}

pub(crate) fn vc_set(p: &mut VocoderParams, k: u8, v: f32) {
    match k {
        vcp::UNVOICED => p.unvoiced = lin_val(v, VC_UNVOICED.0, VC_UNVOICED.1),
        vcp::SENS => p.sens = v.clamp(0.0, 1.0),
        vcp::NOISE_RATE => p.noise_rate = v.clamp(0.0, 1.0),
        vcp::NOISE_DENS => p.noise_density = v.clamp(0.0, 1.0),
        vcp::TRACK_LO => p.track_lo = log_val(v, VC_TRACK.0, VC_TRACK.1),
        vcp::TRACK_HI => p.track_hi = log_val(v, VC_TRACK.0, VC_TRACK.1),
        vcp::PITCH => p.pitch = lin_val(v, -VC_PITCH, VC_PITCH).round(),
        vcp::BANDS => p.bands = norm_idx(v, VC_BANDS.len()),
        vcp::LO => p.lo = log_val(v, VC_RANGE.0, VC_RANGE.1),
        vcp::HI => p.hi = log_val(v, VC_RANGE.0, VC_RANGE.1),
        vcp::BW => p.bw = lin_val(v, VC_BW.0, VC_BW.1),
        vcp::GATE => p.gate = lin_val(v, VC_GATE.0, VC_GATE.1),
        vcp::LEVEL => p.level = lin_val(v, VC_LEVEL.0, VC_LEVEL.1),
        vcp::DEPTH => p.depth = lin_val(v, 0.0, 2.0),
        vcp::ATTACK => p.attack = log_val(v, VC_ATTACK.0, VC_ATTACK.1),
        vcp::RELEASE => p.release = pow_val(v, VC_RELEASE.0, VC_RELEASE.1, 2.8),
        vcp::FORMANT => p.formant = lin_val(v, -VC_FORMANT, VC_FORMANT),
        vcp::DRY_WET => p.dry_wet = v.clamp(0.0, 1.0),
        60..100 => p.band_lv[(k - 60) as usize] = v.clamp(0.0, 1.0),
        _ => {}
    }
}
