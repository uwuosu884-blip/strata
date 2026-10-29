//! GUI root: layout, transport bar, shortcuts, dialogs. Views live in submodules.

pub(crate) mod arrangement;
mod browser;
pub mod clip;
pub mod cursors;
mod detail;
mod invert;
mod io;
mod menu;
mod prefs;
mod session;
pub mod theme;
mod topbar;
pub mod widgets;

use crate::app::{DawApp, DetailMode, Dialog, DragItem, PendingAction, View};
use daw_engine::Command;
use daw_project::{InstrumentDef, TrackKind, db_to_gain};
use eframe::egui::{self, Key, Modifiers};
use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;
use theme::{panel, win_bg};
use widgets::*;

/// Deferred work from a dialog button (runs after the dialog closes).
type DialogAction = Box<dyn FnOnce(&mut DawApp)>;

/// Fixed Device View height.
const DEVICE_VIEW_H: f32 = 199.0; // Live: 2 px focus line + 9 px gap + 188 px panel

impl eframe::App for DawApp {
    /// Take plain (unmodified) key presses before egui sees them, unless a text
    /// field is focused or a dialog is open: Space/Tab/Enter must drive the DAW,
    /// not egui's focus navigation, and piano keys must not trigger widgets.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if matches!(self.dialog, Some(Dialog::ConfirmDiscard(_))) {
            // Keyboard-driven dialog: arrows / Enter / Y N C (see `save_changes_dialog`), not egui focus.
            raw.events.retain(|e| match e {
                egui::Event::Key { modifiers, key, pressed, .. } if !modifiers.command && !modifiers.alt && *key != Key::Escape => {
                    if *pressed {
                        self.dialog_keys.push(*key);
                    }
                    false
                }
                egui::Event::Text(_) => false,
                _ => true,
            });
            return;
        }
        if self.dialog.is_some() || ctx.egui_wants_keyboard_input() {
            return;
        }
        let mut took_key = false;
        raw.events.retain(|e| match e {
            egui::Event::Key { modifiers, key, .. } if !modifiers.command && !modifiers.alt && *key != Key::Escape => {
                self.key_events.push(e.clone());
                took_key = true;
                false
            }
            egui::Event::Text(_) => !took_key,
            _ => true,
        });
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        theme::update(&ctx, &self.settings.theme);
        {
            let t = &self.settings.theme;
            let manual = (!t.auto_assign_colors).then(|| t.default_track_color.unwrap_or(daw_project::PALETTE[0]));
            let random = t.clip_color == theme::ClipColorMode::Random;
            let c = &mut self.project.colors;
            if c.manual != manual || c.reduced != t.reduced_colors || c.random_clips != random {
                (c.manual, c.reduced, c.random_clips) = (manual, t.reduced_colors, random);
            }
        }
        let z = self.settings.zoom() as f32 / 100.0;
        if (ctx.zoom_factor() - z).abs() > 1e-3 {
            ctx.set_zoom_factor(z);
        }
        self.begin_frame();
        self.handle_close(&ctx);
        // Any press ends header focus; a header click (on release) sets it again.
        if ctx.input(|i| i.pointer.primary_pressed()) {
            self.head_focus = false;
        }
        self.shortcuts(&ctx);
        clip::groove_pool(self, &ctx);
        // Clip launch button released anywhere → ReleaseClip.
        let held_id = egui::Id::unique("launch_held");
        if ctx.input(|i| i.pointer.primary_released())
            && let Some((t, s)) = ctx.data(|d| d.get_temp::<(usize, usize)>(held_id))
        {
            ctx.data_mut(|d| d.remove::<(usize, usize)>(held_id));
            self.release_clip(t, s);
        }

        // Live layout at 100 %: 47 px top bar, panels separated by 9 px gaps of the
        // window colour, 19 px status bar, 2 px outline around the focused panel.
        let m = |l: i8, r: i8, t: i8, b: i8| egui::Margin { left: l, right: r, top: t, bottom: b };
        egui::Panel::top("transport")
            .exact_size(47.0)
            .resizable(false)
            .frame(egui::Frame::NONE.fill(win_bg()).inner_margin(m(9, 9, 0, 0)))
            .show(ui, |ui| self.transport_bar(ui));
        egui::Panel::bottom("status")
            .show_separator_line(false)
            .exact_size(36.0)
            .resizable(false)
            .frame(egui::Frame::NONE.fill(win_bg()).inner_margin(m(9, 9, 9, 8)))
            .show(ui, |ui| self.status_bar(ui));
        // Selecting a (different) clip switches the detail panel to the clip view.
        let prev_id = egui::Id::unique("prev_selected_clip");
        let prev: Option<Option<crate::app::ClipRef>> = ctx.data(|d| d.get_temp(prev_id));
        if prev != Some(self.selected_clip) {
            if self.selected_clip.is_some() {
                self.detail_mode = DetailMode::Clip;
            }
            ctx.data_mut(|d| d.insert_temp(prev_id, self.selected_clip));
        }
        // Clip View / piano roll: resizable (own remembered size); Device View: fixed.
        let dpanel = if self.detail_mode == DetailMode::Device || self.master_selected {
            egui::Panel::bottom("detail_device").resizable(false).exact_size(DEVICE_VIEW_H)
        } else {
            egui::Panel::bottom("detail").resizable(true).default_size(300.0).size_range(120.0..=700.0)
        };
        let detail_rect = dpanel
            .show_separator_line(false)
            // Window-coloured 9 px margin (no unpainted gaps), rounded panel inside.
            .frame(egui::Frame::NONE.fill(win_bg()).inner_margin(m(9, 9, 11, 0)))
            .show(ui, |ui| {
                ui.painter().rect_filled(ui.max_rect(), 3.0, panel());
                detail::detail_panel(self, ui)
            })
            .response
            .rect;
        let detail_inner = detail_rect - m(9, 9, 11, 0);
        // Keys like Del / Ctrl+D / 0 act on whatever was clicked last: notes or the main view.
        if let Some(p) = ctx.input(|i| if i.pointer.any_pressed() { i.pointer.press_origin() } else { None }) {
            self.focus_detail = detail_inner.contains(p);
        }
        if self.show_browser {
            egui::Panel::left("browser")
                .resizable(true)
                .default_size(344.0)
                .size_range(200.0..=520.0)
                .frame(egui::Frame::NONE.fill(panel()).outer_margin(m(9, 9, 0, 0)))
                .show(ui, |ui| browser::browser(self, ui));
        }
        ctx.data_mut(|d| d.remove::<Vec<egui::Rect>>(master_zone_id()));
        let left = if self.show_browser { 0 } else { 9 };
        let central = egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(panel()).outer_margin(m(left, 9, 0, 0)))
            .show(ui, |ui| match self.view {
                View::Session => session::session_view(self, ui),
                View::Arrangement => arrangement::arrangement_view(self, ui),
            })
            .response
            .rect;
        let central = central - m(left, 9, 0, 0);
        // Focus outline (drawn in the gap, like Live).
        let fr = if self.focus_detail { detail_inner } else { central };
        ui.painter().rect_stroke(fr, 4.0, egui::Stroke::new(2.0, theme::focus()), egui::StrokeKind::Outside);
        // A press elsewhere in the main view deselects the master.
        if let Some(p) = ctx.input(|i| if i.pointer.any_pressed() { i.pointer.press_origin() } else { None })
            && central.contains(p)
            && !ctx.data(|d| d.get_temp::<Vec<egui::Rect>>(master_zone_id())).is_some_and(|z| z.iter().any(|r| r.contains(p)))
        {
            self.master_selected = false;
        }

        self.dialogs(&ctx);
        drag_preview(&ctx);
        cursors::paint(&ctx);
        self.end_frame();
        self.update_title(&ctx);

        let moving = self.status.playing
            || self.meter.iter().any(|m| *m > 1e-4)
            || self.track_meters.iter().any(|m| m[0] > 1e-4)
            || self.message.is_some()
            || self.cache.is_pending();
        ctx.request_repaint_after(if moving { Duration::from_millis(16) } else { Duration::from_millis(250) });
    }
}

impl DawApp {
    fn handle_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.dirty {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.dialog = Some(Dialog::ConfirmDiscard(PendingAction::Quit));
        }
        if self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let t = format!("{}{} — daw", self.project.name, if self.dirty { " *" } else { "" });
        if t != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(t.clone()));
            self.title = t;
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.dialog.is_some() {
            // Plain keys drive the dialog (arrows / Enter / Y N C in "Save changes").
            for ev in self.key_events.drain(..) {
                if let egui::Event::Key { key, pressed: true, .. } = ev {
                    self.dialog_keys.push(key);
                }
            }
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.dialog = None;
            }
            return;
        }
        let typing = ctx.egui_wants_keyboard_input();
        let cmd = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, k));
        let cmd_shift = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, k));
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT | Modifiers::ALT, Key::E)) {
            self.toggle_audio_engine();
        }
        if cmd_shift(Key::Z) {
            self.redo();
        }
        if cmd(Key::Z) {
            self.undo();
        }
        if cmd(Key::Y) {
            self.redo();
        }
        if cmd_shift(Key::S) {
            self.dialog = Some(Dialog::SaveAs(self.default_path("dawproj")));
        }
        if cmd(Key::S) {
            self.save();
        }
        if cmd(Key::O) {
            self.dialog = Some(Dialog::Open(self.default_path("dawproj")));
        }
        if cmd(Key::N) {
            self.guarded(PendingAction::New);
        }
        if cmd_shift(Key::R) {
            self.dialog = Some(Dialog::Export { path: self.default_path("wav"), scene: None, bits: 24 });
        }
        // Alt+Plus / Alt+Minus: track height (Live).
        let alt = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::ALT, k));
        if !typing {
            if alt(Key::Plus) || alt(Key::Equals) {
                self.zoom_heights_key(1.25);
            }
            if alt(Key::Minus) {
                self.zoom_heights_key(0.8);
            }
        }
        // Shift variants first: consume_key(COMMAND, k) also matches Ctrl+Shift+k.
        if cmd_shift(Key::T) {
            self.add_track(TrackKind::Midi);
        }
        if cmd(Key::T) {
            self.add_track(TrackKind::Audio);
        }
        if cmd_shift(Key::G) {
            self.ungroup_selected();
        }
        if cmd(Key::G) {
            self.group_selected_tracks();
        }
        if cmd_shift(Key::M) {
            // Session: the selected empty slot; Arrangement: the time selection.
            match (self.view, self.empty_slot) {
                (View::Session, Some((t, s))) if self.selected_clip.is_none() => {
                    if self.project.tracks.get(t).is_some_and(|tr| tr.kind == TrackKind::Midi) {
                        self.create_midi_clip(t, s);
                    }
                }
                _ => self.insert_midi_clip_in_selection(),
            }
        }
        if cmd(Key::L) {
            self.loop_selection();
        }
        if cmd(Key::E) {
            self.split_selection();
        }
        if cmd_shift(Key::J) {
            // Crop the clip shown in Clip View.
            if let Some(c) = self.selected_clip {
                self.crop_clip(c);
            }
        }
        if cmd(Key::J) {
            self.consolidate_selection();
        }
        if cmd_shift(Key::U) {
            // Quantize settings = MIDI Tools ▸ Quantize.
            self.cv.panel = clip::NotesPanel::Transform;
            self.cv.transform = daw_project::miditools::Transform::Quantize;
            self.cv.folded = false;
        }
        if cmd(Key::U)
            && let Some(c) = self.selected_clip
        {
            self.quantize_clip(c);
        }
        if cmd(Key::R) {
            self.rename_selection();
        }
        if cmd(Key::A) {
            // Piano roll focused: all notes of the clip; else all arrangement clips.
            if !(self.focus_detail && self.select_all_notes()) && self.view == View::Arrangement {
                self.arr_sel = self.project.tracks.iter().flat_map(|t| t.arrangement.iter().map(|a| a.clip.id)).collect();
                self.resync_selected_clip();
            }
        }
        if cmd(Key::D)
            && !self.duplicate_selected_notes()
            && !(!self.focus_detail && self.duplicate_selected_tracks())
            && !(self.view == View::Arrangement && self.duplicate_arrangement_selection())
        {
            self.duplicate_selected_clip();
        }
        // Grid (Live: Ctrl+1 narrow, Ctrl+2 widen, Ctrl+3 triplet, Ctrl+4 snap).
        if !typing && !self.focus_detail {
            if cmd(Key::Num1) {
                self.narrow_grid(true);
            }
            if cmd(Key::Num2) {
                self.narrow_grid(false);
            }
            if cmd(Key::Num3) {
                self.arr_triplet = !self.arr_triplet;
            }
            if cmd(Key::Num4) {
                self.toggle_snap();
            }
        }
        // Cut / Copy / Paste arrive as events (egui-winit), not as keys.
        if !typing && self.view == View::Arrangement && !self.focus_detail {
            let (copy, cut, paste) = ctx.input(|i| {
                i.events.iter().fold((false, false, false), |a, e| match e {
                    egui::Event::Copy => (true, a.1, a.2),
                    egui::Event::Cut => (a.0, true, a.2),
                    egui::Event::Paste(_) => (a.0, a.1, true),
                    _ => a,
                })
            });
            let text = if cut {
                self.cut_selection()
            } else if copy {
                self.copy_selection()
            } else {
                None
            };
            if let Some(t) = text {
                ctx.copy_text(t);
            }
            if paste {
                self.paste();
            }
        }
        if cmd(Key::B) {
            self.show_browser = !self.show_browser;
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::ALT, Key::I)) {
            self.show_io = !self.show_io;
        }
        if cmd(Key::Comma) {
            self.open_settings(crate::app::SettingsTab::Audio);
        }
        if typing {
            self.key_events.clear();
            return;
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.time_sel = None;
            self.arr_sel.clear();
            self.track_sel.clear();
            self.note_sel.clear();
            self.touched_track = None;
            self.pr_time_sel = None;
            self.renaming = None;
        }
        let events = std::mem::take(&mut self.key_events);
        let hover_monitor = self.hover_monitor.take(); // set again by this frame's UI
        for ev in events {
            let egui::Event::Key { key, pressed, repeat, modifiers, .. } = ev else { continue };
            if modifiers.command || modifiers.alt {
                continue;
            }
            if pressed && !repeat {
                match key {
                    Key::Space => {
                        if modifiers.shift {
                            self.send_now(Command::Play); // continue from where it stopped
                        } else {
                            self.toggle_play();
                        }
                        continue;
                    }
                    Key::Tab if modifiers.shift => {
                        // Clip view (piano roll / sample) ⇄ device view.
                        self.detail_mode = match self.detail_mode {
                            DetailMode::Clip => DetailMode::Device,
                            DetailMode::Device => DetailMode::Clip,
                        };
                        continue;
                    }
                    Key::Tab => {
                        self.view = if self.view == View::Session { View::Arrangement } else { View::Session };
                        continue;
                    }
                    Key::Num0 => {
                        self.toggle_disable();
                        continue;
                    }
                    // Zoom to / back from the time selection (Live), unless the
                    // computer MIDI keyboard uses Z/X for octaves.
                    Key::Z if !self.kb.enabled && self.view == View::Arrangement => {
                        self.zoom_to_selection();
                        continue;
                    }
                    Key::X if !self.kb.enabled && self.view == View::Arrangement => {
                        self.zoom_back_from_selection();
                        continue;
                    }
                    Key::F9 => {
                        self.toggle_record();
                        continue;
                    }
                    Key::Delete | Key::Backspace if hover_monitor.is_some() => {
                        // Monitor radio: Return to Default.
                        if let Some(t) = hover_monitor {
                            self.set_monitor(t, None);
                        }
                        continue;
                    }
                    Key::Delete | Key::Backspace => {
                        if self.focus_detail {
                            self.delete_selected_notes(); // piano roll focused: never touch clips/tracks
                            continue;
                        }
                        if !(self.view == View::Arrangement && self.delete_arrangement_selection()) {
                            // Clips first; tracks when they were selected in the headers.
                            let slot = matches!(self.selected_clip, Some(crate::app::ClipRef::Slot { .. }));
                            if !(slot && self.view == View::Session) && self.delete_selected_tracks() {
                                continue;
                            }
                            self.delete_selected_clip();
                        }
                        continue;
                    }
                    // Loop brace selected (Clip View): Up/Down move it by its length.
                    Key::ArrowUp | Key::ArrowDown if self.focus_detail && self.cv.brace_sel && !modifiers.shift => {
                        if let Some(c) = self.selected_clip {
                            self.shift_loop(c, if key == Key::ArrowUp { 1.0 } else { -1.0 });
                        }
                        continue;
                    }
                    // Selected notes: Up/Down = semitone, Shift+Up/Down = octave.
                    Key::ArrowUp | Key::ArrowDown if self.focus_detail && !self.note_sel.is_empty() => {
                        let s = if modifiers.shift { 12 } else { 1 };
                        self.transpose_selected_notes(if key == Key::ArrowUp { s } else { -s });
                        continue;
                    }
                    // Live: M toggles the computer keyboard (with Shift too).
                    Key::M => {
                        self.kb.enabled = !self.kb.enabled;
                        for (_, p) in std::mem::take(&mut self.kb.held) {
                            self.send_now(Command::KeyboardNote { pitch: p, velocity: 0 });
                        }
                        continue;
                    }
                    Key::Enter => {
                        if let Some(crate::app::ClipRef::Slot { scene, .. }) = self.selected_clip {
                            self.launch_scene(scene);
                        }
                        continue;
                    }
                    _ => {}
                }
            }
            if self.kb.enabled && !(pressed && repeat) {
                self.key_note(key, pressed);
            }
        }
        // Release held notes if the window loses focus.
        if !ctx.input(|i| i.focused) && !self.kb.held.is_empty() {
            for (k, _) in self.kb.held.clone() {
                self.key_note(k, false);
            }
        }
    }

    fn tempo_box(&mut self, ui: &mut egui::Ui) {
        let id = ui.make_persistent_id("tempo_box");
        let edit_id = id.with("editing");
        let acc_id = id.with("acc");
        let mut editing: Option<String> = ui.data(|d| d.get_temp(edit_id));
        let size = ui.available_size();
        if let Some(text) = editing.as_mut() {
            let r = ui.add_sized(size, egui::TextEdit::singleline(text).id(id.with("te")));
            // Focus once when editing starts; a click anywhere else commits.
            let started = id.with("started");
            if !ui.data(|d| d.get_temp::<bool>(started).unwrap_or(false)) {
                r.request_focus();
                ui.data_mut(|d| d.insert_temp(started, true));
            }
            let outside = ui.input(|i| i.pointer.any_pressed()) && !r.hovered();
            if outside {
                r.surrender_focus();
            }
            if r.lost_focus() || outside {
                ui.data_mut(|d| d.remove::<bool>(started));
                if !ui.input(|i| i.key_pressed(Key::Escape))
                    && let Ok(v) = text.trim().trim_end_matches("BPM").trim().parse::<f64>()
                {
                    let v = (v.clamp(20.0, 999.0) * 100.0).round() / 100.0;
                    self.edit(|p| p.tempo_bpm = v);
                    self.send_now(Command::SetTempo(v));
                }
                editing = None;
            }
            ui.data_mut(|d| match editing {
                Some(t) => {
                    d.insert_temp(edit_id, t);
                }
                None => d.remove::<String>(edit_id),
            });
            return;
        }
        let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
        let delta = endless_drag(ui, &resp);
        let shift = ui.input(|i| i.modifiers.shift);
        if resp.drag_started() {
            self.begin_gesture();
            ui.data_mut(|d| d.insert_temp(acc_id, self.project.tempo_bpm));
        }
        // Wheel: one notch = 1 BPM (Shift: 0.01). Shift+wheel arrives as horizontal scroll.
        let mut wheel = 0.0;
        if resp.hovered() {
            let notch = wheel_notch(ui);
            if notch != 0.0 {
                wheel = notch as f64;
                ui.input_mut(|i| i.smooth_scroll_delta = egui::Vec2::ZERO);
            }
        }
        let mut target = None;
        if delta != egui::Vec2::ZERO {
            // Accumulate the raw drag so slow drags still step whole BPMs.
            let k = if shift { 0.01 } else { 0.25 };
            let acc: f64 = ui.data(|d| d.get_temp(acc_id)).unwrap_or(self.project.tempo_bpm) + ((delta.x - delta.y) * k) as f64;
            ui.data_mut(|d| d.insert_temp(acc_id, acc));
            target = Some(if shift { acc } else { acc.round() });
        } else if wheel != 0.0 {
            // One undo step per burst of wheel notches.
            if ui.input(|i| i.time) - ui.data(|d| d.get_temp::<f64>(id.with("wt"))).unwrap_or(f64::MIN) > 0.6 {
                self.begin_gesture();
            }
            ui.data_mut(|d| d.insert_temp(id.with("wt"), ui.input(|i| i.time)));
            target = Some(if shift { self.project.tempo_bpm + wheel * 0.01 } else { self.project.tempo_bpm.round() + wheel });
        }
        if let Some(t) = target {
            let t = (t.clamp(20.0, 999.0) * 100.0).round() / 100.0;
            if t != self.project.tempo_bpm {
                self.project.tempo_bpm = t;
                self.dirty = true;
                self.send_now(Command::SetTempo(t));
            }
        }
        if resp.double_clicked() {
            ui.data_mut(|d| d.insert_temp(edit_id, tempo_text(self.project.tempo_bpm)));
        }
        let active = resp.hovered() || resp.dragged();
        ui.painter().rect_filled(rect, 0.0, if active { gray(0x40) } else { panel() });
        // Unfollowed tempo automation: value shown dim (Live greys the box).
        let col = if self.project.tempo_follow { theme::text() } else { gray(0x80) };
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            format!("{:.2}", self.project.tempo_bpm),
            theme::font(theme::FS),
            col,
        );
        let follow = self.project.tempo_follow;
        resp.context_menu(|ui| {
            if ui.selectable_label(follow, "Follow Tempo Automation / Leader Clips").clicked() {
                self.edit(|p| p.tempo_follow = !follow);
                self.send_now(Command::SetTempoFollow(!follow));
                ui.close();
            }
        });
        resp.on_hover_text(
            "Tempo: drag up/down or wheel (Shift = 0.01 steps), double-click to type\nRight-click: Follow tempo automation / Leader clips",
        );
    }

    /// DAW output (monitoring) level: added on top of the master on the device
    /// only, never in renders / Resampling. Endless drag like the tempo box.
    fn output_box(&mut self, ui: &mut egui::Ui) {
        const MIN: f32 = -70.0; // shown as -inf
        let id = ui.make_persistent_id("output_box");
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        let delta = endless_drag(ui, &resp);
        let shift = ui.input(|i| i.modifiers.shift);
        let cur = self.settings.output_gain_db;
        if resp.drag_started() {
            ui.data_mut(|d| d.insert_temp(id, cur));
        }
        let mut target = None;
        if delta != egui::Vec2::ZERO {
            let k = if shift { 0.01 } else { 0.1 };
            let acc: f32 = ui.data(|d| d.get_temp(id)).unwrap_or(cur) + (delta.x - delta.y) * k;
            let acc = acc.clamp(MIN, 6.0);
            ui.data_mut(|d| d.insert_temp(id, acc));
            target = Some(if shift { acc } else { (acc * 10.0).round() / 10.0 });
        }
        if resp.hovered() {
            let notch = wheel_notch(ui);
            if notch != 0.0 {
                target = Some(cur + notch * if shift { 0.1 } else { 0.5 });
                ui.input_mut(|i| i.smooth_scroll_delta = egui::Vec2::ZERO);
            }
        }
        if resp.double_clicked() {
            target = Some(0.0);
        }
        if let Some(t) = target {
            let t = t.clamp(MIN, 6.0);
            if t != cur {
                self.settings.output_gain_db = t;
                self.send_now(Command::SetMonitorGain(if t <= MIN { 0.0 } else { db_to_gain(t) }));
            }
        }
        if resp.drag_stopped() || (target.is_some() && !resp.dragged()) {
            self.save_settings();
        }
        let active = resp.hovered() || resp.dragged();
        ui.painter().rect_filled(rect, 0.0, if active { gray(0x40) } else { panel() });
        let db = self.settings.output_gain_db;
        let text = if db <= MIN { "-inf".to_string() } else { format!("{db:.1}") };
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, theme::font(theme::FS_SMALL), theme::text());
        resp.on_hover_text(
            "Output level (monitoring only, not part of renders): drag up/down or wheel (Shift = fine), double-click = 0 dB",
        );
    }

    /// Time signature (N/4): drag up/down or wheel; double-click resets to 4/4.
    fn signature_box(&mut self, ui: &mut egui::Ui) {
        let id = ui.make_persistent_id("sig_box");
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        let delta = endless_drag(ui, &resp);
        if resp.drag_started() {
            ui.data_mut(|d| d.insert_temp(id, self.project.beats_per_bar as f32));
        }
        let mut target = None;
        if delta != egui::Vec2::ZERO {
            let acc: f32 = ui.data(|d| d.get_temp(id)).unwrap_or(self.project.beats_per_bar as f32) + (delta.x - delta.y) * 0.08;
            ui.data_mut(|d| d.insert_temp(id, acc));
            target = Some(acc.round() as i64);
        }
        if resp.hovered() {
            let notch = wheel_notch(ui);
            if notch != 0.0 {
                target = Some(self.project.beats_per_bar as i64 + notch as i64);
                ui.input_mut(|i| i.smooth_scroll_delta = egui::Vec2::ZERO);
            }
        }
        if resp.double_clicked() {
            target = Some(4);
        }
        if let Some(t) = target {
            let bpb = t.clamp(1, 32) as u32;
            if bpb != self.project.beats_per_bar {
                self.edit(|p| p.beats_per_bar = bpb);
                self.send_now(Command::SetBeatsPerBar(bpb));
            }
        }
        let active = resp.hovered() || resp.dragged();
        ui.painter().rect_filled(rect, 0.0, if active { gray(0x40) } else { panel() });
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            format!("{} / 4", self.project.beats_per_bar),
            theme::font(theme::FS),
            theme::text(),
        );
        resp.on_hover_text("Time signature: drag up/down or wheel, double-click = 4/4");
    }

    pub fn send_loop(&mut self) {
        let p = &self.project;
        let c = Command::SetLoop { enabled: p.loop_enabled, start: p.loop_start, end: p.loop_end };
        self.send_now(c);
    }

    pub(super) fn file_menu(&mut self, ui: &mut egui::Ui) {
        if ui.button("New            Ctrl+N").clicked() {
            self.guarded(PendingAction::New);
            ui.close();
        }
        if ui.button("Open…          Ctrl+O").clicked() {
            self.dialog = Some(Dialog::Open(self.default_path("dawproj")));
            ui.close();
        }
        if ui.button("Save           Ctrl+S").clicked() {
            self.save();
            ui.close();
        }
        if ui.button("Save As…  Ctrl+Shift+S").clicked() {
            self.dialog = Some(Dialog::SaveAs(self.default_path("dawproj")));
            ui.close();
        }
        if ui.button("Export Audio… Ctrl+Shift+R").clicked() {
            self.dialog = Some(Dialog::Export { path: self.default_path("wav"), scene: None, bits: 24 });
            ui.close();
        }
        ui.separator();
        if ui.add_enabled(self.history.can_undo(), egui::Button::new("Undo           Ctrl+Z")).clicked() {
            self.undo();
            ui.close();
        }
        if ui.add_enabled(self.history.can_redo(), egui::Button::new("Redo     Ctrl+Shift+Z")).clicked() {
            self.redo();
            ui.close();
        }
        ui.separator();
        if ui.button("Add Audio Track Ctrl+T").clicked() {
            self.add_track(TrackKind::Audio);
            ui.close();
        }
        if ui.button("Add MIDI Track  Ctrl+Shift+T").clicked() {
            self.add_track(TrackKind::Midi);
            ui.close();
        }
        if ui.button("Group Tracks    Ctrl+G").clicked() {
            self.group_selected_tracks();
            ui.close();
        }
        if ui.button("Ungroup   Ctrl+Shift+G").clicked() {
            self.ungroup_selected();
            ui.close();
        }
        if ui.button("Add Scene").clicked() {
            self.edit(|p| {
                p.add_scene();
            });
            ui.close();
        }
        if ui.button("Settings…       Ctrl+,").clicked() {
            self.open_settings(crate::app::SettingsTab::Audio);
            ui.close();
        }
        ui.separator();
        if ui.button("About").clicked() {
            self.dialog = Some(Dialog::About);
            ui.close();
        }
        if ui.button("Quit").clicked() {
            self.guarded(PendingAction::Quit);
            ui.close();
        }
    }

    pub(super) fn view_menu(&mut self, ui: &mut egui::Ui) {
        if ui.checkbox(&mut self.show_io, "In/Out      Ctrl+Alt+I").clicked() {
            ui.close();
        }
        if ui.checkbox(&mut self.show_browser, "Browser       Ctrl+B").clicked() {
            ui.close();
        }
        let mut kb = self.kb.enabled;
        if ui.checkbox(&mut kb, "Computer MIDI Keyboard   M").clicked() {
            self.kb.enabled = kb;
            ui.close();
        }
        ui.separator();
        if ui.button("Audio Settings…").clicked() {
            self.open_settings(crate::app::SettingsTab::Audio);
            ui.close();
        }
        if ui.button("MIDI Settings…").clicked() {
            self.open_settings(crate::app::SettingsTab::Midi);
            ui.close();
        }
        if ui.button("Rescan MIDI devices").clicked() {
            self.rescan_midi();
            ui.close();
        }
    }

    /// Live status bar: info/message area, selected track, Device/Clip view switches.
    /// Segment edges from Live at 1920: 42–1560, 1570–1789, 1799–1863.
    /// Live 12 status bar (measured at 1920 px): ⓘ, message field 42..1555,
    /// ▶ + field 1565..1784, orange ▲ + track name ending at 1863, meter, ▾.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        use egui::{Align2, CornerRadius, Rect, Sense, pos2, vec2};
        let mr = ui.max_rect();
        ui.allocate_rect(mr, Sense::hover());
        let p = ui.painter().clone();
        let (x0, w) = (mr.left() - 9.0, mr.width() + 18.0);
        let rx = |x: f32| x0 + w - (1920.0 - x);
        let (y0, y1) = (mr.top(), mr.top() + 19.0);
        let seg = |a: f32, b: f32| Rect::from_min_max(pos2(a, y0), pos2(b + 1.0, y1));
        let fs = theme::font(theme::FS);
        // ⓘ (audio health on hover; red when the engine runs without a device).
        let info = &self.audio.info;
        let ok = info.null_reason.is_none();
        let ic = pos2(x0 + 18.0, y0 + 9.5);
        p.circle_filled(ic, 7.5, if ok { gray(0x46) } else { REC_RED });
        p.rect_filled(Rect::from_min_size(pos2(ic.x - 0.5, ic.y - 4.0), vec2(2.0, 2.0)), 0.0, gray(0x2a));
        p.rect_filled(Rect::from_min_size(pos2(ic.x - 0.5, ic.y - 1.0), vec2(2.0, 5.0)), 0.0, gray(0x2a));
        let input = match &info.input {
            Ok(n) => format!("In: {n}"),
            Err(e) => format!("No input ({e})"),
        };
        let midi = if self.io_map.midi_in.is_empty() && self.io_map.midi_out.is_empty() {
            self.midi.error.clone().unwrap_or_else(|| "No MIDI ports".into())
        } else {
            format!("MIDI: {}", self.io_map.midi_in.iter().chain(&self.io_map.midi_out).cloned().collect::<Vec<_>>().join(", "))
        };
        ui.interact(Rect::from_center_size(ic, vec2(15.0, 15.0)), ui.make_persistent_id("st_dot"), Sense::hover())
            .on_hover_text(format!("{}\n{input}\n{midi}", info.summary()));
        // Message field: toasts, else the insert marker / time selection (Live).
        let main = seg(x0 + 42.0, rx(1555.0));
        p.rect_filled(main, CornerRadius::same(3), panel());
        // Insert mark / time selection always; toasts and (for 4 s after a change)
        // the computer keyboard's octave and velocity follow it.
        let bpb = self.project.beats_per_bar.max(1);
        let spb = 60.0 / self.project.tempo_bpm.max(1.0);
        let pos = |b: f64| topbar::bbs(b, bpb, false).replace(' ', "");
        let len = |b: f64| topbar::bbs(b, bpb, true).replace(' ', "");
        let time = |b: f64| {
            let s = (b * spb).max(0.0);
            format!("{}:{:02}", (s / 60.0) as u64, (s % 60.0) as u64)
        };
        let mut msg = match self.time_sel {
            Some(ts) if ts.end - ts.start > 1e-9 => format!(
                "Time Selection  Start: {}  End: {}  Length: {} (Duration: {})",
                pos(ts.start),
                pos(ts.end),
                len(ts.end - ts.start),
                time(ts.end - ts.start)
            ),
            _ => format!("Insert Mark {} (Time: {})", pos(self.play_start), time(self.play_start)),
        };
        let col = theme::text();
        let recent = |at: &std::time::Instant| at.elapsed() < Duration::from_secs(4);
        if self.kb_changed.as_ref().is_some_and(recent) {
            msg = format!("{msg}    Computer MIDI Keyboard: Octave C{}  Velocity {}", self.kb.octave, self.kb.velocity);
        }
        match &self.message {
            Some((m, at)) if recent(at) => msg = format!("{msg}    {m}"),
            Some(_) => self.message = None,
            None => {}
        }
        p.with_clip_rect(main.shrink2(vec2(6.0, 0.0))).text(main.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, msg, fs.clone(), col);
        // ▶ box + field (health warnings live here).
        let play = seg(rx(1565.0), rx(1583.0));
        let fld = seg(rx(1585.0), rx(1784.0));
        p.rect_filled(play, CornerRadius { nw: 3, sw: 3, ne: 0, se: 0 }, panel());
        p.rect_filled(fld, CornerRadius { nw: 0, sw: 0, ne: 3, se: 3 }, panel());
        p.rect_filled(Rect::from_min_max(pos2(play.right(), y0), pos2(fld.left(), y1)), 0.0, theme::sep());
        let c = play.center();
        p.add(egui::Shape::convex_polygon(
            vec![pos2(c.x - 2.5, c.y - 4.0), pos2(c.x + 3.5, c.y), pos2(c.x - 2.5, c.y + 4.0)],
            gray(0x8c),
            egui::Stroke::NONE,
        ));
        let h = &self.audio.health;
        let mut warn = Vec::new();
        let xruns = h.xruns.load(Relaxed);
        if xruns > 0 {
            warn.push(format!("{xruns} dropouts"));
        }
        let errs = h.stream_errors.load(Relaxed);
        if errs > 0 {
            warn.push(format!("{errs} audio errors"));
        }
        if h.realtime_denied.load(Relaxed) {
            warn.push("no RT priority".into());
        }
        if self.dropped_commands > 0 {
            warn.push(format!("{} commands dropped", self.dropped_commands));
        }
        if !warn.is_empty() {
            p.with_clip_rect(fld.shrink2(vec2(6.0, 0.0))).text(
                fld.left_center() + vec2(6.0, 0.0),
                Align2::LEFT_CENTER,
                warn.join(" · "),
                fs.clone(),
                ACCENT,
            );
            if errs > 0 {
                let last = h.last_error.try_lock().ok().and_then(|g| g.clone()).unwrap_or_default();
                ui.interact(fld, ui.make_persistent_id("st_err"), Sense::hover()).on_hover_text(last);
            }
        }
        // Orange ▲ + selected track name (right edge 1863, width = text + 12).
        // Clicking either toggles Device / Clip view (Shift+Tab).
        let name = if self.master_selected { "Main".to_string() } else { self.project.track_label(self.selected_track) };
        let bf = theme::bold(theme::FS);
        let tw = ui.fonts_mut(|f| f.layout_no_wrap(name.clone(), bf.clone(), egui::Color32::WHITE).size().x);
        let nr = seg(rx(1863.0) - tw.ceil() - 12.0, rx(1863.0));
        let tri = seg(nr.left() - 20.0, nr.left() - 2.0);
        p.rect_filled(nr, CornerRadius { nw: 0, sw: 0, ne: 3, se: 3 }, panel());
        p.text(nr.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, &name, bf, theme::text());
        p.rect_filled(tri, CornerRadius { nw: 3, sw: 3, ne: 0, se: 0 }, theme::ORANGE);
        let c = tri.center();
        let (a, b2) = if self.detail_mode == DetailMode::Device { (-1.0, 1.0) } else { (1.0, -1.0) };
        p.add(egui::Shape::convex_polygon(
            vec![pos2(c.x, c.y + 3.0 * a), pos2(c.x + 4.0, c.y + 3.0 * b2), pos2(c.x - 4.0, c.y + 3.0 * b2)],
            gray(0x0c),
            egui::Stroke::NONE,
        ));
        let resp = ui
            .interact(tri.union(nr), ui.make_persistent_id("st_dv"), Sense::click())
            .on_hover_text("Toggle Device / Clip View (Shift+Tab)");
        if resp.clicked() {
            self.detail_mode = if self.detail_mode == DetailMode::Device { DetailMode::Clip } else { DetailMode::Device };
        }
        // Decorative meter bars and ▾ (Live's CPU / disk overload area).
        for (i, top) in [5.0, 3.0, 0.5, 3.0].into_iter().enumerate() {
            let x = rx(1881.0 + 4.0 * i as f32);
            p.rect_filled(Rect::from_min_max(pos2(x, y0 + 2.0 + top), pos2(x + 2.0, y0 + 17.0)), 0.0, gray(0x46));
        }
        let c = pos2(rx(1904.5), y0 + 8.5);
        p.add(egui::Shape::convex_polygon(
            vec![pos2(c.x - 3.0, c.y - 2.0), pos2(c.x + 3.0, c.y - 2.0), pos2(c.x, c.y + 2.0)],
            gray(0x46),
            egui::Stroke::NONE,
        ));
        // Window-corner grip (orange).
        let g = pos2(rx(1912.0), y1 + 1.0);
        p.rect_stroke(Rect::from_min_size(g, vec2(7.0, 7.0)), 0.0, egui::Stroke::new(1.0, theme::ORANGE), egui::StrokeKind::Inside);
        p.line_segment([g + vec2(2.0, 5.0), g + vec2(5.0, 2.0)], egui::Stroke::new(1.0, theme::ORANGE));
    }

    /// "Save changes" dialog (hh-save-changes, 414 × 139 at 100 %): #20 title bar
    /// (31 px, Live icon, ×), #36 body with the warning triangle, an 11 px message
    /// and three 98 × 13 pill buttons (#1e, #11 outline; the highlighted one gets
    /// a #91 ring 1 px outside). ←/→/Tab move the highlight, Enter/Space
    /// activates, Y / N / C pick directly, Esc cancels.
    fn save_changes_dialog(&mut self, ctx: &egui::Context, pending: PendingAction, keys: &[Key]) {
        use egui::{Align2, CornerRadius, Rect, Sense, Shape, Stroke, vec2};
        let focus_id = egui::Id::unique("confirm_focus");
        let mut focus: usize = ctx.data(|d| d.get_temp(focus_id)).unwrap_or(1);
        let mut choice: Option<usize> = None;
        for k in keys {
            match k {
                Key::ArrowLeft => focus = (focus + 2) % 3,
                Key::ArrowRight | Key::Tab => focus = (focus + 1) % 3,
                Key::Enter | Key::Space => choice = Some(focus),
                Key::Y => choice = Some(0),
                Key::N => choice = Some(1),
                Key::C => choice = Some(2),
                _ => {}
            }
        }
        let name =
            self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled".into());
        let size = vec2(414.0, 139.0);
        let screen = ctx.content_rect();
        let o = (screen.center() - size * 0.5).round();
        egui::Area::new(egui::Id::unique("save_changes")).order(egui::Order::Foreground).fixed_pos(o).show(ctx, |ui| {
            let (win, _) = ui.allocate_exact_size(size, Sense::click());
            let p = ui.painter();
            let at = |x: f32, y: f32| win.min + vec2(x, y);
            // Soft shadow, window, title bar.
            for i in 1..=8 {
                let a = (6 - i.min(6)) as u8 * 6;
                p.rect_filled(win.expand(i as f32), CornerRadius::same(8 + i as u8), egui::Color32::from_black_alpha(a));
            }
            p.rect_filled(win, CornerRadius::same(8), gray(0x20));
            p.rect_filled(
                Rect::from_min_max(at(1.0, 31.0), at(size.x - 1.0, size.y - 1.0)),
                CornerRadius { nw: 0, ne: 0, sw: 7, se: 7 },
                gray(0x36),
            );
            // Live icon (11 × 9 at +11,+9).
            let icon = Rect::from_min_size(at(10.0, 8.0), vec2(13.0, 11.0));
            p.rect_filled(icon, CornerRadius::same(1), gray(0x12));
            p.text(icon.center(), Align2::CENTER_CENTER, "Live", theme::bold(6.0), gray(0xda));
            // Close ×: 10 × 10 at +394,+10.
            let xr = Rect::from_min_size(at(394.0, 10.0), vec2(10.0, 10.0));
            let xs = Stroke::new(1.3, gray(0xe8));
            p.line_segment([xr.left_top(), xr.right_bottom()], xs);
            p.line_segment([xr.right_top(), xr.left_bottom()], xs);
            let close_resp = ui.interact(xr.expand(6.0), ui.make_persistent_id("sc_close"), Sense::click());
            if close_resp.clicked() {
                choice = Some(2);
            }
            // Warning triangle (#b5): hull of r = 3 circles at the inner
            // vertices gives Live's rounded corners (outer 17..71 × 47..95).
            let hull = {
                let mut pts: Vec<egui::Pos2> = Vec::new();
                for c in [at(43.0, 50.0), at(20.0, 92.0), at(68.0, 92.0)] {
                    for k in 0..24 {
                        let a = k as f32 / 24.0 * std::f32::consts::TAU;
                        pts.push(c + 3.0 * vec2(a.cos(), a.sin()));
                    }
                }
                convex_hull(pts)
            };
            p.add(Shape::convex_polygon(hull, gray(0xb5), Stroke::NONE));
            p.add(Shape::convex_polygon(vec![at(40.4, 61.0), at(44.6, 61.0), at(43.7, 78.0), at(41.3, 78.0)], gray(0x36), Stroke::NONE));
            p.circle_filled(at(42.5, 85.0), 2.5, gray(0x36));
            // Message: 11 px, baseline at +72.
            p.text(
                at(89.0, 69.5),
                Align2::LEFT_CENTER,
                format!("Save changes to \"{name}\" before closing?"),
                theme::font(12.0),
                gray(0xb5),
            );
            // Buttons.
            for (i, (label, x)) in [("Yes", 56.0), ("No", 158.0), ("Cancel", 260.0)].into_iter().enumerate() {
                let b = Rect::from_min_size(at(x, 106.0), vec2(98.0, 13.0));
                let resp = ui.interact(b, ui.make_persistent_id(("sc_btn", i)), Sense::click());
                if resp.hovered() && ui.input(|inp| inp.pointer.delta() != egui::Vec2::ZERO) {
                    focus = i;
                }
                if resp.clicked() {
                    choice = Some(i);
                }
                p.rect_filled(b, CornerRadius::same(6), gray(0x1e));
                p.rect_stroke(b, CornerRadius::same(6), Stroke::new(1.0, gray(0x11)), egui::StrokeKind::Inside);
                if i == focus {
                    p.rect_stroke(b.expand(1.0), CornerRadius::same(7), Stroke::new(1.0, gray(0x91)), egui::StrokeKind::Inside);
                }
                p.text(b.center() + vec2(1.0, 0.0), Align2::CENTER_CENTER, label, theme::font(theme::FS), gray(0xb5));
            }
        });
        ctx.data_mut(|d| d.insert_temp(focus_id, focus));
        let Some(c) = choice else { return };
        ctx.data_mut(|d| d.remove::<usize>(focus_id));
        self.dialog = None;
        match c {
            0 => {
                if self.save() {
                    self.run_pending(pending);
                }
            }
            1 => self.run_pending(pending),
            _ => {}
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.clone() else { return };
        let mut close = false;
        let mut action: Option<DialogAction> = None;
        let title = match &dialog {
            Dialog::Open(_) => "Open Project",
            Dialog::SaveAs(_) => "Save Project As",
            Dialog::Export { .. } => "Export Audio",
            Dialog::ConfirmDiscard(_) => "Unsaved Changes",
            Dialog::About => "About daw",
            Dialog::Settings(_) => "Settings",
        };
        if let Dialog::Settings(tab) = &mut dialog {
            let closed = prefs::window(self, ctx, tab);
            if closed {
                self.dialog = None;
            } else if matches!(self.dialog, Some(Dialog::Settings(_))) {
                self.dialog = Some(dialog);
            }
            return;
        }
        let keys = std::mem::take(&mut self.dialog_keys);
        if let Dialog::ConfirmDiscard(pending) = &dialog {
            let pending = pending.clone();
            self.save_changes_dialog(ctx, pending, &keys);
            return;
        }
        let scenes = self.project.scenes.len();
        egui::Window::new(title).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            match &mut dialog {
                Dialog::Open(path) | Dialog::SaveAs(path) => {
                    let is_open = matches!(self.dialog, Some(Dialog::Open(_)));
                    ui.label("File path:");
                    let r = ui.add(egui::TextEdit::singleline(path).desired_width(420.0));
                    r.request_focus();
                    if is_open {
                        // Quick picks: project files next to the current folder.
                        let dir = PathBuf::from(&*path).parent().map(PathBuf::from).unwrap_or_default();
                        if let Ok(rd) = std::fs::read_dir(&dir) {
                            let mut files: Vec<PathBuf> = rd
                                .flatten()
                                .map(|e| e.path())
                                .filter(|p| p.extension().is_some_and(|e| e == daw_project::FILE_EXTENSION))
                                .collect();
                            files.sort();
                            for f in files.into_iter().take(12) {
                                if ui.selectable_label(false, f.file_name().unwrap_or_default().to_string_lossy()).clicked() {
                                    *path = f.to_string_lossy().into_owned();
                                }
                            }
                        }
                    }
                    let enter = ui.input(|i| i.key_pressed(Key::Enter));
                    ui.horizontal(|ui| {
                        if ui.button(if is_open { "Open" } else { "Save" }).clicked() || enter {
                            let p = path.clone();
                            action = Some(if is_open {
                                Box::new(move |a: &mut DawApp| a.guarded(PendingAction::Open(p)))
                            } else {
                                Box::new(move |a: &mut DawApp| {
                                    a.save_as(PathBuf::from(p));
                                })
                            });
                            close = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::Export { path, scene, bits } => {
                    ui.label("Output WAV file:");
                    ui.add(egui::TextEdit::singleline(path).desired_width(420.0));
                    ui.horizontal(|ui| {
                        ui.label("Source:");
                        ui.radio_value(scene, None, "Arrangement");
                        egui::ComboBox::from_id_salt("exp_scene")
                            .selected_text(scene.map_or("Scene…".to_string(), |s| format!("Scene {}", s + 1)))
                            .show_ui(ui, |ui| {
                                for s in 0..scenes {
                                    ui.selectable_value(scene, Some(s), format!("Scene {}", s + 1));
                                }
                            });
                    });
                    ui.horizontal(|ui| {
                        ui.label("Format:");
                        ui.radio_value(bits, 16, "16-bit");
                        ui.radio_value(bits, 24, "24-bit");
                        ui.radio_value(bits, 32, "32-bit float");
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Export").clicked() {
                            let (p, s, b) = (PathBuf::from(&*path), *scene, *bits);
                            action = Some(Box::new(move |a: &mut DawApp| a.export(p, s, b)));
                            close = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::ConfirmDiscard(_) => {}
                Dialog::Settings(_) => {}
                Dialog::About => {
                    ui.label("daw: a small, fast Rust DAW.");
                    ui.label(format!("Engine at {} Hz", self.audio.info.sample_rate));
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                }
            }
        });
        self.dialog = if close { None } else { Some(dialog) };
        if let Some(a) = action {
            a(self);
        }
    }
}

/// -1 / 0 / +1 from this frame's mouse-wheel events (either axis: Shift turns
/// the wheel into horizontal scroll).
/// Screen area of the master header / strip this frame (presses there keep it selected).
pub fn master_zone_id() -> egui::Id {
    egui::Id::unique("master_zone")
}

pub fn add_master_zone(ui: &egui::Ui, r: egui::Rect) {
    ui.data_mut(|d| d.get_temp_mut_or_default::<Vec<egui::Rect>>(master_zone_id()).push(r));
}

fn wheel_notch(ui: &egui::Ui) -> f32 {
    let d: f32 = ui.input(|i| {
        i.events
            .iter()
            .map(|e| match e {
                egui::Event::MouseWheel { delta, .. } => delta.x + delta.y,
                _ => 0.0,
            })
            .sum()
    });
    if d == 0.0 { 0.0 } else { d.signum() }
}

/// "120" for whole tempos, "120.25" otherwise.
fn tempo_text(t: f64) -> String {
    if (t - t.round()).abs() < 1e-6 { format!("{t:.0}") } else { format!("{t:.2}") }
}

pub(super) fn quant_label(q: f64) -> String {
    match q {
        0.0 => "None".into(),
        0.25 => "1/16".into(),
        0.5 => "1/8".into(),
        1.0 => "1/4".into(),
        2.0 => "1/2".into(),
        4.0 => "1 Bar".into(),
        v => format!("{} Bars", v / 4.0),
    }
}

/// Floating label next to the cursor while dragging something from the browser.
#[allow(deprecated)]
fn drag_preview(ctx: &egui::Context) {
    let Some(item) = egui::DragAndDrop::payload::<DragItem>(ctx) else { return };
    let Some(pos) = ctx.pointer_latest_pos() else { return };
    let text = match &*item {
        DragItem::Sample { name, .. } => name.clone(),
        DragItem::SynthPreset(n) => format!("Synth: {n}"),
        DragItem::DrumKit => "Drum Rack".into(),
        DragItem::SessionClip { .. } => "Clip".into(),
    };
    egui::Area::new(egui::Id::new("drag_preview"))
        .order(egui::Order::Tooltip)
        .fixed_pos(pos + egui::vec2(12.0, 8.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(text));
        });
}

/// Instrument for a dropped browser item.
pub fn instrument_for(item: &DragItem) -> Option<InstrumentDef> {
    match item {
        DragItem::SynthPreset(n) => Some(InstrumentDef::Synth(daw_engine::SynthParams::preset(n))),
        DragItem::DrumKit => Some(InstrumentDef::factory_drums()),
        DragItem::Sample { key, .. } => Some(InstrumentDef::Sampler { sample: Some(key.clone()), params: Default::default() }),
        DragItem::SessionClip { .. } => None,
    }
}

/// Inline name editor over `rect` while `app.renaming` targets `id` (a track
/// id, or a clip id when `clip`) at this location (`in_detail` = the detail
/// panel's clip name bar). Enter or clicking elsewhere commits, Esc cancels.
pub fn rename_editor(app: &mut DawApp, ui: &mut egui::Ui, rect: egui::Rect, id: u64, clip: bool, in_detail: bool) {
    let Some(r) = app.renaming.as_mut() else { return };
    if r.id != id || r.clip != clip || (clip && r.in_detail != in_detail) {
        return;
    }
    let te_id = ui.make_persistent_id(("rename", r.id, clip));
    let resp = ui.put(rect, egui::TextEdit::singleline(&mut r.text).id(te_id).font(theme::font(11.0)).margin(egui::vec2(2.0, 1.0)));
    if r.fresh {
        r.fresh = false;
        resp.request_focus();
        // Select everything so typing replaces the name.
        if let Some(mut st) = egui::text_edit::TextEditState::load(ui.ctx(), te_id) {
            let n = r.text.chars().count();
            st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(n))));
            st.store(ui.ctx(), te_id);
        }
        return;
    }
    if resp.lost_focus() {
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            app.renaming = None;
        } else {
            app.commit_rename();
        }
    }
}

/// Convex hull (monotone chain), clockwise in screen space, for egui's
/// `convex_polygon`.
fn convex_hull(mut pts: Vec<egui::Pos2>) -> Vec<egui::Pos2> {
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    let cross = |o: egui::Pos2, a: egui::Pos2, b: egui::Pos2| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut h: Vec<egui::Pos2> = Vec::with_capacity(pts.len() * 2);
    for pass in 0..2 {
        let start = h.len();
        let it: Box<dyn Iterator<Item = &egui::Pos2>> = if pass == 0 { Box::new(pts.iter()) } else { Box::new(pts.iter().rev()) };
        for &p in it {
            while h.len() >= start + 2 && cross(h[h.len() - 2], h[h.len() - 1], p) <= 0.0 {
                h.pop();
            }
            h.push(p);
        }
        h.pop();
    }
    h
}

#[cfg(test)]
mod hull_tests {
    use super::convex_hull;
    use eframe::egui::pos2;

    #[test]
    fn hull_drops_interior_points() {
        let h = convex_hull(vec![pos2(0.0, 0.0), pos2(4.0, 0.0), pos2(2.0, 1.0), pos2(4.0, 4.0), pos2(0.0, 4.0), pos2(1.0, 2.0)]);
        assert_eq!(h.len(), 4);
        assert!(!h.contains(&pos2(2.0, 1.0)) && !h.contains(&pos2(1.0, 2.0)));
    }
}
