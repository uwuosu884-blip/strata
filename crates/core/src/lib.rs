//! # daw-core
//!
//! Everything that is not real-time and not GUI: sample decoding/caching,
//! factory sounds, compiling a `Project` into an engine `Song`, draining
//! recordings into takes, offline export and project persistence.

pub mod compile;
pub mod export;
pub mod factory;
pub mod persist;
pub mod recorder;
pub mod samples;

pub use compile::{IoMap, RtSlots, compile, compile_io};
pub use export::{ExportOptions, render_project, write_wav, write_wav_channels};
pub use persist::{load_project, save_project};
pub use recorder::{LiveLane, LiveTake, RecordedNote, Recorder, Take, TakeLane};
pub use samples::{SampleCache, decode_file, resample};
