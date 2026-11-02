//! Vinyl Distortion (Live 12): Tracing Model and Pinch sections (response
//! display with an XY handle, On / Drive / Freq / B rows), Soft/Hard and
//! Stereo/Mono, and the Crackle column (Volume, Density) with the global
//! Drive. Geometry measured from Live 12 at 100 % (l/t = device left/top;
//! text positions are baselines).

use super::dynamics::{KN, knob_lv, sig3, tbutton};
use super::fx4::*;
use super::fx5::*;
use super::*;
use daw_project::devices::params5::vdp;
use daw_project::devices::vinyl::*;

pub fn width(_d: &VinylParams) -> f32 {
    236.0
}

fn set(cx: &mut Ctx, f: impl FnOnce(&mut VinylParams)) {
    if let FxParams::VinylDistortion(d) = &mut cx.params {
        f(d);
        cx.undo_step();
    }
}

fn hz(f: f32) -> String {
    if f >= 999.5 { format!("{} kHz", sig3(f / 1000.0)) } else { format!("{f:.0} Hz") }
}

const MINOR_X: [f32; 20] = [12., 17., 21., 24., 48., 60., 68., 75., 80., 85., 88., 92., 115., 127., 136., 142., 148., 152., 156., 160.];
const MAJOR_X: [f32; 3] = [27., 95., 163.];

pub(super) fn draw(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect) {
    let FxParams::VinylDistortion(d) = cx.params else { return };
    let (l, t) = (r.left(), r.top());

    text_at(p, l + 6.0, t + 29.0, Align2::LEFT_BOTTOM, "Tracing Model", bold(FS), text());
    section(cx, ui, p, l, t, 0.0, false, d.tr_on, d.tr_drive, d.tr_freq, d.tr_b);

    // Pinch header: label, Soft / Hard, Stereo / Mono.
    text_at(p, l + 6.0, t + 113.0, Align2::LEFT_BOTTOM, "Pinch", bold(FS), text());
    for (i, (x0, x1, lab)) in [(39.0, 68.0, "Soft"), (71.0, 100.0, "Hard")].into_iter().enumerate() {
        cx.salt = i as u32;
        if tbutton(cx, ui, p, rr(l, t, x0, 103.0, x1, 116.0), vdp::SOFT, lab, d.soft == (i == 0), true) {
            set(cx, |q| q.soft = i == 0);
        }
    }
    for (i, (x0, x1, lab)) in [(106.0, 143.0, "Stereo"), (146.0, 183.0, "Mono")].into_iter().enumerate() {
        cx.salt = i as u32;
        if tbutton(cx, ui, p, rr(l, t, x0, 103.0, x1, 116.0), vdp::STEREO, lab, d.stereo == (i == 0), true) {
            set(cx, |q| q.stereo = i == 0);
        }
    }
    cx.salt = 0;
    section(cx, ui, p, l, t, 86.0, true, d.pi_on, d.pi_drive, d.pi_freq, d.pi_b);

    // Crackle column: bracket, Volume / Density, then the global Drive.
    px(p, l, t, 187.0, 20.0, 188.0, 134.0, gray(0x24));
    px(p, l, t, 187.0, 133.0, 231.0, 134.0, gray(0x24));
    text_at(p, l + 191.0, t + 29.0, Align2::LEFT_BOTTOM, "Crackle", bold(FS), text());
    knob_lv(cx, ui, p, l + 209.5, t + 42.0, vdp::VOLUME, "Volume", &format!("{:.2}", d.volume), 0.0, KN, 14.5, 36.0, true);
    knob_lv(cx, ui, p, l + 209.5, t + 92.0, vdp::DENSITY, "Density", &sig3(d.density), 0.0, KN, 14.5, 36.0, true);
    knob_lv(cx, ui, p, l + 209.5, t + 145.0, vdp::DRIVE, "Drive", &format!("{:.2}", d.drive), 0.0, KN, 14.5, 36.0, true);
}

/// One section (`dy` 0 = Tracing, 86 = Pinch): display + control row.
#[allow(clippy::too_many_arguments)]
fn section(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, dy: f32, pinch: bool, on: bool, drive: f32, freq: f32, b: f32) {
    let (k_on, k_drive, k_freq, k_b) =
        if pinch { (vdp::PI_ON, vdp::PI_DRIVE, vdp::PI_FREQ, vdp::PI_B) } else { (vdp::TR_ON, vdp::TR_DRIVE, vdp::TR_FREQ, vdp::TR_B) };
    let live = on && cx.on;

    // Display: #18, translucent grid, response curve, handle ring.
    let dr = rr(l, t, 7.0, 34.0 + dy, 181.0, 81.0 + dy);
    p.rect_filled(dr, CornerRadius::same(2), gray(0x18));
    for x in MINOR_X {
        gline(p, rr(l, t, x, 34.0 + dy, x + 1.0, 81.0 + dy), 0.082);
    }
    for x in MAJOR_X {
        gline(p, rr(l, t, x, 34.0 + dy, x + 1.0, 81.0 + dy), 0.169);
    }
    for (y, a) in [(49.0, 0.169), (57.0, 0.082), (65.0, 0.082), (73.0, 0.082)] {
        gline(p, rr(l, t, 7.0, y + dy, 181.0, y + dy + 1.0), a);
    }
    let col = if live {
        TEAL
    } else if cx.on {
        gray(0x86)
    } else {
        gray(0x75)
    };
    let base = 80.1 + dy;
    let pts: Vec<Pos2> = (0..=348)
        .map(|i| 7.0 + i as f32 * 0.5)
        .map(|x| {
            let f = 100.0 * 10f32.powf((x - 27.0) / 68.0);
            pos2(l + x, t + base - 30.6 * band_mag(f, freq, b))
        })
        .collect();
    p.with_clip_rect(dr).add(Shape::line(pts, Stroke::new(1.35, col)));
    let n = norm(&cx.params, k_freq).unwrap_or(0.0);
    let hc = pos2(l + 14.5 + n * 160.0, t + 74.5 + dy - drive * 34.0);
    let ring = if live {
        ORANGE
    } else if cx.on {
        gray(0x86)
    } else {
        gray(0x75)
    };
    p.circle_stroke(hc, 5.2, Stroke::new(1.6, ring));
    cx.salt = pinch as u32;
    xy_drag(cx, ui, dr, k_freq, k_drive, Some(k_b), 160.0, 34.0);

    // Control row.
    let y0 = 85.0 + dy;
    if tbutton(cx, ui, p, rr(l, t, 6.0, y0, 29.0, y0 + 13.0), k_on, "On", on, true) {
        set(cx, |q| if pinch { q.pi_on = !q.pi_on } else { q.tr_on = !q.tr_on });
    }
    let bg = if on { box_fill(cx.on) } else { panel() };
    value_box_ex(cx, ui, p, rr(l, t, 33.0, y0, 62.0, y0 + 13.0), k_drive, &format!("{drive:.2}"), true, on, bg);
    text_at(p, l + 66.0, t + y0 + 10.0, Align2::LEFT_BOTTOM, "Freq", font(FS), text());
    value_box_ex(cx, ui, p, rr(l, t, 92.0, y0, 138.0, y0 + 13.0), k_freq, &hz(freq), true, on, bg);
    text_at(p, l + 144.0, t + y0 + 10.0, Align2::LEFT_BOTTOM, "B", font(FS), text());
    value_box_ex(cx, ui, p, rr(l, t, 154.0, y0, 183.0, y0 + 13.0), k_b, &format!("{b:.2}"), true, on, bg);
    cx.salt = 0;
}

pub(super) fn reset(p: &mut FxParams, k: u8) {
    let d = FxParams::VinylDistortion(VinylParams::default());
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
        vdp::TR_DRIVE => "Tracing Drive",
        vdp::TR_FREQ => "Tracing Freq",
        vdp::TR_B => "Tracing Width",
        vdp::PI_DRIVE => "Pinch Drive",
        vdp::PI_FREQ => "Pinch Freq",
        vdp::PI_B => "Pinch Width",
        vdp::VOLUME => "Crackle Volume",
        vdp::DENSITY => "Crackle Density",
        vdp::DRIVE => "Global Drive",
        vdp::TR_ON => "Tracing On",
        vdp::PI_ON => "Pinch On",
        vdp::SOFT => "Pinch Soft On",
        vdp::STEREO => "Pinch Mono",
        _ => "",
    }
}
