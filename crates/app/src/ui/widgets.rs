//! Custom-painted widgets: no icon fonts, crisp at any DPI, cheap to draw.

use daw_project::gain_to_db;
use eframe::egui::{self, Color32, CornerRadius, Rect, Sense, Stroke, Vec2, pos2, vec2};

pub const ACCENT: Color32 = Color32::from_rgb(255, 160, 40);
pub const PLAY_GREEN: Color32 = Color32::from_rgb(90, 200, 110);
pub const REC_RED: Color32 = Color32::from_rgb(230, 70, 60);
pub const PANEL_BG: Color32 = Color32::from_gray(38);
pub const BG_DARK: Color32 = Color32::from_gray(28);
pub const SLOT_BG: Color32 = Color32::from_gray(46);
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
        (true, _) => Color32::from_gray(72),
        (false, true) => Color32::from_gray(62),
        (false, false) => Color32::from_gray(52),
    };
    p.rect_filled(rect, CornerRadius::same(4), bg);
    let c = rect.center();
    let fg = match icon {
        Icon::Play if active => PLAY_GREEN,
        Icon::Record if active => REC_RED,
        _ if active => ACCENT,
        _ => Color32::from_gray(if hovered { 235 } else { 200 }),
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
    let (rect, resp) = ui.allocate_exact_size(vec2(22.0, 18.0), Sense::click());
    let bg = if on {
        on_color
    } else if resp.hovered() {
        Color32::from_gray(70)
    } else {
        Color32::from_gray(58)
    };
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::same(2), bg);
    let fg = if on { Color32::BLACK } else { Color32::from_gray(210) };
    if text == "REC" {
        // Painted record dot (font-independent).
        p.circle_filled(rect.center(), 4.0, if on { Color32::BLACK } else { REC_RED });
    } else {
        p.text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(11.0), fg);
    }
    resp.on_hover_text(tooltip)
}

/// Horizontal two-channel peak meter, -60…0 dBFS. Click resets the clip LED.
pub fn stereo_meter(ui: &mut egui::Ui, levels: [f32; 2], clipped: bool, size: Vec2) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::same(2), Color32::from_gray(22));
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
    p.rect_filled(clip, CornerRadius::same(1), if clipped { Color32::RED } else { Color32::from_gray(45) });
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
    p.rect_filled(rect, CornerRadius::same(1), Color32::from_gray(20));
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
    p.line_segment([pos2(cx, rect.min.y), pos2(cx, rect.max.y)], Stroke::new(3.0, Color32::from_gray(20)));
    let y0 = to_y(0.0);
    p.line_segment([pos2(rect.min.x, y0), pos2(rect.max.x, y0)], Stroke::new(1.0, Color32::from_gray(80)));
    let y = to_y(*db);
    p.rect_filled(
        Rect::from_center_size(pos2(cx, y), vec2(rect.width(), 7.0)),
        CornerRadius::same(2),
        if resp.hovered() || resp.dragged() { Color32::from_gray(230) } else { Color32::from_gray(190) },
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
    p.rect_filled(rect, CornerRadius::same(2), Color32::from_gray(30));
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
    p.text(rect.center(), egui::Align2::CENTER_CENTER, label(*v), egui::FontId::proportional(10.0), Color32::from_gray(230));
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
