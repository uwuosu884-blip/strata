//! # daw-project
//!
//! The document the user edits and saves. This crate knows nothing about
//! audio threads: the GUI edits a `Project`, and changes are turned into
//! engine commands. Files are human-readable RON so they are easy to inspect,
//! diff and debug.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// File format version; bump when the layout changes incompatibly.
pub const FORMAT_VERSION: u32 = 1;
pub const FILE_EXTENSION: &str = "dawproj";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    pub format_version: u32,
    pub name: String,
    pub tempo_bpm: f64,
    pub beats_per_bar: u32,
    pub metronome: bool,
    pub master_gain_db: f32,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            format_version: FORMAT_VERSION,
            name: "Untitled".into(),
            tempo_bpm: 120.0,
            beats_per_bar: 4,
            metronome: true,
            master_gain_db: 0.0,
        }
    }
}

#[derive(Debug)]
pub enum ProjectError {
    Io(std::io::Error),
    Parse(ron::error::SpannedError),
    Serialize(ron::Error),
    UnsupportedVersion(u32),
}

impl std::fmt::Display for ProjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "file error: {e}"),
            Self::Parse(e) => write!(f, "invalid project file: {e}"),
            Self::Serialize(e) => write!(f, "could not serialize project: {e}"),
            Self::UnsupportedVersion(v) => write!(
                f,
                "project format version {v} is newer than this app supports ({FORMAT_VERSION})"
            ),
        }
    }
}

impl std::error::Error for ProjectError {}

impl Project {
    pub fn to_ron(&self) -> Result<String, ProjectError> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).map_err(ProjectError::Serialize)
    }

    pub fn from_ron(text: &str) -> Result<Self, ProjectError> {
        let project: Project = ron::from_str(text).map_err(ProjectError::Parse)?;
        if project.format_version > FORMAT_VERSION {
            return Err(ProjectError::UnsupportedVersion(project.format_version));
        }
        Ok(project)
    }

    pub fn load(path: &Path) -> Result<Self, ProjectError> {
        Self::from_ron(&std::fs::read_to_string(path).map_err(ProjectError::Io)?)
    }

    /// Writes to a temporary file first, then renames: a crash mid-save can
    /// never leave a half-written project behind.
    pub fn save(&self, path: &Path) -> Result<(), ProjectError> {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, self.to_ron()?).map_err(ProjectError::Io)?;
        std::fs::rename(&tmp, path).map_err(ProjectError::Io)
    }

    pub fn master_gain_linear(&self) -> f32 {
        db_to_gain(self.master_gain_db)
    }
}

/// Decibels to linear gain. -60 dB and below is treated as silence.
pub fn db_to_gain(db: f32) -> f32 {
    if db <= -60.0 { 0.0 } else { 10f32.powf(db / 20.0) }
}

/// Linear gain to decibels (floored at -100 dB).
pub fn gain_to_db(gain: f32) -> f32 {
    if gain <= 1.0e-5 { -100.0 } else { 20.0 * gain.log10() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ron_roundtrip() {
        let p = Project { name: "Jam".into(), tempo_bpm: 128.5, metronome: false, ..Default::default() };
        assert_eq!(Project::from_ron(&p.to_ron().unwrap()).unwrap(), p);
    }

    #[test]
    fn missing_fields_use_defaults() {
        let p = Project::from_ron("(tempo_bpm: 90.0)").unwrap();
        assert_eq!(p.tempo_bpm, 90.0);
        assert_eq!(p.beats_per_bar, 4);
    }

    #[test]
    fn newer_format_is_rejected() {
        let err = Project::from_ron("(format_version: 999)").unwrap_err();
        assert!(matches!(err, ProjectError::UnsupportedVersion(999)));
    }

    #[test]
    fn save_and_load_file() {
        let dir = std::env::temp_dir().join(format!("daw-project-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("song.dawproj");
        let p = Project { tempo_bpm: 99.0, ..Default::default() };
        p.save(&path).unwrap();
        assert_eq!(Project::load(&path).unwrap(), p);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn decibel_conversions() {
        assert_eq!(db_to_gain(0.0), 1.0);
        assert!((db_to_gain(-6.0) - 0.501).abs() < 1e-3);
        assert_eq!(db_to_gain(-60.0), 0.0);
        assert!((gain_to_db(0.5) + 6.02).abs() < 0.01);
    }
}
