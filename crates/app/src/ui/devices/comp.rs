//! Compressor (Live 12): Ratio / Attack / Release column, a display with
//! three views (activity, transfer curve, collapsed meters), Makeup, Peak /
//! RMS / Expand, Dry/Wet, and the sidechain strip + columns. Geometry
//! measured from Live 12 at 100 % (l/t = device left/top; text positions
//! are baselines).

use super::dynamics::{KN, knob_lv, sc_columns, sc_get, sc_reset, sc_strip, sig3, tbutton};
use super::modfx::{drag_text, text_menu};
use super::*;
use daw_engine::dynfx::{COMP_KNEE_MAX, COMP_LOOKAHEAD, COMP_RATIO_INF, CompMode, CompView, CompressorParams, lin_db};
use daw_project::devices::dev_tap;
use daw_project::devices::params::{cmp, sc};

pub const CMP_W: f32 = 469.0;
/// Collapsed view (meters only).
pub const CMP_COLLAPSED_W: f32 = 338.0;
/// Unfolded sidechain (External column).
pub const CMP_SC_W: f32 = 78.0;

pub fn width(c: &CompressorParams) -> f32 {
    (if c.view == CompView::Collapsed { CMP_COLLAPSED_W } else { CMP_W }) + if c.sc.shown { CMP_SC_W } else { 0.0 }
}

/// Grey labels in the display.
const LABEL: Color32 = Color32::from_rgb(0x9a, 0x9a, 0x9a);
const GR_ORANGE: Color32 = Color32::from_rgb(0xff, 0xa4, 0x18);
const GR_BLUE: Color32 = Color32::from_rgb(0x4f, 0x7f, 0xe0);

fn db2(x: f32) -> String {
    if x.abs() < 9.995 { format!("{x:.2} dB") } else { format!("{x:.1} dB") }
}

fn ms3(x: f32) -> String {
    if x >= 999.5 { format!("{:.2} s", x / 1000.0) } else { format!("{} ms", sig3(x)) }
}

pub(super) fn ratio_text(c: &CompressorParams) -> String {
    if c.mode == CompMode::Expand {
        format!("1 : {:.2}", c.expand_ratio())
    } else if c.ratio >= COMP_RATIO_INF {
        "inf : 1".into()
    } else {
        format!("{:.2} : 1", c.ratio)
    }
}

// ---- level meters ----------------------------------------------------------

/// Meter scale: 2.83 px per dB, 0 dB 17 px below the inner top (+6 dB).
fn meter_y(t: f32, db: f32) -> f32 {
    (t + 52.0 - db * 2.83).clamp(t + 35.0, t + 149.0)
}

fn meter_db(t: f32, y: f32) -> f32 {
    (t + 52.0 - y) / 2.83
}

fn lerp(a: (u8, u8, u8), b: (u8, u8, u8), u: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * u.clamp(0.0, 1.0)).round() as u8;
    Color32::from_rgb(f(a.0, b.0), f(a.1, b.1), f(a.2, b.2))
}

/// Live's device meter colour at `db`: red over 0 dB, orange → yellow,
/// yellow → green.
fn level_col(db: f32) -> Color32 {
    const ORA: (u8, u8, u8) = (0xff, 0x82, 0x0c);
    const YEL: (u8, u8, u8) = (0xfc, 0xbd, 0x00);
    const GRN: (u8, u8, u8) = (0x00, 0xf7, 0x58);
    if db > 0.0 {
        Color32::from_rgb(0xfe, 0x0a, 0x0a)
    } else if db > -7.0 {
        lerp(ORA, YEL, -db / 7.0)
    } else if db > -14.0 {
        lerp(YEL, YEL, 0.0)
    } else if db > -18.0 {
        lerp(YEL, GRN, (-14.0 - db) / 4.0)
    } else {
        lerp(GRN, GRN, 0.0)
    }
}

/// Smoothed meter values per device: (peak, rms, hold, hold time).
#[derive(Clone, Copy, Default)]
struct Ballistics {
    v: [[f32; 4]; 2],
}

/// One bar column `x0..x1` filled from `level` (peak, dim) and `rms`
/// (bright) down to the bottom, with a 1 px held peak.
fn bar(p: &Painter, t: f32, x0: f32, x1: f32, pk: f32, rms: f32, hold: f32) {
    let fill = |a: f32, dim: bool| {
        if a <= 1e-5 {
            return;
        }
        let top = meter_y(t, lin_db(a)).round();
        let mut y = top;
        while y < t + 149.0 {
            let c = level_col(meter_db(t, y + 0.5));
            let c = if dim { c.gamma_multiply(0.66) } else { c };
            quad(p, Rect::from_min_max(pos2(x0, y), pos2(x1, y + 1.0)), c);
            y += 1.0;
        }
    };
    fill(pk, true);
    fill(rms, false);
    if hold > rms && hold > 1e-5 {
        let y = meter_y(t, lin_db(hold)).round();
        quad(p, Rect::from_min_max(pos2(x0, y), pos2(x1, y + 1.0)), level_col(lin_db(hold)));
    }
}

/// Meter frame (11 px wide at `x`, t+34..t+150) with the 0 dB notch.
fn frame(p: &Painter, x: f32, t: f32) {
    let r = Rect::from_min_max(pos2(x, t + 34.0), pos2(x + 11.0, t + 150.0));
    p.rect_filled(r, CornerRadius::ZERO, gray(0x3e));
    p.rect_filled(r.shrink(1.0), CornerRadius::ZERO, gray(0x18));
}

/// ◀ handle (tip at `x`) at height `y`.
fn handle(p: &Painter, x: f32, y: f32, en: bool) {
    p.add(Shape::convex_polygon(
        vec![pos2(x, y), pos2(x + 8.0, y - 5.0), pos2(x + 8.0, y + 5.0)],
        if en { gray(0x82) } else { gray(0x55) },
        Stroke::NONE,
    ));
}

/// Draggable handle for a dB parameter on the meter scale.
fn handle_drag(cx: &mut Ctx, ui: &mut egui::Ui, r: Rect, t: f32, k: u8) {
    cx.salt = 2;
    let resp = cx.sense(ui, r, k, Sense::click_and_drag());
    cx.salt = 0;
    if resp.drag_started() {
        cx.focus(k);
    }
    if resp.dragged()
        && let Some(pp) = resp.interact_pointer_pos()
    {
        let db = meter_db(t, pp.y);
        cx.undo_step();
        if let FxParams::Compressor(c) = &mut cx.params {
            match k {
                cmp::THRESHOLD => c.threshold = db.clamp(daw_engine::dynfx::COMP_THRESH.0, daw_engine::dynfx::COMP_THRESH.1),
                _ => c.output = db.clamp(-daw_engine::dynfx::COMP_OUT_MAX, daw_engine::dynfx::COMP_OUT_MAX),
            }
        }
    }
}

/// Centered orange draggable value under a meter.
fn value_c(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, x: f32, base: f32, k: u8, s: &str) {
    let w = p.layout_no_wrap(s.to_string(), font(FS), ORANGE).size().x;
    drag_text(cx, ui, p, x - w / 2.0, base, k, s, true, ORANGE);
}

// ---- display icons ---------------------------------------------------------

fn view_icon(p: &Painter, c: Pos2, v: CompView, col: Color32) {
    let s = Stroke::new(1.0, col);
    match v {
        CompView::Collapsed => {
            for (dx, h) in [(-3.0, 4.0), (0.0, 8.0), (3.0, 5.0)] {
                quad(p, Rect::from_min_max(pos2(c.x + dx - 0.5, c.y + 4.0 - h), pos2(c.x + dx + 0.5, c.y + 4.0)), col);
            }
        }
        CompView::Curve => {
            p.add(Shape::line(vec![pos2(c.x - 4.0, c.y + 4.0), pos2(c.x - 0.5, c.y - 2.5), pos2(c.x + 4.5, c.y - 3.0)], s));
        }
        CompView::Activity => {
            quad(p, Rect::from_min_max(pos2(c.x - 5.0, c.y - 0.5), pos2(c.x + 6.0, c.y + 0.5)), col);
            for (i, h) in [3.0, 5.0, 4.0, 5.5, 3.5, 2.5].into_iter().enumerate() {
                let x = c.x - 4.5 + i as f32 * 2.0;
                quad(p, Rect::from_min_max(pos2(x - 0.5, c.y - h), pos2(x + 0.5, c.y + h * 0.6)), col);
            }
        }
    }
}

// ---- the device ------------------------------------------------------------

pub(super) fn compressor(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect) {
    let FxParams::Compressor(c0) = cx.params else { return };
    let (l0, t) = (r.left(), r.top());
    let s = sc_get(&cx.params);
    let ext = s.on && s.source != 0;
    p.rect_filled(Rect::from_min_max(pos2(l0, t + 17.0), r.max), CornerRadius { nw: 0, ne: 0, sw: 3, se: 3 }, gray(0x46));
    let pnl = |x0: f32, x1: f32| {
        p.rect_filled(Rect::from_min_max(pos2(x0, t + 17.0), pos2(x1, t + 186.0)), CornerRadius::same(2), panel());
    };
    let ox = if s.shown { CMP_SC_W } else { 0.0 };
    // Sidechain: strip + SC Filter column (+ External when unfolded).
    pnl(l0 + 2.0, l0 + 81.0 + ox);
    sc_strip(cx, ui, p, l0 + 1.5, t, false);
    if s.shown {
        sc_columns(cx, ui, p, t, Some(l0 + 26.0), l0 + 104.0);
    } else {
        sc_columns(cx, ui, p, t, None, l0 + 26.0);
    }
    let l = l0 + ox;
    let collapsed = c0.view == CompView::Collapsed;
    let dr_x = if collapsed { l + 275.0 } else { l + 406.0 };
    pnl(l + 83.0, l + 142.0);
    pnl(dr_x + 2.0, dr_x + 61.0);

    // ---- Ratio / Attack / Release / Auto ----
    let expand = c0.mode == CompMode::Expand;
    knob_lv(cx, ui, p, l + 112.5, t + 30.0, cmp::RATIO, "Ratio", &ratio_text(&c0), 0.0, KN, 15.0, 36.0, true);
    knob_lv(cx, ui, p, l + 112.5, t + 79.5, cmp::ATTACK, "Attack", &ms3(c0.attack), 0.0, KN, 15.0, 36.0, true);
    knob_lv(cx, ui, p, l + 112.5, t + 129.0, cmp::RELEASE, "Release", &ms3(c0.release), 0.0, KN, 15.0, 36.0, !c0.auto_release);
    if tbutton(cx, ui, p, Rect::from_min_size(pos2(l + 87.0, t + 170.0), vec2(51.0, 13.0)), cmp::AUTO, "Auto", c0.auto_release, true)
        && let FxParams::Compressor(c) = &mut cx.params
    {
        c.auto_release = !c.auto_release;
    }

    // ---- right column ----
    let rx = dr_x + 6.0;
    if tbutton(cx, ui, p, Rect::from_min_size(pos2(rx, t + 21.0), vec2(51.0, 13.0)), cmp::MAKEUP, "Makeup", c0.makeup, !ext && !expand)
        && let FxParams::Compressor(c) = &mut cx.params
    {
        c.makeup = !c.makeup;
    }
    for (i, (m, label)) in [(CompMode::Peak, "Peak"), (CompMode::Rms, "RMS"), (CompMode::Expand, "Expand")].into_iter().enumerate() {
        cx.salt = i as u32;
        if tbutton(
            cx,
            ui,
            p,
            Rect::from_min_size(pos2(rx, t + 62.0 + 16.0 * i as f32), vec2(51.0, 13.0)),
            cmp::MODE,
            label,
            c0.mode == m,
            true,
        ) && let FxParams::Compressor(c) = &mut cx.params
        {
            c.mode = m;
        }
        cx.salt = 0;
    }
    knob_lv(cx, ui, p, rx + 25.5, t + 144.0, cmp::DRY_WET, "Dry/Wet", &format!("{:.0} %", c0.dry_wet * 100.0), 0.0, KN, 15.5, 36.0, true);

    // ---- display ----
    let d = Rect::from_min_max(pos2(l + 144.0, t + 17.0), pos2(dr_x, t + 186.0));
    p.rect_filled(d, CornerRadius::same(2), gray(0x18));
    let dp = p.with_clip_rect(d.intersect(p.clip_rect()));
    let tap = dev_tap(cx.dev.id);
    if tap.written() > 0 {
        cx.app.animating = true;
    }
    // Meter ballistics (GUI side): peaks fall ~24 dB/s, holds last 1.5 s.
    let bid = egui::Id::unique(("cmp_meter", cx.dev.id));
    let now = ui.input(|i| i.time) as f32;
    let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.1);
    let mut bl: Ballistics = ui.data(|dd| dd.get_temp(bid)).unwrap_or_default();
    let fall = 10f32.powf(-24.0 * dt / 20.0);
    for (j, (kp, kr)) in [(1usize, 5usize), (2, 6)].into_iter().enumerate() {
        let (pk, rm) = (tap.take(kp, 0.0), tap.take(kr, 0.0));
        let v = &mut bl.v[j];
        v[0] = pk.max(v[0] * fall);
        v[1] = rm.max(v[1] * fall);
        if pk >= v[2] || now - v[3] > 1.5 {
            v[2] = pk;
            v[3] = now;
        }
    }
    ui.data_mut(|dd| dd.insert_temp(bid, bl));
    let gr_now = tap.get(0).max(0.0);
    let det = tap.get(3);

    // Bottom row: view switches, Knee (+ Lookahead, Envelope).
    quad(p, Rect::from_min_max(pos2(d.left(), t + 166.0), pos2(d.right(), t + 167.0)), gray(0x3e));
    for (i, v) in [CompView::Collapsed, CompView::Curve, CompView::Activity].into_iter().enumerate() {
        let x0 = l + 144.0 + 20.0 * i as f32;
        quad(p, Rect::from_min_max(pos2(x0 + 20.0, t + 167.0), pos2(x0 + 21.0, t + 186.0)), gray(0x3e));
        let sel = c0.view == v;
        let col = if sel { if cx.on { ORANGE } else { gray(0x80) } } else { gray(0xa0) };
        view_icon(p, pos2(x0 + 10.0, t + 176.5), v, col);
        let br = Rect::from_min_max(pos2(x0, t + 167.0), pos2(x0 + 20.0, t + 186.0));
        cx.salt = 10 + i as u32;
        if cx.toggle(ui, br, cmp::VIEW, true)
            && let FxParams::Compressor(c) = &mut cx.params
        {
            c.view = v;
        }
        cx.salt = 0;
    }
    text_at(p, l + 212.0, t + 180.0, Align2::LEFT_BOTTOM, "Knee", font(FS), LABEL);
    drag_text(cx, ui, p, l + 239.0, t + 180.0, cmp::KNEE, &format!("{:.1} dB", c0.knee.clamp(0.0, COMP_KNEE_MAX)), true, TEAL);
    if !collapsed {
        text_at(p, l + 278.0, t + 180.0, Align2::LEFT_BOTTOM, "Look.", font(FS), LABEL);
        let la = ["0 ms", "1 ms", "10 ms"];
        let lr = Rect::from_min_max(pos2(l + 305.0, t + 169.0), pos2(l + 343.0, t + 184.0));
        if let Some(i) =
            text_menu(cx, ui, p, lr, cmp::LOOKAHEAD, l + 307.0, t + 180.0, Some(l + 334.0), (c0.lookahead as usize).min(2), &la, true, TEAL)
            && let FxParams::Compressor(c) = &mut cx.params
        {
            c.lookahead = i as u8;
        }
        let _ = COMP_LOOKAHEAD;
        text_at(p, l + 350.0, t + 180.0, Align2::LEFT_BOTTOM, "Env.", font(FS), LABEL);
        let er = Rect::from_min_max(pos2(l + 373.0, t + 172.0), pos2(l + 398.0, t + 182.0));
        p.rect_filled(er, CornerRadius::ZERO, if cx.on { TEAL } else { gray(0x5a) });
        text_at(p, er.center().x, t + 181.0, Align2::CENTER_BOTTOM, if c0.log_env { "Log" } else { "Lin" }, font(FS), gray(0x18));
        if cx.toggle(ui, er, cmp::ENV, true)
            && let FxParams::Compressor(c) = &mut cx.params
        {
            c.log_env = !c.log_env;
        }
    }

    match c0.view {
        CompView::Activity => activity(cx, ui, p, &dp, l, t, &c0, &tap),
        CompView::Curve | CompView::Collapsed => {
            // Meters: Thresh (input), GR, Out; the transfer curve between.
            let (xt, xg, xo) = if collapsed { (l + 163.0, l + 204.0, l + 245.0) } else { (l + 163.0, l + 337.0, l + 376.0) };
            for (x, s) in [(xt, "Thresh"), (xg, "GR"), (xo, "Out")] {
                text_at(p, x + 5.5, t + 30.0, Align2::CENTER_BOTTOM, s, font(FS), LABEL);
                frame(p, x, t);
            }
            for (j, x) in [(0usize, xt), (1, xo)] {
                let v = bl.v[j];
                for dx in [1.0, 6.0] {
                    bar(p, t, x + dx, x + dx + 4.0, v[0], v[1], v[2]);
                }
                quad(p, Rect::from_min_max(pos2(x + 1.0, t + 52.0), pos2(x + 10.0, t + 53.0)), gray(0x3e));
            }
            // GR fills down from the 0 dB notch (blue when expanding).
            if gr_now > 0.01 {
                let y1 = (t + 52.0 + gr_now * 2.83).min(t + 149.0);
                quad(p, Rect::from_min_max(pos2(xg + 1.0, t + 52.0), pos2(xg + 10.0, y1)), if expand { GR_BLUE } else { GR_ORANGE });
            }
            quad(p, Rect::from_min_max(pos2(xg + 1.0, t + 51.0), pos2(xg + 10.0, t + 52.0)), gray(0x3e));
            let grt = if gr_now < 0.05 {
                "0.0".to_string()
            } else if expand {
                format!("{gr_now:.1}")
            } else if gr_now < 9.95 {
                format!("-{gr_now:.1}")
            } else {
                format!("-{gr_now:.0}")
            };
            text_at(p, xg + 5.5, t + 161.0, Align2::CENTER_BOTTOM, &grt, font(FS), LABEL);
            // Threshold / output handles and values.
            let (yt, yo) = (meter_y(t, c0.threshold), meter_y(t, c0.output));
            handle(p, xt + 12.0, yt, cx.on);
            handle(p, xo + 12.0, yo, cx.on);
            handle_drag(cx, ui, Rect::from_min_max(pos2(xt, t + 30.0), pos2(xt + 22.0, t + 154.0)), t, cmp::THRESHOLD);
            handle_drag(cx, ui, Rect::from_min_max(pos2(xo, t + 30.0), pos2(xo + 22.0, t + 154.0)), t, cmp::OUTPUT);
            value_c(cx, ui, p, xt + 5.5, t + 161.0, cmp::THRESHOLD, &db2(c0.threshold));
            value_c(cx, ui, p, xo + 5.5, t + 161.0, cmp::OUTPUT, &db2(c0.output));
            if !collapsed {
                curve(cx, ui, p, l, t, &c0, det);
            }
        }
    }
}

/// Transfer curve box: input dB → x, output dB → y (Live: 2.65 / 2.3 px
/// per dB, 0 dB at 112 px from the left, 17 px below the top).
fn curve(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, c0: &CompressorParams, det: f32) {
    let b = Rect::from_min_max(pos2(l + 195.0, t + 34.0), pos2(l + 325.0, t + 150.0));
    p.rect_filled(b, CornerRadius::ZERO, gray(0x3e));
    p.rect_filled(b.shrink(1.0), CornerRadius::ZERO, gray(0x18));
    for i in 1..8 {
        let x = (b.left() + i as f32 * (b.width() - 1.0) / 8.0).round();
        quad(p, Rect::from_min_max(pos2(x, b.top() + 1.0), pos2(x + 1.0, b.bottom() - 1.0)), gray(0x2b));
        let y = (b.top() + i as f32 * (b.height() - 1.0) / 8.0).round();
        quad(p, Rect::from_min_max(pos2(b.left() + 1.0, y), pos2(b.right() - 1.0, y + 1.0)), gray(0x2b));
    }
    let x_of = |db: f32| b.left() + 112.0 + db * 2.65;
    let y_of = |db: f32| b.top() + 17.0 - db * 2.3;
    let db_of_x = |x: f32| (x - b.left() - 112.0) / 2.65;
    let cp = p.with_clip_rect(b.shrink(1.0).intersect(p.clip_rect()));
    // Knee: lighter verticals at threshold ± knee.
    if c0.knee > 0.05 {
        for dk in [-1.0, 1.0] {
            let x = x_of(c0.threshold + dk * c0.knee).round();
            quad(&cp, Rect::from_min_max(pos2(x, b.top() + 1.0), pos2(x + 1.0, b.bottom() - 1.0)), gray(0x60));
        }
    }
    let mk = c0.makeup_db();
    let n = 130;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let x = b.left() + 1.0 + i as f32 * (b.width() - 2.0) / n as f32;
            let db = db_of_x(x);
            pos2(x, y_of(c0.transfer(db) + mk))
        })
        .collect();
    let col = if cx.on { TEAL } else { gray(0x75) };
    cp.add(Shape::line(pts, Stroke::new(1.5, col)));
    // Threshold handle (big dot) and the signal (small dot).
    let hc = pos2(x_of(c0.threshold), y_of(c0.transfer(c0.threshold) + mk));
    cp.circle_filled(hc, 6.5, if cx.on { ORANGE } else { gray(0x75) });
    if det > -100.0 && cx.app.animating {
        let sx = det.clamp(-60.0, 12.0);
        cp.circle_filled(pos2(x_of(sx), y_of(c0.transfer(sx) + mk)), 2.5, if cx.on { ORANGE } else { gray(0x75) });
    }
    cx.salt = 3;
    let resp = cx.sense(ui, Rect::from_center_size(hc, vec2(16.0, 16.0)), cmp::THRESHOLD, Sense::click_and_drag());
    cx.salt = 0;
    if resp.drag_started() {
        cx.focus(cmp::THRESHOLD);
    }
    if resp.dragged() {
        let dlt = resp.drag_delta();
        let ddb = dlt.x / 2.65 - dlt.y / 2.3;
        cx.undo_step();
        if let FxParams::Compressor(c) = &mut cx.params {
            c.threshold = (c.threshold + ddb).clamp(daw_engine::dynfx::COMP_THRESH.0, daw_engine::dynfx::COMP_THRESH.1);
        }
    }
}

/// Activity view: header (Thresh, GR / Output, Out), the level history and
/// the threshold line (draggable).
#[allow(clippy::too_many_arguments)]
fn activity(
    cx: &mut Ctx,
    ui: &mut egui::Ui,
    p: &Painter,
    dp: &Painter,
    l: f32,
    t: f32,
    c0: &CompressorParams,
    tap: &daw_engine::devtap::DevTap,
) {
    quad(p, Rect::from_min_max(pos2(l + 144.0, t + 36.0), pos2(l + 406.0, t + 37.0)), gray(0x3e));
    text_at(p, l + 150.0, t + 30.0, Align2::LEFT_BOTTOM, "Thresh", font(FS), LABEL);
    drag_text(cx, ui, p, l + 187.0, t + 30.0, cmp::THRESHOLD, &db2(c0.threshold), true, TEAL);
    for (i, (x, label, on)) in [(l + 246.0, "GR", !c0.show_output), (l + 277.0, "Output", c0.show_output)].into_iter().enumerate() {
        let sq = Rect::from_min_size(pos2(x, t + 22.0), vec2(7.0, 7.0));
        let col = if cx.on { TEAL } else { gray(0x75) };
        if on {
            p.rect_filled(sq, CornerRadius::ZERO, col);
        } else {
            p.rect_stroke(sq, CornerRadius::ZERO, Stroke::new(1.0, col), egui::StrokeKind::Inside);
        }
        let tr = text_at(p, x + 11.0, t + 30.0, Align2::LEFT_BOTTOM, label, font(FS), LABEL);
        cx.salt = i as u32;
        if cx.toggle(ui, Rect::from_min_max(pos2(x - 2.0, t + 19.0), pos2(tr.right() + 2.0, t + 33.0)), cmp::SHOW_OUT, true)
            && let FxParams::Compressor(c) = &mut cx.params
        {
            c.show_output = i == 1;
        }
        cx.salt = 0;
    }
    text_at(p, l + 336.0, t + 30.0, Align2::LEFT_BOTTOM, "Out", font(FS), LABEL);
    drag_text(cx, ui, p, l + 358.0, t + 30.0, cmp::OUTPUT, &db2(c0.output), true, TEAL);

    // History graph: 2.69 px per dB, 0 dB at t+56.5.
    let (gx0, gx1, gy0, gy1) = (l + 144.0, l + 406.0, t + 37.0, t + 166.0);
    let y_of = |db: f32| (t + 56.5 - db * 2.69).clamp(gy0, gy1);
    let cols = (gx1 - gx0) as usize;
    const PER: usize = 4;
    let w = tap.written();
    let h = tap.history_to(w - w % PER, cols * PER);
    let off = cols * PER - h.len().min(cols * PER);
    let mut outline: Vec<Pos2> = Vec::new();
    let mut runs: Vec<Vec<Pos2>> = Vec::new();
    let mut last = [0.0f32; 4];
    for c in 0..cols {
        let a = (c * PER).saturating_sub(off);
        let b = ((c + 1) * PER).saturating_sub(off).min(h.len());
        if a >= b {
            continue;
        }
        let mut m = [0.0f32; 4];
        for f in &h[a..b] {
            for k in 0..4 {
                m[k] = m[k].max(f[k]);
            }
        }
        last = m;
        let x = gx0 + c as f32;
        if m[0].max(m[1]) < 3.2e-4 {
            if outline.len() > 1 {
                runs.push(std::mem::take(&mut outline));
            }
            outline.clear();
            continue;
        }
        let yi = y_of(lin_db(m[0]));
        if c0.show_output {
            let yo = y_of(lin_db(m[1]));
            quad(dp, Rect::from_min_max(pos2(x, yi), pos2(x + 1.0, gy1)), gray(0x62));
            quad(dp, Rect::from_min_max(pos2(x, yo), pos2(x + 1.0, gy1)), gray(0x3c));
            outline.push(pos2(x + 0.5, yo));
        } else {
            quad(dp, Rect::from_min_max(pos2(x, yi), pos2(x + 1.0, gy1)), gray(0x53));
            let gr = m[2] * 100.0;
            if gr > 0.05 {
                quad(dp, Rect::from_min_max(pos2(x, gy0), pos2(x + 1.0, (gy0 + gr * 2.69).min(gy1))), GR_ORANGE);
            }
        }
    }
    if outline.len() > 1 {
        runs.push(outline);
    }
    for run in runs {
        dp.add(Shape::line(run, Stroke::new(1.0, gray(0xd8))));
    }
    // Threshold line.
    let ty = y_of(c0.threshold).floor();
    quad(dp, Rect::from_min_max(pos2(gx0, ty), pos2(gx1, ty + 2.0)), if cx.on { TEAL } else { gray(0x75) });
    let gr = Rect::from_min_max(pos2(gx0, gy0), pos2(gx1, gy1));
    cx.salt = 1;
    let resp = cx.sense(ui, gr, cmp::THRESHOLD, Sense::click_and_drag());
    cx.salt = 0;
    if resp.drag_started() {
        cx.focus(cmp::THRESHOLD);
    }
    if resp.dragged()
        && let Some(pp) = resp.interact_pointer_pos()
    {
        let db = (t + 56.5 - pp.y) / 2.69;
        cx.undo_step();
        if let FxParams::Compressor(c) = &mut cx.params {
            c.threshold = db.clamp(daw_engine::dynfx::COMP_THRESH.0, daw_engine::dynfx::COMP_THRESH.1);
        }
    }
    // Readout: the reduction (GR view) or the output level (Output view).
    let v = if c0.show_output { if last[1] > 1e-6 { lin_db(last[1]) } else { -70.0 } } else { -(last[2] * 100.0) };
    let s = if v <= -69.9 { "-inf dB".to_string() } else { format!("{:.2} dB", if v.abs() < 0.005 { 0.0 } else { v }) };
    text_at(p, gx1 - 9.0, t + 160.0, Align2::RIGHT_BOTTOM, &s, font(FS), ORANGE);
}

// ---- parameters --------------------------------------------------------------

pub(super) fn reset(p: &mut FxParams, k: u8) {
    let FxParams::Compressor(c) = p else { return };
    let d = CompressorParams::default();
    match k {
        cmp::THRESHOLD => c.threshold = d.threshold,
        cmp::RATIO => c.ratio = d.ratio,
        cmp::ATTACK => c.attack = d.attack,
        cmp::RELEASE => c.release = d.release,
        cmp::AUTO => c.auto_release = d.auto_release,
        cmp::KNEE => c.knee = d.knee,
        cmp::LOOKAHEAD => c.lookahead = d.lookahead,
        cmp::ENV => c.log_env = d.log_env,
        cmp::MODE => c.mode = d.mode,
        cmp::MAKEUP => c.makeup = d.makeup,
        cmp::OUTPUT => c.output = d.output,
        cmp::DRY_WET => c.dry_wet = d.dry_wet,
        sc::FREQ => c.sc.freq = d.sc.freq,
        _ => sc_reset(&mut c.sc, k),
    }
}

pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    match (&mut *p, k) {
        (FxParams::Compressor(c), cmp::LOOKAHEAD) => c.lookahead = (c.lookahead as i32 - dir as i32).clamp(0, 2) as u8,
        (FxParams::Compressor(c), cmp::MODE) => {
            let i = (c.mode as i32 - dir as i32).clamp(0, 2);
            c.mode = [CompMode::Peak, CompMode::Rms, CompMode::Expand][i as usize];
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
        cmp::THRESHOLD => "Threshold",
        cmp::RATIO => "Ratio",
        cmp::ATTACK => "Attack",
        cmp::RELEASE => "Release",
        cmp::AUTO => "Auto Release",
        cmp::KNEE => "Knee",
        cmp::LOOKAHEAD => "Lookahead",
        cmp::ENV => "Envelope Mode",
        cmp::MODE => "Model",
        cmp::MAKEUP => "Makeup",
        cmp::OUTPUT => "Output Gain",
        cmp::DRY_WET => "Dry/Wet",
        cmp::VIEW => "Display",
        cmp::SHOW_OUT => "Activity View",
        sc::ON => "S/C On",
        sc::SOURCE => "S/C Source",
        sc::TAP => "S/C Tap",
        sc::GAIN => "S/C Gain",
        sc::MIX => "S/C Mix",
        sc::EQ_ON => "S/C EQ On",
        sc::EQ => "S/C EQ Type",
        sc::FREQ => "S/C EQ Freq",
        sc::Q => "S/C EQ Q",
        sc::EQ_GAIN => "S/C EQ Gain",
        sc::LISTEN => "S/C Listen",
        _ => "",
    }
}
