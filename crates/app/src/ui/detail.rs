//! Detail panel: clip editor (piano roll / audio clip) or the selected track's device.

use super::widgets::*;
use crate::app::{ClipRef, DawApp, DragItem};
use daw_engine::{DRUM_BASE_NOTE, DRUM_PADS, Wave};
use daw_project::{Clip, ClipContent, InstrumentDef, Note, PadDef, TrackKind};
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, pos2, vec2};

const KEYS_W: f32 = 46.0;
const ROW_H: f32 = 11.0;
const VEL_H: f32 = 34.0;
const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

pub fn note_name(p: u8) -> String {
    format!("{}{}", NOTE_NAMES[(p % 12) as usize], p as i32 / 12 - 2)
}

pub fn detail_panel(app: &mut DawApp, ui: &mut egui::Ui) {
    if app.project.tracks.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new("Add a track: Ctrl+T (MIDI) or Ctrl+Shift+T (audio)").color(Color32::GRAY))
        });
        return;
    }
    match app.selected_clip {
        Some(c) if app.clip(c).is_some() => clip_editor(app, ui, c),
        _ => device_panel(app, ui),
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
                        super::arrangement::draw_clip(&p, rect, &c, false, Some(buf), zoom, app.project.tempo_bpm);
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
    let (r, _) = ui.allocate_exact_size(vec2(170.0, 18.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(2), rgb(clip.color));
    ui.painter().with_clip_rect(r).text(
        r.left_center() + vec2(4.0, 0.0),
        Align2::LEFT_CENTER,
        &clip.name,
        FontId::proportional(12.0),
        Color32::BLACK,
    );
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
            let grid = app.grid;
            let mut op: Option<NoteOp> = None;
            if ui.button("Quantize").on_hover_text("Snap note starts to the grid").clicked() {
                op = Some(Box::new(move |n: &mut Vec<Note>, _| n.iter_mut().for_each(|x| x.start = (x.start / grid).round() * grid)));
            }
            if ui.button("×2").on_hover_text("Duplicate loop").clicked() {
                op = Some(Box::new(|n: &mut Vec<Note>, len: &mut f64| {
                    let l = *len;
                    let copy: Vec<Note> = n.iter().filter(|x| x.start < l).map(|x| Note { start: x.start + l, ..*x }).collect();
                    n.extend(copy);
                    *len = l * 2.0;
                }));
            }
            if ui.button("÷2").on_hover_text("Halve length").clicked() {
                op = Some(Box::new(|_, len: &mut f64| *len = (*len / 2.0).max(0.25)));
            }
            if ui.button("+12").clicked() {
                op = Some(Box::new(|n: &mut Vec<Note>, _| n.iter_mut().for_each(|x| x.pitch = (x.pitch + 12).min(127))));
            }
            if ui.button("-12").clicked() {
                op = Some(Box::new(|n: &mut Vec<Note>, _| n.iter_mut().for_each(|x| x.pitch = x.pitch.saturating_sub(12))));
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
                "Grid {} · dbl-click: add/remove\nDrag: move · edge: resize",
                super::arrangement::grid_label(app.grid)
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
    Move { grab_beat: f64, grab_pitch: i32, orig: Option<(f64, u8)> },
    Resize,
    Velocity,
}

fn piano_roll(app: &mut DawApp, ui: &mut egui::Ui, cref: ClipRef, track: usize, clip: &Clip, notes: &[Note]) {
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::ZERO, Color32::from_gray(26));
    let drums: Option<Vec<String>> = match &app.project.tracks[track].instrument {
        InstrumentDef::Drums { pads } => Some(pads.iter().map(|d| d.name.clone()).collect()),
        _ => None,
    };
    let grid_area = Rect::from_min_max(pos2(rect.min.x + KEYS_W, rect.min.y), pos2(rect.max.x, rect.max.y - VEL_H));
    let vel_area = Rect::from_min_max(pos2(rect.min.x + KEYS_W, rect.max.y - VEL_H + 2.0), rect.max);
    let rows = (grid_area.height() / ROW_H).floor().max(1.0) as i32;

    // Vertical scroll over the roll changes the visible pitch range.
    if resp.hovered() {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        if dy != 0.0 {
            app.pr_low_note = (app.pr_low_note + (dy / ROW_H).round() as i32).clamp(0, 127 - rows.min(127));
        }
    }
    // On selecting a different clip, scroll so its notes (or the drum pads) are visible.
    let seen_id = ui.scope_id().with("pr_clip");
    if ui.data(|d| d.get_temp::<u64>(seen_id)) != Some(clip.id) {
        ui.data_mut(|d| d.insert_temp(seen_id, clip.id));
        app.note_sel.clear();
        app.pr_low_note = if drums.is_some() {
            DRUM_BASE_NOTE as i32
        } else if notes.is_empty() {
            48
        } else {
            let (lo, hi) = notes.iter().fold((127i32, 0i32), |(l, h), n| (l.min(n.pitch as i32), h.max(n.pitch as i32)));
            ((lo + hi) / 2 - rows / 2).clamp(0, (127 - rows).max(0))
        };
    }
    let low = app.pr_low_note;
    let len = clip.length.max(0.25);
    let ppb = (grid_area.width() as f64 / len) as f32;
    let grid = app.grid.min(1.0);
    let x_of = |b: f64| grid_area.min.x + b as f32 * ppb;
    let y_of = |pitch: i32| grid_area.max.y - (pitch - low + 1) as f32 * ROW_H;
    let beat_at = |x: f32| ((x - grid_area.min.x) / ppb) as f64;
    let pitch_at = |y: f32| low + ((grid_area.max.y - y) / ROW_H).floor() as i32;

    // Rows + keys
    for r in 0..rows {
        let pitch = low + r;
        if pitch > 127 {
            break;
        }
        let y = y_of(pitch);
        let black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
        p.rect_filled(
            Rect::from_min_max(pos2(grid_area.min.x, y), pos2(grid_area.max.x, y + ROW_H)),
            CornerRadius::ZERO,
            if black { Color32::from_gray(30) } else { Color32::from_gray(36) },
        );
        let key = Rect::from_min_max(pos2(rect.min.x, y), pos2(rect.min.x + KEYS_W - 2.0, y + ROW_H - 1.0));
        p.rect_filled(key, CornerRadius::ZERO, if black { Color32::from_gray(50) } else { Color32::from_gray(200) });
        let label = match &drums {
            Some(names) if pitch >= DRUM_BASE_NOTE as i32 && ((pitch - DRUM_BASE_NOTE as i32) as usize) < names.len() => {
                Some(names[(pitch - DRUM_BASE_NOTE as i32) as usize].clone())
            }
            _ if pitch % 12 == 0 => Some(note_name(pitch as u8)),
            _ => None,
        };
        if let Some(l) = label {
            p.with_clip_rect(key).text(
                key.left_center() + vec2(2.0, 0.0),
                Align2::LEFT_CENTER,
                l,
                FontId::proportional(9.0),
                if black { Color32::WHITE } else { Color32::BLACK },
            );
        }
    }
    // Beat lines
    let bpb = app.project.beats_per_bar as f64;
    let mut b = 0.0;
    while b <= len + 1e-9 {
        let c = if (b / bpb).fract().abs() < 1e-9 {
            Color32::from_gray(85)
        } else if b.fract().abs() < 1e-9 {
            Color32::from_gray(55)
        } else {
            Color32::from_gray(42)
        };
        p.line_segment([pos2(x_of(b), grid_area.min.y), pos2(x_of(b), rect.max.y)], Stroke::new(1.0, c));
        b += grid;
    }

    // Notes
    let color = rgb(clip.color);
    let mut hit: Option<(usize, bool)> = None; // (index, on resize edge)
    let pointer = resp.hover_pos().or(resp.interact_pointer_pos());
    for (i, n) in notes.iter().enumerate() {
        let (x1, x2) = (x_of(n.start), x_of(n.start + n.length).max(x_of(n.start) + 3.0));
        let y = y_of(n.pitch as i32);
        let r = Rect::from_min_max(pos2(x1, y + 1.0), pos2(x2, y + ROW_H - 1.0));
        let sel = app.note_sel.contains(&i);
        if r.max.y > grid_area.min.y && r.min.y < grid_area.max.y {
            let c = shade(color, 0.5 + n.velocity as f32 / 127.0 * 0.7);
            p.rect_filled(r, CornerRadius::same(1), c);
            p.rect_stroke(
                r,
                CornerRadius::same(1),
                Stroke::new(1.0, if sel { Color32::WHITE } else { Color32::from_black_alpha(160) }),
                egui::StrokeKind::Inside,
            );
            if let Some(pt) = pointer
                && r.contains(pt)
            {
                hit = Some((i, pt.x > r.max.x - 5.0));
            }
        }
        // Velocity lane
        let vh = (vel_area.height() - 2.0) * n.velocity as f32 / 127.0;
        p.line_segment(
            [pos2(x1 + 1.0, vel_area.max.y), pos2(x1 + 1.0, vel_area.max.y - vh)],
            Stroke::new(3.0, if sel { Color32::WHITE } else { c_or(color) }),
        );
    }
    p.line_segment(
        [pos2(vel_area.min.x, vel_area.min.y - 1.0), pos2(vel_area.max.x, vel_area.min.y - 1.0)],
        Stroke::new(1.0, Color32::from_gray(70)),
    );
    if let Some((_, true)) = hit {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }

    // Interaction
    let drag_id = ui.scope_id().with("note_drag");
    let in_vel = |pt: egui::Pos2| vel_area.contains(pt);
    let snap = |b: f64| (b / grid).floor() * grid;
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
                let pitch = pitch_at(pt.y).clamp(0, 127) as u8;
                let start = snap(beat_at(pt.x)).clamp(0.0, len - grid.min(len));
                app.edit(|pr| {
                    if let Some(Clip { content: ClipContent::Midi { notes }, .. }) = DawApp::clip_mut(pr, cref) {
                        notes.push(Note { start, length: grid, pitch, velocity: 100 });
                        notes.sort_by(|a, b| a.start.total_cmp(&b.start));
                    }
                });
                app.note_sel.clear();
                app.audition_note(track, pitch, 100);
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
                app.audition_note(track, n.pitch, n.velocity);
            }
            None if !in_vel(pt) => app.note_sel.clear(),
            None => {}
        }
    }
    if resp.secondary_clicked()
        && let Some((i, _)) = hit
    {
        app.note_sel = vec![i];
        app.delete_selected_notes();
        return;
    }
    if resp.drag_started()
        && let Some(pt) = ui.input(|i| i.pointer.press_origin())
    {
        // Re-hit-test at the press point (the pointer has moved past the drag threshold).
        let hit = notes.iter().enumerate().rev().find_map(|(i, n)| {
            let (x1, x2) = (x_of(n.start), x_of(n.start + n.length).max(x_of(n.start) + 3.0));
            let y = y_of(n.pitch as i32);
            let r = Rect::from_min_max(pos2(x1, y + 1.0), pos2(x2, y + ROW_H - 1.0));
            r.contains(pt).then_some((i, pt.x > r.max.x - 5.0))
        });
        let mode = if in_vel(pt) {
            Some(NoteDrag::Velocity)
        } else {
            match hit {
                Some((i, edge)) => {
                    if !app.note_sel.contains(&i) {
                        app.note_sel = vec![i];
                    }
                    Some(if edge {
                        NoteDrag::Resize
                    } else {
                        NoteDrag::Move {
                            grab_beat: beat_at(pt.x),
                            grab_pitch: pitch_at(pt.y),
                            orig: Some((notes[i].start, notes[i].pitch)),
                        }
                    })
                }
                None => None,
            }
        };
        if let Some(m) = mode {
            app.begin_gesture();
            // Snapshot of the notes at drag start lets moves be computed from absolute deltas.
            ui.data_mut(|d| {
                d.insert_temp(drag_id, m);
                d.insert_temp(drag_id.with("orig"), notes.to_vec());
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
        match mode {
            NoteDrag::Move { grab_beat, grab_pitch, orig: anchor } => {
                let (a_start, _) = anchor.unwrap_or((0.0, 0));
                let target = snap(a_start + beat_at(pt.x) - grab_beat + grid * 0.5);
                let db = target - a_start;
                let dp = pitch_at(pt.y) - grab_pitch;
                for &i in &sel {
                    if let Some(n) = new.get_mut(i) {
                        n.start = (orig[i].start + db).clamp(0.0, (len - 1.0 / 64.0).max(0.0));
                        n.pitch = (orig[i].pitch as i32 + dp).clamp(0, 127) as u8;
                    }
                }
            }
            NoteDrag::Resize => {
                for &i in &sel {
                    if let Some(n) = new.get_mut(i) {
                        let end = ((beat_at(pt.x) / grid).round() * grid).max(n.start + grid.min(0.25) / 4.0);
                        n.length = (end - n.start).max(1.0 / 64.0);
                    }
                }
            }
            NoteDrag::Velocity => {
                let v = (((vel_area.max.y - pt.y) / (vel_area.height() - 2.0)) * 127.0).clamp(1.0, 127.0) as u8;
                let b = beat_at(pt.x);
                // Selected notes, or the note nearest the pointer.
                let targets: Vec<usize> = if sel.is_empty() {
                    new.iter()
                        .enumerate()
                        .min_by(|a, c| (a.1.start - b).abs().total_cmp(&(c.1.start - b).abs()))
                        .map(|x| vec![x.0])
                        .unwrap_or_default()
                } else {
                    sel.clone()
                };
                for i in targets {
                    if let Some(n) = new.get_mut(i) {
                        n.velocity = v;
                    }
                }
            }
        }
        if let Some(Clip { content: ClipContent::Midi { notes }, .. }) = DawApp::clip_mut(&mut app.project, cref) {
            *notes = new;
        }
    }
    if resp.drag_stopped() && ui.data(|d| d.get_temp::<NoteDrag>(drag_id)).is_some() {
        ui.data_mut(|d| {
            d.remove::<NoteDrag>(drag_id);
            d.remove::<Vec<Note>>(drag_id.with("orig"));
        });
        // Re-sort while keeping the selection on the same notes.
        if let Some(Clip { content: ClipContent::Midi { notes }, .. }) = DawApp::clip_mut(&mut app.project, cref) {
            let selected: Vec<Note> = app.note_sel.iter().filter_map(|&i| notes.get(i).copied()).collect();
            notes.sort_by(|a, b| a.start.total_cmp(&b.start));
            app.note_sel = selected.iter().filter_map(|s| notes.iter().position(|n| n == s)).collect();
        }
        app.needs_compile = true;
    }
    playhead(app, &p, grid_area, cref, track, len, grid_area.min.x);
}

fn c_or(c: Color32) -> Color32 {
    shade(c, 1.1)
}

// ============================ Devices ============================

fn device_panel(app: &mut DawApp, ui: &mut egui::Ui) {
    let t = app.selected_track;
    let track = &app.project.tracks[t];
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(10.0, 16.0), Sense::hover());
        ui.painter().rect_filled(r, CornerRadius::same(2), rgb(track.color));
        ui.label(egui::RichText::new(&track.name).strong());
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
