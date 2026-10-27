//! Custom-painted widgets: no icon fonts, crisp at any DPI, cheap to draw.

use eframe::egui::{self, Color32, CornerRadius, Rect, Sense, Stroke, Vec2, pos2, vec2};

pub const ACCENT: Color32 = super::theme::ORANGE;
pub const PLAY_GREEN: Color32 = Color32::from_rgb(90, 200, 110);
pub const REC_RED: Color32 = Color32::from_rgb(0xff, 0x55, 0x48);
pub use super::theme::gray;
pub const METER_FLOOR_DB: f32 = -60.0;

pub fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// Darker/lighter variant of a colour.
pub fn shade(c: Color32, f: f32) -> Color32 {
    let m = |v: u8| ((v as f32 * f).clamp(0.0, 255.0)) as u8;
    Color32::from_rgb(m(c.r()), m(c.g()), m(c.b()))
}

/// Play triangle (session launch buttons).
pub fn draw_play(p: &egui::Painter, c: egui::Pos2, fg: Color32) {
    p.add(egui::Shape::convex_polygon(vec![c + vec2(-5.0, -6.5), c + vec2(6.5, 0.0), c + vec2(-5.0, 6.5)], fg, Stroke::NONE));
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

/// Live round knob (bipolar −1…1): arc from the top, drag up/down (Shift = fine),
/// double-click = centre.
pub fn knob(ui: &mut egui::Ui, c: egui::Pos2, r: f32, id: egui::Id, v: &mut f32) -> (egui::Response, bool) {
    let rect = Rect::from_center_size(c, vec2(r * 2.0 + 4.0, r * 2.0 + 4.0));
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    let mut changed = false;
    let d = endless_drag(ui, &resp);
    if resp.double_clicked() {
        *v = 0.0;
        changed = true;
    } else if d != Vec2::ZERO {
        let k = if ui.input(|i| i.modifiers.shift) { 0.002 } else { 0.012 };
        let nv = (*v - (d.y - d.x) * k).clamp(-1.0, 1.0);
        if (nv - *v).abs() > 1e-6 {
            *v = nv;
            changed = true;
        }
    }
    let p = ui.painter();
    let col = if resp.hovered() || resp.dragged() { gray(0xd0) } else { gray(0xa8) };
    // 300° track with a gap at the bottom; value arc from 12 o'clock.
    let ang = |t: f32| std::f32::consts::PI * (-0.5 + t * 5.0 / 6.0);
    let pts = |a0: f32, a1: f32| -> Vec<egui::Pos2> {
        let n = 24;
        (0..=n)
            .map(|i| {
                let a = a0 + (a1 - a0) * i as f32 / n as f32;
                c + vec2(a.cos(), a.sin()) * r
            })
            .collect()
    };
    p.add(egui::Shape::line(pts(ang(-1.0), ang(1.0)), Stroke::new(1.6, gray(0x22))));
    if v.abs() > 0.005 {
        p.add(egui::Shape::line(pts(ang(0.0), ang(*v)), Stroke::new(1.6, col)));
    }
    let a = ang(*v);
    p.line_segment([c, c + vec2(a.cos(), a.sin()) * (r - 1.0)], Stroke::new(1.6, col));
    p.add(egui::Shape::convex_polygon(
        vec![c + vec2(0.0, -r - 1.5), c + vec2(-3.0, -r - 5.5), c + vec2(3.0, -r - 5.5)],
        gray(0x22),
        Stroke::NONE,
    ));
    (resp.on_hover_text(format!("Pan {} · drag · double-click: centre", pan_label(*v))), changed)
}

/// Live volume pill above the pan knob: dB value, drag to change.
pub fn vol_pill(ui: &mut egui::Ui, rect: Rect, id: egui::Id, db: &mut f32) -> (egui::Response, bool) {
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
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(5), if resp.hovered() { gray(0x2a) } else { super::theme::off_fill() });
    p.rect_stroke(rect, CornerRadius::same(5), Stroke::new(1.0, super::theme::outline()), egui::StrokeKind::Inside);
    let t = if *db <= METER_FLOOR_DB {
        "-∞".to_string()
    } else if db.abs() < 0.05 {
        "0".into()
    } else {
        format!("{db:.1}")
    };
    p.text(rect.center(), egui::Align2::CENTER_CENTER, t, super::theme::font(super::theme::FS_SMALL), super::theme::text());
    (resp.on_hover_text("Track volume (dB) · drag · double-click: 0 dB"), changed)
}

/// Live meter / fader scale (fraction from the top): near-linear to −36 dB,
/// compressed below, measured from Live's mixer.
const METER_PTS: [(f32, f32); 8] =
    [(6.0, 0.0), (0.0, 0.09), (-12.0, 0.30), (-24.0, 0.48), (-36.0, 0.67), (-48.0, 0.81), (-60.0, 0.93), (-70.0, 1.0)];

pub fn meter_frac(db: f32) -> f32 {
    let db = db.clamp(-70.0, 6.0);
    for w in METER_PTS.windows(2) {
        let ((d0, f0), (d1, f1)) = (w[0], w[1]);
        if db >= d1 {
            return f0 + (f1 - f0) * (d0 - db) / (d0 - d1);
        }
    }
    1.0
}

pub fn meter_db(frac: f32) -> f32 {
    let f = frac.clamp(0.0, 1.0);
    for w in METER_PTS.windows(2) {
        let ((d0, f0), (d1, f1)) = (w[0], w[1]);
        if f <= f1 {
            return d0 + (d1 - d0) * (f - f0) / (f1 - f0);
        }
    }
    -70.0
}

/// Live channel fader: ▶ handle left of a stereo meter, dB scale (0…60) at the right.
pub fn fader_meter(ui: &mut egui::Ui, rect: Rect, id: egui::Id, db: &mut f32, levels: [f32; 2]) -> (egui::Response, bool) {
    let meter = Rect::from_min_max(pos2(rect.min.x + 10.0, rect.min.y + 2.0), pos2(rect.min.x + 20.0, rect.max.y - 2.0));
    let resp = ui.interact(Rect::from_min_max(rect.min, pos2(meter.max.x + 2.0, rect.max.y)), id, Sense::click_and_drag());
    let mut changed = false;
    if resp.double_clicked() {
        *db = 0.0;
        changed = true;
    } else if resp.dragged() {
        let k = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
        let f = (meter_frac(*db) + resp.drag_delta().y * k / meter.height()).clamp(0.0, 1.0);
        let nd = meter_db(f);
        let nd = if nd <= METER_FLOOR_DB { METER_FLOOR_DB } else { nd };
        if (nd - *db).abs() > 1e-5 {
            *db = nd;
            changed = true;
        }
    }
    let p = ui.painter();
    p.rect_filled(meter, CornerRadius::ZERO, Color32::from_gray(0x14));
    p.rect_stroke(meter, CornerRadius::ZERO, Stroke::new(1.0, super::theme::outline()), egui::StrokeKind::Outside);
    let y_of = |d: f32| meter.min.y + meter_frac(d) * meter.height();
    for (i, l) in levels.iter().enumerate() {
        let ldb = if *l <= 1e-6 { METER_FLOOR_DB } else { (20.0 * l.log10()).max(METER_FLOOR_DB) };
        if ldb > METER_FLOOR_DB {
            let x0 = meter.min.x + 1.0 + i as f32 * 4.0;
            let col = if ldb > 0.0 {
                Color32::from_rgb(0xff, 0x4a, 0x3a)
            } else if ldb > -6.0 {
                Color32::from_rgb(0xff, 0xd0, 0x40)
            } else {
                Color32::from_rgb(0x5f, 0xd2, 0x5f)
            };
            p.rect_filled(Rect::from_min_max(pos2(x0, y_of(ldb)), pos2(x0 + 3.5, meter.max.y - 1.0)), CornerRadius::ZERO, col);
        }
    }
    // Scale: 0 / 12 / 24 / 36 / 48 / 60 dB below full scale.
    for d in [0.0f32, -12.0, -24.0, -36.0, -48.0, -60.0] {
        let y = y_of(d);
        p.line_segment([pos2(meter.max.x + 2.0, y), pos2(meter.max.x + 5.0, y)], Stroke::new(1.0, gray(0x70)));
        p.text(
            pos2(meter.max.x + 8.0, y),
            egui::Align2::LEFT_CENTER,
            format!("{}", -d as i32),
            super::theme::font(super::theme::FS_SMALL),
            super::theme::text(),
        );
    }
    let y = y_of(*db);
    let hc = if resp.hovered() || resp.dragged() { gray(0xe0) } else { gray(0xb5) };
    p.add(egui::Shape::convex_polygon(
        vec![pos2(meter.min.x - 1.0, y), pos2(meter.min.x - 9.0, y - 5.0), pos2(meter.min.x - 9.0, y + 5.0)],
        hc,
        Stroke::NONE,
    ));
    p.line_segment([pos2(meter.min.x, y), pos2(meter.max.x, y)], Stroke::new(1.0, hc));
    let txt = if *db <= METER_FLOOR_DB { "-inf".to_string() } else { format!("{db:.1} dB") };
    (resp.on_hover_text(format!("Volume {txt} · drag · double-click: 0 dB")), changed)
}

#[cfg(test)]
mod tests {
    #[test]
    fn meter_scale_roundtrip() {
        for d in [-60.0f32, -40.0, -12.5, -3.0, 0.0, 4.0] {
            assert!((super::meter_db(super::meter_frac(d)) - d).abs() < 1e-3);
        }
        assert!(super::meter_frac(0.0) < super::meter_frac(-12.0));
    }
}
