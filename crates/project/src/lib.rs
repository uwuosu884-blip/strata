//! # daw-project
//!
//! The document the user edits and saves (RON). Pure data + editing helpers
//! + undo history. Knows nothing about audio threads.

mod arrange;
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
    /// Audio group bus: children (tracks whose `group` is this id) are summed into it.
    Group,
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
    /// Id of the parent group track (children always follow their group contiguously).
    pub group: Option<u64>,
    /// Group tracks only: children hidden in the UI.
    pub folded: bool,
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
            group: None,
            folded: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum InstrumentDef {
    #[default]
    None,
    Synth(SynthParams),
    Sampler {
        sample: Option<String>,
        params: SamplerParams,
    },
    Drums {
        pads: Vec<PadDef>,
    },
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
            TrackKind::Group => format!("{} Group", n + 1),
        };
        let instrument = match kind {
            TrackKind::Audio | TrackKind::Group => InstrumentDef::None,
            TrackKind::Midi => InstrumentDef::Synth(SynthParams::default()),
        };
        self.tracks.push(Track { id, name, kind, color: PALETTE[n % PALETTE.len()], instrument, ..Default::default() });
        self.normalize();
        n
    }

    /// Duplicate a track (groups are duplicated with all their children).
    pub fn duplicate_track(&mut self, idx: usize) -> Option<usize> {
        self.tracks.get(idx)?;
        let end = self.subtree_end(idx);
        let mut copy: Vec<Track> = self.tracks[idx..end].to_vec();
        let mut remap = std::collections::HashMap::new();
        for t in &mut copy {
            let id = self.alloc_id();
            remap.insert(t.id, id);
            t.id = id;
            t.solo = false;
            t.arm = false;
            for c in t.slots.iter_mut().flatten() {
                c.id = self.alloc_id();
            }
            for a in &mut t.arrangement {
                a.clip.id = self.alloc_id();
            }
        }
        for t in copy.iter_mut().skip(1) {
            t.group = t.group.map(|g| remap.get(&g).copied().unwrap_or(g));
        }
        copy[0].name = format!("{} copy", copy[0].name);
        self.tracks.splice(end..end, copy);
        Some(end)
    }

    /// Index of a track's parent group.
    pub fn parent_index(&self, i: usize) -> Option<usize> {
        self.tracks.get(i)?.group.and_then(|g| self.track_index(g))
    }

    /// Nesting depth (0 = top level).
    pub fn depth(&self, i: usize) -> usize {
        let mut d = 0;
        let mut cur = i;
        while let Some(p) = self.parent_index(cur) {
            d += 1;
            cur = p;
            if d > self.tracks.len() {
                break; // corrupt (cycle); normalize() prevents this
            }
        }
        d
    }

    /// True if track `i` is (transitively) inside the group at index `anc`.
    pub fn is_descendant(&self, i: usize, anc: usize) -> bool {
        let mut cur = i;
        for _ in 0..self.tracks.len() {
            match self.parent_index(cur) {
                Some(p) if p == anc => return true,
                Some(p) => cur = p,
                None => return false,
            }
        }
        false
    }

    /// One past the last descendant of `i` (children are contiguous after their group).
    pub fn subtree_end(&self, i: usize) -> usize {
        let mut j = i + 1;
        while j < self.tracks.len() && self.is_descendant(j, i) {
            j += 1;
        }
        j
    }

    /// Hidden by a folded ancestor group?
    pub fn is_visible(&self, i: usize) -> bool {
        let mut cur = i;
        for _ in 0..self.tracks.len() {
            match self.parent_index(cur) {
                Some(p) if self.tracks[p].folded => return false,
                Some(p) => cur = p,
                None => return true,
            }
        }
        true
    }

    /// Indices of tracks shown in the UI (respecting folded groups).
    pub fn visible_tracks(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.tracks.len());
        let mut i = 0;
        while i < self.tracks.len() {
            out.push(i);
            i = if self.tracks[i].folded && self.tracks[i].kind == TrackKind::Group { self.subtree_end(i) } else { i + 1 };
        }
        out
    }

    /// Wrap the given tracks (with their children) in a new group.
    /// The group is inserted at the first selected track and nested in their
    /// common parent if they share one. Returns the group's index.
    pub fn group_tracks(&mut self, sel: &[usize]) -> Option<usize> {
        let mut roots: Vec<usize> = sel.iter().copied().filter(|&i| i < self.tracks.len()).collect();
        roots.sort_unstable();
        roots.dedup();
        // Drop items already covered by a selected ancestor.
        let all = roots.clone();
        roots.retain(|&i| !all.iter().any(|&a| a != i && self.is_descendant(i, a)));
        let first = *roots.first()?;
        let parent = self.tracks[first].group;
        let parent = if roots.iter().all(|&i| self.tracks[i].group == parent) { parent } else { None };
        let ranges: Vec<(usize, usize)> = roots.iter().map(|&i| (i, self.subtree_end(i))).collect();
        let mut moved: Vec<Vec<Track>> = Vec::with_capacity(ranges.len());
        for &(a, b) in ranges.iter().rev() {
            moved.push(self.tracks.drain(a..b).collect());
        }
        moved.reverse();
        let gid = self.alloc_id();
        let color = moved[0][0].color;
        let n_groups = self.tracks.iter().filter(|t| t.kind == TrackKind::Group).count();
        let mut block = vec![Track {
            id: gid,
            name: format!("Group {}", n_groups + 1),
            kind: TrackKind::Group,
            color,
            group: parent,
            ..Default::default()
        }];
        for mut m in moved {
            m[0].group = Some(gid);
            block.extend(m);
        }
        let at = first.min(self.tracks.len());
        self.tracks.splice(at..at, block);
        self.normalize();
        self.track_index(gid)
    }

    /// Dissolve a group: its children move up one level.
    pub fn ungroup(&mut self, g: usize) -> bool {
        let Some(t) = self.tracks.get(g) else { return false };
        if t.kind != TrackKind::Group {
            return false;
        }
        let (gid, parent) = (t.id, t.group);
        for t in &mut self.tracks {
            if t.group == Some(gid) {
                t.group = parent;
            }
        }
        self.tracks.remove(g);
        self.normalize();
        true
    }

    /// Remove a track together with its children (if a group). Returns how many were removed.
    pub fn remove_track(&mut self, i: usize) -> usize {
        if i >= self.tracks.len() {
            return 0;
        }
        let end = self.subtree_end(i);
        self.tracks.drain(i..end);
        end - i
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
                std::iter::once(t.id).chain(t.slots.iter().flatten().map(|c| c.id)).chain(t.arrangement.iter().map(|a| a.clip.id))
            })
            .max()
            .unwrap_or(0);
        self.next_id = self.next_id.max(max_id + 1);
        // Ids must be unique and non-zero (old/hand-written files may repeat them).
        let mut seen = std::collections::HashSet::new();
        let mut next = self.next_id;
        let mut fix = |id: &mut u64| {
            if *id == 0 || !seen.insert(*id) {
                *id = next;
                seen.insert(next);
                next += 1;
            }
        };
        for t in &mut self.tracks {
            fix(&mut t.id);
            for c in t.slots.iter_mut().flatten() {
                fix(&mut c.id);
            }
            for a in &mut t.arrangement {
                fix(&mut a.clip.id);
            }
        }
        self.next_id = next;
        self.normalize_groups();
    }

    /// Groups carry no clips/instrument; parents must be existing groups without
    /// cycles; children are reordered to directly follow their group (pre-order).
    fn normalize_groups(&mut self) {
        let groups: std::collections::HashMap<u64, Option<u64>> =
            self.tracks.iter().filter(|t| t.kind == TrackKind::Group).map(|t| (t.id, t.group)).collect();
        for t in &mut self.tracks {
            if t.kind == TrackKind::Group {
                t.instrument = InstrumentDef::None;
                t.arrangement.clear();
                t.slots.iter_mut().for_each(|s| *s = None);
                t.arm = false;
                t.monitor = false;
            } else {
                t.folded = false;
            }
            // Valid parent: an existing group, and following the chain never returns to us.
            if let Some(g) = t.group {
                let mut ok = groups.contains_key(&g) && g != t.id;
                let mut cur = g;
                for _ in 0..groups.len() {
                    match groups.get(&cur).copied().flatten() {
                        Some(n) if n == t.id => {
                            ok = false;
                            break;
                        }
                        Some(n) => cur = n,
                        None => break,
                    }
                }
                if !ok {
                    t.group = None;
                }
            }
        }
        // Pre-order: roots in original order, each followed by its children (original order).
        let n = self.tracks.len();
        let ids: Vec<u64> = self.tracks.iter().map(|t| t.id).collect();
        let parent: Vec<Option<usize>> = self.tracks.iter().map(|t| t.group.and_then(|g| ids.iter().position(|&i| i == g))).collect();
        let mut order = Vec::with_capacity(n);
        fn visit(i: usize, parent: &[Option<usize>], order: &mut Vec<usize>) {
            order.push(i);
            for (c, p) in parent.iter().enumerate() {
                if *p == Some(i) {
                    visit(c, parent, order);
                }
            }
        }
        for i in 0..n {
            if parent[i].is_none() {
                visit(i, &parent, &mut order);
            }
        }
        if order.len() == n && order.iter().enumerate().any(|(a, &b)| a != b) {
            let mut old: Vec<Option<Track>> = self.tracks.drain(..).map(Some).collect();
            self.tracks = order.into_iter().map(|i| old[i].take().expect("permutation")).collect();
        }
    }

    pub fn track_index(&self, id: u64) -> Option<usize> {
        self.tracks.iter().position(|t| t.id == id)
    }

    /// Create an empty MIDI clip in a session slot. Returns the clip id.
    pub fn new_midi_clip(&mut self, track: usize, scene: usize, length: f64) -> Option<u64> {
        let id = self.alloc_id();
        let t = self.tracks.get_mut(track)?;
        let slot = t.slots.get_mut(scene)?;
        *slot = Some(Clip {
            id,
            name: format!("Clip {id}"),
            color: t.color,
            length,
            looping: true,
            content: ClipContent::Midi { notes: Vec::new() },
        });
        Some(id)
    }

    /// Make room for new material in `[start, end)` on a track's arrangement:
    /// covered clips are removed, overlapping ones trimmed or split
    /// (MIDI notes / audio offsets are shifted so content stays in place).
    pub fn clear_arrangement_range(&mut self, track: usize, start: f64, end: f64, frames_per_beat: impl Fn(&Clip) -> f64) {
        if end <= start {
            return;
        }
        let mut next_id = self.next_id;
        let Some(t) = self.tracks.get_mut(track) else { return };
        let mut out = Vec::with_capacity(t.arrangement.len() + 1);
        for a in t.arrangement.drain(..) {
            let (s, e) = (a.start, a.start + a.clip.length);
            if e <= start || s >= end {
                out.push(a);
                continue;
            }
            if s < start {
                // Head survives.
                let mut head = a.clone();
                head.clip.length = start - s;
                out.push(head);
            }
            if e > end {
                // Tail survives: shift content left by (end - s).
                let cut = end - s;
                let mut tail = a.clone();
                tail.start = end;
                tail.clip.length = e - end;
                if s < start {
                    tail.clip.id = next_id;
                    next_id += 1;
                }
                let fpb = frames_per_beat(&tail.clip);
                match &mut tail.clip.content {
                    ClipContent::Midi { notes } => {
                        notes.retain(|n| n.start >= cut);
                        notes.iter_mut().for_each(|n| n.start -= cut);
                    }
                    ClipContent::Audio { offset_frames, .. } => *offset_frames += cut * fpb,
                }
                out.push(tail);
            }
        }
        out.sort_by(|a, b| a.start.total_cmp(&b.start));
        t.arrangement = out;
        self.next_id = next_id;
    }

    /// Split the arrangement clip with id `clip_id` at absolute beat `at`.
    /// Returns the id of the new right-hand clip.
    pub fn split_arrangement_clip(&mut self, track: usize, clip_id: u64, at: f64, frames_per_beat: impl Fn(&Clip) -> f64) -> Option<u64> {
        const MIN: f64 = 1e-6;
        let new_id = self.next_id;
        let t = self.tracks.get_mut(track)?;
        let i = t.arrangement.iter().position(|a| a.clip.id == clip_id)?;
        let a = &mut t.arrangement[i];
        let cut = at - a.start;
        if cut <= MIN || cut >= a.clip.length - MIN {
            return None;
        }
        let mut tail = a.clone();
        a.clip.length = cut;
        if let ClipContent::Midi { notes } = &mut a.clip.content {
            notes.retain(|n| n.start < cut);
        }
        tail.start = at;
        tail.clip.length -= cut;
        tail.clip.id = new_id;
        let fpb = frames_per_beat(&tail.clip);
        match &mut tail.clip.content {
            ClipContent::Midi { notes } => {
                notes.retain(|n| n.start >= cut);
                notes.iter_mut().for_each(|n| n.start -= cut);
            }
            ClipContent::Audio { offset_frames, .. } => *offset_frames += cut * fpb,
        }
        t.arrangement.insert(i + 1, tail);
        self.next_id += 1;
        Some(new_id)
    }

    /// Locate an arrangement clip by id: (track index, clip index).
    pub fn find_arrangement_clip(&self, clip_id: u64) -> Option<(usize, usize)> {
        self.tracks.iter().enumerate().find_map(|(ti, t)| t.arrangement.iter().position(|a| a.clip.id == clip_id).map(|ci| (ti, ci)))
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
            clip: Clip {
                content: ClipContent::Audio { sample: "factory:Kick".into(), offset_frames: 0.0, warp_bpm: Some(120.0), gain_db: -3.0 },
                ..Default::default()
            },
        });
        p.normalize();
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
    fn normalize_repairs_zero_and_duplicate_ids() {
        let mut p = Project::new_default();
        for t in 0..2 {
            p.tracks[t].slots[0] = Some(Clip::default());
            p.tracks[t].arrangement.push(ArrangementClip::default());
        }
        p.tracks[1].id = p.tracks[0].id;
        p.normalize();
        let mut ids: Vec<u64> = p.tracks.iter().map(|t| t.id).collect();
        ids.extend(p.tracks.iter().flat_map(|t| t.slots.iter().flatten().map(|c| c.id).chain(t.arrangement.iter().map(|a| a.clip.id))));
        assert!(ids.iter().all(|&i| i > 0));
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
        assert!(p.next_id > *ids.last().unwrap());
    }

    #[test]
    fn clearing_a_range_trims_splits_and_removes() {
        let mut p = Project::new_default();
        let midi = |len: f64, starts: &[f64]| Clip {
            length: len,
            content: ClipContent::Midi {
                notes: starts.iter().map(|&s| Note { start: s, length: 0.1, pitch: 60, velocity: 100 }).collect(),
            },
            ..Default::default()
        };
        p.tracks[1].arrangement = vec![
            ArrangementClip { start: 0.0, clip: midi(8.0, &[0.0, 3.0, 6.0]) }, // split around [2,5)
            ArrangementClip { start: 10.0, clip: midi(2.0, &[0.0]) },          // untouched
        ];
        p.normalize();
        p.clear_arrangement_range(1, 2.0, 5.0, |_| 0.0);
        let a = &p.tracks[1].arrangement;
        assert_eq!(a.len(), 3);
        assert_eq!((a[0].start, a[0].clip.length), (0.0, 2.0));
        assert_eq!((a[1].start, a[1].clip.length), (5.0, 3.0));
        match &a[1].clip.content {
            ClipContent::Midi { notes } => assert_eq!(notes.iter().map(|n| n.start).collect::<Vec<_>>(), vec![1.0]),
            _ => unreachable!(),
        }
        assert_ne!(a[0].clip.id, a[1].clip.id);
        assert_eq!(a[2].start, 10.0);
        // Fully covered → removed; audio tail gets an offset.
        p.tracks[3].arrangement = vec![ArrangementClip {
            start: 4.0,
            clip: Clip {
                length: 4.0,
                content: ClipContent::Audio { sample: "x".into(), offset_frames: 0.0, warp_bpm: None, gain_db: 0.0 },
                ..Default::default()
            },
        }];
        p.clear_arrangement_range(3, 0.0, 6.0, |_| 24_000.0);
        let a = &p.tracks[3].arrangement;
        assert_eq!((a.len(), a[0].start, a[0].clip.length), (1, 6.0, 2.0));
        assert!(matches!(a[0].clip.content, ClipContent::Audio { offset_frames, .. } if offset_frames == 48_000.0));
        p.clear_arrangement_range(1, 0.0, 100.0, |_| 0.0);
        assert!(p.tracks[1].arrangement.is_empty());
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

    fn blank(n: usize) -> Project {
        let mut p = Project::new_default();
        p.tracks.clear();
        for _ in 0..n {
            p.add_track(TrackKind::Midi);
        }
        p
    }

    #[test]
    fn group_and_ungroup_keep_preorder() {
        let mut p = blank(4); // 0 1 2 3
        let ids: Vec<u64> = p.tracks.iter().map(|t| t.id).collect();
        let g = p.group_tracks(&[1, 3]).unwrap();
        assert_eq!(g, 1);
        assert_eq!(p.tracks[1].kind, TrackKind::Group);
        let order: Vec<u64> = p.tracks.iter().map(|t| t.id).collect();
        assert_eq!(order, vec![ids[0], p.tracks[1].id, ids[1], ids[3], ids[2]]);
        assert_eq!(p.subtree_end(1), 4);
        assert_eq!(p.depth(3), 1);
        // Nested group inside the first one.
        let g2 = p.group_tracks(&[2, 3]).unwrap();
        assert_eq!(p.tracks[g2].group, Some(p.tracks[1].id));
        assert_eq!(p.depth(g2 + 1), 2);
        p.tracks[1].folded = true;
        assert_eq!(p.visible_tracks(), vec![0, 1, p.tracks.len() - 1]);
        // Ungroup outer: inner group moves to top level.
        assert!(p.ungroup(1));
        assert_eq!(p.tracks[1].kind, TrackKind::Group);
        assert_eq!(p.tracks[1].group, None);
        assert_eq!(p.remove_track(1), 3);
        assert_eq!(p.tracks.len(), 2);
    }

    #[test]
    fn normalize_fixes_bad_parents_and_order() {
        let mut p = blank(3);
        let g = p.add_track(TrackKind::Group);
        let gid = p.tracks[g].id;
        p.tracks[0].group = Some(gid); // child before its group -> reordered
        p.tracks[1].group = Some(p.tracks[2].id); // parent is not a group -> cleared
        p.tracks[g].group = Some(gid); // self parent -> cleared
        p.normalize();
        let gi = p.track_index(gid).unwrap();
        assert_eq!(p.tracks[gi].group, None);
        assert_eq!(p.tracks[gi + 1].group, Some(gid));
        assert!(p.tracks.iter().filter(|t| t.id != p.tracks[gi + 1].id).all(|t| t.group.is_none()));
        // Duplicate a group duplicates its child.
        let d = p.duplicate_track(gi).unwrap();
        assert_eq!(p.tracks[d].kind, TrackKind::Group);
        assert_eq!(p.tracks[d + 1].group, Some(p.tracks[d].id));
    }

    #[test]
    fn split_clip_shifts_content() {
        let mut p = blank(1);
        let id = p.alloc_id();
        let notes =
            vec![Note { pitch: 60, start: 0.5, length: 0.5, velocity: 100 }, Note { pitch: 62, start: 2.5, length: 0.5, velocity: 100 }];
        p.tracks[0].arrangement.push(ArrangementClip {
            start: 4.0,
            clip: Clip { id, length: 4.0, looping: false, content: ClipContent::Midi { notes }, ..Default::default() },
        });
        assert!(p.split_arrangement_clip(0, id, 4.0, |_| 1.0).is_none());
        let r = p.split_arrangement_clip(0, id, 6.0, |_| 1.0).unwrap();
        let arr = &p.tracks[0].arrangement;
        assert_eq!(arr.len(), 2);
        assert_eq!((arr[0].start, arr[0].clip.length), (4.0, 2.0));
        assert_eq!((arr[1].start, arr[1].clip.length, arr[1].clip.id), (6.0, 2.0, r));
        let ClipContent::Midi { notes } = &arr[1].clip.content else { panic!() };
        assert_eq!(notes.len(), 1);
        assert!((notes[0].start - 0.5).abs() < 1e-9);
        assert_eq!(p.find_arrangement_clip(r), Some((0, 1)));
    }
}
