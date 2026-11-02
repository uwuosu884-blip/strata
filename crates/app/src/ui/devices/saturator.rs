//! Saturator (Live 12): curve chooser and transfer display, Post Clip, Color
//! with Amt Lo, Drive / Output / Dry/Wet; the title-bar ▶/▼ button opens the
//! extended view with the Color Curve (Amt Hi, Frequency, Width) and the
//! Waveshaper parameters. Geometry measured from Live 12 at 100 % (l/t =
//! device left/top; text positions are baselines).

use super::dynamics::{KN, knob_lv, sig3, tbutton};
use super::modfx::{drag_text, text_menu};
use super::redux::pct;
use super::*;
use daw_engine::saturator::*;
use daw_project::devices::params3::sap;

pub const WIDTH: f32 = 226.0;
pub const WIDTH_EXT: f32 = 489.0;
const LABEL: Color32 = Color32::from_rgb(0x86, 0x86, 0x86);
const FRAME: u8 = 0x3e;
/// Color Curve: 1 kHz grid line at l+373, 83.5 px per decade; handle ② sits
/// 5.5 px left of its frequency line (Live) and ① 119 px left of ②.
const C_X1K: f32 = 373.0;
const C_DEC: f32 = 83.5;
const C_MID: f32 = 75.0;
const C_PX_DB: f32 = 2.0;
const H_RANGE: f32 = 26.0;

fn set(cx: &mut Ctx, f: impl FnOnce(&mut SaturatorParams)) {
    if let FxParams::Saturator(p) = &mut cx.params {
        f(p);
    }
}

fn r(l: f32, t: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
    Rect::from_min_max(pos2(l + x0, t + y0), pos2(l + x1, t + y1))
}

fn hz(f: f32) -> String {
    if f >= 999.5 { format!("{} kHz", sig3(f / 1000.0)) } else { format!("{f:.0} Hz") }
}

/// Title-bar button: ▼ opens the extended view, ▶ closes it.
pub(super) fn title_button(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32) {
    let FxParams::Saturator(s) = cx.params else { return };
    let c = pos2(l + 27.5, t + 8.5);
    p.circle_filled(c, 6.5, gray(0x11));
    p.circle_filled(c, 5.5, gray(0x1e));
    let pts = if s.expanded {
        vec![pos2(l + 23.5, t + 5.0), pos2(l + 30.5, t + 8.5), pos2(l + 23.5, t + 12.0)]
    } else {
        vec![pos2(l + 23.0, t + 6.0), pos2(l + 30.0, t + 6.0), pos2(l + 26.5, t + 12.0)]
    };
    p.add(Shape::convex_polygon(pts, text(), Stroke::NONE));
    if cx.toggle(ui, Rect::from_center_size(c, vec2(13.0, 13.0)), sap::EXPAND, true) {
        set(cx, |q| q.expanded = !q.expanded);
    }
}

fn frame(p: &Painter, x0: f32, y0: f32, x1: f32, y1: f32) {
    let c = gray(FRAME);
    quad(p, Rect::from_min_max(pos2(x0, y0), pos2(x1 + 1.0, y0 + 1.0)), c);
    quad(p, Rect::from_min_max(pos2(x0, y1), pos2(x1 + 1.0, y1 + 1.0)), c);
    quad(p, Rect::from_min_max(pos2(x0, y0), pos2(x0 + 1.0, y1 + 1.0)), c);
    quad(p, Rect::from_min_max(pos2(x1, y0), pos2(x1 + 1.0, y1 + 1.0)), c);
}

pub(super) fn draw(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, rect: Rect) {
    let FxParams::Saturator(s) = cx.params else { return };
    let (l, t) = (rect.left(), rect.top());
    let on = cx.on;
    let teal = if on { TEAL } else { gray(0x75) };
    p.rect_filled(r(l, t, 4.0, 21.0, 223.0, 130.0), CornerRadius::same(2), gray(0x18));

    // Curve chooser (teal bar).
    let cr = r(l, t, 8.0, 25.0, 113.0, 38.0);
    p.rect_filled(cr, CornerRadius::ZERO, teal);
    let dark = gray(0x18);
    text_at(p, l + 11.0, t + 35.0, Align2::LEFT_BOTTOM, SAT_CURVES[(s.curve as usize).min(7)], font(FS), dark);
    p.add(Shape::convex_polygon(vec![pos2(l + 102.0, t + 29.0), pos2(l + 109.0, t + 29.0), pos2(l + 105.5, t + 34.0)], dark, Stroke::NONE));
    let resp = cx.sense(ui, cr, sap::CURVE, Sense::click());
    cx.param_menu(&resp, sap::CURVE);
    if resp.clicked() {
        cx.focus(sap::CURVE);
    }
    let mut pick = None;
    menu::dropdown_aligned(&resp, 120.0, egui::RectAlign::BOTTOM_START, |ui| {
        for (i, it) in SAT_CURVES.iter().enumerate() {
            if menu::check_item(ui, it, "", i == s.curve as usize, true) {
                pick = Some(i as u8);
            }
        }
    });
    if let Some(i) = pick {
        cx.undo_step();
        set(cx, |q| q.curve = i);
    }
    if s.curve == 2 {
        text_at(p, l + 122.0, t + 35.0, Align2::LEFT_BOTTOM, "Threshold", font(FS), LABEL);
        drag_text(cx, ui, p, l + 172.0, t + 35.0, sap::BASS, &format!("{:.1} dB", s.bass), true, TEAL);
    }

    // Transfer curve.
    frame(p, l + 8.0, t + 42.0, l + 218.0, t + 108.0);
    quad(p, r(l, t, 8.0, 75.0, 219.0, 76.0), gray(FRAME));
    quad(p, r(l, t, 113.0, 42.0, 114.0, 109.0), gray(FRAME));
    let gp = p.with_clip_rect(r(l, t, 9.0, 42.0, 218.0, 108.0).intersect(p.clip_rect()));
    let pts: Vec<Pos2> = (0..=208)
        .map(|i| {
            let x = 9.0 + i as f32;
            let y = s.display((x + 0.5 - 113.5) / 104.5);
            pos2(l + x + 0.5, t + 75.75 - y.clamp(-1.2, 1.2) * 33.0)
        })
        .collect();
    gp.add(Shape::line(pts, Stroke::new(1.5, teal)));

    // Post Clip, Color, Amt Lo.
    let clip = text_menu(
        cx,
        ui,
        p,
        r(l, t, 20.0, 113.0, 80.0, 126.0),
        sap::CLIP,
        l + 23.0,
        t + 123.0,
        Some(l + 71.0),
        s.clip as usize,
        &SAT_CLIPS,
        true,
        TEAL,
    );
    if let Some(i) = clip {
        set(cx, |q| q.clip = i as u8);
    }
    if tbutton(cx, ui, p, r(l, t, 93.0, 113.0, 134.0, 126.0), sap::COLOR, "Color", s.color, true) {
        set(cx, |q| q.color = !q.color);
    }
    text_at(p, l + 147.0, t + 123.0, Align2::LEFT_BOTTOM, "Amt Lo", font(FS), LABEL);
    drag_text(cx, ui, p, l + 185.0, t + 123.0, sap::AMT_LO, &pct(s.amt_lo), s.color, ORANGE);

    knob_lv(cx, ui, p, l + 46.0, t + 144.0, sap::DRIVE, "Drive", &format!("{:.1} dB", s.drive), 0.5, KN, 15.0, 36.0, true);
    knob_lv(cx, ui, p, l + 113.5, t + 144.0, sap::OUTPUT, "Output", &format!("{:.1} dB", s.output), 0.0, KN, 15.0, 36.0, true);
    knob_lv(cx, ui, p, l + 182.5, t + 144.0, sap::DRY_WET, "Dry/Wet", &pct(s.dry_wet), 0.0, KN, 15.0, 36.0, true);

    if s.expanded {
        extended(cx, ui, p, l, t, &s);
    }
}

fn cx_of(l: f32, f: f32) -> f32 {
    l + C_X1K + C_DEC * (f / 1000.0).log10()
}

fn extended(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, s: &SaturatorParams) {
    let on = cx.on;
    let col_en = s.color;
    let orange = if on && col_en { ORANGE } else { gray(0x75) };
    p.rect_filled(r(l, t, 227.0, 21.0, 486.0, 130.0), CornerRadius::same(2), gray(0x18));
    text_at(p, l + 233.0, t + 35.0, Align2::LEFT_BOTTOM, "Color Curve", font(FS), LABEL);
    quad(p, r(l, t, 418.0, 31.0, 428.0, 33.0), gray(0x5a));
    text_at(p, l + 433.0, t + 35.0, Align2::LEFT_BOTTOM, "In", font(FS), LABEL);
    quad(p, r(l, t, 449.0, 31.0, 459.0, 33.0), gray(0xe0));
    text_at(p, l + 464.0, t + 35.0, Align2::LEFT_BOTTOM, "Out", font(FS), LABEL);
    frame(p, l + 231.0, t + 42.0, l + 481.0, t + 108.0);
    for x in [289.0, 373.0, 456.0] {
        quad(p, r(l, t, x, 42.0, x + 1.0, 109.0), gray(FRAME));
    }
    // Pre-EQ response (the post EQ is its inverse).
    let sr = 48_000.0;
    let gp = p.with_clip_rect(r(l, t, 232.0, 43.0, 481.0, 108.0).intersect(p.clip_rect()));
    let pts: Vec<Pos2> = (0..=249)
        .map(|i| {
            let x = 232.0 + i as f32;
            let f = 1000.0 * 10f32.powf((x + 0.5 - C_X1K) / C_DEC);
            let db = if col_en { s.color_db(f, sr) } else { 0.0 };
            pos2(l + x + 0.5, t + C_MID + 0.5 - db * C_PX_DB)
        })
        .collect();
    gp.add(Shape::line(pts, Stroke::new(2.0, orange)));
    // Handles: ① Amt Lo, ② Frequency / Amt Hi.
    let h2 = pos2(cx_of(l, s.freq.clamp(SAT_FREQ.0, SAT_FREQ.1)) - 5.5, t + C_MID + 0.5 - s.amt_hi * H_RANGE);
    let h1 = pos2(h2.x - 119.0, t + C_MID + 0.5 - s.amt_lo * H_RANGE);
    let dark = gray(0x18);
    for (i, c) in [h1, h2].into_iter().enumerate() {
        gp.circle_filled(c, 6.5, orange);
        if i == 1 {
            gp.circle_stroke(c, 4.5, Stroke::new(1.0, dark));
        }
        text_at(&gp, c.x, c.y + 4.0, Align2::CENTER_BOTTOM, if i == 0 { "1" } else { "2" }, bold(FS_SMALL), dark);
        cx.salt = 10 + i as u32;
        let k = if i == 0 { sap::AMT_LO } else { sap::FREQ };
        let resp = cx.sense(ui, Rect::from_center_size(c, vec2(15.0, 15.0)), k, Sense::click_and_drag());
        cx.salt = 0;
        if resp.drag_started() || resp.clicked() {
            cx.focus(k);
        }
        if resp.dragged() && col_en {
            let d = resp.drag_delta();
            if d != egui::Vec2::ZERO {
                cx.undo_step();
                let dy = -d.y / (2.0 * H_RANGE);
                if i == 0 {
                    let v = norm(&cx.params, sap::AMT_LO).unwrap_or(0.5);
                    set_norm(&mut cx.params, sap::AMT_LO, v + dy);
                } else {
                    let v = norm(&cx.params, sap::AMT_HI).unwrap_or(0.5);
                    set_norm(&mut cx.params, sap::AMT_HI, v + dy);
                    let span = (SAT_FREQ.1 / SAT_FREQ.0).log10() * C_DEC;
                    let f = norm(&cx.params, sap::FREQ).unwrap_or(0.5);
                    set_norm(&mut cx.params, sap::FREQ, f + d.x / span);
                }
            }
        }
        if resp.double_clicked() {
            cx.undo_step();
            if i == 0 {
                reset(&mut cx.params, sap::AMT_LO);
            } else {
                reset(&mut cx.params, sap::AMT_HI);
                reset(&mut cx.params, sap::FREQ);
            }
        }
        cx.param_menu(&resp, k);
    }
    text_at(p, l + 233.0, t + 123.0, Align2::LEFT_BOTTOM, "Amt Hi", font(FS), LABEL);
    drag_text(cx, ui, p, l + 269.0, t + 123.0, sap::AMT_HI, &pct(s.amt_hi), col_en, ORANGE);
    text_at(p, l + 312.0, t + 123.0, Align2::LEFT_BOTTOM, "Frequency", font(FS), LABEL);
    drag_text(cx, ui, p, l + 367.0, t + 123.0, sap::FREQ, &hz(s.freq), col_en, ORANGE);
    text_at(p, l + 419.0, t + 123.0, Align2::LEFT_BOTTOM, "Width", font(FS), LABEL);
    drag_text(cx, ui, p, l + 452.0, t + 123.0, sap::WIDTH, &format!("{:.0} %", s.width * 100.0), col_en, ORANGE);

    // Waveshaper.
    text_at(p, l + 231.0, t + 144.0, Align2::LEFT_BOTTOM, "Waveshaper", bold(FS), text());
    let ws = s.curve == 7;
    let rows = [
        [("Drive", sap::WS_DRIVE, s.ws_drive), ("Curve", sap::WS_CURVE, s.ws_curve), ("Depth", sap::WS_DEPTH, s.ws_depth)],
        [("Linear", sap::WS_LIN, s.ws_lin), ("Damp", sap::WS_DAMP, s.ws_damp), ("Period", sap::WS_PERIOD, s.ws_period)],
    ];
    for (ri, row) in rows.iter().enumerate() {
        let base = 162.0 + 18.0 * ri as f32;
        for (ci, &(name, k, v)) in row.iter().enumerate() {
            let lx = [232.0, 321.0, 411.0][ci];
            let bx = [264.0, 354.0, 444.0][ci];
            text_at(p, l + lx, t + base, Align2::LEFT_BOTTOM, name, font(FS), text());
            value_box_ex(cx, ui, p, r(l, t, bx, base - 10.0, bx + 41.0, base + 3.0), k, &pct(v), true, ws, panel());
        }
    }
}

// ---- parameters --------------------------------------------------------------

pub(super) fn reset(p: &mut FxParams, k: u8) {
    let d = FxParams::Saturator(SaturatorParams::default());
    if let Some(v) = norm(&d, k) {
        set_norm(p, k, v);
    }
}

pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    let FxParams::Saturator(s) = p else { return };
    match k {
        sap::DRIVE => s.drive = ((s.drive * 10.0).round() + dir * 10.0).clamp(SAT_DRIVE.0 * 10.0, SAT_DRIVE.1 * 10.0) / 10.0,
        sap::OUTPUT => s.output = ((s.output * 10.0).round() + dir * 10.0).clamp(SAT_OUT.0 * 10.0, SAT_OUT.1 * 10.0) / 10.0,
        sap::BASS => s.bass = (s.bass.round() + dir).clamp(SAT_BASS.0, SAT_BASS.1),
        sap::FREQ => {
            if let Some(v) = norm(p, k) {
                set_norm(p, k, v + dir / 200.0);
            }
        }
        _ => {
            if let Some(v) = norm(p, k) {
                let n = if matches!(k, sap::AMT_LO | sap::AMT_HI) { 200.0 } else { 100.0 };
                set_norm(p, k, ((v * n).round() + dir) / n);
            }
        }
    }
}

pub(super) fn name(k: u8) -> &'static str {
    match k {
        sap::DRIVE => "Drive",
        sap::OUTPUT => "Output",
        sap::DRY_WET => "Dry/Wet",
        sap::AMT_LO => "Color Amt Lo",
        sap::AMT_HI => "Color Amt Hi",
        sap::FREQ => "Color Frequency",
        sap::WIDTH => "Color Width",
        sap::BASS => "Bass Shaper Threshold",
        sap::WS_DRIVE => "WS Drive",
        sap::WS_CURVE => "WS Curve",
        sap::WS_DEPTH => "WS Depth",
        sap::WS_LIN => "WS Linear",
        sap::WS_DAMP => "WS Damp",
        sap::WS_PERIOD => "WS Period",
        sap::CURVE => "Type",
        sap::CLIP => "Post Clip",
        sap::COLOR => "Color",
        _ => "",
    }
}
