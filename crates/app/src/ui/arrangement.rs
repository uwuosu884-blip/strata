//! Arrangement view: timeline with clips per track, ruler (position + loop),
//! playhead, drag/resize/create clips, drops from the browser/session.

use super::widgets::*;
use crate::app::{ClipRef, DawApp, DragItem, TrackFlag};
use daw_engine::{Command, SampleBuffer};
use daw_project::{ArrangementClip, Clip, ClipContent, TrackKind};
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};

const HEAD_W: f32 = 130.0;
const RULER_H: f32 = 30.0;
const ROW_H: f32 = 58.0;
const EDGE: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Move,
    Resize,
}

#[derive(Clone, Copy, Debug)]
struct Drag {
    track: usize,
    index: usize,
    id: u64,
    mode: Mode,
    grab: f64,
    orig_start: f64,
    orig_len: f64,
}

pub fn arrangement_view(app: &mut DawApp, ui: &mut egui::Ui) {
    let full = ui.available_rect_before_wrap();
    let zoom = app.arr_zoom as f64;
    let bpb = app.project.beats_per_bar.max(1) as f64;
    let timeline_x0 = full.min.x + HEAD_W;
    let to_x = |b: f64, scroll: f64| timeline_x0 + ((b - scroll) * zoom) as f32;

    // Zoom (Ctrl+wheel) and horizontal scroll (wheel / Shift+wheel) over the timeline.
    let hover = ui.rect_contains_pointer(full);
    if hover {
        let (scroll, cmd, shift, pointer) =
            ui.input(|i| (i.smooth_scroll_delta, i.modifiers.command, i.modifiers.shift, i.pointer.hover_pos()));
        if cmd && scroll.y != 0.0 {
            let px = pointer.map_or(timeline_x0, |p| p.x);
            let beat_at = app.arr_scroll as f64 + ((px - timeline_x0) as f64 / zoom);
            app.arr_zoom = (app.arr_zoom * (1.0 + scroll.y * 0.003)).clamp(3.0, 400.0);
            app.arr_scroll = (beat_at - (px - timeline_x0) as f64 / app.arr_zoom as f64).max(0.0) as f32;
        } else if shift || scroll.x != 0.0 {
            let d = if shift { scroll.y + scroll.x } else { scroll.x };
            app.arr_scroll = (app.arr_scroll - d / app.arr_zoom).max(0.0);
        }
    }
    let zoom = app.arr_zoom as f64;
    let scroll = app.arr_scroll as f64;
    let grid = grid_for_zoom(zoom, bpb);
    app.grid = grid;
    let snap = |b: f64| (b / grid).round() * grid;

    // ---- Ruler ---------------------------------------------------------------
    let ruler = Rect::from_min_size(full.min, vec2(full.width(), RULER_H));
    let ruler_tl = Rect::from_min_max(pos2(timeline_x0, ruler.min.y), ruler.max);
    let resp = ui.interact(ruler_tl, ui.scope_id().with("ruler"), Sense::click_and_drag());
    let p = ui.painter_at(ruler);
    p.rect_filled(ruler, CornerRadius::ZERO, PANEL_BG);
    let beat_at = |x: f32| (scroll + (x - timeline_x0) as f64 / zoom).max(0.0);
    let loop_band = Rect::from_min_max(ruler_tl.min, pos2(ruler_tl.max.x, ruler_tl.min.y + 12.0));
    // Loop brace
    let (ls, le) = (app.project.loop_start, app.project.loop_end);
    let brace = Rect::from_min_max(pos2(to_x(ls, scroll), loop_band.min.y + 2.0), pos2(to_x(le, scroll), loop_band.max.y));
    p.rect_filled(brace, CornerRadius::same(2), if app.project.loop_enabled { shade(ACCENT, 0.9) } else { Color32::from_gray(90) });
    // Bars
    let first_bar = (scroll / bpb).floor() as i64;
    let bar_px = bpb * zoom;
    let label_every = ((50.0 / bar_px).ceil() as i64).max(1);
    let mut bar = first_bar.max(0);
    while to_x(bar as f64 * bpb, scroll) < full.max.x {
        let x = to_x(bar as f64 * bpb, scroll);
        if bar % label_every == 0 {
            p.line_segment([pos2(x, ruler.min.y + 14.0), pos2(x, ruler.max.y)], Stroke::new(1.0, Color32::from_gray(140)));
            p.text(
                pos2(x + 3.0, ruler.min.y + 14.0),
                Align2::LEFT_TOP,
                format!("{}", bar + 1),
                FontId::proportional(11.0),
                Color32::from_gray(200),
            );
        }
        bar += 1;
    }
    // Ruler interactions: drag in the loop band sets the loop, click below moves the playhead.
    let loop_drag_id = ui.scope_id().with("loopdrag");
    if resp.drag_started()
        && let Some(pos) = ui.input(|i| i.pointer.press_origin())
        && loop_band.contains(pos)
    {
        app.begin_gesture();
        ui.data_mut(|d| d.insert_temp(loop_drag_id, snap(beat_at(pos.x))));
    }
    if resp.dragged()
        && let Some(anchor) = ui.data(|d| d.get_temp::<f64>(loop_drag_id))
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let b = snap(beat_at(pos.x));
        let (s, e) = if b >= anchor { (anchor, b) } else { (b, anchor) };
        if e - s >= grid {
            app.project.loop_start = s;
            app.project.loop_end = e;
            app.project.loop_enabled = true;
            app.send_loop();
        }
    }
    if resp.drag_stopped() {
        ui.data_mut(|d| d.remove::<f64>(loop_drag_id));
    }
    if resp.clicked()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        if loop_band.contains(pos) && brace.contains(pos) {
            let on = !app.project.loop_enabled;
            app.edit(|p| p.loop_enabled = on);
            app.send_loop();
        } else {
            app.send_now(Command::SetPosition(snap(beat_at(pos.x))));
        }
    }
    p.text(
        ruler.left_center() + vec2(6.0, 0.0),
        Align2::LEFT_CENTER,
        format!("Grid {}", grid_label(grid)),
        FontId::proportional(11.0),
        Color32::GRAY,
    );

    // ---- Tracks --------------------------------------------------------------
    let body = Rect::from_min_max(pos2(full.min.x, full.min.y + RULER_H), full.max);
    ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
        egui::ScrollArea::vertical().id_salt("arr_rows").auto_shrink([false, false]).show(ui, |ui| {
            let n = app.project.tracks.len();
            let height = (n as f32 * ROW_H + ROW_H).max(body.height());
            let (area, _) = ui.allocate_exact_size(vec2(body.width(), height), Sense::hover());
            let p = ui.painter_at(area.intersect(body));
            // Grid lines
            let mut b = (scroll / grid).floor() * grid;
            while to_x(b, scroll) < area.max.x {
                let x = to_x(b, scroll);
                let on_bar = (b / bpb).fract().abs() < 1e-9;
                let on_beat = b.fract().abs() < 1e-9;
                let c = if on_bar {
                    Color32::from_gray(70)
                } else if on_beat {
                    Color32::from_gray(48)
                } else {
                    Color32::from_gray(38)
                };
                p.line_segment([pos2(x, area.min.y), pos2(x, area.max.y)], Stroke::new(1.0, c));
                b += grid;
            }
            if app.project.loop_enabled {
                let r = Rect::from_min_max(pos2(to_x(ls, scroll), area.min.y), pos2(to_x(le, scroll), area.max.y));
                p.rect_filled(r, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(255, 160, 40, 12));
            }
            for t in 0..n {
                let row = Rect::from_min_size(pos2(area.min.x, area.min.y + t as f32 * ROW_H), vec2(area.width(), ROW_H - 1.0));
                track_row(app, ui, t, row, timeline_x0, scroll, zoom, &snap);
            }
            // Drop below the last track = new track
            let empty = Rect::from_min_size(pos2(area.min.x, area.min.y + n as f32 * ROW_H), vec2(area.width(), ROW_H));
            let r = ui.interact(empty, ui.scope_id().with("arr_empty"), Sense::click());
            if r.dnd_hover_payload::<DragItem>().is_some() {
                p.rect_stroke(empty.shrink(2.0), CornerRadius::same(3), Stroke::new(1.0, ACCENT), egui::StrokeKind::Inside);
            }
            p.text(
                empty.left_center() + vec2(8.0, 0.0),
                Align2::LEFT_CENTER,
                "Drop here to add a track",
                FontId::proportional(11.0),
                Color32::from_gray(90),
            );
            if let Some(item) = r.dnd_release_payload::<DragItem>()
                && let DragItem::Sample { key, name } = &*item
                && let Some(mut clip) = app.audio_clip_for(key, name)
            {
                let at = r.interact_pointer_pos().map_or(0.0, |p| snap(beat_at(p.x)));
                app.edit(|p| {
                    let t = p.add_track(TrackKind::Audio);
                    p.tracks[t].name = name.clone();
                    clip.id = p.alloc_id();
                    clip.looping = false;
                    p.tracks[t].arrangement.push(ArrangementClip { start: at, clip });
                });
            }
            r.context_menu(|ui| {
                if ui.button("Add MIDI Track").clicked() {
                    app.add_track(TrackKind::Midi);
                    ui.close();
                }
                if ui.button("Add Audio Track").clicked() {
                    app.add_track(TrackKind::Audio);
                    ui.close();
                }
            });
        });
    });

    // Playhead over everything.
    let x = to_x(app.status.beat_position, scroll);
    if x >= timeline_x0 && x <= full.max.x {
        ui.painter().line_segment([pos2(x, full.min.y), pos2(x, full.max.y)], Stroke::new(1.5, Color32::WHITE));
    }
    // Follow playhead while playing.
    if app.status.playing {
        let view_beats = (full.width() - HEAD_W) as f64 / zoom;
        let pos = app.status.beat_position;
        if pos > scroll + view_beats * 0.9 || pos < scroll {
            app.arr_scroll = (pos - view_beats * 0.1).max(0.0) as f32;
        }
    }
    ui.allocate_rect(full, Sense::hover());
}

#[allow(clippy::too_many_arguments)]
fn track_row(app: &mut DawApp, ui: &mut egui::Ui, t: usize, row: Rect, x0: f32, scroll: f64, zoom: f64, snap: &dyn Fn(f64) -> f64) {
    let to_x = |b: f64| x0 + ((b - scroll) * zoom) as f32;
    let beat_at = |x: f32| (scroll + (x - x0) as f64 / zoom).max(0.0);
    let painter = ui.painter_at(row);
    let head = Rect::from_min_size(row.min, vec2(HEAD_W - 2.0, row.height()));
    let lane = Rect::from_min_max(pos2(x0, row.min.y), row.max);
    let track = &app.project.tracks[t];
    let color = rgb(track.color);
    let selected = app.selected_track == t;
    let (mute, solo, arm) = (track.mute, track.solo, track.arm);

    // Header
    painter.rect_filled(head, CornerRadius::same(2), if selected { Color32::from_gray(64) } else { Color32::from_gray(48) });
    painter.rect_filled(Rect::from_min_size(head.min, vec2(5.0, head.height())), CornerRadius::ZERO, color);
    painter.with_clip_rect(head.shrink(2.0)).text(
        head.min + vec2(10.0, 6.0),
        Align2::LEFT_TOP,
        &track.name,
        FontId::proportional(12.0),
        Color32::WHITE,
    );
    let hr = ui.interact(head, ui.scope_id().with(("arr_head", t)), Sense::click());
    if hr.clicked() {
        app.selected_track = t;
    }
    if let Some(item) = hr.dnd_release_payload::<DragItem>()
        && let Some(def) = super::instrument_for(&item)
    {
        app.set_instrument(t, def);
    }
    hr.context_menu(|ui| super::session::track_menu(app, ui, t));
    let mut bui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_size(head.min + vec2(10.0, 28.0), vec2(HEAD_W - 14.0, 22.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    bui.spacing_mut().item_spacing.x = 3.0;
    if toggle_box(&mut bui, &format!("{}", t + 1), !mute, ACCENT, "Track activator").clicked() {
        app.toggle_track_flag(t, TrackFlag::Mute, false);
    }
    let excl = !ui.input(|i| i.modifiers.command);
    if toggle_box(&mut bui, "S", solo, Color32::from_rgb(80, 160, 255), "Solo").clicked() {
        app.toggle_track_flag(t, TrackFlag::Solo, excl);
    }
    if toggle_box(&mut bui, "REC", arm, REC_RED, "Arm").clicked() {
        app.toggle_track_flag(t, TrackFlag::Arm, excl);
    }
    if app.track_status.get(t).is_some_and(|s| s.arrangement_off) {
        bui.label(egui::RichText::new("session").small().color(ACCENT))
            .on_hover_text("A session clip overrides the arrangement on this track");
    }

    // Lane background + interactions
    painter.rect_filled(lane, CornerRadius::ZERO, Color32::from_black_alpha(if selected { 0 } else { 30 }));
    let lr = ui.interact(lane, ui.scope_id().with(("lane", t)), Sense::click());
    if lr.double_clicked()
        && app.project.tracks[t].kind == TrackKind::Midi
        && let Some(pos) = lr.interact_pointer_pos()
    {
        let bpb = app.project.beats_per_bar as f64;
        let start = (beat_at(pos.x) / bpb).floor() * bpb;
        let idx = app.edit(|p| {
            let id = p.alloc_id();
            let tr = &mut p.tracks[t];
            tr.arrangement.push(ArrangementClip {
                start,
                clip: Clip {
                    id,
                    name: format!("Clip {id}"),
                    color: tr.color,
                    length: bpb,
                    looping: false,
                    content: ClipContent::Midi { notes: vec![] },
                },
            });
            tr.arrangement.sort_by(|a, b| a.start.total_cmp(&b.start));
            tr.arrangement.iter().position(|a| a.clip.id == id)
        });
        app.selected_clip = idx.map(|index| ClipRef::Arr { track: t, index });
    } else if lr.clicked() {
        app.selected_track = t;
        app.selected_clip = None;
    }
    if lr.dnd_hover_payload::<DragItem>().is_some() {
        painter.rect_stroke(lane, CornerRadius::ZERO, Stroke::new(1.0, ACCENT), egui::StrokeKind::Inside);
    }
    if let Some(item) = lr.dnd_release_payload::<DragItem>() {
        let at = lr.interact_pointer_pos().map_or(0.0, |p| snap(beat_at(p.x)));
        drop_on_lane(app, &item, t, at);
    }

    // Clips
    let drag_id = ui.scope_id().with("arr_clip_drag");
    let n = app.project.tracks[t].arrangement.len();
    for i in 0..n {
        let a = app.project.tracks[t].arrangement[i].clone();
        let (x1, x2) = (to_x(a.start), to_x(a.start + a.clip.length));
        if x2 < lane.min.x || x1 > lane.max.x {
            continue;
        }
        let r = Rect::from_min_max(pos2(x1, row.min.y + 1.0), pos2(x2.max(x1 + 3.0), row.max.y - 1.0));
        let sel = app.selected_clip == Some(ClipRef::Arr { track: t, index: i });
        draw_clip(
            &painter.with_clip_rect(lane),
            r,
            &a.clip,
            sel,
            app.cache.peek(sample_key(&a.clip)).map(|b| &**b),
            app.audio.info.sample_rate,
            zoom,
            app.project.tempo_bpm,
        );

        let cr = ui.interact(r.intersect(lane), ui.scope_id().with(("aclip", t, i, a.clip.id)), Sense::click_and_drag());
        let near_edge = cr.hover_pos().is_some_and(|p| p.x > r.max.x - EDGE);
        if near_edge || ui.data(|d| d.get_temp::<Drag>(drag_id)).is_some_and(|d| d.mode == Mode::Resize && d.id == a.clip.id) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if cr.clicked() {
            app.selected_clip = Some(ClipRef::Arr { track: t, index: i });
            app.selected_track = t;
        }
        if cr.drag_started()
            && let Some(pos) = ui.input(|i| i.pointer.press_origin())
        {
            app.begin_gesture();
            let mode = if pos.x > r.max.x - EDGE { Mode::Resize } else { Mode::Move };
            let d = Drag {
                track: t,
                index: i,
                id: a.clip.id,
                mode,
                grab: beat_at(pos.x) - a.start,
                orig_start: a.start,
                orig_len: a.clip.length,
            };
            ui.data_mut(|m| m.insert_temp(drag_id, d));
            app.selected_clip = Some(ClipRef::Arr { track: t, index: i });
        }
        if cr.dragged()
            && let Some(d) = ui.data(|m| m.get_temp::<Drag>(drag_id))
            && d.id == app.project.tracks[t].arrangement[i].clip.id
            && let Some(pos) = cr.interact_pointer_pos()
        {
            let a = &mut app.project.tracks[t].arrangement[i];
            match d.mode {
                Mode::Move => a.start = snap(beat_at(pos.x) - d.grab).max(0.0),
                Mode::Resize => a.clip.length = (snap(beat_at(pos.x)) - a.start).max(app.grid.max(1.0 / 16.0)),
            }
        }
        if cr.drag_stopped()
            && let Some(d) = ui.data(|m| m.get_temp::<Drag>(drag_id))
        {
            ui.data_mut(|m| m.remove::<Drag>(drag_id));
            finish_drag(app, d, ui.ctx().pointer_latest_pos(), row.min.y);
            return; // indices changed
        }
        cr.context_menu(|ui| {
            if ui.button("Duplicate  Ctrl+D").clicked() {
                app.selected_clip = Some(ClipRef::Arr { track: t, index: i });
                app.duplicate_selected_clip();
                ui.close();
            }
            if ui.button("Delete  Del").clicked() {
                app.selected_clip = Some(ClipRef::Arr { track: t, index: i });
                app.delete_selected_clip();
                ui.close();
            }
        });
    }
}

/// End of a clip drag: move to the track under the pointer (same kind), re-sort, keep selection.
fn finish_drag(app: &mut DawApp, d: Drag, pointer: Option<Pos2>, row_top: f32) {
    let p = &mut app.project;
    let mut target = d.track;
    if d.mode == Mode::Move
        && let Some(pos) = pointer
    {
        let t = d.track as i64 + ((pos.y - row_top) / ROW_H).floor() as i64;
        if t >= 0 && (t as usize) < p.tracks.len() && t as usize != d.track {
            let t = t as usize;
            let is_audio = p.tracks[d.track]
                .arrangement
                .iter()
                .find(|a| a.clip.id == d.id)
                .is_some_and(|a| matches!(a.clip.content, ClipContent::Audio { .. }));
            if is_audio == (p.tracks[t].kind == TrackKind::Audio)
                && let Some(i) = p.tracks[d.track].arrangement.iter().position(|a| a.clip.id == d.id)
            {
                let a = p.tracks[d.track].arrangement.remove(i);
                p.tracks[t].arrangement.push(a);
                target = t;
            }
        }
    }
    let tr = &mut p.tracks[target];
    tr.arrangement.sort_by(|a, b| a.start.total_cmp(&b.start));
    let idx = tr.arrangement.iter().position(|a| a.clip.id == d.id);
    app.selected_clip = idx.map(|index| ClipRef::Arr { track: target, index });
    app.selected_track = target;
    app.needs_compile = true;
    let _ = (d.orig_start, d.orig_len, d.index);
}

fn drop_on_lane(app: &mut DawApp, item: &DragItem, t: usize, at: f64) {
    let clip = match item {
        DragItem::Sample { key, name } => {
            if app.project.tracks[t].kind != TrackKind::Audio {
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
                audio == (app.project.tracks[t].kind == TrackKind::Audio)
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
        let idx = app.edit(|p| {
            clip.id = p.alloc_id();
            clip.looping = false;
            let id = clip.id;
            let tr = &mut p.tracks[t];
            tr.arrangement.push(ArrangementClip { start: at, clip });
            tr.arrangement.sort_by(|a, b| a.start.total_cmp(&b.start));
            tr.arrangement.iter().position(|a| a.clip.id == id)
        });
        app.selected_clip = idx.map(|index| ClipRef::Arr { track: t, index });
    }
}

fn sample_key(c: &Clip) -> &str {
    match &c.content {
        ClipContent::Audio { sample, .. } => sample,
        _ => "",
    }
}

/// Draw a clip body with a MIDI or waveform preview.
#[allow(clippy::too_many_arguments)]
pub fn draw_clip(p: &egui::Painter, r: Rect, clip: &Clip, selected: bool, sample: Option<&SampleBuffer>, sr: u32, zoom: f64, tempo: f64) {
    let c = rgb(clip.color);
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
            for n in notes.iter().filter(|n| n.start < clip.length) {
                let x1 = body.min.x + (n.start * zoom) as f32;
                let x2 = (x1 + (n.length.min(clip.length - n.start) * zoom) as f32).max(x1 + 1.0);
                if x2 < visible.min.x || x1 > visible.max.x {
                    continue;
                }
                let y = body.max.y - ((n.pitch - lo) as f32 + 1.0) / span * body.height();
                p.rect_filled(Rect::from_min_max(pos2(x1, y), pos2(x2, y + nh)), CornerRadius::ZERO, ink);
            }
        }
        ClipContent::Audio { offset_frames, warp_bpm, .. } => {
            let Some(buf) = sample else {
                p.text(body.center(), Align2::CENTER_CENTER, "loading…", FontId::proportional(10.0), Color32::GRAY);
                return;
            };
            let frames_per_beat = 60.0 / warp_bpm.unwrap_or(tempo) * buf.sample_rate as f64;
            let _ = sr;
            draw_wave(p, body, visible, buf, *offset_frames, frames_per_beat / zoom, clip.length, clip.looping, ink);
        }
    }
}

/// Min/max waveform, one column per pixel, from the precomputed peak blocks.
#[allow(clippy::too_many_arguments)]
pub fn draw_wave(
    p: &egui::Painter,
    body: Rect,
    visible: Rect,
    buf: &SampleBuffer,
    offset: f64,
    frames_per_px: f64,
    len_beats: f64,
    looping: bool,
    ink: Color32,
) {
    if buf.is_empty() || buf.peaks.is_empty() {
        return;
    }
    let mid = body.center().y;
    let h = body.height() * 0.5;
    let block = daw_engine::PEAK_BLOCK as f64;
    let total = buf.len() as f64;
    let _ = len_beats;
    let mut x = visible.min.x.floor();
    while x < visible.max.x {
        let f0 = offset + (x - body.min.x) as f64 * frames_per_px;
        let f1 = f0 + frames_per_px.max(1.0);
        let (f0, f1) = if looping { (f0 % total, f0 % total + (f1 - f0)) } else { (f0, f1) };
        if f0 >= total {
            break;
        }
        let (mut lo, mut hi) = (0.0f32, 0.0f32);
        if frames_per_px < block * 2.0 {
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
        p.line_segment([pos2(x, mid - hi * h), pos2(x, mid - lo * h + 0.5)], Stroke::new(1.0, ink));
        x += 1.0;
    }
}

fn grid_for_zoom(zoom: f64, bpb: f64) -> f64 {
    // Aim for >= 12 px per grid line.
    for g in [0.25, 0.5, 1.0] {
        if g * zoom >= 12.0 {
            return g;
        }
    }
    let mut g = bpb;
    while g * zoom < 12.0 {
        g *= 2.0;
    }
    g
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

#[cfg(test)]
mod tests {
    #[test]
    fn grid_adapts_to_zoom() {
        assert_eq!(super::grid_for_zoom(100.0, 4.0), 0.25);
        assert_eq!(super::grid_for_zoom(30.0, 4.0), 0.5);
        assert_eq!(super::grid_for_zoom(5.0, 4.0), 4.0);
        assert_eq!(super::grid_for_zoom(1.0, 4.0), 16.0);
    }
}
