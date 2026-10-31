//! Clip View (Live 12 ch. 8–9, 11): title bar, Clip / Launch / Audio / Notes
//! (Pitch & Time, Transform, Generate) / Envelopes / MPE panels, the marker
//! strip (start / end / loop brace), envelope + MPE lanes and the warped
//! sample editor with warp markers and transients.

use super::widgets::*;
use crate::app::{ClipRef, DawApp};
use daw_project::clipx::{self, ClipEnvelope, EnvPoint, EnvTarget, FollowAction, LAUNCH_QUANTS, LaunchMode, WarpMarker};
use daw_project::miditools::{self as mt, Generate, PKind, Transform};
use daw_project::{Clip, ClipContent, Note};
use eframe::egui::{self, Align2, Color32, CornerRadius, Rect, Sense, Stroke, pos2, vec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    /// Notes (MIDI) / Sample (audio).
    #[default]
    Main,
    Envelopes,
    Mpe,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NotesPanel {
    #[default]
    PitchTime,
    Transform,
    Generate,
}

/// Snapshot for MIDI Tools Auto Apply: the notes before the tool ran.
#[derive(Clone, Debug)]
pub struct ToolBase {
    clip_id: u64,
    notes: Vec<Note>,
    mpe: Vec<clipx::NoteMpe>,
    targets: Vec<usize>,
    range: (f64, f64),
    applied: Vec<Note>,
}

impl ToolBase {
    pub fn new(clip_id: u64, notes: Vec<Note>, mpe: Vec<clipx::NoteMpe>, targets: Vec<usize>, range: (f64, f64)) -> Self {
        Self { clip_id, applied: notes.clone(), notes, mpe, targets, range }
    }
    pub fn clip_id(&self) -> u64 {
        self.clip_id
    }
    pub fn applied(&self) -> &Vec<Note> {
        &self.applied
    }
    pub fn set_applied(&mut self, v: Vec<Note>) {
        self.applied = v;
    }
    pub fn parts(&self) -> (&[Note], &[clipx::NoteMpe], &[usize], (f64, f64)) {
        (&self.notes, &self.mpe, &self.targets, self.range)
    }
    pub fn into_original(self) -> (Vec<Note>, Vec<clipx::NoteMpe>) {
        (self.notes, self.mpe)
    }
}

#[derive(Debug)]
pub struct ClipView {
    pub tab: Tab,
    pub panel: NotesPanel,
    pub folded: bool,
    pub env_target: Option<EnvTarget>,
    pub mpe_lane: usize,
    pub transform: Transform,
    pub generate: Generate,
    pub t_params: Vec<Vec<f32>>,
    pub g_params: Vec<Vec<f32>>,
    pub auto_apply: bool,
    pub base: Option<ToolBase>,
    pub interval: i32,
    pub duration: f64,
    pub humanize: f64,
    pub follow: bool,
    /// Loop brace selected: arrow keys move / resize it.
    pub brace_sel: bool,
    pub quant_grid: f32,
    pub quant_amount: f32,
    pub sel_marker: Option<usize>,
    /// Envelope point being dragged / MPE drawing in progress.
    pub env_drag: Option<usize>,
    /// Left-panel sections unfolded (cview::Sec).
    pub open: [bool; 6],
    /// Pitch & Time stretch knob (applied on release).
    pub stretch: f64,
    /// Piano roll Fold: only rows that hold notes.
    pub fold: bool,
    /// Library groove picked in the chooser (added to the pool next frame).
    pub pending_groove: Option<clipx::Groove>,
    pub show_pool: bool,
    /// Audio Pitch knob selected: Up/Down = ±1 st, Shift = ±12 (queued by the shortcuts).
    pub pitch_focus: bool,
    /// Clip gain fader selected (corner brackets).
    pub gain_focus: bool,
    pub pitch_nudge: i32,
    /// The knob was drawn since the last shortcuts pass.
    pub pitch_drawn: bool,
}

impl Default for ClipView {
    fn default() -> Self {
        Self {
            tab: Tab::Main,
            panel: NotesPanel::PitchTime,
            folded: false,
            env_target: None,
            mpe_lane: 0,
            transform: Transform::Arpeggiate,
            generate: Generate::Rhythm,
            t_params: Transform::ALL.iter().map(|t| mt::defaults(t.params())).collect(),
            g_params: Generate::ALL.iter().map(|g| mt::defaults(g.params())).collect(),
            auto_apply: true,
            base: None,
            interval: 12,
            duration: 0.0,
            humanize: 0.5,
            follow: false,
            brace_sel: false,
            quant_grid: 7.0,
            quant_amount: 100.0,
            sel_marker: None,
            env_drag: None,
            open: super::cview::OPEN_DEFAULT,
            stretch: 1.0,
            fold: false,
            pending_groove: None,
            show_pool: false,
            pitch_focus: false,
            gain_focus: false,
            pitch_nudge: 0,
            pitch_drawn: false,
        }
    }
}

// ---- helpers ---------------------------------------------------------------------------------

/// Bars.beats.sixteenths: positions are 1-based ("1.1.1" = 0), lengths 0-based.
pub fn bbs(v: f64, bpb: f64, pos: bool) -> String {
    let o = if pos { 1.0 } else { 0.0 };
    let v = v.max(if pos { f64::NEG_INFINITY } else { 0.0 });
    let neg = v < -1e-9;
    let a = v.abs() + 1e-9;
    let bars = (a / bpb).floor();
    let beats = (a - bars * bpb).floor();
    let six = ((a - bars * bpb - beats) * 4.0).floor();
    format!("{}{}.{}.{}", if neg { "-" } else { "" }, bars + o, beats + o, six + o)
}

pub fn parse_bbs(s: &str, bpb: f64, pos: bool) -> Option<f64> {
    let o = if pos { 1.0 } else { 0.0 };
    let parts: Vec<f64> = s.trim().split('.').map(|x| x.trim().parse::<f64>()).collect::<Result<_, _>>().ok()?;
    match parts.as_slice() {
        [b] => Some((b - o) * bpb),
        [b, q] => Some((b - o) * bpb + (q - o)),
        [b, q, x] => Some((b - o) * bpb + (q - o) + (x - o) / 4.0),
        _ => None,
    }
}

fn beat_field(ui: &mut egui::Ui, v: &mut f64, bpb: f64, pos: bool, range: std::ops::RangeInclusive<f64>) -> egui::Response {
    ui.add(
        egui::DragValue::new(v)
            .range(range)
            .speed(0.05)
            .custom_formatter(move |x, _| bbs(x, bpb, pos))
            .custom_parser(move |s| parse_bbs(s, bpb, pos)),
    )
}

/// Collects gesture / change flags from widgets (one undo step per gesture).
#[derive(Default)]
pub(super) struct Ed {
    pub(super) gesture: bool,
    pub(super) changed: bool,
}

impl Ed {
    pub(super) fn t(&mut self, r: egui::Response) -> egui::Response {
        self.gesture |= r.drag_started() || r.gained_focus() || r.clicked();
        self.changed |= r.changed();
        r
    }
    pub(super) fn set(&mut self) {
        self.gesture = true;
        self.changed = true;
    }
}

/// Current content play position of the clip (None if it isn't playing).
pub fn play_pos(app: &DawApp, cref: ClipRef) -> Option<f64> {
    match cref {
        ClipRef::Slot { track, scene } => {
            let st = app.track_status.get(track).copied().unwrap_or_default();
            (st.playing_scene == Some(scene as u32)).then_some(st.clip_pos)
        }
        ClipRef::Arr { track, index } => {
            let a = app.project.tracks.get(track)?.arrangement.get(index)?;
            let b = app.status.beat_position - a.start;
            let off = app.track_status.get(track).is_some_and(|s| s.arrangement_off);
            if app.status.playing && (0.0..a.clip.length).contains(&b) && !off { a.clip.content_at(b) } else { None }
        }
    }
}

/// Play window (content beats): what the markers enclose.
pub fn play_window(c: &Clip) -> (f64, f64) {
    if c.looping {
        let (a, b) = c.loop_range();
        (c.start.min(a), b)
    } else {
        (c.start, c.start + c.length)
    }
}

/// Region tools / Crop act on: the loop (looping) or start..end.
pub fn region(c: &Clip) -> (f64, f64) {
    if c.looping { c.loop_range() } else { (c.start, c.start + c.length) }
}

pub(super) fn commit(app: &mut DawApp, cref: ClipRef, ed: Ed, c: Clip) {
    if ed.gesture {
        app.begin_gesture();
    }
    if ed.changed {
        if let Some(dst) = DawApp::clip_mut(&mut app.project, cref) {
            *dst = c;
        }
        app.needs_compile = true;
        app.dirty = true;
    }
}

pub(super) fn audio_map(app: &DawApp, c: &Clip) -> daw_engine::WarpMap {
    match &c.content {
        ClipContent::Audio { offset_frames, warp_bpm, .. } => {
            let sr = app.audio.info.sample_rate as f64;
            daw_core::compile::warp_map(c, *offset_frames, Some(warp_bpm.unwrap_or(app.project.tempo_bpm)), sr)
        }
        _ => daw_engine::WarpMap::linear(0.0, 1.0),
    }
}

// ---- panels -------------------------------------------------------------------------------------

pub(super) fn launch_box(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef, clip: &Clip) {
    let bpb = app.project.beats_per_bar as f64;
    let mut c = clip.clone();
    let mut ed = Ed::default();
    egui::Grid::new("cv_launch").num_columns(2).spacing([4.0, 3.0]).show(ui, |ui| {
        ui.label("Mode");
        egui::ComboBox::from_id_salt("cv_lmode").width(90.0).selected_text(c.launch.mode.label()).show_ui(ui, |ui| {
            for m in LaunchMode::ALL {
                ed.t(ui.selectable_value(&mut c.launch.mode, m, m.label()));
            }
        });
        ui.end_row();
        ui.label("Legato");
        if ui.add(egui::Button::selectable(c.launch.legato, "Legato")).clicked() {
            c.launch.legato = !c.launch.legato;
            ed.set();
        }
        ui.end_row();
        ui.label("Quantize");
        let ql = LAUNCH_QUANTS.iter().find(|q| q.0 == c.launch.quantize).map_or("Global", |q| q.1);
        egui::ComboBox::from_id_salt("cv_lq").width(90.0).selected_text(ql).show_ui(ui, |ui| {
            for (v, l) in LAUNCH_QUANTS {
                ed.t(ui.selectable_value(&mut c.launch.quantize, v, l));
            }
        });
        ui.end_row();
        ui.label("Velocity");
        ed.t(ui.add(egui::DragValue::new(&mut c.launch.velocity).range(0.0..=100.0).suffix(" %")));
        ui.end_row();
    });
    ui.separator();
    let fa = &mut c.launch.follow;
    ui.horizontal(|ui| {
        if ui.add(egui::Button::selectable(fa.enabled, "Follow Action")).clicked() {
            fa.enabled = !fa.enabled;
            ed.set();
        }
    });
    ui.add_enabled_ui(fa.enabled, |ui| {
        egui::Grid::new("cv_follow").num_columns(2).spacing([4.0, 3.0]).show(ui, |ui| {
            for (k, name) in ["A", "B"].iter().enumerate() {
                ui.label(*name);
                ui.horizontal(|ui| {
                    let (act, jump) = if k == 0 { (&mut fa.a, &mut fa.jump_a) } else { (&mut fa.b, &mut fa.jump_b) };
                    egui::ComboBox::from_id_salt(("cv_fa", k)).width(80.0).selected_text(act.label()).show_ui(ui, |ui| {
                        for a in FollowAction::ALL {
                            ed.t(ui.selectable_value(act, a, a.label()));
                        }
                    });
                    if *act == FollowAction::Jump {
                        if *jump == 0 {
                            *jump = 1;
                        }
                        ed.t(ui.add(egui::DragValue::new(jump).range(1..=999)).on_hover_text("Jump target (scene)"));
                    }
                });
                ui.end_row();
            }
            ui.label("Chance");
            ed.t(ui.add(egui::Slider::new(&mut fa.chance_a, 0..=100).text("A %")));
            ui.end_row();
            ui.label("Time");
            ui.add_enabled_ui(!fa.linked, |ui| ed.t(beat_field(ui, &mut fa.time, bpb, false, (1.0 / 16.0)..=4096.0)));
            ui.end_row();
            ui.label("");
            ui.horizontal(|ui| {
                if ui
                    .add(egui::Button::selectable(fa.linked, "Linked"))
                    .on_hover_text("Follow Action time = clip length × multiplier")
                    .clicked()
                {
                    fa.linked = !fa.linked;
                    ed.set();
                }
                if fa.linked {
                    ed.t(ui.add(egui::DragValue::new(&mut fa.multiplier).range(1..=64).prefix("×")));
                }
            });
            ui.end_row();
        });
    });
    ui.separator();
    egui::Grid::new("cv_prog").num_columns(2).spacing([4.0, 3.0]).show(ui, |ui| {
        for (name, v) in [("Bank", &mut c.program.bank), ("Sub-Bank", &mut c.program.sub), ("Program", &mut c.program.program)] {
            ui.label(name);
            // -1 shows as "—" (none sent).
            let mut x: i32 = v.map_or(-1, |b| b as i32 + 1);
            let r = ed.t(ui.add(
                egui::DragValue::new(&mut x).range(-1..=128).custom_formatter(|n, _| if n < 0.5 { "—".into() } else { format!("{n:.0}") }),
            ));
            if r.changed() {
                *v = if x < 1 { None } else { Some((x - 1) as u8) };
            }
            ui.end_row();
        }
    });
    commit(app, cref, ed, c);
}

pub(super) fn open_external(path: &str) {
    let p = std::path::Path::new(path);
    if !p.exists() {
        return;
    }
    #[cfg(target_os = "linux")]
    let cmd = "xdg-open";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(target_os = "windows")]
    let cmd = "explorer";
    let _ = std::process::Command::new(cmd).arg(p).spawn();
}

pub(super) fn stretch_sel(x: &mut [Note], f: f64) {
    let a = x.iter().map(|n| n.start).fold(f64::INFINITY, f64::min);
    if a.is_finite() {
        mt::stretch(x, a, f);
    }
}

/// Run `f` on the selected notes (or all), writing them back in place.
pub(super) fn with_sel(v: &mut [Note], sel: &[usize], f: impl FnOnce(&mut [Note])) {
    if sel.is_empty() {
        f(v);
        return;
    }
    let mut picked: Vec<Note> = sel.iter().filter_map(|&i| v.get(i).copied()).collect();
    f(&mut picked);
    for (&i, n) in sel.iter().zip(picked) {
        if let Some(d) = v.get_mut(i) {
            *d = n;
        }
    }
}

/// MIDI Tools panel (Transform or Generate).
pub(super) fn tools(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef, clip: &Clip) {
    let gen_ = app.cv.panel == NotesPanel::Generate;
    let mut changed = false;
    ui.horizontal(|ui| {
        if gen_ {
            let cur = app.cv.generate;
            egui::ComboBox::from_id_salt("cv_gen").width(110.0).selected_text(cur.label()).show_ui(ui, |ui| {
                for g in Generate::ALL {
                    if ui.selectable_value(&mut app.cv.generate, g, g.label()).changed() {
                        app.cv.base = None;
                    }
                }
            });
        } else {
            let cur = app.cv.transform;
            egui::ComboBox::from_id_salt("cv_tr").width(110.0).selected_text(cur.label()).show_ui(ui, |ui| {
                for t in Transform::ALL {
                    if ui.selectable_value(&mut app.cv.transform, t, t.label()).changed() {
                        app.cv.base = None;
                    }
                }
            });
        }
        let r = ui
            .add(egui::Button::selectable(app.cv.auto_apply, "Auto"))
            .on_hover_text("Auto Apply: changes apply immediately; off restores the original notes");
        if r.clicked() {
            app.cv.auto_apply = !app.cv.auto_apply;
            if !app.cv.auto_apply {
                app.restore_tool_base(cref);
            }
        }
    });
    let (specs, idx) = if gen_ {
        let i = Generate::ALL.iter().position(|g| *g == app.cv.generate).unwrap_or(0);
        (app.cv.generate.params(), i)
    } else {
        let i = Transform::ALL.iter().position(|t| *t == app.cv.transform).unwrap_or(0);
        (app.cv.transform.params(), i)
    };
    let mpe_tool = !gen_ && app.cv.transform.is_mpe();
    egui::Grid::new(("cv_tool", gen_)).num_columns(2).spacing([4.0, 2.0]).show(ui, |ui| {
        let vals = if gen_ { &mut app.cv.g_params[idx] } else { &mut app.cv.t_params[idx] };
        for (k, s) in specs.iter().enumerate() {
            // Chop pattern: 16 step toggles.
            if s.name == "Pattern" && !gen_ {
                ui.label("Pattern");
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 1.0;
                    let mut bits = vals[k] as u32;
                    for i in 0..16 {
                        let on = bits & (1 << i) != 0;
                        if ui.add(egui::Button::selectable(on, "").min_size(vec2(8.0, 12.0))).clicked() {
                            bits ^= 1 << i;
                            changed = true;
                        }
                    }
                    vals[k] = bits as f32;
                });
                ui.end_row();
                continue;
            }
            ui.label(s.name);
            let v = &mut vals[k];
            let r = match s.kind {
                PKind::Num => {
                    // Whole-number specs (Steps, Parts, Voices…) drag in integers.
                    let int = [s.min, s.max, s.default].iter().all(|x| x.fract() == 0.0) && s.max - s.min > 2.0;
                    let d = egui::DragValue::new(v).range(s.min..=s.max);
                    ui.add(if int { d.speed(0.1).fixed_decimals(0) } else { d.speed(((s.max - s.min) / 200.0).max(0.01)).max_decimals(2) })
                }
                PKind::Pct => ui.add(egui::DragValue::new(v).range(s.min..=s.max).suffix(" %")),
                PKind::Toggle => {
                    let mut on = *v > 0.5;
                    let r = ui.checkbox(&mut on, "");
                    *v = if on { 1.0 } else { 0.0 };
                    r
                }
                PKind::Grid => {
                    let cur = mt::GRIDS[(*v as usize).min(mt::GRIDS.len() - 1)].0;
                    let mut ch = false;
                    let r = egui::ComboBox::from_id_salt(("cv_g", gen_, k)).width(60.0).selected_text(cur).show_ui(ui, |ui| {
                        for (i, (l, _)) in mt::GRIDS.iter().enumerate() {
                            ch |= ui.selectable_value(v, i as f32, *l).changed();
                        }
                    });
                    changed |= ch;
                    r.response
                }
                PKind::Choice(list) => {
                    let cur = list[(*v as usize).min(list.len() - 1)];
                    let mut ch = false;
                    let r = egui::ComboBox::from_id_salt(("cv_c", gen_, k)).width(96.0).selected_text(cur).show_ui(ui, |ui| {
                        for (i, l) in list.iter().enumerate() {
                            ch |= ui.selectable_value(v, i as f32, *l).changed();
                        }
                    });
                    changed |= ch;
                    r.response
                }
            };
            changed |= r.changed();
            ui.end_row();
        }
    });
    if mpe_tool {
        ui.label(egui::RichText::new("Writes note expression (MPE)").small().color(Color32::GRAY));
    }
    // Stacks: chord pads (one click = that chord into the current slot).
    if gen_ && app.cv.generate == Generate::Stacks {
        ui.horizontal_wrapped(|ui| {
            for (i, (name, _)) in mt::CHORDS.iter().enumerate() {
                if ui.small_button(*name).clicked() {
                    app.cv.g_params[idx][2] = i as f32;
                    changed = true;
                }
            }
        });
    }
    ui.horizontal(|ui| {
        let apply = ui.button("Apply").on_hover_text("Ctrl+Enter").clicked()
            || ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter));
        if ui.button("Reset").on_hover_text("Default parameters").clicked() {
            let d = mt::defaults(specs);
            if gen_ {
                app.cv.g_params[idx] = d;
            } else {
                app.cv.t_params[idx] = d;
            }
            changed = true;
        }
        if apply {
            app.run_tool(cref, clip, true);
            app.cv.base = None; // committed: the next change starts from here
        } else if changed && app.cv.auto_apply {
            app.run_tool(cref, clip, false);
        }
    });
    let target = if !app.note_sel.is_empty() {
        "selected notes"
    } else if app.pr_time_sel.is_some() {
        "time selection"
    } else {
        "loop"
    };
    ui.label(egui::RichText::new(format!("Acts on: {target}")).small().color(Color32::GRAY));
}

pub(super) fn envelopes_box(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef, clip: &Clip) {
    let audio = matches!(clip.content, ClipContent::Audio { .. });
    let list: &[EnvTarget] = if audio { &EnvTarget::AUDIO } else { &EnvTarget::MIDI };
    let cur = app.cv.env_target.filter(|t| list.contains(t)).unwrap_or(list[0]);
    app.cv.env_target = Some(cur);
    ui.label("Control");
    for t in list {
        let has = clip.envelope(*t).is_some_and(|e| !e.points.is_empty());
        let txt = if has { egui::RichText::new(t.label()).color(ACCENT) } else { egui::RichText::new(t.label()) };
        if ui.selectable_label(cur == *t, txt).clicked() {
            app.cv.env_target = Some(*t);
        }
    }
    ui.separator();
    let mut c = clip.clone();
    let mut ed = Ed::default();
    let bpb = app.project.beats_per_bar as f64;
    let e = c.envelope_mut(cur);
    ui.horizontal(|ui| {
        if ui
            .add(egui::Button::selectable(!e.unlinked, "Linked"))
            .on_hover_text("Linked: follows the clip loop; unlinked: own loop length")
            .clicked()
        {
            e.unlinked = !e.unlinked;
            if e.loop_len <= 0.0 {
                e.loop_len = 4.0;
            }
            ed.set();
        }
    });
    if e.unlinked {
        ui.horizontal(|ui| {
            ui.label("Length");
            ed.t(beat_field(ui, &mut e.loop_len, bpb, false, (1.0 / 16.0)..=4096.0));
        });
    }
    if ui.button("Clear Envelope").clicked() {
        e.points.clear();
        ed.set();
    }
    ui.label(egui::RichText::new("Lane: click adds · drag moves\ndouble-click deletes · Shift: fine").small().color(Color32::GRAY));
    if ed.changed {
        c.envelopes.retain(|e| !e.points.is_empty() || e.unlinked);
    }
    commit(app, cref, ed, c);
}

pub(super) fn mpe_box(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef, clip: &Clip) {
    ui.label("Note Expression");
    for (i, l) in ["Pitch", "Slide", "Pressure"].iter().enumerate() {
        ui.selectable_value(&mut app.cv.mpe_lane, i, *l);
    }
    ui.separator();
    let any = !app.note_sel.is_empty();
    if ui.add_enabled(any, egui::Button::new("Clear Expression")).on_hover_text("Selected notes, this lane").clicked() {
        let sel = app.note_sel.clone();
        let lane = app.cv.mpe_lane;
        app.edit(|p| {
            if let Some(c) = DawApp::clip_mut(p, cref) {
                for i in sel {
                    if let Some(e) = c.note_mpe_mut(i) {
                        match lane {
                            0 => e.pitch.clear(),
                            1 => e.slide.clear(),
                            _ => e.pressure.clear(),
                        }
                    }
                }
                c.compact_mpe();
            }
        });
    }
    let _ = clip;
    ui.label(
        egui::RichText::new("Select a note, then draw in the\nlane below. Glissando / LFO\ntools (Transform) also write here.")
            .small()
            .color(Color32::GRAY),
    );
}

/// Groove Pool window: per-groove Base / Quantize / Timing / Random /
/// Velocity, the global Amount, extract from the selected MIDI clip.
pub fn groove_pool(app: &mut DawApp, ctx: &egui::Context) {
    if !app.cv.show_pool {
        return;
    }
    let mut open = true;
    let mut changed = false;
    let mut gesture = false;
    let mut pool = app.project.grooves.clone();
    let mut amount = app.project.groove_amount;
    let mut remove: Option<usize> = None;
    let mut extract = false;
    egui::Window::new("Groove Pool").open(&mut open).resizable(true).default_width(520.0).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Global Amount");
            let r = ui.add(egui::Slider::new(&mut amount, 0.0..=1.3).custom_formatter(|v, _| format!("{:.0} %", v * 100.0)));
            gesture |= r.drag_started();
            changed |= r.changed();
            if ui.button("Extract from selected clip").on_hover_text("New groove from the selected MIDI clip's timing").clicked() {
                extract = true;
            }
        });
        egui::Grid::new("pool").num_columns(7).striped(true).show(ui, |ui| {
            for h in ["Groove", "Base", "Quantize", "Timing", "Random", "Velocity", ""] {
                ui.label(egui::RichText::new(h).small().strong());
            }
            ui.end_row();
            for (i, g) in pool.iter_mut().enumerate() {
                ui.label(&g.name);
                let bl = mt::GRIDS.iter().find(|x| (x.1 - g.base).abs() < 1e-9).map_or("1/16", |x| x.0);
                egui::ComboBox::from_id_salt(("gb", i)).width(50.0).selected_text(bl).show_ui(ui, |ui| {
                    for (l, b) in mt::GRIDS {
                        if ui.selectable_value(&mut g.base, b, l).changed() {
                            gesture = true;
                            changed = true;
                        }
                    }
                });
                for (v, lo) in [(&mut g.quantize, 0.0), (&mut g.timing, 0.0), (&mut g.random, 0.0), (&mut g.velocity, -100.0)] {
                    let r = ui.add(egui::DragValue::new(v).range(lo..=100.0).suffix(" %"));
                    gesture |= r.drag_started() || r.gained_focus();
                    changed |= r.changed();
                }
                if ui.small_button("x").on_hover_text("Remove from pool").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
        if pool.is_empty() {
            ui.label(egui::RichText::new("Pick a groove in a clip's Groove chooser to add it here.").color(Color32::GRAY));
        }
    });
    app.cv.show_pool = open;
    if extract
        && let Some(c) = app.selected_clip.and_then(|c| app.clip(c)).cloned()
        && let ClipContent::Midi { notes } = &c.content
    {
        let mut g = clipx::Groove::from_notes(&c.name, notes, c.loop_len());
        g.id = pool.iter().map(|x| x.id).max().unwrap_or(0) + 1;
        pool.push(g);
        gesture = true;
        changed = true;
    }
    if let Some(i) = remove {
        let id = pool[i].id;
        pool.remove(i);
        gesture = true;
        changed = true;
        // Clips using it lose their groove.
        for t in &mut app.project.tracks {
            for c in t.slots.iter_mut().flatten().chain(t.arrangement.iter_mut().map(|a| &mut a.clip)) {
                if c.groove == Some(id) {
                    c.groove = None;
                }
            }
        }
    }
    if gesture {
        app.begin_gesture();
    }
    if changed {
        app.project.grooves = pool;
        app.project.groove_amount = amount;
        app.needs_compile = true;
        app.dirty = true;
    }
}

// ---- marker strip ------------------------------------------------------------------------------

/// Start / end flags and the loop brace above the editor. Drags move them
/// (snapped); the brace moves as a whole from its middle.
#[allow(clippy::too_many_arguments)]
pub fn marker_strip(
    app: &mut DawApp,
    ui: &mut egui::Ui,
    rect: Rect,
    cref: ClipRef,
    clip: &Clip,
    x_of: &dyn Fn(f64) -> f32,
    beat_at: &dyn Fn(f32) -> f64,
    snap: &dyn Fn(f64) -> f64,
) {
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::ZERO, gray(36));
    let (ls, le) = clip.loop_range();
    let col = rgb(clip.color);
    // Live 12 (22 px): loop brace row on top, start / end flags below.
    let two = rect.height() >= 20.0;
    let mid = (rect.min.y + rect.height() / 2.0).round();
    let (brace_y, flag_y) = if two {
        ((rect.min.y + 2.0, mid - 1.0), (mid, rect.max.y))
    } else {
        ((rect.min.y + 2.0, rect.max.y - 1.0), (rect.min.y, rect.max.y))
    };
    let brace = Rect::from_min_max(pos2(x_of(ls), brace_y.0), pos2(x_of(le), brace_y.1));
    if two {
        // Measured (hh-piano-roll-note-selected): brace row #3c between #91 lines,
        // marker row #36; outlined #5d flags 7×11 with #91 edges.
        let _ = brace;
        let row1 = Rect::from_min_max(pos2(rect.min.x, rect.min.y), pos2(rect.max.x, mid));
        let row2 = Rect::from_min_max(pos2(rect.min.x, mid), rect.max);
        p.rect_filled(row1, CornerRadius::ZERO, gray(0x36));
        p.rect_filled(row2, CornerRadius::ZERO, gray(0x36));
        let lc = if !clip.looping {
            gray(0x5d)
        } else if app.cv.brace_sel {
            super::theme::SEL_BLUE
        } else {
            gray(0x91)
        };
        let br = Rect::from_min_max(pos2(x_of(ls), row1.min.y), pos2(x_of(le), row1.max.y));
        if clip.looping {
            p.rect_filled(br, CornerRadius::ZERO, gray(0x3c));
        }
        p.line_segment([pos2(br.min.x, br.min.y + 0.5), pos2(br.max.x, br.min.y + 0.5)], Stroke::new(1.0, lc));
        p.line_segment([pos2(br.min.x, br.max.y - 0.5), pos2(br.max.x, br.max.y - 0.5)], Stroke::new(1.0, lc));
        let oflag = |x: f32, row: Rect, right: bool, edge: Color32| {
            let d = if right { -7.0 } else { 7.0 };
            let x = x + if right { -0.5 } else { 0.5 };
            let pts = vec![pos2(x, row.min.y), pos2(x + d, row.center().y), pos2(x, row.max.y)];
            p.add(egui::Shape::convex_polygon(pts.clone(), gray(0x5d), Stroke::NONE));
            p.add(egui::Shape::line(pts, Stroke::new(1.0, edge)));
        };
        oflag(x_of(ls), row1, false, lc);
        oflag(x_of(le), row1, true, lc);
        oflag(x_of(clip.start), row2, false, gray(0x91));
        if !clip.looping {
            oflag(x_of(clip.start + clip.length), row2, true, gray(0x91));
        }
    } else {
        if clip.looping {
            let c = if app.cv.brace_sel { col } else { col.gamma_multiply(0.55) };
            p.rect_filled(brace, CornerRadius::same(2), c);
        } else {
            p.rect_stroke(brace, CornerRadius::same(2), Stroke::new(1.0, gray(90)), egui::StrokeKind::Inside);
        }
        let flag = |x: f32, right: bool, c: Color32| {
            let d = if right { -7.0 } else { 7.0 };
            let h = flag_y.1 - flag_y.0;
            p.add(egui::Shape::convex_polygon(
                vec![pos2(x, flag_y.0), pos2(x + d, flag_y.0 + h.min(14.0) / 2.0), pos2(x, flag_y.0 + h.min(14.0))],
                c,
                Stroke::NONE,
            ));
            p.line_segment([pos2(x + 0.5, flag_y.0), pos2(x + 0.5, flag_y.1)], Stroke::new(1.0, c));
        };
        flag(x_of(clip.start), false, gray(225));
        if !clip.looping {
            flag(x_of(clip.start + clip.length), true, gray(225));
        }
    }
    let id = ui.scope_id().with(("cv_markers", clip.id));
    let r = ui.interact(rect, id, Sense::click_and_drag());
    #[derive(Clone, Copy)]
    enum Grab {
        Start,
        End,
        LoopL,
        LoopR,
        Brace(f64),
    }
    let gid = id.with("grab");
    let near = |x: f32, b: f64| (x - x_of(b)).abs() < 5.0;
    if let Some(pt) = r.hover_pos() {
        // Live: start / loop-start show "[", end / loop-end "]".
        let left = near(pt.x, clip.start) || (clip.looping && near(pt.x, ls));
        let right = (!clip.looping && near(pt.x, clip.start + clip.length)) || (clip.looping && near(pt.x, le));
        if left {
            super::cursors::set(ui.ctx(), super::cursors::Cur::DragLeftEdge);
        } else if right {
            super::cursors::set(ui.ctx(), super::cursors::Cur::DragRightEdge);
        }
    }
    if r.drag_started()
        && let Some(pt) = r.interact_pointer_pos()
    {
        let g = if near(pt.x, clip.start) {
            Grab::Start
        } else if !clip.looping && near(pt.x, clip.start + clip.length) {
            Grab::End
        } else if near(pt.x, ls) {
            Grab::LoopL
        } else if near(pt.x, le) {
            Grab::LoopR
        } else {
            Grab::Brace(beat_at(pt.x) - ls)
        };
        let code = match g {
            Grab::Start => (0u8, 0.0),
            Grab::End => (1, 0.0),
            Grab::LoopL => (2, 0.0),
            Grab::LoopR => (3, 0.0),
            Grab::Brace(o) => (4, o),
        };
        ui.data_mut(|d| d.insert_temp(gid, code));
        app.cv.brace_sel = clip.looping;
        app.begin_gesture();
    }
    if r.dragged()
        && let Some(pt) = r.interact_pointer_pos()
        && let Some((g, off)) = ui.data(|d| d.get_temp::<(u8, f64)>(gid))
    {
        let b = snap(beat_at(pt.x));
        let min = 1.0 / 16.0;
        if let Some(c) = DawApp::clip_mut(&mut app.project, cref) {
            c.fix_loop();
            match g {
                0 => c.start = b,
                1 => c.length = (b - c.start).max(min),
                2 => c.loop_start = b.min(c.loop_end - min),
                3 => {
                    c.loop_end = b.max(c.loop_start + min);
                    if matches!(cref, ClipRef::Slot { .. }) {
                        c.length = c.loop_end - c.loop_start;
                    }
                }
                _ => {
                    let l = c.loop_end - c.loop_start;
                    c.loop_start = snap(beat_at(pt.x) - off);
                    c.loop_end = c.loop_start + l;
                }
            }
            app.needs_compile = true;
        }
    }
    if r.clicked() {
        app.cv.brace_sel = clip.looping && r.interact_pointer_pos().is_some_and(|pt| brace.x_range().contains(pt.x));
    }
    r.on_hover_text("Start flag · loop brace (drag the ends or the middle) · end flag");
}

/// Loop brace keys (brace selected): Up/Down shift by its length,
/// Ctrl+Left/Right shorten / lengthen by the grid, Ctrl+Up/Down double / halve.
pub fn brace_keys(app: &mut DawApp, ui: &egui::Ui, cref: ClipRef, grid: f64) {
    if !app.cv.brace_sel || !app.focus_detail {
        return;
    }
    use egui::{Key, Modifiers};
    let (mut resize, mut scale) = (0.0, 1.0);
    ui.input_mut(|i| {
        if i.consume_key(Modifiers::COMMAND, Key::ArrowUp) {
            scale = 2.0;
        } else if i.consume_key(Modifiers::COMMAND, Key::ArrowDown) {
            scale = 0.5;
        } else if i.consume_key(Modifiers::COMMAND, Key::ArrowRight) {
            resize = grid;
        } else if i.consume_key(Modifiers::COMMAND, Key::ArrowLeft) {
            resize = -grid;
        }
        // Plain Up/Down (shift by loop length) arrive via key_events → shift_loop.
    });
    if resize == 0.0 && scale == 1.0 {
        return;
    }
    app.edit(|p| {
        if let Some(c) = DawApp::clip_mut(p, cref) {
            c.fix_loop();
            let l = c.loop_end - c.loop_start;
            c.loop_end = c.loop_start + ((l * scale) + resize).max(1.0 / 16.0);
            if matches!(cref, ClipRef::Slot { .. }) {
                c.length = c.loop_end - c.loop_start;
            }
        }
    });
}

// ---- envelope / MPE lanes ---------------------------------------------------------------------------

/// Breakpoint editor for the selected envelope (content beats).
#[allow(clippy::too_many_arguments)]
pub fn env_lane(
    app: &mut DawApp,
    ui: &mut egui::Ui,
    rect: Rect,
    cref: ClipRef,
    clip: &Clip,
    x_of: &dyn Fn(f64) -> f32,
    beat_at: &dyn Fn(f32) -> f64,
    snap: &dyn Fn(f64) -> f64,
) {
    let audio = matches!(clip.content, ClipContent::Audio { .. });
    let list: &[EnvTarget] = if audio { &EnvTarget::AUDIO } else { &EnvTarget::MIDI };
    let t = app.cv.env_target.filter(|t| list.contains(t)).unwrap_or(list[0]);
    let (lo, hi, neutral) = t.range();
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::ZERO, gray(24));
    let y_of = |v: f32| rect.max.y - (v - lo) / (hi - lo) * rect.height();
    let v_at = |y: f32| (lo + (rect.max.y - y) / rect.height() * (hi - lo)).clamp(lo, hi);
    let empty = ClipEnvelope { target: t, ..Default::default() };
    let env = clip.envelope(t).unwrap_or(&empty);
    let col = Color32::from_rgb(230, 80, 80);
    p.line_segment([pos2(rect.min.x, y_of(neutral)), pos2(rect.max.x, y_of(neutral))], Stroke::new(1.0, Color32::from_white_alpha(18)));
    p.text(rect.left_top() + vec2(4.0, 2.0), Align2::LEFT_TOP, t.label(), super::theme::font(9.0), gray(150));
    // Unlinked: the envelope's own loop repeats along the time axis.
    let pts = &env.points;
    if pts.is_empty() {
        p.line_segment([pos2(rect.min.x, y_of(neutral)), pos2(rect.max.x, y_of(neutral))], Stroke::new(1.5, col.gamma_multiply(0.5)));
    } else {
        let line: Vec<egui::Pos2> = std::iter::once(pos2(rect.min.x, y_of(pts[0].value)))
            .chain(pts.iter().map(|q| pos2(x_of(q.beat), y_of(q.value))))
            .chain(std::iter::once(pos2(rect.max.x, y_of(pts[pts.len() - 1].value))))
            .collect();
        p.add(egui::Shape::line(line, Stroke::new(1.5, col)));
        for (i, q) in pts.iter().enumerate() {
            let c = pos2(x_of(q.beat), y_of(q.value));
            p.circle_filled(c, if app.cv.env_drag == Some(i) { 4.0 } else { 3.0 }, col);
        }
    }
    if env.unlinked && env.loop_len > 0.0 {
        let x = x_of(env.loop_len);
        p.line_segment([pos2(x, rect.min.y), pos2(x, rect.max.y)], Stroke::new(1.0, gray(140)));
    }
    let r = ui.interact(rect, ui.scope_id().with(("cv_env", clip.id)), Sense::click_and_drag());
    let hit = |pt: egui::Pos2| pts.iter().position(|q| (pos2(x_of(q.beat), y_of(q.value)) - pt).length() < 6.0);
    if r.double_clicked()
        && let Some(pt) = r.interact_pointer_pos()
        && let Some(i) = hit(pt)
    {
        app.edit(|p| {
            if let Some(c) = DawApp::clip_mut(p, cref) {
                c.envelope_mut(t).points.remove(i);
                c.envelopes.retain(|e| !e.points.is_empty() || e.unlinked);
            }
        });
        return;
    }
    if r.drag_started()
        && let Some(pt) = r.interact_pointer_pos()
    {
        app.begin_gesture();
        let i = match hit(pt) {
            Some(i) => i,
            None => {
                // Press on empty lane: new point there, then drag it.
                let b = snap(beat_at(pt.x));
                let c = DawApp::clip_mut(&mut app.project, cref).expect("clip");
                let e = c.envelope_mut(t);
                e.points.push(EnvPoint { beat: b, value: v_at(pt.y) });
                e.sort();
                e.points.iter().position(|q| q.beat == b).unwrap_or(0)
            }
        };
        app.cv.env_drag = Some(i);
        app.needs_compile = true;
    }
    if r.dragged()
        && let (Some(i), Some(pt)) = (app.cv.env_drag, r.interact_pointer_pos())
        && let Some(c) = DawApp::clip_mut(&mut app.project, cref)
    {
        let e = c.envelope_mut(t);
        if i < e.points.len() {
            let fine = ui.input(|x| x.modifiers.shift);
            let prev = if i > 0 { e.points[i - 1].beat } else { f64::NEG_INFINITY };
            let next = e.points.get(i + 1).map_or(f64::INFINITY, |q| q.beat);
            let b = if fine { beat_at(pt.x) } else { snap(beat_at(pt.x)) };
            e.points[i] = EnvPoint { beat: b.clamp(prev.min(next), next), value: v_at(pt.y) };
            app.needs_compile = true;
            app.dirty = true;
        }
    }
    if r.drag_stopped() {
        app.cv.env_drag = None;
    }
    if r.clicked()
        && let Some(pt) = r.interact_pointer_pos()
        && hit(pt).is_none()
    {
        let b = snap(beat_at(pt.x));
        app.edit(|p| {
            if let Some(c) = DawApp::clip_mut(p, cref) {
                let e = c.envelope_mut(t);
                e.points.push(EnvPoint { beat: b, value: v_at(pt.y) });
                e.sort();
            }
        });
    }
}

/// Note expression lane for the first selected note: draw to write breakpoints.
#[allow(clippy::too_many_arguments)]
pub fn mpe_lane(
    app: &mut DawApp,
    ui: &mut egui::Ui,
    rect: Rect,
    cref: ClipRef,
    clip: &Clip,
    notes: &[Note],
    x_of: &dyn Fn(f64) -> f32,
    beat_at: &dyn Fn(f32) -> f64,
) {
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::ZERO, gray(24));
    let lane = app.cv.mpe_lane;
    let (lo, hi) = if lane == 0 { (-12.0f32, 12.0f32) } else { (0.0, 1.0) };
    let y_of = |v: f32| rect.max.y - (v.clamp(lo, hi) - lo) / (hi - lo) * rect.height();
    let v_at = |y: f32| lo + (rect.max.y - y).clamp(0.0, rect.height().max(1.0)) / rect.height().max(1.0) * (hi - lo);
    p.text(
        rect.left_top() + vec2(4.0, 2.0),
        Align2::LEFT_TOP,
        ["Pitch (±12 st)", "Slide", "Pressure"][lane],
        super::theme::font(9.0),
        gray(150),
    );
    let col = Color32::from_rgb(120, 200, 255);
    // Every note's curve faintly; the selected one bright.
    let sel = app.note_sel.first().copied().filter(|&i| i < notes.len());
    for (i, n) in notes.iter().enumerate() {
        let Some(e) = clip.note_mpe(n) else { continue };
        let pts = match lane {
            0 => &e.pitch,
            1 => &e.slide,
            _ => &e.pressure,
        };
        if pts.is_empty() {
            continue;
        }
        let line: Vec<egui::Pos2> = pts.iter().map(|&(t, v)| pos2(x_of(n.start + t), y_of(v))).collect();
        p.add(egui::Shape::line(
            line,
            Stroke::new(if Some(i) == sel { 2.0 } else { 1.0 }, if Some(i) == sel { col } else { col.gamma_multiply(0.35) }),
        ));
    }
    let Some(i) = sel else {
        p.text(rect.center(), Align2::CENTER_CENTER, "Select a note to edit its expression", super::theme::font(10.0), gray(110));
        return;
    };
    let n = notes[i];
    let span = Rect::from_min_max(pos2(x_of(n.start), rect.min.y), pos2(x_of(n.start + n.length), rect.max.y));
    p.rect_filled(span, CornerRadius::ZERO, Color32::from_white_alpha(8));
    let r = ui.interact(rect, ui.scope_id().with(("cv_mpe", clip.id)), Sense::click_and_drag());
    if r.drag_started() || r.clicked() {
        app.begin_gesture();
    }
    if (r.dragged() || r.clicked())
        && let Some(pt) = r.interact_pointer_pos()
        && let Some(c) = DawApp::clip_mut(&mut app.project, cref)
        && let Some(e) = c.note_mpe_mut(i)
    {
        // Freehand: replace points within ±1 px and insert this one.
        let t = (beat_at(pt.x) - n.start).clamp(0.0, n.length);
        let pts = match lane {
            0 => &mut e.pitch,
            1 => &mut e.slide,
            _ => &mut e.pressure,
        };
        let px = (beat_at(pt.x + 2.0) - beat_at(pt.x)).abs();
        pts.retain(|q| (q.0 - t).abs() > px);
        pts.push((t, v_at(pt.y)));
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        app.needs_compile = true;
        app.dirty = true;
    }
}

// ---- sample editor ----------------------------------------------------------------------------------

/// Warped waveform (x = content beats) with marker strip, warp markers,
/// transients, an optional envelope lane and the playhead.
pub fn sample_editor(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef, clip: &Clip) {
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::ZERO, gray(0x39));
    let ClipContent::Audio { sample, warp_bpm, gain_db, .. } = &clip.content else { return };
    let Some(buf) = app.cache.peek(sample).cloned() else {
        let msg = app.cache.error(sample).map_or("Loading…".to_string(), |e| format!("Missing sample: {e}"));
        p.text(rect.center(), Align2::CENTER_CENTER, msg, super::theme::font(11.0), Color32::GRAY);
        return;
    };
    let warped = warp_bpm.is_some();
    // Hover: Live's status line "Name Start: 1.1.1 End: 1.1.4+ Length: 0.0.3+ path | 48.0 kHz | …".
    if warped && resp.hovered() {
        let bpb = app.project.beats_per_bar as f64;
        let plus = |v: f64, pos: bool| {
            let q = (v * 4.0 + 1e-6).fract() > 2e-6;
            format!("{}{}", bbs(v, bpb, pos).replace(' ', ""), if q { "+" } else { "" })
        };
        let (s0, e0) = (clip.start, clip.start + clip.length);
        let fmt = match hound::WavReader::open(sample).map(|r| r.spec()) {
            Ok(sp) => format!(" | {} Bit | {} Ch", sp.bits_per_sample, sp.channels),
            Err(_) => String::new(),
        };
        app.status_hover = Some(format!(
            "{}  Start: {}  End: {}  Length: {}  {} | {:.1} kHz{}",
            clip.name,
            plus(s0, true),
            plus(e0, true),
            plus(e0 - s0, false),
            sample,
            buf.sample_rate as f64 / 1000.0,
            fmt
        ));
    }
    let map = audio_map(app, clip);
    let total = buf.len() as f64;
    let (b_first, b_last) = (map.beat_at(0.0), map.beat_at(total));
    let (w0, w1) = play_window(clip);
    let view0 = b_first.min(w0);
    let view_len = (b_last.max(w1) - view0).max(0.25);
    let env_h = if app.cv.tab == Tab::Envelopes { (rect.height() * 0.35).max(40.0) } else { 0.0 };
    // Live 12 rows (measured): warped = bars ruler 17 | loop + marker rows 22 | sep |
    // strip 16 | sep | waveform | sep | bottom strip 17. Unwarped: no bars ruler, a
    // 15 px min:sec ruler under the waveform.
    let top = rect.min.y;
    let nums = Rect::from_min_max(rect.min, pos2(rect.max.x, top + if warped { 17.0 } else { 0.0 }));
    let marker = Rect::from_min_max(pos2(rect.min.x, nums.max.y), pos2(rect.max.x, nums.max.y + 22.0));
    let ruler = Rect::from_min_max(pos2(rect.min.x, marker.max.y + 1.0), pos2(rect.max.x, marker.max.y + 17.0));
    let bottom = Rect::from_min_max(pos2(rect.min.x, rect.max.y - 17.0), rect.max);
    let secs_r =
        Rect::from_min_max(pos2(rect.min.x, bottom.min.y - 1.0 - if warped { 0.0 } else { 15.0 }), pos2(rect.max.x, bottom.min.y - 1.0));
    let wave = Rect::from_min_max(pos2(rect.min.x, ruler.max.y + 1.0), pos2(rect.max.x, secs_r.min.y - env_h));
    let env_r = Rect::from_min_max(pos2(rect.min.x, wave.max.y), pos2(rect.max.x, secs_r.min.y));
    // Zoom / scroll: shared with the piano roll state (reset per clip).
    let seen = ui.scope_id().with("se_clip");
    if ui.data(|d| d.get_temp::<u64>(seen)) != Some(clip.id) {
        ui.data_mut(|d| d.insert_temp(seen, clip.id));
        app.pr_zoom = 0.0;
        app.pr_scroll = view0;
        app.cv.sel_marker = None;
    }
    let fit = (rect.width() as f64 / view_len).max(1e-3);
    let mut ppb = if app.pr_zoom > 0.0 { (app.pr_zoom as f64).max(fit) } else { fit };
    if resp.hovered() {
        let (zoom, scroll, hover) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta, i.pointer.hover_pos()));
        if zoom != 1.0
            && let Some(h) = hover
        {
            let b = app.pr_scroll + (h.x - rect.min.x) as f64 / ppb;
            // max(): a short sample can fit at > 2000 px/beat (clamp panics if min > max).
            ppb = (ppb * zoom as f64).clamp(fit, 2000.0f64.max(fit));
            app.pr_zoom = if ppb <= fit * 1.0001 { 0.0 } else { ppb as f32 };
            app.pr_scroll = b - (h.x - rect.min.x) as f64 / ppb;
        } else if scroll.x != 0.0 || scroll.y != 0.0 {
            app.pr_scroll -= (scroll.x + scroll.y) as f64 / ppb;
        }
    }
    let visible = rect.width() as f64 / ppb;
    if app.cv.follow
        && let Some(pp) = play_pos(app, cref)
        && (pp < app.pr_scroll || pp > app.pr_scroll + visible)
    {
        app.pr_scroll = pp;
    }
    app.pr_scroll = app.pr_scroll.clamp(view0, (view0 + view_len - visible).max(view0));
    let scroll = app.pr_scroll;
    let x_of = |b: f64| rect.min.x + ((b - scroll) * ppb) as f32;
    let beat_at = |x: f32| scroll + (x - rect.min.x) as f64 / ppb;
    let bpb = app.project.beats_per_bar as f64;
    let grid = crate::arr_ops::adaptive_step(ppb, bpb, 24.0);
    app.pr_grid_now = grid;
    let free = ui.input(|i| i.modifiers.alt);
    let snap = |b: f64| if free { b } else { (b / grid).round() * grid };
    // Strips, separators, rulers.
    let sepc = super::theme::sep();
    for y in [marker.max.y, ruler.max.y, bottom.min.y - 1.0] {
        p.rect_filled(Rect::from_min_max(pos2(rect.min.x, y), pos2(rect.max.x, y + 1.0)), CornerRadius::ZERO, sepc);
    }
    p.rect_filled(ruler, CornerRadius::ZERO, gray(0x30));
    p.rect_filled(bottom, CornerRadius::ZERO, super::theme::panel());
    p.rect_filled(wave, CornerRadius::ZERO, gray(0x3e));
    if warped {
        p.rect_filled(nums, CornerRadius::ZERO, gray(0x39));
    } else {
        // min:sec ruler under the waveform: ticks every nice time step.
        p.rect_filled(secs_r, CornerRadius::ZERO, gray(0x39));
        let sr = buf.sample_rate as f64;
        let px_per_sec = ppb * map.beat_at(sr) - ppb * map.beat_at(0.0);
        let step = [0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0]
            .into_iter()
            .find(|s| s * px_per_sec >= 70.0)
            .unwrap_or(60.0);
        let t0 = map.frame_at(scroll) / sr;
        let mut t = (t0 / step).floor() * step;
        let t_end = map.frame_at(scroll + visible) / sr;
        while t <= t_end {
            let x = x_of(map.beat_at(t * sr)).round();
            p.line_segment([pos2(x + 0.5, secs_r.min.y), pos2(x + 0.5, secs_r.min.y + 9.0)], Stroke::new(1.0, gray(0x91)));
            p.line_segment([pos2(x + 0.5, secs_r.min.y + 8.5), pos2(x + 2.5, secs_r.min.y + 8.5)], Stroke::new(1.0, gray(0x91)));
            let ms = ((t.max(0.0) * 1000.0).round() as i64) % 1000;
            let label = format!("{}:{:02}:{:03}", (t.max(0.0) / 60.0) as i64, (t.max(0.0) % 60.0) as i64, ms);
            p.text(
                pos2(x + 4.0, secs_r.center().y + 1.0),
                Align2::LEFT_CENTER,
                label,
                super::theme::font(super::theme::FS),
                super::theme::text(),
            );
            t += step;
        }
    }
    // Warped: alternating lighter bar bands.
    let mut bar = (scroll / bpb).floor();
    while warped && bar * bpb <= scroll + visible {
        if (bar as i64).rem_euclid(2) == 0 {
            let r = Rect::from_min_max(pos2(x_of(bar * bpb), wave.min.y), pos2(x_of((bar + 1.0) * bpb), wave.max.y));
            p.rect_filled(r, CornerRadius::ZERO, gray(0x3e));
        }
        bar += 1.0;
    }
    let mut b = (scroll / grid).floor() * grid;
    while b <= scroll + visible {
        let x = x_of(b);
        let bar = (b / bpb).fract().abs() < 1e-9;
        if warped {
            p.line_segment([pos2(x, wave.min.y), pos2(x, wave.max.y)], Stroke::new(1.0, gray(if bar { 0x2c } else { 0x34 })));
            if bar || b.fract().abs() < 1e-9 && grid * ppb >= 40.0 {
                let label = if bar {
                    format!("{}", (b / bpb).floor() as i64 + 1)
                } else {
                    format!("{}.{}", (b / bpb).floor() as i64 + 1, (b - (b / bpb).floor() * bpb) as i64 + 1)
                };
                p.line_segment([pos2(x + 0.5, nums.max.y - 9.0), pos2(x + 0.5, nums.max.y)], Stroke::new(1.0, gray(0x91)));
                p.line_segment([pos2(x + 0.5, nums.max.y - 8.5), pos2(x + 2.5, nums.max.y - 8.5)], Stroke::new(1.0, gray(0x91)));
                p.text(
                    pos2(x + 4.0, nums.max.y - 5.5),
                    Align2::LEFT_CENTER,
                    label,
                    super::theme::font(super::theme::FS),
                    super::theme::text(),
                );
            }
        }
        b += grid;
    }
    // Waveform (gain-scaled, cut at the editor).
    {
        let col = gray(0x1a); // Live: dark waveform on the light editor
        let body = wave.shrink2(vec2(0.0, wave.height() * 0.05));
        let fx = |x: f32| Some(map.frame_at(beat_at(x)));
        let src = super::wave::Source::of(&buf);
        super::wave::draw(&p.with_clip_rect(wave.intersect(p.clip_rect())), body, wave, &src, &fx, daw_project::db_to_gain(*gain_db), col);
    }
    // Outside the markers: dimmed.
    for (a, z) in [(f64::NEG_INFINITY, w0), (w1, f64::INFINITY)] {
        let r = Rect::from_min_max(pos2(x_of(a).max(wave.min.x), wave.min.y), pos2(x_of(z).min(wave.max.x), wave.max.y));
        if r.width() > 0.0 {
            p.rect_filled(r, CornerRadius::ZERO, Color32::from_black_alpha(120));
        }
    }
    // Transients (grey ticks) and warp markers (yellow).
    let transients: Vec<f64> = clip.audio.transients.clone().unwrap_or_else(|| buf.transients.clone());
    for &f in &transients {
        let x = x_of(map.beat_at(f));
        if wave.x_range().contains(x) {
            p.line_segment([pos2(x, wave.min.y), pos2(x, wave.min.y + 6.0)], Stroke::new(1.0, gray(150)));
        }
    }
    let warp_col = Color32::from_rgb(240, 200, 60);
    if warped {
        for (i, m) in clip.audio.markers.iter().enumerate() {
            let x = x_of(m.beat);
            let sel = app.cv.sel_marker == Some(i);
            p.line_segment([pos2(x, wave.min.y), pos2(x, wave.max.y)], Stroke::new(if sel { 2.0 } else { 1.0 }, warp_col));
            p.add(egui::Shape::convex_polygon(
                vec![pos2(x - 4.0, ruler.min.y), pos2(x + 4.0, ruler.min.y), pos2(x, ruler.max.y)],
                warp_col,
                Stroke::NONE,
            ));
        }
    }
    marker_strip(app, ui, marker, cref, clip, &x_of, &beat_at, &snap);
    if let Some(pp) = play_pos(app, cref) {
        let x = x_of(pp);
        p.line_segment([pos2(x, ruler.min.y), pos2(x, wave.max.y)], Stroke::new(1.5, Color32::WHITE));
    }
    // Warp marker interaction (ruler strip + waveform).
    if warped {
        warp_interact(app, ui, &resp, ruler, wave, cref, clip, &map, &x_of, &beat_at, &snap, &transients);
    }
    if env_h > 0.0 {
        env_lane(app, ui, env_r, cref, clip, &x_of, &beat_at, &snap);
    }
}

#[allow(clippy::too_many_arguments)]
fn warp_interact(
    app: &mut DawApp,
    ui: &mut egui::Ui,
    resp: &egui::Response,
    ruler: Rect,
    wave: Rect,
    cref: ClipRef,
    clip: &Clip,
    map: &daw_engine::WarpMap,
    x_of: &dyn Fn(f64) -> f32,
    beat_at: &dyn Fn(f32) -> f64,
    snap: &dyn Fn(f64) -> f64,
    transients: &[f64],
) {
    let markers = &clip.audio.markers;
    let hit = |x: f32| markers.iter().position(|m| (x_of(m.beat) - x).abs() < 5.0);
    let zone = Rect::from_min_max(ruler.min, pos2(ruler.max.x, wave.max.y));
    let Some(pt) = resp.interact_pointer_pos().or(resp.hover_pos()) else { return };
    if !zone.contains(pt) {
        return;
    }
    if hit(pt.x).is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let did = ui.scope_id().with(("warp_drag", clip.id));
    // Double-click: add a marker (pinned at the frame under the pointer) or delete one.
    if resp.double_clicked() {
        let at = hit(pt.x);
        let b = snap(beat_at(pt.x));
        let f = map.frame_at(beat_at(pt.x));
        app.edit(|p| {
            if let Some(c) = DawApp::clip_mut(p, cref) {
                match at {
                    Some(i) => {
                        c.audio.markers.remove(i);
                    }
                    None => {
                        if c.audio.markers.is_empty() {
                            // Anchor the current mapping first.
                            c.audio.markers.push(WarpMarker { beat: 0.0, frame: map.frame_at(0.0) });
                        }
                        c.audio.markers.push(WarpMarker { beat: beat_at(pt.x).max(b.min(beat_at(pt.x))), frame: f });
                        c.audio.markers.sort_by(|a, b| a.beat.total_cmp(&b.beat));
                    }
                }
            }
        });
        return;
    }
    if resp.drag_started() {
        let i = hit(pt.x);
        app.cv.sel_marker = i;
        ui.data_mut(|d| d.insert_temp(did, i));
        if i.is_some() {
            app.begin_gesture();
        }
    }
    if resp.dragged()
        && let Some(Some(i)) = ui.data(|d| d.get_temp::<Option<usize>>(did))
        && let Some(c) = DawApp::clip_mut(&mut app.project, cref)
        && i < c.audio.markers.len()
    {
        // Shift: move the waveform under a fixed marker (change its frame).
        if ui.input(|x| x.modifiers.shift) {
            let df = map.frame_at(beat_at(pt.x)) - map.frame_at(c.audio.markers[i].beat);
            c.audio.markers[i].frame = (c.audio.markers[i].frame - df).max(0.0);
        } else {
            let prev = if i > 0 { c.audio.markers[i - 1].beat + 1.0 / 64.0 } else { f64::NEG_INFINITY };
            let next = c.audio.markers.get(i + 1).map_or(f64::INFINITY, |m| m.beat - 1.0 / 64.0);
            c.audio.markers[i].beat = snap(beat_at(pt.x)).clamp(prev.min(next), next);
        }
        app.needs_compile = true;
        app.dirty = true;
    }
    if resp.clicked() {
        app.cv.sel_marker = hit(pt.x);
    }
    let sel = app.cv.sel_marker;
    if let Some(i) = sel
        && app.focus_detail
        && ui.input_mut(|x| x.consume_key(egui::Modifiers::NONE, egui::Key::Delete))
    {
        app.edit(|p| {
            if let Some(c) = DawApp::clip_mut(p, cref)
                && i < c.audio.markers.len()
            {
                c.audio.markers.remove(i);
            }
        });
        app.cv.sel_marker = None;
    }
    let here = beat_at(pt.x);
    let here_f = map.frame_at(here);
    let near_tr = transients.iter().copied().min_by(|a, b| (a - here_f).abs().total_cmp(&(b - here_f).abs()));
    resp.context_menu(|ui| {
        if ui.button("Set 1.1.1 Here").clicked() {
            let sr = app.audio.info.sample_rate as f64;
            app.edit(|p| {
                if let Some(c) = DawApp::clip_mut(p, cref) {
                    shift_audio(c, here, sr);
                }
            });
            ui.close();
        }
        if ui.button("Warp From Here").on_hover_text("Remove later markers; straight from here").clicked() {
            app.edit(|p| {
                if let Some(c) = DawApp::clip_mut(p, cref) {
                    c.audio.markers.retain(|m| m.beat <= here);
                    c.audio.markers.push(WarpMarker { beat: here, frame: here_f });
                }
            });
            ui.close();
        }
        let tempo = app.project.tempo_bpm;
        if ui.button(format!("Warp From Here (Start at {tempo:.0} BPM)")).clicked() {
            app.edit(|p| {
                if let Some(c) = DawApp::clip_mut(p, cref) {
                    c.audio.markers.retain(|m| m.beat <= here);
                    c.audio.markers.push(WarpMarker { beat: here, frame: here_f });
                    if let ClipContent::Audio { warp_bpm, .. } = &mut c.content {
                        *warp_bpm = Some(tempo);
                    }
                }
            });
            ui.close();
        }
        ui.menu_button("Warp Sample As", |ui| {
            for bars in [1.0, 2.0, 4.0, 8.0, 16.0] {
                if ui.button(format!("{bars} Bar Loop")).clicked() {
                    let total = map.frame_at(map.beat_at(f64::MAX.min(1e15))).max(1.0);
                    app.warp_as(cref, bars, total);
                    ui.close();
                }
            }
        });
        ui.separator();
        if let Some(i) = hit(pt.x)
            && ui.button("Delete Warp Marker").clicked()
        {
            app.edit(|p| {
                if let Some(c) = DawApp::clip_mut(p, cref) {
                    c.audio.markers.remove(i);
                }
            });
            ui.close();
        }
        if ui.button("Delete All Warp Markers").clicked() {
            app.edit(|p| {
                if let Some(c) = DawApp::clip_mut(p, cref) {
                    c.audio.markers.clear();
                }
            });
            ui.close();
        }
        if ui.button("Insert Warp Markers at Transients").on_hover_text("Ctrl+Shift+I").clicked() {
            let tr = transients.to_vec();
            app.edit(|p| {
                if let Some(c) = DawApp::clip_mut(p, cref) {
                    for f in tr {
                        let b = map.beat_at(f);
                        if !c.audio.markers.iter().any(|m| (m.frame - f).abs() < 1.0) {
                            c.audio.markers.push(WarpMarker { beat: b, frame: f });
                        }
                    }
                    c.audio.markers.sort_by(|a, b| a.beat.total_cmp(&b.beat));
                }
            });
            ui.close();
        }
        ui.separator();
        if ui.button("Insert Transient").clicked() {
            let mut tr = transients.to_vec();
            tr.push(here_f);
            tr.sort_by(f64::total_cmp);
            app.edit(|p| {
                if let Some(c) = DawApp::clip_mut(p, cref) {
                    c.audio.transients = Some(tr);
                }
            });
            ui.close();
        }
        if let Some(f) = near_tr
            && ui.button("Delete Transient").clicked()
        {
            let tr: Vec<f64> = transients.iter().copied().filter(|&x| x != f).collect();
            app.edit(|p| {
                if let Some(c) = DawApp::clip_mut(p, cref) {
                    c.audio.transients = Some(tr);
                }
            });
            ui.close();
        }
        if clip.audio.transients.is_some() && ui.button("Reset Transients").clicked() {
            app.edit(|p| {
                if let Some(c) = DawApp::clip_mut(p, cref) {
                    c.audio.transients = None;
                }
            });
            ui.close();
        }
    });
}

/// Make content beat `here` the new 1.1.1: everything pinned to the content
/// (warp markers, loop, envelopes, start) moves by `-here`; straight-warped
/// clips move their sample offset instead of markers.
pub fn shift_audio(c: &mut Clip, here: f64, sr: f64) {
    if c.audio.markers.is_empty()
        && let ClipContent::Audio { offset_frames, warp_bpm: Some(b), .. } = &mut c.content
    {
        *offset_frames = (*offset_frames + here * sr * 60.0 / *b).max(0.0);
    }
    c.fix_loop();
    c.shift_markers(here);
    c.start -= here;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bbs_roundtrip() {
        assert_eq!(bbs(0.0, 4.0, true), "1.1.1");
        assert_eq!(bbs(5.25, 4.0, true), "2.2.2");
        assert_eq!(bbs(4.0, 4.0, false), "1.0.0");
        assert_eq!(parse_bbs("2.2.2", 4.0, true), Some(5.25));
        assert_eq!(parse_bbs("1.0.0", 4.0, false), Some(4.0));
        assert_eq!(parse_bbs("8", 4.0, false), Some(32.0));
    }

    #[test]
    fn region_and_window() {
        let mut c = Clip { length: 4.0, looping: true, ..Default::default() };
        c.loop_start = 1.0;
        c.loop_end = 3.0;
        c.start = 0.5;
        assert_eq!(play_window(&c), (0.5, 3.0));
        assert_eq!(region(&c), (1.0, 3.0));
        c.looping = false;
        assert_eq!(region(&c), (0.5, 4.5));
    }
}
