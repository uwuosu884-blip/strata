//! Detail panel: clip editor (piano roll / audio clip) or the selected track's device.

use super::widgets::*;
use crate::app::{ClipRef, DawApp, DragItem};
use daw_engine::{DRUM_BASE_NOTE, DRUM_PADS, Wave};
use daw_project::{Clip, ClipContent, InstrumentDef, Note, PadDef, TrackKind};
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, pos2, vec2};

const KEYS_W: f32 = 46.0;
const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

pub fn note_name(p: u8) -> String {
    format!("{}{}", NOTE_NAMES[(p % 12) as usize], p as i32 / 12 - 2)
}

pub fn detail_panel(app: &mut DawApp, ui: &mut egui::Ui) {
    // A resizable panel stores its *content* size: fill exactly the user-set size so
    // switching between the (tall) piano roll and a short device view never changes it.
    let full = ui.max_rect().size();
    ui.set_min_size(full);
    ui.set_max_size(full);
    if app.project.tracks.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new("Add a track: Ctrl+T (audio) or Ctrl+Shift+T (MIDI)").color(Color32::GRAY))
        });
        return;
    }
    // Clip view only for a clip on the selected track (selecting another track,
    // e.g. by its header, shows that track's devices). Shift+Tab toggles.
    let clip = app.selected_clip.filter(|&c| {
        let t = match c {
            ClipRef::Slot { track, .. } | ClipRef::Arr { track, .. } => track,
        };
        t == app.selected_track && app.clip(c).is_some()
    });
    match (app.detail_mode, clip) {
        (crate::app::DetailMode::Clip, Some(c)) => clip_editor(app, ui, c),
        (crate::app::DetailMode::Clip, None) => {
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new("No clip selected on this track · Shift+Tab: devices").color(Color32::GRAY))
            });
        }
        (crate::app::DetailMode::Device, _) => {
            egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| device_panel(app, ui));
        }
    }
}

// ============================ Clip editor ============================

fn clip_editor(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef) {
    let track = match cref {
        ClipRef::Slot { track, .. } | ClipRef::Arr { track, .. } => track,
    };
    let clip = app.clip(cref).cloned().expect("checked");
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(170.0);
            clip_props(app, ui, cref, &clip);
        });
        ui.separator();
        match &clip.content {
            ClipContent::Midi { notes } => piano_roll(app, ui, cref, track, &clip, notes),
            ClipContent::Audio { sample, .. } => {
                let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
                let p = ui.painter_at(rect);
                p.rect_filled(rect, CornerRadius::same(2), Color32::from_gray(22));
                let zoom = rect.width() as f64 / clip.length.max(0.25);
                match app.cache.peek(sample) {
                    Some(buf) => {
                        let mut c = clip.clone();
                        c.name.clear();
                        super::arrangement::draw_clip(&p, rect, &c, false, Some(buf), zoom, app.project.tempo_bpm, false);
                    }
                    None => {
                        let msg = app.cache.error(sample).map_or("Loading…".to_string(), |e| format!("Missing sample: {e}"));
                        p.text(rect.center(), Align2::CENTER_CENTER, msg, FontId::proportional(12.0), Color32::GRAY);
                    }
                }
                playhead(app, &p, rect, cref, track, clip.length, rect.min.x);
            }
        }
    });
}

fn clip_props(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef, clip: &Clip) {
    // Clip name bar: double-click (or Ctrl+R) renames.
    let (r, nr) = ui.allocate_exact_size(vec2(170.0, 18.0), Sense::click());
    let renaming = app.renaming.as_ref().is_some_and(|x| x.clip && x.id == clip.id && x.in_detail);
    if renaming {
        super::rename_editor(app, ui, r, clip.id, true, true);
    } else {
        ui.painter().rect_filled(r, CornerRadius::same(2), rgb(clip.color));
        ui.painter().with_clip_rect(r).text(
            r.left_center() + vec2(4.0, 0.0),
            Align2::LEFT_CENTER,
            &clip.name,
            FontId::proportional(12.0),
            text_on(rgb(clip.color)),
        );
        if nr.on_hover_text("Double-click or Ctrl+R: rename").double_clicked() {
            app.start_rename_clip(clip.id, true);
        }
    }
    let mut c = clip.clone();
    let mut gesture = false;
    let mut changed = false;
    let bpb = app.project.beats_per_bar as f64;
    egui::Grid::new("clip_props").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
        ui.label("Length");
        let r = ui.add(
            egui::DragValue::new(&mut c.length)
                .range(0.25..=4096.0)
                .speed(0.05)
                .custom_formatter(move |v, _| {
                    format!("{:.0}.{:.0}.{:.0}", (v / bpb).floor(), (v % bpb).floor(), ((v.fract()) * 4.0).round())
                })
                .custom_parser(|s| s.parse().ok()),
        );
        gesture |= r.drag_started() || r.gained_focus();
        changed |= r.changed();
        ui.end_row();
        ui.label("Loop");
        let r = ui.checkbox(&mut c.looping, "");
        gesture |= r.clicked();
        changed |= r.changed();
        ui.end_row();
        ui.label("Active");
        let mut active = !c.muted;
        let r = ui.checkbox(&mut active, "").on_hover_text("Deactivate / activate (0)");
        if r.changed() {
            c.muted = !active;
            gesture = true;
            changed = true;
        }
        ui.end_row();
        if let ClipContent::Audio { gain_db, warp_bpm, offset_frames, sample } = &mut c.content {
            ui.label("Gain");
            let r = ui.add(egui::DragValue::new(gain_db).range(-60.0..=24.0).speed(0.1).suffix(" dB"));
            gesture |= r.drag_started() || r.gained_focus();
            changed |= r.changed();
            ui.end_row();
            ui.label("Warp");
            let mut warp = warp_bpm.is_some();
            let r = ui.checkbox(&mut warp, "follow tempo");
            if r.changed() {
                gesture = true;
                changed = true;
                *warp_bpm = if warp { Some(app.project.tempo_bpm) } else { None };
            }
            ui.end_row();
            if let Some(bpm) = warp_bpm.as_mut() {
                ui.label("Orig. BPM");
                let r = ui.add(egui::DragValue::new(bpm).range(20.0..=999.0).speed(0.1).fixed_decimals(2));
                gesture |= r.drag_started() || r.gained_focus();
                changed |= r.changed();
                ui.end_row();
            }
            ui.label("Start");
            let r = ui.add(egui::DragValue::new(offset_frames).range(0.0..=f64::MAX).speed(50.0).suffix(" smp"));
            gesture |= r.drag_started() || r.gained_focus();
            changed |= r.changed();
            ui.end_row();
            ui.label("File");
            let name = std::path::Path::new(sample.as_str())
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| sample.clone());
            ui.label(egui::RichText::new(name).small()).on_hover_text(sample.as_str());
            ui.end_row();
        }
    });
    if gesture {
        app.begin_gesture();
    }
    if changed {
        if let Some(dst) = DawApp::clip_mut(&mut app.project, cref) {
            *dst = c;
        }
        app.needs_compile = true;
        app.dirty = true;
    }
    if let ClipContent::Midi { .. } = clip.content {
        ui.horizontal_wrapped(|ui| {
            let grid = app.pr_grid_now;
            let mut op: Option<NoteOp> = None;
            if ui.button("Quantize").on_hover_text("Snap note starts to the grid").clicked() {
                op = Some(Box::new(move |n: &mut Vec<Note>, _| n.iter_mut().for_each(|x| x.start = (x.start / grid).round() * grid)));
            }
            // These act on the selected notes only.
            let any = !app.note_sel.is_empty();
            if ui.add_enabled(any, egui::Button::new("×2")).on_hover_text("Stretch selected notes").clicked() {
                app.scale_selected_notes(2.0);
            }
            if ui.add_enabled(any, egui::Button::new("÷2")).on_hover_text("Compress selected notes").clicked() {
                app.scale_selected_notes(0.5);
            }
            if ui.add_enabled(any, egui::Button::new("+12")).on_hover_text("Selected notes up an octave (Shift+Up)").clicked() {
                app.transpose_selected_notes(12);
            }
            if ui.add_enabled(any, egui::Button::new("-12")).on_hover_text("Selected notes down an octave (Shift+Down)").clicked() {
                app.transpose_selected_notes(-12);
            }
            if ui.button("Clear").clicked() {
                op = Some(Box::new(|n: &mut Vec<Note>, _| n.clear()));
            }
            if let Some(f) = op {
                app.note_sel.clear();
                app.edit(|p| {
                    if let Some(Clip { content: ClipContent::Midi { notes }, length, .. }) = DawApp::clip_mut(p, cref) {
                        f(notes, length);
                        notes.sort_by(|a, b| a.start.total_cmp(&b.start));
                    }
                });
            }
        });
        ui.label(
            egui::RichText::new(format!(
                "Grid {} · Alt: off grid\ndbl-click: add/remove · drag: move\nCtrl+drag: copy · edge: resize\nempty: select · Ctrl+D: duplicate\nCtrl/Alt+wheel: zoom",
                if app.pr_grid.snaps() { super::arrangement::grid_label(app.pr_grid_now) } else { "off".into() }
            ))
            .small()
            .color(Color32::GRAY),
        );
    }
}

fn playhead(app: &DawApp, p: &egui::Painter, rect: Rect, cref: ClipRef, track: usize, length: f64, x0: f32) {
    let pos = match cref {
        ClipRef::Slot { scene, .. } => {
            let st = app.track_status.get(track).copied().unwrap_or_default();
            (st.playing_scene == Some(scene as u32)).then_some(st.clip_pos)
        }
        ClipRef::Arr { track, index } => {
            let a = &app.project.tracks[track].arrangement[index];
            let b = app.status.beat_position - a.start;
            (app.status.playing && (0.0..a.clip.length).contains(&b) && !app.track_status.get(track).is_some_and(|s| s.arrangement_off))
                .then_some(b)
        }
    };
    if let Some(pos) = pos {
        let x = x0 + ((pos / length) as f32) * (rect.max.x - x0);
        p.line_segment([pos2(x, rect.min.y), pos2(x, rect.max.y)], Stroke::new(1.5, Color32::WHITE));
    }
}

#[derive(Clone, Copy, Debug)]
enum NoteDrag {
    Move {
        grab_beat: f64,
        grab_pitch: i32,
        orig: Option<(f64, u8)>,
    },
    /// Note-end drag; `grab` = the note under the pointer. Every selected note
    /// keeps its own length and changes by the same delta.
    Resize {
        grab: usize,
    },
    Velocity,
    /// Rubber-band selection; `base` = selection kept when Shift/Ctrl was held.
    Marquee,
    /// Playhead scrub in the ruler.
    Ruler,
}

const PR_RULER_H: f32 = 16.0;

fn piano_roll(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef, track: usize, clip: &Clip, notes: &[Note]) {
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::ZERO, Color32::from_gray(26));
    let drums: Option<Vec<String>> = match &app.project.tracks[track].instrument {
        InstrumentDef::Drums { pads } => Some(pads.iter().map(|d| d.name.clone()).collect()),
        _ => None,
    };
    // Velocity lane height: user-resizable via the splitter above it.
    let vel_h = app.pr_vel_h.clamp(24.0, (rect.height() * 0.6).max(24.0));
    let ruler = Rect::from_min_max(pos2(rect.min.x + KEYS_W, rect.min.y), pos2(rect.max.x, rect.min.y + PR_RULER_H));
    let grid_area = Rect::from_min_max(pos2(rect.min.x + KEYS_W, ruler.max.y), pos2(rect.max.x, rect.max.y - vel_h));
    let vel_area = Rect::from_min_max(pos2(rect.min.x + KEYS_W, rect.max.y - vel_h + 3.0), pos2(rect.max.x, rect.max.y - 2.0));
    let len = clip.length.max(0.25);
    // Notes may lie past the clip end (silent, shown in the dark area): the view
    // reaches the furthest of clip end and note ends.
    let view_len = notes.iter().map(|n| n.start + n.length).fold(len, f64::max);
    let bpb = app.project.beats_per_bar as f64;

    // On selecting a different clip: reset zoom/scroll and show its notes (or the drum pads).
    let seen_id = ui.scope_id().with("pr_clip");
    let fresh = ui.data(|d| d.get_temp::<u64>(seen_id)) != Some(clip.id);
    if fresh {
        ui.data_mut(|d| d.insert_temp(seen_id, clip.id));
        app.note_sel.clear();
        app.pr_time_sel = None;
        app.pr_zoom = 0.0;
        app.pr_scroll = 0.0;
    }
    let mut row_h = app.pr_row_h.clamp(5.0, 32.0);
    let mut rows = (grid_area.height() / row_h).floor().max(1.0) as i32;
    if fresh {
        app.pr_low_note = if drums.is_some() {
            DRUM_BASE_NOTE as i32
        } else if notes.is_empty() {
            48
        } else {
            let (lo, hi) = notes.iter().fold((127i32, 0i32), |(l, h), n| (l.min(n.pitch as i32), h.max(n.pitch as i32)));
            ((lo + hi) / 2 - rows / 2).clamp(0, (127 - rows).max(0))
        };
    }

    // Wheel: plain = pitch scroll, Shift = horizontal scroll, Ctrl = horizontal zoom
    // (around the pointer), Alt = row height.
    let fit = (grid_area.width() as f64 / view_len).max(1e-3);
    let mut ppb = if app.pr_zoom > 0.0 { (app.pr_zoom as f64).max(fit) } else { fit };
    if resp.hovered() {
        let (zoom, scroll, alt, hover) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta, i.modifiers.alt, i.pointer.hover_pos()));
        // Zoom gestures anchor at the point under the cursor when the gesture
        // STARTED (a gesture ends after 0.35 s without zoom input): that beat /
        // pitch stays at its screen position however the cursor moves meanwhile.
        let now = ui.input(|i| i.time);
        let live = |a: &Option<(f64, f32, f64)>| a.filter(|a| now - a.2 < 0.35);
        if zoom != 1.0
            && let Some(h) = hover
        {
            let (beat, off) = match live(&app.pr_zoom_anchor) {
                Some((b, o, _)) => (b, o),
                None => {
                    let o = (h.x - grid_area.min.x).max(0.0);
                    (app.pr_scroll + o as f64 / ppb, o)
                }
            };
            app.pr_zoom_anchor = Some((beat, off, now));
            ppb = (ppb * zoom as f64).clamp(fit, 600.0);
            app.pr_zoom = if ppb <= fit * 1.0001 { 0.0 } else { ppb as f32 };
            app.pr_scroll = beat - off as f64 / ppb;
        } else if alt && scroll.y != 0.0 {
            let new_h = (row_h * (1.0 + scroll.y * 0.004)).clamp(5.0, 32.0);
            app.pr_row_h = new_h;
            if let Some(h) = hover {
                let (fpitch, below) = match live(&app.pr_row_anchor) {
                    Some((p, b, _)) => (p as f32, b),
                    None => {
                        let below = (grid_area.max.y - h.y).max(0.0);
                        (app.pr_low_note as f32 + below / row_h, below)
                    }
                };
                app.pr_row_anchor = Some((fpitch as f64, below, now));
                let new_rows = (grid_area.height() / new_h).floor().max(1.0) as i32;
                app.pr_low_note = (fpitch - below / new_h).round().clamp(0.0, (127 - new_rows + 1).max(0) as f32) as i32;
            }
        } else {
            if scroll.x != 0.0 {
                app.pr_scroll -= scroll.x as f64 / ppb;
            }
            if scroll.y != 0.0 {
                app.pr_low_note = (app.pr_low_note + (scroll.y / row_h).round() as i32).clamp(0, 127 - rows.min(127));
            }
        }
    }
    row_h = app.pr_row_h.clamp(5.0, 32.0);
    rows = (grid_area.height() / row_h).floor().max(1.0) as i32;
    let visible = grid_area.width() as f64 / ppb;
    app.pr_scroll = app.pr_scroll.clamp(0.0, (view_len - visible).max(0.0));
    let scroll = app.pr_scroll;
    let low = app.pr_low_note.clamp(0, (127 - rows + 1).max(0));
    app.pr_low_note = low;

    let grid = app.pr_grid.step(ppb, bpb).min(len);
    app.pr_grid_now = grid;
    let free = ui.input(|i| i.modifiers.alt) || !app.pr_grid.snaps();
    let x_of = |b: f64| grid_area.min.x + ((b - scroll) * ppb) as f32;
    let y_of = |pitch: i32| grid_area.max.y - (pitch - low + 1) as f32 * row_h;
    let beat_at = |x: f32| scroll + ((x - grid_area.min.x) as f64) / ppb;
    let pitch_at = |y: f32| low + ((grid_area.max.y - y) / row_h).floor() as i32;
    // Alt (or grid Off) = place freely.
    let snap = |b: f64| if free { b } else { (b / grid).floor() * grid };
    let snap_round = |b: f64| if free { b } else { (b / grid).round() * grid };

    // Rows + keys
    for r in 0..rows {
        let pitch = low + r;
        if pitch > 127 {
            break;
        }
        let y = y_of(pitch);
        let black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
        p.rect_filled(
            Rect::from_min_max(pos2(grid_area.min.x, y), pos2(grid_area.max.x, y + row_h)),
            CornerRadius::ZERO,
            if black { Color32::from_gray(30) } else { Color32::from_gray(36) },
        );
        let key = Rect::from_min_max(pos2(rect.min.x, y), pos2(rect.min.x + KEYS_W - 2.0, y + row_h - 1.0));
        p.rect_filled(key, CornerRadius::ZERO, if black { Color32::from_gray(50) } else { Color32::from_gray(200) });
        let label = match &drums {
            Some(names) if pitch >= DRUM_BASE_NOTE as i32 && ((pitch - DRUM_BASE_NOTE as i32) as usize) < names.len() => {
                Some(names[(pitch - DRUM_BASE_NOTE as i32) as usize].clone())
            }
            _ if pitch % 12 == 0 => Some(note_name(pitch as u8)),
            _ => None,
        };
        if let Some(l) = label
            && row_h >= 7.0
        {
            p.with_clip_rect(key).text(
                key.left_center() + vec2(2.0, 0.0),
                Align2::LEFT_CENTER,
                l,
                FontId::proportional((row_h - 2.0).min(10.0)),
                if black { Color32::WHITE } else { Color32::BLACK },
            );
        }
    }
    // Ruler + beat lines (thinned when the grid gets denser than ~5 px).
    let gp = ui.painter_at(Rect::from_min_max(ruler.min, rect.max));
    gp.rect_filled(ruler, CornerRadius::ZERO, Color32::from_gray(44));
    let line_step = if grid * ppb < 5.0 { crate::arr_ops::adaptive_step(ppb, bpb, 8.0) } else { grid };
    let mut b = (scroll / line_step).floor() * line_step;
    let end = (scroll + visible).min(view_len);
    while b <= end + 1e-9 {
        let bar = (b / bpb).fract().abs() < 1e-9;
        let c = if bar {
            Color32::from_gray(85)
        } else if b.fract().abs() < 1e-9 {
            Color32::from_gray(55)
        } else {
            Color32::from_gray(42)
        };
        let x = x_of(b);
        gp.line_segment([pos2(x, grid_area.min.y), pos2(x, rect.max.y)], Stroke::new(1.0, c));
        if bar {
            gp.line_segment([pos2(x, ruler.min.y + 6.0), pos2(x, ruler.max.y)], Stroke::new(1.0, Color32::from_gray(120)));
            gp.text(
                pos2(x + 2.0, ruler.min.y),
                Align2::LEFT_TOP,
                format!("{}", (b / bpb) as i64 + 1),
                FontId::proportional(9.0),
                Color32::from_gray(170),
            );
        }
        b += line_step;
    }
    // Area past the clip end (notes there are kept but silent).
    if x_of(len) < grid_area.max.x {
        gp.rect_filled(Rect::from_min_max(pos2(x_of(len), ruler.min.y), rect.max), CornerRadius::ZERO, Color32::from_black_alpha(110));
        gp.line_segment([pos2(x_of(len), ruler.min.y), pos2(x_of(len), rect.max.y)], Stroke::new(1.0, Color32::from_gray(120)));
    }
    // Rubber-band time range (Ctrl+D duplicates it with its notes).
    if let Some((a, b)) = app.pr_time_sel {
        let r = Rect::from_min_max(pos2(x_of(a).max(grid_area.min.x), ruler.min.y), pos2(x_of(b).min(grid_area.max.x), grid_area.max.y));
        if r.width() > 0.0 {
            gp.rect_filled(r, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(120, 170, 255, 22));
        }
    }

    // Notes (clipped to the scrolling area).
    let np = ui.painter_at(grid_area);
    let vp = ui.painter_at(vel_area);
    let color = rgb(clip.color);
    let note_rect = |n: &Note| {
        let (x1, x2) = (x_of(n.start), x_of(n.start + n.length).max(x_of(n.start) + 3.0));
        let y = y_of(n.pitch as i32);
        Rect::from_min_max(pos2(x1, y + 1.0), pos2(x2, y + row_h - 1.0))
    };
    let mut hit: Option<(usize, bool)> = None; // (index, on resize edge)
    let pointer = resp.hover_pos().or(resp.interact_pointer_pos());
    for (i, n) in notes.iter().enumerate() {
        let r = note_rect(n);
        let sel = app.note_sel.contains(&i);
        if r.max.y > grid_area.min.y && r.min.y < grid_area.max.y && r.max.x > grid_area.min.x && r.min.x < grid_area.max.x {
            let outside = n.start >= len - 1e-9;
            let base = if n.muted || outside { disabled(color) } else { color };
            let c = shade(base, 0.5 + n.velocity as f32 / 127.0 * 0.7);
            np.rect_filled(r, CornerRadius::same(1), c);
            np.rect_stroke(
                r,
                CornerRadius::same(1),
                Stroke::new(1.0, if sel { Color32::WHITE } else { Color32::from_black_alpha(160) }),
                egui::StrokeKind::Inside,
            );
            if let Some(pt) = pointer
                && grid_area.contains(pt)
                && r.contains(pt)
            {
                hit = Some((i, pt.x > r.max.x - 5.0));
            }
        }
    }
    // Velocity lane: tallest stems first so every overlapping (chord) stem keeps a
    // visible head; selected stems are drawn last on top.
    let mut order: Vec<usize> = (0..notes.len()).collect();
    order.sort_by_key(|&i| (app.note_sel.contains(&i), std::cmp::Reverse(notes[i].velocity)));
    for i in order {
        let n = &notes[i];
        let x = x_of(n.start) + 1.0;
        if x < vel_area.min.x - 4.0 || x > vel_area.max.x + 4.0 {
            continue;
        }
        let sel = app.note_sel.contains(&i);
        let vh = vel_area.height() * n.velocity as f32 / 127.0;
        let c = if sel {
            Color32::WHITE
        } else if n.muted {
            disabled(c_or(color))
        } else {
            c_or(color)
        };
        let top = pos2(x, vel_area.max.y - vh);
        // Slim line showing the note's length.
        let x_end = (x_of(n.start + n.length) - 1.0).max(x + 2.0);
        vp.line_segment([top, pos2(x_end, top.y)], Stroke::new(1.0, c.gamma_multiply(0.6)));
        vp.line_segment([pos2(x, vel_area.max.y), top], Stroke::new(1.5, c));
        vp.circle_filled(top, 2.5, c);
        vp.circle_stroke(top, 2.5, Stroke::new(1.0, Color32::from_black_alpha(180)));
    }
    // Velocity scale (1 / 64 / 127) in the key column.
    let sp = ui.painter_at(Rect::from_min_max(pos2(rect.min.x, vel_area.min.y - 4.0), pos2(vel_area.min.x, rect.max.y)));
    sp.rect_filled(
        Rect::from_min_max(pos2(rect.min.x, grid_area.max.y), pos2(vel_area.min.x - 2.0, rect.max.y)),
        CornerRadius::ZERO,
        Color32::from_gray(30),
    );
    for v in [127u8, 64, 1] {
        let y = vel_area.max.y - vel_area.height() * v as f32 / 127.0;
        sp.line_segment([pos2(vel_area.min.x - 6.0, y), pos2(vel_area.min.x - 2.0, y)], Stroke::new(1.0, Color32::from_gray(120)));
        sp.text(pos2(vel_area.min.x - 8.0, y), Align2::RIGHT_CENTER, v.to_string(), FontId::proportional(9.0), Color32::from_gray(150));
        if v == 64 {
            vp.line_segment([pos2(vel_area.min.x, y), pos2(vel_area.max.x, y)], Stroke::new(1.0, Color32::from_white_alpha(10)));
        }
    }
    // Splitter between the note grid and the velocity lane.
    let split = Rect::from_min_max(pos2(rect.min.x, grid_area.max.y - 3.0), pos2(rect.max.x, grid_area.max.y + 4.0));
    let sr = ui.interact(split, ui.scope_id().with("vel_split"), Sense::drag());
    p.line_segment(
        [pos2(rect.min.x, grid_area.max.y + 1.0), pos2(rect.max.x, grid_area.max.y + 1.0)],
        Stroke::new(
            if sr.hovered() || sr.dragged() { 2.0 } else { 1.0 },
            Color32::from_gray(if sr.hovered() || sr.dragged() { 140 } else { 70 }),
        ),
    );
    if sr.hovered() || sr.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if sr.dragged()
        && let Some(pt) = sr.interact_pointer_pos()
    {
        app.pr_vel_h = (rect.max.y - pt.y).clamp(24.0, (rect.height() * 0.6).max(24.0));
    }
    // Piano keys: press / glide to play the pitch (held until release).
    let keys = Rect::from_min_max(pos2(rect.min.x, grid_area.min.y), pos2(rect.min.x + KEYS_W - 2.0, grid_area.max.y));
    let kr = ui.interact(keys, ui.scope_id().with("pr_keys"), Sense::click_and_drag());
    let held_id = ui.scope_id().with("pr_key_held");
    let held: Option<u8> = ui.data(|d| d.get_temp(held_id)).flatten();
    let want = if kr.is_pointer_button_down_on() {
        ui.ctx().pointer_latest_pos().filter(|pt| keys.contains(*pt)).map(|pt| pitch_at(pt.y).clamp(0, 127) as u8).or(held)
    } else {
        None
    };
    if want != held {
        if let Some(rt) = app.rt(track) {
            if let Some(h) = held {
                app.send_now(daw_engine::Command::NoteOff { track: rt, pitch: h });
            }
            if let Some(w) = want {
                app.send_now(daw_engine::Command::NoteOn { track: rt, pitch: w, velocity: 100 });
            }
        }
        ui.data_mut(|d| d.insert_temp(held_id, want));
    }
    if let Some(h) = want {
        p.rect_filled(
            Rect::from_min_max(pos2(keys.min.x, y_of(h as i32)), pos2(keys.max.x, y_of(h as i32) + row_h - 1.0)),
            CornerRadius::ZERO,
            Color32::from_rgba_unmultiplied(255, 180, 60, 150),
        );
    }
    // Headphones (top-left, above the keys): preview notes while editing.
    let hp = Rect::from_min_size(rect.min, vec2(KEYS_W - 2.0, PR_RULER_H));
    let hr = ui.interact(hp, ui.scope_id().with("pr_audition"), Sense::click());
    p.rect_filled(
        hp,
        CornerRadius::same(2),
        if app.pr_audition {
            shade(ACCENT, 0.8)
        } else if hr.hovered() {
            Color32::from_gray(70)
        } else {
            Color32::from_gray(50)
        },
    );
    draw_headphones(&p, hp.center(), if app.pr_audition { Color32::BLACK } else { Color32::from_gray(170) });
    if hr.on_hover_text("Preview notes while placing / moving them").clicked() {
        app.pr_audition = !app.pr_audition;
    }
    if let Some((_, true)) = hit {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }

    // Interaction
    let drag_id = ui.scope_id().with("note_drag");
    let in_vel = |pt: egui::Pos2| vel_area.contains(pt);
    // Ruler click: move the song playhead (arrangement clips map to song time).
    let arr_start = match cref {
        ClipRef::Arr { track, index } => app.project.tracks.get(track).and_then(|t| t.arrangement.get(index)).map(|a| a.start),
        ClipRef::Slot { .. } => None,
    };
    if resp.clicked()
        && let Some(pt) = resp.interact_pointer_pos()
        && ruler.contains(pt)
    {
        if let Some(s0) = arr_start {
            app.set_playhead(s0 + snap_round(beat_at(pt.x)).clamp(0.0, len));
        }
        return;
    }
    if resp.double_clicked()
        && let Some(pt) = resp.interact_pointer_pos()
        && grid_area.contains(pt)
    {
        match hit {
            Some((i, _)) => {
                app.note_sel = vec![i];
                app.delete_selected_notes();
            }
            None => {
                // Length / velocity of the last edited note become the defaults.
                let pitch = pitch_at(pt.y).clamp(0, 127) as u8;
                let length = if app.pr_note_len > 0.0 { app.pr_note_len } else { grid.min(len) };
                let velocity = app.pr_note_vel.clamp(1, 127);
                let start = snap(beat_at(pt.x)).max(0.0);
                app.edit(|pr| {
                    if let Some(Clip { content: ClipContent::Midi { notes }, .. }) = DawApp::clip_mut(pr, cref) {
                        notes.push(Note { start, length, pitch, velocity, muted: false, mpe: 0 });
                        notes.sort_by(|a, b| a.start.total_cmp(&b.start));
                    }
                });
                app.note_sel.clear();
                app.pr_time_sel = None;
                if app.pr_audition {
                    app.audition_note(track, pitch, velocity);
                }
            }
        }
        return;
    }
    if resp.clicked()
        && let Some(pt) = resp.interact_pointer_pos()
    {
        let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
        match hit {
            Some((i, _)) => {
                if add {
                    if let Some(k) = app.note_sel.iter().position(|&s| s == i) {
                        app.note_sel.remove(k);
                    } else {
                        app.note_sel.push(i);
                    }
                } else {
                    app.note_sel = vec![i];
                }
                let n = notes[i];
                app.pr_time_sel = None;
                if app.pr_audition {
                    app.audition_note(track, n.pitch, n.velocity);
                }
            }
            None if !in_vel(pt) => {
                app.note_sel.clear();
                app.pr_time_sel = None;
            }
            None => {}
        }
    }
    // Right-click: select the note under the pointer, then the context menu.
    if resp.secondary_clicked()
        && let Some((i, _)) = hit
        && !app.note_sel.contains(&i)
    {
        app.note_sel = vec![i];
    }
    resp.context_menu(|ui| {
        let any = !app.note_sel.is_empty();
        if ui.add_enabled(any, egui::Button::new("Delete                   Del")).clicked() {
            app.delete_selected_notes();
            ui.close();
        }
        if ui.add_enabled(any, egui::Button::new("Duplicate               Ctrl+D")).clicked() {
            app.duplicate_selected_notes();
            ui.close();
        }
        if ui.add_enabled(any, egui::Button::new("Activate / Deactivate   0")).clicked() {
            app.focus_detail = true;
            app.toggle_disable();
            ui.close();
        }
        if ui.button("Select All              Ctrl+A").clicked() {
            app.note_sel = (0..notes.len()).collect();
            ui.close();
        }
        ui.separator();
        super::widgets::grid_menu(ui, &mut app.pr_grid);
    });
    if resp.drag_started()
        && let Some(pt) = ui.input(|i| i.pointer.press_origin())
    {
        // Re-hit-test at the press point (the pointer has moved past the drag threshold).
        let hit = grid_area
            .contains(pt)
            .then(|| {
                notes.iter().enumerate().rev().find_map(|(i, n)| {
                    let r = note_rect(n);
                    r.contains(pt).then_some((i, pt.x > r.max.x - 5.0))
                })
            })
            .flatten();
        let mode = if ruler.contains(pt) {
            Some(NoteDrag::Ruler)
        } else if in_vel(pt) {
            Some(NoteDrag::Velocity)
        } else if grid_area.contains(pt) {
            match hit {
                Some((i, edge)) => {
                    if !app.note_sel.contains(&i) {
                        app.note_sel = vec![i];
                    }
                    Some(if edge {
                        NoteDrag::Resize { grab: i }
                    } else {
                        NoteDrag::Move {
                            grab_beat: beat_at(pt.x),
                            grab_pitch: pitch_at(pt.y),
                            orig: Some((notes[i].start, notes[i].pitch)),
                        }
                    })
                }
                None => Some(NoteDrag::Marquee),
            }
        } else {
            None
        };
        if let Some(m) = mode {
            if matches!(m, NoteDrag::Move { .. } | NoteDrag::Resize { .. } | NoteDrag::Velocity) {
                app.begin_gesture();
            }
            let base = if ui.input(|i| i.modifiers.shift || i.modifiers.command) { app.note_sel.clone() } else { Vec::new() };
            let mut snapshot = notes.to_vec();
            // Ctrl+drag on a note: move copies, leaving the originals in place.
            if matches!(m, NoteDrag::Move { .. }) && ui.input(|i| i.modifiers.command) {
                let first = snapshot.len();
                let copies: Vec<Note> = app.note_sel.iter().filter_map(|&i| snapshot.get(i).copied()).collect();
                snapshot.extend(copies);
                app.note_sel = (first..snapshot.len()).collect();
                if let Some(Clip { content: ClipContent::Midi { notes }, .. }) = DawApp::clip_mut(&mut app.project, cref) {
                    *notes = snapshot.clone();
                }
            }
            if matches!(m, NoteDrag::Marquee) && base.is_empty() {
                app.pr_time_sel = None;
            }
            // Snapshot of the notes at drag start lets moves be computed from absolute deltas.
            ui.data_mut(|d| {
                d.insert_temp(drag_id, m);
                d.insert_temp(drag_id.with("orig"), snapshot);
                d.insert_temp(drag_id.with("base"), base);
            });
        }
    }
    if resp.dragged()
        && let Some(mode) = ui.data(|d| d.get_temp::<NoteDrag>(drag_id))
        && let Some(orig) = ui.data(|d| d.get_temp::<Vec<Note>>(drag_id.with("orig")))
        && let Some(pt) = resp.interact_pointer_pos()
    {
        let sel = app.note_sel.clone();
        let mut new = orig.clone();
        let mut write = true;
        match mode {
            NoteDrag::Move { grab_beat, grab_pitch, orig: anchor } => {
                let (a_start, _) = anchor.unwrap_or((0.0, 0));
                let raw = a_start + beat_at(pt.x) - grab_beat;
                let target = if free { raw } else { snap(raw + grid * 0.5) };
                let db = target - a_start;
                let dp = pitch_at(pt.y) - grab_pitch;
                // Notes may leave the clip to the right (kept, silent) but not
                // before 0 (unless already there: hidden head after a left trim).
                let db = sel.iter().filter_map(|&i| orig.get(i)).fold(db, |d, n| d.max(n.start.min(0.0) - n.start));
                for &i in &sel {
                    if let Some(n) = new.get_mut(i) {
                        n.start = orig[i].start + db;
                        n.pitch = (orig[i].pitch as i32 + dp).clamp(0, 127) as u8;
                    }
                }
                // Preview the new pitch of the grabbed note when it changes.
                if let Some((_, p0)) = anchor {
                    let np = (p0 as i32 + dp).clamp(0, 127) as u8;
                    let key = drag_id.with("last_pitch");
                    if ui.data(|d| d.get_temp::<u8>(key)) != Some(np) {
                        ui.data_mut(|d| d.insert_temp(key, np));
                        if dp != 0 && app.pr_audition {
                            let v = sel.first().and_then(|&i| orig.get(i)).map_or(100, |n| n.velocity);
                            app.audition_note(track, np, v);
                        }
                    }
                }
            }
            NoteDrag::Resize { grab } => {
                let min = if free { 1.0 / 128.0 } else { grid.min(0.25) / 4.0 };
                if let Some(g) = orig.get(grab) {
                    // The grabbed note's end follows the (snapped) pointer; the rest shift by the same delta.
                    let end = snap_round(beat_at(pt.x)).max(g.start + min);
                    let delta = end - (g.start + g.length);
                    for &i in &sel {
                        if let (Some(n), Some(o)) = (new.get_mut(i), orig.get(i)) {
                            n.length = (o.length + delta).max(min).max(1.0 / 128.0);
                        }
                    }
                }
                if let Some(n) = sel.first().and_then(|&i| new.get(i)) {
                    app.pr_note_len = n.length;
                }
            }
            NoteDrag::Velocity => {
                let v = (((vel_area.max.y - pt.y) / vel_area.height()) * 127.0).round().clamp(1.0, 127.0) as u8;
                app.pr_note_vel = v;
                // Value readout next to the pointer while dragging.
                let tp = ui.painter();
                let lr = Rect::from_center_size(pt + vec2(18.0, -12.0), vec2(28.0, 15.0));
                tp.rect_filled(lr, CornerRadius::same(2), Color32::from_black_alpha(210));
                tp.text(lr.center(), Align2::CENTER_CENTER, v.to_string(), FontId::proportional(11.0), Color32::WHITE);
                let press = ui.input(|i| i.pointer.press_origin()).unwrap_or(pt);
                let b = beat_at(press.x);
                // Selected notes, else every note starting at the nearest start (a chord).
                let targets: Vec<usize> = if sel.is_empty() {
                    let nearest = orig.iter().map(|n| n.start).min_by(|a, c| (a - b).abs().total_cmp(&(c - b).abs()));
                    match nearest {
                        Some(s) => (0..orig.len()).filter(|&i| (orig[i].start - s).abs() < 1e-9).collect(),
                        None => Vec::new(),
                    }
                } else {
                    sel.clone()
                };
                for i in targets {
                    if let Some(n) = new.get_mut(i) {
                        n.velocity = v;
                    }
                }
            }
            NoteDrag::Marquee => {
                write = false;
                let start = ui.input(|i| i.pointer.press_origin()).unwrap_or(pt);
                let band = Rect::from_two_pos(start, pt);
                let mut s: Vec<usize> = ui.data(|d| d.get_temp::<Vec<usize>>(drag_id.with("base"))).unwrap_or_default();
                for (i, n) in orig.iter().enumerate() {
                    if note_rect(n).intersects(band) && !s.contains(&i) {
                        s.push(i);
                    }
                }
                app.note_sel = s;
                let (a, b) = (beat_at(band.min.x).max(0.0), beat_at(band.max.x).max(0.0));
                app.pr_time_sel =
                    (b - a > 1e-6).then_some((snap_round(a), snap_round(b).max(snap_round(a) + if free { b - a } else { grid })));
                np.rect_filled(band, CornerRadius::ZERO, Color32::from_white_alpha(18));
                np.rect_stroke(band, CornerRadius::ZERO, Stroke::new(1.0, Color32::from_white_alpha(120)), egui::StrokeKind::Inside);
            }
            NoteDrag::Ruler => {
                write = false;
                if let Some(s0) = arr_start {
                    app.set_playhead(s0 + snap_round(beat_at(pt.x)).clamp(0.0, len));
                }
            }
        }
        // Auto-scroll while dragging past the edges.
        if pt.x > grid_area.max.x {
            app.pr_scroll += 4.0 / ppb;
        } else if pt.x < grid_area.min.x && app.pr_scroll > 0.0 {
            app.pr_scroll -= 4.0 / ppb;
        }
        if write && let Some(Clip { content: ClipContent::Midi { notes }, .. }) = DawApp::clip_mut(&mut app.project, cref) {
            *notes = new;
        }
    }
    if resp.drag_stopped()
        && let Some(mode) = ui.data(|d| d.get_temp::<NoteDrag>(drag_id))
    {
        ui.data_mut(|d| {
            d.remove::<u8>(drag_id.with("last_pitch"));
            d.remove::<NoteDrag>(drag_id);
            d.remove::<Vec<Note>>(drag_id.with("orig"));
            d.remove::<Vec<usize>>(drag_id.with("base"));
        });
        if matches!(mode, NoteDrag::Move { .. } | NoteDrag::Resize { .. } | NoteDrag::Velocity) {
            // Re-sort while keeping the selection on the same notes.
            if let Some(Clip { content: ClipContent::Midi { notes }, .. }) = DawApp::clip_mut(&mut app.project, cref) {
                let selected: Vec<Note> = app.note_sel.iter().filter_map(|&i| notes.get(i).copied()).collect();
                notes.sort_by(|a, b| a.start.total_cmp(&b.start));
                app.note_sel = selected.iter().filter_map(|s| notes.iter().position(|n| n == s)).collect();
            }
            app.needs_compile = true;
        }
    }
    // Playhead
    let pos = match cref {
        ClipRef::Slot { scene, .. } => {
            let st = app.track_status.get(track).copied().unwrap_or_default();
            (st.playing_scene == Some(scene as u32)).then_some(st.clip_pos)
        }
        ClipRef::Arr { .. } => arr_start.and_then(|s0| {
            let b = app.status.beat_position - s0;
            (app.status.playing && (0.0..len).contains(&b) && !app.track_status.get(track).is_some_and(|s| s.arrangement_off)).then_some(b)
        }),
    };
    if let Some(pos) = pos {
        let x = x_of(pos);
        gp.line_segment([pos2(x, ruler.min.y), pos2(x, rect.max.y)], Stroke::new(1.5, Color32::WHITE));
    }
}

fn c_or(c: Color32) -> Color32 {
    shade(c, 1.1)
}

// ============================ Devices ============================

fn device_panel(app: &mut DawApp, ui: &mut egui::Ui) {
    let t = app.selected_track;
    let Some(track) = app.project.tracks.get(t) else { return };
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(10.0, 16.0), Sense::hover());
        ui.painter().rect_filled(r, CornerRadius::same(2), rgb(track.color));
        ui.label(egui::RichText::new(app.project.track_label(t)).strong());
        ui.label(egui::RichText::new(track.instrument.label()).color(Color32::GRAY));
        if track.kind == TrackKind::Midi {
            ui.label(egui::RichText::new("Drop an instrument from the browser here to replace").small().color(Color32::from_gray(110)));
        }
    });
    if track.kind == TrackKind::Group {
        let n = app.project.subtree_end(t) - t - 1;
        ui.label(
            egui::RichText::new(format!(
                "Group bus with {n} track(s): their audio is summed here, then this track's volume, pan, mute and solo apply. Ctrl+Shift+G ungroups."
            ))
            .color(Color32::GRAY),
        );
        return;
    }
    if track.kind == TrackKind::Audio {
        ui.label(egui::RichText::new("Audio track: drop samples on its slots or arrangement lane, arm it (red dot) and record with F9 or a slot's record button. IN = monitor input.").color(Color32::GRAY));
        return;
    }
    let mut inst = track.instrument.clone();
    let mut ed = Edit::default();
    let frame = egui::Frame::NONE.fill(Color32::from_gray(34)).corner_radius(4).inner_margin(6);
    let (resp, dropped) = ui.dnd_drop_zone::<DragItem, _>(frame, |ui| {
        ui.set_min_size(vec2(ui.available_width(), 120.0));
        match &mut inst {
            InstrumentDef::None => {
                ui.label("No instrument. Drag one from the browser.");
            }
            InstrumentDef::Synth(s) => synth_ui(ui, s, &mut ed),
            InstrumentDef::Sampler { sample, params } => {
                ui.horizontal(|ui| {
                    ui.label("Sample:");
                    let name = sample
                        .as_deref()
                        .map(|s| s.rsplit(['/', '\\', ':']).next().unwrap_or(s).to_string())
                        .unwrap_or_else(|| "(drop a sample here)".into());
                    ui.label(egui::RichText::new(name).color(ACCENT));
                });
                ui.horizontal(|ui| {
                    ui.label("Root");
                    ed.drag(ui.add(egui::DragValue::new(&mut params.root_note).range(0..=127).custom_formatter(|v, _| note_name(v as u8))));
                    ed.slider(ui, "Tune", &mut params.tune_semitones, -24.0..=24.0, false);
                    ed.slider(ui, "Gain", &mut params.gain, 0.0..=2.0, false);
                });
                adsr_ui(ui, "Amp", &mut params.amp, &mut ed);
            }
            InstrumentDef::Drums { pads } => {
                if let Some(a) = drums_ui(ui, pads, &mut ed) {
                    app.audition_note(t, a, 110);
                }
            }
        }
    });
    let _ = resp;
    if let Some(item) = dropped {
        if let Some(def) = match &*item {
            // Sample onto a sampler: keep its params.
            DragItem::Sample { key, .. } if matches!(inst, InstrumentDef::Sampler { .. }) => {
                if let InstrumentDef::Sampler { params, .. } = &inst {
                    Some(InstrumentDef::Sampler { sample: Some(key.clone()), params: *params })
                } else {
                    None
                }
            }
            DragItem::Sample { .. } if matches!(inst, InstrumentDef::Drums { .. }) => None, // handled per pad
            other => super::instrument_for(other),
        } {
            app.set_instrument(t, def);
        }
        return;
    }
    if let Some((pad, key)) = ed.pad_drop.take() {
        if let InstrumentDef::Drums { pads } = &mut inst {
            while pads.len() <= pad {
                pads.push(PadDef { name: format!("Pad {}", pads.len() + 1), gain: 0.8, ..Default::default() });
            }
            pads[pad].name = key.1;
            pads[pad].sample = Some(key.0);
        }
        ed.gesture = true;
        ed.changed = true;
    }
    if ed.gesture {
        app.begin_gesture();
    }
    if ed.changed {
        app.project.tracks[t].instrument = inst;
        app.needs_compile = true;
        app.dirty = true;
    }
}

#[derive(Default)]
struct Edit {
    gesture: bool,
    changed: bool,
    pad_drop: Option<(usize, (String, String))>,
}

impl Edit {
    fn drag(&mut self, r: egui::Response) {
        self.gesture |= r.drag_started() || r.gained_focus() || (r.clicked() && r.changed());
        self.changed |= r.changed();
    }

    fn slider(&mut self, ui: &mut egui::Ui, label: &str, v: &mut f32, range: std::ops::RangeInclusive<f32>, log: bool) {
        let r = ui.add(egui::Slider::new(v, range).text(label).logarithmic(log).max_decimals(2));
        self.drag(r);
    }
}

fn synth_ui(ui: &mut egui::Ui, s: &mut daw_engine::SynthParams, ed: &mut Edit) {
    ui.horizontal(|ui| {
        ui.label("Preset");
        egui::ComboBox::from_id_salt("preset").selected_text("Load…").show_ui(ui, |ui| {
            for p in daw_engine::SynthParams::PRESETS {
                if ui.selectable_label(false, p).clicked() {
                    *s = daw_engine::SynthParams::preset(p);
                    ed.gesture = true;
                    ed.changed = true;
                }
            }
        });
        wave_combo(ui, "Osc 1", &mut s.osc1, ed);
        wave_combo(ui, "Osc 2", &mut s.osc2, ed);
    });
    ui.columns(3, |c| {
        c[0].label(egui::RichText::new("OSCILLATORS").small().color(Color32::GRAY));
        ed.slider(&mut c[0], "Osc2 semi", &mut s.osc2_semitones, -24.0..=24.0, false);
        ed.slider(&mut c[0], "Detune ct", &mut s.osc2_detune_cents, -50.0..=50.0, false);
        ed.slider(&mut c[0], "Mix", &mut s.osc_mix, 0.0..=1.0, false);
        ed.slider(&mut c[0], "Gain", &mut s.gain, 0.0..=1.5, false);
        c[1].label(egui::RichText::new("FILTER").small().color(Color32::GRAY));
        ed.slider(&mut c[1], "Cutoff Hz", &mut s.cutoff_hz, 30.0..=18000.0, true);
        ed.slider(&mut c[1], "Resonance", &mut s.resonance, 0.0..=1.0, false);
        ed.slider(&mut c[1], "Env oct", &mut s.filter_env_octaves, -4.0..=8.0, false);
        adsr_ui(&mut c[1], "Filter env", &mut s.filter, ed);
        c[2].label(egui::RichText::new("AMP").small().color(Color32::GRAY));
        adsr_ui(&mut c[2], "Amp env", &mut s.amp, ed);
    });
}

fn wave_combo(ui: &mut egui::Ui, label: &str, w: &mut Wave, ed: &mut Edit) {
    ui.label(label);
    egui::ComboBox::from_id_salt(label).selected_text(format!("{w:?}")).width(80.0).show_ui(ui, |ui| {
        for v in [Wave::Sine, Wave::Saw, Wave::Square, Wave::Triangle] {
            if ui.selectable_label(*w == v, format!("{v:?}")).clicked() && *w != v {
                *w = v;
                ed.gesture = true;
                ed.changed = true;
            }
        }
    });
}

fn adsr_ui(ui: &mut egui::Ui, label: &str, a: &mut daw_engine::AdsrParams, ed: &mut Edit) {
    ui.label(egui::RichText::new(label).small());
    ed.slider(ui, "A s", &mut a.attack, 0.0005..=5.0, true);
    ed.slider(ui, "D s", &mut a.decay, 0.001..=5.0, true);
    ed.slider(ui, "S", &mut a.sustain, 0.0..=1.0, false);
    ed.slider(ui, "R s", &mut a.release, 0.001..=8.0, true);
}

/// A whole-clip MIDI edit: (notes, clip length).
type NoteOp = Box<dyn Fn(&mut Vec<Note>, &mut f64)>;

/// 4×4 pad grid (bottom-left = C1 like hardware). Returns a pad to audition.
fn drums_ui(ui: &mut egui::Ui, pads: &mut [PadDef], ed: &mut Edit) -> Option<u8> {
    let sel_id = ui.scope_id().with("pad_sel");
    let mut sel: usize = ui.data(|d| d.get_temp(sel_id)).unwrap_or(0);
    let mut audition = None;
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
            for row in (0..4).rev() {
                ui.horizontal(|ui| {
                    for col in 0..4 {
                        let i = row * 4 + col;
                        if i >= DRUM_PADS {
                            continue;
                        }
                        let (r, resp) = ui.allocate_exact_size(vec2(78.0, 34.0), Sense::click());
                        let has = pads.get(i).is_some_and(|p| p.sample.is_some());
                        let bg = if i == sel {
                            ACCENT
                        } else if has {
                            Color32::from_gray(70)
                        } else {
                            Color32::from_gray(44)
                        };
                        ui.painter().rect_filled(r, CornerRadius::same(3), bg);
                        let name = pads.get(i).map_or(String::new(), |p| p.name.clone());
                        ui.painter().with_clip_rect(r).text(
                            r.left_top() + vec2(4.0, 3.0),
                            Align2::LEFT_TOP,
                            &name,
                            FontId::proportional(10.0),
                            if i == sel { Color32::BLACK } else { Color32::WHITE },
                        );
                        ui.painter().text(
                            r.right_bottom() - vec2(3.0, 2.0),
                            Align2::RIGHT_BOTTOM,
                            note_name(DRUM_BASE_NOTE + i as u8),
                            FontId::proportional(8.0),
                            Color32::from_gray(150),
                        );
                        if resp.clicked() {
                            sel = i;
                            audition = Some(DRUM_BASE_NOTE + i as u8);
                        }
                        if resp.dnd_hover_payload::<DragItem>().is_some() {
                            ui.painter().rect_stroke(r, CornerRadius::same(3), Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Inside);
                        }
                        if let Some(item) = resp.dnd_release_payload::<DragItem>()
                            && let DragItem::Sample { key, name } = &*item
                        {
                            ed.pad_drop = Some((i, (key.clone(), name.clone())));
                            sel = i;
                        }
                    }
                });
            }
        });
        ui.separator();
        ui.vertical(|ui| {
            if let Some(pad) = pads.get_mut(sel) {
                ui.label(egui::RichText::new(format!("Pad {} · {}", sel + 1, note_name(DRUM_BASE_NOTE + sel as u8))).strong());
                let r = ui.text_edit_singleline(&mut pad.name);
                ed.drag(r);
                ed.slider(ui, "Gain", &mut pad.gain, 0.0..=2.0, false);
                ed.slider(ui, "Tune", &mut pad.tune_semitones, -24.0..=24.0, false);
                let r = ui.add(egui::DragValue::new(&mut pad.choke).range(0..=8).prefix("Choke group "));
                ed.drag(r);
                ui.label(
                    egui::RichText::new(pad.sample.clone().unwrap_or_else(|| "(empty: drop a sample)".into())).small().color(Color32::GRAY),
                );
            } else {
                ui.label("Empty pad: drop a sample on it.");
            }
        });
    });
    ui.data_mut(|d| d.insert_temp(sel_id, sel));
    audition
}

/// Small headphones glyph (font-independent).
fn draw_headphones(p: &egui::Painter, c: egui::Pos2, col: Color32) {
    let r = 4.5;
    let pts: Vec<egui::Pos2> = (0..=12)
        .map(|i| {
            let a = std::f32::consts::PI * (1.0 + i as f32 / 12.0);
            c + vec2(a.cos() * r, a.sin() * r + 1.0)
        })
        .collect();
    p.add(egui::Shape::line(pts, Stroke::new(1.3, col)));
    for sx in [-1.0f32, 1.0] {
        p.rect_filled(Rect::from_center_size(c + vec2(sx * (r - 0.5), 2.5), vec2(2.5, 4.0)), CornerRadius::same(1), col);
    }
}
