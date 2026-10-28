//! Session view: clip-slot grid (tracks × scenes), scene launchers, mixer strips.

use super::theme::*;
use super::widgets::*;
use crate::app::{ClipRef, DawApp, DragItem, TrackFlag};
use daw_engine::{Command, PendingLaunch};
use daw_project::{ClipContent, TrackKind};
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, pos2, vec2};

// Live 12 Session metrics at 100 %: 94 px columns, 17 px header, 17 px slots
// on an 18 px pitch, ~128 px mixer section.
const COL_W: f32 = 94.0;
const SLOT_H: f32 = 17.0;
const HEADER_H: f32 = 17.0;
const MIXER_H: f32 = 128.0;

pub fn session_view(app: &mut DawApp, ui: &mut egui::Ui) {
    let blink = (app.status.beat_position * 2.0).fract() < 0.5 || !app.status.playing;
    let avail = ui.available_size();
    egui::ScrollArea::both().id_salt("session").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
        ui.horizontal_top(|ui| {
            let n0 = app.project.tracks.len();
            for t in app.project.visible_tracks() {
                if app.project.tracks.len() != n0 {
                    // A header menu added/removed tracks: indices are stale, redraw next frame.
                    ui.ctx().request_repaint();
                    break;
                }
                ui.vertical(|ui| {
                    ui.set_width(COL_W);
                    track_header(app, ui, t);
                    if app.project.tracks.len() != n0 {
                        return;
                    }
                    for s in 0..app.project.scenes.len() {
                        slot(app, ui, t, s, blink);
                    }
                    stop_button(app, ui, t);
                    ui.add_space(4.0);
                    if app.show_io {
                        super::io::io_section(app, ui, t, COL_W);
                    }
                    mixer_strip(app, ui, t);
                });
            }
            // Empty drop column: drop instruments/samples to create tracks.
            let drop_h = (HEADER_H + (SLOT_H + 2.0) * app.project.scenes.len() as f32).max(120.0);
            ui.vertical(|ui| {
                let (rect, resp) = ui.allocate_exact_size(vec2(COL_W, drop_h), Sense::click());
                let hovering = resp.dnd_hover_payload::<DragItem>().is_some();
                ui.painter().rect_stroke(
                    rect.shrink(4.0),
                    CornerRadius::same(4),
                    Stroke::new(1.0, if hovering { ACCENT } else { gray(60) }),
                    egui::StrokeKind::Inside,
                );
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    "Drop instruments\nor samples here",
                    FontId::proportional(10.0),
                    gray(110),
                );
                if let Some(item) = resp.dnd_release_payload::<DragItem>() {
                    drop_new_track(app, &item);
                }
                resp.context_menu(|ui| {
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
            // Master column: scene launchers + master strip.
            let _ = avail;
            ui.vertical(|ui| {
                ui.set_width(COL_W);
                // Master header: click = master device view, right-click = colour.
                let (rect, resp) = ui.allocate_exact_size(vec2(COL_W, HEADER_H), Sense::click());
                super::add_master_zone(ui, rect);
                let bg = if app.master_selected { gray(0x5a) } else { gray(0x46) };
                ui.painter().rect_filled(rect, CornerRadius::ZERO, bg);
                ui.painter().text(rect.left_center() + vec2(4.0, 0.0), Align2::LEFT_CENTER, "Main", bold(FS), text());
                if resp.clicked() {
                    app.select_master();
                }
                resp.context_menu(|ui| master_menu(app, ui));
                for s in 0..app.project.scenes.len() {
                    scene_button(app, ui, s);
                }
                let (rect, resp) = ui.allocate_exact_size(vec2(COL_W, SLOT_H), Sense::click());
                ui.painter().rect_filled(rect, CornerRadius::ZERO, if resp.hovered() { gray(0x40) } else { panel() });
                ui.painter().rect_filled(
                    Rect::from_center_size(rect.left_center() + vec2(10.0, 0.0), vec2(8.0, 8.0)),
                    CornerRadius::ZERO,
                    gray(0x1e),
                );
                ui.painter().text(rect.left_center() + vec2(20.0, 0.0), Align2::LEFT_CENTER, "Stop Clips", font(FS), text());
                if resp.clicked() {
                    app.send(Command::StopAllClips);
                }
                ui.add_space(4.0);
                if app.show_io {
                    super::io::master_io(app, ui, COL_W);
                }
                master_strip(app, ui);
            });
        });
    });
}

fn track_header(app: &mut DawApp, ui: &mut egui::Ui, t: usize) {
    let track = &app.project.tracks[t];
    let (rect, resp) = ui.allocate_exact_size(vec2(COL_W, HEADER_H), Sense::click());
    let selected = app.is_track_selected(t);
    let color = rgb(track.color);
    let is_group = track.kind == TrackKind::Group;
    let folded = track.folded;
    let parent_color = app.project.parent_index(t).map(|p| rgb(app.project.tracks[p].color));
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, if selected { shade(color, 1.12) } else { color });
    let mut x = rect.min.x + 4.0;
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
        p.with_clip_rect(rect.shrink(2.0)).text(
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
    resp.context_menu(|ui| track_menu(app, ui, t));
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

pub fn track_menu(app: &mut DawApp, ui: &mut egui::Ui, t: usize) {
    if ui.button("Rename               Ctrl+R").clicked() {
        app.select_track_header(t, crate::arr_ops::SelMode::Single);
        app.start_rename(t);
        ui.close();
    }
    ui.horizontal(|ui| {
        for (i, c) in daw_project::PALETTE.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::click());
            ui.painter().rect_filled(rect, CornerRadius::same(2), rgb(*c));
            if resp.clicked() {
                app.edit(|p| p.tracks[t].color = daw_project::PALETTE[i]);
            }
        }
    });
    let is_group = app.project.tracks[t].kind == TrackKind::Group;
    if ui.button("Group Tracks       Ctrl+G").clicked() {
        if !app.is_track_selected(t) {
            app.click_track(t, false);
        }
        app.group_selected_tracks();
        ui.close();
    }
    if is_group && ui.button("Ungroup       Ctrl+Shift+G").clicked() {
        app.click_track(t, false);
        app.ungroup_selected();
        ui.close();
    }
    if ui.button(if is_group { "Duplicate Group" } else { "Duplicate Track" }).clicked() {
        app.edit(|p| p.duplicate_track(t));
        app.clear_structure_selection();
        ui.close();
    }
    if ui.button(if is_group { "Delete Group and Tracks" } else { "Delete Track" }).clicked() {
        app.delete_track(t);
        ui.close();
    }
}

/// Group slot: launches the children's clips of that scene (Live).
fn group_slot(app: &mut DawApp, ui: &mut egui::Ui, g: usize, s: usize) {
    let (rect, resp) = ui.allocate_exact_size(vec2(COL_W, SLOT_H), Sense::click());
    let p = &app.project;
    let kids = g + 1..p.subtree_end(g);
    let has = kids.clone().any(|c| p.tracks[c].slots.get(s).is_some_and(Option::is_some));
    let playing = kids.clone().any(|c| app.track_status.get(c).is_some_and(|st| st.playing_scene == Some(s as u32)));
    let color = rgb(p.tracks[g].color);
    let painter = ui.painter_at(rect);
    if has {
        painter.rect_filled(rect, CornerRadius::same(2), shade(color, if playing { 0.8 } else { 0.4 }));
        let launch = rect.left_center() + vec2(10.0, 0.0);
        draw_play(&painter, launch, if playing { PLAY_GREEN } else { gray(25) });
        // Mini overview: one tick per child with a clip.
        let n = kids.len().max(1) as f32;
        for (k, c) in kids.clone().enumerate() {
            if let Some(Some(clip)) = p.tracks[c].slots.get(s) {
                let w = (rect.width() - 26.0) / n;
                let r = Rect::from_min_size(
                    pos2(rect.min.x + 22.0 + k as f32 * w, rect.min.y + 6.0),
                    vec2((w - 2.0).max(1.0), rect.height() - 12.0),
                );
                painter.rect_filled(r, CornerRadius::same(1), rgb(clip.color));
            }
        }
    } else {
        painter.rect_filled(rect, CornerRadius::same(2), if resp.hovered() { gray(56) } else { shade(gray(46), 0.9) });
    }
    if resp.clicked() {
        app.click_track(g, false);
        if has {
            app.launch_group_slot(g, s);
        } else {
            app.stop_group(g);
        }
    }
}

fn slot(app: &mut DawApp, ui: &mut egui::Ui, t: usize, s: usize, blink: bool) {
    if app.project.tracks[t].kind == TrackKind::Group {
        return group_slot(app, ui, t, s);
    }
    let (rect, resp) = ui.allocate_exact_size(vec2(COL_W, SLOT_H), Sense::click_and_drag());
    let st = app.track_status.get(t).copied().unwrap_or_default();
    let playing = st.playing_scene == Some(s as u32);
    let pending = st.pending == Some(PendingLaunch::Scene(s as u32));
    let selected = app.selected_clip == Some(ClipRef::Slot { track: t, scene: s });
    let track = &app.project.tracks[t];
    let armed = track.arm;
    let recording_here = app.session_rec.is_some_and(|(id, sc)| id == track.id && sc == s);
    let p = ui.painter_at(rect);
    let launch_zone = Rect::from_min_size(rect.min, vec2(16.0, rect.height()));

    match track.slots.get(s).and_then(|c| c.as_ref()) {
        Some(clip) => {
            let c = if clip.muted { disabled(rgb(clip.color)) } else { rgb(clip.color) };
            let bg = if selected { SEL_BLUE } else { c };
            p.rect_filled(rect, CornerRadius::ZERO, bg);
            let tri = if playing || (pending && blink) { PLAY_GREEN } else { Color32::from_gray(0x1c) };
            let lc = launch_zone.center();
            p.add(egui::Shape::convex_polygon(vec![lc + vec2(-3.5, -4.5), lc + vec2(4.5, 0.0), lc + vec2(-3.5, 4.5)], tri, Stroke::NONE));
            let name = match &clip.content {
                ClipContent::Audio { .. } => clip.name.clone(),
                ClipContent::Midi { .. } => clip.name.clone(),
            };
            p.with_clip_rect(rect.shrink(2.0)).text(
                pos2(rect.min.x + 18.0, rect.center().y),
                Align2::LEFT_CENTER,
                name,
                font(FS),
                text_on(bg),
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
            p.rect_filled(rect, CornerRadius::ZERO, if resp.hovered() { gray(0x40) } else { panel() });
            p.line_segment([rect.left_bottom() + vec2(0.0, 0.5), rect.right_bottom() + vec2(0.0, 0.5)], Stroke::new(1.0, gray(0x2a)));
            if armed {
                let col = if recording_here && blink { REC_RED } else { gray(if recording_here { 90 } else { 120 }) };
                p.circle_filled(rect.left_center() + vec2(10.0, 0.0), 4.5, col);
            } else {
                p.rect_filled(Rect::from_center_size(rect.left_center() + vec2(10.0, 0.0), vec2(8.0, 8.0)), CornerRadius::ZERO, gray(0x1e));
            }
            if pending && blink {
                p.rect_stroke(rect, CornerRadius::same(2), Stroke::new(1.0, PLAY_GREEN), egui::StrokeKind::Inside);
            }
            if resp.double_clicked() && track.kind == TrackKind::Midi {
                app.create_midi_clip(t, s);
            } else if resp.clicked() {
                app.selected_track = t;
                if armed {
                    app.record_into_slot(t, s);
                } else if let Some(rt) = app.rt(t) {
                    app.send(Command::StopTrack { track: rt });
                }
            }
            resp.context_menu(|ui| {
                if ui.button("Insert MIDI Clip").clicked() {
                    app.create_midi_clip(t, s);
                    ui.close();
                }
            });
        }
    }
    // Drops: samples / instruments / clips.
    if resp.dnd_hover_payload::<DragItem>().is_some() {
        p.rect_stroke(rect, CornerRadius::same(2), Stroke::new(1.5, ACCENT), egui::StrokeKind::Inside);
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
        DragItem::SessionClip { .. } => {}
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

fn stop_button(app: &mut DawApp, ui: &mut egui::Ui, t: usize) {
    let (rect, resp) = ui.allocate_exact_size(vec2(COL_W, SLOT_H), Sense::click());
    let is_group = app.project.tracks[t].kind == TrackKind::Group;
    let st = app.track_status.get(t).copied().unwrap_or_default();
    let active = if is_group {
        (t + 1..app.project.subtree_end(t)).any(|c| app.track_status.get(c).is_some_and(|s| s.playing_scene.is_some()))
    } else {
        st.playing_scene.is_some()
    };
    ui.painter().rect_filled(rect, CornerRadius::ZERO, if resp.hovered() { gray(0x40) } else { panel() });
    let c = if st.pending == Some(PendingLaunch::Stop) {
        ACCENT
    } else if active {
        gray(0xc8)
    } else {
        gray(0x1e)
    };
    ui.painter().rect_filled(Rect::from_center_size(rect.left_center() + vec2(10.0, 0.0), vec2(8.0, 8.0)), CornerRadius::ZERO, c);
    if resp.clicked() {
        if is_group {
            app.stop_group(t);
        } else if let Some(rt) = app.rt(t) {
            app.send(Command::StopTrack { track: rt });
        }
    }
}

fn scene_button(app: &mut DawApp, ui: &mut egui::Ui, s: usize) {
    let (rect, resp) = ui.allocate_exact_size(vec2(COL_W, SLOT_H), Sense::click());
    // Live scene launcher: ▶ at the left, scene name right-aligned.
    ui.painter().rect_filled(rect, CornerRadius::ZERO, if resp.hovered() { gray(0x46) } else { gray(0x3c) });
    let c = rect.left_center() + vec2(9.0, 0.0);
    ui.painter().add(egui::Shape::convex_polygon(
        vec![c + vec2(-3.5, -4.5), c + vec2(4.5, 0.0), c + vec2(-3.5, 4.5)],
        gray(0x1c),
        Stroke::NONE,
    ));
    ui.painter().text(rect.right_center() - vec2(10.0, 0.0), Align2::RIGHT_CENTER, &app.project.scenes[s].name, font(FS), text());
    if resp.clicked() {
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

fn mixer_strip(app: &mut DawApp, ui: &mut egui::Ui, t: usize) {
    let (rect, _) = ui.allocate_exact_size(vec2(COL_W, MIXER_H), Sense::hover());
    let sel = app.is_track_selected(t);
    ui.painter().rect_filled(rect, CornerRadius::ZERO, if sel { gray(0x46) } else { panel() });
    let o = rect.min;
    // Volume pill · pan knob · fader with dB scale (Live).
    let mut db = app.project.tracks[t].gain_db;
    let (r, changed) =
        vol_pill(ui, Rect::from_min_size(o + vec2(8.0, 6.0), vec2(30.0, 11.0)), ui.scope_id().with(("svol_pill", t)), &mut db);
    let (r2, changed2) = fader_meter(
        ui,
        Rect::from_min_size(o + vec2(44.0, 6.0), vec2(46.0, MIXER_H - 12.0)),
        ui.scope_id().with(("sfader", t)),
        &mut db,
        app.track_meters.get(t).copied().unwrap_or([0.0; 2]),
    );
    if r.drag_started() || r.double_clicked() || r2.drag_started() || r2.double_clicked() {
        app.begin_gesture();
    }
    if changed || changed2 {
        app.set_track_gain(t, db);
    }
    let mut pan = app.project.tracks[t].pan;
    let (r, changed) = knob(ui, o + vec2(23.0, 36.0), 12.0, ui.scope_id().with(("span", t)), &mut pan);
    if r.drag_started() || r.double_clicked() {
        app.begin_gesture();
    }
    if changed {
        app.set_track_pan(t, pan);
    }
    // Activator / S / arm stacked (Live).
    let track = &app.project.tracks[t];
    let (mute, solo, arm, kind, has_input) = (track.mute, track.solo, track.arm, track.kind, track.has_input());
    let ctrl = ui.input(|i| i.modifiers.command);
    let act_col = if app.solo_silenced.get(t).copied().unwrap_or(false) { solo_dimmed(ACCENT) } else { ACCENT };
    let act = Rect::from_min_size(o + vec2(13.0, 63.0), vec2(21.0, 15.0));
    if rect_toggle(ui, act, ui.scope_id().with(("sact", t)), &format!("{}", t + 1), !mute, act_col, "Track activator (mute)").clicked() {
        app.toggle_track_flag(t, TrackFlag::Mute, false);
    }
    let sr = Rect::from_min_size(o + vec2(13.0, 82.0), vec2(21.0, 13.0));
    if rect_toggle(ui, sr, ui.scope_id().with(("ssolo", t)), "S", solo, Color32::from_rgb(0x5a, 0xa8, 0xff), "Solo (Ctrl: add)").clicked() {
        app.toggle_track_flag(t, TrackFlag::Solo, !ctrl);
    }
    let ar = Rect::from_min_size(o + vec2(13.0, 99.0), vec2(21.0, 13.0));
    if kind != TrackKind::Group
        && has_input
        && rect_toggle(
            ui,
            ar,
            ui.scope_id().with(("sarm", t)),
            "REC",
            arm,
            REC_RED,
            "Arm for recording (all selected tracks; Ctrl: just this one)",
        )
        .clicked()
    {
        app.click_arm(t, ctrl);
    }
    let voices = app.track_status.get(t).map_or(0, |s| s.voices);
    if kind == TrackKind::Midi && voices > 0 {
        ui.painter().text(o + vec2(23.0, MIXER_H - 6.0), Align2::CENTER_CENTER, format!("{voices}v"), font(FS_SMALL), dim());
    }
}

fn master_strip(app: &mut DawApp, ui: &mut egui::Ui) {
    let (rect, resp) = ui.allocate_exact_size(vec2(COL_W, MIXER_H), Sense::click());
    super::add_master_zone(ui, rect);
    if resp.clicked() {
        app.select_master();
    }
    ui.painter().rect_filled(rect, CornerRadius::ZERO, if app.master_selected { gray(0x46) } else { panel() });
    let o = rect.min;
    let mut db = app.project.master_gain_db;
    let (r, changed) = vol_pill(ui, Rect::from_min_size(o + vec2(8.0, 6.0), vec2(30.0, 11.0)), ui.scope_id().with("mvol_pill"), &mut db);
    let (r2, changed2) = fader_meter(
        ui,
        Rect::from_min_size(o + vec2(44.0, 6.0), vec2(46.0, MIXER_H - 12.0)),
        ui.scope_id().with("mfader"),
        &mut db,
        app.meter,
    );
    if r.drag_started() || r.double_clicked() || r2.drag_started() || r2.double_clicked() {
        app.begin_gesture();
    }
    if changed || changed2 {
        app.project.master_gain_db = db;
        app.dirty = true;
        app.send_now(Command::SetMasterGain(daw_project::db_to_gain(db)));
    }
    let mut pan = app.project.master_pan;
    let (r, changed) = knob(ui, o + vec2(23.0, 36.0), 12.0, ui.scope_id().with("mpan_knob"), &mut pan);
    if r.drag_started() || r.double_clicked() {
        app.begin_gesture();
    }
    if changed {
        app.project.master_pan = pan;
        app.dirty = true;
        app.send_now(Command::SetMasterPan(pan));
    }
    // Blue Cue volume knob (metronome + browser preview), like Live's Cue knob.
    let mut cue = app.project.cue_gain_db;
    let mut cv = (db_to_pos(cue) * 2.0 - 1.0).clamp(-1.0, 1.0);
    let (r, changed) = knob(ui, o + vec2(23.0, 80.0), 10.0, ui.scope_id().with("mcue_knob"), &mut cv);
    if r.drag_started() || r.double_clicked() {
        app.begin_gesture();
    }
    if changed {
        cue = if r.double_clicked() { 0.0 } else { pos_to_db((cv + 1.0) * 0.5) };
        app.project.cue_gain_db = cue;
        app.dirty = true;
        app.send_now(Command::SetCueGain(daw_project::db_to_gain(cue)));
    }
    let _ = cue_box;
    ui.painter().circle_stroke(o + vec2(23.0, 80.0), 10.0, Stroke::new(1.5, CUE_BLUE));
    r.on_hover_text(format!(
        "Cue volume {} dB (metronome + preview)",
        if cue <= METER_FLOOR_DB { "-inf".into() } else { format!("{cue:.1}") }
    ));
}
