//! GUI root: layout, transport bar, shortcuts, dialogs. Views live in submodules.

mod arrangement;
mod browser;
mod detail;
mod io;
mod session;
pub mod widgets;

use crate::app::{DawApp, DetailMode, Dialog, DragItem, PendingAction, View};
use daw_engine::{Command, RecordState};
use daw_project::{InstrumentDef, TrackKind, db_to_gain};
use eframe::egui::{self, Color32, Key, Modifiers};
use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;
use widgets::*;

/// Deferred work from a dialog button (runs after the dialog closes).
type DialogAction = Box<dyn FnOnce(&mut DawApp)>;

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
        self.begin_frame();
        self.handle_close(&ctx);
        self.shortcuts(&ctx);

        egui::Panel::top("transport")
            .frame(egui::Frame::NONE.fill(PANEL_BG).inner_margin(egui::Margin::symmetric(8, 6)))
            .show(ui, |ui| self.transport_bar(ui));
        egui::Panel::bottom("status")
            .frame(egui::Frame::NONE.fill(PANEL_BG).inner_margin(egui::Margin::symmetric(8, 3)))
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
        let detail_rect = egui::Panel::bottom("detail")
            .resizable(true)
            .default_size(250.0)
            .size_range(120.0..=600.0)
            .frame(egui::Frame::NONE.fill(BG_DARK).inner_margin(egui::Margin::same(4)))
            .show(ui, |ui| detail::detail_panel(self, ui))
            .response
            .rect;
        // Keys like Del / Ctrl+D / 0 act on whatever was clicked last: notes or the main view.
        if let Some(p) = ctx.input(|i| if i.pointer.any_pressed() { i.pointer.press_origin() } else { None }) {
            self.focus_detail = detail_rect.contains(p);
        }
        if self.show_browser {
            egui::Panel::left("browser")
                .resizable(true)
                .default_size(190.0)
                .size_range(140.0..=400.0)
                .frame(egui::Frame::NONE.fill(PANEL_BG).inner_margin(egui::Margin::same(6)))
                .show(ui, |ui| browser::browser(self, ui));
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(BG_DARK)).show(ui, |ui| match self.view {
            View::Session => session::session_view(self, ui),
            View::Arrangement => arrangement::arrangement_view(self, ui),
        });

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
        if cmd(Key::J) {
            self.consolidate_selection();
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

    fn transport_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            ui.menu_button("File", |ui| self.file_menu(ui));
            ui.menu_button("View", |ui| self.view_menu(ui));

            self.tempo_box(ui);
            self.signature_box(ui);
            if icon_button(ui, Icon::Metronome, self.project.metronome, "Metronome").clicked() {
                let on = !self.project.metronome;
                self.edit(|p| p.metronome = on);
                self.send_now(Command::SetMetronome(on));
            }
            let q = self.project.launch_quantize;
            egui::ComboBox::from_id_salt("quant")
                .width(64.0)
                .selected_text(quant_label(q))
                .show_ui(ui, |ui| {
                    for v in [0.0, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0] {
                        if ui.selectable_label(q == v, quant_label(v)).clicked() {
                            self.edit(|p| p.launch_quantize = v);
                            self.send_now(Command::SetLaunchQuantize(v));
                        }
                    }
                })
                .response
                .on_hover_text("Global launch quantization");

            ui.add_space(12.0);
            let pos = self.status.position;
            ui.label(
                egui::RichText::new(format!("{:>3}.{}.{}", pos.bar, pos.beat, pos.sixteenth))
                    .monospace()
                    .size(17.0)
                    .color(if self.status.playing { Color32::WHITE } else { Color32::LIGHT_GRAY }),
            );
            if icon_button(ui, Icon::Play, self.status.playing, "Play from the start point (Space) · Shift+Space: continue").clicked() {
                // Live: Play while playing restarts from the start point.
                self.send_now(Command::SetPosition(self.play_start));
                self.send_now(Command::Play);
            }
            if icon_button(ui, Icon::Stop, false, "Stop; press again to return to start").clicked() {
                self.stop();
            }
            let rec_active = matches!(self.status.record_state, RecordState::Recording | RecordState::StartPending);
            let blink = matches!(self.status.record_state, RecordState::StartPending | RecordState::StopPending)
                && (self.status.beat_position * 2.0).fract() < 0.5;
            if icon_button(ui, Icon::Record, rec_active && !blink, "Record (F9): arrangement recording into armed tracks").clicked() {
                self.toggle_record();
            }
            if icon_button(ui, Icon::Loop, self.project.loop_enabled, "Arrangement loop").clicked() {
                let on = !self.project.loop_enabled;
                self.edit(|p| p.loop_enabled = on);
                self.send_loop();
            }
            if self.track_status.iter().any(|t| t.arrangement_off) {
                let b = ui
                    .add(egui::Button::new(egui::RichText::new("Back to Arrangement").color(Color32::BLACK)).fill(ACCENT))
                    .on_hover_text("Session clips override the arrangement on some tracks");
                if b.clicked() {
                    self.send_now(Command::BackToArrangement);
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(format!("CPU {:>3.0}%", self.cpu_display * 100.0)).monospace());
                if stereo_meter(ui, self.meter, self.clipped, egui::vec2(120.0, 18.0)).clicked() {
                    self.clipped = false;
                }
                let mut db = self.project.master_gain_db;
                let r = ui
                    .add(egui::DragValue::new(&mut db).range(-60.0..=6.0).speed(0.2).fixed_decimals(1).suffix(" dB"))
                    .on_hover_text("Master volume");
                if r.drag_started() {
                    self.begin_gesture();
                }
                if r.changed() {
                    self.project.master_gain_db = db;
                    self.send_now(Command::SetMasterGain(db_to_gain(db)));
                }
                ui.label("Master");
                ui.separator();
                if icon_button(ui, Icon::Arrangement, self.view == View::Arrangement, "Arrangement view (Tab)").clicked() {
                    self.view = View::Arrangement;
                }
                if icon_button(ui, Icon::Session, self.view == View::Session, "Session view (Tab)").clicked() {
                    self.view = View::Session;
                }
                let kb = icon_button(
                    ui,
                    Icon::Keyboard,
                    self.kb.enabled,
                    "Computer MIDI keyboard (M)\nA W S E D F T G Y H U J K = notes, Z/X octave, C/V velocity",
                );
                if kb.clicked() {
                    self.kb.enabled = !self.kb.enabled;
                }
                io::midi_leds(self, ui);
            });
        });
    }

    /// BPM: drag up/down (or wheel) in whole BPM; Shift = 0.01 steps (the only
    /// way to get decimals besides typing). The cursor is hidden and parked while
    /// dragging so it never hits the screen edge. Double-click to type a value.
    fn tempo_box(&mut self, ui: &mut egui::Ui) {
        let id = ui.make_persistent_id("tempo_box");
        let edit_id = id.with("editing");
        let acc_id = id.with("acc");
        let mut editing: Option<String> = ui.data(|d| d.get_temp(edit_id));
        let size = egui::vec2(84.0, 20.0);
        if let Some(text) = editing.as_mut() {
            let r = ui.add_sized(size, egui::TextEdit::singleline(text).id(id.with("te")));
            r.request_focus();
            if r.lost_focus() {
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
        ui.painter().rect_filled(rect, egui::CornerRadius::same(2), if active { Color32::from_gray(60) } else { Color32::from_gray(48) });
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            format!("{} BPM", tempo_text(self.project.tempo_bpm)),
            egui::FontId::proportional(12.0),
            Color32::WHITE,
        );
        resp.on_hover_text("Tempo: drag up/down or wheel (Shift = 0.01 steps), double-click to type");
    }

    /// Time signature (N/4): drag up/down or wheel; double-click resets to 4/4.
    fn signature_box(&mut self, ui: &mut egui::Ui) {
        let id = ui.make_persistent_id("sig_box");
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(44.0, 20.0), egui::Sense::click_and_drag());
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
        ui.painter().rect_filled(rect, egui::CornerRadius::same(2), if active { Color32::from_gray(60) } else { Color32::from_gray(48) });
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            format!("{} / 4", self.project.beats_per_bar),
            egui::FontId::proportional(12.0),
            Color32::WHITE,
        );
        resp.on_hover_text("Time signature: drag up/down or wheel, double-click = 4/4");
    }

    pub fn send_loop(&mut self) {
        let p = &self.project;
        let c = Command::SetLoop { enabled: p.loop_enabled, start: p.loop_start, end: p.loop_end };
        self.send_now(c);
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) {
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

    fn view_menu(&mut self, ui: &mut egui::Ui) {
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

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let info = &self.audio.info;
            let ok = info.null_reason.is_none();
            let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 4.0, if ok { PLAY_GREEN } else { REC_RED });
            ui.label(egui::RichText::new(info.summary()).small());
            let input = match &info.input {
                Ok(n) => format!("In: {n}"),
                Err(e) => format!("No input ({e})"),
            };
            ui.label(egui::RichText::new(input).small().color(Color32::GRAY));
            let midi = if self.io_map.midi_in.is_empty() && self.io_map.midi_out.is_empty() {
                self.midi.error.clone().unwrap_or_else(|| "No MIDI ports".into())
            } else {
                format!("MIDI: {}", self.io_map.midi_in.iter().chain(&self.io_map.midi_out).cloned().collect::<Vec<_>>().join(", "))
            };
            ui.label(egui::RichText::new(midi).small().color(Color32::GRAY));
            if self.kb.enabled {
                ui.label(egui::RichText::new(format!("Keys: C{} vel {}", self.kb.octave, self.kb.velocity)).small().color(ACCENT));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let h = &self.audio.health;
                let xruns = h.xruns.load(Relaxed);
                if xruns > 0 {
                    ui.colored_label(ACCENT, format!("{xruns} dropouts"));
                }
                let errs = h.stream_errors.load(Relaxed);
                if errs > 0 {
                    let last = h.last_error.try_lock().ok().and_then(|g| g.clone()).unwrap_or_default();
                    ui.colored_label(Color32::LIGHT_RED, format!("{errs} audio errors")).on_hover_text(last);
                }
                if h.realtime_denied.load(Relaxed) {
                    ui.colored_label(ACCENT, "no RT priority");
                }
                if self.dropped_commands > 0 {
                    ui.colored_label(Color32::LIGHT_RED, format!("{} commands dropped", self.dropped_commands));
                }
                if let Some((msg, at)) = &self.message {
                    if at.elapsed() < Duration::from_secs(4) {
                        ui.label(egui::RichText::new(msg).color(Color32::WHITE));
                    } else {
                        self.message = None;
                    }
                }
            });
        });
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
                Dialog::Settings(tab) => {
                    if io::settings_dialog(self, ui, tab) {
                        close = true;
                    }
                }
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

fn quant_label(q: f64) -> String {
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
        ui.put(rect, egui::TextEdit::singleline(&mut r.text).id(te_id).font(egui::FontId::proportional(12.0)).margin(egui::vec2(2.0, 1.0)));
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
