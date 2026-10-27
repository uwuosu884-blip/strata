//! Custom-painted widgets: no icon fonts, crisp at any DPI, cheap to draw.

use daw_project::gain_to_db;
use eframe::egui::{self, Color32, CornerRadius, Rect, Sense, Stroke, Vec2, pos2, vec2};

pub const ACCENT: Color32 = super::theme::ORANGE;
pub const PLAY_GREEN: Color32 = Color32::from_rgb(90, 200, 110);
pub const REC_RED: Color32 = Color32::from_rgb(0xff, 0x55, 0x48);
pub use super::theme::{gray, grid};
pub const METER_FLOOR_DB: f32 = -60.0;

pub fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// Darker/lighter variant of a colour.
pub fn shade(c: Color32, f: f32) -> Color32 {
    let m = |v: u8| ((v as f32 * f).clamp(0.0, 255.0)) as u8;
    Color32::from_rgb(m(c.r()), m(c.g()), m(c.b()))
}

#[derive(Clone, Copy, PartialEq)]
pub enum Icon {
    Play,
    Stop,
    Record,
    Metronome,
    Loop,
    Keyboard,
    Session,
    Arrangement,
}

pub fn icon_button(ui: &mut egui::Ui, icon: Icon, active: bool, tooltip: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(32.0, 24.0), Sense::click());
    let p = ui.painter_at(rect);
    let hovered = response.hovered();
    let bg = match (active, hovered) {
        (true, _) => gray(72),
        (false, true) => gray(62),
        (false, false) => gray(52),
    };
    p.rect_filled(rect, CornerRadius::same(4), bg);
    let c = rect.center();
    let fg = match icon {
        Icon::Play if active => PLAY_GREEN,
        Icon::Record if active => REC_RED,
        _ if active => ACCENT,
        _ => gray(if hovered { 235 } else { 200 }),
    };
    draw_icon(&p, icon, c, fg);
    response.on_hover_text(tooltip)
}

pub fn draw_icon(p: &egui::Painter, icon: Icon, c: egui::Pos2, fg: Color32) {
    match icon {
        Icon::Play => {
            p.add(egui::Shape::convex_polygon(vec![c + vec2(-5.0, -6.5), c + vec2(6.5, 0.0), c + vec2(-5.0, 6.5)], fg, Stroke::NONE));
        }
        Icon::Stop => {
            p.rect_filled(Rect::from_center_size(c, Vec2::splat(11.0)), CornerRadius::same(1), fg);
        }
        Icon::Record => {
            p.circle_filled(c, 6.0, fg);
        }
        Icon::Metronome => {
            p.circle_filled(c + vec2(-4.5, 0.0), 3.5, fg);
            p.circle_filled(c + vec2(4.5, 0.0), 3.5, fg);
        }
        Icon::Loop => {
            let s = Stroke::new(2.0, fg);
            p.line_segment([c + vec2(-8.0, -4.0), c + vec2(8.0, -4.0)], s);
            p.line_segment([c + vec2(-8.0, 4.0), c + vec2(8.0, 4.0)], s);
            p.line_segment([c + vec2(-8.0, -4.0), c + vec2(-8.0, 4.0)], s);
            p.add(egui::Shape::convex_polygon(vec![c + vec2(8.0, -1.0), c + vec2(4.0, 4.0), c + vec2(8.0, 8.0)], fg, Stroke::NONE));
        }
        Icon::Keyboard => {
            let r = Rect::from_center_size(c, vec2(18.0, 11.0));
            p.rect_stroke(r, CornerRadius::same(1), Stroke::new(1.5, fg), egui::StrokeKind::Inside);
            for i in 1..4 {
                let x = r.min.x + i as f32 * 4.5;
                p.line_segment([pos2(x, r.min.y), pos2(x, r.max.y)], Stroke::new(1.0, fg));
            }
        }
        Icon::Session => {
            for i in 0..3 {
                for j in 0..3 {
                    p.rect_filled(
                        Rect::from_center_size(c + vec2(i as f32 * 5.0 - 5.0, j as f32 * 4.0 - 4.0), vec2(4.0, 3.0)),
                        CornerRadius::ZERO,
                        fg,
                    );
                }
            }
        }
        Icon::Arrangement => {
            for (j, (a, b)) in [(-8.0, 2.0), (-3.0, 8.0), (-8.0, -1.0)].iter().enumerate() {
                p.rect_filled(
                    Rect::from_min_max(c + vec2(*a, j as f32 * 4.0 - 6.0), c + vec2(*b, j as f32 * 4.0 - 3.0)),
                    CornerRadius::ZERO,
                    fg,
                );
            }
        }
    }
}

/// Small square text toggle (mute/solo/arm style).
pub fn toggle_box(ui: &mut egui::Ui, text: &str, on: bool, on_color: Color32, tooltip: &str) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(vec2(21.0, 13.0), Sense::hover());
    rect_toggle(ui, rect, ui.next_auto_id(), text, on, on_color, tooltip)
}

/// Horizontal two-channel peak meter, -60…0 dBFS. Click resets the clip LED.
pub fn stereo_meter(ui: &mut egui::Ui, levels: [f32; 2], clipped: bool, size: Vec2) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::same(2), gray(22));
    let clip_w = 6.0;
    let bar = Rect::from_min_max(rect.min, pos2(rect.max.x - clip_w - 2.0, rect.max.y));
    let row_h = (bar.height() - 3.0) / 2.0;
    for (ch, &level) in levels.iter().enumerate() {
        let t = db_frac(level);
        if t > 0.0 {
            let y0 = bar.min.y + 1.0 + ch as f32 * (row_h + 1.0);
            let r = Rect::from_min_size(pos2(bar.min.x + 1.0, y0), vec2((bar.width() - 2.0) * t, row_h));
            p.rect_filled(r, CornerRadius::ZERO, meter_color(level));
        }
    }
    let clip = Rect::from_min_max(pos2(rect.max.x - clip_w, rect.min.y + 1.0), pos2(rect.max.x - 1.0, rect.max.y - 1.0));
    p.rect_filled(clip, CornerRadius::same(1), if clipped { Color32::RED } else { gray(45) });
    response.on_hover_text(format!("{:.1} / {:.1} dB, click to reset clip", gain_to_db(levels[0]), gain_to_db(levels[1])))
}

fn db_frac(level: f32) -> f32 {
    ((gain_to_db(level) - METER_FLOOR_DB) / -METER_FLOOR_DB).clamp(0.0, 1.0)
}

fn meter_color(level: f32) -> Color32 {
    let db = gain_to_db(level);
    if db > -3.0 {
        Color32::from_rgb(230, 70, 60)
    } else if db > -12.0 {
        Color32::from_rgb(220, 200, 60)
    } else {
        Color32::from_rgb(70, 190, 90)
    }
}

/// Vertical stereo meter (mixer strip).
pub fn vmeter(p: &egui::Painter, rect: Rect, levels: [f32; 2]) {
    p.rect_filled(rect, CornerRadius::same(1), gray(20));
    let w = (rect.width() - 1.0) / 2.0;
    for (ch, &l) in levels.iter().enumerate() {
        let h = rect.height() * db_frac(l);
        if h > 0.0 {
            let x = rect.min.x + ch as f32 * (w + 1.0);
            p.rect_filled(Rect::from_min_max(pos2(x, rect.max.y - h), pos2(x + w, rect.max.y)), CornerRadius::ZERO, meter_color(l));
        }
    }
}

/// Vertical volume fader in dB (-60…+6). Double-click resets to 0 dB.
/// Returns (response, changed).
pub fn fader(ui: &mut egui::Ui, db: &mut f32, size: Vec2) -> (egui::Response, bool) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let to_y = |d: f32| {
        // Slightly non-linear: more resolution near 0 dB.
        let t = ((d - METER_FLOOR_DB) / (6.0 - METER_FLOOR_DB)).clamp(0.0, 1.0).powf(2.0);
        rect.max.y - t * rect.height()
    };
    let from_y = |y: f32| {
        let t = ((rect.max.y - y) / rect.height()).clamp(0.0, 1.0).sqrt();
        METER_FLOOR_DB + t * (6.0 - METER_FLOOR_DB)
    };
    let mut changed = false;
    if resp.double_clicked() {
        *db = 0.0;
        changed = true;
    } else if resp.dragged()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let fine = ui.input(|i| i.modifiers.shift);
        let nd = if fine { *db - resp.drag_delta().y * 0.1 } else { from_y(pos.y) };
        let nd = nd.clamp(METER_FLOOR_DB, 6.0);
        if (nd - *db).abs() > 1e-4 {
            *db = nd;
            changed = true;
        }
    }
    let p = ui.painter_at(rect);
    let cx = rect.center().x;
    p.line_segment([pos2(cx, rect.min.y), pos2(cx, rect.max.y)], Stroke::new(3.0, gray(20)));
    let y0 = to_y(0.0);
    p.line_segment([pos2(rect.min.x, y0), pos2(rect.max.x, y0)], Stroke::new(1.0, gray(80)));
    let y = to_y(*db);
    p.rect_filled(
        Rect::from_center_size(pos2(cx, y), vec2(rect.width(), 7.0)),
        CornerRadius::same(2),
        if resp.hovered() || resp.dragged() { gray(230) } else { gray(190) },
    );
    let text = if *db <= METER_FLOOR_DB { "-inf dB".to_string() } else { format!("{db:.1} dB") };
    (resp.on_hover_text(text), changed)
}

/// Horizontal mini slider for pan etc. Double-click resets to `default`.
pub fn hslider(
    ui: &mut egui::Ui,
    v: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    default: f32,
    size: Vec2,
    label: impl Fn(f32) -> String,
) -> (egui::Response, bool) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let (lo, hi) = (*range.start(), *range.end());
    let mut changed = false;
    if resp.double_clicked() {
        *v = default;
        changed = true;
    } else if resp.dragged() {
        let nv = (*v + resp.drag_delta().x / rect.width() * (hi - lo)).clamp(lo, hi);
        if nv != *v {
            *v = nv;
            changed = true;
        }
    }
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::same(2), gray(30));
    let t = (*v - lo) / (hi - lo);
    let d = ((default - lo) / (hi - lo)).clamp(0.0, 1.0);
    let x0 = rect.min.x + rect.width() * d;
    let x1 = rect.min.x + rect.width() * t;
    if (x1 - x0).abs() >= 1.0 {
        p.rect_filled(
            Rect::from_min_max(pos2(x0.min(x1), rect.min.y + 2.0), pos2(x0.max(x1), rect.max.y - 2.0)),
            CornerRadius::ZERO,
            shade(ACCENT, 0.8),
        );
    }
    p.text(rect.center(), egui::Align2::CENTER_CENTER, label(*v), egui::FontId::proportional(10.0), gray(230));
    (resp, changed)
}

pub fn pan_label(v: f32) -> String {
    if v.abs() < 0.005 {
        "C".into()
    } else if v < 0.0 {
        format!("{:.0}L", -v * 50.0)
    } else {
        format!("{:.0}R", v * 50.0)
    }
}

/// Relative drag that never runs into the screen edge (BPM, header volume/pan):
/// the cursor is hidden and parked at the press point while movement comes from
/// raw mouse deltas (winit `DeviceEvent::MouseMotion` → `Event::MouseMoved`).
/// Falls back to ordinary pointer deltas if the platform sends no raw motion.
/// Returns this frame's movement in points.
pub fn endless_drag(ui: &egui::Ui, resp: &egui::Response) -> Vec2 {
    let ctx = ui.ctx();
    let seen_id = egui::Id::unique("endless_raw_seen");
    let anchor_id = resp.id.with("endless_anchor");
    let raw: Vec2 = ui.input(|i| {
        i.events.iter().filter_map(|e| if let egui::Event::MouseMoved(d) = e { Some(*d) } else { None }).fold(Vec2::ZERO, |a, b| a + b)
    });
    let mut seen = ctx.data(|d| d.get_temp::<bool>(seen_id)).unwrap_or(false);
    if raw != Vec2::ZERO && !seen {
        seen = true;
        ctx.data_mut(|d| d.insert_temp(seen_id, true));
    }
    if resp.drag_started() {
        let anchor = ui.input(|i| i.pointer.press_origin()).unwrap_or(resp.rect.center());
        ctx.data_mut(|d| d.insert_temp(anchor_id, anchor));
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(false));
        // Wayland/macOS lock the pointer; X11/Windows refuse and we re-park it below instead.
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(egui::viewport::CursorGrab::Locked));
    }
    let anchor = ctx.data(|d| d.get_temp::<egui::Pos2>(anchor_id));
    let mut delta = Vec2::ZERO;
    if resp.dragged() {
        if seen {
            delta = raw / ctx.pixels_per_point();
            if raw != Vec2::ZERO
                && let Some(a) = anchor
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::CursorPosition(a));
            }
        } else {
            delta = resp.drag_delta();
        }
    }
    if resp.drag_stopped() {
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(egui::viewport::CursorGrab::None));
        if let Some(a) = anchor {
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorPosition(a));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(true));
        ctx.data_mut(|d| d.remove::<egui::Pos2>(anchor_id));
    }
    delta
}

/// dB ↔ 0..1 position used by faders and volume boxes (more resolution near 0 dB).
pub fn db_to_pos(db: f32) -> f32 {
    ((db - METER_FLOOR_DB) / (6.0 - METER_FLOOR_DB)).clamp(0.0, 1.0).powf(2.0)
}

pub fn pos_to_db(t: f32) -> f32 {
    METER_FLOOR_DB + t.clamp(0.0, 1.0).sqrt() * (6.0 - METER_FLOOR_DB)
}

/// Live-style track volume box: shows dB, drag up/down (Shift = fine), double-click = 0 dB.
pub fn volume_box(ui: &mut egui::Ui, rect: Rect, id: egui::Id, db: &mut f32) -> (egui::Response, bool) {
    level_box(ui, rect, id, db, super::theme::VOL_TEAL, "Track volume (dB) · drag · double-click: 0 dB")
}

/// Cue colour (Live's blue preview/cue knob).
pub const CUE_BLUE: Color32 = Color32::from_rgb(70, 130, 220);

/// Master Cue volume: metronome + browser preview level (blue).
pub fn cue_box(ui: &mut egui::Ui, rect: Rect, id: egui::Id, db: &mut f32) -> (egui::Response, bool) {
    level_box(ui, rect, id, db, super::theme::CUE_BLUE, "Cue volume: metronome + browser preview (dB) · drag · double-click: 0 dB")
}

/// Activator colour while another track's solo silences this one.
pub fn solo_dimmed(c: Color32) -> Color32 {
    shade(c, 0.42)
}

fn level_box(ui: &mut egui::Ui, rect: Rect, id: egui::Id, db: &mut f32, fill: Color32, tip: &str) -> (egui::Response, bool) {
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    let mut changed = false;
    let d = endless_drag(ui, &resp);
    if resp.double_clicked() {
        *db = 0.0;
        changed = true;
    } else if d != Vec2::ZERO {
        let k = if ui.input(|i| i.modifiers.shift) { 0.0007 } else { 0.006 };
        let nd = pos_to_db(db_to_pos(*db) - (d.y - d.x) * k).clamp(METER_FLOOR_DB, 6.0);
        if (nd - *db).abs() > 1e-5 {
            *db = nd;
            changed = true;
        }
    }
    // Live number slider: dark box, coloured fill up to the value, 1 px outline.
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::ZERO, super::theme::off_fill());
    let w = rect.width() * db_to_pos(*db);
    p.rect_filled(Rect::from_min_size(rect.min, vec2(w, rect.height())), CornerRadius::ZERO, fill);
    p.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0, super::theme::outline()), egui::StrokeKind::Inside);
    let text = if *db <= METER_FLOOR_DB {
        "-inf".to_string()
    } else if db.abs() < 0.05 {
        "0".to_string()
    } else {
        format!("{db:.1}")
    };
    p.text(rect.center(), egui::Align2::CENTER_CENTER, text, super::theme::font(super::theme::FS), Color32::from_gray(0xe0));
    (resp.on_hover_text(tip), changed)
}

/// Live-style pan box: 50L … C … 50R, drag up/down (Shift = fine), double-click = centre.
pub fn pan_box(ui: &mut egui::Ui, rect: Rect, id: egui::Id, pan: &mut f32) -> (egui::Response, bool) {
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    let mut changed = false;
    let d = endless_drag(ui, &resp);
    if resp.double_clicked() {
        *pan = 0.0;
        changed = true;
    } else if d != Vec2::ZERO {
        let k = if ui.input(|i| i.modifiers.shift) { 0.002 } else { 0.02 };
        let np = (*pan - (d.y - d.x) * k).clamp(-1.0, 1.0);
        if (np - *pan).abs() > 1e-6 {
            *pan = np;
            changed = true;
        }
    }
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::ZERO, super::theme::off_fill());
    let cx = rect.center().x;
    let x = cx + *pan * rect.width() * 0.5;
    if pan.abs() > 0.005 {
        p.rect_filled(Rect::from_x_y_ranges(cx.min(x)..=cx.max(x), rect.y_range()), CornerRadius::ZERO, super::theme::VOL_TEAL);
    }
    p.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0, super::theme::outline()), egui::StrokeKind::Inside);
    p.line_segment([pos2(cx, rect.min.y + 1.0), pos2(cx, rect.min.y + 3.0)], Stroke::new(1.0, gray(0x8a)));
    p.text(rect.center(), egui::Align2::CENTER_CENTER, pan_label(*pan), super::theme::font(super::theme::FS), Color32::from_gray(0xe0));
    (resp.on_hover_text("Pan · drag · double-click: centre"), changed)
}

/// ⊳ / ⊽ in a circle: collapse button at the left of arrangement track headers.
pub fn draw_collapse_button(p: &egui::Painter, c: egui::Pos2, collapsed: bool, col: Color32) {
    p.circle_stroke(c, 5.5, Stroke::new(1.1, col));
    let pts = if collapsed {
        vec![c + vec2(-1.8, -3.2), c + vec2(3.0, 0.0), c + vec2(-1.8, 3.2)]
    } else {
        vec![c + vec2(-3.2, -1.6), c + vec2(3.2, -1.6), c + vec2(0.0, 3.0)]
    };
    p.add(egui::Shape::convex_polygon(pts, col, Stroke::NONE));
}

/// Deactivated clip / note colour: greyer and darker.
pub fn disabled(c: Color32) -> Color32 {
    let g = (c.r() as u32 + c.g() as u32 + c.b() as u32) / 3;
    let m = |v: u8| ((v as u32 + 2 * g) / 3 * 55 / 100) as u8;
    Color32::from_rgb(m(c.r()), m(c.g()), m(c.b()))
}

/// Right-click grid menu (arrangement and piano roll).
pub fn grid_menu(ui: &mut egui::Ui, mode: &mut crate::arr_ops::GridMode) {
    use crate::arr_ops::GridMode;
    ui.menu_button("Grid", |ui| {
        ui.label(egui::RichText::new("Adaptive").small().color(Color32::GRAY));
        for (lvl, name) in GridMode::ADAPTIVE {
            if ui.radio(*mode == GridMode::Adaptive(lvl), name).clicked() {
                *mode = GridMode::Adaptive(lvl);
                ui.close();
            }
        }
        ui.separator();
        ui.label(egui::RichText::new("Fixed").small().color(Color32::GRAY));
        for (g, name) in GridMode::FIXED {
            if ui.radio(*mode == GridMode::Fixed(g), name).clicked() {
                *mode = GridMode::Fixed(g);
                ui.close();
            }
        }
        ui.separator();
        if ui.radio(*mode == GridMode::Off, "Off (no snapping)").clicked() {
            *mode = GridMode::Off;
            ui.close();
        }
        ui.label(egui::RichText::new("Hold Alt to ignore the grid").small().color(Color32::GRAY));
    });
}

/// Header toggle painted into `rect` (arrangement headers: activator, S, arm).
/// `text == "REC"` paints the record dot.
pub fn rect_toggle(ui: &mut egui::Ui, rect: Rect, id: egui::Id, text: &str, on: bool, on_color: Color32, tooltip: &str) -> egui::Response {
    // Live switch: coloured with dark text when on, near-black with a 1 px outline when off.
    let resp = ui.interact(rect, id, Sense::click());
    let bg = if on {
        if resp.hovered() { shade(on_color, 1.1) } else { on_color }
    } else if resp.hovered() {
        gray(0x2c)
    } else {
        super::theme::off_fill()
    };
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::ZERO, bg);
    p.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0, super::theme::outline()), egui::StrokeKind::Inside);
    if text == "REC" {
        let r = (rect.height() * 0.3).min(4.0);
        if on {
            p.circle_filled(rect.center(), r, Color32::from_gray(0x16));
        } else {
            p.circle_stroke(rect.center(), r, Stroke::new(1.2, gray(0x9a)));
            p.circle_filled(rect.center(), r * 0.45, gray(0x9a));
        }
    } else {
        let fg = if on { Color32::from_gray(0x16) } else { super::theme::text() };
        p.text(rect.center(), egui::Align2::CENTER_CENTER, text, super::theme::font(super::theme::FS), fg);
    }
    resp.on_hover_text(tooltip)
}

/// Black or white text, whichever reads better on `bg`.
pub fn text_on(bg: Color32) -> Color32 {
    let l = 0.299 * bg.r() as f32 + 0.587 * bg.g() as f32 + 0.114 * bg.b() as f32;
    if l > 140.0 { Color32::BLACK } else { Color32::WHITE }
}
