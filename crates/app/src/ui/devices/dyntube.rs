//! Dynamic Tube (Live 12): Dry/Wet / Output / Drive column, the dark tube
//! display (characteristic curve + output meters) with the A / B / C model
//! buttons, Tone / Bias, and the Envelope / Attack / Release column.
//! Geometry measured from Live 12 at 100 % (l/t = device left/top; text
//! positions are baselines).

use super::dynamics::{KN, knob_lv, tbutton};
use super::fx4::*;
use super::*;
use daw_project::devices::dev_tap;
use daw_project::devices::dyntube::*;
use daw_project::devices::params5::dtp;

pub const WIDTH: f32 = 196.0;

fn set(cx: &mut Ctx, f: impl FnOnce(&mut DynTubeParams)) {
    if let FxParams::DynamicTube(d) = &mut cx.params {
        f(d);
        cx.undo_step();
    }
}

fn pct1(x: f32) -> String {
    format!("{:.1} %", x * 100.0)
}

/// Display characteristic of each model (x 0..1 → y, 1 = top): A is flat up
/// to its knee, B dips slightly before rising, C bends early and curves.
/// Bias moves the knee towards zero.
pub fn disp(tube: u8, u: f32, bias: f32) -> f32 {
    let (k0, dip, pw) = match tube {
        0 => (0.667, 0.0, 1.0),
        1 => (0.625, 0.07, 1.12),
        _ => (0.354, 0.03, 1.8),
    };
    let k = k0 * (1.0 - 0.9 * bias.clamp(0.0, 1.0));
    if u < k { -dip * (std::f32::consts::PI * u / k).sin() } else { ((u - k) / (1.0 - k)).powf(pw) }
}

#[derive(Clone, Copy, Default)]
struct Meters {
    lv: [f32; 2],
    hold: [f32; 2],
    hold_t: [f32; 2],
}

/// Meter y for a level (+6 dB at the top line, 1 px per dB).
fn meter_y(lv: f32) -> f32 {
    let db = 20.0 * lv.max(1e-6).log10();
    (30.0 + (6.0 - db)).clamp(30.0, 91.0)
}

pub(super) fn draw(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect) {
    let FxParams::DynamicTube(d) = cx.params else { return };
    let (l, t) = (r.left(), r.top());
    let on = cx.on;

    // Left column.
    knob_lv(cx, ui, p, l + 26.5, t + 30.0, dtp::DRY_WET, "Dry/Wet", &format!("{:.0} %", d.dry_wet * 100.0), 0.0, KN, 14.5, 36.0, true);
    knob_lv(cx, ui, p, l + 26.5, t + 79.0, dtp::OUTPUT, "Output", &db2(d.output), 0.5, KN, 20.5, 42.0, true);
    marker(p, l + 26.5, t + 99.5, marker_col(cx, true, d.output.abs() > 0.005));
    knob_lv(cx, ui, p, l + 26.5, t + 136.0, dtp::DRIVE, "Drive", &db2(d.drive), 0.5, KN, 20.5, 42.0, true);
    marker(p, l + 26.5, t + 156.5, marker_col(cx, true, d.drive.abs() > 0.005));
    knob_lv(cx, ui, p, l + 74.5, t + 136.0, dtp::TONE, "Tone", &format!("{:.2}", d.tone), 0.5, KN, 20.5, 42.0, true);
    marker(p, l + 74.5, t + 156.5, marker_col(cx, true, d.tone.abs() > 0.005));
    knob_lv(cx, ui, p, l + 117.5, t + 136.0, dtp::BIAS, "Bias", &pct1(d.bias), 0.0, KN, 22.5, 44.0, true);

    // Right column after the divider.
    px(p, l, t, 142.0, 23.0, 143.0, 184.0, gray(0x24));
    knob_lv(cx, ui, p, l + 170.5, t + 31.0, dtp::ENVELOPE, "Envelope", &pct1(d.envelope), 0.5, KN, 20.5, 42.0, true);
    marker(p, l + 170.5, t + 51.5, marker_col(cx, true, d.envelope.abs() > 0.0005));
    knob_lv(cx, ui, p, l + 170.5, t + 91.0, dtp::ATTACK, "Attack", &ms(d.attack), 0.0, KN, 14.5, 36.0, true);
    knob_lv(cx, ui, p, l + 170.5, t + 144.0, dtp::RELEASE, "Release", &ms(d.release), 0.0, KN, 14.5, 36.0, true);

    // Display panel.
    p.rect_filled(rr(l, t, 54.0, 22.0, 137.0, 99.0), CornerRadius::same(2), gray(0x18));
    let g3 = gray(0x3e);
    for (x0, y0, x1, y1) in [
        (61.0, 29.0, 112.0, 30.0),
        (61.0, 91.0, 112.0, 92.0),
        (61.0, 29.0, 62.0, 92.0),
        (111.0, 29.0, 112.0, 92.0),
        (94.0, 29.0, 95.0, 92.0),
        (61.0, 80.0, 112.0, 81.0),
        (119.0, 29.0, 130.0, 30.0),
        (119.0, 91.0, 130.0, 92.0),
        (119.0, 29.0, 120.0, 92.0),
        (124.0, 29.0, 125.0, 92.0),
        (129.0, 29.0, 130.0, 92.0),
    ] {
        px(p, l, t, x0, y0, x1, y1, g3);
    }
    // Characteristic curve.
    let curve_col = if on { TEAL } else { gray(0x75) };
    let pts: Vec<Pos2> =
        (0..=96).map(|i| i as f32 / 96.0).map(|u| pos2(l + 62.0 + u * 48.0, t + 80.5 - disp(d.tube, u, d.bias) * 50.0)).collect();
    p.with_clip_rect(rr(l, t, 62.0, 30.0, 111.0, 91.0)).add(Shape::line(pts, Stroke::new(1.4, curve_col)));

    // Output meters with peak hold.
    let tap = dev_tap(cx.dev.id);
    let mid = egui::Id::unique(("dt_meter", cx.dev.id));
    let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.1);
    let mut m: Meters = ui.data(|dd| dd.get_temp(mid)).unwrap_or_default();
    let fall = 10f32.powf(-24.0 * dt / 20.0);
    #[allow(clippy::needless_range_loop)]
    for c in 0..2 {
        m.lv[c] = tap.take(TAP_OUT[c], 0.0).max(m.lv[c] * fall);
        if m.lv[c] >= m.hold[c] {
            (m.hold[c], m.hold_t[c]) = (m.lv[c], 0.0);
        } else {
            m.hold_t[c] += dt;
            if m.hold_t[c] > 1.5 {
                m.hold[c] = m.lv[c];
            }
        }
    }
    if on && m.lv.iter().chain(&m.hold).any(|v| *v > 1e-4) {
        cx.app.animating = true;
    }
    ui.data_mut(|dd| dd.insert_temp(mid, m));
    let (fill, top) =
        if on { (Color32::from_rgb(0xff, 0xa4, 0x18), Color32::from_rgb(0x99, 0x66, 0x18)) } else { (gray(0x75), gray(0x4a)) };
    for (c, x0) in [120.0, 125.0].into_iter().enumerate() {
        px(p, l, t, x0, 66.0, x0 + 4.0, 67.0, panel());
        if m.lv[c] > 1e-4 {
            let y = meter_y(m.lv[c]).round();
            if y < 91.0 {
                px(p, l, t, x0, y, x0 + 4.0, y + 1.0, top);
                px(p, l, t, x0, y + 1.0, x0 + 4.0, 91.0, fill);
            }
        }
        if m.hold[c] > 1e-4 {
            let y = meter_y(m.hold[c]).round();
            if y < 91.0 {
                px(p, l, t, x0, y, x0 + 4.0, y + 1.0, fill);
            }
        }
    }

    // Tube models.
    for (i, x) in [70.0, 88.0, 106.0].into_iter().enumerate() {
        cx.salt = i as u32;
        if tbutton(cx, ui, p, rr(l, t, x, 105.0, x + 15.0, 118.0), dtp::TUBE, DT_TUBES[i], d.tube == i as u8, true) {
            set(cx, |q| q.tube = i as u8);
        }
        cx.salt = 0;
    }
}

// ---- parameters --------------------------------------------------------------

pub(super) fn reset(p: &mut FxParams, k: u8) {
    let d = FxParams::DynamicTube(DynTubeParams::default());
    if let Some(v) = norm(&d, k) {
        set_norm(p, k, v);
    }
}

pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    if let Some(v) = norm(p, k) {
        set_norm(p, k, ((v * 100.0).round() + dir) / 100.0);
    }
}

pub(super) fn name(k: u8) -> &'static str {
    match k {
        dtp::DRIVE => "Drive",
        dtp::TONE => "Tone",
        dtp::BIAS => "Bias",
        dtp::ENVELOPE => "Envelope",
        dtp::ATTACK => "Attack",
        dtp::RELEASE => "Release",
        dtp::OUTPUT => "Output",
        dtp::DRY_WET => "Dry/Wet",
        dtp::TUBE => "Tube Type",
        _ => "",
    }
}
