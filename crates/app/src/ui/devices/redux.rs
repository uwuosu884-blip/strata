//! Redux (Live 12): Rate / Bits big knobs, Jitter / Shape, Pre / Post filter
//! switches with the Post octave box, DC Shift and Dry/Wet. Geometry
//! measured from Live 12 at 100 % (l/t = device left/top; text positions are
//! baselines).

use super::dynamics::{KN, KN_BIG, knob_lv, tbutton};
use super::*;
use daw_engine::redux::*;
use daw_project::devices::params3::rdp;

pub const WIDTH: f32 = 146.0;

fn set(cx: &mut Ctx, f: impl FnOnce(&mut ReduxParams)) {
    if let FxParams::Redux(p) = &mut cx.params {
        f(p);
    }
}

pub(super) fn pct(x: f32) -> String {
    let v = x * 100.0;
    if v.abs() < 9.95 { format!("{v:.1} %") } else { format!("{v:.0} %") }
}

/// ◀ marker at the right end of a bipolar number box `r`.
pub(super) fn bip_arrow(p: &Painter, r: Rect, col: Color32) {
    let (x, y) = (r.right(), r.top());
    p.add(Shape::convex_polygon(vec![pos2(x - 8.0, y + 6.5), pos2(x - 1.0, y + 2.0), pos2(x - 1.0, y + 11.0)], col, Stroke::NONE));
}

/// Marker colour: teal off-centre, dark at the centre, grey when inactive.
pub(super) fn arrow_col(cx: &Ctx, active: bool, nonzero: bool) -> Color32 {
    match (active && cx.on, nonzero) {
        (false, _) => gray(0x75),
        (true, true) => TEAL,
        (true, false) => gray(0x1e),
    }
}

fn r(l: f32, t: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
    Rect::from_min_max(pos2(l + x0, t + y0), pos2(l + x1, t + y1))
}

pub(super) fn draw(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, rect: Rect) {
    let FxParams::Redux(d) = cx.params else { return };
    let (l, t) = (rect.left(), rect.top());
    quad(p, r(l, t, 73.0, 23.0, 74.0, 182.0), gray(0x24));
    knob_lv(cx, ui, p, l + 36.5, t + 32.0, rdp::RATE, "Rate", &d.rate_label(), 0.0, KN_BIG, 23.5, 51.0, true);
    knob_lv(cx, ui, p, l + 110.5, t + 32.0, rdp::BITS, "Bits", &d.bits_label(), 0.0, KN_BIG, 23.5, 51.0, true);
    knob_lv(cx, ui, p, l + 36.5, t + 97.0, rdp::JITTER, "Jitter", &pct(d.jitter), 0.0, KN, 15.5, 36.0, true);
    knob_lv(cx, ui, p, l + 110.5, t + 97.0, rdp::SHAPE, "Shape", &pct(d.shape), 0.0, KN, 15.5, 36.0, true);

    text_at(p, l + 7.0, t + 150.0, Align2::LEFT_BOTTOM, "Filter", font(FS), text());
    if tbutton(cx, ui, p, r(l, t, 6.0, 153.0, 35.0, 166.0), rdp::PRE, "Pre", d.pre, true) {
        set(cx, |q| q.pre = !q.pre);
    }
    if tbutton(cx, ui, p, r(l, t, 38.0, 153.0, 67.0, 166.0), rdp::POST, "Post", d.post, true) {
        set(cx, |q| q.post = !q.post);
    }
    let pr = r(l, t, 6.0, 169.0, 67.0, 182.0);
    let bg = if d.post { box_fill(cx.on) } else { panel() };
    value_box_ex(cx, ui, p, pr, rdp::POST_OCT, &format!("{:.2}", d.post_oct), true, false, bg);
    bip_arrow(p, pr, arrow_col(cx, d.post, d.post_oct.abs() > 1e-3));

    if tbutton(cx, ui, p, r(l, t, 80.0, 140.0, 141.0, 153.0), rdp::DC, "DC Shift", d.dc, true) {
        set(cx, |q| q.dc = !q.dc);
    }
    text_at(p, l + 110.5, t + 166.0, Align2::CENTER_BOTTOM, "Dry/Wet", font(FS), text());
    value_box(cx, ui, p, r(l, t, 80.0, 169.0, 141.0, 182.0), rdp::DRY_WET, &pct(d.dry_wet), true);
}

// ---- parameters --------------------------------------------------------------

pub(super) fn reset(p: &mut FxParams, k: u8) {
    let d = FxParams::Redux(ReduxParams::default());
    if let Some(v) = norm(&d, k) {
        set_norm(p, k, v);
    }
}

pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    let FxParams::Redux(d) = p else { return };
    match k {
        rdp::BITS => d.bits = (d.bits.round() + dir).clamp(RDX_BITS.0, RDX_BITS.1),
        rdp::POST_OCT => d.post_oct = ((d.post_oct * 100.0).round() + dir * 10.0).clamp(-RDX_OCT * 100.0, RDX_OCT * 100.0) / 100.0,
        rdp::JITTER | rdp::SHAPE | rdp::DRY_WET => {
            let v = norm(p, k).unwrap_or(0.0);
            set_norm(p, k, ((v * 100.0).round() + dir) / 100.0);
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
        rdp::RATE => "Sample Rate",
        rdp::JITTER => "Jitter",
        rdp::POST_OCT => "Post-Filter Octave",
        rdp::BITS => "Bit Depth",
        rdp::SHAPE => "Quantizer Shape",
        rdp::DRY_WET => "Dry/Wet",
        rdp::PRE => "Pre-Filter",
        rdp::POST => "Post-Filter",
        rdp::DC => "DC Shift",
        _ => "",
    }
}
