//! Spectrum (Live 12 manual 29.38): an analyzer that passes audio through.
//! The audio thread only feeds the device's [`SpecTap`] (L, R or L+R);
//! the GUI runs the FFT, averaging, max hold and drawing.

use crate::eq8::SpecTap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// FFT block sizes (samples).
pub const SPEC_BLOCKS: [u32; 8] = [256, 512, 1024, 2048, 4096, 8192, 16384, 32768];
/// Refresh rate (ms).
pub const SPEC_REFRESH: (f32, f32) = (10.0, 500.0);
pub const SPEC_AVG_MAX: f32 = 100.0;
/// Manual range limits (dB).
pub const SPEC_DB: (f32, f32) = (-120.0, 24.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SpecChannel {
    L,
    R,
    #[default]
    LR,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SpecScale {
    Lin,
    #[default]
    Log,
    /// Semitones: a log axis labelled in notes, one bar per semitone.
    St,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpectrumParams {
    pub block: u32,
    pub channel: SpecChannel,
    pub refresh: f32,
    pub avg: f32,
    /// Graph: line (else bins).
    pub line: bool,
    /// Max hold line.
    pub max: bool,
    pub scale: SpecScale,
    /// Auto range (else `hi` / `lo`).
    pub auto: bool,
    pub hi: f32,
    pub lo: f32,
    /// Display unfolded into the main window.
    pub expanded: bool,
}

impl Default for SpectrumParams {
    fn default() -> Self {
        Self {
            block: 8192,
            channel: SpecChannel::LR,
            refresh: 60.0,
            avg: 1.0,
            line: true,
            max: true,
            scale: SpecScale::Log,
            auto: true,
            hi: 0.0,
            lo: -74.0,
            expanded: false,
        }
    }
}

/// Feeds the tap; the signal is untouched.
#[derive(Debug, Clone, Default)]
pub struct SpectrumState {
    pub(crate) tap: Option<Arc<SpecTap>>,
}

impl SpectrumState {
    pub fn new(tap: Option<Arc<SpecTap>>) -> Self {
        Self { tap }
    }

    pub fn process(&mut self, p: &SpectrumParams, l: &[f32], r: &[f32], sr: f32) {
        let Some(t) = &self.tap else { return };
        let n = l.len().min(r.len());
        match p.channel {
            SpecChannel::L => t.write(l, l, n, sr),
            SpecChannel::R => t.write(r, r, n, sr),
            SpecChannel::LR => t.write(l, r, n, sr),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spectrum_passes_audio_and_feeds_the_chosen_channel() {
        let tap = crate::eq8::spectrum_tap(u64::MAX - 77);
        let mut st = SpectrumState::new(Some(tap.clone()));
        let l = vec![1.0f32; 64];
        let r = vec![0.0f32; 64];
        let mut p = SpectrumParams { channel: SpecChannel::R, ..Default::default() };
        st.process(&p, &l, &r, 48_000.0);
        let mut out = vec![9.0; 64];
        tap.read(&mut out);
        assert!(out.iter().all(|&x| x == 0.0));
        p.channel = SpecChannel::L;
        st.process(&p, &l, &r, 48_000.0);
        tap.read(&mut out);
        assert!(out.iter().all(|&x| x == 1.0));
        p.channel = SpecChannel::LR;
        st.process(&p, &l, &r, 48_000.0);
        tap.read(&mut out);
        assert!(out.iter().all(|&x| x == 0.5));
        assert_eq!(tap.written(), 192);
    }
}
