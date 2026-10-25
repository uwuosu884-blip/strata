//! Application state + non-drawing logic. The UI modules read/modify this;
//! every project change goes through `edit` (undo) and marks a recompile.

use crate::audio::AudioSystem;
use crate::midi::MidiSystem;
use daw_core::{RtSlots, SampleCache, Take};
use daw_engine::{Command, EngineHandle, RecordState, SampleBuffer, StatusSnapshot, TrackSnapshot};
use daw_project::{ArrangementClip, Clip, ClipContent, History, InstrumentDef, Note, Project, RECORDING_PREFIX, TrackKind, db_to_gain};
use std::path::PathBuf;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Session,
    Arrangement,
}

/// What the detail panel (bottom) shows.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ClipRef {
    Slot { track: usize, scene: usize },
    Arr { track: usize, index: usize },
}

/// Drag & drop payloads.
#[derive(Clone, Debug)]
pub enum DragItem {
    Sample { key: String, name: String },
    SynthPreset(String),
    DrumKit,
    SessionClip { track: usize, scene: usize },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Dialog {
    Open(String),
    SaveAs(String),
    Export { path: String, scene: Option<usize>, bits: u16 },
    ConfirmDiscard(PendingAction),
    About,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PendingAction {
    New,
    Open(String),
    Quit,
}

pub struct KeyboardMidi {
    pub enabled: bool,
    pub octave: i32,
    pub velocity: u8,
    /// Note currently sounding per key (for correct note-off after octave changes).
    pub held: Vec<(eframe::egui::Key, u8)>,
}

/// Computer-keyboard piano layout (Live's): A=C, W=C#, S=D …
pub const KEY_NOTES: [(eframe::egui::Key, u8); 18] = {
    use eframe::egui::Key::*;
    [
        (A, 0),
        (W, 1),
        (S, 2),
        (E, 3),
        (D, 4),
        (F, 5),
        (T, 6),
        (G, 7),
        (Y, 8),
        (H, 9),
        (U, 10),
        (J, 11),
        (K, 12),
        (O, 13),
        (L, 14),
        (P, 15),
        (Semicolon, 16),
        (Quote, 17),
    ]
};

pub struct DawApp {
    pub engine: EngineHandle,
    pub audio: AudioSystem,
    pub midi: MidiSystem,
    pub recorder: Option<daw_core::Recorder>,

    pub project: Project,
    pub history: History,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub cache: SampleCache,
    pub slots: RtSlots,
    pub needs_compile: bool,
    /// Commands that must reach the engine after the next song swap.
    outbox: Vec<Command>,

    pub status: StatusSnapshot,
    /// Per project track (same index), refreshed every frame.
    pub track_status: Vec<TrackSnapshot>,
    pub meter: [f32; 2],
    pub track_meters: Vec<[f32; 2]>,
    pub clipped: bool,
    pub cpu_display: f32,

    pub view: View,
    pub selected_track: usize,
    pub selected_clip: Option<ClipRef>,
    /// Selected note indices in the selected MIDI clip (piano roll).
    pub note_sel: Vec<usize>,
    /// Auditioned notes to release: (rt, pitch, when).
    pub audition: Vec<(u16, u8, Instant)>,
    pub kb: KeyboardMidi,
    /// Plain key events taken from egui's raw input (see `raw_input_hook`).
    pub key_events: Vec<eframe::egui::Event>,
    pub midi_target: Option<u16>,
    /// Session recording into (track id, scene).
    pub session_rec: Option<(u64, usize)>,
    rec_counter: u32,
    pub dialog: Option<Dialog>,
    pub message: Option<(String, Instant)>,
    pub dropped_commands: u32,
    pub last_frame: Instant,
    pub title: String,
    pub allow_close: bool,

    // View state
    pub arr_zoom: f32,
    pub arr_scroll: f32,
    pub pr_low_note: i32,
    pub grid: f64,
    pub browser_dir: PathBuf,
    pub show_browser: bool,
}

const METER_FALL_DB_PER_SEC: f32 = 24.0;

impl DawApp {
    pub fn new(mut engine: EngineHandle, audio: AudioSystem, project: Project, path: Option<PathBuf>, midi_enabled: bool) -> Self {
        let midi = MidiSystem::new(engine.take_midi_input().expect("midi producer"), midi_enabled);
        let recorder = engine.take_recorder().map(daw_core::Recorder::spawn);
        let sr = audio.info.sample_rate;
        let status = engine.snapshot();
        let browser_dir =
            std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        let mut app = Self {
            engine,
            audio,
            midi,
            recorder,
            project,
            history: History::default(),
            path,
            dirty: false,
            cache: SampleCache::new(sr),
            slots: RtSlots::default(),
            needs_compile: true,
            outbox: Vec::new(),
            status,
            track_status: Vec::new(),
            meter: [0.0; 2],
            track_meters: Vec::new(),
            clipped: false,
            cpu_display: 0.0,
            view: View::Session,
            selected_track: 0,
            selected_clip: None,
            note_sel: Vec::new(),
            audition: Vec::new(),
            kb: KeyboardMidi { enabled: true, octave: 4, velocity: 100, held: Vec::new() },
            key_events: Vec::new(),
            midi_target: None,
            session_rec: None,
            rec_counter: 0,
            dialog: None,
            message: None,
            dropped_commands: 0,
            last_frame: Instant::now(),
            title: String::new(),
            allow_close: false,
            arr_zoom: 28.0,
            arr_scroll: 0.0,
            pr_low_note: 48,
            grid: 0.25,
            browser_dir,
            show_browser: true,
        };
        app.sync_globals();
        app
    }

    // ----- engine communication -------------------------------------------------

    /// Send now, unless a recompile is pending (then after the song swap, so
    /// e.g. launching a just-created clip finds it in the song).
    pub fn send(&mut self, cmd: Command) {
        if self.needs_compile {
            self.outbox.push(cmd);
        } else if self.engine.send(cmd).is_err() {
            self.dropped_commands += 1;
        }
    }

    /// Always immediate (notes, mixer moves).
    pub fn send_now(&mut self, cmd: Command) {
        if self.engine.send(cmd).is_err() {
            self.dropped_commands += 1;
        }
    }

    pub fn rt(&self, track: usize) -> Option<u16> {
        self.project.tracks.get(track).and_then(|t| self.slots.get(t.id))
    }

    pub fn sync_globals(&mut self) {
        let p = &self.project;
        let cmds = [
            Command::SetTempo(p.tempo_bpm),
            Command::SetBeatsPerBar(p.beats_per_bar),
            Command::SetMetronome(p.metronome),
            Command::SetMasterGain(p.master_gain_linear()),
            Command::SetLaunchQuantize(p.launch_quantize),
            Command::SetLoop { enabled: p.loop_enabled, start: p.loop_start, end: p.loop_end },
        ];
        for c in cmds {
            self.send_now(c);
        }
    }

    /// Per-frame housekeeping before drawing.
    pub fn begin_frame(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.25);
        self.last_frame = now;
        self.status = self.engine.snapshot();
        self.track_status =
            self.project.tracks.iter().map(|t| self.slots.get(t.id).map(|rt| self.engine.track(rt)).unwrap_or_default()).collect();

        let fall = 10f32.powf(-METER_FALL_DB_PER_SEC * dt / 20.0);
        for ch in 0..2 {
            let p = self.status.peak[ch];
            self.meter[ch] = p.max(self.meter[ch] * fall);
            if p >= 1.0 {
                self.clipped = true;
            }
        }
        self.track_meters.resize(self.project.tracks.len(), [0.0; 2]);
        for (m, s) in self.track_meters.iter_mut().zip(&self.track_status) {
            for (d, &p) in m.iter_mut().zip(&s.peak) {
                *d = p.max(*d * fall);
            }
        }
        self.cpu_display += (self.status.cpu_load - self.cpu_display) * (dt * 4.0).min(1.0);

        if self.cache.poll() > 0 {
            self.needs_compile = true;
        }
        let takes = self.recorder.as_ref().map(|r| r.take_finished()).unwrap_or_default();
        for t in takes {
            self.apply_take(t);
        }
        self.engine.collect_garbage();
        self.update_midi_target();
        if self.selected_track >= self.project.tracks.len() {
            self.selected_track = self.project.tracks.len().saturating_sub(1);
        }
        if let Some(c) = self.selected_clip
            && self.clip(c).is_none()
        {
            self.selected_clip = None;
        }
        // Release auditioned notes.
        let now = Instant::now();
        let due: Vec<(u16, u8)> = self.audition.iter().filter(|a| now >= a.2).map(|a| (a.0, a.1)).collect();
        self.audition.retain(|a| now < a.2);
        for (rt, pitch) in due {
            self.send_now(Command::NoteOff { track: rt, pitch });
        }
    }

    /// Play a short preview note on a track.
    pub fn audition_note(&mut self, track: usize, pitch: u8, velocity: u8) {
        if let Some(rt) = self.rt(track) {
            self.send_now(Command::NoteOn { track: rt, pitch, velocity });
            self.audition.push((rt, pitch, Instant::now() + std::time::Duration::from_millis(250)));
        }
    }

    pub fn delete_selected_notes(&mut self) -> bool {
        let Some(c) = self.selected_clip else { return false };
        if self.note_sel.is_empty() {
            return false;
        }
        let mut sel = std::mem::take(&mut self.note_sel);
        sel.sort_unstable();
        sel.dedup();
        self.edit(|p| {
            if let Some(Clip { content: ClipContent::Midi { notes }, .. }) = Self::clip_mut(p, c) {
                for &i in sel.iter().rev() {
                    if i < notes.len() {
                        notes.remove(i);
                    }
                }
            }
        });
        true
    }

    /// After drawing: push a new song if anything structural changed.
    pub fn end_frame(&mut self) {
        if self.needs_compile {
            let song = daw_core::compile(&self.project, &mut self.cache, &mut self.slots);
            match self.engine.set_song(song) {
                Ok(()) => self.needs_compile = false,
                Err(_) => return, // queue full: retry next frame
            }
        }
        for c in std::mem::take(&mut self.outbox) {
            self.send_now(c);
        }
    }

    fn update_midi_target(&mut self) {
        // First armed MIDI track, else the selected track (Live's auto-arm).
        let p = &self.project;
        let idx = p
            .tracks
            .iter()
            .position(|t| t.arm && t.kind == TrackKind::Midi)
            .or_else(|| p.tracks.get(self.selected_track).filter(|t| t.kind == TrackKind::Midi).map(|_| self.selected_track));
        let target = idx.and_then(|i| self.rt(i));
        if target != self.midi_target {
            self.midi_target = target;
            self.send_now(Command::SetMidiTarget(target));
        }
    }

    // ----- editing ---------------------------------------------------------------

    /// Undoable edit.
    pub fn edit<R>(&mut self, f: impl FnOnce(&mut Project) -> R) -> R {
        self.history.checkpoint(&self.project);
        let r = f(&mut self.project);
        self.dirty = true;
        self.needs_compile = true;
        r
    }

    /// Start of a continuous gesture (drag): one undo step for the whole drag.
    pub fn begin_gesture(&mut self) {
        self.history.checkpoint(&self.project);
        self.dirty = true;
    }

    pub fn undo(&mut self) {
        if self.history.undo(&mut self.project) {
            self.after_history();
            self.toast("Undo");
        }
    }

    pub fn redo(&mut self) {
        if self.history.redo(&mut self.project) {
            self.after_history();
            self.toast("Redo");
        }
    }

    fn after_history(&mut self) {
        self.dirty = true;
        self.needs_compile = true;
        self.sync_globals();
    }

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.message = Some((msg.into(), Instant::now()));
    }

    pub fn clip(&self, c: ClipRef) -> Option<&Clip> {
        match c {
            ClipRef::Slot { track, scene } => self.project.tracks.get(track)?.slots.get(scene)?.as_ref(),
            ClipRef::Arr { track, index } => self.project.tracks.get(track)?.arrangement.get(index).map(|a| &a.clip),
        }
    }

    pub fn clip_mut(p: &mut Project, c: ClipRef) -> Option<&mut Clip> {
        match c {
            ClipRef::Slot { track, scene } => p.tracks.get_mut(track)?.slots.get_mut(scene)?.as_mut(),
            ClipRef::Arr { track, index } => p.tracks.get_mut(track)?.arrangement.get_mut(index).map(|a| &mut a.clip),
        }
    }

    pub fn add_track(&mut self, kind: TrackKind) {
        let at = self.edit(|p| p.add_track(kind));
        self.selected_track = at;
    }

    pub fn delete_track(&mut self, idx: usize) {
        if idx < self.project.tracks.len() {
            self.edit(|p| {
                p.tracks.remove(idx);
            });
            self.selected_clip = None;
        }
    }

    pub fn delete_selected_clip(&mut self) {
        let Some(c) = self.selected_clip.take() else { return };
        self.edit(|p| match c {
            ClipRef::Slot { track, scene } => {
                if let Some(s) = p.tracks.get_mut(track).and_then(|t| t.slots.get_mut(scene)) {
                    *s = None;
                }
            }
            ClipRef::Arr { track, index } => {
                if let Some(t) = p.tracks.get_mut(track)
                    && index < t.arrangement.len()
                {
                    t.arrangement.remove(index);
                }
            }
        });
    }

    pub fn duplicate_selected_clip(&mut self) {
        let Some(c) = self.selected_clip else { return };
        let Some(mut clip) = self.clip(c).cloned() else { return };
        let sel = self.edit(|p| {
            clip.id = p.alloc_id();
            match c {
                ClipRef::Slot { track, scene } => {
                    let t = &mut p.tracks[track];
                    // Next free slot below.
                    let free = (scene + 1..t.slots.len()).find(|&s| t.slots[s].is_none());
                    let s = match free {
                        Some(s) => s,
                        None => {
                            p.add_scene();
                            p.scenes.len() - 1
                        }
                    };
                    p.tracks[track].slots[s] = Some(clip);
                    Some(ClipRef::Slot { track, scene: s })
                }
                ClipRef::Arr { track, index } => {
                    let start = p.tracks[track].arrangement[index].start + clip.length;
                    let t = &mut p.tracks[track];
                    t.arrangement.push(ArrangementClip { start, clip });
                    t.arrangement.sort_by(|a, b| a.start.total_cmp(&b.start));
                    let i = t.arrangement.iter().position(|a| a.start == start)?;
                    Some(ClipRef::Arr { track, index: i })
                }
            }
        });
        self.selected_clip = sel;
    }

    pub fn create_midi_clip(&mut self, track: usize, scene: usize) {
        let bars = self.project.beats_per_bar as f64;
        if self.edit(|p| p.new_midi_clip(track, scene, bars)).is_some() {
            self.selected_clip = Some(ClipRef::Slot { track, scene });
            self.selected_track = track;
        }
    }

    /// Build an audio clip for a sample, auto-detecting loops (Live-style auto-warp).
    pub fn audio_clip_for(&mut self, key: &str, name: &str) -> Option<Clip> {
        let buf = match self.cache.load_blocking(key) {
            Ok(b) => b,
            Err(e) => {
                self.toast(format!("Could not load {name}: {e}"));
                return None;
            }
        };
        let (length, warp) = loop_guess(&buf, self.project.tempo_bpm);
        let color = daw_project::PALETTE[(name.len() * 7) % daw_project::PALETTE.len()];
        Some(Clip {
            id: 0,
            name: name.to_string(),
            color,
            length,
            looping: warp.is_some(),
            content: ClipContent::Audio { sample: key.to_string(), offset_frames: 0.0, warp_bpm: warp, gain_db: 0.0 },
        })
    }

    pub fn launch_clip(&mut self, track: usize, scene: usize) {
        if let Some(rt) = self.rt(track) {
            self.send(Command::LaunchClip { track: rt, scene: scene as u32 });
        }
    }

    pub fn launch_scene(&mut self, scene: usize) {
        self.send(Command::LaunchScene { scene: scene as u32 });
    }

    pub fn toggle_play(&mut self) {
        self.send_now(if self.status.playing { Command::Stop } else { Command::Play });
    }

    /// Live: Stop while playing pauses; Stop while stopped rewinds.
    pub fn stop(&mut self) {
        self.send_now(if self.status.playing { Command::Stop } else { Command::Rewind });
    }

    // ----- mixer (immediate commands + project value) -------------------------------------

    pub fn set_track_gain(&mut self, track: usize, db: f32) {
        self.project.tracks[track].gain_db = db;
        if let Some(rt) = self.rt(track) {
            self.send_now(Command::SetTrackGain { track: rt, gain: db_to_gain(db) });
        }
    }

    pub fn set_track_pan(&mut self, track: usize, pan: f32) {
        self.project.tracks[track].pan = pan;
        if let Some(rt) = self.rt(track) {
            self.send_now(Command::SetTrackPan { track: rt, pan });
        }
    }

    pub fn toggle_track_flag(&mut self, track: usize, flag: TrackFlag, exclusive: bool) {
        self.edit(|p| {
            let v = {
                let t = &mut p.tracks[track];
                let f = match flag {
                    TrackFlag::Mute => &mut t.mute,
                    TrackFlag::Solo => &mut t.solo,
                    TrackFlag::Arm => &mut t.arm,
                    TrackFlag::Monitor => &mut t.monitor,
                };
                *f = !*f;
                *f
            };
            // Plain click on solo/arm is exclusive (Live default); Ctrl adds.
            if exclusive && v && matches!(flag, TrackFlag::Solo | TrackFlag::Arm) {
                for (i, t) in p.tracks.iter_mut().enumerate() {
                    if i != track {
                        match flag {
                            TrackFlag::Solo => t.solo = false,
                            _ => t.arm = false,
                        }
                    }
                }
            }
        });
    }

    // ----- recording ---------------------------------------------------------------

    pub fn recording(&self) -> bool {
        matches!(self.status.record_state, RecordState::Recording | RecordState::StartPending)
    }

    /// Arrangement record (F9).
    pub fn toggle_record(&mut self) {
        if self.recording() {
            self.send_now(Command::SetRecording(false));
        } else {
            if !self.project.tracks.iter().any(|t| t.arm) {
                // Arm the selected track for convenience.
                let s = self.selected_track;
                if s < self.project.tracks.len() {
                    self.edit(|p| p.tracks[s].arm = true);
                }
            }
            self.session_rec = None;
            self.send(Command::SetRecording(true));
            if !self.status.playing {
                self.send(Command::Play);
            }
        }
    }

    /// Record a new clip into an empty session slot.
    pub fn record_into_slot(&mut self, track: usize, scene: usize) {
        if self.recording() {
            self.send_now(Command::SetRecording(false));
            return;
        }
        let id = self.project.tracks[track].id;
        if !self.project.tracks[track].arm {
            self.edit(|p| p.tracks[track].arm = true);
        }
        self.session_rec = Some((id, scene));
        if let Some(rt) = self.rt(track) {
            self.send(Command::StopTrack { track: rt });
        }
        self.send(Command::SetRecording(true));
        if !self.status.playing {
            self.send(Command::Play);
        }
    }

    fn store_recording(&mut self, take: &Take) -> String {
        self.rec_counter += 1;
        let key = format!("{RECORDING_PREFIX}take-{}-{}", std::process::id(), self.rec_counter);
        let (l, r): (Vec<f32>, Vec<f32>) = take.audio.as_chunks::<2>().0.iter().map(|f| (f[0], f[1])).unzip();
        self.cache.insert(&key, SampleBuffer::new(format!("Take {}", self.rec_counter), self.audio.info.sample_rate, l, r));
        key
    }

    /// Turn a finished take into clips.
    pub fn apply_take(&mut self, mut take: Take) {
        let len = (take.end_beat - take.start_beat).max(0.0);
        if len <= 1e-6 {
            return;
        }
        take.compensate_latency(self.audio.info.round_trip_frames());
        let has_audio_target = self.project.tracks.iter().any(|t| t.arm && t.kind == TrackKind::Audio);
        let audio_key = if has_audio_target && self.audio.info.input.is_ok() && !take.audio.is_empty() {
            Some(self.store_recording(&take))
        } else {
            None
        };
        let tempo = self.project.tempo_bpm;
        let rts: Vec<Option<u16>> = (0..self.project.tracks.len()).map(|i| self.rt(i)).collect();
        let playing: Vec<Option<u32>> = self.track_status.iter().map(|s| s.playing_scene).collect();
        let session = self.session_rec.take();
        let mut launch = None;
        let mut created = 0;
        // Sample rates of audio clips that may get trimmed (offsets are in source frames).
        let mut rates = std::collections::HashMap::new();
        for t in self.project.tracks.iter().filter(|t| t.arm) {
            for a in &t.arrangement {
                if let ClipContent::Audio { sample, .. } = &a.clip.content
                    && let Some(b) = self.cache.get(sample)
                {
                    rates.insert(sample.clone(), b.sample_rate as f64);
                }
            }
        }
        let sr = self.audio.info.sample_rate as f64;
        let fpb_for = |c: &Clip| match &c.content {
            ClipContent::Audio { warp_bpm, sample, .. } => 60.0 / warp_bpm.unwrap_or(tempo) * rates.get(sample).copied().unwrap_or(sr),
            _ => 0.0,
        };
        self.edit(|p| {
            let notes_for = |rt: Option<u16>, origin: f64| -> Vec<Note> {
                take.notes
                    .iter()
                    .filter(|n| Some(n.track) == rt)
                    .map(|n| Note {
                        start: (n.start_beat - origin).max(0.0),
                        length: (n.end_beat - n.start_beat).max(1.0 / 64.0),
                        pitch: n.pitch,
                        velocity: n.velocity,
                    })
                    .collect()
            };
            let mut arr_new: Vec<(usize, Clip)> = Vec::new();
            for (i, t) in p.tracks.iter_mut().enumerate() {
                if !t.arm {
                    continue;
                }
                if let Some((id, _)) = session
                    && t.id != id
                {
                    continue;
                }
                let content = match t.kind {
                    TrackKind::Midi => {
                        let notes = notes_for(rts[i], take.start_beat);
                        // Overdub into the playing session clip.
                        if session.is_none()
                            && let Some(scene) = playing.get(i).copied().flatten()
                            && let Some(Some(clip)) = t.slots.get_mut(scene as usize)
                            && let ClipContent::Midi { notes: existing } = &mut clip.content
                        {
                            let l = clip.length;
                            for n in take.notes.iter().filter(|n| Some(n.track) == rts[i]) {
                                let start = n.clip_pos.unwrap_or(n.start_beat - take.start_beat).rem_euclid(l);
                                existing.push(Note {
                                    start,
                                    length: (n.end_beat - n.start_beat).max(1.0 / 64.0),
                                    pitch: n.pitch,
                                    velocity: n.velocity,
                                });
                            }
                            existing.sort_by(|a, b| a.start.total_cmp(&b.start));
                            created += 1;
                            continue;
                        }
                        if notes.is_empty() {
                            continue;
                        }
                        ClipContent::Midi { notes }
                    }
                    TrackKind::Audio => match &audio_key {
                        Some(k) => ClipContent::Audio { sample: k.clone(), offset_frames: 0.0, warp_bpm: Some(tempo), gain_db: 0.0 },
                        None => continue,
                    },
                };
                created += 1;
                let id = p.next_id;
                p.next_id += 1;
                let clip = Clip { id, name: format!("Rec {id}"), color: t.color, length: len, looping: true, content };
                match session {
                    Some((_, scene)) => {
                        if scene < t.slots.len() {
                            t.slots[scene] = Some(clip);
                            launch = Some((i, scene));
                        }
                    }
                    None => arr_new.push((i, clip)),
                }
            }
            // Arrangement recording overwrites what was there (like Live).
            for (i, clip) in arr_new {
                p.clear_arrangement_range(i, take.start_beat, take.end_beat, fpb_for);
                let t = &mut p.tracks[i];
                t.arrangement.push(ArrangementClip { start: take.start_beat, clip: Clip { looping: false, ..clip } });
                t.arrangement.sort_by(|a, b| a.start.total_cmp(&b.start));
            }
        });
        if let Some((track, scene)) = launch {
            self.launch_clip(track, scene);
            self.selected_clip = Some(ClipRef::Slot { track, scene });
        }
        self.toast(if created > 0 {
            format!("Recorded {len:.1} beats")
        } else {
            "Nothing recorded (arm a track, play notes / enable input)".into()
        });
    }

    // ----- computer keyboard MIDI ---------------------------------------------------

    pub fn key_note(&mut self, key: eframe::egui::Key, pressed: bool) -> bool {
        use eframe::egui::Key;
        if pressed {
            match key {
                Key::Z => {
                    self.kb.octave = (self.kb.octave - 1).max(-1);
                    return true;
                }
                Key::X => {
                    self.kb.octave = (self.kb.octave + 1).min(8);
                    return true;
                }
                Key::C => {
                    self.kb.velocity = self.kb.velocity.saturating_sub(20).max(1);
                    return true;
                }
                Key::V => {
                    self.kb.velocity = (self.kb.velocity + 20).min(127);
                    return true;
                }
                _ => {}
            }
        }
        let Some(&(_, off)) = KEY_NOTES.iter().find(|(k, _)| *k == key) else { return false };
        let Some(rt) = self.midi_target else { return true };
        if pressed {
            if self.kb.held.iter().any(|(k, _)| *k == key) {
                return true; // key repeat
            }
            let pitch = ((self.kb.octave + 1) * 12 + off as i32).clamp(0, 127) as u8;
            self.kb.held.push((key, pitch));
            self.send_now(Command::NoteOn { track: rt, pitch, velocity: self.kb.velocity });
        } else if let Some(i) = self.kb.held.iter().position(|(k, _)| *k == key) {
            let (_, pitch) = self.kb.held.remove(i);
            self.send_now(Command::NoteOff { track: rt, pitch });
        }
        true
    }

    // ----- files ----------------------------------------------------------------------

    pub fn new_project(&mut self) {
        self.replace_project(Project::new_default(), None);
        self.toast("New project");
    }

    pub fn open_project(&mut self, path: PathBuf) {
        match daw_core::load_project(&path) {
            Ok(p) => {
                self.replace_project(p, Some(path.clone()));
                self.toast(format!("Opened {}", path.display()));
            }
            Err(e) => self.toast(format!("Open failed: {e}")),
        }
    }

    fn replace_project(&mut self, p: Project, path: Option<PathBuf>) {
        self.send_now(Command::Stop);
        self.send_now(Command::StopAllClips);
        self.send_now(Command::Rewind);
        self.project = p;
        self.path = path;
        self.history.clear();
        self.dirty = false;
        self.needs_compile = true;
        self.selected_clip = None;
        self.selected_track = 0;
        self.sync_globals();
    }

    pub fn save(&mut self) -> bool {
        match self.path.clone() {
            Some(p) => self.save_as(p),
            None => {
                self.dialog = Some(Dialog::SaveAs(self.default_path("dawproj")));
                false
            }
        }
    }

    pub fn save_as(&mut self, mut path: PathBuf) -> bool {
        if path.extension().is_none() {
            path.set_extension(daw_project::FILE_EXTENSION);
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            self.project.name = stem.to_string();
        }
        match daw_core::save_project(&mut self.project, &path, &mut self.cache) {
            Ok(()) => {
                self.toast(format!("Saved {}", path.display()));
                self.path = Some(path);
                self.dirty = false;
                self.needs_compile = true; // recording refs were re-keyed
                true
            }
            Err(e) => {
                self.toast(format!("Save failed: {e}"));
                false
            }
        }
    }

    pub fn default_path(&self, ext: &str) -> String {
        let dir = self.path.as_ref().and_then(|p| p.parent().map(PathBuf::from)).unwrap_or_else(|| self.browser_dir.clone());
        dir.join(format!("{}.{ext}", self.project.name)).to_string_lossy().into_owned()
    }

    pub fn export(&mut self, path: PathBuf, scene: Option<usize>, bits: u16) {
        let opts = daw_core::ExportOptions { sample_rate: self.audio.info.sample_rate, scene, ..Default::default() };
        let t = Instant::now();
        match daw_core::export::render_with_cache(&self.project, &mut self.cache, &opts) {
            Ok(r) => match daw_core::write_wav(&path, &r.data, r.sample_rate, bits) {
                Ok(()) => self.toast(format!(
                    "Exported {} ({:.1} s audio in {:.2} s)",
                    path.display(),
                    r.data.len() as f64 / 2.0 / r.sample_rate as f64,
                    t.elapsed().as_secs_f64()
                )),
                Err(e) => self.toast(format!("Export failed: {e}")),
            },
            Err(e) => self.toast(format!("Export failed: {e}")),
        }
    }

    /// Run an action, asking first when there are unsaved changes.
    pub fn guarded(&mut self, action: PendingAction) {
        if self.dirty {
            self.dialog = Some(Dialog::ConfirmDiscard(action));
        } else {
            self.run_pending(action);
        }
    }

    pub fn run_pending(&mut self, action: PendingAction) {
        match action {
            PendingAction::New => self.new_project(),
            PendingAction::Open(p) => self.open_project(PathBuf::from(p)),
            PendingAction::Quit => self.allow_close = true,
        }
    }

    pub fn set_instrument(&mut self, track: usize, def: InstrumentDef) {
        self.edit(|p| {
            if let Some(t) = p.tracks.get_mut(track) {
                t.instrument = def;
                t.kind = TrackKind::Midi;
            }
        });
    }
}

#[derive(Clone, Copy)]
pub enum TrackFlag {
    Mute,
    Solo,
    Arm,
    Monitor,
}

/// Guess whether a sample is a loop: returns (length in beats, warp bpm).
/// A loop is 1/2/4/8/16 bars long at a tempo between 80 and 160 BPM.
pub fn loop_guess(buf: &SampleBuffer, tempo: f64) -> (f64, Option<f64>) {
    let secs = buf.duration_secs();
    if secs >= 1.0 {
        for bars in [1.0, 2.0, 4.0, 8.0, 16.0] {
            let bpm = bars * 4.0 * 60.0 / secs;
            if (80.0..=160.0).contains(&bpm) {
                return (bars * 4.0, Some(bpm));
            }
        }
    }
    // One-shot: plays at its own speed.
    ((((secs * tempo / 60.0) * 4.0).ceil() / 4.0).max(0.25), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_detection() {
        let sr = 48_000;
        let mk = |secs: f64| SampleBuffer::mono("x", sr, vec![0.0; (secs * sr as f64) as usize]);
        // 2 s = 1 bar at 120.
        assert_eq!(loop_guess(&mk(2.0), 120.0), (4.0, Some(120.0)));
        // 8 s: 4 bars at 120 (1 bar would be 30 BPM, 2 bars 60).
        assert_eq!(loop_guess(&mk(8.0), 120.0), (16.0, Some(120.0)));
        // Short one-shot at 120 BPM: 0.3 s = 0.6 beats → 0.75.
        assert_eq!(loop_guess(&mk(0.3), 120.0), (0.75, None));
    }
}
