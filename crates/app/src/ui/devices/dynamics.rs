//! Gate, Glue Compressor and Limiter (Live 12), plus the Gate / Glue
//! sidechain strip and section. Geometry measured from Live 12 at 100 %
//! (l/t = device left/top; text positions are baselines).

use super::*;
use daw_engine::dynfx::{GATE_FLOOR_MIN, GLUE_ATTACKS, GLUE_RELEASES, LIM_LOOKAHEAD, SC_GAIN_MIN};
use daw_project::devices::{
    GateParams, GlueParams, LimMode, LimiterParams, ScEq, ScTap, Sidechain, dev_tap,
    params::{GATE_SCALE, cp, gate_db_norm, gate_norm_db, gp, lp, mono_cubic, sc},
};

pub const GATE_W: f32 = 283.0;
pub const GLUE_W: f32 = 295.0;
pub const LIMITER_W: f32 = 284.0;
/// Unfolded sidechain section.
pub const SC_W: f32 = 156.0;

pub fn gate_width(g: &GateParams) -> f32 {
    GATE_W + if g.sc.shown { SC_W } else { 0.0 }
}

pub fn glue_width(g: &GlueParams) -> f32 {
    GLUE_W + if g.sc.shown { SC_W } else { 0.0 }
}

// ----------------------------------------------------------------------------
// Live 12 knobs: the teal arc runs from `origin` (min, or the centre for
// bipolar knobs) to the value.
// ----------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub(super) struct Kn {
    /// Ring radius (stroke centre), stroke width, half range (radians).
    pub r: f32,
    pub w: f32,
    pub span: f32,
}

pub(super) const KN: Kn = Kn { r: 11.9, w: 2.15, span: 150.0 * PI / 180.0 };
pub(super) const KN_BIG: Kn = Kn { r: 17.2, w: 2.5, span: 150.0 * PI / 180.0 };
pub(super) const KN_STEP: Kn = Kn { r: 10.0, w: 2.0, span: 120.0 * PI / 180.0 };

pub(super) fn knob12(p: &Painter, c: Pos2, v: f32, origin: f32, kn: Kn, enabled: bool, modv: Option<(f32, bool)>) {
    let remote = modv.is_some_and(|m| m.1);
    let enabled = enabled && !remote;
    let (track, hi, needle) = if enabled { (gray(0x11), TEAL, gray(0x18)) } else { (gray(0x2a), gray(0x75), gray(0x2a)) };
    let v = if remote { modv.map_or(v, |m| m.0) } else { v }.clamp(0.0, 1.0);
    let ang = |x: f32| -kn.span + 2.0 * kn.span * x;
    let gap = 0.2 * 11.9 / kn.r;
    let (a, o, lo, top) = (ang(v), ang(origin), ang(0.0), ang(1.0));
    let w = Stroke::new(kn.w, track);
    if a - gap > lo {
        arc(p, c, kn.r, lo, a - gap, w);
    }
    if a + gap < top {
        arc(p, c, kn.r, a + gap, top, w);
    }
    let hs = Stroke::new(kn.w, hi);
    if a - gap > o {
        arc(p, c, kn.r, o, a - gap, hs);
    } else if a + gap < o {
        arc(p, c, kn.r, a + gap, o, hs);
    }
    // Modulation (LFO Mod mode): a moving dot on the ring.
    if let Some((m, false)) = modv {
        let am = ang(m.clamp(0.0, 1.0));
        p.circle_filled(c + vec2(am.sin(), -am.cos()) * kn.r, kn.w * 0.9, gray(0xe0));
    }
    p.add(Shape::line(vec![c, c + vec2(a.sin(), -a.cos()) * (kn.r + 0.6)], Stroke::new(2.1, needle)));
}

/// Knob with label above and value below: label baseline `base`, knob
/// centre `base + dy`, value baseline `base + vy`.
#[allow(clippy::too_many_arguments)]
pub(super) fn knob_lv(
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
    enabled: bool,
) {
    let half = (kn.r + 10.0).max(22.5);
    let area = Rect::from_min_max(pos2(x - half, base - 9.0), pos2(x + half, base + vy + 2.0));
    cx.drag(ui, area, k, enabled);
    let v = norm(&cx.params, k).unwrap_or(0.0);
    if !label.is_empty() {
        text_at(p, x, base, Align2::CENTER_BOTTOM, label, font(FS), text());
    }
    knob12(p, pos2(x, base + dy), v, origin, kn, enabled && cx.on, cx.modv(k));
    if !value.is_empty() {
        text_at(p, x, base + vy, Align2::CENTER_BOTTOM, value, font(FS), text());
    }
    if cx.focused(k) {
        brackets(p, area);
    }
}

/// Live's stepped knob (Glue Attack / Release / Ratio, Limiter Lookahead):
/// ticks at the steps, labels around. `labels`: (text, dx, base dy, align).
#[allow(clippy::too_many_arguments)]
fn stepped(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, c: Pos2, k: u8, n: usize, labels: &[(&str, f32, f32, Align2)], small: bool) {
    let area = Rect::from_center_size(c, vec2(44.0, 36.0));
    cx.drag(ui, area, k, true);
    let v = norm(&cx.params, k).unwrap_or(0.0);
    knob12(p, c, v, 0.0, KN_STEP, cx.on, cx.modv(k));
    let tick = gray(0x9b);
    for i in 0..n {
        let a = -KN_STEP.span + 2.0 * KN_STEP.span * i as f32 / (n - 1) as f32;
        let d = vec2(a.sin(), -a.cos());
        p.add(Shape::line(vec![c + d * 13.4, c + d * 15.6], Stroke::new(1.2, tick)));
    }
    let f = font(if small { FS_SMALL } else { FS });
    for &(s, dx, dy, al) in labels {
        text_at(p, c.x + dx, c.y + dy, al, s, f.clone(), text());
    }
    if cx.focused(k) {
        brackets(p, area);
    }
}

/// Live number formatting with three significant digits ("3.50", "15.0", "100").
pub(super) fn sig3(x: f32) -> String {
    let a = x.abs();
    if a < 9.995 {
        format!("{x:.2}")
    } else if a < 99.95 {
        format!("{x:.1}")
    } else {
        format!("{x:.0}")
    }
}

fn ms(x: f32) -> String {
    format!("{} ms", sig3(x))
}

fn db1(x: f32) -> String {
    format!("{x:.1} dB")
}

fn pct(x: f32) -> String {
    format!("{:.0} %", x * 100.0)
}

/// Clickable toggle button (`toggle_look`), returns true when clicked.
#[allow(clippy::too_many_arguments)]
pub(super) fn tbutton(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect, k: u8, label: &str, on: bool, enabled: bool) -> bool {
    toggle_look(p, r, label, on, enabled, cx.on);
    cx.toggle(ui, r, k, enabled)
}

// ----------------------------------------------------------------------------
// Sidechain strip (always shown) and section (unfolded).
// ----------------------------------------------------------------------------

fn sc_of(p: &mut FxParams) -> Option<&mut Sidechain> {
    match p {
        FxParams::Gate(g) => Some(&mut g.sc),
        FxParams::Glue(g) => Some(&mut g.sc),
        FxParams::AutoFilter(a) => Some(&mut a.sc),
        FxParams::Compressor(c) => Some(&mut c.sc),
        _ => None,
    }
}

pub(super) fn sc_get(p: &FxParams) -> Sidechain {
    p.sidechain().copied().unwrap_or_default()
}

/// The narrow strip at the device's left: fold ▼, listen, "Sidechain", LED.
pub(super) fn sc_strip(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32, divider: bool) {
    let s = sc_get(&cx.params);
    if divider {
        quad(p, Rect::from_min_max(pos2(l + 19.0, t + 21.0), pos2(l + 20.0, t + 184.0)), gray(0x24));
    }
    // Fold button.
    let fc = pos2(l + 9.5, t + 27.5);
    p.circle_filled(fc, 6.5, gray(0x12));
    p.circle_filled(fc, 5.5, gray(0x1c));
    let g = gray(0xb5);
    let tri = if s.shown {
        vec![pos2(l + 7.0, t + 24.0), pos2(l + 13.5, t + 27.5), pos2(l + 7.0, t + 31.0)]
    } else {
        vec![pos2(l + 6.0, t + 25.0), pos2(l + 13.0, t + 25.0), pos2(l + 9.5, t + 31.5)]
    };
    p.add(Shape::convex_polygon(tri, g, Stroke::NONE));
    let fr = ui.interact(Rect::from_center_size(fc, vec2(13.0, 13.0)), egui::Id::unique(("sc_fold", cx.dev.id)), Sense::click());
    if fr.clicked()
        && let Some(sc) = sc_of(&mut cx.params)
    {
        sc.shown = !sc.shown;
    }
    // Listen (headphones).
    let hc = pos2(l + 9.5, t + 43.5);
    p.circle_filled(hc, 6.5, gray(0x12));
    p.circle_filled(hc, 5.5, if s.listen { ORANGE } else { gray(0x1c) });
    headphones(p, hc, if s.listen { gray(0x20) } else { gray(0x90) });
    if cx.toggle(ui, Rect::from_center_size(hc, vec2(13.0, 13.0)), sc::LISTEN, true)
        && let Some(sc) = sc_of(&mut cx.params)
    {
        sc.listen = !sc.listen;
    }
    // Vertical caption, read bottom to top.
    let gal = p.layout_no_wrap("Sidechain".into(), font(FS), text());
    let asc = gal.rows.first().map_or(8.0, |r| r.row.glyphs.first().map_or(8.0, |gl| gl.pos.y));
    p.add(egui::epaint::TextShape::new(pos2(l + 13.0 - asc, t + 171.0), gal, text()).with_angle(-PI / 2.0));
    // LED: lit while an external sidechain is active.
    let lit = s.on && s.source != 0;
    p.circle_filled(pos2(l + 9.5, t + 178.5), 3.5, if lit { ORANGE } else { gray(0x24) });
}

fn headphones(p: &Painter, c: Pos2, col: Color32) {
    arc(p, c + vec2(0.0, 1.0), 3.8, -2.0, 2.0, Stroke::new(1.2, col));
    for dx in [-3.5, 2.0] {
        quad(p, Rect::from_min_size(c + vec2(dx, 0.5), vec2(1.5, 3.5)), col);
    }
}

/// Sidechain filter-type icon (Live's 3×2 buttons).
fn sc_icon(p: &Painter, r: Rect, kind: ScEq, col: Color32) {
    let s = Stroke::new(1.0, col);
    let (x0, x1, y0, y1) = (r.left() + 3.0, r.right() - 3.0, r.top() + 3.0, r.bottom() - 3.0);
    let ym = (y0 + y1) / 2.0;
    let xm = (x0 + x1) / 2.0;
    let pts = match kind {
        ScEq::LowShelf => {
            vec![pos2(x0, y0), pos2(xm - 3.0, y0), pos2(xm + 1.0, ym), pos2(x1, ym), pos2(xm + 1.0, ym), pos2(xm - 3.0, y1), pos2(x0, y1)]
        }
        ScEq::Peak => vec![pos2(x0, ym), pos2(xm - 4.0, ym), pos2(xm - 1.5, y0), pos2(xm + 1.5, y0), pos2(xm + 4.0, ym), pos2(x1, ym)],
        ScEq::HighShelf => {
            vec![pos2(x1, y0), pos2(xm + 3.0, y0), pos2(xm - 1.0, ym), pos2(x0, ym), pos2(xm - 1.0, ym), pos2(xm + 3.0, y1), pos2(x1, y1)]
        }
        ScEq::LowPass => vec![pos2(x0, y0), pos2(xm + 2.0, y0), pos2(x1 - 1.0, y0 + 2.0), pos2(x1 - 1.0, y1)],
        ScEq::BandPass => vec![
            pos2(x0 + 1.0, y1),
            pos2(x0 + 1.0, y0 + 2.0),
            pos2(x0 + 3.0, y0),
            pos2(x1 - 3.0, y0),
            pos2(x1 - 1.0, y0 + 2.0),
            pos2(x1 - 1.0, y1),
        ],
        ScEq::HighPass => vec![pos2(x0 + 1.0, y1), pos2(x0 + 1.0, y0 + 2.0), pos2(x0 + 3.0, y0), pos2(x1, y0)],
    };
    if kind == ScEq::Peak {
        // Bell: the dip drawn too (Live shows an outline lens).
        let lower = vec![pos2(xm - 4.0, ym), pos2(xm - 1.5, y1), pos2(xm + 1.5, y1), pos2(xm + 4.0, ym)];
        p.add(Shape::line(lower, s));
    }
    p.add(Shape::line(pts, s));
}

/// Fold button in the title bar while the sidechain is unfolded.
pub(super) fn title_fold(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32) {
    let c = pos2(l + 27.5, t + 8.5);
    p.circle_filled(c, 6.5, gray(0x11));
    p.circle_filled(c, 5.5, ORANGE);
    p.add(Shape::convex_polygon(
        vec![pos2(c.x - 2.0, c.y - 3.5), pos2(c.x + 3.0, c.y), pos2(c.x - 2.0, c.y + 3.5)],
        gray(0x20),
        Stroke::NONE,
    ));
    let r = ui.interact(Rect::from_center_size(c, vec2(13.0, 13.0)), egui::Id::unique(("sc_unfold", cx.dev.id)), Sense::click());
    if r.clicked()
        && let Some(sc) = sc_of(&mut cx.params)
    {
        sc.shown = false;
    }
}

/// Unfolded section (replaces the strip; the main section moves right by
/// [`SC_W`]). Layout from the Live 12 manual screenshots.
fn sc_section(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32) {
    let s = sc_get(&cx.params);
    let own = match crate::fx_ops::top_of(&cx.app.project, cx.chain) {
        ChainRef::Track(id) => Some(id),
        _ => None,
    };
    if tbutton(cx, ui, p, Rect::from_min_size(pos2(l + 14.0, t + 24.0), vec2(60.0, 13.0)), sc::ON, "Sidechain", s.on, true)
        && let Some(sc) = sc_of(&mut cx.params)
    {
        sc.on = !sc.on;
    }
    let hc = pos2(l + 87.5, t + 30.5);
    p.circle_filled(hc, 7.0, gray(0x12));
    p.circle_filled(hc, 6.0, if s.listen { ORANGE } else { gray(0x1c) });
    headphones(p, hc, if s.listen { gray(0x20) } else { gray(0x90) });
    cx.salt = 1;
    let hit = cx.toggle(ui, Rect::from_center_size(hc, vec2(14.0, 14.0)), sc::LISTEN, true);
    cx.salt = 0;
    if hit && let Some(sc) = sc_of(&mut cx.params) {
        sc.listen = !sc.listen;
    }
    if tbutton(cx, ui, p, Rect::from_min_size(pos2(l + 101.0, t + 24.0), vec2(58.0, 13.0)), sc::EQ_ON, "EQ", s.eq_on, true)
        && let Some(sc) = sc_of(&mut cx.params)
    {
        sc.eq_on = !sc.eq_on;
    }
    // Audio From: tracks other than this one.
    text_at(p, l + 8.0, t + 49.0, Align2::LEFT_BOTTOM, "Audio From", font(FS), if s.on { text() } else { dim() });
    let prj = &cx.app.project;
    let tracks: Vec<(u64, String)> =
        prj.tracks.iter().enumerate().filter(|(_, x)| Some(x.id) != own).map(|(i, x)| (x.id, prj.track_label(i))).collect();
    let cur = tracks.iter().find(|x| x.0 == s.source).map_or("No Input".to_string(), |x| x.1.clone());
    let mut items: Vec<&str> = vec!["No Input"];
    items.extend(tracks.iter().map(|x| x.1.as_str()));
    let cr = Rect::from_min_size(pos2(l + 7.0, t + 52.0), vec2(72.0, 13.0));
    let shown = fit(p, &cur, &font(FS), cr.width() - 14.0);
    if let Some(i) = chooser(cx, ui, p, cr, sc::SOURCE, &shown, &items, s.on)
        && let Some(sc) = sc_of(&mut cx.params)
    {
        sc.source = if i == 0 { 0 } else { tracks[i - 1].0 };
    }
    // Tap chooser with its meter icon.
    let tr = Rect::from_min_size(pos2(l + 7.0, t + 69.0), vec2(72.0, 13.0));
    let taps: Vec<&str> = ScTap::ALL.iter().map(|x| x.label()).collect();
    let ext = s.on && s.source != 0;
    if let Some(i) = chooser(cx, ui, p, tr, sc::TAP, &format!("    {}", s.tap.label()), &taps, ext)
        && let Some(sc) = sc_of(&mut cx.params)
    {
        sc.tap = ScTap::ALL[i];
    }
    for (i, c) in [Color32::from_rgb(0x00, 0xd4, 0x4a), Color32::from_rgb(0xff, 0xa0, 0x30)].into_iter().enumerate() {
        let bx = tr.left() + 4.0 + i as f32 * 3.0;
        quad(
            p,
            Rect::from_min_max(pos2(bx, tr.top() + 3.0), pos2(bx + 2.0, tr.bottom() - 3.0)),
            if ext { c } else { mix(c, gray(0x24), 0.6) },
        );
    }
    // Gain / Mix.
    let gain = if s.gain <= SC_GAIN_MIN { "-inf dB".to_string() } else { format!("{} dB", sig3(s.gain)) };
    knob_lv(cx, ui, p, l + 41.0, t + 96.0, sc::GAIN, "Gain", &gain, 0.0, KN, 15.5, 36.0, ext);
    knob_lv(cx, ui, p, l + 41.0, t + 147.0, sc::MIX, "Mix", &pct(s.mix), 0.0, KN, 15.5, 36.0, ext);
    // Filter section.
    quad(p, Rect::from_min_max(pos2(l + 88.0, t + 40.0), pos2(l + 89.0, t + 184.0)), gray(0x24));
    let eq = s.eq_on;
    text_at(p, l + 130.5, t + 48.0, Align2::CENTER_BOTTOM, "Filter Type", font(FS), text());
    for (i, kind) in ScEq::ALL.into_iter().enumerate() {
        let x = [96.0, 120.5, 145.0][i % 3];
        let r = Rect::from_min_size(pos2(l + x, t + 54.0 + (i / 3) as f32 * 16.0), vec2(20.0, 12.0));
        let sel = s.eq == kind;
        let (fill, col) = match (sel, eq && cx.on) {
            (true, true) => (ORANGE, gray(0x20)),
            (true, false) => (gray(0x5a), gray(0x24)),
            (false, true) => (gray(0x1e), gray(0x9a)),
            (false, false) => (gray(0x20), gray(0x55)),
        };
        boxed(p, r, fill);
        sc_icon(p, r, kind, col);
        cx.salt = i as u32;
        if cx.toggle(ui, r, sc::EQ, true)
            && let Some(sc) = sc_of(&mut cx.params)
        {
            sc.eq = kind;
        }
        cx.salt = 0;
    }
    let fq = if s.freq >= 1000.0 { format!("{} kHz", sig3(s.freq / 1000.0)) } else { format!("{:.0} Hz", s.freq) };
    knob_lv(cx, ui, p, l + 130.5, t + 96.0, sc::FREQ, "Freq", &fq, 0.0, KN, 15.5, 36.0, eq);
    if s.eq.has_gain() {
        knob_lv(cx, ui, p, l + 130.5, t + 147.0, sc::EQ_GAIN, "Gain", &format!("{} dB", sig3(s.eq_gain)), 0.5, KN, 15.5, 36.0, eq);
    } else {
        knob_lv(cx, ui, p, l + 130.5, t + 147.0, sc::Q, "Q", &format!("{:.2}", s.q), 0.0, KN, 15.5, 36.0, eq);
    }
    // Divider to the main section with its ▸.
    quad(p, Rect::from_min_max(pos2(l + 174.0, t + 40.0), pos2(l + 175.0, t + 184.0)), gray(0x24));
    p.add(Shape::convex_polygon(
        vec![pos2(l + 170.0, t + 100.0), pos2(l + 173.5, t + 103.5), pos2(l + 170.0, t + 107.0)],
        gray(0x55),
        Stroke::NONE,
    ));
}

/// Auto Filter sidechain section (right of the strip; Live 12 manual
/// layout): External, Source / tap, Mix, SC Gain | SC Filter, type, Freq,
/// Res (Gain for shelves / peak).
pub(super) fn af_sc_section(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32) {
    sc_columns(cx, ui, p, t, Some(l + 24.0), l + 102.0);
}

/// Sidechain columns: External (button left at `ext`, omitted when None)
/// and SC Filter (button left at `f`).
pub(super) fn sc_columns(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, t: f32, ext: Option<f32>, f: f32) {
    let s = sc_get(&cx.params);
    if let Some(e) = ext {
        let own = match crate::fx_ops::top_of(&cx.app.project, cx.chain) {
            ChainRef::Track(id) => Some(id),
            _ => None,
        };
        if tbutton(cx, ui, p, Rect::from_min_size(pos2(e, t + 21.0), vec2(69.0, 13.0)), sc::ON, "External", s.on, true)
            && let Some(sc) = sc_of(&mut cx.params)
        {
            sc.on = !sc.on;
        }
        quad(p, Rect::from_min_max(pos2(f - 5.0, t + 21.0), pos2(f - 4.0, t + 184.0)), gray(0x24));
        let ext_on = s.on && s.source != 0;
        let cx0 = e + 34.5;
        text_at(p, cx0, t + 57.0, Align2::CENTER_BOTTOM, "Source", font(FS), if s.on { text() } else { dim() });
        let prj = &cx.app.project;
        let tracks: Vec<(u64, String)> =
            prj.tracks.iter().enumerate().filter(|(_, x)| Some(x.id) != own).map(|(i, x)| (x.id, prj.track_label(i))).collect();
        let cur = tracks.iter().find(|x| x.0 == s.source).map_or("No Input".to_string(), |x| x.1.clone());
        let mut items: Vec<&str> = vec!["No Input"];
        items.extend(tracks.iter().map(|x| x.1.as_str()));
        let cr = Rect::from_min_size(pos2(e, t + 60.0), vec2(69.0, 13.0));
        let shown = fit(p, &cur, &font(FS), cr.width() - 14.0);
        if let Some(i) = chooser(cx, ui, p, cr, sc::SOURCE, &shown, &items, s.on)
            && let Some(sc) = sc_of(&mut cx.params)
        {
            sc.source = if i == 0 { 0 } else { tracks[i - 1].0 };
        }
        let tr = Rect::from_min_size(pos2(e, t + 76.0), vec2(69.0, 13.0));
        let taps: Vec<&str> = ScTap::ALL.iter().map(|x| x.label()).collect();
        if let Some(i) = chooser(cx, ui, p, tr, sc::TAP, &format!("    {}", s.tap.label()), &taps, ext_on)
            && let Some(sc) = sc_of(&mut cx.params)
        {
            sc.tap = ScTap::ALL[i];
        }
        for (i, c) in [Color32::from_rgb(0x00, 0xd4, 0x4a), Color32::from_rgb(0xff, 0xa0, 0x30)].into_iter().enumerate() {
            let bx = tr.left() + 4.0 + i as f32 * 3.0;
            quad(
                p,
                Rect::from_min_max(pos2(bx, tr.top() + 3.0), pos2(bx + 2.0, tr.bottom() - 3.0)),
                if ext_on { c } else { mix(c, gray(0x24), 0.6) },
            );
        }
        text_at(p, cx0, t + 122.0, Align2::CENTER_BOTTOM, "Mix", font(FS), if ext_on { text() } else { dim() });
        value_box(
            cx,
            ui,
            p,
            Rect::from_min_size(pos2(e, t + 125.0), vec2(69.0, 13.0)),
            sc::MIX,
            &format!("{:.1} %", s.mix * 100.0),
            ext_on,
        );
        text_at(p, cx0, t + 166.0, Align2::CENTER_BOTTOM, "SC Gain", font(FS), if ext_on { text() } else { dim() });
        let gain = if s.gain <= SC_GAIN_MIN { "-inf dB".to_string() } else { format!("{} dB", sig3(s.gain)) };
        value_box(cx, ui, p, Rect::from_min_size(pos2(e, t + 169.0), vec2(69.0, 13.0)), sc::GAIN, &gain, ext_on);
    }
    if tbutton(cx, ui, p, Rect::from_min_size(pos2(f, t + 21.0), vec2(51.0, 13.0)), sc::EQ_ON, "SC Filter", s.eq_on, true)
        && let Some(sc) = sc_of(&mut cx.params)
    {
        sc.eq_on = !sc.eq_on;
    }
    // Filter column.
    let eq = s.eq_on;
    let fc = f + 25.5;
    text_at(p, fc, t + 57.0, Align2::CENTER_BOTTOM, "Type", font(FS), if eq { text() } else { dim() });
    let ty = Rect::from_min_size(pos2(f, t + 60.0), vec2(51.0, 13.0));
    let names: Vec<&str> = ScEq::ALL.iter().map(|k| k.label()).collect();
    if let Some(i) = chooser(cx, ui, p, ty, sc::EQ, "", &names, eq)
        && let Some(sc) = sc_of(&mut cx.params)
    {
        sc.eq = ScEq::ALL[i];
    }
    sc_icon(p, Rect::from_min_max(ty.min, pos2(ty.left() + 24.0, ty.bottom())), s.eq, if eq && cx.on { text() } else { dim() });
    let fq = if s.freq >= 1000.0 {
        format!("{} kHz", sig3(s.freq / 1000.0))
    } else if s.freq >= 100.0 {
        format!("{:.0} Hz", s.freq)
    } else {
        format!("{:.1} Hz", s.freq)
    };
    knob_lv(cx, ui, p, fc, t + 98.0, sc::FREQ, "Freq", &fq, 0.0, KN, 15.5, 36.0, eq);
    let br = Rect::from_min_size(pos2(f, t + 169.0), vec2(51.0, 13.0));
    if s.eq.has_gain() {
        text_at(p, fc, t + 166.0, Align2::CENTER_BOTTOM, "Gain", font(FS), if eq { text() } else { dim() });
        value_box(cx, ui, p, br, sc::EQ_GAIN, &format!("{} dB", sig3(s.eq_gain)), eq);
    } else {
        text_at(p, fc, t + 166.0, Align2::CENTER_BOTTOM, "Res", font(FS), if eq { text() } else { dim() });
        value_box(cx, ui, p, br, sc::Q, &format!("{:.2}", s.q), eq);
    }
}

// ----------------------------------------------------------------------------
// Gate (283 px + sidechain).
// ----------------------------------------------------------------------------

/// Gate display height of `db` (pixels above the scale bottom, 98 px span).
fn gate_y(bottom: f32, db: f32) -> f32 {
    bottom - mono_cubic(&GATE_SCALE, db) * 98.0
}

pub(super) fn gate(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32) {
    let FxParams::Gate(g0) = cx.params else { return };
    let ox = if g0.sc.shown {
        sc_section(cx, ui, p, l, t);
        SC_W
    } else {
        sc_strip(cx, ui, p, l, t, true);
        0.0
    };
    let l = l + ox;
    // Left column.
    knob_lv(cx, ui, p, l + 53.5, t + 32.0, gp::THRESHOLD, "Threshold", &db1(g0.threshold), 0.0, KN, 15.5, 36.0, true);
    knob_lv(cx, ui, p, l + 53.5, t + 86.0, gp::RETURN, "Return", &format!("{} dB", sig3(g0.ret)), 0.0, KN, 15.5, 36.0, true);
    quad(p, Rect::from_min_max(pos2(l + 27.0, t + 131.0), pos2(l + 80.0, t + 132.0)), gray(0x24));
    if tbutton(cx, ui, p, Rect::from_min_size(pos2(l + 27.0, t + 137.0), vec2(53.0, 13.0)), gp::FLIP, "Flip", g0.flip, true)
        && let FxParams::Gate(g) = &mut cx.params
    {
        g.flip = !g.flip;
    }
    text_at(p, l + 53.5, t + 167.0, Align2::CENTER_BOTTOM, "Lookahead", font(FS), text());
    let la = ["0 ms", "1.5 ms", "10 ms"];
    if let Some(i) = chooser(
        cx,
        ui,
        p,
        Rect::from_min_size(pos2(l + 27.0, t + 170.0), vec2(53.0, 13.0)),
        gp::LOOKAHEAD,
        la[(g0.lookahead as usize).min(2)],
        &la,
        true,
    ) && let FxParams::Gate(g) = &mut cx.params
    {
        g.lookahead = i as u8;
    }
    // Display.
    let d = Rect::from_min_max(pos2(l + 87.0, t + 21.0), pos2(l + 279.0, t + 132.0));
    p.rect_filled(d, CornerRadius::same(2), gray(0x18));
    let (gx0, gx1, gy0, gy1) = (l + 111.0, l + 260.0, t + 27.0, t + 125.0);
    let line = gray(0x3e);
    for (x0, x1) in [(gx0, gx1), (l + 266.0, l + 272.0)] {
        quad(p, Rect::from_min_max(pos2(x0, gy0), pos2(x1 + 1.0, gy0 + 1.0)), line);
        quad(p, Rect::from_min_max(pos2(x0, gy1), pos2(x1 + 1.0, gy1 + 1.0)), line);
        quad(p, Rect::from_min_max(pos2(x0, gy0), pos2(x0 + 1.0, gy1 + 1.0)), line);
        quad(p, Rect::from_min_max(pos2(x1, gy0), pos2(x1 + 1.0, gy1 + 1.0)), line);
    }
    let bottom = t + 125.5;
    for (db, lbl) in [(6.0, "+6"), (0.0, "0"), (-6.0, "-6"), (-12.0, "-12"), (-18.0, "-18"), (-36.0, "-36"), (-70.0, "-70")] {
        let y = gate_y(bottom, db).floor();
        quad(p, Rect::from_min_max(pos2(l + 109.0, y), pos2(l + 111.0, y + 1.0)), line);
        text_at(p, l + 107.0, y + 4.0, Align2::RIGHT_BOTTOM, lbl, font(FS), gray(0x84));
    }
    // 0 dB dotted line.
    let y0 = gate_y(bottom, 0.0).floor();
    let mut x = gx0 + 2.0;
    while x < gx1 {
        quad(p, Rect::from_min_size(pos2(x, y0), vec2(1.0, 1.0)), line);
        x += 2.0;
    }
    // History: input (light), output (darker, white outline); the
    // sidechain in green while listening.
    let tap = dev_tap(cx.dev.id);
    let cols = (gx1 - gx0 - 1.0) as usize;
    const PER: usize = 16;
    // Buckets start on absolute multiples of PER: the picture scrolls by
    // whole columns instead of re-binning (twitching) every repaint.
    let w = tap.written();
    let h = tap.history_to(w - w % PER, cols * PER);
    let off = cols * PER - h.len().min(cols * PER);
    // Output outline, broken at silent columns (no lines across gaps).
    let mut outline: Vec<Pos2> = Vec::with_capacity(cols);
    let mut runs: Vec<Vec<Pos2>> = Vec::new();
    let yy = |v: f32| gate_y(bottom, daw_engine::dynfx::lin_db(v).max(-70.0)).clamp(gy0 + 1.0, gy1);
    for c in 0..cols {
        let a = (c * PER).saturating_sub(off);
        let b = ((c + 1) * PER).saturating_sub(off).min(h.len());
        if a >= b {
            outline.clear();
            continue;
        }
        let mut m = [0.0f32; 4];
        for f in &h[a..b] {
            for k in 0..4 {
                m[k] = m[k].max(f[k]);
            }
        }
        let x = gx0 + 1.0 + c as f32;
        if m[0].max(m[1]).max(m[2]) < 3.2e-4 {
            if outline.len() > 1 {
                runs.push(std::mem::take(&mut outline));
            }
            outline.clear();
            continue;
        }
        let (yi, yo) = (yy(m[0]), yy(m[1]));
        if g0.sc.listen {
            let ys = yy(m[2]);
            quad(p, Rect::from_min_max(pos2(x, ys), pos2(x + 1.0, gy1)), Color32::from_rgb(0x0d, 0x80, 0x3a));
        } else {
            quad(p, Rect::from_min_max(pos2(x, yi), pos2(x + 1.0, gy1)), gray(0x5a));
            quad(p, Rect::from_min_max(pos2(x, yo), pos2(x + 1.0, gy1)), gray(0x3c));
            outline.push(pos2(x + 0.5, yo));
        }
    }
    if outline.len() > 1 {
        runs.push(outline);
    }
    for run in runs {
        p.add(Shape::line(run, Stroke::new(1.0, gray(0xd8))));
    }
    if tap.written() > 0 {
        cx.app.animating = true;
    }
    // Threshold / return lines (draggable).
    let ty = gate_y(bottom, g0.threshold);
    let ry = gate_y(bottom, g0.threshold - g0.ret);
    for (y, a0, a1) in [(ty, 0.6, 0.9), (ry, 0.75, 0.75)] {
        let y = y.floor();
        quad(p, Rect::from_min_max(pos2(gx0 + 1.0, y - 1.0), pos2(gx1, y)), TEAL.gamma_multiply(a0));
        quad(p, Rect::from_min_max(pos2(gx0 + 1.0, y), pos2(gx1, y + 1.0)), TEAL.gamma_multiply(a1));
    }
    let gr = Rect::from_min_max(pos2(gx0, gy0), pos2(gx1, gy1));
    cx.salt = 1;
    let resp = cx.sense(ui, gr, gp::THRESHOLD, Sense::click_and_drag());
    cx.salt = 0;
    let did = egui::Id::unique(("gate_line", cx.dev.id));
    if resp.drag_started()
        && let Some(pp) = resp.interact_pointer_pos()
    {
        let k = if (pp.y - ry).abs() < (pp.y - ty).abs() { gp::RETURN } else { gp::THRESHOLD };
        ui.data_mut(|dd| dd.insert_temp(did, k));
        cx.focus(k);
    }
    if resp.dragged()
        && let Some(pp) = resp.interact_pointer_pos()
    {
        let k: u8 = ui.data(|dd| dd.get_temp(did)).unwrap_or(gp::THRESHOLD);
        let db = gate_norm_db((bottom - pp.y) / 98.0);
        cx.undo_step();
        if let FxParams::Gate(g) = &mut cx.params {
            if k == gp::THRESHOLD {
                g.threshold = db.clamp(-70.0, 6.0);
            } else {
                g.ret = (g.threshold - db).clamp(0.0, daw_engine::dynfx::GATE_RETURN_MAX);
            }
        }
    }
    // Attenuation meter (orange from 0 dB down).
    let live = h.last().is_some_and(|f| f[0] > 3.2e-4);
    let att = if live { tap.get(0).clamp(0.0, 70.0) } else { 0.0 };
    if att > 0.05 {
        let (ya, yb) = (gate_y(bottom, 0.0), gate_y(bottom, -att));
        quad(p, Rect::from_min_max(pos2(l + 267.0, ya), pos2(l + 272.0, yb)), ORANGE);
    }
    quad(p, Rect::from_min_max(pos2(l + 267.0, y0 - 1.0), pos2(l + 272.0, y0)), gray(0x36));
    // Envelope row.
    knob_lv(cx, ui, p, l + 109.5, t + 145.0, gp::ATTACK, "Attack", &ms(g0.attack), 0.0, KN, 15.5, 36.0, true);
    knob_lv(cx, ui, p, l + 156.5, t + 145.0, gp::HOLD, "Hold", &ms(g0.hold), 0.0, KN, 15.5, 36.0, true);
    knob_lv(cx, ui, p, l + 203.5, t + 145.0, gp::RELEASE, "Release", &ms(g0.release), 0.0, KN, 15.5, 36.0, true);
    quad(p, Rect::from_min_max(pos2(l + 229.0, t + 138.0), pos2(l + 230.0, t + 184.0)), gray(0x24));
    let fl = if g0.floor <= GATE_FLOOR_MIN { "-inf dB".to_string() } else { db1(g0.floor) };
    knob_lv(cx, ui, p, l + 255.5, t + 145.0, gp::FLOOR, "Floor", &fl, 0.0, KN, 15.5, 36.0, true);
    let _ = gate_db_norm;
}

// ----------------------------------------------------------------------------
// Glue Compressor (295 px + sidechain).
// ----------------------------------------------------------------------------

/// Labels of a 7-step knob: (text, dx, base dy, align).
fn seven(t: [&str; 7]) -> [(&str, f32, f32, Align2); 7] {
    [
        (t[0], -14.5, 12.5, Align2::RIGHT_BOTTOM),
        (t[1], -17.5, 0.5, Align2::RIGHT_BOTTOM),
        (t[2], -9.5, -13.5, Align2::RIGHT_BOTTOM),
        (t[3], -0.5, -16.5, Align2::CENTER_BOTTOM),
        (t[4], 9.5, -13.5, Align2::LEFT_BOTTOM),
        (t[5], 17.5, 0.5, Align2::LEFT_BOTTOM),
        (t[6], 16.5, 12.5, Align2::LEFT_BOTTOM),
    ]
}

pub(super) fn glue(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32) {
    let FxParams::Glue(g0) = cx.params else { return };
    let ox = if g0.sc.shown {
        sc_section(cx, ui, p, l, t);
        SC_W
    } else {
        sc_strip(cx, ui, p, l, t, true);
        0.0
    };
    let l = l + ox;
    // Stepped knobs.
    text_at(p, l + 54.5, t + 32.0, Align2::CENTER_BOTTOM, "Attack", font(FS), text());
    stepped(cx, ui, p, pos2(l + 54.5, t + 58.5), cp::ATTACK, 7, &seven([".01", ".1", ".3", "1", "3", "10", "30"]), false);
    text_at(p, l + 54.5, t + 85.0, Align2::CENTER_BOTTOM, "Release", font(FS), text());
    stepped(cx, ui, p, pos2(l + 54.5, t + 112.5), cp::RELEASE, 7, &seven([".1", ".2", ".4", ".6", ".8", "1.2", "A"]), false);
    text_at(p, l + 54.5, t + 140.0, Align2::CENTER_BOTTOM, "Ratio", font(FS), text());
    let rl = [("2", -16.5, 12.5, Align2::RIGHT_BOTTOM), ("4", -0.5, -16.5, Align2::CENTER_BOTTOM), ("10", 17.5, 12.5, Align2::LEFT_BOTTOM)];
    stepped(cx, ui, p, pos2(l + 54.5, t + 166.5), cp::RATIO, 3, &rl, false);
    // Needle display: gradient, scale, gain reduction.
    let d = Rect::from_min_max(pos2(l + 92.0, t + 24.0), pos2(l + 232.0, t + 102.0));
    let mut mesh = egui::Mesh::default();
    let (top_c, bot_c) = (gray(0x18), gray(0x23));
    let ym = d.top() + 34.0;
    mesh.add_colored_rect(Rect::from_min_max(d.min, pos2(d.right(), ym)), top_c);
    let i = mesh.vertices.len() as u32;
    for (pt, c) in [(pos2(d.left(), ym), top_c), (pos2(d.right(), ym), top_c), (d.right_bottom(), bot_c), (d.left_bottom(), bot_c)] {
        mesh.colored_vertex(pt, c);
    }
    mesh.add_triangle(i, i + 1, i + 2);
    mesh.add_triangle(i, i + 2, i + 3);
    p.add(Shape::mesh(mesh));
    p.rect_filled(Rect::from_min_max(pos2(d.left(), d.bottom()), pos2(d.right(), t + 181.0)), CornerRadius::same(2), gray(0x32));
    let pivot = pos2(l + 163.0, t + 110.0);
    let ang = |db: f32| (-48.0 + 4.8 * db) * PI / 180.0;
    for (db, lbl, pos) in [
        (0.0, "0", (111.0, 67.0)),
        (5.0, "5", (134.0, 49.0)),
        (10.0, "10", (161.5, 43.0)),
        (15.0, "15", (190.5, 49.0)),
        (20.0, "20", (215.5, 67.0)),
    ] {
        let a = ang(db);
        let dir = vec2(a.sin(), -a.cos());
        p.add(Shape::line(vec![pivot + dir * 56.0, pivot + dir * 64.0], Stroke::new(1.5, gray(0x3e))));
        text_at(p, l + pos.0, t + pos.1, Align2::CENTER_BOTTOM, lbl, font(FS), gray(0x86));
    }
    let tap = dev_tap(cx.dev.id);
    let gr = tap.get(0).clamp(0.0, 22.0);
    let a = ang(gr);
    let dir = vec2(a.sin(), -a.cos());
    p.with_clip_rect(d.shrink(1.0)).add(Shape::line(vec![pivot, pivot + dir * 64.0], Stroke::new(2.0, TEAL)));
    cx.app.animating = true;
    // Threshold / Makeup.
    knob_lv(
        cx,
        ui,
        p,
        l + 132.5,
        t + 120.0,
        cp::THRESHOLD,
        "Threshold",
        &format!("{} dB", sig3(g0.threshold)),
        0.0,
        KN_BIG,
        23.5,
        51.0,
        true,
    );
    knob_lv(cx, ui, p, l + 191.5, t + 120.0, cp::MAKEUP, "Makeup", &format!("{} dB", sig3(g0.makeup)), 0.0, KN_BIG, 23.5, 51.0, true);
    // Clip LED + Soft.
    let clip = tap.take(2, 0.0);
    let led_id = egui::Id::unique(("glue_clip", cx.dev.id));
    let now = ui.input(|i| i.time);
    if clip > 0.0 {
        ui.data_mut(|dd| dd.insert_temp(led_id, (now, clip)));
    }
    let (when, kind): (f64, f32) = ui.data(|dd| dd.get_temp(led_id)).unwrap_or((-10.0, 0.0));
    let lit = now - when < 0.25;
    let led = if !lit {
        gray(0x25)
    } else if kind >= 2.0 {
        Color32::from_rgb(0xff, 0xe0, 0x30)
    } else {
        Color32::from_rgb(0xff, 0x3a, 0x3a)
    };
    p.circle_filled(pos2(l + 252.0, t + 28.0), 3.0, led);
    text_at(p, l + 259.0, t + 32.0, Align2::LEFT_BOTTOM, "Clip", font(FS), text());
    if tbutton(cx, ui, p, Rect::from_min_size(pos2(l + 245.0, t + 37.0), vec2(37.0, 13.0)), cp::SOFT, "Soft", g0.soft_clip, true)
        && let FxParams::Glue(g) = &mut cx.params
    {
        g.soft_clip = !g.soft_clip;
    }
    quad(p, Rect::from_min_max(pos2(l + 241.0, t + 58.0), pos2(l + 286.0, t + 59.0)), gray(0x24));
    knob_lv(cx, ui, p, l + 263.5, t + 81.0, cp::RANGE, "Range", &db1(g0.range), 0.0, KN, 15.5, 36.0, true);
    knob_lv(cx, ui, p, l + 263.5, t + 142.0, cp::DRY_WET, "Dry/Wet", &pct(g0.dry_wet), 0.0, KN, 15.5, 36.0, true);
}

/// "cytomic" in the Glue title bar.
pub(super) fn cytomic(p: &Painter, right: f32, t: f32) {
    text_at(p, right - 51.0, t + 12.0, Align2::RIGHT_BOTTOM, "cytomic", bold(9.0), gray(0x2c));
}

// ----------------------------------------------------------------------------
// Limiter (284 px).
// ----------------------------------------------------------------------------

/// Limiter GR bar fill (darker than the orange edge, as the input bars).
const GR_FILL: Color32 = Color32::from_rgb(0xe6, 0xa5, 0x2e);

/// Limiter scale: dB below 0 → y (rows from the device top), Live's ticks.
const LIM_SCALE: [(f32, f32); 9] =
    [(0.0, 42.5), (1.0, 54.5), (2.0, 65.5), (3.0, 75.5), (4.0, 85.5), (5.0, 94.5), (6.0, 103.5), (12.0, 140.5), (24.0, 162.5)];

fn lim_y(t: f32, db_down: f32) -> f32 {
    t + mono_cubic(&LIM_SCALE, db_down.clamp(0.0, 24.0))
}

pub(super) fn limiter(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32) {
    let FxParams::Limiter(m0) = cx.params else { return };
    let mx = m0.maximize;
    // Input / Output.
    if mx {
        knob_lv(cx, ui, p, l + 33.5, t + 33.0, lp::OUTPUT, "Output", &db1(m0.output), 0.0, KN, 15.5, 36.0, true);
    } else {
        knob_lv(cx, ui, p, l + 33.5, t + 33.0, lp::INPUT, "Input", &db1(m0.input), 0.5, KN, 15.5, 36.0, true);
    }
    if tbutton(cx, ui, p, Rect::from_min_size(pos2(l + 5.0, t + 76.0), vec2(57.0, 13.0)), lp::MAXIMIZE, "Maximize", mx, true)
        && let FxParams::Limiter(m) = &mut cx.params
    {
        m.maximize = !m.maximize;
    }
    let rel = format!("{} ms", if m0.release < 99.95 { sig3(m0.release) } else { format!("{:.0}", m0.release) });
    knob_lv(cx, ui, p, l + 33.5, t + 127.0, lp::RELEASE, "Release", &rel, 0.0, KN, 15.5, 36.0, !m0.auto_release);
    if tbutton(cx, ui, p, Rect::from_min_size(pos2(l + 5.0, t + 170.0), vec2(57.0, 13.0)), lp::AUTO, "Auto", m0.auto_release, true)
        && let FxParams::Limiter(m) = &mut cx.params
    {
        m.auto_release = !m.auto_release;
    }
    // Display.
    let d = Rect::from_min_max(pos2(l + 67.0, t + 21.0), pos2(l + 217.0, t + 184.0));
    p.rect_filled(d, CornerRadius::same(2), gray(0x18));
    let key = if mx { lp::THRESHOLD } else { lp::CEILING };
    let accent = if mx { ORANGE } else { TEAL };
    text_at(p, l + 104.0, t + 35.0, Align2::CENTER_BOTTOM, if mx { "Threshold" } else { "Ceiling" }, font(FS), gray(0x80));
    let tap = dev_tap(cx.dev.id);
    // Held maximum gain reduction (click resets).
    let hold_id = egui::Id::unique(("lim_gr", cx.dev.id));
    let (gl, gr) = (tap.take(2, 0.0), tap.take(3, 0.0));
    let mut held: f32 = ui.data(|dd| dd.get_temp(hold_id)).unwrap_or(0.0);
    held = held.max(gl).max(gr);
    let grr = Rect::from_min_max(pos2(l + 160.0, t + 26.0), pos2(l + 202.0, t + 38.0));
    let gresp = cx.sense(ui, grr, lp::GR, Sense::click());
    if gresp.clicked() {
        held = 0.0;
    }
    ui.data_mut(|dd| dd.insert_temp(hold_id, held));
    if held > 0.005 {
        text_at(p, l + 181.0, t + 35.0, Align2::CENTER_BOTTOM, &format!("-{held:.2}"), font(FS), ORANGE);
    } else {
        text_at(p, l + 181.0, t + 35.0, Align2::CENTER_BOTTOM, "GR", font(FS), gray(0x80));
    }
    // Bars.
    let bar_top = t + 42.0;
    let bar_bot = t + 163.0;
    let bars = [(89.0, 103.0), (104.0, 118.0), (166.0, 180.0), (181.0, 195.0)];
    for &(a, b) in &bars {
        quad(p, Rect::from_min_max(pos2(l + a, bar_top), pos2(l + b, bar_bot)), gray(0x27));
    }
    // Input peaks (green, with a held peak line).
    let pk = [tap.take(0, 0.0), tap.take(1, 0.0)];
    let mid = egui::Id::unique(("lim_in", cx.dev.id));
    let now = ui.input(|i| i.time) as f32;
    let mut st: [(f32, f32, f32); 2] = ui.data(|dd| dd.get_temp(mid)).unwrap_or([(-60.0, -60.0, 0.0); 2]);
    for ch in 0..2 {
        let db = daw_engine::dynfx::lin_db(pk[ch]);
        // Fall 24 dB/s; hold 1 s.
        let (lvl, hold, ht) = &mut st[ch];
        *lvl = (*lvl - 0.4).max(db.max(-60.0));
        if db >= *hold || now - *ht > 1.0 {
            *hold = db.max(-60.0);
            *ht = now;
        }
        let (a, b) = bars[ch];
        let down = -*lvl;
        if down < 24.0 {
            let y = lim_y(t, down).floor();
            quad(p, Rect::from_min_max(pos2(l + a, y), pos2(l + b, bar_bot)), Color32::from_rgb(0x0d, 0xb0, 0x47));
        }
        if -*hold < 24.0 {
            let y = lim_y(t, -*hold).floor();
            quad(p, Rect::from_min_max(pos2(l + a, y), pos2(l + b, y + 1.0)), Color32::from_rgb(0x00, 0xf7, 0x58));
        }
    }
    ui.data_mut(|dd| dd.insert_temp(mid, st));
    // GR: orange line at the top and at the current depth.
    for (ch, g) in [gl, gr].into_iter().enumerate() {
        let (a, b) = bars[2 + ch];
        if g > 0.005 {
            // Filled from the top down to the reduction (like the input
            // bars fill from the bottom), bright edge lines on both ends.
            let y = lim_y(t, g).floor();
            quad(p, Rect::from_min_max(pos2(l + a, bar_top), pos2(l + b, y + 1.0)), GR_FILL);
            quad(p, Rect::from_min_max(pos2(l + a, bar_top), pos2(l + b, bar_top + 1.0)), ORANGE);
            quad(p, Rect::from_min_max(pos2(l + a, y), pos2(l + b, y + 1.0)), ORANGE);
        }
    }
    cx.app.animating = true;
    // Scale.
    let tick = gray(0x3e);
    for db in [0.0f32, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 12.0, 24.0] {
        let y = lim_y(t, db).floor();
        let major = matches!(db as i32, 0 | 3 | 6 | 12 | 24);
        let w = if major { 6.0 } else { 3.0 };
        quad(p, Rect::from_min_max(pos2(l + 120.0, y), pos2(l + 120.0 + w, y + 1.0)), tick);
        quad(p, Rect::from_min_max(pos2(l + 164.0 - w, y), pos2(l + 164.0, y + 1.0)), tick);
    }
    for (lbl, base) in [("0", 49.0), ("3", 79.0), ("6", 107.0), ("12", 144.0), ("24", 163.0)] {
        text_at(p, l + 142.0, t + base, Align2::CENTER_BOTTOM, lbl, font(FS), gray(0x80));
    }
    // Ceiling / threshold marker (draggable).
    let cdb = if mx { m0.threshold } else { m0.ceiling };
    let cy = lim_y(t, -cdb) - 0.5;
    p.add(Shape::convex_polygon(vec![pos2(l + 120.5, cy), pos2(l + 128.5, cy - 5.0), pos2(l + 128.5, cy + 5.0)], accent, Stroke::NONE));
    let mr = Rect::from_min_max(pos2(l + 85.0, bar_top - 4.0), pos2(l + 160.0, bar_bot + 4.0));
    let resp = cx.drag(ui, mr, key, true);
    let _ = resp;
    text_at(p, l + 104.0, t + 177.0, Align2::CENTER_BOTTOM, &db1(cdb), font(FS), accent);
    if mx {
        // Maximize: orange corner brackets around the threshold section.
        let br = Rect::from_min_max(pos2(l + 76.0, t + 24.0), pos2(l + 131.0, t + 180.0));
        let s = Stroke::new(1.0, ORANGE);
        for (c, dx, dy) in
            [(br.left_top(), 1.0, 1.0), (br.right_top(), -1.0, 1.0), (br.left_bottom(), 1.0, -1.0), (br.right_bottom(), -1.0, -1.0)]
        {
            let c = c + vec2(-0.5 * dx, -0.5 * dy) * -1.0;
            p.line_segment([c, c + vec2(4.0 * dx, 0.0)], s);
            p.line_segment([c, c + vec2(0.0, 4.0 * dy)], s);
        }
    }
    // Link.
    text_at(p, l + 154.0, t + 177.0, Align2::LEFT_BOTTOM, "Link", font(FS), gray(0x80));
    let lr = Rect::from_min_max(pos2(l + 177.0, t + 167.0), pos2(l + 212.0, t + 180.0));
    cx.drag(ui, lr, lp::LINK, true);
    text_at(p, l + 179.0, t + 177.0, Align2::LEFT_BOTTOM, &pct(m0.link), font(FS), TEAL);
    if cx.focused(lp::LINK) {
        brackets(p, lr);
    }
    // Lookahead.
    text_at(p, l + 251.5, t + 33.0, Align2::CENTER_BOTTOM, "Lookahead", font(FS), text());
    let ll = [("1.5", -16.5, 11.0, Align2::RIGHT_BOTTOM), ("3", 0.0, -19.0, Align2::CENTER_BOTTOM), ("6", 16.5, 11.0, Align2::LEFT_BOTTOM)];
    stepped(cx, ui, p, pos2(l + 251.5, t + 61.0), lp::LOOKAHEAD, LIM_LOOKAHEAD.len(), &ll, true);
    // Mode with its soft-clip LED.
    text_at(p, l + 251.0, t + 95.0, Align2::CENTER_BOTTOM, "Mode", font(FS), text());
    let clip = tap.take(4, 0.0) > 0.0;
    let led_id = egui::Id::unique(("lim_clip", cx.dev.id));
    let tnow = ui.input(|i| i.time);
    if clip {
        ui.data_mut(|dd| dd.insert_temp(led_id, tnow));
    }
    let when: f64 = ui.data(|dd| dd.get_temp(led_id)).unwrap_or(-10.0);
    let lit = m0.mode == LimMode::SoftClip && tnow - when < 0.2;
    if m0.mode == LimMode::SoftClip {
        p.circle_filled(pos2(l + 272.5, t + 92.0), 3.0, if lit { ORANGE } else { gray(0x24) });
    }
    for (i, (mode, name)) in
        [(LimMode::Standard, "Standard"), (LimMode::SoftClip, "Soft Clip"), (LimMode::TruePeak, "True Peak")].into_iter().enumerate()
    {
        let r = Rect::from_min_size(pos2(l + 222.0, t + 98.0 + 16.0 * i as f32), vec2(57.0, 13.0));
        cx.salt = i as u32;
        if tbutton(cx, ui, p, r, lp::MODE, name, m0.mode == mode, true)
            && let FxParams::Limiter(m) = &mut cx.params
        {
            m.mode = mode;
        }
        cx.salt = 0;
    }
    text_at(p, l + 251.0, t + 166.0, Align2::CENTER_BOTTOM, "Routing", font(FS), text());
    for (i, (ms_on, name)) in [(false, "L/R"), (true, "M/S")].into_iter().enumerate() {
        let r = Rect::from_min_size(pos2(l + 222.0 + 30.0 * i as f32, t + 170.0), vec2(27.0, 13.0));
        cx.salt = i as u32;
        if tbutton(cx, ui, p, r, lp::ROUTING, name, m0.mid_side == ms_on, true)
            && let FxParams::Limiter(m) = &mut cx.params
        {
            m.mid_side = ms_on;
        }
        cx.salt = 0;
    }
    let _ = (GLUE_ATTACKS, GLUE_RELEASES);
}

// ----------------------------------------------------------------------------
// Parameters: names, defaults, steps.
// ----------------------------------------------------------------------------

pub(super) fn name(p: &FxParams, k: u8) -> &'static str {
    let scn = match k {
        sc::ON => Some("S/C On"),
        sc::SOURCE => Some("S/C Audio From"),
        sc::TAP => Some("S/C Tap"),
        sc::GAIN => Some("S/C Gain"),
        sc::MIX => Some("S/C Mix"),
        sc::EQ_ON => Some("S/C EQ On"),
        sc::EQ => Some("S/C EQ Type"),
        sc::FREQ => Some("S/C EQ Freq"),
        sc::Q => Some("S/C EQ Q"),
        sc::EQ_GAIN => Some("S/C EQ Gain"),
        sc::LISTEN => Some("S/C Listen"),
        _ => None,
    };
    match p {
        FxParams::Gate(_) => scn.unwrap_or(match k {
            gp::THRESHOLD => "Threshold",
            gp::RETURN => "Return",
            gp::FLIP => "Flip",
            gp::LOOKAHEAD => "Lookahead",
            gp::ATTACK => "Attack",
            gp::HOLD => "Hold",
            gp::RELEASE => "Release",
            _ => "Floor",
        }),
        FxParams::Glue(_) => scn.unwrap_or(match k {
            cp::THRESHOLD => "Threshold",
            cp::RATIO => "Ratio",
            cp::ATTACK => "Attack",
            cp::RELEASE => "Release",
            cp::MAKEUP => "Makeup",
            cp::RANGE => "Range",
            cp::DRY_WET => "Dry/Wet",
            cp::SOFT => "Peak Clip In",
            _ => "Oversampling",
        }),
        _ => match k {
            lp::INPUT => "Gain",
            lp::OUTPUT => "Output",
            lp::MAXIMIZE => "Maximize",
            lp::CEILING => "Ceiling",
            lp::THRESHOLD => "Threshold",
            lp::RELEASE => "Release",
            lp::AUTO => "Auto Release",
            lp::LOOKAHEAD => "Lookahead",
            lp::MODE => "Mode",
            lp::ROUTING => "Stereo Mode",
            lp::LINK => "Link",
            _ => "Gain Reduction",
        },
    }
}

pub(super) fn sc_reset_pub(s: &mut Sidechain, k: u8) {
    sc_reset(s, k)
}

pub(super) fn sc_reset(s: &mut Sidechain, k: u8) {
    let d = Sidechain::default();
    match k {
        sc::ON => s.on = d.on,
        sc::TAP => s.tap = d.tap,
        sc::GAIN => s.gain = d.gain,
        sc::MIX => s.mix = d.mix,
        sc::EQ_ON => s.eq_on = d.eq_on,
        sc::EQ => s.eq = d.eq,
        sc::FREQ => s.freq = d.freq,
        sc::Q => s.q = d.q,
        sc::EQ_GAIN => s.eq_gain = d.eq_gain,
        sc::LISTEN => s.listen = false,
        _ => {}
    }
}

pub(super) fn reset(p: &mut FxParams, k: u8) {
    match p {
        FxParams::Gate(g) => {
            let d = GateParams::default();
            match k {
                gp::THRESHOLD => g.threshold = d.threshold,
                gp::RETURN => g.ret = d.ret,
                gp::FLIP => g.flip = false,
                gp::LOOKAHEAD => g.lookahead = d.lookahead,
                gp::ATTACK => g.attack = d.attack,
                gp::HOLD => g.hold = d.hold,
                gp::RELEASE => g.release = d.release,
                gp::FLOOR => g.floor = d.floor,
                _ => sc_reset(&mut g.sc, k),
            }
        }
        FxParams::Glue(g) => {
            let d = GlueParams::default();
            match k {
                cp::THRESHOLD => g.threshold = d.threshold,
                cp::RATIO => g.ratio = d.ratio,
                cp::ATTACK => g.attack = d.attack,
                cp::RELEASE => g.release = d.release,
                cp::MAKEUP => g.makeup = d.makeup,
                cp::RANGE => g.range = d.range,
                cp::DRY_WET => g.dry_wet = d.dry_wet,
                cp::SOFT => g.soft_clip = d.soft_clip,
                _ => sc_reset(&mut g.sc, k),
            }
        }
        FxParams::Limiter(m) => {
            let d = LimiterParams::default();
            match k {
                lp::INPUT => m.input = d.input,
                lp::OUTPUT => m.output = d.output,
                lp::CEILING => m.ceiling = d.ceiling,
                lp::THRESHOLD => m.threshold = d.threshold,
                lp::RELEASE => m.release = d.release,
                lp::LOOKAHEAD => m.lookahead = d.lookahead,
                lp::LINK => m.link = d.link,
                lp::MODE => m.mode = d.mode,
                lp::ROUTING => m.mid_side = false,
                lp::AUTO => m.auto_release = false,
                lp::MAXIMIZE => m.maximize = false,
                _ => {}
            }
        }
        _ => {}
    }
}

/// Arrow keys: discrete parameters step, continuous ones move 1/200.
pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    match (p, k) {
        (FxParams::Gate(g), gp::LOOKAHEAD) => g.lookahead = (g.lookahead as i32 - dir as i32).clamp(0, 2) as u8,
        (FxParams::Limiter(m), lp::MODE) => {
            let i = (m.mode as i32 - dir as i32).clamp(0, 2);
            m.mode = [LimMode::Standard, LimMode::SoftClip, LimMode::TruePeak][i as usize];
        }
        (p, k) => {
            if let Some(v) = norm(p, k) {
                let n = match (&*p, k) {
                    (FxParams::Glue(_), cp::RATIO) => 2.0,
                    (FxParams::Glue(_), cp::ATTACK | cp::RELEASE) => 6.0,
                    (FxParams::Limiter(_), lp::LOOKAHEAD) => 2.0,
                    _ => 200.0,
                };
                set_norm(p, k, v + dir / n);
            }
        }
    }
}
