//! Auto Shift (Live 12): MIDI In / Input / latency column, the Quantizer,
//! MIDI and LFO tabs in the display, Pitch and Vibrato columns, and the
//! title-bar tabs, Live Mode and Use Current Scale. Geometry measured from
//! Live 12 at 100 % (l/t = device left/top; text positions are baselines).

use super::dynamics::{KN, knob_lv, sig3, tbutton};
use super::modfx::{drag_text, small_tri};
use super::*;
use daw_engine::autoshift::*;
use daw_engine::modfx::SYNC_DIVS;
use daw_project::clipx::NOTE_NAMES;
use daw_project::devices::dev_tap;
use daw_project::devices::params::asp;

pub const AS_W: f32 = 453.0;
/// Unfolded MIDI In column.
pub const AS_MIDI_W: f32 = 69.0;

pub fn width(a: &AutoShiftParams) -> f32 {
    AS_W + if a.midi_open { AS_MIDI_W } else { 0.0 }
}

/// Grey labels in the display.
const LABEL: Color32 = Color32::from_rgb(0x86, 0x86, 0x86);
/// Use Current Scale (Live's scale purple).
const KEY_ON: Color32 = Color32::from_rgb(3, 195, 213);
const KEY_OFF: Color32 = Color32::from_rgb(19, 58, 61);
const BAR_L: Color32 = Color32::from_rgb(70, 54, 36);
const BAR_R: Color32 = Color32::from_rgb(20, 58, 62);

fn ms3(x: f32) -> String {
    if x >= 999.5 { format!("{} s", sig3(x / 1000.0)) } else { format!("{} ms", sig3(x)) }
}

fn pct1(x: f32) -> String {
    format!("{:.1} %", x * 100.0)
}

fn note_name(m: f32) -> String {
    let n = m.round() as i32;
    format!("{}{}", NOTE_NAMES[n.rem_euclid(12) as usize], n.div_euclid(12) - 2)
}

/// 7x7 / 9x9 square check box: filled when on, outlined when off.
fn check(p: &Painter, r: Rect, on: bool, col: Color32) {
    if on {
        p.rect_filled(r, CornerRadius::ZERO, col);
    } else {
        p.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, col), egui::StrokeKind::Inside);
    }
}

/// Text centred at `x` (draggable value).
#[allow(clippy::too_many_arguments)]
fn drag_c(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, x: f32, base: f32, k: u8, s: &str, en: bool, col: Color32) {
    let w = p.layout_no_wrap(s.to_string(), font(FS), col).size().x;
    drag_text(cx, ui, p, x - w / 2.0, base, k, s, en, col);
}

/// Dropdown on a text label; `enabled(i)` greys menu items.
#[allow(clippy::too_many_arguments)]
fn menu_at(
    cx: &mut Ctx,
    ui: &mut egui::Ui,
    r: Rect,
    k: u8,
    cur: usize,
    items: &[&str],
    en: bool,
    item_on: &dyn Fn(usize) -> bool,
) -> Option<usize> {
    let resp = cx.sense(ui, r, k, Sense::click());
    cx.param_menu(&resp, k);
    if resp.clicked() {
        cx.focus(k);
    }
    let mut pick = None;
    if en {
        menu::dropdown_aligned(&resp, r.width().max(110.0), egui::RectAlign::BOTTOM_START, |ui| {
            for (i, it) in items.iter().enumerate() {
                if menu::check_item(ui, it, "", i == cur, item_on(i)) {
                    pick = Some(i);
                }
            }
        });
    }
    if pick.is_some() {
        cx.undo_step();
    }
    pick
}

fn set(cx: &mut Ctx, f: impl FnOnce(&mut AutoShiftParams)) {
    if let FxParams::AutoShift(a) = &mut cx.params {
        f(a);
    }
}

// ---- the device ------------------------------------------------------------

pub(super) fn auto_shift(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect) {
    let FxParams::AutoShift(a0) = cx.params else { return };
    let (l0, t) = (r.left(), r.top());
    let tap = dev_tap(cx.dev.id);
    if cx.on {
        cx.app.animating = true;
    }
    p.rect_filled(Rect::from_min_max(pos2(l0, t + 17.0), r.max), CornerRadius { nw: 0, ne: 0, sw: 3, se: 3 }, gray(0x46));
    let pnl = |x0: f32, y0: f32, x1: f32, y1: f32| {
        p.rect_filled(Rect::from_min_max(pos2(x0, t + y0), pos2(x1, t + y1)), CornerRadius::same(2), panel());
    };
    let ox = if a0.midi_open { AS_MIDI_W } else { 0.0 };
    let l = l0 + ox;

    // ---- left column: MIDI In, Input, latency ----
    pnl(l0 + 2.0, 17.0, l0 + 62.0, 39.0);
    pnl(l0 + 2.0, 40.0, l0 + 62.0, 166.0);
    pnl(l0 + 2.0, 167.0, l0 + 62.0, 187.0);
    let mc = pos2(l0 + 12.5, t + 27.5);
    p.circle_filled(mc, 6.5, gray(0x1e));
    let tri = if a0.midi_open {
        vec![pos2(mc.x - 2.0, mc.y - 3.5), pos2(mc.x + 3.0, mc.y), pos2(mc.x - 2.0, mc.y + 3.5)]
    } else {
        vec![pos2(mc.x - 3.5, mc.y - 2.0), pos2(mc.x + 3.5, mc.y - 2.0), pos2(mc.x, mc.y + 3.0)]
    };
    p.add(Shape::convex_polygon(tri, gray(0xc8), Stroke::NONE));
    text_at(p, l0 + 24.0, t + 31.0, Align2::LEFT_BOTTOM, "MIDI In", font(FS), text());
    if cx.toggle(ui, Rect::from_min_max(pos2(l0 + 4.0, t + 19.0), pos2(l0 + 60.0, t + 37.0)), asp::MIDI_OPEN, true) {
        set(cx, |a| a.midi_open = !a.midi_open);
    }
    text_at(p, l0 + 31.5, t + 54.0, Align2::CENTER_BOTTOM, "Input", font(FS), text());
    quad(p, Rect::from_min_max(pos2(l0 + 6.0, t + 58.0), pos2(l0 + 58.0, t + 59.0)), gray(0x24));
    let det = tap.get(tapv::INPUT);
    if det > 0.0 && cx.on {
        text_at(p, l0 + 31.5, t + 76.0, Align2::CENTER_BOTTOM, &note_name(det), font(FS), text());
    } else {
        quad(p, Rect::from_min_max(pos2(l0 + 27.0, t + 71.0), pos2(l0 + 37.0, t + 73.0)), gray(0x5a));
    }
    for (i, name) in AS_RANGES.iter().enumerate() {
        let rr = Rect::from_min_size(pos2(l0 + 6.0, t + 84.0 + 16.0 * i as f32), vec2(51.0, 13.0));
        let sel = a0.range as usize == i;
        let (fill, col) = match (sel, cx.on) {
            (true, true) => (ORANGE, gray(0x12)),
            (true, false) => (mix(ORANGE, gray(0x36), 0.5), gray(0x24)),
            (false, _) => (gray(0x1e), text()),
        };
        boxed(p, rr, fill);
        p.circle_filled(pos2(rr.left() + 6.5, rr.center().y), 3.0, if sel { gray(0x24) } else { gray(0x26) });
        text_at(p, rr.center().x + 3.0, rr.top() + 10.0, Align2::CENTER_BOTTOM, name, font(FS), col);
        cx.salt = i as u32;
        if cx.toggle(ui, rr, asp::RANGE, true) {
            set(cx, |a| a.range = i as u8);
        }
        cx.salt = 0;
    }
    text_at(p, l0 + 31.5, t + 145.0, Align2::CENTER_BOTTOM, "Gain", font(FS), text());
    value_box(
        cx,
        ui,
        p,
        Rect::from_min_size(pos2(l0 + 6.0, t + 148.0), vec2(51.0, 13.0)),
        asp::IN_GAIN,
        &format!("{:.1} dB", a0.in_gain),
        true,
    );
    let lat = if a0.live { 0.0 } else { AS_LAT_MS[(a0.range as usize).min(2)] };
    text_at(p, l0 + 10.0, t + 180.0, Align2::LEFT_BOTTOM, &format!("L {lat:.1} ms"), font(FS), text());

    if a0.midi_open {
        midi_column(cx, ui, p, l0, t, &a0);
    }

    // ---- title bar: tabs, Live Mode, Use Current Scale ----
    let midi_tab = a0.midi_mode();
    for (i, (x, label, on, col)) in [
        (116.0, if midi_tab { "MIDI" } else { "Quantizer" }, if midi_tab { true } else { a0.quant }, TEAL),
        (197.0, "LFO", a0.lfo_on, ORANGE),
    ]
    .into_iter()
    .enumerate()
    {
        let sel = a0.tab as usize == i;
        let tr = Rect::from_min_max(pos2(l + x, t + 2.0), pos2(l + x + 80.0, t + if sel { 17.0 } else { 15.0 }));
        p.rect_filled(tr, CornerRadius { nw: 2, ne: 2, sw: if sel { 0 } else { 2 }, se: if sel { 0 } else { 2 } }, gray(0x18));
        let cb = Rect::from_min_size(pos2(l + x + 9.0, t + 5.0), vec2(7.0, 7.0));
        check(p, cb, on, if cx.on { col } else { gray(0x75) });
        text_at(p, l + x + 20.0, t + 12.0, Align2::LEFT_BOTTOM, label, font(FS), if sel { text() } else { LABEL });
        // The check box switches the section, the rest selects the tab.
        cx.salt = i as u32;
        if (!midi_tab || i == 1) && cx.toggle(ui, cb.expand(2.0), if i == 0 { asp::QUANT } else { asp::LFO_ON }, true) {
            set(cx, |a| if i == 0 { a.quant = !a.quant } else { a.lfo_on = !a.lfo_on });
        }
        if cx.toggle(ui, Rect::from_min_max(pos2(cb.right() + 2.0, tr.top()), tr.max), asp::TAB, true) {
            set(cx, |a| a.tab = i as u8);
        }
        cx.salt = 0;
    }
    let lm = Rect::from_min_size(pos2(l + 312.0, t + 4.0), vec2(9.0, 9.0));
    boxed(
        p,
        lm,
        if a0.live && cx.on {
            ORANGE
        } else if a0.live {
            gray(0x75)
        } else {
            gray(0x1e)
        },
    );
    let lt = text_at(p, l + 328.0, t + 12.0, Align2::LEFT_BOTTOM, "Live Mode", font(FS), text());
    if cx.toggle(ui, Rect::from_min_max(pos2(lm.left() - 2.0, t + 2.0), pos2(lt.right() + 2.0, t + 15.0)), asp::LIVE, true) {
        set(cx, |a| a.live = !a.live);
    }
    if super::fx4::scale_btn(cx, ui, p, l + 389.0, t, asp::USE_CUR, a0.use_current) {
        set(cx, |a| a.use_current = !a.use_current);
    }

    // ---- display ----
    let disp = Rect::from_min_max(pos2(l + 63.0, t + 17.0), pos2(l + 330.0, t + 187.0));
    p.rect_filled(disp, CornerRadius::same(2), gray(0x18));
    match (a0.tab, midi_tab) {
        (1, _) => lfo_tab(cx, ui, p, l, t, &a0),
        (_, true) => midi_tab_ui(cx, ui, p, l, t, &a0),
        _ => quantizer(cx, ui, p, l, t, &a0),
    }

    // ---- Pitch column ----
    pnl(l + 331.0, 17.0, l + 391.0, 187.0);
    knob_lv(cx, ui, p, l + 360.5, t + 30.0, asp::PITCH, "Pitch", &format!("{:.0} st", a0.pitch), 0.5, KN, 13.5, 36.0, true);
    text_at(p, l + 360.5, t + 82.0, Align2::CENTER_BOTTOM, "Fine", font(FS), text());
    value_box(cx, ui, p, Rect::from_min_size(pos2(l + 335.0, t + 85.0), vec2(51.0, 13.0)), asp::FINE, &format!("{:.0} ct", a0.fine), true);
    knob_lv(cx, ui, p, l + 360.5, t + 114.0, asp::FORMANT, "Formant", &pct1(a0.formant), 0.5, KN, 13.5, 36.0, true);
    text_at(p, l + 360.5, t + 166.0, Align2::CENTER_BOTTOM, "F. Follow", font(FS), text());
    value_box(cx, ui, p, Rect::from_min_size(pos2(l + 335.0, t + 169.0), vec2(51.0, 13.0)), asp::F_FOLLOW, &pct1(a0.f_follow), true);

    // ---- Vibrato column ----
    pnl(l + 392.0, 17.0, l + 452.0, 187.0);
    let lc = pos2(l + 399.5, t + 26.5);
    p.circle_filled(lc, 3.5, if a0.vib_on && cx.on { ORANGE } else { gray(0x1e) });
    text_at(p, l + 406.0, t + 30.0, Align2::LEFT_BOTTOM, "Vibrato", font(FS), text());
    if cx.toggle(ui, Rect::from_min_max(pos2(l + 394.0, t + 19.0), pos2(l + 450.0, t + 33.0)), asp::VIB_ON, true) {
        set(cx, |a| a.vib_on = !a.vib_on);
    }
    // The Vibrato controls stay editable with the switch off (Live).
    let v = true;
    knob_lv(cx, ui, p, l + 421.5, t + 33.0, asp::VIB_AMT, "", &format!("{:.0} ct", a0.vib_amt), 0.0, KN, 10.5, 33.0, v);
    text_at(p, l + 421.5, t + 82.0, Align2::CENTER_BOTTOM, "Rate", font(FS), text());
    value_box(
        cx,
        ui,
        p,
        Rect::from_min_size(pos2(l + 396.0, t + 85.0), vec2(51.0, 13.0)),
        asp::VIB_RATE,
        &format!("{:.2} Hz", a0.vib_rate),
        v,
    );
    text_at(p, l + 421.5, t + 112.0, Align2::CENTER_BOTTOM, "Fade-In", font(FS), text());
    value_box(cx, ui, p, Rect::from_min_size(pos2(l + 396.0, t + 115.0), vec2(51.0, 13.0)), asp::VIB_FADE, &ms3(a0.vib_fade), v);
    if tbutton(cx, ui, p, Rect::from_min_size(pos2(l + 396.0, t + 133.0), vec2(51.0, 13.0)), asp::NATURAL, "Natural", a0.natural, v) {
        set(cx, |a| a.natural = !a.natural);
    }
    quad(p, Rect::from_min_max(pos2(l + 396.0, t + 151.0), pos2(l + 449.0, t + 152.0)), gray(0x24));
    text_at(p, l + 421.5, t + 166.0, Align2::CENTER_BOTTOM, "Dry/Wet", font(FS), text());
    value_box(
        cx,
        ui,
        p,
        Rect::from_min_size(pos2(l + 396.0, t + 169.0), vec2(51.0, 13.0)),
        asp::DRY_WET,
        &format!("{:.0} %", a0.dry_wet * 100.0),
        true,
    );
}

/// Unfolded MIDI In column: MIDI On, External Source / tap, Mono / Poly,
/// Voices, Glide.
fn midi_column(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, a0: &AutoShiftParams) {
    p.rect_filled(Rect::from_min_max(pos2(l + 63.0, t + 17.0), pos2(l + 130.0, t + 186.0)), CornerRadius::same(2), panel());
    let on = a0.midi.on;
    if tbutton(cx, ui, p, Rect::from_min_size(pos2(l + 67.0, t + 21.0), vec2(59.0, 13.0)), asp::MIDI_ON, "MIDI On", on, true) {
        set(cx, |a| a.midi.on = !a.midi.on);
    }
    // External Source: MIDI tracks other than this one.
    let own = match crate::fx_ops::top_of(&cx.app.project, cx.chain) {
        ChainRef::Track(id) => Some(id),
        _ => None,
    };
    let prj = &cx.app.project;
    let tracks: Vec<(u64, String)> = prj
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, x)| Some(x.id) != own && x.kind == daw_project::TrackKind::Midi)
        .map(|(i, x)| (x.id, prj.track_label(i)))
        .collect();
    let cur = tracks.iter().find(|x| x.0 == a0.midi.source).map_or("No Input".to_string(), |x| x.1.clone());
    let mut items: Vec<&str> = vec!["No Input"];
    items.extend(tracks.iter().map(|x| x.1.as_str()));
    let cr = Rect::from_min_size(pos2(l + 67.0, t + 41.0), vec2(59.0, 13.0));
    let shown = fit(p, &cur, &font(FS), cr.width() - 14.0);
    if let Some(i) = chooser(cx, ui, p, cr, asp::SOURCE, &shown, &items, on) {
        let src = if i == 0 { 0 } else { tracks[i - 1].0 };
        set(cx, |a| a.midi.source = src);
    }
    let ext = on && a0.midi.source != 0;
    let taps: Vec<&str> = ScTap::ALL.iter().map(|x| x.label()).collect();
    let tr = Rect::from_min_size(pos2(l + 67.0, t + 57.0), vec2(59.0, 13.0));
    let tap_s = if ext { a0.midi.tap.label() } else { "" };
    if let Some(i) = chooser(cx, ui, p, tr, asp::TAP, tap_s, &taps, ext) {
        set(cx, |a| a.midi.tap = ScTap::ALL[i]);
    }
    for (i, label) in ["Mono", "Poly"].into_iter().enumerate() {
        let rr = Rect::from_min_size(pos2(l + 67.0, t + 77.0 + 16.0 * i as f32), vec2(59.0, 13.0));
        cx.salt = i as u32;
        if tbutton(cx, ui, p, rr, asp::POLY, label, a0.poly == (i == 1), on) {
            set(cx, |a| a.poly = i == 1);
        }
        cx.salt = 0;
    }
    text_at(p, l + 96.5, t + 122.0, Align2::CENTER_BOTTOM, "Voices", font(FS), if on && a0.poly { text() } else { dim() });
    let vs: Vec<String> = AS_VOICES.iter().map(|v| v.to_string()).collect();
    let vi: Vec<&str> = vs.iter().map(|s| s.as_str()).collect();
    let vr = Rect::from_min_size(pos2(l + 67.0, t + 126.0), vec2(59.0, 13.0));
    if let Some(i) = chooser(cx, ui, p, vr, asp::VOICES, vi[(a0.voices as usize).min(2)], &vi, on && a0.poly) {
        set(cx, |a| a.voices = i as u8);
    }
    text_at(p, l + 96.5, t + 166.0, Align2::CENTER_BOTTOM, "Glide", font(FS), if on && !a0.poly { text() } else { dim() });
    value_box(cx, ui, p, Rect::from_min_size(pos2(l + 67.0, t + 169.0), vec2(59.0, 13.0)), asp::GLIDE, &ms3(a0.glide), on && !a0.poly);
}

/// Quantizer tab: correction meter, Strength / Smooth, Included Notes,
/// Root / Scale / Shift.
fn quantizer(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, a0: &AutoShiftParams) {
    let q = a0.quant && cx.on;
    let lab = if q { LABEL } else { gray(0x6a) };
    text_at(p, l + 77.0, t + 33.0, Align2::LEFT_BOTTOM, "-100 ct", font(FS), lab);
    text_at(p, l + 196.0, t + 33.0, Align2::CENTER_BOTTOM, "0 ct", font(FS), lab);
    text_at(p, l + 320.0, t + 33.0, Align2::RIGHT_BOTTOM, "+100 ct", font(FS), lab);
    // Correction meter: lit bars from the centre out to the correction.
    let cents = if q { dev_tap(cx.dev.id).get(tapv::CENTS).clamp(-100.0, 100.0) } else { 0.0 };
    let lit = (cents.abs() / 100.0 * 41.0).round() as usize;
    for i in 0..41 {
        let (xl, xr) = (l + 72.0 + 3.0 * i as f32, l + 198.0 + 3.0 * i as f32);
        let on_l = cents < 0.0 && 40 - i < lit;
        let on_r = cents > 0.0 && i < lit;
        let (cl, cr) = if q { (if on_l { ORANGE } else { BAR_L }, if on_r { TEAL } else { BAR_R }) } else { (gray(0x2e), gray(0x2e)) };
        quad(p, Rect::from_min_max(pos2(xl, t + 37.0), pos2(xl + 2.0, t + 51.0)), cl);
        quad(p, Rect::from_min_max(pos2(xr, t + 37.0), pos2(xr + 2.0, t + 51.0)), cr);
    }
    text_at(p, l + 130.5, t + 69.0, Align2::CENTER_BOTTOM, "Strength", font(FS), lab);
    drag_c(cx, ui, p, l + 130.5, t + 85.0, asp::STRENGTH, &format!("{:.0} %", a0.strength * 100.0), a0.quant, TEAL);
    let sb = Rect::from_min_size(pos2(l + 237.0, t + 61.0), vec2(9.0, 9.0));
    check(p, sb, a0.smooth, if q { TEAL } else { gray(0x6a) });
    let st = text_at(p, l + 251.0, t + 69.0, Align2::LEFT_BOTTOM, "Smooth", font(FS), lab);
    if cx.toggle(ui, Rect::from_min_max(pos2(sb.left() - 2.0, t + 58.0), pos2(st.right() + 2.0, t + 72.0)), asp::SMOOTH, a0.quant) {
        set(cx, |a| a.smooth = !a.smooth);
    }
    drag_c(cx, ui, p, l + 262.5, t + 85.0, asp::SMOOTH_MS, &ms3(a0.smooth_ms), a0.quant && a0.smooth, TEAL);
    // Target note.
    let target = dev_tap(cx.dev.id).get(tapv::TARGET);
    if q && target > 0.0 && dev_tap(cx.dev.id).get(tapv::INPUT) > 0.0 {
        text_at(p, l + 192.0, t + 77.0, Align2::CENTER_BOTTOM, &note_name(target), font(FS), text());
    } else {
        quad(p, Rect::from_min_max(pos2(l + 182.0, t + 73.0), pos2(l + 203.0, t + 74.0)), gray(0x2e));
    }
    // Included notes: click a key for a Custom scale.
    let (root, mask) = a0.included();
    let keys_en = a0.quant && !a0.use_current;
    let (k_on, k_off) = if q { (KEY_ON, KEY_OFF) } else { (gray(0x86), gray(0x3e)) };
    let mut click: Option<u8> = None;
    let mut key = |cx: &mut Ctx, rr: Rect, pc: u8, label_base: f32| {
        let inc = mask & (1 << pc) != 0;
        quad(p, rr, if inc { k_on } else { k_off });
        let col = if inc {
            k_off
        } else if q {
            k_on
        } else {
            gray(0x86)
        };
        text_at(p, rr.center().x, label_base, Align2::CENTER_BOTTOM, NOTE_NAMES[pc as usize], font(FS), col);
        cx.salt = pc as u32;
        if cx.toggle(ui, rr, asp::KEY, keys_en) {
            click = Some(pc);
        }
        cx.salt = 0;
    };
    const WHITE: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
    for (i, &pc) in WHITE.iter().enumerate() {
        let x = l + 99.0 + 28.0 * i as f32;
        key(cx, Rect::from_min_max(pos2(x, t + 97.0), pos2(x + 27.0, t + 158.0)), pc, t + 153.0);
    }
    for (i, &pc) in WHITE.iter().enumerate() {
        if matches!(pc, 4 | 11) {
            continue;
        }
        let x = l + 99.0 + 28.0 * i as f32 + 19.0;
        quad(p, Rect::from_min_max(pos2(x - 1.0, t + 97.0), pos2(x + 19.0, t + 128.0)), gray(0x18));
        key(cx, Rect::from_min_max(pos2(x, t + 97.0), pos2(x + 18.0, t + 127.0)), pc + 1, t + 122.0);
    }
    if let Some(pc) = click {
        set(cx, |a| {
            a.custom = mask ^ (1 << pc);
            a.root = root;
            a.scale = AS_SCALE_CUSTOM;
        });
    }
    // Root / Scale / Shift.
    let rs = a0.quant && !a0.use_current;
    let rsc = if rs && cx.on { TEAL } else { gray(0x6a) };
    text_at(p, l + 71.0, t + 178.0, Align2::LEFT_BOTTOM, "Root", font(FS), lab);
    text_at(p, l + 97.0, t + 178.0, Align2::LEFT_BOTTOM, NOTE_NAMES[(root % 12) as usize], font(FS), rsc);
    small_tri(p, l + 120.0, t + 174.0, rsc);
    if let Some(i) = menu_at(
        cx,
        ui,
        Rect::from_min_max(pos2(l + 69.0, t + 166.0), pos2(l + 128.0, t + 181.0)),
        asp::ROOT,
        root as usize,
        &NOTE_NAMES,
        rs,
        &|_| true,
    ) {
        set(cx, |a| {
            // A Custom scale moves with its root.
            if a.scale == AS_SCALE_CUSTOM {
                let d = (i as i32 - a.root as i32).rem_euclid(12) as u32;
                let m = a.custom as u32 & 0xFFF;
                a.custom = (((m << d) | (m >> (12 - d))) & 0xFFF) as u16;
            }
            a.root = i as u8;
        });
    }
    text_at(p, l + 134.0, t + 178.0, Align2::LEFT_BOTTOM, "Scale", font(FS), lab);
    // Use Current Scale: the name of the scale it follows.
    let name = match current_scale().filter(|_| a0.use_current) {
        Some((r0, m)) => SCALES.iter().find(|x| scale_mask(r0, x.1) == m).map_or("Custom", |x| x.0),
        None => a0.scale_name(),
    };
    let shown = fit(p, name, &font(FS), 78.0);
    text_at(p, l + 164.0, t + 178.0, Align2::LEFT_BOTTOM, &shown, font(FS), rsc);
    small_tri(p, l + 246.0, t + 174.0, rsc);
    let names: Vec<&str> = as_scale_names().collect();
    if let Some(i) = menu_at(
        cx,
        ui,
        Rect::from_min_max(pos2(l + 132.0, t + 166.0), pos2(l + 254.0, t + 181.0)),
        asp::SCALE,
        a0.scale as usize,
        &names,
        rs,
        &|_| true,
    ) {
        set(cx, |a| {
            if i as u8 == AS_SCALE_CUSTOM && a.scale != AS_SCALE_CUSTOM {
                a.custom = a.included().1;
            }
            a.scale = i as u8;
        });
    }
    text_at(p, l + 263.0, t + 178.0, Align2::LEFT_BOTTOM, "Shift", font(FS), lab);
    drag_text(cx, ui, p, l + 290.0, t + 178.0, asp::SHIFT, &format!("{} sd", a0.shift), a0.quant, TEAL);
}

/// MIDI tab (MIDI On with a source): envelope, Note Latch, PB Range and the
/// Mod Routing of Pitch / Formant / Volume / Pan.
fn midi_tab_ui(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, a0: &AutoShiftParams) {
    let col = TEAL;
    text_at(p, l + 71.0, t + 33.0, Align2::LEFT_BOTTOM, "Attack", font(FS), LABEL);
    drag_text(cx, ui, p, l + 106.0, t + 33.0, asp::ATTACK, &ms3(a0.attack), true, col);
    text_at(p, l + 160.0, t + 33.0, Align2::LEFT_BOTTOM, "Release", font(FS), LABEL);
    drag_text(cx, ui, p, l + 201.0, t + 33.0, asp::RELEASE, &ms3(a0.release), true, col);
    text_at(p, l + 254.0, t + 33.0, Align2::LEFT_BOTTOM, "PB", font(FS), LABEL);
    drag_text(cx, ui, p, l + 272.0, t + 33.0, asp::PB_RANGE, &format!("{} st", a0.pb_range), true, col);
    let lb = Rect::from_min_size(pos2(l + 71.0, t + 46.0), vec2(9.0, 9.0));
    check(p, lb, a0.latch, if cx.on { col } else { gray(0x75) });
    let lt = text_at(p, l + 85.0, t + 54.0, Align2::LEFT_BOTTOM, "Note Latch", font(FS), LABEL);
    if cx.toggle(ui, Rect::from_min_max(pos2(lb.left() - 2.0, t + 43.0), pos2(lt.right() + 2.0, t + 57.0)), asp::LATCH, true) {
        set(cx, |a| a.latch = !a.latch);
    }
    routing_header(p, l, t + 100.0);
    for (i, target) in AS_TARGETS.iter().enumerate() {
        let x = l + 71.0 + 64.0 * i as f32;
        let src = a0.mod_src[i] as usize;
        let c = if cx.on { col } else { gray(0x5a) };
        let tr = text_at(p, x, t + 122.0, Align2::LEFT_BOTTOM, AS_MOD_SRC[src.min(5)], font(FS), c);
        small_tri(p, tr.right() + 3.0, t + 118.0, c);
        cx.salt = i as u32;
        // Only Velocity is generated by the engine; the others are greyed.
        if let Some(s) = menu_at(
            cx,
            ui,
            Rect::from_min_max(pos2(x - 2.0, t + 110.0), pos2(x + 60.0, t + 125.0)),
            asp::MOD_SRC + i as u8,
            src,
            &AS_MOD_SRC,
            true,
            &|j| j == 0,
        ) {
            set(cx, |a| a.mod_src[i] = s as u8);
        }
        cx.salt = 0;
        let v = a0.mod_amt[i];
        let s = if i == 0 { format!("{:.2} st", v * AS_MOD_ST) } else { pct1(v) };
        drag_text(cx, ui, p, x, t + 146.0, asp::MOD_AMT + i as u8, &s, true, col);
        text_at(p, x, t + 162.0, Align2::LEFT_BOTTOM, target, font(FS), LABEL);
    }
}

fn routing_header(p: &Painter, l: f32, base: f32) {
    let r = text_at(p, l + 71.0, base, Align2::LEFT_BOTTOM, "Mod Routing", font(FS), LABEL);
    quad(p, Rect::from_min_max(pos2(r.right() + 6.0, base - 1.0), pos2(l + 321.0, base)), gray(0x3e));
}

/// LFO tab: Onset / Reset, Delay, Fade-In, the scope, Wave, Rate and the
/// depths per target.
fn lfo_tab(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, a0: &AutoShiftParams) {
    let on = a0.lfo_on && cx.on;
    let acc = if on { ORANGE } else { gray(0x5a) };
    let tap = dev_tap(cx.dev.id);
    // Onset LED (lit 100 ms after an onset).
    let oid = egui::Id::unique(("as_onset", cx.dev.id));
    let now = ui.input(|i| i.time);
    if tap.take(tapv::ONSET, 0.0) > 0.0 {
        ui.data_mut(|d| d.insert_temp(oid, now));
    }
    let lit = ui.data(|d| d.get_temp::<f64>(oid)).is_some_and(|t0| now - t0 < 0.1);
    p.circle_filled(pos2(l + 75.0, t + 29.5), 3.0, if lit && on { ORANGE } else { gray(0x38) });
    text_at(p, l + 83.0, t + 33.0, Align2::LEFT_BOTTOM, "Onset", font(FS), LABEL);
    let rb = Rect::from_min_size(pos2(l + 118.0, t + 24.0), vec2(13.0, 13.0));
    p.rect_filled(rb, CornerRadius::same(1), if a0.lfo_reset { acc } else { gray(0x1e) });
    text_at(p, rb.center().x, rb.top() + 10.0, Align2::CENTER_BOTTOM, "R", font(FS), if a0.lfo_reset { gray(0x12) } else { LABEL });
    if cx.toggle(ui, rb, asp::LFO_RESET, true) {
        set(cx, |a| a.lfo_reset = !a.lfo_reset);
    }
    text_at(p, l + 162.0, t + 33.0, Align2::LEFT_BOTTOM, "Delay", font(FS), LABEL);
    drag_text(cx, ui, p, l + 194.0, t + 33.0, asp::LFO_DELAY, &ms3(a0.lfo_delay), a0.lfo_on, ORANGE);
    text_at(p, l + 242.0, t + 33.0, Align2::LEFT_BOTTOM, "Fade-In", font(FS), LABEL);
    drag_text(cx, ui, p, l + 284.0, t + 33.0, asp::LFO_FADE, &ms3(a0.lfo_fade), a0.lfo_on, ORANGE);
    // Scope: 110 px per cycle, scrolling with the LFO phase.
    let b = Rect::from_min_max(pos2(l + 71.0, t + 42.0), pos2(l + 322.0, t + 108.0));
    p.rect_stroke(b, CornerRadius::ZERO, Stroke::new(1.0, gray(0x3e)), egui::StrokeKind::Inside);
    quad(p, Rect::from_min_max(pos2(b.left() + 1.0, t + 75.0), pos2(b.right() - 1.0, t + 76.0)), gray(0x3e));
    let ph = if a0.lfo_on { tap.get(tapv::LFO_PH) as f64 } else { 0.0 };
    let n = ((b.width() - 2.0) * 2.0) as usize;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let x = b.left() + 1.0 + i as f32 * 0.5;
            let c = ph + ((x - b.left() - 1.0) / 110.0) as f64;
            pos2(x, t + 75.5 - 31.5 * as_lfo_shape(a0.lfo_wave, c).clamp(-1.0, 1.0))
        })
        .collect();
    p.with_clip_rect(b.shrink(1.0))
        .add(Shape::line(pts, Stroke::new(1.5, if on { Color32::from_rgb(0x3d, 0xb3, 0xba) } else { gray(0x5a) })));
    // Wave chooser (orange box) and rate.
    text_at(p, l + 71.0, t + 123.0, Align2::LEFT_BOTTOM, "Wave", font(FS), LABEL);
    let wr = Rect::from_min_max(pos2(l + 105.0, t + 115.0), pos2(l + 181.0, t + 128.0));
    p.rect_filled(wr, CornerRadius::ZERO, acc);
    text_at(p, wr.left() + 3.0, wr.top() + 10.0, Align2::LEFT_BOTTOM, AS_WAVES[(a0.lfo_wave as usize).min(8)], font(FS), gray(0x12));
    p.add(Shape::convex_polygon(
        vec![pos2(wr.right() - 12.0, t + 119.0), pos2(wr.right() - 4.0, t + 119.0), pos2(wr.right() - 8.0, t + 124.0)],
        gray(0x12),
        Stroke::NONE,
    ));
    if let Some(i) = menu_at(cx, ui, wr, asp::LFO_WAVE, a0.lfo_wave as usize, &AS_WAVES, a0.lfo_on, &|_| true) {
        set(cx, |a| a.lfo_wave = i as u8);
    }
    text_at(p, l + 215.0, t + 123.0, Align2::LEFT_BOTTOM, "Rate", font(FS), LABEL);
    if a0.lfo_sync {
        let d = SYNC_DIVS[(a0.lfo_div as usize).min(SYNC_DIVS.len() - 1)].1;
        drag_text(cx, ui, p, l + 243.0, t + 123.0, asp::LFO_DIV, d, a0.lfo_on, ORANGE);
    } else {
        drag_text(cx, ui, p, l + 243.0, t + 123.0, asp::LFO_RATE, &format!("{} Hz", sig3(a0.lfo_rate)), a0.lfo_on, ORANGE);
    }
    for (i, x) in [285.0, 304.0].into_iter().enumerate() {
        let br = Rect::from_min_max(pos2(l + x, t + 115.0), pos2(l + x + 16.0, t + 128.0));
        let sel = a0.lfo_sync == (i == 1);
        if sel {
            p.rect_filled(br, CornerRadius::ZERO, acc);
        } else {
            p.rect_stroke(br, CornerRadius::ZERO, Stroke::new(1.0, acc), egui::StrokeKind::Inside);
        }
        let gc = if sel { gray(0x12) } else { acc };
        if i == 0 {
            text_at(p, br.center().x, br.top() + 10.0, Align2::CENTER_BOTTOM, "Hz", font(FS), gc);
        } else {
            // Note glyph.
            p.circle_filled(pos2(br.center().x - 2.0, br.bottom() - 4.0), 2.0, gc);
            p.line_segment([pos2(br.center().x - 0.5, br.bottom() - 4.0), pos2(br.center().x - 0.5, br.top() + 3.0)], Stroke::new(1.0, gc));
            p.line_segment([pos2(br.center().x - 0.5, br.top() + 3.0), pos2(br.center().x + 3.0, br.top() + 5.0)], Stroke::new(1.0, gc));
        }
        cx.salt = i as u32;
        if cx.toggle(ui, br, asp::LFO_SYNC, a0.lfo_on) {
            set(cx, |a| a.lfo_sync = i == 1);
        }
        cx.salt = 0;
    }
    routing_header(p, l, t + 146.0);
    for (i, target) in AS_TARGETS.iter().enumerate() {
        let x = l + 71.0 + 64.0 * i as f32;
        let v = a0.lfo_amt[i];
        let s = if i == 0 { format!("{:.2} st", v * AS_MOD_ST) } else { pct1(v) };
        drag_text(cx, ui, p, x, t + 162.0, asp::LFO_AMT + i as u8, &s, a0.lfo_on, ORANGE);
        text_at(p, x, t + 178.0, Align2::LEFT_BOTTOM, target, font(FS), LABEL);
    }
}

// ---- parameters --------------------------------------------------------------

pub(super) fn reset(p: &mut FxParams, k: u8) {
    let d = FxParams::AutoShift(AutoShiftParams::default());
    if let Some(v) = norm(&d, k) {
        set_norm(p, k, v);
    }
}

pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    let FxParams::AutoShift(a) = p else { return };
    let d = dir as i32;
    match k {
        asp::SHIFT => a.shift = (a.shift as i32 + d).clamp(-(AS_SHIFT as i32), AS_SHIFT as i32) as i8,
        asp::PITCH => a.pitch = (a.pitch + dir).clamp(-AS_PITCH_ST, AS_PITCH_ST),
        asp::FINE => a.fine = (a.fine.round() + dir).clamp(-AS_FINE_CT, AS_FINE_CT),
        asp::PB_RANGE => a.pb_range = (a.pb_range as i32 + d).clamp(0, AS_PB_MAX as i32) as u8,
        asp::LFO_DIV => a.lfo_div = (a.lfo_div as i32 + d).clamp(0, SYNC_DIVS.len() as i32 - 1) as u8,
        asp::RANGE => a.range = (a.range as i32 - d).clamp(0, 2) as u8,
        asp::ROOT => a.root = (a.root as i32 + d).rem_euclid(12) as u8,
        asp::SCALE => a.scale = (a.scale as i32 - d).clamp(0, AS_SCALE_COUNT as i32 - 1) as u8,
        asp::LFO_WAVE => a.lfo_wave = (a.lfo_wave as i32 - d).clamp(0, 8) as u8,
        asp::VOICES => a.voices = (a.voices as i32 - d).clamp(0, 2) as u8,
        _ => {
            if let Some(v) = norm(p, k) {
                set_norm(p, k, v + dir / 200.0);
            }
        }
    }
}

pub(super) fn name(k: u8) -> &'static str {
    match k {
        asp::IN_GAIN => "Input Gain",
        asp::GLIDE => "Glide",
        asp::STRENGTH => "Correction Strength",
        asp::SMOOTH_MS => "Smoothing Time",
        asp::SHIFT => "Pitch Shift",
        asp::ATTACK => "Attack",
        asp::RELEASE => "Release",
        asp::PB_RANGE => "PB Range",
        8 => "Pitch Mod Amount",
        9 => "Formant Mod Amount",
        10 => "Volume Mod Amount",
        11 => "Pan Mod Amount",
        asp::LFO_DELAY => "LFO Delay",
        asp::LFO_FADE => "LFO Fade-In",
        asp::LFO_RATE => "LFO Rate",
        asp::LFO_DIV => "LFO Synced Rate",
        16 => "LFO Pitch Amount",
        17 => "LFO Formant Amount",
        18 => "LFO Volume Amount",
        19 => "LFO Pan Amount",
        asp::PITCH => "Pitch",
        asp::FINE => "Fine",
        asp::FORMANT => "Formant",
        asp::F_FOLLOW => "Formant Follow",
        asp::VIB_AMT => "Vibrato Amount",
        asp::VIB_RATE => "Vibrato Rate",
        asp::VIB_FADE => "Vibrato Fade-In",
        asp::DRY_WET => "Dry/Wet",
        asp::TAB => "Tab",
        asp::MIDI_OPEN => "MIDI In",
        asp::LIVE => "Live Mode",
        asp::RANGE => "Input Range",
        asp::MIDI_ON => "MIDI On",
        asp::SOURCE => "External Source",
        asp::TAP => "Tapping Point",
        asp::POLY => "Mono/Poly",
        asp::VOICES => "Voice Count",
        asp::QUANT => "Quantizer On",
        asp::SMOOTH => "Smooth",
        asp::ROOT => "Root",
        asp::SCALE => "Scale",
        asp::KEY => "Included Notes",
        asp::USE_CUR => "Use Current Scale",
        asp::LATCH => "Note Latch",
        76..=79 => "Mod Source",
        asp::LFO_ON => "LFO On",
        asp::LFO_RESET => "LFO Reset",
        asp::LFO_WAVE => "LFO Waveform",
        asp::LFO_SYNC => "LFO Sync",
        asp::VIB_ON => "Vibrato On",
        asp::NATURAL => "Natural",
        _ => "",
    }
}
