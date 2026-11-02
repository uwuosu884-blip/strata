//! Echo (Live 12): Left / Right (Mid / Side) times with Sync, sync mode and
//! offset, Stereo Link, Input / Feedback with Clip Dry and Invert; the
//! title-bar tabs pick the display: Echo (tunnel + filter), Modulation or
//! Character; Reverb, Stereo, Output, Dry/Wet and the channel mode on the
//! right. Geometry measured from Live 12 at 100 % (l/t = device left/top;
//! text positions are baselines).

use super::dynamics::{KN, knob_lv, sig3, tbutton};
use super::modfx::drag_text;
use super::*;
use daw_engine::echo::*;
use daw_project::devices::dev_tap;
use daw_project::devices::params2::ecp;

pub const WIDTH: f32 = 512.0;

/// Display labels.
const LABEL: Color32 = Color32::from_rgb(0x86, 0x86, 0x86);
/// Display bottom (strip line) and the filter graph.
const D_TOP: f32 = 17.0;
const D_BOT: f32 = 166.0;
/// Expanded filter: tunnel above `F_TOP`, graph to `D_BOT`, 0 dB at `F_0DB`.
const F_TOP: f32 = 105.0;
const F_0DB: f32 = 126.0;
const F_PX_DB: f32 = 19.0 / 6.0;
/// Filter graph: 100 Hz at l+186, 92 px per decade.
const F_X100: f32 = 186.0;
const F_DEC: f32 = 92.0;

fn set(cx: &mut Ctx, f: impl FnOnce(&mut EchoParams)) {
    if let FxParams::Echo(e) = &mut cx.params {
        f(e);
    }
}

fn hz(f: f32) -> String {
    if f >= 999.5 { format!("{} kHz", sig3(f / 1000.0)) } else { format!("{f:.0} Hz") }
}

fn pct(x: f32) -> String {
    let v = x * 100.0;
    if v.abs() < 9.95 { format!("{v:.1} %") } else { format!("{v:.0} %") }
}

fn ms(x: f32) -> String {
    if x >= 999.5 {
        format!("{} s", sig3(x / 1000.0))
    } else if x < 99.95 {
        format!("{x:.1} ms")
    } else {
        format!("{x:.0} ms")
    }
}

fn db(x: f32) -> String {
    if x <= ECHO_GAIN_DB.0 { "-inf dB".into() } else { format!("{x:.1} dB") }
}

/// 9 × 9 switch square: filled on, outlined off.
fn check(p: &Painter, r: Rect, on: bool, col: Color32) {
    if on {
        p.rect_filled(r, CornerRadius::ZERO, col);
    } else {
        p.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, col), egui::StrokeKind::Inside);
    }
}

fn px(p: &Painter, x0: f32, y0: f32, x1: f32, y1: f32, c: Color32) {
    quad(p, Rect::from_min_max(pos2(x0, y0), pos2(x1, y1)), c);
}

/// SVF magnitude (dB) of the feedback filter at `f`: HP × LP with Live's
/// resonance mapping (k = 2 − 1.9 res), as in the engine.
fn filter_db(e: &EchoParams, f: f32) -> f32 {
    let mag = |r: f32, k: f32, hp: bool| {
        let (re, im) = (1.0 - r * r, k * r);
        let den = (re * re + im * im).sqrt().max(1e-9);
        if hp { r * r / den } else { 1.0 / den }
    };
    let k = |res: f32| 2.0 - 1.9 * res.clamp(0.0, 1.0);
    let h = mag(f / e.hp.max(1.0), k(e.hp_res), true) * mag(f / e.lp.max(1.0), k(e.lp_res), false);
    20.0 * h.max(1e-6).log10()
}

pub(super) fn draw(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect) {
    let FxParams::Echo(e0) = cx.params else { return };
    let (l, t) = (r.left(), r.top());

    // ---- title-bar tabs ----
    for (i, (x0, x1, label)) in [(175.0, 213.0, "Echo"), (214.0, 284.0, "Modulation"), (285.0, 349.0, "Character")].into_iter().enumerate()
    {
        let sel = e0.tab as usize == i;
        let tr = Rect::from_min_max(pos2(l + x0, t + 2.0), pos2(l + x1, t + if sel { D_TOP } else { 15.0 }));
        let cr = if sel { CornerRadius { nw: 2, ne: 2, sw: 0, se: 0 } } else { CornerRadius::same(2) };
        p.rect_filled(tr, cr, if sel { gray(0x18) } else { panel() });
        text_at(p, tr.center().x, t + 13.0, Align2::CENTER_BOTTOM, label, font(FS), if sel { LABEL } else { text() });
        cx.salt = i as u32;
        if cx.toggle(ui, tr, ecp::TAB, true) {
            set(cx, |e| e.tab = i as u8);
        }
        cx.salt = 0;
    }

    // ---- panels ----
    for (x0, x1) in [(2.0, 119.0), (401.0, 511.0)] {
        p.rect_filled(Rect::from_min_max(pos2(l + x0, t + D_TOP), pos2(l + x1, t + 186.0)), CornerRadius::same(2), panel());
    }
    let disp = Rect::from_min_max(pos2(l + 121.0, t + D_TOP), pos2(l + 399.0, t + 186.0));
    p.rect_filled(disp, CornerRadius::same(2), gray(0x18));

    left_panel(cx, ui, p, l, t, &e0);
    right_panel(cx, ui, p, l, t, &e0);
    match e0.tab {
        0 => echo_tab(cx, ui, p, l, t, &e0),
        1 => mod_tab(cx, ui, p, l, t, &e0),
        _ => char_tab(cx, ui, p, l, t, &e0),
    }
}

#[allow(clippy::needless_range_loop)]
fn left_panel(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, e0: &EchoParams) {
    let ms_mode = e0.mode == 2;
    let names = if ms_mode { ["Mid", "Side"] } else { ["Left", "Right"] };
    let line = gray(0x24);
    // Centre divider around the link button.
    px(p, l + 60.0, t + 19.0, l + 61.0, t + 37.0, line);
    px(p, l + 60.0, t + 52.0, l + 61.0, t + 129.0, line);
    for s in 0..2 {
        let x = if s == 0 { 28.5 } else { 92.5 };
        let bx = if s == 0 { 5.0 } else { 69.0 };
        let k = if e0.link { 0 } else { s };
        knob_lv(cx, ui, p, l + x, t + 28.0, ecp::TIME[s], names[s], &e0.time_label(s), 0.0, KN, 15.5, 36.0, true);
        // Sync, sync mode, offset.
        let sr = Rect::from_min_max(pos2(l + bx, t + 79.0), pos2(l + bx + 47.0, t + 92.0));
        cx.salt = s as u32;
        if tbutton(cx, ui, p, sr, ecp::SYNC[s], "Sync", e0.sync[k], true) {
            set(cx, |e| {
                let v = !e.sync[k];
                e.sync[k] = v;
                if e.link {
                    e.sync = [v; 2];
                }
            });
        }
        let cr = Rect::from_min_max(pos2(l + bx, t + 95.0), pos2(l + bx + 47.0, t + 108.0));
        let cur = ECHO_SYNC_MODES[(e0.sync_mode[k] as usize).min(3)];
        if let Some(i) = chooser(cx, ui, p, cr, ecp::SYNC_MODE[s], cur, &ECHO_SYNC_MODES, e0.sync[k]) {
            set(cx, |e| {
                e.sync_mode[k] = i as u8;
                if e.link {
                    e.sync_mode = [i as u8; 2];
                }
            });
        }
        let or = Rect::from_min_max(pos2(l + bx, t + 111.0), pos2(l + bx + 47.0, t + 124.0));
        let off = e0.offset[s];
        value_box_ex(cx, ui, p, or, ecp::OFFSET[s], &format!("{:.1} %", off * 100.0), true, false, box_fill(cx.on));
        if off.abs() > 1e-4 {
            let c = if cx.on { TEAL } else { gray(0x75) };
            p.add(Shape::convex_polygon(
                vec![pos2(l + bx + 40.0, t + 117.5), pos2(l + bx + 46.0, t + 114.0), pos2(l + bx + 46.0, t + 121.0)],
                c,
                Stroke::NONE,
            ));
        }
        cx.salt = 0;
    }
    // Stereo Link: two chain links.
    let lr = Rect::from_min_max(pos2(l + 51.0, t + 39.0), pos2(l + 70.0, t + 50.0));
    let (fill, g) = match (e0.link, cx.on) {
        (true, true) => (ORANGE, gray(0x07)),
        (true, false) => (gray(0x75), gray(0x24)),
        (false, _) => (gray(0x1e), text()),
    };
    boxed(p, lr, fill);
    p.rect_stroke(
        Rect::from_min_max(pos2(l + 57.0, t + 42.0), pos2(l + 64.0, t + 47.0)),
        CornerRadius::ZERO,
        Stroke::new(1.0, g),
        egui::StrokeKind::Inside,
    );
    px(p, l + 54.0, t + 44.0, l + 58.0, t + 45.0, g);
    px(p, l + 63.0, t + 44.0, l + 67.0, t + 45.0, g);
    if cx.toggle(ui, lr, ecp::LINK, true) {
        set(cx, |e| e.link = !e.link);
    }

    // Input / Feedback with Clip Dry (D) and Invert (Ø) between them.
    knob_lv(cx, ui, p, l + 28.5, t + 145.0, ecp::INPUT, "Input", &db(e0.input), 0.0, KN, 15.5, 36.0, true);
    knob_lv(cx, ui, p, l + 92.5, t + 145.0, ecp::FEEDBACK, "Feedback", &pct(e0.feedback), 0.0, KN, 15.5, 36.0, true);
    px(p, l + 43.0, t + 151.0, l + 53.0, t + 152.0, line);
    px(p, l + 68.0, t + 167.0, l + 78.0, t + 168.0, line);
    let dr = Rect::from_min_max(pos2(l + 53.0, t + 145.0), pos2(l + 68.0, t + 158.0));
    if tbutton(cx, ui, p, dr, ecp::CLIP, "D", e0.clip_dry, true) {
        set(cx, |e| e.clip_dry = !e.clip_dry);
    }
    let ir = Rect::from_min_max(pos2(l + 53.0, t + 161.0), pos2(l + 68.0, t + 174.0));
    if tbutton(cx, ui, p, ir, ecp::INVERT, "Ø", e0.invert, true) {
        set(cx, |e| e.invert = !e.invert);
    }
}

fn right_panel(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, e0: &EchoParams) {
    knob_lv(cx, ui, p, l + 431.5, t + 28.0, ecp::REVERB, "Reverb", &pct(e0.reverb), 0.0, KN, 15.5, 36.0, true);
    knob_lv(cx, ui, p, l + 485.5, t + 28.0, ecp::STEREO, "Stereo", &pct(e0.stereo), 0.0, KN, 15.5, 36.0, true);
    knob_lv(cx, ui, p, l + 485.5, t + 86.0, ecp::OUTPUT, "Output", &db(e0.output), 0.0, KN, 15.5, 36.0, true);
    knob_lv(cx, ui, p, l + 485.5, t + 145.0, ecp::DRY_WET, "Dry/Wet", &pct(e0.dry_wet), 0.0, KN, 15.5, 36.0, true);
    let vr = Rect::from_min_max(pos2(l + 404.0, t + 80.0), pos2(l + 457.0, t + 93.0));
    let cur = ECHO_VERB_LOC[(e0.verb_loc as usize).min(2)];
    if let Some(i) = chooser(cx, ui, p, vr, ecp::VERB_LOC, cur, &ECHO_VERB_LOC, true) {
        set(cx, |e| e.verb_loc = i as u8);
    }
    text_at(p, l + 405.0, t + 105.0, Align2::LEFT_BOTTOM, "Decay", font(FS), text());
    let dr = Rect::from_min_max(pos2(l + 404.0, t + 108.0), pos2(l + 457.0, t + 121.0));
    value_box(cx, ui, p, dr, ecp::VERB_DECAY, &format!("{:.0} %", e0.verb_decay * 100.0), true);
    for (i, name) in ECHO_MODES.iter().enumerate() {
        let y = 137.0 + 16.0 * i as f32;
        let br = Rect::from_min_max(pos2(l + 404.0, t + y), pos2(l + 457.0, t + y + 13.0));
        cx.salt = i as u32;
        if tbutton(cx, ui, p, br, ecp::MODE, name, e0.mode as usize == i, true) {
            set(cx, |e| e.mode = i as u8);
        }
        cx.salt = 0;
    }
}

// ---- Echo tab -----------------------------------------------------------------

fn fx_of(l: f32, f: f32) -> f32 {
    l + F_X100 + F_DEC * (f / 100.0).log10()
}

fn f_of(l: f32, x: f32) -> f32 {
    100.0 * 10f32.powf((x - l - F_X100) / F_DEC)
}

/// Tunnel: one half ring per repeat at radius R / (1 + delay × n) (left
/// quarter = left time, right quarter = right time), fading with Feedback;
/// white dots on the centre line every 1/8 note.
fn tunnel(cx: &mut Ctx, p: &Painter, l: f32, t: f32, bottom: f32, e0: &EchoParams) {
    let area = Rect::from_min_max(pos2(l + 121.0, t + D_TOP), pos2(l + 399.0, bottom));
    let tp = p.with_clip_rect(area.intersect(p.clip_rect()));
    let c = pos2(l + 260.0, bottom);
    // Rings keep the full width and squash vertically with the filter shown.
    let big_r = 126.0;
    let ry = 0.8456 * (bottom - t - D_TOP) / big_r;
    let tempo = cx.app.project.tempo_bpm;
    let tap = dev_tap(cx.dev.id);
    let secs = |s: usize| {
        let v = tap.get(if s == 0 { etap::TIME_L } else { etap::TIME_R });
        if cx.on && v > 0.0 { v } else { e0.secs(s, tempo) }
    };
    let fb = e0.feedback.clamp(0.0, ECHO_FB_MAX);
    let base = if cx.on { ORANGE } else { gray(0x75) };
    for s in 0..2 {
        let d = secs(s).max(0.002);
        let (a0, a1) = if s == 0 { (-PI / 2.0, 0.0) } else { (0.0, PI / 2.0) };
        for n in 0..200 {
            let rr = big_r / (1.0 + d * n as f32);
            if rr < 2.0 {
                break;
            }
            let a = if n == 0 { 1.0 } else { fb.powf((n - 1) as f32 * 0.6).min(1.0) };
            if a < 0.02 {
                break;
            }
            let m = ((a1 - a0) * rr / 3.0).clamp(6.0, 64.0) as usize;
            let pts: Vec<Pos2> = (0..=m)
                .map(|i| {
                    let u = a0 + (a1 - a0) * i as f32 / m as f32;
                    pos2(c.x + rr * u.sin(), c.y - rr * ry * u.cos())
                })
                .collect();
            tp.add(Shape::line(pts, Stroke::new(1.6, base.gamma_multiply(a))));
        }
    }
    // Beat dots (1/8 notes).
    let eighth = 30.0 / tempo.max(1.0) as f32;
    for j in 0..64 {
        let depth = eighth * j as f32;
        let rr = big_r / (1.0 + depth);
        if j > 0 && (big_r / (1.0 + depth - eighth) - rr) * ry < 2.0 {
            break;
        }
        let rad = if j == 0 { 2.2 } else { (1.6 / (1.0 + depth * 0.5)).max(0.9) };
        let a = 1.0 / (1.0 + 0.12 * j as f32);
        tp.circle_filled(pos2(c.x, c.y - rr * ry), rad, Color32::WHITE.gamma_multiply(if cx.on { a } else { a * 0.5 }));
    }
}

fn echo_tab(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, e0: &EchoParams) {
    let line = gray(0x3e);
    let shown = e0.filter_shown;
    let tb = if shown { t + F_TOP } else { t + D_BOT };
    tunnel(cx, p, l, t, tb, e0);
    if shown {
        px(p, l + 121.0, t + F_TOP, l + 399.0, t + F_TOP + 1.0, line);
        let g = Rect::from_min_max(pos2(l + 121.0, t + F_TOP + 1.0), pos2(l + 399.0, t + D_BOT));
        let gp = p.with_clip_rect(g.intersect(p.clip_rect()));
        // Grid: decades #3f, the rest #2b; 0 dB and −6 dB lines.
        for dec in [10.0f32, 100.0, 1000.0, 10_000.0] {
            for m in 1..10 {
                let x = fx_of(l, dec * m as f32).round();
                if x > g.left() && x < g.right() {
                    px(&gp, x, g.top(), x + 1.0, g.bottom(), if m == 1 { gray(0x3f) } else { gray(0x2b) });
                }
            }
        }
        px(&gp, g.left(), t + F_0DB, g.right(), t + F_0DB + 1.0, gray(0x3f));
        px(&gp, g.left(), t + 145.0, g.right(), t + 146.0, gray(0x2b));
        for (f, s) in [(100.0f32, "100"), (1000.0, "1k"), (10_000.0, "10k")] {
            text_at(&gp, fx_of(l, f).round() + 0.5, t + 164.0, Align2::CENTER_BOTTOM, s, font(FS), LABEL);
        }
        let on = cx.on && e0.filter_on;
        let pts: Vec<Pos2> = (0..=(g.width() as usize * 2))
            .map(|i| {
                let x = g.left() + i as f32 * 0.5;
                let d = if e0.filter_on { filter_db(e0, f_of(l, x)) } else { 0.0 };
                pos2(x, (t + F_0DB + 0.5 - d * F_PX_DB).clamp(g.top() - 3.0, g.bottom() + 3.0))
            })
            .collect();
        gp.add(Shape::line(pts, Stroke::new(1.6, if on { TEAL } else { gray(0x75) })));
        // Handles: HP filled, LP ring; drag x = frequency, y = resonance.
        let oc = if on { ORANGE } else { gray(0x75) };
        for (i, (f, res, kf, kr)) in
            [(e0.hp, e0.hp_res, ecp::HP, ecp::HP_RES), (e0.lp, e0.lp_res, ecp::LP, ecp::LP_RES)].into_iter().enumerate()
        {
            // Live insets the handles slightly from the cutoff.
            let c = pos2(fx_of(l, if i == 0 { f * 1.08 } else { f / 1.11 }), t + 159.5);
            if i == 0 {
                gp.circle_filled(c, 6.5, oc);
            } else {
                gp.circle_stroke(c, 5.7, Stroke::new(1.6, oc));
            }
            cx.salt = 7;
            let resp = cx.sense(ui, Rect::from_center_size(c, vec2(14.0, 14.0)), kf, Sense::click_and_drag());
            cx.salt = 0;
            if resp.drag_started() || resp.clicked() {
                cx.focus(kf);
            }
            if resp.dragged() {
                let d = resp.drag_delta();
                if d != egui::Vec2::ZERO {
                    cx.undo_step();
                    let span = (ECHO_FILT_HZ.1 / ECHO_FILT_HZ.0).log10() * F_DEC;
                    let fv = norm(&cx.params, kf).unwrap_or(0.0);
                    set_norm(&mut cx.params, kf, fv + d.x / span);
                    set_norm(&mut cx.params, kr, (res - d.y / 60.0).clamp(0.0, 1.0));
                }
            }
            cx.param_menu(&resp, kf);
        }
    }
    px(p, l + 121.0, t + D_BOT, l + 399.0, t + D_BOT + 1.0, line);

    // Strip: show / hide, Filter switch, HP / LP with resonance.
    let ac = LABEL;
    if shown {
        p.add(Shape::convex_polygon(
            vec![pos2(l + 127.0, t + 179.0), pos2(l + 134.0, t + 179.0), pos2(l + 130.5, t + 173.0)],
            ac,
            Stroke::NONE,
        ));
    } else {
        p.add(Shape::convex_polygon(
            vec![pos2(l + 128.0, t + 173.0), pos2(l + 134.0, t + 176.5), pos2(l + 128.0, t + 180.0)],
            ac,
            Stroke::NONE,
        ));
    }
    if cx.toggle(ui, Rect::from_min_max(pos2(l + 124.0, t + 169.0), pos2(l + 138.0, t + 184.0)), ecp::FILTER_SHOWN, true) {
        set(cx, |e| e.filter_shown = !e.filter_shown);
    }
    let sq = Rect::from_min_size(pos2(l + 141.0, t + 172.0), vec2(9.0, 9.0));
    check(p, sq, e0.filter_on, if cx.on { TEAL } else { gray(0x75) });
    if cx.toggle(ui, sq.expand(2.0), ecp::FILTER, true) {
        set(cx, |e| e.filter_on = !e.filter_on);
    }
    let fe = e0.filter_on;
    let b = t + 180.0;
    for (x, s) in [(155.0, "Filter"), (183.0, "HP"), (246.0, "Res"), (292.0, "LP"), (353.0, "Res")] {
        text_at(p, l + x, b, Align2::LEFT_BOTTOM, s, font(FS), ac);
    }
    drag_text(cx, ui, p, l + 200.0, b, ecp::HP, &hz(e0.hp), fe, TEAL);
    drag_text(cx, ui, p, l + 266.0, b, ecp::HP_RES, &format!("{:.2}", e0.hp_res), fe, TEAL);
    drag_text(cx, ui, p, l + 306.0, b, ecp::LP, &hz(e0.lp), fe, TEAL);
    drag_text(cx, ui, p, l + 373.0, b, ecp::LP_RES, &format!("{:.2}", e0.lp_res), fe, TEAL);
}

// ---- Modulation tab -----------------------------------------------------------

/// Wave glyphs as Live draws them (pixel offsets from the glyph origin and
/// coverage), measured from Live 12.
#[rustfmt::skip]
const WAVE_PX: [&[(i8, i8, u8)]; 6] = [
    &[(3, -1, 34), (4, -1, 100), (5, -1, 39), (2, 0, 100), (3, 0, 200), (4, 0, 146), (5, 0, 200), (6, 0, 100), (2, 1, 255), (3, 1, 78), (5, 1, 32), (6, 1, 255), (14, 1, 255), (1, 2, 194), (2, 2, 194), (6, 2, 136), (7, 2, 155), (13, 2, 155), (14, 2, 201), (1, 3, 255), (7, 3, 255), (13, 3, 255), (0, 4, 201), (1, 4, 155), (7, 4, 194), (8, 4, 136), (12, 4, 194), (13, 4, 194), (0, 5, 255), (8, 5, 255), (9, 5, 32), (11, 5, 78), (12, 5, 255), (8, 6, 100), (9, 6, 200), (10, 6, 146), (11, 6, 200), (12, 6, 100), (9, 7, 39), (10, 7, 100), (11, 7, 39)],
    &[(6, 0, 39), (7, 0, 253), (8, 0, 39), (5, 1, 79), (6, 1, 236), (8, 1, 236), (9, 1, 79), (4, 2, 123), (5, 2, 216), (9, 2, 216), (10, 2, 123), (3, 3, 165), (4, 3, 178), (10, 3, 178), (11, 3, 165), (2, 4, 202), (3, 4, 137), (11, 4, 137), (12, 4, 202), (1, 5, 232), (2, 5, 95), (12, 5, 95), (13, 5, 232), (0, 6, 255), (1, 6, 53), (13, 6, 53), (14, 6, 255)],
    &[(13, 0, 255), (14, 0, 255), (10, 1, 51), (11, 1, 255), (12, 1, 239), (14, 1, 250), (8, 2, 90), (9, 2, 255), (10, 2, 204), (14, 2, 253), (6, 3, 132), (7, 3, 255), (8, 3, 165), (14, 3, 253), (4, 4, 174), (5, 4, 255), (6, 4, 123), (14, 4, 253), (2, 5, 209), (3, 5, 255), (4, 5, 81), (14, 5, 253), (0, 6, 255), (1, 6, 255), (2, 6, 42), (14, 6, 253)],
    &[(0, 0, 255), (1, 0, 255), (0, 1, 250), (2, 1, 239), (3, 1, 255), (4, 1, 51), (0, 2, 253), (4, 2, 204), (5, 2, 255), (6, 2, 90), (0, 3, 253), (6, 3, 165), (7, 3, 255), (8, 3, 132), (0, 4, 253), (8, 4, 123), (9, 4, 255), (10, 4, 174), (0, 5, 253), (10, 5, 81), (11, 5, 255), (12, 5, 209), (0, 6, 253), (12, 6, 42), (13, 6, 255), (14, 6, 255)],
    &[(0, 0, 255), (1, 0, 255), (2, 0, 255), (3, 0, 255), (4, 0, 255), (5, 0, 255), (6, 0, 255), (7, 0, 255), (14, 0, 255), (0, 1, 255), (7, 1, 255), (14, 1, 255), (0, 2, 255), (7, 2, 255), (14, 2, 255), (0, 3, 255), (7, 3, 255), (14, 3, 255), (0, 4, 255), (7, 4, 255), (14, 4, 255), (0, 5, 255), (7, 5, 255), (14, 5, 255), (0, 6, 255), (7, 6, 255), (8, 6, 255), (9, 6, 255), (10, 6, 255), (11, 6, 255), (12, 6, 255), (13, 6, 255), (14, 6, 255)],
    &[(3, 0, 255), (4, 0, 255), (2, 1, 255), (5, 1, 255), (13, 1, 255), (14, 1, 255), (2, 2, 255), (5, 2, 255), (12, 2, 255), (2, 3, 255), (5, 3, 255), (9, 3, 255), (10, 3, 255), (11, 3, 255), (2, 4, 255), (5, 4, 255), (8, 4, 255), (0, 5, 255), (1, 5, 255), (5, 5, 255), (8, 5, 255), (6, 6, 255), (7, 6, 255)],
];

/// Wave glyph `w` at (x, y).
fn wave_glyph(p: &Painter, x: f32, y: f32, w: u8, col: Color32) {
    let mut mesh = egui::Mesh::default();
    for &(dx, dy, a) in WAVE_PX[(w as usize).min(5)] {
        mesh.add_colored_rect(
            Rect::from_min_size(pos2(x + dx as f32, y + dy as f32), vec2(1.0, 1.0)),
            col.gamma_multiply(a as f32 / 255.0),
        );
    }
    p.add(Shape::mesh(mesh));
}

fn mod_tab(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, e0: &EchoParams) {
    let on = cx.on;
    // Scope: 2.24 s across the display, scrolling with the LFO.
    let sr = Rect::from_min_max(pos2(l + 121.0, t + D_TOP), pos2(l + 397.0, t + 117.0));
    let rate = e0.mod_rate(cx.app.project.tempo_bpm).max(1e-3);
    let ppc = (278.0 / (rate * 2.24)).max(4.0);
    let ph_now = if on { dev_tap(cx.dev.id).get(etap::LFO_PH) } else { 0.25 };
    let n = (sr.width() * 2.0) as usize;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let x = sr.left() + i as f32 * 0.5;
            let ph = ph_now - (sr.right() - x) / ppc;
            let v = echo_wave(e0.wave, ph - ph.floor(), (ph.floor() * 0.618_034).fract() * 2.0 - 1.0);
            pos2(x, t + 60.0 - 29.5 * v.clamp(-1.0, 1.0))
        })
        .collect();
    p.with_clip_rect(sr.intersect(p.clip_rect())).add(Shape::line(pts, Stroke::new(1.6, if on { TEAL } else { gray(0x75) })));
    cx.app.animating |= on;
    for w in 0..6u8 {
        let x = l + 201.0 + 20.0 * w as f32;
        let sel = e0.wave == w;
        wave_glyph(
            p,
            x,
            t + 101.0,
            w,
            if sel && on {
                ORANGE
            } else if sel {
                gray(0xa0)
            } else {
                LABEL
            },
        );
        cx.salt = w as u32;
        if cx.toggle(ui, Rect::from_min_max(pos2(x - 2.0, t + 98.0), pos2(x + 17.0, t + 111.0)), ecp::WAVE, true) {
            set(cx, |e| e.wave = w);
        }
        cx.salt = 0;
    }
    px(p, l + 121.0, t + 118.0, l + 399.0, t + 119.0, gray(0x3e));

    // Sync / Freq (Rate) / Phase.
    let sq = Rect::from_min_size(pos2(l + 131.0, t + 130.0), vec2(9.0, 9.0));
    check(p, sq, e0.mod_sync, if on { TEAL } else { gray(0x75) });
    text_at(p, l + 145.0, t + 139.0, Align2::LEFT_BOTTOM, "Sync", font(FS), LABEL);
    if cx.toggle(ui, Rect::from_min_max(pos2(l + 129.0, t + 128.0), pos2(l + 170.0, t + 141.0)), ecp::MOD_SYNC, true) {
        set(cx, |e| e.mod_sync = !e.mod_sync);
    }
    let (lab, val) =
        if e0.mod_sync { ("Rate", EchoParams::mod_div_label(e0.mod_div as usize)) } else { ("Freq", format!("{:.2} Hz", e0.mod_hz)) };
    text_at(p, l + 145.0, t + 156.0, Align2::LEFT_BOTTOM, lab, font(FS), LABEL);
    drag_text(cx, ui, p, l + 180.0, t + 156.0, ecp::MOD_RATE, &val, true, TEAL);
    text_at(p, l + 145.0, t + 173.0, Align2::LEFT_BOTTOM, "Phase", font(FS), LABEL);
    drag_text(cx, ui, p, l + 179.0, t + 173.0, ecp::PHASE, &format!("{:.1}°", e0.phase), true, TEAL);

    // ×4, Mod Delay, Mod Filter | Env Mix.
    let xr = Rect::from_min_max(pos2(l + 245.0, t + 146.0), pos2(l + 261.0, t + 157.0));
    let xc = match (e0.mod_x4, on) {
        (true, true) => {
            p.rect_filled(xr, CornerRadius::ZERO, ORANGE);
            gray(0x20)
        }
        (false, true) => {
            p.rect_stroke(xr, CornerRadius::ZERO, Stroke::new(1.0, ORANGE), egui::StrokeKind::Inside);
            ORANGE
        }
        (true, false) => {
            p.rect_filled(xr, CornerRadius::ZERO, gray(0x5a));
            gray(0x24)
        }
        (false, false) => {
            p.rect_stroke(xr, CornerRadius::ZERO, Stroke::new(1.0, gray(0x5a)), egui::StrokeKind::Inside);
            gray(0x5a)
        }
    };
    text_at(p, xr.center().x, t + 155.0, Align2::CENTER_BOTTOM, "×4", font(FS), xc);
    if cx.toggle(ui, xr, ecp::X4, true) {
        set(cx, |e| e.mod_x4 = !e.mod_x4);
    }
    for (x, top, bot, k, v, col) in [
        (266.0, "Mod", "Delay", ecp::MOD_DELAY, e0.mod_delay, ORANGE),
        (303.0, "Mod", "Filter", ecp::MOD_FILTER, e0.mod_filter, ORANGE),
        (355.0, "Env", "Mix", ecp::ENV_MIX, e0.env_mix, TEAL),
    ] {
        text_at(p, l + x, t + 139.0, Align2::LEFT_BOTTOM, top, font(FS), LABEL);
        cx.salt = 3;
        drag_text(cx, ui, p, l + x + 2.0, t + 156.0, k, &format!("{:.1} %", v * 100.0), true, col);
        cx.salt = 0;
        px(p, l + x + 6.0, t + 158.0, l + x + 7.0, t + 162.0, gray(0x3e));
        text_at(p, l + x, t + 173.0, Align2::LEFT_BOTTOM, bot, font(FS), LABEL);
    }
    px(p, l + 344.0, t + 127.0, l + 345.0, t + 176.0, gray(0x3e));
}

// ---- Character tab ------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn char_tab(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, e0: &EchoParams) {
    let on = cx.on;
    let div = gray(0x24);
    px(p, l + 260.0, t + 19.0, l + 261.0, t + 89.0, div);
    px(p, l + 260.0, t + 94.0, l + 261.0, t + 164.0, div);
    px(p, l + 121.0, t + 91.0, l + 399.0, t + 92.0, div);
    px(p, l + 121.0, t + D_BOT, l + 399.0, t + D_BOT + 1.0, gray(0x3e));
    // (x, y, accent, switch, name, labels + x, params + values)
    let sections: [(f32, f32, Color32, u8, bool, &str, [(f32, &str, u8, String); 2]); 4] = [
        (
            126.0,
            22.0,
            ORANGE,
            ecp::GATE,
            e0.gate,
            "Gate",
            [(140.0, "Threshold", ecp::GATE_THR, format!("{:.1} dB", e0.gate_thr)), (193.0, "Release", ecp::GATE_REL, ms(e0.gate_rel))],
        ),
        (
            266.0,
            22.0,
            ORANGE,
            ecp::DUCK,
            e0.duck,
            "Ducking",
            [(280.0, "Threshold", ecp::DUCK_THR, format!("{:.1} dB", e0.duck_thr)), (334.0, "Release", ecp::DUCK_REL, ms(e0.duck_rel))],
        ),
        (
            126.0,
            97.0,
            TEAL,
            ecp::NOISE,
            e0.noise,
            "Noise",
            [(140.0, "Amount", ecp::NOISE_AMT, pct(e0.noise_amt)), (193.0, "Morph", ecp::NOISE_MORPH, pct(e0.noise_morph))],
        ),
        (
            266.0,
            97.0,
            TEAL,
            ecp::WOBBLE,
            e0.wobble,
            "Wobble",
            [(280.0, "Amount", ecp::WOB_AMT, pct(e0.wobble_amt)), (334.0, "Morph", ecp::WOB_MORPH, pct(e0.wobble_morph))],
        ),
    ];
    for (x, y, accent, k, sw, name, params) in sections {
        let acc = if on { accent } else { gray(0x75) };
        let sq = Rect::from_min_size(pos2(l + x, t + y), vec2(9.0, 9.0));
        check(p, sq, sw, acc);
        let nr = text_at(p, l + x + 14.0, t + y + 8.0, Align2::LEFT_BOTTOM, name, font(FS), LABEL);
        if cx.toggle(ui, Rect::from_min_max(pos2(sq.left() - 2.0, sq.top() - 2.0), pos2(nr.right() + 2.0, sq.bottom() + 2.0)), k, true) {
            set(cx, |e| match k {
                ecp::GATE => e.gate = !e.gate,
                ecp::DUCK => e.duck = !e.duck,
                ecp::NOISE => e.noise = !e.noise,
                _ => e.wobble = !e.wobble,
            });
        }
        for (lx, lab, pk, val) in params {
            text_at(p, l + lx, t + y + 33.0, Align2::LEFT_BOTTOM, lab, font(FS), LABEL);
            drag_text(cx, ui, p, l + lx + 2.0, t + y + 48.0, pk, &val, true, if sw { accent } else { LABEL });
        }
    }
    let sq = Rect::from_min_size(pos2(l + 126.0, t + 172.0), vec2(9.0, 9.0));
    check(p, sq, e0.repitch, if on { TEAL } else { gray(0x75) });
    let nr = text_at(p, l + 140.0, t + 180.0, Align2::LEFT_BOTTOM, "Repitch", font(FS), LABEL);
    if cx.toggle(ui, Rect::from_min_max(pos2(l + 124.0, t + 170.0), pos2(nr.right() + 2.0, t + 183.0)), ecp::REPITCH, true) {
        set(cx, |e| e.repitch = !e.repitch);
    }
    text_at(p, l + 193.0, t + 180.0, Align2::LEFT_BOTTOM, "Smoothing Time", font(FS), LABEL);
    drag_text(cx, ui, p, l + 277.0, t + 180.0, ecp::SMOOTH, &ms(e0.smooth_ms), true, if e0.repitch { TEAL } else { LABEL });
}

// ---- parameters --------------------------------------------------------------

pub(super) fn reset(p: &mut FxParams, k: u8) {
    let FxParams::Echo(e) = p else { return };
    let d = EchoParams::default();
    match k {
        ecp::MODE => e.mode = d.mode,
        ecp::LINK => e.link = d.link,
        ecp::CLIP => e.clip_dry = false,
        ecp::INVERT => e.invert = false,
        ecp::FILTER => e.filter_on = d.filter_on,
        ecp::WAVE => e.wave = 0,
        ecp::MOD_SYNC => e.mod_sync = false,
        ecp::X4 => e.mod_x4 = d.mod_x4,
        ecp::GATE => e.gate = d.gate,
        ecp::DUCK => e.duck = d.duck,
        ecp::NOISE => e.noise = d.noise,
        ecp::WOBBLE => e.wobble = d.wobble,
        ecp::REPITCH => e.repitch = false,
        ecp::VERB_LOC => e.verb_loc = d.verb_loc,
        41 | 42 => e.sync[(k - 41) as usize] = true,
        43 | 44 => e.sync_mode[(k - 43) as usize] = 0,
        _ => {
            let df = FxParams::Echo(EchoParams { link: e.link, sync: e.sync, sync_mode: e.sync_mode, mod_sync: e.mod_sync, ..d });
            if let Some(v) = norm(&df, k) {
                set_norm(p, k, v);
            }
        }
    }
}

pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    let FxParams::Echo(e) = p else { return };
    match k {
        ecp::MODE => e.mode = (e.mode as i32 + dir as i32).clamp(0, 2) as u8,
        ecp::VERB_LOC => e.verb_loc = (e.verb_loc as i32 + dir as i32).clamp(0, 2) as u8,
        ecp::WAVE => e.wave = (e.wave as i32 + dir as i32).clamp(0, 5) as u8,
        43 | 44 => {
            let s = (k - 43) as usize;
            e.sync_mode[s] = (e.sync_mode[s] as i32 + dir as i32).clamp(0, 3) as u8;
        }
        k if k >= 40 => {}
        0 | 1 => {
            // Synced times step by one division.
            let s = if e.link { 0 } else { k as usize };
            if e.sync[s] {
                let n = if e.sync_mode[s] == 3 { 16 } else { ECHO_NOTES.len() };
                if let Some(v) = norm(p, k) {
                    set_norm(p, k, v + dir / (n - 1) as f32);
                }
            } else if let Some(v) = norm(p, k) {
                set_norm(p, k, v + dir / 200.0);
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
        0 => "L Time",
        1 => "R Time",
        2 => "L Offset",
        3 => "R Offset",
        ecp::INPUT => "Input Gain",
        ecp::FEEDBACK => "Feedback",
        ecp::HP => "HP Freq",
        ecp::HP_RES => "HP Res",
        ecp::LP => "LP Freq",
        ecp::LP_RES => "LP Res",
        ecp::MOD_RATE => "Mod Freq",
        ecp::PHASE => "Mod Phase",
        ecp::MOD_DELAY => "Dly < Mod",
        ecp::MOD_FILTER => "Filter < Mod",
        ecp::ENV_MIX => "Env Mix",
        ecp::GATE_THR => "Gate Thr",
        ecp::GATE_REL => "Gate Release",
        ecp::DUCK_THR => "Duck Thr",
        ecp::DUCK_REL => "Duck Release",
        ecp::NOISE_AMT => "Noise Amt",
        ecp::NOISE_MORPH => "Noise Mrph",
        ecp::WOB_AMT => "Wobble Amt",
        ecp::WOB_MORPH => "Wobble Mrph",
        ecp::SMOOTH => "Repitch Smoothing",
        ecp::REVERB => "Reverb Level",
        ecp::VERB_DECAY => "Reverb Decay",
        ecp::STEREO => "Stereo Width",
        ecp::OUTPUT => "Output Gain",
        ecp::DRY_WET => "Dry Wet",
        ecp::MODE => "Channel Mode",
        41 => "L Sync",
        42 => "R Sync",
        43 => "L Sync Mode",
        44 => "R Sync Mode",
        ecp::LINK => "Link",
        ecp::CLIP => "Clip Dry",
        ecp::INVERT => "Feedback Inv",
        ecp::TAB => "Tab",
        ecp::FILTER => "Filter On",
        ecp::FILTER_SHOWN => "Filter View",
        ecp::WAVE => "Mod Wave",
        ecp::MOD_SYNC => "Mod Sync",
        ecp::X4 => "Mod 4x",
        ecp::GATE => "Gate On",
        ecp::DUCK => "Duck On",
        ecp::NOISE => "Noise On",
        ecp::WOBBLE => "Wobble On",
        ecp::REPITCH => "Repitch",
        ecp::VERB_LOC => "Reverb Loc",
        _ => "Equal Loudness",
    }
}
