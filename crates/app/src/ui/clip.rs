//! Clip View (Live 12 ch. 8–9, 11): title bar, Clip / Launch / Audio / Notes
//! (Pitch & Time, Transform, Generate) / Envelopes / MPE panels, the marker
//! strip (start / end / loop brace), envelope + MPE lanes and the warped
//! sample editor with warp markers and transients.

use super::widgets::*;
use crate::app::{ClipRef, DawApp};
use daw_project::clipx::{
    self, ClipEnvelope, EnvPoint, EnvTarget, FollowAction, LAUNCH_QUANTS, LaunchMode, NOTE_NAMES, SCALES, WarpMarker, WarpMode,
};
use daw_project::miditools::{self as mt, Generate, PKind, Transform};
use daw_project::{Clip, ClipContent, Note};
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, pos2, vec2};

pub const BOX_W: f32 = 196.0;
pub const MARKER_H: f32 = 12.0;

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
    pub show_launch: bool,
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
    pub transpose: i32,
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
    pub info_edit: Option<String>,
    /// Library groove picked in the chooser (added to the pool next frame).
    pub pending_groove: Option<clipx::Groove>,
    pub show_pool: bool,
}

impl Default for ClipView {
    fn default() -> Self {
        Self {
            tab: Tab::Main,
            panel: NotesPanel::PitchTime,
            show_launch: true,
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
            transpose: 0,
            duration: 4.0,
            humanize: 0.5,
            follow: false,
            brace_sel: false,
            quant_grid: 7.0,
            quant_amount: 100.0,
            sel_marker: None,
            env_drag: None,
            info_edit: None,
            pending_groove: None,
            show_pool: false,
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

/// Unwarped audio: positions in min:sec (beats ↔ seconds at the song tempo).
fn sec_field(ui: &mut egui::Ui, v: &mut f64, spb: f64, range: std::ops::RangeInclusive<f64>) -> egui::Response {
    ui.add(
        egui::DragValue::new(v)
            .range(range)
            .speed(0.05)
            .custom_formatter(move |x, _| {
                let t = x * spb;
                let sign = if t < 0.0 { "-" } else { "" };
                let t = t.abs();
                format!("{sign}{}:{:06.3}", (t / 60.0).floor() as i64, t % 60.0)
            })
            .custom_parser(move |s| {
                let s = s.trim();
                let (neg, s) = s.strip_prefix('-').map_or((false, s), |r| (true, r));
                let secs = match s.split_once(':') {
                    Some((m, x)) => m.trim().parse::<f64>().ok()? * 60.0 + x.trim().parse::<f64>().ok()?,
                    None => s.parse::<f64>().ok()?,
                };
                Some(if neg { -secs } else { secs } / spb)
            }),
    )
}

/// Collects gesture / change flags from widgets (one undo step per gesture).
#[derive(Default)]
struct Ed {
    gesture: bool,
    changed: bool,
}

impl Ed {
    fn t(&mut self, r: egui::Response) -> egui::Response {
        self.gesture |= r.drag_started() || r.gained_focus() || r.clicked();
        self.changed |= r.changed();
        r
    }
    fn set(&mut self) {
        self.gesture = true;
        self.changed = true;
    }
}

fn track_of(c: ClipRef) -> usize {
    match c {
        ClipRef::Slot { track, .. } | ClipRef::Arr { track, .. } => track,
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

fn commit(app: &mut DawApp, cref: ClipRef, ed: Ed, c: Clip) {
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

fn audio_map(app: &DawApp, c: &Clip) -> daw_engine::WarpMap {
    match &c.content {
        ClipContent::Audio { offset_frames, warp_bpm, .. } => {
            let sr = app.audio.info.sample_rate as f64;
            daw_core::compile::warp_map(c, *offset_frames, Some(warp_bpm.unwrap_or(app.project.tempo_bpm)), sr)
        }
        _ => daw_engine::WarpMap::linear(0.0, 1.0),
    }
}

// ---- panels -------------------------------------------------------------------------------------

/// The property panels left of the editor.
pub fn panels(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef, clip: &Clip) {
    let session = matches!(cref, ClipRef::Slot { .. });
    egui::ScrollArea::horizontal().id_salt("cv_panels").auto_shrink([true, false]).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            framed(ui, "Clip", |ui| clip_box(app, ui, cref, clip));
            if session && app.cv.show_launch {
                framed(ui, "Launch", |ui| launch_box(app, ui, cref, clip));
            }
            let audio = matches!(clip.content, ClipContent::Audio { .. });
            framed(ui, "", |ui| {
                ui.horizontal(|ui| {
                    let main = if audio { "Audio" } else { "Notes" };
                    ui.selectable_value(&mut app.cv.tab, Tab::Main, main);
                    ui.selectable_value(&mut app.cv.tab, Tab::Envelopes, "Envelopes");
                    if !audio {
                        ui.selectable_value(&mut app.cv.tab, Tab::Mpe, "MPE");
                    }
                });
                if audio && app.cv.tab == Tab::Mpe {
                    app.cv.tab = Tab::Main;
                }
                ui.separator();
                match (app.cv.tab, audio) {
                    (Tab::Main, true) => audio_box(app, ui, cref, clip),
                    (Tab::Main, false) => notes_box(app, ui, cref, clip),
                    (Tab::Envelopes, _) => envelopes_box(app, ui, cref, clip),
                    (Tab::Mpe, _) => mpe_box(app, ui, cref, clip),
                }
            });
        });
    });
}

fn framed(ui: &mut egui::Ui, title: &str, f: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(gray(34)).inner_margin(4.0).corner_radius(2.0).show(ui, |ui| {
        ui.set_width(BOX_W);
        ui.set_min_height(ui.available_height());
        // Frame inherits the row layout: force a column.
        ui.vertical(|ui| {
            egui::ScrollArea::vertical().id_salt(("cv_box", title)).auto_shrink([false, false]).show(ui, |ui| {
                if !title.is_empty() {
                    ui.label(egui::RichText::new(title).small().strong().color(gray(170)));
                }
                f(ui)
            });
        });
    });
}

fn clip_box(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef, clip: &Clip) {
    let bpb = app.project.beats_per_bar as f64;
    // Title bar: activator · name (double-click renames; double-click on the
    // bar folds the panels) · launch panel toggle. Right-click: colour / info.
    let mut c = clip.clone();
    let mut ed = Ed::default();
    ui.horizontal(|ui| {
        let r = ui.add(egui::Button::selectable(!c.muted, " ").min_size(vec2(14.0, 16.0))).on_hover_text("Clip Activator (0)");
        if r.clicked() {
            c.muted = !c.muted;
            ed.set();
        }
        let w = ui.available_width() - if matches!(cref, ClipRef::Slot { .. }) { 22.0 } else { 0.0 };
        let (r, nr) = ui.allocate_exact_size(vec2(w.max(40.0), 18.0), Sense::click());
        let renaming = app.renaming.as_ref().is_some_and(|x| x.clip && x.id == clip.id && x.in_detail);
        if renaming {
            super::rename_editor(app, ui, r, clip.id, true, true);
        } else {
            let sel = app.arr_sel.len().max(1);
            let p = ui.painter();
            p.rect_filled(r, CornerRadius::same(2), rgb(clip.color));
            if sel > 1 {
                // Several clips selected: striped bar + count.
                for k in 0..((r.width() / 8.0) as i32) {
                    let x = r.min.x + k as f32 * 8.0;
                    p.line_segment([pos2(x, r.max.y), pos2(x + 6.0, r.min.y)], Stroke::new(2.0, Color32::from_black_alpha(40)));
                }
            }
            let label = if sel > 1 { format!("{} Clips", sel) } else { clip.name.clone() };
            p.with_clip_rect(r).text(
                r.left_center() + vec2(4.0, 0.0),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(11.0),
                text_on(rgb(clip.color)),
            );
            if nr.double_clicked() {
                if ui.input(|i| i.modifiers.shift) {
                    app.cv.folded = !app.cv.folded;
                } else {
                    app.start_rename_clip(clip.id, true);
                }
            }
            let hover = if clip.info.is_empty() {
                "Double-click: rename · Shift+double-click: fold · right-click: colour, info".to_string()
            } else {
                clip.info.clone()
            };
            let nr = nr.on_hover_text(hover);
            nr.context_menu(|ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.set_max_width(160.0);
                    for col in daw_project::PALETTE {
                        let (cr, resp) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::click());
                        ui.painter().rect_filled(cr, CornerRadius::same(2), rgb(col));
                        if resp.clicked() {
                            c.color = col;
                            ed.set();
                            ui.close();
                        }
                    }
                });
                if ui.button("Assign Track Color to Clip").clicked() {
                    c.color = app.project.tracks[track_of(cref)].color;
                    ed.set();
                    ui.close();
                }
                if ui.button("Rename (Ctrl+R)").clicked() {
                    app.start_rename_clip(clip.id, true);
                    ui.close();
                }
                ui.separator();
                ui.label("Info Text");
                let txt = app.cv.info_edit.get_or_insert_with(|| clip.info.clone());
                let r = ui.add(egui::TextEdit::multiline(txt).desired_rows(2).desired_width(160.0));
                if r.lost_focus() || ui.button("Set Info").clicked() {
                    c.info = app.cv.info_edit.take().unwrap_or_default();
                    ed.set();
                }
            });
        }
        if matches!(cref, ClipRef::Slot { .. }) {
            let r = ui
                .add(egui::Button::selectable(app.cv.show_launch, "L").min_size(vec2(18.0, 16.0)))
                .on_hover_text("Show / hide the Launch panel");
            if r.clicked() {
                app.cv.show_launch = !app.cv.show_launch;
            }
        }
    });
    let pos = play_pos(app, cref);
    let set_btn = |ui: &mut egui::Ui| {
        ui.add_enabled(pos.is_some(), egui::Button::new("Set").small()).on_hover_text("Set to the play position").clicked()
    };
    egui::Grid::new("cv_clip").num_columns(3).spacing([4.0, 3.0]).show(ui, |ui| {
        // Signature (display / grid only).
        ui.label("Signature");
        ui.horizontal(|ui| {
            ed.t(ui.add(egui::DragValue::new(&mut c.sig.0).range(1..=99)));
            ui.label("/");
            ed.t(ui.add(egui::DragValue::new(&mut c.sig.1).range(1..=16)));
        });
        ui.end_row();
        // Groove.
        ui.label("Groove");
        let gname = c.groove.and_then(|id| app.project.grooves.iter().find(|g| g.id == id)).map_or("None".to_string(), |g| g.name.clone());
        egui::ComboBox::from_id_salt("cv_groove").width(96.0).selected_text(gname).show_ui(ui, |ui| {
            if ui.selectable_label(c.groove.is_none(), "None").clicked() {
                c.groove = None;
                ed.set();
            }
            for g in &app.project.grooves {
                if ui.selectable_label(c.groove == Some(g.id), &g.name).clicked() {
                    c.groove = Some(g.id);
                    ed.set();
                }
            }
            ui.separator();
            ui.label(egui::RichText::new("Library").small());
            for g in daw_project::clipx::Groove::factory() {
                if ui.button(&g.name).clicked() {
                    app.cv.pending_groove = Some(g);
                }
            }
        });
        if ui
            .add_enabled(c.groove.is_some() && matches!(c.content, ClipContent::Midi { .. }), egui::Button::new("Commit").small())
            .on_hover_text("Write the groove into the notes")
            .clicked()
        {
            app.commit_groove(cref);
        }
        if ui.add(egui::Button::selectable(app.cv.show_pool, "Pool").small()).on_hover_text("Groove Pool").clicked() {
            app.cv.show_pool = !app.cv.show_pool;
        }
        ui.end_row();
        // Scale.
        ui.label("Scale");
        ui.horizontal(|ui| {
            if ui.add(egui::Button::selectable(c.scale.on, "#").min_size(vec2(18.0, 16.0))).on_hover_text("Scale Mode").clicked() {
                c.scale.on = !c.scale.on;
                ed.set();
            }
            egui::ComboBox::from_id_salt("cv_root").width(34.0).selected_text(NOTE_NAMES[c.scale.root as usize % 12]).show_ui(ui, |ui| {
                for (i, n) in NOTE_NAMES.iter().enumerate() {
                    ed.t(ui.selectable_value(&mut c.scale.root, i as u8, *n));
                }
            });
        });
        egui::ComboBox::from_id_salt("cv_scale").width(70.0).selected_text(c.scale.name()).show_ui(ui, |ui| {
            for (i, (n, _)) in SCALES.iter().enumerate() {
                ed.t(ui.selectable_value(&mut c.scale.scale, i as u8, *n));
            }
        });
        ui.end_row();
        // Start / End.
        ui.label("Start");
        let spb = 60.0 / app.project.tempo_bpm.max(1.0);
        let secs = warp_off(&c);
        let field = move |ui: &mut egui::Ui, v: &mut f64, pos: bool, r: std::ops::RangeInclusive<f64>| {
            if secs { sec_field(ui, v, spb, r) } else { beat_field(ui, v, bpb, pos, r) }
        };
        ed.t(field(ui, &mut c.start, true, -4096.0..=4096.0));
        if set_btn(ui) {
            c.start = pos.unwrap_or(c.start);
            ed.set();
        }
        ui.end_row();
        if !c.looping {
            ui.label("End");
            let mut end = c.start + c.length;
            let r = ed.t(field(ui, &mut end, true, (c.start + 1.0 / 16.0)..=(c.start + 4096.0)));
            if r.changed() {
                c.length = end - c.start;
            }
            if set_btn(ui) && pos.unwrap_or(0.0) > c.start {
                c.length = pos.unwrap_or(0.0) - c.start;
                ed.set();
            }
            ui.end_row();
        }
        // Loop.
        ui.label("Loop");
        let audio_unwarped = matches!(c.content, ClipContent::Audio { warp_bpm: None, .. });
        let r =
            ui.add_enabled(!audio_unwarped, egui::Button::selectable(c.looping, "Loop")).on_hover_text("Loop switch (audio needs Warp)");
        if r.clicked() {
            c.fix_loop();
            c.looping = !c.looping;
            ed.set();
        }
        ui.end_row();
        if c.looping {
            let (ls, le) = c.loop_range();
            let (mut pos_v, mut len_v) = (ls, le - ls);
            ui.label("Position");
            let r = ed.t(beat_field(ui, &mut pos_v, bpb, true, -4096.0..=4096.0));
            if r.changed() {
                (c.loop_start, c.loop_end) = (pos_v, pos_v + len_v);
            }
            if set_btn(ui) {
                let p0 = pos.unwrap_or(ls);
                (c.loop_start, c.loop_end) = (p0, p0 + len_v);
                ed.set();
            }
            ui.end_row();
            ui.label("Length");
            let r = ed.t(beat_field(ui, &mut len_v, bpb, false, (1.0 / 16.0)..=4096.0));
            if r.changed() {
                (c.loop_start, c.loop_end) = (pos_v, pos_v + len_v);
                if matches!(cref, ClipRef::Slot { .. }) {
                    c.length = len_v;
                }
            }
            if set_btn(ui) && pos.unwrap_or(0.0) > ls {
                c.loop_end = pos.unwrap_or(le);
                c.loop_start = ls;
                ed.set();
            }
            ui.end_row();
        }
    });
    if c.looping
        && matches!(c.content, ClipContent::Midi { .. })
        && ui.small_button("Duplicate Loop").on_hover_text("Double the loop, copying its notes").clicked()
    {
        app.duplicate_loop(cref);
    }
    if let Some(mut g) = app.cv.pending_groove.take() {
        // Library groove → pool (reusing an entry with the same name) → clip.
        let id = match app.project.grooves.iter().find(|x| x.name == g.name) {
            Some(x) => x.id,
            None => {
                app.begin_gesture();
                g.id = app.project.grooves.iter().map(|x| x.id).max().unwrap_or(0) + 1;
                let id = g.id;
                app.project.grooves.push(g);
                id
            }
        };
        c.groove = Some(id);
        ed.changed = true;
    }
    let crop = ui.small_button("Crop").on_hover_text("Delete everything outside the loop / markers (Ctrl+Shift+J)").clicked();
    commit(app, cref, ed, c);
    if crop {
        app.crop_clip(cref);
    }
}

fn launch_box(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef, clip: &Clip) {
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

fn audio_box(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef, clip: &Clip) {
    let mut c = clip.clone();
    let mut ed = Ed::default();
    let arr = matches!(cref, ClipRef::Arr { .. });
    let ClipContent::Audio { sample, warp_bpm, .. } = &mut c.content else { return };
    let sample = sample.clone();
    ui.horizontal(|ui| {
        let mut warp = warp_bpm.is_some();
        if ui.add(egui::Button::selectable(warp, "WARP")).on_hover_text("Warp: follow the song tempo").clicked() {
            warp = !warp;
            *warp_bpm = if warp {
                let guess = app.cache.peek(&sample).map(|b| crate::app::loop_guess(b, app.project.tempo_bpm).1);
                Some(guess.flatten().unwrap_or(app.project.tempo_bpm))
            } else {
                None
            };
            if !warp {
                c.looping = false;
            }
            ed.set();
        }
        if arr
            && ui
                .add(egui::Button::selectable(c.audio.lead, if c.audio.lead { "Leader" } else { "Follower" }))
                .on_hover_text("Leader clips set the song tempo (bottom-most wins)")
                .clicked()
        {
            c.audio.lead = !c.audio.lead;
            ed.set();
        }
    });
    let ClipContent::Audio { warp_bpm, .. } = &mut c.content else { return };
    if let Some(bpm) = warp_bpm.as_mut() {
        ui.horizontal(|ui| {
            ui.label("Seg. BPM");
            let old = *bpm;
            let r = ed.t(ui.add(egui::DragValue::new(bpm).range(20.0..=999.0).speed(0.1).fixed_decimals(2)));
            let mut f = if r.changed() { old / *bpm } else { 1.0 };
            if ui.small_button("×2").clicked() {
                *bpm = (*bpm * 2.0).min(999.0);
                f = old / *bpm;
                ed.set();
            }
            if ui.small_button("÷2").clicked() {
                *bpm = (*bpm / 2.0).max(20.0);
                f = old / *bpm;
                ed.set();
            }
            // Markers keep their frames; their beats scale with the tempo.
            if f != 1.0 {
                c.audio.markers.iter_mut().for_each(|m| m.beat *= f);
            }
        });
        egui::Grid::new("cv_warp").num_columns(2).spacing([4.0, 3.0]).show(ui, |ui| {
            ui.label("Mode");
            egui::ComboBox::from_id_salt("cv_wmode").width(100.0).selected_text(c.audio.mode.label()).show_ui(ui, |ui| {
                for m in WarpMode::ALL {
                    ed.t(ui.selectable_value(&mut c.audio.mode, m, m.label()));
                }
            });
            ui.end_row();
            let wp = &mut c.audio.params;
            match c.audio.mode {
                WarpMode::Beats => {
                    ui.label("Preserve");
                    egui::ComboBox::from_id_salt("cv_pres").width(100.0).selected_text(wp.preserve.label()).show_ui(ui, |ui| {
                        for p in clipx::Preserve::ALL {
                            ed.t(ui.selectable_value(&mut wp.preserve, p, p.label()));
                        }
                    });
                    ui.end_row();
                    ui.label("Loop");
                    ui.horizontal(|ui| {
                        for (m, l, tip) in [
                            (clipx::TransientLoop::Off, "Off", "Loop Off"),
                            (clipx::TransientLoop::Forward, "Fwd", "Loop Forward"),
                            (clipx::TransientLoop::BackAndForth, "B-F", "Loop Back-and-Forth"),
                        ] {
                            ed.t(ui.selectable_value(&mut wp.loop_mode, m, l).on_hover_text(tip));
                        }
                    });
                    ui.end_row();
                    ui.label("Envelope");
                    ed.t(ui.add(egui::DragValue::new(&mut wp.envelope).range(0.0..=100.0)));
                    ui.end_row();
                }
                WarpMode::Tones => {
                    ui.label("Grain Size");
                    ed.t(ui.add(egui::DragValue::new(&mut wp.grain_tones).range(12.0..=100.0)));
                    ui.end_row();
                }
                WarpMode::Texture => {
                    ui.label("Grain Size");
                    ed.t(ui.add(egui::DragValue::new(&mut wp.grain_texture).range(12.0..=100.0)));
                    ui.end_row();
                    ui.label("Flux");
                    ed.t(ui.add(egui::DragValue::new(&mut wp.flux).range(0.0..=100.0)));
                    ui.end_row();
                }
                WarpMode::ComplexPro => {
                    ui.label("Formants");
                    ed.t(ui.add(egui::DragValue::new(&mut wp.formants).range(0.0..=100.0).suffix(" %")));
                    ui.end_row();
                    ui.label("Envelope");
                    ed.t(ui.add(egui::DragValue::new(&mut wp.cpro_envelope).range(8.0..=256.0)));
                    ui.end_row();
                }
                WarpMode::RePitch | WarpMode::Complex => {}
            }
        });
    }
    ui.separator();
    let mut gain = match &c.content {
        ClipContent::Audio { gain_db, .. } => *gain_db,
        _ => 0.0,
    };
    let unwarped = warp_off(&c);
    egui::Grid::new("cv_audio").num_columns(2).spacing([4.0, 3.0]).show(ui, |ui| {
        ui.label("Gain");
        ed.t(ui.add(egui::DragValue::new(&mut gain).range(-60.0..=24.0).speed(0.1).suffix(" dB")));
        ui.end_row();
        ui.label("Transpose");
        ui.add_enabled_ui(c.audio.mode != WarpMode::RePitch || unwarped, |ui| {
            ed.t(ui.add(egui::DragValue::new(&mut c.audio.transpose).range(-48..=48).suffix(" st")));
        });
        ui.end_row();
        ui.label("Detune");
        ed.t(ui.add(egui::DragValue::new(&mut c.audio.detune).range(-50.0..=50.0).suffix(" ct")));
        ui.end_row();
    });
    if let ClipContent::Audio { gain_db, .. } = &mut c.content {
        *gain_db = gain;
    }
    ui.horizontal_wrapped(|ui| {
        for (v, l, tip) in [(0usize, "Fade", "Clip Fade (declick)"), (1, "RAM", "Load into RAM"), (2, "HiQ", "High-quality interpolation")]
        {
            let on = [c.audio.fade, c.audio.ram, c.audio.hiq][v];
            if ui.add(egui::Button::selectable(on, l)).on_hover_text(tip).clicked() {
                match v {
                    0 => c.audio.fade = !on,
                    1 => c.audio.ram = !on,
                    _ => c.audio.hiq = !on,
                }
                ed.set();
            }
        }
    });
    let reverse = ui.horizontal(|ui| {
        let rev = ui.button("Reverse").on_hover_text("Reverse the sample (new file)").clicked();
        if ui.button("Edit").on_hover_text("Open in the external sample editor").clicked() {
            open_external(&sample);
        }
        rev
    });
    ui.separator();
    ui.label(egui::RichText::new("Transform").small().strong());
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("cv_qgrid").width(52.0).selected_text(mt::GRIDS[app.cv.quant_grid as usize].0).show_ui(ui, |ui| {
            for (i, (l, _)) in mt::GRIDS.iter().enumerate() {
                ui.selectable_value(&mut app.cv.quant_grid, i as f32, *l);
            }
        });
        ui.add(egui::DragValue::new(&mut app.cv.quant_amount).range(0.0..=100.0).suffix(" %"));
    });
    let quant = ui.button("Quantize Markers").on_hover_text("Move warp markers to transients on the grid (Ctrl+U)").clicked();
    commit(app, cref, ed, c);
    if reverse.inner {
        app.reverse_clip(cref);
    }
    if quant {
        app.quantize_warp(cref, mt::grid_beats(app.cv.quant_grid), app.cv.quant_amount as f64 / 100.0);
    }
}

fn warp_off(c: &Clip) -> bool {
    matches!(c.content, ClipContent::Audio { warp_bpm: None, .. })
}

fn open_external(path: &str) {
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

fn notes_box(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef, clip: &Clip) {
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(&mut app.cv.panel, NotesPanel::PitchTime, "Pitch & Time");
        ui.selectable_value(&mut app.cv.panel, NotesPanel::Transform, "Transform");
        ui.selectable_value(&mut app.cv.panel, NotesPanel::Generate, "Generate");
    });
    match app.cv.panel {
        NotesPanel::PitchTime => pitch_time(app, ui, cref, clip),
        NotesPanel::Transform | NotesPanel::Generate => tools(app, ui, cref, clip),
    }
}

/// Pitch & Time utilities: act on the selected notes, else the whole clip.
fn pitch_time(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef, clip: &Clip) {
    let scale = clip.scale.on.then_some(clip.scale);
    type NoteOp = Box<dyn FnOnce(&mut Vec<Note>, &[usize])>;
    let mut op: Option<NoteOp> = None;
    ui.horizontal(|ui| {
        ui.label(if scale.is_some() { "Transpose (deg)" } else { "Transpose" });
        ui.add(egui::DragValue::new(&mut app.cv.transpose).range(-48..=48));
        let n = app.cv.transpose;
        if ui.small_button("Apply").clicked() && n != 0 {
            op = Some(Box::new(move |v, s| with_sel(v, s, |x| mt::transpose(x, n, scale))));
        }
    });
    ui.horizontal(|ui| {
        if ui.small_button("−").on_hover_text("Down one step").clicked() {
            op = Some(Box::new(move |v, s| with_sel(v, s, |x| mt::transpose(x, -1, scale))));
        }
        if ui.small_button("+").on_hover_text("Up one step").clicked() {
            op = Some(Box::new(move |v, s| with_sel(v, s, |x| mt::transpose(x, 1, scale))));
        }
        if ui.add_enabled(scale.is_some(), egui::Button::new("Fit to Scale").small()).clicked() {
            let s = clip.scale;
            op = Some(Box::new(move |v, sel| with_sel(v, sel, |x| mt::fit_to_scale(x, s))));
        }
        if ui.small_button("Invert").clicked() {
            op = Some(Box::new(move |v, s| with_sel(v, s, |x| mt::invert(x, scale))));
        }
    });
    ui.horizontal(|ui| {
        ui.label("Interval");
        ui.add(egui::DragValue::new(&mut app.cv.interval).range(-48..=48));
        let n = app.cv.interval;
        if ui.small_button("Add Interval").clicked() {
            op = Some(Box::new(move |v, s| {
                let s: Vec<usize> = if s.is_empty() { (0..v.len()).collect() } else { s.to_vec() };
                mt::add_interval(v, &s, n, scale);
            }));
        }
    });
    ui.horizontal(|ui| {
        ui.label("Stretch");
        if ui.small_button("×2").clicked() {
            op = Some(Box::new(|v, s| with_sel(v, s, |x| stretch_sel(x, 2.0))));
        }
        if ui.small_button("÷2").clicked() {
            op = Some(Box::new(|v, s| with_sel(v, s, |x| stretch_sel(x, 0.5))));
        }
    });
    ui.horizontal(|ui| {
        ui.label("Duration");
        let bpb = app.project.beats_per_bar as f64;
        beat_field(ui, &mut app.cv.duration, bpb, false, (1.0 / 16.0)..=1024.0);
        let d = app.cv.duration;
        if ui.small_button("Set Length").clicked() {
            op = Some(Box::new(move |v, s| with_sel(v, s, |x| mt::set_length(x, d))));
        }
    });
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut app.cv.humanize).range(0.0..=1.0).speed(0.01));
        let a = app.cv.humanize;
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64);
        if ui.small_button("Humanize").clicked() {
            op = Some(Box::new(move |v, s| with_sel(v, s, |x| mt::humanize(x, a, seed))));
        }
    });
    ui.horizontal(|ui| {
        if ui.small_button("Reverse").clicked() {
            op = Some(Box::new(|v, s| with_sel(v, s, mt::reverse)));
        }
        if ui.small_button("Legato").clicked() {
            op = Some(Box::new(|v, s| with_sel(v, s, mt::legato)));
        }
    });
    if let Some(f) = op {
        let sel = app.note_sel.clone();
        app.cv.base = None;
        app.edit(|p| {
            if let Some(c) = DawApp::clip_mut(p, cref)
                && let ClipContent::Midi { notes } = &mut c.content
            {
                f(notes, &sel);
                for n in notes.iter_mut() {
                    n.start = n.start.max(-c.head);
                }
            }
        });
        if !sel.is_empty() {
            // Selection stays on the same notes (indices unchanged; additions appended).
        } else {
            app.note_sel.clear();
        }
    }
}

fn stretch_sel(x: &mut [Note], f: f64) {
    let a = x.iter().map(|n| n.start).fold(f64::INFINITY, f64::min);
    if a.is_finite() {
        mt::stretch(x, a, f);
    }
}

/// Run `f` on the selected notes (or all), writing them back in place.
fn with_sel(v: &mut [Note], sel: &[usize], f: impl FnOnce(&mut [Note])) {
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
fn tools(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef, clip: &Clip) {
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

fn envelopes_box(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef, clip: &Clip) {
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

fn mpe_box(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef, clip: &Clip) {
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
    let brace = Rect::from_min_max(pos2(x_of(ls), rect.min.y + 2.0), pos2(x_of(le), rect.max.y - 1.0));
    if clip.looping {
        let c = if app.cv.brace_sel { col } else { col.gamma_multiply(0.55) };
        p.rect_filled(brace, CornerRadius::same(2), c);
    } else {
        p.rect_stroke(brace, CornerRadius::same(2), Stroke::new(1.0, gray(90)), egui::StrokeKind::Inside);
    }
    let flag = |x: f32, right: bool, c: Color32| {
        let d = if right { -7.0 } else { 7.0 };
        p.add(egui::Shape::convex_polygon(vec![pos2(x, rect.min.y), pos2(x + d, rect.min.y + 5.0), pos2(x, rect.max.y)], c, Stroke::NONE));
    };
    flag(x_of(clip.start), false, gray(225));
    if !clip.looping {
        flag(x_of(clip.start + clip.length), true, gray(225));
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
        let resize = near(pt.x, clip.start)
            || (!clip.looping && near(pt.x, clip.start + clip.length))
            || (clip.looping && (near(pt.x, ls) || near(pt.x, le)));
        if resize {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
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
    p.text(rect.left_top() + vec2(4.0, 2.0), Align2::LEFT_TOP, t.label(), FontId::proportional(9.0), gray(150));
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
            e.points[i] = EnvPoint { beat: b.clamp(prev, next), value: v_at(pt.y) };
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
    let v_at = |y: f32| lo + (rect.max.y - y).clamp(0.0, rect.height()) / rect.height() * (hi - lo);
    p.text(
        rect.left_top() + vec2(4.0, 2.0),
        Align2::LEFT_TOP,
        ["Pitch (±12 st)", "Slide", "Pressure"][lane],
        FontId::proportional(9.0),
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
        p.text(rect.center(), Align2::CENTER_CENTER, "Select a note to edit its expression", FontId::proportional(10.0), gray(110));
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
    let ClipContent::Audio { sample, warp_bpm, .. } = &clip.content else { return };
    let Some(buf) = app.cache.peek(sample).cloned() else {
        let msg = app.cache.error(sample).map_or("Loading…".to_string(), |e| format!("Missing sample: {e}"));
        p.text(rect.center(), Align2::CENTER_CENTER, msg, FontId::proportional(11.0), Color32::GRAY);
        return;
    };
    let warped = warp_bpm.is_some();
    let map = audio_map(app, clip);
    let total = buf.len() as f64;
    let (b_first, b_last) = (map.beat_at(0.0), map.beat_at(total));
    let (w0, w1) = play_window(clip);
    let view0 = b_first.min(w0);
    let view_len = (b_last.max(w1) - view0).max(0.25);
    let env_h = if app.cv.tab == Tab::Envelopes { (rect.height() * 0.35).max(40.0) } else { 0.0 };
    let marker = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.min.y + MARKER_H));
    let ruler = Rect::from_min_max(pos2(rect.min.x, marker.max.y), pos2(rect.max.x, marker.max.y + 14.0));
    let wave = Rect::from_min_max(pos2(rect.min.x, ruler.max.y), pos2(rect.max.x, rect.max.y - env_h));
    let env_r = Rect::from_min_max(pos2(rect.min.x, wave.max.y), rect.max);
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
            ppb = (ppb * zoom as f64).clamp(fit, 2000.0);
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
    let free = ui.input(|i| i.modifiers.alt);
    let snap = |b: f64| if free { b } else { (b / grid).round() * grid };
    // Ruler + grid lines.
    p.rect_filled(ruler, CornerRadius::ZERO, super::theme::panel());
    // Live: alternating lighter bar bands.
    let mut bar = (scroll / bpb).floor();
    while bar * bpb <= scroll + visible {
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
        p.line_segment([pos2(x, wave.min.y), pos2(x, wave.max.y)], Stroke::new(1.0, gray(if bar { 0x2c } else { 0x34 })));
        if bar {
            let label = if warped {
                format!("{}", (b / bpb).floor() as i64 + 1)
            } else {
                format!("{:.2}s", map.frame_at(b) / buf.sample_rate as f64)
            };
            p.text(pos2(x + 2.0, ruler.min.y + 1.0), Align2::LEFT_TOP, label, FontId::proportional(8.0), gray(170));
        }
        b += grid;
    }
    // Waveform: per column, min/max of the peak blocks its beat span covers.
    let col = gray(0x1a); // Live: dark waveform on the light editor
    let mid = wave.center().y;
    let half = wave.height() * 0.45;
    let pk = &buf.peaks;
    let mut x = wave.min.x;
    while x < wave.max.x {
        let (f0, f1) = (map.frame_at(beat_at(x)), map.frame_at(beat_at(x + 1.0)));
        if f1 > 0.0 && f0 < total {
            let block = daw_engine::PEAK_BLOCK as f64;
            let (mut lo, mut hi) = (0.0f32, 0.0f32);
            if f1 - f0 < block * 2.0 {
                // Zoomed in: raw samples (peak blocks would draw as steps).
                let a = (f0.max(0.0) as usize).min(buf.len() - 1);
                let b = (f1.min(total).ceil() as usize).clamp(a + 1, buf.len());
                for &v in &buf.left[a..b] {
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            } else {
                let i0 = (f0.max(0.0) / block) as usize;
                let i1 = ((f1.min(total) / block) as usize).max(i0 + 1).min(pk.len());
                let step = ((i1 - i0) / 64).max(1);
                let mut i = i0;
                while i < i1 {
                    lo = lo.min(pk[i].0);
                    hi = hi.max(pk[i].1);
                    i += step;
                }
            }
            p.line_segment([pos2(x, mid - hi * half), pos2(x, mid - lo * half)], Stroke::new(1.0, col));
        }
        x += 1.0;
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
    p.text(
        pos2(rect.max.x - 4.0, ruler.min.y + 1.0),
        Align2::RIGHT_TOP,
        format!("{} · {} Hz · {:.2} s{}", buf.name, buf.sample_rate, buf.duration_secs(), if warped { "" } else { " · unwarped" }),
        FontId::proportional(8.0),
        gray(150),
    );
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
            c.audio.markers[i].beat = snap(beat_at(pt.x)).clamp(prev, next);
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
