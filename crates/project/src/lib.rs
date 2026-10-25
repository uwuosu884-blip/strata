//! # daw-project
//!
//! The document the user edits and saves (RON). Pure data + editing helpers
//! + undo history. Knows nothing about audio threads.

mod history;
pub use history::History;

pub use daw_engine::{AdsrParams, Note, SamplerParams, SynthParams, Wave};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const FORMAT_VERSION: u32 = 2;
pub const FILE_EXTENSION: &str = "dawproj";
/// Prefix for built-in (synthesised) sounds, e.g. `factory:Kick`.
pub const FACTORY_PREFIX: &str = "factory:";
/// Prefix for audio recorded this session and not yet saved to disk.
pub const RECORDING_PREFIX: &str = "recording:";

pub const PALETTE: [[u8; 3]; 12] = [
    [255, 148, 72],
    [255, 210, 80],
    [190, 230, 90],
    [100, 210, 120],
    [80, 200, 200],
    [90, 160, 255],
    [150, 130, 255],
    [220, 120, 230],
    [255, 110, 140],
    [200, 160, 110],
    [160, 170, 180],
    [240, 240, 240],
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    pub format_version: u32,
    pub name: String,
    pub tempo_bpm: f64,
    pub beats_per_bar: u32,
    pub metronome: bool,
    pub master_gain_db: f32,
    /// Launch quantization in beats (0 = none).
    pub launch_quantize: f64,
    pub loop_enabled: bool,
    pub loop_start: f64,
    pub loop_end: f64,
    pub scenes: Vec<Scene>,
    pub tracks: Vec<Track>,
    pub next_id: u64,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            format_version: FORMAT_VERSION,
            name: "Untitled".into(),
            tempo_bpm: 120.0,
            beats_per_bar: 4,
            metronome: false,
            master_gain_db: 0.0,
            launch_quantize: 4.0,
            loop_enabled: false,
            loop_start: 0.0,
            loop_end: 16.0,
            scenes: Vec::new(),
            tracks: Vec::new(),
            next_id: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Scene {
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TrackKind {
    Audio,
    #[default]
    Midi,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Track {
    pub id: u64,
    pub name: String,
    pub kind: TrackKind,
    pub color: [u8; 3],
    pub gain_db: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub arm: bool,
    pub monitor: bool,
    pub instrument: InstrumentDef,
    /// Session slots; always `scenes.len()` long (see [`Project::normalize`]).
    pub slots: Vec<Option<Clip>>,
    pub arrangement: Vec<ArrangementClip>,
}

impl Default for Track {
    fn default() -> Self {
        Self {
            id: 0,
            name: "Track".into(),
            kind: TrackKind::Midi,
            color: PALETTE[0],
            gain_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            arm: false,
            monitor: false,
            instrument: InstrumentDef::None,
            slots: Vec::new(),
            arrangement: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum InstrumentDef {
    #[default]
    None,
    Synth(SynthParams),
    Sampler { sample: Option<String>, params: SamplerParams },
    Drums { pads: Vec<PadDef> },
}

impl InstrumentDef {
    pub fn label(&self) -> &'static str {
        match self {
            Self::None => "No instrument",
            Self::Synth(_) => "Synth",
            Self::Sampler { .. } => "Sampler",
            Self::Drums { .. } => "Drum Rack",
        }
    }

    /// The factory drum kit (sounds synthesised by daw-core).
    pub fn factory_drums() -> Self {
        let names = ["Kick", "Snare", "Clap", "Closed Hat", "Open Hat", "Low Tom", "Hi Tom", "Rim", "Cowbell", "Crash"];
        let pads = names
            .iter()
            .map(|n| PadDef {
                name: n.to_string(),
                sample: Some(format!("{FACTORY_PREFIX}{n}")),
                gain: match *n {
                    "Kick" => 0.9,
                    "Snare" => 0.75,
                    "Closed Hat" | "Open Hat" | "Cowbell" | "Crash" => 0.4,
                    _ => 0.6,
                },
                tune_semitones: 0.0,
                choke: if n.ends_with("Hat") { 1 } else { 0 },
            })
            .collect();
        Self::Drums { pads }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PadDef {
    pub name: String,
    pub sample: Option<String>,
    pub gain: f32,
    pub tune_semitones: f32,
    pub choke: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Clip {
    pub id: u64,
    pub name: String,
    pub color: [u8; 3],
    pub length: f64,
    pub looping: bool,
    pub content: ClipContent,
}

impl Default for Clip {
    fn default() -> Self {
        Self { id: 0, name: "Clip".into(), color: PALETTE[0], length: 4.0, looping: true, content: ClipContent::Midi { notes: Vec::new() } }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClipContent {
    Audio {
        /// File path (absolute or relative to the project file), `factory:` or `recording:` reference.
        sample: String,
        offset_frames: f64,
        warp_bpm: Option<f64>,
        gain_db: f32,
    },
    Midi {
        notes: Vec<Note>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ArrangementClip {
    pub start: f64,
    pub clip: Clip,
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
            Self::UnsupportedVersion(v) => {
                write!(f, "project format version {v} is newer than this app supports ({FORMAT_VERSION})")
            }
        }
    }
}

impl std::error::Error for ProjectError {}

impl Project {
    /// A new project laid out like Live's default set.
    pub fn new_default() -> Self {
        let mut p = Self::default();
        for _ in 0..8 {
            p.add_scene();
        }
        let drums = p.add_track(TrackKind::Midi);
        p.tracks[drums].name = "Drums".into();
        p.tracks[drums].instrument = InstrumentDef::factory_drums();
        let bass = p.add_track(TrackKind::Midi);
        p.tracks[bass].name = "Bass".into();
        p.tracks[bass].instrument = InstrumentDef::Synth(SynthParams::preset("Bass"));
        let keys = p.add_track(TrackKind::Midi);
        p.tracks[keys].name = "Keys".into();
        p.tracks[keys].instrument = InstrumentDef::Synth(SynthParams::preset("Pad"));
        let audio = p.add_track(TrackKind::Audio);
        p.tracks[audio].name = "Audio".into();
        p
    }

    pub fn alloc_id(&mut self) -> u64 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }

    pub fn add_scene(&mut self) -> usize {
        let n = self.scenes.len();
        self.scenes.push(Scene { name: format!("{}", n + 1) });
        self.normalize();
        n
    }

    pub fn insert_scene(&mut self, at: usize) {
        let at = at.min(self.scenes.len());
        self.scenes.insert(at, Scene { name: format!("{}", at + 1) });
        for t in &mut self.tracks {
            if at <= t.slots.len() {
                t.slots.insert(at, None);
            }
        }
        self.normalize();
    }

    pub fn remove_scene(&mut self, at: usize) {
        if at < self.scenes.len() {
            self.scenes.remove(at);
            for t in &mut self.tracks {
                if at < t.slots.len() {
                    t.slots.remove(at);
                }
            }
        }
    }

    pub fn add_track(&mut self, kind: TrackKind) -> usize {
        let id = self.alloc_id();
        let n = self.tracks.len();
        let name = match kind {
            TrackKind::Audio => format!("{} Audio", n + 1),
            TrackKind::Midi => format!("{} MIDI", n + 1),
        };
        let instrument = match kind {
            TrackKind::Audio => InstrumentDef::None,
            TrackKind::Midi => InstrumentDef::Synth(SynthParams::default()),
        };
        self.tracks.push(Track { id, name, kind, color: PALETTE[n % PALETTE.len()], instrument, ..Default::default() });
        self.normalize();
        n
    }

    pub fn duplicate_track(&mut self, idx: usize) -> Option<usize> {
        let mut t = self.tracks.get(idx)?.clone();
        t.id = self.alloc_id();
        t.name = format!("{} copy", t.name);
        t.solo = false;
        t.arm = false;
        for c in t.slots.iter_mut().flatten() {
            c.id = self.alloc_id();
        }
        for a in &mut t.arrangement {
            a.clip.id = self.alloc_id();
        }
        self.tracks.insert(idx + 1, t);
        Some(idx + 1)
    }

    /// Make every track's slot list match the scene count, fix ids.
    pub fn normalize(&mut self) {
        let n = self.scenes.len();
        for t in &mut self.tracks {
            t.slots.resize(n, None);
            t.arrangement.sort_by(|a, b| a.start.total_cmp(&b.start));
        }
        let max_id = self
            .tracks
            .iter()
            .flat_map(|t| {
                std::iter::once(t.id)
                    .chain(t.slots.iter().flatten().map(|c| c.id))
                    .chain(t.arrangement.iter().map(|a| a.clip.id))
            })
            .max()
            .unwrap_or(0);
        self.next_id = self.next_id.max(max_id + 1);
    }

    pub fn track_index(&self, id: u64) -> Option<usize> {
        self.tracks.iter().position(|t| t.id == id)
    }

    /// Create an empty MIDI clip in a session slot. Returns the clip id.
    pub fn new_midi_clip(&mut self, track: usize, scene: usize, length: f64) -> Option<u64> {
        let id = self.alloc_id();
        let t = self.tracks.get_mut(track)?;
        let slot = t.slots.get_mut(scene)?;
        *slot = Some(Clip { id, name: format!("Clip {id}"), color: t.color, length, looping: true, content: ClipContent::Midi { notes: Vec::new() } });
        Some(id)
    }

    /// End of the arrangement (beats).
    pub fn arrangement_end(&self) -> f64 {
        self.tracks.iter().flat_map(|t| t.arrangement.iter().map(|a| a.start + a.clip.length)).fold(0.0, f64::max)
    }

    pub fn any_solo(&self) -> bool {
        self.tracks.iter().any(|t| t.solo)
    }

    pub fn to_ron(&self) -> Result<String, ProjectError> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).map_err(ProjectError::Serialize)
    }

    pub fn from_ron(text: &str) -> Result<Self, ProjectError> {
        let mut project: Project = ron::from_str(text).map_err(ProjectError::Parse)?;
        if project.format_version > FORMAT_VERSION {
            return Err(ProjectError::UnsupportedVersion(project.format_version));
        }
        project.format_version = FORMAT_VERSION;
        project.normalize();
        Ok(project)
    }

    pub fn load(path: &Path) -> Result<Self, ProjectError> {
        Self::from_ron(&std::fs::read_to_string(path).map_err(ProjectError::Io)?)
    }

    /// Atomic save: write a temp file, then rename over the target.
    pub fn save(&self, path: &Path) -> Result<(), ProjectError> {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, self.to_ron()?).map_err(ProjectError::Io)?;
        std::fs::rename(&tmp, path).map_err(ProjectError::Io)
    }

    pub fn master_gain_linear(&self) -> f32 {
        db_to_gain(self.master_gain_db)
    }
}

/// Decibels to linear gain; -60 dB and below is silence.
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
    fn default_set_roundtrips_through_ron() {
        let mut p = Project::new_default();
        let id = p.new_midi_clip(1, 2, 8.0).unwrap();
        if let Some(Some(Clip { content: ClipContent::Midi { notes }, .. })) = p.tracks[1].slots.get_mut(2) {
            notes.push(Note { start: 0.5, length: 0.25, pitch: 36, velocity: 100 });
        }
        p.tracks[3].arrangement.push(ArrangementClip {
            start: 4.0,
            clip: Clip { content: ClipContent::Audio { sample: "factory:Kick".into(), offset_frames: 0.0, warp_bpm: Some(120.0), gain_db: -3.0 }, ..Default::default() },
        });
        let back = Project::from_ron(&p.to_ron().unwrap()).unwrap();
        assert_eq!(back, p);
        assert!(id > 0 && back.next_id > id);
    }

    #[test]
    fn version_1_files_still_load() {
        let p = Project::from_ron("(format_version: 1, tempo_bpm: 90.0)").unwrap();
        assert_eq!(p.tempo_bpm, 90.0);
        assert_eq!(p.format_version, FORMAT_VERSION);
        assert!(matches!(Project::from_ron("(format_version: 999)"), Err(ProjectError::UnsupportedVersion(999))));
    }

    #[test]
    fn scenes_and_slots_stay_in_sync() {
        let mut p = Project::new_default();
        assert!(p.tracks.iter().all(|t| t.slots.len() == 8));
        p.new_midi_clip(0, 3, 4.0).unwrap();
        p.insert_scene(2);
        assert!(p.tracks.iter().all(|t| t.slots.len() == 9));
        assert!(p.tracks[0].slots[4].is_some(), "clip moved down with insert");
        p.remove_scene(4);
        assert!(p.tracks[0].slots.iter().all(Option::is_none));
        let t = p.add_track(TrackKind::Audio);
        assert_eq!(p.tracks[t].slots.len(), 8);
    }

    #[test]
    fn ids_are_unique_after_duplicate_and_load() {
        let mut p = Project::new_default();
        p.new_midi_clip(0, 0, 4.0);
        let d = p.duplicate_track(0).unwrap();
        let mut ids: Vec<u64> = p.tracks.iter().map(|t| t.id).collect();
        ids.extend(p.tracks.iter().flat_map(|t| t.slots.iter().flatten().map(|c| c.id)));
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
        assert_eq!(p.tracks[d].name, "Drums copy");
        let mut text_loaded = Project::from_ron(&p.to_ron().unwrap()).unwrap();
        let fresh = text_loaded.alloc_id();
        assert!(!ids.contains(&fresh));
    }

    #[test]
    fn save_and_load_file() {
        let dir = std::env::temp_dir().join(format!("daw-project-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("song.dawproj");
        let p = Project::new_default();
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
