//! Audio effect devices (a track's device chain).

pub use daw_engine::eq8::{
    EQ_AVG_MAX, EQ_BLOCKS, EQ_FREQ_MAX, EQ_FREQ_MIN, EQ_GAIN_MAX, EQ_OUT_MAX, EQ_Q_MAX, EQ_Q_MIN, EQ_REFRESH_MAX, EQ_REFRESH_MIN,
    EQ_SCALE_MAX,
};
pub use daw_engine::fx::{
    ALIGN_DIST_MAX_M, ALIGN_SAMPLES_MAX, ALIGN_TEMP_MAX_C, ALIGN_TEMP_MIN_C, ALIGN_TIME_MAX_MS, BASS_MONO_MAX_HZ, BASS_MONO_MIN_HZ,
    FEET_PER_M, UTIL_GAIN_MAX_DB, UTIL_GAIN_MIN_DB, UTIL_WIDTH_MAX,
};
pub use daw_engine::{AlignDelayParams, ChannelMode, DelayMode, EqBand, EqEightParams, EqFilter, EqMode, FxParams, UtilityParams};
use serde::{Deserialize, Serialize};

/// One device in a chain.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub id: u64,
    /// Device Activator.
    #[serde(default = "yes")]
    pub on: bool,
    pub params: FxParams,
}

fn yes() -> bool {
    true
}

/// Audio effects offered by the browser (name, default parameters).
pub fn audio_effects() -> [(&'static str, FxParams); 3] {
    [
        ("Align Delay", FxParams::AlignDelay(AlignDelayParams::default())),
        ("EQ Eight", FxParams::EqEight(EqEightParams::default())),
        ("Utility", FxParams::Utility(UtilityParams::default())),
    ]
}

impl Device {
    pub fn new(id: u64, params: FxParams) -> Self {
        Self { id, on: true, params }
    }
}
