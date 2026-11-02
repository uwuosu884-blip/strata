//! Resonators (Live 12): input filter, Mode / Decay / Color, five resonator
//! columns (On, Note or Pitch, Fine, Gain), Width / Output / Dry/Wet and the
//! title-bar ♭♯ button. Geometry measured from Live 12 at 100 %.

use super::dynamics::{KN, knob_lv, sig3, tbutton};
use super::fx4::*;
use super::redux::{arrow_col, bip_arrow};
use super::*;
use daw_engine::autoshift::current_scale;
use daw_project::devices::params4::rsp;
use daw_project::devices::resonators::*;

pub const WIDTH: f32 = 394.0;
const COLS: [f32; 5] = [130.5, 178.5, 226.5, 274.5, 322.5];
const ROMAN: [&str; 5] = ["I", "II", "III", "IV", "V"];

fn set(cx: &mut Ctx, f: impl FnOnce(&mut ResonatorsParams)) {
    if let FxParams::Resonators(d) = &mut cx.params {
        f(d);
        cx.undo_step();
    }
}

fn pct(x: f32) -> String {
    format!("{:.0} %", x * 100.0)
}

/// Filter-type icon (LP / HP / BP / Notch) in a 24×13 button at `l`/`top`.
fn filter_icon(p: &Painter, l: f32, top: f32, kind: u8, col: Color32) {
    let s = Stroke::new(1.0, col);
    let pts = |v: &[(f32, f32)]| v.iter().map(|&(x, y)| pos2(l + x, top + y)).collect::<Vec<_>>();
    // Coordinates relative to the button's top-left (x 18, y of the row).
    match kind {
        0 => {
            p.add(Shape::line(pts(&[(6.0, 3.5), (17.0, 3.5), (19.5, 6.0), (19.5, 10.0)]), s));
        }
        1 => {
            p.add(Shape::line(pts(&[(18.0, 3.5), (7.5, 3.5), (5.5, 5.5), (5.5, 10.0)]), s));
        }
        2 => {
            p.add(Shape::line(pts(&[(5.5, 10.0), (5.5, 5.5), (7.5, 3.5), (17.0, 3.5), (19.5, 6.0), (19.5, 10.0)]), s));
        }
        _ => {
            p.add(Shape::line(pts(&[(5.0, 3.5), (7.5, 3.5), (11.5, 7.0), (11.5, 10.0)]), s));
            p.add(Shape::line(pts(&[(20.0, 3.5), (17.5, 3.5), (13.5, 7.0), (13.5, 10.0)]), s));
        }
    }
}

pub(super) fn draw(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect) {
    let FxParams::Resonators(d) = cx.params else { return };
    let (l, t) = (r.left(), r.top());
    let scale = current_scale().filter(|_| d.scale_aware);

    if scale_btn(cx, ui, p, l + 331.0, t, rsp::SCALE, d.scale_aware) {
        set(cx, |q| q.scale_aware = !q.scale_aware);
    }
    px(p, l, t, 59.0, 21.0, 60.0, 183.0, gray(0x24));

    // Input filter.
    text_at(p, l + 30.5, t + 30.0, Align2::CENTER_BOTTOM, "Filter", font(FS), text());
    if tbutton(cx, ui, p, rr(l, t, 18.0, 33.0, 43.0, 46.0), rsp::FILTER_ON, "On", d.filter_on, true) {
        set(cx, |q| q.filter_on = !q.filter_on);
    }
    knob_lv(cx, ui, p, l + 30.5, t + 68.0, rsp::FREQ, "Frequency", &hz(d.freq), 0.0, KN, 15.5, 37.0, d.filter_on);
    for (i, y) in [121.0, 137.0, 153.0, 169.0].into_iter().enumerate() {
        let br = rr(l, t, 18.0, y, 42.0, y + 13.0);
        let sel = d.filter == i as u8;
        let (fill, col) = match (sel, d.filter_on && cx.on) {
            (true, true) => (ORANGE, gray(0x1e)),
            (true, false) => (gray(0x75), gray(0x36)),
            (false, true) => (gray(0x1e), text()),
            (false, false) => (gray(0x1e), dim()),
        };
        boxed(p, br, fill);
        filter_icon(p, l + 17.0, t + y, i as u8, col);
        cx.salt = i as u32;
        if cx.toggle(ui, br, rsp::FILTER, d.filter_on) {
            set(cx, |q| q.filter = i as u8);
        }
        cx.salt = 0;
    }

    // Mode / Decay / Color.
    text_at(p, l + 83.5, t + 30.0, Align2::CENTER_BOTTOM, "Mode", font(FS), text());
    if tbutton(cx, ui, p, rr(l, t, 71.0, 33.0, 95.0, 46.0), rsp::MODE, if d.mode_b { "B" } else { "A" }, true, true) {
        set(cx, |q| q.mode_b = !q.mode_b);
    }
    knob_lv(cx, ui, p, l + 83.5, t + 63.0, rsp::DECAY, "Decay", &sig3(d.decay * 100.0), 0.0, KN, 15.5, 37.0, true);
    if tbutton(cx, ui, p, rr(l, t, 67.0, 112.0, 98.0, 125.0), rsp::CONST, "Const", d.constant, true) {
        set(cx, |q| q.constant = !q.constant);
    }
    knob_lv(cx, ui, p, l + 83.5, t + 143.0, rsp::COLOR, "Color", &sig3(d.color * 100.0), 0.0, KN, 15.5, 37.0, true);

    // Resonators I–V.
    for (i, &x) in COLS.iter().enumerate() {
        let x = l + x;
        let en = d.on[i];
        text_at(p, x, t + 30.0, Align2::CENTER_BOTTOM, ROMAN[i], font(FS), text());
        if tbutton(cx, ui, p, Rect::from_min_max(pos2(x - 12.5, t + 34.0), pos2(x + 12.5, t + 47.0)), rsp::ON[i], "On", en, true) {
            set(cx, |q| q.on[i] = !q.on[i]);
        }
        let shown = d.pitch_of(i, scale);
        let (label, value, nonzero) = if i == 0 {
            ("Note", note_name(shown.round() as i32), (d.note - 48.0).abs() > 0.5)
        } else {
            ("Pitch", st0(d.pitch[i]), d.pitch[i].abs() > 0.5)
        };
        knob_lv(cx, ui, p, x, t + 58.0, rsp::PITCH[i], label, &value, 0.5, KN, 19.5, 44.0, en);
        marker(p, x, t + 77.5, marker_col(cx, en, nonzero));
        let fr = Rect::from_min_max(pos2(x - 13.5, t + 112.0), pos2(x + 13.5, t + 125.0));
        let bg = if en { box_fill(cx.on) } else { panel() };
        value_box_ex(cx, ui, p, fr, rsp::FINE[i], &format!("{}", d.fine[i].round() as i32), en, false, bg);
        bip_arrow(p, fr, arrow_col(cx, en, d.fine[i].abs() > 0.5));
        let gl = if i == 0 { "Output" } else { "Gain" };
        knob_lv(cx, ui, p, x, t + 142.0, rsp::GAIN[i], gl, &db2(d.gain[i]), 0.0, KN, 16.5, 38.0, en);
    }

    // Width / Output / Dry/Wet.
    knob_lv(cx, ui, p, l + 368.5, t + 30.0, rsp::WIDTH, "Width", &pct(d.width), 0.0, KN, 15.5, 37.0, true);
    knob_lv(cx, ui, p, l + 368.5, t + 84.0, rsp::OUTPUT, "Output", &db2(d.output), 0.5, KN, 17.5, 42.0, true);
    marker(p, l + 368.5, t + 101.5, marker_col(cx, true, d.output.abs() > 0.005));
    knob_lv(cx, ui, p, l + 368.5, t + 142.0, rsp::DRY_WET, "Dry/Wet", &pct(d.dry_wet), 0.0, KN, 16.5, 38.0, true);
}

// ---- parameters --------------------------------------------------------------

pub(super) fn reset(p: &mut FxParams, k: u8) {
    let d = FxParams::Resonators(ResonatorsParams::default());
    if let Some(v) = norm(&d, k) {
        set_norm(p, k, v);
    }
}

pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    let FxParams::Resonators(d) = p else { return };
    match k {
        rsp::NOTE => d.note = (d.note + dir).clamp(RES_NOTE.0, RES_NOTE.1),
        4..=7 => {
            let i = (k - 3) as usize;
            d.pitch[i] = (d.pitch[i] + dir).clamp(-RES_PITCH, RES_PITCH);
        }
        8..=12 => {
            let i = (k - 8) as usize;
            d.fine[i] = (d.fine[i] + dir).clamp(-RES_FINE, RES_FINE);
        }
        _ => {
            if let Some(v) = norm(p, k) {
                set_norm(p, k, v + dir / 100.0);
            }
        }
    }
}

pub(super) fn name(k: u8) -> &'static str {
    const NAMES: [&str; 21] = [
        "Filter Frequency",
        "Decay",
        "Color",
        "I Note",
        "II Pitch",
        "III Pitch",
        "IV Pitch",
        "V Pitch",
        "I Fine",
        "II Fine",
        "III Fine",
        "IV Fine",
        "V Fine",
        "I Gain",
        "II Gain",
        "III Gain",
        "IV Gain",
        "V Gain",
        "Width",
        "Output Gain",
        "Dry/Wet",
    ];
    match k {
        0..=20 => NAMES[k as usize],
        rsp::FILTER_ON => "Filter On",
        rsp::FILTER => "Filter Type",
        rsp::MODE => "Mode",
        rsp::CONST => "Const",
        44..=48 => ["I On", "II On", "III On", "IV On", "V On"][(k - 44) as usize],
        rsp::SCALE => "Scale Awareness",
        _ => "",
    }
}
