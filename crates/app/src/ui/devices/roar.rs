//! Roar (Live 12): Sidechain strip / column (External SC, MIDI > FB Note),
//! Input (Drive, Tone, Colour), routing display, stage tabs in the title bar
//! with Amount / Bias / Frequency / Pre and the shaper + filter displays,
//! Modulation strip / panel (Mod Sources: LFO 1, LFO 2, Env, Noise; Matrix),
//! Feedback and Global sections, and the expanded view (all stages + the
//! modulation matrix above the device). Geometry measured from Live 12 at
//! 100 % (l/t = device left/top; text positions are baselines).

use super::dynamics::{KN, KN_BIG, Kn, knob_lv, knob12, sig3, tbutton};
use super::fx4::*;
use super::modfx::{drag_text, text_menu};
use super::shifter::ct;
use super::*;
use daw_engine::modfx::SYNC_DIVS;
use daw_project::devices::params6::rop;
use daw_project::devices::roar::*;
use std::f32::consts::{PI, TAU};

/// Sidechain column and Mod panel widths.
pub const SC_W: f32 = 98.0;
pub const MOD_W: f32 = 274.0;
const PINK: Color32 = Color32::from_rgb(0xff, 0x69, 0x7f);
const GREEN: Color32 = Color32::from_rgb(0x00, 0xd3, 0x8d);
const KN_S: Kn = Kn { r: 9.0, w: 2.0, span: 150.0 * PI / 180.0 };
const LAB: Color32 = Color32::from_rgb(0x86, 0x86, 0x86);
/// Matrix target names (continuous params 0..35).
const TARGETS: [&str; MT] = [
    "Drive",
    "Tone Amt",
    "Tone Freq",
    "Blend",
    "Low XO",
    "High XO",
    "S1 Amount",
    "S1 Bias",
    "S1 Freq",
    "S1 Res",
    "S1 Level",
    "S1 Peak",
    "S1 Morph",
    "S2 Amount",
    "S2 Bias",
    "S2 Freq",
    "S2 Res",
    "S2 Level",
    "S2 Peak",
    "S2 Morph",
    "S3 Amount",
    "S3 Bias",
    "S3 Freq",
    "S3 Res",
    "S3 Level",
    "S3 Peak",
    "S3 Morph",
    "FB Amount",
    "FB Time",
    "FB Freq",
    "Compress",
    "Dry/Wet",
    "FB Width",
    "Output",
    "LFO 1 Rate",
    "LFO 2 Rate",
];

pub fn width(d: &RoarParams) -> f32 {
    581.0 + if d.sc_open { SC_W } else { 0.0 } + if d.mod_open { MOD_W } else { 0.0 }
}

fn set(cx: &mut Ctx, f: impl FnOnce(&mut RoarParams)) {
    if let FxParams::Roar(d) = &mut cx.params {
        f(d);
        cx.undo_step();
    }
}

fn st(cx: &Ctx, c: Color32) -> Color32 {
    if cx.on { c } else { gray(0x75) }
}

/// Stage colour (Low / Stage 1 orange, Mid / Stage 2 teal, High pink).
fn stage_col(cx: &Ctx, s: usize) -> Color32 {
    st(cx, [ORANGE, TEAL, PINK][s.min(2)])
}

fn src_col(cx: &Ctx, s: usize) -> Color32 {
    st(cx, [ORANGE, ORANGE, TEAL, GREEN][s.min(3)])
}

fn db(v: f32) -> String {
    format!("{v:.1} dB")
}

fn rate_text(mode: u8, rate: f32, div: u8) -> String {
    let d = SYNC_DIVS[(div as usize).min(SYNC_DIVS.len() - 1)].1;
    match mode {
        0 => format!("{} Hz", sig3(rate)),
        2 => format!("{d}T"),
        3 => format!("{d}D"),
        4 => format!("{}", div as usize + 1),
        _ => d.to_string(),
    }
}

/// Knob ring in an arbitrary accent colour (stage knobs).
fn knob_c(p: &Painter, c: Pos2, v: f32, origin: f32, kn: Kn, col: Color32, en: bool) {
    let (track, hi, needle) = if en { (gray(0x11), col, gray(0x18)) } else { (gray(0x2a), gray(0x75), gray(0x2a)) };
    let ang = |x: f32| -kn.span + 2.0 * kn.span * x.clamp(0.0, 1.0);
    let gap = 0.2 * 11.9 / kn.r;
    let (a, o) = (ang(v), ang(origin));
    let w = Stroke::new(kn.w, track);
    if a - gap > ang(0.0) {
        arc(p, c, kn.r, ang(0.0), a - gap, w);
    }
    if a + gap < ang(1.0) {
        arc(p, c, kn.r, a + gap, ang(1.0), w);
    }
    let hs = Stroke::new(kn.w, hi);
    if a - gap > o {
        arc(p, c, kn.r, o, a - gap, hs);
    } else if a + gap < o {
        arc(p, c, kn.r, a + gap, o, hs);
    }
    p.add(Shape::line(vec![c, c + vec2(a.sin(), -a.cos()) * (kn.r + 0.6)], Stroke::new(2.1, needle)));
}

/// Coloured knob with label / value (stage section).
#[allow(clippy::too_many_arguments)]
fn knob_col(
    cx: &mut Ctx,
    ui: &mut egui::Ui,
    p: &Painter,
    x: f32,
    base: f32,
    k: u8,
    label: &str,
    value: &str,
    origin: f32,
    kn: Kn,
    dy: f32,
    vy: f32,
    col: Color32,
) {
    let half = (kn.r + 10.0).max(22.5);
    let area = Rect::from_min_max(pos2(x - half, base - 9.0), pos2(x + half, base + vy + 2.0));
    cx.drag(ui, area, k, true);
    let v = norm(&cx.params, k).unwrap_or(0.0);
    text_at(p, x, base, Align2::CENTER_BOTTOM, label, font(FS), text());
    knob_c(p, pos2(x, base + dy), v, origin, kn, col, cx.on);
    text_at(p, x, base + vy, Align2::CENTER_BOTTOM, value, font(FS), text());
    if cx.focused(k) {
        brackets(p, area);
    }
}

/// Grey label over a coloured draggable value (display rows).
#[allow(clippy::too_many_arguments)]
fn dparam(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, x: f32, base: f32, k: u8, label: &str, v: &str, col: Color32) {
    text_at(p, x, base, Align2::LEFT_BOTTOM, label, font(FS), LAB);
    drag_text(cx, ui, p, x, base + 13.5, k, v, true, col);
}

/// 8×8 display checkbox in `col` (filled when on).
#[allow(clippy::too_many_arguments)]
fn dchk(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, x: f32, y: f32, k: u8, on: bool, col: Color32) -> bool {
    let b = Rect::from_min_size(pos2(x, y), vec2(8.0, 8.0));
    if on {
        p.rect_filled(b, CornerRadius::ZERO, col);
    } else {
        p.rect_stroke(b, CornerRadius::ZERO, Stroke::new(1.0, col), egui::StrokeKind::Inside);
    }
    cx.toggle(ui, b.expand(2.0), k, true)
}

/// Headphones icon (listen) in a 13 px circle.
fn phones(p: &Painter, c: Pos2, on: bool, col: Color32) {
    p.circle_filled(c, 6.5, if on { col } else { gray(0x12) });
    let ic = if on { gray(0x12) } else { gray(0xb5) };
    p.add(Shape::line(
        (0..=8)
            .map(|i| {
                let a = PI + PI * i as f32 / 8.0;
                pos2(c.x + 3.5 * a.cos(), c.y + 1.0 + 3.5 * a.sin())
            })
            .collect(),
        Stroke::new(1.2, ic),
    ));
    for dx in [-3.5, 2.5] {
        p.rect_filled(Rect::from_min_size(pos2(c.x + dx, c.y + 0.5), vec2(1.2, 3.0)), CornerRadius::ZERO, ic);
    }
}

/// Title-bar expand button (circle, arrow up to a bar) after the activator.
pub(super) fn title_button(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32) {
    let FxParams::Roar(d) = cx.params else { return };
    let c = pos2(l + 23.0, t + 8.5);
    p.circle_filled(c, 6.5, if d.expanded { st(cx, ORANGE) } else { gray(0x12) });
    let ic = if d.expanded { gray(0x12) } else { gray(0xb5) };
    p.line_segment([pos2(c.x - 3.0, c.y + 3.5), pos2(c.x + 3.0, c.y + 3.5)], Stroke::new(1.2, ic));
    p.line_segment([pos2(c.x, c.y - 1.0), pos2(c.x, c.y + 2.5)], Stroke::new(1.2, ic));
    p.add(Shape::convex_polygon(vec![pos2(c.x - 3.0, c.y), pos2(c.x, c.y - 3.5), pos2(c.x + 3.0, c.y)], ic, Stroke::NONE));
    if cx.toggle(ui, Rect::from_center_size(c, vec2(13.0, 13.0)), rop::EXPAND, true) {
        set(cx, |q| q.expanded = !q.expanded);
    }
}

pub(super) fn draw(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect) {
    let FxParams::Roar(d) = cx.params else { return };
    let (l0, t) = (r.left(), r.top());
    let tap = dev_tap(cx.dev.id);
    if cx.on && (d.mod_active() || d.compress > 0.0) {
        cx.app.animating = true;
    }
    strip(cx, ui, p, l0, t, &d);
    if d.sc_open {
        sc_column(cx, ui, p, l0, t, &d);
    }
    let l = l0 + if d.sc_open { SC_W } else { 0.0 };
    let gap = |x: f32, y0: f32, y1: f32| px(p, l, t, x, y0, x + 2.0, y1, gray(0x46));
    gap(145.0, 17.0, 188.0);
    gap(426.0, 17.0, 188.0);
    gap(447.0, 17.0, 188.0);
    px(p, l, t, 21.0, 89.0, 426.0, 91.0, gray(0x46));

    // Input: Drive, Tone (+ Colour, freq).
    knob_lv(cx, ui, p, l + 50.0, t + 31.0, rop::DRIVE, "Drive", &db(d.drive), 0.0, KN_BIG, 23.0, 49.0, true);
    p.circle_filled(pos2(l + 70.5, t + 41.0), 2.0, gray(0x1e));
    ct(p, l + 95.0, t + 31.0, "Tone", text());
    cx.drag(ui, rr(l, t, 80.0, 22.0, 118.0, 62.0), rop::TONE_AMT, true);
    knob12(p, pos2(l + 95.0, t + 48.0), norm(&cx.params, rop::TONE_AMT).unwrap_or(0.5), 0.5, KN_S, cx.on, cx.modv(rop::TONE_AMT));
    p.add(Shape::convex_polygon(
        vec![pos2(l + 84.0, t + 33.0), pos2(l + 89.5, t + 33.0), pos2(l + 84.0, t + 38.5)],
        marker_col(cx, true, d.tone_amt.abs() > 0.005),
        Stroke::NONE,
    ));
    text_at(p, l + 114.0, t + 60.0, Align2::CENTER_BOTTOM, &format!("{:.1} %", d.tone_amt * 100.0), font(FS), text());
    let cb = rr(l, t, 120.0, 27.0, 138.0, 38.0);
    boxed(p, cb, if d.color { st(cx, TEAL) } else { box_fill(cx.on) });
    let ic = if d.color { gray(0x12) } else { text() };
    p.add(Shape::line(
        vec![
            pos2(cb.left() + 3.0, cb.top() + 3.5),
            pos2(cb.left() + 8.0, cb.top() + 3.5),
            pos2(cb.left() + 12.0, cb.bottom() - 3.0),
            pos2(cb.right() - 3.0, cb.bottom() - 3.0),
        ],
        Stroke::new(1.2, ic),
    ));
    if cx.toggle(ui, cb, rop::COLOR, true) {
        set(cx, |q| q.color = !q.color);
    }
    value_box(cx, ui, p, rr(l, t, 83.0, 71.0, 138.0, 84.0), rop::TONE_FREQ, &hz(d.tone_freq), true);

    // Routing display.
    let rd = rr(l, t, 21.0, 91.0, 145.0, 186.0);
    p.rect_filled(rd, CornerRadius::ZERO, gray(0x18));
    px(p, l, t, 116.0, 96.0, 117.0, 150.0, gray(0x3e));
    routing_icon(cx, p, l + 35.0, t + 105.0, &d);
    text_at(p, l + 29.0, t + 128.0, Align2::LEFT_BOTTOM, "Routing", font(FS), LAB);
    let routs: Vec<&str> = RO_ROUTINGS.to_vec();
    if let Some(i) = text_menu(
        cx,
        ui,
        p,
        rr(l, t, 26.0, 131.0, 112.0, 143.0),
        rop::ROUTING,
        l + 29.0,
        t + 141.5,
        None,
        d.routing as usize,
        &routs,
        true,
        ORANGE,
    ) {
        set(cx, |q| {
            q.routing = i as u8;
            q.stage_tab = q.stage_tab.min(q.n_stages() as u8 - 1);
        });
    }
    // Stage level meters (H / M / L) — stage output activity.
    let names = d.stage_names();
    for (i, s) in (0..d.n_stages()).rev().enumerate() {
        let y = 98.0 + 17.0 * i as f32;
        let lab = names[s].chars().next().unwrap_or(' ').to_string();
        text_at(p, l + 124.0, t + y + 11.0, Align2::CENTER_BOTTOM, &lab, font(FS), dim());
        for x in [132.0, 135.0] {
            p.rect_stroke(rr(l, t, x, y, x + 2.0, y + 12.0), CornerRadius::ZERO, Stroke::new(1.0, gray(0x3e)), egui::StrokeKind::Inside);
        }
    }
    if d.routing == 3 {
        dparam(cx, ui, p, l + 29.0, t + 165.0, rop::XO_LO, "Low", &hz(d.xo_lo), stage_col(cx, 0));
        dparam(cx, ui, p, l + 77.0, t + 165.0, rop::XO_HI, "High", &hz(d.xo_hi), stage_col(cx, 2));
    } else if d.has_blend() {
        let lab = if d.routing == 5 { "Feedback" } else { "Blend" };
        dparam(cx, ui, p, l + 29.0, t + 165.0, rop::BLEND, lab, &format!("{:.0} %", d.blend * 100.0), ORANGE);
    }

    // Stage tabs (title bar) + the selected stage.
    let n = d.n_stages();
    let tw = (279.0 - 2.0 * (n as f32 - 1.0)) / n as f32;
    for (s, name) in names.iter().enumerate().take(n) {
        let x0 = 147.0 + s as f32 * (tw + 2.0);
        let tr = rr(l, t, x0, 2.0, x0 + tw, 17.0);
        // Selected tab joins the panel; others stop above a #46 seam.
        let fill = if d.stage_tab as usize == s { tr } else { Rect::from_min_max(tr.min, pos2(tr.right(), t + 15.0)) };
        p.rect_filled(fill, CornerRadius { nw: 2, ne: 2, sw: 0, se: 0 }, gray(0x36));
        let lab_w = p.layout_no_wrap(name.to_string(), font(FS), text()).size().x;
        let sx = (tr.center().x - (lab_w + 13.0) / 2.0).round();
        let sq = Rect::from_min_size(pos2(sx, t + 4.0), vec2(9.0, 9.0));
        p.rect_filled(sq, CornerRadius::ZERO, gray(0x11));
        p.rect_filled(sq.shrink(1.0), CornerRadius::ZERO, if d.stages[s].on { stage_col(cx, s) } else { gray(0x1e) });
        text_at(p, sx + 13.0, t + 13.0, Align2::LEFT_BOTTOM, name, font(FS), text());
        cx.salt = s as u32;
        if cx.toggle(ui, sq.expand(2.0), rop::STAGE_ON + s as u8, true) {
            set(cx, |q| q.stages[s].on = !q.stages[s].on);
        }
        if cx.toggle(ui, tr, rop::STAGE_TAB, true) {
            set(cx, |q| q.stage_tab = s as u8);
        }
        cx.salt = 0;
        if d.stage_tab as usize == s && cx.focused(rop::STAGE_TAB) {
            brackets(p, tr.shrink2(vec2(2.0, 0.0)));
        }
    }
    let s = (d.stage_tab as usize).min(n - 1);
    stage_panel(cx, ui, p, l + 147.0, t, s, &d, 0);

    // Modulation strip (fold + source LEDs) and panel.
    let mx = l + 428.0;
    let fc = pos2(mx + 9.5, t + 27.5);
    p.circle_filled(fc, 6.5, gray(0x12));
    p.circle_filled(fc, 5.5, gray(0x1c));
    let tri = if d.mod_open {
        vec![pos2(fc.x - 2.5, fc.y - 3.5), pos2(fc.x + 3.5, fc.y), pos2(fc.x - 2.5, fc.y + 3.5)]
    } else {
        vec![pos2(fc.x - 3.5, fc.y - 2.5), pos2(fc.x + 3.5, fc.y - 2.5), pos2(fc.x, fc.y + 3.5)]
    };
    p.add(Shape::convex_polygon(tri, gray(0xb5), Stroke::NONE));
    if cx.toggle(ui, Rect::from_center_size(fc, vec2(13.0, 13.0)), rop::MOD_OPEN, true) {
        set(cx, |q| q.mod_open = !q.mod_open);
    }
    #[allow(clippy::needless_range_loop)]
    for i in 0..4 {
        let lit = d.source_used(i) && cx.on;
        let c = if lit {
            let v = tap.get(TAP_SRC[i]).abs().clamp(0.0, 1.0);
            mix(gray(0x2a), src_col(cx, i), 0.4 + 0.6 * v)
        } else {
            gray(0x2a)
        };
        p.circle_filled(pos2(mx + 9.5, t + 91.0 + 6.67 * i as f32), 3.0, c);
    }
    vtext(p, mx + 13.0, t + 180.0, "Modulation");
    let b = l + if d.mod_open { MOD_W } else { 0.0 };
    if d.mod_open {
        mod_panel(cx, ui, p, l + 449.0, t, &d);
        px(p, b, t, 447.0, 17.0, 449.0, 188.0, gray(0x46));
    }
    px(p, b, t, 520.0, 17.0, 522.0, 188.0, gray(0x46));

    // Feedback.
    ct(p, b + 484.0, t + 31.0, "FB Mode", text());
    let modes: Vec<&str> = RO_FB_MODES.to_vec();
    if let Some(i) =
        chooser(cx, ui, p, rr(b, t, 454.0, 35.0, 515.0, 48.0), rop::FB_MODE, RO_FB_MODES[(d.fb_mode as usize).min(4)], &modes, true)
    {
        set(cx, |q| q.fb_mode = i as u8);
    }
    let tb = rr(b, t, 454.0, 51.0, 515.0, 64.0);
    match d.fb_mode {
        0 => value_box(cx, ui, p, tb, rop::FB_TIME, &format!("{} ms", sig3(d.fb_ms)), true),
        4 => value_box(cx, ui, p, tb, rop::FB_NOTE, &note_name(d.fb_note), true),
        m => value_box(cx, ui, p, tb, rop::FB_DIV, &rate_text(m, 0.0, d.fb_div), true),
    }
    knob_lv(cx, ui, p, b + 484.0, t + 76.5, rop::FB_AMT, "Amount", &format!("{:.1} %", d.fb_amt * 100.0), 0.0, KN, 15.5, 37.0, true);
    let inv = rr(b, t, 470.0, 119.0, 484.0, 131.0);
    boxed(p, inv, if d.fb_invert { st(cx, ORANGE) } else { box_fill(cx.on) });
    ct(p, inv.center().x, inv.bottom() - 2.0, "Ø", if d.fb_invert { gray(0x20) } else { text() });
    if cx.toggle(ui, inv, rop::FB_INV, true) {
        set(cx, |q| q.fb_invert = !q.fb_invert);
    }
    let gb = rr(b, t, 487.0, 119.0, 501.0, 131.0);
    boxed(p, gb, if d.fb_gate { st(cx, ORANGE) } else { box_fill(cx.on) });
    let gc = if d.fb_gate { gray(0x20) } else { text() };
    for (i, h) in [8.0, 6.0, 3.0, 1.5].iter().enumerate() {
        let x = gb.left() + 3.5 + 2.0 * i as f32 + if i > 0 { 1.0 } else { 0.0 };
        px(p, 0.0, 0.0, x, gb.bottom() - 2.0 - h, x + 1.0, gb.bottom() - 2.0, gc);
    }
    if cx.toggle(ui, gb, rop::FB_GATE, true) {
        set(cx, |q| q.fb_gate = !q.fb_gate);
    }
    ct(p, b + 484.0, t + 149.0, "Freq|Width", text());
    value_box(cx, ui, p, rr(b, t, 454.0, 152.0, 515.0, 165.0), rop::FB_FREQ, &hz(d.fb_freq), true);
    value_box(cx, ui, p, rr(b, t, 454.0, 168.0, 515.0, 181.0), rop::FB_WIDTH, &format!("{:.2}", d.fb_width), true);

    // Global.
    knob_lv(cx, ui, p, b + 550.0, t + 31.0, rop::COMPRESS, "Compress", &format!("{:.0} %", d.compress * 100.0), 0.0, KN, 15.0, 35.5, true);
    let gr = tap.get(TAP_GR).abs();
    let _ = gr;
    p.circle_filled(pos2(b + 569.0, t + 38.0), 2.0, gray(0x1e));
    if tbutton(cx, ui, p, rr(b, t, 527.0, 73.0, 575.0, 85.0), rop::SC_HPF, "SC HPF", d.sc_hpf, true) {
        set(cx, |q| q.sc_hpf = !q.sc_hpf);
    }
    knob_lv(cx, ui, p, b + 550.0, t + 107.0, rop::OUTPUT, "Output", &db(d.output), 0.0, KN, 16.0, 34.5, true);
    p.circle_filled(pos2(b + 570.5, t + 110.0), 2.0, gray(0x1e));
    ct(p, b + 551.0, t + 165.0, "Dry/Wet", text());
    value_box(cx, ui, p, rr(b, t, 527.0, 168.0, 575.0, 181.0), rop::DRY_WET, &format!("{:.0} %", d.dry_wet * 100.0), true);

    if d.expanded {
        expanded(cx, ui, r, &d);
    }
}

fn note_name(n: f32) -> String {
    let n = n.round() as i32;
    format!("{}{}", daw_project::clipx::NOTE_NAMES[n.rem_euclid(12) as usize], n.div_euclid(12) - 2)
}

/// Vertical caption reading upwards, baseline column `x`, ending at `y`.
fn vtext(p: &Painter, x: f32, y: f32, s: &str) {
    let gal = p.layout_no_wrap(s.into(), font(FS), text());
    let asc = gal.rows.first().map_or(8.0, |r| r.row.glyphs.first().map_or(8.0, |gl| gl.pos.y));
    p.add(egui::epaint::TextShape::new(pos2(x - asc, y), gal, text()).with_angle(-PI / 2.0));
}

/// Routing diagram: one coloured square per stage (stacked = parallel,
/// chained = serial, loop = feedback / delay).
fn routing_icon(cx: &Ctx, p: &Painter, x: f32, y: f32, d: &RoarParams) {
    let line = gray(0x86);
    let sq = |c: Pos2, s: usize| {
        p.rect_filled(Rect::from_center_size(c, vec2(7.0, 7.0)), CornerRadius::ZERO, stage_col(cx, s));
    };
    let s = Stroke::new(1.0, line);
    match d.routing {
        0 => {
            p.line_segment([pos2(x, y - 9.0), pos2(x, y + 9.0)], s);
            sq(pos2(x, y), 0);
        }
        1 | 6 => {
            p.line_segment([pos2(x, y - 11.0), pos2(x, y + 11.0)], s);
            sq(pos2(x, y - 4.5), 0);
            sq(pos2(x, y + 4.5), 1);
        }
        3 => {
            p.line_segment([pos2(x, y - 13.0), pos2(x, y + 13.0)], s);
            p.line_segment([pos2(x - 8.0, y - 8.0), pos2(x + 8.0, y - 8.0)], s);
            p.line_segment([pos2(x - 8.0, y + 8.0), pos2(x + 8.0, y + 8.0)], s);
            p.line_segment([pos2(x - 8.0, y - 8.0), pos2(x - 8.0, y + 8.0)], s);
            p.line_segment([pos2(x + 8.0, y - 8.0), pos2(x + 8.0, y + 8.0)], s);
            sq(pos2(x, y - 6.5), 2);
            sq(pos2(x, y), 1);
            sq(pos2(x, y + 6.5), 0);
        }
        5 => {
            p.line_segment([pos2(x - 5.0, y - 11.0), pos2(x - 5.0, y + 11.0)], s);
            p.rect_stroke(
                Rect::from_min_max(pos2(x - 5.0, y - 6.0), pos2(x + 6.0, y + 6.0)),
                CornerRadius::ZERO,
                s,
                egui::StrokeKind::Middle,
            );
            sq(pos2(x - 5.0, y - 2.0), 0);
            sq(pos2(x + 6.0, y + 2.0), 1);
        }
        _ => {
            p.line_segment([pos2(x, y - 11.0), pos2(x, y - 6.0)], s);
            p.line_segment([pos2(x, y + 6.0), pos2(x, y + 11.0)], s);
            p.rect_stroke(
                Rect::from_min_max(pos2(x - 6.0, y - 6.0), pos2(x + 6.0, y + 6.0)),
                CornerRadius::ZERO,
                s,
                egui::StrokeKind::Middle,
            );
            sq(pos2(x - 6.0, y), 0);
            sq(pos2(x + 6.0, y), 1);
        }
    }
}

/// One stage: Amount / Bias / Frequency / Pre (y 17..88) above the shaper and
/// filter displays (y 91..185), 279 px wide from `x`.
#[allow(clippy::too_many_arguments)]
fn stage_panel(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, x: f32, t: f32, s: usize, d: &RoarParams, salt: u32) {
    let g = &d.stages[s];
    let col = stage_col(cx, s);
    let k = |o: u8| rop::stage(s, o);
    cx.salt = salt;
    knob_col(
        cx,
        ui,
        p,
        x + 60.0,
        t + 31.0,
        k(rop::S_AMOUNT),
        "Amount",
        &format!("{:.1} %", g.amount * 100.0),
        0.0,
        KN_BIG,
        23.0,
        49.0,
        col,
    );
    knob_col(cx, ui, p, x + 138.0, t + 38.0, k(rop::S_BIAS), "Bias", &format!("{:.2}", g.bias), 0.0, KN, 15.0, 36.0, col);
    knob_col(cx, ui, p, x + 217.0, t + 31.0, k(rop::S_FREQ), "Frequency", &hz(g.freq), 0.0, KN_BIG, 23.0, 49.0, col);
    px(p, x, t, 238.0, 52.0, 245.0, 53.0, gray(0x4a));
    let pr = rr(x, t, 251.0, 46.0, 273.0, 59.0);
    let pre_on = g.pre;
    boxed(p, pr, if pre_on { col } else { gray(0x1e) });
    ct(p, pr.center().x, pr.bottom() - 2.5, "Pre", if pre_on { gray(0x20) } else { text() });
    if cx.toggle(ui, pr, rop::PRE + s as u8, true) {
        set(cx, |q| q.stages[s].pre = !q.stages[s].pre);
    }

    // Shaper display.
    let sd = rr(x, t, 0.0, 91.0, 138.0, 186.0);
    p.rect_filled(sd, CornerRadius::ZERO, gray(0x18));
    let gr = rr(x, t, 5.0, 96.0, 134.0, 151.0);
    p.rect_stroke(gr, CornerRadius::ZERO, Stroke::new(1.0, gray(0x3e)), egui::StrokeKind::Inside);
    px(p, x, t, 6.0, 123.0, 133.0, 124.0, gray(0x3f));
    let bx = gr.center().x + g.bias * (gr.width() / 2.0 - 1.0);
    let mut y = gr.top() + 1.0;
    while y < gr.bottom() - 1.0 {
        p.line_segment([pos2(bx.round() + 0.5, y), pos2(bx.round() + 0.5, (y + 2.0).min(gr.bottom() - 1.0))], Stroke::new(1.0, gray(0xc0)));
        y += 4.0;
    }
    let sc = if g.shaper_on { col } else { gray(0x5a) };
    let gc = p.with_clip_rect(gr.shrink(1.0));
    let pts: Vec<Pos2> = (0..=126)
        .map(|i| {
            let u = (-1.0 + 2.0 * i as f32 / 126.0) * 1.6;
            let v = if g.shaper_on { stage_curve(g, u) } else { u };
            pos2(gr.left() + 1.0 + i as f32, gr.center().y - v.clamp(-1.4, 1.4) * 17.5)
        })
        .collect();
    gc.add(Shape::line(pts, Stroke::new(1.5, sc)));
    if dchk(cx, ui, p, x + 5.0, t + 157.0, rop::SHAPER_ON + s as u8, g.shaper_on, col) {
        set(cx, |q| q.stages[s].shaper_on = !q.stages[s].shaper_on);
    }
    text_at(p, x + 18.0, t + 165.0, Align2::LEFT_BOTTOM, "Shaper", font(FS), LAB);
    let shapers: Vec<&str> = RO_SHAPERS_SHORT.to_vec();
    if let Some(i) = text_menu(
        cx,
        ui,
        p,
        rr(x, t, 4.0, 168.0, 60.0, 181.0),
        rop::SHAPER + s as u8,
        x + 6.0,
        t + 178.5,
        Some(x + 55.0),
        g.shaper as usize,
        &shapers,
        true,
        col,
    ) {
        set(cx, |q| q.stages[s].shaper = i as u8);
    }
    dparam(cx, ui, p, x + 80.0, t + 165.0, k(rop::S_LEVEL), "Level", &db(g.level), col);

    // Filter display.
    let fd = rr(x, t, 140.0, 91.0, 279.0, 186.0);
    p.rect_filled(fd, CornerRadius::ZERO, gray(0x18));
    let fr = rr(x, t, 145.0, 96.0, 274.0, 151.0);
    p.rect_stroke(fr, CornerRadius::ZERO, Stroke::new(1.0, gray(0x3e)), egui::StrokeKind::Inside);
    let fxp = |f: f32| fr.left() + (f / 20.0).log10() * (fr.width() / 3.0);
    for f in [100.0f32, 1000.0, 10_000.0] {
        let xx = fxp(f).round();
        p.line_segment([pos2(xx + 0.5, fr.top() + 1.0), pos2(xx + 0.5, fr.bottom() - 1.0)], Stroke::new(1.0, gray(0x3f)));
    }
    for yy in [100.0, 128.0, 143.0] {
        px(p, x, t, 146.0, yy, 273.0, yy + 1.0, gray(0x2b));
    }
    let fc = if g.filter_on { col } else { gray(0x5a) };
    let fcl = p.with_clip_rect(fr.shrink(1.0));
    let pts: Vec<Pos2> = (0..=127)
        .map(|i| {
            let f = 20.0 * 1000f32.powf(i as f32 / 127.0);
            let m = if g.filter_on { filter_mag(g, f, 48_000.0) } else { 1.0 };
            let dbv = 20.0 * m.max(1e-4).log10();
            pos2(fr.left() + 1.0 + i as f32, fr.top() + 18.0 - dbv * 0.75)
        })
        .collect();
    fcl.add(Shape::line(pts, Stroke::new(1.5, fc)));
    if dchk(cx, ui, p, x + 145.0, t + 157.0, rop::FILTER_ON + s as u8, g.filter_on, col) {
        set(cx, |q| q.stages[s].filter_on = !q.stages[s].filter_on);
    }
    text_at(p, x + 158.0, t + 165.0, Align2::LEFT_BOTTOM, "Filter", font(FS), LAB);
    let filters: Vec<&str> = RO_FILTERS.to_vec();
    if let Some(i) = text_menu(
        cx,
        ui,
        p,
        rr(x, t, 144.0, 168.0, 200.0, 181.0),
        rop::FILTER + s as u8,
        x + 146.0,
        t + 178.5,
        Some(x + 184.0),
        g.filter as usize,
        &filters,
        true,
        col,
    ) {
        set(cx, |q| q.stages[s].filter = i as u8);
    }
    match g.filter {
        4 => {
            dparam(cx, ui, p, x + 205.0, t + 165.0, k(rop::S_RES), "Res", &format!("{:.2}", g.res), col);
            dparam(cx, ui, p, x + 237.0, t + 165.0, k(rop::S_PEAK), "Gain", &db(g.peak), col);
        }
        5 => {
            dparam(cx, ui, p, x + 205.0, t + 165.0, k(rop::S_RES), "Res", &format!("{:.2}", g.res), col);
            dparam(cx, ui, p, x + 237.0, t + 165.0, k(rop::S_MORPH), "Morph", &format!("{:.0} %", g.morph * 100.0), col);
        }
        _ => dparam(cx, ui, p, x + 201.0, t + 165.0, k(rop::S_RES), "Res", &format!("{:.2}", g.res), col),
    }
    cx.salt = 0;
}

/// Mod Sources / Matrix panel (273 px from `x`).
fn mod_panel(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, x: f32, t: f32, d: &RoarParams) {
    // Tabs in the title bar.
    for (i, name) in ["Mod Sources", "Matrix"].iter().enumerate() {
        let x0 = x + 137.0 * i as f32;
        let tr = Rect::from_min_max(pos2(x0, t + 2.0), pos2(x0 + 135.0, t + 17.0));
        let sel = d.matrix_tab == (i == 1);
        // Selected tab joins the #18 panel; the other is a #36 tab above a #46 seam.
        if sel {
            p.rect_filled(tr, CornerRadius { nw: 2, ne: 2, sw: 0, se: 0 }, gray(0x18));
        } else {
            p.rect_filled(Rect::from_min_max(tr.min, pos2(tr.right(), t + 15.0)), CornerRadius { nw: 2, ne: 2, sw: 0, se: 0 }, gray(0x36));
        }
        ct(p, tr.center().x, t + 13.0, name, text());
        cx.salt = i as u32;
        if cx.toggle(ui, tr, rop::MOD_TAB, true) {
            set(cx, |q| q.matrix_tab = i == 1);
        }
    }
    cx.salt = 0;
    p.rect_filled(Rect::from_min_max(pos2(x, t + 17.0), pos2(x + 271.0, t + 186.0)), CornerRadius::ZERO, gray(0x18));
    if d.matrix_tab {
        matrix(cx, ui, p, x, t, d);
        return;
    }
    // Source tabs.
    let names = ["LFO 1", "LFO 2", "Env", "Noise"];
    for (i, n) in names.iter().enumerate() {
        let x0 = x + 68.0 * i as f32;
        let tr = Rect::from_min_max(pos2(x0, t + 19.0), pos2(x0 + 67.0, t + 38.0));
        let sel = d.src_tab as usize == i;
        ct(p, tr.center().x, t + 31.0, n, if sel { src_col(cx, i) } else { text() });
        p.rect_filled(
            Rect::from_min_max(pos2(x0, t + 38.0), pos2(x0 + 68.0, t + 39.0)),
            CornerRadius::ZERO,
            if sel { src_col(cx, i) } else { gray(0x36) },
        );
        cx.salt = i as u32;
        if cx.toggle(ui, tr, rop::SRC_TAB, true) {
            set(cx, |q| q.src_tab = i as u8);
        }
    }
    cx.salt = 0;
    px(p, x, t, 0.0, 148.0, 271.0, 149.0, gray(0x36));
    let tap = dev_tap(cx.dev.id);
    let si = (d.src_tab as usize).min(3);
    let col = src_col(cx, si);
    match si {
        0 | 1 => {
            let lf = &d.lfo[si];
            let (wk, mk, rk, dk, mo, sm) = if si == 0 {
                (rop::L1_WAVE, rop::L1_MODE, rop::L1_RATE, rop::L1_DIV, rop::L1_MORPH, rop::L1_SMOOTH)
            } else {
                (rop::L2_WAVE, rop::L2_MODE, rop::L2_RATE, rop::L2_DIV, rop::L2_MORPH, rop::L2_SMOOTH)
            };
            for w in 0..5u8 {
                let y = t + 49.0 + 19.0 * w as f32;
                let br = Rect::from_min_max(pos2(x + 9.0, y), pos2(x + 38.0, y + 12.0));
                let sel = lf.wave == w;
                if sel {
                    p.rect_filled(br, CornerRadius::ZERO, col);
                }
                let ic = if sel { gray(0x18) } else { col };
                // Hand-drawn wave glyphs (13×7) as in Live.
                let (ox, oy) = (br.left() + 8.0, br.center().y);
                let pts: Vec<Pos2> = match w {
                    0 => (0..=14).map(|i| pos2(ox + i as f32 * 13.0 / 14.0, oy - (i as f32 / 14.0 * TAU).sin() * 3.5)).collect(),
                    1 => vec![pos2(ox, oy + 3.5), pos2(ox + 6.5, oy - 3.5), pos2(ox + 13.0, oy + 3.5)],
                    2 => vec![
                        pos2(ox, oy + 3.5),
                        pos2(ox, oy - 3.5),
                        pos2(ox + 6.5, oy - 3.5),
                        pos2(ox + 6.5, oy + 3.5),
                        pos2(ox + 13.0, oy + 3.5),
                        pos2(ox + 13.0, oy - 3.5),
                    ],
                    3 => vec![pos2(ox, oy + 3.5), pos2(ox + 12.0, oy - 3.5), pos2(ox + 12.0, oy + 3.5)],
                    _ => vec![pos2(ox, oy + 3.5), pos2(ox, oy - 3.5), pos2(ox + 12.0, oy + 3.5)],
                };
                p.add(Shape::line(pts, Stroke::new(1.0, ic)));
                cx.salt = w as u32;
                if cx.toggle(ui, br, wk, true) {
                    set(cx, |q| q.lfo[si].wave = w);
                }
            }
            cx.salt = 0;
            let wr = Rect::from_min_max(pos2(x + 45.0, t + 54.0), pos2(x + 263.0, t + 132.0));
            let pts: Vec<Pos2> = (0..=218)
                .map(|i| {
                    let u = i as f32 / 218.0;
                    pos2(wr.left() + i as f32, wr.center().y - lfo_shape(lf.wave, u, lf.morph) * 35.0)
                })
                .collect();
            p.with_clip_rect(wr.expand(2.0)).add(Shape::line(pts, Stroke::new(1.5, col)));
            let modes: Vec<&str> = RO_RATE_MODES.to_vec();
            text_at(p, x + 6.0, t + 165.0, Align2::LEFT_BOTTOM, "Mode", font(FS), LAB);
            if let Some(i) = text_menu(
                cx,
                ui,
                p,
                rr(x, t, 4.0, 168.0, 66.0, 181.0),
                mk,
                x + 6.0,
                t + 178.5,
                Some(x + 60.0),
                lf.mode as usize,
                &modes,
                true,
                col,
            ) {
                set(cx, |q| {
                    q.lfo[si].mode = i as u8;
                    q.lfo[si].div = q.lfo[si].div.min(if i == 4 { RO_SIXTEENTHS as u8 - 1 } else { SYNC_DIVS.len() as u8 - 1 });
                });
            }
            let rkey = if lf.mode == 0 { rk } else { dk };
            dparam(cx, ui, p, x + 72.0, t + 165.0, rkey, "Rate", &rate_text(lf.mode, lf.rate, lf.div), col);
            dparam(cx, ui, p, x + 122.0, t + 165.0, mo, "Morph", &format!("{:.1} %", lf.morph * 100.0), col);
            dparam(cx, ui, p, x + 173.0, t + 165.0, sm, "Smooth", &format!("{:.1} %", lf.smooth * 100.0), col);
        }
        2 => {
            // Env: level trace + threshold; listen.
            let v = if cx.on { tap.get(TAP_SRC[2]).clamp(0.0, 1.0) } else { 0.0 };
            let y = t + 141.0 - v * 85.0;
            p.rect_filled(Rect::from_min_max(pos2(x + 8.0, y - 1.0), pos2(x + 263.0, y + 1.0)), CornerRadius::ZERO, col);
            let th = (d.env_thresh - RO_THRESH.0) / (RO_THRESH.1 - RO_THRESH.0);
            if th > 0.0 {
                let ty = t + 141.0 - th * 85.0;
                p.line_segment([pos2(x + 8.0, ty), pos2(x + 263.0, ty)], Stroke::new(1.0, gray(0x5a)));
            }
            let hc = pos2(x + 256.5, t + 53.5);
            // Listen: blue outline when off (Live), filled when on.
            let blue = if cx.on { Color32::from_rgb(0x39, 0x64, 0xaa) } else { gray(0x4a) };
            let ic = if d.env_listen {
                p.circle_filled(hc, 7.0, col);
                gray(0x12)
            } else {
                p.circle_stroke(hc, 6.4, Stroke::new(1.2, blue));
                blue
            };
            p.add(Shape::line(
                (0..=10)
                    .map(|i| {
                        let a = PI + PI * i as f32 / 10.0;
                        pos2(hc.x + 3.5 * a.cos(), hc.y + 0.5 + 3.5 * a.sin())
                    })
                    .collect(),
                Stroke::new(1.2, ic),
            ));
            for dx in [-3.6, 2.4] {
                p.rect_filled(Rect::from_min_size(pos2(hc.x + dx, hc.y + 0.5), vec2(1.2, 3.0)), CornerRadius::ZERO, ic);
            }
            if cx.toggle(ui, Rect::from_center_size(hc, vec2(13.0, 13.0)), rop::ENV_LISTEN, true) {
                set(cx, |q| q.env_listen = !q.env_listen);
            }
            text_at(p, x + 7.0, t + 165.0, Align2::LEFT_BOTTOM, "Att.", font(FS), LAB);
            let hb = rr(x, t, 31.0, 156.0, 41.0, 166.0);
            p.rect_filled(hb, CornerRadius::ZERO, if d.env_hold { col } else { gray(0x24) });
            ct(p, hb.center().x, hb.bottom() - 1.0, "H", if d.env_hold { gray(0x18) } else { text() });
            if cx.toggle(ui, hb, rop::ENV_HOLD, true) {
                set(cx, |q| q.env_hold = !q.env_hold);
            }
            drag_text(cx, ui, p, x + 7.0, t + 178.5, rop::ENV_ATT, &format!("{} ms", sig3(d.env_attack)), true, col);
            dparam(cx, ui, p, x + 53.0, t + 165.0, rop::ENV_REL, "Release", &format!("{} ms", sig3(d.env_release)), col);
            let thr = if d.env_thresh <= RO_THRESH.0 + 0.05 { "−∞ dB".to_string() } else { db(d.env_thresh) };
            dparam(cx, ui, p, x + 100.0, t + 165.0, rop::ENV_THR, "Thresh", &thr, col);
            dparam(cx, ui, p, x + 144.0, t + 165.0, rop::ENV_GAIN, "Gain", &db(d.env_gain), col);
            dparam(cx, ui, p, x + 184.0, t + 165.0, rop::ENV_FREQ, "Freq", &hz(d.env_freq), col);
            dparam(cx, ui, p, x + 234.0, t + 165.0, rop::ENV_WIDTH, "Width", &format!("{:.2}", d.env_width), col);
        }
        _ => {
            for (i, n) in RO_NOISES.iter().enumerate() {
                let y = t + 60.0 + 24.0 * i as f32;
                let w = p.layout_no_wrap(n.to_string(), font(FS), col).size().x;
                let br = Rect::from_center_size(pos2(x + 30.0, y - 2.5), vec2(w + 10.0, 12.0));
                let sel = d.noise as usize == i;
                if sel {
                    p.rect_filled(br, CornerRadius::ZERO, col);
                }
                ct(p, x + 30.0, y + 1.5, n, if sel { gray(0x18) } else { col });
                cx.salt = i as u32;
                if cx.toggle(ui, br, rop::N_KIND, true) {
                    set(cx, |q| q.noise = i as u8);
                }
            }
            cx.salt = 0;
            let v = if cx.on { tap.get(TAP_SRC[3]).clamp(-1.0, 1.0) } else { 0.0 };
            let y = t + 93.0 - v * 36.0;
            p.line_segment([pos2(x + 61.0, y), pos2(x + 271.0, y)], Stroke::new(1.5, col));
            let modes: Vec<&str> = RO_RATE_MODES.to_vec();
            text_at(p, x + 6.0, t + 165.0, Align2::LEFT_BOTTOM, "Mode", font(FS), LAB);
            if let Some(i) = text_menu(
                cx,
                ui,
                p,
                rr(x, t, 4.0, 168.0, 66.0, 181.0),
                rop::N_MODE,
                x + 6.0,
                t + 178.5,
                Some(x + 60.0),
                d.noise_mode as usize,
                &modes,
                true,
                col,
            ) {
                set(cx, |q| {
                    q.noise_mode = i as u8;
                    q.noise_div = q.noise_div.min(if i == 4 { RO_SIXTEENTHS as u8 - 1 } else { SYNC_DIVS.len() as u8 - 1 });
                });
            }
            let rkey = if d.noise_mode == 0 { rop::N_RATE } else { rop::N_DIV };
            dparam(cx, ui, p, x + 72.0, t + 165.0, rkey, "Rate", &rate_text(d.noise_mode, d.noise_rate, d.noise_div), col);
            dparam(cx, ui, p, x + 122.0, t + 165.0, rop::N_SMOOTH, "Smooth", &format!("{:.1} %", d.noise_smooth * 100.0), col);
        }
    }
}

/// Modulation matrix: targets × (LFO 1, LFO 2, Env, Noise), scrollable,
/// Global Amount below. `rows` visible from `y0`.
#[allow(clippy::too_many_arguments)]
fn matrix_rows(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, x: f32, y0: f32, w: f32, rows: usize, d: &RoarParams, salt: u32) {
    let cw = ((w - 104.0) / 4.0).floor();
    let hdr = ["LFO 1", "LFO 2", "Env", "Noise"];
    text_at(p, x + 6.0, y0 + 9.0, Align2::LEFT_BOTTOM, "Target", font(FS), LAB);
    let xc = pos2(x + 90.0, y0 + 5.0);
    p.circle_stroke(xc, 5.0, Stroke::new(1.0, st(cx, ORANGE)));
    for sgn in [-1.0, 1.0] {
        p.line_segment([pos2(xc.x - 2.5, xc.y - 2.5 * sgn), pos2(xc.x + 2.5, xc.y + 2.5 * sgn)], Stroke::new(1.0, st(cx, ORANGE)));
    }
    cx.salt = salt;
    if cx.toggle(ui, Rect::from_center_size(xc, vec2(12.0, 12.0)), rop::MOD_CLEAR, d.mod_active()) {
        set(cx, |q| q.matrix = [[0.0; MT]; 4]);
    }
    for (i, h) in hdr.iter().enumerate() {
        let cx0 = x + 104.0 + cw * i as f32;
        ct(p, cx0 + cw / 2.0, y0 + 9.0, h, src_col(cx, i));
        px(p, 0.0, 0.0, cx0 + 2.0, y0 + 12.0, cx0 + cw - 1.0, y0 + 13.0, gray(0x46));
    }
    let id = egui::Id::unique(("roar_mx", cx.dev.id, salt));
    let mut off = ui.data(|m| m.get_temp::<f32>(id)).unwrap_or(0.0);
    let area = Rect::from_min_max(pos2(x, y0 + 15.0), pos2(x + w, y0 + 15.0 + 13.0 * rows as f32));
    if ui.rect_contains_pointer(area) {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        off = (off - dy / 13.0).clamp(0.0, (MT - rows) as f32);
        ui.data_mut(|m| m.insert_temp(id, off));
    }
    let first = off.round() as usize;
    for (row, tg) in (first..(first + rows).min(MT)).enumerate() {
        let y = y0 + 15.0 + 13.0 * row as f32;
        let nr = Rect::from_min_max(pos2(x + 2.0, y), pos2(x + 103.0, y + 12.0));
        p.rect_filled(nr, CornerRadius::ZERO, gray(0x2a));
        text_at(p, nr.left() + 3.0, nr.bottom() - 1.5, Align2::LEFT_BOTTOM, TARGETS[tg], font(FS), text());
        for s in 0..4 {
            let cr = Rect::from_min_max(pos2(x + 105.0 + cw * s as f32, y), pos2(x + 104.0 + cw * (s + 1) as f32 - 1.0, y + 12.0));
            p.rect_filled(cr, CornerRadius::ZERO, gray(0x2a));
            let v = d.matrix[s][tg];
            if v != 0.0 {
                let c = cr.center().x;
                let e = c + v * cr.width() / 2.0;
                p.rect_filled(
                    Rect::from_min_max(pos2(c.min(e), cr.top() + 1.0), pos2(c.max(e), cr.bottom() - 1.0)),
                    CornerRadius::ZERO,
                    src_col(cx, s).gamma_multiply(0.55),
                );
                ct(p, c, cr.bottom() - 1.5, &format!("{:.0}", v * 100.0), text());
            }
            cx.drag(ui, cr, rop::cell(s, tg), true);
        }
    }
    cx.salt = 0;
}

fn matrix(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, x: f32, t: f32, d: &RoarParams) {
    matrix_rows(cx, ui, p, x, t + 22.0, 271.0, 8, d, 0);
    px(p, x, t, 0.0, 148.0, 271.0, 149.0, gray(0x36));
    dparam(cx, ui, p, x + 6.0, t + 165.0, rop::MOD_AMT, "Global Amount", &format!("{:.0} %", d.mod_amt * 100.0), st(cx, ORANGE));
}

/// Folded Sidechain strip: fold, listen, "Sidechain" + LED, "MIDI In" + LED.
fn strip(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, d: &RoarParams) {
    if d.sc_open {
        px(p, l, t, 19.0, 21.0, 20.0, 182.0, gray(0x24));
    } else {
        px(p, l, t, 19.0, 17.0, 21.0, 188.0, gray(0x46));
    }
    let fc = pos2(l + 9.5, t + 25.5);
    p.circle_filled(fc, 6.5, gray(0x12));
    p.circle_filled(fc, 5.5, gray(0x1c));
    let tri = if d.sc_open {
        vec![pos2(l + 7.0, t + 22.0), pos2(l + 13.5, t + 25.5), pos2(l + 7.0, t + 29.0)]
    } else {
        vec![pos2(l + 6.0, t + 23.0), pos2(l + 13.0, t + 23.0), pos2(l + 9.5, t + 29.5)]
    };
    p.add(Shape::convex_polygon(tri, gray(0xb5), Stroke::NONE));
    if cx.toggle(ui, Rect::from_center_size(fc, vec2(13.0, 13.0)), rop::SC_OPEN, true) {
        set(cx, |q| q.sc_open = !q.sc_open);
    }
    let hc = pos2(l + 9.5, t + 41.5);
    phones(p, hc, d.sc.listen, st(cx, ORANGE));
    if cx.toggle(ui, Rect::from_center_size(hc, vec2(13.0, 13.0)), rop::SC_LISTEN, true) {
        set(cx, |q| q.sc.listen = !q.sc.listen);
    }
    vtext(p, l + 13.0, t + 113.0, "Sidechain");
    let lc = pos2(l + 9.5, t + 121.5);
    p.circle_filled(lc, 3.5, if d.sc.on && cx.on { ORANGE } else { gray(0x24) });
    cx.salt = 0x1000;
    let sc_t = cx.toggle(ui, Rect::from_center_size(lc, vec2(11.0, 11.0)), rop::SC_ON, true);
    cx.salt = 0;
    if sc_t {
        set(cx, |q| q.sc.on = !q.sc.on);
    }
    vtext(p, l + 13.0, t + 167.0, "MIDI In");
    let mc = pos2(l + 9.5, t + 176.5);
    p.circle_filled(mc, 3.5, if d.midi.on && cx.on { ORANGE } else { gray(0x24) });
    cx.salt = 0x1000;
    let mi_t = cx.toggle(ui, Rect::from_center_size(mc, vec2(11.0, 11.0)), rop::MIDI_ON, true);
    cx.salt = 0;
    if mi_t {
        set(cx, |q| q.midi.on = !q.midi.on);
    }
}

/// Track chooser (`midi` = MIDI tracks, else audio-capable sources).
#[allow(clippy::too_many_arguments)]
fn source_chooser(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect, k: u8, cur_src: u64, midi: bool) -> Option<u64> {
    let own = match crate::fx_ops::top_of(&cx.app.project, cx.chain) {
        ChainRef::Track(id) => Some(id),
        _ => None,
    };
    let prj = &cx.app.project;
    let tracks: Vec<(u64, String)> = prj
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, x)| Some(x.id) != own && (x.kind == daw_project::TrackKind::Midi) == midi)
        .map(|(i, x)| (x.id, prj.track_label(i)))
        .collect();
    let cur = tracks.iter().find(|x| x.0 == cur_src).map_or("No Input".to_string(), |x| x.1.clone());
    let mut items: Vec<&str> = vec!["No Input"];
    items.extend(tracks.iter().map(|x| x.1.as_str()));
    let shown = fit(p, &cur, &font(FS), r.width() - 14.0);
    chooser(cx, ui, p, r, k, &shown, &items, true).map(|i| if i == 0 { 0 } else { tracks[i - 1].0 })
}

/// Unfolded Sidechain column: External SC (source, tap, Mix, Gain) and
/// MIDI > FB Note (source, tap).
fn sc_column(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, d: &RoarParams) {
    px(p, l, t, 117.0, 17.0, 119.0, 188.0, gray(0x46));
    if tbutton(cx, ui, p, rr(l, t, 24.0, 21.0, 113.0, 34.0), rop::SC_ON, "External SC", d.sc.on, true) {
        set(cx, |q| q.sc.on = !q.sc.on);
    }
    ct(p, l + 68.5, t + 48.0, "Source", text());
    if let Some(src) = source_chooser(cx, ui, p, rr(l, t, 24.0, 53.0, 113.0, 66.0), rop::SC_SRC, d.sc.source, false) {
        set(cx, |q| {
            q.sc.source = src;
            q.sc.on = src != 0;
        });
    }
    let taps: Vec<&str> = ScTap::ALL.iter().map(|x| x.label()).collect();
    let tr = rr(l, t, 24.0, 70.0, 113.0, 83.0);
    if d.sc.source == 0 {
        boxed(p, tr, box_fill(cx.on));
    } else if let Some(i) = chooser(cx, ui, p, tr, rop::SC_TAP, d.sc.tap.label(), &taps, true) {
        set(cx, |q| q.sc.tap = ScTap::ALL[i]);
    }
    ct(p, l + 45.0, t + 96.0, "Mix", text());
    let mr0 = rr(l, t, 24.0, 100.0, 66.0, 113.0);
    value_box_ex(cx, ui, p, mr0, rop::SC_MIX, &format!("{:.0} %", d.sc.mix * 100.0), true, false, gray(0x36));
    p.rect_stroke(mr0.shrink(0.5), CornerRadius::ZERO, Stroke::new(1.0, gray(0x11)), egui::StrokeKind::Middle);
    ct(p, l + 91.5, t + 96.0, "SC Gain", text());
    let gr0 = rr(l, t, 70.0, 100.0, 113.0, 113.0);
    value_box_ex(cx, ui, p, gr0, rop::SC_GAIN, &format!("{:.2} dB", d.sc.gain), true, false, gray(0x36));
    p.rect_stroke(gr0.shrink(0.5), CornerRadius::ZERO, Stroke::new(1.0, gray(0x11)), egui::StrokeKind::Middle);
    if tbutton(cx, ui, p, rr(l, t, 24.0, 123.0, 113.0, 136.0), rop::MIDI_ON, "MIDI > FB Note", d.midi.on, true) {
        set(cx, |q| q.midi.on = !q.midi.on);
    }
    ct(p, l + 68.5, t + 148.0, "MIDI From", text());
    if let Some(src) = source_chooser(cx, ui, p, rr(l, t, 24.0, 153.0, 113.0, 166.0), rop::MIDI_SRC, d.midi.source, true) {
        set(cx, |q| {
            q.midi.source = src;
            q.midi.on = src != 0;
            if src != 0 {
                q.fb_mode = 4;
            }
        });
    }
    let mr = rr(l, t, 24.0, 169.0, 113.0, 182.0);
    if d.midi.source == 0 {
        boxed(p, mr, box_fill(cx.on));
    } else if let Some(i) = chooser(cx, ui, p, mr, rop::MIDI_TAP, d.midi.tap.label(), &taps, true) {
        set(cx, |q| q.midi.tap = ScTap::ALL[i]);
    }
}

/// Expanded view: every stage plus the modulation matrix in a panel above
/// the device.
fn expanded(cx: &mut Ctx, ui: &mut egui::Ui, r: Rect, d: &RoarParams) {
    let n = d.n_stages();
    let w = 281.0 * n as f32 + 280.0;
    let h = 188.0;
    let pos = pos2(r.left(), (r.top() - h - 4.0).max(0.0));
    egui::Area::new(egui::Id::unique(("roar_exp", cx.dev.id))).order(egui::Order::Foreground).fixed_pos(pos).show(ui.ctx(), |ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
        let p = ui.painter_at(rect);
        let (x, t) = (rect.left(), rect.top());
        p.rect_filled(rect, CornerRadius::same(3), gray(0x46));
        let names = d.stage_names();
        #[allow(clippy::needless_range_loop)]
        for s in 0..n {
            let x0 = x + 281.0 * s as f32;
            p.rect_filled(Rect::from_min_max(pos2(x0 + 1.0, t), pos2(x0 + 280.0, t + 17.0)), CornerRadius::ZERO, gray(0x2a));
            p.rect_filled(Rect::from_min_max(pos2(x0 + 1.0, t + 17.0), pos2(x0 + 280.0, t + 89.0)), CornerRadius::ZERO, gray(0x36));
            let sq = Rect::from_min_size(pos2(x0 + 6.0, t + 4.0), vec2(9.0, 9.0));
            p.rect_filled(sq, CornerRadius::ZERO, gray(0x11));
            p.rect_filled(sq.shrink(1.0), CornerRadius::ZERO, if d.stages[s].on { stage_col(cx, s) } else { gray(0x1e) });
            text_at(&p, x0 + 20.0, t + 13.0, Align2::LEFT_BOTTOM, names[s], bold(FS), text());
            cx.salt = 0x2000 + s as u32;
            if cx.toggle(ui, sq.expand(2.0), rop::STAGE_ON + s as u8, true) {
                set(cx, |q| q.stages[s].on = !q.stages[s].on);
            }
            stage_panel(cx, ui, &p, x0 + 1.0, t, s, d, 0x2000);
        }
        let mx = x + 281.0 * n as f32;
        p.rect_filled(Rect::from_min_max(pos2(mx, t), pos2(mx + 279.0, t + 17.0)), CornerRadius::ZERO, gray(0x2a));
        text_at(&p, mx + 6.0, t + 13.0, Align2::LEFT_BOTTOM, "Modulation Matrix", bold(FS), text());
        p.rect_filled(Rect::from_min_max(pos2(mx, t + 17.0), pos2(mx + 279.0, t + 186.0)), CornerRadius::ZERO, gray(0x18));
        matrix_rows(cx, ui, &p, mx, t + 22.0, 277.0, 9, d, 0x3000);
        dparam(cx, ui, &p, mx + 6.0, t + 165.0, rop::MOD_AMT, "Global Amount", &format!("{:.0} %", d.mod_amt * 100.0), st(cx, ORANGE));
        cx.salt = 0;
    });
}

pub(super) fn reset(p: &mut FxParams, k: u8) {
    let d = FxParams::Roar(RoarParams::default());
    if let Some(v) = norm(&d, k) {
        set_norm(p, k, v);
    }
}

pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    let FxParams::Roar(d) = *p else { return };
    if let Some(v) = norm(p, k) {
        let rc = |m: u8| if m == 4 { RO_SIXTEENTHS } else { SYNC_DIVS.len() };
        let n = match k {
            rop::FB_DIV => (SYNC_DIVS.len() - 1) as f32,
            rop::FB_NOTE => 127.0,
            rop::L1_DIV => (rc(d.lfo[0].mode) - 1) as f32,
            rop::L2_DIV => (rc(d.lfo[1].mode) - 1) as f32,
            rop::N_DIV => (rc(d.noise_mode) - 1) as f32,
            _ => 100.0,
        };
        set_norm(p, k, ((v * n).round() + dir) / n);
    }
}

pub(super) fn name(k: u8) -> &'static str {
    const STAGE: [&str; 7] = ["Amount", "Bias", "Frequency", "Resonance", "Level", "Peak", "Morph"];
    match k {
        rop::DRIVE => "Drive",
        rop::TONE_AMT => "Tone Amount",
        rop::TONE_FREQ => "Tone Frequency",
        rop::BLEND => "Blend",
        rop::XO_LO => "Low Crossover",
        rop::XO_HI => "High Crossover",
        6..=26 => STAGE[((k - 6) % 7) as usize],
        rop::FB_AMT => "Feedback Amount",
        rop::FB_TIME => "Feedback Time",
        rop::FB_FREQ => "Feedback Frequency",
        rop::COMPRESS => "Compress",
        rop::DRY_WET => "Dry/Wet",
        rop::FB_WIDTH => "Feedback Width",
        rop::OUTPUT => "Output",
        rop::L1_RATE => "LFO 1 Rate",
        rop::L2_RATE => "LFO 2 Rate",
        rop::FB_DIV => "Feedback Sync",
        rop::FB_NOTE => "Feedback Note",
        rop::L1_DIV => "LFO 1 Sync Rate",
        rop::L1_MORPH => "LFO 1 Morph",
        rop::L1_SMOOTH => "LFO 1 Smooth",
        rop::L2_DIV => "LFO 2 Sync Rate",
        rop::L2_MORPH => "LFO 2 Morph",
        rop::L2_SMOOTH => "LFO 2 Smooth",
        rop::ENV_ATT => "Env Attack",
        rop::ENV_REL => "Env Release",
        rop::ENV_THR => "Env Threshold",
        rop::ENV_GAIN => "Env Gain",
        rop::ENV_FREQ => "Env Frequency",
        rop::ENV_WIDTH => "Env Width",
        rop::N_RATE => "Noise Rate",
        rop::N_DIV => "Noise Sync Rate",
        rop::N_SMOOTH => "Noise Smooth",
        rop::SC_MIX => "Sidechain Mix",
        rop::SC_GAIN => "Sidechain Gain",
        rop::MOD_AMT => "Global Mod Amount",
        rop::COLOR => "Tone Colour",
        rop::ROUTING => "Routing",
        62..=64 => "Stage On",
        65..=67 => "Shaper Type",
        68..=70 => "Shaper On",
        71..=73 => "Filter Type",
        74..=76 => "Filter On",
        77..=79 => "Filter Pre",
        rop::FB_MODE => "Feedback Mode",
        rop::FB_INV => "Feedback Invert",
        rop::FB_GATE => "Feedback Gate",
        rop::SC_HPF => "SC HPF",
        rop::L1_WAVE => "LFO 1 Waveform",
        rop::L1_MODE => "LFO 1 Mode",
        rop::L2_WAVE => "LFO 2 Waveform",
        rop::L2_MODE => "LFO 2 Mode",
        rop::ENV_HOLD => "Env Hold",
        rop::ENV_LISTEN => "Env Listen",
        rop::N_KIND => "Noise Type",
        rop::N_MODE => "Noise Mode",
        rop::SC_ON => "External Sidechain",
        rop::SC_SRC => "Sidechain Source",
        rop::SC_TAP => "Sidechain Tap",
        rop::MIDI_ON => "MIDI > FB Note",
        rop::MIDI_SRC => "MIDI From",
        rop::MIDI_TAP => "MIDI From Tap",
        rop::SC_LISTEN => "Sidechain Listen",
        rop::MOD_CLEAR => "Clear Matrix",
        100..=243 => "Mod Amount",
        _ => "",
    }
}
