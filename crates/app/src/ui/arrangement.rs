//! Arrangement view (Live-style): timeline lanes on the left, track headers on
//! the right, ruler with loop brace on top.
//!
//! Mouse: clip title bar = move (all selected clips), clip edges = trim (never
//! past the source material), lane / clip body drag = time selection, drag inside
//! a selection = cut that part out and move it, click = playhead + select.
//! Alt = ignore the grid. Wheel = vertical scroll, Shift+wheel = horizontal
//! scroll, Ctrl+wheel = zoom, Alt+wheel = height of the selected tracks (all
//! when none is selected; the minimum collapses lanes to clip title bars).
//! Headers: click/Shift+click select, drag = reorder / move in or out of groups,
//! double-click = rename, ⊙ = collapse.

use super::theme::{panel, win_bg};
use super::widgets::*;
use crate::app::{ClipRef, DawApp, DragItem, TrackFlag};
use crate::arr_ops::{SelMode, TimeSel};
use daw_engine::SampleBuffer;
use daw_project::{ArrangementClip, Clip, ClipContent, TrackDrop, TrackKind};
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};

const HEAD_W: f32 = 236.0;
const RULER_H: f32 = 30.0;
const EDGE: f32 = 6.0;
const TITLE_H: f32 = 14.0;
const INDENT: f32 = 9.0;

/// Screen geometry of the current frame.
struct Geo {
    x0: f32,
    zoom: f64,
    scroll: f64,
    body: Rect,
    lanes: Rect,
    rows: Vec<usize>,
    /// Content-space top of each row; `ys[rows.len()]` = drop zone top, last = content end.
    ys: Vec<f32>,
    vscroll: f32,
    grid: f64,
    /// Grid snapping active (grid not Off and Alt not held).
    snap_on: bool,
    /// In/Out column width (0 = hidden).
    io_w: f32,
}

impl Geo {
    fn to_x(&self, b: f64) -> f32 {
        self.x0 + ((b - self.scroll) * self.zoom) as f32
    }
    fn beat_at(&self, x: f32) -> f64 {
        (self.scroll + (x - self.x0) as f64 / self.zoom).max(0.0)
    }
    fn snap(&self, b: f64) -> f64 {
        if self.snap_on { (b / self.grid).round() * self.grid } else { b }
    }
    fn row_y(&self, k: usize) -> f32 {
        self.body.min.y - self.vscroll + self.ys[k.min(self.ys.len() - 1)]
    }
    fn row_h(&self, k: usize) -> f32 {
        if k + 1 < self.ys.len() { self.ys[k + 1] - self.ys[k] } else { 0.0 }
    }
    /// Lane rect of visible row `k` (k == rows.len() is the "add track" drop zone).
    fn lane(&self, k: usize) -> Rect {
        let y = self.row_y(k);
        Rect::from_min_max(pos2(self.lanes.min.x, y), pos2(self.lanes.max.x, y + self.row_h(k) - 1.0))
    }
    /// Row index under `y` (rows.len() = drop zone, rows.len() + 1 = below everything).
    fn row_at(&self, y: f32) -> usize {
        let cy = y - self.body.min.y + self.vscroll;
        if cy < 0.0 {
            return 0;
        }
        self.ys.partition_point(|&v| v <= cy).saturating_sub(1)
    }
    fn track_at(&self, y: f32) -> Option<usize> {
        self.rows.get(self.row_at(y)).copied()
    }
    /// Clamped row index (for drags past the ends).
    fn row_at_clamped(&self, y: f32) -> usize {
        self.row_at(y).min(self.rows.len().saturating_sub(1))
    }
}

/// Clip move in progress.
#[derive(Clone, Debug)]
struct MoveDrag {
    /// (id, original track, original start, length, is audio)
    clips: Vec<(u64, usize, f64, f64, bool)>,
    anchor_beat: f64,
    anchor_row: usize,
    primary_start: f64,
    /// False when the gesture already made its undo step (split-for-drag).
    checkpoint: bool,
    /// Started from a time selection: the selection follows the clips.
    from_selection: bool,
}

#[derive(Clone, Debug)]
enum Gesture {
    Select {
        anchor: f64,
        track: usize,
    },
    Move(MoveDrag),
    /// Edge trim; `orig` is the clip at drag start (limits come from it).
    Resize {
        orig: Box<ArrangementClip>,
        left: bool,
        fpb: f64,
        total: Option<f64>,
    },
    LoopNew {
        anchor: f64,
    },
    LoopMove {
        grab: f64,
    },
    LoopEdge {
        start: bool,
    },
    /// Header drag: reorder / regroup these tracks.
    Tracks {
        ids: Vec<u64>,
    },
}

fn gesture_id() -> egui::Id {
    egui::Id::unique("arr_gesture")
}

pub fn arrangement_view(app: &mut DawApp, ui: &mut egui::Ui) {
    let full = ui.available_rect_before_wrap();
    let io_w = if app.show_io { io_width(full.width()) } else { 0.0 };
    let head_w = HEAD_W + io_w;
    let lanes_x1 = (full.max.x - head_w).max(full.min.x + 40.0);
    let master_h = if app.project.master_collapsed {
        crate::arr_ops::COLLAPSED_H + 2.0
    } else if app.project.master_height > 0.0 {
        app.project.master_height
    } else {
        app.arr_row_h
    };
    let body = Rect::from_min_max(
        pos2(full.min.x, full.min.y + RULER_H),
        pos2(full.max.x, (full.max.y - master_h).max(full.min.y + RULER_H + 20.0)),
    );
    let lanes = Rect::from_min_max(body.min, pos2(lanes_x1, body.max.y));
    let x0 = full.min.x;

    wheel(app, ui, full, x0);

    let rows = app.project.visible_tracks();
    let mut ys = Vec::with_capacity(rows.len() + 2);
    let mut y = 0.0;
    for &t in &rows {
        ys.push(y);
        y += app.track_height(t);
    }
    ys.push(y);
    ys.push(y + app.arr_row_h); // drop zone
    let content_h = y + app.arr_row_h;
    app.arr_vscroll = app.arr_vscroll.clamp(0.0, (content_h - body.height()).max(0.0));
    let bpb = app.project.beats_per_bar.max(1) as f64;
    let zoom = app.arr_zoom as f64;
    let grid = app.arr_grid.step(zoom, bpb);
    app.grid = grid;
    let alt = ui.input(|i| i.modifiers.alt);
    let g = Geo {
        x0,
        zoom,
        scroll: app.arr_scroll as f64,
        body,
        lanes,
        rows,
        ys,
        vscroll: app.arr_vscroll,
        grid,
        snap_on: app.arr_grid.snaps() && !alt,
        io_w,
    };

    ruler(app, ui, &g, Rect::from_min_max(full.min, pos2(lanes_x1, full.min.y + RULER_H)), bpb);
    let side = Rect::from_min_max(pos2(lanes_x1, full.min.y), pos2(full.max.x, full.min.y + RULER_H));
    ui.painter().rect_filled(side, CornerRadius::ZERO, panel());
    let grid_txt = match app.arr_grid {
        crate::arr_ops::GridMode::Off => "Grid off".to_string(),
        _ => format!("Grid {}", grid_label(grid)),
    };
    ui.painter().text(side.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, grid_txt, FontId::proportional(11.0), Color32::GRAY);
    let side_r = ui.interact(side, ui.scope_id().with("arr_grid_label"), Sense::click());
    side_r.context_menu(|ui| grid_menu(ui, &mut app.arr_grid));

    // ---- Lanes background ------------------------------------------------------
    let p = ui.painter_at(lanes);
    p.rect_filled(lanes, CornerRadius::ZERO, win_bg());
    let mut b = (g.scroll / grid).floor() * grid;
    while g.to_x(b) < lanes.max.x {
        let x = g.to_x(b);
        let c = if (b / bpb).fract().abs() < 1e-9 {
            gray(70)
        } else if b.fract().abs() < 1e-9 {
            gray(48)
        } else {
            gray(38)
        };
        p.line_segment([pos2(x, lanes.min.y), pos2(x, lanes.max.y)], Stroke::new(1.0, c));
        b += grid;
    }
    if app.project.loop_enabled {
        let r = Rect::from_min_max(pos2(g.to_x(app.project.loop_start), lanes.min.y), pos2(g.to_x(app.project.loop_end), lanes.max.y));
        p.rect_filled(r, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(255, 160, 40, 12));
    }

    // Whole-lanes interaction (registered first so clip widgets sit on top).
    let lr = ui.interact(lanes, ui.scope_id().with("arr_lanes"), Sense::click_and_drag());

    // ---- Rows ----------------------------------------------------------------------
    let first = g.row_at(body.min.y);
    let last = g.row_at(body.max.y).min(g.rows.len());
    let n0 = app.project.tracks.len();
    for k in first..=last {
        if app.project.tracks.len() != n0 {
            // A header menu added/removed tracks: row indices are stale, redraw next frame.
            ui.ctx().request_repaint();
            ui.allocate_rect(full, Sense::hover());
            return;
        }
        if k == g.rows.len() {
            drop_zone_row(ui, &g, k);
            continue;
        }
        let t = g.rows[k];
        lane_row(app, ui, &g, k, t);
        header(app, ui, &g, k, t);
    }
    if app.project.tracks.len() != n0 {
        ui.ctx().request_repaint();
        ui.allocate_rect(full, Sense::hover());
        return;
    }

    // Empty header column below the last track: deselect everything.
    let y_end = g.lane(g.rows.len()).min.y.max(body.min.y);
    if y_end < body.max.y {
        let empty = Rect::from_min_max(pos2(lanes.max.x, y_end), pos2(body.max.x, body.max.y));
        if ui.interact(empty, ui.scope_id().with("arr_head_empty"), Sense::click()).clicked() {
            app.deselect_all();
        }
    }
    lanes_interaction(app, ui, &g, &lr);
    update_gesture(app, ui, &g);
    master_row(
        app,
        ui,
        Rect::from_min_max(pos2(full.min.x, body.max.y), pos2(lanes_x1, full.max.y)),
        Rect::from_min_max(pos2(lanes_x1 + 1.0, body.max.y), full.max),
        io_w,
    );
    os_file_drop(app, ui, &g);

    // ---- Overlays --------------------------------------------------------------------
    let p = ui.painter_at(lanes);
    if let Some(ts) = app.time_sel {
        let (x1, x2) = (g.to_x(ts.start), g.to_x(ts.end).max(g.to_x(ts.start) + 1.0));
        for (k, &t) in g.rows.iter().enumerate() {
            if (ts.t0..=ts.t1).contains(&t) {
                let r = g.lane(k);
                p.rect_filled(
                    Rect::from_min_max(pos2(x1, r.min.y), pos2(x2, r.max.y + 1.0)),
                    CornerRadius::ZERO,
                    Color32::from_white_alpha(38),
                );
            }
        }
    }
    match ui.data(|m| m.get_temp::<Gesture>(gesture_id())) {
        Some(Gesture::Move(d)) => {
            if let Some(pos) = ui.ctx().pointer_latest_pos() {
                for (id, t, start) in move_targets(app, &g, &d, pos) {
                    let Some(&(_, _, _, len, _)) = d.clips.iter().find(|c| c.0 == id) else { continue };
                    let Some(k) = g.rows.iter().position(|&r| r == t) else { continue };
                    let lane = g.lane(k);
                    let r = Rect::from_min_max(pos2(g.to_x(start), lane.min.y + 1.0), pos2(g.to_x(start + len), lane.max.y - 1.0));
                    p.rect_filled(r, CornerRadius::same(3), Color32::from_white_alpha(30));
                    p.rect_stroke(r, CornerRadius::same(3), Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Inside);
                }
            }
        }
        Some(Gesture::Tracks { .. }) => {
            if let Some(pos) = ui.ctx().pointer_latest_pos() {
                let (_, mark) = track_drop_target(app, &g, pos.y);
                let hp = ui.painter_at(body);
                let mark = Rect::from_min_max(pos2(body.min.x, mark.min.y), pos2(body.max.x, mark.max.y));
                if mark.height() > 3.0 {
                    hp.rect_stroke(mark, CornerRadius::same(2), Stroke::new(2.0, ACCENT), egui::StrokeKind::Inside);
                } else {
                    hp.line_segment([mark.left_center(), mark.right_center()], Stroke::new(2.0, ACCENT));
                }
            }
        }
        _ => {}
    }
    // Playhead over lanes + ruler.
    let x = g.to_x(app.status.beat_position);
    if x >= lanes.min.x && x <= lanes.max.x {
        ui.painter().line_segment([pos2(x, full.min.y), pos2(x, full.max.y)], Stroke::new(1.5, Color32::WHITE));
    }
    // Follow the playhead while playing.
    if app.status.playing {
        let view_beats = lanes.width() as f64 / zoom;
        let pos = app.status.beat_position;
        if pos > g.scroll + view_beats * 0.9 || pos < g.scroll {
            app.arr_scroll = (pos - view_beats * 0.1).max(0.0) as f32;
        }
    }
    ui.allocate_rect(full, Sense::hover());
}

/// Wheel: vertical scroll, Shift = horizontal (egui maps it to x), Ctrl = zoom
/// around the pointer (egui reports it as zoom_delta), Alt = track heights.
fn wheel(app: &mut DawApp, ui: &mut egui::Ui, full: Rect, x0: f32) {
    if !ui.rect_contains_pointer(full) {
        return;
    }
    let (zoom_d, delta, alt, pointer) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta, i.modifiers.alt, i.pointer.hover_pos()));
    let pointer = pointer.unwrap_or(full.center());
    if zoom_d != 1.0 {
        let px = (pointer.x - x0) as f64;
        let beat = app.arr_scroll as f64 + px / app.arr_zoom as f64;
        app.arr_zoom = (app.arr_zoom * zoom_d).clamp(1.5, 600.0);
        app.arr_scroll = (beat - px / app.arr_zoom as f64).max(0.0) as f32;
    }
    if delta == egui::Vec2::ZERO {
        return;
    }
    if alt {
        let dy = if delta.y != 0.0 { delta.y } else { delta.x };
        app.zoom_track_heights(1.0 + (dy * 0.004).clamp(-0.5, 0.5));
    } else {
        app.arr_scroll = (app.arr_scroll - delta.x / app.arr_zoom).max(0.0);
        app.arr_vscroll = (app.arr_vscroll - delta.y).max(0.0);
    }
    // Don't let anything else (e.g. a parent ScrollArea) scroll as well.
    ui.input_mut(|i| i.smooth_scroll_delta = egui::Vec2::ZERO);
}

fn ruler(app: &mut DawApp, ui: &mut egui::Ui, g: &Geo, ruler: Rect, bpb: f64) {
    let resp = ui.interact(ruler, ui.scope_id().with("ruler"), Sense::click_and_drag());
    let p = ui.painter_at(ruler);
    p.rect_filled(ruler, CornerRadius::ZERO, panel());
    let band = Rect::from_min_max(ruler.min, pos2(ruler.max.x, ruler.min.y + 12.0));
    let (ls, le) = (app.project.loop_start, app.project.loop_end);
    let brace = Rect::from_min_max(pos2(g.to_x(ls), band.min.y + 2.0), pos2(g.to_x(le).max(g.to_x(ls) + 4.0), band.max.y));
    let col = if app.project.loop_enabled { shade(ACCENT, 0.9) } else { gray(90) };
    p.rect_filled(brace, CornerRadius::same(2), col);
    // Corner handles: drag to adjust the loop start / end.
    for x in [brace.min.x, brace.max.x] {
        p.rect_filled(
            Rect::from_center_size(pos2(x, brace.center().y), vec2(3.0, brace.height() + 2.0)),
            CornerRadius::ZERO,
            shade(col, 1.3),
        );
    }
    let edge_at = |pos: Pos2| -> Option<bool> {
        if !band.expand(2.0).contains(pos) {
            return None;
        }
        let (dl, dr) = ((pos.x - brace.min.x).abs(), (pos.x - brace.max.x).abs());
        if dl.min(dr) > 5.0 { None } else { Some(dl <= dr) }
    };
    if let Some(h) = resp.hover_pos()
        && edge_at(h).is_some()
    {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    if let Some(ts) = app.time_sel.filter(|t| t.len() > 1e-9) {
        let r = Rect::from_min_max(pos2(g.to_x(ts.start), ruler.max.y - 3.0), pos2(g.to_x(ts.end), ruler.max.y));
        p.rect_filled(r, CornerRadius::ZERO, Color32::from_white_alpha(120));
    }
    let bar_px = bpb * g.zoom;
    let label_every = ((50.0 / bar_px).ceil() as i64).max(1);
    let mut bar = (g.scroll / bpb).floor().max(0.0) as i64;
    while g.to_x(bar as f64 * bpb) < ruler.max.x {
        let x = g.to_x(bar as f64 * bpb);
        if bar % label_every == 0 {
            p.line_segment([pos2(x, ruler.min.y + 14.0), pos2(x, ruler.max.y)], Stroke::new(1.0, gray(140)));
            p.text(pos2(x + 3.0, ruler.min.y + 14.0), Align2::LEFT_TOP, format!("{}", bar + 1), FontId::proportional(11.0), gray(200));
        }
        bar += 1;
    }
    // Drag a brace corner = resize, the brace body = move, elsewhere = draw a new range.
    // None of these switch looping on (click the brace or Ctrl+L for that).
    if resp.drag_started()
        && let Some(pos) = ui.input(|i| i.pointer.press_origin())
    {
        app.begin_gesture();
        let b = g.beat_at(pos.x);
        let gst = if let Some(start) = edge_at(pos) {
            Gesture::LoopEdge { start }
        } else if band.contains(pos) && brace.contains(pos) {
            Gesture::LoopMove { grab: b - ls }
        } else {
            Gesture::LoopNew { anchor: g.snap(b) }
        };
        ui.data_mut(|d| d.insert_temp(gesture_id(), gst));
    }
    if resp.clicked()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        if band.contains(pos) && brace.contains(pos) {
            let on = !app.project.loop_enabled;
            app.edit(|p| p.loop_enabled = on);
            app.send_loop();
        } else {
            app.set_playhead(g.snap(g.beat_at(pos.x)));
        }
    }
    resp.context_menu(|ui| grid_menu(ui, &mut app.arr_grid));
}

fn drop_zone_row(ui: &mut egui::Ui, g: &Geo, k: usize) {
    let r = g.lane(k);
    ui.painter_at(g.lanes).text(
        r.left_center() + vec2(8.0, 0.0),
        Align2::LEFT_CENTER,
        "Drop a sample here to add a track · right-click to add tracks",
        FontId::proportional(11.0),
        gray(90),
    );
}

fn clip_at(app: &DawApp, t: usize, beat: f64) -> Option<u64> {
    app.project.tracks.get(t)?.arrangement.iter().rev().find(|a| a.start <= beat && beat < a.start + a.clip.length).map(|a| a.clip.id)
}

fn lane_row(app: &mut DawApp, ui: &mut egui::Ui, g: &Geo, k: usize, t: usize) {
    let lane = g.lane(k);
    let painter = ui.painter_at(lane.intersect(g.lanes));
    let track = &app.project.tracks[t];
    let selected = app.is_track_selected(t);
    let collapsed = track.collapsed;
    if track.kind == TrackKind::Group {
        painter.rect_filled(lane, CornerRadius::ZERO, rgb(track.color).gamma_multiply(0.10));
        // Overview of the children's clips, one thin band per child track.
        let kids: Vec<usize> = (t + 1..app.project.subtree_end(t)).filter(|&c| app.project.tracks[c].kind != TrackKind::Group).collect();
        let n = kids.len().max(1) as f32;
        let h = ((lane.height() - 6.0) / n).max(2.0);
        for (j, &c) in kids.iter().enumerate() {
            let y = lane.min.y + 3.0 + j as f32 * h;
            for a in &app.project.tracks[c].arrangement {
                let (x1, x2) = (g.to_x(a.start), g.to_x(a.start + a.clip.length));
                if x2 < lane.min.x || x1 > lane.max.x {
                    continue;
                }
                let r = Rect::from_min_max(pos2(x1, y), pos2(x2.max(x1 + 2.0), y + (h - 1.0).max(1.0)));
                let col = if a.clip.muted { disabled(rgb(a.clip.color)) } else { shade(rgb(a.clip.color), 0.8) };
                painter.rect_filled(r, CornerRadius::same(1), col);
                painter.rect_stroke(r, CornerRadius::same(1), Stroke::new(1.0, Color32::from_black_alpha(90)), egui::StrokeKind::Inside);
            }
        }
        if selected {
            painter.rect_filled(lane, CornerRadius::ZERO, Color32::from_white_alpha(8));
        }
        painter.line_segment([lane.left_bottom(), lane.right_bottom()], Stroke::new(1.0, gray(20)));
        return;
    }
    painter.rect_filled(lane, CornerRadius::ZERO, if selected { Color32::from_white_alpha(10) } else { Color32::from_black_alpha(25) });
    painter.line_segment([lane.left_bottom(), lane.right_bottom()], Stroke::new(1.0, gray(20)));

    let moving: Vec<u64> = match ui.data(|m| m.get_temp::<Gesture>(gesture_id())) {
        Some(Gesture::Move(d)) => d.clips.iter().map(|c| c.0).collect(),
        _ => Vec::new(),
    };
    let n = app.project.tracks[t].arrangement.len();
    for i in 0..n {
        let a = &app.project.tracks[t].arrangement[i];
        let (x1, x2) = (g.to_x(a.start), g.to_x(a.start + a.clip.length));
        if x2 < lane.min.x || x1 > lane.max.x {
            continue;
        }
        let id = a.clip.id;
        let r = Rect::from_min_max(pos2(x1, lane.min.y + 1.0), pos2(x2.max(x1 + 3.0), lane.max.y - 1.0));
        let sel = app.arr_sel.contains(&id) || app.selected_clip == Some(ClipRef::Arr { track: t, index: i });
        let p = &painter;
        draw_clip(p, r, &a.clip, sel, app.cache.peek(sample_key(&a.clip)).map(|b| &**b), g.zoom, app.project.tempo_bpm, collapsed);
        if moving.contains(&id) {
            p.rect_filled(r, CornerRadius::same(3), Color32::from_black_alpha(110));
        }

        // Title bar = move, edges = trim. The body belongs to the lane (time selection).
        let vis = r.intersect(g.lanes).intersect(g.body);
        if vis.width() <= 0.0 || vis.height() <= 0.0 {
            continue;
        }
        let ew = EDGE.min(r.width() / 3.0);
        let redge = Rect::from_min_max(pos2(r.max.x - ew, r.min.y), r.max).intersect(vis);
        let ledge = Rect::from_min_max(r.min, pos2(r.min.x + ew, r.max.y)).intersect(vis);
        let title = Rect::from_min_max(pos2(r.min.x + ew, r.min.y), pos2(r.max.x - ew, r.min.y + TITLE_H.min(r.height()))).intersect(vis);
        let tr = ui.interact(title, ui.scope_id().with(("aclip_title", id)), Sense::click_and_drag());
        let er = ui.interact(redge, ui.scope_id().with(("aclip_edge", id)), Sense::drag());
        let el = ui.interact(ledge, ui.scope_id().with(("aclip_ledge", id)), Sense::drag());
        if er.hovered() || er.dragged() || el.hovered() || el.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        } else if tr.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        let mode = ui.input(|i| SelMode::from_mods(&i.modifiers));
        if tr.clicked() {
            app.select_arr_clip_mode(id, mode);
            app.time_sel = None;
            app.playhead_to_clip(id);
        }
        if tr.double_clicked() {
            app.select_arr_clip_mode(id, SelMode::Single);
            app.start_rename_clip(id, false);
        }
        if tr.drag_started()
            && let Some(pos) = ui.input(|i| i.pointer.press_origin())
        {
            if !app.arr_sel.contains(&id) {
                app.select_arr_clip_mode(id, mode);
            }
            start_move(app, ui, g, pos, true, false);
        }
        if app.renaming.as_ref().is_some_and(|r| r.clip && r.id == id && !r.in_detail) {
            let er = Rect::from_min_size(
                pos2(r.min.x.max(vis.min.x), r.min.y),
                vec2((r.width()).max(60.0).min(g.lanes.max.x - r.min.x.max(vis.min.x)), 16.0),
            );
            super::rename_editor(app, ui, er, id, true, false);
        }
        for (resp, left) in [(&er, false), (&el, true)] {
            if resp.drag_started() {
                app.begin_gesture();
                app.select_arr_clip(id, false);
                let a = app.project.tracks[t].arrangement[i].clone();
                let fpb = (app.fpb_fn())(&a.clip);
                let total = app.clip_total_frames(&a.clip);
                ui.data_mut(|m| m.insert_temp(gesture_id(), Gesture::Resize { orig: Box::new(a), left, fpb, total }));
            }
        }
        tr.context_menu(|ui| {
            if !app.arr_sel.contains(&id) {
                app.select_arr_clip(id, false);
            }
            selection_menu(app, ui);
        });
    }
}

/// Begin moving the selected clips (`arr_sel`) with the pointer at `pos`.
fn start_move(app: &mut DawApp, ui: &mut egui::Ui, g: &Geo, pos: Pos2, checkpoint: bool, from_selection: bool) {
    let p = &app.project;
    let clips: Vec<(u64, usize, f64, f64, bool)> = app
        .arr_sel
        .iter()
        .filter_map(|&id| p.find_arrangement_clip(id))
        .map(|(t, i)| {
            let a = &p.tracks[t].arrangement[i];
            (a.clip.id, t, a.start, a.clip.length, matches!(a.clip.content, ClipContent::Audio { .. }))
        })
        .collect();
    if clips.is_empty() {
        return;
    }
    let anchor_beat = g.beat_at(pos.x);
    // Snap relative to the clip under the pointer (else the earliest one).
    let primary_start = g
        .track_at(pos.y)
        .and_then(|t| clips.iter().find(|c| c.1 == t && c.2 <= anchor_beat && anchor_beat < c.2 + c.3))
        .or_else(|| clips.iter().min_by(|a, b| a.2.total_cmp(&b.2)))
        .map_or(0.0, |c| c.2);
    let d = MoveDrag { clips, anchor_beat, anchor_row: g.row_at_clamped(pos.y), primary_start, checkpoint, from_selection };
    ui.data_mut(|m| m.insert_temp(gesture_id(), Gesture::Move(d)));
}

/// Destinations for a move drag at pointer `pos`: (id, track, start).
fn move_targets(app: &DawApp, g: &Geo, d: &MoveDrag, pos: Pos2) -> Vec<(u64, usize, f64)> {
    let raw = g.beat_at(pos.x) - d.anchor_beat;
    let mut delta = g.snap(d.primary_start + raw) - d.primary_start;
    let min_start = d.clips.iter().map(|c| c.2).fold(f64::MAX, f64::min);
    delta = delta.max(-min_start);
    let dr = g.row_at_clamped(pos.y) as isize - d.anchor_row as isize;
    let last = g.rows.len().saturating_sub(1) as isize;
    d.clips
        .iter()
        .map(|&(id, t, start, _, audio)| {
            let dest = g
                .rows
                .iter()
                .position(|&r| r == t)
                .map(|k| g.rows[(k as isize + dr).clamp(0, last) as usize])
                .filter(|&nt| {
                    let kind = app.project.tracks[nt].kind;
                    if audio { kind == TrackKind::Audio } else { kind == TrackKind::Midi }
                })
                .unwrap_or(t);
            (id, dest, start + delta)
        })
        .collect()
}

/// Where a header drag at `y` would drop, plus the marker to draw
/// (thin rect = insertion line, tall rect = "into this group").
fn track_drop_target(app: &DawApp, g: &Geo, y: f32) -> (TrackDrop, Rect) {
    let k = g.row_at(y);
    let line = |yy: f32| Rect::from_min_max(pos2(g.lanes.min.x, yy - 1.0), pos2(g.body.max.x, yy + 1.0));
    if k >= g.rows.len() {
        return (TrackDrop::Before(None), line(g.row_y(g.rows.len())));
    }
    let t = g.rows[k];
    let r = g.lane(k);
    let f = (y - r.min.y) / r.height().max(1.0);
    let tr = &app.project.tracks[t];
    if tr.kind == TrackKind::Group && (0.25..0.75).contains(&f) {
        return (TrackDrop::Into(tr.id), Rect::from_min_max(pos2(g.lanes.min.x, r.min.y), pos2(g.body.max.x, r.max.y)));
    }
    if f < 0.5 {
        (TrackDrop::Before(Some(tr.id)), line(r.min.y))
    } else {
        let next = g.rows.get(k + 1).map(|&n| app.project.tracks[n].id);
        (TrackDrop::Before(next), line(r.max.y + 1.0))
    }
}

// Header layout (Live 12): left = track-colour title area with the collapse
// button and the name; middle = In/Out (Ctrl+Alt+I); right = two rows of boxes:
//   [activator #][S][arm]
//   [ volume    ][ pan  ]
// Pan = volume width; solo + arm (equal) span one knob.
const BOX_H: f32 = 15.0;
const ACT_W: f32 = 44.0;
const GAP: f32 = 3.0;
const SOLO_W: f32 = (ACT_W - GAP) * 0.5;
const ARM_W: f32 = SOLO_W;
const PAN_W: f32 = ACT_W;
const RIGHT_W: f32 = ACT_W + PAN_W + GAP + 16.0;

/// In/Out column width: shrinks with the window (narrow arrangement).
pub fn io_width(full_w: f32) -> f32 {
    super::io::IO_W.min(((full_w - HEAD_W) * 0.14).max(64.0))
}

fn header(app: &mut DawApp, ui: &mut egui::Ui, g: &Geo, k: usize, t: usize) {
    let lane = g.lane(k);
    let head = Rect::from_min_max(pos2(g.lanes.max.x + 1.0, lane.min.y), pos2(g.body.max.x, lane.max.y));
    let clip = head.intersect(g.body);
    if clip.height() <= 0.0 {
        return;
    }
    let painter = ui.painter_at(clip);
    let track = &app.project.tracks[t];
    let color = rgb(track.color);
    let is_group = track.kind == TrackKind::Group;
    let selected = app.is_track_selected(t);
    let (mute, solo, arm, folded, collapsed, id) = (track.mute, track.solo, track.arm, track.folded, track.collapsed, track.id);
    let name = app.project.track_label(t);

    // Ancestor colour strips (Live shows the group's colour down the left of its children).
    let mut chain = Vec::new();
    let mut cur = t;
    while let Some(pi) = app.project.parent_index(cur) {
        chain.push(pi);
        cur = pi;
    }
    let depth = chain.len();
    painter.rect_filled(head, CornerRadius::ZERO, gray(30));
    for (d, &a) in chain.iter().rev().enumerate() {
        let x = head.min.x + d as f32 * INDENT;
        painter.rect_filled(
            Rect::from_min_size(pos2(x, head.min.y), vec2(INDENT - 3.0, head.height() + 1.0)),
            CornerRadius::ZERO,
            rgb(app.project.tracks[a].color),
        );
    }
    let inner = Rect::from_min_max(pos2(head.min.x + depth as f32 * INDENT, head.min.y), head.max);
    let split = (inner.max.x - RIGHT_W).max(inner.min.x + 40.0 + g.io_w);
    // In/Out sits between the coloured name area and the buttons.
    let io = Rect::from_min_max(pos2(split - g.io_w, inner.min.y), pos2(split, inner.max.y));
    let left = Rect::from_min_max(inner.min, pos2(io.min.x, inner.max.y));
    let right = Rect::from_min_max(pos2(split, inner.min.y), inner.max);
    let title_bg = if selected { color } else { shade(color, 0.72) };
    painter.rect_filled(left, CornerRadius::ZERO, title_bg);
    painter.rect_filled(right, CornerRadius { nw: 0, sw: 0, ne: 2, se: 2 }, if selected { gray(74) } else { gray(50) });
    let text_col = text_on(title_bg);

    // Top-left: ⊙ collapse button, (group fold arrow), name.
    let row1 = inner.min.y + if collapsed { ((inner.height() - BOX_H) * 0.5).max(1.0) } else { 3.0 };
    let cbtn = Rect::from_min_size(pos2(left.min.x + 3.0, row1 - 1.0), vec2(16.0, BOX_H + 2.0));
    draw_collapse_button(&painter, cbtn.center(), collapsed, text_col);
    let mut x = cbtn.max.x + 2.0;
    let fold_zone = Rect::from_min_size(pos2(x - 2.0, row1 - 1.0), vec2(16.0, BOX_H + 2.0));
    if is_group {
        super::session::draw_fold_arrow(&painter, fold_zone.center(), folded, text_col);
        x += 15.0;
    }
    let name_rect = Rect::from_min_max(pos2(x, row1 - 1.0), pos2(left.max.x - 3.0, row1 + BOX_H + 1.0));
    let renaming = app.renaming.as_ref().is_some_and(|r| r.id == id && !r.clip);
    if !renaming {
        painter.with_clip_rect(name_rect.intersect(clip)).text(
            name_rect.left_center(),
            Align2::LEFT_CENTER,
            &name,
            FontId::proportional(12.0),
            text_col,
        );
    }
    // Slim meter at the right edge.
    let m = app.track_meters.get(t).copied().unwrap_or([0.0; 2]);
    let lvl = ((daw_project::gain_to_db(m[0].max(m[1])) + 60.0) / 60.0).clamp(0.0, 1.0);
    let mr = Rect::from_min_max(pos2(inner.max.x - 4.0, inner.min.y + 2.0), pos2(inner.max.x - 1.0, inner.max.y - 2.0));
    painter.rect_filled(mr, CornerRadius::ZERO, gray(24));
    painter.rect_filled(Rect::from_min_max(pos2(mr.min.x, mr.max.y - mr.height() * lvl), mr.max), CornerRadius::ZERO, PLAY_GREEN);

    let hr = ui.interact(clip, ui.scope_id().with(("arr_head", t)), Sense::click_and_drag());
    if hr.clicked()
        && let Some(pos) = hr.interact_pointer_pos()
    {
        if cbtn.contains(pos) {
            app.toggle_collapse(t);
            app.touched_track = Some(t);
        } else if is_group && fold_zone.contains(pos) {
            app.toggle_fold(t);
        } else {
            let mode = ui.input(|i| SelMode::from_mods(&i.modifiers));
            app.select_track_header(t, mode);
            app.arr_sel.clear();
            app.time_sel = None;
            app.resync_selected_clip();
        }
    }
    if hr.double_clicked() && hr.interact_pointer_pos().is_some_and(|p| !cbtn.contains(p) && !(is_group && fold_zone.contains(p))) {
        app.select_track_header(t, SelMode::Single);
        app.start_rename(t);
    }
    if hr.drag_started() {
        // Drag the header selection if this track is part of it, else just this track.
        if !app.track_sel.contains(&t) {
            app.select_track_header(t, SelMode::Single);
        }
        let ids: Vec<u64> = app.track_sel.iter().filter_map(|&i| app.project.tracks.get(i).map(|x| x.id)).collect();
        ui.data_mut(|m| m.insert_temp(gesture_id(), Gesture::Tracks { ids }));
    }
    if hr.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    if let Some(item) = hr.dnd_release_payload::<DragItem>()
        && let Some(def) = super::instrument_for(&item)
    {
        app.set_instrument(t, def);
    }
    hr.context_menu(|ui| super::session::track_menu(app, ui, t));
    if renaming {
        super::rename_editor(app, ui, name_rect.intersect(clip), id, false, false);
    }
    if g.io_w > 0.0 {
        super::io::io_compact(app, ui, t, io, io.intersect(clip));
        if app.project.tracks.len() <= t {
            return;
        }
    }

    // Right region, row 1: activator · S · arm (collapsed: activator · S only).
    let bx = right.min.x + 4.0;
    let act = Rect::from_min_size(pos2(bx, row1), vec2(ACT_W, BOX_H));
    let solo_r = Rect::from_min_size(pos2(act.max.x + GAP, row1), vec2(SOLO_W, BOX_H));
    let arm_r = Rect::from_min_size(pos2(solo_r.max.x + GAP, row1), vec2(ARM_W, BOX_H));
    if !clip.contains_rect(act) {
        return;
    }
    let act_col = if app.solo_silenced.get(t).copied().unwrap_or(false) { solo_dimmed(ACCENT) } else { ACCENT };
    if rect_toggle(ui, act, ui.scope_id().with(("hact", id)), &format!("{}", t + 1), !mute, act_col, "Track activator (mute)").clicked() {
        app.toggle_track_flag(t, TrackFlag::Mute, false);
    }
    let excl = !ui.input(|i| i.modifiers.command);
    if clip.contains_rect(solo_r)
        && rect_toggle(ui, solo_r, ui.scope_id().with(("hsolo", id)), "S", solo, Color32::from_rgb(80, 160, 255), "Solo (Ctrl: add)")
            .clicked()
    {
        app.toggle_track_flag(t, TrackFlag::Solo, excl);
    }
    if collapsed {
        return;
    }
    // "No Input" hides Arm; a selected track's Arm arms the whole selection, Ctrl = just this one.
    if !is_group
        && app.project.tracks[t].has_input()
        && clip.contains_rect(arm_r)
        && rect_toggle(
            ui,
            arm_r,
            ui.scope_id().with(("harm", id)),
            "REC",
            arm,
            REC_RED,
            "Arm for recording (all selected tracks; Ctrl: just this one)",
        )
        .clicked()
    {
        app.click_arm(t, !excl);
    }
    // Row 2: volume under the activator, pan under solo.
    let row2 = row1 + BOX_H + GAP;
    let vr = Rect::from_min_size(pos2(bx, row2), vec2(ACT_W, BOX_H));
    let pr = Rect::from_min_size(pos2(vr.max.x + GAP, row2), vec2(PAN_W, BOX_H));
    if clip.contains_rect(vr) && clip.contains_rect(pr) {
        let mut db = app.project.tracks[t].gain_db;
        let (r, changed) = volume_box(ui, vr, ui.scope_id().with(("hvol", id)), &mut db);
        if r.drag_started() || r.double_clicked() {
            app.begin_gesture();
        }
        if changed {
            app.set_track_gain(t, db);
        }
        let mut pan = app.project.tracks[t].pan;
        let (r, changed) = pan_box(ui, pr, ui.scope_id().with(("hpan", id)), &mut pan);
        if r.drag_started() || r.double_clicked() {
            app.begin_gesture();
        }
        if changed {
            app.set_track_pan(t, pan);
        }
    }
    if row2 + BOX_H + 14.0 <= inner.max.y && app.track_status.get(t).is_some_and(|s| s.arrangement_off) {
        painter.text(pos2(left.min.x + 4.0, row2 + 2.0), Align2::LEFT_TOP, "session clip", FontId::proportional(10.0), text_col);
    }
}

/// Master track row, pinned under the lanes (Live). Collapsed by default; no
/// activator / solo / arm; volume and pan are always shown.
fn master_row(app: &mut DawApp, ui: &mut egui::Ui, lanes: Rect, head: Rect, io_w: f32) {
    let p = ui.painter();
    p.rect_filled(lanes, CornerRadius::ZERO, gray(22));
    p.line_segment([lanes.left_top(), pos2(head.max.x, lanes.min.y)], Stroke::new(2.0, gray(14)));
    let collapsed = app.project.master_collapsed;
    let split = (head.max.x - RIGHT_W).max(head.min.x + 40.0 + io_w);
    let left = Rect::from_min_max(pos2(head.min.x, head.min.y + 2.0), pos2(split - io_w, head.max.y));
    let right = Rect::from_min_max(pos2(split, head.min.y + 2.0), head.max);
    // Name area: click selects the master (device view), right-click = colour.
    let sel = app.master_selected;
    let col = rgb(app.project.master_color);
    let title_bg = if sel { col } else { shade(col, 0.72) };
    p.rect_filled(left, CornerRadius::ZERO, title_bg);
    super::add_master_zone(ui, head);
    let hr = ui.interact(left, ui.scope_id().with("master_head"), Sense::click());
    if hr.clicked() {
        app.select_master();
    }
    hr.context_menu(|ui| super::session::master_menu(app, ui));
    if io_w > 0.0 {
        let io = Rect::from_min_max(pos2(left.max.x, head.min.y + 2.0), pos2(split, head.max.y));
        ui.painter().rect_filled(io, CornerRadius::ZERO, gray(38));
        super::io::master_io_compact(app, ui, io);
    }
    let p = ui.painter();
    p.rect_filled(right, CornerRadius { nw: 0, sw: 0, ne: 2, se: 2 }, gray(if sel { 74 } else { 50 }));
    let row1 = left.min.y + if collapsed { ((left.height() - BOX_H) * 0.5).max(0.0) } else { 3.0 };
    let cbtn = Rect::from_min_size(pos2(left.min.x + 3.0, row1 - 1.0), vec2(16.0, BOX_H + 2.0));
    let tc = text_on(title_bg);
    draw_collapse_button(p, cbtn.center(), collapsed, tc);
    p.text(pos2(cbtn.max.x + 2.0, row1 + BOX_H * 0.5), Align2::LEFT_CENTER, "Master", FontId::proportional(12.0), tc);
    let r = ui.interact(cbtn, ui.scope_id().with("master_collapse"), Sense::click());
    if r.clicked() {
        app.project.master_collapsed = !collapsed;
        app.dirty = true;
    }
    // Master meter in the lane (expanded) and at the right edge.
    let lvl = |v: f32| ((daw_project::gain_to_db(v) + 60.0) / 60.0).clamp(0.0, 1.0);
    let mr = Rect::from_min_max(pos2(head.max.x - 4.0, head.min.y + 4.0), pos2(head.max.x - 1.0, head.max.y - 2.0));
    p.rect_filled(mr, CornerRadius::ZERO, gray(24));
    let m = app.meter[0].max(app.meter[1]);
    p.rect_filled(Rect::from_min_max(pos2(mr.min.x, mr.max.y - mr.height() * lvl(m)), mr.max), CornerRadius::ZERO, PLAY_GREEN);
    let bx = right.min.x + 4.0;
    // Row 1: volume · blue Cue; pan below them once the row is tall enough.
    let vr = Rect::from_min_size(pos2(bx, row1), vec2(ACT_W, BOX_H));
    let cr = Rect::from_min_size(pos2(vr.max.x + GAP, row1), vec2(PAN_W, BOX_H));
    let pr = Rect::from_min_size(pos2(bx, row1 + BOX_H + GAP), vec2(ACT_W, BOX_H));
    let mut cue = app.project.cue_gain_db;
    let (r, changed) = cue_box(ui, cr, ui.scope_id().with("mcue"), &mut cue);
    if r.drag_started() || r.double_clicked() {
        app.begin_gesture();
    }
    if changed {
        app.project.cue_gain_db = cue;
        app.dirty = true;
        app.send_now(daw_engine::Command::SetCueGain(daw_project::db_to_gain(cue)));
    }
    let mut db = app.project.master_gain_db;
    let (r, changed) = volume_box(ui, vr, ui.scope_id().with("mvol"), &mut db);
    if r.drag_started() || r.double_clicked() {
        app.begin_gesture();
    }
    if changed {
        app.project.master_gain_db = db;
        app.dirty = true;
        app.send_now(daw_engine::Command::SetMasterGain(daw_project::db_to_gain(db)));
    }
    if collapsed || pr.max.y > head.max.y - 2.0 {
        return;
    }
    let mut pan = app.project.master_pan;
    let (r, changed) = pan_box(ui, pr, ui.scope_id().with("mpan"), &mut pan);
    if r.drag_started() || r.double_clicked() {
        app.begin_gesture();
    }
    if changed {
        app.project.master_pan = pan;
        app.dirty = true;
        app.send_now(daw_engine::Command::SetMasterPan(pan));
    }
}

/// Clicks / drags / drops on the lanes (clip bodies included).
fn lanes_interaction(app: &mut DawApp, ui: &mut egui::Ui, g: &Geo, lr: &egui::Response) {
    let toggle = ui.input(|i| i.modifiers.shift || i.modifiers.command);
    if lr.drag_started()
        && let Some(pos) = ui.input(|i| i.pointer.press_origin())
        && let Some(t) = g.track_at(pos.y)
    {
        let beat = g.beat_at(pos.x);
        match app.time_sel {
            // Drag inside a selection over a clip: cut that part out and move it.
            Some(ts) if ts.len() > 1e-9 && ts.contains(beat, t) && clip_at(app, t, beat).is_some() => {
                app.split_for_drag(ts);
                start_move(app, ui, g, pos, false, true);
            }
            _ => {
                let a = g.snap(beat);
                app.touched_track = Some(t);
                app.time_sel = Some(TimeSel::new(a, a, t, t));
                app.arr_sel.clear();
                app.resync_selected_clip();
                if !toggle {
                    app.click_track(t, false);
                }
                ui.data_mut(|m| m.insert_temp(gesture_id(), Gesture::Select { anchor: a, track: t }));
            }
        }
    }
    if lr.clicked()
        && let Some(pos) = lr.interact_pointer_pos()
    {
        let beat = g.beat_at(pos.x);
        app.time_sel = None;
        let mode = ui.input(|i| SelMode::from_mods(&i.modifiers));
        match g.track_at(pos.y) {
            Some(t) => {
                match clip_at(app, t, beat) {
                    // Clip body: select it, start point at the clicked spot (Live's insert
                    // marker). Only the title bar jumps to the clip start.
                    Some(id) => {
                        app.select_arr_clip_mode(id, mode);
                        app.set_playhead(g.snap(beat));
                    }
                    None => {
                        app.set_playhead(g.snap(beat));
                        if !toggle {
                            app.arr_sel.clear();
                            app.resync_selected_clip();
                        }
                    }
                }
                if !toggle {
                    app.click_track(t, false);
                }
                app.touched_track = Some(t);
            }
            // Empty space below the tracks: deselect everything, playhead stays.
            None => app.deselect_all(),
        }
    }
    if lr.double_clicked()
        && let Some(pos) = lr.interact_pointer_pos()
        && let Some(t) = g.track_at(pos.y)
        && app.project.tracks[t].kind == TrackKind::Midi
        && clip_at(app, t, g.beat_at(pos.x)).is_none()
    {
        // Double-click an empty MIDI lane: new one-bar clip.
        let bpb = app.project.beats_per_bar as f64;
        let start = (g.beat_at(pos.x) / bpb).floor() * bpb;
        let end = app.project.tracks[t].arrangement.iter().map(|a| a.start).filter(|&s| s > start).fold(start + bpb, f64::min);
        if let Ok(id) = app.edit(|p| p.insert_midi_clip(t, start, end)) {
            app.arr_sel = vec![id];
            app.resync_selected_clip();
        }
    }
    if lr.secondary_clicked()
        && let Some(pos) = lr.interact_pointer_pos()
        && let Some(t) = g.track_at(pos.y)
        && let Some(id) = clip_at(app, t, g.beat_at(pos.x))
        && !app.arr_sel.contains(&id)
    {
        app.select_arr_clip(id, false);
    }
    lr.context_menu(|ui| selection_menu(app, ui));

    // Browser drops.
    if lr.dnd_hover_payload::<DragItem>().is_some()
        && let Some(pos) = ui.ctx().pointer_latest_pos()
    {
        let k = g.row_at(pos.y).min(g.rows.len());
        ui.painter_at(g.lanes).rect_stroke(g.lane(k), CornerRadius::ZERO, Stroke::new(1.0, ACCENT), egui::StrokeKind::Inside);
    }
    if let Some(item) = lr.dnd_release_payload::<DragItem>()
        && let Some(pos) = lr.interact_pointer_pos().or(ui.ctx().pointer_latest_pos())
    {
        let at = g.snap(g.beat_at(pos.x));
        match g.track_at(pos.y) {
            Some(t) => drop_on_lane(app, &item, t, at),
            None => drop_new_track(app, &item, at),
        }
    }
}

/// Context menu for the lanes / clips (acts on the current selection).
fn selection_menu(app: &mut DawApp, ui: &mut egui::Ui) {
    if ui.button("Split               Ctrl+E").clicked() {
        app.split_selection();
        ui.close();
    }
    if ui.button("Consolidate         Ctrl+J").clicked() {
        app.consolidate_selection();
        ui.close();
    }
    if ui.button("Loop Selection      Ctrl+L").clicked() {
        app.loop_selection();
        ui.close();
    }
    if ui.button("Insert MIDI Clip  Ctrl+Shift+M").clicked() {
        app.insert_midi_clip_in_selection();
        ui.close();
    }
    if ui.button("Duplicate           Ctrl+D").clicked() {
        app.duplicate_arrangement_selection();
        ui.close();
    }
    if ui.button("Deactivate / Activate   0").clicked() {
        app.focus_detail = false;
        app.toggle_disable();
        ui.close();
    }
    if ui.button("Delete              Del").clicked() {
        app.delete_arrangement_selection();
        ui.close();
    }
    ui.separator();
    grid_menu(ui, &mut app.arr_grid);
    ui.separator();
    if ui.button("Add Audio Track     Ctrl+T").clicked() {
        app.add_track(TrackKind::Audio);
        ui.close();
    }
    if ui.button("Add MIDI Track  Ctrl+Shift+T").clicked() {
        app.add_track(TrackKind::Midi);
        ui.close();
    }
}

/// Drive the active drag gesture; finish it when the button is released.
fn update_gesture(app: &mut DawApp, ui: &mut egui::Ui, g: &Geo) {
    let Some(gst) = ui.data(|m| m.get_temp::<Gesture>(gesture_id())) else { return };
    let (down, pos) = ui.input(|i| (i.pointer.primary_down(), i.pointer.latest_pos()));
    let Some(pos) = pos else { return };
    match &gst {
        Gesture::Select { anchor, track } => {
            let t = g.rows.get(g.row_at_clamped(pos.y)).copied().unwrap_or(*track);
            let b = g.snap(g.beat_at(pos.x));
            app.time_sel = Some(TimeSel::new(*anchor, b, *track, t));
            if !down && let Some(ts) = app.time_sel {
                if ts.len() < 1e-9 {
                    app.time_sel = None;
                }
                app.set_playhead(ts.start);
            }
        }
        Gesture::Resize { orig, left, fpb, total } => {
            let min_len = if g.snap_on { g.grid.min(orig.clip.length) } else { 1.0 / 64.0 };
            let b = g.snap(g.beat_at(pos.x));
            // Shift on a warped audio clip: stretch sample and clip together.
            let end = orig.start + orig.clip.length;
            let stretched = if ui.input(|i| i.modifiers.shift) {
                let len = if *left { end - b.min(end - min_len) } else { b.max(orig.start + min_len) - orig.start };
                daw_project::stretch_clip(orig, len.max(min_len), *left)
            } else {
                None
            };
            let new = if let Some(st) = stretched {
                st
            } else if *left {
                daw_project::trim_left(orig, b, min_len, *fpb, *total)
            } else {
                daw_project::trim_right(orig, b - orig.start, min_len, *fpb, *total)
            };
            if let Some((t, i)) = app.project.find_arrangement_clip(orig.clip.id) {
                let a = &mut app.project.tracks[t].arrangement[i];
                if *a != new {
                    *a = new;
                    app.needs_compile = true;
                }
            }
            if !down {
                let fpb = app.fpb_fn();
                app.project.overwrite_around(orig.clip.id, &fpb);
                app.needs_compile = true;
                app.resync_selected_clip();
            }
        }
        Gesture::LoopNew { anchor } => {
            let b = g.snap(g.beat_at(pos.x));
            let (s, e) = if b >= *anchor { (*anchor, b) } else { (b, *anchor) };
            let min = if g.snap_on { g.grid } else { 1.0 / 16.0 };
            if e - s >= min && (s, e) != (app.project.loop_start, app.project.loop_end) {
                app.project.loop_start = s;
                app.project.loop_end = e;
                app.send_loop();
            }
        }
        Gesture::LoopMove { grab } => {
            let len = app.project.loop_end - app.project.loop_start;
            let s = g.snap(g.beat_at(pos.x) - grab).max(0.0);
            if (s - app.project.loop_start).abs() > 1e-12 {
                app.project.loop_start = s;
                app.project.loop_end = s + len;
                app.send_loop();
            }
        }
        Gesture::LoopEdge { start } => {
            let b = g.snap(g.beat_at(pos.x));
            let min = if g.snap_on { g.grid } else { 1.0 / 16.0 };
            let (ls, le) = (app.project.loop_start, app.project.loop_end);
            let (ns, ne) = if *start { (b.min(le - min).max(0.0), le) } else { (ls, b.max(ls + min)) };
            if (ns, ne) != (ls, le) {
                app.project.loop_start = ns;
                app.project.loop_end = ne;
                app.send_loop();
            }
        }
        Gesture::Tracks { ids } => {
            if !down {
                let (to, _) = track_drop_target(app, g, pos.y);
                let ids = ids.clone();
                app.drop_tracks(&ids, to);
            }
        }
        Gesture::Move(d) => {
            if !down {
                let targets = move_targets(app, g, d, pos);
                let changed = targets.iter().zip(&d.clips).any(|(a, b)| a.1 != b.1 || (a.2 - b.2).abs() > 1e-9);
                if changed {
                    app.move_arr_clips(&targets, d.checkpoint);
                    if d.from_selection
                        && let Some(ts) = app.time_sel
                    {
                        let delta = targets[0].2 - d.clips[0].2;
                        let (t0, t1) = targets.iter().fold((usize::MAX, 0), |acc, x| (acc.0.min(x.1), acc.1.max(x.1)));
                        app.time_sel = Some(TimeSel { start: ts.start + delta, end: ts.end + delta, t0, t1 });
                    }
                }
                if !d.from_selection {
                    app.time_sel = None;
                }
            }
        }
    }
    if !down {
        ui.data_mut(|m| m.remove::<Gesture>(gesture_id()));
    } else {
        ui.ctx().request_repaint();
    }
}

/// Files dropped from the OS file manager onto the lanes.
fn os_file_drop(app: &mut DawApp, ui: &mut egui::Ui, g: &Geo) {
    let files = ui.input(|i| i.raw.dropped_files.clone());
    if files.is_empty() {
        return;
    }
    let pos = ui.ctx().pointer_latest_pos();
    if pos.is_some_and(|p| !g.lanes.contains(p)) {
        return;
    }
    let mut at = pos.map_or(app.status.beat_position, |p| g.snap(g.beat_at(p.x)));
    let target = pos.and_then(|p| g.track_at(p.y));
    for f in files {
        let path = f.path();
        if path.as_os_str().is_empty() {
            continue;
        }
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("sample").to_string();
        let item = DragItem::Sample { key: path.to_string_lossy().replace('\\', "/"), name };
        match target {
            Some(t) if app.project.tracks[t].kind == TrackKind::Audio => drop_on_lane(app, &item, t, at),
            _ => drop_new_track(app, &item, at),
        }
        // Several files: place them one after another.
        if let Some(c) = app.arr_sel.first().and_then(|&id| app.project.find_arrangement_clip(id)) {
            let a = &app.project.tracks[c.0].arrangement[c.1];
            at = a.start + a.clip.length;
        }
    }
}

fn drop_new_track(app: &mut DawApp, item: &DragItem, at: f64) {
    let DragItem::Sample { key, name } = item else { return };
    let Some(mut clip) = app.audio_clip_for(key, name) else { return };
    let (t, id) = app.edit(|p| {
        let t = p.add_track(TrackKind::Audio);
        p.tracks[t].name = format!("# {name}");
        clip.id = p.alloc_id();
        clip.color = p.new_clip_color(t);
        clip.looping = false;
        let id = clip.id;
        p.tracks[t].arrangement.push(ArrangementClip { start: at, clip });
        (t, id)
    });
    app.clear_structure_selection();
    app.selected_track = t;
    app.arr_sel = vec![id];
    app.resync_selected_clip();
}

fn drop_on_lane(app: &mut DawApp, item: &DragItem, t: usize, at: f64) {
    let kind = app.project.tracks[t].kind;
    if kind == TrackKind::Group {
        return;
    }
    let clip = match item {
        DragItem::Sample { key, name } => {
            if kind != TrackKind::Audio {
                if let Some(def) = super::instrument_for(item) {
                    app.set_instrument(t, def);
                }
                return;
            }
            app.audio_clip_for(key, name)
        }
        DragItem::SessionClip { track, scene } => {
            app.project.tracks.get(*track).and_then(|x| x.slots.get(*scene)).cloned().flatten().filter(|c| {
                let audio = matches!(c.content, ClipContent::Audio { .. });
                audio == (kind == TrackKind::Audio)
            })
        }
        _ => {
            if let Some(def) = super::instrument_for(item) {
                app.set_instrument(t, def);
            }
            return;
        }
    };
    if let Some(mut clip) = clip {
        let fpb = app.fpb_fn();
        let id = app.edit(|p| {
            clip.id = p.alloc_id();
            if matches!(item, DragItem::Sample { .. }) {
                clip.color = p.new_clip_color(t);
            }
            clip.looping = false;
            let id = clip.id;
            // Dropped material overwrites what is underneath (Live).
            p.clear_arrangement_range(t, at, at + clip.length, &fpb);
            let tr = &mut p.tracks[t];
            tr.arrangement.push(ArrangementClip { start: at, clip });
            tr.arrangement.sort_by(|a, b| a.start.total_cmp(&b.start));
            id
        });
        app.arr_sel = vec![id];
        app.time_sel = None;
        app.resync_selected_clip();
    }
}

fn sample_key(c: &Clip) -> &str {
    match &c.content {
        ClipContent::Audio { sample, .. } => sample,
        _ => "",
    }
}

/// Draw a clip with a MIDI or waveform preview (`collapsed`: title bar only).
/// Deactivated clips are drawn grey.
#[allow(clippy::too_many_arguments)]
pub fn draw_clip(
    p: &egui::Painter,
    r: Rect,
    clip: &Clip,
    selected: bool,
    sample: Option<&SampleBuffer>,
    zoom: f64,
    tempo: f64,
    collapsed: bool,
) {
    let c = if clip.muted { disabled(rgb(clip.color)) } else { rgb(clip.color) };
    if collapsed {
        p.rect_filled(r, CornerRadius::same(3), c);
        p.with_clip_rect(r.intersect(p.clip_rect())).text(
            r.left_center() + vec2(4.0, 0.0),
            Align2::LEFT_CENTER,
            &clip.name,
            FontId::proportional(10.0),
            Color32::BLACK,
        );
        if selected {
            p.rect_stroke(r, CornerRadius::same(3), Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Inside);
        }
        return;
    }
    p.rect_filled(r, CornerRadius::same(3), shade(c, 0.45));
    let title = Rect::from_min_size(r.min, vec2(r.width(), 14.0));
    p.rect_filled(title, CornerRadius::same(3), c);
    p.with_clip_rect(title.intersect(p.clip_rect())).text(
        title.left_center() + vec2(4.0, 0.0),
        Align2::LEFT_CENTER,
        &clip.name,
        FontId::proportional(10.0),
        Color32::BLACK,
    );
    if selected {
        p.rect_stroke(r, CornerRadius::same(3), Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Inside);
    }
    let body = Rect::from_min_max(pos2(r.min.x + 1.0, title.max.y + 2.0), pos2(r.max.x - 1.0, r.max.y - 2.0));
    let visible = body.intersect(p.clip_rect());
    if visible.width() <= 0.0 {
        return;
    }
    let ink = shade(c, 1.25);
    match &clip.content {
        ClipContent::Midi { notes } => {
            if notes.is_empty() {
                return;
            }
            let (lo, hi) = notes.iter().fold((127u8, 0u8), |(l, h), n| (l.min(n.pitch), h.max(n.pitch)));
            let span = (hi - lo).max(1) as f32 + 1.0;
            let nh = (body.height() / span).clamp(1.0, 4.0);
            // Playback order: start marker → loop end, then the loop repeated.
            for (l0, l1, u0) in play_segments(clip) {
                let (xs, xe) = (body.min.x + (l0 * zoom) as f32, body.min.x + (l1 * zoom) as f32);
                if xe < visible.min.x || xs > visible.max.x {
                    continue;
                }
                let u1 = u0 + (l1 - l0);
                for n in notes.iter().filter(|n| n.start >= u0 - 1e-9 && n.start < u1) {
                    let x1 = xs + ((n.start - u0) * zoom) as f32;
                    let x2 = (x1 + (n.length.min(u1 - n.start) * zoom) as f32).max(x1 + 1.0);
                    if x2 < visible.min.x || x1 > visible.max.x {
                        continue;
                    }
                    let y = body.max.y - ((n.pitch - lo) as f32 + 1.0) / span * body.height();
                    p.rect_filled(
                        Rect::from_min_max(pos2(x1, y), pos2(x2, y + nh)),
                        CornerRadius::ZERO,
                        if n.muted { shade(c, 0.7) } else { ink },
                    );
                }
            }
        }
        ClipContent::Audio { offset_frames, warp_bpm, .. } => {
            let Some(buf) = sample else {
                p.text(body.center(), Align2::CENTER_CENTER, "loading…", FontId::proportional(10.0), Color32::GRAY);
                return;
            };
            let sr = buf.sample_rate as f64;
            // Unwarped = raw playback (markers ignored), like the engine.
            let map = match warp_bpm {
                Some(b) => daw_core::compile::warp_map(clip, *offset_frames, Some(*b), sr),
                None => daw_engine::WarpMap::from_markers(&[], *offset_frames, sr * 60.0 / tempo.max(1.0)),
            };
            let frame_of = |local: f64| clip.content_at(local).map(|u| map.frame_at(u));
            draw_wave(p, body, visible, buf, zoom, &frame_of, ink);
        }
    }
}

/// Clip playback as (local start, local end, content start) pieces.
pub fn play_segments(clip: &Clip) -> Vec<(f64, f64, f64)> {
    let (a, b) = clip.loop_range();
    let end = clip.length;
    let mut v = Vec::new();
    let (mut local, mut u) = (0.0, clip.wrap(clip.start));
    while local < end - 1e-9 && v.len() < 4096 {
        let l1 = if clip.looping && b > a + 1e-9 { (local + (b - u).max(0.0)).min(end) } else { end };
        v.push((local, l1, u));
        if l1 <= local + 1e-12 {
            // Degenerate loop: stop instead of spinning.
            break;
        }
        local = l1;
        u = a;
    }
    v
}

/// Min/max waveform, one column per pixel, from the precomputed peak blocks.
pub fn draw_wave(
    p: &egui::Painter,
    body: Rect,
    visible: Rect,
    buf: &SampleBuffer,
    px_per_beat: f64,
    frame_of: &dyn Fn(f64) -> Option<f64>,
    ink: Color32,
) {
    if buf.is_empty() || buf.peaks.is_empty() || px_per_beat <= 0.0 {
        return;
    }
    let mid = body.center().y;
    let h = body.height() * 0.5;
    let block = daw_engine::PEAK_BLOCK as f64;
    let total = buf.len() as f64;
    let mut fpp = 1.0;
    let mut x = visible.min.x.floor();
    while x < visible.max.x {
        let l0 = (x - body.min.x) as f64 / px_per_beat;
        let Some(f0) = frame_of(l0) else { break };
        // Frames per pixel from the neighbour (keeps the last value across loop wraps).
        if let Some(f1) = frame_of(l0 + 1.0 / px_per_beat).filter(|f1| *f1 > f0) {
            fpp = f1 - f0;
        }
        x += 1.0;
        if f0 < 0.0 || f0 >= total {
            continue;
        }
        let f1 = (f0 + fpp.max(1.0)).min(total);
        let (mut lo, mut hi) = (0.0f32, 0.0f32);
        if fpp < block * 2.0 {
            // Zoomed in: read raw samples (few per pixel).
            let (a, b) = (f0 as usize, (f1.ceil() as usize).min(buf.len()).max(f0 as usize + 1));
            for &v in &buf.left[a.min(buf.len() - 1)..b.min(buf.len())] {
                lo = lo.min(v);
                hi = hi.max(v);
            }
        } else {
            let (b0, b1) = ((f0 / block) as usize, ((f1 / block).ceil() as usize).min(buf.peaks.len()).max((f0 / block) as usize + 1));
            for &(mn, mx) in &buf.peaks[b0.min(buf.peaks.len() - 1)..b1.min(buf.peaks.len())] {
                lo = lo.min(mn);
                hi = hi.max(mx);
            }
        }
        let x = x - 1.0;
        p.line_segment([pos2(x, mid - hi * h), pos2(x, mid - lo * h + 0.5)], Stroke::new(1.0, ink));
    }
}

pub fn grid_label(g: f64) -> String {
    match g {
        0.0625 => "1/64".into(),
        0.125 => "1/32".into(),
        0.25 => "1/16".into(),
        0.5 => "1/8".into(),
        1.0 => "1/4".into(),
        2.0 => "1/2".into(),
        v => format!("{} bar", v / 4.0),
    }
}
