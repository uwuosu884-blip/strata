//! EQ Three, Chorus-Ensemble, Auto Pan-Tremolo and Auto Filter (Live 12).
//! Geometry measured from Live 12 at 100 % (l/t = device left/top; text
//! positions are baselines).

use super::dynamics::{KN, KN_BIG, knob_lv, sig3};
use super::*;
use daw_engine::modfx::{
    AF_FREQ, AfWave, AutoFilterParams, AutoPanParams, CHO_TIMES, ChorusMode, ChorusParams, Circuit, EQ3_GAIN_MIN, EqThreeParams,
    FilterType, PanWave, Quantize, SYNC_DIVS, TimeMode,
};
use daw_project::devices::dev_tap;
use daw_project::devices::params::{afp, chp, e3p, pnp, sc};

pub const EQ3_W: f32 = 150.0;
pub const CHO_W: f32 = 245.0;
pub const PAN_W: f32 = 251.0;
pub const AF_W: f32 = 443.0;
/// Unfolded Auto Filter sidechain section (the strip stays).
pub const AF_SC_W: f32 = 138.0;

pub fn af_width(a: &AutoFilterParams) -> f32 {
    AF_W + if a.sc.shown { AF_SC_W } else { 0.0 }
}

/// Grey labels inside the dark displays.
const DLABEL: Color32 = Color32::from_rgb(0x84, 0x84, 0x84);
const PINK: Color32 = Color32::from_rgb(255, 105, 127);
const DISPLAY: u8 = 0x18;

// ----------------------------------------------------------------------------
// Shared helpers.
// ----------------------------------------------------------------------------

fn pct(x: f32) -> String {
    let v = x * 100.0;
    if v.abs() < 9.95 { format!("{v:.1} %") } else { format!("{v:.0} %") }
}

fn deg(x: f32) -> String {
    if x.abs() < 99.95 { format!("{x:.1}°") } else { format!("{x:.0}°") }
}

fn hz(f: f32) -> String {
    if f >= 1000.0 {
        format!("{} kHz", sig3(f / 1000.0))
    } else if f >= 100.0 {
        format!("{f:.0} Hz")
    } else {
        format!("{f:.1} Hz")
    }
}

#[allow(clippy::too_many_arguments)]
fn knob(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, x: f32, base: f32, k: u8, label: &str, value: &str, origin: f32, en: bool) {
    knob_lv(cx, ui, p, x, base, k, label, value, origin, KN, 15.5, 36.0, en);
}

fn display_bg(p: &Painter, r: Rect) {
    p.rect_filled(r, CornerRadius::same(3), gray(DISPLAY));
}

/// Orange display button: filled when on, outlined when off; greyed when
/// disabled. Returns the content colour.
fn obox(p: &Painter, r: Rect, on: bool, live: bool) -> Color32 {
    match (on, live) {
        (true, true) => {
            p.rect_filled(r, CornerRadius::ZERO, ORANGE);
            gray(0x20)
        }
        (false, true) => {
            p.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, ORANGE), egui::StrokeKind::Inside);
            ORANGE
        }
        (true, false) => {
            p.rect_filled(r, CornerRadius::ZERO, gray(0x5a));
            gray(0x24)
        }
        (false, false) => {
            p.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, gray(0x5a)), egui::StrokeKind::Inside);
            gray(0x5a)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn obutton(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect, k: u8, label: &str, on: bool, en: bool) -> bool {
    let col = obox(p, r, on, en && cx.on);
    text_at(p, r.center().x, r.top() + 10.0, Align2::CENTER_BOTTOM, label, font(FS), col);
    cx.toggle(ui, r, k, en)
}

/// Small ▾ after a display menu.
fn small_tri(p: &Painter, x: f32, y: f32, col: Color32) {
    p.add(Shape::convex_polygon(vec![pos2(x, y), pos2(x + 5.0, y), pos2(x + 2.5, y + 2.5)], col, Stroke::NONE));
}

/// Orange text menu in a display ("Rate ▾"): returns the picked index.
#[allow(clippy::too_many_arguments)]
fn text_menu(
    cx: &mut Ctx,
    ui: &mut egui::Ui,
    p: &Painter,
    r: Rect,
    k: u8,
    x: f32,
    base: f32,
    tri_x: Option<f32>,
    cur: usize,
    items: &[&str],
    en: bool,
) -> Option<usize> {
    let col = if en && cx.on { ORANGE } else { gray(0x5a) };
    let tr = text_at(p, x, base, Align2::LEFT_BOTTOM, items.get(cur).copied().unwrap_or(""), font(FS), col);
    small_tri(p, tri_x.unwrap_or(tr.right() + 4.0), base - 4.0, col);
    let resp = cx.sense(ui, r, k, Sense::click());
    cx.param_menu(&resp, k);
    if resp.clicked() {
        cx.focus(k);
    }
    let mut pick = None;
    if en {
        menu::dropdown_aligned(&resp, r.width().max(80.0), egui::RectAlign::BOTTOM_START, |ui| {
            for (i, it) in items.iter().enumerate() {
                if menu::check_item(ui, it, "", i == cur, true) {
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

/// Draggable number text in a display.
#[allow(clippy::too_many_arguments)]
fn drag_text(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, x: f32, base: f32, k: u8, s: &str, en: bool, col: Color32) {
    let col = if en && cx.on { col } else { gray(0x5a) };
    let r = text_at(p, x, base, Align2::LEFT_BOTTOM, s, font(FS), col);
    let hit = Rect::from_min_max(pos2(r.left() - 2.0, base - 10.0), pos2(r.right().max(r.left() + 24.0) + 2.0, base + 3.0));
    cx.drag(ui, hit, k, en);
    if cx.focused(k) {
        brackets(p, hit);
    }
}

/// Little waveform glyph in `r` (`f`: phase 0..1 → −1..1).
fn wave_icon(p: &Painter, r: Rect, col: Color32, f: impl Fn(f32) -> f32) {
    let n = 32;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let u = i as f32 / n as f32;
            pos2(r.left() + u * r.width(), r.center().y - f(u).clamp(-1.0, 1.0) * r.height() * 0.5)
        })
        .collect();
    p.add(Shape::line(pts, Stroke::new(1.2, col)));
}

/// Deterministic pseudo-random value (−1..1) for display cycle `i`.
fn hash(i: i64, salt: u64) -> f32 {
    let mut x = (i as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ salt;
    x ^= x >> 31;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 29;
    (x >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
}

/// Scrolling waveform polyline across `r`: `ppc` pixels per cycle, `ph`
/// the current phase, `f(cycle, phase)`.
#[allow(clippy::too_many_arguments)]
fn scope(p: &Painter, r: Rect, cy: f32, amp: f32, ppc: f32, ph: f64, col: Color32, f: impl Fn(i64, f32) -> f32) {
    let ppc = ppc.max(4.0);
    let n = (r.width() * 2.0) as usize;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let x = r.left() + i as f32 * 0.5;
            let c = ph + ((x - r.left()) / ppc) as f64;
            let ci = c.floor();
            pos2(x, cy - amp * f(ci as i64, (c - ci) as f32))
        })
        .collect();
    p.with_clip_rect(r).add(Shape::line(pts, Stroke::new(1.6, col)));
}

fn tap_get(id: u64, k: usize) -> f32 {
    dev_tap(id).get(k)
}

// ----------------------------------------------------------------------------
// EQ Three (150 px).
// ----------------------------------------------------------------------------

fn eq3_db(db: f32) -> String {
    if db <= EQ3_GAIN_MIN {
        "-inf dB".into()
    } else if db.abs() < 9.995 {
        format!("{db:.2} dB")
    } else {
        format!("{db:.1} dB")
    }
}

pub(super) fn eq_three(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect) {
    let FxParams::EqThree(e0) = cx.params else { return };
    let (l, t) = (r.left(), r.top());
    let tap = dev_tap(cx.dev.id);
    const LBL: [&str; 3] = ["GainLow", "GainMid", "GainHi"];
    const BTN: [&str; 3] = ["L", "M", "H"];
    let thr = 10f32.powf(-24.0 / 20.0);
    let mut lit_any = false;
    for b in 0..3 {
        let x = l + [28.5, 75.5, 122.5][b];
        // Signal LED (−24 dB threshold).
        let lv = tap.take(b, 0.0);
        let lit = cx.on && lv > thr;
        lit_any |= lit;
        let c = pos2(x, t + 25.5);
        p.circle_filled(c, 5.0, gray(0x19));
        p.circle_filled(c, 4.5, if lit { ORANGE } else { gray(DISPLAY) });
        knob(cx, ui, p, x, t + 43.0, e3p::GAIN[b], LBL[b], &eq3_db(e0.gain[b]), 0.0, true);
        let br = Rect::from_min_size(pos2(l + [14.0, 61.0, 108.0][b], t + 86.0), vec2(28.0, 13.0));
        toggle_look(p, br, BTN[b], e0.on[b], true, cx.on);
        if cx.toggle(ui, br, e3p::ON[b], true)
            && let FxParams::EqThree(e) = &mut cx.params
        {
            e.on[b] = !e.on[b];
        }
    }
    if lit_any || cx.on {
        cx.app.animating = true;
    }
    knob(cx, ui, p, l + 28.5, t + 143.0, e3p::FREQ_LO, "FreqLow", &hz(e0.freq_lo), 0.0, true);
    knob(cx, ui, p, l + 122.5, t + 143.0, e3p::FREQ_HI, "FreqHi", &hz(e0.freq_hi), 0.0, true);
    for (i, s48) in [false, true].into_iter().enumerate() {
        let br = Rect::from_min_size(pos2(l + 61.0, t + 140.0 + 16.0 * i as f32), vec2(28.0, 13.0));
        toggle_look(p, br, if s48 { "48" } else { "24" }, e0.slope48 == s48, true, cx.on);
        cx.salt = i as u32;
        if cx.toggle(ui, br, e3p::SLOPE, true)
            && let FxParams::EqThree(e) = &mut cx.params
        {
            e.slope48 = s48;
        }
        cx.salt = 0;
    }
}

// ----------------------------------------------------------------------------
// Chorus-Ensemble (245 px).
// ----------------------------------------------------------------------------

fn cho_time_label(i: usize) -> String {
    let t = CHO_TIMES[i.min(CHO_TIMES.len() - 1)];
    if t <= 0.0 { "Auto".into() } else { format!("{} ms", sig3(t).trim_end_matches(".00").trim_end_matches(".0")) }
}

pub(super) fn chorus(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect) {
    let FxParams::Chorus(c0) = cx.params else { return };
    let (l, t) = (r.left(), r.top());
    let on = cx.on;
    let vib = c0.mode == ChorusMode::Vibrato;
    // Display.
    let dr = Rect::from_min_max(pos2(l + 4.0, t + 21.0), pos2(l + 186.0, t + 130.0));
    display_bg(p, dr);
    let wr = Rect::from_min_max(pos2(l + 8.0, t + 30.0), pos2(l + 182.0, t + 76.0));
    let ph = tap_get(cx.dev.id, 0) as f64;
    // 54 px per second (0.90 Hz → 60 px per cycle).
    let ppc = 54.0 / c0.rate.max(0.01);
    let amp = 19.0 * c0.amount.clamp(0.0, 1.0);
    let cols = [TEAL, ORANGE, PINK];
    let c = c0;
    if on {
        if vib {
            let off = (c.offset / 360.0) as f64;
            if c.offset.abs() > 0.05 {
                scope(p, wr, t + 49.5, amp, ppc, ph + off, TEAL, |_, u| c.lfo(u));
            }
            scope(p, wr, t + 49.5, amp, ppc, ph, ORANGE, |_, u| c.lfo(u));
        } else {
            for (i, v) in c.voices().iter().enumerate().rev() {
                scope(p, wr, t + 49.5, amp, ppc, ph + *v as f64, cols[i], |_, u| c.lfo(u));
            }
        }
        cx.app.animating = true;
    }
    // Drag in the display: rate.
    cx.salt = 1;
    cx.drag(ui, wr, chp::RATE, true);
    cx.salt = 0;
    for (i, m) in ChorusMode::ALL.into_iter().enumerate() {
        let br = Rect::from_min_size(pos2(l + 8.0 + 59.0 * i as f32, t + 80.0), vec2(56.0, 13.0));
        cx.salt = i as u32;
        if obutton(cx, ui, p, br, chp::MODE, m.label(), c0.mode == m, true)
            && let FxParams::Chorus(c) = &mut cx.params
        {
            c.mode = m;
        }
        cx.salt = 0;
    }
    // HP filter.
    let cb = Rect::from_min_size(pos2(l + 11.0, t + 100.0), vec2(9.0, 9.0));
    let col = obox(p, cb, c0.hp_on, on);
    let _ = col;
    if cx.toggle(ui, cb.expand(2.0), chp::HP_ON, true)
        && let FxParams::Chorus(c) = &mut cx.params
    {
        c.hp_on = !c.hp_on;
    }
    let hc = if c0.hp_on && on { text() } else { gray(0x86) };
    p.add(Shape::line(
        vec![pos2(l + 25.5, t + 107.5), pos2(l + 25.5, t + 104.0), pos2(l + 27.5, t + 101.5), pos2(l + 38.5, t + 101.5)],
        Stroke::new(1.0, hc),
    ));
    drag_text(cx, ui, p, l + 11.0, t + 122.0, chp::HP_FREQ, &hz(c0.hp_freq), c0.hp_on, ORANGE);
    if vib {
        text_at(p, l + 60.0, t + 108.0, Align2::LEFT_BOTTOM, "Offset", font(FS), DLABEL);
        drag_text(cx, ui, p, l + 59.0, t + 122.0, chp::OFFSET, &deg(c0.offset), true, ORANGE);
        text_at(p, l + 105.0, t + 108.0, Align2::LEFT_BOTTOM, "Shape", font(FS), DLABEL);
        drag_text(cx, ui, p, l + 105.0, t + 122.0, chp::SHAPE, &pct(c0.shape), true, ORANGE);
    } else {
        text_at(p, l + 60.0, t + 108.0, Align2::LEFT_BOTTOM, "Width", font(FS), DLABEL);
        drag_text(cx, ui, p, l + 61.0, t + 122.0, chp::WIDTH, &pct(c0.width), true, ORANGE);
        let chorus = c0.mode == ChorusMode::Chorus;
        text_at(p, l + 106.0, t + 108.0, Align2::LEFT_BOTTOM, "Time", font(FS), if chorus { DLABEL } else { gray(0x5a) });
        let labels: Vec<String> = (0..CHO_TIMES.len()).map(cho_time_label).collect();
        let items: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
        let mr = Rect::from_min_max(pos2(l + 104.0, t + 112.0), pos2(l + 146.0, t + 125.0));
        if let Some(i) = text_menu(cx, ui, p, mr, chp::TIME, l + 106.0, t + 122.0, None, c0.time as usize, &items, chorus)
            && let FxParams::Chorus(c) = &mut cx.params
        {
            c.time = i as u8;
        }
        text_at(p, l + 153.0, t + 108.0, Align2::LEFT_BOTTOM, "Taps", font(FS), if chorus { DLABEL } else { gray(0x5a) });
        for (i, n) in [1u8, 2].into_iter().enumerate() {
            let br = Rect::from_min_size(pos2(l + 153.0 + 16.0 * i as f32, t + 112.0), vec2(13.0, 13.0));
            cx.salt = i as u32;
            if obutton(cx, ui, p, br, chp::TAPS, &n.to_string(), c0.taps == n, chorus)
                && let FxParams::Chorus(c) = &mut cx.params
            {
                c.taps = n;
            }
            cx.salt = 0;
        }
    }
    // Bottom row.
    knob(cx, ui, p, l + 32.5, t + 144.0, chp::RATE, "Rate", &format!("{} Hz", sig3(c0.rate)), 0.0, true);
    knob(cx, ui, p, l + 82.5, t + 144.0, chp::AMOUNT, "Amount", &pct(c0.amount), 0.0, true);
    knob(cx, ui, p, l + 134.5, t + 144.0, chp::FEEDBACK, "Feedback", &pct(c0.feedback), 0.0, !vib);
    quad(p, Rect::from_min_max(pos2(l + 152.0, t + 160.5), pos2(l + 158.0, t + 161.5)), gray(0x5a));
    let ir = Rect::from_min_size(pos2(l + 164.0, t + 152.0), vec2(15.0, 13.0));
    let (fill, col) = match (c0.fb_inv, !vib && on) {
        (true, true) => (ORANGE, gray(0x20)),
        (true, false) => (gray(0x5a), gray(0x24)),
        (false, true) => (gray(0x1e), text()),
        (false, false) => (gray(0x20), gray(0x55)),
    };
    boxed(p, ir, fill);
    text_at(p, ir.center().x, ir.top() + 10.0, Align2::CENTER_BOTTOM, "Ø", font(FS), col);
    if cx.toggle(ui, ir, chp::FB_INV, !vib)
        && let FxParams::Chorus(c) = &mut cx.params
    {
        c.fb_inv = !c.fb_inv;
    }
    quad(p, Rect::from_min_max(pos2(l + 185.0, t + 133.0), pos2(l + 186.0, t + 185.0)), gray(0x24));
    knob(cx, ui, p, l + 215.5, t + 31.0, chp::OUTPUT, "Output", &format!("{:.1} dB", c0.output), 0.0, true);
    knob(cx, ui, p, l + 215.5, t + 87.0, chp::WARMTH, "Warmth", &pct(c0.warmth), 0.0, true);
    knob(cx, ui, p, l + 215.5, t + 144.0, chp::DRY_WET, "Dry/Wet", &pct(c0.dry_wet), 0.0, !vib);
}

// ----------------------------------------------------------------------------
// Auto Pan-Tremolo (251 px).
// ----------------------------------------------------------------------------

fn pan_wave_icon(p: &Painter, r: Rect, w: PanWave, col: Color32) {
    let a = AutoPanParams { wave: w, ..Default::default() };
    wave_icon(p, r, col, |u| {
        let seg = (u * 3.0).floor() as i64;
        let rnd = (hash(seg, 7) * 0.9, hash(seg + 1, 7) * 0.9);
        let ph = if w.random() { (u * 3.0).fract() } else { u };
        a.wave_at(ph, rnd)
    });
}

pub(super) fn auto_pan(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect) {
    let FxParams::AutoPan(a0) = cx.params else { return };
    let (l, t) = (r.left(), r.top());
    let on = cx.on;
    let trem = a0.tremolo;
    let dr = Rect::from_min_max(pos2(l + 4.0, t + 21.0), pos2(l + 247.0, t + 130.0));
    display_bg(p, dr);
    for (i, (label, m)) in [("Panning", false), ("Tremolo", true)].into_iter().enumerate() {
        let br = Rect::from_min_size(pos2(l + 8.0 + 119.0 * i as f32, t + 25.0), vec2(116.0, 13.0));
        cx.salt = i as u32;
        if obutton(cx, ui, p, br, pnp::MODE, label, trem == m, true)
            && let FxParams::AutoPan(a) = &mut cx.params
        {
            a.tremolo = m;
        }
        cx.salt = 0;
    }
    // Waveforms (L teal / R orange; Tremolo one orange wave).
    let tap = dev_tap(cx.dev.id);
    let tempo = cx.app.project.tempo_bpm;
    let f = a0.rate.hz(tempo) as f32;
    let ppc = 66.7 / f.max(0.01);
    let amp = 23.0 * a0.amount.clamp(0.0, 1.0);
    let wr = Rect::from_min_max(pos2(l + 4.0, t + 42.0), pos2(l + 247.0, t + 94.0));
    let a = a0;
    let wave = move |salt: u64| {
        move |ci: i64, u: f32| {
            let rnd = (hash(ci, salt), hash(ci + 1, salt));
            a.wave_at(u, rnd)
        }
    };
    let ph = tap.get(0) as f64;
    if on {
        if trem {
            scope(p, wr, t + 67.5, amp, ppc, ph, ORANGE, wave(1));
        } else {
            let off = tap.get(5) as f64;
            scope(p, wr, t + 67.5, amp, ppc, ph, TEAL, wave(1));
            scope(p, wr, t + 67.5, amp, ppc, ph + off, ORANGE, wave(if a0.wave.random() { 2 } else { 1 }));
        }
        cx.app.animating = true;
    }
    cx.salt = 1;
    cx.drag(ui, wr, pnp::RATE, true);
    cx.salt = 0;
    // Position / amplitude bar.
    let (tl, tr) = (tap.get(3), tap.get(4));
    let seg_x = |i: usize| if i < 26 { l + 21.0 + 4.0 * i as f32 } else { l + 127.0 + 4.0 * (i - 26) as f32 };
    let centre = l + 125.0;
    let half = 104.0;
    for i in 0..52 {
        let x = seg_x(i);
        let xc = x + 1.5;
        let col = if trem {
            let d = ((xc - centre).abs() / half).min(1.0);
            let g = if on { tl.clamp(0.0, 1.0) } else { 0.0 };
            mix(gray(0x17), ORANGE, ((1.0 - d) * g).powf(1.3) * 0.95)
        } else {
            let pan = if on && tl + tr > 1e-6 { (tr - tl) / (tr + tl).max(1e-6) } else { 0.0 };
            let pos = centre + pan.clamp(-1.0, 1.0) * half;
            let k = if on { (1.0 - (xc - pos).abs() / 26.0).max(0.0) } else { 0.0 };
            if i < 26 { mix(Color32::from_rgb(0x14, 0x3d, 0x41), TEAL, k) } else { mix(Color32::from_rgb(0x4a, 0x38, 0x26), ORANGE, k) }
        };
        quad(p, Rect::from_min_size(pos2(x, t + 99.0), vec2(3.0, 7.0)), col);
    }
    if !trem {
        text_at(p, l + 11.0, t + 106.0, Align2::LEFT_BOTTOM, "L", font(FS), if on { TEAL } else { gray(0x5a) });
        text_at(p, l + 241.0, t + 106.0, Align2::RIGHT_BOTTOM, "R", font(FS), if on { ORANGE } else { gray(0x5a) });
    }
    // Bottom row of the display.
    if trem {
        for (i, (k, v)) in [(pnp::HARMONIC, a0.harmonic), (pnp::VINTAGE, a0.vintage)].into_iter().enumerate() {
            let br = Rect::from_min_size(pos2(l + 8.0 + 26.0 * i as f32, t + 113.0), vec2(21.0, 13.0));
            let col = obox(p, br, v, on);
            let ir = br.shrink2(vec2(4.0, 3.5));
            if i == 0 {
                // Harmonic: crossing bands.
                p.add(Shape::line(vec![ir.left_top(), pos2(ir.center().x, ir.center().y), ir.right_top()], Stroke::new(1.0, col)));
                p.add(Shape::line(vec![ir.left_bottom(), pos2(ir.center().x, ir.center().y), ir.right_bottom()], Stroke::new(1.0, col)));
            } else {
                // Vintage: soft S-curve.
                wave_icon(p, ir, col, |u| ((u - 0.5) * 6.0).tanh());
            }
            if cx.toggle(ui, br, k, true)
                && let FxParams::AutoPan(a) = &mut cx.params
            {
                if i == 0 {
                    a.harmonic = !a.harmonic;
                } else {
                    a.vintage = !a.vintage;
                }
            }
        }
    } else {
        let cr = Rect::from_min_size(pos2(l + 8.0, t + 113.0), vec2(47.0, 13.0));
        let (fill, col) = if on { (ORANGE, gray(0x20)) } else { (gray(0x5a), gray(0x24)) };
        p.rect_filled(cr, CornerRadius::ZERO, fill);
        pan_wave_icon(
            p,
            Rect::from_min_max(pos2(cr.left() + 4.0, cr.top() + 3.0), pos2(cr.left() + 24.0, cr.bottom() - 3.0)),
            a0.wave,
            col,
        );
        p.add(Shape::convex_polygon(
            vec![pos2(cr.right() - 10.0, cr.top() + 4.0), pos2(cr.right() - 3.0, cr.top() + 4.0), pos2(cr.right() - 6.5, cr.top() + 8.5)],
            col,
            Stroke::NONE,
        ));
        let resp = cx.sense(ui, cr, pnp::WAVE, Sense::click());
        cx.param_menu(&resp, pnp::WAVE);
        if resp.clicked() {
            cx.focus(pnp::WAVE);
        }
        let mut pick = None;
        menu::dropdown_aligned(&resp, 90.0, egui::RectAlign::BOTTOM_START, |ui| {
            for w in PanWave::ALL {
                if menu::check_item(ui, w.label(), "", w == a0.wave, true) {
                    pick = Some(w);
                }
            }
        });
        if let Some(w) = pick {
            cx.undo_step();
            if let FxParams::AutoPan(a) = &mut cx.params {
                a.wave = w;
            }
        }
    }
    let modes: Vec<&str> = TimeMode::ALL.iter().map(|m| m.label()).collect();
    let cur = TimeMode::ALL.iter().position(|&m| m == a0.rate.mode).unwrap_or(0);
    let mr = Rect::from_min_max(pos2(l + 70.0, t + 113.0), pos2(l + 119.0, t + 126.0));
    if let Some(i) = text_menu(cx, ui, p, mr, pnp::TIME_MODE, l + 74.0, t + 123.0, Some(l + 112.0), cur, &modes, true)
        && let FxParams::AutoPan(a) = &mut cx.params
    {
        a.rate.mode = TimeMode::ALL[i];
    }
    // Stereo mode (Ø = Phase, else Spin) and its amount; Panning only.
    let stereo_en = !trem;
    let sr = Rect::from_min_size(pos2(l + 129.0, t + 113.0), vec2(13.0, 13.0));
    let col = obox(p, sr, !a0.spin_mode, stereo_en && on);
    let col = if a0.spin_mode && !(stereo_en && on) { gray(0x5a) } else { col };
    text_at(p, sr.center().x, sr.top() + 10.0, Align2::CENTER_BOTTOM, if a0.spin_mode { "S" } else { "Ø" }, font(FS), col);
    if cx.toggle(ui, sr, pnp::STEREO, stereo_en && !a0.wave.random())
        && let FxParams::AutoPan(a) = &mut cx.params
    {
        a.spin_mode = !a.spin_mode;
    }
    let sv = if a0.spin_mode { pct(a0.spin) } else { deg(a0.phase) };
    drag_text(cx, ui, p, l + 149.0, t + 123.0, pnp::PHASE, &sv, stereo_en, ORANGE);
    // Invert.
    let ir = Rect::from_min_size(pos2(l + 176.0, t + 113.0), vec2(13.0, 13.0));
    let col = obox(p, ir, a0.invert, on);
    let s = Stroke::new(1.0, col);
    let (ax, bx, y0, y1) = (ir.left() + 4.5, ir.right() - 4.5, ir.top() + 3.0, ir.bottom() - 3.0);
    p.line_segment([pos2(ax, y0), pos2(ax, y1)], s);
    p.line_segment([pos2(bx, y0), pos2(bx, y1)], s);
    p.add(Shape::line(vec![pos2(ax - 1.5, y0 + 1.5), pos2(ax, y0), pos2(ax + 1.5, y0 + 1.5)], s));
    p.add(Shape::line(vec![pos2(bx - 1.5, y1 - 1.5), pos2(bx, y1), pos2(bx + 1.5, y1 - 1.5)], s));
    if cx.toggle(ui, ir, pnp::INVERT, true)
        && let FxParams::AutoPan(a) = &mut cx.params
    {
        a.invert = !a.invert;
    }
    // Phase offset (synced modes; stereo width for the random waves).
    let off_en = a0.rate.mode.synced() || a0.wave.random();
    let oc = if off_en && on { DLABEL } else { gray(0x5a) };
    offset_glyph(p, pos2(l + 203.0, t + 123.0), oc);
    drag_text(cx, ui, p, l + 214.0, t + 123.0, pnp::OFFSET, &deg(a0.offset), off_en, gray(0xb0));
    // Knobs.
    knob(cx, ui, p, l + 31.5, t + 144.0, pnp::AMOUNT, "Amount", &pct(a0.amount), 0.0, true);
    knob(cx, ui, p, l + 95.5, t + 144.0, pnp::RATE, a0.rate.label(), &a0.rate.text(), 0.0, true);
    let (shape, origin) = if trem { (a0.shape_trem, 0.5) } else { (a0.shape_pan, 0.0) };
    knob(cx, ui, p, l + 159.5, t + 144.0, pnp::SHAPE, "Shape", &pct(shape), origin, true);
    text_at(p, l + 220.0, t + 144.0, Align2::CENTER_BOTTOM, "Att | Mod", font(FS), text());
    let att = format!("{} ms", sig3(a0.attack));
    value_box(cx, ui, p, Rect::from_min_size(pos2(l + 198.0, t + 149.0), vec2(45.0, 13.0)), pnp::ATTACK, &att, true);
    let bg = box_fill(on);
    value_box_ex(
        cx,
        ui,
        p,
        Rect::from_min_size(pos2(l + 198.0, t + 167.0), vec2(45.0, 13.0)),
        pnp::DYN_MOD,
        &pct(a0.dyn_mod),
        true,
        false,
        bg,
    );
}

/// Live's phase-offset glyph (a sine with a tick).
fn offset_glyph(p: &Painter, base: Pos2, col: Color32) {
    let r = Rect::from_min_size(pos2(base.x, base.y - 7.0), vec2(8.0, 7.0));
    wave_icon(p, r, col, |u| -(u * std::f32::consts::TAU).sin());
}

// ----------------------------------------------------------------------------
// Auto Filter (443 px + sidechain).
// ----------------------------------------------------------------------------

/// Display x of `f` (log 20 Hz – 20 kHz, 98 px per decade).
fn af_x(dl: f32, f: f32) -> f32 {
    dl + 0.5 + 98.0 * (f.max(1.0) / 20.0).log10()
}

fn af_f(dl: f32, x: f32) -> f32 {
    20.0 * 10f32.powf((x - dl - 0.5) / 98.0)
}

fn filter_icon(p: &Painter, r: Rect, kind: FilterType, col: Color32) {
    let a = AutoFilterParams { kind, res: 0.35, morph: 0.5, ..Default::default() };
    let fc = if kind == FilterType::Dj { 1000.0 } else { 900.0 };
    let n = 28;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let u = i as f32 / n as f32;
            let f = 60.0 * 250f32.powf(u);
            let db = if kind == FilterType::Resampling {
                if (u * 4.0).fract() < 0.5 { 0.0 } else { -12.0 }
            } else {
                a.response_db(fc, f).clamp(-20.0, 6.0)
            };
            pos2(r.left() + u * r.width(), r.top() + 2.0 + (-db).max(-2.0) / 20.0 * (r.height() - 2.0))
        })
        .collect();
    p.with_clip_rect(r.expand(1.0)).add(Shape::line(pts, Stroke::new(1.0, col)));
}

fn af_wave_icon(p: &Painter, r: Rect, w: AfWave, col: Color32) {
    wave_icon(p, r, col, |u| match w {
        AfWave::Wander => (u * 9.0).sin() * 0.6 + (u * 3.7).cos() * 0.4,
        AfWave::SampleHold => hash((u * 4.0).floor() as i64, 3),
        _ => w.at(u, 0.0),
    });
}

pub(super) fn auto_filter(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect) {
    let FxParams::AutoFilter(a0) = cx.params else { return };
    let (l0, t) = (r.left(), r.top());
    let on = cx.on;
    // Body: #46 with #36 panels.
    p.rect_filled(Rect::from_min_max(pos2(l0, t + 17.0), r.max), CornerRadius { nw: 0, ne: 0, sw: 3, se: 3 }, gray(0x46));
    let pnl = |x0: f32, y0: f32, x1: f32, y1: f32, c: Color32| {
        p.rect_filled(Rect::from_min_max(pos2(x0, t + y0), pos2(x1, t + y1)), CornerRadius::same(2), c);
    };
    // Sidechain strip (and section).
    let ox = if a0.sc.shown { AF_SC_W } else { 0.0 };
    pnl(l0 + 2.0, 17.0, l0 + 21.0 + if a0.sc.shown { AF_SC_W - 2.0 } else { 0.0 }, 186.0, panel());
    dynamics::sc_strip(cx, ui, p, l0 + 1.5, t, false);
    if a0.sc.shown {
        dynamics::af_sc_section(cx, ui, p, l0, t);
    }
    let l = l0 + ox;
    let env_view = a0.env_view;
    let sel = gray(0x46);
    pnl(l + 23.0, 17.0, l + 82.0, 186.0, panel());
    pnl(l + 84.0, 128.0, l + 297.0, 186.0, if env_view { panel() } else { sel });
    pnl(l + 300.0, 128.0, l + 379.0, 186.0, if env_view { sel } else { panel() });
    pnl(l + 383.0, 17.0, l + 440.0, 186.0, panel());
    // Section selection (LFO / Envelope): clicks on the panels.
    for (i, (x0, x1, v)) in [(84.0, 297.0, false), (300.0, 379.0, true)].into_iter().enumerate() {
        let pr = Rect::from_min_max(pos2(l + x0, t + 128.0), pos2(l + x1, t + 186.0));
        if ui.interact(pr, egui::Id::unique(("af_sect", cx.dev.id, i)), Sense::click()).clicked() && env_view != v {
            cx.undo_step();
            if let FxParams::AutoFilter(a) = &mut cx.params {
                a.env_view = v;
            }
        }
    }

    // --- Display. ---
    let dl = l + 84.0;
    let dr = Rect::from_min_max(pos2(dl, t + 17.0), pos2(l + 380.0, t + 127.0));
    p.rect_filled(dr, CornerRadius::same(2), gray(DISPLAY));
    let gr = Rect::from_min_max(dr.min, pos2(dr.right(), t + 105.0));
    let gp = p.with_clip_rect(gr.intersect(p.clip_rect()));
    let id = cx.dev.id;
    // Output spectrum.
    if on {
        let sr = cx.app.spectra.sample_rate(id).max(8000.0);
        let spec = cx.app.spectra.update(id, 4096, 30.0, 1.0);
        let live = spec.live;
        let cols = gr.width() as usize + 1;
        let db = crate::spectrum::curve_db(spec, sr, cols, |i| af_f(dl, dl + i));
        let mut mesh = egui::Mesh::default();
        let col = gray(0x51);
        let bottom = gr.bottom();
        for (i, db) in db.iter().enumerate() {
            let x = dl + i as f32;
            let y = (bottom - (db + 84.0) / 84.0 * (bottom - t - 30.0)).clamp(gr.top(), bottom);
            let v = mesh.vertices.len() as u32;
            mesh.colored_vertex(pos2(x, y), col);
            mesh.colored_vertex(pos2(x, bottom), col);
            if i > 0 {
                mesh.add_triangle(v - 2, v - 1, v);
                mesh.add_triangle(v - 1, v, v + 1);
            }
        }
        gp.add(Shape::mesh(mesh));
        cx.app.animating |= live;
    } else {
        cx.app.spectra.clear(id);
    }
    // Grid: 100 Hz / 1 kHz / 10 kHz and 0 dB.
    for f in [100.0, 1000.0, 10_000.0] {
        let x = af_x(dl, f).floor();
        gp.rect_filled(Rect::from_min_max(pos2(x, gr.top()), pos2(x + 1.0, gr.bottom())), CornerRadius::ZERO, gray(0x3f));
    }
    let y0db = t + 61.0;
    gp.rect_filled(Rect::from_min_max(pos2(gr.left(), y0db), pos2(gr.right(), y0db + 1.0)), CornerRadius::ZERO, gray(0x3f));
    // Curves: modulated L / R (dim), then the set curve.
    let fy = |db: f32| y0db + 0.5 - db * 1.1;
    let curve = |fc: f32, col: Color32, w: f32| {
        let n = (gr.width() * 2.0) as usize;
        let pts: Vec<Pos2> = (0..=n)
            .map(|i| {
                let x = dl + i as f32 * 0.5;
                pos2(x, fy(a0.response_db(fc, af_f(dl, x))).min(gr.bottom() + 4.0))
            })
            .collect();
        gp.add(Shape::line(pts, Stroke::new(w, col)));
    };
    let fc = a0.cutoff();
    let modulating = on && (a0.lfo_amt > 0.0 || a0.env_amt.abs() > 0.0 || a0.sc.on);
    if modulating {
        let tap = dev_tap(id);
        let (ml, mr) = (tap.get(0), tap.get(1));
        if ml > 0.0 {
            if (mr - ml).abs() > 0.5 {
                curve(mr.clamp(AF_FREQ.0, AF_FREQ.1), Color32::from_rgba_unmultiplied(0xc8, 0x9a, 0x50, 170), 1.6);
            }
            curve(ml.clamp(AF_FREQ.0, AF_FREQ.1), Color32::from_rgba_unmultiplied(3, 195, 213, 150), 1.6);
        }
        cx.app.animating = true;
    }
    curve(fc, if on { TEAL } else { gray(0x75) }, 2.0);
    // Handle: x = cutoff, y = resonance.
    let hx = af_x(dl, fc).clamp(dr.left() + 6.0, dr.right() - 6.0);
    let hy = t + 98.0 - a0.res.clamp(0.0, 1.0) * 70.0;
    gp.circle_stroke(pos2(hx, hy), 6.5, Stroke::new(2.0, if on { ORANGE } else { gray(0x75) }));
    cx.salt = 1;
    let resp = cx.sense(ui, gr, afp::FREQ, Sense::click_and_drag());
    cx.salt = 0;
    if resp.drag_started() || resp.clicked() {
        cx.focus(afp::FREQ);
    }
    let d = super::super::widgets::endless_drag(ui, &resp);
    if d != egui::Vec2::ZERO {
        cx.undo_step();
        if let Some(v) = norm(&cx.params, afp::FREQ) {
            set_norm(&mut cx.params, afp::FREQ, (v + d.x / gr.width()).clamp(0.0, 1.0));
        }
        if a0.kind != FilterType::Resampling
            && let Some(v) = norm(&cx.params, afp::RES)
        {
            set_norm(&mut cx.params, afp::RES, (v - d.y / 70.0).clamp(0.0, 1.0));
        }
    }
    cx.param_menu(&resp, afp::FREQ);
    // Separator and the parameter row.
    quad(p, Rect::from_min_max(pos2(dl, t + 105.0), pos2(dr.right(), t + 106.0)), gray(0x3e));
    let rb = t + 120.0;
    if env_view {
        af_env_row(cx, ui, p, dl, rb);
    } else {
        af_lfo_row(cx, ui, p, l, rb);
    }

    // --- Left column: Freq, type, slope, Res (/ Morph). ---
    let (flabel, fval, forigin) = match a0.kind {
        FilterType::Dj => ("Control", format!("{:.0} %", a0.dj * 100.0), 0.5),
        FilterType::Vowel => ("Pitch", format!("{:.1} st", a0.pitch), 0.5),
        _ => ("Freq", hz(a0.freq), 0.0),
    };
    knob_lv(cx, ui, p, l + 52.5, t + 30.0, afp::FREQ, flabel, &fval, forigin, KN_BIG, 22.5, 50.0, true);
    let tr = Rect::from_min_size(pos2(l + 27.0, t + 94.0), vec2(51.0, 13.0));
    let items: Vec<&str> = FilterType::ALL.iter().map(|k| k.label()).collect();
    if let Some(i) = chooser(cx, ui, p, tr, afp::TYPE, "", &items, true)
        && let FxParams::AutoFilter(a) = &mut cx.params
    {
        a.kind = FilterType::ALL[i];
        a.slope = a.slope_ok();
    }
    filter_icon(
        p,
        Rect::from_min_max(pos2(tr.left() + 4.0, tr.top() + 3.0), pos2(tr.left() + 24.0, tr.bottom() - 3.0)),
        a0.kind,
        if on { text() } else { dim() },
    );
    let slopes = a0.kind.slopes();
    let n = slopes.len();
    if n > 0 {
        let bw = if n == 2 { 24.0 } else { 12.0 };
        let gap = if n == 2 { 3.0 } else { 1.0 };
        for (i, &s) in slopes.iter().enumerate() {
            let br = Rect::from_min_size(pos2(l + 27.0 + (bw + gap) * i as f32, t + 110.0), vec2(bw, 13.0));
            let label = s.to_string();
            toggle_look(p, br, &label, a0.slope_ok() == s, true, on);
            cx.salt = i as u32;
            if cx.toggle(ui, br, afp::SLOPE, true)
                && let FxParams::AutoFilter(a) = &mut cx.params
            {
                a.slope = s;
            }
            cx.salt = 0;
        }
    }
    let res_en = a0.kind != FilterType::Resampling;
    let res_label = if a0.kind == FilterType::Vowel { "Formant" } else { "Res" };
    let res_val = if a0.kind == FilterType::Vowel {
        ["a", "e", "i", "o", "u"][((a0.res.clamp(0.0, 1.0) * 4.0).round() as usize).min(4)].to_string()
    } else {
        pct(a0.res)
    };
    if a0.kind.has_morph() {
        text_at(p, l + 52.5, t + 137.0, Align2::CENTER_BOTTOM, res_label, font(FS), if res_en { text() } else { dim() });
        value_box(cx, ui, p, Rect::from_min_size(pos2(l + 27.0, t + 139.0), vec2(51.0, 13.0)), afp::RES, &res_val, res_en);
        text_at(p, l + 52.5, t + 166.0, Align2::CENTER_BOTTOM, "Morph", font(FS), text());
        value_box(cx, ui, p, Rect::from_min_size(pos2(l + 27.0, t + 169.0), vec2(51.0, 13.0)), afp::MORPH, &pct(a0.morph), true);
    } else {
        knob(cx, ui, p, l + 52.5, t + 143.0, afp::RES, res_label, &res_val, 0.0, res_en);
    }

    // --- LFO section. ---
    knob(cx, ui, p, l + 117.5, t + 143.0, afp::LFO_AMT, "LFO Amt", &pct(a0.lfo_amt), 0.0, true);
    knob(cx, ui, p, l + 171.5, t + 143.0, afp::RATE, "Rate", &a0.rate.text(), 0.0, true);
    for (i, s) in ["Wave", if a0.wave == AfWave::SampleHold { "Smooth" } else { "Morph" }, "Mode"].into_iter().enumerate() {
        text_at(p, l + 202.0, t + 143.0 + 18.0 * i as f32, Align2::LEFT_BOTTOM, s, font(FS), text());
    }
    let wr = Rect::from_min_size(pos2(l + 242.0, t + 133.0), vec2(51.0, 13.0));
    let items: Vec<&str> = AfWave::ALL.iter().map(|w| w.label()).collect();
    if let Some(i) = chooser(cx, ui, p, wr, afp::WAVE, "", &items, true)
        && let FxParams::AutoFilter(a) = &mut cx.params
    {
        a.wave = AfWave::ALL[i];
        if a.wave == AfWave::SampleHold {
            a.lfo_morph = a.lfo_morph.max(0.0);
        }
    }
    af_wave_icon(
        p,
        Rect::from_min_max(pos2(wr.left() + 4.0, wr.top() + 3.0), pos2(wr.left() + 24.0, wr.bottom() - 3.0)),
        a0.wave,
        if on { text() } else { dim() },
    );
    let mv = if a0.wave == AfWave::SampleHold { pct(a0.lfo_morph.max(0.0)) } else { pct(a0.lfo_morph) };
    let bg = box_fill(on);
    value_box_ex(
        cx,
        ui,
        p,
        Rect::from_min_size(pos2(l + 242.0, t + 151.0), vec2(51.0, 13.0)),
        afp::LFO_MORPH,
        &mv,
        a0.wave != AfWave::Wander,
        a0.wave == AfWave::SampleHold,
        bg,
    );
    let modes: Vec<&str> = TimeMode::ALL.iter().map(|m| m.label()).collect();
    let cur = a0.rate.mode.label();
    if let Some(i) =
        chooser(cx, ui, p, Rect::from_min_size(pos2(l + 242.0, t + 169.0), vec2(51.0, 13.0)), afp::TIME_MODE, cur, &modes, true)
        && let FxParams::AutoFilter(a) = &mut cx.params
    {
        a.rate.mode = TimeMode::ALL[i];
    }

    // --- Envelope section. ---
    knob(cx, ui, p, l + 339.5, t + 143.0, afp::ENV_AMT, "Envelope", &pct(a0.env_amt), 0.5, true);

    // --- Right column: Drive, circuit, Clip, Output, Dry/Wet. ---
    knob(cx, ui, p, l + 411.5, t + 30.0, afp::DRIVE, "Drive", &pct(a0.drive), 0.0, true);
    let cr = Rect::from_min_size(pos2(l + 386.0, t + 72.0), vec2(51.0, 13.0));
    let items: Vec<&str> = Circuit::ALL.iter().map(|c| c.label()).collect();
    if let Some(i) = chooser(cx, ui, p, cr, afp::CIRCUIT, a0.circuit.label(), &items, a0.kind.has_circuit())
        && let FxParams::AutoFilter(a) = &mut cx.params
    {
        a.circuit = Circuit::ALL[i];
    }
    let clr = Rect::from_min_size(pos2(l + 386.0, t + 94.0), vec2(40.0, 13.0));
    toggle_look(p, clr, "Clip", a0.clip, true, on);
    if cx.toggle(ui, clr, afp::CLIP, true)
        && let FxParams::AutoFilter(a) = &mut cx.params
    {
        a.clip = !a.clip;
    }
    // Clip LED: flashes while the soft clipper works.
    let led = dev_tap(id).take(2, 0.0);
    let lit = on && a0.clip && led > 1.0;
    p.circle_filled(pos2(l + 433.5, t + 102.0), 3.5, if lit { Color32::from_rgb(0xff, 0x4a, 0x3a) } else { gray(0x24) });
    // Output: teal number box.
    let orr = Rect::from_min_size(pos2(l + 386.0, t + 110.0), vec2(51.0, 13.0));
    boxed(p, orr, if on { BOX_TEAL } else { gray(0x55) });
    text_at(p, orr.center().x, orr.top() + 10.0, Align2::CENTER_BOTTOM, &format!("{:.1} dB", a0.output), font(FS), text());
    if cx.focused(afp::OUTPUT) {
        p.rect_stroke(orr.expand(1.0), CornerRadius::ZERO, Stroke::new(1.0, gray(0x91)), egui::StrokeKind::Inside);
    }
    cx.drag(ui, orr, afp::OUTPUT, true);
    knob(cx, ui, p, l + 411.5, t + 143.0, afp::DRY_WET, "Dry/Wet", &pct(a0.dry_wet), 0.0, true);
}

/// LFO parameter row of the display.
fn af_lfo_row(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, base: f32) {
    let FxParams::AutoFilter(a0) = cx.params else { return };
    let on = cx.on;
    text_at(p, l + 91.0, base, Align2::LEFT_BOTTOM, "LFO", bold(FS), text());
    // Phase / Spin.
    let random = a0.wave.random();
    let br = Rect::from_min_max(pos2(l + 123.0, base - 10.0), pos2(l + 158.0, base + 3.0));
    let col = obox(p, br, true, on);
    text_at(p, br.center().x, br.top() + 10.0, Align2::CENTER_BOTTOM, if a0.spin_mode { "Spin" } else { "Phase" }, font(FS), col);
    if cx.toggle(ui, br, afp::STEREO, !random)
        && let FxParams::AutoFilter(a) = &mut cx.params
    {
        a.spin_mode = !a.spin_mode;
    }
    text_at(p, l + 174.0, base, Align2::LEFT_BOTTOM, "Ø", font(FS), DLABEL);
    let sv = if a0.spin_mode { pct(a0.spin) } else { deg(a0.phase) };
    drag_text(cx, ui, p, l + 187.0, base, afp::PHASE, &sv, !(random && a0.spin_mode), ORANGE);
    // Phase offset (sync modes).
    let off_en = a0.rate.mode.synced();
    offset_glyph(p, pos2(l + 229.0, base), if off_en && on { DLABEL } else { gray(0x5a) });
    drag_text(cx, ui, p, l + 240.0, base, afp::OFFSET, &deg(a0.offset), off_en, ORANGE);
    // Quantize.
    let qc = DLABEL;
    let qx = l + 286.0;
    p.add(Shape::line(
        vec![pos2(qx, base), pos2(qx + 3.0, base - 8.0), pos2(qx + 5.0, base - 8.0), pos2(qx + 8.0, base), pos2(qx, base)],
        Stroke::new(1.0, qc),
    ));
    p.line_segment([pos2(qx + 4.0, base - 2.0), pos2(qx + 8.0, base - 7.0)], Stroke::new(1.0, qc));
    let items: Vec<&str> = Quantize::ALL.iter().map(|q| q.label()).collect();
    let cur = Quantize::ALL.iter().position(|&q| q == a0.quantize).unwrap_or(0);
    let mr = Rect::from_min_max(pos2(l + 300.0, base - 10.0), pos2(l + 340.0, base + 3.0));
    if let Some(i) = text_menu(cx, ui, p, mr, afp::QUANT, l + 302.0, base, Some(l + 332.0), cur, &items, true)
        && let FxParams::AutoFilter(a) = &mut cx.params
    {
        a.quantize = Quantize::ALL[i];
    }
    let sv = match a0.quantize {
        Quantize::SampleHold => SYNC_DIVS[(a0.q_div as usize).min(SYNC_DIVS.len() - 1)].1.to_string(),
        _ => a0.steps.to_string(),
    };
    let en = a0.quantize != Quantize::None;
    drag_text(cx, ui, p, l + 348.0, base, afp::STEPS, &sv, en, if en { ORANGE } else { DLABEL });
}

/// Envelope parameter row (Live 12 manual layout, our colours).
fn af_env_row(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, dl: f32, base: f32) {
    let FxParams::AutoFilter(a0) = cx.params else { return };
    let on = cx.on;
    text_at(p, dl + 7.0, base, Align2::LEFT_BOTTOM, "Envelope", bold(FS), text());
    text_at(p, dl + 58.0, base, Align2::LEFT_BOTTOM, "A", font(FS), DLABEL);
    drag_text(cx, ui, p, dl + 69.0, base, afp::ATTACK, &format!("{} ms", sig3(a0.attack)), true, ORANGE);
    let hb = Rect::from_min_size(pos2(dl + 119.0, base - 9.0), vec2(11.0, 11.0));
    obox(p, hb, a0.hold, on);
    text_at(p, dl + 134.0, base, Align2::LEFT_BOTTOM, "Hold", font(FS), DLABEL);
    if cx.toggle(ui, hb.union(Rect::from_min_size(hb.min, vec2(38.0, 11.0))), afp::HOLD, true)
        && let FxParams::AutoFilter(a) = &mut cx.params
    {
        a.hold = !a.hold;
    }
    text_at(p, dl + 164.0, base, Align2::LEFT_BOTTOM, "R", font(FS), DLABEL);
    drag_text(cx, ui, p, dl + 175.0, base, afp::RELEASE, &format!("{} ms", sig3(a0.release)), true, ORANGE);
    let sb = Rect::from_min_size(pos2(dl + 221.0, base - 9.0), vec2(11.0, 11.0));
    obox(p, sb, a0.env_sh, on);
    text_at(p, dl + 236.0, base, Align2::LEFT_BOTTOM, "S&H", font(FS), DLABEL);
    if cx.toggle(ui, sb.union(Rect::from_min_size(sb.min, vec2(36.0, 11.0))), afp::ENV_SH, true)
        && let FxParams::AutoFilter(a) = &mut cx.params
    {
        a.env_sh = !a.env_sh;
    }
    let div = SYNC_DIVS[(a0.env_div as usize).min(SYNC_DIVS.len() - 1)].1.replace('/', " / ");
    drag_text(cx, ui, p, dl + 261.0, base, afp::ENV_DIV, &div, a0.env_sh, ORANGE);
}

// ----------------------------------------------------------------------------
// Parameter names, defaults and steps.
// ----------------------------------------------------------------------------

pub(super) fn name(p: &FxParams, k: u8) -> &'static str {
    match p {
        FxParams::EqThree(_) => match k {
            0 => "GainLo",
            1 => "GainMid",
            2 => "GainHi",
            3 => "LowOn",
            4 => "MidOn",
            5 => "HighOn",
            e3p::FREQ_LO => "FreqLo",
            e3p::FREQ_HI => "FreqHi",
            _ => "Slope",
        },
        FxParams::Chorus(_) => match k {
            chp::MODE => "Mode",
            chp::HP_ON => "Highpass Enabled",
            chp::HP_FREQ => "Highpass Frequency",
            chp::WIDTH => "Width",
            chp::TIME => "Delay Time",
            chp::TAPS => "Taps",
            chp::OFFSET => "Vibrato Offset",
            chp::SHAPE => "Vibrato Shape",
            chp::RATE => "Rate",
            chp::AMOUNT => "Amount",
            chp::FEEDBACK => "Feedback",
            chp::FB_INV => "Invert Feedback",
            chp::OUTPUT => "Output",
            chp::WARMTH => "Warmth",
            _ => "Dry/Wet",
        },
        FxParams::AutoPan(_) => match k {
            pnp::MODE => "Mode",
            pnp::WAVE => "Waveform",
            pnp::TIME_MODE => "Time Mode",
            pnp::RATE => "Rate",
            pnp::STEREO => "Stereo Mode",
            pnp::PHASE => "Phase",
            pnp::OFFSET => "Offset",
            pnp::INVERT => "Invert",
            pnp::HARMONIC => "Harmonic",
            pnp::VINTAGE => "Vintage",
            pnp::AMOUNT => "Amount",
            pnp::SHAPE => "Shape",
            pnp::ATTACK => "Attack Time",
            _ => "Dynamic Frequency Modulation",
        },
        _ => match k {
            afp::TYPE => "Filter Type",
            afp::SLOPE => "Slope",
            afp::CIRCUIT => "Circuit",
            afp::FREQ => "Frequency",
            afp::RES => "Resonance",
            afp::MORPH => "Morph",
            afp::DRIVE => "Drive",
            afp::CLIP => "Soft Clip",
            afp::OUTPUT => "Output",
            afp::DRY_WET => "Dry/Wet",
            afp::LFO_AMT => "LFO Amount",
            afp::RATE => "LFO Rate",
            afp::WAVE => "LFO Waveform",
            afp::LFO_MORPH => "LFO Morph",
            afp::TIME_MODE => "LFO Time Mode",
            afp::STEREO => "LFO Stereo Mode",
            afp::PHASE => "LFO Phase",
            afp::OFFSET => "LFO Offset",
            afp::QUANT => "LFO Quantize",
            afp::STEPS => "LFO Quantize Steps",
            afp::ENV_AMT => "Envelope Amount",
            afp::ATTACK => "Envelope Attack",
            afp::HOLD => "Envelope Hold",
            afp::RELEASE => "Envelope Release",
            afp::ENV_SH => "Envelope S&H",
            afp::ENV_DIV => "Envelope S&H Rate",
            afp::SECTION => "Section",
            sc::ON => "S/C On",
            sc::SOURCE => "S/C Audio From",
            sc::TAP => "S/C Tap",
            sc::GAIN => "S/C Gain",
            sc::MIX => "S/C Mix",
            sc::EQ_ON => "S/C Filter On",
            sc::EQ => "S/C Filter Type",
            sc::FREQ => "S/C Filter Freq",
            sc::Q => "S/C Filter Q",
            sc::EQ_GAIN => "S/C Filter Gain",
            _ => "S/C Listen",
        },
    }
}

pub(super) fn reset(p: &mut FxParams, k: u8) {
    match p {
        FxParams::EqThree(e) => {
            let d = EqThreeParams::default();
            match k {
                0..=2 => e.gain[k as usize] = 0.0,
                3..=5 => e.on[k as usize - 3] = true,
                e3p::FREQ_LO => e.freq_lo = d.freq_lo,
                e3p::FREQ_HI => e.freq_hi = d.freq_hi,
                _ => e.slope48 = d.slope48,
            }
        }
        FxParams::Chorus(c) => {
            let d = ChorusParams::default();
            match k {
                chp::HP_FREQ => c.hp_freq = d.hp_freq,
                chp::WIDTH => c.width = d.width,
                chp::OFFSET => c.offset = d.offset,
                chp::SHAPE => c.shape = d.shape,
                chp::RATE => c.rate = d.rate,
                chp::AMOUNT => c.amount = d.amount,
                chp::FEEDBACK => c.feedback = d.feedback,
                chp::OUTPUT => c.output = d.output,
                chp::WARMTH => c.warmth = d.warmth,
                chp::DRY_WET => c.dry_wet = d.dry_wet,
                _ => {}
            }
        }
        FxParams::AutoPan(a) => {
            let d = AutoPanParams::default();
            match k {
                pnp::RATE => {
                    let m = a.rate.mode;
                    a.rate = d.rate;
                    a.rate.mode = m;
                }
                pnp::PHASE if a.spin_mode => a.spin = d.spin,
                pnp::PHASE => a.phase = d.phase,
                pnp::OFFSET => a.offset = d.offset,
                pnp::AMOUNT => a.amount = d.amount,
                pnp::SHAPE if a.tremolo => a.shape_trem = 0.0,
                pnp::SHAPE => a.shape_pan = 0.0,
                pnp::ATTACK => a.attack = d.attack,
                pnp::DYN_MOD => a.dyn_mod = d.dyn_mod,
                _ => {}
            }
        }
        FxParams::AutoFilter(a) => {
            let d = AutoFilterParams::default();
            match k {
                afp::FREQ => match a.kind {
                    FilterType::Dj => a.dj = 0.0,
                    FilterType::Vowel => a.pitch = 0.0,
                    _ => a.freq = d.freq,
                },
                afp::RES => a.res = d.res,
                afp::MORPH => a.morph = d.morph,
                afp::DRIVE => a.drive = d.drive,
                afp::OUTPUT => a.output = d.output,
                afp::DRY_WET => a.dry_wet = d.dry_wet,
                afp::LFO_AMT => a.lfo_amt = d.lfo_amt,
                afp::RATE => {
                    let m = a.rate.mode;
                    a.rate = d.rate;
                    a.rate.mode = m;
                }
                afp::LFO_MORPH => a.lfo_morph = 0.0,
                afp::PHASE if a.spin_mode => a.spin = d.spin,
                afp::PHASE => a.phase = d.phase,
                afp::OFFSET => a.offset = d.offset,
                afp::STEPS => {
                    a.steps = d.steps;
                    a.q_div = d.q_div;
                }
                afp::ENV_AMT => a.env_amt = d.env_amt,
                afp::ATTACK => a.attack = d.attack,
                afp::RELEASE => a.release = d.release,
                afp::ENV_DIV => a.env_div = d.env_div,
                _ => dynamics::sc_reset_pub(&mut a.sc, k),
            }
        }
        _ => {}
    }
}

pub(super) fn step(p: &mut FxParams, k: u8, dir: f32) {
    let divs = (SYNC_DIVS.len() - 1) as f32;
    let n = match (&*p, k) {
        (FxParams::AutoPan(a), pnp::RATE) if a.rate.mode.synced() => divs,
        (FxParams::AutoFilter(a), afp::RATE) if a.rate.mode.synced() => divs,
        (FxParams::AutoFilter(_), afp::ENV_DIV) => divs,
        _ => 200.0,
    };
    if let Some(v) = norm(p, k) {
        set_norm(p, k, v + dir / n);
    }
}
