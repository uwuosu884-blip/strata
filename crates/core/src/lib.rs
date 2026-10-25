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

pub use compile::{RtSlots, compile};
pub use export::{ExportOptions, render_project, write_wav};
pub use persist::{load_project, save_project};
pub use recorder::{RecordedNote, Recorder, Take};
pub use samples::{SampleCache, decode_file, resample};
