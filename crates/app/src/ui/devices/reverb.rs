//! Reverb (Live 12): Input Filter (XY: Freq × Width), Early Reflections
//! (Spin, Shape), Diffusion Network (High / Low shelves on an XY display,
//! Diffusion, Scale), Chorus; Predelay, Smooth, Size, Decay, Freeze / Flat /
//! Cut, Stereo, Density; Reflect, Diffuse, Dry/Wet. Geometry measured from
//! Live 12 at 100 % (l/t = device left/top; text positions are baselines).

use super::dynamics::{KN, knob_lv, sig3, tbutton};
use super::modfx::drag_text;
use super::*;
use daw_engine::reverb::*;
use daw_project::devices::params2::rvp;

pub const WIDTH: f32 = 512.0;

const LABEL: Color32 = Color32::from_rgb(0x86, 0x86, 0x86);
const FRAME: Color32 = Color32::from_rgb(0x3e, 0x3e, 0x3e);
/// Input Filter graph: 100 Hz at l+18, 35.5 px per decade, 0 dB at t+73.5,
/// 9 px per 6 dB.
const IF_X100: f32 = 18.0;
const IF_DEC: f32 = 35.5;
/// Diffusion Network graph: 100 Hz at l+244, 45.5 px per decade, 0 dB at
/// t+68.5.
const DN_X100: f32 = 244.0;
const DN_DEC: f32 = 45.5;

fn set(cx: &mut Ctx, f: impl FnOnce(&mut ReverbParams)) {
    if let FxParams::Reverb(r) = &mut cx.params {
        f(r);
    }
}

fn hz(f: f32) -> String {
    if f >= 999.5 {
        format!("{:.2} kHz", f / 1000.0)
    } else if f < 99.95 {
        format!("{f:.1} Hz")
    } else {
        format!("{f:.0} Hz")
    }
}

fn px(p: &Painter, x0: f32, y0: f32, x1: f32, y1: f32, c: Color32) {
    quad(p, Rect::from_min_max(pos2(x0, y0), pos2(x1, y1)), c);
}

/// Display button: filled `col` when on (dark label), outlined when off.
#[allow(clippy::too_many_arguments)]
fn dbutton(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect, k: u8, label: &str, on: bool, col: Color32) -> bool {
    let col = if cx.on { col } else { gray(0x5a) };
    let tc = if on {
        p.rect_filled(r, CornerRadius::ZERO, col);
        gray(0x18)
    } else {
        p.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, col), egui::StrokeKind::Inside);
        col
    };
    if !label.is_empty() {
        text_at(p, r.center().x, r.top() + 10.0, Align2::CENTER_BOTTOM, label, font(FS), tc);
    }
    cx.toggle(ui, r, k, true)
}

/// Centred draggable text.
#[allow(clippy::too_many_arguments)]
fn drag_c(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, x: f32, base: f32, k: u8, s: &str, en: bool, col: Color32) {
    let w = p.layout_no_wrap(s.to_string(), font(FS), col).size().x;
    drag_text(cx, ui, p, x - w / 2.0, base, k, s, en, col);
}

/// XY handle drag: x moves `kx` over `span` px, y moves `ky` over `hy` px.
#[allow(clippy::too_many_arguments)]
fn xy_drag(cx: &mut Ctx, ui: &mut egui::Ui, c: Pos2, rad: f32, kx: u8, ky: u8, span: f32, hy: f32, salt: u32) {
    cx.salt = salt;
    let resp = cx.sense(ui, Rect::from_center_size(c, vec2(rad * 2.0 + 2.0, rad * 2.0 + 2.0)), kx, Sense::click_and_drag());
    cx.salt = 0;
    if resp.drag_started() || resp.clicked() {
        cx.focus(kx);
    }
    if resp.dragged() {
        let d = resp.drag_delta();
        if d != egui::Vec2::ZERO {
            cx.undo_step();
            let v = norm(&cx.params, kx).unwrap_or(0.0);
            set_norm(&mut cx.params, kx, v + d.x / span);
            let v = norm(&cx.params, ky).unwrap_or(0.0);
            set_norm(&mut cx.params, ky, v - d.y / hy);
        }
    }
    cx.param_menu(&resp, kx);
}

/// Display curve (linear gain) of the network shelves as Live draws it:
/// steep (2nd-order) transitions centred on the handles.
fn net_gain(r: &ReverbParams, f: f32) -> f32 {
    let sh = |fc: f32, g: f32| {
        let lp = 1.0 / (1.0 + (f / fc).powi(4)).sqrt();
        lp + g * (1.0 - lp)
    };
    let mut g = 1.0;
    if r.hi_on {
        g *= sh(r.hi_freq / 1.27, if r.hi_lp { 0.0 } else { r.hi_gain });
    }
    if r.lo_on {
        let x = (f / (r.lo_freq * 1.2)).powi(4);
        let hp = (x / (1.0 + x)).sqrt();
        g *= hp + r.lo_gain * (1.0 - hp);
    }
    g
}

pub(super) fn draw(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect) {
    let FxParams::Reverb(r0) = cx.params else { return };
    let (l, t) = (r.left(), r.top());
    let on = cx.on;
    for (x0, x1) in [(2.0, 104.0), (106.0, 205.0), (207.0, 398.0), (400.0, 453.0)] {
        p.rect_filled(Rect::from_min_max(pos2(l + x0, t + 17.0), pos2(l + x1, t + 132.0)), CornerRadius::same(2), gray(0x18));
    }
    p.rect_filled(Rect::from_min_max(pos2(l + 455.0, t + 17.0), pos2(l + 510.0, t + 186.0)), CornerRadius::same(2), panel());
    p.rect_filled(Rect::from_min_max(pos2(l + 2.0, t + 134.0), pos2(l + 453.0, t + 186.0)), CornerRadius::same(2), panel());

    // ---- Input Filter ----
    text_at(p, l + 7.0, t + 31.0, Align2::LEFT_BOTTOM, "Input Filter", font(FS), text());
    px(p, l + 64.0, t + 30.0, l + 101.0, t + 31.0, FRAME);
    if dbutton(cx, ui, p, Rect::from_min_max(pos2(l + 6.0, t + 37.0), pos2(l + 51.0, t + 50.0)), rvp::LO_CUT, "Lo Cut", r0.lo_cut, ORANGE) {
        set(cx, |r| r.lo_cut = !r.lo_cut);
    }
    if dbutton(cx, ui, p, Rect::from_min_max(pos2(l + 55.0, t + 37.0), pos2(l + 100.0, t + 50.0)), rvp::HI_CUT, "Hi Cut", r0.hi_cut, ORANGE)
    {
        set(cx, |r| r.hi_cut = !r.hi_cut);
    }
    let g = Rect::from_min_max(pos2(l + 6.0, t + 54.0), pos2(l + 100.0, t + 111.0));
    p.rect_stroke(g, CornerRadius::ZERO, Stroke::new(1.0, FRAME), egui::StrokeKind::Inside);
    let gi = g.shrink(1.0);
    let gp = p.with_clip_rect(gi.intersect(p.clip_rect()));
    let ifx = |f: f32| l + IF_X100 + IF_DEC * (f / 100.0).log10();
    for y in [82.0, 91.0, 100.0] {
        px(&gp, gi.left(), t + y, gi.right(), t + y + 1.0, gray(0x2b));
    }
    for f in [100.0f32, 1000.0, 10_000.0] {
        let x = ifx(f).round();
        px(&gp, x, gi.top(), x + 1.0, gi.bottom(), gray(0x3f));
    }
    let any = r0.lo_cut || r0.hi_cut;
    let sr = cx.app.spectra.sample_rate(cx.dev.id).max(8000.0);
    let pts: Vec<Pos2> = (0..=(gi.width() as usize * 2))
        .map(|i| {
            let x = gi.left() + i as f32 * 0.5;
            let f = 100.0 * 10f32.powf((x - l - IF_X100) / IF_DEC);
            pos2(x, (t + 73.5 - 1.5 * r0.input_db(f, sr)).clamp(gi.top() - 3.0, gi.bottom() + 3.0))
        })
        .collect();
    gp.add(Shape::line(pts, Stroke::new(1.6, if on && any { TEAL } else { gray(0x75) })));
    let w = r0.in_width.clamp(RV_IN_WIDTH.0, RV_IN_WIDTH.1);
    let hc = pos2(ifx(r0.in_freq.clamp(RV_IN_FREQ.0, RV_IN_FREQ.1)), t + 104.0 - (w - RV_IN_WIDTH.0) * 4.953);
    let oc = if on && any { ORANGE } else { gray(0x75) };
    gp.circle_stroke(hc, 5.7, Stroke::new(1.6, oc));
    let span = (RV_IN_FREQ.1 / RV_IN_FREQ.0).log10() * IF_DEC;
    xy_drag(cx, ui, hc, 6.5, rvp::IN_FREQ, rvp::IN_WIDTH, span, (RV_IN_WIDTH.1 - RV_IN_WIDTH.0) * 4.953, 5);
    let vc = if any { ORANGE } else { LABEL };
    drag_text(cx, ui, p, l + 9.0, t + 125.0, rvp::IN_FREQ, &hz(r0.in_freq), true, vc);
    let ws = format!("{:.2}", r0.in_width);
    let ww = p.layout_no_wrap(ws.clone(), font(FS), vc).size().x;
    drag_text(cx, ui, p, l + 96.0 - ww, t + 125.0, rvp::IN_WIDTH, &ws, true, vc);

    // ---- Early Reflections ----
    text_at(p, l + 155.5, t + 31.0, Align2::CENTER_BOTTOM, "Early Reflections", font(FS), text());
    if dbutton(cx, ui, p, Rect::from_min_max(pos2(l + 135.0, t + 48.0), pos2(l + 176.0, t + 61.0)), rvp::SPIN, "Spin", r0.spin, TEAL) {
        set(cx, |r| r.spin = !r.spin);
    }
    text_at(p, l + 132.5, t + 78.0, Align2::CENTER_BOTTOM, "Amount", font(FS), LABEL);
    text_at(p, l + 178.5, t + 78.0, Align2::CENTER_BOTTOM, "Rate", font(FS), LABEL);
    let sc = if r0.spin { TEAL } else { LABEL };
    drag_c(cx, ui, p, l + 132.0, t + 91.0, rvp::SPIN_AMT, &format!("{:.2}", r0.spin_amount), true, sc);
    drag_c(cx, ui, p, l + 178.5, t + 91.0, rvp::SPIN_RATE, &format!("{:.2} Hz", r0.spin_rate), true, sc);
    px(p, l + 110.0, t + 110.0, l + 201.0, t + 111.0, gray(0x24));
    text_at(p, l + 118.0, t + 125.0, Align2::LEFT_BOTTOM, "Shape", font(FS), LABEL);
    drag_text(cx, ui, p, l + 167.0, t + 125.0, rvp::SHAPE, &format!("{:.2}", r0.shape), true, TEAL);

    // ---- Diffusion Network ----
    text_at(p, l + 212.0, t + 31.0, Align2::LEFT_BOTTOM, "Diffusion Network", font(FS), text());
    px(p, l + 304.0, t + 30.0, l + 395.0, t + 31.0, FRAME);
    if dbutton(cx, ui, p, Rect::from_min_max(pos2(l + 211.0, t + 37.0), pos2(l + 239.0, t + 50.0)), rvp::HI_ON, "High", r0.hi_on, ORANGE) {
        set(cx, |r| r.hi_on = !r.hi_on);
    }
    let tr = Rect::from_min_max(pos2(l + 243.0, t + 37.0), pos2(l + 264.0, t + 50.0));
    if dbutton(cx, ui, p, tr, rvp::HI_LP, "", true, ORANGE) {
        set(cx, |r| r.hi_lp = !r.hi_lp);
    }
    // Shelf (—<) or low-pass (—\) glyph.
    let gs = Stroke::new(1.2, gray(0x18));
    let q = |x: f32, y: f32| pos2(tr.left() + x, tr.top() + y);
    if r0.hi_lp {
        p.add(Shape::line(vec![q(4.0, 4.5), q(10.0, 4.5), q(16.0, 10.0)], gs));
    } else {
        p.add(Shape::line(vec![q(4.0, 6.5), q(9.5, 6.5)], gs));
        p.add(Shape::line(vec![q(16.5, 3.0), q(9.5, 6.5), q(16.5, 10.0)], gs));
    }
    let hc = if r0.hi_on { ORANGE } else { LABEL };
    drag_text(cx, ui, p, l + 271.0, t + 47.0, rvp::HI_FREQ, &hz(r0.hi_freq), true, hc);
    drag_text(cx, ui, p, l + 323.0, t + 47.0, rvp::HI_GAIN, &format!("{:.2}", r0.hi_gain), !r0.hi_lp, hc);
    let g = Rect::from_min_max(pos2(l + 211.0, t + 54.0), pos2(l + 345.0, t + 111.0));
    p.rect_stroke(g, CornerRadius::ZERO, Stroke::new(1.0, FRAME), egui::StrokeKind::Inside);
    let gi = g.shrink(1.0);
    let gp = p.with_clip_rect(gi.intersect(p.clip_rect()));
    let dnx = |f: f32| l + DN_X100 + DN_DEC * (f / 100.0).log10();
    px(&gp, gi.left(), t + 101.0, gi.right(), t + 102.0, gray(0x2b));
    for f in [100.0f32, 1000.0, 10_000.0] {
        let x = dnx(f).round();
        px(&gp, x, gi.top(), x + 1.0, gi.bottom(), gray(0x3f));
    }
    let pts: Vec<Pos2> = (0..=(gi.width() as usize * 2))
        .map(|i| {
            let x = gi.left() + i as f32 * 0.5;
            let f = 100.0 * 10f32.powf((x - l - DN_X100) / DN_DEC);
            pos2(x, (t + 69.0 + 28.3 * (1.0 - net_gain(&r0, f))).clamp(gi.top() - 3.0, gi.bottom() + 3.0))
        })
        .collect();
    gp.add(Shape::line(pts, Stroke::new(1.6, if on { TEAL } else { gray(0x75) })));
    // Handles ① Low, ② High: x = freq (Live insets them a little), y = gain.
    let gy = |g: f32| t + 112.5 - 50.0 * g.clamp(RV_SHELF_GAIN.0, RV_SHELF_GAIN.1);
    for (i, (f, gain, en, kf, kg)) in [
        (r0.lo_freq * 1.2, r0.lo_gain, r0.lo_on, rvp::LO_FREQ, rvp::LO_GAIN),
        (r0.hi_freq / 1.27, r0.hi_gain, r0.hi_on, rvp::HI_FREQ, rvp::HI_GAIN),
    ]
    .into_iter()
    .enumerate()
    {
        let c = pos2(dnx(f), gy(gain));
        let col = if en && on { ORANGE } else { LABEL };
        gp.circle_stroke(c, 5.7, Stroke::new(1.6, col));
        text_at(&gp, c.x + 0.5, c.y + 4.0, Align2::CENTER_BOTTOM, if i == 0 { "1" } else { "2" }, font(FS), col);
        let span = (RV_HI_FREQ.1 / RV_HI_FREQ.0).log10() * DN_DEC;
        xy_drag(cx, ui, c, 6.5, kf, kg, span, 50.0 / (RV_SHELF_GAIN.1 - RV_SHELF_GAIN.0) * 0.8, 6 + i as u32);
    }
    text_at(p, l + 371.5, t + 61.0, Align2::CENTER_BOTTOM, "Diffusion", font(FS), LABEL);
    drag_c(cx, ui, p, l + 371.5, t + 74.0, rvp::DIFFUSION, &format!("{:.0} %", r0.diffusion * 100.0), true, ORANGE);
    text_at(p, l + 371.0, t + 96.0, Align2::CENTER_BOTTOM, "Scale", font(FS), LABEL);
    drag_c(cx, ui, p, l + 371.5, t + 109.0, rvp::SCALE, &format!("{:.0} %", r0.scale * 100.0), true, ORANGE);
    if dbutton(cx, ui, p, Rect::from_min_max(pos2(l + 211.0, t + 115.0), pos2(l + 239.0, t + 128.0)), rvp::LO_ON, "Low", r0.lo_on, ORANGE) {
        set(cx, |r| r.lo_on = !r.lo_on);
    }
    let lc = if r0.lo_on { ORANGE } else { LABEL };
    drag_text(cx, ui, p, l + 271.0, t + 125.0, rvp::LO_FREQ, &hz(r0.lo_freq), true, lc);
    drag_text(cx, ui, p, l + 323.0, t + 125.0, rvp::LO_GAIN, &format!("{:.2}", r0.lo_gain), true, lc);

    // ---- Chorus ----
    if dbutton(cx, ui, p, Rect::from_min_max(pos2(l + 404.0, t + 21.0), pos2(l + 449.0, t + 34.0)), rvp::CHORUS, "Chorus", r0.chorus, TEAL)
    {
        set(cx, |r| r.chorus = !r.chorus);
    }
    let cc = if r0.chorus { TEAL } else { LABEL };
    text_at(p, l + 426.5, t + 61.0, Align2::CENTER_BOTTOM, "Amount", font(FS), LABEL);
    drag_c(cx, ui, p, l + 425.5, t + 74.0, rvp::CHO_AMT, &format!("{:.2}", r0.cho_amount), true, cc);
    text_at(p, l + 426.5, t + 96.0, Align2::CENTER_BOTTOM, "Rate", font(FS), LABEL);
    drag_c(cx, ui, p, l + 426.5, t + 109.0, rvp::CHO_RATE, &format!("{:.2} Hz", r0.cho_rate), true, cc);

    // ---- bottom row ----
    let pd = if r0.predelay >= 999.5 { format!("{} s", sig3(r0.predelay / 1000.0)) } else { format!("{} ms", sig3(r0.predelay)) };
    knob_lv(cx, ui, p, l + 53.5, t + 145.0, rvp::PREDELAY, "Predelay", &pd, 0.0, KN, 15.5, 36.0, true);
    text_at(p, l + 130.5, t + 154.0, Align2::CENTER_BOTTOM, "Smooth", font(FS), text());
    let cr = Rect::from_min_max(pos2(l + 111.0, t + 157.0), pos2(l + 150.0, t + 170.0));
    if let Some(i) = chooser(cx, ui, p, cr, rvp::SMOOTH, RV_SMOOTH[(r0.smooth as usize).min(2)], &RV_SMOOTH, true) {
        set(cx, |r| r.smooth = i as u8);
    }
    px(p, l + 153.0, t + 161.0, l + 162.0, t + 162.0, gray(0x24));
    knob_lv(cx, ui, p, l + 178.5, t + 145.0, rvp::SIZE, "Size", &format!("{:.2}", r0.size), 0.0, KN, 15.5, 36.0, true);
    let dc = if r0.decay >= 9.995 { format!("{:.1} s", r0.decay) } else { format!("{:.2} s", r0.decay) };
    knob_lv(cx, ui, p, l + 234.0, t + 145.0, rvp::DECAY, "Decay", &dc, 0.0, KN, 15.5, 36.0, !r0.freeze);
    if tbutton(
        cx,
        ui,
        p,
        Rect::from_min_max(pos2(l + 273.0, t + 146.0), pos2(l + 324.0, t + 159.0)),
        rvp::FREEZE,
        "Freeze",
        r0.freeze,
        true,
    ) {
        set(cx, |r| r.freeze = !r.freeze);
    }
    if tbutton(cx, ui, p, Rect::from_min_max(pos2(l + 273.0, t + 162.0), pos2(l + 297.0, t + 175.0)), rvp::FLAT, "Flat", r0.flat, true) {
        set(cx, |r| r.flat = !r.flat);
    }
    if tbutton(cx, ui, p, Rect::from_min_max(pos2(l + 300.0, t + 162.0), pos2(l + 324.0, t + 175.0)), rvp::CUT, "Cut", r0.cut, true) {
        set(cx, |r| r.cut = !r.cut);
    }
    knob_lv(cx, ui, p, l + 365.0, t + 145.0, rvp::STEREO, "Stereo", &format!("{:.2}", r0.stereo), 0.0, KN, 15.5, 36.0, true);
    text_at(p, l + 419.0, t + 154.0, Align2::CENTER_BOTTOM, "Density", font(FS), text());
    let dr = Rect::from_min_max(pos2(l + 400.0, t + 157.0), pos2(l + 445.0, t + 170.0));
    if let Some(i) = chooser(cx, ui, p, dr, rvp::DENSITY, RV_DENSITY[(r0.density as usize).min(3)], &RV_DENSITY, true) {
        set(cx, |r| r.density = i as u8);
    }

    // ---- levels ----
    let lv = |d: f32| format!("{d:.1} dB");
    knob_lv(cx, ui, p, l + 482.5, t + 30.0, rvp::REFLECT, "Reflect", &lv(r0.reflect), 0.0, KN, 15.5, 36.0, true);
    knob_lv(cx, ui, p, l + 482.5, t + 86.0, rvp::DIFFUSE, "Diffuse", &lv(r0.diffuse), 0.0, KN, 15.5, 36.0, true);
    knob_lv(cx, ui, p, l + 482.5, t + 143.0, rvp::DRY_WET, "Dry/Wet", &format!("{:.0} %", r0.dry_wet * 100.0), 0.0, KN, 15.5, 36.0, true);
}

// ---- parameters --------------------------------------------------------------

pub(super) fn reset(p: &mut FxParams, k: u8) {
    let FxParams::Reverb(r) = p else { return };
    let d = ReverbParams::default();
    match k {
        rvp::LO_CUT => r.lo_cut = d.lo_cut,
        rvp::HI_CUT => r.hi_cut = d.hi_cut,
        rvp::SPIN => r.spin = d.spin,
        rvp::HI_ON => r.hi_on = d.hi_on,
        rvp::HI_LP => r.hi_lp = d.hi_lp,
        rvp::LO_ON => r.lo_on = d.lo_on,
        rvp::CHORUS => r.chorus = d.chorus,
        rvp::SMOOTH => r.smooth = d.smooth,
        rvp::FREEZE => r.freeze = false,
        rvp::FLAT => r.flat = d.flat,
        rvp::CUT => r.cut = d.cut,
        rvp::DENSITY => r.density = d.density,
        _ => {
            if let Some(v) = norm(&FxParams::Reverb(d), k) {
                set_norm(p, k, v);
            }
        }
    }
}

pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    let FxParams::Reverb(r) = p else { return };
    match k {
        rvp::SMOOTH => r.smooth = (r.smooth as i32 + dir as i32).clamp(0, 2) as u8,
        rvp::DENSITY => r.density = (r.density as i32 + dir as i32).clamp(0, 3) as u8,
        k if k >= 40 => {}
        _ => {
            if let Some(v) = norm(p, k) {
                set_norm(p, k, v + dir / 200.0);
            }
        }
    }
}

pub(super) fn name(k: u8) -> &'static str {
    match k {
        rvp::IN_FREQ => "In Filter Freq",
        rvp::IN_WIDTH => "In Filter Width",
        rvp::SPIN_AMT => "ER Spin Amount",
        rvp::SPIN_RATE => "ER Spin Rate",
        rvp::SHAPE => "ER Shape",
        rvp::HI_FREQ => "HiShelf Freq",
        rvp::HI_GAIN => "HiShelf Gain",
        rvp::LO_FREQ => "LowShelf Freq",
        rvp::LO_GAIN => "LowShelf Gain",
        rvp::DIFFUSION => "Diffusion",
        rvp::SCALE => "Scale",
        rvp::CHO_AMT => "Chorus Amount",
        rvp::CHO_RATE => "Chorus Rate",
        rvp::PREDELAY => "Predelay",
        rvp::SIZE => "Room Size",
        rvp::DECAY => "Decay Time",
        rvp::STEREO => "Stereo Image",
        rvp::REFLECT => "Reflect Level",
        rvp::DIFFUSE => "Diffuse Level",
        rvp::DRY_WET => "Dry/Wet",
        rvp::LO_CUT => "In LowCut On",
        rvp::HI_CUT => "In HighCut On",
        rvp::SPIN => "ER Spin On",
        rvp::HI_ON => "HiShelf On",
        rvp::HI_LP => "HiFilter Type",
        rvp::LO_ON => "LowShelf On",
        rvp::CHORUS => "Chorus On",
        rvp::SMOOTH => "Smoothing",
        rvp::FREEZE => "Freeze On",
        rvp::FLAT => "Flat On",
        rvp::CUT => "Cut On",
        _ => "Density",
    }
}
