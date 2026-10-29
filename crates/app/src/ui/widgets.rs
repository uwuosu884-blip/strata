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
/// `max` = coarsest fixed step offered (beats).
pub fn grid_menu(ui: &mut egui::Ui, mode: &mut crate::arr_ops::GridMode, max: f64) {
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
        for (g, name) in GridMode::FIXED.into_iter().filter(|(g, _)| *g <= max) {
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
        // Live: a plain 7 px disc, light when off, dark on the red when armed.
        let r = (rect.height() * 0.27).min(3.5);
        p.circle_filled(rect.center(), r, if on { Color32::from_gray(0x16) } else { super::theme::text() });
    } else {
        let fg = if on { Color32::from_gray(0x16) } else { super::theme::text() };
        p.text(rect.center(), egui::Align2::CENTER_CENTER, text, super::theme::font(super::theme::FS), fg);
    }
    resp.on_hover_text(tooltip)
}

/// Black or white text, whichever reads better on `bg`.
/// Live keeps it a touch soft: a dark tint of the colour, or light grey.
pub fn text_on(bg: Color32) -> Color32 {
    let l = 0.299 * bg.r() as f32 + 0.587 * bg.g() as f32 + 0.114 * bg.b() as f32;
    if l > 140.0 { shade(bg, 0.25) } else { gray(0xd2) }
}

/// Live round knob (bipolar −1…1): arc from the top, drag up/down (Shift = fine),
/// double-click = centre.
pub fn knob(ui: &mut egui::Ui, c: egui::Pos2, r: f32, id: egui::Id, v: &mut f32) -> (egui::Response, bool) {
    knob_ex(ui, c, r, id, v, true)
}

/// [`knob`] with the centre ▼ optional (Live's Cue knob has none).
pub fn knob_ex(ui: &mut egui::Ui, c: egui::Pos2, r: f32, id: egui::Id, v: &mut f32, mark: bool) -> (egui::Response, bool) {
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
    // Live: dark 2 px ring and pointer; the value arc is drawn in the text grey.
    let col = if resp.hovered() || resp.dragged() { gray(0xd0) } else { gray(0xa8) };
    let ptr = if resp.hovered() || resp.dragged() { gray(0x60) } else { gray(0x18) };
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
    p.add(egui::Shape::line(pts(ang(-1.0), ang(1.0)), Stroke::new(2.0, gray(0x11))));
    if v.abs() > 0.005 {
        p.add(egui::Shape::line(pts(ang(0.0), ang(*v)), Stroke::new(2.0, col)));
    }
    let a = ang(*v);
    p.line_segment([c, c + vec2(a.cos(), a.sin()) * (r - 1.0)], Stroke::new(2.0, ptr));
    // Centre mark: 7 px ▼ above the ring.
    if mark {
        p.add(egui::Shape::convex_polygon(
            vec![c + vec2(0.0, -r - 1.0), c + vec2(-3.5, -r - 4.5), c + vec2(3.5, -r - 4.5)],
            gray(0x11),
            Stroke::NONE,
        ));
    }
    (resp.on_hover_text(format!("Pan {} · drag · double-click: centre", pan_label(*v))), changed)
}

/// Send level ↔ knob position (0 = −inf at 7 o'clock, 1 = 0 dB at 5 o'clock),
/// on the meter scale so the knob feels like Live's.
pub fn send_pos(db: f32) -> f32 {
    if db <= daw_project::SEND_OFF_DB + 1e-3 {
        return 0.0;
    }
    let f0 = meter_frac(0.0);
    ((1.0 - meter_frac(db)) / (1.0 - f0)).clamp(0.0, 1.0)
}
pub fn send_db(pos: f32) -> f32 {
    if pos <= 1e-3 {
        return daw_project::SEND_OFF_DB;
    }
    let f0 = meter_frac(0.0);
    meter_db(1.0 - pos.clamp(0.0, 1.0) * (1.0 - f0)).clamp(daw_project::SEND_OFF_DB, 0.0)
}

/// Live send knob (unipolar): dark 300° track, light value arc from the
/// minimum, dark pointer; `enabled = false` draws it greyed (no interaction).
pub fn send_knob(ui: &mut egui::Ui, c: egui::Pos2, r: f32, id: egui::Id, db: &mut f32, enabled: bool) -> (egui::Response, bool) {
    let rect = Rect::from_center_size(c, vec2(r * 2.0 + 4.0, r * 2.0 + 4.0));
    let resp = ui.interact(rect, id, if enabled { Sense::click_and_drag() } else { Sense::hover() });
    let mut changed = false;
    let mut pos = send_pos(*db);
    let d = if enabled { endless_drag(ui, &resp) } else { Vec2::ZERO };
    if resp.double_clicked() {
        pos = if pos > 0.0 { 0.0 } else { 1.0 };
        changed = true;
    } else if d != Vec2::ZERO {
        let k = if ui.input(|i| i.modifiers.shift) { 0.001 } else { 0.006 };
        let np = (pos - (d.y - d.x) * k).clamp(0.0, 1.0);
        if (np - pos).abs() > 1e-6 {
            pos = np;
            changed = true;
        }
    }
    if changed {
        *db = send_db(pos);
    }
    let p = ui.painter();
    let hot = enabled && (resp.hovered() || resp.dragged());
    let (track, arc, ptr) = if enabled {
        (gray(0x11), if hot { gray(0xd0) } else { gray(0xa8) }, if hot { gray(0x60) } else { gray(0x18) })
    } else {
        (gray(0x2a), gray(0x5a), gray(0x2a))
    };
    // 300° with the gap at the bottom: t = 0 → 7 o'clock (−inf), 1 → 5 o'clock.
    let ang = |t: f32| std::f32::consts::PI * (2.0 / 3.0 + t * 5.0 / 3.0);
    let pts = |a0: f32, a1: f32| -> Vec<egui::Pos2> {
        (0..=24)
            .map(|i| {
                let a = a0 + (a1 - a0) * i as f32 / 24.0;
                c + vec2(a.cos(), a.sin()) * r
            })
            .collect()
    };
    p.add(egui::Shape::line(pts(ang(0.0), ang(1.0)), Stroke::new(2.0, track)));
    if pos > 0.005 {
        p.add(egui::Shape::line(pts(ang(0.0), ang(pos)), Stroke::new(2.0, arc)));
    }
    let a = ang(pos);
    p.line_segment([c, c + vec2(a.cos(), a.sin()) * (r - 1.0)], Stroke::new(2.0, ptr));
    let tip = if *db <= daw_project::SEND_OFF_DB { "-inf dB".to_string() } else { format!("{:.1} dB", db) };
    (resp.on_hover_text(format!("Send {tip} · drag · double-click: 0 dB / off")), changed)
}

/// Live meter / fader scale (fraction from the top): near-linear to −36 dB,
/// compressed below, measured from Live's mixer.
const METER_PTS: [(f32, f32); 8] =
    [(6.0, 0.0), (0.0, 0.112), (-12.0, 0.306), (-24.0, 0.493), (-36.0, 0.68), (-48.0, 0.82), (-60.0, 0.93), (-70.0, 1.0)];

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

const M_GREEN: Color32 = Color32::from_rgb(0x5f, 0xd2, 0x5f);
const M_YELLOW: Color32 = Color32::from_rgb(0xff, 0xd0, 0x40);
const M_RED: Color32 = Color32::from_rgb(0xff, 0x4a, 0x3a);
/// Unlit meter / clip cell.
pub const M_OFF: Color32 = Color32::from_gray(0x18);

/// One vertical peak bar (`peak` = linear amplitude) on the Live meter scale:
/// green, yellow above −6 dB, red above 0 dB.
pub fn peak_bar(p: &egui::Painter, r: Rect, peak: f32) {
    p.rect_filled(r, CornerRadius::ZERO, M_OFF);
    if peak <= 1e-6 {
        return;
    }
    let db = 20.0 * peak.log10();
    if db <= METER_FLOOR_DB {
        return;
    }
    let y_of = |d: f32| r.min.y + meter_frac(d) * r.height();
    let top = y_of(db).round();
    for (lo, hi, c) in [(r.max.y, y_of(-6.0), M_GREEN), (y_of(-6.0), y_of(0.0), M_YELLOW), (y_of(0.0), r.min.y, M_RED)] {
        let (y0, y1) = (hi.round().max(top), lo.round());
        if y1 > y0 {
            p.rect_filled(Rect::from_x_y_ranges(r.x_range(), y0..=y1), CornerRadius::ZERO, c);
        }
    }
}

/// Live track meter: two 3 px bars (1 px apart) under an 8 px clip cell each.
/// `r` is the full column (7 px wide).
pub fn track_meter(p: &egui::Painter, r: Rect, peaks: [f32; 2], clip: [bool; 2]) {
    for i in 0..2 {
        let x0 = r.min.x + i as f32 * 4.0;
        let cell = Rect::from_min_max(pos2(x0, r.min.y), pos2(x0 + 3.0, r.min.y + 8.0));
        p.rect_filled(cell, CornerRadius::ZERO, if clip[i] { M_RED } else { M_OFF });
        if r.height() > 12.0 {
            peak_bar(p, Rect::from_min_max(pos2(x0, r.min.y + 9.0), pos2(x0 + 3.0, r.max.y)), peaks[i]);
        }
    }
}

/// Small input meter inside a chooser: 9 px tall, 1 px clip row on top.
/// One 5 px bar (mono) or two 2 px bars (stereo).
pub fn input_meter(p: &egui::Painter, r: Rect, peaks: &[(f32, bool)]) {
    let w = if peaks.len() > 1 { 2.0 } else { 5.0 };
    for (i, &(pk, clip)) in peaks.iter().enumerate() {
        let x0 = r.min.x + i as f32 * (w + 1.0);
        p.rect_filled(
            Rect::from_min_max(pos2(x0, r.min.y), pos2(x0 + w, r.min.y + 1.0)),
            CornerRadius::ZERO,
            if clip { M_RED } else { M_OFF },
        );
        peak_bar(p, Rect::from_min_max(pos2(x0, r.min.y + 1.0), pos2(x0 + w, r.max.y)), pk);
    }
}

/// MIDI activity: a 3 px column of three 3 px cells lit bottom-up by `level` 0..1.
pub fn midi_meter(p: &egui::Painter, r: Rect, level: f32) {
    let lit = (level * 3.0).ceil() as i32;
    for i in 0..3 {
        let y1 = r.max.y - i as f32 * 3.0;
        let c = if i < lit { Color32::from_rgb(0xff, 0xd0, 0x40) } else { M_OFF };
        p.rect_filled(Rect::from_min_max(pos2(r.min.x, y1 - 3.0), pos2(r.min.x + 3.0, y1)), CornerRadius::ZERO, c);
    }
}

/// Live channel fader (Session mixer, measured at 100 %): an 11 px ▶ handle,
/// a 13 px meter (two bars split by a 1 px line) spanning `rect`'s height and
/// the dB scale (0…60). `rect.min.x` = handle's back edge. `ticks_left`: the
/// Main column draws its ticks between handle and meter.
pub fn fader_meter(
    ui: &mut egui::Ui,
    rect: Rect,
    id: egui::Id,
    db: &mut f32,
    levels: [f32; 2],
    ticks_left: bool,
) -> (egui::Response, bool) {
    let meter = Rect::from_min_max(pos2(rect.min.x + 12.0, rect.min.y), pos2(rect.min.x + 25.0, rect.max.y));
    let resp = ui.interact(Rect::from_min_max(rect.min - vec2(0.0, 6.0), pos2(meter.max.x, rect.max.y)), id, Sense::click_and_drag());
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
    p.rect_filled(meter, CornerRadius::ZERO, M_OFF);
    let mid = meter.min.x + 6.0;
    let y_of = |d: f32| (meter.min.y + meter_frac(d) * meter.height()).round();
    for (i, l) in levels.iter().enumerate() {
        let bar = if i == 0 {
            Rect::from_min_max(meter.min, pos2(mid, meter.max.y))
        } else {
            Rect::from_min_max(pos2(mid + 1.0, meter.min.y), meter.max)
        };
        peak_bar(p, bar, *l);
    }
    p.rect_filled(Rect::from_min_max(pos2(mid, meter.min.y), pos2(mid + 1.0, meter.max.y)), CornerRadius::ZERO, gray(0x3e));
    // Scale: 0 / 12 / 24 / 36 / 48 / 60 dB below full scale.
    let (tick_x, label_x) = if ticks_left { (meter.min.x - 12.0, meter.max.x + 4.0) } else { (meter.max.x + 3.0, meter.max.x + 7.0) };
    // Minor ticks every 6 dB (and -3), labels every 12 (Live).
    for d in [-3.0f32, -6.0, -18.0, -30.0, -42.0, -54.0, -66.0] {
        let y = y_of(d) + 0.5;
        p.line_segment([pos2(tick_x, y), pos2(tick_x + 2.0, y)], Stroke::new(1.0, gray(0x5a)));
    }
    for d in [0.0f32, -12.0, -24.0, -36.0, -48.0, -60.0] {
        let y = y_of(d) + 0.5;
        p.line_segment([pos2(tick_x, y), pos2(tick_x + 2.0, y)], Stroke::new(1.0, gray(0x7e)));
        p.text(
            pos2(label_x, y),
            egui::Align2::LEFT_CENTER,
            format!("{}", -d as i32),
            super::theme::font(super::theme::FS_SMALL),
            super::theme::text(),
        );
    }
    let y = y_of(*db) + 0.5;
    let hc = if resp.hovered() || resp.dragged() { gray(0xe0) } else { gray(0xb5) };
    p.add(egui::Shape::convex_polygon(
        vec![pos2(meter.min.x - 1.0, y), pos2(meter.min.x - 12.0, y - 6.5), pos2(meter.min.x - 12.0, y + 6.5)],
        hc,
        Stroke::NONE,
    ));
    p.line_segment([pos2(meter.min.x + 1.0, y), pos2(meter.max.x - 1.0, y)], Stroke::new(1.0, gray(0x86)));
    let txt = if *db <= METER_FLOOR_DB { "-inf".to_string() } else { format!("{db:.1} dB") };
    (resp.on_hover_text(format!("Volume {txt} · drag · double-click: 0 dB")), changed)
}

/// Live peak-level pill above the pan knob: highest level since the last
/// click ("-∞" while silent). Click = reset.
pub fn peak_pill(ui: &mut egui::Ui, rect: Rect, id: egui::Id, levels: [f32; 2]) {
    let resp = ui.interact(rect, id, Sense::click());
    let now = levels[0].max(levels[1]);
    let hold = ui.data(|d| d.get_temp::<f32>(id)).unwrap_or(0.0);
    let hold = if resp.clicked() { 0.0 } else { hold.max(now) };
    ui.data_mut(|d| d.insert_temp(id, hold));
    let p = ui.painter();
    let r = CornerRadius::same((rect.height() / 2.0) as u8);
    p.rect_filled(rect, r, sep_fill());
    p.rect_stroke(rect, r, Stroke::new(1.0, super::theme::outline()), egui::StrokeKind::Inside);
    let db = if hold > 1e-6 { 20.0 * hold.log10() } else { f32::NEG_INFINITY };
    let t = if db <= METER_FLOOR_DB { "-∞".to_string() } else { format!("{db:.1}") };
    let col = if db > 0.0 { Color32::from_rgb(0xff, 0x4a, 0x3a) } else { super::theme::text() };
    p.text(rect.center(), egui::Align2::CENTER_CENTER, t, super::theme::font(super::theme::FS), col);
    resp.on_hover_text("Peak level · click: reset");
}

fn sep_fill() -> Color32 {
    super::theme::sep()
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
