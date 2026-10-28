//! Session view: clip-slot grid (tracks × scenes), scene launchers, mixer strips.
//! Layout follows Live 12 at 100 %: track columns from the left, the Main column
//! pinned to the right edge, In/Out + mixer pinned to the bottom.

use super::theme::*;
use super::widgets::*;
use crate::app::{ClipRef, DawApp, DragItem, TrackFlag};
use daw_engine::{Command, PendingLaunch};
use daw_project::{ClipContent, TrackKind};
use eframe::egui::{self, Align2, Color32, CornerRadius, Rect, Sense, Stroke, pos2, vec2};

// Live 12 Session metrics (measured): 93 px columns + 2 px separators, 17 px
// header, 17 px slots on an 18 px pitch, 17 px stop row, 115 px In/Out, 120 px
// mixer, 6 px colour strip; the 103 px Main column sits behind a 3 px gap.
const COL_W: f32 = 93.0;
const PITCH: f32 = 95.0;
const HEADER_H: f32 = 17.0;
const SLOT_H: f32 = 17.0;
const SLOT_PITCH: f32 = 18.0;
const STOP_H: f32 = 17.0;
const IO_H: f32 = 115.0;
const MIXER_H: f32 = 120.0;
const STRIP_H: f32 = 6.0;
const MASTER_W: f32 = 103.0;
const MASTER_GAP: f32 = 3.0;

/// Vertical bands shared by every column, as (top, bottom) y.
#[derive(Clone, Copy)]
struct Bands {
    header: (f32, f32),
    slots: (f32, f32),
    stop: (f32, f32),
    io: Option<(f32, f32)>,
    mixer: (f32, f32),
    strip: (f32, f32),
    scroll_y: f32,
    scenes: usize,
}

impl Bands {
    fn rect(x0: f32, x1: f32, b: (f32, f32)) -> Rect {
        Rect::from_min_max(pos2(x0, b.0), pos2(x1, b.1))
    }
    fn slot(&self, s: usize) -> (f32, f32) {
        let y = self.slots.0 + s as f32 * SLOT_PITCH - self.scroll_y;
        (y, y + SLOT_H)
    }
    /// First y below the last slot's 2 px separator.
    fn slots_end(&self) -> f32 {
        self.slots.0 + self.scenes as f32 * SLOT_PITCH + 1.0 - self.scroll_y
    }
    fn slot_band(&self, x0: f32, x1: f32) -> Rect {
        Self::rect(x0, x1, self.slots)
    }
}

pub fn session_view(app: &mut DawApp, ui: &mut egui::Ui) {
    let full = ui.available_rect_before_wrap();
    ui.allocate_rect(full, Sense::hover());
    let blink = (app.status.beat_position * 2.0).fract() < 0.5 || !app.status.playing;
    let scenes = app.project.scenes.len();
    app.sel_scene = app.sel_scene.min(scenes.saturating_sub(1));

    // Bands, bottom-up.
    let strip = (full.max.y - STRIP_H, full.max.y);
    let mixer = (strip.0 - 2.0 - MIXER_H, strip.0 - 2.0);
    let io = app.show_io.then_some((mixer.0 - 2.0 - IO_H, mixer.0 - 2.0));
    let above = io.map_or(mixer.0, |b| b.0);
    let stop = (above - 2.0 - STOP_H, above - 2.0);
    let header = (full.min.y, full.min.y + HEADER_H);
    let slots = (header.1 + 2.0, (stop.0 - 2.0).max(header.1 + 2.0));

    // Scrolling: wheel = scenes, Shift+wheel / horizontal = tracks.
    let master_x = full.max.x - MASTER_W;
    let area = Rect::from_min_max(full.min, pos2(master_x - MASTER_GAP, full.max.y));
    let tracks = app.project.visible_tracks();
    let sid = egui::Id::unique("sess").with("session_scroll");
    let (mut sx, mut sy): (f32, f32) = ui.data(|d| d.get_temp(sid)).unwrap_or_default();
    if ui.rect_contains_pointer(full) {
        let d = ui.input(|i| i.smooth_scroll_delta);
        sx -= d.x;
        sy -= d.y;
    }
    let content_w = tracks.len() as f32 * PITCH + 120.0;
    let content_h = scenes as f32 * SLOT_PITCH + 1.0;
    sx = sx.clamp(0.0, (content_w - area.width()).max(0.0));
    sy = sy.clamp(0.0, (content_h - (slots.1 - slots.0)).max(0.0));
    ui.data_mut(|d| d.insert_temp(sid, (sx, sy)));
    let b = Bands { header, slots, stop, io, mixer, strip, scroll_y: sy, scenes };

    let p = ui.painter_at(full);
    p.rect_filled(full, CornerRadius::ZERO, panel());
    // The selected scene's row is highlighted across the whole view (Live).
    let (y0, y1) = b.slot(app.sel_scene);
    if scenes > 0 {
        p.with_clip_rect(b.slot_band(full.min.x, full.max.x)).rect_filled(
            Rect::from_min_max(pos2(full.min.x, y0), pos2(master_x, y1)),
            CornerRadius::ZERO,
            gray(0x46),
        );
    }

    let n0 = app.project.tracks.len();
    let clip = ui.clip_rect().intersect(area);
    for (k, &t) in tracks.iter().enumerate() {
        if app.project.tracks.len() != n0 {
            // A menu added/removed tracks: indices are stale, redraw next frame.
            ui.ctx().request_repaint();
            break;
        }
        let x0 = (area.min.x + k as f32 * PITCH - sx).round();
        if x0 > area.max.x || x0 + PITCH < area.min.x {
            continue;
        }
        let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(pos2(x0, full.min.y), pos2(x0 + COL_W, full.max.y))));
        cui.set_clip_rect(clip);
        track_column(app, &mut cui, t, x0, &b, blink);
    }

    // Empty space right of the tracks: drop target (new track) and track menu.
    let ex0 = (area.min.x + tracks.len() as f32 * PITCH - sx).max(area.min.x);
    if ex0 < area.max.x - 1.0 && app.project.tracks.len() == n0 {
        let er = Rect::from_min_max(pos2(ex0, full.min.y), pos2(area.max.x, full.max.y));
        let resp = ui.interact(er, egui::Id::unique("sess").with("session_drop"), Sense::click());
        let cy = ((slots.0 + b.slots_end()) / 2.0).clamp(slots.0 + 8.0, slots.1.max(slots.0 + 8.0));
        p.with_clip_rect(er).text(pos2(er.center().x, cy), Align2::CENTER_CENTER, "Drop Files and Devices Here", bold(FS), text());
        if resp.dnd_hover_payload::<DragItem>().is_some() {
            p.rect_stroke(er.shrink(1.0), CornerRadius::ZERO, Stroke::new(1.0, ACCENT), egui::StrokeKind::Inside);
        }
        if let Some(item) = resp.dnd_release_payload::<DragItem>() {
            drop_new_track(app, &item);
        }
        super::menu::context(&resp, super::menu::W_TRACK, |ui| {
            use super::menu::item;
            if item(ui, "Insert Audio Track", "Ctrl+T", true) {
                app.add_track(TrackKind::Audio);
            }
            if item(ui, "Insert MIDI Track", "Ctrl+Shift+T", true) {
                app.add_track(TrackKind::Midi);
            }
            item(ui, "Insert Return Track", "Ctrl+Alt+T", false);
        });
    }

    // Main column, pinned right.
    p.rect_filled(Rect::from_min_max(pos2(master_x - MASTER_GAP, full.min.y), pos2(master_x, full.max.y)), CornerRadius::ZERO, sep());
    master_column(app, ui, master_x, &b);
}

/// One track column: header, slots, stop row, In/Out, mixer, colour strip.
fn track_column(app: &mut DawApp, ui: &mut egui::Ui, t: usize, x0: f32, b: &Bands, blink: bool) {
    let x1 = x0 + COL_W;
    let bg = if app.is_track_selected(t) { gray(0x46) } else { panel() };
    let p = ui.painter().clone();
    // Separators = whatever the bands leave uncovered (2 px right, 1 px between slots).
    p.rect_filled(Rect::from_min_max(pos2(x0, b.header.0), pos2(x0 + PITCH, b.strip.1)), CornerRadius::ZERO, sep());
    track_header(app, ui, t, Bands::rect(x0, x1, b.header));
    if app.project.tracks.len() <= t {
        return;
    }
    let band = b.slot_band(x0, x1);
    let mut sui = ui.new_child(egui::UiBuilder::new().max_rect(band));
    sui.set_clip_rect(band.intersect(ui.clip_rect()));
    for s in 0..b.scenes {
        let (y0, y1) = b.slot(s);
        if y1 < band.min.y || y0 > band.max.y {
            continue;
        }
        let row_bg = if s == app.sel_scene { gray(0x46) } else { bg };
        slot(app, &mut sui, t, s, Rect::from_min_max(pos2(x0, y0), pos2(x1, y1)), row_bg, blink);
    }
    let rest = b.slots_end();
    if rest < band.max.y {
        p.with_clip_rect(band.intersect(ui.clip_rect())).rect_filled(
            Rect::from_min_max(pos2(x0, rest.max(band.min.y)), band.max),
            CornerRadius::ZERO,
            bg,
        );
    }
    stop_button(app, ui, t, Bands::rect(x0, x1, b.stop));
    if let Some(io) = b.io {
        let r = Bands::rect(x0, x1, io);
        p.rect_filled(r, CornerRadius::ZERO, bg);
        super::io::io_section(app, ui, t, r);
    }
    let r = Bands::rect(x0, x1, b.mixer);
    p.rect_filled(r, CornerRadius::ZERO, bg);
    mixer_strip(app, ui, t, r);
    if let Some(track) = app.project.tracks.get(t) {
        p.rect_filled(Bands::rect(x0, x1, b.strip), CornerRadius::ZERO, rgb(track.color));
    }
}

/// Main column: header, scene launchers, Stop All, Cue/Main Out, master mixer.
fn master_column(app: &mut DawApp, ui: &mut egui::Ui, x0: f32, b: &Bands) {
    let x1 = x0 + MASTER_W;
    let p = ui.painter().clone();
    let mcol = if app.project.master_color == [150, 150, 150] { gray(0x3c) } else { rgb(app.project.master_color) };
    p.rect_filled(Rect::from_min_max(pos2(x0, b.header.0), pos2(x1, b.strip.1)), CornerRadius::ZERO, sep());
    // Header: click = master device view, right-click = colour.
    let hr = Bands::rect(x0, x1, b.header);
    let resp = ui.interact(hr, egui::Id::unique("sess").with("main_header"), Sense::click());
    super::add_master_zone(ui, hr);
    p.rect_filled(hr, CornerRadius::ZERO, if app.master_selected { shade(mcol, 1.25) } else { mcol });
    p.text(hr.left_center() + vec2(4.0, 0.0), Align2::LEFT_CENTER, "Main", bold(FS), text_on(mcol));
    if resp.clicked() {
        app.select_master();
    }
    resp.context_menu(|ui| master_menu(app, ui));
    // Scene launchers.
    let band = b.slot_band(x0, x1);
    let mut sui = ui.new_child(egui::UiBuilder::new().max_rect(band));
    sui.set_clip_rect(band.intersect(ui.clip_rect()));
    for s in 0..b.scenes {
        let (y0, y1) = b.slot(s);
        if y1 < band.min.y || y0 > band.max.y {
            continue;
        }
        scene_button(app, &mut sui, s, Rect::from_min_max(pos2(x0, y0), pos2(x1, y1)));
    }
    let rest = b.slots_end();
    if rest < band.max.y {
        p.with_clip_rect(band).rect_filled(Rect::from_min_max(pos2(x0, rest.max(band.min.y)), band.max), CornerRadius::ZERO, panel());
    }
    // Stop row: Stop All Clips + two (greyed) Live buttons.
    let sr = Bands::rect(x0, x1, b.stop);
    let resp = ui.interact(sr, egui::Id::unique("sess").with("stop_all"), Sense::click());
    p.rect_filled(sr, CornerRadius::ZERO, panel());
    let sq = Rect::from_min_size(sr.min + vec2(4.0, 4.0), vec2(9.0, 9.0));
    p.rect_filled(sq, CornerRadius::ZERO, if resp.hovered() { gray(0x60) } else { off_fill() });
    if resp.clicked() && resp.interact_pointer_pos().is_some_and(|q| q.x < sr.min.x + 40.0) {
        app.send(Command::StopAllClips);
    }
    let b1 = Rect::from_min_size(sr.min + vec2(62.0, 2.0), vec2(13.0, 13.0));
    let b2 = Rect::from_min_size(sr.min + vec2(79.0, 2.0), vec2(20.0, 13.0));
    for (r, fill) in [(b1, gray(0x5a)), (b2, off_fill())] {
        p.rect_filled(r, CornerRadius::ZERO, fill);
        p.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, outline()), egui::StrokeKind::Inside);
    }
    let c = b1.center();
    p.line_segment([c + vec2(-3.0, -4.0), c + vec2(-3.0, 4.0)], Stroke::new(1.0, gray(0x24)));
    p.add(egui::Shape::convex_polygon(vec![c + vec2(-1.0, -4.0), c + vec2(3.5, 0.0), c + vec2(-1.0, 4.0)], gray(0x24), Stroke::NONE));
    let c = b2.center() - vec2(4.0, 0.0);
    p.add(egui::Shape::convex_polygon(vec![c + vec2(-3.0, -4.0), c + vec2(3.0, 0.0), c + vec2(-3.0, 4.0)], dim(), Stroke::NONE));
    for dy in [-3.0, 0.0, 3.0] {
        p.line_segment([c + vec2(4.0, dy), c + vec2(10.0, dy)], Stroke::new(1.0, dim()));
    }
    if let Some(io) = b.io {
        let r = Bands::rect(x0, x1, io);
        p.rect_filled(r, CornerRadius::ZERO, panel());
        super::io::master_io(app, ui, r);
    }
    let r = Bands::rect(x0, x1, b.mixer);
    p.rect_filled(r, CornerRadius::ZERO, if app.master_selected { gray(0x46) } else { panel() });
    master_strip(app, ui, r);
    p.rect_filled(Bands::rect(x0, x1, b.strip), CornerRadius::ZERO, mcol);
}

fn track_header(app: &mut DawApp, ui: &mut egui::Ui, t: usize, rect: Rect) {
    let track = &app.project.tracks[t];
    let resp = ui.interact(rect, egui::Id::unique("sess").with(("sess_head", track.id)), Sense::click());
    let color = rgb(track.color);
    let is_group = track.kind == TrackKind::Group;
    let folded = track.folded;
    let parent_color = app.project.parent_index(t).map(|p| rgb(app.project.tracks[p].color));
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, color);
    let mut x = rect.min.x + 5.0;
    if let Some(pc) = parent_color {
        // Child of a group: group colour strip on the left.
        p.rect_filled(Rect::from_min_size(rect.min, vec2(4.0, rect.height())), CornerRadius::ZERO, pc);
        x += 3.0;
    }
    let fold_zone = Rect::from_min_size(pos2(x - 2.0, rect.min.y), vec2(16.0, rect.height()));
    if is_group {
        draw_fold_arrow(p, fold_zone.center(), folded, Color32::BLACK);
        x += 14.0;
    }
    let renaming = app.renaming.as_ref().is_some_and(|r| r.id == track.id && !r.clip);
    if !renaming {
        p.with_clip_rect(rect.shrink(2.0).intersect(ui.clip_rect())).text(
            pos2(x, rect.center().y),
            Align2::LEFT_CENTER,
            app.project.track_label(t),
            bold(FS),
            text_on(color),
        );
    }
    if resp.clicked() {
        if is_group && resp.interact_pointer_pos().is_some_and(|p| fold_zone.contains(p)) {
            app.toggle_fold(t);
        } else {
            let mode = ui.input(|i| crate::arr_ops::SelMode::from_mods(&i.modifiers));
            app.select_track_header(t, mode);
            app.selected_clip = None;
            app.empty_slot = None;
        }
    }
    if resp.double_clicked() {
        app.select_track_header(t, crate::arr_ops::SelMode::Single);
        app.start_rename(t);
    }
    if renaming {
        super::rename_editor(
            app,
            ui,
            Rect::from_min_max(pos2(x - 2.0, rect.min.y + 1.0), rect.max - vec2(1.0, 1.0)),
            app.project.tracks[t].id,
            false,
            false,
        );
    }
    if let Some(item) = resp.dnd_release_payload::<DragItem>()
        && let Some(def) = super::instrument_for(&item)
    {
        app.set_instrument(t, def);
    }
    open_track_menu(app, &resp, t);
}

/// ▶ (folded) / ▼ (open) triangle for group headers.
pub fn draw_fold_arrow(p: &egui::Painter, c: egui::Pos2, folded: bool, col: Color32) {
    let pts = if folded {
        vec![c + vec2(-3.0, -4.5), c + vec2(4.0, 0.0), c + vec2(-3.0, 4.5)]
    } else {
        vec![c + vec2(-4.5, -3.0), c + vec2(4.5, -3.0), c + vec2(0.0, 4.0)]
    };
    p.add(egui::Shape::convex_polygon(pts, col, Stroke::NONE));
}

/// Master header right-click: colour (Live lets the Main track be recoloured).
pub fn master_menu(app: &mut DawApp, ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("Master colour").small().color(Color32::GRAY));
    ui.horizontal(|ui| {
        for c in daw_project::PALETTE.into_iter().chain([[150, 150, 150]]) {
            let (rect, resp) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::click());
            ui.painter().rect_filled(rect, CornerRadius::same(2), rgb(c));
            if app.project.master_color == c {
                ui.painter().rect_stroke(rect, CornerRadius::same(2), Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Outside);
            }
            if resp.clicked() {
                app.edit(|p| p.master_color = c);
            }
        }
    });
}

/// Right-click on a track header (Session or Arrangement): Live's track menu.
/// The clicked track joins the selection first (unselected → selected alone).
pub fn open_track_menu(app: &mut DawApp, resp: &egui::Response, t: usize) {
    if resp.secondary_clicked() && !app.is_track_selected(t) {
        app.select_track_header(t, crate::arr_ops::SelMode::Single);
    }
    super::menu::context(resp, super::menu::W_TRACK, |ui| track_menu(app, ui, t));
}

pub fn track_menu(app: &mut DawApp, ui: &mut egui::Ui, t: usize) {
    use super::menu::*;
    let Some(track) = app.project.tracks.get(t) else { return };
    let (is_group, kind, color, has_clips) = (track.kind == TrackKind::Group, track.kind, track.color, !track.arrangement.is_empty());
    let sel = {
        let s = app.selected_tracks();
        if s.contains(&t) { s } else { vec![t] }
    };
    // Track commands act on the headers (not on clips / a time selection).
    let focus_headers = |app: &mut DawApp| {
        app.head_focus = true;
        app.focus_detail = false;
        app.arr_sel.clear();
        app.time_sel = None;
    };
    if item(ui, "Cut", "Ctrl+X", true) {
        focus_headers(app);
        if let Some(s) = app.cut_selection() {
            ui.ctx().copy_text(s);
        }
    }
    if item(ui, "Copy", "Ctrl+C", true) {
        focus_headers(app);
        if let Some(s) = app.copy_selection() {
            ui.ctx().copy_text(s);
        }
    }
    if item(ui, "Duplicate", "Ctrl+D", true) {
        focus_headers(app);
        app.duplicate_selected_tracks();
    }
    if item(ui, "Delete", "Del", true) {
        focus_headers(app);
        app.delete_selected_tracks();
    }
    sep(ui);
    if item(ui, "Rename", "Ctrl+R", true) {
        app.select_track_header(t, crate::arr_ops::SelMode::Single);
        app.start_rename(t);
    }
    item(ui, "Edit Info Text", "", false);
    sep(ui);
    item(ui, "Freeze Track", "Ctrl+Shift+Alt+F", false);
    item(ui, "Bounce Track in Place", "", false);
    sep(ui);
    if item(ui, "Insert Audio Track", "Ctrl+T", true) {
        app.add_track(TrackKind::Audio);
    }
    if item(ui, "Insert MIDI Track", "Ctrl+Shift+T", true) {
        app.add_track(TrackKind::Midi);
    }
    item(ui, "Insert Return Track", "Ctrl+Alt+T", false);
    if item(ui, "Group Tracks", "Ctrl+G", true) {
        app.group_selected_tracks();
    }
    if is_group && item(ui, "Ungroup Tracks", "Ctrl+Shift+G", true) {
        app.click_track(t, false);
        app.ungroup_selected();
    }
    sep(ui);
    if item(ui, "Select Track Content", "", has_clips) {
        app.select_track_content(t);
    }
    sep(ui);
    item(ui, "Insert Take Lane", "Shift+Alt+T", false);
    item(ui, "Show Take Lanes", "Ctrl+Alt+U", false);
    item(ui, "Delete All Take Lanes", "", false);
    item(ui, "Delete Unused Take Lanes", "", false);
    sep(ui);
    item(ui, "Link Tracks", "", false);
    item(ui, "Unlink Track(s)", "", false);
    sep(ui);
    item(ui, if kind == TrackKind::Audio { "Save as Default Audio Track" } else { "Save as Default MIDI Track" }, "", false);
    sep(ui);
    if let Some(c) = color_grid(ui, color) {
        app.set_tracks_color(&sel, c);
        ui.close();
    }
    sep(ui);
    if item(ui, "Assign Track Color to Clips", "", true) {
        app.assign_track_color_to_clips(&sel);
    }
}

/// Group slot: launches the children's clips of that scene (Live).
fn group_slot(app: &mut DawApp, ui: &mut egui::Ui, g: usize, s: usize, rect: Rect, bg: Color32) {
    let resp = ui.interact(rect, egui::Id::unique("sess").with(("gslot", g, s)), Sense::click());
    let p = &app.project;
    let kids = g + 1..p.subtree_end(g);
    let has = kids.clone().any(|c| p.tracks[c].slots.get(s).is_some_and(Option::is_some));
    let playing = kids.clone().any(|c| app.track_status.get(c).is_some_and(|st| st.playing_scene == Some(s as u32)));
    let color = rgb(p.tracks[g].color);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::ZERO, bg);
    if has {
        painter.rect_filled(rect, CornerRadius::ZERO, shade(color, if playing { 0.8 } else { 0.4 }));
        let launch = rect.left_center() + vec2(8.0, 0.0);
        draw_play(&painter, launch, if playing { PLAY_GREEN } else { gray(25) });
        // Mini overview: one tick per child with a clip.
        let n = kids.len().max(1) as f32;
        for (k, c) in kids.clone().enumerate() {
            if let Some(Some(clip)) = p.tracks[c].slots.get(s) {
                let w = (rect.width() - 26.0) / n;
                let r = Rect::from_min_size(
                    pos2(rect.min.x + 22.0 + k as f32 * w, rect.min.y + 5.0),
                    vec2((w - 2.0).max(1.0), rect.height() - 10.0),
                );
                painter.rect_filled(r, CornerRadius::same(1), rgb(clip.color));
            }
        }
    } else {
        painter.rect_filled(Rect::from_min_size(rect.min + vec2(4.0, 4.0), vec2(9.0, 9.0)), CornerRadius::ZERO, off_fill());
    }
    if resp.clicked() {
        app.click_track(g, false);
        app.sel_scene = s;
        if has {
            app.launch_group_slot(g, s);
        } else {
            app.stop_group(g);
        }
    }
}

fn slot(app: &mut DawApp, ui: &mut egui::Ui, t: usize, s: usize, rect: Rect, bg: Color32, blink: bool) {
    if app.project.tracks[t].kind == TrackKind::Group {
        return group_slot(app, ui, t, s, rect, bg);
    }
    let resp = ui.interact(rect, egui::Id::unique("sess").with(("slot", t, s)), Sense::click_and_drag());
    let st = app.track_status.get(t).copied().unwrap_or_default();
    let playing = st.playing_scene == Some(s as u32);
    let pending = st.pending == Some(PendingLaunch::Scene(s as u32));
    let selected = app.selected_clip == Some(ClipRef::Slot { track: t, scene: s })
        || (app.selected_clip.is_none() && app.selected_track == t && app.empty_slot == Some((t, s)));
    let track = &app.project.tracks[t];
    let armed = track.arm;
    let recording_here = app.session_rec.is_some_and(|(id, sc)| id == track.id && sc == s);
    let p = ui.painter_at(rect);
    let launch_zone = Rect::from_min_size(rect.min, vec2(16.0, rect.height()));
    // Live: 9 px launch / stop / record button, 4 px from the slot's top-left.
    let btn = Rect::from_min_size(rect.min + vec2(4.0, 4.0), vec2(9.0, 9.0));

    match track.slots.get(s).and_then(|c| c.as_ref()) {
        Some(clip) => {
            let c = if clip.muted { disabled(rgb(clip.color)) } else { rgb(clip.color) };
            let fill = if selected { SEL_BLUE } else { c };
            p.rect_filled(rect, CornerRadius::ZERO, fill);
            let tri = if playing || (pending && blink) { PLAY_GREEN } else { Color32::from_gray(0x1c) };
            let lc = btn.center();
            p.add(egui::Shape::convex_polygon(vec![lc + vec2(-3.5, -4.5), lc + vec2(4.5, 0.0), lc + vec2(-3.5, 4.5)], tri, Stroke::NONE));
            let name = match &clip.content {
                ClipContent::Audio { .. } => clip.name.clone(),
                ClipContent::Midi { .. } => clip.name.clone(),
            };
            p.with_clip_rect(rect.shrink(1.0).intersect(ui.clip_rect())).text(
                pos2(rect.min.x + 18.0, rect.center().y),
                Align2::LEFT_CENTER,
                name,
                font(FS),
                text_on(fill),
            );
            if playing && clip.length > 0.0 {
                let f = (st.clip_pos / clip.length).clamp(0.0, 1.0) as f32;
                p.rect_filled(
                    Rect::from_min_size(pos2(rect.min.x, rect.max.y - 3.0), vec2(rect.width() * f, 3.0)),
                    CornerRadius::ZERO,
                    Color32::from_black_alpha(140),
                );
            }
            // Launch on press (Live); release matters for Gate / Repeat modes.
            let held_id = egui::Id::unique("launch_held");
            if resp.hovered()
                && ui.input(|i| i.pointer.primary_pressed())
                && ui.ctx().pointer_latest_pos().is_some_and(|p| launch_zone.contains(p))
            {
                app.launch_clip(t, s);
                ui.data_mut(|d| d.insert_temp(held_id, (t, s)));
            }
            if resp.clicked() {
                app.sel_scene = s;
                if resp.interact_pointer_pos().is_some_and(|p| launch_zone.contains(p)) {
                    // Launched on press.
                } else {
                    app.selected_clip = Some(ClipRef::Slot { track: t, scene: s });
                    app.selected_track = t;
                }
            }
            if resp.double_clicked() {
                app.launch_clip(t, s);
            }
            resp.dnd_set_drag_payload(DragItem::SessionClip { track: t, scene: s });
            resp.context_menu(|ui| clip_menu(app, ui, t, s));
        }
        None => {
            p.rect_filled(rect, CornerRadius::ZERO, if selected { SEL_BLUE } else { bg });
            if armed {
                let col = if recording_here && blink { REC_RED } else { gray(if recording_here { 90 } else { 120 }) };
                p.circle_filled(btn.center(), 4.5, col);
            } else {
                p.rect_filled(btn, CornerRadius::ZERO, if resp.hovered() { gray(0x60) } else { off_fill() });
            }
            if pending && blink {
                p.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0, PLAY_GREEN), egui::StrokeKind::Inside);
            }
            let midi = track.kind == TrackKind::Midi;
            if resp.double_clicked() && midi {
                app.create_midi_clip(t, s);
            } else if resp.clicked() {
                app.selected_track = t;
                app.sel_scene = s;
                app.selected_clip = None;
                app.empty_slot = Some((t, s));
                if resp.interact_pointer_pos().is_some_and(|p| launch_zone.contains(p)) {
                    if armed {
                        app.record_into_slot(t, s);
                    } else if let Some(rt) = app.rt(t) {
                        app.send(Command::StopTrack { track: rt });
                    }
                }
            }
            super::menu::context(&resp, super::menu::W_CLIP, |ui| {
                if super::menu::item(ui, "Insert MIDI Clip", "Ctrl+Shift+M", midi) {
                    app.create_midi_clip(t, s);
                }
            });
        }
    }
    // Drops: samples / instruments / clips.
    if resp.dnd_hover_payload::<DragItem>().is_some() {
        p.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.5, ACCENT), egui::StrokeKind::Inside);
    }
    if let Some(item) = resp.dnd_release_payload::<DragItem>() {
        drop_on_slot(app, &item, t, s, ui.input(|i| i.modifiers.command));
    }
}

fn clip_menu(app: &mut DawApp, ui: &mut egui::Ui, t: usize, s: usize) {
    let Some(clip) = app.project.tracks[t].slots[s].clone() else { return };
    let mut name = clip.name.clone();
    let r = ui.text_edit_singleline(&mut name);
    if r.gained_focus() {
        app.begin_gesture();
    }
    if r.changed()
        && let Some(c) = app.project.tracks[t].slots[s].as_mut()
    {
        c.name = name;
    }
    if ui.checkbox(&mut clip.looping.clone(), "Loop").clicked() {
        app.edit(|p| {
            if let Some(c) = p.tracks[t].slots[s].as_mut() {
                c.looping = !c.looping;
            }
        });
    }
    if ui.button("Duplicate  Ctrl+D").clicked() {
        app.selected_clip = Some(ClipRef::Slot { track: t, scene: s });
        app.duplicate_selected_clip();
        ui.close();
    }
    if ui.button("Copy to Arrangement at Playhead").clicked() {
        let at = (app.status.beat_position / app.grid).floor() * app.grid;
        app.edit(|p| {
            let mut c = clip.clone();
            c.id = p.alloc_id();
            c.looping = false;
            p.tracks[t].arrangement.push(daw_project::ArrangementClip { start: at, clip: c });
            p.normalize();
        });
        ui.close();
    }
    if ui.button("Delete  Del").clicked() {
        app.selected_clip = Some(ClipRef::Slot { track: t, scene: s });
        app.delete_selected_clip();
        ui.close();
    }
}

pub fn drop_on_slot(app: &mut DawApp, item: &DragItem, t: usize, s: usize, copy: bool) {
    if app.project.tracks[t].kind == TrackKind::Group {
        return;
    }
    match item {
        DragItem::Sample { key, name } => {
            if app.project.tracks[t].kind == TrackKind::Audio {
                if let Some(mut clip) = app.audio_clip_for(key, name) {
                    app.edit(|p| {
                        clip.id = p.alloc_id();
                        clip.color = p.new_clip_color(t);
                        p.tracks[t].slots[s] = Some(clip);
                    });
                    app.selected_clip = Some(ClipRef::Slot { track: t, scene: s });
                }
            } else if let Some(def) = super::instrument_for(item) {
                app.set_instrument(t, def);
            }
        }
        DragItem::SessionClip { track, scene } => {
            let (st, ss) = (*track, *scene);
            if (st, ss) == (t, s) {
                return;
            }
            app.edit(|p| {
                let Some(mut c) = p.tracks.get(st).and_then(|x| x.slots.get(ss)).cloned().flatten() else { return };
                if copy {
                    c.id = p.alloc_id();
                } else {
                    p.tracks[st].slots[ss] = None;
                }
                p.tracks[t].slots[s] = Some(c);
            });
            app.selected_clip = Some(ClipRef::Slot { track: t, scene: s });
        }
        _ => {
            if let Some(def) = super::instrument_for(item) {
                app.set_instrument(t, def);
            }
        }
    }
}

fn drop_new_track(app: &mut DawApp, item: &DragItem) {
    match item {
        DragItem::Sample { key, name } => {
            let Some(mut clip) = app.audio_clip_for(key, name) else { return };
            let t = app.edit(|p| {
                let t = p.add_track(TrackKind::Audio);
                p.tracks[t].name = format!("# {name}");
                clip.id = p.alloc_id();
                clip.color = p.new_clip_color(t);
                if p.scenes.is_empty() {
                    p.add_scene();
                }
                p.tracks[t].slots[0] = Some(clip);
                t
            });
            app.selected_track = t;
        }
        DragItem::SessionClip { track, scene } => {
            // New track of the clip's kind, clip in the same scene (MIDI keeps the instrument).
            let (st, ss) = (*track, *scene);
            let Some(mut clip) = app.project.tracks.get(st).and_then(|x| x.slots.get(ss)).cloned().flatten() else { return };
            let audio = matches!(clip.content, ClipContent::Audio { .. });
            let inst = app.project.tracks[st].instrument.clone();
            let t = app.edit(|p| {
                let t = p.add_track(if audio { TrackKind::Audio } else { TrackKind::Midi });
                if !audio {
                    p.tracks[t].instrument = inst;
                }
                clip.id = p.alloc_id();
                if let Some(slot) = p.tracks[t].slots.get_mut(ss) {
                    *slot = Some(clip);
                }
                t
            });
            app.selected_track = t;
        }
        _ => {
            if let Some(def) = super::instrument_for(item) {
                let t = app.edit(|p| {
                    let t = p.add_track(TrackKind::Midi);
                    p.tracks[t].name = format!("# {}", def.label());
                    p.tracks[t].instrument = def;
                    t
                });
                app.selected_track = t;
            }
        }
    }
}

fn stop_button(app: &mut DawApp, ui: &mut egui::Ui, t: usize, rect: Rect) {
    let resp = ui.interact(rect, egui::Id::unique("sess").with(("stop", t)), Sense::click());
    let is_group = app.project.tracks[t].kind == TrackKind::Group;
    let st = app.track_status.get(t).copied().unwrap_or_default();
    let active = if is_group {
        (t + 1..app.project.subtree_end(t)).any(|c| app.track_status.get(c).is_some_and(|s| s.playing_scene.is_some()))
    } else {
        st.playing_scene.is_some()
    };
    let p = ui.painter();
    // Live: the stop row keeps the plain panel colour even on a selected track.
    p.rect_filled(rect, CornerRadius::ZERO, panel());
    let c = if st.pending == Some(PendingLaunch::Stop) {
        ACCENT
    } else if active {
        gray(0xc8)
    } else if resp.hovered() {
        gray(0x60)
    } else {
        off_fill()
    };
    p.rect_filled(Rect::from_min_size(rect.min + vec2(4.0, 4.0), vec2(9.0, 9.0)), CornerRadius::ZERO, c);
    // 1 px mark at the column centre (Live).
    let x = rect.min.x + 46.0;
    p.rect_filled(Rect::from_min_max(pos2(x, rect.min.y + 2.0), pos2(x + 1.0, rect.min.y + 15.0)), CornerRadius::ZERO, text());
    if resp.clicked() {
        if is_group {
            app.stop_group(t);
        } else if let Some(rt) = app.rt(t) {
            app.send(Command::StopTrack { track: rt });
        }
    }
}

fn scene_button(app: &mut DawApp, ui: &mut egui::Ui, s: usize, rect: Rect) {
    let resp = ui.interact(rect, egui::Id::unique("sess").with(("scene", s)), Sense::click());
    // Live scene launcher: dark ▶ at the left, scene name right-aligned.
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, if s == app.sel_scene { gray(0x46) } else { panel() });
    let c = rect.min + vec2(7.5, 8.5);
    let tri = if resp.hovered() { gray(0x60) } else { off_fill() };
    p.add(egui::Shape::convex_polygon(vec![c + vec2(-3.5, -4.5), c + vec2(3.5, 0.0), c + vec2(-3.5, 4.5)], tri, Stroke::NONE));
    p.with_clip_rect(rect.intersect(ui.clip_rect())).text(
        rect.right_center() - vec2(11.0, 0.0),
        Align2::RIGHT_CENTER,
        &app.project.scenes[s].name,
        font(FS),
        text(),
    );
    if resp.clicked() {
        app.sel_scene = s;
        if resp.interact_pointer_pos().is_some_and(|q| q.x < rect.min.x + 16.0) {
            app.launch_scene(s);
        }
    }
    if resp.double_clicked() {
        app.launch_scene(s);
    }
    resp.context_menu(|ui| {
        let mut name = app.project.scenes[s].name.clone();
        let r = ui.text_edit_singleline(&mut name);
        if r.gained_focus() {
            app.begin_gesture();
        }
        if r.changed() {
            app.project.scenes[s].name = name;
        }
        if ui.button("Insert Scene").clicked() {
            app.edit(|p| p.insert_scene(s + 1));
            ui.close();
        }
        if ui.button("Delete Scene").clicked() {
            app.edit(|p| p.remove_scene(s));
            app.selected_clip = None;
            ui.close();
        }
    });
}

/// Track mixer (120 px, Live 12 measures from the column's left edge `L`):
/// peak pill L+7 (35×13), pan knob at L+24 (r 13), number L+13 (23×23),
/// S / arm 23×13, fader handle from L+46, meter L+58 (13 px), scale L+78.
/// MIDI tracks without an instrument show the activity dots instead.
fn mixer_strip(app: &mut DawApp, ui: &mut egui::Ui, t: usize, rect: Rect) {
    let o = rect.min;
    let track = &app.project.tracks[t];
    let (mute, solo, arm, kind, has_input, midi_only) =
        (track.mute, track.solo, track.arm, track.kind, track.has_input(), track.kind == TrackKind::Midi && track.sends_midi());
    if midi_only {
        let level = app.track_status.get(t).map_or(0.0, |s| if s.voices > 0 { 1.0 } else { 0.0 });
        let lit = (level * 13.0) as i32;
        for i in 0..13 {
            let c = pos2(o.x + 65.0, o.y + 115.0 - 3.0 - i as f32 * 8.0);
            ui.painter().circle_filled(c, 3.0, if i < lit { Color32::from_rgb(0xff, 0xd0, 0x40) } else { M_OFF });
        }
    } else {
        let levels = app.track_meters.get(t).copied().unwrap_or([0.0; 2]);
        peak_pill(ui, Rect::from_min_size(o + vec2(7.0, 6.0), vec2(35.0, 13.0)), egui::Id::unique("sess").with(("speak", t)), levels);
        let mut db = app.project.tracks[t].gain_db;
        let (r2, changed2) = fader_meter(
            ui,
            Rect::from_min_max(o + vec2(46.0, 6.0), o + vec2(93.0, 114.0)),
            egui::Id::unique("sess").with(("sfader", t)),
            &mut db,
            levels,
            false,
        );
        if r2.drag_started() || r2.double_clicked() {
            app.begin_gesture();
        }
        if changed2 {
            app.set_track_gain(t, db);
        }
        let mut pan = app.project.tracks[t].pan;
        let (r, changed) = knob(ui, o + vec2(24.0, 42.0), 12.0, egui::Id::unique("sess").with(("span", t)), &mut pan);
        if r.drag_started() || r.double_clicked() {
            app.begin_gesture();
        }
        if changed {
            app.set_track_pan(t, pan);
        }
    }
    // Activator / S / arm stacked (Live).
    let ctrl = ui.input(|i| i.modifiers.command);
    let act_col = if app.solo_silenced.get(t).copied().unwrap_or(false) { solo_dimmed(ACCENT) } else { ACCENT };
    let act = Rect::from_min_size(o + vec2(13.0, 57.0), vec2(23.0, 23.0));
    if rect_toggle(ui, act, egui::Id::unique("sess").with(("sact", t)), &format!("{}", t + 1), !mute, act_col, "Track activator (mute)")
        .clicked()
    {
        app.toggle_track_flag(t, TrackFlag::Mute, false);
    }
    let sr = Rect::from_min_size(o + vec2(13.0, 84.0), vec2(23.0, 13.0));
    if rect_toggle(ui, sr, egui::Id::unique("sess").with(("ssolo", t)), "S", solo, Color32::from_rgb(0x5a, 0xa8, 0xff), "Solo (Ctrl: add)")
        .clicked()
    {
        app.toggle_track_flag(t, TrackFlag::Solo, !ctrl);
    }
    let ar = Rect::from_min_size(o + vec2(13.0, 101.0), vec2(23.0, 13.0));
    if kind != TrackKind::Group
        && has_input
        && rect_toggle(
            ui,
            ar,
            egui::Id::unique("sess").with(("sarm", t)),
            "REC",
            arm,
            REC_RED,
            "Arm for recording (all selected tracks; Ctrl: just this one)",
        )
        .clicked()
    {
        app.click_arm(t, ctrl);
    }
}

/// Main mixer: peak pill, pan, Solo/Cue switch (greyed), blue Cue knob, fader
/// with its ticks between handle and meter (Live).
fn master_strip(app: &mut DawApp, ui: &mut egui::Ui, rect: Rect) {
    let resp = ui.interact(rect, egui::Id::unique("sess").with("master_strip"), Sense::click());
    super::add_master_zone(ui, rect);
    if resp.clicked() {
        app.select_master();
    }
    let o = rect.min;
    peak_pill(ui, Rect::from_min_size(o + vec2(14.0, 6.0), vec2(35.0, 13.0)), egui::Id::unique("sess").with("mpeak"), app.meter);
    let mut db = app.project.master_gain_db;
    let (r2, changed2) = fader_meter(
        ui,
        Rect::from_min_max(o + vec2(53.0, 6.0), o + vec2(100.0, 114.0)),
        egui::Id::unique("sess").with("mfader"),
        &mut db,
        app.meter,
        true,
    );
    if r2.drag_started() || r2.double_clicked() {
        app.begin_gesture();
    }
    if changed2 {
        app.project.master_gain_db = db;
        app.dirty = true;
        app.send_now(Command::SetMasterGain(daw_project::db_to_gain(db)));
    }
    let mut pan = app.project.master_pan;
    let (r, changed) = knob(ui, o + vec2(31.0, 45.0), 12.0, egui::Id::unique("sess").with("mpan_knob"), &mut pan);
    if r.drag_started() || r.double_clicked() {
        app.begin_gesture();
    }
    if changed {
        app.project.master_pan = pan;
        app.dirty = true;
        app.send_now(Command::SetMasterPan(pan));
    }
    // Solo / Cue switch: no cue bus here, so it stays on "Solo" (greyed look).
    let sw = Rect::from_min_size(o + vec2(17.0, 69.0), vec2(28.0, 13.0));
    let p = ui.painter();
    p.rect_filled(sw, CornerRadius::ZERO, gray(0x75));
    p.rect_stroke(sw, CornerRadius::ZERO, Stroke::new(1.0, outline()), egui::StrokeKind::Inside);
    p.text(sw.center(), Align2::CENTER_CENTER, "Solo", font(FS), gray(0x24));
    // Blue Cue volume knob (metronome + browser preview) with the headphones mark.
    let c = o + vec2(31.0, 95.0);
    let mut cue = app.project.cue_gain_db;
    let mut cv = (db_to_pos(cue) * 2.0 - 1.0).clamp(-1.0, 1.0);
    let (r, changed) = knob_ex(ui, c, 9.0, egui::Id::unique("sess").with("mcue_knob"), &mut cv, false);
    if r.drag_started() || r.double_clicked() {
        app.begin_gesture();
    }
    if changed {
        cue = if r.double_clicked() { 0.0 } else { pos_to_db((cv + 1.0) * 0.5) };
        app.project.cue_gain_db = cue;
        app.dirty = true;
        app.send_now(Command::SetCueGain(daw_project::db_to_gain(cue)));
    }
    let ang = |t: f32| std::f32::consts::PI * (-0.5 + t * 5.0 / 6.0);
    let pts: Vec<egui::Pos2> = (0..=24)
        .map(|i| {
            let a = ang(-1.0) + (ang(cv) - ang(-1.0)) * i as f32 / 24.0;
            c + vec2(a.cos(), a.sin()) * 9.0
        })
        .collect();
    let p = ui.painter();
    p.add(egui::Shape::line(pts, Stroke::new(2.0, Color32::from_rgb(0x3c, 0x6a, 0xb6))));
    let hp = c + vec2(0.0, 11.0);
    p.circle_stroke(hp, 2.5, Stroke::new(1.0, gray(0x83)));
    r.on_hover_text(format!(
        "Cue volume {} dB (metronome + preview)",
        if cue <= METER_FLOOR_DB { "-inf".into() } else { format!("{cue:.1}") }
    ));
}
