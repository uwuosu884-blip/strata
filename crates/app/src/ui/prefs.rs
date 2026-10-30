//! Live-style Settings window (588×629 at 100 %): tab list on the left, section
//! headers with rules and 21 px label/control rows on the right. Theme & Colors
//! and Display & Input zoom are ours; Audio and Tempo & MIDI reuse the device
//! pages; tabs without features are greyed out.

use super::theme::{self, *};
use super::widgets::*;
use crate::app::{DawApp, SettingsTab};
use eframe::egui::{self, Align2, Color32, Rect, Sense, Stroke, pos2, vec2};

const W: f32 = 562.0;
const H: f32 = 596.0;
const TITLE_H: f32 = 32.0;
const TABS_W: f32 = 140.0;
const ROW_H: f32 = 21.0;
/// Control column offset from the content's left edge and width.
const CTRL_X: f32 = 200.0;
const CTRL_W: f32 = 106.0;

const TABS: [(SettingsTab, &str, bool); 10] = [
    (SettingsTab::Display, "Display & Input", true),
    (SettingsTab::Theme, "Theme & Colors", true),
    (SettingsTab::Audio, "Audio", true),
    (SettingsTab::Link, "Link", false),
    (SettingsTab::Midi, "Tempo & MIDI", true),
    (SettingsTab::Files, "File & Folder", false),
    (SettingsTab::Library, "Library", false),
    (SettingsTab::Plugins, "Plug-Ins", false),
    (SettingsTab::Record, "Record, Warp & Launch", false),
    (SettingsTab::Licenses, "Licenses & Updates", false),
];

/// Returns true when the window was closed.
pub fn window(app: &mut DawApp, ctx: &egui::Context, tab: &mut SettingsTab) -> bool {
    let mut close = false;
    // Movable by the title bar: offset from the centred position (kept for the session).
    let off_id = egui::Id::unique("settings_window_offset");
    let off: egui::Vec2 = ctx.data(|d| d.get_temp(off_id)).unwrap_or_default();
    egui::Window::new("Settings")
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .anchor(Align2::CENTER_CENTER, off)
        .fixed_size([W, H])
        .frame(egui::Frame::NONE.fill(panel()).stroke(Stroke::new(1.0, outline())).shadow(ctx.global_style().visuals.window_shadow))
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            let (full, _) = ui.allocate_exact_size(vec2(W, H), Sense::hover());
            let p = ui.painter().clone();
            // Title bar.
            let title = Rect::from_min_size(full.min, vec2(W, TITLE_H));
            p.rect_filled(title, 0.0, gray(0x20));
            p.text(title.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, "Settings", font(12.0), gray(0xe6));
            let xr = Rect::from_center_size(title.right_center() - vec2(18.0, 0.0), vec2(20.0, 20.0));
            let xresp = ui.interact(xr, ui.make_persistent_id("set_close"), Sense::click());
            let xc = if xresp.hovered() { Color32::WHITE } else { gray(0xc8) };
            for s in [-1.0, 1.0] {
                p.line_segment([xr.center() + vec2(-5.5, -5.5 * s), xr.center() + vec2(5.5, 5.5 * s)], Stroke::new(1.3, xc));
            }
            if xresp.clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
            let drag = ui.interact(title.with_max_x(xr.min.x - 2.0), ui.make_persistent_id("set_move"), Sense::drag());
            if drag.dragged() {
                let screen = ui.ctx().content_rect();
                let lim = egui::vec2((screen.width() + W) * 0.5 - 60.0, (screen.height() + H) * 0.5 - TITLE_H);
                let lim_top = (screen.height() - H) * 0.5; // title bar stays on screen
                let o = off + drag.drag_delta();
                let o = egui::vec2(o.x.clamp(-lim.x, lim.x), o.y.clamp(-lim_top, lim.y));
                ui.ctx().data_mut(|d| d.insert_temp(off_id, o));
            }
            // Tab list.
            let body = Rect::from_min_max(pos2(full.left(), title.bottom()), full.max);
            let list = Rect::from_min_size(body.min, vec2(TABS_W, body.height()));
            p.line_segment([pos2(list.right() + 0.5, body.top()), pos2(list.right() + 0.5, body.bottom())], Stroke::new(1.0, gray(0x1d)));
            for (i, (t, label, enabled)) in TABS.iter().enumerate() {
                let r = Rect::from_min_size(pos2(list.left(), body.top() + 5.0 + i as f32 * 25.0), vec2(TABS_W, 25.0));
                let resp = ui.interact(r, ui.make_persistent_id(("set_tab", i)), if *enabled { Sense::click() } else { Sense::hover() });
                if *t == *tab {
                    p.rect_filled(r, 0.0, sel());
                } else if resp.hovered() && *enabled {
                    p.rect_filled(r, 0.0, gray(0x3c));
                }
                p.text(r.left_center() + vec2(11.0, 0.0), Align2::LEFT_CENTER, *label, bold(FS), if *enabled { gray(0xd2) } else { dim() });
                if resp.clicked() {
                    if *t != *tab {
                        app.device_lists = None;
                    }
                    *tab = *t;
                }
                if !enabled {
                    resp.on_hover_text("Not available in this version");
                }
            }
            // Page.
            let page = Rect::from_min_max(pos2(list.right() + 15.0, body.top() + 8.0), body.max - vec2(14.0, 8.0));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(page).layout(egui::Layout::top_down(egui::Align::Min)));
            egui::ScrollArea::vertical().id_salt("set_page").auto_shrink([false, false]).show(&mut child, |ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 0.0);
                match tab {
                    SettingsTab::Theme => theme_page(app, ui),
                    SettingsTab::Display => display_page(app, ui),
                    SettingsTab::Audio => {
                        section(ui, "Audio Device");
                        ui.spacing_mut().item_spacing = vec2(6.0, 4.0);
                        super::io::audio_settings(app, ui);
                    }
                    SettingsTab::Midi => {
                        section(ui, "MIDI Ports");
                        ui.spacing_mut().item_spacing = vec2(6.0, 4.0);
                        super::io::midi_settings(app, ui);
                    }
                    _ => {}
                }
            });
        });
    close
}

/// Bold header with a rule to the right edge.
fn section(ui: &mut egui::Ui, title: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 25.0), Sense::hover());
    let p = ui.painter();
    let g = p.layout_no_wrap(title.to_string(), bold(FS), gray(0xd2));
    let tw = g.size().x;
    p.galley(pos2(r.left(), r.center().y - g.size().y / 2.0), g, Color32::PLACEHOLDER);
    p.line_segment(
        [pos2(r.left() + tw + 8.0, r.center().y + 0.5), pos2(r.right() - 4.0, r.center().y + 0.5)],
        Stroke::new(1.0, gray(0x52)),
    );
}

/// One label/control row; returns the control rect.
fn row(ui: &mut egui::Ui, label: &str, w: f32, enabled: bool) -> (Rect, egui::Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
    ui.painter().text(r.left_center(), Align2::LEFT_CENTER, label, font(FS), if enabled { text() } else { dim() });
    let c = Rect::from_min_size(pos2(r.left() + CTRL_X, r.center().y - 6.5), vec2(w, 13.0));
    let child =
        ui.new_child(egui::UiBuilder::new().max_rect(c.expand2(vec2(0.0, 2.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    (c, child)
}

/// Live chooser: #242424 box, 1 px outline, ▼ at the right.
fn chooser<T: Copy + PartialEq>(ui: &mut egui::Ui, label: &str, cur: &mut T, items: &[(T, &str)], enabled: bool) -> bool {
    let (c, mut cui) = row(ui, label, CTRL_W, enabled);
    let mut changed = false;
    let txt = items.iter().find(|(v, _)| v == cur).map_or("", |(_, s)| *s);
    cui.add_enabled_ui(enabled, |ui| {
        ui.set_width(c.width());
        egui::ComboBox::from_id_salt(("set_ch", label)).width(c.width()).height(260.0).selected_text(txt).show_ui(ui, |ui| {
            for (v, s) in items {
                if ui.selectable_label(v == cur, *s).clicked() && v != cur {
                    *cur = *v;
                    changed = true;
                }
            }
        });
    });
    changed
}

fn onoff(ui: &mut egui::Ui, label: &str, v: &mut bool) -> bool {
    let (c, _) = row(ui, label, 52.0, true);
    let r = ui.interact(c, ui.make_persistent_id(("set_tg", label)), Sense::click());
    live_toggle(ui.painter(), c, *v, if *v { "On" } else { "Off" }, r.hovered());
    if r.clicked() {
        *v = !*v;
        return true;
    }
    false
}

/// Live number box: drag up/down (Shift = fine), double-click = default.
#[allow(clippy::too_many_arguments)]
fn number(
    ui: &mut egui::Ui,
    label: &str,
    v: &mut f32,
    min: f32,
    max: f32,
    default: f32,
    per_px: f32,
    fmt: impl Fn(f32) -> String,
    arrow: bool,
) -> bool {
    let (c, _) = row(ui, label, CTRL_W, true);
    let r = ui.interact(c, ui.make_persistent_id(("set_num", label)), Sense::click_and_drag());
    let old = *v;
    if r.dragged() {
        let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
        *v = (*v - r.drag_delta().y * per_px * fine).clamp(min, max);
    }
    if r.double_clicked() {
        *v = default;
    }
    if r.hovered() {
        let s = ui.input(|i| i.smooth_scroll_delta.y);
        if s != 0.0 {
            *v = (*v + s.signum() * per_px * 2.0).clamp(min, max);
        }
    }
    let p = ui.painter();
    p.rect_filled(c, 0.0, if r.hovered() || r.dragged() { gray(0x2a) } else { off_fill() });
    p.rect_stroke(c, 0.0, Stroke::new(1.0, outline()), egui::StrokeKind::Inside);
    p.text(c.center(), Align2::CENTER_CENTER, fmt(*v), font(FS), text());
    if arrow {
        let a = pos2(c.right() - 7.0, c.center().y);
        p.add(egui::Shape::convex_polygon(vec![a + vec2(-3.5, 0.0), a + vec2(2.5, -3.5), a + vec2(2.5, 3.5)], dim(), Stroke::NONE));
    }
    *v != old
}

fn theme_page(app: &mut DawApp, ui: &mut egui::Ui) {
    let mut t = app.settings.theme.clone();
    section(ui, "Theme");
    let themes: Vec<(ThemeName, &str)> = ThemeName::ALL.iter().map(|v| (*v, v.label())).collect();
    chooser(ui, "Theme", &mut t.theme, &themes, true);
    let apps: Vec<(Appearance, &str)> = Appearance::ALL.iter().map(|v| (*v, v.label())).collect();
    chooser(ui, "Appearance", &mut t.appearance, &apps, t.theme == ThemeName::Default);
    let tones: Vec<(Tone, &str)> = Tone::ALL.iter().map(|v| (*v, v.label())).collect();
    chooser(ui, "Tone", &mut t.tone, &tones, true);
    onoff(ui, "High Contrast", &mut t.high_contrast);
    ui.add_space(4.0);
    // Temporary (0.7.7): pick the waveform drawing algorithm.
    section(ui, "Waveform (temporary)");
    let waves: Vec<(super::wave::WaveStyle, &str)> = super::wave::WaveStyle::ALL.iter().map(|v| (*v, v.label())).collect();
    chooser(ui, "Waveform Style", &mut t.wave_style, &waves, true);
    ui.add_space(4.0);
    section(ui, "Customization");
    let pct = |v: f32| format!("{:.0} %", v * 100.0);
    number(ui, "Grid Line Intensity", &mut t.grid_intensity, 0.0, 2.0, 1.0, 0.01, pct, false);
    number(ui, "Brightness", &mut t.brightness, 0.5, 1.5, 1.0, 0.005, pct, false);
    number(ui, "Color Intensity", &mut t.color_intensity, 0.0, 1.0, 0.0, 0.005, pct, false);
    number(ui, "Color Hue", &mut t.color_hue, 0.0, 359.0, 0.0, 1.0, |v| format!("{v:.0}°"), true);
    ui.add_space(4.0);
    section(ui, "Track And Clip Colors");
    onoff(ui, "Auto-Assign Track Colors", &mut t.auto_assign_colors);
    {
        // Default Track Color: palette swatch chooser.
        let (c, mut cui) = row(ui, "Default Track Color", CTRL_W, true);
        let cur = t.default_track_color;
        cui.set_width(c.width());
        let resp = egui::ComboBox::from_id_salt("set_dtc").width(c.width()).selected_text("").show_ui(&mut cui, |ui| {
            ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
            egui::Grid::new("dtc_grid").spacing([2.0, 2.0]).show(ui, |ui| {
                for (i, col) in daw_project::PALETTE.iter().enumerate() {
                    let (r, s) = ui.allocate_exact_size(vec2(16.0, 13.0), Sense::click());
                    ui.painter().rect_filled(r, 0.0, rgb(*col));
                    if cur == Some(*col) || s.hovered() {
                        ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Inside);
                    }
                    if s.clicked() {
                        t.default_track_color = Some(*col);
                    }
                    if i % 6 == 5 {
                        ui.end_row();
                    }
                }
            });
        });
        if let Some(col) = cur {
            let sw = Rect::from_min_max(c.min + vec2(2.0, 2.0), pos2(c.right() - 16.0, c.bottom() - 2.0));
            ui.painter().rect_filled(sw, 0.0, rgb(col));
        }
        let _ = resp;
    }
    onoff(ui, "Reduced Automatic Colors", &mut t.reduced_colors);
    {
        let (c, _) = row(ui, "Clip Color", CTRL_W, true);
        let r = ui.interact(c, ui.make_persistent_id("set_clipcol"), Sense::click());
        let track = t.clip_color == ClipColorMode::TrackColor;
        live_toggle(ui.painter(), c, track, t.clip_color.label(), r.hovered());
        if r.clicked() {
            t.clip_color = if track { ClipColorMode::Random } else { ClipColorMode::TrackColor };
        }
    }
    if t != app.settings.theme {
        app.settings.theme = t;
        theme::update(ui.ctx(), &app.settings.theme);
        app.save_settings();
    }
}

fn display_page(app: &mut DawApp, ui: &mut egui::Ui) {
    section(ui, "Display");
    let mut z = app.settings.zoom();
    let zooms: Vec<(u16, String)> = [50u16, 75, 100, 125, 150, 175, 200].iter().map(|v| (*v, format!("{v} %"))).collect();
    let items: Vec<(u16, &str)> = zooms.iter().map(|(v, s)| (*v, s.as_str())).collect();
    if chooser(ui, "Zoom Display", &mut z, &items, true) {
        app.settings.zoom_pct = z;
        app.save_settings();
    }
    for l in ["Language", "Follow Behavior", "Hide Labels"] {
        row(ui, l, CTRL_W, false);
    }
    ui.add_space(4.0);
    section(ui, "Computer Keyboard");
    row(ui, "Computer MIDI Keyboard: toggle with M or the top-bar keyboard button", 0.0, true);
}

/// Live on/off box: orange with dark text when on, near-black when off.
pub fn live_toggle(p: &egui::Painter, r: Rect, on: bool, label: &str, hovered: bool) {
    let fill = match (on, hovered) {
        (true, false) => ORANGE,
        (true, true) => Color32::from_rgb(0xff, 0xbd, 0x78),
        (false, false) => off_fill(),
        (false, true) => gray(0x2a),
    };
    p.rect_filled(r, 0.0, fill);
    p.rect_stroke(r, 0.0, Stroke::new(1.0, outline()), egui::StrokeKind::Inside);
    p.text(r.center(), Align2::CENTER_CENTER, label, font(FS), if on { Color32::from_gray(0x10) } else { text() });
}
