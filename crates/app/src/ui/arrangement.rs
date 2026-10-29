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

use super::cursors::Cur;
use super::theme::*;
use super::widgets::*;
use crate::app::{ClipRef, DawApp, DragItem, TrackFlag};
use crate::arr_ops::{SelMode, TimeSel};
use daw_engine::SampleBuffer;
use daw_project::{ArrangementClip, Clip, ClipContent, TrackDrop, TrackKind};
use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, pos2, vec2};

/// Live: name 93 + 2 + mixer 119 (+ In/Out 74 + 2) = 290 px.
const HEAD_W: f32 = 214.0;
const NAME_W: f32 = 93.0;
/// Overview 12 + 2 + beat ruler 21 + loop row 22 + 2.
const RULER_H: f32 = 59.0;
/// Bottom time ruler (2 px separator + 21 px).
const TIME_H: f32 = 23.0;
const EDGE: f32 = 6.0;
/// Coarsest arrangement grid: a quarter note (beats).
const ARR_GRID_MAX: f64 = 1.0;
/// Legs of the isosceles right triangles marking loop / selection edges.
const MARK: f32 = 6.0;
/// Clip outline and title/body divider.
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
    // Return rows: pinned above Main (Live), 17 px each, 3 px separator above
    // the first and 2 px between (the last one's is Main's).
    let nret = app.project.returns.len();
    let ret_h = if nret > 0 { 3.0 + nret as f32 * (RET_H + 2.0) - 2.0 } else { 0.0 };
    let master_h = master_h - if nret > 0 { 1.0 } else { 0.0 };
    let body = Rect::from_min_max(
        pos2(full.min.x, full.min.y + RULER_H),
        pos2(full.max.x, (full.max.y - master_h - TIME_H - ret_h).max(full.min.y + RULER_H + 20.0)),
    );
    let lanes = Rect::from_min_max(body.min, pos2(lanes_x1, body.max.y));
    let x0 = full.min.x;

    wheel(app, ui, full, x0);
    // Follow: the playhead runs to the centre, then the song scrolls under it.
    // A manual scroll/zoom pauses it until the next Play.
    let playing = app.status.playing;
    if playing && !app.was_playing {
        app.follow_paused = false;
    }
    app.was_playing = playing;
    if playing && app.follow && !app.follow_paused {
        let half = (lanes.width() * 0.5) as f64 / app.arr_zoom as f64;
        app.arr_scroll = (app.status.beat_position - half).max(0.0) as f32;
    }

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
    // Arrangement grid never gets coarser than a quarter note, however far out.
    // Adaptive grid stops at a quarter note; fixed steps (up to 8 bars) are kept.
    let raw = app.arr_grid.step(zoom, bpb);
    let grid = if matches!(app.arr_grid, crate::arr_ops::GridMode::Fixed(_)) { raw } else { raw.min(ARR_GRID_MAX) }
        * if app.arr_triplet { 2.0 / 3.0 } else { 1.0 };
    app.arr_lanes_w = lanes.width();
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
    locator_area(ui, side);
    overview(app, ui, &g, Rect::from_min_max(full.min, pos2(full.max.x - 10.0, full.min.y + 12.0)));

    // ---- Lanes background: alternating bar shades + grid lines (Live) ------------
    let p = ui.painter_at(lanes);
    paint_bars(&p, lanes, &g, bpb, false);

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
    let mb = full.max.y - TIME_H;
    let rb = body.max.y + ret_h;
    if nret > 0 {
        return_rows(app, ui, &g, body.max.y, rb, lanes_x1, full.max.x);
    }
    master_row(
        app,
        ui,
        &g,
        Rect::from_min_max(pos2(full.min.x, rb), pos2(lanes_x1, mb)),
        Rect::from_min_max(pos2(lanes_x1, rb), pos2(full.max.x, mb)),
        io_w,
        if nret > 0 { 2.0 } else { 3.0 },
    );
    time_ruler(
        app,
        ui,
        &g,
        Rect::from_min_max(pos2(full.min.x, mb), pos2(lanes_x1, full.max.y)),
        Rect::from_min_max(pos2(lanes_x1, mb), full.max),
    );
    // One continuous 2 px separator between lanes and headers, down past the master.
    ui.painter().rect_filled(Rect::from_min_max(pos2(lanes_x1, body.min.y), pos2(lanes_x1 + 2.0, mb)), CornerRadius::ZERO, sep());
    os_file_drop(app, ui, &g);

    // ---- Overlays --------------------------------------------------------------------
    let p = ui.painter_at(lanes);
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
    let top = full.min.y + RULER_H - 2.0; // bottom of the loop row
    let marks = ui.painter_at(Rect::from_min_max(pos2(lanes.min.x, full.min.y), pos2(lanes.max.x, full.max.y)));
    // Isosceles right triangle with its vertical leg on pixel column `x`, the other
    // leg pointing `out` (-1 left / 1 right) at `y_far`, the tip at `y_tip`.
    let edge_tri = |x: f32, out: f32, y_tip: f32, y_far: f32| {
        let e = x - 0.5 * out;
        egui::Shape::convex_polygon(vec![pos2(e, y_tip), pos2(e, y_far), pos2(e + MARK * out, y_far)], gray(0x91), Stroke::NONE)
    };
    // Active loop (hh-loop-active): 1 px grey lines at both ends from the brace
    // down to the time ruler; no triangles at the bottom.
    if app.project.loop_enabled {
        let (y0, yb) = (full.min.y + 35.0 + 11.0, mb + 2.0);
        let x1 = g.to_x(app.project.loop_start).round() + 0.5;
        let x2 = (g.to_x(app.project.loop_end).round() + 0.5).max(x1 + 16.0);
        for x in [x1, x2] {
            marks.line_segment([pos2(x, y0), pos2(x, yb)], Stroke::new(1.0, gray(0x91)));
        }
    }
    // Time selection: only triangles, outside the range, at the bottom of the loop
    // row (right angle up) and the top of the time ruler (right angle down).
    let sel = app.time_sel.filter(|t| t.len() > 1e-9);
    if let Some(ts) = sel {
        let x1 = g.to_x(ts.start).round() + 0.5;
        let x2 = (g.to_x(ts.end).round() + 0.5).max(x1 + 1.0);
        for (x, out) in [(x1, -1.0f32), (x2, 1.0)] {
            marks.add(edge_tri(x, out, top, top - MARK));
            marks.add(edge_tri(x, out, mb + 2.0, mb + 2.0 + MARK));
        }
    }
    // Start marker and playhead are independent (Live): the marker (where the next
    // Play starts) is a line through the selected track(s) / master only, hidden
    // while a time selection exists; the playhead spans every lane while playing.
    // Both are 1 px inverted with ▼ at the loop row bottom and ▲ at the time ruler.
    let mut inv = super::invert::Inverted::default();
    let in_view = |x: f32| x >= lanes.min.x && x < lanes.max.x;
    let xp = g.to_x(app.status.beat_position).round() + 0.5;
    let xm = g.to_x(app.play_start).round() + 0.5;
    if app.status.playing && in_view(xp) {
        inv.tri(pos2(xp, top), 7.0, 4.0, true);
        inv.tri(pos2(xp, mb + 2.0), 7.0, 4.0, false);
        inv.vline(xp, top, mb + 2.0);
    }
    // Inverted shapes cancel where they overlap: skip the marker under the playhead.
    let covered = app.status.playing && (xp - xm).abs() < 0.5;
    if sel.is_none() && in_view(xm) && !covered {
        inv.tri(pos2(xm, top), 7.0, 4.0, true);
        inv.tri(pos2(xm, mb + 2.0), 7.0, 4.0, false);
        for (k, &t) in g.rows.iter().enumerate() {
            if app.is_track_selected(t) {
                let r = g.lane(k).intersect(body);
                inv.vline(xm, r.min.y, r.max.y);
            }
        }
        if app.master_selected {
            inv.vline(xm, body.max.y + 3.0, mb);
        }
    }
    inv.paint(&marks);
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
    if zoom_d != 1.0 || delta.x != 0.0 {
        app.follow_paused |= app.status.playing;
    }
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
    p.rect_filled(
        Rect::from_min_max(pos2(ruler.min.x, ruler.min.y + 12.0), pos2(ruler.max.x, ruler.min.y + 14.0)),
        CornerRadius::ZERO,
        sep(),
    );
    p.rect_filled(Rect::from_min_max(pos2(ruler.min.x, ruler.max.y - 2.0), ruler.max), CornerRadius::ZERO, sep());
    // Loop row (22 px): painted like the lanes; the brace is an outline over the top
    // half (no grid inside) with inward triangles at both ends.
    let band = Rect::from_min_max(pos2(ruler.min.x, ruler.min.y + 35.0), pos2(ruler.max.x, ruler.min.y + 57.0));
    paint_bars(&p.with_clip_rect(band.intersect(ruler)), band, g, bpb, false);
    let (ls, le) = (app.project.loop_start, app.project.loop_end);
    let bx1 = g.to_x(ls).round() + 0.5;
    let bx2 = (g.to_x(le).round() + 0.5).max(bx1 + 16.0);
    let brace = Rect::from_min_max(pos2(bx1, band.min.y + 0.5), pos2(bx2, band.min.y + 10.5));
    // hh-loop-active: an active loop fills the brace #5c; the outline and the
    // inward triangles (rows +1..+9, tip at +5, 7 px deep) stay #91.
    let col = gray(0x91);
    let fill = if app.project.loop_enabled { gray(0x5c) } else { panel() };
    p.rect_filled(brace, CornerRadius::ZERO, fill);
    p.rect_stroke(brace, CornerRadius::ZERO, Stroke::new(1.0, col), egui::StrokeKind::Middle);
    for (x, d) in [(bx1, 1.0f32), (bx2, -1.0)] {
        let y = brace.min.y;
        p.add(egui::Shape::line(vec![pos2(x, y + 1.0), pos2(x + 7.0 * d, y + 5.0), pos2(x, y + 9.0)], Stroke::new(1.0, col)));
    }
    let brace = Rect::from_min_max(pos2(bx1, band.min.y), pos2(bx2, band.min.y + 11.0));
    let edge_at = |pos: Pos2| -> Option<bool> {
        if !band.expand(2.0).contains(pos) {
            return None;
        }
        let (dl, dr) = ((pos.x - brace.min.x).abs(), (pos.x - brace.max.x).abs());
        if dl.min(dr) > 5.0 { None } else { Some(dl <= dr) }
    };
    if let Some(left) = resp.hover_pos().and_then(edge_at) {
        super::cursors::set(ui.ctx(), if left { Cur::DragLeftEdge } else { Cur::DragRightEdge });
    }
    // Beat ruler (21 px): ┌ markers standing on the bottom edge at grid lines, numbers
    // to their right; short ticks on every other grid line (sub-beats).
    let ry = ruler.min.y + 14.0;
    let yb = ry + 21.0;
    let bar_px = bpb * g.zoom;
    // Label every 2^n bars so numbers keep ~50 px apart.
    let mut label_every = 1i64;
    while (label_every as f64) * bar_px < 50.0 {
        label_every *= 2;
    }
    let beat_labels = g.zoom >= 60.0;
    let mark = gray(0x64);
    let step = g.grid.max(1e-6).min(bpb * label_every as f64);
    let tick_step = if step * g.zoom >= 6.0 { step } else { bpb * label_every as f64 };
    let mut v = (g.scroll / tick_step).floor().max(0.0) * tick_step;
    while g.to_x(v) < ruler.max.x {
        let x = g.to_x(v).round() + 0.5;
        let bars = v / bpb;
        let bar = (bars + 1e-9).floor() as i64;
        let on_bar = (bars - bar as f64).abs() < 1e-6;
        let beat_f = v - bar as f64 * bpb;
        let on_beat = (beat_f - beat_f.round()).abs() < 1e-6;
        let label = if on_bar && bar % label_every == 0 {
            Some(format!("{}", bar + 1))
        } else if beat_labels && on_beat && !on_bar {
            Some(format!("{}.{}", bar + 1, beat_f.round() as i64 + 1))
        } else {
            None
        };
        if let Some(l) = label {
            p.line_segment([pos2(x, ry + 15.0), pos2(x, yb)], Stroke::new(1.0, mark));
            p.line_segment([pos2(x, ry + 15.5), pos2(x + 2.0, ry + 15.5)], Stroke::new(1.0, mark));
            p.text(pos2(x + 4.0, ry + 15.0), Align2::LEFT_CENTER, l, font(FS), gray(0x92));
        } else {
            // Sub-beat ticks: dimmer than the numbered markers (hh-loop-active ≈ #4b).
            let h = if on_beat { 3.0 } else { 2.0 };
            p.line_segment([pos2(x, yb - h), pos2(x, yb)], Stroke::new(1.0, gray(0x4e)));
        }
        v += tick_step;
    }
    // Playhead triangle under the brace (inverted, see arrangement_view).
    // Drag a brace corner = resize, the brace body = move, elsewhere = draw a new range.
    // None of these switch looping on (click the brace or Ctrl+L for that).
    // Beat-time ruler: drag up/down zooms around the grabbed beat (it stays under
    // the pointer, so a sideways drag scrolls), like Live; Zoom cursor.
    let beat_row = Rect::from_min_max(pos2(ruler.min.x, ry), pos2(ruler.max.x, yb));
    let zid = resp.id.with("ruler_zoom");
    let zoom_anchor: Option<f64> = ui.data(|d| d.get_temp(zid));
    if zoom_anchor.is_some() || resp.hover_pos().is_some_and(|h| beat_row.contains(h)) {
        super::cursors::set(ui.ctx(), Cur::Zoom);
    }
    if let Some(anchor) = zoom_anchor {
        if resp.dragged() {
            let (dy, px) = ui.input(|i| (i.pointer.delta().y, i.pointer.latest_pos().map_or(0.0, |p| p.x)));
            let x0 = g.to_x(app.arr_scroll as f64);
            app.arr_zoom = (app.arr_zoom * (dy * 0.01).exp()).clamp(1.5, 600.0);
            app.arr_scroll = (anchor - ((px - x0) / app.arr_zoom) as f64).max(0.0) as f32;
            app.follow_paused |= app.status.playing;
        } else {
            ui.data_mut(|d| d.remove::<f64>(zid));
        }
    }
    if resp.drag_started()
        && let Some(pos) = ui.input(|i| i.pointer.press_origin())
        && beat_row.contains(pos)
    {
        ui.data_mut(|d| d.insert_temp(zid, g.beat_at(pos.x)));
    } else if resp.drag_started()
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
    super::menu::context(&resp, super::menu::W_CLIP, |ui| grid_section(app, ui));
}

fn drop_zone_row(ui: &mut egui::Ui, g: &Geo, k: usize) {
    // Centred in all the empty lane space below the last track.
    let r = Rect::from_min_max(pos2(g.lanes.min.x, g.lane(k).min.y.max(g.body.min.y)), g.lanes.max);
    if r.height() > 8.0 {
        ui.painter_at(r).text(r.center(), Align2::CENTER_CENTER, "Drop Files and Devices Here", font(FS), gray(90));
    }
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
        painter.rect_filled(Rect::from_min_max(pos2(lane.min.x, lane.max.y - 2.0), lane.max), CornerRadius::ZERO, sep());
        return;
    }
    if selected {
        paint_bars(&painter, lane, g, app.project.beats_per_bar.max(1) as f64, true);
    }
    let ts_x = time_sel_x(app, g, t);
    if let Some((x1, x2)) = ts_x {
        let band = Rect::from_x_y_ranges(x1..=x2, lane.min.y..=lane.max.y - 2.0);
        paint_bars_tinted(
            &painter.with_clip_rect(band.intersect(painter.clip_rect())),
            band,
            g,
            app.project.beats_per_bar.max(1) as f64,
            true,
            true,
        );
    }
    // Live: 2 px separator under every track.
    painter.rect_filled(Rect::from_min_max(pos2(lane.min.x, lane.max.y - 2.0), lane.max), CornerRadius::ZERO, sep());

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
        let r = Rect::from_min_max(pos2(x1, lane.min.y), pos2(x2.max(x1 + 3.0), lane.max.y - 2.0));
        let sel = app.arr_sel.contains(&id) || app.selected_clip == Some(ClipRef::Arr { track: t, index: i });
        let p = &painter;
        draw_clip(p, r, &a.clip, sel, app.cache.peek(sample_key(&a.clip)).map(|b| &**b), g.zoom, app.project.tempo_bpm, collapsed);
        if !collapsed && g.grid * g.zoom >= 4.0 {
            // Grid lines show through the clip body (Live).
            let body = Rect::from_min_max(pos2(r.min.x + 1.0, r.min.y + 16.0), r.max - vec2(1.0, 1.0)).intersect(lane);
            let pc = p.with_clip_rect(body.intersect(p.clip_rect()));
            let mut v = (g.beat_at(body.min.x).max(0.0) / g.grid).ceil() * g.grid;
            while g.to_x(v) < body.max.x {
                let x = g.to_x(v).round() + 0.5;
                pc.line_segment([pos2(x, body.min.y), pos2(x, body.max.y)], Stroke::new(1.0, Color32::from_black_alpha(26)));
                v += g.grid;
            }
        }
        if let Some((sx1, sx2)) = ts_x
            && !sel
            && sx2 > r.min.x
            && sx1 < r.max.x
        {
            // The part inside the time selection is drawn selected (Live).
            let pc = p.with_clip_rect(Rect::from_x_y_ranges(sx1..=sx2, r.y_range()).intersect(p.clip_rect()));
            draw_clip(&pc, r, &a.clip, true, app.cache.peek(sample_key(&a.clip)).map(|b| &**b), g.zoom, app.project.tempo_bpm, collapsed);
        }
        if moving.contains(&id) && !ui.input(|i| i.modifiers.command) {
            p.rect_filled(r, CornerRadius::ZERO, Color32::from_black_alpha(110));
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
        // Live: "[" / "]" on the trim edges, Drag (Ctrl: DragCopy) while moving.
        if er.hovered() || er.dragged() {
            super::cursors::set(ui.ctx(), Cur::DragRightEdge);
        } else if el.hovered() || el.dragged() {
            super::cursors::set(ui.ctx(), Cur::DragLeftEdge);
        } else if tr.dragged() {
            let copy = ui.input(|i| i.modifiers.command);
            super::cursors::set(ui.ctx(), if copy { Cur::DragCopy } else { Cur::Drag });
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
        if !collapsed && matches!(app.project.tracks[t].arrangement[i].clip.content, ClipContent::Audio { .. }) {
            fade_handles(app, ui, g, r, vis, t, i);
        }
        if tr.secondary_clicked() && !app.arr_sel.contains(&id) {
            app.select_arr_clip(id, false);
        }
        super::menu::context(&tr, super::menu::W_CLIP, |ui| selection_menu(app, ui));
    }
}

/// Live fade handles: while the pointer is over an audio clip, small squares
/// sit at the top of the body where each fade ends (drag sideways = length) and
/// a dot on each fade curve (drag up/down = curvature, Curve cursor).
fn fade_handles(app: &mut DawApp, ui: &mut egui::Ui, g: &Geo, r: Rect, vis: Rect, t: usize, i: usize) {
    let clip = &app.project.tracks[t].arrangement[i].clip;
    let id = clip.id;
    let body = Rect::from_min_max(pos2(r.min.x, r.min.y + 16.0), r.max);
    if body.height() < 8.0 || r.width() < 12.0 {
        return;
    }
    let (fi, fo, ci, co) = clip_fades(clip);
    let len = clip.length;
    let zoom = g.zoom as f32;
    let xin = (body.min.x + fi as f32 * zoom).round();
    let xout = (body.max.x - fo as f32 * zoom).round();
    let curve_pt = |x0: f32, w: f32, c: f32, left: bool| {
        let gm = daw_engine::fade_gain(0.5, c);
        pos2(if left { x0 + w * 0.5 } else { x0 - w * 0.5 }, body.max.y - gm * body.height())
    };
    // 0 = fade-in length, 1 = fade-out length, 2 = in curve, 3 = out curve.
    let handles = [
        Rect::from_center_size(pos2(xin + 2.5, body.min.y + 2.5), vec2(9.0, 9.0)),
        Rect::from_center_size(pos2(xout - 2.5, body.min.y + 2.5), vec2(9.0, 9.0)),
        Rect::from_center_size(curve_pt(body.min.x, fi as f32 * zoom, ci, true), vec2(9.0, 9.0)),
        Rect::from_center_size(curve_pt(body.max.x, fo as f32 * zoom, co, false), vec2(9.0, 9.0)),
    ];
    let gid = ui.scope_id().with(("fade_drag", id));
    let active: Option<(usize, f64, f32)> = ui.data(|d| d.get_temp(gid));
    let hover = ui.rect_contains_pointer(r.intersect(vis)) || active.is_some();
    if !hover {
        return;
    }
    let p = ui.painter_at(vis);
    let edge = clip_edge(clip, app.arr_sel.contains(&id));
    for (k, h) in handles.iter().enumerate() {
        let shown = k < 2 || (k == 2 && fi > 0.0) || (k == 3 && fo > 0.0);
        if !shown {
            continue;
        }
        let resp = ui.interact(h.intersect(vis), ui.scope_id().with(("fade_h", id, k)), Sense::drag());
        let hot = resp.hovered() || active.is_some_and(|a| a.0 == k);
        if k < 2 {
            let sq = Rect::from_center_size(h.center(), vec2(5.0, 5.0));
            p.rect_filled(sq, CornerRadius::ZERO, if hot { Color32::WHITE } else { gray(0xd0) });
            p.rect_stroke(sq, CornerRadius::ZERO, Stroke::new(1.0, edge), egui::StrokeKind::Outside);
        } else {
            p.circle_filled(h.center(), 2.5, if hot { Color32::WHITE } else { gray(0xd0) });
            p.circle_stroke(h.center(), 2.5, Stroke::new(1.0, edge));
        }
        if hot {
            super::cursors::set(ui.ctx(), if k < 2 { Cur::ResizeLeftRight } else { Cur::Curve });
        }
        if resp.drag_started() {
            app.begin_gesture();
            let (v, c) = match k {
                0 => (fi, ci),
                1 => (fo, co),
                2 => (fi, ci),
                _ => (fo, co),
            };
            ui.data_mut(|d| d.insert_temp(gid, (k, v, c)));
        }
        if resp.dragged()
            && let Some((k, _, _)) = active
        {
            let d = ui.input(|inp| inp.pointer.delta());
            let a = &mut app.project.tracks[t].arrangement[i].clip.audio;
            match k {
                0 => a.fade_in = (a.fade_in + (d.x / zoom) as f64).clamp(0.0, (len - a.fade_out).max(0.0)),
                1 => a.fade_out = (a.fade_out - (d.x / zoom) as f64).clamp(0.0, (len - a.fade_in).max(0.0)),
                2 => a.fade_in_curve = (a.fade_in_curve - d.y * 0.01).clamp(-1.0, 1.0),
                _ => a.fade_out_curve = (a.fade_out_curve - d.y * 0.01).clamp(-1.0, 1.0),
            }
            app.needs_compile = true;
        }
        if resp.drag_stopped() {
            ui.data_mut(|d| d.remove::<(usize, f64, f32)>(gid));
        }
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
// Live header metrics: 13 px controls on a 17 px pitch; activator 46, S / arm 21.
const BOX_H: f32 = 13.0;
const ACT_W: f32 = 46.0;
const GAP: f32 = 4.0;
const SOLO_W: f32 = 21.0;
const ARM_W: f32 = SOLO_W;
const PAN_W: f32 = SOLO_W * 2.0 + GAP;
const RIGHT_W: f32 = 119.0;

/// In/Out column width (74 px + 2 px separator in Live); shrinks in narrow windows.
pub fn io_width(full_w: f32) -> f32 {
    76.0f32.min(((full_w - HEAD_W) * 0.14).max(64.0))
}

fn header(app: &mut DawApp, ui: &mut egui::Ui, g: &Geo, k: usize, t: usize) {
    let lane = g.lane(k);
    let head = Rect::from_min_max(pos2(g.lanes.max.x + 2.0, lane.min.y), pos2(g.body.max.x, lane.max.y));
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
    painter.rect_filled(head, CornerRadius::ZERO, sep());
    for (d, &a) in chain.iter().rev().enumerate() {
        let x = head.min.x + d as f32 * INDENT;
        painter.rect_filled(
            Rect::from_min_size(pos2(x, head.min.y), vec2(INDENT - 3.0, head.height() + 1.0)),
            CornerRadius::ZERO,
            rgb(app.project.tracks[a].color),
        );
    }
    let inner = Rect::from_min_max(pos2(head.min.x + depth as f32 * INDENT, head.min.y), pos2(head.max.x, head.max.y - 2.0));
    let split = (inner.max.x - RIGHT_W).max(inner.min.x + 40.0 + g.io_w);
    // In/Out sits between the coloured name area and the buttons.
    // Columns separated by 2 px: name (track colour) | In/Out | mixer.
    let io = Rect::from_min_max(pos2(split - g.io_w, inner.min.y), pos2(split - 2.0, inner.max.y));
    let left = Rect::from_min_max(inner.min, pos2(split - g.io_w - 2.0, inner.max.y));
    let right = Rect::from_min_max(pos2(split, inner.min.y), inner.max);
    let title_bg = if selected { shade(color, 1.12) } else { color };
    painter.rect_filled(left, CornerRadius::ZERO, title_bg);
    painter.rect_filled(right, CornerRadius::ZERO, if selected { sel() } else { panel() });
    let _ = NAME_W;
    let text_col = text_on(title_bg);

    // Top-left: ⊙ collapse button, (group fold arrow), name.
    let row1 = inner.min.y + if collapsed { ((inner.height() - BOX_H) * 0.5).max(1.0) } else { 2.0 };
    let cbtn = Rect::from_min_size(pos2(left.min.x + 2.0, row1 - 1.0), vec2(15.0, BOX_H + 2.0));
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
        painter.with_clip_rect(name_rect.intersect(clip)).text(name_rect.left_center(), Align2::LEFT_CENTER, &name, font(FS), text_col);
    }
    // Right edge: MIDI activity dots on MIDI tracks without an instrument, else a
    // stereo peak meter (Live).
    if app.project.tracks[t].sends_midi() {
        let lvl = midi_activity(app, t);
        dot_meter(
            &painter,
            // Same column as the stereo meters (x −17…−10) so they line up.
            Rect::from_min_max(pos2(inner.max.x - 17.0, inner.min.y + 2.0), pos2(inner.max.x - 10.0, inner.max.y - 2.0)),
            lvl,
        );
    } else {
        let m = app.track_meters.get(t).copied().unwrap_or([0.0; 2]);
        let r = Rect::from_min_max(pos2(inner.max.x - 17.0, inner.min.y + 2.0), pos2(inner.max.x - 10.0, inner.max.y - 2.0));
        track_meter(&painter, r, m, [m[0] >= 1.0, m[1] >= 1.0]);
    }

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
        super::cursors::set(ui.ctx(), Cur::Drag);
    }
    if let Some(item) = hr.dnd_release_payload::<DragItem>()
        && let Some(def) = super::instrument_for(&item)
    {
        app.set_instrument(t, def);
    }
    super::session::open_track_menu(app, &hr, t);
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
    let bx = right.min.x + 2.0;
    let act = Rect::from_min_size(pos2(bx, row1), vec2(ACT_W, BOX_H));
    if !clip.contains_rect(act) {
        return;
    }
    let act_col = if app.solo_silenced.get(t).copied().unwrap_or(false) { solo_dimmed(ACCENT) } else { ACCENT };
    let solo_r = Rect::from_min_size(pos2(act.max.x + 6.0, row1), vec2(SOLO_W, BOX_H));
    let arm_r = Rect::from_min_size(pos2(solo_r.max.x + GAP, row1), vec2(ARM_W, BOX_H));
    if rect_toggle(ui, act, ui.scope_id().with(("hact", id)), &format!("{}", t + 1), !mute, act_col, "Track activator (mute)").clicked() {
        app.toggle_track_flag(t, TrackFlag::Mute, false);
    }
    let excl = !ui.input(|i| i.modifiers.command);
    if clip.contains_rect(solo_r)
        && rect_toggle(ui, solo_r, ui.scope_id().with(("hsolo", id)), "S", solo, SOLO_BLUE, "Solo (Ctrl: add)").clicked()
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
    let pr = Rect::from_min_size(pos2(vr.max.x + 6.0, row2), vec2(PAN_W, BOX_H));
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
        painter.text(pos2(left.min.x + 4.0, row2 + 2.0), Align2::LEFT_TOP, "session clip", font(FS_SMALL), text_col);
    }
}

const RET_H: f32 = 17.0;

/// Return track rows between the lanes and Main (y0..y1), collapsed like Live's
/// (17 px): colour name area with ⊙, an empty Audio To box, activator letter,
/// S and the (fixed) Post switch, plus a peak meter.
fn return_rows(app: &mut DawApp, ui: &mut egui::Ui, g: &Geo, y0: f32, y1: f32, lanes_x1: f32, x1: f32) {
    let p = ui.painter().clone();
    p.rect_filled(Rect::from_min_max(pos2(g.x0, y0), pos2(x1, y1)), CornerRadius::ZERO, sep());
    let bpb = app.project.beats_per_bar.max(1) as f64;
    let n0 = app.project.returns.len();
    for k in 0..n0 {
        if app.project.returns.len() != n0 {
            ui.ctx().request_repaint();
            return;
        }
        let top = y0 + 3.0 + k as f32 * (RET_H + 2.0);
        let lane = Rect::from_min_max(pos2(g.x0, top), pos2(lanes_x1, top + RET_H));
        paint_bars(&ui.painter_at(lane), lane, g, bpb, false);
        let head = Rect::from_min_max(pos2(lanes_x1 + 2.0, top), pos2(x1, top + RET_H));
        let split = (head.max.x - RIGHT_W).max(head.min.x + 40.0 + g.io_w);
        let left = Rect::from_min_max(head.min, pos2(split - g.io_w - 2.0, head.max.y));
        let right = Rect::from_min_max(pos2(split, head.min.y), head.max);
        let r = &app.project.returns[k];
        let (col, mute, solo, name) = (rgb(r.color), r.mute, r.solo, r.name.clone());
        p.rect_filled(left, CornerRadius::ZERO, col);
        p.rect_filled(right, CornerRadius::ZERO, panel());
        let row1 = head.min.y + 2.0;
        let tc = text_on(col);
        let cbtn = Rect::from_min_size(pos2(left.min.x + 2.0, row1 - 1.0), vec2(15.0, BOX_H + 2.0));
        draw_collapse_button(&p, cbtn.center(), true, tc);
        let name_rect = Rect::from_min_max(pos2(cbtn.max.x + 2.0, row1 - 1.0), pos2(left.max.x - 3.0, row1 + BOX_H + 1.0));
        p.with_clip_rect(name_rect).text(name_rect.left_center(), Align2::LEFT_CENTER, &name, font(FS), tc);
        let hr = ui.interact(left, ui.scope_id().with(("ret_head", k)), Sense::click());
        super::menu::context(&hr, super::menu::W_TRACK, |ui| super::session::return_menu(app, ui, k));
        if g.io_w > 0.0 {
            let io = Rect::from_min_max(pos2(split - g.io_w, head.min.y), pos2(split - 2.0, head.max.y));
            p.rect_filled(io, CornerRadius::ZERO, panel());
            let b = Rect::from_min_max(pos2(io.min.x + 2.0, row1), pos2(io.max.x - 2.0, row1 + BOX_H));
            p.rect_stroke(b, CornerRadius::ZERO, Stroke::new(1.0, outline()), egui::StrokeKind::Inside);
        }
        let m = app.return_meters.get(k).copied().unwrap_or([0.0; 2]);
        let mr = Rect::from_min_max(pos2(head.max.x - 17.0, head.min.y + 2.0), pos2(head.max.x - 10.0, head.max.y - 2.0));
        track_meter(&p, mr, m, [m[0] >= 1.0, m[1] >= 1.0]);
        // Activator 46 · 4 · S 17 · 4 · Post 25 (Live).
        let bx = right.min.x + 2.0;
        let act = Rect::from_min_size(pos2(bx, row1), vec2(ACT_W, BOX_H));
        let sr = Rect::from_min_size(pos2(act.max.x + 4.0, row1), vec2(17.0, BOX_H));
        let pr = Rect::from_min_size(pos2(sr.max.x + 4.0, row1), vec2(25.0, BOX_H));
        let letter = ((b'A' + (k as u8 % 26)) as char).to_string();
        let act_col = if app.solo_silenced_returns.get(k).copied().unwrap_or(false) { solo_dimmed(ACCENT) } else { ACCENT };
        if rect_toggle(ui, act, ui.scope_id().with(("ret_act", k)), &letter, !mute, act_col, "Return activator (mute)").clicked() {
            app.toggle_return(k, false, false);
        }
        let excl = !ui.input(|i| i.modifiers.command);
        if rect_toggle(ui, sr, ui.scope_id().with(("ret_solo", k)), "S", solo, SOLO_BLUE, "Solo (Ctrl: add)").clicked() {
            app.toggle_return(k, true, excl);
        }
        rect_toggle(ui, pr, ui.scope_id().with(("ret_post", k)), "Post", true, ACCENT, "Sends to this return are post-fader");
    }
}

/// Master track row, pinned under the lanes (Live). Collapsed by default; no
/// activator / solo / arm; volume and pan are always shown.
fn master_row(app: &mut DawApp, ui: &mut egui::Ui, g: &Geo, lanes: Rect, head: Rect, io_w: f32, sep_h: f32) {
    let p = ui.painter();
    // Live: 3 px separator above the master (2 under return rows); its lane
    // shows the grid in large type.
    p.rect_filled(Rect::from_min_max(lanes.min, pos2(head.max.x, lanes.min.y + sep_h)), CornerRadius::ZERO, sep());
    let lanes = Rect::from_min_max(pos2(lanes.min.x, lanes.min.y + sep_h), lanes.max);
    let head = Rect::from_min_max(pos2(head.min.x, head.min.y + sep_h - 3.0), head.max);
    let head = Rect::from_min_max(pos2(head.min.x + 2.0, head.min.y + 1.0), head.max);
    paint_bars(&ui.painter_at(lanes), lanes, g, app.project.beats_per_bar.max(1) as f64, false);
    let gl = match app.arr_grid {
        crate::arr_ops::GridMode::Off => "Off".to_string(),
        _ => grid_label(app.grid),
    };
    let gr = p.text(lanes.right_center() - vec2(6.0, 0.0), Align2::RIGHT_CENTER, gl, font(lanes.height().clamp(11.0, 22.0)), gray(0x6a));
    let gresp = ui
        .interact(gr.expand(3.0), ui.scope_id().with("arr_grid_label"), Sense::click())
        .on_hover_text("Grid (right-click: grid settings · Ctrl+1–4)");
    super::menu::context(&gresp, super::menu::W_CLIP, |ui| grid_section(app, ui));
    let p = ui.painter();
    let collapsed = app.project.master_collapsed;
    let split = (head.max.x - RIGHT_W).max(head.min.x + 40.0 + io_w);
    let left = Rect::from_min_max(pos2(head.min.x, head.min.y + 2.0), pos2(split - io_w - 2.0, head.max.y));
    let right = Rect::from_min_max(pos2(split, head.min.y + 2.0), head.max);
    // Name area: click selects the master (device view), right-click = colour.
    let sel = app.master_selected;
    let col = rgb(app.project.master_color);
    let title_bg = if sel { col } else { shade(col, 0.72) };
    // Separator colour behind everything: 2 px lines between name | In/Out | mixer.
    p.rect_filled(Rect::from_min_max(pos2(head.min.x, head.min.y), head.max), CornerRadius::ZERO, sep());
    p.rect_filled(left, CornerRadius::ZERO, title_bg);
    super::add_master_zone(ui, head);
    let hr = ui.interact(left, ui.scope_id().with("master_head"), Sense::click());
    if hr.clicked() {
        app.select_master();
    }
    hr.context_menu(|ui| super::session::master_menu(app, ui));
    if io_w > 0.0 {
        let io = Rect::from_min_max(pos2(left.max.x + 2.0, head.min.y + 2.0), pos2(split - 2.0, head.max.y));
        ui.painter().rect_filled(io, CornerRadius::ZERO, panel());
        super::io::master_io_compact(app, ui, io, collapsed);
    }
    let p = ui.painter();
    p.rect_filled(right, CornerRadius::ZERO, if sel { sel_bg() } else { panel() });
    let row1 = left.min.y + if collapsed { ((left.height() - BOX_H) * 0.5).max(0.0) } else { 2.0 };
    let cbtn = Rect::from_min_size(pos2(left.min.x + 2.0, row1 - 1.0), vec2(15.0, BOX_H + 2.0));
    let tc = text_on(title_bg);
    draw_collapse_button(p, cbtn.center(), collapsed, tc);
    p.text(pos2(cbtn.max.x + 3.0, row1 + BOX_H * 0.5), Align2::LEFT_CENTER, "Main", bold(FS), tc);
    let r = ui.interact(cbtn, ui.scope_id().with("master_collapse"), Sense::click());
    if r.clicked() {
        app.project.master_collapsed = !collapsed;
        app.dirty = true;
    }
    // Master meter in the lane (expanded) and at the right edge.
    let r = Rect::from_min_max(pos2(head.max.x - 17.0, head.min.y + 2.0), pos2(head.max.x - 10.0, head.max.y - 2.0));
    track_meter(p, r, app.meter, [app.clipped, app.clipped]);
    let bx = right.min.x + 2.0;
    // Row 1: volume · blue Cue; pan below them once the row is tall enough.
    let vr = Rect::from_min_size(pos2(bx, row1), vec2(ACT_W, BOX_H));
    let cr = Rect::from_min_size(pos2(vr.max.x + 6.0, row1), vec2(PAN_W, BOX_H));
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
    let shift_only = ui.input(|i| i.modifiers.shift && !i.modifiers.command);
    if lr.clicked()
        && shift_only
        && let Some(pos) = lr.interact_pointer_pos()
        && let Some(t) = g.track_at(pos.y)
        && clip_at(app, t, g.beat_at(pos.x)).is_none()
    {
        // Shift+click on empty lane space: time selection from the start marker
        // (or the current selection) to here, across the tracks in between (Live).
        app.extend_time_selection(g.snap(g.beat_at(pos.x)), t);
    } else if lr.clicked()
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
                        app.set_start_marker(g.snap(beat));
                    }
                    None => {
                        app.set_start_marker(g.snap(beat));
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
    super::menu::context(lr, super::menu::W_CLIP, |ui| selection_menu(app, ui));

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

/// Live's clip-area context menu (acts on the current selection). The editing
/// block on top appears with a time selection or selected clips.
fn selection_menu(app: &mut DawApp, ui: &mut egui::Ui) {
    use super::menu::*;
    use crate::edit_ops::Clipboard;
    let has_ts = app.time_sel.is_some_and(|t| t.len() > 1e-9);
    let has_clip = !app.arr_sel.is_empty();
    let can_paste = matches!(app.clipboard, Clipboard::Clips { .. });
    app.focus_detail = false;
    app.head_focus = false;
    if has_ts || has_clip {
        if item(ui, "Cut", "Ctrl+X", true)
            && let Some(t) = app.cut_selection()
        {
            ui.ctx().copy_text(t);
        }
        if item(ui, "Copy", "Ctrl+C", true)
            && let Some(t) = app.copy_selection()
        {
            ui.ctx().copy_text(t);
        }
        if item(ui, "Paste", "Ctrl+V", can_paste) {
            app.paste();
        }
        if item(ui, "Duplicate", "Ctrl+D", true) {
            app.duplicate_arrangement_selection();
        }
        if item(ui, "Delete", "Del", true) {
            app.delete_arrangement_selection();
        }
        sep(ui);
        if item(ui, "Split", "Ctrl+E", true) {
            app.split_selection();
        }
        if item(ui, "Consolidate", "Ctrl+J", true) {
            app.consolidate_selection();
        }
        if has_ts {
            let midi = app.time_sel.is_some_and(|ts| app.project.tracks.get(ts.t0).is_some_and(|t| t.kind == TrackKind::Midi));
            if item(ui, "Insert MIDI Clip", "Ctrl+Shift+M", midi) {
                app.insert_midi_clip_in_selection();
            }
        }
        if has_clip {
            let off = app
                .arr_sel
                .iter()
                .all(|&id| app.project.find_arrangement_clip(id).is_some_and(|(t, i)| app.project.tracks[t].arrangement[i].clip.muted));
            if item(ui, if off { "Activate Clip(s)" } else { "Deactivate Clip(s)" }, "0", true) {
                app.toggle_disable();
            }
        }
        sep(ui);
    } else if can_paste {
        if item(ui, "Paste", "Ctrl+V", true) {
            app.paste();
        }
        sep(ui);
    }
    if has_ts {
        if item(ui, "Zoom to Time Selection", "Z", true) {
            app.zoom_to_selection();
        }
    } else if item(ui, "Zoom Back from Time Selection", "X", app.zoom_back.is_some()) {
        app.zoom_back_from_selection();
    }
    sep(ui);
    if item(ui, "Create Fade", "Ctrl+Alt+F", app.can_create_fade()) {
        app.create_fade();
    }
    if item(ui, "Reset Fades", "Ctrl+Alt+BS", has_ts || has_clip) {
        app.reset_fades();
    }
    let loop_label = if has_ts || has_clip {
        "Loop Selection"
    } else if app.project.loop_enabled {
        "Deactivate Loop"
    } else {
        "Activate Loop"
    };
    if item(ui, loop_label, "Ctrl+L", true) {
        app.loop_selection();
    }
    sep(ui);
    item(ui, "Freeze Track", "Ctrl+Shift+Alt+F", false);
    item(ui, "Bounce Track in Place", "", false);
    item(ui, "Bounce to New Track", "Ctrl+B", false);
    item(ui, "Paste Bounced Audio", "Ctrl+Alt+V", false);
    sep(ui);
    grid_section(app, ui);
}

/// Snap / adaptive / fixed grid block + Narrow / Widen / Triplet (Live).
fn grid_section(app: &mut DawApp, ui: &mut egui::Ui) {
    use super::menu::*;
    use crate::arr_ops::GridMode;
    let mode = app.arr_grid;
    if check_item(ui, "Snap to Grid", "Ctrl+4", mode != GridMode::Off, true) {
        app.toggle_snap();
    }
    ui.add_space(5.0);
    caption(ui, "Adaptive Grid:", 14.0);
    let ad = |l: i8| mode == GridMode::Adaptive(l);
    if let Some(i) = choice_row(ui, 17.0, &[(15.0, "Widest", ad(-2)), (92.0, "Wide", ad(-1)), (169.0, "Medium", ad(0))]) {
        app.arr_grid = GridMode::Adaptive(i as i8 - 2);
        ui.close();
    }
    if let Some(i) = choice_row(ui, 20.0, &[(15.0, "Narrow", ad(1)), (92.0, "Narrowest", ad(2))]) {
        app.arr_grid = GridMode::Adaptive(i as i8 + 1);
        ui.close();
    }
    caption(ui, "Fixed Grid:", 14.0);
    let fx = |g: f64| matches!(mode, GridMode::Fixed(v) if (v - g).abs() < 1e-9);
    let row1 = [(32.0, "8 Bars"), (16.0, "4 Bars"), (8.0, "2 Bars"), (4.0, "1 Bar"), (2.0, "1/2")];
    let row2 = [(1.0, "1/4"), (0.5, "1/8"), (0.25, "1/16"), (0.125, "1/32")];
    let xs = [15.0, 70.0, 125.0, 180.0, 235.0];
    let items1: Vec<(f32, &str, bool)> = row1.iter().zip(xs).map(|(&(g, n), x)| (x, n, fx(g))).collect();
    if let Some(i) = choice_row(ui, 17.0, &items1) {
        app.arr_grid = GridMode::Fixed(row1[i].0);
        ui.close();
    }
    let items2: Vec<(f32, &str, bool)> = row2.iter().zip(xs).map(|(&(g, n), x)| (x, n, fx(g))).collect();
    if let Some(i) = choice_row(ui, 15.0, &items2) {
        app.arr_grid = GridMode::Fixed(row2[i].0);
        ui.close();
    }
    sep(ui);
    let can = mode != GridMode::Off;
    if item(ui, "Narrow Grid", "Ctrl+1", can) {
        app.narrow_grid(true);
    }
    if item(ui, "Widen Grid", "Ctrl+2", can) {
        app.narrow_grid(false);
    }
    sep(ui);
    if check_item(ui, "Triplet Grid", "Ctrl+3", app.arr_triplet, true) {
        app.arr_triplet = !app.arr_triplet;
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
                app.set_start_marker(ts.start);
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
                // Ctrl held on release = copy (Live), the originals stay.
                let copy = ui.input(|i| i.modifiers.command);
                if copy && changed {
                    app.copy_arr_clips(&targets, d.checkpoint);
                    if d.from_selection
                        && let Some(ts) = app.time_sel
                    {
                        let delta = targets[0].2 - d.clips[0].2;
                        let (t0, t1) = targets.iter().fold((usize::MAX, 0), |acc, x| (acc.0.min(x.1), acc.1.max(x.1)));
                        app.time_sel = Some(TimeSel { start: ts.start + delta, end: ts.end + delta, t0, t1 });
                    }
                } else if changed {
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
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if ext == "mid" || ext == "midi" {
            // MIDI file: onto a MIDI lane = one clip there; elsewhere = new MIDI track(s).
            let parsed = std::fs::read(path).map_err(|e| e.to_string()).and_then(|d| daw_project::midifile::parse_smf(&d));
            match parsed {
                Ok(file) => {
                    let placed = app.edit(|p| p.import_midi(&file, target, at, &name));
                    if let Some(&(t, _)) = placed.first() {
                        app.clear_structure_selection();
                        app.selected_track = t;
                        app.arr_sel = placed.iter().map(|x| x.1).collect();
                        app.time_sel = None;
                        app.resync_selected_clip();
                    }
                }
                Err(e) => app.toast(format!("{name}: {e}")),
            }
            continue;
        }
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

/// Drop into the empty space below the tracks: a new track holding the item
/// (sample → audio track + clip, instrument → MIDI track, Session clip → a
/// track of its kind with the clip at `at`).
fn drop_new_track(app: &mut DawApp, item: &DragItem, at: f64) {
    let clip = match item {
        DragItem::Sample { key, name } => app.audio_clip_for(key, name).map(|c| (c, TrackKind::Audio, format!("# {name}"), true)),
        DragItem::SessionClip { track, scene } => {
            app.project.tracks.get(*track).and_then(|x| x.slots.get(*scene)).cloned().flatten().map(|c| {
                let kind = if matches!(c.content, ClipContent::Audio { .. }) { TrackKind::Audio } else { TrackKind::Midi };
                let name = if kind == TrackKind::Audio { "# Audio".to_string() } else { "# MIDI".to_string() };
                (c, kind, name, false)
            })
        }
        _ => {
            if let Some(def) = super::instrument_for(item) {
                let t = app.edit(|p| {
                    let t = p.add_track(TrackKind::Midi);
                    p.tracks[t].name = format!("# {}", def.label());
                    p.tracks[t].instrument = def;
                    t
                });
                app.clear_structure_selection();
                app.selected_track = t;
            }
            return;
        }
    };
    let Some((mut clip, kind, name, recolor)) = clip else { return };
    let src_inst = match item {
        DragItem::SessionClip { track, .. } if kind == TrackKind::Midi => app.project.tracks.get(*track).map(|x| x.instrument.clone()),
        _ => None,
    };
    let (t, id) = app.edit(|p| {
        let t = p.add_track(kind);
        p.tracks[t].name = name;
        if let Some(i) = src_inst {
            // A MIDI clip keeps sounding like its source track.
            p.tracks[t].instrument = i;
        }
        clip.id = p.alloc_id();
        if recolor {
            clip.color = p.new_clip_color(t);
        }
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
    draw_clip_fill(p, r, clip, selected, sample, zoom, tempo, collapsed);
    // hh-cips-fades: 1 px top + left edge in the darkened fill colour (×0.53);
    // the right / bottom run straight into the lane.
    let e = clip_edge(clip, selected);
    p.rect_filled(Rect::from_min_max(r.min, pos2(r.max.x, r.min.y + 1.0)), CornerRadius::ZERO, e);
    p.rect_filled(Rect::from_min_max(r.min, pos2(r.min.x + 1.0, r.max.y)), CornerRadius::ZERO, e);
}

fn clip_fill(clip: &Clip, selected: bool) -> Color32 {
    if selected {
        SEL_BLUE
    } else if clip.muted {
        disabled(rgb(clip.color))
    } else {
        rgb(clip.color)
    }
}

fn clip_edge(clip: &Clip, selected: bool) -> Color32 {
    shade(clip_fill(clip, selected), 0.53)
}

/// Arrangement fades of an audio clip: (in, out, in curve, out curve), beats.
pub fn clip_fades(clip: &Clip) -> (f64, f64, f32, f32) {
    match clip.content {
        ClipContent::Audio { .. } => {
            let a = &clip.audio;
            let fi = a.fade_in.clamp(0.0, clip.length);
            (fi, a.fade_out.clamp(0.0, clip.length - fi), a.fade_in_curve, a.fade_out_curve)
        }
        _ => (0.0, 0.0, 0.0, 0.0),
    }
}

/// Shade the part of `body` above the fade curve (fade-in from the left edge,
/// fade-out from the right), ×0.85 of the fill like Live.
fn paint_fades(p: &egui::Painter, body: Rect, clip: &Clip, zoom: f64, fill: Color32) {
    let (fi, fo, ci, co) = clip_fades(clip);
    let col = shade(fill, 0.85);
    let h = body.height();
    let mut mesh = egui::Mesh::default();
    let column = |mesh: &mut egui::Mesh, x: f32, g: f32| {
        let i = mesh.vertices.len() as u32;
        let y = body.max.y - g * h;
        mesh.colored_vertex(pos2(x, body.min.y), col);
        mesh.colored_vertex(pos2(x, y), col);
        if i >= 2 {
            mesh.add_triangle(i - 2, i - 1, i);
            mesh.add_triangle(i - 1, i, i + 1);
        }
    };
    let clip_r = p.clip_rect();
    for (len, curve, left) in [(fi, ci, true), (fo, co, false)] {
        let w = (len * zoom) as f32;
        if w < 1.0 {
            continue;
        }
        let steps = (w.ceil() as usize).clamp(2, 512);
        for k in 0..=steps {
            let f = k as f32 / steps as f32;
            let (x, g) = if left {
                (body.min.x + f * w, daw_engine::fade_gain(f as f64, curve))
            } else {
                (body.max.x - w + f * w, daw_engine::fade_gain(1.0 - f as f64, curve))
            };
            if x < clip_r.min.x - 1.0 || x > clip_r.max.x + 1.0 {
                continue;
            }
            column(&mut mesh, x, g);
        }
        p.add(std::mem::take(&mut mesh));
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_clip_fill(
    p: &egui::Painter,
    r: Rect,
    clip: &Clip,
    selected: bool,
    sample: Option<&SampleBuffer>,
    zoom: f64,
    tempo: f64,
    collapsed: bool,
) {
    // Live 12: coloured title + body with dark contents; selected = the body turns
    // light blue, the title keeps the clip colour. 1 px dark-grey outline and divider.
    // hh-cips-fades: title and body share the clip colour; selection turns the
    // body light blue. Contents are near-black (#18) on any colour.
    let c = if clip.muted { disabled(rgb(clip.color)) } else { rgb(clip.color) };
    let (title_bg, title_fg) = (c, text_on(c));
    let body_bg = clip_fill(clip, selected);
    if collapsed {
        p.rect_filled(r, CornerRadius::ZERO, title_bg);
        p.with_clip_rect(r.intersect(p.clip_rect())).text(
            r.left_center() + vec2(3.0, 0.0),
            Align2::LEFT_CENTER,
            &clip.name,
            font(FS),
            title_fg,
        );
        return;
    }
    p.rect_filled(r, CornerRadius::ZERO, body_bg);
    let title = Rect::from_min_size(r.min, vec2(r.width(), 15.0_f32.min(r.height())));
    p.rect_filled(title, CornerRadius::ZERO, title_bg);
    if r.height() > title.height() + 1.0 {
        p.rect_filled(Rect::from_min_max(pos2(r.min.x, title.max.y), pos2(r.max.x, title.max.y + 1.0)), CornerRadius::ZERO, shade(c, 0.53));
    }
    p.with_clip_rect(title.intersect(p.clip_rect())).text(
        title.left_center() + vec2(3.0, 0.0),
        Align2::LEFT_CENTER,
        &clip.name,
        font(FS),
        title_fg,
    );
    let body = Rect::from_min_max(pos2(r.min.x, title.max.y + 1.0), r.max);
    let visible = body.intersect(p.clip_rect());
    if visible.width() <= 0.0 {
        return;
    }
    let ink = Color32::from_gray(0x18);
    paint_fades(&p.with_clip_rect(visible), body, clip, zoom, body_bg);
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
                        if n.muted { shade(body_bg, 0.7) } else { ink },
                    );
                }
            }
        }
        ClipContent::Audio { offset_frames, warp_bpm, .. } => {
            let Some(buf) = sample else {
                p.text(body.center(), Align2::CENTER_CENTER, "loading…", super::theme::font(9.0), Color32::GRAY);
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
        // Triplet grid (Ctrl+3): 2/3 of a straight step.
        v if [0.0625, 0.125, 0.25, 0.5, 1.0, 2.0].iter().any(|s| (s * 2.0 / 3.0 - v).abs() < 1e-9) => format!("{}T", grid_label(v * 1.5)),
        v => format!("{} bar", v / 4.0),
    }
}

/// Live lane background: bars alternate two shades, grid lines a little darker
/// (bar lines darkest). `sel` = selected track shades. Grid Line Intensity applies.
fn paint_bars(p: &egui::Painter, r: Rect, g: &Geo, bpb: f64, sel: bool) {
    paint_bars_tinted(p, r, g, bpb, sel, false);
}

/// `tint`: draw in the time-selection blue (shade-multiplied, Live).
fn paint_bars_tinted(p: &egui::Painter, r: Rect, g: &Geo, bpb: f64, sel: bool, tint: bool) {
    let (a, b): (u8, u8) = if sel || tint { (0x40, 0x46) } else { (0x32, 0x36) };
    let gray = |v: u8| if tint { sel_tint(v) } else { gray(v) };
    let grid = |n: u8, bg: u8| if tint { sel_tint(n.saturating_sub(0x08)) } else { grid(n, bg) };
    let bar_px = (bpb * g.zoom) as f32;
    // Very far out: alternate every n bars so rectangles stay a few px wide.
    let n = (6.0 / bar_px.max(0.01)).ceil().max(1.0) as i64;
    let span = bpb * n as f64;
    let mut i = (g.beat_at(r.min.x).max(0.0) / span).floor() as i64;
    p.rect_filled(r, CornerRadius::ZERO, gray(a));
    loop {
        let x1 = g.to_x(i as f64 * span);
        if x1 >= r.max.x {
            break;
        }
        let x2 = g.to_x((i + 1) as f64 * span);
        if i % 2 == 1 {
            p.rect_filled(Rect::from_x_y_ranges(x1.max(r.min.x)..=x2.min(r.max.x), r.y_range()), CornerRadius::ZERO, gray(b));
        }
        i += 1;
    }
    let step = g.grid.max(1e-6);
    // Lines stay at the grid even when dense; below 2 px they'd just fill the lane.
    if (step * g.zoom) < 2.0 {
        return;
    }
    let mut v = (g.beat_at(r.min.x).max(0.0) / step).floor() * step;
    while g.to_x(v) < r.max.x {
        let x = g.to_x(v).round() + 0.5;
        let bar = (v / bpb + 1e-9).floor() as i64;
        let bg = if (bar / n) % 2 == 1 { b } else { a };
        let on_bar = ((v / bpb).fract().abs() < 1e-9) || ((v / bpb).fract() > 1.0 - 1e-9);
        let c = grid(bg - if on_bar { 0x0c } else { 0x0a }, bg);
        p.line_segment([pos2(x, r.min.y), pos2(x, r.max.y)], Stroke::new(1.0, c));
        v += step;
    }
}

/// View Range Indicator (12 px, Live): song extent with clips, the playhead and
/// the visible range. Spans the lanes and the header column up to the meters.
fn overview(app: &DawApp, ui: &mut egui::Ui, g: &Geo, ov: Rect) {
    let p = ui.painter_at(ov);
    // View Range Indicator (12 px): song extent, clips, playhead, visible range.
    p.rect_filled(ov, CornerRadius::ZERO, gray(0x2e));
    let end = app.project.tracks.iter().flat_map(|t| t.arrangement.iter().map(|a| a.start + a.clip.length)).fold(64.0f64, f64::max) * 1.1;
    let k = ov.width() as f64 / end;
    let vis = Rect::from_min_max(
        pos2(ov.min.x + (g.scroll * k) as f32, ov.min.y),
        pos2(ov.min.x + ((g.scroll + g.lanes.width() as f64 / g.zoom) * k) as f32, ov.max.y),
    );
    for t in &app.project.tracks {
        for a in &t.arrangement {
            let x1 = ov.min.x + (a.start * k) as f32;
            let x2 = ov.min.x + ((a.start + a.clip.length) * k) as f32;
            p.rect_filled(
                Rect::from_min_max(pos2(x1, ov.min.y + 3.0), pos2(x2.max(x1 + 1.0), ov.max.y - 3.0)),
                CornerRadius::ZERO,
                rgb(a.clip.color).gamma_multiply(0.6),
            );
        }
    }
    let phx = (ov.min.x + (app.status.beat_position * k) as f32).round() + 0.5;
    p.line_segment([pos2(phx, ov.min.y + 1.0), pos2(phx, ov.max.y - 1.0)], Stroke::new(1.0, text()));
    p.rect_stroke(vis, CornerRadius::same(3), Stroke::new(2.0, gray(0x75)), egui::StrokeKind::Inside);
    let strip = Rect::from_min_max(pos2(g.lanes.max.x, ov.max.y), pos2(ov.max.x + 10.0, ov.max.y + 2.0));
    ui.painter().rect_filled(strip, CornerRadius::ZERO, sep());
}

/// Header column above the tracks: Live's locator Set / ◂ ▸ and the two view
/// switches (greyed: no locators here).
fn locator_area(ui: &mut egui::Ui, side: Rect) {
    let p = ui.painter();
    p.rect_filled(side, CornerRadius::ZERO, panel());
    p.rect_filled(Rect::from_min_max(pos2(side.min.x, side.max.y - 2.0), side.max), CornerRadius::ZERO, sep());
    let dimc = gray(0x62);
    let set = Rect::from_min_size(side.min + vec2(3.0, 23.0), vec2(30.0, 13.0));
    p.rect_filled(set, CornerRadius::same(6), gray(0x26));
    p.text(set.center(), Align2::CENTER_CENTER, "Set", font(FS), dimc);
    for i in 0..2 {
        let c = pos2(side.min.x + 10.0 + i as f32 * 16.0, side.min.y + 46.0);
        p.circle_filled(c, 6.0, gray(0x26));
        let d = if i == 0 { -1.0 } else { 1.0 };
        p.add(egui::Shape::convex_polygon(
            vec![c + vec2(2.5 * d, 0.0), c + vec2(-1.5 * d, -3.0), c + vec2(-1.5 * d, 3.0)],
            dimc,
            Stroke::NONE,
        ));
    }
    for i in 0..2 {
        let r = Rect::from_min_size(side.min + vec2(44.0 + i as f32 * (SOLO_W + GAP), 23.0), vec2(SOLO_W, BOX_H));
        p.rect_filled(r, CornerRadius::ZERO, gray(0x26));
        p.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, outline()), egui::StrokeKind::Inside);
        let c = r.center();
        if i == 0 {
            p.line_segment([c + vec2(-3.0, 3.0), c + vec2(3.0, -3.0)], Stroke::new(1.2, dimc));
        } else {
            p.rect_filled(Rect::from_center_size(c + vec2(0.0, 1.5), vec2(7.0, 5.0)), CornerRadius::ZERO, dimc);
            p.circle_stroke(c + vec2(0.0, -1.5), 2.5, Stroke::new(1.0, dimc));
        }
    }
    let zone = Rect::from_min_max(side.min + vec2(0.0, 20.0), pos2(side.min.x + 120.0, side.max.y - 2.0));
    ui.interact(zone, ui.scope_id().with("arr_locators"), Sense::hover()).on_hover_text("Locators (not available in this version)");
}

/// Bottom time ruler (min:sec) under the master row; header side: H / W and
/// zoom display (greyed).
fn time_ruler(app: &DawApp, ui: &mut egui::Ui, g: &Geo, lanes: Rect, head: Rect) {
    let p = ui.painter();
    p.rect_filled(Rect::from_min_max(lanes.min, pos2(head.max.x, lanes.min.y + 2.0)), CornerRadius::ZERO, sep());
    let r = Rect::from_min_max(pos2(lanes.min.x, lanes.min.y + 2.0), lanes.max);
    p.rect_filled(r, CornerRadius::ZERO, panel());
    let spb = 60.0 / app.project.tempo_bpm.max(1.0);
    let pr = ui.painter_at(r);
    let mark = gray(0x64);
    // 2 px ticks on each grid line; every n-th grid line (≥ 70 px apart) gets a └
    // marker hanging from the top edge with its exact time.
    let step = g.grid.max(1e-6);
    let tick = step * g.zoom >= 4.0;
    let mut n = 1i64;
    while step * n as f64 * g.zoom < 70.0 {
        n *= 2;
    }
    let mut i = (g.scroll / step).floor().max(0.0) as i64;
    while g.to_x(i as f64 * step) < r.max.x {
        let b = i as f64 * step;
        let x = g.to_x(b).round() + 0.5;
        if i % n == 0 {
            pr.line_segment([pos2(x, r.min.y), pos2(x, r.min.y + 8.0)], Stroke::new(1.0, mark));
            pr.line_segment([pos2(x, r.min.y + 7.5), pos2(x + 2.0, r.min.y + 7.5)], Stroke::new(1.0, mark));
            pr.text(pos2(x + 3.0, r.min.y + 9.5), Align2::LEFT_CENTER, time_label(b * spb), font(FS - 1.0), gray(0x8c));
        } else if tick {
            pr.line_segment([pos2(x, r.min.y), pos2(x, r.min.y + 2.0)], Stroke::new(1.0, gray(0x6b)));
        }
        i += 1;
    }
    let h = Rect::from_min_max(pos2(head.min.x, head.min.y + 2.0), head.max);
    p.rect_filled(h, CornerRadius::ZERO, panel());
    let dimc = gray(0x62);
    let mut x = h.min.x + 2.0;
    for (w, label) in [(15.0, "|||"), (36.0, "1.00x"), (15.0, "H"), (15.0, "W")] {
        let b = Rect::from_min_size(pos2(x, h.min.y + 4.0), vec2(w, 13.0));
        p.rect_filled(b, CornerRadius::ZERO, gray(0x26));
        p.rect_stroke(b, CornerRadius::ZERO, Stroke::new(1.0, outline()), egui::StrokeKind::Inside);
        p.text(b.center(), Align2::CENTER_CENTER, label, font(FS), dimc);
        x += w + if label == "1.00x" { 10.0 } else { 3.0 };
    }
    ui.interact(Rect::from_min_size(h.min, vec2(x - h.min.x, h.height())), ui.scope_id().with("arr_hw"), Sense::hover())
        .on_hover_text("Optimize Height / Width (not available in this version)");
}

/// m:ss, or m:ss.mmm with trailing zeros trimmed (Live's time ruler).
fn time_label(secs: f64) -> String {
    let ms = (secs * 1000.0).round().max(0.0) as i64;
    let (m, s, f) = (ms / 60_000, (ms / 1000) % 60, ms % 1000);
    if f == 0 {
        format!("{m}:{s:02}")
    } else {
        let frac = format!("{f:03}");
        format!("{m}:{s:02}.{}", frac.trim_end_matches('0'))
    }
}

/// Time selection x range on track `t` (pixel-snapped), if it covers it.
fn time_sel_x(app: &DawApp, g: &Geo, t: usize) -> Option<(f32, f32)> {
    let ts = app.time_sel.filter(|s| s.len() > 1e-9)?;
    if !(ts.t0..=ts.t1).contains(&t) {
        return None;
    }
    let x1 = g.to_x(ts.start).round();
    Some((x1, g.to_x(ts.end).round().max(x1 + 1.0)))
}

fn sel_bg() -> Color32 {
    sel()
}

/// Solo switch colour (Live blue).
const SOLO_BLUE: Color32 = Color32::from_rgb(0x5a, 0xa8, 0xff);

/// MIDI note activity of a track (0..1): highest velocity sounding at the playhead
/// (arrangement or session clip), or recent input while armed / monitoring.
fn midi_activity(app: &DawApp, t: usize) -> f32 {
    let tr = &app.project.tracks[t];
    // Loudest note sounding at content position `u`.
    let at_u = |c: &Clip, u: f64| -> f32 {
        let ClipContent::Midi { notes } = &c.content else { return 0.0 };
        notes.iter().filter(|n| !n.muted && n.start <= u && u < n.start + n.length).map(|n| n.velocity as f32 / 127.0).fold(0.0, f32::max)
    };
    let at = |c: &Clip, local: f64| -> f32 {
        let Some(&(l0, _, u0)) = play_segments(c).iter().find(|(l0, l1, _)| local >= *l0 && local < *l1) else { return 0.0 };
        at_u(c, u0 + (local - l0))
    };
    let mut v = 0.0f32;
    if app.status.playing {
        let st = app.track_status.get(t);
        if let Some(sc) = st.and_then(|s| s.playing_scene) {
            if let Some(Some(c)) = tr.slots.get(sc as usize) {
                v = at_u(c, c.wrap(st.map_or(0.0, |s| s.clip_pos)));
            }
        } else {
            let pos = app.status.beat_position;
            if let Some(a) = tr.arrangement.iter().find(|a| pos >= a.start && pos < a.start + a.clip.length) {
                v = at(&a.clip, pos - a.start);
            }
        }
    }
    if tr.arm {
        v = v.max(super::io::activity_level(app.midi.activity.age(&app.midi.activity.track_in)));
    }
    v
}

/// Live MIDI activity meter, adaptive (hh-midi-dots-adaptive): tracks with at
/// least 88 px of content get 6 px dots on an 8 px pitch from the top + 6;
/// shorter ones get 2 px dots on a 4 px pitch from the top + 4, filling the
/// height. `r` is the track content inset by 2 px vertically. Lit bottom-up.
pub fn dot_meter(p: &egui::Painter, r: Rect, level: f32) {
    let (top, h) = (r.min.y - 2.0, r.height() + 4.0);
    let big = h >= 88.0;
    let (pitch, first, rad) = if big { (8.0, 9.0, 3.0) } else { (4.0, 5.0, 1.15) };
    let n = if big { ((h - 8.0) / 8.0).floor() } else { ((h - 5.0) / 4.0).floor() }.max(1.0) as usize;
    let lit = (level * n as f32).round() as usize;
    for i in 0..n {
        let c = pos2(r.center().x, top + first + i as f32 * pitch);
        let k = n - 1 - i; // index from the bottom
        let col = if k < lit {
            let f = k as f32 / n as f32;
            if f > 0.85 {
                Color32::from_rgb(0xff, 0x4a, 0x3a)
            } else if f > 0.65 {
                Color32::from_rgb(0xff, 0xd0, 0x40)
            } else {
                Color32::from_rgb(0x5f, 0xd2, 0x5f)
            }
        } else {
            gray(0x14)
        };
        p.circle_filled(c, rad, col);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn time_labels_trim_zeros() {
        assert_eq!(super::time_label(0.0), "0:00");
        assert_eq!(super::time_label(2.5), "0:02.5");
        assert_eq!(super::time_label(61.25), "1:01.25");
        assert_eq!(super::time_label(0.125), "0:00.125");
    }
}
