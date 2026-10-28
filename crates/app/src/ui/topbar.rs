//! Live 12 top bar, laid out on the 1920 px grid measured from Live (boxes
//! 17 px tall at y 15–31, 3 px apart). Left group anchors left, transport group
//! to the centre, status group to the right. Live controls the DAW doesn't have
//! are drawn greyed out and only show a tooltip.

use super::theme::*;
use super::widgets::*;
use crate::app::{DawApp, View};
use daw_engine::{Command, RecordState};
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};

const NA: &str = " (not available in this version)";

/// Box layout context: maps Live x coordinates to this window.
struct Grid {
    x0: f32,
    w: f32,
    top: f32,
}

impl Grid {
    fn l(&self, a: f32, b: f32) -> Rect {
        Rect::from_min_max(pos2(self.x0 + a, self.top + 15.0), pos2(self.x0 + b + 1.0, self.top + 32.0))
    }
    fn c(&self, a: f32, b: f32) -> Rect {
        let o = self.x0 + self.w / 2.0 - 960.0;
        Rect::from_min_max(pos2(o + a, self.top + 15.0), pos2(o + b + 1.0, self.top + 32.0))
    }
    fn r(&self, a: f32, b: f32) -> Rect {
        let o = self.x0 + self.w - 1920.0;
        Rect::from_min_max(pos2(o + a, self.top + 15.0), pos2(o + b + 1.0, self.top + 32.0))
    }
}

/// Foreground colour for a box state.
fn fg(on: bool, enabled: bool) -> Color32 {
    if on {
        Color32::from_gray(0x16)
    } else if enabled {
        text()
    } else {
        gray(0x62)
    }
}

/// A flat Live box. `on` = orange (switch on).
pub fn tb_box(ui: &mut egui::Ui, r: Rect, id: &str, on: bool, enabled: bool, tip: &str) -> egui::Response {
    let resp = ui.interact(r, ui.make_persistent_id(("tb", id)), if enabled { Sense::click_and_drag() } else { Sense::hover() });
    let fill = match (on, enabled && (resp.hovered() || resp.dragged())) {
        (true, false) => ORANGE,
        (true, true) => Color32::from_rgb(0xff, 0xbd, 0x78),
        (false, true) => gray(0x40),
        (false, false) => panel(),
    };
    ui.painter().rect_filled(r, 0.0, fill);
    if enabled { resp.on_hover_text(tip) } else { resp.on_hover_text(format!("{tip}{NA}")) }
}

fn label(ui: &egui::Ui, r: Rect, t: &str, c: Color32) {
    ui.painter().text(r.center(), Align2::CENTER_CENTER, t, font(FS), c);
}

fn tri(p: &egui::Painter, pts: [Pos2; 3], c: Color32) {
    p.add(egui::Shape::convex_polygon(pts.to_vec(), c, Stroke::NONE));
}

/// Small ▾ at the right of a chooser box.
fn arrow_down(p: &egui::Painter, r: Rect, c: Color32) {
    let a = pos2(r.right() - 7.0, r.center().y + 1.5);
    tri(p, [a + vec2(-3.0, -3.0), a + vec2(3.0, -3.0), a], c);
}

/// "3. 1. 1" (position) or "4. 0. 0" (length) in bars / beats / sixteenths.
pub fn bbs(beats: f64, bpb: u32, length: bool) -> String {
    let bpb = bpb.max(1) as f64;
    let b = beats.max(0.0) + 1e-9;
    let bars = (b / bpb).floor();
    let beat = (b - bars * bpb).floor();
    let six = ((b - bars * bpb - beat) * 4.0).floor();
    let k = if length { 0.0 } else { 1.0 };
    format!("{}. {}. {}", bars + k, beat + k, six + k)
}

/// Live draws bars / beats / sixteenths in three fixed cells, right-aligned.
fn cells(ui: &egui::Ui, r: Rect, s: &str, c: Color32) {
    let cw = (r.width() - 8.0) / 3.0;
    for (i, f) in s.split(' ').enumerate() {
        let x = r.left() + 4.0 + cw * (i as f32 + 1.0) - if i == 2 { 6.0 } else { 0.0 };
        ui.painter().text(pos2(x, r.center().y), Align2::RIGHT_CENTER, f, font(FS), c);
    }
}

/// Live position box: drag the bar / beat / sixteenth field up/down.
fn pos_box(ui: &mut egui::Ui, r: Rect, id: &str, v: f64, bpb: u32, length: bool, tip: &str) -> Option<f64> {
    let resp = tb_box(ui, r, id, false, true, tip);
    cells(ui, r, &bbs(v, bpb, length), text());
    let key = ui.make_persistent_id(("tb_acc", id));
    if resp.drag_started() {
        let field = ((resp.interact_pointer_pos()?.x - r.left()) / r.width() * 3.0).floor().clamp(0.0, 2.0) as u8;
        ui.data_mut(|d| d.insert_temp(key, (field, 0.0f32)));
    }
    if resp.dragged() {
        let (field, acc): (u8, f32) = ui.data(|d| d.get_temp(key))?;
        let acc = acc - resp.drag_delta().y;
        let steps = (acc / 6.0).trunc();
        ui.data_mut(|d| d.insert_temp(key, (field, acc - steps * 6.0)));
        if steps != 0.0 {
            let unit = [bpb as f64, 1.0, 0.25][field as usize];
            let min = if length { 0.25 } else { 0.0 };
            return Some((v + steps as f64 * unit).max(min));
        }
    }
    None
}

impl DawApp {
    pub(super) fn transport_bar(&mut self, ui: &mut egui::Ui) {
        let mr = ui.max_rect();
        let g = Grid { x0: mr.left() - 9.0, w: mr.width() + 18.0, top: mr.top() };
        ui.allocate_rect(mr, Sense::hover());
        let p = ui.painter().clone();
        let bpb = self.project.beats_per_bar;

        // ---- Left: browser toggle + main menu ----
        let br = Rect::from_min_max(pos2(g.x0 + 9.0, g.top + 15.0), pos2(g.x0 + 26.0, g.top + 32.0));
        let resp = ui.interact(br, ui.make_persistent_id("tb_browser"), Sense::click()).on_hover_text("Show/Hide Browser (Ctrl+Alt+B)");
        let c = if self.show_browser { ORANGE } else { gray(0x8a) };
        p.rect_filled(Rect::from_min_size(br.min + vec2(1.0, 2.0), vec2(6.0, 13.0)), 0.0, c);
        p.rect_filled(Rect::from_min_size(br.min + vec2(9.0, 2.0), vec2(6.0, 13.0)), 0.0, c);
        if resp.clicked() {
            self.show_browser = !self.show_browser;
        }
        let mr2 = Rect::from_min_max(pos2(g.x0 + 28.0, g.top + 15.0), pos2(g.x0 + 40.0, g.top + 32.0));
        let mresp = ui.interact(mr2, ui.make_persistent_id("tb_menu"), Sense::click()).on_hover_text("File / View menu");
        arrow_down(&p, mr2.translate(vec2(4.0, 0.0)), if mresp.hovered() { text() } else { gray(0x62) });
        egui::Popup::menu(&mresp).show(|ui| {
            ui.set_min_width(200.0);
            ui.menu_button("File", |ui| self.file_menu(ui));
            ui.menu_button("View", |ui| self.view_menu(ui));
        });

        // ---- Tempo group ----
        self.tap_box(ui, g.l(55.0, 80.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(g.l(83.0, 124.0)));
        self.tempo_box(&mut child);
        self.nudge_boxes(ui, g.l(127.0, 143.0), g.l(146.0, 162.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(g.l(165.0, 206.0)));
        self.signature_box(&mut child);
        // Metronome: ○● switch (orange = on).
        let r = g.l(209.0, 247.0);
        let on = self.project.metronome;
        if tb_box(ui, r, "metro", on, true, "Metronome").clicked() {
            self.edit(|p| p.metronome = !on);
            self.send_now(Command::SetMetronome(!on));
        }
        let f = fg(on, true);
        // Two discs with a clear ~4 px gap between them.
        let cy = r.center().y.round() + 0.5;
        p.circle_stroke(pos2(r.left().round() + 7.5, cy), 3.5, Stroke::new(1.1, f));
        p.circle_filled(pos2(r.left().round() + 19.5, cy), 4.0, f);
        arrow_down(&p, r, f);
        // Global launch quantization.
        let r = g.l(250.0, 294.0);
        let q = self.project.launch_quantize;
        let resp = tb_box(ui, r, "quant", false, true, "Global Launch Quantization");
        p.text(r.left_center() + vec2(5.0, 0.0), Align2::LEFT_CENTER, super::quant_label(q), font(FS), text());
        arrow_down(&p, r, text());
        egui::Popup::menu(&resp).show(|ui| {
            for v in [0.0, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0] {
                if ui.selectable_label(q == v, super::quant_label(v)).clicked() {
                    self.edit(|p| p.launch_quantize = v);
                    self.send_now(Command::SetLaunchQuantize(v));
                }
            }
        });
        // Scale (not implemented): purple ♭♯ switch, root and scale name.
        let r = g.l(316.0, 332.0);
        tb_box(ui, r, "scale", false, false, "Scale Mode");
        p.rect_filled(r, 0.0, Color32::from_rgb(0x5c, 0x4f, 0x7e));
        p.text(r.center(), Align2::CENTER_CENTER, "b#", font(FS_SMALL), Color32::from_gray(0x20));
        for (a, b, t) in [(335.0, 378.0, "C"), (381.0, 450.0, "Major")] {
            let r = g.l(a, b);
            tb_box(ui, r, t, false, false, "Scale");
            p.text(r.left_center() + vec2(5.0, 0.0), Align2::LEFT_CENTER, t, font(FS), fg(false, false));
            arrow_down(&p, r, fg(false, false));
        }

        // ---- Transport (centre) ----
        let r = g.c(716.0, 736.0);
        let fol = self.follow;
        if tb_box(ui, r, "follow", fol, true, "Follow: scroll the Arrangement with the playhead (Ctrl+F)").clicked() {
            self.follow = !fol;
        }
        let (y, f) = (r.center().y, fg(fol, true));
        p.line_segment([pos2(r.left() + 4.0, y), pos2(r.left() + 11.0, y)], Stroke::new(1.2, f));
        tri(&p, [pos2(r.left() + 9.0, y - 3.5), pos2(r.left() + 13.0, y), pos2(r.left() + 9.0, y + 3.5)], f);
        for k in 0..2 {
            let x = r.left() + 15.0 + k as f32 * 3.0;
            p.line_segment([pos2(x, y), pos2(x + 1.5, y)], Stroke::new(1.2, f));
        }
        let pos = self.status.position;
        let r = g.c(739.0, 820.0);
        tb_box(ui, r, "pos", false, true, "Arrangement Position");
        cells(ui, r, &format!("{}. {}. {}", pos.bar, pos.beat, pos.sixteenth), text());
        // Play / Stop / Record: unboxed icons like Live.
        let ir = |x: f32| g.c(x, x + 17.0);
        let r = ir(840.0);
        let resp = ui.interact(r, ui.make_persistent_id("tb_play"), Sense::click()).on_hover_text("Play (Space) · Shift+Space: continue");
        let pc = if self.status.playing {
            Color32::from_rgb(0x5f, 0xd2, 0x5f)
        } else if resp.hovered() {
            gray(0xd8)
        } else {
            gray(0xb0)
        };
        let c = r.center();
        tri(&p, [c + vec2(-6.0, -7.5), c + vec2(7.5, 0.0), c + vec2(-6.0, 7.5)], pc);
        if resp.clicked() {
            self.send_now(Command::SetPosition(self.play_start));
            self.send_now(Command::Play);
        }
        let r = ir(860.0);
        let resp = ui.interact(r, ui.make_persistent_id("tb_stop"), Sense::click()).on_hover_text("Stop; press again to return to start");
        p.rect_filled(Rect::from_center_size(r.center(), vec2(13.0, 13.0)), 0.0, if resp.hovered() { gray(0xd8) } else { gray(0xb0) });
        if resp.clicked() {
            self.stop();
        }
        let r = ir(880.0);
        let resp = ui.interact(r, ui.make_persistent_id("tb_rec"), Sense::click()).on_hover_text("Arrangement Record (F9)");
        let rec = matches!(self.status.record_state, RecordState::Recording | RecordState::StartPending);
        let blink = matches!(self.status.record_state, RecordState::StartPending | RecordState::StopPending)
            && (self.status.beat_position * 2.0).fract() < 0.5;
        let rc = if rec && !blink {
            Color32::from_rgb(0xff, 0x55, 0x48)
        } else if resp.hovered() {
            gray(0xd8)
        } else {
            gray(0xb0)
        };
        p.circle_filled(r.center(), 7.0, rc);
        if resp.clicked() {
            self.toggle_record();
        }
        // + (MIDI Arrangement Overdub), automation arm, back to arrangement, capture, session record.
        let r = g.c(919.0, 935.0);
        tb_box(ui, r, "overdub", false, false, "MIDI Arrangement Overdub");
        let c = r.center();
        p.line_segment([c - vec2(5.0, 0.0), c + vec2(5.0, 0.0)], Stroke::new(1.3, fg(false, false)));
        p.line_segment([c - vec2(0.0, 5.0), c + vec2(0.0, 5.0)], Stroke::new(1.3, fg(false, false)));
        let r = g.c(938.0, 954.0);
        tb_box(ui, r, "autoarm", false, false, "Automation Arm");
        let c = r.center();
        let f = fg(false, false);
        p.line_segment([c + vec2(-4.0, 3.5), c + vec2(4.0, -3.5)], Stroke::new(1.2, f));
        p.circle_stroke(c + vec2(-4.0, 3.5), 2.2, Stroke::new(1.2, f));
        p.circle_stroke(c + vec2(4.0, -3.5), 2.2, Stroke::new(1.2, f));
        let r = g.c(957.0, 973.0);
        let back = self.track_status.iter().any(|t| t.arrangement_off);
        if tb_box(ui, r, "back", back, back, "Back to Arrangement").clicked() && back {
            self.send_now(Command::BackToArrangement);
        }
        let c = r.center();
        let f = fg(back, back);
        p.line_segment([c + vec2(-5.0, 0.0), c + vec2(5.0, 0.0)], Stroke::new(1.3, f));
        tri(&p, [c + vec2(-6.0, 0.0), c + vec2(-2.0, -4.0), c + vec2(-2.0, 4.0)], f);
        let r = g.c(976.0, 992.0);
        tb_box(ui, r, "capture", false, false, "Capture MIDI");
        let sq = Rect::from_center_size(r.center(), vec2(10.0, 10.0));
        for (a, b) in [
            (sq.left_top(), sq.right_top()),
            (sq.right_top(), sq.right_bottom()),
            (sq.right_bottom(), sq.left_bottom()),
            (sq.left_bottom(), sq.left_top()),
        ] {
            p.add(egui::Shape::dashed_line(&[a, b], Stroke::new(1.0, fg(false, false)), 2.0, 2.0));
        }
        let r = g.c(995.0, 1011.0);
        tb_box(ui, r, "srec", false, false, "Session Record");
        p.circle_stroke(r.center(), 5.0, Stroke::new(1.3, fg(false, false)));
        // Loop / punch.
        let ls = self.project.loop_start;
        let len = self.project.loop_end - self.project.loop_start;
        if let Some(v) = pos_box(ui, g.c(1033.0, 1114.0), "loopstart", ls, bpb, false, "Loop / Punch-Region Start: drag a field") {
            self.edit(|p| {
                p.loop_end = v + len;
                p.loop_start = v;
            });
            self.send_loop();
        }
        let r = g.c(1117.0, 1137.0);
        tb_box(ui, r, "punchin", false, false, "Punch-In Switch");
        let (y, f) = (r.center().y, fg(false, false));
        p.line_segment([pos2(r.left() + 4.0, y - 3.0), pos2(r.left() + 9.0, y - 3.0)], Stroke::new(1.2, f));
        p.line_segment([pos2(r.left() + 9.0, y - 3.0), pos2(r.left() + 13.0, y + 3.0)], Stroke::new(1.2, f));
        p.line_segment([pos2(r.left() + 13.0, y + 3.0), pos2(r.left() + 17.0, y + 3.0)], Stroke::new(1.2, f));
        let r = g.c(1140.0, 1160.0);
        let lp = self.project.loop_enabled;
        if tb_box(ui, r, "loop", lp, true, "Loop Switch (Ctrl+L)").clicked() {
            self.edit(|p| p.loop_enabled = !lp);
            self.send_loop();
        }
        let f = fg(lp, true);
        let lr = Rect::from_center_size(r.center(), vec2(11.0, 8.0));
        p.rect_stroke(lr, 1.5, Stroke::new(1.2, f), egui::StrokeKind::Middle);
        tri(&p, [lr.center_bottom() + vec2(-3.0, 0.0), lr.center_bottom() + vec2(1.0, -3.0), lr.center_bottom() + vec2(1.0, 3.0)], f);
        let r = g.c(1163.0, 1183.0);
        tb_box(ui, r, "punchout", false, false, "Punch-Out Switch");
        let (y, f) = (r.center().y, fg(false, false));
        p.line_segment([pos2(r.left() + 4.0, y + 3.0), pos2(r.left() + 8.0, y + 3.0)], Stroke::new(1.2, f));
        p.line_segment([pos2(r.left() + 8.0, y + 3.0), pos2(r.left() + 12.0, y - 3.0)], Stroke::new(1.2, f));
        p.line_segment([pos2(r.left() + 12.0, y - 3.0), pos2(r.left() + 17.0, y - 3.0)], Stroke::new(1.2, f));
        if let Some(v) = pos_box(ui, g.c(1186.0, 1267.0), "looplen", len, bpb, true, "Loop / Punch-Region Length: drag a field") {
            self.edit(|p| p.loop_end = p.loop_start + v);
            self.send_loop();
        }

        // ---- Right: draw, keyboard, Key/MIDI map, sample rate, CPU, output ----
        let r = g.r(1604.0, 1620.0);
        tb_box(ui, r, "draw", false, false, "Draw Mode (B)");
        let c = r.center();
        p.line_segment([c + vec2(-4.0, 4.0), c + vec2(4.0, -4.0)], Stroke::new(2.0, fg(false, false)));
        let r = g.r(1623.0, 1639.0);
        let kb = self.kb.enabled;
        if tb_box(ui, r, "kb", kb, true, "Computer MIDI Keyboard (M)\nA W S E D F T G Y H U J K = notes, Z/X octave, C/V velocity")
            .clicked()
        {
            self.kb.enabled = !kb;
        }
        let f = fg(kb, true);
        let kr = Rect::from_center_size(r.center(), vec2(11.0, 11.0));
        p.rect_stroke(kr, 0.0, Stroke::new(1.0, f), egui::StrokeKind::Inside);
        for i in 0..3 {
            p.rect_filled(Rect::from_min_size(kr.min + vec2(2.0 + i as f32 * 3.0, 0.0), vec2(1.5, 6.5)), 0.0, f);
        }
        let r = g.r(1642.0, 1667.0);
        tb_box(ui, r, "keymap", false, false, "Key Map Mode (Ctrl+K)");
        label(ui, r, "Key", fg(false, false));
        // MIDI track in/out activity (small box), MIDI map (greyed).
        let r = g.r(1670.0, 1677.0);
        p.rect_filled(r, 0.0, panel());
        let act = &self.midi.activity;
        for (i, age) in [act.age(&act.track_in), act.age(&act.track_out)].into_iter().enumerate() {
            let led = Rect::from_min_size(pos2(r.left() + 2.0, r.top() + 2.0 + i as f32 * 7.0), vec2(4.0, 6.0));
            let lvl = super::io::activity_level(age);
            p.rect_filled(led, 0.0, gray(0x24).lerp_to_gamma(Color32::from_rgb(0xff, 0xc8, 0x3c), lvl));
        }
        ui.interact(r, ui.make_persistent_id("tb_midiled"), Sense::hover()).on_hover_text("MIDI Track input (top) / output (bottom)");
        if act.age(&act.track_in) < 0.5 || act.age(&act.track_out) < 0.5 {
            ui.ctx().request_repaint();
        }
        let r = g.r(1680.0, 1707.0);
        tb_box(ui, r, "midimap", false, false, "MIDI Map Mode (Ctrl+M)");
        label(ui, r, "MIDI", fg(false, false));
        let r = g.r(1710.0, 1757.0);
        let sr = self.audio_info_rate();
        if tb_box(ui, r, "sr", false, true, "Sample rate: click for Audio settings").clicked() {
            self.open_settings(crate::app::SettingsTab::Audio);
        }
        label(ui, r, &format!("{:.1} kHz", sr as f32 / 1000.0), text());
        let r = g.r(1762.0, 1815.0);
        tb_box(ui, r, "cpu", false, true, "CPU load of the audio thread");
        p.rect_filled(Rect::from_min_size(r.min - vec2(2.0, 0.0), vec2(1.0, r.height())), 0.0, ORANGE);
        p.text(r.center() - vec2(4.0, 0.0), Align2::CENTER_CENTER, format!("{:.0} %", self.cpu_display * 100.0), font(FS), text());
        arrow_down(&p, r, text());
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(g.r(1818.0, 1845.0)));
        self.output_box(&mut child);
        // Master output meter in the 8 px box.
        let r = g.r(1848.0, 1855.0);
        p.rect_filled(r, 0.0, panel());
        let resp = ui.interact(r, ui.make_persistent_id("tb_meter"), Sense::click()).on_hover_text("Output level (click to reset clip)");
        for (i, l) in self.meter.iter().enumerate() {
            let db = gain_to_db_floor(*l);
            let h = ((db - METER_FLOOR_DB) / -METER_FLOOR_DB).clamp(0.0, 1.0) * (r.height() - 2.0);
            let x = r.left() + 1.0 + i as f32 * 3.0;
            let col = if self.clipped { Color32::from_rgb(0xff, 0x40, 0x30) } else { Color32::from_rgb(0x6d, 0xd8, 0x5e) };
            p.rect_filled(Rect::from_min_max(pos2(x, r.bottom() - 1.0 - h), pos2(x + 2.0, r.bottom() - 1.0)), 0.0, col);
        }
        if resp.clicked() {
            self.clipped = false;
        }
        // View switches: Session (|||) and Arrangement (≡); the active one is orange.
        let sv = g.r(1872.0, 1886.0);
        let av = g.r(1895.0, 1908.0);
        for (r, v, tip) in [(sv, View::Session, "Session View (Tab)"), (av, View::Arrangement, "Arrangement View (Tab)")] {
            let resp = ui.interact(r, ui.make_persistent_id(("tb_view", tip)), Sense::click()).on_hover_text(tip);
            let c = if self.view == v {
                ORANGE
            } else if resp.hovered() {
                gray(0xb0)
            } else {
                gray(0x70)
            };
            for i in 0..3 {
                let o = i as f32 * 4.5;
                let rr = if v == View::Session {
                    Rect::from_min_size(pos2(r.left() + 2.0 + o, r.top() + 2.0), vec2(2.0, 13.0))
                } else {
                    Rect::from_min_size(pos2(r.left(), r.top() + 3.0 + o), vec2(13.0, 2.0))
                };
                p.rect_filled(rr, 0.0, c);
            }
            if resp.clicked() {
                self.view = v;
            }
        }
    }

    fn audio_info_rate(&self) -> u32 {
        self.cache.sample_rate()
    }

    /// Tap Tempo: 4+ taps set the tempo; a 2 s pause restarts.
    fn tap_box(&mut self, ui: &mut egui::Ui, r: Rect) {
        let resp = tb_box(ui, r, "tap", false, true, "Tap Tempo: tap 4+ times on the beat");
        let flash = resp.is_pointer_button_down_on();
        if flash {
            ui.painter().rect_filled(r, 0.0, ORANGE);
        }
        label(ui, r, "Tap", fg(flash, true));
        let tap_id = egui::Id::unique("tap_tempo");
        let now = ui.input(|i| i.time);
        if resp.drag_started() || resp.clicked() && !resp.drag_stopped() {
            let mut taps: Vec<f64> = ui.data(|d| d.get_temp(tap_id)).unwrap_or_default();
            if taps.last().is_some_and(|&l| now - l < 0.05) {
                return;
            }
            if taps.last().is_some_and(|&l| now - l > 2.0) {
                taps.clear();
            }
            taps.push(now);
            if taps.len() > 8 {
                taps.remove(0);
            }
            if taps.len() >= 4 {
                let iv = (taps[taps.len() - 1] - taps[0]) / (taps.len() - 1) as f64;
                let bpm = ((60.0 / iv).clamp(20.0, 999.0) * 100.0).round() / 100.0;
                self.edit(|p| p.tempo_bpm = bpm);
                self.send_now(Command::SetTempo(bpm));
            }
            ui.data_mut(|d| d.insert_temp(tap_id, taps));
        }
    }

    /// Phase Nudge down/up (rate offset only while held).
    fn nudge_boxes(&mut self, ui: &mut egui::Ui, down: Rect, up: Rect) {
        let held_id = egui::Id::unique("nudge_held");
        let held: f64 = ui.data(|d| d.get_temp(held_id)).unwrap_or(1.0);
        let mut want = 1.0;
        for (r, k, id, tip) in [(down, 0.95, "nudge_dn", "Phase Nudge Down (hold)"), (up, 1.05, "nudge_up", "Phase Nudge Up (hold)")] {
            let resp = tb_box(ui, r, id, false, true, tip);
            let on = resp.is_pointer_button_down_on();
            if on {
                want = k;
                ui.painter().rect_filled(r, 0.0, ORANGE);
            }
            let f = fg(on, true);
            // |||| with the gap moved left (down) or right (up), like Live.
            for i in 0..4 {
                let gap = if k < 1.0 { (i >= 1) as i32 as f32 * 1.5 } else { (i >= 3) as i32 as f32 * 1.5 };
                let x = r.left() + 3.5 + i as f32 * 2.5 + gap;
                ui.painter().rect_filled(Rect::from_min_size(pos2(x, r.top() + 3.0), vec2(1.2, 11.0)), 0.0, f);
            }
        }
        if want != held {
            ui.data_mut(|d| d.insert_temp(held_id, want));
            self.send_now(Command::Nudge(want));
        }
    }
}

fn gain_to_db_floor(g: f32) -> f32 {
    if g <= 1e-6 { METER_FLOOR_DB } else { (20.0 * g.log10()).max(METER_FLOOR_DB) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn bars_beats_sixteenths() {
        assert_eq!(super::bbs(8.0, 4, false), "3. 1. 1");
        assert_eq!(super::bbs(16.0, 4, true), "4. 0. 0");
        assert_eq!(super::bbs(5.75, 4, false), "2. 2. 4");
        assert_eq!(super::bbs(1.5, 3, true), "0. 1. 2");
    }
}
