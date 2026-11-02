//! Spectral Time (Live 12): the Freezer (Manual / Retrigger, Onsets / Sync,
//! Crossfade / Envelope fades, Freeze pad), the Delay (Time / Feedback /
//! Shift, Mode / Stereo / Mix, Tilt / Spray / Mask), the spectrogram with
//! Resolution, and Send / processing order / Dry/Wet. Geometry measured from
//! Live 12 at 100 % (l/t = device left/top; text = baselines).

use super::dynamics::{KN, knob_lv, tbutton};
use super::fx4::*;
use super::modfx::text_menu;
use super::*;
use daw_project::devices::params4::stp;
use daw_project::devices::spectime::*;

pub const WIDTH: f32 = 570.0;

fn set(cx: &mut Ctx, f: impl FnOnce(&mut SpecTimeParams)) {
    if let FxParams::SpectralTime(d) = &mut cx.params {
        f(d);
        cx.undo_step();
    }
}

fn pct(x: f32) -> String {
    format!("{:.0} %", x * 100.0)
}

fn pct1(x: f32) -> String {
    format!("{:.1} %", x * 100.0)
}

/// Section header: orange 8×8 switch and title.
#[allow(clippy::too_many_arguments)]
fn header(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, x: f32, y: f32, k: u8, label: &str, on: bool) -> bool {
    let b = rr(l, t, x, y, x + 8.0, y + 8.0);
    p.rect_filled(b, CornerRadius::ZERO, if on && cx.on { ORANGE } else { gray(0x1e) });
    if !on {
        p.rect_stroke(b, CornerRadius::ZERO, Stroke::new(1.0, gray(0x11)), egui::StrokeKind::Inside);
    }
    let tr = text_at(p, l + x + 14.0, b.bottom(), Align2::LEFT_BOTTOM, label, font(FS), text());
    cx.toggle(ui, Rect::from_min_max(b.min - vec2(2.0, 2.0), pos2(tr.right() + 2.0, b.bottom() + 2.0)), k, true)
}

/// Fade-type icon button (Crossfade ⨯ / Envelope ∧).
fn fade_btn(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect, env: bool, sel: bool, en: bool) -> bool {
    let (fill, col) = match (sel, en && cx.on) {
        (true, true) => (ORANGE, gray(0x20)),
        (true, false) => (gray(0x75), gray(0x36)),
        (false, _) => (gray(0x1e), if en { text() } else { gray(0x75) }),
    };
    boxed(p, r, fill);
    let c = r.center();
    let s = Stroke::new(1.0, col);
    if env {
        p.add(Shape::line(vec![c + vec2(-8.0, 3.5), c + vec2(0.0, -3.5), c + vec2(8.0, 3.5)], s));
    } else {
        p.add(Shape::line(vec![c + vec2(-8.0, -3.5), c + vec2(-3.0, 0.0), c + vec2(3.0, 0.0), c + vec2(8.0, -3.5)], s));
        p.add(Shape::line(vec![c + vec2(-8.0, 3.5), c + vec2(-3.0, 0.0)], s));
        p.add(Shape::line(vec![c + vec2(3.0, 0.0), c + vec2(8.0, 3.5)], s));
    }
    cx.salt = env as u32;
    let hit = cx.toggle(ui, r, stp::ENVELOPE, en);
    cx.salt = 0;
    hit
}

pub(super) fn draw(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect) {
    let FxParams::SpectralTime(d) = cx.params else { return };
    let (l, t) = (r.left(), r.top());
    panels(p, r, &[(2.0, 144.0), (146.0, 307.0), (507.0, 569.0)], &[(309.0, 505.0)]);
    let ct = |p: &Painter, x: f32, y: f32, s: &str, en: bool| {
        text_at(p, l + x, t + y, Align2::CENTER_BOTTOM, s, font(FS), if en { text() } else { dim() });
    };

    // ---- Freezer ----
    if header(cx, ui, p, l, t, 7.0, 22.0, stp::FRZ_ON, "1 Freezer", d.frz_on) {
        set(cx, |q| q.frz_on = !q.frz_on);
    }
    let fe = d.frz_on;
    if tbutton(cx, ui, p, rr(l, t, 6.0, 37.0, 82.0, 50.0), stp::RETRIG, "Manual", !d.retrigger, fe) {
        set(cx, |q| q.retrigger = false);
    }
    cx.salt = 1;
    if tbutton(cx, ui, p, rr(l, t, 6.0, 53.0, 82.0, 66.0), stp::RETRIG, "Retrigger", d.retrigger, fe) {
        set(cx, |q| q.retrigger = true);
    }
    cx.salt = 0;
    let re = fe && d.retrigger;
    if sel_btn(cx, ui, p, rr(l, t, 6.0, 68.0, 48.0, 81.0), stp::SYNC, "Onsets", !d.sync, re) {
        set(cx, |q| q.sync = false);
    }
    cx.salt = 1;
    if sel_btn(cx, ui, p, rr(l, t, 51.0, 68.0, 82.0, 81.0), stp::SYNC, "Sync", d.sync, re) {
        set(cx, |q| q.sync = true);
    }
    cx.salt = 0;
    if re {
        if d.sync {
            let (s, _) = ST_DIVS[(d.interval as usize).min(ST_DIVS.len() - 1)];
            knob_lv(cx, ui, p, l + 44.0, t + 100.0, stp::INTERVAL, "Interval", s, 0.0, KN, 14.5, 36.0, true);
        } else {
            knob_lv(cx, ui, p, l + 44.0, t + 100.0, stp::SENS, "Sensitivity", &pct(d.sensitivity), 0.0, KN, 14.5, 36.0, true);
        }
    }
    if fade_btn(cx, ui, p, rr(l, t, 91.0, 37.0, 114.0, 50.0), false, !d.envelope, re) {
        set(cx, |q| q.envelope = false);
    }
    if fade_btn(cx, ui, p, rr(l, t, 117.0, 37.0, 140.0, 50.0), true, d.envelope, re) {
        set(cx, |q| q.envelope = true);
    }
    if re && !d.envelope {
        knob_lv(cx, ui, p, l + 115.5, t + 70.0, stp::XFADE, "X-Fade", &pct(d.xfade), 0.0, KN, 14.5, 36.0, fe);
    } else {
        knob_lv(cx, ui, p, l + 115.5, t + 70.0, stp::FADE_IN, "Fade In", &ms(d.fade_in), 0.0, KN, 14.5, 36.0, fe);
        knob_lv(cx, ui, p, l + 115.5, t + 130.0, stp::FADE_OUT, "Fade Out", &ms(d.fade_out), 0.0, KN, 14.5, 36.0, fe);
    }
    ct(p, 44.5, 151.0, "Freeze", fe);
    let pad = rr(l, t, 6.0, 155.0, 82.0, 182.0);
    boxed(p, pad, if d.freeze && cx.on && fe { ORANGE } else { gray(0x1e) });
    let ac = if d.freeze && cx.on && fe {
        gray(0x20)
    } else if fe {
        Color32::BLACK
    } else {
        gray(0x11)
    };
    let c = pad.center() + vec2(0.0, 0.5);
    for i in 0..3 {
        let a = PI / 3.0 * i as f32 + PI / 2.0;
        let dv = vec2(a.cos(), a.sin()) * 5.0;
        p.add(Shape::line_segment([c - dv, c + dv], Stroke::new(1.2, ac)));
    }
    if cx.toggle(ui, pad, stp::FREEZE, fe) {
        set(cx, |q| q.freeze = !q.freeze);
    }
    px(p, l, t, 86.0, 37.0, 87.0, 182.0, gray(0x24));

    // ---- Delay ----
    if header(cx, ui, p, l, t, 151.0, 21.0, stp::DLY_ON, "2 Delay", d.dly_on) {
        set(cx, |q| q.dly_on = !q.dly_on);
    }
    let de = d.dly_on;
    knob_lv(cx, ui, p, l + 174.5, t + 47.0, stp::TIME, "Time", &d.time_label(), 0.0, KN, 14.5, 36.0, de);
    knob_lv(cx, ui, p, l + 226.5, t + 47.0, stp::FEEDBACK, "Feedback", &pct1(d.feedback), 0.0, KN, 14.5, 36.0, de);
    knob_lv(
        cx,
        ui,
        p,
        l + 278.5,
        t + 47.0,
        stp::SHIFT,
        "Shift",
        &format!("{} Hz", super::dynamics::sig3(d.shift)),
        0.0,
        KN,
        14.5,
        36.0,
        de,
    );
    ct(p, 174.5, 104.0, "Mode", de);
    let mi = (d.time_mode as usize).min(ST_MODES.len() - 1);
    if let Some(i) = chooser(cx, ui, p, rr(l, t, 150.0, 108.0, 199.0, 121.0), stp::MODE, ST_MODES[mi], &ST_MODES, de) {
        set(cx, |q| q.time_mode = i as u8);
    }
    ct(p, 226.0, 104.0, "Stereo", de);
    value_box_ex(cx, ui, p, rr(l, t, 202.0, 108.0, 250.0, 121.0), stp::STEREO, &pct1(d.stereo), de, false, box_fill(cx.on));
    ct(p, 278.0, 104.0, "Mix", de);
    value_box(cx, ui, p, rr(l, t, 254.0, 108.0, 302.0, 121.0), stp::MIX, &pct(d.mix), de);
    knob_lv(cx, ui, p, l + 174.5, t + 144.0, stp::TILT, "Tilt", &ms(d.tilt), 0.0, KN, 14.5, 36.0, de);
    knob_lv(cx, ui, p, l + 226.5, t + 144.0, stp::SPRAY, "Spray", &ms(d.spray), 0.0, KN, 14.5, 36.0, de);
    knob_lv(cx, ui, p, l + 278.5, t + 144.0, stp::MASK, "Mask", &format!("{:.2}", d.mask), 0.0, KN, 14.5, 36.0, de);

    // ---- Spectrogram ----
    if play_btn(cx, ui, p, pos2(l + 319.0, t + 26.3), stp::DISPLAY, d.display) {
        set(cx, |q| q.display = !q.display);
    }
    text_at(p, l + 409.0, t + 30.0, Align2::LEFT_BOTTOM, "Resolution", font(FS), gray(0x86));
    let ri = (d.resolution as usize).min(ST_RES.len() - 1);
    let mr = rr(l, t, 462.0, 19.0, 502.0, 33.0);
    if let Some(i) = text_menu(cx, ui, p, mr, stp::RES, l + 466.0, t + 30.0, Some(l + 493.0), ri, &ST_RES, true, ORANGE) {
        set(cx, |q| q.resolution = i as u8);
    }
    spectrogram(cx, ui, p, rr(l, t, 310.0, 36.0, 504.0, 185.0), d.display);

    // ---- Send / order / Dry/Wet ----
    knob_lv(cx, ui, p, l + 538.5, t + 29.0, stp::SEND, "Send", &format!("{:.1} dB", d.send), 0.0, KN, 14.5, 36.0, true);
    led(p, pos2(l + 562.5, t + 43.5), false);
    if tbutton(cx, ui, p, rr(l, t, 514.0, 87.0, 562.0, 100.0), stp::ORDER, "Frz > Dly", !d.dly_first, true) {
        set(cx, |q| q.dly_first = false);
    }
    cx.salt = 1;
    if tbutton(cx, ui, p, rr(l, t, 514.0, 103.0, 562.0, 116.0), stp::ORDER, "Dly > Frz", d.dly_first, true) {
        set(cx, |q| q.dly_first = true);
    }
    cx.salt = 0;
    knob_lv(cx, ui, p, l + 538.5, t + 144.0, stp::DRY_WET, "Dry/Wet", &pct(d.dry_wet), 0.0, KN, 14.5, 36.0, true);
}

// ---- parameters --------------------------------------------------------------

pub(super) fn reset(p: &mut FxParams, k: u8) {
    let d = FxParams::SpectralTime(SpecTimeParams::default());
    if let Some(v) = norm(&d, k) {
        set_norm(p, k, v);
    }
}

pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    let FxParams::SpectralTime(d) = p else { return };
    match k {
        stp::TIME if d.time_mode == 1 => d.div = (d.div as f32 + dir).clamp(0.0, (ST_DIVS.len() - 1) as f32) as u8,
        stp::TIME if d.time_mode > 1 => d.steps = (d.steps + dir).clamp(ST_STEPS.0, ST_STEPS.1),
        stp::INTERVAL => d.interval = (d.interval as f32 + dir).clamp(0.0, (ST_DIVS.len() - 1) as f32) as u8,
        _ => {
            if let Some(v) = norm(p, k) {
                set_norm(p, k, ((v * 100.0).round() + dir) / 100.0);
            }
        }
    }
}

pub(super) fn name(k: u8) -> &'static str {
    match k {
        stp::FADE_IN => "Fade In",
        stp::FADE_OUT => "Fade Out",
        stp::XFADE => "X-Fade",
        stp::SENS => "Sensitivity",
        stp::INTERVAL => "Interval",
        stp::TIME => "Delay Time",
        stp::FEEDBACK => "Feedback",
        stp::SHIFT => "Frequency Shift",
        stp::STEREO => "Stereo",
        stp::MIX => "Delay Mix",
        stp::TILT => "Tilt",
        stp::SPRAY => "Spray",
        stp::MASK => "Mask",
        stp::SEND => "Input Send",
        stp::DRY_WET => "Dry/Wet",
        stp::FRZ_ON => "Freezer On",
        stp::RETRIG => "Freeze Mode",
        stp::SYNC => "Retrigger Source",
        stp::ENVELOPE => "Fade Type",
        stp::FREEZE => "Freeze",
        stp::DLY_ON => "Delay On",
        stp::MODE => "Delay Time Mode",
        stp::RES => "Resolution",
        stp::ORDER => "Processing Order",
        stp::DISPLAY => "Spectrogram",
        _ => "",
    }
}
