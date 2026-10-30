//! Live 12 Clip View chrome, measured from hh-clip-voew=warp-deactivated.png,
//! hh-clip-view-warp-activated.png and hh-piano-roll-note-selected.png (1.0 scale):
//! the 170 px left panel (title bar, foldable Clip / Launch / Audio / Pitch & Time /
//! Transform / Generate sections, every body 189 px) and the editor header
//! (sample info or search / Fold, centred tabs, grid chooser).

use super::clip::{Ed, Tab, bbs, play_pos};
use super::menu;
use super::theme::*;
use super::widgets::rgb;
use crate::app::{ClipRef, DawApp};
use daw_project::clipx::{self, NOTE_NAMES, SCALES, WarpMode};
use daw_project::miditools as mt;
use daw_project::{Clip, ClipContent, Note};
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Response, Sense, Stroke, pos2, vec2};

/// Left panel width (a 2 px separator follows).
pub const LEFT_W: f32 = 170.0;
/// Editor header strip height (a 1 px separator follows).
pub const HEAD_H: f32 = 17.0;
const SEC_H: f32 = 17.0;
const BODY_H: f32 = 189.0;
const TEAL: Color32 = Color32::from_rgb(0x00, 0x73, 0x83);

/// Foldable sections (index into `ClipView::open`).
#[derive(Clone, Copy)]
pub enum Sec {
    Clip = 0,
    Launch = 1,
    Audio = 2,
    PitchTime = 3,
    Transform = 4,
    Generate = 5,
}

pub const OPEN_DEFAULT: [bool; 6] = [true, false, true, true, false, false];

// ---- painter kit -------------------------------------------------------------------------------

fn label(p: &egui::Painter, pos: Pos2, s: &str, col: Color32) {
    p.text(pos, Align2::LEFT_CENTER, s, font(FS), col);
}

/// 13 px value box: #11 border, `fill` inside.
fn boxed(p: &egui::Painter, r: Rect, fill: Color32) {
    p.rect_filled(r, 0.0, gray(0x11));
    p.rect_filled(r.shrink(1.0), 0.0, fill);
}

/// Three right-aligned parts ("1.  1.  1", "0:  0.377") at Live's columns.
fn parts(p: &egui::Painter, r: Rect, v: &[String; 3], col: Color32) {
    let k = r.width() / 73.0;
    for (i, s) in v.iter().enumerate() {
        let x = r.left() + [26.0, 47.0, 71.0][i] * k;
        p.text(pos2(x, r.center().y), Align2::RIGHT_CENTER, s, font(FS), col);
    }
}

fn bbs_parts(v: f64, bpb: f64, pos: bool) -> [String; 3] {
    let s = bbs(v, bpb, pos);
    let mut it = s.split('.');
    let (a, b, c) = (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or(""));
    [format!("{a}."), format!("{b}."), c.to_string()]
}

fn sec_parts(t: f64) -> [String; 3] {
    let neg = t < 0.0;
    let t = t.abs() + 1e-7;
    let ms = ((t.fract()) * 1000.0).floor() as i64;
    let m = format!("{}{}:", if neg { "-" } else { "" }, (t / 60.0).floor() as i64);
    let sec = (t % 60.0).floor() as i64;
    // Live: "0: 0. 0" at whole seconds, "0: 0.377" otherwise (seconds + padded ms).
    if ms == 0 { [m, format!("{sec}."), "0".into()] } else { [m, String::new(), format!("{sec}.{ms:03}")] }
}

/// Rounded pill button (#11 outline, #1e fill).
fn pill(ui: &mut egui::Ui, r: Rect, id: impl std::hash::Hash + std::fmt::Debug, txt: &str, enabled: bool) -> Response {
    let resp = ui.interact(r, ui.make_persistent_id(id), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let rad = r.height() / 2.0;
    p.rect_filled(r, rad, gray(0x11));
    p.rect_filled(r.shrink(1.0), rad - 1.0, if enabled && resp.is_pointer_button_down_on() { gray(0x46) } else { gray(0x1e) });
    p.text(r.center(), Align2::CENTER_CENTER, txt, font(FS), if enabled { text_col() } else { gray(0x69) });
    resp
}

fn text_col() -> Color32 {
    text()
}

/// Square toggle: orange (or `on_col`) when on; dimmed when disabled.
fn toggle(
    ui: &mut egui::Ui,
    r: Rect,
    id: impl std::hash::Hash + std::fmt::Debug,
    txt: &str,
    on: bool,
    enabled: bool,
    on_col: Color32,
) -> Response {
    let resp = ui.interact(r, ui.make_persistent_id(id), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let fill = match (on, enabled) {
        (true, true) => on_col,
        (true, false) => gray(0x75),
        _ => gray(0x1e),
    };
    boxed(p, r, fill);
    let col = if on {
        gray(0x12)
    } else if enabled {
        text()
    } else {
        gray(0x5a)
    };
    p.text(r.center(), Align2::CENTER_CENTER, txt, font(FS), col);
    resp
}

/// Chooser box (#11 border, panel fill, ▼ at the right). Click opens `menu`.
fn chooser(
    ui: &mut egui::Ui,
    r: Rect,
    id: impl std::hash::Hash + std::fmt::Debug,
    txt: &str,
    enabled: bool,
    menu_w: f32,
    add: impl FnOnce(&mut egui::Ui),
) {
    let resp = ui.interact(r, ui.make_persistent_id(id), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    boxed(p, r, panel());
    let col = if enabled { text() } else { gray(0x5a) };
    p.with_clip_rect(r.shrink(1.0)).text(pos2(r.left() + 4.0, r.center().y), Align2::LEFT_CENTER, txt, font(FS), col);
    let a = pos2(r.right() - 7.0, r.center().y + 1.0);
    p.add(egui::Shape::convex_polygon(vec![a + vec2(-3.5, -2.5), a + vec2(3.5, -2.5), a + vec2(0.0, 2.0)], col, Stroke::NONE));
    if enabled {
        menu::dropdown(&resp, menu_w, add);
    }
}

/// Drag-to-change value box; double-click types a value (`parse`). Returns the
/// response and the new value when it changed.
#[allow(clippy::too_many_arguments)]
fn value_box(
    ui: &mut egui::Ui,
    r: Rect,
    id: impl std::hash::Hash + std::fmt::Debug,
    fill: Color32,
    shown: Field,
    step_px: f64,
    edit_text: String,
    parse: impl Fn(&str) -> Option<f64>,
) -> (Response, Option<f64>) {
    let id = ui.make_persistent_id(id);
    let resp = ui.interact(r, id, Sense::click_and_drag());
    let p = ui.painter().clone();
    boxed(&p, r, fill);
    let edit_id = id.with("edit");
    let mut out = None;
    let editing: Option<String> = ui.data(|d| d.get_temp(edit_id));
    if let Some(mut s) = editing {
        let te = egui::TextEdit::singleline(&mut s).frame(egui::Frame::NONE).font(font(FS)).align(Align2::CENTER_CENTER);
        let tr = ui.put(r.shrink2(vec2(2.0, 0.0)), te);
        tr.request_focus();
        if tr.lost_focus() {
            ui.data_mut(|d| d.remove::<String>(edit_id));
            if !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                out = parse(&s);
            }
        } else {
            ui.data_mut(|d| d.insert_temp(edit_id, s));
        }
        return (resp, out);
    }
    match &shown {
        Field::Parts(v) => parts(&p, r, v, text()),
        Field::Text(s) => {
            p.text(r.center(), Align2::CENTER_CENTER, s, font(FS), text());
        }
    }
    if resp.double_clicked() {
        ui.data_mut(|d| d.insert_temp(edit_id, edit_text));
    } else if resp.dragged() {
        // Part under the press picks the step (bars / beats / sixteenths …).
        let acc_id = id.with("acc");
        let mut acc: f32 = ui.data(|d| d.get_temp(acc_id)).unwrap_or(0.0);
        acc -= resp.drag_delta().y;
        let n = (acc / 3.0).trunc();
        acc -= n * 3.0;
        ui.data_mut(|d| d.insert_temp(acc_id, acc));
        if n != 0.0 {
            out = Some(n as f64 * step_px);
        }
    }
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    (resp, out)
}

pub enum Field {
    Parts([String; 3]),
    Text(String),
}

/// Which of the three parts the press landed in (0..2).
fn part_at(ui: &egui::Ui, r: Rect) -> usize {
    let x = ui.input(|i| i.pointer.press_origin()).map_or(r.right(), |p| p.x);
    let k = (x - r.left()) / r.width();
    if k < 0.36 {
        0
    } else if k < 0.65 {
        1
    } else {
        2
    }
}

/// Section header (bg #46, icon, title, ▼ open / ◀ folded) + its 1 px separator
/// when folded. Returns whether the body is shown.
fn header(ui: &mut egui::Ui, app: &mut DawApp, s: Sec, title: &str, icon: fn(&egui::Painter, Pos2, Color32)) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(LEFT_W, SEC_H), Sense::click());
    let p = ui.painter();
    p.rect_filled(r, 0.0, gray(0x46));
    icon(p, pos2(r.left() + 9.0, r.center().y), text());
    p.text(pos2(r.left() + 18.0, r.center().y), Align2::LEFT_CENTER, title, font(FS), text());
    let open = app.cv.open[s as usize];
    let a = pos2(r.right() - 14.5, r.center().y);
    let tri = if open {
        vec![a + vec2(-3.5, -2.5), a + vec2(3.5, -2.5), a + vec2(0.0, 3.0)]
    } else {
        vec![a + vec2(2.5, -3.5), a + vec2(2.5, 3.5), a + vec2(-3.0, 0.0)]
    };
    p.add(egui::Shape::convex_polygon(tri, text(), Stroke::NONE));
    if resp.clicked() {
        app.cv.open[s as usize] = !open;
    }
    if !app.cv.open[s as usize] {
        sep_row(ui);
    }
    app.cv.open[s as usize]
}

fn sep_row(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(LEFT_W, 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, sep());
}

fn hsep(p: &egui::Painter, o: Pos2, y: f32, x0: f32, x1: f32) {
    p.rect_filled(Rect::from_min_max(o + vec2(x0, y), o + vec2(x1, y + 1.0)), 0.0, sep());
}

// Section icons (9 px).
fn ico_clip(p: &egui::Painter, c: Pos2, col: Color32) {
    let r = Rect::from_center_size(c, vec2(9.0, 9.0));
    p.rect_stroke(r.shrink(0.5), 0.0, Stroke::new(1.0, col), egui::StrokeKind::Inside);
    p.add(egui::Shape::convex_polygon(vec![c + vec2(-1.5, -2.5), c + vec2(2.5, 0.0), c + vec2(-1.5, 2.5)], col, Stroke::NONE));
}
fn ico_audio(p: &egui::Painter, c: Pos2, col: Color32) {
    for (i, h) in [3.0f32, 7.0, 9.0, 5.0, 7.0].iter().enumerate() {
        let x = c.x - 4.0 + i as f32 * 2.0;
        p.line_segment([pos2(x, c.y - h / 2.0), pos2(x, c.y + h / 2.0)], Stroke::new(1.0, col));
    }
}
fn ico_launch(p: &egui::Painter, c: Pos2, col: Color32) {
    p.line_segment([pos2(c.x - 4.0, c.y - 4.0), pos2(c.x - 4.0, c.y + 4.0)], Stroke::new(1.0, col));
    p.add(egui::Shape::convex_polygon(vec![c + vec2(-2.0, -4.0), c + vec2(2.0, 0.0), c + vec2(-2.0, 4.0)], col, Stroke::NONE));
    p.add(egui::Shape::convex_polygon(vec![c + vec2(1.0, -4.0), c + vec2(5.0, 0.0), c + vec2(1.0, 4.0)], col, Stroke::NONE));
}
fn ico_pitch(p: &egui::Painter, c: Pos2, col: Color32) {
    for (i, w) in [(0, 8.0f32), (1, 5.0)] {
        let y = c.y - 2.0 + i as f32 * 4.0;
        p.line_segment([pos2(c.x - 4.0, y), pos2(c.x - 4.0 + w, y)], Stroke::new(1.0, col));
    }
    p.circle_filled(pos2(c.x + 2.0, c.y - 2.0), 1.5, col);
    p.circle_filled(pos2(c.x - 1.0, c.y + 2.0), 1.5, col);
}
fn ico_transform(p: &egui::Painter, c: Pos2, col: Color32) {
    p.circle_filled(c + vec2(-1.5, -1.0), 3.0, col);
    for d in [vec2(3.5, 2.5), vec2(1.0, 4.0), vec2(4.0, -0.5)] {
        p.circle_filled(c + d, 0.8, col);
    }
}
fn ico_generate(p: &egui::Painter, c: Pos2, col: Color32) {
    p.circle_stroke(c, 3.5, Stroke::new(1.0, col));
    p.circle_filled(c, 1.3, col);
    for d in [vec2(-4.5, -4.5), vec2(4.5, -4.5), vec2(-4.5, 4.5), vec2(4.5, 4.5)] {
        p.circle_filled(c + d * 0.9, 0.8, col);
    }
}

// ---- left panel --------------------------------------------------------------------------------

/// The left panel in `rect` (scrolls when the sections don't fit).
pub fn left_panel(app: &mut DawApp, ui: &mut egui::Ui, rect: Rect, cref: ClipRef, clip: &Clip) {
    ui.painter().rect_filled(rect, 0.0, panel());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child.spacing_mut().item_spacing = vec2(0.0, 0.0);
    let audio = matches!(clip.content, ClipContent::Audio { .. });
    let session = matches!(cref, ClipRef::Slot { .. });
    egui::ScrollArea::vertical().id_salt("cv_left").auto_shrink([false, false]).show(&mut child, |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        title_bar(app, ui, cref, clip);
        sep_row(ui);
        // Envelopes / MPE tabs: their lane choosers come first.
        match app.cv.tab {
            Tab::Envelopes => egui_body(ui, |ui| super::clip::envelopes_box(app, ui, cref, clip)),
            Tab::Mpe => egui_body(ui, |ui| super::clip::mpe_box(app, ui, cref, clip)),
            Tab::Main => {}
        }
        if header(ui, app, Sec::Clip, "Clip", ico_clip) {
            clip_body(app, ui, cref, clip);
            sep_row(ui);
        }
        if (session || !audio) && header(ui, app, Sec::Launch, "Launch", ico_launch) {
            egui_body(ui, |ui| {
                if session {
                    super::clip::launch_box(app, ui, cref, clip)
                } else {
                    ui.label(egui::RichText::new("Launch settings apply to Session clips").color(dim()));
                }
            });
        }
        if audio {
            if header(ui, app, Sec::Audio, "Audio", ico_audio) {
                audio_body(app, ui, cref, clip);
                sep_row(ui);
            }
            if header(ui, app, Sec::Transform, "Transform", ico_transform) {
                audio_transform(app, ui, cref);
            }
        } else {
            if header(ui, app, Sec::PitchTime, "Pitch & Time", ico_pitch) {
                pitch_time_body(app, ui, cref, clip);
                sep_row(ui);
            }
            if header(ui, app, Sec::Transform, "Transform", ico_transform) {
                app.cv.panel = super::clip::NotesPanel::Transform;
                egui_body(ui, |ui| super::clip::tools(app, ui, cref, clip));
            }
            if header(ui, app, Sec::Generate, "Generate", ico_generate) {
                app.cv.panel = super::clip::NotesPanel::Generate;
                egui_body(ui, |ui| super::clip::tools(app, ui, cref, clip));
            }
        }
    });
}

/// Section body made of regular widgets, then a separator.
fn egui_body(ui: &mut egui::Ui, f: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(vec2(LEFT_W, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
        egui::Frame::NONE.inner_margin(egui::Margin { left: 10, right: 10, top: 5, bottom: 6 }).show(ui, |ui| {
            ui.set_width(LEFT_W - 20.0);
            ui.spacing_mut().item_spacing = vec2(4.0, 3.0);
            f(ui)
        });
    });
    sep_row(ui);
}

/// 17 px title: clip colour square, name (double-click renames), save button (audio).
fn title_bar(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef, clip: &Clip) {
    let (r, resp) = ui.allocate_exact_size(vec2(LEFT_W, 17.0), Sense::click());
    let notes_focus = matches!(clip.content, ClipContent::Midi { .. }) && !app.note_sel.is_empty();
    let (bg, fg) = if notes_focus { (gray(0x3c), gray(0xc6)) } else { (Color32::from_rgb(0x85, 0xa5, 0xc2), gray(0x20)) };
    let p = ui.painter().clone();
    p.rect_filled(r, 0.0, bg);
    let sq = Rect::from_min_size(r.min + vec2(4.0, 4.0), vec2(9.0, 9.0));
    let act = ui.interact(sq, ui.make_persistent_id("cv_activator"), Sense::click()).on_hover_text("Clip Activator (0)");
    p.rect_filled(sq, 0.0, gray(0x11));
    p.rect_filled(sq.shrink(1.0), 0.0, if clip.muted { gray(0x5a) } else { rgb(clip.color) });
    let sel = app.arr_sel.len().max(1);
    let name = if sel > 1 { format!("{sel} Clips") } else { clip.name.clone() };
    let renaming = app.renaming.as_ref().is_some_and(|x| x.clip && x.id == clip.id && x.in_detail);
    let name_r = Rect::from_min_max(r.min + vec2(16.0, 1.0), r.max - vec2(22.0, 1.0));
    if renaming {
        super::rename_editor(app, ui, name_r, clip.id, true, true);
    } else {
        p.with_clip_rect(name_r).text(pos2(name_r.left() + 1.0, r.center().y), Align2::LEFT_CENTER, &name, bold(FS), fg);
    }
    if matches!(clip.content, ClipContent::Audio { .. }) {
        // Save Default Clip (not available): dark round button with a disk glyph.
        let c = pos2(r.right() - 9.0, r.center().y);
        p.circle_filled(c, 6.5, gray(0x1d));
        p.rect_filled(Rect::from_center_size(c + vec2(0.0, 0.5), vec2(7.0, 6.0)), 1.0, text());
        p.rect_filled(Rect::from_center_size(c + vec2(0.0, -1.0), vec2(4.0, 2.0)), 0.0, gray(0x1d));
        ui.interact(Rect::from_center_size(c, vec2(13.0, 13.0)), ui.make_persistent_id("cv_save_default"), Sense::hover())
            .on_hover_text("Save Default Clip (not available in this version)");
    }
    let mut c = clip.clone();
    let mut ed = Ed::default();
    if act.clicked() {
        c.muted = !c.muted;
        ed.set();
    }
    if resp.double_clicked() {
        if ui.input(|i| i.modifiers.shift) {
            app.cv.folded = !app.cv.folded;
        } else {
            app.start_rename_clip(clip.id, true);
        }
    }
    let resp = if clip.info.is_empty() { resp } else { resp.on_hover_text(&clip.info) };
    menu::context(&resp, 200.0, |ui| {
        if menu::item(ui, "Rename", "Ctrl+R", true) {
            app.start_rename_clip(clip.id, true);
        }
        if menu::item(ui, "Assign Track Color to Clip", "", true) {
            c.color = app.project.tracks[track_of(cref)].color;
            ed.set();
        }
        menu::sep(ui);
        if let Some(col) = menu::color_grid(ui, c.color) {
            c.color = col;
            ed.set();
        }
    });
    super::clip::commit(app, cref, ed, c);
}

fn track_of(c: ClipRef) -> usize {
    match c {
        ClipRef::Slot { track, .. } | ClipRef::Arr { track, .. } => track,
    }
}

/// Clip section body: Start / End, Duplicate (MIDI), Loop, Position / Length,
/// Signature / Groove, Scale.
fn clip_body(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef, clip: &Clip) {
    let (r, _) = ui.allocate_exact_size(vec2(LEFT_W, BODY_H), Sense::hover());
    let o = r.min;
    let p = ui.painter().clone();
    let at = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(o + vec2(x0, y0), o + vec2(x1 + 1.0, y1 + 1.0));
    let bpb = app.project.beats_per_bar as f64;
    let spb = 60.0 / app.project.tempo_bpm.max(1.0);
    let secs = matches!(clip.content, ClipContent::Audio { warp_bpm: None, .. });
    let midi = matches!(clip.content, ClipContent::Midi { .. });
    let pos = play_pos(app, cref);
    let mut c = clip.clone();
    let mut ed = Ed::default();
    let pos_box =
        |ui: &mut egui::Ui, ed: &mut Ed, r: Rect, id: &str, v: f64, position: bool| pos_box(ui, ed, r, id, v, position, secs, bpb, spb);
    label(&p, o + vec2(10.0, 10.0), "Start", text());
    label(&p, o + vec2(87.0, 10.0), "End", text());
    let set_ok = pos.is_some();
    let s1 = pill(ui, at(60.0, 4.0, 82.0, 16.0), "cv_set_start", "Set", set_ok);
    let s2 = pill(ui, at(137.0, 4.0, 159.0, 16.0), "cv_set_end", "Set", set_ok);
    if let Some(v) = pos_box(ui, &mut ed, at(10.0, 21.0, 82.0, 33.0), "cv_start", c.start, true) {
        c.start = v;
        ed.changed = true;
    }
    let end = c.start + c.length;
    if let Some(v) = pos_box(ui, &mut ed, at(87.0, 21.0, 159.0, 33.0), "cv_end", end, true)
        && v > c.start + 1.0 / 64.0
    {
        c.length = v - c.start;
        ed.changed = true;
    }
    if s1.clicked() {
        c.start = pos.unwrap_or(c.start);
        ed.set();
    }
    if s2.clicked() && pos.unwrap_or(0.0) > c.start {
        c.length = pos.unwrap_or(0.0) - c.start;
        ed.set();
    }
    let mut dup = false;
    if midi {
        dup = pill(ui, at(10.0, 38.0, 159.0, 50.0), "cv_dup", "Duplicate", c.looping).clicked();
    }
    hsep(&p, o, 55.0, 10.0, 160.0);
    // Loop (audio needs Warp): orange when on, greyed when not available.
    let can_loop = !secs;
    let lr = at(10.0, 60.0, 159.0, 72.0);
    let lresp = toggle(ui, lr, "cv_loop", "", c.looping, can_loop, rgb([0xff, 0xad, 0x56]));
    let lcol = if c.looping {
        gray(0x12)
    } else if can_loop {
        text()
    } else {
        gray(0x5a)
    };
    let g = p.layout_no_wrap("Loop".into(), font(FS), lcol);
    let x0 = lr.center().x - (g.size().x + 13.0) / 2.0;
    let ic = Rect::from_min_size(pos2(x0, lr.center().y - 4.0), vec2(9.0, 7.0));
    p.rect_stroke(ic, 1.0, Stroke::new(1.0, lcol), egui::StrokeKind::Inside);
    p.add(egui::Shape::convex_polygon(
        vec![ic.left_bottom() + vec2(3.0, -2.0), ic.left_bottom() + vec2(6.0, 0.0), ic.left_bottom() + vec2(3.0, 2.0)],
        lcol,
        Stroke::NONE,
    ));
    p.galley(pos2(x0 + 13.0, lr.center().y - g.size().y / 2.0), g, lcol);
    let lresp = lresp.on_hover_text(if can_loop { "Loop" } else { "Loop (audio clips need Warp)" });
    if lresp.clicked() {
        c.fix_loop();
        c.looping = !c.looping;
        ed.set();
    }
    label(&p, o + vec2(10.0, 83.0), "Position", text());
    label(&p, o + vec2(87.0, 83.0), "Length", text());
    let s3 = pill(ui, at(60.0, 77.0, 82.0, 89.0), "cv_set_pos", "Set", set_ok && c.looping);
    let s4 = pill(ui, at(137.0, 77.0, 159.0, 89.0), "cv_set_len", "Set", set_ok && c.looping);
    let (ls, le) = if c.looping { c.loop_range() } else { (c.start, c.start + c.length) };
    if let Some(v) = pos_box(ui, &mut ed, at(10.0, 94.0, 82.0, 106.0), "cv_lpos", ls, true) {
        (c.loop_start, c.loop_end) = (v, v + (le - ls));
        c.fix_loop();
        ed.changed = true;
    }
    if let Some(v) = pos_box(ui, &mut ed, at(87.0, 94.0, 159.0, 106.0), "cv_llen", le - ls, false)
        && v >= 1.0 / 16.0
    {
        (c.loop_start, c.loop_end) = (ls, ls + v);
        if matches!(cref, ClipRef::Slot { .. }) {
            c.length = v;
        }
        ed.changed = true;
    }
    if s3.clicked() {
        let p0 = pos.unwrap_or(ls);
        (c.loop_start, c.loop_end) = (p0, p0 + (le - ls));
        ed.set();
    }
    if s4.clicked() && pos.unwrap_or(0.0) > ls {
        (c.loop_start, c.loop_end) = (ls, pos.unwrap_or(le));
        ed.set();
    }
    hsep(&p, o, 111.0, 10.0, 160.0);
    // Signature / Groove (hot-swap opens the pool; commit writes it into the notes).
    label(&p, o + vec2(10.0, 122.0), "Signature", text());
    label(&p, o + vec2(87.0, 122.0), "Groove", text());
    let pool_c = o + vec2(138.0, 122.0);
    let commit_c = o + vec2(153.0, 122.0);
    for (k, cc) in [pool_c, commit_c].iter().enumerate() {
        p.circle_filled(*cc, 6.0, gray(0x1e));
        let col = if k == 1 && !(c.groove.is_some() && midi) { gray(0x5a) } else { text() };
        if k == 0 {
            p.circle_stroke(*cc, 3.0, Stroke::new(1.0, col));
        } else {
            p.add(egui::Shape::convex_polygon(
                vec![*cc + vec2(-2.0, -3.0), *cc + vec2(3.0, 0.0), *cc + vec2(-2.0, 3.0)],
                col,
                Stroke::NONE,
            ));
        }
    }
    if ui
        .interact(Rect::from_center_size(pool_c, vec2(12.0, 12.0)), ui.make_persistent_id("cv_pool"), Sense::click())
        .on_hover_text("Groove Pool")
        .clicked()
    {
        app.cv.show_pool = !app.cv.show_pool;
    }
    let commit = c.groove.is_some()
        && midi
        && ui
            .interact(Rect::from_center_size(commit_c, vec2(12.0, 12.0)), ui.make_persistent_id("cv_commit"), Sense::click())
            .on_hover_text("Commit Groove")
            .clicked();
    for (i, (x0, x1)) in [(10.0, 42.0), (49.0, 81.0)].iter().enumerate() {
        let v = if i == 0 { c.sig.0 } else { c.sig.1 } as f64;
        let (resp, out) =
            value_box(ui, at(*x0, 133.0, *x1, 145.0), ("cv_sig", i), gray(0x24), Field::Text(format!("{v}")), 1.0, format!("{v}"), |s| {
                s.trim().parse().ok()
            });
        ed.gesture |= resp.drag_started();
        if let Some(d) = out {
            let nv = if resp.dragged() { v + d } else { d };
            let lim = if i == 0 { 99.0 } else { 16.0 };
            let nv = nv.round().clamp(1.0, lim) as u8;
            if i == 0 {
                c.sig.0 = nv as _;
            } else {
                c.sig.1 = nv as _;
            }
            ed.changed = true;
        }
    }
    p.text(o + vec2(45.5, 140.0), Align2::CENTER_CENTER, "/", font(FS), text());
    let gname = c.groove.and_then(|id| app.project.grooves.iter().find(|g| g.id == id)).map_or("None".to_string(), |g| g.name.clone());
    let grooves: Vec<(u64, String)> = app.project.grooves.iter().map(|g| (g.id, g.name.clone())).collect();
    let mut pick: Option<Option<u64>> = None;
    let mut lib_pick = None;
    chooser(ui, at(87.0, 133.0, 159.0, 145.0), "cv_groove", &gname, true, 160.0, |ui| {
        if menu::check_item(ui, "None", "", c.groove.is_none(), true) {
            pick = Some(None);
        }
        for (id, n) in &grooves {
            if menu::check_item(ui, n, "", c.groove == Some(*id), true) {
                pick = Some(Some(*id));
            }
        }
        menu::sep(ui);
        for g in clipx::Groove::factory() {
            if menu::item(ui, &g.name, "", true) {
                lib_pick = Some(g);
            }
        }
    });
    if let Some(g) = pick {
        c.groove = g;
        ed.set();
    }
    if lib_pick.is_some() {
        app.cv.pending_groove = lib_pick;
    }
    hsep(&p, o, 150.0, 10.0, 160.0);
    // Scale: ♭♯ switch, root, scale name.
    label(&p, o + vec2(10.0, 161.0), "Scale", text());
    let sr = at(10.0, 172.0, 22.0, 184.0);
    let sresp = ui.interact(sr, ui.make_persistent_id("cv_scale_on"), Sense::click()).on_hover_text("Scale Mode");
    boxed(&p, sr, if c.scale.on { rgb([0xff, 0xad, 0x56]) } else { gray(0x1e) });
    let sc = if c.scale.on { gray(0x12) } else { text() };
    p.text(sr.center() + vec2(-2.0, 1.5), Align2::CENTER_CENTER, "b", font(FS_SMALL), sc);
    p.text(sr.center() + vec2(2.0, -1.5), Align2::CENTER_CENTER, "#", font(FS_SMALL), sc);
    if sresp.clicked() {
        c.scale.on = !c.scale.on;
        ed.set();
    }
    let mut root = None;
    chooser(ui, at(27.0, 172.0, 68.0, 184.0), "cv_root", NOTE_NAMES[c.scale.root as usize % 12], true, 60.0, |ui| {
        for (i, n) in NOTE_NAMES.iter().enumerate() {
            if menu::check_item(ui, n, "", c.scale.root as usize == i, true) {
                root = Some(i as u8);
            }
        }
    });
    let mut scale = None;
    chooser(ui, at(73.0, 172.0, 159.0, 184.0), "cv_scale", c.scale.name(), true, 170.0, |ui| {
        for (i, (n, _)) in SCALES.iter().enumerate() {
            if menu::check_item(ui, n, "", c.scale.scale as usize == i, true) {
                scale = Some(i as u8);
            }
        }
    });
    if let Some(v) = root {
        c.scale.root = v;
        ed.set();
    }
    if let Some(v) = scale {
        c.scale.scale = v;
        ed.set();
    }
    if let Some(mut g) = app.cv.pending_groove.take() {
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
    super::clip::commit(app, cref, ed, c);
    if dup {
        app.duplicate_loop(cref);
    }
    if commit {
        app.commit_groove(cref);
    }
}

/// A position box: bars-beats-sixteenths, or min:sec for unwarped audio. Returns
/// the new value (beats).
#[allow(clippy::too_many_arguments)]
fn pos_box(ui: &mut egui::Ui, ed: &mut Ed, r: Rect, id: &str, v: f64, position: bool, secs: bool, bpb: f64, spb: f64) -> Option<f64> {
    let shown = if secs { sec_parts(v * spb) } else { bbs_parts(v, bpb, position) };
    let step = if secs { [60.0 / spb, 1.0 / spb, 0.001 / spb][part_at(ui, r)] } else { [bpb, 1.0, 0.25][part_at(ui, r)] };
    let txt: String = shown.iter().map(|s| s.trim()).collect();
    let (resp, out) = value_box(ui, r, id, gray(0x24), Field::Parts(shown), step, txt, move |s| {
        if secs {
            let s = s.trim();
            let secs_v = match s.split_once(':') {
                Some((m, x)) => m.trim().parse::<f64>().ok()? * 60.0 + x.trim().parse::<f64>().ok()?,
                None => s.parse::<f64>().ok()?,
            };
            Some(secs_v / spb)
        } else {
            super::clip::parse_bbs(s, bpb, position)
        }
    });
    ed.gesture |= resp.drag_started() || resp.double_clicked();
    out.map(|d| if resp.dragged() { v + d } else { d })
}

/// Plain number box; returns the clamped new value.
#[allow(clippy::too_many_arguments)]
fn num_box(
    ui: &mut egui::Ui,
    ed: &mut Ed,
    r: Rect,
    id: &str,
    v: f32,
    range: (f32, f32),
    fill: Color32,
    fmt: &dyn Fn(f32) -> String,
) -> Option<f32> {
    let (resp, out) =
        value_box(ui, r, id, fill, Field::Text(fmt(v)), 1.0, format!("{v}"), |s| s.trim().trim_end_matches('%').trim().parse().ok());
    ed.gesture |= resp.drag_started();
    out.map(|d| if resp.dragged() { v + d as f32 } else { d as f32 }.clamp(range.0, range.1))
}

/// Audio section body: Warp / Follow, mode + its parameters, BPM, Reverse / Edit,
/// Fade / RAM / HiQ; right column: Gain fader and Pitch knob + st / ct boxes.
fn audio_body(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef, clip: &Clip) {
    let (r, _) = ui.allocate_exact_size(vec2(LEFT_W, BODY_H), Sense::hover());
    let o = r.min;
    let p = ui.painter().clone();
    let at = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(o + vec2(x0, y0), o + vec2(x1 + 1.0, y1 + 1.0));
    let mut c = clip.clone();
    let mut ed = Ed::default();
    let arr = matches!(cref, ClipRef::Arr { .. });
    let ClipContent::Audio { sample, warp_bpm, gain_db, .. } = &clip.content else { return };
    let (sample, warp_bpm, gain_db) = (sample.clone(), *warp_bpm, *gain_db);
    let warped = warp_bpm.is_some();
    let orange = rgb([0xff, 0xad, 0x56]);
    // Warp / Follow.
    if toggle(ui, at(10.0, 4.0, 49.0, 16.0), "cv_warp", "Warp", warped, true, orange).on_hover_text("Warp: follow the song tempo").clicked()
    {
        if let ClipContent::Audio { warp_bpm: w, .. } = &mut c.content {
            *w = if warped {
                None
            } else {
                let guess = app.cache.peek(&sample).map(|b| crate::app::loop_guess(b, app.project.tempo_bpm).1);
                Some(guess.flatten().unwrap_or(app.project.tempo_bpm))
            };
        }
        if warped {
            c.looping = false;
        }
        ed.set();
    }
    let lead = c.audio.lead;
    if toggle(ui, at(54.0, 4.0, 94.0, 16.0), "cv_follow", if lead { "Lead" } else { "Follow" }, lead, warped && arr, orange)
        .on_hover_text("Follow / Lead: Leader clips set the song tempo (bottom-most wins)")
        .clicked()
    {
        c.audio.lead = !lead;
        ed.set();
    }
    // Warp mode and its parameters.
    let mut mode = None;
    chooser(ui, at(10.0, 21.0, 94.0, 33.0), "cv_wmode", c.audio.mode.label(), warped, 120.0, |ui| {
        for m in WarpMode::ALL {
            if menu::check_item(ui, m.label(), "", c.audio.mode == m, true) {
                mode = Some(m);
            }
        }
    });
    if let Some(m) = mode {
        c.audio.mode = m;
        ed.set();
    }
    let lab = |s: &str| label(&p, o + vec2(10.0, 42.0), s, if warped { text() } else { gray(0x5a) });
    let fill = if warped { TEAL } else { gray(0x24) };
    let int = |v: f32| format!("{}", v.round());
    match c.audio.mode {
        WarpMode::Beats => {
            lab("Preserve");
            let mut pres = None;
            chooser(ui, at(10.0, 49.0, 94.0, 61.0), "cv_pres", &c.audio.params.preserve.label(), warped, 120.0, |ui| {
                for v in clipx::Preserve::ALL {
                    if menu::check_item(ui, &v.label(), "", c.audio.params.preserve == v, true) {
                        pres = Some(v);
                    }
                }
            });
            if let Some(v) = pres {
                c.audio.params.preserve = v;
                ed.set();
            }
            let lm = c.audio.params.loop_mode;
            let mut nl = None;
            let lr = at(10.0, 66.0, 49.0, 78.0);
            chooser(ui, lr, "cv_tloop", "", warped, 150.0, |ui| {
                for (m, l) in [
                    (clipx::TransientLoop::Off, "Loop Off"),
                    (clipx::TransientLoop::Forward, "Loop Forward"),
                    (clipx::TransientLoop::BackAndForth, "Loop Back-and-Forth"),
                ] {
                    if menu::check_item(ui, l, "", lm == m, true) {
                        nl = Some(m);
                    }
                }
            });
            loop_glyph(&p, pos2(lr.left() + 11.0, lr.center().y), lm, if warped { text() } else { gray(0x5a) });
            if let Some(m) = nl {
                c.audio.params.loop_mode = m;
                ed.set();
            }
            if let Some(v) = num_box(ui, &mut ed, at(54.0, 66.0, 94.0, 78.0), "cv_env", c.audio.params.envelope, (0.0, 100.0), fill, &int) {
                c.audio.params.envelope = v;
                ed.changed = true;
            }
        }
        WarpMode::Tones | WarpMode::Texture => {
            lab("Grain Size");
            let tones = c.audio.mode == WarpMode::Tones;
            let v = if tones { c.audio.params.grain_tones } else { c.audio.params.grain_texture };
            if let Some(v) = num_box(ui, &mut ed, at(10.0, 49.0, 94.0, 61.0), "cv_grain", v, (12.0, 100.0), fill, &int) {
                if tones {
                    c.audio.params.grain_tones = v;
                } else {
                    c.audio.params.grain_texture = v;
                }
                ed.changed = true;
            }
            if !tones {
                label(&p, o + vec2(10.0, 72.0), "Flux", text());
                if let Some(v) = num_box(ui, &mut ed, at(54.0, 66.0, 94.0, 78.0), "cv_flux", c.audio.params.flux, (0.0, 100.0), fill, &int)
                {
                    c.audio.params.flux = v;
                    ed.changed = true;
                }
            }
        }
        WarpMode::ComplexPro => {
            lab("Formants");
            if let Some(v) =
                num_box(ui, &mut ed, at(10.0, 49.0, 94.0, 61.0), "cv_form", c.audio.params.formants, (0.0, 100.0), fill, &|v| {
                    format!("{} %", v.round())
                })
            {
                c.audio.params.formants = v;
                ed.changed = true;
            }
            label(&p, o + vec2(10.0, 72.0), "Envelope", text());
            if let Some(v) =
                num_box(ui, &mut ed, at(54.0, 66.0, 94.0, 78.0), "cv_cenv", c.audio.params.cpro_envelope, (8.0, 256.0), fill, &int)
            {
                c.audio.params.cpro_envelope = v;
                ed.changed = true;
            }
        }
        WarpMode::RePitch | WarpMode::Complex => {}
    }
    // BPM (segment tempo; the song tempo while unwarped) with ÷2 / ×2.
    label(&p, o + vec2(10.0, 96.0), "BPM", text());
    let bpm = warp_bpm.unwrap_or(app.project.tempo_bpm);
    let mut new_bpm = None;
    {
        let (resp, out) = value_box(
            ui,
            at(10.0, 103.0, 48.0, 115.0),
            "cv_bpm",
            gray(0x24),
            Field::Text(format!("{bpm:.2}")),
            0.01,
            format!("{bpm:.2}"),
            |s| s.trim().parse().ok(),
        );
        ed.gesture |= resp.drag_started();
        if warped && let Some(d) = out {
            new_bpm = Some(if resp.dragged() { bpm + d * 10.0 } else { d });
        }
    }
    if pill(ui, at(53.0, 103.0, 71.0, 115.0), "cv_half", "/2", warped).clicked() {
        new_bpm = Some(bpm / 2.0);
        ed.set();
    }
    if pill(ui, at(75.0, 103.0, 93.0, 115.0), "cv_dbl", "×2", warped).clicked() {
        new_bpm = Some(bpm * 2.0);
        ed.set();
    }
    if let Some(nb) = new_bpm {
        let nb = nb.clamp(20.0, 999.0);
        let f = bpm / nb;
        c.audio.markers.iter_mut().for_each(|m| m.beat *= f);
        if let ClipContent::Audio { warp_bpm: w, .. } = &mut c.content {
            *w = Some(nb);
        }
        ed.changed = true;
    }
    hsep(&p, o, 145.0, 10.0, 95.0);
    let reverse =
        pill(ui, at(10.0, 150.0, 63.0, 162.0), "cv_rev", "Reverse", true).on_hover_text("Reverse the sample (new file)").clicked();
    if pill(ui, at(68.0, 150.0, 93.0, 162.0), "cv_edit", "Edit", true).on_hover_text("Open in the external sample editor").clicked() {
        super::clip::open_external(&sample);
    }
    hsep(&p, o, 167.0, 10.0, 95.0);
    let flags = [
        (c.audio.fade, "Fade", 10.0, 36.0, "Clip Fade (declick)"),
        (c.audio.ram, "RAM", 39.0, 65.0, "Load into RAM"),
        (c.audio.hiq, "HiQ", 68.0, 94.0, "High-quality interpolation"),
    ];
    for (i, (on, l, x0, x1, tip)) in flags.into_iter().enumerate() {
        let enabled = !(i == 0 && arr);
        if toggle(ui, at(x0, 172.0, x1, 184.0), ("cv_flag", i), l, on, enabled, orange).on_hover_text(tip).clicked() {
            match i {
                0 => c.audio.fade = !on,
                1 => c.audio.ram = !on,
                _ => c.audio.hiq = !on,
            }
            ed.set();
        }
    }
    // Right column.
    p.rect_filled(Rect::from_min_max(o + vec2(102.0, 4.0), o + vec2(103.0, 185.0)), 0.0, sep());
    p.text(o + vec2(134.0, 10.0), Align2::CENTER_CENTER, "Gain", font(FS), text());
    // Gain fader: +24 (y 19) … 0 (y 68) … −70 (y 101).
    let y_of = |db: f32| if db >= 0.0 { 68.0 - db / 24.0 * 49.0 } else { 68.0 + (-db).min(70.0) / 70.0 * 33.0 };
    let db_of = |y: f32| if y <= 68.0 { (68.0 - y) / 49.0 * 24.0 } else { -((y - 68.0) / 33.0 * 70.0) };
    let track = at(133.0, 19.0, 134.0, 101.0);
    p.rect_filled(track, 0.0, gray(0x07));
    for k in 0..9 {
        let y = 19.0 + k as f32 * (82.0 / 8.0);
        p.rect_filled(Rect::from_min_size(o + vec2(129.0, y.round()), vec2(3.0, 1.0)), 0.0, gray(0x75));
    }
    for (s, y) in [("24", 21.0), ("0", 68.0), ("70", 99.0)] {
        p.text(o + vec2(140.0, y), Align2::LEFT_CENTER, s, font(FS), text());
    }
    for (x, y, dx, dy) in [(110.0, 15.0, 1.0, 1.0), (160.0, 15.0, -1.0, 1.0), (110.0, 107.0, 1.0, -1.0), (160.0, 107.0, -1.0, -1.0)] {
        let c0 = o + vec2(x, y);
        p.line_segment([c0, c0 + vec2(4.0 * dx, 0.0)], Stroke::new(1.0, gray(0x75)));
        p.line_segment([c0, c0 + vec2(0.0, 4.0 * dy)], Stroke::new(1.0, gray(0x75)));
    }
    let gy = y_of(gain_db);
    let tri = o + vec2(128.0, gy);
    p.add(egui::Shape::convex_polygon(vec![tri + vec2(-4.0, -5.0), tri + vec2(3.0, 0.0), tri + vec2(-4.0, 5.0)], text(), Stroke::NONE));
    let gresp =
        ui.interact(at(110.0, 15.0, 160.0, 107.0), ui.make_persistent_id("cv_gain"), Sense::click_and_drag()).on_hover_text("Clip Gain");
    p.text(o + vec2(134.0, 112.0), Align2::CENTER_CENTER, format!("{:.1} dB", gain_db), font(FS), text());
    let mut new_gain = None;
    if gresp.drag_started() {
        ed.gesture = true;
    }
    if gresp.dragged() {
        let ny = (gy + gresp.drag_delta().y).clamp(19.0, 101.0);
        new_gain = Some(db_of(ny));
    }
    if gresp.double_clicked() {
        ed.gesture = true;
        new_gain = Some(0.0);
    }
    if let Some(g) = new_gain
        && let ClipContent::Audio { gain_db: gd, .. } = &mut c.content
    {
        *gd = (g * 10.0).round() / 10.0;
        ed.changed = true;
    }
    // Pitch: knob (semitones) + st / ct boxes.
    p.text(o + vec2(134.0, 131.0), Align2::CENTER_CENTER, "Pitch", font(FS), text());
    let kc = o + vec2(134.0, 150.0);
    let pitch_ok = c.audio.mode != WarpMode::RePitch || !warped;
    // Same design as the mixer pan knob (bipolar: ±48 st).
    let kresp = ui.interact(Rect::from_center_size(kc, vec2(28.0, 28.0)), ui.make_persistent_id("cv_pitch"), Sense::click_and_drag());
    // Click selects the knob (arrow keys then step it); a press elsewhere deselects.
    app.cv.pitch_drawn = true;
    if kresp.is_pointer_button_down_on() {
        app.cv.pitch_focus = true;
    } else if ui.input(|i| i.pointer.any_pressed()) {
        app.cv.pitch_focus = false;
    }
    let nudge = std::mem::take(&mut app.cv.pitch_nudge);
    if pitch_ok && nudge != 0 {
        c.audio.transpose = (c.audio.transpose + nudge).clamp(-48, 48) as _;
        ed.gesture = true;
        ed.changed = true;
    }
    let st = c.audio.transpose as f32;
    super::widgets::knob_paint(&p, kc, 12.0, st / 48.0, kresp.hovered() || kresp.dragged() || app.cv.pitch_focus, true);
    if pitch_ok && kresp.drag_started() {
        ed.gesture = true;
    }
    if pitch_ok && kresp.double_clicked() && c.audio.transpose != 0 {
        c.audio.transpose = 0;
        ed.gesture = true;
        ed.changed = true;
    }
    let acc_id = ui.make_persistent_id("cv_pitch_acc");
    if pitch_ok && kresp.dragged() {
        let mut acc: f32 = ui.data(|d| d.get_temp(acc_id)).unwrap_or(0.0) - kresp.drag_delta().y;
        let n = (acc / 4.0).trunc();
        acc -= n * 4.0;
        ui.data_mut(|d| d.insert_temp(acc_id, acc));
        if n != 0.0 {
            c.audio.transpose = (c.audio.transpose + n as i32).clamp(-48, 48) as _;
            ed.changed = true;
        }
    }
    p.text(o + vec2(134.0, 165.0), Align2::CENTER_CENTER, "st", font(FS), text());
    let tv = c.audio.transpose as f64;
    let (r1, o1) =
        value_box(ui, at(110.0, 172.0, 134.0, 184.0), "cv_st", gray(0x1e), Field::Text(format!("{tv}")), 1.0, format!("{tv}"), |s| {
            s.trim().parse().ok()
        });
    if pitch_ok && let Some(d) = o1 {
        c.audio.transpose = (if r1.dragged() { tv + d } else { d }).round().clamp(-48.0, 48.0) as _;
        ed.changed = true;
    }
    ed.gesture |= r1.drag_started();
    let dv = c.audio.detune as f64;
    let (r2, o2) = value_box(
        ui,
        at(135.0, 172.0, 159.0, 184.0),
        "cv_ct",
        gray(0x1e),
        Field::Text(format!("{}", dv.round())),
        1.0,
        format!("{dv}"),
        |s| s.trim().parse().ok(),
    );
    if let Some(d) = o2 {
        c.audio.detune = (if r2.dragged() { dv + d } else { d }).round().clamp(-50.0, 50.0) as _;
        ed.changed = true;
    }
    ed.gesture |= r2.drag_started();
    super::clip::commit(app, cref, ed, c);
    if reverse {
        app.reverse_clip(cref);
    }
}

/// ⇄ (forward) / ↔ (back-and-forth) / – (off) glyph of the transient loop mode.
fn loop_glyph(p: &egui::Painter, c: Pos2, m: clipx::TransientLoop, col: Color32) {
    let s = Stroke::new(1.0, col);
    match m {
        clipx::TransientLoop::Off => {
            p.line_segment([c + vec2(-5.0, 0.0), c + vec2(5.0, 0.0)], s);
        }
        clipx::TransientLoop::Forward => {
            p.line_segment([c + vec2(-5.0, -2.0), c + vec2(5.0, -2.0)], s);
            p.line_segment([c + vec2(5.0, -2.0), c + vec2(2.5, -4.5)], s);
            p.line_segment([c + vec2(-5.0, 2.0), c + vec2(5.0, 2.0)], s);
            p.line_segment([c + vec2(-5.0, 2.0), c + vec2(-2.5, 4.5)], s);
        }
        clipx::TransientLoop::BackAndForth => {
            p.line_segment([c + vec2(-5.0, 0.0), c + vec2(5.0, 0.0)], s);
            p.line_segment([c + vec2(-5.0, 0.0), c + vec2(-2.5, -2.5)], s);
            p.line_segment([c + vec2(-5.0, 0.0), c + vec2(-2.5, 2.5)], s);
            p.line_segment([c + vec2(5.0, 0.0), c + vec2(2.5, -2.5)], s);
            p.line_segment([c + vec2(5.0, 0.0), c + vec2(2.5, 2.5)], s);
        }
    }
}

/// Audio Transform: quantize warp markers to transients on a grid.
fn audio_transform(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef) {
    let (r, _) = ui.allocate_exact_size(vec2(LEFT_W, 46.0), Sense::hover());
    let o = r.min;
    let at = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(o + vec2(x0, y0), o + vec2(x1 + 1.0, y1 + 1.0));
    let mut g = None;
    let gi = app.cv.quant_grid as usize;
    chooser(ui, at(10.0, 6.0, 82.0, 18.0), "cv_qgrid", mt::GRIDS[gi].0, true, 80.0, |ui| {
        for (i, (l, _)) in mt::GRIDS.iter().enumerate() {
            if menu::check_item(ui, l, "", gi == i, true) {
                g = Some(i as f32);
            }
        }
    });
    if let Some(v) = g {
        app.cv.quant_grid = v;
    }
    let a = app.cv.quant_amount as f64;
    let (resp, out) = value_box(
        ui,
        at(87.0, 6.0, 159.0, 18.0),
        "cv_qamt",
        gray(0x24),
        Field::Text(format!("{} %", a.round())),
        1.0,
        format!("{a}"),
        |s| s.trim().trim_end_matches('%').trim().parse().ok(),
    );
    if let Some(d) = out {
        app.cv.quant_amount = (if resp.dragged() { a + d } else { d }).clamp(0.0, 100.0) as f32;
    }
    if pill(ui, at(10.0, 25.0, 159.0, 37.0), "cv_qwarp", "Quantize Warp Markers", true)
        .on_hover_text("Move warp markers to transients on the grid (Ctrl+U)")
        .clicked()
    {
        app.quantize_warp(cref, mt::grid_beats(app.cv.quant_grid), app.cv.quant_amount as f64 / 100.0);
    }
    sep_row(ui);
}

/// Pitch & Time body (acts on the selected notes, else the whole clip).
fn pitch_time_body(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef, clip: &Clip) {
    let (r, _) = ui.allocate_exact_size(vec2(LEFT_W, BODY_H), Sense::hover());
    let o = r.min;
    let p = ui.painter().clone();
    let at = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(o + vec2(x0, y0), o + vec2(x1 + 1.0, y1 + 1.0));
    let ClipContent::Midi { notes } = &clip.content else { return };
    let scale = clip.scale.on.then_some(clip.scale);
    type NoteOp = Box<dyn FnOnce(&mut Vec<Note>, &[usize])>;
    let mut op: Option<NoteOp> = None;
    let sel: Vec<&Note> =
        if app.note_sel.is_empty() { notes.iter().collect() } else { app.note_sel.iter().filter_map(|&i| notes.get(i)).collect() };
    // Pitch box: lowest–highest selected pitch; drag transposes (scale degrees with a scale).
    let pr = at(10.0, 4.0, 82.0, 36.0);
    p.rect_filled(pr, 3.0, gray(0x18));
    let lo = sel.iter().map(|n| n.pitch).min();
    let hi = sel.iter().map(|n| n.pitch).max();
    let name = |k: u8| super::detail::note_name(k);
    let txt = match (lo, hi) {
        (Some(a), Some(b)) if a == b => name(a),
        (Some(a), Some(b)) => format!("{} - {}", name(a), name(b)),
        _ => "—".into(),
    };
    p.text(pr.center(), Align2::CENTER_CENTER, txt, bold(FS), rgb([0xff, 0xad, 0x56]));
    let presp = ui.interact(pr, ui.make_persistent_id("cv_tr_box"), Sense::drag()).on_hover_text("Transpose: drag up / down");
    if presp.hovered() || presp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    let acc_id = ui.make_persistent_id("cv_tr_acc");
    if presp.dragged() {
        let mut acc: f32 = ui.data(|d| d.get_temp(acc_id)).unwrap_or(0.0) - presp.drag_delta().y;
        let n = (acc / 5.0).trunc();
        acc -= n * 5.0;
        ui.data_mut(|d| d.insert_temp(acc_id, acc));
        if n != 0.0 {
            let n = n as i32;
            op = Some(Box::new(move |v, s| super::clip::with_sel(v, s, |x| mt::transpose(x, n, scale))));
        }
    }
    if pill(ui, at(87.0, 5.0, 159.0, 17.0), "cv_fit", "Fit to Scale", scale.is_some()).clicked() {
        let s = clip.scale;
        op = Some(Box::new(move |v, sel| super::clip::with_sel(v, sel, |x| mt::fit_to_scale(x, s))));
    }
    if pill(ui, at(87.0, 22.0, 159.0, 34.0), "cv_inv", "Invert", true).clicked() {
        op = Some(Box::new(move |v, s| super::clip::with_sel(v, s, |x| mt::invert(x, scale))));
    }
    hsep(&p, o, 41.0, 10.0, 160.0);
    let iv = app.cv.interval as f64;
    let (ir, io) =
        value_box(ui, at(10.0, 46.0, 82.0, 58.0), "cv_interval", gray(0x1e), Field::Text(format!("{iv} st")), 1.0, format!("{iv}"), |s| {
            s.trim().trim_end_matches("st").trim().parse().ok()
        });
    if let Some(d) = io {
        app.cv.interval = (if ir.dragged() { iv + d } else { d }).round().clamp(-48.0, 48.0) as i32;
    }
    if pill(ui, at(87.0, 46.0, 159.0, 58.0), "cv_addint", "Add Interval", true).clicked() {
        let n = app.cv.interval;
        op = Some(Box::new(move |v, s| {
            let s: Vec<usize> = if s.is_empty() { (0..v.len()).collect() } else { s.to_vec() };
            mt::add_interval(v, &s, n, scale);
        }));
    }
    hsep(&p, o, 63.0, 10.0, 160.0);
    // Stretch knob (drag, applied on release) with ×2 / /2.
    p.text(o + vec2(46.0, 75.0), Align2::CENTER_CENTER, "Stretch", font(FS), text());
    let kc = o + vec2(46.0, 90.0);
    let f = app.cv.stretch;
    p.circle_stroke(kc, 10.5, Stroke::new(2.0, gray(0x11)));
    let ang = |v: f64| std::f32::consts::PI * (0.75 + 1.5 * ((v.log2() + 2.0) / 4.0) as f32);
    let a = ang(f);
    let pts: Vec<Pos2> =
        (0..=16).map(|i| ang(0.25) + (a - ang(0.25)) * i as f32 / 16.0).map(|t| kc + vec2(t.cos(), t.sin()) * 10.5).collect();
    p.add(egui::Shape::line(pts, Stroke::new(2.0, Color32::from_rgb(0x03, 0xc3, 0xd5))));
    p.line_segment([kc, kc + vec2(a.cos(), a.sin()) * 10.0], Stroke::new(2.0, text()));
    p.text(o + vec2(46.0, 112.0), Align2::CENTER_CENTER, format!("×{f:.1}"), font(FS), text());
    let kresp = ui
        .interact(Rect::from_center_size(kc, vec2(24.0, 24.0)), ui.make_persistent_id("cv_stretch"), Sense::drag())
        .on_hover_text("Stretch (applied on release)");
    if kresp.dragged() {
        app.cv.stretch = (f * (1.0 - kresp.drag_delta().y as f64 * 0.01)).clamp(0.25, 4.0);
    }
    if kresp.drag_stopped() && (app.cv.stretch - 1.0).abs() > 1e-3 {
        let k = app.cv.stretch;
        op = Some(Box::new(move |v, s| super::clip::with_sel(v, s, |x| super::clip::stretch_sel(x, k))));
        app.cv.stretch = 1.0;
    }
    if pill(ui, at(87.0, 79.0, 159.0, 91.0), "cv_x2", "×2", true).clicked() {
        op = Some(Box::new(|v, s| super::clip::with_sel(v, s, |x| super::clip::stretch_sel(x, 2.0))));
    }
    if pill(ui, at(87.0, 96.0, 159.0, 108.0), "cv_d2", "/2", true).clicked() {
        op = Some(Box::new(|v, s| super::clip::with_sel(v, s, |x| super::clip::stretch_sel(x, 0.5))));
    }
    hsep(&p, o, 123.0, 10.0, 160.0);
    // Duration chooser (Grid = current grid) + Set Length.
    const DUR: [(f64, &str); 8] =
        [(0.0, "Grid"), (0.125, "1/32"), (0.25, "1/16"), (0.5, "1/8"), (1.0, "1/4"), (2.0, "1/2"), (4.0, "1 Bar"), (8.0, "2 Bars")];
    let cur = DUR.iter().find(|(v, _)| (*v - app.cv.duration).abs() < 1e-9).map_or("Grid", |d| d.1);
    let mut pick = None;
    chooser(ui, at(10.0, 128.0, 82.0, 140.0), "cv_dur", cur, true, 90.0, |ui| {
        for (v, l) in DUR {
            if menu::check_item(ui, l, "", l == cur, true) {
                pick = Some(v);
            }
        }
    });
    if let Some(v) = pick {
        app.cv.duration = v;
    }
    if pill(ui, at(87.0, 128.0, 159.0, 140.0), "cv_setlen", "Set Length", true).clicked() {
        let d = if app.cv.duration <= 0.0 { app.pr_grid_now } else { app.cv.duration };
        op = Some(Box::new(move |v, s| super::clip::with_sel(v, s, |x| mt::set_length(x, d))));
    }
    hsep(&p, o, 145.0, 10.0, 160.0);
    // Humanize amount (teal bar) + Humanize.
    let hr = at(10.0, 150.0, 82.0, 162.0);
    let hv = app.cv.humanize;
    let (hresp, ho) = value_box(ui, hr, "cv_hum", gray(0x1e), Field::Text(String::new()), 0.01, format!("{}", (hv * 100.0).round()), |s| {
        s.trim().trim_end_matches('%').trim().parse::<f64>().ok().map(|v| v / 100.0)
    });
    if let Some(d) = ho {
        app.cv.humanize = (if hresp.dragged() { hv + d } else { d }).clamp(0.0, 1.0);
    }
    let inner = hr.shrink(1.0);
    p.rect_filled(Rect::from_min_size(inner.min, vec2(inner.width() * app.cv.humanize as f32, inner.height())), 0.0, TEAL);
    p.text(hr.center(), Align2::CENTER_CENTER, format!("{} %", (app.cv.humanize * 100.0).round()), font(FS), text());
    if pill(ui, at(87.0, 150.0, 159.0, 162.0), "cv_humanize", "Humanize", true).clicked() {
        let a = app.cv.humanize;
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64);
        op = Some(Box::new(move |v, s| super::clip::with_sel(v, s, |x| mt::humanize(x, a, seed))));
    }
    hsep(&p, o, 167.0, 10.0, 160.0);
    if pill(ui, at(10.0, 172.0, 82.0, 184.0), "cv_reverse", "Reverse", true).clicked() {
        op = Some(Box::new(|v, s| super::clip::with_sel(v, s, mt::reverse)));
    }
    if pill(ui, at(87.0, 172.0, 159.0, 184.0), "cv_legato", "Legato", true).clicked() {
        op = Some(Box::new(|v, s| super::clip::with_sel(v, s, mt::legato)));
    }
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
    }
}

// ---- editor header -----------------------------------------------------------------------------

/// Grid value label ("1/16", "1 Bar", "2 Bars").
pub fn grid_label(g: f64) -> String {
    if g >= 4.0 - 1e-9 {
        let bars = (g / 4.0).round() as i64;
        if bars == 1 { "1 Bar".into() } else { format!("{bars} Bars") }
    } else {
        format!("1/{}", (4.0 / g).round() as i64)
    }
}

/// Editor header: sample info (audio) or search / Fold (MIDI) at the left,
/// centred tabs, grid chooser at the right (MIDI and warped audio).
pub fn editor_header(app: &mut DawApp, ui: &mut egui::Ui, r: Rect, clip: &Clip, info: Option<String>, grid: Option<f64>) {
    let p = ui.painter().clone();
    p.rect_filled(r, 0.0, panel());
    p.rect_filled(Rect::from_min_max(pos2(r.left(), r.bottom()), pos2(r.right(), r.bottom() + 1.0)), 0.0, sep());
    let audio = matches!(clip.content, ClipContent::Audio { .. });
    if let Some(s) = info {
        // Waveform icon box + "name.wav  48.0 kHz · 32-Bit · 2 Ch".
        let ib = Rect::from_min_size(pos2(r.left() + 3.0, r.top() + 3.0), vec2(16.0, 11.0));
        p.rect_stroke(ib, 1.0, Stroke::new(1.0, gray(0x75)), egui::StrokeKind::Inside);
        for (i, h) in [3.0f32, 6.0, 8.0, 5.0, 7.0, 3.0].iter().enumerate() {
            let x = ib.left() + 3.5 + i as f32 * 2.0;
            p.line_segment([pos2(x, ib.center().y - h / 2.0), pos2(x, ib.center().y + h / 2.0)], Stroke::new(1.0, gray(0x75)));
        }
        p.text(pos2(r.left() + 21.0, r.center().y), Align2::LEFT_CENTER, s, font(FS), text());
    } else if !audio {
        // Search (greyed) and Fold.
        let sb = Rect::from_min_size(pos2(r.left() + 3.0, r.top() + 2.0), vec2(22.0, 13.0));
        p.rect_filled(sb, 0.0, gray(0x11));
        p.rect_filled(sb.shrink(1.0), 0.0, gray(0x1e));
        p.circle_stroke(sb.center() + vec2(-1.0, -1.0), 3.0, Stroke::new(1.0, gray(0x75)));
        p.line_segment([sb.center() + vec2(1.2, 1.2), sb.center() + vec2(3.5, 3.5)], Stroke::new(1.0, gray(0x75)));
        let fr = Rect::from_min_size(pos2(r.left() + 32.0, r.top() + 2.0), vec2(36.0, 13.0));
        let fold = app.cv.fold;
        if toggle(ui, fr, "pr_fold_btn", "Fold", fold, true, rgb([0xff, 0xad, 0x56]))
            .on_hover_text("Fold: show only rows with notes")
            .clicked()
        {
            app.cv.fold = !fold;
        }
    }
    // Tabs: 1 px #24 dividers, selected tab #46.
    let tabs: Vec<(&str, Tab)> = if audio {
        let warped = matches!(clip.content, ClipContent::Audio { warp_bpm: Some(_), .. });
        if warped { vec![("Sample", Tab::Main), ("Envelopes", Tab::Envelopes)] } else { vec![("Sample", Tab::Main)] }
    } else {
        vec![("Notes", Tab::Main), ("Envelopes", Tab::Envelopes), ("MPE", Tab::Mpe)]
    };
    if !tabs.iter().any(|t| t.1 == app.cv.tab) {
        app.cv.tab = Tab::Main;
    }
    let tw = 60.0;
    let total = tw * tabs.len() as f32;
    let mut x = (r.center().x - total / 2.0).round();
    for (name, t) in tabs {
        let tr = Rect::from_min_max(pos2(x, r.top()), pos2(x + tw, r.bottom()));
        let resp = ui.interact(tr, ui.make_persistent_id(("cv_tab", name)), Sense::click());
        let on = app.cv.tab == t;
        if on {
            p.rect_filled(tr, 0.0, gray(0x46));
        }
        p.rect_filled(Rect::from_min_max(tr.left_top(), pos2(tr.left() + 1.0, tr.bottom())), 0.0, sep());
        p.rect_filled(Rect::from_min_max(pos2(tr.right() - 1.0, tr.top()), tr.right_bottom()), 0.0, sep());
        p.text(tr.center(), Align2::CENTER_CENTER, name, font(FS), if on { text() } else { gray(0x8c) });
        if resp.clicked() {
            app.cv.tab = t;
        }
        x += tw;
    }
    if let Some(g) = grid {
        let gr = Rect::from_min_max(pos2(r.right() - 60.0, r.top()), r.right_bottom());
        let resp = ui.interact(gr, ui.make_persistent_id("cv_grid_chooser"), Sense::click()).on_hover_text("Grid (Ctrl+1 … Ctrl+4)");
        let l = grid_label(g);
        p.text(pos2(r.right() - 17.0, r.center().y), Align2::RIGHT_CENTER, l, font(FS), text());
        let a = pos2(r.right() - 9.0, r.center().y + 1.0);
        p.add(egui::Shape::convex_polygon(vec![a + vec2(-3.5, -2.5), a + vec2(3.5, -2.5), a + vec2(0.0, 2.0)], text(), Stroke::NONE));
        let mut mode = app.pr_grid;
        menu::dropdown(&resp, 150.0, |ui| {
            use crate::arr_ops::GridMode;
            menu::caption(ui, "Adaptive Grid:", 15.0);
            for (lvl, name) in GridMode::ADAPTIVE {
                if menu::check_item(ui, name, "", mode == GridMode::Adaptive(lvl), true) {
                    mode = GridMode::Adaptive(lvl);
                }
            }
            menu::sep(ui);
            menu::caption(ui, "Fixed Grid:", 15.0);
            for (v, name) in GridMode::FIXED {
                if menu::check_item(ui, name, "", mode == GridMode::Fixed(v), true) {
                    mode = GridMode::Fixed(v);
                }
            }
            menu::sep(ui);
            if menu::check_item(ui, "Off", "", mode == GridMode::Off, true) {
                mode = GridMode::Off;
            }
        });
        app.pr_grid = mode;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_labels() {
        assert_eq!(grid_label(0.25), "1/16");
        assert_eq!(grid_label(4.0 / 512.0), "1/512");
        assert_eq!(grid_label(4.0), "1 Bar");
        assert_eq!(grid_label(8.0), "2 Bars");
    }

    #[test]
    fn time_parts_match_live() {
        assert_eq!(sec_parts(0.0), ["0:".to_string(), "0.".into(), "0".into()]);
        assert_eq!(sec_parts(0.377), ["0:".to_string(), String::new(), "0.377".into()]);
        assert_eq!(sec_parts(65.5)[0], "1:");
        assert_eq!(bbs_parts(0.0, 4.0, true), ["1.".to_string(), "1.".into(), "1".into()]);
    }
}
