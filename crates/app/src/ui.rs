//! The GUI. Runs on the main thread; talks to the engine only through
//! `EngineHandle` (commands out, snapshots in). Nothing here can block audio.

use crate::audio::AudioSystem;
use daw_engine::{Command, EngineHandle, StatusSnapshot};
use daw_project::{Project, db_to_gain, gain_to_db};
use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2};
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

const ACCENT: Color32 = Color32::from_rgb(255, 160, 40); // Ableton-ish orange
const PLAY_GREEN: Color32 = Color32::from_rgb(90, 200, 110);
const PANEL_BG: Color32 = Color32::from_gray(38);
const METER_FLOOR_DB: f32 = -60.0;
/// Peak meter fall-back speed.
const METER_FALL_DB_PER_SEC: f32 = 24.0;
const FRAME_INTERVAL: Duration = Duration::from_millis(16);

pub struct DawApp {
    engine: EngineHandle,
    audio: AudioSystem,
    project: Project,
    status: StatusSnapshot,
    /// Displayed meter levels (linear), with fall-back ballistics.
    meter: [f32; 2],
    /// Latched until clicked: did the output ever reach 0 dBFS?
    clipped: bool,
    cpu_display: f32,
    last_frame: Instant,
    dropped_commands: u32,
}

impl DawApp {
    pub fn new(cc: &eframe::CreationContext<'_>, engine: EngineHandle, audio: AudioSystem) -> Self {
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        let project = Project::default();
        let status = engine.snapshot();
        let mut app = Self {
            engine,
            audio,
            project,
            status,
            meter: [0.0; 2],
            clipped: false,
            cpu_display: 0.0,
            last_frame: Instant::now(),
            dropped_commands: 0,
        };
        app.sync_project_to_engine();
        app
    }

    fn send(&mut self, cmd: Command) {
        if self.engine.send(cmd).is_err() {
            self.dropped_commands += 1;
        }
    }

    fn sync_project_to_engine(&mut self) {
        let p = self.project.clone();
        self.send(Command::SetTempo(p.tempo_bpm));
        self.send(Command::SetBeatsPerBar(p.beats_per_bar));
        self.send(Command::SetMetronome(p.metronome));
        self.send(Command::SetMasterGain(p.master_gain_linear()));
    }

    fn toggle_play(&mut self) {
        self.send(if self.status.playing { Command::Stop } else { Command::Play });
    }

    /// Ableton behaviour: Stop while playing pauses; Stop while stopped rewinds.
    fn stop(&mut self) {
        self.send(if self.status.playing { Command::Stop } else { Command::Rewind });
    }

    fn update_meters(&mut self, dt: f32) {
        let fall = 10f32.powf(-METER_FALL_DB_PER_SEC * dt / 20.0);
        for ch in 0..2 {
            let peak = self.status.peak[ch];
            self.meter[ch] = peak.max(self.meter[ch] * fall);
            if self.meter[ch] < 1.0e-4 {
                self.meter[ch] = 0.0;
            }
            if peak >= 1.0 {
                self.clipped = true;
            }
        }
        // Smooth CPU display a little so it's readable.
        self.cpu_display += (self.status.cpu_load - self.cpu_display) * (dt * 4.0).min(1.0);
    }

    fn transport_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            // --- Tempo & time signature (left, like Live) ---
            let tempo = ui.add(
                egui::DragValue::new(&mut self.project.tempo_bpm)
                    .range(20.0..=999.0)
                    .speed(0.1)
                    .fixed_decimals(2)
                    .suffix(" BPM"),
            );
            if tempo.changed() {
                self.send(Command::SetTempo(self.project.tempo_bpm));
            }
            tempo.on_hover_text("Tempo — drag or double-click to type");

            let sig = ui.add(egui::DragValue::new(&mut self.project.beats_per_bar).range(1..=32).suffix(" / 4"));
            if sig.changed() {
                self.send(Command::SetBeatsPerBar(self.project.beats_per_bar));
            }
            sig.on_hover_text("Beats per bar");

            if icon_button(ui, Icon::Metronome, self.project.metronome, "Metronome").clicked() {
                self.project.metronome = !self.project.metronome;
                self.send(Command::SetMetronome(self.project.metronome));
            }

            ui.add_space(16.0);

            // --- Position + transport buttons (centre) ---
            ui.label(
                egui::RichText::new(format!("{:>4}", self.status.position))
                    .monospace()
                    .size(18.0)
                    .color(if self.status.playing { Color32::WHITE } else { Color32::LIGHT_GRAY }),
            )
            .on_hover_text("Bar . Beat . Sixteenth");

            if icon_button(ui, Icon::Play, self.status.playing, "Play (Space)").clicked() {
                if self.status.playing {
                    // Live restarts from the beginning when Play is pressed while playing.
                    self.send(Command::Rewind);
                } else {
                    self.send(Command::Play);
                }
            }
            if icon_button(ui, Icon::Stop, false, "Stop — press again to return to start").clicked() {
                self.stop();
            }

            // --- Master, meter, CPU (right) ---
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(format!("CPU {:>3.0}%", self.cpu_display * 100.0)).monospace())
                    .on_hover_text("Share of each audio buffer's time budget the engine used");

                let meter = stereo_meter(ui, self.meter, self.clipped);
                if meter.clicked() {
                    self.clipped = false;
                }

                let gain = ui.add(
                    egui::Slider::new(&mut self.project.master_gain_db, METER_FLOOR_DB..=6.0)
                        .suffix(" dB")
                        .fixed_decimals(1)
                        .custom_formatter(|v, _| if v <= -60.0 { "-inf".into() } else { format!("{v:.1}") }),
                );
                if gain.double_clicked() {
                    self.project.master_gain_db = 0.0;
                }
                if gain.changed() || gain.double_clicked() {
                    self.send(Command::SetMasterGain(db_to_gain(self.project.master_gain_db)));
                }
                gain.on_hover_text("Master volume (double-click: 0 dB)");
                ui.label("Master");
            });
        });
    }

    fn central(&mut self, ui: &mut egui::Ui) {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            beat_lights(ui, &self.status);
            ui.add_space(28.0);
            ui.label(egui::RichText::new("Session View").size(22.0).strong());
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("Clip slots, scenes and tracks arrive in Phase 4.\nPress Space to start the metronome.")
                    .color(Color32::GRAY),
            );
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let h = &self.audio.health;
        ui.horizontal(|ui| {
            let (dot, text) = match &self.audio.info.null_reason {
                Some(_) => (Color32::from_rgb(200, 80, 80), self.audio.info.summary()),
                None => (PLAY_GREEN, self.audio.info.summary()),
            };
            let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 4.0, dot);
            ui.label(egui::RichText::new(text).small());

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let xruns = h.xruns.load(Relaxed);
                let errors = h.stream_errors.load(Relaxed);
                if xruns > 0 {
                    ui.colored_label(ACCENT, format!("{xruns} dropouts"))
                        .on_hover_text("Buffer under-runs: try a larger --buffer size");
                }
                if errors > 0 {
                    let last = h.last_error.try_lock().ok().and_then(|g| g.clone()).unwrap_or_default();
                    ui.colored_label(Color32::LIGHT_RED, format!("{errors} audio errors")).on_hover_text(last);
                }
                if h.realtime_denied.load(Relaxed) {
                    ui.colored_label(ACCENT, "no real-time priority").on_hover_text(
                        "The OS refused real-time scheduling for the audio thread.\n\
                         On Linux add your user to the 'audio' group / configure rtprio.",
                    );
                }
                if self.dropped_commands > 0 {
                    ui.colored_label(Color32::LIGHT_RED, format!("{} commands dropped", self.dropped_commands));
                }
            });
        });
    }
}

impl eframe::App for DawApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.25);
        self.last_frame = now;

        self.status = self.engine.snapshot();
        self.update_meters(dt);

        // Space = play/stop, unless a text field is being edited.
        let ctx = ui.ctx().clone();
        if !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.toggle_play();
        }

        egui::Panel::top("transport")
            .frame(egui::Frame::NONE.fill(PANEL_BG).inner_margin(egui::Margin::symmetric(10, 8)))
            .show(ui, |ui| self.transport_bar(ui));
        egui::Panel::bottom("status")
            .frame(egui::Frame::NONE.fill(PANEL_BG).inner_margin(egui::Margin::symmetric(10, 4)))
            .show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::default().show(ui, |ui| self.central(ui));

        // Only animate while something moves; otherwise egui sleeps until input.
        let meters_moving = self.meter.iter().any(|&m| m > 0.0) || self.cpu_display > 0.001;
        if self.status.playing || meters_moving {
            ctx.request_repaint_after(FRAME_INTERVAL);
        } else {
            // Slow refresh keeps the CPU/status readout current while idle.
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }
}

// ---------------------------------------------------------------------------
// Custom-painted widgets (no icon fonts needed, crisp at any DPI)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Icon {
    Play,
    Stop,
    Metronome,
}

fn icon_button(ui: &mut egui::Ui, icon: Icon, active: bool, tooltip: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(34.0, 26.0), Sense::click());
    let painter = ui.painter_at(rect);
    let hovered = response.hovered();
    let bg = match (active, hovered) {
        (true, _) => Color32::from_gray(70),
        (false, true) => Color32::from_gray(62),
        (false, false) => Color32::from_gray(52),
    };
    painter.rect_filled(rect, CornerRadius::same(4), bg);

    let c = rect.center();
    let fg = match icon {
        Icon::Play if active => PLAY_GREEN,
        _ if active => ACCENT,
        _ => Color32::from_gray(if hovered { 235 } else { 200 }),
    };
    match icon {
        Icon::Play => {
            let pts = vec![c + vec2(-5.0, -7.0), c + vec2(7.0, 0.0), c + vec2(-5.0, 7.0)];
            painter.add(egui::Shape::convex_polygon(pts, fg, Stroke::NONE));
        }
        Icon::Stop => {
            painter.rect_filled(Rect::from_center_size(c, Vec2::splat(12.0)), CornerRadius::same(1), fg);
        }
        Icon::Metronome => {
            // Two dots (Live's metronome icon is two circles).
            painter.circle_filled(c + vec2(-5.0, 0.0), 4.0, fg);
            painter.circle_filled(c + vec2(5.0, 0.0), 4.0, fg);
        }
    }
    response.on_hover_text(tooltip)
}

/// Horizontal two-channel peak meter, -60…0 dBFS. Click to reset the clip LED.
fn stereo_meter(ui: &mut egui::Ui, levels: [f32; 2], clipped: bool) -> egui::Response {
    let size = vec2(150.0, 20.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(2), Color32::from_gray(22));

    let clip_w = 8.0;
    let bar_area = Rect::from_min_max(rect.min, pos2(rect.max.x - clip_w - 2.0, rect.max.y));
    let row_h = (bar_area.height() - 3.0) / 2.0;
    for (ch, &level) in levels.iter().enumerate() {
        let db = gain_to_db(level);
        let t = ((db - METER_FLOOR_DB) / -METER_FLOOR_DB).clamp(0.0, 1.0);
        if t <= 0.0 {
            continue;
        }
        let y0 = bar_area.min.y + 1.0 + ch as f32 * (row_h + 1.0);
        let full = Rect::from_min_size(pos2(bar_area.min.x + 1.0, y0), vec2(bar_area.width() - 2.0, row_h));
        // Green up to -12 dB, yellow to -3 dB, red above (drawn as segments).
        let segments = [(-12.0, Color32::from_rgb(70, 190, 90)), (-3.0, Color32::from_rgb(220, 200, 60)), (0.0, Color32::from_rgb(230, 70, 60))];
        let mut start_db = METER_FLOOR_DB;
        for (end_db, color) in segments {
            let seg_end = end_db.min(db);
            if seg_end <= start_db {
                break;
            }
            let x = |d: f32| full.min.x + full.width() * ((d - METER_FLOOR_DB) / -METER_FLOOR_DB);
            painter.rect_filled(Rect::from_min_max(pos2(x(start_db), full.min.y), pos2(x(seg_end), full.max.y)), CornerRadius::ZERO, color);
            start_db = end_db;
        }
    }
    // 0 dB tick marks every 12 dB.
    for db in [-48.0f32, -36.0, -24.0, -12.0] {
        let x = bar_area.min.x + bar_area.width() * ((db - METER_FLOOR_DB) / -METER_FLOOR_DB);
        painter.line_segment([pos2(x, rect.min.y), pos2(x, rect.min.y + 3.0)], Stroke::new(1.0, Color32::from_gray(90)));
    }
    let clip = Rect::from_min_max(pos2(rect.max.x - clip_w, rect.min.y + 1.0), pos2(rect.max.x - 1.0, rect.max.y - 1.0));
    painter.rect_filled(clip, CornerRadius::same(1), if clipped { Color32::RED } else { Color32::from_gray(45) });

    let tip = format!("{:.1} dB / {:.1} dB — click to reset clip", gain_to_db(levels[0]), gain_to_db(levels[1]));
    response.on_hover_text(tip)
}

/// One light per beat of the bar; the downbeat lights orange, others white.
fn beat_lights(ui: &mut egui::Ui, status: &StatusSnapshot) {
    let n = status.beats_per_bar.max(1) as usize;
    let size = 34.0;
    let gap = 10.0;
    let total = vec2(n as f32 * size + (n - 1) as f32 * gap, size);
    let (rect, _) = ui.allocate_exact_size(total, Sense::hover());
    let painter = ui.painter_at(rect);
    let current = status.position.beat as usize - 1;
    for i in 0..n {
        let min = Pos2::new(rect.min.x + i as f32 * (size + gap), rect.min.y);
        let r = Rect::from_min_size(min, Vec2::splat(size));
        let lit = status.playing && i == current;
        let color = match (lit, i == 0) {
            (true, true) => ACCENT,
            (true, false) => Color32::from_gray(230),
            (false, _) => Color32::from_gray(55),
        };
        painter.rect_filled(r, CornerRadius::same(5), color);
    }
}
