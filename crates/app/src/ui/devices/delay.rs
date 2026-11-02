//! Delay (Live 12): Left / Right Sync with the 16th-note grid (or free time)
//! and offset, Stereo Link, Feedback, Freeze; the display shows the
//! band-pass filter (X = Frequency, Y = Width) with the Filter strip and the
//! foldable LFO section; Smoothing (Repitch / Fade / Jump), Ping Pong and
//! Dry/Wet on the right. Geometry measured from Live 12 at 100 % (l/t =
//! device left/top; text positions are baselines).

use super::dynamics::{KN, knob_lv, sig3, tbutton};
use super::modfx::{drag_text, small_tri, text_menu};
use super::redux::{arrow_col, bip_arrow, pct};
use super::*;
use daw_engine::delay::*;
use daw_project::devices::params3::dlp;

pub const WIDTH: f32 = 400.0;
const LABEL: Color32 = Color32::from_rgb(0x86, 0x86, 0x86);
/// Display: x 129..334, 1 kHz at l+233, 80 px per decade.
const D_L: f32 = 129.0;
const D_R: f32 = 334.0;
const D_TOP: f32 = 21.0;
const X1K: f32 = 233.0;
const DEC: f32 = 80.0;
/// Grid line pitch = graph height / 6 = 3.7 dB.
const DB_PER_LINE: f32 = 3.7;

fn set(cx: &mut Ctx, f: impl FnOnce(&mut DelayParams)) {
    if let FxParams::Delay(p) = &mut cx.params {
        f(p);
    }
}

fn r(l: f32, t: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
    Rect::from_min_max(pos2(l + x0, t + y0), pos2(l + x1, t + y1))
}

fn hz(f: f32) -> String {
    if f >= 999.5 { format!("{} kHz", sig3(f / 1000.0)) } else { format!("{f:.0} Hz") }
}

fn gx(l: f32, f: f32) -> f32 {
    l + X1K + DEC * (f / 1000.0).log10()
}

pub(super) fn draw(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, rect: Rect) {
    let FxParams::Delay(d) = cx.params else { return };
    let (l, t) = (rect.left(), rect.top());
    left(cx, ui, p, l, t, &d);
    display(cx, ui, p, l, t, &d);
    // Right column.
    text_at(p, l + 341.0, t + 31.0, Align2::LEFT_BOTTOM, "Smoothing", font(FS), text());
    for (i, name) in DLY_SMOOTH.iter().enumerate() {
        let y = 34.0 + 16.0 * i as f32;
        cx.salt = i as u32;
        if tbutton(cx, ui, p, r(l, t, 340.0, y, 395.0, y + 13.0), dlp::SMOOTH, name, d.smooth as usize == i, true) {
            set(cx, |q| q.smooth = i as u8);
        }
        cx.salt = 0;
    }
    if tbutton(cx, ui, p, r(l, t, 340.0, 114.0, 395.0, 127.0), dlp::PING, "Ping Pong", d.ping_pong, true) {
        set(cx, |q| q.ping_pong = !q.ping_pong);
    }
    knob_lv(cx, ui, p, l + 367.5, t + 144.0, dlp::DRY_WET, "Dry/Wet", &pct(d.dry_wet), 0.0, KN, 15.5, 36.0, true);
}

#[allow(clippy::needless_range_loop)]
fn left(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, d: &DelayParams) {
    let line = gray(0x24);
    quad(p, r(l, t, 64.0, 34.0, 65.0, 71.0), line);
    quad(p, r(l, t, 64.0, 90.0, 65.0, 127.0), line);
    for s in 0..2 {
        let bx = if s == 0 { 6.0 } else { 76.0 };
        let k = d.side(s);
        text_at(p, l + bx + 23.5, t + 31.0, Align2::CENTER_BOTTOM, if s == 0 { "Left" } else { "Right" }, font(FS), text());
        cx.salt = s as u32;
        if tbutton(cx, ui, p, r(l, t, bx, 34.0, bx + 47.0, 47.0), dlp::SYNC[s], "Sync", d.sync[k], true) {
            set(cx, |q| {
                let v = !q.sync[k];
                q.sync[k] = v;
                if q.link {
                    q.sync = [v; 2];
                }
            });
        }
        if d.sync[k] {
            for (i, n) in DLY_SIXTEENTHS.iter().enumerate() {
                let x = bx + if i % 2 == 0 { 0.0 } else { 25.0 };
                let y = 50.0 + 16.0 * (i / 2) as f32;
                cx.salt = (s * 16 + i) as u32 + 100;
                if tbutton(cx, ui, p, r(l, t, x, y, x + 22.0, y + 13.0), dlp::TIME[s], &n.to_string(), d.six[k] as usize == i, true) {
                    set(cx, |q| {
                        q.six[k] = i as u8;
                        if q.link {
                            q.six = [i as u8; 2];
                        }
                    });
                }
            }
        } else {
            cx.salt = s as u32;
            value_box(cx, ui, p, r(l, t, bx, 50.0, bx + 47.0, 63.0), dlp::TIME[s], &DelayParams::ms_label(d.ms[k]), true);
        }
        cx.salt = s as u32;
        let or = r(l, t, bx, 114.0, bx + 47.0, 127.0);
        let off = d.offset[k];
        value_box_ex(cx, ui, p, or, dlp::OFFSET[s], &format!("{:.1} %", off * 100.0), d.sync[k], false, box_fill(cx.on));
        bip_arrow(p, or, arrow_col(cx, d.sync[k], off.abs() > 1e-4));
        cx.salt = 0;
    }
    // Stereo Link.
    let lr = r(l, t, 56.0, 74.0, 73.0, 87.0);
    let (fill, g) = match (d.link, cx.on) {
        (true, true) => (ORANGE, gray(0x07)),
        (true, false) => (gray(0x75), gray(0x24)),
        (false, _) => (gray(0x1e), text()),
    };
    boxed(p, lr, fill);
    p.rect_stroke(r(l, t, 61.0, 78.0, 68.0, 83.0), CornerRadius::ZERO, Stroke::new(1.0, g), egui::StrokeKind::Inside);
    quad(p, r(l, t, 58.0, 80.0, 62.0, 81.0), g);
    quad(p, r(l, t, 67.0, 80.0, 71.0, 81.0), g);
    if cx.toggle(ui, lr, dlp::LINK, true) {
        set(cx, |q| {
            q.link = !q.link;
            if q.link {
                q.sync[1] = q.sync[0];
                q.six[1] = q.six[0];
                q.ms[1] = q.ms[0];
                q.offset[1] = q.offset[0];
            }
        });
    }
    // Feedback + Freeze.
    knob_lv(cx, ui, p, l + 64.5, t + 144.0, dlp::FEEDBACK, "Feedback", &pct(d.feedback), 0.0, KN, 15.5, 36.0, !d.freeze);
    quad(p, r(l, t, 86.0, 159.0, 92.0, 160.0), gray(0x55));
    let fr = r(l, t, 93.0, 153.0, 106.0, 166.0);
    let (fill, g) = match (d.freeze, cx.on) {
        (true, true) => (ORANGE, gray(0x20)),
        (true, false) => (mix(ORANGE, gray(0x36), 0.5), gray(0x36)),
        (false, _) => (gray(0x1e), text()),
    };
    boxed(p, fr, fill);
    let c = pos2(l + 99.5, t + 159.5);
    let s = Stroke::new(1.0, g);
    for (dx, dy) in [(0.0, 4.0), (4.0, 0.0), (2.9, 2.9), (2.9, -2.9)] {
        p.line_segment([c - vec2(dx, dy), c + vec2(dx, dy)], s);
    }
    if cx.toggle(ui, fr, dlp::FREEZE, true) {
        set(cx, |q| q.freeze = !q.freeze);
    }
}

fn display(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, d: &DelayParams) {
    let on = cx.on;
    let disp = r(l, t, D_L, D_TOP, D_R, 184.0);
    p.rect_filled(disp, CornerRadius::same(2), gray(0x18));
    // Graph bottom: 144 (LFO folded) / 110 (unfolded).
    let gb = if d.lfo_shown { 110.0 } else { 144.0 };
    let h = gb - D_TOP;
    let graph = r(l, t, D_L, D_TOP, D_R, gb);
    let gp = p.with_clip_rect(graph.intersect(p.clip_rect()));
    let mut f = 50.0f32;
    while f < 20_000.0 {
        let dec = 10f32.powf(f.log10().floor());
        let major = (f / dec - 1.0).abs() < 1e-3;
        let x = gx(l, f).round();
        quad(&gp, Rect::from_min_max(pos2(x, t + D_TOP), pos2(x + 1.0, t + gb)), gray(if major { 0x3f } else { 0x2b }));
        f += dec;
    }
    let y0 = t + D_TOP + h / 3.0;
    for k in 0..4 {
        let y = (y0 + h / 6.0 * k as f32).round();
        quad(&gp, Rect::from_min_max(pos2(l + D_L, y), pos2(l + D_R, y + 1.0)), gray(if k == 0 { 0x3e } else { 0x2b }));
    }
    // Filter curve and handle.
    let sr = cx.app.audio.info.sample_rate as f32;
    let sr = if sr > 0.0 { sr } else { 48_000.0 };
    let px_db = h / 6.0 / DB_PER_LINE;
    let fon = d.filter && on;
    let curve_c = if fon { TEAL } else { LABEL };
    let pts: Vec<Pos2> = (0..=(D_R - D_L) as i32)
        .map(|i| {
            let x = l + D_L + i as f32 + 0.5;
            let fr = 1000.0 * 10f32.powf((x - l - X1K) / DEC);
            let db = d.filter_db(fr, sr);
            pos2(x, y0 + 0.5 - db.max(-40.0) * px_db)
        })
        .collect();
    gp.add(Shape::line(pts, Stroke::new(1.5, curve_c)));
    let u = (d.width.clamp(DLY_WIDTH.0, DLY_WIDTH.1) - DLY_WIDTH.0) / (DLY_WIDTH.1 - DLY_WIDTH.0);
    let hc = pos2(gx(l, d.freq.clamp(DLY_FREQ.0, DLY_FREQ.1)) + 0.5, t + D_TOP + 6.5 + (1.0 - u) * (h - 13.0));
    gp.circle_stroke(hc, 5.8, Stroke::new(1.6, if fon { ORANGE } else { gray(0x75) }));
    cx.salt = 1;
    let resp = cx.sense(ui, graph, dlp::FREQ, Sense::click_and_drag());
    cx.salt = 0;
    if resp.drag_started() || resp.clicked() {
        cx.focus(dlp::FREQ);
    }
    if resp.dragged() && d.filter {
        let dd = resp.drag_delta();
        if dd != egui::Vec2::ZERO {
            cx.undo_step();
            let span = (DLY_FREQ.1 / DLY_FREQ.0).log10() * DEC;
            let fv = norm(&cx.params, dlp::FREQ).unwrap_or(0.5);
            set_norm(&mut cx.params, dlp::FREQ, fv + dd.x / span);
            let wv = norm(&cx.params, dlp::WIDTH).unwrap_or(0.5);
            set_norm(&mut cx.params, dlp::WIDTH, wv - dd.y / (h - 13.0));
        }
    }
    if resp.double_clicked() {
        cx.undo_step();
        reset(&mut cx.params, dlp::FREQ);
        reset(&mut cx.params, dlp::WIDTH);
    }
    cx.param_menu(&resp, dlp::FREQ);
    let sep = gray(0x3e);
    quad(p, Rect::from_min_max(pos2(l + D_L, t + gb), pos2(l + D_R, t + gb + 1.0)), sep);
    quad(p, Rect::from_min_max(pos2(l + D_L, t + gb + 20.0), pos2(l + D_R, t + gb + 21.0)), sep);

    // Filter strip.
    let fb = t + gb + 14.0;
    let ck = Rect::from_min_max(pos2(l + 134.0, fb - 8.0), pos2(l + 143.0, fb + 1.0));
    let tc = if on { TEAL } else { gray(0x75) };
    if d.filter {
        p.rect_filled(ck, CornerRadius::ZERO, tc);
    } else {
        p.rect_stroke(ck, CornerRadius::ZERO, Stroke::new(1.0, tc), egui::StrokeKind::Inside);
    }
    text_at(p, l + 147.0, fb, Align2::LEFT_BOTTOM, "Filter", font(FS), if d.filter { text() } else { LABEL });
    if cx.toggle(ui, Rect::from_min_max(pos2(l + 132.0, fb - 10.0), pos2(l + 178.0, fb + 3.0)), dlp::FILTER, true) {
        set(cx, |q| q.filter = !q.filter);
    }
    text_at(p, l + 183.0, fb, Align2::LEFT_BOTTOM, "Freq", font(FS), LABEL);
    let fcol = if d.filter { TEAL } else { LABEL };
    drag_text(cx, ui, p, l + 210.0, fb, dlp::FREQ, &hz(d.freq), true, fcol);
    text_at(p, l + 261.0, fb, Align2::LEFT_BOTTOM, "Width", font(FS), LABEL);
    drag_text(cx, ui, p, l + 297.0, fb, dlp::WIDTH, &format!("{:.2}", d.width), true, fcol);

    // LFO section.
    let lb = t + gb + 34.0;
    let tri = if d.lfo_shown {
        vec![pos2(l + 134.0, lb - 6.0), pos2(l + 141.0, lb - 6.0), pos2(l + 137.5, lb)]
    } else {
        vec![pos2(l + 135.0, lb - 7.0), pos2(l + 141.0, lb - 3.5), pos2(l + 135.0, lb)]
    };
    p.add(Shape::convex_polygon(tri, LABEL, Stroke::NONE));
    text_at(p, l + 147.0, lb, Align2::LEFT_BOTTOM, "LFO", bold(FS), text());
    if cx.toggle(ui, Rect::from_min_max(pos2(l + 131.0, lb - 10.0), pos2(l + 166.0, lb + 3.0)), dlp::LFO_SHOWN, true) {
        set(cx, |q| q.lfo_shown = !q.lfo_shown);
    }
    let active = on && (d.lfo_delay > 0.0 || (d.filter && d.lfo_filter > 0.0));
    p.circle_filled(pos2(l + 172.5, lb - 3.5), 2.5, if active { ORANGE } else { gray(0x40) });
    text_at(p, l + 183.0, lb, Align2::LEFT_BOTTOM, "Delay", font(FS), LABEL);
    drag_text(cx, ui, p, l + 216.0, lb, dlp::LFO_DELAY, &pct(d.lfo_delay), true, ORANGE);
    text_at(p, l + 261.0, lb, Align2::LEFT_BOTTOM, "Filter", font(FS), LABEL);
    drag_text(cx, ui, p, l + 297.0, lb, dlp::LFO_FILTER, &pct(d.lfo_filter), true, if d.filter { ORANGE } else { LABEL });
    if !d.lfo_shown {
        return;
    }
    let (b2, b3) = (lb + 17.0, lb + 34.0);
    text_at(p, l + 183.0, b2, Align2::LEFT_BOTTOM, "Rate", font(FS), LABEL);
    drag_text(cx, ui, p, l + 216.0, b2, dlp::LFO_RATE, &d.lfo_rate_label(), true, ORANGE);
    text_at(p, l + 261.0, b2, Align2::LEFT_BOTTOM, "Wave", font(FS), LABEL);
    let oc = if on { ORANGE } else { gray(0x5a) };
    let wr = Rect::from_min_max(pos2(l + 296.0, b2 - 9.0), pos2(l + 330.0, b2 + 2.0));
    let gpts: Vec<Pos2> = (0..=30)
        .map(|i| {
            let u = i as f32 / 30.0;
            let v = lfo_wave(d.wave, (u + 0.75).fract(), d.morph, 0.3 * (u * 9.0).sin());
            pos2(l + 297.0 + 15.0 * u, b2 - 3.5 - 3.5 * v)
        })
        .collect();
    p.add(Shape::line(gpts, Stroke::new(1.0, oc)));
    small_tri(p, l + 323.0, b2 - 4.0, oc);
    let resp = cx.sense(ui, wr, dlp::WAVE, Sense::click());
    cx.param_menu(&resp, dlp::WAVE);
    if resp.clicked() {
        cx.focus(dlp::WAVE);
    }
    let mut pick = None;
    menu::dropdown_aligned(&resp, 100.0, egui::RectAlign::BOTTOM_START, |ui| {
        for (i, it) in DLY_WAVES.iter().enumerate() {
            if menu::check_item(ui, it, "", i == d.wave as usize, true) {
                pick = Some(i as u8);
            }
        }
    });
    if let Some(i) = pick {
        cx.undo_step();
        set(cx, |q| q.wave = i);
    }
    let mr = Rect::from_min_max(pos2(l + 181.0, b3 - 10.0), pos2(l + 252.0, b3 + 3.0));
    let m = text_menu(cx, ui, p, mr, dlp::LFO_MODE, l + 183.0, b3, Some(l + 245.0), d.lfo_mode as usize, &DLY_LFO_MODES, true, ORANGE);
    if let Some(i) = m {
        set(cx, |q| q.lfo_mode = i as u8);
    }
    text_at(p, l + 261.0, b3, Align2::LEFT_BOTTOM, "Morph", font(FS), LABEL);
    drag_text(cx, ui, p, l + 297.0, b3, dlp::MORPH, &pct(d.morph), true, ORANGE);
}

// ---- parameters --------------------------------------------------------------

pub(super) fn reset(p: &mut FxParams, k: u8) {
    let d = FxParams::Delay(DelayParams::default());
    if let Some(v) = norm(&d, k) {
        set_norm(p, k, v);
    }
}

pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    let FxParams::Delay(d) = p else { return };
    match k {
        0 | 1 if d.sync[d.side(k as usize)] => {
            let s = d.side(k as usize);
            let i = (d.six[s] as i32 + dir as i32).clamp(0, 7) as u8;
            d.six[s] = i;
            if d.link {
                d.six = [i; 2];
            }
        }
        2 | 3 => {
            let s = d.side(k as usize - 2);
            let o = ((d.offset[s] * 1000.0).round() + dir).clamp(-DLY_OFFSET * 1000.0, DLY_OFFSET * 1000.0) / 1000.0;
            d.offset[s] = o;
            if d.link {
                d.offset = [o; 2];
            }
        }
        dlp::WIDTH => d.width = ((d.width * 100.0).round() + dir).clamp(DLY_WIDTH.0 * 100.0, DLY_WIDTH.1 * 100.0) / 100.0,
        dlp::FEEDBACK | dlp::LFO_DELAY | dlp::LFO_FILTER | dlp::MORPH | dlp::DRY_WET => {
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
        0 => "L Delay Time",
        1 => "R Delay Time",
        2 => "L Offset",
        3 => "R Offset",
        dlp::FEEDBACK => "Feedback",
        dlp::FREQ => "Filter Freq",
        dlp::WIDTH => "Filter Width",
        dlp::LFO_DELAY => "Mod Delay",
        dlp::LFO_FILTER => "Mod Filter",
        dlp::LFO_RATE => "LFO Rate",
        dlp::MORPH => "LFO Morph",
        dlp::DRY_WET => "Dry/Wet",
        dlp::FREEZE => "Freeze",
        dlp::FILTER => "Filter On",
        dlp::LINK => "Link",
        dlp::PING => "Ping Pong",
        dlp::SMOOTH => "Delay Mode",
        dlp::WAVE => "LFO Waveform",
        dlp::LFO_MODE => "LFO Rate Type",
        40 => "L Sync",
        41 => "R Sync",
        _ => "",
    }
}
