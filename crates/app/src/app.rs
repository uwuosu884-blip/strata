//! Application state + non-drawing logic. The UI modules read/modify this;
//! every project change goes through `edit` (undo) and marks a recompile.

use crate::audio::{AudioOptions, AudioSystem};
use crate::midi::MidiSystem;
use crate::settings::Settings;
use daw_core::{IoMap, RtSlots, SampleCache, Take};
use daw_engine::{Command, EngineHandle, MAX_IO_CHANNELS, RecordState, SampleBuffer, StatusSnapshot, TrackSnapshot};
use daw_project::{
    ArrangementClip, Clip, ClipContent, History, InstrumentDef, Monitor, Note, Project, RECORDING_PREFIX, TrackKind, db_to_gain,
};
use std::path::PathBuf;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Session,
    Arrangement,
}

/// Detail panel content (Shift+Tab toggles).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DetailMode {
    Clip,
    Device,
}

/// Inline track rename in progress.
#[derive(Clone, Debug)]
pub struct Rename {
    /// Track id, or clip id when `clip`.
    pub id: u64,
    pub text: String,
    /// Focus has not been requested yet.
    pub fresh: bool,
    pub clip: bool,
    /// Edit in the detail panel's clip name bar (else in place: header / clip title).
    pub in_detail: bool,
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
    Export {
        path: String,
        scene: Option<usize>,
        bits: u16,
    },
    ConfirmDiscard(PendingAction),
    About,
    /// Preferences: Audio / MIDI tab.
    Settings(SettingsTab),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsTab {
    Display,
    Theme,
    Audio,
    Link,
    /// Tempo & MIDI.
    Midi,
    Files,
    Library,
    Plugins,
    Record,
    Licenses,
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
    /// Multi-selection of tracks (Shift+click headers); empty = just `selected_track`.
    pub track_sel: Vec<usize>,
    /// Empty-area click hid the track selection; stays hidden while
    /// `selected_track` still equals this value.
    pub hide_sel: Option<usize>,
    /// Selected arrangement clips (ids are stable across sorting).
    pub arr_sel: Vec<u64>,
    pub time_sel: Option<crate::arr_ops::TimeSel>,
    pub selected_clip: Option<ClipRef>,
    /// Selected note indices in the selected MIDI clip (piano roll).
    pub note_sel: Vec<usize>,
    /// Auditioned notes to release: (rt, pitch, when).
    pub audition: Vec<(u16, u8, Instant)>,
    pub kb: KeyboardMidi,
    /// Plain key events taken from egui's raw input (see `raw_input_hook`).
    pub key_events: Vec<eframe::egui::Event>,
    /// Runtime track auto-armed for MIDI (selected MIDI track while none is armed).
    pub auto_arm: u16,
    /// App preferences, base CLI audio options and MIDI on/off (for restarts).
    pub settings: Settings,
    pub audio_opts: AudioOptions,
    pub midi_enabled: bool,
    /// MIDI port names the engine knows (index = engine port).
    pub io_map: IoMap,
    /// Input channel meters (decaying), for the Ext. In chooser.
    pub input_meters: [f32; MAX_IO_CHANNELS],
    pub input_clip: [bool; MAX_IO_CHANNELS],
    /// Mixer In/Out section visible (Ctrl+Alt+I).
    pub show_io: bool,
    /// Top-bar Follow: the arrangement scrolls with the playhead.
    pub follow: bool,
    /// Follow suspended by a manual scroll/zoom; resumes on the next Play.
    pub follow_paused: bool,
    /// Transport state seen last frame (detects Play to resume Follow).
    pub was_playing: bool,
    /// Last click went to the track headers (Ctrl+D duplicates the tracks).
    pub head_focus: bool,
    /// Monitor radio under the pointer (Delete returns it to the default).
    pub hover_monitor: Option<usize>,
    /// Device names for the settings dialog (listed when it opens).
    pub device_lists: Option<(Vec<String>, Vec<String>, Vec<String>)>,
    /// Plain keys pressed while a dialog is open (dialog keyboard navigation).
    pub dialog_keys: Vec<eframe::egui::Key>,
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
    /// Arrangement track height (Alt+wheel) and vertical scroll.
    pub arr_row_h: f32,
    pub arr_vscroll: f32,
    pub pr_low_note: i32,
    /// Piano roll: pixels per beat (0 = fit the clip), first visible beat, key row height.
    pub pr_zoom: f32,
    pub pr_scroll: f64,
    /// Zoom gesture anchors: (beat | pitch, screen offset, last input time).
    pub pr_zoom_anchor: Option<(f64, f32, f64)>,
    pub pr_row_anchor: Option<(f64, f32, f64)>,
    pub pr_row_h: f32,
    /// Current arrangement / piano roll grid step (beats), for labels and Quantize.
    pub grid: f64,
    pub pr_grid_now: f64,
    pub arr_grid: crate::arr_ops::GridMode,
    /// Mode restored by Ctrl+4 (Snap to Grid back on).
    pub arr_grid_prev: crate::arr_ops::GridMode,
    /// Ctrl+3: triplet grid (step x 2/3).
    pub arr_triplet: bool,
    /// Internal Cut/Copy/Paste buffer.
    pub clipboard: crate::edit_ops::Clipboard,
    /// View before "Zoom to Time Selection" (zoom, scroll).
    pub zoom_back: Option<(f32, f32)>,
    /// Lanes width last frame (zoom to selection).
    pub arr_lanes_w: f32,
    /// Selected scene (Session highlight row; Live always has one).
    pub sel_scene: usize,
    /// Selected *empty* session slot (track, scene); clip slots use `selected_clip`.
    pub empty_slot: Option<(usize, usize)>,
    pub pr_grid: crate::arr_ops::GridMode,
    pub detail_mode: DetailMode,
    /// Last mouse press was in the detail panel (keys like Del/Ctrl+D/0 act on notes).
    pub focus_detail: bool,
    /// Where Space starts playback (set by clicking the timeline).
    pub play_start: f64,
    pub renaming: Option<Rename>,
    /// Track touched last (any click on its header, lane or clips): Alt+wheel
    /// resizes it; None (Esc / empty area) = all tracks.
    pub touched_track: Option<usize>,
    /// Shift+click range anchors (header / clip).
    pub track_anchor: Option<usize>,
    pub clip_anchor: Option<u64>,
    /// Piano roll: time range of the last rubber band (Ctrl+D duplicates the
    /// whole range), last adjusted note length (0 = grid) / velocity for new
    /// notes, velocity lane height, preview notes while editing.
    pub pr_time_sel: Option<(f64, f64)>,
    pub pr_note_len: f64,
    pub pr_note_vel: u8,
    pub pr_vel_h: f32,
    pub pr_audition: bool,
    pub browser_dir: PathBuf,
    pub show_browser: bool,
    /// Master track selected (its device view shows in the detail panel).
    pub master_selected: bool,
    /// Per track: silenced only by another track's solo (dimmed activator).
    pub solo_silenced: Vec<bool>,
    /// Browser preview: on/off and the key currently playing.
    pub preview_on: bool,
    pub preview: Option<String>,
    /// Clip View state (tabs, MIDI Tools, envelope / MPE lanes).
    pub cv: crate::ui::clip::ClipView,
}

const METER_FALL_DB_PER_SEC: f32 = 24.0;

impl DawApp {
    pub fn new(
        mut engine: EngineHandle,
        audio: AudioSystem,
        project: Project,
        path: Option<PathBuf>,
        midi_enabled: bool,
        settings: Settings,
        audio_opts: AudioOptions,
    ) -> Self {
        let midi = MidiSystem::new(engine.take_midi_input().expect("midi producer"), engine.take_midi_output(), midi_enabled, &settings);
        let io_map = midi.io_map();
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
            track_sel: Vec::new(),
            hide_sel: None,
            arr_sel: Vec::new(),
            time_sel: None,
            selected_clip: None,
            note_sel: Vec::new(),
            audition: Vec::new(),
            kb: KeyboardMidi { enabled: true, octave: 4, velocity: 100, held: Vec::new() },
            key_events: Vec::new(),
            auto_arm: u16::MAX - 1, // forces the first SetAutoArm
            settings,
            audio_opts,
            midi_enabled,
            io_map,
            input_meters: [0.0; MAX_IO_CHANNELS],
            input_clip: [false; MAX_IO_CHANNELS],
            show_io: true,
            hover_monitor: None,
            device_lists: None,
            dialog_keys: Vec::new(),
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
            follow: true,
            follow_paused: false,
            was_playing: false,
            head_focus: false,
            arr_row_h: 88.0,
            arr_vscroll: 0.0,
            pr_low_note: 48,
            pr_zoom: 0.0,
            pr_scroll: 0.0,
            pr_zoom_anchor: None,
            pr_row_anchor: None,
            pr_row_h: 11.0,
            grid: 0.25,
            pr_grid_now: 0.25,
            arr_grid: crate::arr_ops::GridMode::Adaptive(0),
            arr_grid_prev: crate::arr_ops::GridMode::Adaptive(0),
            arr_triplet: false,
            clipboard: Default::default(),
            zoom_back: None,
            arr_lanes_w: 800.0,
            sel_scene: 0,
            empty_slot: None,
            pr_grid: crate::arr_ops::GridMode::Adaptive(0),
            detail_mode: DetailMode::Device,
            cv: Default::default(),
            focus_detail: false,
            play_start: 0.0,
            renaming: None,
            touched_track: None,
            track_anchor: None,
            clip_anchor: None,
            pr_time_sel: None,
            pr_note_len: 0.0,
            pr_note_vel: 100,
            pr_vel_h: 48.0,
            pr_audition: true,
            browser_dir,
            show_browser: true,
            master_selected: false,
            solo_silenced: Vec::new(),
            preview_on: true,
            preview: None,
        };
        app.adopt_project_rate();
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
            Command::SetTempoFollow(p.tempo_follow),
            Command::SetMasterGain(p.master_gain_linear()),
            Command::SetMasterPan(p.master_pan),
            Command::SetCueGain(daw_project::db_to_gain(p.cue_gain_db)),
            Command::SetMonitorGain(daw_project::db_to_gain(self.settings.output_gain_db)),
            Command::SetLaunchQuantize(p.launch_quantize),
            Command::SetLoop { enabled: p.loop_enabled, start: p.loop_start, end: p.loop_end },
        ];
        for c in cmds {
            self.send_now(c);
        }
    }

    /// Browser preview on the Cue bus (starts once the sample has loaded).
    pub fn preview_sample(&mut self, key: &str) {
        self.preview = Some(key.to_string());
        self.poll_preview();
    }

    pub fn stop_preview(&mut self) {
        self.preview = None;
        self.send_now(Command::StopPreview);
    }

    fn poll_preview(&mut self) {
        let Some(key) = self.preview.clone() else { return };
        if let Some(buf) = self.cache.get(&key) {
            self.preview = None;
            self.engine.preview(buf);
        } else if self.cache.error(&key).is_some() {
            self.preview = None;
        }
    }

    /// Select the master track: its device view opens in the detail panel.
    pub fn select_master(&mut self) {
        self.clear_structure_selection();
        self.master_selected = true;
        self.detail_mode = DetailMode::Device;
    }

    /// Per-frame housekeeping before drawing.
    pub fn begin_frame(&mut self) {
        self.solo_silenced = self.project.solo_silenced();
        self.poll_preview();
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
        for (i, p) in self.engine.input_peaks().iter().enumerate() {
            self.input_meters[i] = p.max(self.input_meters[i] * fall);
            self.input_clip[i] |= *p >= 1.0;
        }

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
        self.sanitize_selection();
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
            let song = daw_core::compile_io(&self.project, &mut self.cache, &mut self.slots, &self.io_map);
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
        // Live's auto-arm: the selected MIDI track monitors MIDI while no MIDI track is armed.
        let p = &self.project;
        let any_armed = p.tracks.iter().any(|t| t.arm && t.kind == TrackKind::Midi);
        let sel = p.tracks.get(self.selected_track).filter(|t| t.kind == TrackKind::Midi && t.has_input());
        let rt = if any_armed { None } else { sel.and_then(|_| self.rt(self.selected_track)) }.unwrap_or(u16::MAX);
        if rt != self.auto_arm {
            self.auto_arm = rt;
            self.send_now(Command::SetAutoArm(rt));
        }
    }

    /// Restart the audio device with new options: rebuilds the engine, MIDI
    /// and recorder, then recompiles the song.
    pub fn restart_audio(&mut self, opts: AudioOptions) {
        // Release the device first (a null engine bridges the gap).
        self.recorder = None;
        let (tmp_audio, tmp_engine) = crate::audio::start(&AudioOptions { force_null: true, no_input: true, ..Default::default() });
        self.audio = tmp_audio;
        self.engine = tmp_engine;
        let (audio, mut engine) = crate::audio::start(&opts);
        self.midi =
            MidiSystem::new(engine.take_midi_input().expect("midi producer"), engine.take_midi_output(), self.midi_enabled, &self.settings);
        self.io_map = self.midi.io_map();
        self.recorder = engine.take_recorder().map(daw_core::Recorder::spawn);
        self.audio = audio;
        self.engine = engine;
        self.audio_opts = opts;
        self.adopt_engine_rate();
        self.slots = RtSlots::default();
        self.needs_compile = true;
        self.auto_arm = u16::MAX - 1;
        self.input_meters = [0.0; MAX_IO_CHANNELS];
        self.input_clip = [false; MAX_IO_CHANNELS];
        self.sync_globals();
        let i = &self.audio.info;
        let msg = match &i.null_reason {
            Some(_) => "Audio: no output device, running silently".to_string(),
            None => format!(
                "Audio: {} · {} Hz · {}",
                i.device,
                i.sample_rate,
                i.buffer_frames.map_or("default buffer".into(), |b| format!("{b} frames"))
            ),
        };
        self.toast(msg);
    }

    /// Reopen MIDI ports after a Track switch changed.
    pub fn rescan_midi(&mut self) {
        let s = self.settings.clone();
        self.midi.rescan(&s);
        let io = self.midi.io_map();
        if io != self.io_map {
            self.io_map = io;
            self.needs_compile = true;
        }
    }

    /// Audio options after a change in Settings (the dialog's choices win over
    /// the command line; only --null-audio is kept).
    pub fn options_from_settings(&self) -> AudioOptions {
        let s = &self.settings;
        AudioOptions {
            host: s.host.clone(),
            buffer_frames: s.buffer_frames,
            sample_rate: s.sample_rate,
            force_null: self.audio_opts.force_null,
            no_input: s.no_input,
            output_device: s.output_device.clone(),
            input_device: s.input_device.clone(),
        }
    }

    pub fn open_settings(&mut self, tab: SettingsTab) {
        self.device_lists = None; // re-list devices
        self.dialog = Some(Dialog::Settings(tab));
    }

    pub fn save_settings(&mut self) {
        if let Err(e) = self.settings.save() {
            self.toast(format!("Could not save settings: {e}"));
        }
    }

    /// Channel configuration of the current input / output device.
    pub fn channel_config(&self, input: bool) -> crate::settings::ChannelConfig {
        let dev = if input { self.audio.info.input.as_ref().map(String::as_str).unwrap_or("") } else { self.audio.info.device.as_str() };
        self.settings.channel_config(input, dev)
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
        self.clamp_selection();
    }

    /// Indices may be stale after undo/redo or structural edits (same frame).
    pub fn clamp_selection(&mut self) {
        if self.selected_track >= self.project.tracks.len() {
            self.selected_track = self.project.tracks.len().saturating_sub(1);
        }
        if let Some(c) = self.selected_clip
            && self.clip(c).is_none()
        {
            self.selected_clip = None;
        }
        self.note_sel.clear();
        self.sanitize_selection();
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

    /// New tracks go below the selected track (the end when the master is selected).
    pub fn add_track(&mut self, kind: TrackKind) {
        let after = if self.master_selected || self.project.tracks.is_empty() {
            None
        } else {
            Some(self.track_sel.iter().copied().max().unwrap_or(self.selected_track))
        };
        self.master_selected = false;
        let at = self.edit(|p| p.insert_track(kind, after));
        self.clear_structure_selection();
        self.selected_track = at;
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
            ..Default::default()
        })
    }

    pub fn launch_clip(&mut self, track: usize, scene: usize) {
        if let Some(rt) = self.rt(track) {
            self.send(Command::LaunchClip { track: rt, scene: scene as u32 });
        }
    }

    /// Launch button released (Gate stops, Repeat stops retriggering).
    pub fn release_clip(&mut self, track: usize, scene: usize) {
        if let Some(rt) = self.rt(track) {
            self.send(Command::ReleaseClip { track: rt, scene: scene as u32 });
        }
    }

    pub fn launch_scene(&mut self, scene: usize) {
        self.send(Command::LaunchScene { scene: scene as u32 });
    }

    /// Space: stop (the playhead stays where it stopped), or play from the start
    /// point (the last position set by clicking), not from where it stopped.
    pub fn toggle_play(&mut self) {
        if self.status.playing {
            self.send_now(Command::Stop);
        } else {
            self.send_now(Command::SetPosition(self.play_start));
            self.send_now(Command::Play);
        }
    }

    /// Move the playhead (and the start point for the next play).
    pub fn set_playhead(&mut self, beat: f64) {
        self.play_start = beat.max(0.0);
        self.send_now(Command::SetPosition(self.play_start));
    }

    /// Set the start marker (clicks in the lanes). While playing the playhead
    /// keeps going; the marker only decides where the next Play starts.
    pub fn set_start_marker(&mut self, beat: f64) {
        self.play_start = beat.max(0.0);
        if !self.status.playing {
            self.send_now(Command::SetPosition(self.play_start));
        }
    }

    /// Live: Stop while playing pauses; Stop while stopped rewinds.
    pub fn stop(&mut self) {
        if self.status.playing {
            self.send_now(Command::Stop);
        } else {
            self.play_start = 0.0;
            self.send_now(Command::Rewind);
        }
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
                };
                *f = !*f;
                *f
            };
            // Plain click on solo is exclusive (Live default); Ctrl adds.
            if exclusive && v && flag == TrackFlag::Solo {
                for (i, t) in p.tracks.iter_mut().enumerate() {
                    if i != track {
                        t.solo = false;
                    }
                }
            }
        });
    }

    /// Arm button: on a track inside a multi-selection it (dis)arms every
    /// selected track; Ctrl+click arms just this one.
    pub fn click_arm(&mut self, track: usize, ctrl: bool) {
        let sel = self.selected_tracks();
        let targets: Vec<usize> = if !ctrl && sel.len() > 1 && sel.contains(&track) { sel } else { vec![track] };
        let Some(on) = self.project.tracks.get(track).map(|t| !t.arm) else { return };
        self.edit(|p| {
            for &i in &targets {
                if let Some(t) = p.tracks.get_mut(i)
                    && t.kind != TrackKind::Group
                    && t.has_input()
                {
                    t.arm = on;
                }
            }
        });
    }

    /// Monitor radio (applies to every selected track if `track` is selected).
    /// `None` = return to the kind's default.
    pub fn set_monitor(&mut self, track: usize, mode: Option<Monitor>) {
        let targets = self.io_targets(track);
        self.edit(|p| {
            for &i in &targets {
                if let Some(t) = p.tracks.get_mut(i)
                    && t.kind != TrackKind::Group
                {
                    t.monitoring = mode;
                }
            }
        });
    }

    /// I/O changes apply to all selected tracks when `track` is one of them.
    pub fn io_targets(&self, track: usize) -> Vec<usize> {
        let sel = self.selected_tracks();
        if sel.contains(&track) { sel } else { vec![track] }
    }

    /// Change a routing setting on `track` (and the other selected tracks of
    /// the same kind).
    pub fn set_routing(&mut self, track: usize, f: impl Fn(&mut daw_project::Track)) {
        let Some(kind) = self.project.tracks.get(track).map(|t| t.kind) else { return };
        let targets = self.io_targets(track);
        self.edit(|p| {
            for &i in &targets {
                if let Some(t) = p.tracks.get_mut(i)
                    && t.kind == kind
                {
                    f(t);
                }
            }
            p.normalize();
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

    /// Keep a recorded lane in the cache (mono lanes as mono samples).
    fn store_lane(&mut self, lane: &daw_core::TakeLane) -> String {
        if !lane.mono {
            return self.store_audio("take", &lane.audio);
        }
        self.rec_counter += 1;
        let key = format!("{RECORDING_PREFIX}take-{}-{}", std::process::id(), self.rec_counter);
        let l: Vec<f32> = lane.audio.iter().step_by(2).copied().collect();
        self.cache.insert(&key, SampleBuffer::mono(format!("take {}", self.rec_counter), self.audio.info.sample_rate, l));
        key
    }

    /// Keep interleaved stereo audio in the cache under a `recording:` key
    /// (written next to the project on save). Used for takes and consolidated clips.
    pub fn store_audio(&mut self, kind: &str, interleaved: &[f32]) -> String {
        self.rec_counter += 1;
        let key = format!("{RECORDING_PREFIX}{kind}-{}-{}", std::process::id(), self.rec_counter);
        let (l, r): (Vec<f32>, Vec<f32>) = interleaved.as_chunks::<2>().0.iter().map(|f| (f[0], f[1])).unzip();
        self.cache.insert(&key, SampleBuffer::new(format!("{kind} {}", self.rec_counter), self.audio.info.sample_rate, l, r));
        key
    }

    /// Turn a finished take into clips.
    pub fn apply_take(&mut self, mut take: Take) {
        let len = (take.end_beat - take.start_beat).max(0.0);
        if len <= 1e-6 {
            return;
        }
        let rts: Vec<Option<u16>> = (0..self.project.tracks.len()).map(|i| self.rt(i)).collect();
        // One sample per armed audio track (its own input), latency-compensated:
        // "Keep Monitoring Latency" (In/Auto) keeps the monitoring delay.
        let mut audio_keys: Vec<Option<String>> = vec![None; self.project.tracks.len()];
        for (i, t) in self.project.tracks.iter().enumerate() {
            if !(t.arm && t.kind == TrackKind::Audio) {
                continue;
            }
            let Some(lane) = rts[i].and_then(|rt| take.lanes.iter_mut().find(|l| l.track == rt)) else { continue };
            let keep = t.keep_monitor_latency && t.monitor_mode() != Monitor::Off;
            let resampling = t.audio_in == daw_project::AudioIn::Resampling;
            let latency = if resampling {
                0 // internal: no device latency
            } else if keep {
                self.audio.info.output_latency_frames()
            } else {
                self.audio.info.round_trip_frames()
            };
            lane.compensate_latency(latency);
            if !lane.audio.is_empty()
                && (resampling || self.audio.info.input.is_ok() || !matches!(t.audio_in, daw_project::AudioIn::Ext { .. }))
            {
                audio_keys[i] = Some(String::new());
            }
        }
        for i in 0..audio_keys.len() {
            if audio_keys[i].is_some()
                && let Some(lane) = rts[i].and_then(|rt| take.lanes.iter().find(|l| l.track == rt)).cloned()
            {
                audio_keys[i] = Some(self.store_lane(&lane));
            }
        }
        let tempo = self.project.tempo_bpm;
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
                        muted: false,
                        mpe: 0,
                    })
                    .collect()
            };
            let mut arr_new: Vec<(usize, Clip)> = Vec::new();
            let names: Vec<String> = (0..p.tracks.len()).map(|i| p.next_clip_name(i)).collect();
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
                            // Engine clip_pos is already a content position (markers, loop).
                            let (ls, le) =
                                if clip.loop_end > clip.loop_start + 1e-9 { (clip.loop_start, clip.loop_end) } else { (0.0, clip.length) };
                            for n in take.notes.iter().filter(|n| Some(n.track) == rts[i]) {
                                let u = n.clip_pos.unwrap_or(n.start_beat - take.start_beat);
                                let start = if u >= le { ls + (u - le).rem_euclid(le - ls) } else { u };
                                existing.push(Note {
                                    start,
                                    length: (n.end_beat - n.start_beat).max(1.0 / 64.0),
                                    pitch: n.pitch,
                                    velocity: n.velocity,
                                    muted: false,
                                    mpe: 0,
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
                    TrackKind::Audio => match &audio_keys[i] {
                        Some(k) => ClipContent::Audio { sample: k.clone(), offset_frames: 0.0, warp_bpm: Some(tempo), gain_db: 0.0 },
                        None => continue,
                    },
                    TrackKind::Group => continue,
                };
                created += 1;
                let id = p.next_id;
                p.next_id += 1;
                let color =
                    if p.colors.random_clips { daw_project::PALETTE[(id as usize * 7 + i) % daw_project::PALETTE.len()] } else { t.color };
                let clip = Clip { id, name: String::new(), color, length: len, looping: true, content, ..Default::default() };
                let clip = Clip { name: names[i].clone(), ..clip };
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
        // Routed by the engine like a MIDI port ("Computer Keyboard" / "All Ins").
        if pressed {
            if self.kb.held.iter().any(|(k, _)| *k == key) {
                return true; // key repeat
            }
            let pitch = ((self.kb.octave + 1) * 12 + off as i32).clamp(0, 127) as u8;
            self.kb.held.push((key, pitch));
            self.midi.activity.note_in(usize::MAX, 0);
            self.send_now(Command::KeyboardNote { pitch, velocity: self.kb.velocity });
        } else if let Some(i) = self.kb.held.iter().position(|(k, _)| *k == key) {
            let (_, pitch) = self.kb.held.remove(i);
            self.midi.activity.note_in(usize::MAX, 0);
            self.send_now(Command::KeyboardNote { pitch, velocity: 0 });
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

    /// Device rate changed: resample the cache and move every stored sample
    /// position (project + undo history) to the new rate.
    pub fn adopt_engine_rate(&mut self) {
        let (old, sr) = (self.cache.sample_rate(), self.audio.info.sample_rate);
        if old == sr {
            return;
        }
        self.cache.change_rate(sr);
        let fix = |p: &mut Project| {
            p.rescale_frames(old, sr);
            p.frame_rate = sr;
        };
        fix(&mut self.project);
        self.history.for_each_snapshot(fix);
        self.preview = None;
    }

    /// Project positions were saved at `frame_rate`: bring them to the cache rate.
    fn adopt_project_rate(&mut self) {
        let sr = self.cache.sample_rate();
        let p = &mut self.project;
        if p.frame_rate != 0 && p.frame_rate != sr {
            p.rescale_frames(p.frame_rate, sr);
        }
        p.frame_rate = sr;
    }

    fn replace_project(&mut self, p: Project, path: Option<PathBuf>) {
        self.send_now(Command::Stop);
        self.send_now(Command::StopAllClips);
        self.send_now(Command::Rewind);
        self.project = p;
        self.adopt_project_rate();
        self.path = path;
        self.history.clear();
        self.dirty = false;
        self.needs_compile = true;
        self.selected_clip = None;
        self.selected_track = 0;
        self.play_start = 0.0;
        self.renaming = None;
        self.clear_structure_selection();
        self.arr_sel.clear();
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
        if self.project.tracks.get(track).is_none_or(|t| t.kind == TrackKind::Group) {
            return; // group buses have no instrument
        }
        self.edit(|p| {
            if let Some(t) = p.tracks.get_mut(track) {
                t.instrument = def;
                t.kind = TrackKind::Midi;
            }
        });
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum TrackFlag {
    Mute,
    Solo,
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
