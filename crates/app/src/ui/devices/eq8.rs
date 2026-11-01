//! EQ Eight: main and expanded device views, the filter display (also drawn
//! maximised in the main window) with the analyzer, band choosers and
//! toggles. Geometry measured from Live 12 at 100 % (l/t = device left/top).

use super::*;

pub const WIDTH: f32 = 482.0;

pub use daw_project::devices::params::eqp::*;
#[allow(unused_imports)]
use daw_project::devices::params::eqp::{norm, set_norm};

pub fn reset(e: &mut EqEightParams, k: u8) {
    let d = EqEightParams::default();
    if let Some((ch, b, f)) = split(k) {
        let (band, def) = (&mut e.bands[ch][b], d.bands[ch][b]);
        match f {
            FREQ => band.freq = def.freq,
            GAIN => band.gain = 0.0,
            QF => band.q = def.q,
            ON => band.on = def.on,
            _ => band.kind = def.kind,
        }
        return;
    }
    match k {
        MODE => e.mode = d.mode,
        EDIT => e.edit_b = false,
        ADAPT => e.adaptive_q = d.adaptive_q,
        SCALE => e.scale = d.scale,
        OUTPUT => e.output = 0.0,
        AUDITION => e.audition = false,
        ANALYZE => e.analyze = d.analyze,
        BLOCK => e.block = d.block,
        REFRESH => e.refresh = d.refresh,
        AVG => e.avg = d.avg,
        DOTS => {
            // Delete on the dots: their gain back to 0 dB.
            let ch = e.edit_ch();
            for b in dot_set(e) {
                e.bands[ch][b].gain = 0.0;
            }
        }
        _ => {}
    }
}

/// Bands moved by the dot keys / drags: the enclosed dots, else the selected band.
fn dot_set(e: &EqEightParams) -> Vec<usize> {
    let v: Vec<usize> = (0..8).filter(|b| e.dots & (1 << b) != 0).collect();
    if v.is_empty() { vec![e.sel.min(7) as usize] } else { v }
}

pub fn step(e: &mut EqEightParams, k: u8, dir: f32) {
    if let Some((ch, b, f)) = split(k) {
        let band = &mut e.bands[ch][b];
        match f {
            FREQ => band.freq = norm_freq(freq_norm(band.freq) + dir * 0.005),
            GAIN => band.gain = ((band.gain * 10.0).round() + dir).clamp(-150.0, 150.0) / 10.0,
            QF => band.q = norm_q(q_norm(band.q) + dir * 0.005),
            TYPE => {
                let i = EqFilter::ALL.iter().position(|&x| x == band.kind).unwrap_or(3) as i32 - dir as i32;
                band.kind = EqFilter::ALL[i.clamp(0, EqFilter::ALL.len() as i32 - 1) as usize];
            }
            _ => {}
        }
        return;
    }
    match k {
        SCALE => e.scale = ((e.scale * 100.0).round() + dir).clamp(-200.0, 200.0) / 100.0,
        OUTPUT => e.output = ((e.output * 10.0).round() + dir).clamp(-120.0, 120.0) / 10.0,
        REFRESH => e.refresh = (e.refresh.round() + dir).clamp(EQ_REFRESH_MIN, EQ_REFRESH_MAX),
        AVG => e.avg = (e.avg.round() + dir).clamp(1.0, EQ_AVG_MAX),
        MODE => {
            let i = EqMode::ALL.iter().position(|&m| m == e.mode).unwrap_or(0) as i32 - dir as i32;
            e.mode = EqMode::ALL[i.clamp(0, 2) as usize];
        }
        BLOCK => {
            let i = EQ_BLOCKS.iter().position(|&b| b == e.block).unwrap_or(3) as i32 - dir as i32;
            e.block = EQ_BLOCKS[i.clamp(0, EQ_BLOCKS.len() as i32 - 1) as usize];
        }
        _ => {}
    }
}

/// Arrow keys on the display: left / right = frequency (1/12 octave),
/// up / down = gain (0.5 dB; Q for filters without gain).
pub fn arrow(e: &mut EqEightParams, key: egui::Key) {
    let ch = e.edit_ch();
    let (df, dg) = match key {
        egui::Key::ArrowLeft => (-1.0, 0.0),
        egui::Key::ArrowRight => (1.0, 0.0),
        egui::Key::ArrowUp => (0.0, 1.0),
        egui::Key::ArrowDown => (0.0, -1.0),
        _ => return,
    };
    for b in dot_set(e) {
        let band = &mut e.bands[ch][b];
        band.freq = (band.freq * 2f32.powf(df / 12.0)).clamp(EQ_FREQ_MIN, EQ_FREQ_MAX);
        if band.kind.has_gain() {
            band.gain = (band.gain + dg * 0.5).clamp(-EQ_GAIN_MAX, EQ_GAIN_MAX);
        } else {
            band.q = (band.q * 2f32.powf(dg / 12.0)).clamp(EQ_Q_MIN, EQ_Q_MAX);
        }
    }
}

pub fn name(k: u8) -> &'static str {
    const N: [[&str; 5]; 8] = [
        ["1 Frequency", "1 Gain", "1 Resonance", "1 Filter On", "1 Filter Type"],
        ["2 Frequency", "2 Gain", "2 Resonance", "2 Filter On", "2 Filter Type"],
        ["3 Frequency", "3 Gain", "3 Resonance", "3 Filter On", "3 Filter Type"],
        ["4 Frequency", "4 Gain", "4 Resonance", "4 Filter On", "4 Filter Type"],
        ["5 Frequency", "5 Gain", "5 Resonance", "5 Filter On", "5 Filter Type"],
        ["6 Frequency", "6 Gain", "6 Resonance", "6 Filter On", "6 Filter Type"],
        ["7 Frequency", "7 Gain", "7 Resonance", "7 Filter On", "7 Filter Type"],
        ["8 Frequency", "8 Gain", "8 Resonance", "8 Filter On", "8 Filter Type"],
    ];
    if let Some((_, b, f)) = split(k) {
        return N[b][f as usize];
    }
    match k {
        MODE => "Mode",
        EDIT => "Edit Mode",
        ADAPT => "Adaptive Q",
        SCALE => "Scale",
        OUTPUT => "Output Gain",
        AUDITION => "Audition",
        ANALYZE => "Analyze",
        BLOCK => "Block",
        REFRESH => "Refresh",
        AVG => "Avg",
        _ => "Filter",
    }
}

pub fn typable(k: u8) -> bool {
    matches!(split(k), Some((_, _, FREQ | GAIN | QF))) || matches!(k, SCALE | OUTPUT | REFRESH | AVG)
}

/// A typed value in display units (Hz, dB, Q, %, ms).
pub fn set_typed(e: &mut EqEightParams, k: u8, x: f32) {
    if let Some((ch, b, f)) = split(k) {
        let band = &mut e.bands[ch][b];
        match f {
            FREQ => band.freq = x.clamp(EQ_FREQ_MIN, EQ_FREQ_MAX),
            GAIN => band.gain = x.clamp(-EQ_GAIN_MAX, EQ_GAIN_MAX),
            QF => band.q = x.clamp(EQ_Q_MIN, EQ_Q_MAX),
            _ => {}
        }
        return;
    }
    match k {
        SCALE => e.scale = (x / 100.0).clamp(-EQ_SCALE_MAX, EQ_SCALE_MAX),
        OUTPUT => e.output = x.clamp(-EQ_OUT_MAX, EQ_OUT_MAX),
        REFRESH => e.refresh = x.clamp(EQ_REFRESH_MIN, EQ_REFRESH_MAX),
        AVG => e.avg = x.clamp(1.0, EQ_AVG_MAX),
        _ => {}
    }
}

pub fn fmt_freq(f: f32) -> String {
    if f < 99.95 {
        format!("{f:.1} Hz")
    } else if f < 999.5 {
        format!("{f:.0} Hz")
    } else if f < 9995.0 {
        format!("{:.2} kHz", f / 1000.0)
    } else {
        format!("{:.1} kHz", f / 1000.0)
    }
}

fn fmt_q(q: f32) -> String {
    if q < 9.995 { format!("{q:.2}") } else { format!("{q:.1}") }
}

/// Live's note name (C3 = MIDI 60).
pub fn note_name(f: f32) -> String {
    const N: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let m = (69.0 + 12.0 * (f / 440.0).log2()).round() as i32;
    format!("{}{}", N[m.rem_euclid(12) as usize], m.div_euclid(12) - 2)
}

fn with(cx: &mut Ctx, f: impl FnOnce(&mut EqEightParams)) {
    if let FxParams::EqEight(e) = &mut cx.params {
        f(e);
    }
}

fn eq(cx: &Ctx) -> EqEightParams {
    match cx.params {
        FxParams::EqEight(e) => e,
        _ => EqEightParams::default(),
    }
}

/// Edit colour: teal for Stereo / L / M, orange for R / S.
fn edit_col(e: &EqEightParams) -> Color32 {
    if e.edit_ch() == 1 { ORANGE } else { TEAL }
}

// ----------------------------------------------------------------------------
// Device view.
// ----------------------------------------------------------------------------

/// Header unfold button (left of the name): ↑ folded, ↓ when the display is
/// in the main window.
pub(super) fn header_button(cx: &mut Ctx, ui: &egui::Ui, p: &Painter, l: f32, t: f32) {
    let e = eq(cx);
    let c = pos2(l + 25.5, t + 8.5);
    p.circle_filled(c, 6.5, gray(0x12));
    p.circle_filled(c, 5.5, gray(0x1e));
    let g = gray(0xb5);
    let s = Stroke::new(1.0, g);
    if e.expanded {
        quad(p, Rect::from_min_max(pos2(c.x - 0.5, t + 4.0), pos2(c.x + 0.5, t + 9.0)), g);
        p.add(Shape::line(vec![pos2(c.x - 2.5, t + 6.5), pos2(c.x, t + 9.0), pos2(c.x + 2.5, t + 6.5)], s));
    } else {
        quad(p, Rect::from_min_max(pos2(c.x - 0.5, t + 4.5), pos2(c.x + 0.5, t + 9.0)), g);
        p.add(Shape::line(vec![pos2(c.x - 2.5, t + 7.5), pos2(c.x, t + 5.0), pos2(c.x + 2.5, t + 7.5)], s));
    }
    quad(p, Rect::from_min_max(pos2(c.x - 2.5, t + 11.0), pos2(c.x + 2.5, t + 12.0)), g);
    let r = ui.interact(Rect::from_center_size(c, vec2(13.0, 13.0)), egui::Id::unique(("eq_unfold", cx.dev.id)), Sense::click());
    if r.clicked() {
        with(cx, |e| e.expanded = !e.expanded);
    }
}

pub(super) fn eq_eight(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect) {
    let e0 = eq(cx);
    let (l, t) = (r.left(), r.top());
    let bottom = r.bottom().max(t + 188.0);
    let sec = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(pos2(l + x0, t + y0), pos2(l + x1, t + y1));
    // #46 behind the sections shows as 2 px dividers.
    p.rect_filled(
        Rect::from_min_max(pos2(l, t + TITLE_H), pos2(r.right(), bottom)),
        CornerRadius { nw: 0, ne: 0, sw: 3, se: 3 },
        gray(0x46),
    );
    let hb = bottom - t - 2.0;
    p.rect_filled(sec(2.0, 17.0, 50.0, hb), CornerRadius::ZERO, panel());
    p.rect_filled(sec(428.0, 17.0, 480.0, hb), CornerRadius::ZERO, panel());
    let ch = e0.edit_ch();
    let sel = e0.sel.min(7) as usize;
    let cell_top = if e0.expanded { 17.0 } else { 153.0 };
    for i in 0..8 {
        let x0 = 52.0 + 47.0 * i as f32;
        let cr = sec(x0, cell_top, x0 + 45.0, hb);
        if i != sel {
            p.rect_filled(cr, CornerRadius::ZERO, panel());
        }
        // Click in a band's cell selects the band (widgets inside win).
        let resp = ui.interact(cr, egui::Id::unique(("eq_cell", cx.dev.id, i)), Sense::click());
        if resp.clicked() {
            with(cx, |e| e.sel = i as u8);
            cx.app.dev_sel = Some(cx.dev.id);
            cx.app.focus_detail = true;
        }
    }
    if e0.expanded {
        expanded_left(cx, ui, p, l, t);
    } else {
        // Freq / Gain / Q of the selected band.
        let b = e0.bands[ch][sel];
        let x = l + 26.5;
        eknob(cx, ui, p, x, t + 28.0, band_id(ch, sel, FREQ), "Freq", &fmt_freq(b.freq), 0.0, b.on, true, false);
        eknob(cx, ui, p, x, t + 83.0, band_id(ch, sel, GAIN), "Gain", &fmt_db(b.gain), 0.5, b.on, b.kind.has_gain(), true);
        eknob(cx, ui, p, x, t + 145.0, band_id(ch, sel, QF), "Q", &fmt_q(b.q), 0.0, b.on, true, false);
        display(cx, ui, p, sec(52.0, 17.0, 426.0, 151.0), false);
    }
    for i in 0..8 {
        band_cell(cx, ui, p, l + 52.0 + 47.0 * i as f32, t, i, e0.expanded);
    }
    right_column(cx, ui, p, l, t);
}

/// Label / knob / value at `x` (label baseline `base`). Knobs without the
/// top marker (Freq, Q) sit closer to their label.
#[allow(clippy::too_many_arguments)]
fn eknob(
    cx: &mut Ctx,
    ui: &mut egui::Ui,
    p: &Painter,
    x: f32,
    base: f32,
    k: u8,
    label: &str,
    value: &str,
    def: f32,
    active: bool,
    enabled: bool,
    marker: bool,
) {
    let (kc, vb) = if marker { (21.5, 42.0) } else { (14.6, 36.0) };
    let area = Rect::from_min_max(pos2(x - 22.5, base - 9.0), pos2(x + 22.5, base + vb + 3.0));
    cx.drag(ui, area, k, enabled);
    let v = norm(&eq(cx), k).unwrap_or(def);
    text_at(p, x, base, Align2::CENTER_BOTTOM, label, font(FS), text());
    knob_look_m(p, pos2(x, base + kc), v, def, active && enabled && cx.on, marker);
    text_at(p, x, base + vb, Align2::CENTER_BOTTOM, value, font(FS), text());
    if cx.focused(k) {
        brackets(p, area);
    }
}

/// Analyzer settings (expanded view's left column).
fn expanded_left(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32) {
    let e0 = eq(cx);
    text_at(p, l + 6.0, t + 57.0, Align2::LEFT_BOTTOM, "Analyze", font(FS), text());
    text_at(p, l + 6.0, t + 75.0, Align2::LEFT_BOTTOM, "Block", font(FS), text());
    let labels: Vec<String> = EQ_BLOCKS.iter().map(|b| b.to_string()).collect();
    let refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
    let br = Rect::from_min_size(pos2(l + 5.0, t + 78.0), vec2(42.0, 13.0));
    if let Some(i) = chooser(cx, ui, p, br, BLOCK, &e0.block.to_string(), &refs, e0.analyze) {
        with(cx, |e| e.block = EQ_BLOCKS[i]);
    }
    text_at(p, l + 6.0, t + 106.0, Align2::LEFT_BOTTOM, "Refresh", font(FS), text());
    value_box(
        cx,
        ui,
        p,
        Rect::from_min_size(pos2(l + 5.0, t + 109.0), vec2(42.0, 13.0)),
        REFRESH,
        &format!("{:.2}", e0.refresh),
        e0.analyze,
    );
    text_at(p, l + 6.0, t + 138.0, Align2::LEFT_BOTTOM, "Avg", font(FS), text());
    value_box(cx, ui, p, Rect::from_min_size(pos2(l + 5.0, t + 141.0), vec2(42.0, 13.0)), AVG, &format!("{:.2}", e0.avg), e0.analyze);
}

/// One band's cell: (expanded: knobs and Q box,) filter chooser, toggle.
fn band_cell(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, x0: f32, t: f32, i: usize, expanded: bool) {
    let e0 = eq(cx);
    let ch = e0.edit_ch();
    let b = e0.bands[ch][i];
    let dy = if expanded { -1.0 } else { 0.0 };
    if expanded {
        let x = x0 + 22.5;
        eknob(cx, ui, p, x, t + 29.0, band_id(ch, i, FREQ), "Freq", &fmt_freq(b.freq), 0.0, b.on, true, false);
        eknob(cx, ui, p, x, t + 78.0, band_id(ch, i, GAIN), "Gain", &fmt_db(b.gain), 0.5, b.on, b.kind.has_gain(), true);
        text_at(p, x, t + 133.0, Align2::CENTER_BOTTOM, "Q", font(FS), text());
        let bg = if i == e0.sel as usize { gray(0x46) } else { panel() };
        let qr = Rect::from_min_size(pos2(x0 + 4.0, t + 136.0), vec2(37.0, 13.0));
        value_box_ex(cx, ui, p, qr, band_id(ch, i, QF), &fmt_q(b.q), true, b.on, bg);
    }
    // Filter type chooser (icons).
    let cr = Rect::from_min_size(pos2(x0 + 4.0, t + 156.0 + dy), vec2(37.0, 13.0));
    boxed(p, cr, box_fill(cx.on));
    let gcol = if cx.on { gray(0xb5) } else { gray(0x69) };
    icon(p, pos2(cr.left() + 3.0, cr.top() + 2.0), b.kind, gcol);
    let tr = cr.right();
    p.add(Shape::convex_polygon(
        vec![pos2(tr - 8.0, cr.top() + 4.0), pos2(tr - 3.0, cr.top() + 4.0), pos2(tr - 5.5, cr.top() + 8.5)],
        gcol,
        Stroke::NONE,
    ));
    let k = band_id(ch, i, TYPE);
    let resp = cx.sense(ui, cr, k, Sense::click());
    if resp.clicked() {
        cx.focus(k);
        with(cx, |e| e.sel = i as u8);
    }
    let mut pick = None;
    menu::dropdown_aligned(&resp, 60.0, egui::RectAlign::BOTTOM_START, |ui| {
        for f in EqFilter::ALL {
            if menu::icon_item(ui, 15.0, f == b.kind, |p, r, c| icon(p, pos2(r.left(), r.center().y - 4.5), f, c)) {
                pick = Some(f);
            }
        }
    });
    if let Some(f) = pick {
        cx.undo_step();
        with(cx, |e| e.bands[ch][i].kind = f);
    }
    if cx.focused(k) {
        p.rect_stroke(cr.expand(1.0), CornerRadius::ZERO, Stroke::new(1.0, gray(0x91)), egui::StrokeKind::Inside);
    }
    // Band toggle with its number.
    let tg = Rect::from_min_size(pos2(x0 + 11.0, t + 172.0 + dy), vec2(11.0, 11.0));
    p.rect_filled(tg, CornerRadius::ZERO, gray(0x11));
    if b.on {
        let c = if cx.on { edit_col(&e0) } else { mix(edit_col(&e0), gray(0x36), 0.5) };
        p.rect_filled(tg.shrink(1.0), CornerRadius::ZERO, c);
    }
    text_at(p, x0 + 26.5, tg.bottom(), Align2::LEFT_BOTTOM, &(i + 1).to_string(), font(FS), if cx.on { text() } else { gray(0x69) });
    let k = band_id(ch, i, ON);
    if cx.toggle(ui, tg, k, true) {
        with(cx, |e| {
            e.bands[ch][i].on = !e.bands[ch][i].on;
            e.sel = i as u8;
        });
    }
}

/// Audition / analyzer buttons, Mode, Edit, Adapt. Q, Scale, Output.
fn right_column(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, l: f32, t: f32) {
    let e0 = eq(cx);
    let x = l + 431.0;
    // Headphones (audition) and analyzer switches.
    for (k, bx, on) in [(AUDITION, x, e0.audition), (ANALYZE, x + 25.0, e0.analyze)] {
        let br = Rect::from_min_size(pos2(bx, t + 23.0), vec2(21.0, 13.0));
        let fill = match (on, cx.on) {
            (true, true) => ORANGE,
            (true, false) => mix(ORANGE, gray(0x36), 0.5),
            _ => gray(0x1e),
        };
        boxed(p, br, fill);
        let g = match (on, cx.on) {
            (_, false) => gray(0x69),
            (true, true) => gray(0x34),
            (false, true) => gray(0xb5),
        };
        let c = br.center();
        if k == AUDITION {
            arc(p, c + vec2(0.0, 0.5), 3.6, -PI / 2.0 - 0.3, PI / 2.0 + 0.3, Stroke::new(1.2, g));
            for dx in [-4.0, 2.0] {
                quad(p, Rect::from_min_size(c + vec2(dx, 0.5), vec2(2.0, 3.0)), g);
            }
        } else {
            // Spectrum: six 1 px bars (Live's glyph).
            for (dx, top) in [(4.0, 7.0), (6.0, 3.0), (8.0, 5.0), (10.0, 3.0), (12.0, 7.0), (14.0, 5.0)] {
                quad(p, Rect::from_min_max(pos2(br.left() + dx, br.top() + top), pos2(br.left() + dx + 1.0, br.top() + 10.0)), g);
            }
        }
        if cx.toggle(ui, br, k, true) {
            with(cx, |e| {
                if k == AUDITION {
                    e.audition = !e.audition;
                } else {
                    e.analyze = !e.analyze;
                }
            });
        }
    }
    text_at(p, l + 432.0, t + 48.0, Align2::LEFT_BOTTOM, "Mode", font(FS), text());
    let mr = Rect::from_min_size(pos2(x, t + 51.0), vec2(46.0, 13.0));
    if let Some(i) = chooser(cx, ui, p, mr, MODE, e0.mode.label(), &EqMode::ALL.map(|m| m.label()), true) {
        with(cx, |e| e.mode = EqMode::ALL[i]);
    }
    // Edit: which channel the dials / dots edit (greyed "L" in Stereo).
    text_at(p, l + 432.0, t + 76.0, Align2::LEFT_BOTTOM, "Edit", font(FS), text());
    let er = Rect::from_min_size(pos2(x, t + 79.0), vec2(46.0, 13.0));
    let stereo = e0.mode == EqMode::Stereo;
    let label = match (e0.mode, e0.edit_b) {
        (EqMode::MS, false) => "M",
        (EqMode::MS, true) => "S",
        (_, true) if !stereo => "R",
        _ => "L",
    };
    let (fill, col) = if stereo {
        (gray(0x1e), gray(0x69))
    } else if cx.on {
        (edit_col(&e0), Color32::from_rgb(0x20, 0x20, 0x20))
    } else {
        (mix(edit_col(&e0), gray(0x36), 0.5), gray(0x36))
    };
    boxed(p, er, fill);
    text_at(p, er.center().x, er.top() + 10.0, Align2::CENTER_BOTTOM, label, font(FS), col);
    if cx.toggle(ui, er, EDIT, !stereo) {
        with(cx, |e| {
            e.edit_b = !e.edit_b;
            e.dots = 0;
        });
    }
    text_at(p, l + 432.0, t + 105.0, Align2::LEFT_BOTTOM, "Adapt. Q", font(FS), text());
    let ar = Rect::from_min_size(pos2(x, t + 108.0), vec2(46.0, 13.0));
    toggle_look(p, ar, if e0.adaptive_q { "On" } else { "Off" }, e0.adaptive_q, true, cx.on);
    if cx.toggle(ui, ar, ADAPT, true) {
        with(cx, |e| e.adaptive_q = !e.adaptive_q);
    }
    text_at(p, l + 432.0, t + 134.0, Align2::LEFT_BOTTOM, "Scale", font(FS), text());
    value_box(cx, ui, p, Rect::from_min_size(pos2(x, t + 137.0), vec2(46.0, 13.0)), SCALE, &format!("{:.0} %", e0.scale * 100.0), true);
    text_at(p, l + 432.0, t + 164.0, Align2::LEFT_BOTTOM, "Output", font(FS), text());
    value_box(cx, ui, p, Rect::from_min_size(pos2(x, t + 167.0), vec2(46.0, 13.0)), OUTPUT, &fmt_db(e0.output), true);
}

/// Filter type glyph in a 15 × 9 box at `o` (Live's chooser icons).
pub fn icon(p: &Painter, o: Pos2, f: EqFilter, col: Color32) {
    let s = Stroke::new(1.0, col);
    let pt = |x: f32, y: f32| pos2(o.x + x, o.y + y);
    let line = |pts: Vec<Pos2>| {
        p.add(Shape::line(pts, s));
    };
    match f {
        EqFilter::LowCut12 => line(vec![pt(0.5, 9.0), pt(0.5, 3.5), pt(1.2, 1.7), pt(3.0, 0.5), pt(11.0, 0.5)]),
        EqFilter::LowCut48 => line(vec![pt(1.5, 9.0), pt(1.5, 0.5), pt(11.0, 0.5)]),
        // Between the 12 and 48 dB glyphs.
        EqFilter::LowCut24 => line(vec![pt(1.0, 9.0), pt(1.0, 3.0), pt(1.6, 1.3), pt(2.6, 0.5), pt(11.0, 0.5)]),
        EqFilter::HighCut24 => line(vec![pt(0.0, 0.5), pt(8.4, 0.5), pt(9.4, 1.3), pt(10.0, 3.0), pt(10.0, 9.0)]),
        EqFilter::HighCut12 => line(vec![pt(0.0, 0.5), pt(8.0, 0.5), pt(9.8, 1.7), pt(10.5, 3.5), pt(10.5, 9.0)]),
        EqFilter::HighCut48 => line(vec![pt(0.0, 0.5), pt(9.5, 0.5), pt(9.5, 9.0)]),
        EqFilter::LowShelf => {
            line(vec![pt(0.0, 0.5), pt(5.5, 0.5), pt(8.5, 4.5), pt(5.5, 8.5), pt(0.0, 8.5)]);
            line(vec![pt(8.5, 4.5), pt(15.0, 4.5)]);
        }
        EqFilter::HighShelf => {
            line(vec![pt(15.0, 0.5), pt(9.5, 0.5), pt(6.5, 4.5), pt(9.5, 8.5), pt(15.0, 8.5)]);
            line(vec![pt(0.0, 4.5), pt(6.5, 4.5)]);
        }
        EqFilter::Bell => {
            line(vec![pt(0.0, 4.5), pt(2.5, 4.5), pt(5.0, 0.5), pt(10.0, 0.5), pt(12.5, 4.5), pt(15.0, 4.5)]);
            line(vec![pt(2.5, 4.5), pt(5.0, 8.5), pt(10.0, 8.5), pt(12.5, 4.5)]);
        }
        EqFilter::Notch => line(vec![pt(0.0, 0.5), pt(5.0, 0.5), pt(7.5, 8.5), pt(10.0, 0.5), pt(15.0, 0.5)]),
    }
}

// ----------------------------------------------------------------------------
// Display (device and maximised).
// ----------------------------------------------------------------------------

/// Analyzer level range mapped to the display height.
const SPEC_LO: f32 = -84.0;
const SPEC_HI: f32 = 0.0;
/// Dots and curve keep clear of the display edges.
const INSET: f32 = 6.5;

struct Axis {
    r: Rect,
}

impl Axis {
    fn frac(f: f32) -> f32 {
        (f / EQ_FREQ_MIN).ln() / LN_RANGE
    }
    /// Grid / analyzer / pointer frequency axis.
    fn gx(&self, f: f32) -> f32 {
        self.r.left() + 0.5 + (self.r.width() - 1.0) * Self::frac(f)
    }
    fn g_freq(&self, x: f32) -> f32 {
        EQ_FREQ_MIN * (((x - self.r.left() - 0.5) / (self.r.width() - 1.0)) * LN_RANGE).exp()
    }
    /// Band dots and the curve.
    fn dx(&self, f: f32) -> f32 {
        self.r.left() + INSET + (self.r.width() - 2.0 * INSET) * Self::frac(f)
    }
    fn d_freq(&self, x: f32) -> f32 {
        EQ_FREQ_MIN * (((x - self.r.left() - INSET) / (self.r.width() - 2.0 * INSET)) * LN_RANGE).exp()
    }
    fn px_per_db(&self) -> f32 {
        (self.r.height() - 1.0) / (2.0 * EQ_GAIN_MAX)
    }
    fn y(&self, db: f32) -> f32 {
        self.r.center().y - db * self.px_per_db()
    }
    fn db_at(&self, y: f32) -> f32 {
        (self.r.center().y - y) / self.px_per_db()
    }
}

/// Drag state of the display (egui temp data).
#[derive(Clone)]
struct DotDrag {
    /// Dragged bands and their (freq, gain, q) at the press; empty = rubber band.
    start: Vec<(usize, f32, f32, f32)>,
    origin: Pos2,
}

/// Cut filters: the dot height shows the resonance, 0 dB at Q 0.71.
fn q_db(q: f32) -> f32 {
    20.0 * (q.max(EQ_Q_MIN) / std::f32::consts::FRAC_1_SQRT_2).log10()
}

fn dot_pos(ax: &Axis, b: &EqBand) -> Pos2 {
    let y = if b.kind.has_gain() {
        ax.y(b.gain)
    } else if b.kind.is_cut() {
        ax.y(q_db(b.q).clamp(-EQ_GAIN_MAX, EQ_GAIN_MAX))
    } else {
        ax.y(0.0)
    };
    pos2(ax.dx(b.freq), y)
}

/// Frequency-response display: analyzer, grid, curves and band dots;
/// drag dots (horizontal = frequency, vertical = gain or Q, Alt = Q),
/// drag in empty space to enclose several dots.
pub(super) fn display(cx: &mut Ctx, ui: &mut egui::Ui, p: &Painter, r: Rect, big: bool) {
    let e0 = eq(cx);
    let p = p.with_clip_rect(r.intersect(p.clip_rect()));
    let ax = Axis { r };
    let id = cx.dev.id;
    p.rect_filled(r, CornerRadius::ZERO, gray(0x18));
    let sr = cx.app.spectra.sample_rate(id).max(8000.0);

    // Analyzer (filled, behind the grid); empty while the device is off.
    if e0.analyze && cx.on {
        let spec = cx.app.spectra.update(id, e0.block, e0.refresh, e0.avg);
        let live = spec.live;
        let cols = r.width().max(1.0) as usize + 1;
        let db = crate::spectrum::curve_db(spec, sr, cols, |i| ax.g_freq(r.left() + i));
        let mut mesh = egui::Mesh::default();
        let col = gray(0x53);
        let bottom = r.bottom();
        for (i, db) in db.iter().enumerate() {
            let x = r.left() + i as f32;
            let y = (bottom - (db - SPEC_LO) / (SPEC_HI - SPEC_LO) * r.height()).clamp(r.top(), bottom);
            let v = mesh.vertices.len() as u32;
            mesh.colored_vertex(pos2(x, y), col);
            mesh.colored_vertex(pos2(x, bottom), col);
            if i > 0 {
                mesh.add_triangle(v - 2, v - 1, v);
                mesh.add_triangle(v - 1, v, v + 1);
            }
        }
        p.add(Shape::mesh(mesh));
        // Every frame while it moves: the spectrum glides.
        cx.app.animating |= live;
    } else {
        cx.app.spectra.clear(id);
    }

    // Grid: decades brighter, labels on top.
    let fine = Color32::from_white_alpha(21);
    let bright = Color32::from_white_alpha(43);
    for d in [10.0f32, 100.0, 1000.0, 10_000.0] {
        for m in 1..=9 {
            let f = d * m as f32;
            if f <= EQ_FREQ_MIN || f > EQ_FREQ_MAX {
                continue;
            }
            let x = ax.gx(f).floor();
            p.rect_filled(
                Rect::from_min_max(pos2(x, r.top()), pos2(x + 1.0, r.bottom())),
                CornerRadius::ZERO,
                if m == 1 { bright } else { fine },
            );
        }
    }
    for db in [12.0f32, 6.0, 0.0, -6.0, -12.0] {
        let y = ax.y(db).floor();
        p.rect_filled(Rect::from_min_max(pos2(r.left(), y), pos2(r.right(), y + 1.0)), CornerRadius::ZERO, fine);
    }
    let lf = if big { font(FS_SMALL) } else { font(FS) };
    let lc = gray(0x86);
    let asc = if big { 3.0 } else { 4.0 };
    for (db, s) in [(12.0, "12"), (6.0, "6"), (0.0, "0"), (-6.0, "-6"), (-12.0, "-12")] {
        text_at(&p, r.left() + 19.0, ax.y(db).floor() + asc, Align2::RIGHT_BOTTOM, s, lf.clone(), lc);
    }
    for (f, s) in [(100.0, "100"), (1000.0, "1k"), (10_000.0, "10k")] {
        text_at(&p, ax.gx(f).floor() + 0.5, r.bottom() - 3.0, Align2::CENTER_BOTTOM, s, lf.clone(), lc);
    }

    // Curves: the other channel dim (L/R, M/S), the edited one on top.
    let ch = e0.edit_ch();
    let curve = |c: usize| -> Vec<Pos2> {
        let mut pts = Vec::with_capacity(r.width() as usize + 2);
        let mut x = r.left();
        while x <= r.right() + 0.5 {
            let f = ax.d_freq(x).clamp(1.0, sr * 0.499);
            let db = e0.response_db(c, f, sr);
            pts.push(pos2(x, ax.y(db).clamp(r.top() - 4.0, r.bottom() + 4.0)));
            x += 1.0;
        }
        pts
    };
    if e0.mode != EqMode::Stereo {
        p.add(Shape::line(curve(1 - ch), Stroke::new(1.5, gray(0x70))));
    }
    let ccol = match (ch, big) {
        (1, _) => Color32::from_rgb(0xe8, 0x9d, 0x4f),
        (_, false) => Color32::from_rgb(13, 157, 170),
        (_, true) => Color32::from_rgb(30, 174, 188),
    };
    let ccol = if cx.on { ccol } else { gray(0x69) };
    p.add(Shape::line(curve(ch), Stroke::new(2.0, ccol)));

    // Dots of the edited channel's active bands.
    // 11 px dots: 1.5 px ring over the curve, small number inside.
    let rad = 4.5;
    let dot_col = if cx.on { ORANGE } else { gray(0x69) };
    let bands = e0.bands[ch];
    let dots: Vec<(usize, Pos2)> = (0..8).filter(|&b| bands[b].on).map(|b| (b, dot_pos(&ax, &bands[b]))).collect();
    for &(b, c) in &dots {
        let selected = b == e0.sel as usize || e0.dots & (1 << b) != 0;
        if selected {
            p.circle_filled(c, rad + 1.0, dot_col);
        } else {
            p.circle_filled(c, rad, gray(0x18));
            p.circle_stroke(c, rad, Stroke::new(1.5, dot_col));
        }
        let tc = if selected { Color32::from_rgb(0x20, 0x20, 0x20) } else { dot_col };
        text_at(&p, c.x, c.y + 3.0, Align2::CENTER_BOTTOM, &(b + 1).to_string(), font(FS_SMALL), tc);
    }

    // Interaction.
    let resp = ui.interact(r, egui::Id::unique(("eq_disp", id, big)), Sense::click_and_drag());
    let did = egui::Id::unique(("eq_drag", id));
    let hit = |pos: Pos2| {
        dots.iter()
            .filter(|(_, c)| c.distance(pos) <= rad + 3.0)
            .min_by(|a, b| a.1.distance(pos).total_cmp(&b.1.distance(pos)))
            .map(|d| d.0)
    };
    if resp.drag_started() || resp.clicked() || resp.secondary_clicked() {
        cx.focus(DOTS);
    }
    if resp.drag_started() {
        let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(r.center());
        let start = match hit(origin) {
            Some(b) => {
                let multi = e0.dots & (1 << b) != 0;
                with(cx, |e| {
                    e.sel = b as u8;
                    if !multi {
                        e.dots = 0;
                    }
                    if e.audition {
                        e.solo = Some(b as u8);
                    }
                });
                let set: Vec<usize> = if multi { (0..8).filter(|i| e0.dots & (1 << i) != 0).collect() } else { vec![b] };
                set.into_iter().map(|i| (i, bands[i].freq, bands[i].gain, bands[i].q)).collect()
            }
            None => {
                with(cx, |e| e.dots = 0);
                Vec::new()
            }
        };
        ui.data_mut(|d| d.insert_temp(did, DotDrag { start, origin }));
    } else if resp.clicked() {
        match ui.input(|i| i.pointer.interact_pos()).and_then(hit) {
            Some(b) => with(cx, |e| {
                e.sel = b as u8;
                if e.dots & (1 << b) == 0 {
                    e.dots = 0;
                }
            }),
            None => with(cx, |e| e.dots = 0),
        }
    }
    if let Some(dd) = ui.data(|d| d.get_temp::<DotDrag>(did))
        && (resp.dragged() || resp.drag_stopped())
    {
        let pos = ui.input(|i| i.pointer.interact_pos()).unwrap_or(dd.origin);
        let delta = pos - dd.origin;
        if dd.start.is_empty() {
            // Rubber band: enclose dots.
            let rect = Rect::from_two_pos(dd.origin, pos);
            p.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0, gray(0xb5)), egui::StrokeKind::Inside);
            let mask = dots.iter().filter(|(_, c)| rect.contains(*c)).fold(0u8, |m, (b, _)| m | (1 << b));
            with(cx, |e| e.dots = mask);
        } else if delta != egui::Vec2::ZERO {
            let alt = ui.input(|i| i.modifiers.alt);
            let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
            cx.undo_step();
            let w = r.width() - 2.0 * INSET;
            let ppdb = ax.px_per_db();
            with(cx, |e| {
                for &(b, f0, g0, q0) in &dd.start {
                    let band = &mut e.bands[ch][b];
                    band.freq = (f0 * (delta.x * fine / w * LN_RANGE).exp()).clamp(EQ_FREQ_MIN, EQ_FREQ_MAX);
                    if band.kind.is_cut() {
                        // The dot follows the pointer: Q from its resonance height.
                        let db = q_db(q0) - delta.y * fine / ppdb;
                        band.q = (std::f32::consts::FRAC_1_SQRT_2 * 10f32.powf(db / 20.0)).clamp(EQ_Q_MIN, EQ_Q_MAX);
                        band.gain = g0;
                    } else if alt || !band.kind.has_gain() {
                        band.q = (q0 * 2f32.powf(-delta.y * fine / 40.0)).clamp(EQ_Q_MIN, EQ_Q_MAX);
                        band.gain = g0;
                    } else {
                        band.gain = (g0 - delta.y * fine / ppdb).clamp(-EQ_GAIN_MAX, EQ_GAIN_MAX);
                        band.q = q0;
                    }
                }
            });
        }
        if resp.drag_stopped() {
            ui.data_mut(|d| d.remove::<DotDrag>(did));
            with(cx, |e| e.solo = None);
        }
    }
    if !resp.dragged() && e0.solo.is_some() && !ui.input(|i| i.pointer.any_down()) {
        with(cx, |e| e.solo = None);
    }

    // Maximised: pointer frequency, note and level (Live's note detector).
    if big && let Some(hp) = resp.hover_pos() {
        let f = ax.g_freq(hp.x).clamp(EQ_FREQ_MIN, EQ_FREQ_MAX);
        let fs = if f < 999.5 { format!("{f:.0} Hz") } else { format!("{:.2} kHz", f / 1000.0) };
        let lines = [fs, note_name(f), format!("{:.2} dB", ax.db_at(hp.y))];
        let tf = font(FS_SMALL);
        let w = lines.iter().map(|s| p.layout_no_wrap(s.clone(), tf.clone(), TEAL).size().x).fold(0.0, f32::max);
        let tb = Rect::from_min_size(pos2(r.left() + 11.0, r.bottom() - 50.0), vec2(w + 10.0, 38.0));
        p.rect_filled(tb, CornerRadius::same(4), gray(0x18));
        p.rect_stroke(tb, CornerRadius::same(4), Stroke::new(1.0, TEAL), egui::StrokeKind::Inside);
        for (j, s) in lines.iter().enumerate() {
            text_at(&p, tb.right() - 5.0, tb.top() + 12.0 + 11.0 * j as f32, Align2::RIGHT_BOTTOM, s, tf.clone(), TEAL);
        }
    }
}

/// The unfolded display above the Device View (Live's maximised EQ Eight).
pub fn maximised(app: &mut DawApp, ui: &mut egui::Ui, r: Rect, c: ChainRef, id: u64) {
    let Some(dev) = chain(&app.project, c).iter().find(|d| d.id == id).cloned() else { return };
    let mut cx = Ctx::new(app, c, dev.clone());
    let p = ui.painter().clone();
    display(&mut cx, ui, &p, r, true);
    cx.commit();
}

/// First EQ Eight of the shown chain (at any rack depth) whose display is
/// unfolded, with the chain directly holding it.
pub fn unfolded(app: &DawApp) -> Option<(ChainRef, u64)> {
    fn find(v: &[daw_project::Device]) -> Option<u64> {
        v.iter().find_map(|d| {
            if matches!(d.params, FxParams::EqEight(e) if e.expanded) {
                return Some(d.id);
            }
            d.rack.as_deref().and_then(|r| r.chains.iter().find_map(|c| find(&c.devices)))
        })
    }
    if !app.device_view_shown() {
        return None;
    }
    let top = app.current_chain();
    let id = find(chain(&app.project, top))?;
    Some((crate::fx_ops::chain_of(&app.project, top, id)?, id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_round_trip_and_match_live_boxes() {
        let e = EqEightParams::default();
        // Scale 100 % fills 75 %, Output 0 dB half (Live).
        assert!((norm(&e, SCALE).unwrap() - 0.75).abs() < 1e-6);
        assert!((norm(&e, OUTPUT).unwrap() - 0.5).abs() < 1e-6);
        let mut e2 = e;
        for k in [band_id(0, 2, FREQ), band_id(1, 5, GAIN), band_id(0, 7, QF), SCALE, REFRESH] {
            let v = norm(&e, k).unwrap();
            set_norm(&mut e2, k, v);
            assert!((norm(&e2, k).unwrap() - v).abs() < 1e-5, "{k}");
        }
        assert_eq!(fmt_freq(30.0), "30.0 Hz");
        assert_eq!(fmt_freq(200.0), "200 Hz");
        assert_eq!(fmt_freq(1000.0), "1.00 kHz");
        assert_eq!(fmt_freq(18000.0), "18.0 kHz");
        assert_eq!(note_name(138.0), "C#2");
        assert_eq!(note_name(261.63), "C3");
    }

    #[test]
    fn arrows_move_enclosed_dots() {
        let mut e = EqEightParams { dots: 0b0110, ..Default::default() };
        arrow(&mut e, egui::Key::ArrowUp);
        assert_eq!(e.bands[0][1].gain, 0.5);
        assert_eq!(e.bands[0][2].gain, 0.5);
        assert_eq!(e.bands[0][0].gain, 0.0);
        arrow(&mut e, egui::Key::ArrowRight);
        assert!((e.bands[0][1].freq - 200.0 * 2f32.powf(1.0 / 12.0)).abs() < 0.01);
    }
}
