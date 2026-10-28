//! GUI root: layout, transport bar, shortcuts, dialogs. Views live in submodules.

pub(crate) mod arrangement;
mod browser;
pub mod clip;
mod detail;
mod invert;
mod io;
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
const DEVICE_VIEW_H: f32 = 240.0;

impl eframe::App for DawApp {
    /// Take plain (unmodified) key presses before egui sees them, unless a text
    /// field is focused or a dialog is open: Space/Tab/Enter must drive the DAW,
    /// not egui's focus navigation, and piano keys must not trigger widgets.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if matches!(self.dialog, Some(Dialog::ConfirmDiscard(_))) {
            // Keyboard-driven dialog: arrows / Enter / S D C (see `dialogs`), not egui focus.
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
            .exact_size(28.0)
            .resizable(false)
            .frame(egui::Frame::NONE.fill(win_bg()).inner_margin(m(9, 9, 0, 9)))
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
            .frame(egui::Frame::NONE.fill(panel()).outer_margin(m(9, 9, 9, 0)))
            .show(ui, |ui| detail::detail_panel(self, ui))
            .response
            .rect;
        let detail_inner = detail_rect - m(9, 9, 9, 0);
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
            // Plain keys drive the dialog (arrows / Enter / S D C in "Unsaved Changes").
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
            self.insert_midi_clip_in_selection();
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
        if cmd(Key::D) && !self.duplicate_selected_notes() && !(self.view == View::Arrangement && self.duplicate_arrangement_selection()) {
            self.duplicate_selected_clip();
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
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let mr = ui.max_rect();
        ui.allocate_rect(mr, egui::Sense::hover());
        let p = ui.painter().clone();
        let (x0, w) = (mr.left() - 9.0, mr.width() + 18.0);
        let rx = |x: f32| x0 + w - (1920.0 - x);
        let seg = |a: f32, b: f32| egui::Rect::from_min_max(egui::pos2(a, mr.top()), egui::pos2(b + 1.0, mr.bottom()));
        // Audio health dot (details on hover).
        let info = &self.audio.info;
        let ok = info.null_reason.is_none();
        let dot = seg(x0 + 12.0, x0 + 30.0);
        p.circle_filled(dot.center(), 4.0, if ok { PLAY_GREEN } else { REC_RED });
        let input = match &info.input {
            Ok(n) => format!("In: {n}"),
            Err(e) => format!("No input ({e})"),
        };
        let midi = if self.io_map.midi_in.is_empty() && self.io_map.midi_out.is_empty() {
            self.midi.error.clone().unwrap_or_else(|| "No MIDI ports".into())
        } else {
            format!("MIDI: {}", self.io_map.midi_in.iter().chain(&self.io_map.midi_out).cloned().collect::<Vec<_>>().join(", "))
        };
        ui.interact(dot, ui.make_persistent_id("st_dot"), egui::Sense::hover())
            .on_hover_text(format!("{}\n{input}\n{midi}", info.summary()));
        // Message area.
        let main = seg(x0 + 42.0, rx(1560.0));
        p.rect_filled(main, 0.0, panel());
        let mut msg = String::new();
        let mut col = theme::text();
        if let Some((m, at)) = &self.message {
            if at.elapsed() < Duration::from_secs(4) {
                msg = m.clone();
                col = gray(0xe6);
            } else {
                self.message = None;
            }
        }
        if msg.is_empty() && self.kb.enabled {
            msg = format!("Computer MIDI Keyboard: octave C{}, velocity {}", self.kb.octave, self.kb.velocity);
        }
        p.text(main.left_center() + egui::vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, msg, theme::font(theme::FS), col);
        // Health warnings at the right of the message area.
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
            let r = p.text(
                main.right_center() - egui::vec2(6.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                warn.join(" · "),
                theme::font(theme::FS),
                ACCENT,
            );
            if errs > 0 {
                let last = h.last_error.try_lock().ok().and_then(|g| g.clone()).unwrap_or_default();
                ui.interact(r, ui.make_persistent_id("st_err"), egui::Sense::hover()).on_hover_text(last);
            }
        }
        // Selected track.
        let tr = seg(rx(1570.0), rx(1789.0));
        p.rect_filled(tr, 0.0, panel());
        let name = if self.master_selected {
            "Main".to_string()
        } else {
            self.project.tracks.get(self.selected_track).map(|t| t.name.clone()).unwrap_or_default()
        };
        p.text(tr.right_center() - egui::vec2(6.0, 0.0), egui::Align2::RIGHT_CENTER, name, theme::font(theme::FS), theme::text());
        // Device / Clip view switches (orange = shown).
        for (i, (mode, tip)) in
            [(DetailMode::Device, "Device View (Ctrl+Alt+L)"), (DetailMode::Clip, "Clip View (Ctrl+Alt+L)")].into_iter().enumerate()
        {
            let r = seg(rx(1799.0 + i as f32 * 12.0), rx(1805.0 + i as f32 * 12.0));
            let r = egui::Rect::from_center_size(r.center(), egui::vec2(9.0, 12.0));
            let on = self.detail_mode == mode;
            let resp = ui.interact(r.expand(2.0), ui.make_persistent_id(("st_dv", i)), egui::Sense::click()).on_hover_text(tip);
            p.rect_filled(
                r,
                0.0,
                if on {
                    theme::ORANGE
                } else if resp.hovered() {
                    gray(0x80)
                } else {
                    gray(0x55)
                },
            );
            if resp.clicked() {
                self.detail_mode = mode;
            }
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
        let scenes = self.project.scenes.len();
        let keys = std::mem::take(&mut self.dialog_keys);
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
                Dialog::ConfirmDiscard(pending) => {
                    ui.label("The project has unsaved changes.");
                    // ←/→ (Tab) move the highlight, Enter/Space activates; S / D / C directly.
                    let focus_id = egui::Id::unique("confirm_focus");
                    let mut focus: usize = ui.data(|d| d.get_temp(focus_id)).unwrap_or(0);
                    let mut choice: Option<usize> = None;
                    for k in keys.iter() {
                        match k {
                            Key::ArrowLeft => focus = (focus + 2) % 3,
                            Key::ArrowRight | Key::Tab => focus = (focus + 1) % 3,
                            Key::Enter | Key::Space => choice = Some(focus),
                            Key::S => choice = Some(0),
                            Key::D => choice = Some(1),
                            Key::C => choice = Some(2),
                            _ => {}
                        }
                    }
                    ui.horizontal(|ui| {
                        for (i, label) in ["Save (S)", "Don't Save (D)", "Cancel (C)"].into_iter().enumerate() {
                            let mut b = egui::Button::new(label).min_size(egui::vec2(96.0, 24.0));
                            if i == focus {
                                b = b.stroke(egui::Stroke::new(2.0, ACCENT));
                            }
                            let r = ui.add(b);
                            if r.hovered() && ui.input(|inp| inp.pointer.delta() != egui::Vec2::ZERO) {
                                focus = i;
                            }
                            if r.clicked() {
                                choice = Some(i);
                            }
                        }
                    });
                    ui.data_mut(|d| d.insert_temp(focus_id, focus));
                    if let Some(c) = choice {
                        let pa = pending.clone();
                        action = match c {
                            0 => Some(Box::new(move |a: &mut DawApp| {
                                if a.save() {
                                    a.run_pending(pa);
                                }
                            })),
                            1 => Some(Box::new(move |a: &mut DawApp| a.run_pending(pa))),
                            _ => None,
                        };
                        close = true;
                        ui.data_mut(|d| d.remove::<usize>(focus_id));
                    }
                }
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
    let resp =
        ui.put(rect, egui::TextEdit::singleline(&mut r.text).id(te_id).font(egui::FontId::proportional(11.0)).margin(egui::vec2(2.0, 1.0)));
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
