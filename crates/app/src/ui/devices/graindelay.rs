//! Grain Delay (Live 12): parameter boxes with the Y-axis selectors on the
//! left, 16th-note grid / Sync / offset below them, the X-Y field and the
//! X-axis selector row. Geometry measured from Live 12 at 100 % (l/t =
//! device left/top; text positions are baselines).

use super::dynamics::tbutton;
use super::redux::{arrow_col, bip_arrow};
use super::*;
use daw_engine::graindelay::*;
use daw_project::devices::params3::gdp;

pub const WIDTH: f32 = 378.0;
/// X-Y field and its handle inset.
const XY: (f32, f32, f32, f32) = (110.0, 22.0, 375.0, 167.0);
const INSET: f32 = 6.5;

fn set(cx: &mut Ctx, f: impl FnOnce(&mut GrainDelayParams)) {
    if let FxParams::GrainDelay(p) = &mut cx.params {
        f(p);
    }
}

fn r(l: f32, t: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
    Rect::from_min_max(pos2(l + x0, t + y0), pos2(l + x1, t + y1))
}

pub(super) fn draw(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, rect: Rect) {
    let FxParams::GrainDelay(g) = cx.params else { return };
    let (l, t) = (rect.left(), rect.top());

    // Left column: values and Y-axis selectors.
    let rows: [(u8, String); 6] = [
        (gdp::SPRAY, GrainDelayParams::ms_label(g.spray)),
        (gdp::FREQ, g.freq_label()),
        (gdp::PITCH, format!("{:.2}", g.pitch)),
        (gdp::RAND, format!("{:.2}", g.rand)),
        (gdp::FEEDBACK, GrainDelayParams::pct_label(g.feedback)),
        (gdp::DRY_WET, GrainDelayParams::pct_label(g.dry_wet)),
    ];
    for (i, (k, shown)) in rows.iter().enumerate() {
        let y = 22.0 + 16.0 * i as f32;
        let vr = r(l, t, 5.0, y, 46.0, y + 13.0);
        if *k == gdp::PITCH {
            value_box_ex(cx, ui, p, vr, *k, shown, true, false, box_fill(cx.on));
            bip_arrow(p, vr, arrow_col(cx, true, g.pitch.abs() > 1e-3));
        } else {
            value_box(cx, ui, p, vr, *k, shown, true);
        }
        cx.salt = i as u32;
        if tbutton(cx, ui, p, r(l, t, 51.0, y, 106.0, y + 13.0), gdp::Y_AXIS, GD_Y[i], g.y_axis as usize == i, true) {
            set(cx, |q| q.y_axis = i as u8);
        }
        cx.salt = 0;
    }

    // Time: 16th grid (or free ms), Sync, offset.
    if g.sync {
        for (i, n) in GD_SIXTEENTHS.iter().enumerate() {
            let x = 5.0 + 26.0 * (i % 4) as f32;
            let y = 120.0 + 16.0 * (i / 4) as f32;
            cx.salt = 100 + i as u32;
            if tbutton(cx, ui, p, r(l, t, x, y, x + 23.0, y + 13.0), gdp::TIME, &n.to_string(), g.six as usize == i, true) {
                set(cx, |q| q.six = i as u8);
            }
        }
        cx.salt = 0;
    } else {
        value_box(cx, ui, p, r(l, t, 5.0, 120.0, 106.0, 133.0), gdp::TIME, &GrainDelayParams::ms_label(g.ms), true);
    }
    if tbutton(cx, ui, p, r(l, t, 5.0, 152.0, 37.0, 165.0), gdp::SYNC, "Sync", g.sync, true) {
        set(cx, |q| q.sync = !q.sync);
    }
    let or = r(l, t, 57.0, 152.0, 106.0, 165.0);
    value_box_ex(cx, ui, p, or, gdp::OFFSET, &format!("{:.2} %", g.offset * 100.0), g.sync, false, box_fill(cx.on));
    bip_arrow(p, or, arrow_col(cx, g.sync, g.offset.abs() > 1e-4));

    // X-Y field.
    let field = r(l, t, XY.0, XY.1, XY.2, XY.3);
    p.rect_filled(field, CornerRadius::same(2), gray(0x18));
    let grid = gray(0x3e);
    for i in 1..10 {
        let x = (l + XY.0 + (XY.2 - XY.0) * i as f32 / 10.0).floor();
        quad(p, Rect::from_min_max(pos2(x, t + XY.1), pos2(x + 1.0, t + XY.3)), grid);
        let y = (t + XY.1 + (XY.3 - XY.1) * i as f32 / 10.0).floor();
        quad(p, Rect::from_min_max(pos2(l + XY.0, y), pos2(l + XY.2, y + 1.0)), grid);
    }
    let kx = gdp::X_IDS[(g.x_axis as usize).min(5)];
    let ky = gdp::Y_IDS[(g.y_axis as usize).min(5)];
    let nx = norm(&cx.params, kx).unwrap_or(0.0);
    let ny = norm(&cx.params, ky).unwrap_or(0.0);
    let (w, h) = (XY.2 - XY.0 - 2.0 * INSET, XY.3 - XY.1 - 2.0 * INSET);
    let hc = pos2(l + XY.0 + INSET + nx * w, t + XY.1 + INSET + (1.0 - ny) * h);
    p.with_clip_rect(field.intersect(p.clip_rect())).circle_stroke(hc, 5.8, Stroke::new(1.6, if cx.on { ORANGE } else { gray(0x75) }));
    cx.salt = 1;
    let resp = cx.sense(ui, field, kx, Sense::click_and_drag());
    cx.salt = 0;
    if resp.drag_started() || resp.clicked() {
        cx.focus(kx);
    }
    if resp.dragged() {
        let d = resp.drag_delta();
        if d != egui::Vec2::ZERO {
            cx.undo_step();
            set_norm(&mut cx.params, kx, nx + d.x / w);
            set_norm(&mut cx.params, ky, ny - d.y / h);
        }
    }
    if resp.double_clicked() {
        cx.undo_step();
        reset(&mut cx.params, kx);
        reset(&mut cx.params, ky);
    }
    cx.param_menu(&resp, kx);

    // X-axis selectors.
    for (i, name) in GD_X.iter().enumerate() {
        let x = 5.0 + 62.0 * i as f32;
        cx.salt = 200 + i as u32;
        if tbutton(cx, ui, p, r(l, t, x, 170.0, x + 59.0, 183.0), gdp::X_AXIS, name, g.x_axis as usize == i, true) {
            set(cx, |q| q.x_axis = i as u8);
        }
    }
    cx.salt = 0;
}

// ---- parameters --------------------------------------------------------------

pub(super) fn reset(p: &mut FxParams, k: u8) {
    let d = FxParams::GrainDelay(GrainDelayParams::default());
    if let Some(v) = norm(&d, k) {
        set_norm(p, k, v);
    }
}

pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    let FxParams::GrainDelay(g) = p else { return };
    match k {
        gdp::TIME if g.sync => g.six = (g.six as i32 + dir as i32).clamp(0, 7) as u8,
        gdp::PITCH => g.pitch = ((g.pitch * 100.0).round() + dir * 100.0).clamp(GD_PITCH.0 * 100.0, GD_PITCH.1 * 100.0) / 100.0,
        gdp::RAND => g.rand = (g.rand.round() + dir).clamp(GD_RAND.0, GD_RAND.1),
        gdp::FEEDBACK | gdp::DRY_WET => {
            let v = if k == gdp::DRY_WET { g.dry_wet } else { g.feedback };
            let nv = ((v * 100.0).round() + dir).clamp(0.0, if k == gdp::DRY_WET { 100.0 } else { GD_FB_MAX * 100.0 }) / 100.0;
            if k == gdp::DRY_WET {
                g.dry_wet = nv;
            } else {
                g.feedback = nv;
            }
        }
        _ => {
            if let Some(v) = norm(p, k) {
                set_norm(p, k, v + dir / 200.0);
            }
        }
    }
}

pub(super) fn name(k: u8) -> &'static str {
    match k {
        gdp::TIME => "Delay Time",
        gdp::SPRAY => "Spray",
        gdp::FREQ => "Frequency",
        gdp::PITCH => "Pitch",
        gdp::RAND => "Random Pitch",
        gdp::FEEDBACK => "Feedback",
        gdp::DRY_WET => "DryWet",
        gdp::OFFSET => "Beat Swing",
        gdp::SYNC => "Sync",
        gdp::X_AXIS => "X-Axis",
        gdp::Y_AXIS => "Y-Axis",
        _ => "",
    }
}
